//! `sitofp` / `uitofp` of integers of 65 to 128 bits to `bfloat` on
//! x86-64 (WP-295, `spec/codegen.md` §5.6.5): inline, rounded once
//! to nearest even. Every case runs under the emulator against an exact
//! model (single rounding); the cases where rounding to `float` first
//! gives the same `bfloat` also against `llc -O0 -fast-isel=0` of LLVM
//! 22 and 19, which call `__floattisf` / `__floatuntisf` and then
//! `__truncsfbf2` and so round twice (`common/float_gen.rs`).

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "bit patterns of integers"
)]

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

#[path = "common/float_gen.rs"]
mod float_gen;

use float_gen::{Check, Rng, Suite, T, check};

const MAJORS: [u32; 2] = [22, 19];

/// The nonzero `m` rounded to `p` significant bits, to nearest even:
/// `(kept, exp)` with `m ~ kept * 2^exp` and `2^(p-1) <= kept < 2^p`.
fn round(m: u128, p: u32) -> (u128, i32) {
    let width = 128 - m.leading_zeros();
    if width <= p {
        return (m << (p - width), width as i32 - p as i32);
    }
    let shift = width - p;
    let kept = m >> shift;
    let rest = m & ((1u128 << shift) - 1);
    let half = 1u128 << (shift - 1);
    let up = rest > half || (rest == half && kept & 1 == 1);
    let kept = kept + u128::from(up);
    let shift = shift as i32;
    if kept >> p == 1 { (kept >> 1, shift + 1) } else { (kept, shift) }
}

/// The bits of the binary format with `p` significand bits and exponent
/// bias `bias` of the magnitude `m` rounded once (normal or infinite: an
/// integer is never subnormal), without the sign.
fn encode(m: u128, p: u32, bias: u32) -> u64 {
    if m == 0 {
        return 0;
    }
    let (kept, exp) = round(m, p);
    // The index of the top bit: never negative for an integer.
    let top = (exp + p as i32 - 1) as u32;
    if top > bias {
        return u64::from(2 * bias + 1) << (p - 1);
    }
    (u64::from(top + bias) << (p - 1)) | (kept as u64 & ((1 << (p - 1)) - 1))
}

/// The value of the case `(lo, hi)` for an integer of `bits` bits: its
/// sign and magnitude.
fn value(args: &[u64], bits: u32, signed: bool) -> (bool, u128) {
    let raw = u128::from(args[0]) | (u128::from(args[1]) << 64);
    let raw = if bits == 128 { raw } else { raw & ((1u128 << bits) - 1) };
    if signed && raw >> (bits - 1) == 1 {
        let full = if bits == 128 { raw } else { raw | (u128::MAX << bits) };
        (true, full.wrapping_neg())
    } else {
        (false, raw)
    }
}

/// The `bfloat` hod gives: rounded once.
fn once(args: &[u64], bits: u32, signed: bool) -> u64 {
    let (negative, m) = value(args, bits, signed);
    u64::from(negative) << 15 | encode(m, 8, 127)
}

/// The `bfloat` `llc` gives: rounded to `float`, then to `bfloat`.
fn twice(args: &[u64], bits: u32, signed: bool) -> u64 {
    let (negative, m) = value(args, bits, signed);
    let f = encode(m, 24, 127);
    // `__truncsfbf2` of a number (an infinity stays one).
    let b = (f + 0x7fff + ((f >> 16) & 1)) >> 16;
    u64::from(negative) << 15 | b
}

fn model<const BITS: u32, const SIGNED: bool>(args: &[u64]) -> u64 {
    once(args, BITS, SIGNED)
}

/// A model of a function's result from its arguments.
type Model = fn(&[u64]) -> u64;

/// The widths, with their models (signed, unsigned).
const WIDTHS: [(u32, Model, Model); 4] = [
    (65, model::<65, true>, model::<65, false>),
    (96, model::<96, true>, model::<96, false>),
    (127, model::<127, true>, model::<127, false>),
    (128, model::<128, true>, model::<128, false>),
];

/// The edge values (as 128-bit patterns; a narrower integer takes their
/// low bits): zero, ±1, small exact values, ties at the 8-bit significand
/// boundary and their neighbours at every shift (some of which round
/// differently through `float`), the largest finite `bfloat` and the
/// tie above it (overflow to infinity), the signed minimum and maximum,
/// the unsigned maximum, and every high-word top bit.
fn edges() -> Vec<u128> {
    let mut v: Vec<u128> = vec![0, 1, 2, 3, 127, 255, 256, 257, 511, u128::MAX];
    v.extend([1u128 << 127, (1u128 << 127) - 1, (1u128 << 127) + 1]);
    let max_finite = 0xffu128 << 120;
    v.extend([max_finite, max_finite - 1, max_finite + 1]);
    v.extend([max_finite + (1 << 119) - 1, max_finite + (1 << 119), max_finite + (1 << 119) + 1]);
    for s in 0..=119u32 {
        for tie in [0x181u128, 0x183, 0x1ff, 0x101] {
            let t = tie << s;
            v.extend([t, t.wrapping_sub(1), t + 1]);
        }
    }
    for b in 0..64u32 {
        let hi = 1u128 << (64 + b);
        v.extend([hi, hi | 1, hi | (1 << 63), hi.wrapping_sub(1), hi | (hi - 1)]);
    }
    for bits in [65u32, 96, 127] {
        let top = 1u128 << (bits - 1);
        v.extend([top, top - 1, top + 1, (top << 1) - 1]);
    }
    let negatives: Vec<u128> = v.iter().map(|x| x.wrapping_neg()).collect();
    v.extend(negatives);
    v
}

