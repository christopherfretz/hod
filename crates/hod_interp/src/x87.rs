//! `x86_fp80` in software (`spec/interp.md` §5.11): the x87 extended
//! format (1 sign bit, 15 exponent bits with bias 16383, a 64-bit
//! significand whose top bit is the explicit integer bit), with the
//! IEEE-754 operations in the default environment (round to nearest
//! even, full 64-bit precision: the x87 precision control of Linux).
//!
//! Values are bit patterns in the low 80 bits of a `u128`. The
//! encodings the 80387 and later refuse as operands (pseudo-NaN,
//! pseudo-infinity, unnormal: a non-zero exponent without the integer
//! bit) are invalid operands: an operation on one gives the default NaN,
//! as on the hardware. A pseudo-denormal (exponent 0 with the integer
//! bit) is read as the hardware reads it, with exponent 1.
//!
//! The same rounding serves `float` and `double` results of `fptrunc`.

#![allow(
    clippy::many_single_char_names,
    reason = "operands and significands named as in the arithmetic: a, b, x, y, m, e"
)]

use std::cmp::Ordering;

use crate::Trap;

/// The x87 default NaN ("real indefinite"): negative, quiet.
pub const DEFAULT_NAN: u128 = 0xFFFF_C000_0000_0000_0000;

const MASK80: u128 = (1 << 80) - 1;

/// A binary floating-point format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Format {
    /// Significand bits, the integer bit included.
    pub precision: u32,
    /// Exponent bits.
    pub exponent_bits: u32,
    /// Whether the integer bit is stored (x87) or implicit (IEEE).
    pub explicit: bool,
}

impl Format {
    /// `x86_fp80`.
    pub const X87: Format = Format { precision: 64, exponent_bits: 15, explicit: true };
    /// `double`.
    pub const DOUBLE: Format = Format { precision: 53, exponent_bits: 11, explicit: false };
    /// `float`.
    pub const FLOAT: Format = Format { precision: 24, exponent_bits: 8, explicit: false };

    const fn bias(self) -> i32 {
        (1 << (self.exponent_bits - 1)) - 1
    }

    /// Bits of the stored fraction (with the integer bit for x87).
    const fn fraction_bits(self) -> u32 {
        if self.explicit { self.precision } else { self.precision - 1 }
    }

    const fn sign_shift(self) -> u32 {
        self.fraction_bits() + self.exponent_bits
    }

    const fn max_exponent_field(self) -> u128 {
        (1 << self.exponent_bits) - 1
    }

    fn infinity(self, negative: bool) -> u128 {
        let integer = if self.explicit { 1u128 << 63 } else { 0 };
        (u128::from(negative) << self.sign_shift())
            | (self.max_exponent_field() << self.fraction_bits())
            | integer
    }

    fn zero(self, negative: bool) -> u128 {
        u128::from(negative) << self.sign_shift()
    }

    /// The default quiet NaN of the format.
    pub fn nan(self) -> u128 {
        if self.explicit {
            DEFAULT_NAN
        } else {
            (self.max_exponent_field() << self.fraction_bits()) | (1 << (self.fraction_bits() - 1))
        }
    }
}

/// A value taken apart. `Finite` is `(-1)^negative * mant * 2^exp` with
/// `mant != 0`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unpacked {
    Nan,
    Inf(bool),
    Zero(bool),
    Finite { negative: bool, mant: u128, exp: i32 },
}

/// Takes `bits` of `format` apart.
pub fn unpack(format: Format, bits: u128) -> Unpacked {
    let fraction_bits = format.fraction_bits();
    let negative = (bits >> format.sign_shift()) & 1 == 1;
    let field = (bits >> fraction_bits) & format.max_exponent_field();
    let fraction = bits & ((1 << fraction_bits) - 1);
    let exponent_bias = format.bias();
    let lsb = |field: i32| field - exponent_bias - (format.precision as i32 - 1);
    if format.explicit {
        let integer = fraction >> 63 == 1;
        if field == format.max_exponent_field() {
            let rest = fraction & ((1 << 63) - 1);
            return match (integer, rest) {
                (true, 0) => Unpacked::Inf(negative),
                // A NaN, or a pseudo-NaN / pseudo-infinity (invalid).
                _ => Unpacked::Nan,
            };
        }
        if field == 0 {
            if fraction == 0 {
                return Unpacked::Zero(negative);
            }
            // A denormal, or a pseudo-denormal (read with exponent 1).
            return Unpacked::Finite { negative, mant: fraction, exp: lsb(1) };
        }
        if !integer {
            // An unnormal: an invalid operand.
            return Unpacked::Nan;
        }
        return Unpacked::Finite { negative, mant: fraction, exp: lsb(field as i32) };
    }
    if field == format.max_exponent_field() {
        return if fraction == 0 { Unpacked::Inf(negative) } else { Unpacked::Nan };
    }
    if field == 0 {
        if fraction == 0 {
            return Unpacked::Zero(negative);
        }
        return Unpacked::Finite { negative, mant: fraction, exp: lsb(1) };
    }
    let mant = fraction | (1 << fraction_bits);
    Unpacked::Finite { negative, mant, exp: lsb(field as i32) }
}

