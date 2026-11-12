//! Floating-point literals (`spec/ll.md` §5.4): from LLVM's six written
//! forms to the bit pattern of the constant's type, by LLVM's rules.

use crate::lexer::hex_value;

/// A binary format narrower than `double`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Narrow {
    Half,
    BFloat,
    Float,
}

impl Narrow {
    /// Exponent and fraction field widths.
    fn fields(self) -> (u32, u32) {
        match self {
            Narrow::Half => (5, 10),
            Narrow::BFloat => (8, 7),
            Narrow::Float => (8, 23),
        }
    }
}

/// The `double` nearest to the decimal `text` (`-1.5e+3`, `+2.`), ties to
/// even; out of range is an infinity.
pub(crate) fn decimal(text: &[u8]) -> Option<u64> {
    std::str::from_utf8(text).ok()?.parse::<f64>().ok().map(f64::to_bits)
}

/// The encoding in `to` of the `double` with encoding `bits`, if `to` has
/// that value exactly. A NaN converts when its payload fits, and stays
/// signaling if it was: LLVM takes pains to keep that.
pub(crate) fn narrow(bits: u64, to: Narrow) -> Option<u64> {
    let (exponent_bits, frac_bits) = to.fields();
    let sign = (bits >> 63) << (exponent_bits + frac_bits);
    let exp = (bits >> 52) & 0x7ff;
    let frac = bits & ((1 << 52) - 1);
    let all_ones = ((1u64 << exponent_bits) - 1) << frac_bits;
    let shift = 52 - frac_bits;
    let dropped = (1u64 << shift) - 1;
    if exp == 0x7ff {
        // An infinity, or a NaN whose payload must survive.
        return (frac & dropped == 0).then_some(sign | all_ones | (frac >> shift));
    }
    if exp == 0 {
        // Zero; a subnormal double is below every narrower format.
        return (frac == 0).then_some(sign);
    }
    let excess = (1i64 << (exponent_bits - 1)) - 1;
    let e = exp as i64 - 1023;
    if e > excess {
        return None;
    }
    if e >= 1 - excess {
        let biased = (e + excess) as u64;
        return (frac & dropped == 0).then_some(sign | (biased << frac_bits) | (frac >> shift));
    }
    // Subnormal in the narrower format.
    let significand = (1u64 << 52) | frac;
    let total = i64::from(shift) + (1 - excess - e);
    if total >= 53 {
        return None;
    }
    (significand & ((1u64 << total) - 1) == 0).then_some(sign | (significand >> total))
}

/// The value of at most 16 hexadecimal digits.
pub(crate) fn hex_u64(digits: &[u8]) -> Option<u64> {
    if digits.len() > 16 {
        return None;
    }
    Some(digits.iter().fold(0, |value, &d| (value << 4) | u64::from(hex_value(d))))
}

/// The encoding of `0xK` followed by `digits`: the first four are the
/// sign and exponent, the rest the significand. LLVM reads the pattern
/// into a value and writes the value back, which changes two kinds of
/// pattern that are not values of the format: with a clear integer bit
/// and an exponent that is neither zero nor all ones the exponent becomes
/// all ones (a NaN), and with a set integer bit and a zero exponent the
/// exponent becomes one.
pub(crate) fn fp80(digits: &[u8]) -> Option<u128> {
    let split = digits.len().min(4);
    let high = hex_u64(&digits[..split])?;
    let low = hex_u64(&digits[split..])?;
    let sign = high & 0x8000;
    let mut exp = high & 0x7fff;
    let integer_bit = low >> 63 != 0;
    if exp != 0 && exp != 0x7fff && !integer_bit {
        exp = 0x7fff;
    } else if exp == 0 && integer_bit {
        exp = 1;
    }
    Some(u128::from(sign | exp) << 64 | u128::from(low))
}

