//! The type table (`spec/ir.md` §3): interned types whose layout is computed
//! when they are interned.
//!
//! Layout follows LLVM's `DataLayout` and `StructLayout` exactly, for the
//! data layout the table was created with; `tests/layout_diff.rs` compares
//! it with `llc` over a generated grid of types on both targets.
//!
//! # Sizes
//!
//! Sizes and offsets are `u64` **bytes**. A type whose size in *bits* does
//! not fit `u64` (more than [`MAX_TYPE_SIZE`] bytes) cannot be interned:
//! the constructor fails with `TypeTooLarge`. Nothing wraps.
//!
//! - **store size**: the bytes a `store` of the type writes,
//!   `ceil(bits / 8)`.
//! - **alloc size**: the store size rounded up to the ABI alignment; the
//!   distance between consecutive array elements.
//! - A scalable vector (and an aggregate containing one) has its *minimum*
//!   size, the size at `vscale` 1, and [`TypeTable::is_scalable`] is true.
//!
//! # Identity
//!
//! Every type is interned by structure, LLVM's named structs included: two
//! named structs with the same body are one `TypeId`, and names are not
//! kept. With opaque pointers a struct cannot contain itself, so a producer
//! can always intern bodies bottom-up. Only [`TypeTable::opaque_struct`]
//! creates a new distinct type on every call.

use crate::error::{IrErrorKind, IrResult, err};
use crate::ids::TypeId;
use crate::intern::{Hasher64, InternTable, Layered, Layering, NoLayer, Seg, next_id, pool_start};
use crate::target::{DataLayout, MAX_ADDR_SPACE};

/// The largest integer width LLVM accepts (`IntegerType::MAX_INT_BITS`).
pub const MAX_INT_BITS: u32 = 1 << 23;

/// The largest size of a type in bytes: its size in bits fits `u64`.
pub const MAX_TYPE_SIZE: u64 = u64::MAX / 8;

/// The largest alignment, as a power of two (LLVM's `Align` limit).
const MAX_ALIGN_LOG2: u8 = 32;

