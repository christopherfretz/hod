//! The arithmetic of single instructions (`spec/interp.md` §5.1 to §5.3),
//! as functions of plain numbers: no memory, no types beyond a width.
//!
//! An integer of width `bits` is a `u128` with the bits above the width
//! clear, on the way in and on the way out.

// Exact IEEE comparison is the meaning being implemented.
#![allow(clippy::float_cmp)]

use hod_ir::{FloatPredicate, IntPredicate, Opcode};

use crate::error::Trap;
use crate::value::{mask, sext};

/// The thirteen integer binary instructions.
pub(crate) fn int_binary(op: Opcode, bits: u32, a: u128, b: u128) -> Result<u128, Trap> {
    let m = mask(bits);
    let (sa, sb) = (sext(bits, a), sext(bits, b));
    let min = 1u128 << (bits - 1);
    let shift = || if b >= u128::from(bits) { Err(Trap::ShiftTooLarge) } else { Ok(b as u32) };
    let nonzero = || if b == 0 { Err(Trap::DivisionByZero) } else { Ok(()) };
    let no_overflow = || if a == min && b == m { Err(Trap::DivisionOverflow) } else { Ok(()) };
    Ok(match op {
        Opcode::Add => a.wrapping_add(b),
        Opcode::Sub => a.wrapping_sub(b),
        Opcode::Mul => a.wrapping_mul(b),
        Opcode::UDiv => {
            nonzero()?;
            a / b
        }
        Opcode::URem => {
            nonzero()?;
            a % b
        }
        Opcode::SDiv => {
            nonzero()?;
            no_overflow()?;
            sa.wrapping_div(sb) as u128
        }
        Opcode::SRem => {
            nonzero()?;
            no_overflow()?;
            sa.wrapping_rem(sb) as u128
        }
        Opcode::Shl => a << shift()?,
        Opcode::LShr => a >> shift()?,
        Opcode::AShr => (sa >> shift()?) as u128,
        Opcode::And => a & b,
        Opcode::Or => a | b,
        Opcode::Xor => a ^ b,
        _ => unreachable!("{op:?} is not an integer binary instruction"),
    } & m)
}

/// `icmp`. Pointers compare as 64-bit integers.
pub(crate) fn icmp(pred: IntPredicate, bits: u32, a: u128, b: u128) -> bool {
    let (sa, sb) = (sext(bits, a), sext(bits, b));
    match pred {
        IntPredicate::Eq => a == b,
        IntPredicate::Ne => a != b,
        IntPredicate::Ugt => a > b,
        IntPredicate::Uge => a >= b,
        IntPredicate::Ult => a < b,
        IntPredicate::Ule => a <= b,
        IntPredicate::Sgt => sa > sb,
        IntPredicate::Sge => sa >= sb,
        IntPredicate::Slt => sa < sb,
        IntPredicate::Sle => sa <= sb,
    }
}

/// `fcmp`. A `float` is compared after the (exact) widening to `f64`.
pub(crate) fn fcmp(pred: FloatPredicate, a: f64, b: f64) -> bool {
    let unordered = a.is_nan() || b.is_nan();
    match pred {
        FloatPredicate::False => false,
        FloatPredicate::Oeq => a == b,
        FloatPredicate::Ogt => a > b,
        FloatPredicate::Oge => a >= b,
        FloatPredicate::Olt => a < b,
        FloatPredicate::Ole => a <= b,
        FloatPredicate::One => !unordered && a != b,
        FloatPredicate::Ord => !unordered,
        FloatPredicate::Uno => unordered,
        FloatPredicate::Ueq => unordered || a == b,
        FloatPredicate::Ugt => unordered || a > b,
        FloatPredicate::Uge => unordered || a >= b,
        FloatPredicate::Ult => unordered || a < b,
        FloatPredicate::Ule => unordered || a <= b,
        FloatPredicate::Une => unordered || a != b,
        FloatPredicate::True => true,
    }
}

