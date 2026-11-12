//! Arithmetic on integers of any width, as little-endian 64-bit words.
//!
//! The canonical form of an `N`-bit value (`spec/ir.md` §4.1) is exactly
//! `ceil(N / 64)` words with the bits above `N` clear. Every function here
//! returns canonical values when given canonical operands of one width.

/// Number of words holding `bits` bits.
pub(crate) fn words_for(bits: u32) -> usize {
    bits.div_ceil(64) as usize
}

/// The low `bits` bits set, for `bits <= 64`; all bits for more.
pub(crate) fn mask(bits: u32) -> u64 {
    if bits >= 64 { u64::MAX } else { (1u64 << bits) - 1 }
}

/// `value` as a signed integer, sign-extended from its low `bits` bits;
/// `value` itself for 64 bits or more.
pub fn sext64(value: u64, bits: u32) -> i64 {
    if bits >= 64 {
        value as i64
    } else {
        let shift = 64 - bits;
        ((value << shift) as i64) >> shift
    }
}

/// Reduces `words` modulo `2^bits` into canonical form.
pub(crate) fn truncate(words: &mut Vec<u64>, bits: u32) {
    words.resize(words_for(bits), 0);
    if let Some(top) = words.last_mut() {
        let used = bits % 64;
        if used != 0 {
            *top &= mask(used);
        }
    }
}

/// Sign-extends the canonical `from`-bit value in `words` to `to` bits.
pub(crate) fn sign_extend(words: &mut Vec<u64>, from: u32, to: u32) {
    debug_assert!(from >= 1 && from <= to && words.len() == words_for(from));
    let top = (from - 1) as usize;
    let negative = (words[top / 64] >> (top % 64)) & 1 == 1;
    if negative {
        let used = from % 64;
        if used != 0 {
            words[top / 64] |= !mask(used);
        }
        words.resize(words_for(to), u64::MAX);
    }
    truncate(words, to);
}

/// `a << amount` modulo `2^bits`, for `amount < bits`.
pub(crate) fn shl(a: &[u64], amount: u32, bits: u32) -> Vec<u64> {
    let (word_shift, bit_shift) = ((amount / 64) as usize, amount % 64);
    let mut out = vec![0; a.len()];
    for (from, slot) in out.iter_mut().skip(word_shift).enumerate() {
        *slot = a[from] << bit_shift;
        if bit_shift != 0 && from > 0 {
            *slot |= a[from - 1] >> (64 - bit_shift);
        }
    }
    truncate(&mut out, bits);
    out
}

/// `a + b` modulo `2^bits`.
pub(crate) fn add(a: &[u64], b: &[u64], bits: u32) -> Vec<u64> {
    let mut carry = false;
    let mut out: Vec<u64> = a
        .iter()
        .zip(b)
        .map(|(&x, &y)| {
            let (sum, c1) = x.overflowing_add(y);
            let (sum, c2) = sum.overflowing_add(u64::from(carry));
            carry = c1 || c2;
            sum
        })
        .collect();
    truncate(&mut out, bits);
    out
}

/// `a - b` modulo `2^bits`.
pub(crate) fn sub(a: &[u64], b: &[u64], bits: u32) -> Vec<u64> {
    let mut borrow = false;
    let mut out: Vec<u64> = a
        .iter()
        .zip(b)
        .map(|(&x, &y)| {
            let (diff, b1) = x.overflowing_sub(y);
            let (diff, b2) = diff.overflowing_sub(u64::from(borrow));
            borrow = b1 || b2;
            diff
        })
        .collect();
    truncate(&mut out, bits);
    out
}

/// `a * b` modulo `2^bits`.
pub(crate) fn mul(a: &[u64], b: &[u64], bits: u32) -> Vec<u64> {
    let mut out = vec![0u64; a.len()];
    for (i, &x) in a.iter().enumerate() {
        let mut carry = 0u128;
        for (j, &y) in b.iter().enumerate() {
            let Some(slot) = out.get_mut(i + j) else { break };
            let product = u128::from(x) * u128::from(y) + u128::from(*slot) + carry;
            *slot = product as u64;
            carry = product >> 64;
        }
    }
    truncate(&mut out, bits);
    out
}

