//! The producer kit (`spec/ir.md` "Producer kit", design 0004 H3): the
//! mapping from LLVM's constructs to `hod_ir` that every producer makes
//! the same way, written once. `hod_ll` and `hod_bc` call it; a third
//! producer (a front end lowering into `hod_ir` directly) calls it the
//! same way, and gets what the readers get.
//!
//! Every helper is cheap on the hot path: no boxing, no allocation per
//! call (the scratch vectors are the caller's), and the generic ones are
//! inlined into the caller.

use crate::{
    AttrClass, AttrSyntax, CallDesc, Const, ConstId, ConstPool, FnAttrs, FnAttrsView, FnStr,
    FunctionBuilder, IrResult, Layering, Module, ParamAttrs, ParamMem, TypeId, TypeKind, TypeTable,
    UwTable, Value, attr_info, sext64,
};
use crate::{DebugInfo, DiAtom, DiLocation, FlagValue, FnAttrsId, Retained};
use crate::{
    DiFieldValue, DiId, DiKind, DiMapping, DiNode, DiOther, DiTuple, ExprId, LocId, SymId,
    SymbolKind, di_field, di_record,
};
use std::borrow::Cow;

/// What a lowering made of a construct.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lowered {
    /// The value, built.
    Built(Value),
    /// `hod_ir` cannot represent the construct: the producer records
    /// `construct` (`Module::record_unsupported`, against the function)
    /// and uses `stand_in` for the result, in a body that will not be
    /// kept.
    Refused { construct: &'static str, stand_in: Value },
}

impl Lowered {
    /// The value, built or standing in.
    #[inline]
    pub fn value(self) -> Value {
        match self {
            Lowered::Built(value) | Lowered::Refused { stand_in: value, .. } => value,
        }
    }

    /// The construct to record, if refused.
    #[inline]
    pub fn refused(self) -> Option<&'static str> {
        match self {
            Lowered::Built(_) => None,
            Lowered::Refused { construct, .. } => Some(construct),
        }
    }
}

/// What stands in for the result of type `ty` of an instruction `hod_ir`
/// cannot represent: no value for `void`, `none` for `token`, `poison`
/// for any other type.
///
/// # Errors
/// As [`ConstPool::poison`].
pub fn stand_in<L: Layering>(module: &mut Module<L>, ty: TypeId) -> IrResult<Value> {
    let id = match ty {
        TypeId::VOID => return Ok(Value::NONE),
        TypeId::TOKEN => module.consts.none_token(),
        _ => module.consts.poison(&module.types, ty),
    };
    Value::constant(id?)
}

// ---- getelementptr ----

/// Why a `getelementptr` is malformed (LLVM's parser refuses it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GepError {
    /// The base (or each element of a vector base) is not a pointer.
    BaseNotPointer,
    /// The source element type is not sized.
    SourceUnsized,
    /// An index (or its element) is not an integer.
    IndexNotInteger,
    /// A vector index's length differs from the base's or an earlier
    /// index's.
    VectorLength,
    /// A struct index that is not a constant in range, or an index into a
    /// type that has no elements.
    InvalidIndices,
}

impl GepError {
    /// LLVM's message for the error.
    pub fn message(self) -> &'static str {
        match self {
            GepError::BaseNotPointer => "base of getelementptr must be a pointer",
            GepError::SourceUnsized => "base element of getelementptr must be sized",
            GepError::IndexNotInteger => "getelementptr index must be an integer",
            GepError::VectorLength => "getelementptr vector index has a wrong number of elements",
            GepError::InvalidIndices => "invalid getelementptr indices",
        }
    }
}

/// The length of a vector type and whether it is scalable.
type Shape = Option<(u32, bool)>;

fn shape<L: Layering>(types: &TypeTable<L>, ty: TypeId) -> (TypeId, Shape) {
    match types.kind(ty) {
        TypeKind::Vector { elem, len } => (elem, Some((len, false))),
        TypeKind::ScalableVector { elem, min_len } => (elem, Some((min_len, true))),
        _ => (ty, None),
    }
}

/// The value of a constant used as a `getelementptr` index: sign-extended
/// or truncated to 64 bits. `None` if it is not one integer (the same in
/// every element, for a vector).
#[allow(clippy::inline_always)] // Per instruction; LLVM declined `#[inline]`.
#[inline(always)]
pub fn gep_index_value<L: Layering>(
    types: &TypeTable<L>,
    consts: &ConstPool<L>,
    id: ConstId,
) -> Option<i64> {
    match consts.get(id) {
        Const::Int { ty, value } => Some(sext64(value, types.int_bits(ty).unwrap_or(64))),
        Const::WideInt { words, .. } => Some(words[0] as i64),
        Const::Zero(_) => Some(0),
        Const::Splat { elem, .. } => gep_index_value(types, consts, elem),
        Const::Aggregate { elems, .. } => {
            let first = gep_index_value(types, consts, *elems.first()?)?;
            elems[1..]
                .iter()
                .all(|&elem| gep_index_value(types, consts, elem) == Some(first))
                .then_some(first)
        }
        _ => None,
    }
}

/// One `getelementptr` lowered to a `PtrAdd` (`spec/ir.md` §5.4), index
/// by index, as the producer reads them: [`Gep::new`] with the source
/// element type and the base's type, then for each index
/// [`Gep::index_type`] and [`Gep::index`], then [`Gep::build`].
///
/// Every constant index is folded into the byte offset (wrapping, in 64
/// bits); a variable one is pushed on the caller's list with its scale.
/// `inbounds`, `nuw` and `nusw` are not kept.
#[derive(Clone, Copy, Debug)]
pub struct Gep {
    pointer: TypeId,
    shape: Shape,
    current: TypeId,
    first: bool,
    offset: i64,
    unsupported: Option<&'static str>,
}

impl Gep {
    /// Starts a `getelementptr` over `source` from a base of type
    /// `base_ty` (a pointer, or a vector of pointers).
    ///
    /// # Errors
    /// `BaseNotPointer`, then `SourceUnsized`.
    #[inline]
    pub fn new<L: Layering>(
        types: &TypeTable<L>,
        source: TypeId,
        base_ty: TypeId,
    ) -> Result<Gep, GepError> {
        let (pointer, shape) = shape(types, base_ty);
        if !types.is_ptr(pointer) {
            return Err(GepError::BaseNotPointer);
        }
        if !types.is_sized(source) {
            return Err(GepError::SourceUnsized);
        }
        Ok(Gep { pointer, shape, current: source, first: true, offset: 0, unsupported: None })
    }

