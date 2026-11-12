//! Vector values (`spec/interp.md` §3.1): a fixed-width vector is its
//! memory image, with a poison flag per lane.
//!
//! The image is LLVM's in-memory layout (the language reference's "Vector
//! Type" and "Data Layout"): the vector is one little-endian integer of `len * bits` bits
//! with lane 0 in the lowest bits. For lanes of whole bytes that puts lane
//! `i` at byte `i * bits / 8`; for `<N x i1>` lane `i` is bit `i % 8` of
//! byte `i / 8`. The image is the vector's store size, `ceil(len * bits /
//! 8)` bytes, and the bits above `len * bits` are zero. A poison lane's
//! bits are zero in the image too, so that two equal vectors have equal
//! images.

use std::fmt;

use hod_ir::{TypeId, TypeKind, TypeTable};

use crate::value::{Agg, MAX_INT_BITS, Val, mask};

/// What one lane of a vector is (§3.1).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Lane {
    /// `iN`: `i1`, or a whole number of bytes up to `i128`.
    Int(u32),
    /// `ptr`: an address.
    Ptr,
    /// `float`.
    Float,
    /// `double`.
    Double,
}

impl Lane {
    /// The lane's width in bits.
    pub const fn bits(self) -> u32 {
        match self {
            Lane::Int(bits) => bits,
            Lane::Ptr | Lane::Double => 64,
            Lane::Float => 32,
        }
    }

    /// The lane kind of element type `elem`, or the unsupported reason
    /// (§8) why there is none.
    pub(crate) fn of(types: &TypeTable, elem: TypeId) -> Result<Lane, &'static str> {
        Ok(match types.kind(elem) {
            TypeKind::Int(bits) if bits > MAX_INT_BITS => return Err("type:int-over-128"),
            TypeKind::Int(bits) if bits == 1 || bits.is_multiple_of(8) => Lane::Int(bits),
            TypeKind::Int(_) => return Err("type:vector-odd-element"),
            TypeKind::Ptr { .. } => Lane::Ptr,
            TypeKind::Float => Lane::Float,
            TypeKind::Double => Lane::Double,
            TypeKind::Half => return Err("type:half"),
            TypeKind::BFloat => return Err("type:bfloat"),
            TypeKind::Fp128 => return Err("type:fp128"),
            TypeKind::X86Fp80 => return Err("type:x86_fp80"),
            _ => return Err("type:vector"),
        })
    }

    /// The lane's value from its bits, zero-extended.
    pub(crate) const fn val(self, raw: u128) -> Val {
        match self {
            Lane::Int(bits) => Val::Int { bits, value: raw },
            Lane::Ptr => Val::Ptr(raw as u64),
            Lane::Float => Val::Float(raw as u32),
            Lane::Double => Val::Double(raw as u64),
        }
    }

    /// The bits of `val` if it is a value of this lane kind.
    pub(crate) const fn raw(self, val: &Val) -> Option<u128> {
        match (self, val) {
            (Lane::Int(bits), &Val::Int { bits: have, value }) if bits == have => Some(value),
            (Lane::Ptr, &Val::Ptr(addr)) => Some(addr as u128),
            (Lane::Float, &Val::Float(bits)) => Some(bits as u128),
            (Lane::Double, &Val::Double(bits)) => Some(bits as u128),
            _ => None,
        }
    }
}

/// A value of a fixed-width vector type (§3.1): its image and which of
/// its lanes are poison. It is a [`Val::Agg`] whose type is a vector type.
///
/// Two vectors are equal when they have the same type, image and poison
/// lanes.
pub type Vector = Agg;

/// What an [`Agg`] of a vector type has besides its type and image.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) struct Lanes {
    lane: Lane,
    len: u32,
    /// One bit per lane, set for a poison lane; empty when no lane is.
    poison: Box<[u64]>,
}

impl Agg {
    /// The vector of type `ty` whose lanes are `lanes`, `None` standing
    /// for a poison lane. `None` when `ty` is not a vector type of the
    /// interpreter (§3.1) or a lane is not of its element type.
    pub fn from_lanes(types: &TypeTable, ty: TypeId, lanes: &[Option<Val>]) -> Option<Vector> {
        let (lane, len) = shape(types, ty)?;
        if lanes.len() != len as usize {
            return None;
        }
        let mut vector = Vector::zeroed(ty, lane, len);
        for (index, value) in lanes.iter().enumerate() {
            match value {
                Some(value) => vector.set(index as u32, lane.raw(value)?),
                None => vector.set_poison(index as u32),
            }
        }
        Some(vector)
    }