/// Rounds `(-1)^negative * (mant + sticky) * 2^exp` to `format`, to
/// nearest even; `sticky` stands for bits below `mant`'s lowest that are
/// not all zero.
pub fn round(format: Format, negative: bool, mant: u128, exp: i32, sticky: bool) -> u128 {
    if mant == 0 {
        // A value in (0, 2^exp) with `sticky`: far below any rounding
        // position the callers leave, so it rounds to zero.
        return format.zero(negative);
    }
    let p = format.precision as i32;
    let emin = 1 - format.bias();
    let length = 128 - mant.leading_zeros() as i32;
    // The exponent of the leading bit, and of the result's lowest bit.
    let top = exp + length - 1;
    let lsb = (top - p + 1).max(emin - p + 1);
    let shift = lsb - exp;
    let (mut kept, mut lsb) = if shift <= 0 {
        // Exact: the dropped part is `sticky` only, below half an ulp.
        let shift = (-shift) as u32;
        (mant << shift, lsb)
    } else if shift >= 129 {
        (0, lsb)
    } else {
        let shift = shift as u32;
        let kept = if shift == 128 { 0 } else { mant >> shift };
        let dropped = if shift == 128 { mant } else { mant & ((1u128 << shift) - 1) };
        let half = 1u128 << (shift - 1);
        let up = dropped > half || (dropped == half && (sticky || kept & 1 == 1));
        (kept + u128::from(up), lsb)
    };
    if kept >> p != 0 {
        kept >>= 1;
        lsb += 1;
    }
    if kept == 0 {
        return format.zero(negative);
    }
    let sign = u128::from(negative) << format.sign_shift();
    if kept >> (p - 1) == 1 {
        let biased = lsb + p - 1 + format.bias();
        if biased as u128 >= format.max_exponent_field() {
            return format.infinity(negative);
        }
        let fraction = if format.explicit { kept } else { kept & ((1 << (p - 1)) - 1) };
        return sign | ((biased as u128) << format.fraction_bits()) | fraction;
    }
    // A denormal: exponent field 0, integer bit clear.
    sign | kept
}

/// What an operation gives: the bits, and whether they are a NaN the
/// operation computed (`spec/interp.md` §5.1).
fn nan() -> u128 {
    DEFAULT_NAN
}

/// `fadd`, `fsub`, `fmul`, `fdiv`, `frem` on two `x86_fp80` values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

/// `a op b`; a NaN result is [`DEFAULT_NAN`] (or a NaN operand made
/// quiet).
pub fn binary(op: Op, a: u128, b: u128) -> u128 {
    let (a, b) = (a & MASK80, b & MASK80);
    let f = Format::X87;
    let (x, y) = (unpack(f, a), unpack(f, b));
    if let Some(quiet) = propagate(a, x, b, y) {
        return quiet;
    }
    let y = match (op, y) {
        (Op::Sub, Unpacked::Inf(n)) => Unpacked::Inf(!n),
        (Op::Sub, Unpacked::Zero(n)) => Unpacked::Zero(!n),
        (Op::Sub, Unpacked::Finite { negative, mant, exp }) => {
            Unpacked::Finite { negative: !negative, mant, exp }
        }
        (_, y) => y,
    };
    match op {
        Op::Add | Op::Sub => add(x, y),
        Op::Mul => mul(x, y),
        Op::Div => div(x, y),
        Op::Rem => rem(x, y),
    }
}

/// A NaN operand, made quiet: the result of an operation on it.
fn propagate(a: u128, x: Unpacked, b: u128, y: Unpacked) -> Option<u128> {
    let quiet = |bits: u128| {
        let field = (bits >> 64) & 0x7fff;
        // A genuine NaN (integer bit set) propagates, quieted; an invalid
        // encoding gives the default NaN.
        if field == 0x7fff && (bits >> 63) & 1 == 1 { bits | (1 << 62) } else { nan() }
    };
    match (x, y) {
        (Unpacked::Nan, Unpacked::Nan) => {
            // x87: the NaN with the larger significand.
            let (qa, qb) = (quiet(a), quiet(b));
            Some(if (qa as u64) >= (qb as u64) { qa } else { qb })
        }
        (Unpacked::Nan, _) => Some(quiet(a)),
        (_, Unpacked::Nan) => Some(quiet(b)),
        _ => None,
    }
}