    /// Checks the type of the next index and returns its width in bits;
    /// a vector index makes the result a vector.
    ///
    /// # Errors
    /// `IndexNotInteger`, `VectorLength`.
    #[inline]
    pub fn index_type<L: Layering>(
        &mut self,
        types: &TypeTable<L>,
        index_ty: TypeId,
    ) -> Result<u32, GepError> {
        let (scalar, index_shape) = shape(types, index_ty);
        let Some(bits) = types.int_bits(scalar) else {
            return Err(GepError::IndexNotInteger);
        };
        if index_shape.is_some() {
            if self.shape.is_some() && self.shape != index_shape {
                return Err(GepError::VectorLength);
            }
            self.shape = index_shape;
        }
        Ok(bits)
    }

    /// The next index: `value`, whose constant value (if it is one, as
    /// [`gep_index_value`] reads it) is `constant`. A variable index is
    /// pushed on `indices` with its scale.
    ///
    /// # Errors
    /// `InvalidIndices`.
    #[allow(clippy::inline_always)] // Per instruction; LLVM declined `#[inline]`.
    #[inline(always)]
    pub fn index<L: Layering>(
        &mut self,
        types: &TypeTable<L>,
        value: Value,
        constant: Option<i64>,
        indices: &mut Vec<(Value, u64)>,
    ) -> Result<(), GepError> {
        let current = self.current;
        let stride = if self.first {
            self.first = false;
            (!types.is_scalable(current)).then(|| types.alloc_size_of(current))
        } else {
            match types.kind(current) {
                TypeKind::Struct { fields, .. } => {
                    let field = constant.and_then(|index| {
                        usize::try_from(index).ok().filter(|&index| index < fields.len())
                    });
                    let Some(field) = field else {
                        return Err(GepError::InvalidIndices);
                    };
                    let field_ty = fields[field];
                    if types.is_scalable(current) {
                        self.unsupported = Some("getelementptr:scalable");
                    } else {
                        let at_field = types.field_offset(current, field as u32) as i64;
                        self.offset = self.offset.wrapping_add(at_field);
                    }
                    self.current = field_ty;
                    return Ok(());
                }
                TypeKind::Array { elem, .. } => {
                    self.current = elem;
                    (!types.is_scalable(elem)).then(|| types.alloc_size_of(elem))
                }
                TypeKind::Vector { elem, .. } => {
                    let stride = types.element_stride(current);
                    if stride.is_none() {
                        self.unsupported = Some("getelementptr:vector-element-not-bytes");
                    }
                    self.current = elem;
                    stride.or(Some(0))
                }
                TypeKind::ScalableVector { elem, .. } => {
                    self.current = elem;
                    None
                }
                _ => return Err(GepError::InvalidIndices),
            }
        };
        let Some(stride) = stride else {
            self.unsupported = Some("getelementptr:scalable");
            return Ok(());
        };
        match constant {
            Some(index) => {
                self.offset = self.offset.wrapping_add(index.wrapping_mul(stride as i64));
            }
            None => indices.push((value, stride)),
        }
        Ok(())
    }

    /// The byte offset the constant indices add up to so far.
    #[inline]
    pub fn offset(&self) -> i64 {
        self.offset
    }

    /// The type of the result: the base's pointer type, a vector of it if
    /// the base or an index is a vector.
    ///
    /// # Errors
    /// As [`TypeTable::vector`].
    #[inline]
    pub fn result_type<L: Layering>(&self, types: &mut TypeTable<L>) -> IrResult<TypeId> {
        match self.shape {
            None => Ok(self.pointer),
            Some((len, false)) => types.vector(self.pointer, len),
            Some((len, true)) => types.scalable_vector(self.pointer, len),
        }
    }

    /// The `PtrAdd` of `base` with the variable `indices`, or the reason
    /// `hod_ir` cannot represent the address (`getelementptr:scalable`,
    /// `getelementptr:vector-element-not-bytes`) and a stand-in; with the
    /// result's type.
    ///
    /// # Errors
    /// As [`Gep::result_type`], [`stand_in`] and
    /// [`FunctionBuilder::ptr_add`].
    #[inline]
    pub fn build<L: Layering>(
        self,
        module: &mut Module<L>,
        b: &mut FunctionBuilder,
        base: Value,
        indices: &[(Value, u64)],
    ) -> IrResult<(TypeId, Lowered)> {
        let ty = self.result_type(&mut module.types)?;
        if let Some(construct) = self.unsupported {
            return Ok((ty, Lowered::Refused { construct, stand_in: stand_in(module, ty)? }));
        }
        Ok((ty, Lowered::Built(b.ptr_add(ty, base, self.offset, indices)?)))
    }
}

// ---- alloca ----

/// An `alloca` as the producer read it (`spec/ir.md` §5.2).
#[derive(Clone, Copy, Debug)]
pub struct AllocaDesc {
    /// The allocated type.
    pub ty: TypeId,
    /// The element count (of an integer type); `None` is one element.
    pub count: Option<Value>,
    /// The alignment written; `None` is the type's preferred alignment.
    pub align: Option<u64>,
    /// The address space of the result.
    pub addr_space: u32,
    /// Whether the `alloca` is in the function's entry block.
    pub in_entry_block: bool,
    /// `inalloca`.
    pub in_alloca: bool,
    /// `swifterror`.
    pub swift_error: bool,
}

/// Why an `alloca` could not be lowered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AllocaError {
    /// The type is not sized, or is `token` (LLVM's "invalid type for
    /// alloca").
    InvalidType,
    /// The builder or a table refused.
    Ir(crate::IrError),
}

impl From<crate::IrError> for AllocaError {
    fn from(error: crate::IrError) -> AllocaError {
        AllocaError::Ir(error)
    }
}