    /// The vector of type `ty` whose image is `bytes`, with no poison
    /// lane. `None` when `ty` is not a vector type of the interpreter, the
    /// image is not its store size, or a bit above the lanes is set.
    pub fn from_image(types: &TypeTable, ty: TypeId, bytes: &[u8]) -> Option<Vector> {
        let (lane, len) = shape(types, ty)?;
        let vector = Vector::from_bytes(ty, lane, len, bytes)?;
        vector.padding_is_zero().then_some(vector)
    }

    /// Whether this is a vector.
    pub fn is_vector(&self) -> bool {
        self.lanes.is_some()
    }

    /// What the lanes of a vector are; `None` for a struct or an array.
    pub fn lane_kind(&self) -> Option<Lane> {
        self.lanes.as_ref().map(|lanes| lanes.lane)
    }

    /// How many lanes a vector has; 0 for a struct or an array.
    pub fn lane_count(&self) -> u32 {
        self.count()
    }

    /// Lane `index` of a vector, `None` if it is poison or there is no
    /// such lane.
    pub fn lane(&self, index: u32) -> Option<Val> {
        (index < self.count() && !self.is_poison(index)).then(|| self.kind().val(self.get(index)))
    }

    /// Whether lane `index` of a vector is poison.
    pub fn is_poison(&self, index: u32) -> bool {
        self.poison().get(index as usize / 64).is_some_and(|word| word >> (index % 64) & 1 == 1)
    }

    /// Whether any lane of a vector is poison; `false` for a struct or an
    /// array.
    pub fn has_poison(&self) -> bool {
        !self.poison().is_empty()
    }

    // ---- inside the crate ----

    fn poison(&self) -> &[u64] {
        self.lanes.as_ref().map_or(&[], |lanes| &lanes.poison)
    }

    /// The lane kind of a vector (bytes for anything else).
    pub(crate) fn kind(&self) -> Lane {
        self.lanes.as_ref().map_or(Lane::Int(8), |lanes| lanes.lane)
    }

    /// The lane count of a vector; 0 for anything else.
    pub(crate) fn count(&self) -> u32 {
        self.lanes.as_ref().map_or(0, |lanes| lanes.len)
    }

    /// All lanes zero, none poison.
    pub(crate) fn zeroed(ty: TypeId, lane: Lane, len: u32) -> Vector {
        let size = image_size(lane, len);
        Agg {
            ty,
            bytes: vec![0; size].into(),
            lanes: Some(Lanes { lane, len, poison: Box::new([]) }),
        }
    }

    /// The vector with this image, which is `image_size` long; its
    /// padding bits are not checked.
    pub(crate) fn from_bytes(ty: TypeId, lane: Lane, len: u32, bytes: &[u8]) -> Option<Vector> {
        (bytes.len() == image_size(lane, len)).then(|| Agg {
            ty,
            bytes: bytes.into(),
            lanes: Some(Lanes { lane, len, poison: Box::new([]) }),
        })
    }

    /// Whether the bits above the lanes in the last byte are zero.
    pub(crate) fn padding_is_zero(&self) -> bool {
        let used = u64::from(self.count()) * u64::from(self.kind().bits());
        let spare = (used % 8) as u32;
        spare == 0 || self.bytes.last().is_none_or(|&top| top >> spare == 0)
    }

    /// The bits of lane `index`, poison or not (a poison lane's are 0).
    pub(crate) fn get(&self, index: u32) -> u128 {
        let bits = self.kind().bits();
        if bits == 1 {
            return u128::from(self.bytes[index as usize / 8] >> (index % 8) & 1);
        }
        let width = bits as usize / 8;
        let at = index as usize * width;
        let mut wide = [0u8; 16];
        wide[..width].copy_from_slice(&self.bytes[at..at + width]);
        u128::from_le_bytes(wide)
    }

    /// Sets lane `index` of a vector to `raw` (masked to the lane width)
    /// and makes it not poison.
    pub(crate) fn set(&mut self, index: u32, raw: u128) {
        let bits = self.kind().bits();
        if bits == 1 {
            let byte = &mut self.bytes[index as usize / 8];
            *byte = (*byte & !(1 << (index % 8))) | (((raw & 1) as u8) << (index % 8));
        } else {
            let width = bits as usize / 8;
            let at = index as usize * width;
            let raw = raw & mask(bits);
            self.bytes[at..at + width].copy_from_slice(&raw.to_le_bytes()[..width]);
        }
        if let Some(lanes) = self.lanes.as_mut()
            && !lanes.poison.is_empty()
        {
            lanes.poison[index as usize / 64] &= !(1 << (index % 64));
            if lanes.poison.iter().all(|&word| word == 0) {
                lanes.poison = Box::new([]);
            }
        }
    }