fn add(x: Unpacked, y: Unpacked) -> u128 {
    let f = Format::X87;
    match (x, y) {
        (Unpacked::Inf(n), Unpacked::Inf(m)) => {
            if n == m {
                f.infinity(n)
            } else {
                nan()
            }
        }
        (Unpacked::Inf(n), _) | (_, Unpacked::Inf(n)) => f.infinity(n),
        (Unpacked::Zero(n), Unpacked::Zero(m)) => f.zero(n && m),
        (Unpacked::Zero(_), Unpacked::Finite { negative, mant, exp })
        | (Unpacked::Finite { negative, mant, exp }, Unpacked::Zero(_)) => {
            round(f, negative, mant, exp, false)
        }
        (
            Unpacked::Finite { negative: na, mant: ma, exp: ea },
            Unpacked::Finite { negative: nb, mant: mb, exp: eb },
        ) => {
            // 62 guard bits below a 64-bit significand.
            let (mut na, mut ma, mut ea) = (na, ma << 62, ea - 62);
            let (mut nb, mut mb, mut eb) = (nb, mb << 62, eb - 62);
            if ea < eb {
                (na, ma, ea, nb, mb, eb) = (nb, mb, eb, na, ma, ea);
            }
            let shift = (ea - eb) as u32;
            let mb = if shift == 0 {
                mb
            } else if shift >= 127 {
                u128::from(mb != 0)
            } else {
                (mb >> shift) | u128::from(mb & ((1 << shift) - 1) != 0)
            };
            if na == nb {
                return round(f, na, ma + mb, ea, false);
            }
            match ma.cmp(&mb) {
                Ordering::Equal => f.zero(false),
                Ordering::Greater => round(f, na, ma - mb, ea, false),
                Ordering::Less => round(f, nb, mb - ma, ea, false),
            }
        }
        (Unpacked::Nan, _) | (_, Unpacked::Nan) => nan(),
    }
}

fn mul(x: Unpacked, y: Unpacked) -> u128 {
    let f = Format::X87;
    match (x, y) {
        (Unpacked::Inf(_), Unpacked::Zero(_)) | (Unpacked::Zero(_), Unpacked::Inf(_)) => nan(),
        (Unpacked::Inf(n), y) => f.infinity(n ^ sign(y)),
        (x, Unpacked::Inf(n)) => f.infinity(n ^ sign(x)),
        (Unpacked::Zero(n), y) => f.zero(n ^ sign(y)),
        (x, Unpacked::Zero(n)) => f.zero(n ^ sign(x)),
        (
            Unpacked::Finite { negative: na, mant: ma, exp: ea },
            Unpacked::Finite { negative: nb, mant: mb, exp: eb },
        ) => round(f, na ^ nb, ma * mb, ea + eb, false),
        _ => nan(),
    }
}

fn sign(x: Unpacked) -> bool {
    match x {
        Unpacked::Inf(n) | Unpacked::Zero(n) | Unpacked::Finite { negative: n, .. } => n,
        Unpacked::Nan => false,
    }
}

/// `mant` with its leading bit at bit 63, and the exponent adjusted.
fn normalize64(mant: u128, exp: i32) -> (u128, i32) {
    let length = 128 - mant.leading_zeros() as i32;
    let shift = 64 - length;
    if shift >= 0 { (mant << shift, exp - shift) } else { (mant >> -shift, exp - shift) }
}

fn div(x: Unpacked, y: Unpacked) -> u128 {
    let f = Format::X87;
    match (x, y) {
        (Unpacked::Inf(_), Unpacked::Inf(_)) | (Unpacked::Zero(_), Unpacked::Zero(_)) => nan(),
        (Unpacked::Inf(n), y) => f.infinity(n ^ sign(y)),
        (x, Unpacked::Inf(n)) => f.zero(n ^ sign(x)),
        (Unpacked::Zero(n), y) => f.zero(n ^ sign(y)),
        (x, Unpacked::Zero(n)) => f.infinity(n ^ sign(x)),
        (
            Unpacked::Finite { negative: na, mant: ma, exp: ea },
            Unpacked::Finite { negative: nb, mant: mb, exp: eb },
        ) => {
            // Both significands below 2^64 (they come from 80-bit
            // values), so normalizing loses nothing.
            let (ma, ea) = normalize64(ma, ea);
            let (mb, eb) = normalize64(mb, eb);
            let wide = ma << 64;
            let (q, r) = (wide / mb, wide % mb);
            let (q2, r2) = ((r << 8) / mb, (r << 8) % mb);
            round(f, na ^ nb, (q << 8) | q2, ea - eb - 72, r2 != 0)
        }
        _ => nan(),
    }
}

