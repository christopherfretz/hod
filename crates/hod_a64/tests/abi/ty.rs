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
        let mut clean = vec![0u8; size + SLACK];
        let mut dirty = vec![0u8; size + SLACK];
        for index in 0..size + SLACK {
            let (value, junk) = (rng.byte(), rng.byte());
            clean[index] = value & mask[index];
            dirty[index] = clean[index] | (junk & !mask[index]);
        }
        Image { size, clean, mask, dirty }
    }

    /// `size` random bytes, every bit of which carries: the pointee of a
    /// `byval` argument.
    pub fn bytes(size: usize, rng: &mut Rng) -> Image {
        let mut clean: Vec<u8> = (0..size).map(|_| rng.byte()).collect();
        clean.resize(size + SLACK, 0);
        let mut mask = vec![0xff; size];
        mask.resize(size + SLACK, 0);
        Image { size, dirty: clean.clone(), clean, mask }
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