macro_rules! float_binary {
    ($name:ident, $ty:ty) => {
        /// `fadd` to `frem` in IEEE arithmetic, round to nearest even.
        /// `frem` is C's `fmod`.
        pub(crate) fn $name(op: Opcode, a: $ty, b: $ty) -> $ty {
            match op {
                Opcode::FAdd => a + b,
                Opcode::FSub => a - b,
                Opcode::FMul => a * b,
                Opcode::FDiv => a / b,
                Opcode::FRem => a % b,
                _ => unreachable!("{op:?} is not a float binary instruction"),
            }
        }
    };
}

float_binary!(f32_binary, f32);
float_binary!(f64_binary, f64);

/// `fptoui` and `fptosi` to an integer of `bits` bits: the value rounded
/// toward zero, which must fit.
pub(crate) fn float_to_int(x: f64, bits: u32, signed: bool) -> Result<u128, Trap> {
    if !x.is_finite() {
        return Err(Trap::FloatToIntOverflow);
    }
    let whole = x.trunc();
    // Powers of two up to 2^128 are exact in an `f64`.
    let two = |exponent: u32| 2f64.powi(exponent as i32);
    if signed {
        let limit = two(bits - 1);
        if whole < -limit || whole >= limit {
            return Err(Trap::FloatToIntOverflow);
        }
        Ok(whole as i128 as u128 & mask(bits))
    } else {
        if whole < 0.0 || whole >= two(bits) {
            return Err(Trap::FloatToIntOverflow);
        }
        Ok(whole as u128)
    }
}

/// The bit operations and min/max intrinsics with one result of the
/// operands' width.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum IntOp {
    Abs,
    Smax,
    Smin,
    Umax,
    Umin,
    Ctpop,
    Ctlz,
    Cttz,
    Bswap,
    BitReverse,
    Fshl,
    Fshr,
    SaddSat,
    UaddSat,
    SsubSat,
    UsubSat,
}

/// An [`IntOp`] on operands of `bits` bits. `b` is the second operand or,
/// for `abs`, `ctlz` and `cttz`, the "poison on the special input" flag;
/// `c` is the shift amount of a funnel shift.
pub(crate) fn int_intrinsic(op: IntOp, bits: u32, a: u128, b: u128, c: u128) -> Result<u128, Trap> {
    let m = mask(bits);
    let (sa, sb) = (sext(bits, a), sext(bits, b));
    let min = 1u128 << (bits - 1);
    let max = m >> 1;
    let poison_if = |flagged: bool| if flagged { Err(Trap::PoisonIntrinsic) } else { Ok(()) };
    Ok(match op {
        IntOp::Abs => {
            poison_if(a == min && b != 0)?;
            sa.unsigned_abs()
        }
        IntOp::Smax => {
            if sa >= sb {
                a
            } else {
                b
            }
        }
        IntOp::Smin => {
            if sa <= sb {
                a
            } else {
                b
            }
        }
        IntOp::Umax => a.max(b),
        IntOp::Umin => a.min(b),
        IntOp::Ctpop => u128::from(a.count_ones()),
        IntOp::Ctlz => {
            poison_if(a == 0 && b != 0)?;
            u128::from(a.leading_zeros() - (128 - bits))
        }
        IntOp::Cttz => {
            poison_if(a == 0 && b != 0)?;
            u128::from(a.trailing_zeros().min(bits))
        }
        IntOp::Bswap => a.swap_bytes() >> (128 - bits),
        IntOp::BitReverse => a.reverse_bits() >> (128 - bits),
        IntOp::Fshl | IntOp::Fshr => {
            let amount = (c % u128::from(bits)) as u32;
            match (op, amount) {
                (IntOp::Fshl, 0) => a,
                (_, 0) => b,
                (IntOp::Fshl, _) => (a << amount) | (b >> (bits - amount)),
                _ => (b >> amount) | (a << (bits - amount)),
            }
        }
        IntOp::UaddSat => a.checked_add(b).filter(|&sum| sum <= m).unwrap_or(m),
        IntOp::UsubSat => a.saturating_sub(b),
        IntOp::SaddSat | IntOp::SsubSat => {
            let exact = if op == IntOp::SaddSat { sa.checked_add(sb) } else { sa.checked_sub(sb) };
            match exact {
                Some(value) if fits_signed(value, bits) => value as u128,
                // Overflow goes the way of the first operand's sign, except
                // that a subtraction from zero or more overflows upwards.
                _ if (op == IntOp::SaddSat && sa < 0) || (op == IntOp::SsubSat && sb > 0) => min,
                _ => max,
            }
        }
    } & m)
}

