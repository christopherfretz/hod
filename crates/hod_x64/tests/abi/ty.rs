//! Types as the tests write them: LLVM text, the `hod_ir` type, and the
//! in-memory image of a value (`spec/ir.md` §4.3) with the bits that carry
//! it.

use std::fmt::Write as _;

use hod_ir::{TypeId, TypeTable};

/// A first-class LLVM type.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Ty {
    Int(u32),
    Ptr,
    Half,
    BFloat,
    Float,
    Double,
    Fp128,
    X86Fp80,
    Vec(Box<Ty>, u32),
    Arr(Box<Ty>, u64),
    Struct(Vec<Ty>),
    Packed(Vec<Ty>),
}

pub const I1: Ty = Ty::Int(1);
pub const I8: Ty = Ty::Int(8);
pub const I16: Ty = Ty::Int(16);
pub const I32: Ty = Ty::Int(32);
pub const I64: Ty = Ty::Int(64);
pub const I128: Ty = Ty::Int(128);

pub fn vec(elem: Ty, len: u32) -> Ty {
    Ty::Vec(Box::new(elem), len)
}

pub fn arr(elem: Ty, len: u64) -> Ty {
    Ty::Arr(Box::new(elem), len)
}

pub fn st(fields: &[Ty]) -> Ty {
    Ty::Struct(fields.to_vec())
}

impl Ty {
    /// The type in LLVM's syntax.
    pub fn text(&self) -> String {
        let mut text = String::new();
        self.write(&mut text);
        text
    }

    fn write(&self, out: &mut String) {
        match self {
            Ty::Int(bits) => {
                let _ = write!(out, "i{bits}");
            }
            Ty::Ptr => out.push_str("ptr"),
            Ty::Half => out.push_str("half"),
            Ty::BFloat => out.push_str("bfloat"),
            Ty::Float => out.push_str("float"),
            Ty::Double => out.push_str("double"),
            Ty::Fp128 => out.push_str("fp128"),
            Ty::X86Fp80 => out.push_str("x86_fp80"),
            Ty::Vec(elem, len) => {
                let _ = write!(out, "<{len} x ");
                elem.write(out);
                out.push('>');
            }
            Ty::Arr(elem, len) => {
                let _ = write!(out, "[{len} x ");
                elem.write(out);
                out.push(']');
            }
            Ty::Struct(fields) | Ty::Packed(fields) => {
                let packed = matches!(self, Ty::Packed(_));
                out.push_str(if packed { "<{" } else { "{" });
                for (index, field) in fields.iter().enumerate() {
                    out.push_str(if index == 0 { " " } else { ", " });
                    field.write(out);
                }
                out.push_str(if packed { " }>" } else { " }" });
            }
        }
    }

    /// The type in `types`.
    pub fn intern(&self, types: &mut TypeTable) -> TypeId {
        let made = match self {
            Ty::Int(bits) => types.int(*bits),
            Ty::Ptr => Ok(TypeId::PTR),
            Ty::Half => Ok(TypeId::HALF),
            Ty::BFloat => Ok(TypeId::BFLOAT),
            Ty::Float => Ok(TypeId::FLOAT),
            Ty::Double => Ok(TypeId::DOUBLE),
            Ty::Fp128 => Ok(TypeId::FP128),
            Ty::X86Fp80 => Ok(TypeId::X86_FP80),
            Ty::Vec(elem, len) => {
                let elem = elem.intern(types);
                types.vector(elem, *len)
            }
            Ty::Arr(elem, len) => {
                let elem = elem.intern(types);
                types.array(elem, *len)
            }
            Ty::Struct(fields) | Ty::Packed(fields) => {
                let fields: Vec<TypeId> = fields.iter().map(|f| f.intern(types)).collect();
                types.struct_type(&fields, matches!(self, Ty::Packed(_)))
            }
        };
        made.unwrap_or_else(|error| panic!("{}: {error:?}", self.text()))
    }

    /// Whether the type is an integer, and its width.
    pub fn int_bits(&self) -> Option<u32> {
        if let Ty::Int(bits) = self { Some(*bits) } else { None }
    }

