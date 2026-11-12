//! Runtime values (`spec/interp.md` §3).

use std::fmt;

use hod_ir::TypeId;

use crate::vector::Lanes;

/// The widest integer the interpreter computes with.
pub const MAX_INT_BITS: u32 = 128;

/// The low `bits` bits set.
#[inline]
pub(crate) const fn mask(bits: u32) -> u128 {
    if bits >= 128 { u128::MAX } else { (1u128 << bits) - 1 }
}

/// `value`, an integer of `bits` bits, as a signed number.
#[inline]
pub(crate) const fn sext(bits: u32, value: u128) -> i128 {
    let shift = 128 - bits;
    ((value << shift) as i128) >> shift
}

/// A value of a first-class type the interpreter supports (§3).
///
/// Two values are equal when they are the same bits: floats compare by
/// encoding, so `+0.0` and `-0.0` differ and a NaN equals itself.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Val {
    /// `iN`, `1 <= N <= 128`. `value` has the bits above `bits` clear.
    Int {
        /// The width.
        bits: u32,
        /// The value, zero-extended.
        value: u128,
    },
    /// A pointer: an address.
    Ptr(u64),
    /// `float`, as its IEEE binary32 encoding.
    Float(u32),
    /// `double`, as its IEEE binary64 encoding.
    Double(u64),
    /// A struct, an array, or a fixed-width vector (§3.1).
    Agg(Agg),
}

impl Val {
    /// The integer of `bits` bits whose value is `value` modulo `2^bits`.
    #[inline]
    pub const fn int(bits: u32, value: u128) -> Val {
        Val::Int { bits, value: value & mask(bits) }
    }

    /// An `i1`.
    #[inline]
    pub const fn i1(value: bool) -> Val {
        Val::Int { bits: 1, value: value as u128 }
    }

    /// An `i8`.
    #[inline]
    pub const fn i8(value: u8) -> Val {
        Val::Int { bits: 8, value: value as u128 }
    }

    /// An `i16`.
    #[inline]
    pub const fn i16(value: u16) -> Val {
        Val::Int { bits: 16, value: value as u128 }
    }

    /// An `i32`.
    #[inline]
    pub const fn i32(value: u32) -> Val {
        Val::Int { bits: 32, value: value as u128 }
    }

    /// An `i64`.
    #[inline]
    pub const fn i64(value: u64) -> Val {
        Val::Int { bits: 64, value: value as u128 }
    }

    /// An `i128`.
    #[inline]
    pub const fn i128(value: u128) -> Val {
        Val::Int { bits: 128, value }
    }

    /// A `float`.
    #[inline]
    pub const fn f32(value: f32) -> Val {
        Val::Float(value.to_bits())
    }

    /// A `double`.
    #[inline]
    pub const fn f64(value: f64) -> Val {
        Val::Double(value.to_bits())
    }

    /// The value of an integer, zero-extended.
    #[inline]
    pub const fn as_int(&self) -> Option<u128> {
        match *self {
            Val::Int { value, .. } => Some(value),
            _ => None,
        }
    }

    /// The value of an integer, sign-extended from its width.
    #[inline]
    pub const fn as_signed(&self) -> Option<i128> {
        match *self {
            Val::Int { bits, value } => Some(sext(bits, value)),
            _ => None,
        }
    }

    /// An integer of at most 64 bits, zero-extended, or a pointer's
    /// address.
    #[inline]
    pub const fn as_u64(&self) -> Option<u64> {
        match *self {
            Val::Int { bits, value } if bits <= 64 => Some(value as u64),
            Val::Ptr(addr) => Some(addr),
            _ => None,
        }
    }

    /// A pointer's address.
    #[inline]
    pub const fn as_ptr(&self) -> Option<u64> {
        match *self {
            Val::Ptr(addr) => Some(addr),
            _ => None,
        }
    }

    /// A `float`.
    #[inline]
    pub const fn as_f32(&self) -> Option<f32> {
        match *self {
            Val::Float(bits) => Some(f32::from_bits(bits)),
            _ => None,
        }
    }