fn fits_signed(value: i128, bits: u32) -> bool {
    bits == 128 || (value >= -(1i128 << (bits - 1)) && value < (1i128 << (bits - 1)))
}

/// The arithmetic of the `*.with.overflow` intrinsics.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Checked {
    Sadd,
    Uadd,
    Ssub,
    Usub,
    Smul,
    Umul,
}

/// The wrapped result and whether the exact result does not fit.
pub(crate) fn checked(op: Checked, bits: u32, a: u128, b: u128) -> (u128, bool) {
    let m = mask(bits);
    let (sa, sb) = (sext(bits, a), sext(bits, b));
    let signed = |exact: Option<i128>| exact.is_none_or(|value| !fits_signed(value, bits));
    let unsigned = |exact: Option<u128>| exact.is_none_or(|value| value > m);
    match op {
        Checked::Sadd => (a.wrapping_add(b) & m, signed(sa.checked_add(sb))),
        Checked::Ssub => (a.wrapping_sub(b) & m, signed(sa.checked_sub(sb))),
        Checked::Smul => (a.wrapping_mul(b) & m, signed(sa.checked_mul(sb))),
        Checked::Uadd => (a.wrapping_add(b) & m, unsigned(a.checked_add(b))),
        Checked::Usub => (a.wrapping_sub(b) & m, a < b),
        Checked::Umul => (a.wrapping_mul(b) & m, unsigned(a.checked_mul(b))),
    }
}

/// `scmp` and `ucmp`: -1, 0 or 1 in a result of `result_bits` bits.
pub(crate) fn three_way(signed: bool, bits: u32, a: u128, b: u128, result_bits: u32) -> u128 {
    let ordering = if signed { sext(bits, a).cmp(&sext(bits, b)) } else { a.cmp(&b) };
    (ordering as i128 as u128) & mask(result_bits)
}

#[cfg(test)]
mod tests {
    use super::*;

    const I8: u32 = 8;

    /// §5.1: wrapping arithmetic at a byte and at odd and full widths.
    #[test]
    fn wrapping_arithmetic() {
        assert_eq!(int_binary(Opcode::Add, I8, 250, 10), Ok(4));
        assert_eq!(int_binary(Opcode::Sub, I8, 3, 5), Ok(254));
        assert_eq!(int_binary(Opcode::Mul, I8, 16, 17), Ok(16));
        assert_eq!(int_binary(Opcode::Add, 7, 127, 1), Ok(0));
        assert_eq!(int_binary(Opcode::Mul, 33, 1 << 32, 2), Ok(0));
        assert_eq!(int_binary(Opcode::Add, 1, 1, 1), Ok(0));
        assert_eq!(int_binary(Opcode::Add, 128, u128::MAX, 2), Ok(1));
        assert_eq!(int_binary(Opcode::Sub, 128, 0, 1), Ok(u128::MAX));
        assert_eq!(int_binary(Opcode::And, I8, 0xf0, 0x3c), Ok(0x30));
        assert_eq!(int_binary(Opcode::Or, I8, 0xf0, 0x3c), Ok(0xfc));
        assert_eq!(int_binary(Opcode::Xor, I8, 0xf0, 0x3c), Ok(0xcc));
    }