/// C's `fmod`: exact.
fn rem(x: Unpacked, y: Unpacked) -> u128 {
    let f = Format::X87;
    match (x, y) {
        (Unpacked::Inf(_), _) | (_, Unpacked::Zero(_)) => nan(),
        (Unpacked::Zero(n), _) => f.zero(n),
        (Unpacked::Finite { negative, mant, exp }, Unpacked::Inf(_)) => {
            round(f, negative, mant, exp, false)
        }
        (
            Unpacked::Finite { negative, mant: ma, exp: ea },
            Unpacked::Finite { mant: mb, exp: eb, .. },
        ) => {
            if magnitude_cmp(ma, ea, mb, eb) == Ordering::Less {
                return round(f, negative, ma, ea, false);
            }
            let (r, e) = if ea >= eb {
                // (ma * 2^(ea-eb)) mod mb, 64 bits at a time; mb < 2^64.
                let mut r = ma % mb;
                let mut left = (ea - eb) as u32;
                while left > 0 {
                    let step = left.min(64);
                    r = (r << step) % mb;
                    left -= step;
                }
                (r, eb)
            } else {
                // |x| >= |y| with ea < eb: mb * 2^(eb-ea) <= ma < 2^64.
                (ma % (mb << (eb - ea) as u32), ea)
            };
            if r == 0 { f.zero(negative) } else { round(f, negative, r, e, false) }
        }
        _ => nan(),
    }
}

fn magnitude_cmp(ma: u128, ea: i32, mb: u128, eb: i32) -> Ordering {
    let (la, lb) = (128 - ma.leading_zeros() as i32, 128 - mb.leading_zeros() as i32);
    match (ea + la).cmp(&(eb + lb)) {
        Ordering::Equal => {}
        other => return other,
    }
    // Same leading exponent: align and compare.
    let (ma, mb) = (ma << (128 - la), mb << (128 - lb));
    ma.cmp(&mb)
}

/// The order of two values; `None` when either is a NaN. `-0` equals `+0`.
pub fn compare(format: Format, a: u128, b: u128) -> Option<Ordering> {
    let key = |x: Unpacked| -> Option<(i8, u128, i32)> {
        match x {
            Unpacked::Nan => None,
            Unpacked::Zero(_) => Some((0, 0, 0)),
            Unpacked::Inf(n) => Some((if n { -2 } else { 2 }, 0, 0)),
            Unpacked::Finite { negative, mant, exp } => {
                Some((if negative { -1 } else { 1 }, mant, exp))
            }
        }
    };
    let (x, y) = (key(unpack(format, a))?, key(unpack(format, b))?);
    Some(match x.0.cmp(&y.0) {
        Ordering::Equal if x.0 == 1 => magnitude_cmp(x.1, x.2, y.1, y.2),
        Ordering::Equal if x.0 == -1 => magnitude_cmp(y.1, y.2, x.1, x.2),
        other => other,
    })
}

/// `fpext`/`fptrunc`: `bits` of `from` to `to`, rounded to nearest even.
/// A NaN gives `to`'s default NaN.
pub fn convert(from: Format, to: Format, bits: u128) -> u128 {
    match unpack(from, bits) {
        Unpacked::Nan => to.nan(),
        Unpacked::Inf(n) => to.infinity(n),
        Unpacked::Zero(n) => to.zero(n),
        Unpacked::Finite { negative, mant, exp } => round(to, negative, mant, exp, false),
    }
}

/// `sitofp`/`uitofp` of the `bits`-bit integer `value` to `format`.
pub fn from_int(format: Format, value: u128, bits: u32, signed: bool) -> u128 {
    let negative = signed && bits > 0 && (value >> (bits - 1)) & 1 == 1;
    let magnitude = if negative {
        let mask = if bits == 128 { u128::MAX } else { (1 << bits) - 1 };
        (!value).wrapping_add(1) & mask
    } else {
        value
    };
    if magnitude == 0 {
        return format.zero(false);
    }
    round(format, negative, magnitude, 0, false)
}