    /// Calls `visit` with the byte offset and the width in bits of every
    /// scalar the value is made of, in offset order. A vector is visited
    /// as one scalar of its total width: its elements are whole bytes in
    /// every type the tests build.
    pub fn scalars(&self, types: &mut TypeTable, base: u64, visit: &mut dyn FnMut(u64, u64)) {
        match self {
            Ty::Int(bits) => visit(base, u64::from(*bits)),
            Ty::Ptr | Ty::Double => visit(base, 64),
            Ty::Half | Ty::BFloat => visit(base, 16),
            Ty::Float => visit(base, 32),
            Ty::Fp128 => visit(base, 128),
            Ty::X86Fp80 => visit(base, 80),
            Ty::Vec(..) => {
                let id = self.intern(types);
                visit(base, types.size_in_bits_of(id));
            }
            Ty::Arr(elem, len) => {
                let id = elem.intern(types);
                let stride = types.alloc_size_of(id);
                for index in 0..*len {
                    elem.scalars(types, base + index * stride, visit);
                }
            }
            Ty::Struct(fields) | Ty::Packed(fields) => {
                let id = self.intern(types);
                for (index, field) in fields.iter().enumerate() {
                    let offset = types.field_offset(id, index as u32);
                    field.scalars(types, base + offset, visit);
                }
            }
        }
    }
}

impl Ty {
    /// Calls `visit` with the byte offset of every `half` (`false`) and
    /// `bfloat` (`true`) in the value, vector elements included.
    pub fn half_scalars(&self, types: &mut TypeTable, base: u64, visit: &mut dyn FnMut(u64, bool)) {
        match self {
            Ty::Half => visit(base, false),
            Ty::BFloat => visit(base, true),
            Ty::Vec(elem, len) if matches!(**elem, Ty::Half | Ty::BFloat) => {
                for index in 0..u64::from(*len) {
                    visit(base + 2 * index, matches!(**elem, Ty::BFloat));
                }
            }
            Ty::Arr(elem, len) => {
                let id = elem.intern(types);
                let stride = types.alloc_size_of(id);
                for index in 0..*len {
                    elem.half_scalars(types, base + index * stride, visit);
                }
            }
            Ty::Struct(fields) | Ty::Packed(fields) => {
                let id = self.intern(types);
                for (index, field) in fields.iter().enumerate() {
                    let offset = types.field_offset(id, index as u32);
                    field.half_scalars(types, base + offset, visit);
                }
            }
            _ => {}
        }
    }

    /// Calls `visit` with the byte offset of every `x86_fp80` in the value.
    pub fn x87_scalars(&self, types: &mut TypeTable, base: u64, visit: &mut dyn FnMut(u64)) {
        match self {
            Ty::X86Fp80 => visit(base),
            Ty::Arr(elem, len) => {
                let id = elem.intern(types);
                let stride = types.alloc_size_of(id);
                for index in 0..*len {
                    elem.x87_scalars(types, base + index * stride, visit);
                }
            }
            Ty::Struct(fields) | Ty::Packed(fields) => {
                let id = self.intern(types);
                for (index, field) in fields.iter().enumerate() {
                    let offset = types.field_offset(id, index as u32);
                    field.x87_scalars(types, base + offset, visit);
                }
            }
            _ => {}
        }
    }

    /// Calls `visit` with the byte offset and the width in bytes of every
    /// `float` and `double` scalar (not a vector element) in the value.
    pub fn fp_scalars(&self, types: &mut TypeTable, base: u64, visit: &mut dyn FnMut(u64, u64)) {
        match self {
            Ty::Float => visit(base, 4),
            Ty::Double => visit(base, 8),
            Ty::Arr(elem, len) => {
                let id = elem.intern(types);
                let stride = types.alloc_size_of(id);
                for index in 0..*len {
                    elem.fp_scalars(types, base + index * stride, visit);
                }
            }
            Ty::Struct(fields) | Ty::Packed(fields) => {
                let id = self.intern(types);
                for (index, field) in fields.iter().enumerate() {
                    let offset = types.field_offset(id, index as u32);
                    field.fp_scalars(types, base + offset, visit);
                }
            }
            _ => {}
        }
    }
}

/// A deterministic generator (a 64-bit LCG, high bits).
#[derive(Clone, Debug)]
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 =
            self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 32
    }

    /// A number below `bound`.
    pub fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }

    pub fn chance(&mut self, one_in: u64) -> bool {
        self.below(one_in) == 0
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u64) as usize]
    }

    pub fn byte(&mut self) -> u8 {
        (self.next() >> 8) as u8
    }
}

/// Bytes after an image that a test may read or write as part of a whole
/// register.
pub const SLACK: usize = 16;