/// An `alloca`: a slot when it is static (in the entry block, in address
/// space 0, with a constant count whose product with the element size
/// does not overflow), else the `Alloca` instruction (a count of `i32 1`
/// when none is given). Refused, in this order: `alloca:inalloca`,
/// `alloca:swifterror`, `alloca:scalable`. Returns the result's pointer
/// type with the value.
///
/// # Errors
/// `InvalidType` before anything else; then the builder's errors.
#[allow(clippy::inline_always)] // Per instruction; LLVM declined `#[inline]`.
#[inline(always)]
pub fn lower_alloca<L: Layering>(
    module: &mut Module<L>,
    b: &mut FunctionBuilder,
    desc: &AllocaDesc,
) -> Result<(TypeId, Lowered), AllocaError> {
    let ty = desc.ty;
    if !module.types.is_sized(ty) || ty == TypeId::TOKEN {
        return Err(AllocaError::InvalidType);
    }
    let space = desc.addr_space;
    let result = if space == 0 { TypeId::PTR } else { module.types.ptr(space)? };
    let refused = if desc.in_alloca {
        Some("alloca:inalloca")
    } else if desc.swift_error {
        Some("alloca:swifterror")
    } else if module.types.is_scalable(ty) {
        Some("alloca:scalable")
    } else {
        None
    };
    if let Some(construct) = refused {
        return Ok((result, Lowered::Refused { construct, stand_in: stand_in(module, result)? }));
    }
    let types = &module.types;
    let elem_size = types.alloc_size_of(ty);
    let align = desc.align.unwrap_or_else(|| types.pref_align_of(ty));
    let literal = match desc.count {
        None => Some(1),
        Some(value) => value.as_const().and_then(|id| module.consts.int_value(id)),
    };
    if desc.in_entry_block
        && space == 0
        && let Some(size) = literal.and_then(|count| elem_size.checked_mul(count))
    {
        let slot = b.add_slot(size, align)?;
        return Ok((result, Lowered::Built(b.slot(slot)?)));
    }
    let count = match desc.count {
        Some(count) => count,
        None => Value::constant(module.consts.int(&module.types, TypeId::I32, 1)?)?,
    };
    Ok((result, Lowered::Built(b.alloca(result, count, elem_size, align)?)))
}

// ---- attributes (`spec/ir.md` §5.7) ----

/// The call-site function attributes `hod_ir` keeps: `CallDesc`'s flags.
pub const CALL_SITE_FLAGS: u32 =
    FnAttrs::NORETURN | FnAttrs::NOUNWIND | FnAttrs::RETURNS_TWICE | FnAttrs::CONVERGENT;

/// Sets `desc`'s flags from the call site's function attributes `flags`
/// (any [`FnAttrs`] flags; those outside [`CALL_SITE_FLAGS`] are
/// ignored).
#[inline]
pub fn set_call_site_flags(desc: &mut CallDesc<'_>, flags: u32) {
    desc.noreturn = flags & FnAttrs::NORETURN != 0;
    desc.nounwind = flags & FnAttrs::NOUNWIND != 0;
    desc.returns_twice = flags & FnAttrs::RETURNS_TWICE != 0;
    desc.convergent = flags & FnAttrs::CONVERGENT != 0;
}

/// What an attribute does in the place it was written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttrUse {
    /// Kept in `hod_ir`.
    Kept,
    /// Dropped: no object byte depends on it.
    Dropped,
    /// Unsupported: the producer records [`attr_construct`] of its name.
    Rejected,
    /// A kept attribute in a place it does not apply to: malformed.
    Misplaced,
}

impl AttrUse {
    /// What an attribute of `class` that this place does not keep does:
    /// a kept one does not apply here.
    #[inline]
    pub fn of_class(class: AttrClass) -> AttrUse {
        match class {
            AttrClass::Dropped => AttrUse::Dropped,
            AttrClass::Kept => AttrUse::Misplaced,
            AttrClass::Rejected => AttrUse::Rejected,
        }
    }
}

/// The construct an unsupported attribute is recorded by: `attr:<name>`.
pub fn attr_construct(name: &[u8]) -> String {
    format!("attr:{}", String::from_utf8_lossy(name))
}

/// A string attribute `"name"="value"`: on a function (`fa` is its set),
/// a kept string goes into `fa` (for `no-builtin-<f>`, the name's rest);
/// on a value, or unknown, it is rejected; a dropped one is dropped.
///
/// # Errors
/// As [`FnAttrs::set_string`].
#[inline]
pub fn string_attr(fa: Option<&mut FnAttrs>, name: &[u8], value: &[u8]) -> IrResult<AttrUse> {
    match (attr_info(AttrSyntax::String, name).map(|row| row.class), fa) {
        (Some(AttrClass::Dropped), _) => Ok(AttrUse::Dropped),
        (Some(AttrClass::Kept), Some(fa)) => {
            if let Some((key, rest)) = FnStr::from_name(name) {
                let value = if key == FnStr::NoBuiltin { rest } else { value };
                fa.set_string(key, value)?;
            }
            Ok(AttrUse::Kept)
        }
        _ => Ok(AttrUse::Rejected),
    }
}

/// A function keyword attribute other than `uwtable`, `alignstack` and
/// `align` (whose arguments the producer reads): a flag of [`FnAttrs`] is
/// set in `fa`; any other is used as its class says ([`AttrUse::of_class`]).
///
/// # Errors
/// As [`FnAttrs::set`].
#[inline]
pub fn fn_keyword(fa: &mut FnAttrs, name: &[u8], class: AttrClass) -> IrResult<AttrUse> {
    if let Some(flag) = FnAttrs::flag_named(name) {
        fa.set(flag)?;
        return Ok(AttrUse::Kept);
    }
    Ok(AttrUse::of_class(class))
}

/// What a keyword attribute of a parameter or return value is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueKeyword {
    /// A [`ParamAttrs`] flag (`zeroext`, `signext`, `inreg`, `nest`,
    /// `swiftself`, `swifterror`, `swiftasync`).
    Flag(u16),
    /// A typed attribute (`sret`, `byval`, `byref`, `inalloca`,
    /// `preallocated`, `elementtype`): [`add_typed`].
    Typed(ParamMem),
    /// `align N`.
    Align,
    /// `alignstack(N)`.
    StackAlign,
    /// Any other: used as its class says ([`AttrUse::of_class`]).
    Other,
}