    /// §5.1: division and remainder, their two traps, and signedness.
    #[test]
    fn division() {
        assert_eq!(int_binary(Opcode::UDiv, I8, 200, 7), Ok(28));
        assert_eq!(int_binary(Opcode::URem, I8, 200, 7), Ok(4));
        // 200 is -56: -56 / 7 = -8, -56 % 7 = 0; -57 % 7 = -1.
        assert_eq!(int_binary(Opcode::SDiv, I8, 200, 7), Ok(248));
        assert_eq!(int_binary(Opcode::SRem, I8, 199, 7), Ok(255));
        assert_eq!(int_binary(Opcode::SDiv, I8, 7, 254), Ok(253));
        assert_eq!(int_binary(Opcode::SRem, I8, 7, 254), Ok(1));
        for op in [Opcode::UDiv, Opcode::URem, Opcode::SDiv, Opcode::SRem] {
            assert_eq!(int_binary(op, I8, 5, 0), Err(Trap::DivisionByZero), "{op:?}");
            assert_eq!(int_binary(op, 128, 5, 0), Err(Trap::DivisionByZero), "{op:?}");
        }
        for op in [Opcode::SDiv, Opcode::SRem] {
            assert_eq!(int_binary(op, I8, 0x80, 0xff), Err(Trap::DivisionOverflow));
            assert_eq!(int_binary(op, 1, 1, 1), Err(Trap::DivisionOverflow));
            assert_eq!(int_binary(op, 128, 1 << 127, u128::MAX), Err(Trap::DivisionOverflow));
        }
        assert_eq!(int_binary(Opcode::UDiv, I8, 0x80, 0xff), Ok(0));
        assert_eq!(int_binary(Opcode::SDiv, I8, 0x81, 0xff), Ok(0x7f));
        assert_eq!(int_binary(Opcode::UDiv, 1, 1, 1), Ok(1));
    }

    /// §5.1: shifts, and the trap at the width.
    #[test]
    fn shifts() {
        assert_eq!(int_binary(Opcode::Shl, I8, 0x81, 1), Ok(2));
        assert_eq!(int_binary(Opcode::LShr, I8, 0x81, 1), Ok(0x40));
        assert_eq!(int_binary(Opcode::AShr, I8, 0x81, 1), Ok(0xc0));
        assert_eq!(int_binary(Opcode::AShr, 7, 0x41, 6), Ok(0x7f));
        assert_eq!(int_binary(Opcode::Shl, I8, 1, 7), Ok(0x80));
        assert_eq!(int_binary(Opcode::Shl, 128, 1, 127), Ok(1 << 127));
        assert_eq!(int_binary(Opcode::AShr, 128, 1 << 127, 127), Ok(u128::MAX));
        for op in [Opcode::Shl, Opcode::LShr, Opcode::AShr] {
            assert_eq!(int_binary(op, I8, 1, 8), Err(Trap::ShiftTooLarge), "{op:?}");
            assert_eq!(int_binary(op, 7, 1, 7), Err(Trap::ShiftTooLarge), "{op:?}");
            assert_eq!(int_binary(op, 1, 1, 1), Err(Trap::ShiftTooLarge), "{op:?}");
            assert_eq!(int_binary(op, 128, 1, 128), Err(Trap::ShiftTooLarge), "{op:?}");
            assert_eq!(int_binary(op, 1, 1, 0), Ok(1), "{op:?}");
        }
    }

