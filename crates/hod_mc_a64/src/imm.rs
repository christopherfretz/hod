//! Immediate operand encodings (`spec/mc-a64.md` "Immediates").

use crate::reg::{FpSize, Size};

/// Whether `imm` is an ADD/SUB immediate: a 12-bit value, or a 12-bit value
/// shifted left by 12.
#[inline]
pub const fn is_arith_imm(imm: u64) -> bool {
    imm < 0x1000 || (imm.trailing_zeros() >= 12 && imm < 0x100_0000)
}

/// Encodes `value` as a logical ("bitmask") immediate of the given operand
/// size, returning the 13-bit field `N:immr:imms`.
///
/// A value is encodable when it is a rotation of a contiguous run of ones
/// (neither empty nor full) within an element of 2, 4, 8, 16, 32 or 64 bits,
/// replicated across the operand. For [`Size::W`] the value must fit in 32
/// bits and elements are at most 32 bits wide.
///
/// The encoding returned is the canonical one (the unused high bits of
/// `immr` are zero), which is the one assemblers produce.
pub const fn encode_bitmask(sz: Size, value: u64) -> Option<u16> {
    let v = match sz {
        Size::X => value,
        Size::W => {
            if value >> 32 != 0 {
                return None;
            }
            value | value << 32
        }
    };
    if v == 0 || v == u64::MAX {
        return None;
    }
    // The smallest power-of-two period of the bit pattern.
    let mut esize = 64u32;
    while esize > 2 {
        let half = esize / 2;
        let mask = (1u64 << half) - 1;
        if v & mask != (v >> half) & mask {
            break;
        }
        esize = half;
    }
    let emask = u64::MAX >> (64 - esize);
    let elem = v & emask;
    let ones = elem.count_ones();
    // Where the run of ones starts, going up from bit 0 and wrapping at the
    // element size. If bit 0 is clear the run cannot wrap; otherwise it
    // starts just above the highest clear bit.
    let start = if elem & 1 == 0 {
        elem.trailing_zeros()
    } else {
        let clear = !elem & emask;
        (64 - clear.leading_zeros()) % esize
    };
    let run = u64::MAX >> (64 - ones);
    let rotated =
        if start == 0 { run } else { ((run << start) | (run >> (esize - start))) & emask };
    if rotated != elem {
        return None;
    }
    let immr = (esize - start) % esize;
    let imms = (!(esize * 2 - 1) & 0x3f) | (ones - 1);
    let n = (esize == 64) as u32;
    Some((n << 12 | immr << 6 | imms) as u16)
}

/// Decodes a 13-bit `N:immr:imms` field into the immediate it denotes for
/// the given operand size (the Arm ARM's `DecodeBitMasks`), or `None` for a
/// reserved encoding.
///
/// Bits of `immr` above the element size are ignored, as in the
/// architecture, so several encodings can decode to one value;
/// [`encode_bitmask`] returns the one with those bits zero.
pub const fn decode_bitmask(sz: Size, encoding: u16) -> Option<u64> {
    let enc = encoding as u32;
    if enc >> 13 != 0 {
        return None;
    }
    let n = enc >> 12;
    let immr = (enc >> 6) & 0x3f;
    let imms = enc & 0x3f;
    if n == 1 && matches!(sz, Size::W) {
        return None;
    }
    let key = n << 6 | (!imms & 0x3f);
    if key < 2 {
        return None;
    }
    let esize = 1u32 << key.ilog2();
    let levels = esize - 1;
    let s = imms & levels;
    let r = immr & levels;
    if s == levels {
        return None;
    }
    let emask = u64::MAX >> (64 - esize);
    let run = u64::MAX >> (63 - s);
    let elem = if r == 0 { run } else { ((run >> r) | (run << (esize - r))) & emask };
    let mut value = 0u64;
    let mut at = 0;
    while at < sz.bits() {
        value |= elem << at;
        at += esize;
    }
    Some(value)
}

/// The bit pattern an FMOV immediate field denotes at the given precision
/// (the Arm ARM's `VFPExpandImm`): `(-1)^s * (16 + m) / 16 * 2^e` with `m`
/// in `0..=15` and `e` in `-3..=4`, where `imm8` is `s:e':m`.
///
/// Every `imm8` is valid; the 256 values are distinct. A single-precision
/// pattern is returned in the low 32 bits.
pub const fn decode_fp_imm8(sz: FpSize, imm8: u8) -> u64 {
    let imm = imm8 as u64;
    let (exp_bits, frac_bits) = match sz {
        FpSize::S => (8, 23),
        FpSize::D => (11, 52),
    };
    // The exponent is NOT(b6) : Replicate(b6, exp_bits - 3) : imm8<5:4>.
    let b6 = (imm >> 6) & 1;
    let replicated = if b6 == 1 { (1 << (exp_bits - 3)) - 1 } else { 0 };
    let exp = (b6 ^ 1) << (exp_bits - 1) | replicated << 2 | (imm >> 4) & 3;
    (imm >> 7) << (exp_bits + frac_bits) | exp << frac_bits | (imm & 0xf) << (frac_bits - 4)
}

/// Encodes the floating-point bit pattern `bits` as an FMOV immediate
/// field, or `None` when it is not one of the 256 encodable values.
///
/// For [`FpSize::S`] the pattern is in the low 32 bits and the upper 32
/// must be clear. Neither zero is encodable, nor any infinity, NaN or
/// denormal.
pub const fn encode_fp_imm8(sz: FpSize, bits: u64) -> Option<u8> {
    let (sign_at, frac_bits) = match sz {
        FpSize::S => (31, 23),
        FpSize::D => (63, 52),
    };
    if bits >> sign_at > 1 {
        return None;
    }
    // The only field that can denote `bits`: its sign, the inverse of its
    // top exponent bit, its low two exponent bits and top four fraction
    // bits. It does when the bits it leaves out are the ones implied.
    let top_exp = (bits >> (sign_at - 1)) & 1;
    let imm8 =
        ((bits >> sign_at) << 7 | (top_exp ^ 1) << 6 | (bits >> (frac_bits - 4)) & 0x3f) as u8;
    if decode_fp_imm8(sz, imm8) == bits { Some(imm8) } else { None }
}