/// `fptosi`/`fptoui` of `bits` of `format` to a `width`-bit integer,
/// rounded toward zero.
///
/// # Errors
/// [`Trap::FloatToIntOverflow`] for a NaN, an infinity, or a value that
/// does not fit.
pub fn to_int(format: Format, bits: u128, width: u32, signed: bool) -> Result<u128, Trap> {
    let (negative, magnitude) = match unpack(format, bits) {
        Unpacked::Nan | Unpacked::Inf(_) => return Err(Trap::FloatToIntOverflow),
        Unpacked::Zero(_) => return Ok(0),
        Unpacked::Finite { negative, mant, exp } => {
            let length = 128 - mant.leading_zeros() as i32;
            let magnitude = if exp >= 0 {
                if length + exp > 128 {
                    return Err(Trap::FloatToIntOverflow);
                }
                mant << exp
            } else if -exp >= 128 {
                0
            } else {
                mant >> -exp
            };
            (negative, magnitude)
        }
    };
    let mask = if width == 128 { u128::MAX } else { (1 << width) - 1 };
    if signed {
        let limit = 1u128 << (width - 1);
        if (negative && magnitude > limit) || (!negative && magnitude >= limit) {
            return Err(Trap::FloatToIntOverflow);
        }
        Ok(if negative { (!magnitude).wrapping_add(1) & mask } else { magnitude })
    } else {
        if (negative && magnitude != 0) || magnitude > mask {
            return Err(Trap::FloatToIntOverflow);
        }
        Ok(magnitude)
    }
}

/// Whether `bits` is a NaN of `format` (an invalid x87 encoding counts).
pub fn is_nan(format: Format, bits: u128) -> bool {
    unpack(format, bits) == Unpacked::Nan
}

/// Whether `bits` is a signaling NaN of `format`: a NaN whose top
/// fraction bit (below the x87's integer bit) is clear. An invalid x87
/// encoding counts as signaling: it is an invalid operand like one.
pub fn is_signaling(format: Format, bits: u128) -> bool {
    if !is_nan(format, bits) {
        return false;
    }
    let quiet_bit = if format.explicit { 62 } else { format.precision - 2 };
    let integer_missing = format.explicit && (bits >> 63) & 1 == 0;
    integer_missing || (bits >> quiet_bit) & 1 == 0
}

/// `llvm.minnum` (`max` false) or `llvm.maxnum` (`max` true) on bit
/// patterns of `format` (`spec/interp.md` §5.6): a quiet NaN operand
/// gives the other operand.
///
/// # Errors
/// Where LLVM leaves the result open: [`Trap::NanResult`] for a signaling
/// NaN operand (unless `host_nan`, which treats it as quiet) or for two
/// NaNs (unless `host_nan`, which gives the default NaN), and
/// [`Trap::ZeroSign`] for two zeros of opposite signs.
pub fn min_max(format: Format, a: u128, b: u128, max: bool, host_nan: bool) -> Result<u128, Trap> {
    if !host_nan && (is_signaling(format, a) || is_signaling(format, b)) {
        return Err(Trap::NanResult);
    }
    match (is_nan(format, a), is_nan(format, b)) {
        (true, true) if host_nan => return Ok(format.nan()),
        (true, true) => return Err(Trap::NanResult),
        (true, false) => return Ok(b),
        (false, true) => return Ok(a),
        (false, false) => {}
    }
    match compare(format, a, b) {
        Some(Ordering::Equal) if a != b => Err(Trap::ZeroSign),
        Some(Ordering::Less) => Ok(if max { b } else { a }),
        Some(Ordering::Greater) => Ok(if max { a } else { b }),
        _ => Ok(a),
    }
}

/// The class `llvm.is.fpclass` tests (LangRef "'llvm.is.fpclass'
/// Intrinsic"): the index of its bit in the test mask, 0 (signaling NaN)
/// to 9 (positive infinity). `None` for the encodings only the x87 has,
/// on whose class LLVM's lowerings do not agree (unnormals, pseudo-NaNs,
/// pseudo-infinities, pseudo-denormals).
pub fn fp_class(format: Format, bits: u128) -> Option<u32> {
    let negative = (bits >> format.sign_shift()) & 1 == 1;
    let field = (bits >> format.fraction_bits()) & format.max_exponent_field();
    let fraction = bits & ((1 << format.fraction_bits()) - 1);
    let side = |negative_index: u32, positive_index: u32| {
        Some(if negative { negative_index } else { positive_index })
    };
    if format.explicit {
        let integer = fraction >> 63 == 1;
        let normal = field != 0 && field != format.max_exponent_field();
        if (field == 0 && integer) || (field != 0 && !integer) {
            return None;
        }
        if normal {
            return side(3, 8);
        }
    }
    let rest = if format.explicit { fraction & ((1 << 63) - 1) } else { fraction };
    if field == format.max_exponent_field() {
        if rest == 0 {
            return side(2, 9);
        }
        return Some(u32::from(!is_signaling(format, bits)));
    }
    match (field, rest) {
        (0, 0) => side(5, 6),
        (0, _) => side(4, 7),
        _ => side(3, 8),
    }
}

/// A NaN `x87` operand of a unary operation, made quiet.
fn propagate1(a: u128, x: Unpacked) -> Option<u128> {
    propagate(a, x, a, x)
}