/// A bitwise operation, word by word.
pub(crate) fn bitwise(a: &[u64], b: &[u64], op: impl Fn(u64, u64) -> u64) -> Vec<u64> {
    a.iter().zip(b).map(|(&x, &y)| op(x, y)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §4.1: canonical form is `ceil(N/64)` words with the top bits clear.
    #[test]
    fn truncate_makes_canonical_words() {
        let mut w = vec![u64::MAX, u64::MAX, u64::MAX];
        truncate(&mut w, 65);
        assert_eq!(w, [u64::MAX, 1]);
        truncate(&mut w, 128);
        assert_eq!(w, [u64::MAX, 1]);
        truncate(&mut w, 200);
        assert_eq!(w, [u64::MAX, 1, 0, 0]);
        truncate(&mut w, 3);
        assert_eq!(w, [7]);
        assert_eq!(words_for(1), 1);
        assert_eq!(words_for(64), 1);
        assert_eq!(words_for(65), 2);
        assert_eq!(mask(0), 0);
        assert_eq!(mask(8), 0xff);
        assert_eq!(mask(64), u64::MAX);
        assert_eq!(sext64(0xff, 8), -1);
        assert_eq!(sext64(0x7f, 8), 127);
        assert_eq!(sext64(u64::MAX, 64), -1);
        assert_eq!(sext64(1, 1), -1);
    }

    #[test]
    fn sign_extension() {
        let mut w = vec![0x80];
        sign_extend(&mut w, 8, 72);
        assert_eq!(w, [0xffff_ffff_ffff_ff80, 0xff]);
        let mut w = vec![0x7f];
        sign_extend(&mut w, 8, 72);
        assert_eq!(w, [0x7f, 0]);
        let mut w = vec![0, 1];
        sign_extend(&mut w, 65, 128);
        assert_eq!(w, [0, u64::MAX]);
        let mut w = vec![1u64 << 63];
        sign_extend(&mut w, 64, 64);
        assert_eq!(w, [1u64 << 63]);
    }

    /// Checked against `u128` arithmetic at 128 bits and at an odd width.
    #[test]
    fn arithmetic_matches_u128() {
        let split = |v: u128| vec![v as u64, (v >> 64) as u64];
        let samples = [
            0u128,
            1,
            u128::MAX,
            u128::from(u64::MAX),
            u128::from(u64::MAX) + 1,
            0x1234_5678_9abc_def0_0fed_cba9_8765_4321,
            1 << 127,
            (1 << 100) - 12345,
        ];
        for &x in &samples {
            for &y in &samples {
                assert_eq!(add(&split(x), &split(y), 128), split(x.wrapping_add(y)));
                assert_eq!(sub(&split(x), &split(y), 128), split(x.wrapping_sub(y)));
                assert_eq!(mul(&split(x), &split(y), 128), split(x.wrapping_mul(y)));
                assert_eq!(bitwise(&split(x), &split(y), |a, b| a ^ b), split(x ^ y));
                let m = (1u128 << 100) - 1;
                let (xm, ym) = (split(x & m), split(y & m));
                assert_eq!(add(&xm, &ym, 100), split(x.wrapping_add(y) & m));
                assert_eq!(sub(&xm, &ym, 100), split(x.wrapping_sub(y) & m));
                assert_eq!(mul(&xm, &ym, 100), split(x.wrapping_mul(y) & m));
            }
        }
        // One word, narrow width.
        assert_eq!(add(&[0xff], &[1], 8), [0]);
        assert_eq!(sub(&[0], &[1], 8), [0xff]);
        assert_eq!(mul(&[0x10], &[0x10], 8), [0]);
    }

    /// `shl` moves bits across words and drops those shifted out.
    #[test]
    fn shl_matches_u128() {
        let value = 0x0123_4567_89ab_cdef_0fed_cba9_8765_4321u128;
        let words = [value as u64, (value >> 64) as u64];
        for amount in [0, 1, 7, 63, 64, 65, 100, 127] {
            let expected = value << amount;
            assert_eq!(shl(&words, amount, 128), [expected as u64, (expected >> 64) as u64]);
        }
        assert_eq!(shl(&[0xff], 4, 8), [0xf0]);
        assert_eq!(shl(&[u64::MAX, 1], 1, 65), [u64::MAX - 1, 1]);
    }
}
