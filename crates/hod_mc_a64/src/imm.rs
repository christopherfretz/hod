//! Immediate operand encodings (`spec/mc-a64.md` "Immediates").

use crate::reg::Size;

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