/// `llvm.sqrt` on `x86_fp80`: correctly rounded, as `fsqrt`.
pub fn sqrt(a: u128) -> u128 {
    let a = a & MASK80;
    let f = Format::X87;
    let x = unpack(f, a);
    if let Some(quiet) = propagate1(a, x) {
        return quiet;
    }
    match x {
        Unpacked::Zero(n) => f.zero(n),
        Unpacked::Inf(false) => f.infinity(false),
        Unpacked::Finite { negative: false, mant, exp } => {
            let (mut mant, mut exp) = normalize64(mant, exp);
            if exp.rem_euclid(2) == 1 {
                mant <<= 1;
                exp -= 1;
            }
            // The radicand is `mant * 4^K`: its root has at least 66 bits,
            // two more than the result keeps, and the remainder is sticky.
            const K: u32 = 35;
            let (root, inexact) = isqrt_scaled(mant, K);
            round(f, false, root, exp / 2 - K as i32, inexact)
        }
        _ => nan(),
    }
}

/// The integer square root of `mant * 4^k` (`mant < 2^66`, `k < 40`), and
/// whether a remainder is left: digit by digit, two radicand bits a step.
fn isqrt_scaled(mant: u128, k: u32) -> (u128, bool) {
    let bits = 128 - mant.leading_zeros() + 2 * k;
    let (mut rest, mut root) = (0u128, 0u128);
    for i in (0..bits.div_ceil(2)).rev() {
        let at = 2 * i;
        let pair = if at >= 2 * k { (mant >> (at - 2 * k)) & 3 } else { 0 };
        rest = (rest << 2) | pair;
        let trial = (root << 2) | 1;
        if rest >= trial {
            rest -= trial;
            root = (root << 1) | 1;
        } else {
            root <<= 1;
        }
    }
    (root, rest != 0)
}

/// How [`round_integral`] rounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rounding {
    /// Toward negative infinity: `floor`.
    Floor,
    /// Toward positive infinity: `ceil`.
    Ceil,
    /// Toward zero: `trunc`.
    Trunc,
    /// To nearest, ties to even: `roundeven`, and `rint`/`nearbyint` in
    /// the default rounding mode.
    NearestEven,
    /// To nearest, ties away from zero: `round`.
    NearestAway,
}

/// `x86_fp80` rounded to an integral value; zeros and infinities are
/// unchanged, a result of zero keeps the operand's sign.
pub fn round_integral(a: u128, mode: Rounding) -> u128 {
    let a = a & MASK80;
    let f = Format::X87;
    let x = unpack(f, a);
    if let Some(quiet) = propagate1(a, x) {
        return quiet;
    }
    let Unpacked::Finite { negative, mant, exp } = x else {
        return a;
    };
    if exp >= 0 {
        return round(f, negative, mant, exp, false);
    }
    let shift = exp.unsigned_abs();
    // `mant < 2^64`: past 66 bits everything is dropped and below a half.
    let (int, dropped, half) = if shift > 66 {
        (0, mant, u128::MAX)
    } else {
        (mant >> shift, mant & ((1 << shift) - 1), 1u128 << (shift - 1))
    };
    let up = match mode {
        Rounding::Floor => negative && dropped != 0,
        Rounding::Ceil => !negative && dropped != 0,
        Rounding::Trunc => false,
        Rounding::NearestEven => dropped > half || (dropped == half && int & 1 == 1),
        Rounding::NearestAway => dropped >= half,
    };
    let int = int + u128::from(up);
    if int == 0 { f.zero(negative) } else { round(f, negative, int, 0, false) }
}

/// A 256-bit unsigned integer, for the exact sum inside [`fma`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Wide {
    hi: u128,
    lo: u128,
}

impl Wide {
    /// `m << s`, `s < 256` and the result fitting.
    fn shifted(m: u128, s: u32) -> Wide {
        match s {
            0 => Wide { hi: 0, lo: m },
            1..128 => Wide { hi: m >> (128 - s), lo: m << s },
            _ => Wide { hi: m << (s - 128), lo: 0 },
        }
    }

    fn add(self, other: Wide) -> Wide {
        let (lo, carry) = self.lo.overflowing_add(other.lo);
        Wide { hi: self.hi + other.hi + u128::from(carry), lo }
    }

    /// `self - other`, `self >= other`.
    fn sub(self, other: Wide) -> Wide {
        let (lo, borrow) = self.lo.overflowing_sub(other.lo);
        Wide { hi: self.hi - other.hi - u128::from(borrow), lo }
    }

    fn bits(self) -> u32 {
        if self.hi != 0 { 256 - self.hi.leading_zeros() } else { 128 - self.lo.leading_zeros() }
    }
}

