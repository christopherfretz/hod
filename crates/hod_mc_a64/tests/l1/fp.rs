//! Sweeps for `spec/mc-a64.md` "Scalar floating point".

use hod_mc_a64::*;

use crate::harness::Batch;
use crate::text::{CONDS, FP_FORMATS, FP_SIZES, fp_bits, fp_imm8_value, rz, tuples, v, z};

/// One-, two- and three-source data processing. The constructors are named
/// by their mnemonics.
fn data(b: &mut Batch) {
    macro_rules! one {
        ($($f:ident)*) => {$(
            for (sz, p) in FP_SIZES {
                for [d, n] in tuples([32, 32]) {
                    emit!(b, $f(sz, v(d), v(n)), "{} {p}{d}, {p}{n}", stringify!($f));
                }
            }
        )*};
    }
    one!(fmov fabs fneg fsqrt frintn frintp frintm frintz frinta frintx frinti);

    macro_rules! two {
        ($($f:ident)*) => {$(
            for (sz, p) in FP_SIZES {
                for [d, n, m] in tuples([32, 32, 32]) {
                    emit!(
                        b,
                        $f(sz, v(d), v(n), v(m)),
                        "{} {p}{d}, {p}{n}, {p}{m}", stringify!($f)
                    );
                }
            }
        )*};
    }
    two!(fmul fdiv fadd fsub fmax fmin fmaxnm fminnm fnmul);

    macro_rules! three {
        ($($f:ident)*) => {$(
            for (sz, p) in FP_SIZES {
                for [d, n, m, a] in tuples([32, 32, 32, 32]) {
                    emit!(
                        b,
                        $f(sz, v(d), v(n), v(m), v(a)),
                        "{} {p}{d}, {p}{n}, {p}{m}, {p}{a}", stringify!($f)
                    );
                }
            }
        )*};
    }
    three!(fmadd fmsub fnmadd fnmsub);
}

fn compare_and_select(b: &mut Batch) {
    for (sz, p) in FP_SIZES {
        for [n, m] in tuples([32, 32]) {
            emit!(b, fcmp(sz, v(n), v(m)), "fcmp {p}{n}, {p}{m}");
            emit!(b, fcmpe(sz, v(n), v(m)), "fcmpe {p}{n}, {p}{m}");
        }
        for n in 0..32 {
            emit!(b, fcmp_zero(sz, v(n)), "fcmp {p}{n}, #0.0");
            emit!(b, fcmpe_zero(sz, v(n)), "fcmpe {p}{n}, #0.0");
        }
        for (i, [d, n, m]) in tuples([32, 32, 32]).into_iter().enumerate() {
            let (cond, name) = CONDS[i % 16];
            emit!(b, fcsel(sz, v(d), v(n), v(m), cond), "fcsel {p}{d}, {p}{n}, {p}{m}, {name}");
        }
        for (cond, name) in CONDS {
            emit!(b, fcsel(sz, v(5), v(6), v(7), cond), "fcsel {p}5, {p}6, {p}7, {name}");
        }
    }

    macro_rules! conditional {
        ($($f:ident $mn:literal;)*) => {$(
            for (sz, p) in FP_SIZES {
                for (i, [n, m]) in tuples([32, 32]).into_iter().enumerate() {
                    let (cond, name) = CONDS[i % 16];
                    let nzcv = (i * 7 % 16) as u32;
                    emit!(
                        b,
                        $f(sz, v(n), v(m), nzcv, cond),
                        "{} {p}{n}, {p}{m}, #{nzcv}, {name}", $mn
                    );
                }
                for (cond, name) in CONDS {
                    for nzcv in 0..16 {
                        emit!(
                            b,
                            $f(sz, v(6), v(7), nzcv, cond),
                            "{} {p}6, {p}7, #{nzcv}, {name}", $mn
                        );
                    }
                }
                reject!(b, $f(sz, v(6), v(7), 16, Cond::Eq), "{} {p}6, {p}7, #16, eq", $mn);
                reject!(b, $f(sz, v(6), v(7), u32::MAX, Cond::Eq));
            }
        )*};
    }
    conditional! {
        try_fccmp "fccmp";
        try_fccmpe "fccmpe";
    }
}