    /// A `double`.
    #[inline]
    pub const fn as_f64(&self) -> Option<f64> {
        match *self {
            Val::Double(bits) => Some(f64::from_bits(bits)),
            _ => None,
        }
    }
}

/// `i32 7`, `ptr 0x10040`, `float 0x3f800000`, `double 0x3ff0000000000000`,
/// `agg 0102..`, `<i32 1, poison>`: integers in decimal (unsigned), floats as their encoding.
impl fmt::Display for Val {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Val::Int { bits, value } => write!(f, "i{bits} {value}"),
            Val::Ptr(addr) => write!(f, "ptr {addr:#x}"),
            Val::Float(bits) => write!(f, "float {bits:#010x}"),
            Val::Double(bits) => write!(f, "double {bits:#018x}"),
            Val::Agg(agg) if agg.is_vector() => agg.fmt_lanes(f),
            Val::Agg(agg) => {
                f.write_str("agg ")?;
                agg.bytes.iter().try_for_each(|byte| write!(f, "{byte:02x}"))
            }
        }
    }
}

/// A first-class struct, array or vector value: its type and its image.
///
/// The image of a struct or an array is what `hod_ir`'s `flatten` writes
/// for a constant of the type (`spec/ir.md` §4.3): `alloc_size_of(ty)`
/// bytes, each scalar at its offset in its store size, little-endian,
/// padding zero. That of a vector is its store size, as §3.1 lays it out,
/// and a vector also knows which of its lanes are poison (see
/// [`Vector`](crate::Vector)).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Agg {
    pub(crate) ty: TypeId,
    pub(crate) bytes: Box<[u8]>,
    /// `Some` for a vector.
    pub(crate) lanes: Option<Lanes>,
}

impl Agg {
    /// The struct, array or vector type.
    pub fn ty(&self) -> TypeId {
        self.ty
    }

    /// The image.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §3: integers are kept zero-extended and read back either way.
    #[test]
    fn integers_are_masked_and_sign_extend() {
        assert_eq!(Val::int(7, 0xff), Val::Int { bits: 7, value: 0x7f });
        assert_eq!(Val::int(7, 0x7f).as_signed(), Some(-1));
        assert_eq!(Val::int(128, u128::MAX).as_signed(), Some(-1));
        assert_eq!(Val::i8(0x80).as_signed(), Some(-128));
        assert_eq!(Val::i1(true).as_int(), Some(1));
        assert_eq!(Val::i64(u64::MAX).as_u64(), Some(u64::MAX));
        assert_eq!(Val::i128(1 << 100).as_u64(), None);
        assert_eq!(Val::Ptr(16).as_u64(), Some(16));
        assert_eq!(Val::Ptr(16).as_ptr(), Some(16));
        assert_eq!(Val::i16(3).as_ptr(), None);
        assert_eq!(mask(128), u128::MAX);
        assert_eq!(mask(1), 1);
    }

    /// §3: floats are their encodings; the text form is stable.
    #[test]
    fn floats_are_bits_and_values_print() {
        assert_eq!(Val::f32(1.0), Val::Float(0x3f80_0000));
        assert_eq!(Val::f64(-0.0), Val::Double(1 << 63));
        assert_ne!(Val::f64(0.0), Val::f64(-0.0));
        assert_eq!(Val::Float(0x7fc0_0001), Val::Float(0x7fc0_0001));
        assert_eq!(Val::f32(2.5).as_f32(), Some(2.5));
        assert_eq!(Val::f64(2.5).as_f64(), Some(2.5));
        assert_eq!(Val::i32(7).to_string(), "i32 7");
        assert_eq!(Val::Ptr(0x10040).to_string(), "ptr 0x10040");
        assert_eq!(Val::f32(1.0).to_string(), "float 0x3f800000");
        assert_eq!(Val::f64(1.0).to_string(), "double 0x3ff0000000000000");
        let agg = Agg { ty: TypeId::I8, bytes: vec![1, 0xab].into(), lanes: None };
        assert_eq!(Val::Agg(agg.clone()).to_string(), "agg 01ab");
        assert_eq!((agg.ty(), agg.bytes()), (TypeId::I8, &[1, 0xab][..]));
    }
}
