//! The x86-64 floating-point intrinsics (WP-127, `spec/codegen.md`
//! §5.6.7): every intrinsic of `crate::fpintr::is_fp` on `half`, `float`,
//! `double` and `fp128`, checked against `hod_interp` where it computes
//! the intrinsic exactly and against `llc -O0 -fast-isel=0` (LLVM 22 and
//! 19; 22 only for the intrinsics 19 does not know) under the emulator
//! (`common/float_gen.rs`): the edge values (signed zeros, subnormals,
//! infinities, NaNs with payloads, the conversion boundaries) and random
//! ones; what is refused, by name.

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

#[path = "common/float_gen.rs"]
mod float_gen;

use float_gen::{Check, Rng, Suite, T, cases, check, ll_header, parse, pic};
use hod_cg::{CgError, Stress};

/// The intrinsic's suffix of a type.
fn sfx(t: T) -> &'static str {
    match t {
        T::H => "f16",
        T::B => "bf16",
        T::F => "f32",
        T::D => "f64",
        T::Q => "f128",
        T::I(_) => "",
    }
}

fn tag(t: T) -> &'static str {
    match t {
        T::H => "h",
        T::B => "b",
        T::F => "f",
        T::D => "d",
        T::Q => "q",
        T::I(_) => "i",
    }
}

/// The interpreter checks `float` and `double` where `exact`; `llc` the
/// rest.
fn who(t: T, exact: bool) -> Check {
    if exact && matches!(t, T::F | T::D) { Check::Interp } else { Check::Llc }
}

const SCALARS: [T; 4] = [T::F, T::D, T::H, T::Q];

/// `fp -> fp` intrinsic `name` of `t` as a function: declared, added,
/// cases added.
fn unary(suite: &mut Suite, rng: &mut Rng, name: &str, t: T, check: Check) -> usize {
    let (ty, s) = (t.ll(), sfx(t));
    suite.declare(&format!("declare {ty} @llvm.{name}.{s}({ty})"));
    let f = suite.add(
        &format!("{}_{}", name.replace('.', "_"), tag(t)),
        &[t],
        t,
        &format!("  %r = call {ty} @llvm.{name}.{s}({ty} %a0)\n"),
        check,
    );
    cases(suite, rng, f, &[t], 200, 150);
    f
}

/// `fp, fp -> fp` intrinsic `name` of `t`.
fn binary(suite: &mut Suite, rng: &mut Rng, name: &str, t: T, check: Check) -> usize {
    let (ty, s) = (t.ll(), sfx(t));
    suite.declare(&format!("declare {ty} @llvm.{name}.{s}({ty}, {ty})"));
    let f = suite.add(
        &format!("{name}_{}", tag(t)),
        &[t, t],
        t,
        &format!("  %r = call {ty} @llvm.{name}.{s}({ty} %a0, {ty} %a1)\n"),
        check,
    );
    cases(suite, rng, f, &[t, t], 900, 200);
    // The same operand twice.
    let g = suite.add(
        &format!("{name}_same_{}", tag(t)),
        &[t],
        t,
        &format!("  %r = call {ty} @llvm.{name}.{s}({ty} %a0, {ty} %a0)\n"),
        check,
    );
    cases(suite, rng, g, &[t], 100, 20);
    f
}