/// The keyword attribute `name` of a parameter or return value.
#[allow(clippy::inline_always)] // Per instruction; LLVM declined `#[inline]`.
#[inline(always)]
pub fn value_keyword(name: &[u8]) -> ValueKeyword {
    match name {
        b"zeroext" => ValueKeyword::Flag(ParamAttrs::ZEROEXT),
        b"signext" => ValueKeyword::Flag(ParamAttrs::SIGNEXT),
        b"inreg" => ValueKeyword::Flag(ParamAttrs::INREG),
        b"nest" => ValueKeyword::Flag(ParamAttrs::NEST),
        b"swiftself" => ValueKeyword::Flag(ParamAttrs::SWIFT_SELF),
        b"swifterror" => ValueKeyword::Flag(ParamAttrs::SWIFT_ERROR),
        b"swiftasync" => ValueKeyword::Flag(ParamAttrs::SWIFT_ASYNC),
        b"sret" => ValueKeyword::Typed(ParamMem::Sret),
        b"byval" => ValueKeyword::Typed(ParamMem::ByVal),
        b"byref" => ValueKeyword::Typed(ParamMem::ByRef),
        b"inalloca" => ValueKeyword::Typed(ParamMem::InAlloca),
        b"preallocated" => ValueKeyword::Typed(ParamMem::Preallocated),
        b"elementtype" => ValueKeyword::Typed(ParamMem::ElementType),
        b"align" => ValueKeyword::Align,
        b"alignstack" => ValueKeyword::StackAlign,
        _ => ValueKeyword::Other,
    }
}

/// Why a typed attribute could not be added.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypedAttrError {
    /// Two typed attributes on one value, neither `elementtype`.
    TwoTyped,
    /// As [`ParamAttrs::with_mem`].
    Ir(crate::IrError),
}

impl From<crate::IrError> for TypedAttrError {
    fn from(error: crate::IrError) -> TypedAttrError {
        TypedAttrError::Ir(error)
    }
}

/// `attrs` with the typed attribute `mem` of type `ty`. `elementtype`
/// beside another typed attribute has no representation: the other one
/// is kept, and the construct to record is returned
/// (`attr:elementtype-with-<other>`).
///
/// # Errors
/// `TwoTyped` for two typed attributes neither of which is
/// `elementtype`.
#[inline]
pub fn add_typed(
    attrs: ParamAttrs,
    mem: ParamMem,
    ty: TypeId,
) -> Result<(ParamAttrs, Option<&'static str>), TypedAttrError> {
    if attrs.mem() == ParamMem::None {
        return Ok((attrs.with_mem(mem, ty)?, None));
    }
    if mem != ParamMem::ElementType && attrs.mem() != ParamMem::ElementType {
        return Err(TypedAttrError::TwoTyped);
    }
    let other = if mem == ParamMem::ElementType { attrs.mem() } else { mem };
    let construct = match other {
        ParamMem::Sret => "attr:elementtype-with-sret",
        ParamMem::ByVal => "attr:elementtype-with-byval",
        ParamMem::ByRef => "attr:elementtype-with-byref",
        ParamMem::InAlloca => "attr:elementtype-with-inalloca",
        ParamMem::Preallocated => "attr:elementtype-with-preallocated",
        _ => "attr:elementtype-with-elementtype",
    };
    let attrs = if attrs.mem() == ParamMem::ElementType {
        clear_mem(attrs).with_mem(mem, ty)?
    } else {
        attrs
    };
    Ok((attrs, Some(construct)))
}

/// `attrs` without its typed attribute.
fn clear_mem(attrs: ParamAttrs) -> ParamAttrs {
    let mut out = ParamAttrs::NONE;
    for flag in [
        ParamAttrs::ZEROEXT,
        ParamAttrs::SIGNEXT,
        ParamAttrs::INREG,
        ParamAttrs::NEST,
        ParamAttrs::SWIFT_SELF,
        ParamAttrs::SWIFT_ERROR,
        ParamAttrs::SWIFT_ASYNC,
    ] {
        if attrs.has(flag) {
            out = out.with(flag).unwrap_or(out);
        }
    }
    if let Some(bytes) = attrs.align() {
        out = out.with_align(bytes).unwrap_or(out);
    }
    if let Some(bytes) = attrs.stack_align() {
        out = out.with_stack_align(bytes).unwrap_or(out);
    }
    out
}

/// Adds the attributes of `view` (an interned set) to `out`: its flags,
/// its unwind table kind unless none, its stack alignment and its
/// strings. Two attribute groups on one function merge this way.
///
/// # Errors
/// As the setters of [`FnAttrs`].
pub fn merge_fn_attrs<L: Layering>(out: &mut FnAttrs, view: &FnAttrsView<'_, L>) -> IrResult<()> {
    out.set(view.flags())?;
    if view.uwtable() != UwTable::None {
        out.set_uwtable(view.uwtable());
    }
    if let Some(bytes) = view.stack_align() {
        out.set_stack_align(bytes)?;
    }
    for (key, value) in view.strings() {
        out.set_string(key, value)?;
    }
    Ok(())
}

// ---- special globals and global attachments (`spec/ir.md` §2.2) ----

/// The arrays LLVM names `llvm.*` that are not symbols.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpecialGlobal {
    /// `llvm.global_ctors`.
    Ctors,
    /// `llvm.global_dtors`.
    Dtors,
    /// `llvm.used`.
    Used,
    /// `llvm.compiler.used`.
    CompilerUsed,
    /// `llvm.global.annotations`.
    Annotations,
}

impl SpecialGlobal {
    /// The special array named `name` (without `@`), if it is one.
    pub fn from_name(name: &[u8]) -> Option<SpecialGlobal> {
        match name {
            b"llvm.global_ctors" => Some(SpecialGlobal::Ctors),
            b"llvm.global_dtors" => Some(SpecialGlobal::Dtors),
            b"llvm.used" => Some(SpecialGlobal::Used),
            b"llvm.compiler.used" => Some(SpecialGlobal::CompilerUsed),
            b"llvm.global.annotations" => Some(SpecialGlobal::Annotations),
            _ => None,
        }
    }

    /// A bit of its own, for a producer that refuses a second definition.
    pub fn bit(self) -> u8 {
        1 << self as u8
    }
}