/// The structure of a type. Obtained from [`TypeTable::kind`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TypeKind<'a> {
    /// `void`.
    Void,
    /// `label`.
    Label,
    /// `metadata`.
    Metadata,
    /// `token`.
    Token,
    /// `half`.
    Half,
    /// `bfloat`.
    BFloat,
    /// `float`.
    Float,
    /// `double`.
    Double,
    /// `fp128`.
    Fp128,
    /// `x86_fp80`.
    X86Fp80,
    /// `iN`, `1 <= N <= MAX_INT_BITS`.
    Int(u32),
    /// `ptr addrspace(N)`.
    Ptr {
        /// Address space. hod supports only 0.
        addr_space: u32,
    },
    /// `<len x elem>`.
    Vector {
        /// Element type: an integer, float or pointer.
        elem: TypeId,
        /// Element count, at least 1.
        len: u32,
    },
    /// `<vscale x min_len x elem>`.
    ScalableVector {
        /// Element type: an integer, float or pointer.
        elem: TypeId,
        /// Element count at `vscale` 1.
        min_len: u32,
    },
    /// `[len x elem]`.
    Array {
        /// Element type.
        elem: TypeId,
        /// Element count.
        len: u64,
    },
    /// `{ fields }`, or `<{ fields }>` when packed. Named or literal.
    Struct {
        /// Field types.
        fields: &'a [TypeId],
        /// Whether fields are laid out with no padding.
        packed: bool,
    },
    /// An opaque struct: a distinct type with no size. The number tells
    /// opaque structs apart; it has no other meaning.
    Opaque(u32),
    /// `ret (params)` or `ret (params, ...)`.
    Function {
        /// Return type.
        ret: TypeId,
        /// Parameter types.
        params: &'a [TypeId],
        /// Whether the function takes variable arguments.
        variadic: bool,
    },
    /// `target("name", ...)`. Only the name is kept, and it has no layout.
    TargetExt {
        /// The extension type's name.
        name: &'a str,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(u8)]
enum Tag {
    Void,
    Label,
    Metadata,
    Token,
    Half,
    BFloat,
    Float,
    Double,
    Fp128,
    X86Fp80,
    Int,
    Ptr,
    Vector,
    Array,
    Struct,
    Opaque,
    Function,
    TargetExt,
}

const SIZED: u8 = 1;
const SCALABLE: u8 = 2;
/// A packed struct.
const PACKED: u8 = 4;
/// A variadic function.
const VARIADIC: u8 = 4;

/// One type: its structure and its layout. 24 bytes.
///
/// `a`, `b`, `c` by tag: `Int` width; `Ptr` address space; `Vector` element,
/// length, 1 when scalable; `Array` element, length low and high halves; `Struct` field-pool start, field count; `Opaque` serial;
/// `Function` return type, parameter-pool start, parameter count;
/// `TargetExt` name index.
#[derive(Clone, Copy, Debug)]
struct TypeData {
    /// Store size in bytes; 0 when unsized.
    size: u64,
    a: u32,
    b: u32,
    c: u32,
    tag: Tag,
    /// log2 of the ABI alignment in bytes; 0 when unsized.
    align_log2: u8,
    flags: u8,
}

/// What interning computes for a type besides its structure.
#[derive(Clone, Copy, Debug)]
struct Layout {
    size: u64,
    align_log2: u8,
    flags: u8,
}

const UNSIZED: Layout = Layout { size: 0, align_log2: 0, flags: 0 };

fn log2(align: u64) -> u8 {
    debug_assert!(align.is_power_of_two());
    align.trailing_zeros() as u8
}

fn too_large<T>(what: &str) -> IrResult<T> {
    err(IrErrorKind::TypeTooLarge, format!("the size of {what} is not representable"))
}

/// `size` rounded up to a multiple of `1 << align_log2`.
#[inline]
fn align_up(size: u64, align_log2: u8) -> u64 {
    let mask = (1u64 << align_log2) - 1;
    (size + mask) & !mask
}

/// The interned types of a module, with their layout.
#[derive(Clone, Debug)]
pub struct TypeTable<L: Layering = NoLayer> {
    layout: DataLayout,
    data: Seg<TypeData, L>,
    /// Struct field types; a struct owns a contiguous range.
    fields: Seg<TypeId, L>,
    /// Struct field byte offsets, parallel to `fields`.
    field_offsets: Seg<u64, L>,
    /// Function parameter types; a function type owns a contiguous range.
    params: Seg<TypeId, L>,
    /// Target extension type names.
    names: Seg<Box<str>, L>,
    interner: InternTable<L>,
    opaque_count: u32,
}

impl TypeTable {
    /// A table holding the types with fixed ids (`TypeId::VOID` to
    /// `TypeId::I128`), laid out for `layout`.
    pub fn new(layout: &DataLayout) -> TypeTable {
        let mut table = TypeTable {
            layout: layout.clone(),
            data: Seg::default(),
            fields: Seg::default(),
            field_offsets: Seg::default(),
            params: Seg::default(),
            names: Seg::default(),
            interner: InternTable::new(),
            opaque_count: 0,
        };
        let fixed = [
            (Tag::Void, 0, TypeId::VOID),
            (Tag::Label, 0, TypeId::LABEL),
            (Tag::Metadata, 0, TypeId::METADATA),
            (Tag::Token, 0, TypeId::TOKEN),
            (Tag::Half, 0, TypeId::HALF),
            (Tag::BFloat, 0, TypeId::BFLOAT),
            (Tag::Float, 0, TypeId::FLOAT),
            (Tag::Double, 0, TypeId::DOUBLE),
            (Tag::Fp128, 0, TypeId::FP128),
            (Tag::X86Fp80, 0, TypeId::X86_FP80),
            (Tag::Ptr, 0, TypeId::PTR),
            (Tag::Int, 1, TypeId::I1),
            (Tag::Int, 8, TypeId::I8),
            (Tag::Int, 16, TypeId::I16),
            (Tag::Int, 32, TypeId::I32),
            (Tag::Int, 64, TypeId::I64),
            (Tag::Int, 128, TypeId::I128),
        ];
        for (tag, a, expected) in fixed {
            let id = table.intern_simple(tag, a, 0, 0).expect("the fixed types are valid");
            assert_eq!(id, expected);
        }
        assert_eq!(table.data.len(), TypeId::FIXED_COUNT as usize);
        table
    }

    // ---- §5.1 reading bodies in parallel ----

    /// The base of the forks of parallel reading: a copy of every type.
    pub(crate) fn seed(&self) -> TypeTable<Layered> {
        TypeTable {
            layout: self.layout.clone(),
            data: self.data.seed(),
            fields: self.fields.seed(),
            field_offsets: self.field_offsets.seed(),
            params: self.params.seed(),
            names: self.names.seed(),
            interner: self.interner.seed(),
            opaque_count: self.opaque_count,
        }
    }

    /// Interns, in order, the types of `tail` (a fork's types from
    /// `first`), with every type id they name translated by `map`, and
    /// appends the id each became to `map`'s list: the same calls, in
    /// the same order, that made them in the fork.
    pub(crate) fn adopt(
        &mut self,
        tail: &TypeTable<Layered>,
        first: usize,
        map: &mut Vec<TypeId>,
        base: usize,
    ) -> IrResult<()> {
        let id_of = |map: &[TypeId], ty: TypeId| -> TypeId {
            if ty.index() < base { ty } else { map[ty.index() - base] }
        };
        let mut scratch = Vec::new();
        for id in first..tail.data.len() {
            let made = match tail.kind(TypeId::new(id as u32)) {
                TypeKind::Int(bits) => self.int(bits)?,
                TypeKind::Ptr { addr_space } => self.ptr(addr_space)?,
                TypeKind::Vector { elem, len } => self.vector(id_of(map, elem), len)?,
                TypeKind::ScalableVector { elem, min_len } => {
                    self.scalable_vector(id_of(map, elem), min_len)?
                }
                TypeKind::Array { elem, len } => self.array(id_of(map, elem), len)?,
                TypeKind::Struct { fields, packed } => {
                    scratch.clear();
                    scratch.extend(fields.iter().map(|&field| id_of(map, field)));
                    self.struct_type(&scratch, packed)?
                }
                TypeKind::Opaque(_) => self.opaque_struct()?,
                TypeKind::Function { ret, params, variadic } => {
                    scratch.clear();
                    scratch.extend(params.iter().map(|&param| id_of(map, param)));
                    self.function(id_of(map, ret), &scratch, variadic)?
                }
                TypeKind::TargetExt { name } => self.target_ext(name)?,
                kind => unreachable!("{kind:?} has a fixed id and is never added"),
            };
            map.push(made);
        }
        Ok(())
    }
}

impl TypeTable<Layered> {
    /// A table with this seed's types and none of its own.
    pub(crate) fn fork(&self) -> TypeTable<Layered> {
        TypeTable {
            layout: self.layout.clone(),
            data: self.data.fork(),
            fields: self.fields.fork(),
            field_offsets: self.field_offsets.fork(),
            params: self.params.fork(),
            names: self.names.fork(),
            interner: self.interner.fork(),
            opaque_count: self.opaque_count,
        }
    }

    /// Whether no type was added since the table was forked.
    pub(crate) fn local_is_empty(&self) -> bool {
        self.data.local_is_empty()
    }

    /// The types from `from` on, copied, for [`TypeTable::adopt`]: a
    /// table that can describe only those.
    pub(crate) fn tail(&self, from: usize) -> TypeTable<Layered> {
        let fields = (from..self.data.len())
            .filter(|&id| self.data[id].tag == Tag::Struct)
            .map(|id| self.data[id].a as usize)
            .min()
            .unwrap_or(self.fields.len());
        let params = (from..self.data.len())
            .filter(|&id| self.data[id].tag == Tag::Function)
            .map(|id| self.data[id].b as usize)
            .min()
            .unwrap_or(self.params.len());
        let names = (from..self.data.len())
            .filter(|&id| self.data[id].tag == Tag::TargetExt)
            .map(|id| self.data[id].a as usize)
            .min()
            .unwrap_or(self.names.len());
        TypeTable {
            layout: self.layout.clone(),
            data: self.data.tail(from),
            fields: self.fields.tail(fields),
            field_offsets: self.field_offsets.tail(fields),
            params: self.params.tail(params),
            names: self.names.tail(names),
            interner: InternTable::new(),
            opaque_count: 0,
        }
    }
}

impl<L: Layering> TypeTable<L> {
    /// The data layout the table computes layouts for.
    pub fn data_layout(&self) -> &DataLayout {
        &self.layout
    }

    /// Number of types in the table.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Never true: the fixed types are always present.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    // ---- constructors ----

    /// `iN`. `InvalidType` unless `1 <= bits <= MAX_INT_BITS`.
    pub fn int(&mut self, bits: u32) -> IrResult<TypeId> {
        match bits {
            1 => Ok(TypeId::I1),
            8 => Ok(TypeId::I8),
            16 => Ok(TypeId::I16),
            32 => Ok(TypeId::I32),
            64 => Ok(TypeId::I64),
            128 => Ok(TypeId::I128),
            _ if (1..=MAX_INT_BITS).contains(&bits) => self.intern_simple(Tag::Int, bits, 0, 0),
            _ => err(IrErrorKind::InvalidType, format!("invalid integer width {bits}")),
        }
    }

    /// `ptr addrspace(addr_space)`. `InvalidType` when the address space
    /// does not fit 24 bits.
    pub fn ptr(&mut self, addr_space: u32) -> IrResult<TypeId> {
        if addr_space == 0 {
            return Ok(TypeId::PTR);
        }
        if addr_space > MAX_ADDR_SPACE {
            return err(IrErrorKind::InvalidType, format!("invalid address space {addr_space}"));
        }
        self.intern_simple(Tag::Ptr, addr_space, 0, 0)
    }

    /// `<len x elem>`. `InvalidType` unless `elem` is an integer, float or
    /// pointer and `len` is at least 1.
    pub fn vector(&mut self, elem: TypeId, len: u32) -> IrResult<TypeId> {
        self.any_vector(elem, len, false)
    }

    /// `<vscale x min_len x elem>`. Errors as [`Self::vector`].
    pub fn scalable_vector(&mut self, elem: TypeId, min_len: u32) -> IrResult<TypeId> {
        self.any_vector(elem, min_len, true)
    }

    fn any_vector(&mut self, elem: TypeId, len: u32, scalable: bool) -> IrResult<TypeId> {
        let tag = self.data[elem.index()].tag;
        if !(Tag::Half..=Tag::Ptr).contains(&tag) {
            return err(
                IrErrorKind::InvalidType,
                "a vector element must be an integer, float or pointer",
            );
        }
        if len == 0 {
            return err(IrErrorKind::InvalidType, "a vector has at least one element");
        }
        self.intern_simple(Tag::Vector, elem.raw(), len, u32::from(scalable))
    }

    /// `[len x elem]`. `InvalidType` when `elem` is `void`, `label`,
    /// `metadata`, `token` or a function type.
    pub fn array(&mut self, elem: TypeId, len: u64) -> IrResult<TypeId> {
        self.check_element(elem, "an array element")?;
        self.intern_simple(Tag::Array, elem.raw(), len as u32, (len >> 32) as u32)
    }

    /// The struct with these fields: `{ fields }`, or `<{ fields }>` when
    /// `packed`. LLVM's literal and named structs both map here (§3: named
    /// structs are interned by structure). `InvalidType` for a field that
    /// [`Self::array`] would reject as an element.
    pub fn struct_type(&mut self, fields: &[TypeId], packed: bool) -> IrResult<TypeId> {
        for &field in fields {
            self.check_element(field, "a struct field")?;
        }
        let flags = if packed { PACKED } else { 0 };
        let mut hasher = Hasher64::new(Tag::Struct as u64);
        hasher.pair(u32::from(flags), fields.len() as u32);
        for field in fields {
            hasher.word(u64::from(field.raw()));
        }
        let hash = hasher.finish();
        let found = self.interner.find(hash, |id| {
            let d = &self.data[id as usize];
            d.tag == Tag::Struct
                && d.flags & PACKED == flags
                && self.fields.slice(d.a as usize, d.b as usize) == fields
        });
        if let Some(id) = found {
            return Ok(TypeId::new(id));
        }
        let id = next_id(self.data.len(), "type")?;
        let start = pool_start(self.fields.len(), fields.len(), "struct field")?;
        // Lay out before touching the pools, so a failure leaves no trace.
        let mut offsets = Vec::with_capacity(fields.len());
        let layout = self.struct_layout(fields, packed, &mut offsets)?;
        self.fields.extend_from_slice(fields);
        self.field_offsets.append(&mut offsets);
        self.data.push(TypeData {
            size: layout.size,
            a: start,
            b: fields.len() as u32,
            c: 0,
            tag: Tag::Struct,
            align_log2: layout.align_log2,
            flags: layout.flags | flags,
        });
        self.interner.insert(hash, id);
        Ok(TypeId::new(id))
    }

    /// A new opaque struct: unsized, and distinct from every other type.
    pub fn opaque_struct(&mut self) -> IrResult<TypeId> {
        let id = next_id(self.data.len(), "type")?;
        let serial = self.opaque_count;
        self.opaque_count += 1;
        self.data.push(TypeData {
            size: 0,
            a: serial,
            b: 0,
            c: 0,
            tag: Tag::Opaque,
            align_log2: 0,
            flags: 0,
        });
        Ok(TypeId::new(id))
    }

    /// The function type `ret (params)`, or `ret (params, ...)` when
    /// `variadic`. `InvalidType` when `ret` is `label`, `metadata` or a
    /// function type, or a parameter is `void`, `label` or a function type.
    pub fn function(&mut self, ret: TypeId, params: &[TypeId], variadic: bool) -> IrResult<TypeId> {
        let invalid = |tags: &[Tag], ty: TypeId| tags.contains(&self.data[ty.index()].tag);
        if invalid(&[Tag::Label, Tag::Metadata, Tag::Function], ret) {
            return err(IrErrorKind::InvalidType, "invalid function return type");
        }
        if params.iter().any(|&p| invalid(&[Tag::Void, Tag::Label, Tag::Function], p)) {
            return err(IrErrorKind::InvalidType, "invalid function parameter type");
        }
        let flags = if variadic { VARIADIC } else { 0 };
        let mut hasher = Hasher64::new(Tag::Function as u64);
        hasher.pair(u32::from(flags), params.len() as u32);
        hasher.word(u64::from(ret.raw()));
        for param in params {
            hasher.word(u64::from(param.raw()));
        }
        let hash = hasher.finish();
        let found = self.interner.find(hash, |id| {
            let d = &self.data[id as usize];
            d.tag == Tag::Function
                && d.flags == flags
                && d.a == ret.raw()
                && self.params.slice(d.b as usize, d.c as usize) == params
        });
        if let Some(id) = found {
            return Ok(TypeId::new(id));
        }
        let id = next_id(self.data.len(), "type")?;
        let start = pool_start(self.params.len(), params.len(), "function parameter")?;
        self.params.extend_from_slice(params);
        self.data.push(TypeData {
            size: 0,
            a: ret.raw(),
            b: start,
            c: params.len() as u32,
            tag: Tag::Function,
            align_log2: 0,
            flags,
        });
        self.interner.insert(hash, id);
        Ok(TypeId::new(id))
    }

    /// The target extension type named `name`. Its type and integer
    /// parameters are not kept, and it has no layout.
    pub fn target_ext(&mut self, name: &str) -> IrResult<TypeId> {
        let mut hasher = Hasher64::new(Tag::TargetExt as u64);
        hasher.bytes(name.as_bytes());
        let hash = hasher.finish();
        let found = self.interner.find(hash, |id| {
            let d = &self.data[id as usize];
            d.tag == Tag::TargetExt && *self.names[d.a as usize] == *name
        });
        if let Some(id) = found {
            return Ok(TypeId::new(id));
        }
        let id = next_id(self.data.len(), "type")?;
        let index = next_id(self.names.len(), "type name")?;
        self.names.push(name.into());
        self.data.push(TypeData {
            size: 0,
            a: index,
            b: 0,
            c: 0,
            tag: Tag::TargetExt,
            align_log2: 0,
            flags: 0,
        });
        self.interner.insert(hash, id);
        Ok(TypeId::new(id))
    }

    fn check_element(&self, elem: TypeId, what: &str) -> IrResult<()> {
        match self.data[elem.index()].tag {
            Tag::Void | Tag::Label | Tag::Metadata | Tag::Token | Tag::Function => {
                err(IrErrorKind::InvalidType, format!("{what} cannot have this type"))
            }
            _ => Ok(()),
        }
    }

    /// Interns a type that is fully described by `(tag, a, b, c)`.
    fn intern_simple(&mut self, tag: Tag, a: u32, b: u32, c: u32) -> IrResult<TypeId> {
        let mut hasher = Hasher64::new(tag as u64);
        hasher.pair(a, b);
        hasher.word(u64::from(c));
        let hash = hasher.finish();
        let found = self.interner.find(hash, |id| {
            let d = &self.data[id as usize];
            d.tag == tag && d.a == a && d.b == b && d.c == c
        });
        if let Some(id) = found {
            return Ok(TypeId::new(id));
        }
        let id = next_id(self.data.len(), "type")?;
        let layout = self.simple_layout(tag, a, b, c)?;
        self.data.push(TypeData {
            size: layout.size,
            a,
            b,
            c,
            tag,
            align_log2: layout.align_log2,
            flags: layout.flags,
        });
        self.interner.insert(hash, id);
        Ok(TypeId::new(id))
    }

    // ---- layout, computed once per type ----

    fn simple_layout(&self, tag: Tag, a: u32, b: u32, c: u32) -> IrResult<Layout> {
        let sized = |size: u64, align: u64| Layout { size, align_log2: log2(align), flags: SIZED };
        Ok(match tag {
            Tag::Half | Tag::BFloat | Tag::Float | Tag::Double | Tag::Fp128 | Tag::X86Fp80 => {
                let bits = float_bits(tag);
                sized(u64::from(bits.div_ceil(8)), self.layout.float_align(bits).abi)
            }
            Tag::Int => sized(u64::from(a.div_ceil(8)), self.layout.int_align(a).abi),
            Tag::Ptr => {
                let spec = self.layout.pointer(a);
                sized(u64::from(spec.bits.div_ceil(8)), spec.align.abi)
            }
            Tag::Vector => {
                // Elements are packed at their bit width: `<3 x i24>` is 72
                // bits, `<8 x i1>` is one byte.
                let bits = u64::from(b) * self.size_in_bits_of(TypeId::new(a));
                let size = bits.div_ceil(8);
                let Some(align) = self.layout.vector_align(bits, size) else {
                    return too_large("a vector");
                };
                if log2(align.abi) > MAX_ALIGN_LOG2 {
                    return too_large("a vector");
                }
                let scalable = if c != 0 { SCALABLE } else { 0 };
                Layout { flags: SIZED | scalable, ..sized(size, align.abi) }
            }
            Tag::Array => {
                let elem = &self.data[a as usize];
                if elem.flags & SIZED == 0 {
                    return Ok(UNSIZED);
                }
                let len = u64::from(b) | (u64::from(c) << 32);
                let stride = align_up(elem.size, elem.align_log2);
                match len.checked_mul(stride) {
                    Some(size) if size <= MAX_TYPE_SIZE => Layout {
                        size,
                        align_log2: elem.align_log2,
                        flags: elem.flags & (SIZED | SCALABLE),
                    },
                    _ => return too_large("an array"),
                }
            }
            _ => UNSIZED,
        })
    }

    /// LLVM's `StructLayout`. Appends one offset per field to `offsets`.
    fn struct_layout(
        &self,
        fields: &[TypeId],
        packed: bool,
        offsets: &mut Vec<u64>,
    ) -> IrResult<Layout> {
        let mut size = 0u64;
        let mut align_log2 = 0u8;
        let mut flags = SIZED;
        for &field in fields {
            let d = &self.data[field.index()];
            if d.flags & SIZED == 0 {
                flags = 0;
            }
            flags |= d.flags & SCALABLE;
            let field_align = if packed { 0 } else { d.align_log2 };
            size = align_up(size, field_align);
            align_log2 = align_log2.max(field_align);
            offsets.push(size);
            // Each field takes its alloc size, in a packed struct too.
            size += align_up(d.size, d.align_log2);
            if size > MAX_TYPE_SIZE {
                return too_large("a struct");
            }
        }
        if flags & SIZED == 0 {
            offsets.fill(0);
            return Ok(UNSIZED);
        }
        // Tail padding to the largest field alignment.
        size = align_up(size, align_log2);
        // The ABI alignment is 1 for a packed struct; otherwise the larger
        // of the fields' and the data layout's aggregate alignment.
        if !packed {
            align_log2 = align_log2.max(log2(self.layout.aggregate_align.abi));
        }
        if size > MAX_TYPE_SIZE {
            return too_large("a struct");
        }
        Ok(Layout { size, align_log2, flags })
    }

    // ---- structure ----

    /// The structure of `ty`.
    ///
    /// Inlined: a caller's match on the kind becomes a match on the
    /// type's tag. The kinds that borrow from the table's pools take
    /// [`TypeTable::kind_with_parts`], out of line.
    #[allow(clippy::inline_always)] // Per type query; LLVM declined `#[inline]`.
    #[inline(always)]
    pub fn kind(&self, ty: TypeId) -> TypeKind<'_> {
        let d = &self.data[ty.index()];
        match d.tag {
            Tag::Void => TypeKind::Void,
            Tag::Label => TypeKind::Label,
            Tag::Metadata => TypeKind::Metadata,
            Tag::Token => TypeKind::Token,
            Tag::Half => TypeKind::Half,
            Tag::BFloat => TypeKind::BFloat,
            Tag::Float => TypeKind::Float,
            Tag::Double => TypeKind::Double,
            Tag::Fp128 => TypeKind::Fp128,
            Tag::X86Fp80 => TypeKind::X86Fp80,
            Tag::Int => TypeKind::Int(d.a),
            Tag::Ptr => TypeKind::Ptr { addr_space: d.a },
            Tag::Vector if d.flags & SCALABLE != 0 => {
                TypeKind::ScalableVector { elem: TypeId::new(d.a), min_len: d.b }
            }
            Tag::Vector => TypeKind::Vector { elem: TypeId::new(d.a), len: d.b },
            Tag::Array => TypeKind::Array {
                elem: TypeId::new(d.a),
                len: u64::from(d.b) | (u64::from(d.c) << 32),
            },
            Tag::Opaque => TypeKind::Opaque(d.a),
            Tag::Struct | Tag::Function | Tag::TargetExt => self.kind_with_parts(d),
        }
    }

    /// [`TypeTable::kind`] of a struct, function or target extension
    /// type: the kinds that borrow from the pools.
    #[inline(never)]
    fn kind_with_parts(&self, d: &TypeData) -> TypeKind<'_> {
        match d.tag {
            Tag::Struct => TypeKind::Struct {
                fields: self.fields.slice(d.a as usize, d.b as usize),
                packed: d.flags & PACKED != 0,
            },
            Tag::Function => TypeKind::Function {
                ret: TypeId::new(d.a),
                params: self.params.slice(d.b as usize, d.c as usize),
                variadic: d.flags & VARIADIC != 0,
            },
            Tag::TargetExt => TypeKind::TargetExt { name: &self.names[d.a as usize] },
            _ => unreachable!("kind_with_parts: a type without parts"),
        }
    }

    /// The width of an integer type.
    #[inline]
    pub fn int_bits(&self, ty: TypeId) -> Option<u32> {
        let d = &self.data[ty.index()];
        (d.tag == Tag::Int).then_some(d.a)
    }

    /// Whether `ty` is an integer type.
    #[inline]
    pub fn is_int(&self, ty: TypeId) -> bool {
        self.data[ty.index()].tag == Tag::Int
    }

    /// Whether `ty` is one of the six floating-point types.
    #[inline]
    pub fn is_float(&self, ty: TypeId) -> bool {
        (Tag::Half..=Tag::X86Fp80).contains(&self.data[ty.index()].tag)
    }

    /// Whether `ty` is a pointer type, in any address space.
    #[inline]
    pub fn is_ptr(&self, ty: TypeId) -> bool {
        self.data[ty.index()].tag == Tag::Ptr
    }

    /// Whether `ty` is a fixed or scalable vector type.
    #[inline]
    pub fn is_vector(&self, ty: TypeId) -> bool {
        self.data[ty.index()].tag == Tag::Vector
    }

    /// The field types of a struct.
    #[inline]
    pub fn struct_fields(&self, ty: TypeId) -> Option<&[TypeId]> {
        let d = &self.data[ty.index()];
        (d.tag == Tag::Struct).then(|| self.fields.slice(d.a as usize, d.b as usize))
    }

    /// The element type of an array or a vector.
    #[inline]
    pub fn element_type(&self, ty: TypeId) -> Option<TypeId> {
        let d = &self.data[ty.index()];
        matches!(d.tag, Tag::Array | Tag::Vector).then_some(TypeId::new(d.a))
    }

    // ---- layout accessors: array lookups ----

    /// Whether `ty` has a size: not `void`, `label`, `metadata`, `token`, a
    /// function type, an opaque struct, a target extension type, or an
    /// aggregate containing one of those.
    #[inline]
    pub fn is_sized(&self, ty: TypeId) -> bool {
        self.data[ty.index()].flags & SIZED != 0
    }

    /// Whether `ty` is a scalable vector or contains one. Its sizes and
    /// offsets are then the minimum ones, to be multiplied by `vscale`.
    #[inline]
    pub fn is_scalable(&self, ty: TypeId) -> bool {
        self.data[ty.index()].flags & SCALABLE != 0
    }

    /// Store size in bytes. 0 for an unsized type.
    #[inline]
    pub fn size_of(&self, ty: TypeId) -> u64 {
        self.data[ty.index()].size
    }

    /// Alloc size in bytes: the store size rounded up to the ABI alignment.
    /// 0 for an unsized type.
    #[inline]
    pub fn alloc_size_of(&self, ty: TypeId) -> u64 {
        let d = &self.data[ty.index()];
        align_up(d.size, d.align_log2)
    }

    /// ABI alignment in bytes, a power of two. 1 for an unsized type.
    #[inline]
    pub fn align_of(&self, ty: TypeId) -> u64 {
        1 << self.data[ty.index()].align_log2
    }

    /// log2 of [`Self::align_of`].
    #[inline]
    pub fn align_log2_of(&self, ty: TypeId) -> u8 {
        self.data[ty.index()].align_log2
    }

    /// Size in bits: the width of an integer, float (80 for `x86_fp80`) or
    /// pointer, the element count times the element width for a vector, and
    /// eight times the store size for anything else.
    #[inline]
    pub fn size_in_bits_of(&self, ty: TypeId) -> u64 {
        let d = &self.data[ty.index()];
        match d.tag {
            Tag::Int => u64::from(d.a),
            Tag::Half | Tag::BFloat | Tag::Float | Tag::Double | Tag::Fp128 | Tag::X86Fp80 => {
                u64::from(float_bits(d.tag))
            }
            Tag::Ptr => u64::from(self.layout.pointer(d.a).bits),
            Tag::Vector => u64::from(d.b) * self.size_in_bits_of(TypeId::new(d.a)),
            _ => d.size * 8,
        }
    }

    /// Byte offset of field `index` of struct `ty`.
    ///
    /// # Panics
    /// Panics if `ty` is not a struct or `index` is out of range, as slice
    /// indexing does. Code handling unvalidated input uses
    /// [`Self::field_offsets`].
    #[inline]
    pub fn field_offset(&self, ty: TypeId, index: u32) -> u64 {
        let d = &self.data[ty.index()];
        assert!(d.tag == Tag::Struct && index < d.b, "no field {index} in {ty:?}");
        self.field_offsets[(d.a + index) as usize]
    }

    /// Byte offsets of the fields of a struct, parallel to
    /// [`Self::struct_fields`]. All zero for an unsized struct.
    #[inline]
    pub fn field_offsets(&self, ty: TypeId) -> Option<&[u64]> {
        let d = &self.data[ty.index()];
        (d.tag == Tag::Struct).then(|| self.field_offsets.slice(d.a as usize, d.b as usize))
    }

    /// The distance in bytes between consecutive elements of an array or
    /// vector, as `getelementptr` steps over them.
    ///
    /// For an array it is the element's alloc size. For a vector it is the
    /// element's width in bytes, because vector elements are packed
    /// (`<4 x i24>` steps by 3). `None` for any other type, and for a vector
    /// whose element width is not a whole number of bytes: its elements are
    /// not addressable.
    #[inline]
    pub fn element_stride(&self, ty: TypeId) -> Option<u64> {
        let d = &self.data[ty.index()];
        match d.tag {
            Tag::Array => Some(self.alloc_size_of(TypeId::new(d.a))),
            Tag::Vector => {
                let bits = self.size_in_bits_of(TypeId::new(d.a));
                bits.is_multiple_of(8).then_some(bits / 8)
            }
            _ => None,
        }
    }

    /// Byte offset of element `index` of an array or vector: `index` times
    /// [`Self::element_stride`]. `None` where the stride is. An index past
    /// the end wraps modulo 2^64, as `getelementptr` arithmetic does.
    #[inline]
    pub fn element_offset(&self, ty: TypeId, index: u64) -> Option<u64> {
        self.element_stride(ty).map(|stride| stride.wrapping_mul(index))
    }

    /// Preferred alignment in bytes (LLVM's `getPrefTypeAlign`). Not stored:
    /// computed on each call, in time proportional to the nesting depth of
    /// arrays. Used only for variables with no alignment of their own.
    pub fn pref_align_of(&self, ty: TypeId) -> u64 {
        let d = &self.data[ty.index()];
        match d.tag {
            Tag::Int => self.layout.int_align(d.a).pref,
            Tag::Half | Tag::BFloat | Tag::Float | Tag::Double | Tag::Fp128 | Tag::X86Fp80 => {
                self.layout.float_align(float_bits(d.tag)).pref
            }
            Tag::Ptr => self.layout.pointer(d.a).align.pref,
            Tag::Vector => self
                .layout
                .vector_align(self.size_in_bits_of(ty), d.size)
                .map_or(1 << d.align_log2, |align| align.pref),
            Tag::Array => self.pref_align_of(TypeId::new(d.a)),
            // A packed struct's ABI alignment is 1, but its preferred
            // alignment is the aggregate one like any other struct's.
            Tag::Struct => self.layout.aggregate_align.pref.max(1 << d.align_log2),
            _ => 1,
        }
    }

    /// The alignment in bytes of a defined global variable of type `ty`
    /// (LLVM's `DataLayout::getPreferredAlign`, as `llc` applies it).
    ///
    /// - With an explicit alignment and a section: exactly that alignment.
    /// - With an explicit alignment: at least that, raised to the type's
    ///   ABI alignment when it is below the preferred alignment.
    /// - With none: the type's preferred alignment, raised to 16 when the
    ///   variable is larger than 16 bytes.
    pub fn global_align(&self, ty: TypeId, explicit: Option<u64>, has_section: bool) -> u64 {
        let pref = self.pref_align_of(ty);
        match explicit {
            Some(align) if has_section => align,
            Some(align) if align >= pref => align,
            Some(align) => align.max(self.align_of(ty)),
            None if pref < 16 && self.size_in_bits_of(ty) > 128 => 16,
            None => pref,
        }
    }
}