/// `llvm.fma` on `x86_fp80`: `a * b + c` with one rounding.
pub fn fma(a: u128, b: u128, c: u128) -> u128 {
    let (a, b, c) = (a & MASK80, b & MASK80, c & MASK80);
    let f = Format::X87;
    let (x, y, z) = (unpack(f, a), unpack(f, b), unpack(f, c));
    if let Some(quiet) = propagate(a, x, b, y) {
        return quiet;
    }
    if let Some(quiet) = propagate1(c, z) {
        return quiet;
    }
    // The exact product: `None` for infinity times zero.
    let product = match (x, y) {
        (Unpacked::Inf(_), Unpacked::Zero(_)) | (Unpacked::Zero(_), Unpacked::Inf(_)) => None,
        (Unpacked::Inf(n), other) | (other, Unpacked::Inf(n)) => Some(Unpacked::Inf(n ^ sign(other))),
        (Unpacked::Zero(n), other) | (other, Unpacked::Zero(n)) => {
            Some(Unpacked::Zero(n ^ sign(other)))
        }
        (
            Unpacked::Finite { negative: na, mant: ma, exp: ea },
            Unpacked::Finite { negative: nb, mant: mb, exp: eb },
        ) => Some(Unpacked::Finite { negative: na ^ nb, mant: ma * mb, exp: ea + eb }),
        _ => None,
    };
    let Some(product) = product else { return nan() };
    match (product, z) {
        (Unpacked::Inf(n), Unpacked::Inf(m)) if n != m => nan(),
        (Unpacked::Inf(n), _) | (_, Unpacked::Inf(n)) => f.infinity(n),
        (Unpacked::Zero(n), Unpacked::Zero(m)) => f.zero(n && m),
        (Unpacked::Zero(_), Unpacked::Finite { negative, mant, exp })
        | (Unpacked::Finite { negative, mant, exp }, Unpacked::Zero(_)) => {
            round(f, negative, mant, exp, false)
        }
        (
            Unpacked::Finite { negative: np, mant: mp, exp: ep },
            Unpacked::Finite { negative: nz, mant: mz, exp: ez },
        ) => exact_sum((np, mp, ep), (nz, mz, ez)),
        _ => nan(),
    }
}

/// `(-1)^n1 * m1 * 2^e1 + (-1)^n2 * m2 * 2^e2`, rounded once; both
/// significands nonzero and below 2^128.
fn exact_sum(p: (bool, u128, i32), q: (bool, u128, i32)) -> u128 {
    let top = |(_, m, e): (bool, u128, i32)| e + (128 - m.leading_zeros()) as i32;
    // A window of 250 bits below the higher top. The operand with that top
    // fits in it; the other either fits or is so far below that its
    // truncated bits only matter as a sticky bit.
    let low = top(p).max(top(q)) - 250;
    let place = |(_, m, e): (bool, u128, i32)| -> (Wide, bool) {
        if e >= low {
            (Wide::shifted(m, (e - low) as u32), false)
        } else {
            let s = (low - e) as u32;
            if s >= 128 { (Wide::shifted(0, 0), true) } else { (Wide::shifted(m >> s, 0), m & ((1 << s) - 1) != 0) }
        }
    };
    let ((wp, sp), (wq, sq)) = (place(p), place(q));
    let (negative, sum, sticky) = if p.0 == q.0 {
        (p.0, wp.add(wq), sp || sq)
    } else {
        // Larger minus smaller; at most the smaller one lost bits, and
        // `big - (small + e)`, `0 < e < 1`, is `(big - small - 1) + (1 - e)`.
        let ((nb, wb, _), (ws, ss)) =
            if (wp, sp) >= (wq, sq) { ((p.0, wp, sp), (wq, sq)) } else { ((q.0, wq, sq), (wp, sp)) };
        let diff = wb.sub(ws);
        if ss { (nb, diff.sub(Wide::shifted(1, 0)), true) } else { (nb, diff, false) }
    };
    if sum == Wide::shifted(0, 0) && !sticky {
        return Format::X87.zero(false);
    }
    let bits = sum.bits();
    let (mant, exp, sticky) = if bits > 128 {
        let s = bits - 128;
        let lost = sum.lo & ((1u128 << s) - 1) != 0;
        let mant = (sum.lo >> s) | (sum.hi << (128 - s));
        (mant, low + s as i32, sticky || lost)
    } else {
        (sum.lo, low, sticky)
    };
    round(Format::X87, negative, mant, exp, sticky)
}

#[cfg(test)]
#[allow(
    clippy::float_cmp,
    clippy::cast_lossless,
    clippy::cast_precision_loss,
    reason = "exact comparisons with the host's own arithmetic"
)]
mod tests {
    use super::*;

    const ONE: u128 = 0x3FFF_8000_0000_0000_0000;
    const TWO: u128 = 0x4000_8000_0000_0000_0000;
    const THREE: u128 = 0x4000_C000_0000_0000_0000;