    /// §5.2: the ten `icmp` predicates on a value pair that tells signed
    /// from unsigned.
    #[test]
    fn integer_compares() {
        use IntPredicate::{Eq, Ne, Sge, Sgt, Sle, Slt, Uge, Ugt, Ule, Ult};
        // 0xff is 255 unsigned and -1 signed; the other operand is 1.
        let table = [
            (Eq, false, true),
            (Ne, true, false),
            (Ugt, true, false),
            (Uge, true, true),
            (Ult, false, false),
            (Ule, false, true),
            (Sgt, false, false),
            (Sge, false, true),
            (Slt, true, false),
            (Sle, true, true),
        ];
        assert_eq!(table.len(), IntPredicate::ALL.len());
        for (pred, apart, same) in table {
            assert_eq!(icmp(pred, I8, 0xff, 1), apart, "{pred:?}");
            assert_eq!(icmp(pred, I8, 1, 1), same, "{pred:?}");
        }
        assert!(icmp(Slt, 7, 0x40, 0));
        assert!(icmp(Sgt, 128, 0, u128::MAX));
        assert!(icmp(Ult, 64, 1, u128::from(u64::MAX)));
    }

    /// §5.2: the sixteen `fcmp` predicates on less, equal, greater and
    /// unordered operands.
    #[test]
    fn float_compares() {
        use FloatPredicate as P;
        // Columns: 1 < 2, 2 == 2, 3 > 2, NaN ? 2.
        let table = [
            (P::False, [false, false, false, false]),
            (P::Oeq, [false, true, false, false]),
            (P::Ogt, [false, false, true, false]),
            (P::Oge, [false, true, true, false]),
            (P::Olt, [true, false, false, false]),
            (P::Ole, [true, true, false, false]),
            (P::One, [true, false, true, false]),
            (P::Ord, [true, true, true, false]),
            (P::Uno, [false, false, false, true]),
            (P::Ueq, [false, true, false, true]),
            (P::Ugt, [false, false, true, true]),
            (P::Uge, [false, true, true, true]),
            (P::Ult, [true, false, false, true]),
            (P::Ule, [true, true, false, true]),
            (P::Une, [true, false, true, true]),
            (P::True, [true, true, true, true]),
        ];
        assert_eq!(table.len(), FloatPredicate::ALL.len());
        for (pred, expected) in table {
            let got = [1.0, 2.0, 3.0, f64::NAN].map(|a| fcmp(pred, a, 2.0));
            assert_eq!(got, expected, "{pred:?}");
            assert_eq!(fcmp(pred, 2.0, f64::NAN), expected[3], "{pred:?}");
        }
        assert!(fcmp(P::Oeq, 0.0, -0.0));
    }

    /// §5.1: float arithmetic is the host's IEEE arithmetic.
    #[test]
    fn float_arithmetic() {
        assert_eq!(f32_binary(Opcode::FAdd, 0.1, 0.2).to_bits(), 0x3e99_999a);
        assert_eq!(f64_binary(Opcode::FAdd, 0.1, 0.2).to_bits(), 0x3fd3_3333_3333_3334);
        assert_eq!(f64_binary(Opcode::FSub, 1.0, 0.75), 0.25);
        assert_eq!(f64_binary(Opcode::FMul, 1.5, -2.0), -3.0);
        assert_eq!(f32_binary(Opcode::FDiv, 1.0, 3.0).to_bits(), 0x3eaa_aaab);
        assert_eq!(f64_binary(Opcode::FDiv, 1.0, 0.0), f64::INFINITY);
        assert_eq!(f64_binary(Opcode::FRem, 7.5, 2.0), 1.5);
        assert_eq!(f64_binary(Opcode::FRem, -7.5, 2.0), -1.5);
        assert!(f32_binary(Opcode::FRem, 1.0, 0.0).is_nan());
    }