/// The encoding of `0xL` or `0xM` followed by `digits`: the low 64 bits
/// come first. With fewer than 16 digits, they are the high half.
pub(crate) fn fp128(digits: &[u8]) -> Option<u128> {
    let (low, high) = if digits.len() >= 16 {
        (hex_u64(&digits[..16])?, hex_u64(&digits[16..])?)
    } else {
        (0, hex_u64(digits)?)
    };
    Some(u128::from(high) << 64 | u128::from(low))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f32_of(value: f64) -> Option<u64> {
        narrow(value.to_bits(), Narrow::Float)
    }

    /// §5.4: decimal text rounds as `strtod` does.
    #[test]
    fn decimals() {
        assert_eq!(decimal(b"1.0"), Some(1.0f64.to_bits()));
        assert_eq!(decimal(b"-0.0"), Some(0x8000_0000_0000_0000));
        assert_eq!(decimal(b"+2.5"), Some(2.5f64.to_bits()));
        assert_eq!(decimal(b"2.e3"), Some(2000f64.to_bits()));
        assert_eq!(decimal(b"1."), Some(1.0f64.to_bits()));
        assert_eq!(decimal(b"1.5e+10"), Some(1.5e10f64.to_bits()));
        assert_eq!(decimal(b"1.0e999"), Some(f64::INFINITY.to_bits()));
        assert_eq!(decimal(b"0.1"), Some(0x3FB9_9999_9999_999A));
    }

    /// §5.4: a `double` narrows only when nothing is lost.
    #[test]
    fn narrowing_is_exact_or_refused() {
        assert_eq!(f32_of(1.5), Some(u64::from(1.5f32.to_bits())));
        assert_eq!(f32_of(-0.0), Some(0x8000_0000));
        assert_eq!(f32_of(0.1), None);
        assert_eq!(f32_of(f64::INFINITY), Some(0x7F80_0000));
        assert_eq!(f32_of(f64::from(f32::MAX)), Some(u64::from(f32::MAX.to_bits())));
        assert_eq!(f32_of(f64::from(f32::MAX) * 2.0), None);
        assert_eq!(f32_of(f64::from(f32::MIN_POSITIVE)), Some(0x0080_0000));
        // Subnormal results: the smallest float, and half of it.
        assert_eq!(f32_of(f64::from(f32::from_bits(1))), Some(1));
        assert_eq!(f32_of(f64::from(f32::from_bits(1)) / 2.0), None);
        assert_eq!(f32_of(f64::from(f32::from_bits(0x0040_0001))), Some(0x0040_0001));
        assert_eq!(f32_of(f64::from_bits(1)), None);
        // Every float pattern that is a number goes there and back.
        for bits in [0u32, 1, 0x7F7F_FFFF, 0x3F80_0000, 0x8000_0001, 0x0012_3456, 0xC2F6_E979] {
            let wide = f64::from(f32::from_bits(bits));
            assert_eq!(f32_of(wide), Some(u64::from(bits)), "{bits:#x}");
        }
        assert_eq!(narrow(1.5f64.to_bits(), Narrow::Half), Some(0x3E00));
        assert_eq!(narrow(1.0f64.to_bits(), Narrow::BFloat), Some(0x3F80));
        assert_eq!(narrow(65504f64.to_bits(), Narrow::Half), Some(0x7BFF));
        assert_eq!(narrow(65536f64.to_bits(), Narrow::Half), None);
        // 2^-24 is the smallest half; 2^-25 is not one.
        assert_eq!(narrow(2f64.powi(-24).to_bits(), Narrow::Half), Some(1));
        assert_eq!(narrow(2f64.powi(-25).to_bits(), Narrow::Half), None);
    }

    /// §5.4: NaNs keep their sign, their payload and whether they signal.
    #[test]
    fn nans() {
        assert_eq!(narrow(0x7FF8_0000_0000_0000, Narrow::Float), Some(0x7FC0_0000));
        assert_eq!(narrow(0xFFF8_0000_0000_0000, Narrow::Float), Some(0xFFC0_0000));
        assert_eq!(narrow(0x7FF4_0000_0000_0000, Narrow::Float), Some(0x7FA0_0000));
        assert_eq!(narrow(0x7FF0_0000_2000_0000, Narrow::Float), Some(0x7F80_0001));
        assert_eq!(narrow(0x7FF0_0000_0000_0001, Narrow::Float), None);
        assert_eq!(narrow(0x7FF8_0000_0000_0000, Narrow::Half), Some(0x7E00));
    }

    /// §5.4: the wide hexadecimal forms.
    #[test]
    fn wide_forms() {
        assert_eq!(hex_u64(b"3FF0000000000000"), Some(0x3FF0_0000_0000_0000));
        assert_eq!(hex_u64(b"1f"), Some(31));
        assert_eq!(hex_u64(b"00000000000000001"), None);
        assert_eq!(fp80(b"3FFF8000000000000000"), Some(0x3FFF_8000_0000_0000_0000));
        assert_eq!(fp80(b"1"), Some(1 << 64 | 0x7FFE << 64));
        assert_eq!(fp80(b"00008000000000000000"), Some(0x0001_8000_0000_0000_0000));
        assert_eq!(fp80(b"C0010000000000000001"), Some(0xFFFF_0000_0000_0000_0001));
        assert_eq!(fp80(b"3FFF80000000000000001"), None);
        assert_eq!(
            fp128(b"00000000000000013FFF000000000000"),
            Some(0x3FFF_0000_0000_0000_0000_0000_0000_0001)
        );
        assert_eq!(fp128(b"3FFF"), Some(0x3FFF << 64));
        assert_eq!(fp128(b"000000000000000000000000000000001"), None);
    }
}