    fn d(x: f64) -> u128 {
        convert(Format::DOUBLE, Format::X87, u128::from(x.to_bits()))
    }

    fn back(x: u128) -> f64 {
        f64::from_bits(convert(Format::X87, Format::DOUBLE, x) as u64)
    }

    #[test]
    fn small_exact_values() {
        assert_eq!(d(1.0), ONE);
        assert_eq!(d(3.0), THREE);
        assert_eq!(binary(Op::Add, ONE, TWO), THREE);
        assert_eq!(binary(Op::Sub, ONE, ONE), 0);
        assert_eq!(binary(Op::Mul, THREE, TWO), d(6.0));
        assert_eq!(binary(Op::Div, THREE, TWO), d(1.5));
        assert_eq!(binary(Op::Rem, d(7.5), TWO), d(1.5));
        assert_eq!(binary(Op::Rem, d(-7.5), TWO), d(-1.5));
        assert_eq!(from_int(Format::X87, (-3i64) as u64 as u128, 64, true), d(-3.0));
        assert_eq!(to_int(Format::X87, d(-3.75), 32, true), Ok((-3i32) as u32 as u128));
        assert_eq!(to_int(Format::X87, d(-0.5), 32, false), Ok(0));
        assert_eq!(to_int(Format::X87, d(-1.0), 32, false), Err(Trap::FloatToIntOverflow));
        assert_eq!(compare(Format::X87, d(-0.0), d(0.0)), Some(Ordering::Equal));
        assert_eq!(compare(Format::X87, d(-2.0), d(-1.0)), Some(Ordering::Less));
        assert_eq!(compare(Format::X87, DEFAULT_NAN, ONE), None);
    }

    /// Double arithmetic is exact in the extended format for sums and
    /// products of small integers, and a third rounds at bit 64.
    #[test]
    fn a_third() {
        let third = binary(Op::Div, ONE, THREE);
        assert_eq!(third, 0x3FFD_AAAA_AAAA_AAAA_AAAB);
        assert_eq!(back(third), 1.0 / 3.0);
        // 1 + 2^-63 is representable; 1 + 2^-64 ties to even (1).
        let tiny = round(Format::X87, false, 1, -63, false);
        assert_eq!(binary(Op::Add, ONE, tiny), ONE | 1);
        let tinier = round(Format::X87, false, 1, -64, false);
        assert_eq!(binary(Op::Add, ONE, tinier), ONE);
        assert_eq!(binary(Op::Add, ONE | 1, tinier), ONE | 2);
    }

    #[test]
    fn specials() {
        let inf = Format::X87.infinity(false);
        assert_eq!(binary(Op::Div, ONE, 0), inf);
        assert_eq!(binary(Op::Sub, inf, inf), DEFAULT_NAN);
        assert_eq!(binary(Op::Mul, inf, 0), DEFAULT_NAN);
        assert!(is_nan(Format::X87, binary(Op::Rem, ONE, 0)));
        // An unnormal is an invalid operand.
        assert_eq!(binary(Op::Add, 0x3FFF_0000_0000_0000_0001, ONE), DEFAULT_NAN);
        // Overflow and the smallest denormal.
        let max = 0x7FFE_FFFF_FFFF_FFFF_FFFF;
        assert_eq!(binary(Op::Add, max, max), inf);
        assert_eq!(binary(Op::Div, 1, TWO), 0);
        assert_eq!(binary(Op::Div, 3, TWO), 2);
        assert_eq!(convert(Format::X87, Format::DOUBLE, 1), 0);
        assert_eq!(convert(Format::X87, Format::FLOAT, inf), u128::from(f32::INFINITY.to_bits()));
    }

    /// `fptrunc` agrees with the host's `f64 -> f32` on doubles, and the
    /// round trip through `x86_fp80` is the identity.
    #[test]
    fn conversions_agree_with_the_host() {
        let mut x = 0x9E37_79B9_7F4A_7C15u64;
        for _ in 0..20_000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let v = f64::from_bits(x);
            if v.is_nan() {
                continue;
            }
            let wide = d(v);
            assert_eq!(back(wide).to_bits(), v.to_bits(), "{v:e}");
            let narrow = convert(Format::DOUBLE, Format::FLOAT, u128::from(x)) as u32;
            assert_eq!(narrow, (v as f32).to_bits(), "{v:e}");
            let i = x as i64;
            assert_eq!(back(from_int(Format::X87, u128::from(x), 64, true)), i as f64);
            if v.is_finite() && v.abs() < 9.0e18 {
                assert_eq!(to_int(Format::X87, wide, 64, true), Ok(v as i64 as u64 as u128));
            }
        }
    }
}