fn float_bits(tag: Tag) -> u32 {
    match tag {
        Tag::Half | Tag::BFloat => 16,
        Tag::Float => 32,
        Tag::Double => 64,
        Tag::X86Fp80 => 80,
        Tag::Fp128 => 128,
        _ => unreachable!("not a floating-point type"),
    }
}

#[cfg(test)]
#[allow(clippy::many_single_char_names)]
mod tests {
    use super::*;
    use crate::target::{AARCH64_LINUX_DATA_LAYOUT, X86_64_LINUX_DATA_LAYOUT};

    fn table(layout: &str) -> TypeTable {
        TypeTable::new(&DataLayout::parse(layout).unwrap())
    }

    fn a64() -> TypeTable {
        table(AARCH64_LINUX_DATA_LAYOUT)
    }

    fn x64() -> TypeTable {
        table(X86_64_LINUX_DATA_LAYOUT)
    }

    fn kind_of(result: IrResult<TypeId>) -> IrErrorKind {
        result.unwrap_err().kind
    }

    /// §3: the table entry is compact, and its size is fixed.
    #[test]
    fn type_data_is_24_bytes() {
        assert_eq!(size_of::<TypeData>(), 24);
        assert_eq!(size_of::<TypeKind<'_>>(), 24);
    }

    /// §3: the common types have fixed ids.
    #[test]
    fn fixed_ids() {
        let mut t = a64();
        assert_eq!(t.len(), 17);
        assert!(!t.is_empty());
        assert_eq!(t.kind(TypeId::VOID), TypeKind::Void);
        assert_eq!(t.kind(TypeId::LABEL), TypeKind::Label);
        assert_eq!(t.kind(TypeId::METADATA), TypeKind::Metadata);
        assert_eq!(t.kind(TypeId::TOKEN), TypeKind::Token);
        assert_eq!(t.kind(TypeId::HALF), TypeKind::Half);
        assert_eq!(t.kind(TypeId::BFLOAT), TypeKind::BFloat);
        assert_eq!(t.kind(TypeId::FLOAT), TypeKind::Float);
        assert_eq!(t.kind(TypeId::DOUBLE), TypeKind::Double);
        assert_eq!(t.kind(TypeId::FP128), TypeKind::Fp128);
        assert_eq!(t.kind(TypeId::X86_FP80), TypeKind::X86Fp80);
        assert_eq!(t.kind(TypeId::PTR), TypeKind::Ptr { addr_space: 0 });
        for (bits, id) in [
            (1, TypeId::I1),
            (8, TypeId::I8),
            (16, TypeId::I16),
            (32, TypeId::I32),
            (64, TypeId::I64),
            (128, TypeId::I128),
        ] {
            assert_eq!(t.int(bits).unwrap(), id);
            assert_eq!(t.kind(id), TypeKind::Int(bits));
            assert_eq!(t.int_bits(id), Some(bits));
        }
        assert_eq!(t.ptr(0).unwrap(), TypeId::PTR);
        assert_eq!(t.len(), 17);
    }

    /// §3: structural types are hash-consed.
    #[test]
    fn interning_identity() {
        let mut t = a64();
        let i24 = t.int(24).unwrap();
        assert_eq!(t.int(24).unwrap(), i24);
        assert_ne!(t.int(25).unwrap(), i24);
        let p1 = t.ptr(1).unwrap();
        assert_eq!(t.ptr(1).unwrap(), p1);
        assert_ne!(p1, TypeId::PTR);
        let v = t.vector(TypeId::I32, 4).unwrap();
        assert_eq!(t.vector(TypeId::I32, 4).unwrap(), v);
        assert_ne!(t.vector(TypeId::I32, 5).unwrap(), v);
        assert_ne!(t.vector(TypeId::FLOAT, 4).unwrap(), v);
        let sv = t.scalable_vector(TypeId::I32, 4).unwrap();
        assert_ne!(sv, v);
        assert_eq!(t.scalable_vector(TypeId::I32, 4).unwrap(), sv);
        assert_eq!(t.kind(sv), TypeKind::ScalableVector { elem: TypeId::I32, min_len: 4 });
        // A length above 2^32, and the same length truncated to 32 bits.
        let a = t.array(TypeId::I8, 5_000_000_000).unwrap();
        assert_eq!(t.array(TypeId::I8, 5_000_000_000).unwrap(), a);
        assert_ne!(t.array(TypeId::I8, 0x1_2a05_f200 & 0xffff_ffff).unwrap(), a);
        assert_eq!(t.kind(a), TypeKind::Array { elem: TypeId::I8, len: 5_000_000_000 });
        // An array and a vector with the same element and length differ.
        assert_ne!(t.array(TypeId::I32, 4).unwrap(), v);
        let f = t.function(TypeId::VOID, &[TypeId::I32, TypeId::PTR], false).unwrap();
        assert_eq!(t.function(TypeId::VOID, &[TypeId::I32, TypeId::PTR], false).unwrap(), f);
        assert_ne!(t.function(TypeId::VOID, &[TypeId::I32, TypeId::PTR], true).unwrap(), f);
        assert_ne!(t.function(TypeId::I32, &[TypeId::I32, TypeId::PTR], false).unwrap(), f);
        assert_ne!(t.function(TypeId::VOID, &[TypeId::I32], false).unwrap(), f);
        assert_eq!(
            t.kind(f),
            TypeKind::Function {
                ret: TypeId::VOID,
                params: &[TypeId::I32, TypeId::PTR],
                variadic: false
            }
        );
        let x = t.target_ext("aarch64.svcount").unwrap();
        assert_eq!(t.target_ext("aarch64.svcount").unwrap(), x);
        assert_ne!(t.target_ext("spirv.Image").unwrap(), x);
        assert_eq!(t.kind(x), TypeKind::TargetExt { name: "aarch64.svcount" });
        let before = t.len();
        t.int(24).unwrap();
        t.vector(TypeId::I32, 4).unwrap();
        t.function(TypeId::VOID, &[TypeId::I32, TypeId::PTR], false).unwrap();
        assert_eq!(t.len(), before);
    }

    /// §3: a named struct is its body; an opaque struct is only itself.
    #[test]
    fn structs_are_interned_by_structure_and_opaque_structs_are_distinct() {
        let mut t = a64();
        // `%A = type { i32, ptr }` and `%B = type { i32, ptr }` and the
        // literal `{ i32, ptr }` are one type.
        let a = t.struct_type(&[TypeId::I32, TypeId::PTR], false).unwrap();
        let b = t.struct_type(&[TypeId::I32, TypeId::PTR], false).unwrap();
        assert_eq!(a, b);
        let packed = t.struct_type(&[TypeId::I32, TypeId::PTR], true).unwrap();
        assert_ne!(packed, a);
        assert_ne!(t.struct_type(&[TypeId::PTR, TypeId::I32], false).unwrap(), a);
        assert_ne!(t.struct_type(&[TypeId::I32], false).unwrap(), a);
        let nested = t.struct_type(&[a, a], false).unwrap();
        assert_eq!(t.struct_type(&[b, b], false).unwrap(), nested);
        assert_eq!(
            t.kind(a),
            TypeKind::Struct { fields: &[TypeId::I32, TypeId::PTR], packed: false }
        );
        assert_eq!(
            t.kind(packed),
            TypeKind::Struct { fields: &[TypeId::I32, TypeId::PTR], packed: true }
        );
        assert_eq!(t.struct_fields(nested), Some(&[a, a][..]));
        assert_eq!(t.struct_fields(TypeId::I32), None);
        let empty = t.struct_type(&[], false).unwrap();
        assert_eq!(t.struct_type(&[], false).unwrap(), empty);
        assert_ne!(t.struct_type(&[], true).unwrap(), empty);

        let o1 = t.opaque_struct().unwrap();
        let o2 = t.opaque_struct().unwrap();
        assert_ne!(o1, o2);
        assert_eq!(t.kind(o1), TypeKind::Opaque(0));
        assert_eq!(t.kind(o2), TypeKind::Opaque(1));
        assert!(!t.is_sized(o1));
        // Aggregates of an opaque struct are unsized, and still interned.
        let holder = t.struct_type(&[TypeId::I32, o1], false).unwrap();
        assert!(!t.is_sized(holder));
        assert_eq!(t.field_offsets(holder), Some(&[0, 0][..]));
        assert_eq!(t.struct_type(&[TypeId::I32, o1], false).unwrap(), holder);
        assert_ne!(t.struct_type(&[TypeId::I32, o2], false).unwrap(), holder);
        let arr = t.array(o1, 4).unwrap();
        assert!(!t.is_sized(arr));
        assert_eq!(t.size_of(arr), 0);
        assert_eq!(t.align_of(arr), 1);
    }

    /// §3 `InvalidType`.
    #[test]
    fn invalid_types_are_errors() {
        let mut t = a64();
        let func = t.function(TypeId::VOID, &[], false).unwrap();
        let st = t.struct_type(&[TypeId::I32], false).unwrap();
        let arr = t.array(TypeId::I32, 2).unwrap();
        let vec = t.vector(TypeId::I32, 2).unwrap();
        let opaque = t.opaque_struct().unwrap();
        let before = t.len();
        let invalid = IrErrorKind::InvalidType;
        assert_eq!(kind_of(t.int(0)), invalid);
        assert_eq!(kind_of(t.int(MAX_INT_BITS + 1)), invalid);
        assert_eq!(kind_of(t.ptr(MAX_ADDR_SPACE + 1)), invalid);
        for elem in [
            TypeId::VOID,
            TypeId::LABEL,
            TypeId::METADATA,
            TypeId::TOKEN,
            st,
            arr,
            vec,
            func,
            opaque,
        ] {
            assert_eq!(kind_of(t.vector(elem, 4)), invalid, "{elem:?}");
            assert_eq!(kind_of(t.scalable_vector(elem, 4)), invalid, "{elem:?}");
        }
        assert_eq!(kind_of(t.vector(TypeId::I32, 0)), invalid);
        for elem in [TypeId::VOID, TypeId::LABEL, TypeId::METADATA, TypeId::TOKEN, func] {
            assert_eq!(kind_of(t.array(elem, 4)), invalid, "{elem:?}");
            assert_eq!(kind_of(t.struct_type(&[TypeId::I32, elem], false)), invalid, "{elem:?}");
        }
        for ret in [TypeId::LABEL, TypeId::METADATA, func] {
            assert_eq!(kind_of(t.function(ret, &[], false)), invalid);
        }
        for param in [TypeId::VOID, TypeId::LABEL, func] {
            assert_eq!(kind_of(t.function(TypeId::VOID, &[TypeId::I32, param], true)), invalid);
        }
        assert_eq!(t.len(), before, "a rejected type leaves no entry");
        // Accepted where LLVM requires them.
        t.function(TypeId::TOKEN, &[TypeId::METADATA, TypeId::TOKEN], false).unwrap();
        t.int(MAX_INT_BITS).unwrap();
        t.ptr(MAX_ADDR_SPACE).unwrap();
    }

    /// §3 `TypeTooLarge`: sizes are an error, not a wrap.
    #[test]
    fn oversized_types_are_errors() {
        let mut t = a64();
        let too_large = IrErrorKind::TypeTooLarge;
        let big = t.array(TypeId::I8, MAX_TYPE_SIZE).unwrap();
        assert_eq!(t.size_of(big), MAX_TYPE_SIZE);
        assert_eq!(kind_of(t.array(TypeId::I8, MAX_TYPE_SIZE + 1)), too_large);
        assert_eq!(kind_of(t.array(TypeId::I64, MAX_TYPE_SIZE / 8 + 1)), too_large);
        assert_eq!(kind_of(t.array(TypeId::I64, u64::MAX)), too_large);
        assert_eq!(kind_of(t.array(big, 2)), too_large);
        assert_eq!(kind_of(t.struct_type(&[big, TypeId::I8], false)), too_large);
        assert_eq!(
            kind_of(t.struct_type(&[big, big, big, big, big, big, big, big, big], true)),
            too_large
        );
        // The natural alignment of a huge vector is past LLVM's limit.
        let wide = t.int(MAX_INT_BITS).unwrap();
        assert_eq!(kind_of(t.vector(wide, u32::MAX)), too_large);
        let fields_before = t.fields.len();
        assert_eq!(kind_of(t.struct_type(&[TypeId::I8, big, TypeId::I8], false)), too_large);
        assert_eq!(t.fields.len(), fields_before);
        assert_eq!(t.field_offsets.len(), fields_before);
        // A zero-length array of a huge type is fine.
        assert_eq!(t.array(big, 0).map(|ty| t.size_of(ty)), Ok(0));
    }

    fn layout(t: &TypeTable, ty: TypeId) -> (u64, u64, u64) {
        (t.size_of(ty), t.alloc_size_of(ty), t.align_of(ty))
    }

    /// §3: scalar layout on both targets (store size, alloc size, alignment).
    #[test]
    fn scalar_layout() {
        for (mut t, x86) in [(a64(), false), (x64(), true)] {
            assert_eq!(layout(&t, TypeId::I1), (1, 1, 1));
            assert_eq!(layout(&t, TypeId::I8), (1, 1, 1));
            assert_eq!(layout(&t, TypeId::I16), (2, 2, 2));
            assert_eq!(layout(&t, TypeId::I32), (4, 4, 4));
            assert_eq!(layout(&t, TypeId::I64), (8, 8, 8));
            assert_eq!(layout(&t, TypeId::I128), (16, 16, 16));
            assert_eq!(layout(&t, TypeId::HALF), (2, 2, 2));
            assert_eq!(layout(&t, TypeId::BFLOAT), (2, 2, 2));
            assert_eq!(layout(&t, TypeId::FLOAT), (4, 4, 4));
            assert_eq!(layout(&t, TypeId::DOUBLE), (8, 8, 8));
            assert_eq!(layout(&t, TypeId::FP128), (16, 16, 16));
            assert_eq!(layout(&t, TypeId::X86_FP80), (10, 16, 16));
            assert_eq!(layout(&t, TypeId::PTR), (8, 8, 8));
            for (bits, expected) in [
                (2, (1, 1, 1)),
                (7, (1, 1, 1)),
                (9, (2, 2, 2)),
                (17, (3, 4, 4)),
                (24, (3, 4, 4)),
                (33, (5, 8, 8)),
                (48, (6, 8, 8)),
                (65, (9, 16, 16)),
                (96, (12, 16, 16)),
                (129, (17, 32, 16)),
                (256, (32, 32, 16)),
                (257, (33, 48, 16)),
            ] {
                let ty = t.int(bits).unwrap();
                assert_eq!(layout(&t, ty), expected, "i{bits}");
                assert_eq!(t.size_in_bits_of(ty), u64::from(bits));
            }
            let p270 = t.ptr(270).unwrap();
            assert_eq!(layout(&t, p270), (4, 4, 4));
            assert_eq!(t.size_in_bits_of(p270), 32);
            let p1 = t.ptr(1).unwrap();
            assert_eq!(layout(&t, p1), (8, 8, 8));
            assert_eq!(t.size_in_bits_of(TypeId::X86_FP80), 80);
            assert_eq!(t.size_in_bits_of(TypeId::FP128), 128);
            // Preferred alignment differs between the targets for small
            // integers only.
            assert_eq!(t.pref_align_of(TypeId::I8), if x86 { 1 } else { 4 });
            assert_eq!(t.pref_align_of(TypeId::I16), if x86 { 2 } else { 4 });
            assert_eq!(t.pref_align_of(TypeId::I64), 8);
            for unsized_ty in [TypeId::VOID, TypeId::LABEL, TypeId::METADATA, TypeId::TOKEN] {
                assert!(!t.is_sized(unsized_ty));
                assert_eq!(layout(&t, unsized_ty), (0, 0, 1));
            }
            assert!(t.is_sized(TypeId::I1) && !t.is_scalable(TypeId::I1));
        }
        // With LLVM's default layout i64 is 4-aligned and i128 follows it.
        let mut defaults = table("");
        assert_eq!(layout(&defaults, TypeId::I64), (8, 8, 4));
        assert_eq!(layout(&defaults, TypeId::I128), (16, 16, 4));
        let i65 = defaults.int(65).unwrap();
        assert_eq!(layout(&defaults, i65), (9, 12, 4));
    }

    /// §3: vector layout: packed elements, and the natural-alignment
    /// fallback for widths the data layout does not name.
    #[test]
    fn vector_layout() {
        let mut t = a64();
        let mut vec = |elem: TypeId, len: u32| {
            let ty = t.vector(elem, len).unwrap();
            (layout(&t, ty), t.size_in_bits_of(ty), t.element_stride(ty))
        };
        assert_eq!(vec(TypeId::I32, 4), ((16, 16, 16), 128, Some(4)));
        assert_eq!(vec(TypeId::I32, 2), ((8, 8, 8), 64, Some(4)));
        assert_eq!(vec(TypeId::I32, 3), ((12, 16, 16), 96, Some(4)));
        assert_eq!(vec(TypeId::I32, 1), ((4, 4, 4), 32, Some(4)));
        assert_eq!(vec(TypeId::I8, 3), ((3, 4, 4), 24, Some(1)));
        assert_eq!(vec(TypeId::I8, 17), ((17, 32, 32), 136, Some(1)));
        assert_eq!(vec(TypeId::I64, 8), ((64, 64, 64), 512, Some(8)));
        assert_eq!(vec(TypeId::I1, 8), ((1, 1, 1), 8, None));
        assert_eq!(vec(TypeId::I1, 3), ((1, 1, 1), 3, None));
        assert_eq!(vec(TypeId::I1, 9), ((2, 2, 2), 9, None));
        assert_eq!(vec(TypeId::I1, 64), ((8, 8, 8), 64, None));
        assert_eq!(vec(TypeId::PTR, 3), ((24, 32, 32), 192, Some(8)));
        assert_eq!(vec(TypeId::X86_FP80, 2), ((20, 32, 32), 160, Some(10)));
        assert_eq!(vec(TypeId::DOUBLE, 2), ((16, 16, 16), 128, Some(8)));
        let i24 = t.int(24).unwrap();
        let v = t.vector(i24, 3).unwrap();
        assert_eq!(layout(&t, v), (9, 16, 16));
        assert_eq!(t.element_stride(v), Some(3));
        assert_eq!(t.element_offset(v, 2), Some(6));
        assert_eq!(t.element_type(v), Some(i24));
        assert!(t.is_vector(v) && !t.is_vector(i24));
        assert_eq!(t.pref_align_of(v), 16);
    }

    /// §3: scalable vectors have a minimum size and a flag, which
    /// aggregates inherit.
    #[test]
    fn scalable_vector_layout() {
        let mut t = a64();
        let sv = t.scalable_vector(TypeId::I32, 4).unwrap();
        assert!(t.is_sized(sv) && t.is_scalable(sv));
        assert_eq!(layout(&t, sv), (16, 16, 16));
        let sb = t.scalable_vector(TypeId::I1, 16).unwrap();
        assert_eq!(layout(&t, sb), (2, 2, 2));
        let fixed = t.vector(TypeId::I32, 4).unwrap();
        assert!(!t.is_scalable(fixed));
        let arr = t.array(sv, 3).unwrap();
        assert!(t.is_scalable(arr));
        assert_eq!(t.size_of(arr), 48);
        let st = t.struct_type(&[sv, sv], false).unwrap();
        assert!(t.is_scalable(st) && t.is_sized(st));
        assert_eq!(t.field_offsets(st), Some(&[0, 16][..]));
        let plain = t.struct_type(&[fixed, fixed], false).unwrap();
        assert!(!t.is_scalable(plain));
    }

    /// §3: arrays, including zero-length ones.
    #[test]
    fn array_layout() {
        let mut t = x64();
        let i24 = t.int(24).unwrap();
        let a = t.array(i24, 5).unwrap();
        assert_eq!(layout(&t, a), (20, 20, 4));
        assert_eq!(t.element_stride(a), Some(4));
        assert_eq!(t.element_offset(a, 3), Some(12));
        assert_eq!(t.element_offset(a, u64::MAX), Some(4u64.wrapping_neg()));
        assert_eq!(t.element_type(a), Some(i24));
        let z = t.array(TypeId::I64, 0).unwrap();
        assert_eq!(layout(&t, z), (0, 0, 8));
        let f80 = t.array(TypeId::X86_FP80, 3).unwrap();
        assert_eq!(layout(&t, f80), (48, 48, 16));
        let nested = t.array(a, 2).unwrap();
        assert_eq!(layout(&t, nested), (40, 40, 4));
        assert_eq!(t.size_in_bits_of(nested), 320);
        assert_eq!(t.element_stride(TypeId::I32), None);
        assert_eq!(t.element_offset(TypeId::I32, 1), None);
        assert_eq!(t.element_type(TypeId::I32), None);
        assert_eq!(t.pref_align_of(nested), 4);
    }

    /// §3: struct layout by LLVM's `StructLayout`: padded, packed, nested,
    /// empty.
    #[test]
    fn struct_layout_rules() {
        let mut t = a64();
        let i24 = t.int(24).unwrap();
        let i65 = t.int(65).unwrap();
        let s = t.struct_type(&[TypeId::I8, TypeId::I32, TypeId::I8, TypeId::I64], false).unwrap();
        assert_eq!(layout(&t, s), (24, 24, 8));
        assert_eq!(t.field_offsets(s), Some(&[0, 4, 8, 16][..]));
        assert_eq!(t.field_offset(s, 3), 16);
        let p = t.struct_type(&[TypeId::I8, TypeId::I32, TypeId::I8, TypeId::I64], true).unwrap();
        assert_eq!(layout(&t, p), (14, 14, 1));
        assert_eq!(t.field_offsets(p), Some(&[0, 1, 5, 6][..]));
        // A packed field still takes its alloc size: i24 takes 4 bytes,
        // x86_fp80 takes 16, i65 takes 16.
        let q = t.struct_type(&[TypeId::I8, i24, TypeId::X86_FP80, i65, TypeId::I8], true).unwrap();
        assert_eq!(t.field_offsets(q), Some(&[0, 1, 5, 21, 37][..]));
        assert_eq!(layout(&t, q), (38, 38, 1));
        let u =
            t.struct_type(&[TypeId::I8, i24, TypeId::X86_FP80, i65, TypeId::I8], false).unwrap();
        assert_eq!(t.field_offsets(u), Some(&[0, 4, 16, 32, 48][..]));
        assert_eq!(layout(&t, u), (64, 64, 16));
        let empty = t.struct_type(&[], false).unwrap();
        assert_eq!(layout(&t, empty), (0, 0, 1));
        assert_eq!(t.field_offsets(empty), Some(&[][..]));
        let zero_len = t.array(TypeId::I64, 0).unwrap();
        let tail = t.struct_type(&[TypeId::I8, zero_len, empty, TypeId::I8], false).unwrap();
        assert_eq!(t.field_offsets(tail), Some(&[0, 8, 8, 8][..]));
        assert_eq!(layout(&t, tail), (16, 16, 8));
        let nested = t.struct_type(&[TypeId::I8, s, p, TypeId::I16], false).unwrap();
        assert_eq!(t.field_offsets(nested), Some(&[0, 8, 32, 46][..]));
        assert_eq!(layout(&t, nested), (48, 48, 8));
        assert_eq!(t.field_offsets(TypeId::I32), None);
        // Preferred alignment of any struct is at least the aggregate one.
        assert_eq!(t.pref_align_of(p), 8);
        assert_eq!(t.pref_align_of(empty), 8);
        assert_eq!(t.pref_align_of(u), 16);
        // An aggregate ABI alignment raises the alignment and the alloc
        // size, but not the store size, and not for a packed struct.
        let mut wide = table("a:64");
        let one = wide.struct_type(&[TypeId::I8], false).unwrap();
        assert_eq!(layout(&wide, one), (1, 8, 8));
        let one_packed = wide.struct_type(&[TypeId::I8], true).unwrap();
        assert_eq!(layout(&wide, one_packed), (1, 1, 1));
        let outer = wide.struct_type(&[TypeId::I8, one, TypeId::I8], false).unwrap();
        assert_eq!(wide.field_offsets(outer), Some(&[0, 8, 16][..]));
        assert_eq!(layout(&wide, outer), (24, 24, 8));
    }

    /// §3: `field_offset` panics like slice indexing when misused.
    #[test]
    #[should_panic(expected = "no field 2")]
    fn field_offset_out_of_range_panics() {
        let mut t = a64();
        let s = t.struct_type(&[TypeId::I8, TypeId::I32], false).unwrap();
        t.field_offset(s, 2);
    }

    /// §3: the alignment of a variable with and without one of its own.
    #[test]
    fn global_alignment() {
        let mut t = a64();
        let a16 = t.array(TypeId::I8, 16).unwrap();
        let a17 = t.array(TypeId::I8, 17).unwrap();
        let shorts = t.array(TypeId::I16, 40).unwrap();
        assert_eq!(t.global_align(TypeId::I8, None, false), 4);
        assert_eq!(t.global_align(a16, None, false), 4);
        assert_eq!(t.global_align(a17, None, false), 16);
        assert_eq!(t.global_align(TypeId::I64, Some(1), false), 8);
        assert_eq!(t.global_align(TypeId::I64, Some(1), true), 1);
        assert_eq!(t.global_align(shorts, Some(2), false), 2);
        assert_eq!(t.global_align(shorts, Some(32), false), 32);
        assert_eq!(t.global_align(TypeId::I8, Some(2), false), 2);
        assert_eq!(t.global_align(TypeId::I128, None, false), 16);
    }

    /// §3: predicates.
    #[test]
    fn predicates() {
        let t = a64();
        assert!(t.is_int(TypeId::I32) && !t.is_int(TypeId::FLOAT));
        assert!(t.is_ptr(TypeId::PTR) && !t.is_ptr(TypeId::I64));
        for ty in [
            TypeId::HALF,
            TypeId::BFLOAT,
            TypeId::FLOAT,
            TypeId::DOUBLE,
            TypeId::FP128,
            TypeId::X86_FP80,
        ] {
            assert!(t.is_float(ty));
        }
        assert!(!t.is_float(TypeId::I32) && !t.is_float(TypeId::PTR) && !t.is_float(TypeId::TOKEN));
        assert_eq!(t.int_bits(TypeId::PTR), None);
        assert_eq!(t.data_layout(), &DataLayout::parse(AARCH64_LINUX_DATA_LAYOUT).unwrap());
    }
}