    /// Makes lane `index` of a vector poison; its bits become zero.
    pub(crate) fn set_poison(&mut self, index: u32) {
        self.set(index, 0);
        if let Some(lanes) = self.lanes.as_mut() {
            if lanes.poison.is_empty() {
                lanes.poison = vec![0; (lanes.len as usize).div_ceil(64)].into();
            }
            lanes.poison[index as usize / 64] |= 1 << (index % 64);
        }
    }

    /// The bytes of the image that hold some bit of lane `index`.
    pub(crate) fn byte_range(&self, index: u32) -> std::ops::Range<usize> {
        let bits = self.kind().bits() as usize;
        let first = index as usize * bits / 8;
        let last = (index as usize * bits + bits - 1) / 8;
        first..last + 1
    }

    /// `<i32 1, poison, i32 3>`: the lanes as [`Val`] prints them.
    pub(crate) fn fmt_lanes(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<")?;
        for index in 0..self.count() {
            if index != 0 {
                f.write_str(", ")?;
            }
            match self.lane(index) {
                Some(value) => write!(f, "{value}")?,
                None => f.write_str("poison")?,
            }
        }
        f.write_str(">")
    }
}

/// The store size in bytes of `len` lanes of `lane`.
pub(crate) fn image_size(lane: Lane, len: u32) -> usize {
    (u64::from(len) * u64::from(lane.bits())).div_ceil(8) as usize
}

/// The lane kind and count of `ty`, if it is a vector type whose lanes
/// the interpreter supports.
pub(crate) fn shape(types: &TypeTable, ty: TypeId) -> Option<(Lane, u32)> {
    if ty.index() >= types.len() {
        return None;
    }
    match types.kind(ty) {
        TypeKind::Vector { elem, len } if len != 0 => Some((Lane::of(types, elem).ok()?, len)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hod_ir::TargetSpec;

    fn types() -> TypeTable {
        TypeTable::new(TargetSpec::aarch64_linux_gnu().data_layout())
    }

    /// §3.1: byte lanes at multiples of their size, little-endian.
    #[test]
    fn byte_lanes_are_little_endian() {
        let mut t = types();
        let ty = t.vector(TypeId::I16, 3).unwrap();
        let lanes = [Some(Val::i16(0x0102)), None, Some(Val::i16(0xa0b0))];
        let v = Vector::from_lanes(&t, ty, &lanes).unwrap();
        assert!(v.is_vector() && v.lane_kind() == Some(Lane::Int(16)));
        assert_eq!(v.bytes(), [0x02, 0x01, 0, 0, 0xb0, 0xa0]);
        assert_eq!((v.lane(0), v.lane(1), v.lane(3)), (Some(Val::i16(0x0102)), None, None));
        assert!(v.is_poison(1) && v.has_poison() && !v.is_poison(2));
        assert_eq!(Val::Agg(v.clone()).to_string(), "<i16 258, poison, i16 41136>");
        assert_eq!(v.byte_range(2), 4..6);
        let mut w = v.clone();
        w.set(1, 7);
        assert!(!w.has_poison());
        assert_eq!(Vector::from_image(&t, ty, w.bytes()), Some(w));
    }

    /// §3.1: `<N x i1>` is bit-packed, lane 0 in bit 0, the bits above
    /// zero.
    #[test]
    fn masks_are_bit_packed() {
        let mut t = types();
        let ty = t.vector(TypeId::I1, 11).unwrap();
        let lanes: Vec<Option<Val>> = (0..11).map(|i| Some(Val::i1(i % 3 == 0))).collect();
        let v = Vector::from_lanes(&t, ty, &lanes).unwrap();
        // Lanes 0, 3, 6, 9.
        assert_eq!(v.bytes(), [0b0100_1001, 0b0000_0010]);
        assert_eq!(v.byte_range(9), 1..2);
        assert!(Vector::from_image(&t, ty, &[0, 0b1000]).is_none());
        assert!(Vector::from_image(&t, ty, &[0xff, 0b111]).is_some());
        let i7 = t.int(7).unwrap();
        let odd = t.vector(i7, 2).unwrap();
        assert!(shape(&t, odd).is_none());
        assert_eq!(Lane::of(&t, i7), Err("type:vector-odd-element"));
    }
}