/// Why a special array is malformed (LLVM's verifier refuses it). Every
/// one is a type error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpecialError {
    /// The initializer is not an array (nor zero, `undef` or `poison`).
    NotAnArray,
    /// A member is not a symbol's address (nor null).
    NotASymbol,
    /// A member of `llvm.used` or `llvm.compiler.used` is null.
    NullUsed,
    /// A structor entry is not `{ i32, ptr, ptr }` (or `{ i32, ptr }`).
    BadEntry,
    /// A structor's priority is not an integer constant.
    BadPriority,
}

impl SpecialError {
    /// The message for the error.
    pub fn message(self) -> &'static str {
        match self {
            SpecialError::NotAnArray => "this special array is not an array of its members",
            SpecialError::NotASymbol => "a member of this special array is not a symbol",
            SpecialError::NullUsed => "a used symbol cannot be null",
            SpecialError::BadEntry => "an entry is { i32 priority, ptr function, ptr data }",
            SpecialError::BadPriority => "a priority is an integer constant",
        }
    }
}

/// The construct to record for `llvm.global.annotations` in `section`:
/// `llc` emits nothing of it in `llvm.metadata`, anything else is
/// unsupported.
pub fn annotations_construct(section: Option<&[u8]>) -> Option<&'static str> {
    (section != Some(&b"llvm.metadata"[..])).then_some("special-global:llvm.global.annotations")
}

/// Delivers the special array `which` with the initializer `init` (an
/// array of addresses, or of structor entries) into `module`: the members
/// of `llvm.used` and `llvm.compiler.used` are retained
/// (`Module::retain`), each structor entry is added
/// (`Module::add_ctor`/`add_dtor`) unless it is all zeros or its function
/// is null, and `llvm.global.annotations` is ignored (see
/// [`annotations_construct`]). A zero, `undef` or `poison` array has no
/// members.
///
/// # Errors
/// A [`SpecialError`] for a malformed array, else
/// `Module::retain`'s error.
pub fn deliver_special<L: Layering>(
    module: &mut Module<L>,
    which: SpecialGlobal,
    init: ConstId,
) -> Result<(), SpecialDeliveryError> {
    let elems: Vec<ConstId> = match module.consts.get(init) {
        Const::Aggregate { elems, .. } => elems.to_vec(),
        Const::Zero(_) | Const::Undef(_) | Const::Poison(_) => Vec::new(),
        _ => return Err(SpecialError::NotAnArray.into()),
    };
    let symbol_of = |module: &Module<L>, id: ConstId| match module.consts.get(id) {
        Const::Addr { sym, addend: 0 } => Ok(Some(sym)),
        Const::Null(_) => Ok(None),
        _ => Err(SpecialError::NotASymbol),
    };
    for elem in elems {
        match which {
            SpecialGlobal::Used | SpecialGlobal::CompilerUsed => {
                let Some(sym) = symbol_of(module, elem)? else {
                    return Err(SpecialError::NullUsed.into());
                };
                let how = if which == SpecialGlobal::Used {
                    Retained::Used
                } else {
                    Retained::CompilerUsed
                };
                module.retain(sym, how)?;
            }
            SpecialGlobal::Annotations => {}
            SpecialGlobal::Ctors | SpecialGlobal::Dtors => {
                let fields = match module.consts.get(elem) {
                    Const::Aggregate { elems, .. } if matches!(elems.len(), 2 | 3) => {
                        elems.to_vec()
                    }
                    // An entry that is all zeros has a null function.
                    Const::Zero(_) => continue,
                    _ => return Err(SpecialError::BadEntry.into()),
                };
                let Some(priority) = module.consts.int_value(fields[0]) else {
                    return Err(SpecialError::BadPriority.into());
                };
                // An entry whose function is null is skipped.
                let Some(func) = symbol_of(module, fields[1])? else {
                    continue;
                };
                let associated = match fields.get(2) {
                    Some(&data) => symbol_of(module, data)?,
                    None => None,
                };
                let priority = priority as u32;
                if which == SpecialGlobal::Ctors {
                    module.add_ctor(priority, func, associated);
                } else {
                    module.add_dtor(priority, func, associated);
                }
            }
        }
    }
    Ok(())
}

/// Why [`deliver_special`] failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpecialDeliveryError {
    /// The array is malformed.
    Malformed(SpecialError),
    /// The module refused.
    Ir(crate::IrError),
}

impl From<SpecialError> for SpecialDeliveryError {
    fn from(error: SpecialError) -> SpecialDeliveryError {
        SpecialDeliveryError::Malformed(error)
    }
}

impl From<crate::IrError> for SpecialDeliveryError {
    fn from(error: crate::IrError) -> SpecialDeliveryError {
        SpecialDeliveryError::Ir(error)
    }
}

/// What a metadata attachment `!kind` on a global value is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlobalAttachment {
    /// `!dbg`: the debug information of the symbol.
    Debug,
    /// Dropped: `!type`, `!vcall_visibility`, `!prof`, `!section_prefix`,
    /// `!annotation`.
    Dropped,
    /// Any other: unsupported, recorded as [`global_attachment_construct`].
    Unsupported,
}

impl GlobalAttachment {
    /// The attachment of kind `kind` (without `!`).
    #[inline]
    pub fn of(kind: &[u8]) -> GlobalAttachment {
        match kind {
            b"dbg" => GlobalAttachment::Debug,
            b"type" | b"vcall_visibility" | b"prof" | b"section_prefix" | b"annotation" => {
                GlobalAttachment::Dropped
            }
            _ => GlobalAttachment::Unsupported,
        }
    }
}

/// The construct an unsupported attachment is recorded by:
/// `global-metadata:<kind>`.
pub fn global_attachment_construct(kind: &[u8]) -> String {
    format!("global-metadata:{}", String::from_utf8_lossy(kind))
}

// ---- named metadata and module flags (`spec/ir.md` §2.1) ----

/// What a named metadata node of the module is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NamedMetadataKind {
    /// `!llvm.module.flags`: each operand is a flag, [`module_flag`].
    ModuleFlags,
    /// `!llvm.linker.options`: each operand is a tuple of strings,
    /// `Module::add_linker_option`.
    LinkerOptions,
    /// `!llvm.dependent-libraries`: each operand is a tuple of one
    /// string, `Module::add_dependent_library`.
    DependentLibraries,
    /// `!llvm.dbg.cu`: the compile units (debug information, §6).
    DebugCompileUnits,
    /// `!llvm.ident`, `!llvm.errno.tbaa`: dropped.
    Dropped,
    /// Any other: unsupported, recorded (against no symbol) as
    /// [`named_metadata_construct`].
    Unsupported,
}