    /// §5.3: float to integer rounds toward zero and traps outside the
    /// result type.
    #[test]
    fn float_to_integer() {
        let over = Err(Trap::FloatToIntOverflow);
        assert_eq!(float_to_int(3.99, 8, false), Ok(3));
        assert_eq!(float_to_int(-3.99, 8, true), Ok(253));
        assert_eq!(float_to_int(-0.99, 8, false), Ok(0));
        assert_eq!(float_to_int(255.9, 8, false), Ok(255));
        assert_eq!(float_to_int(256.0, 8, false), over);
        assert_eq!(float_to_int(-1.0, 8, false), over);
        assert_eq!(float_to_int(127.9, 8, true), Ok(127));
        assert_eq!(float_to_int(128.0, 8, true), over);
        assert_eq!(float_to_int(-128.9, 8, true), Ok(128));
        assert_eq!(float_to_int(-129.0, 8, true), over);
        assert_eq!(float_to_int(f64::NAN, 32, true), over);
        assert_eq!(float_to_int(f64::INFINITY, 128, false), over);
        assert_eq!(float_to_int(1.0, 1, false), Ok(1));
        assert_eq!(float_to_int(-1.0, 1, true), Ok(1));
        assert_eq!(float_to_int(1.0, 1, true), over);
        assert_eq!(float_to_int(2f64.powi(127), 128, false), Ok(1 << 127));
        assert_eq!(float_to_int(2f64.powi(127), 128, true), over);
        assert_eq!(float_to_int(-(2f64.powi(127)), 128, true), Ok(1 << 127));
        assert_eq!(float_to_int(2f64.powi(128), 128, false), over);
    }

    /// §5.6: the bit intrinsics.
    #[test]
    fn bit_intrinsics() {
        let run = |op, bits, a, b, c| int_intrinsic(op, bits, a, b, c);
        assert_eq!(run(IntOp::Abs, I8, 0xfb, 0, 0), Ok(5));
        assert_eq!(run(IntOp::Abs, I8, 0x80, 0, 0), Ok(0x80));
        assert_eq!(run(IntOp::Abs, I8, 0x80, 1, 0), Err(Trap::PoisonIntrinsic));
        assert_eq!(run(IntOp::Abs, 128, 1 << 127, 0, 0), Ok(1 << 127));
        assert_eq!(run(IntOp::Smax, I8, 0xff, 1, 0), Ok(1));
        assert_eq!(run(IntOp::Smin, I8, 0xff, 1, 0), Ok(0xff));
        assert_eq!(run(IntOp::Umax, I8, 0xff, 1, 0), Ok(0xff));
        assert_eq!(run(IntOp::Umin, I8, 0xff, 1, 0), Ok(1));
        assert_eq!(run(IntOp::Ctpop, 33, 0x1_0000_00ff, 0, 0), Ok(9));
        assert_eq!(run(IntOp::Ctlz, I8, 1, 0, 0), Ok(7));
        assert_eq!(run(IntOp::Ctlz, 7, 1, 1, 0), Ok(6));
        assert_eq!(run(IntOp::Ctlz, I8, 0, 0, 0), Ok(8));
        assert_eq!(run(IntOp::Ctlz, I8, 0, 1, 0), Err(Trap::PoisonIntrinsic));
        assert_eq!(run(IntOp::Cttz, I8, 0x50, 0, 0), Ok(4));
        assert_eq!(run(IntOp::Cttz, 7, 0, 0, 0), Ok(7));
        assert_eq!(run(IntOp::Cttz, I8, 0, 1, 0), Err(Trap::PoisonIntrinsic));
        assert_eq!(run(IntOp::Bswap, 16, 0x1234, 0, 0), Ok(0x3412));
        assert_eq!(run(IntOp::Bswap, 48, 0x0102_0304_0506, 0, 0), Ok(0x0605_0403_0201));
        assert_eq!(run(IntOp::BitReverse, I8, 0b1101_0000, 0, 0), Ok(0b0000_1011));
        assert_eq!(run(IntOp::BitReverse, 3, 0b110, 0, 0), Ok(0b011));
        assert_eq!(run(IntOp::Fshl, I8, 0x81, 0x40, 2), Ok(0x05));
        assert_eq!(run(IntOp::Fshr, I8, 0x81, 0x40, 2), Ok(0x50));
        assert_eq!(run(IntOp::Fshl, I8, 0x81, 0x40, 8), Ok(0x81));
        assert_eq!(run(IntOp::Fshr, I8, 0x81, 0x40, 16), Ok(0x40));
        assert_eq!(run(IntOp::Fshl, I8, 0x81, 0x40, 10), Ok(0x05));
        assert_eq!(run(IntOp::Fshl, 128, 1, 1 << 127, 1), Ok(3));
    }