/// `sqrt`, the rounding family, `canonicalize`, `arithmetic.fence`,
/// `fabs`, `copysign`, `fma`, `fmuladd`, `minnum`/`maxnum`/`minimum`/
/// `maximum`.
#[test]
fn the_inline_and_rounding_intrinsics_agree() {
    let mut rng = Rng(1270);
    let mut suite = Suite::default();
    for t in SCALARS {
        for name in
            ["sqrt", "floor", "ceil", "trunc", "round", "roundeven", "nearbyint", "rint", "fabs"]
        {
            unary(&mut suite, &mut rng, name, t, who(t, true));
        }
        // `llc` makes `x * 1.0` of it; the interpreter quiets a NaN
        // its own way.
        let f = unary(&mut suite, &mut rng, "canonicalize", t, Check::Llc);
        // `llc` 19 cannot select it on x86-64.
        suite.funcs[f].since = 22;
        // The interpreter does not know the fence.
        unary(&mut suite, &mut rng, "arithmetic.fence", t, Check::Llc);
        binary(&mut suite, &mut rng, "copysign", t, who(t, true));
        // The NaN `minss` returns is the hardware's: `llc` is the oracle.
        for name in ["minnum", "maxnum", "minimum", "maximum"] {
            binary(&mut suite, &mut rng, name, t, Check::Llc);
        }
        if t == T::Q {
            // `llc` cannot select `minimum`/`maximum` of `fp128`: a model
            // of the ordered results; the NaN `fminimumf128` returns is
            // the runtime's.
            for (name, model) in
                [("minimum", quad_minimum as fn(&[u64]) -> u64), ("maximum", quad_maximum)]
            {
                for f in suite.funcs.iter_mut().filter(|f| f.name.starts_with(&format!("{name}_")))
                {
                    if f.name.ends_with("_q") {
                        f.llc = false;
                        f.model = Some(model);
                    }
                }
            }
            let funcs = &suite.funcs;
            suite.cases.retain(|c| {
                funcs[c.func].model.is_none()
                    || !(quad_nan(&c.args[0..2]) || quad_nan(&c.args[2..4]))
            });
        }
        let (ty, s) = (t.ll(), sfx(t));
        for (name, exact) in [("fma", true), ("fmuladd", false)] {
            suite.declare(&format!("declare {ty} @llvm.{name}.{s}({ty}, {ty}, {ty})"));
            let f = suite.add(
                &format!("{name}_{}", tag(t)),
                &[t, t, t],
                t,
                &format!("  %r = call {ty} @llvm.{name}.{s}({ty} %a0, {ty} %a1, {ty} %a2)\n"),
                who(t, exact),
            );
            cases(&mut suite, &mut rng, f, &[t, t, t], 900, 200);
            let g = suite.add(
                &format!("{name}_aab_{}", tag(t)),
                &[t, t],
                t,
                &format!("  %r = call {ty} @llvm.{name}.{s}({ty} %a0, {ty} %a1, {ty} %a0)\n"),
                who(t, exact),
            );
            cases(&mut suite, &mut rng, g, &[t, t], 300, 50);
        }
    }
    float_gen::assert_sse2(&suite);
    check("fpintr-inline", &suite, &[22, 19]);
}

/// Whether the `fp128` in words `w` (low, high) is a NaN.
fn quad_nan(w: &[u64]) -> bool {
    w[1] & 0x7fff_0000_0000_0000 == 0x7fff_0000_0000_0000
        && (w[0] != 0 || w[1] & 0xffff_ffff_ffff != 0)
}

/// The order of an `fp128` that is no NaN, `-0.0` below `+0.0`.
fn quad_key(w: &[u64]) -> (i128, bool) {
    let bits = (u128::from(w[1]) << 64) | u128::from(w[0]);
    let magnitude = (bits & !(1 << 127)) as i128;
    let negative = bits >> 127 == 1;
    (if negative { -magnitude } else { magnitude }, !negative)
}

/// An `fp128` result as the wrapper folds it.
fn quad_hash(w: &[u64]) -> u64 {
    w[1].wrapping_mul(-7_046_029_254_386_353_131_i64 as u64) ^ w[0]
}

/// `llvm.minimum.f128` of two values that are no NaNs (one, twice).
fn quad_minimum(args: &[u64]) -> u64 {
    let (a, b) = (&args[0..2], args.get(2..4).unwrap_or(&args[0..2]));
    quad_hash(if quad_key(a) <= quad_key(b) { a } else { b })
}

/// `llvm.maximum.f128` of two values that are no NaNs (one, twice).
fn quad_maximum(args: &[u64]) -> u64 {
    let (a, b) = (&args[0..2], args.get(2..4).unwrap_or(&args[0..2]));
    quad_hash(if quad_key(a) >= quad_key(b) { a } else { b })
}