impl NamedMetadataKind {
    /// The named metadata `name` (without `!`).
    pub fn of(name: &[u8]) -> NamedMetadataKind {
        match name {
            b"llvm.module.flags" => NamedMetadataKind::ModuleFlags,
            b"llvm.linker.options" => NamedMetadataKind::LinkerOptions,
            b"llvm.dependent-libraries" => NamedMetadataKind::DependentLibraries,
            b"llvm.dbg.cu" => NamedMetadataKind::DebugCompileUnits,
            b"llvm.ident" | b"llvm.errno.tbaa" => NamedMetadataKind::Dropped,
            _ => NamedMetadataKind::Unsupported,
        }
    }
}

/// The construct unsupported named metadata is recorded by:
/// `named-metadata:<name>`.
pub fn named_metadata_construct(name: &[u8]) -> String {
    format!("named-metadata:{}", String::from_utf8_lossy(name))
}

/// What LLVM's reader does to the branch-protection attributes of the
/// definitions of an AArch64 module whose branch-protection module flags
/// are not in the current form (`spec/ll.md` §7.3): the flags read so far.
/// Start with `Some(BranchUpgrade::default())`, feed every integer module
/// flag to [`BranchUpgrade::observe`], and apply the result to each
/// definition's attribute set with [`BranchUpgrade::apply`] (AArch64 only).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)] // One for each of six module flags.
pub struct BranchUpgrade {
    /// `branch-target-enforcement`.
    pub bte: bool,
    /// `branch-protection-pauth-lr`.
    pub pauth_lr: bool,
    /// `guarded-control-stack`.
    pub gcs: bool,
    /// `sign-return-address`.
    pub sign: bool,
    /// `sign-return-address-all`.
    pub sign_all: bool,
    /// `sign-return-address-with-bkey`.
    pub sign_b_key: bool,
}

impl BranchUpgrade {
    /// Notes the integer module flag `name` = `value`: a branch-protection
    /// flag sets its field (value 1); the value 2 (the current form) ends
    /// the upgrade (`*upgrade` becomes `None`).
    #[inline]
    pub fn observe(upgrade: &mut Option<BranchUpgrade>, name: &[u8], value: u64) {
        let field = upgrade.as_mut().and_then(|up| match name {
            b"branch-target-enforcement" => Some(&mut up.bte),
            b"branch-protection-pauth-lr" => Some(&mut up.pauth_lr),
            b"guarded-control-stack" => Some(&mut up.gcs),
            b"sign-return-address" => Some(&mut up.sign),
            b"sign-return-address-all" => Some(&mut up.sign_all),
            b"sign-return-address-with-bkey" => Some(&mut up.sign_b_key),
            _ => None,
        });
        if let Some(field) = field {
            if value == 2 {
                *upgrade = None;
            } else {
                *field = value == 1;
            }
        }
    }

    /// The attribute set LLVM's reader turns `id` into on a definition;
    /// `cache` remembers the sets already upgraded (one entry per set).
    ///
    /// # Errors
    /// As the setters of [`FnAttrs`] and `Module::intern_fn_attrs`.
    pub fn apply<L: Layering>(
        self,
        module: &mut Module<L>,
        id: FnAttrsId,
        cache: &mut Vec<(FnAttrsId, FnAttrsId)>,
    ) -> IrResult<FnAttrsId> {
        if let Some(&(_, done)) = cache.iter().find(|(from, _)| *from == id) {
            return Ok(done);
        }
        let view = module.fn_attrs(id);
        let mut out = FnAttrs::new();
        out.set(view.flags())?;
        out.set_uwtable(view.uwtable());
        if let Some(bytes) = view.stack_align() {
            out.set_stack_align(bytes)?;
        }
        let drop_sign = !self.sign && view.string(FnStr::SignReturnAddress) == Some(b"none");
        for (key, value) in view.strings() {
            let value = match key {
                FnStr::SignReturnAddress | FnStr::SignReturnAddressKey if drop_sign => continue,
                FnStr::BranchTargetEnforcement
                | FnStr::BranchProtectionPauthLr
                | FnStr::GuardedControlStack => {
                    if value.eq_ignore_ascii_case(b"false") {
                        continue;
                    }
                    if value.eq_ignore_ascii_case(b"true") { b"" } else { value }
                }
                _ => value,
            };
            out.set_string(key, value)?;
        }
        let mut default = |key: FnStr, wanted: bool, value: &[u8]| {
            if wanted && view.string(key).is_none() {
                return out.set_string(key, value);
            }
            Ok(())
        };
        let kind: &[u8] = if self.sign_all { b"all" } else { b"non-leaf" };
        let sign_key: &[u8] = if self.sign_b_key { b"b_key" } else { b"a_key" };
        default(FnStr::SignReturnAddress, self.sign, kind)?;
        default(FnStr::SignReturnAddressKey, self.sign, sign_key)?;
        default(FnStr::BranchTargetEnforcement, self.bte, b"")?;
        default(FnStr::BranchProtectionPauthLr, self.pauth_lr, b"")?;
        default(FnStr::GuardedControlStack, self.gcs, b"")?;
        let done = module.intern_fn_attrs(&out)?;
        cache.push((id, done));
        Ok(done)
    }
}

/// One module flag `!{i32 behavior, !"name", value}` into `module`: the
/// value is an integer when it is an integer constant (`int`), else a
/// string when it is one (`text`), else "other" (a node, `null`, or any
/// other constant); a behavior over `u32::MAX` is `u32::MAX` (refused by
/// `set_module_flag`). An integer flag is also fed to `upgrade`
/// ([`BranchUpgrade::observe`]).
///
/// # Errors
/// As `Module::set_module_flag`.
pub fn module_flag<L: Layering>(
    module: &mut Module<L>,
    behavior: u64,
    name: &[u8],
    int: Option<u64>,
    text: Option<&[u8]>,
    upgrade: &mut Option<BranchUpgrade>,
) -> IrResult<()> {
    let value = match (int, text) {
        (Some(value), _) => FlagValue::Int(value),
        (None, Some(text)) => FlagValue::Str(text),
        (None, None) => FlagValue::Other,
    };
    let behavior = u32::try_from(behavior).unwrap_or(u32::MAX);
    module.set_module_flag(behavior, name, value)?;
    if let Some(value) = int {
        BranchUpgrade::observe(upgrade, name, value);
    }
    Ok(())
}