/// FCVT between every two different formats; the same format twice is not
/// an instruction.
fn formats(b: &mut Batch) {
    for (to, tp) in FP_FORMATS {
        for (from, fp) in FP_FORMATS {
            if to == from {
                reject!(b, try_fcvt(to, v(5), from, v(6)), "fcvt {tp}5, {fp}6");
                continue;
            }
            for [d, n] in tuples([32, 32]) {
                emit!(b, try_fcvt(to, v(d), from, v(n)), "fcvt {tp}{d}, {fp}{n}");
            }
        }
    }
}

/// FMOV (scalar, immediate): all 256 fields at both precisions, written as
/// the decimal value; and patterns that are not immediates.
fn immediates(b: &mut Batch) {
    for (sz, p) in FP_SIZES {
        for imm8 in 0..=255u8 {
            let value = fp_imm8_value(imm8);
            let d = u32::from(imm8) % 32;
            emit!(b, try_fmov_imm(sz, v(d), fp_bits(sz, value)), "fmov {p}{d}, #{value:?}");
        }
        // Next to the range's ends, between two neighbours, and common
        // constants that are not in the set. All are exact in single
        // precision: LLVM rounds the text to single before it looks.
        for value in [0.1f64, 0.0625, 0.117_187_5, 32.0, 33.0, 1.03125, -0.1, 100.0, 0.5 / 3.0] {
            let value = f64::from(value as f32);
            reject!(b, try_fmov_imm(sz, v(5), fp_bits(sz, value)), "fmov {p}5, #{value:?}");
        }
        // Negative zero has text LLVM refuses; positive zero is its alias
        // of `fmov <reg>, <zero register>`, and the others have no text.
        reject!(b, try_fmov_imm(sz, v(5), fp_bits(sz, -0.0)), "fmov {p}5, #-0.0");
        for value in [0.0, f64::INFINITY, f64::NEG_INFINITY, f64::NAN, f64::MIN_POSITIVE] {
            reject!(b, try_fmov_imm(sz, v(5), fp_bits(sz, value)));
        }
        // One bit away from 1.0 in the last place.
        reject!(b, try_fmov_imm(sz, v(5), fp_bits(sz, 1.0) + 1));
    }
    // A double that is not exact in single precision, and a
    // single-precision pattern with bits above bit 31.
    reject!(b, try_fmov_imm(FpSize::D, v(5), 0.1f64.to_bits()));
    reject!(b, try_fmov_imm(FpSize::S, v(5), 1.0f64.to_bits()));
    reject!(b, try_fmov_imm(FpSize::S, v(5), u64::from(1.0f32.to_bits()) | 1 << 32));
}

/// Conversions to and from the general-purpose registers, and the FMOV
/// forms that copy bits between the two register files.
fn integers(b: &mut Batch) {
    macro_rules! from_int {
        ($($f:ident)*) => {$(
            for (fsz, p) in FP_SIZES {
                for sz in Size::ALL {
                    for [d, n] in tuples([32, 32]) {
                        emit!(
                            b,
                            $f(fsz, v(d), sz, z(n)),
                            "{} {p}{d}, {}", stringify!($f), rz(sz, n)
                        );
                    }
                }
            }
        )*};
    }
    from_int!(scvtf ucvtf);

    macro_rules! to_int {
        ($($f:ident)*) => {$(
            for (fsz, p) in FP_SIZES {
                for sz in Size::ALL {
                    for [d, n] in tuples([32, 32]) {
                        emit!(
                            b,
                            $f(sz, z(d), fsz, v(n)),
                            "{} {}, {p}{n}", stringify!($f), rz(sz, d)
                        );
                    }
                }
            }
        )*};
    }
    to_int!(fcvtns fcvtnu fcvtas fcvtau fcvtps fcvtpu fcvtms fcvtmu fcvtzs fcvtzu);

    for (sz, p) in [(Size::W, 's'), (Size::X, 'd')] {
        for [g, f] in tuples([32, 32]) {
            emit!(b, fmov_fp_to_gpr(sz, z(g), v(f)), "fmov {}, {p}{f}", rz(sz, g));
            emit!(b, fmov_gpr_to_fp(sz, v(f), z(g)), "fmov {p}{f}, {}", rz(sz, g));
        }
    }
    for [g, f] in tuples([32, 32]) {
        emit!(b, fmov_hi_to_gpr(z(g), v(f)), "fmov {}, v{f}.d[1]", rz(Size::X, g));
        emit!(b, fmov_gpr_to_hi(v(f), z(g)), "fmov v{f}.d[1], {}", rz(Size::X, g));
    }
}

pub fn build(b: &mut Batch) {
    data(b);
    compare_and_select(b);
    formats(b);
    immediates(b);
    integers(b);
}