/// `llvm.ldexp.f32.i64`: exact in `double`, rounded once.
fn ldexp_float(args: &[u64]) -> u64 {
    let x = f32::from_bits(args[0] as u32);
    let e = (args[1] as i64).clamp(-400, 400);
    let scale = f64::from_bits(((1023 + e) as u64) << 52);
    u64::from(((f64::from(x) * scale) as f32).to_bits())
}

/// `llvm.ldexp.f64.i64`: `scalbn` as musl writes it (rounded once).
fn ldexp_double(args: &[u64]) -> u64 {
    let pow = |n: i64| f64::from_bits(((1023 + n) as u64) << 52);
    let mut y = f64::from_bits(args[0]);
    let mut n = (args[1] as i64).clamp(-5000, 5000);
    if n > 1023 {
        y *= pow(1023);
        n -= 1023;
        if n > 1023 {
            y *= pow(1023);
            n = (n - 1023).min(1023);
        }
    } else if n < -1022 {
        y *= pow(-1022) * pow(53);
        n += 1022 - 53;
        if n < -1022 {
            y *= pow(-1022) * pow(53);
            n = (n + 1022 - 53).max(-1022);
        }
    }
    (y * pow(n)).to_bits()
}

/// `minimumnum`/`maximumnum` and the libm intrinsics LLVM 19 does not
/// know (`tan` .. `tanh`, `atan2`), against `llc` 22.
#[test]
fn the_newer_intrinsics_agree() {
    let mut rng = Rng(1271);
    let mut suite = Suite::default();
    for t in SCALARS {
        for name in ["minimumnum", "maximumnum"] {
            binary(&mut suite, &mut rng, name, t, Check::Llc);
        }
        for name in ["tan", "asin", "acos", "atan", "sinh", "cosh", "tanh"] {
            unary(&mut suite, &mut rng, name, t, Check::Llc);
        }
        binary(&mut suite, &mut rng, "atan2", t, Check::Llc);
    }
    for f in &mut suite.funcs {
        f.since = 22;
    }
    // Of two `fp128` NaNs `llc` 22 returns the second unquieted, `hod`
    // the first quieted: both are NaNs, the payload is unspecified.
    let nan = |w: &[u64]| {
        w[1] & 0x7fff_0000_0000_0000 == 0x7fff_0000_0000_0000
            && (w[0] != 0 || w[1] & 0xffff_ffff_ffff != 0)
    };
    let funcs = &suite.funcs;
    suite.cases.retain(|c| {
        let name = &funcs[c.func].name;
        !(name.ends_with("num_q") || name.ends_with("num_same_q"))
            || !(nan(&c.args[0..2]) && (name.contains("same") || nan(&c.args[2..4])))
    });
    float_gen::assert_sse2(&suite);
    check("fpintr-new", &suite, &[22]);
}