// ---- debug fields (`spec/ir.md` §6) ----

/// The construct recorded when debug metadata breaks a rule LLVM's
/// verifier checks: LLVM strips the module's debug information then (with
/// a warning), and hod refuses the module by this name instead.
pub const DEBUG_INVALID: &str = "debug:invalid";

/// The node `kind` with the fields `fields` set over LLVM's defaults
/// (`DiNode::llvm_default`); a kind `hod_ir` does not represent (`None`)
/// is `DiNode::Other` named `name`.
///
/// # Errors
/// As `DiNode::set`.
#[inline]
pub fn di_node<'s>(
    kind: Option<DiKind>,
    name: &'s [u8],
    fields: impl IntoIterator<Item = (&'static str, DiFieldValue<'s>)>,
) -> IrResult<DiNode<'s>> {
    let Some(kind) = kind else {
        return Ok(DiNode::Other(DiOther { kind: name }));
    };
    let mut node = DiNode::llvm_default(kind);
    for (field, value) in fields {
        node.set(field.as_bytes(), value)?;
    }
    Ok(node)
}

/// Defines the reserved node `id` as `node`, distinct or uniqued.
///
/// # Errors
/// As `DebugInfo::define` and `define_distinct`.
#[inline]
pub fn define_di_node<L: Layering>(
    debug: &mut DebugInfo<L>,
    id: DiId,
    node: &DiNode<'_>,
    distinct: bool,
) -> IrResult<()> {
    if distinct { debug.define_distinct(id, node) } else { debug.define(id, node) }
}

/// Defines the reserved node `id` as the tuple of `elements`
/// (`DiId::NONE` for `null`).
///
/// # Errors
/// As `DebugInfo::define`.
#[inline]
pub fn define_di_tuple<L: Layering>(
    debug: &mut DebugInfo<L>,
    id: DiId,
    elements: &[DiId],
) -> IrResult<()> {
    debug.define(id, &DiNode::Tuple(DiTuple { elements }))
}

/// What stands for a reference LLVM's verifier refuses (a location, an
/// expression or an assign id where a node is named, or a value of a
/// function): the empty tuple. The producer records [`DEBUG_INVALID`].
///
/// # Errors
/// As `DebugInfo::add`.
#[inline]
pub fn invalid_di_ref<L: Layering>(debug: &mut DebugInfo<L>) -> IrResult<DiId> {
    debug.add(&DiNode::Tuple(DiTuple { elements: &[] }))
}

/// The construct a field of `kind` that `hod_ir` cannot represent is
/// recorded by: the mapping's (`di_field`), else `debug:<kind>.<field>`.
pub fn unsupported_field_construct(kind: &[u8], field: &[u8]) -> Cow<'static, str> {
    match di_field(kind, field) {
        Some(DiMapping::Unsupported(construct)) => Cow::Borrowed(construct),
        _ => Cow::Owned(format!(
            "debug:{}.{}",
            String::from_utf8_lossy(kind),
            String::from_utf8_lossy(field)
        )),
    }
}

/// A field of `kind` that `hod_ir` cannot represent: recorded (against no
/// symbol) unless its value is the default (`0`, `false`, `null`, `""`,
/// `!{}`, or the field's own default), which LLVM writes as "no field".
pub fn unsupported_field<L: Layering>(
    module: &mut Module<L>,
    kind: &[u8],
    field: &[u8],
    default: bool,
) {
    if !default {
        module.record_unsupported(&unsupported_field_construct(kind, field), None);
    }
}

/// The key-instruction atom of a location (WP-149, `spec/dwarf.md` §5.9)
/// from its `atomGroup` and `atomRank`: `None` for a rank over 8 bits,
/// which LLVM's reader refuses (a group or rank over what LLVM stores is
/// `hod_ir`'s refusal, when the location is defined).
#[inline]
pub fn di_atom(group: u64, rank: u64) -> Option<DiAtom> {
    u8::try_from(rank).ok().map(|rank| DiAtom { group, rank })
}

/// A location written in place: added, distinct or uniqued.
///
/// # Errors
/// As `DebugInfo::loc_in_atom` and `distinct_loc_in_atom`.
#[inline]
pub fn add_location<L: Layering>(
    debug: &mut DebugInfo<L>,
    loc: DiLocation,
    atom: DiAtom,
    distinct: bool,
) -> IrResult<LocId> {
    if distinct { debug.distinct_loc_in_atom(loc, atom) } else { debug.loc_in_atom(loc, atom) }
}