/// Every width signed and unsigned over the edges and random values:
/// hod plain against the single-rounding model, hod stressed against hod
/// plain, `llc` 22 and 19 against hod plain where double rounding agrees.
#[test]
fn wide_integers_to_bfloat_round_once() {
    let mut rng = Rng(295);
    let mut suite = Suite::default();
    let mut values = edges();
    for _ in 0..400 {
        let shift = rng.below(128) as u32;
        let x = (u128::from(rng.next()) << 64 | u128::from(rng.next())) >> shift;
        values.push(x);
    }
    let mut differ = 0;
    for (bits, signed_model, unsigned_model) in WIDTHS {
        let int = format!("i{bits}");
        for (op, signed, model) in
            [("sitofp", true, signed_model), ("uitofp", false, unsigned_model)]
        {
            let body = format!(
                "  %wl = zext i64 %a0 to {int}\n  %wz = zext i64 %a1 to {int}\n  \
                 %ws = shl {int} %wz, 64\n  %v = or {int} %wl, %ws\n  \
                 %r = {op} {int} %v to bfloat\n"
            );
            let ops = [T::I(64), T::I(64)];
            let all = suite.add(&format!("{op}_i{bits}_b"), &ops, T::B, &body, Check::Llc);
            suite.funcs[all].llc = false;
            suite.funcs[all].model = Some(model);
            let with_llc = suite.add(&format!("{op}_i{bits}_b_llc"), &ops, T::B, &body, Check::Llc);
            suite.funcs[with_llc].model = Some(model);
            for &x in &values {
                let args = [x as u64, (x >> 64) as u64];
                suite.case(all, &args);
                if once(&args, bits, signed) == twice(&args, bits, signed) {
                    suite.case(with_llc, &args);
                } else {
                    differ += 1;
                }
            }
        }
    }
    // A second oracle for the model: libgcc's single-rounding routines
    // (what GCC calls), called by hod and by `llc` alike.
    for (routine, model) in [("__floattibf", WIDTHS[3].1), ("__floatuntibf", WIDTHS[3].2)] {
        suite.declare(&format!("declare bfloat @{routine}(i128)"));
        let body = format!(
            "  %wl = zext i64 %a0 to i128\n  %wz = zext i64 %a1 to i128\n  \
             %ws = shl i128 %wz, 64\n  %v = or i128 %wl, %ws\n  \
             %r = call bfloat @{routine}(i128 %v)\n"
        );
        let name = format!("libgcc{routine}");
        let f = suite.add(&name, &[T::I(64), T::I(64)], T::B, &body, Check::Llc);
        suite.funcs[f].model = Some(model);
        for &x in &values {
            suite.case(f, &[x as u64, (x >> 64) as u64]);
        }
    }
    assert!(differ > 0, "the edges hold cases that round differently through float");
    eprintln!("wide bfloat: {differ} cases where llc's double rounding differs (model only)");
    check("wide-bfloat", &suite, &MAJORS);
}

/// The other conversions between `bfloat` and integers wider than 64
/// bits stay refused by name: to an integer of 65 to 128 bits, and from
/// one wider than 128.
#[test]
fn other_wide_bfloat_conversions_stay_refused() {
    use hod_x64::aggr::WIDE_FP_CAST;
    let cases = [
        "define void @f(ptr %p, bfloat %a) {\n  %r = fptosi bfloat %a to i128\n  store i128 %r, ptr %p\n  ret void\n}\n",
        "define void @f(ptr %p, bfloat %a) {\n  %r = fptoui bfloat %a to i96\n  store i96 %r, ptr %p\n  ret void\n}\n",
        "define bfloat @f(i129 %a) {\n  %r = sitofp i129 %a to bfloat\n  ret bfloat %r\n}\n",
    ];
    for text in cases {
        let (module, bodies) = float_gen::parse(&format!("{}{text}", float_gen::ll_header()));
        match hod_x64::compile_to_object(
            &module,
            &bodies,
            &float_gen::pic(hod_cg::Stress::default()),
        ) {
            Err(hod_x64::ModuleError::Function { error, .. }) => {
                assert_eq!(error, hod_cg::CgError::unsupported(WIDE_FP_CAST), "{text}");
            }
            Ok(_) => panic!("{text}: compiled"),
            Err(other) => panic!("{text}: {other}"),
        }
    }
}