/// The libm intrinsics LLVM 19 knows, `powi`, `ldexp` (an `i64`
/// exponent clamped), `frexp`.
#[test]
fn the_calls_agree() {
    let mut rng = Rng(1272);
    let mut suite = Suite::default();
    for t in SCALARS {
        for name in ["sin", "cos", "exp", "exp2", "exp10", "log", "log2", "log10"] {
            unary(&mut suite, &mut rng, name, t, Check::Llc);
        }
        binary(&mut suite, &mut rng, "pow", t, Check::Llc);
        let (ty, s) = (t.ll(), sfx(t));
        for (name, n, exact) in [
            // `llc` refuses an exponent other than `i32` on x86-64.
            ("powi", 32, false),
            ("ldexp", 32, false),
            ("ldexp", 64, false),
            ("ldexp", 5, false),
        ] {
            suite.declare(&format!("declare {ty} @llvm.{name}.{s}.i{n}({ty}, i{n})"));
            let f = suite.add(
                &format!("{name}{n}_{}", tag(t)),
                &[t, T::I(n)],
                t,
                &format!("  %r = call {ty} @llvm.{name}.{s}.i{n}({ty} %a0, i{n} %a1)\n"),
                who(t, exact),
            );
            if t == T::H && name == "powi" {
                // `llc` 19 drops the sign of `(-0.0)^n` for odd `n`.
                suite.funcs[f].since = 22;
            }
            cases(&mut suite, &mut rng, f, &[t, T::I(n)], 900, 200);
        }
        suite.declare(&format!("declare {{ {ty}, i32 }} @llvm.frexp.{s}.i32({ty})"));
        let w = t.bits().min(64);
        let fold = if t == T::Q {
            // The fraction's words folded, the exponent added.
            "  %fr = extractvalue { fp128, i32 } %p, 0\n  %fs = alloca fp128, align 16\n  \
             store fp128 %fr, ptr %fs\n  %fl = load i64, ptr %fs\n  \
             %fhp = getelementptr i8, ptr %fs, i64 8\n  %fh = load i64, ptr %fhp\n  \
             %fm = mul i64 %fh, 31\n  %m = xor i64 %fm, %fl\n"
                .to_string()
        } else {
            let widen = if w == 64 {
                "%m = or i64 %fb, 0\n".to_string()
            } else {
                format!("%m = zext i{w} %fb to i64\n")
            };
            format!(
                "  %fr = extractvalue {{ {ty}, i32 }} %p, 0\n  %fb = bitcast {ty} %fr to i{w}\n  \
                 {widen}"
            )
        };
        let f = suite.add(
            &format!("frexp_{}", tag(t)),
            &[t],
            T::I(64),
            &format!(
                "  %p = call {{ {ty}, i32 }} @llvm.frexp.{s}.i32({ty} %a0)\n{fold}  \
                 %e = extractvalue {{ {ty}, i32 }} %p, 1\n  %ez = zext i32 %e to i64\n  \
                 %es = shl i64 %ez, 40\n  %r = xor i64 %m, %es\n"
            ),
            Check::Llc,
        );
        // The exponent of an infinity or a NaN is unspecified.
        let finite: Vec<Vec<u64>> = float_gen::operands(t)
            .into_iter()
            .chain((0..200).map(|_| float_gen::random(&mut rng, t)))
            .filter(|v| finite(t, v))
            .collect();
        for v in finite {
            suite.case(f, &v);
        }
    }
    // `llc` truncates an `i64` exponent of `ldexp` to `i32` (2^32 + 1
    // scales by 2): `float` and `double` against a model of every
    // exponent, `half` and `fp128` against `llc` where it is right.
    for f in &mut suite.funcs {
        match f.name.as_str() {
            "ldexp64_f" => (f.llc, f.model) = (false, Some(ldexp_float as fn(&[u64]) -> u64)),
            "ldexp64_d" => (f.llc, f.model) = (false, Some(ldexp_double as fn(&[u64]) -> u64)),
            _ => {}
        }
    }
    let funcs = &suite.funcs;
    suite.cases.retain(|c| {
        let name = &funcs[c.func].name;
        let exponent = if name == "ldexp64_h" {
            c.args[1]
        } else if name == "ldexp64_q" {
            c.args[2]
        } else {
            return true;
        };
        i32::try_from(exponent as i64).is_ok()
    });
    float_gen::assert_sse2(&suite);
    check("fpintr-calls", &suite, &[22, 19]);
}

/// Whether `bits` of `t` is finite.
fn finite(t: T, bits: &[u64]) -> bool {
    match t {
        T::H => bits[0] & 0x7c00 != 0x7c00,
        T::F => bits[0] & 0x7f80_0000 != 0x7f80_0000,
        T::D => bits[0] & 0x7ff0_0000_0000_0000 != 0x7ff0_0000_0000_0000,
        T::Q => bits[1] & 0x7fff_0000_0000_0000 != 0x7fff_0000_0000_0000,
        _ => true,
    }
}