/// A value of some type as bytes.
#[derive(Clone, Debug)]
pub struct Image {
    /// The alloc size of the type.
    pub size: usize,
    /// The in-memory image: `size` bytes with zero padding and zero bits
    /// above an integer's width, then `SLACK` zero bytes.
    pub clean: Vec<u8>,
    /// The bits of `clean` that carry the value.
    pub mask: Vec<u8>,
    /// `clean` with every other bit random.
    pub dirty: Vec<u8>,
    /// `clean` with every byte that carries no bit of the value random:
    /// what a `store` of the value may leave in memory (`spec/ir.md` §4.3
    /// keeps the bits above an integer's width zero; padding is anything).
    pub memory: Vec<u8>,
}

impl Image {
    /// A random value of type `ty`.
    pub fn random(types: &mut TypeTable, ty: &Ty, rng: &mut Rng) -> Image {
        let id = ty.intern(types);
        let size = usize::try_from(types.alloc_size_of(id)).expect("a small type");
        let mut mask = vec![0u8; size + SLACK];
        ty.scalars(types, 0, &mut |offset, bits| {
            for bit in 0..bits {
                mask[(offset + bit / 8) as usize] |= 1 << (bit % 8);
            }
        });
        let mut values: Vec<u8> = (0..size + SLACK).map(|_| rng.byte()).collect();
        ty.x87_scalars(types, 0, &mut |offset| {
            // A normal number: the explicit integer bit set and an
            // exponent neither 0 nor all ones, which the x87 loads and
            // stores unchanged (an unnormal it may not).
            let at = offset as usize;
            values[at + 7] |= 0x80;
            values[at + 8] = values[at + 8].max(1);
            values[at + 9] &= 0xbf;
        });
        ty.half_scalars(types, 0, &mut |offset, bfloat| {
            // No signaling NaN: LLVM may convert a `half` or `bfloat` to
            // `float` and back on the way, which quiets one. A NaN gets
            // its quiet bit (9 of a `half`, 6 of a `bfloat`).
            let at = offset as usize;
            let bits = u16::from_le_bytes([values[at], values[at + 1]]);
            let (exponent, quiet) = if bfloat { (0x7f80, 0x0040) } else { (0x7c00, 0x0200) };
            if bits & exponent == exponent && bits & !(exponent | 0x8000) != 0 {
                let [low, high] = (bits | quiet).to_le_bytes();
                values[at] = low;
                values[at + 1] = high;
            }
        });
        ty.fp_scalars(types, 0, &mut |offset, bytes| {
            // No signaling NaN either: a `float` or `double` returned in
            // `st0`/`st1` (§5.1) is loaded and stored by the x87, which
            // quiets one. A NaN gets its quiet bit (22 of a `float`, 51 of
            // a `double`): the top bit of its third or seventh byte.
            let at = offset as usize;
            let (top, quiet) = if bytes == 4 { (at + 2, 0x40) } else { (at + 6, 0x08) };
            let high = u16::from_le_bytes([values[top], values[top + 1]]) & 0x7fff;
            let exponent = if bytes == 4 { 0x7f80 } else { 0x7ff0 };
            if high & exponent == exponent {
                values[top] |= quiet;
            }
        });
        let mut clean = vec![0u8; size + SLACK];
        let mut dirty = vec![0u8; size + SLACK];
        let mut memory = vec![0u8; size + SLACK];
        for index in 0..size + SLACK {
            let (value, junk) = (values[index], rng.byte());
            clean[index] = value & mask[index];
            dirty[index] = clean[index] | (junk & !mask[index]);
            memory[index] = if mask[index] == 0 { junk } else { clean[index] };
        }
        Image { size, clean, mask, dirty, memory }
    }

    /// `size` random bytes, every bit of which carries: the pointee of a
    /// `byval` argument.
    pub fn bytes(size: usize, rng: &mut Rng) -> Image {
        let mut clean: Vec<u8> = (0..size).map(|_| rng.byte()).collect();
        clean.resize(size + SLACK, 0);
        let mut mask = vec![0xff; size];
        mask.resize(size + SLACK, 0);
        Image { size, dirty: clean.clone(), memory: clean.clone(), clean, mask }
    }

    /// The image of the integer of `bits` bits in `self`, extended to
    /// `to` bits, with a mask of `to` bits.
    pub fn extended(&self, bits: u32, signed: bool, to: u32) -> (Vec<u8>, Vec<u8>) {
        let mut value = self.clean.clone();
        let mut mask = vec![0u8; value.len()];
        let negative = value[((bits - 1) / 8) as usize] >> ((bits - 1) % 8) & 1 == 1;
        for bit in 0..to {
            let (byte, shift) = ((bit / 8) as usize, bit % 8);
            mask[byte] |= 1 << shift;
            if bit >= bits && signed && negative {
                value[byte] |= 1 << shift;
            }
        }
        (value, mask)
    }
}