/// One operand of `!llvm.dbg.cu`: listed when it is a compile unit
/// (true); anything else is not (false), and the producer records
/// [`DEBUG_INVALID`].
///
/// # Errors
/// As `DebugInfo::add_compile_unit`.
#[inline]
pub fn compile_unit<L: Layering>(module: &mut Module<L>, unit: Option<DiId>) -> IrResult<bool> {
    match unit {
        Some(unit) if module.debug.find_kind(unit) == Some(DiKind::CompileUnit) => {
            module.debug.add_compile_unit(unit)?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// One node a function's `!dbg` names: the function's subprogram when it
/// is one; anything else is `None`, and the producer records
/// [`DEBUG_INVALID`]. Of several, the last subprogram is the function's.
#[inline]
pub fn function_subprogram<L: Layering>(module: &Module<L>, node: Option<DiId>) -> Option<DiId> {
    node.filter(|&node| module.debug.find_kind(node) == Some(DiKind::Subprogram))
}

/// One node a variable's `!dbg` names: attached to `sym` when it is a
/// global-variable expression (true); anything else is not (false), and
/// the producer records [`DEBUG_INVALID`].
///
/// # Errors
/// As `Module::attach_global_expr`.
#[inline]
pub fn variable_expression<L: Layering>(
    module: &mut Module<L>,
    sym: SymId,
    node: Option<DiId>,
) -> IrResult<bool> {
    match node {
        Some(node) if module.debug.find_kind(node) == Some(DiKind::GlobalVariableExpression) => {
            module.attach_global_expr(sym, node)?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// After the module's items: the debug tables completed
/// (`Module::finish_debug`) and, when the module uses them (`listed_units`:
/// it has `!llvm.dbg.cu`; or a variable or a function names its debug
/// information), checked as LLVM's verifier checks them. A failure, or
/// debug information without a `Debug Info Version` of 3 (which LLVM
/// drops), is recorded as [`DEBUG_INVALID`]. Nothing for a module without
/// debug information.
///
/// # Errors
/// As `Module::finish_debug`.
pub fn finish_debug<L: Layering>(module: &mut Module<L>, listed_units: bool) -> IrResult<()> {
    if module.debug.is_empty() && !listed_units {
        return Ok(());
    }
    module.finish_debug()?;
    let used = listed_units
        || !module.global_exprs().is_empty()
        || module.symbols().any(|symbol| {
            matches!(symbol.kind(), SymbolKind::Function { subprogram: Some(_), .. })
        });
    // LLVM's verifier checks only what is used, and so does this.
    if used && (module.props.debug_info_version != Some(3) || module.verify_debug().is_err()) {
        module.record_unsupported(DEBUG_INVALID, None);
    }
    Ok(())
}

/// The four debug records (`#dbg_*`, `llvm.dbg.*`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DebugRecordKind {
    /// `#dbg_declare`, `llvm.dbg.declare`.
    Declare,
    /// `#dbg_value`, `llvm.dbg.value`.
    Value,
    /// `#dbg_assign`, `llvm.dbg.assign`.
    Assign,
    /// `#dbg_label`, `llvm.dbg.label`.
    Label,
}

impl DebugRecordKind {
    /// The record a call of the intrinsic `name` stands for. Any other
    /// `llvm.dbg.*` is recorded as `debug:<name>`
    /// ([`debug_intrinsic_construct`]).
    #[inline]
    pub fn of_intrinsic(name: &[u8]) -> Option<DebugRecordKind> {
        match name {
            b"llvm.dbg.declare" => Some(DebugRecordKind::Declare),
            b"llvm.dbg.value" => Some(DebugRecordKind::Value),
            b"llvm.dbg.assign" => Some(DebugRecordKind::Assign),
            b"llvm.dbg.label" => Some(DebugRecordKind::Label),
            _ => None,
        }
    }

    /// The record named `name` (`#dbg_declare`, …). Any other is
    /// [`unsupported_record_construct`] or malformed.
    #[inline]
    pub fn of_record(name: &[u8]) -> Option<DebugRecordKind> {
        match name {
            b"#dbg_declare" => Some(DebugRecordKind::Declare),
            b"#dbg_value" => Some(DebugRecordKind::Value),
            b"#dbg_assign" => Some(DebugRecordKind::Assign),
            b"#dbg_label" => Some(DebugRecordKind::Label),
            _ => None,
        }
    }

    /// The kind of node the record's variable (or label) is.
    #[inline]
    pub fn variable_kind(self) -> DiKind {
        if self == DebugRecordKind::Label { DiKind::Label } else { DiKind::LocalVariable }
    }
}

/// The construct a call of an `llvm.dbg.*` intrinsic that is no record is
/// recorded by: `debug:<name>`.
pub fn debug_intrinsic_construct(name: &[u8]) -> String {
    format!("debug:{}", String::from_utf8_lossy(name))
}

/// The construct a debug record `hod_ir` does not keep is recorded by
/// (`di_record`), if it is one LLVM knows.
#[inline]
pub fn unsupported_record_construct(name: &[u8]) -> Option<&'static str> {
    match di_record(name) {
        Some(DiMapping::Unsupported(construct)) => Some(construct),
        _ => None,
    }
}

/// A debug record, read: everything but its value(s) and its location.
#[derive(Clone, Copy, Debug)]
pub struct DebugRecord {
    /// The kind.
    pub kind: DebugRecordKind,
    /// The variable (the label of a label record); `DiId::NONE` when it
    /// is not one (the producer has recorded [`DEBUG_INVALID`]).
    pub var: DiId,
    /// The expression.
    pub expr: ExprId,
    /// Whether the value is a `DIArgList` (several values).
    pub list: bool,
    /// An assignment's address.
    pub address: Value,
    /// An assignment's address expression.
    pub address_expr: ExprId,
}

impl DebugRecord {
    /// A record of `kind` with nothing read yet.
    #[inline]
    pub fn new(kind: DebugRecordKind) -> DebugRecord {
        DebugRecord {
            kind,
            var: DiId::NONE,
            expr: ExprId::EMPTY,
            list: false,
            address: Value::NONE,
            address_expr: ExprId::EMPTY,
        }
    }
}

/// Hands `record` with its value(s) `values` (the first is the value; a
/// `DIArgList`'s are all of them; none is `Value::NONE`) at `loc` to the
/// builder. A `declare` of a `DIArgList` is not made (false): the
/// producer records [`DEBUG_INVALID`].
///
/// # Errors
/// As `FunctionBuilder::dbg_declare` and the other record builders.
#[inline]
pub fn emit_debug_record(
    b: &mut FunctionBuilder,
    record: &DebugRecord,
    values: &[Value],
    loc: LocId,
) -> IrResult<bool> {
    let value = values.first().copied().unwrap_or(Value::NONE);
    match record.kind {
        DebugRecordKind::Declare if record.list => return Ok(false),
        DebugRecordKind::Declare => b.dbg_declare(value, record.var, record.expr, loc)?,
        DebugRecordKind::Value if record.list => {
            b.dbg_value_list(values, record.var, record.expr, loc)?;
        }
        DebugRecordKind::Value => b.dbg_value(value, record.var, record.expr, loc)?,
        DebugRecordKind::Assign => {
            b.dbg_assign(value, record.var, record.expr, record.address, record.address_expr, loc)?;
        }
        DebugRecordKind::Label => b.dbg_label(record.var, loc)?,
    }
    Ok(true)
}

/// The construct an enumerator whose value does not fit 64 bits is
/// recorded by (its value is read as 0).
pub const WIDE_ENUMERATOR: &str = "debug:DIEnumerator.wide";

/// The construct an assignment whose value or address is a `DIArgList`
/// is recorded by.
pub const ASSIGN_ARG_LIST: &str = "debug:#dbg_assign.DIArgList";