    /// §5.6: saturating arithmetic, the checked family and the three-way
    /// compares.
    #[test]
    fn saturating_checked_and_three_way() {
        let sat = |op, a, b| int_intrinsic(op, I8, a, b, 0).unwrap();
        assert_eq!(sat(IntOp::UaddSat, 200, 100), 255);
        assert_eq!(sat(IntOp::UaddSat, 100, 100), 200);
        assert_eq!(sat(IntOp::UsubSat, 100, 200), 0);
        assert_eq!(sat(IntOp::SaddSat, 100, 100), 127);
        assert_eq!(sat(IntOp::SaddSat, 0x9c, 0x9c), 0x80);
        assert_eq!(sat(IntOp::SaddSat, 100, 0x9c), 0);
        assert_eq!(sat(IntOp::SsubSat, 100, 0x9c), 127);
        assert_eq!(sat(IntOp::SsubSat, 0x9c, 100), 0x80);
        assert_eq!(sat(IntOp::SsubSat, 0, 0x80), 127);
        assert_eq!(int_intrinsic(IntOp::UaddSat, 128, u128::MAX, 1, 0), Ok(u128::MAX));
        assert_eq!(int_intrinsic(IntOp::SaddSat, 128, 1 << 127, 1 << 127, 0), Ok(1 << 127));
        assert_eq!(int_intrinsic(IntOp::SsubSat, 128, 0, 1 << 127, 0), Ok(u128::MAX >> 1));

        assert_eq!(checked(Checked::Sadd, I8, 100, 27), (127, false));
        assert_eq!(checked(Checked::Sadd, I8, 100, 28), (128, true));
        assert_eq!(checked(Checked::Uadd, I8, 200, 55), (255, false));
        assert_eq!(checked(Checked::Uadd, I8, 200, 56), (0, true));
        assert_eq!(checked(Checked::Ssub, I8, 0x80, 1), (127, true));
        assert_eq!(checked(Checked::Ssub, I8, 0, 0x80), (0x80, true));
        assert_eq!(checked(Checked::Usub, I8, 1, 2), (255, true));
        assert_eq!(checked(Checked::Usub, I8, 2, 2), (0, false));
        assert_eq!(checked(Checked::Smul, I8, 16, 8), (128, true));
        assert_eq!(checked(Checked::Smul, I8, 0xf0, 8), (0x80, false));
        assert_eq!(checked(Checked::Umul, I8, 16, 16), (0, true));
        assert_eq!(checked(Checked::Umul, I8, 15, 17), (255, false));
        assert_eq!(checked(Checked::Umul, 128, 1 << 64, 1 << 64), (0, true));
        assert_eq!(checked(Checked::Smul, 128, 1 << 126, 2), (1 << 127, true));
        assert_eq!(checked(Checked::Sadd, 128, u128::MAX, 1), (0, false));
        assert_eq!(checked(Checked::Uadd, 128, u128::MAX, 1), (0, true));
        assert_eq!(checked(Checked::Uadd, 1, 1, 1), (0, true));
        assert_eq!(checked(Checked::Sadd, 1, 1, 1), (0, true));

        assert_eq!(three_way(true, I8, 0xff, 1, 2), 3);
        assert_eq!(three_way(false, I8, 0xff, 1, 2), 1);
        assert_eq!(three_way(true, I8, 4, 4, 32), 0);
        assert_eq!(three_way(false, I8, 1, 2, 8), 0xff);
    }
}