/// `lround`/`llround`/`lrint`/`llrint` to `i32` and `i64`,
/// `fptosi.sat`/`fptoui.sat` to every width around the boundaries,
/// `is.fpclass` with masks of single classes and of groups.
#[test]
fn the_integer_intrinsics_agree() {
    let mut rng = Rng(1273);
    let mut suite = Suite::default();
    for t in SCALARS {
        let (ty, s) = (t.ll(), sfx(t));
        for name in ["lround", "llround", "lrint", "llrint"] {
            // `llc` selects `llround`/`llrint` to `i64` only.
            let widths: &[u32] = if name.starts_with("ll") { &[64] } else { &[32, 64] };
            for &n in widths {
                suite.declare(&format!("declare i{n} @llvm.{name}.i{n}.{s}({ty})"));
                let f = suite.add(
                    &format!("{name}{n}_{}", tag(t)),
                    &[t],
                    T::I(n),
                    &format!("  %r = call i{n} @llvm.{name}.i{n}.{s}({ty} %a0)\n"),
                    Check::Llc,
                );
                if t == T::H {
                    // `llc` 19 cannot select it.
                    suite.funcs[f].since = 22;
                }
                cases(&mut suite, &mut rng, f, &[t], 200, 150);
            }
        }
        if t != T::Q {
            for n in [1, 8, 16, 31, 32, 33, 63, 64] {
                for name in ["fptosi.sat", "fptoui.sat"] {
                    suite.declare(&format!("declare i{n} @llvm.{name}.i{n}.{s}({ty})"));
                    let f = suite.add(
                        &format!("{}{n}_{}", name.replace('.', "_"), tag(t)),
                        &[t],
                        T::I(n),
                        &format!("  %r = call i{n} @llvm.{name}.i{n}.{s}({ty} %a0)\n"),
                        Check::Llc,
                    );
                    cases(&mut suite, &mut rng, f, &[t], 200, 150);
                }
            }
        }
        suite.declare(&format!("declare i1 @llvm.is.fpclass.{s}({ty}, i32)"));
        for mask in
            [1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 3, 0x204, 0x90, 0x60, 0x1f8, 0x3fc, 0, 0x3ff]
        {
            let f = suite.add(
                &format!("fpclass{mask}_{}", tag(t)),
                &[t],
                T::I(1),
                &format!("  %r = call i1 @llvm.is.fpclass.{s}({ty} %a0, i32 {mask})\n"),
                Check::Llc,
            );
            cases(&mut suite, &mut rng, f, &[t], 200, 60);
        }
    }
    float_gen::assert_sse2(&suite);
    check("fpintr-int", &suite, &[22, 19]);
}

/// What is not lowered, by name, one construct per program: an intrinsic
/// of `bfloat`, `fptosi.sat` of `fp128`, a `powi` exponent wider than 32
/// bits, an `lround` to `i128` (vectors are WP-132's, `tests/lanes.rs`).
#[test]
fn what_is_not_lowered_is_refused_by_name() {
    use hod_x64::fpintr;
    for (text, name) in [
        (
            "declare bfloat @llvm.fabs.bf16(bfloat)\ndefine bfloat @f(bfloat %a) {\n  \
             %r = call bfloat @llvm.fabs.bf16(bfloat %a)\n  ret bfloat %r\n}\n",
            fpintr::BFLOAT,
        ),
        (
            "declare i32 @llvm.fptosi.sat.i32.f128(fp128)\ndefine i32 @f(fp128 %a) {\n  \
             %r = call i32 @llvm.fptosi.sat.i32.f128(fp128 %a)\n  ret i32 %r\n}\n",
            fpintr::FP128,
        ),
        (
            "declare float @llvm.powi.f32.i64(float, i64)\ndefine float @f(float %a, i64 %n) {\n  \
             %r = call float @llvm.powi.f32.i64(float %a, i64 %n)\n  ret float %r\n}\n",
            fpintr::INTRINSIC,
        ),
        (
            "declare i128 @llvm.lround.i128.f64(double)\ndefine i128 @f(double %a) {\n  \
             %r = call i128 @llvm.lround.i128.f64(double %a)\n  ret i128 %r\n}\n",
            fpintr::WIDE,
        ),
    ] {
        let (m, bodies) = parse(&format!("{}{text}", ll_header()));
        match hod_x64::compile_to_object(&m, &bodies, &pic(Stress::default())) {
            Err(hod_x64::ModuleError::Function { error, .. }) => {
                assert_eq!(error, CgError::unsupported(name), "{text}");
            }
            other => panic!("not refused: {text}: {:?}", other.map(|o| o.len())),
        }
    }
}
