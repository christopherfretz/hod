//! The floating-point intrinsics (WP-070, `spec/codegen.md` §5.5.1
//! "Intrinsics, the second set"): every intrinsic at `half`, `float`,
//! `double` and `fp128`, natively against `llc -O0 -global-isel=0
//! -fast-isel=0`, C, the interpreter where it evaluates the intrinsic,
//! and a Rust model where the result is defined bit for bit
//! (`common/fpintr_gen.rs`); all 1024 `is.fpclass` masks; many live
//! values across the called intrinsics; what is refused, by name.

mod common;

#[path = "common/fpintr_gen.rs"]
mod fpintr_gen;

use std::fmt::Write as _;

use common::{assert_refused, compile_ll};
use fpintr_gen::{D, F, FLOATS, Func, Gen, H, I, Q, Ty, fp_class, intrinsic, run_native, saturate};
use hod_a64::{fpintr, intrinsic as intr};

/// `half` through `float` in C: `(T) name((float) a0, ...)`.
fn c_call(ty: Ty, name: &str, args: usize) -> String {
    let list: Vec<String> =
        (0..args).map(|i| if ty == H { format!("(float)a{i}") } else { format!("a{i}") }).collect();
    format!("{}({})", ty.libm(name), list.join(", "))
}

/// `fabs`, `copysign`, exactly, NaNs included.
fn sign_ops(ty: Ty) -> Vec<Func> {
    let s = ty.suffix();
    let (abs, copy) = match ty {
        H => ("H(BH(a0) & 0x7fff)", "H((BH(a0) & 0x7fff) | (BH(a1) & 0x8000))"),
        F => ("F(BF(a0) & 0x7fffffffu)", "F((BF(a0) & 0x7fffffffu) | (BF(a1) & 0x80000000u))"),
        D => ("D(BD(a0) & ~(1ull << 63))", "D((BD(a0) & ~(1ull << 63)) | (BD(a1) & (1ull << 63)))"),
        _ => (
            "Q(QL(a0), QH(a0) & ~(1ull << 63))",
            "Q(QL(a0), (QH(a0) & ~(1ull << 63)) | (QH(a1) & (1ull << 63)))",
        ),
    };
    let interp = matches!(ty, F | D);
    vec![
        intrinsic(&format!("fabs_{s}"), &format!("fabs.{s}"), ty, &[ty])
            .c(abs)
            .exact()
            .interp(interp),
        intrinsic(&format!("copysign_{s}"), &format!("copysign.{s}"), ty, &[ty, ty])
            .c(copy)
            .exact()
            .interp(interp)
            .with(Gen::Pairs),
    ]
}

/// The unary operations computed by an instruction or `*l`.
fn unary_ops(ty: Ty) -> Vec<Func> {
    let s = ty.suffix();
    let mut out = Vec::new();
    for (name, c) in [
        ("sqrt", "sqrt"),
        ("floor", "floor"),
        ("ceil", "ceil"),
        ("trunc", "trunc"),
        ("round", "round"),
        ("roundeven", "roundeven"),
        ("rint", "rint"),
        ("nearbyint", "nearbyint"),
    ] {
        let interp = matches!(ty, F | D);
        out.push(
            intrinsic(&format!("{name}_{s}"), &format!("{name}.{s}"), ty, &[ty])
                .c(c_call(ty, c, 1))
                .interp(interp),
        );
        // Values that round: quarters and half-way points.
        out.push(
            intrinsic(&format!("{name}_{s}_small"), &format!("{name}.{s}"), ty, &[ty])
                .c(c_call(ty, c, 1))
                .interp(interp)
                .with(Gen::Small),
        );
    }
    let canon = match ty {
        H => "(float)a0 * ONE_F",
        F => "a0 * ONE_F",
        D => "a0 * ONE_D",
        _ => "a0 * ONE_Q",
    };
    out.push(
        intrinsic(&format!("canonicalize_{s}"), &format!("canonicalize.{s}"), ty, &[ty]).c(canon),
    );
    out
}

/// `fma`, `fmuladd`, and the min/max family.
fn binary_ops(ty: Ty) -> Vec<Func> {
    let s = ty.suffix();
    let mut out = Vec::new();
    let fma = if ty == H {
        "fma((double)a0, (double)a1, (double)a2)".to_string()
    } else {
        format!("{}(a0, a1, a2)", ty.libm("fma"))
    };
    let muladd = match ty {
        H => "(float)(_Float16)((float)a0 * (float)a1) + (float)a2".to_string(),
        Q => "a0 * a1 + a2".to_string(),
        _ => fma.clone(),
    };
    let interp = matches!(ty, F | D);
    out.push(
        intrinsic(&format!("fma_{s}"), &format!("fma.{s}"), ty, &[ty, ty, ty])
            .c(fma.clone())
            .interp(interp)
            .with(Gen::Pairs),
    );
    out.push(
        intrinsic(&format!("fmuladd_{s}"), &format!("fmuladd.{s}"), ty, &[ty, ty, ty])
            .c(muladd)
            .with(Gen::Pairs),
    );
    // fma(x, x, y): one value in two operands.
    let t = ty.ll();
    out.push(
        Func::new(
            format!("fma_sq_{s}"),
            ty,
            &[ty, ty],
            format!("  %r = call {t} @llvm.fma.{s}({t} %a0, {t} %a0, {t} %a1)\n  ret {t} %r"),
        )
        .decl(format!("declare {t} @llvm.fma.{s}({t}, {t}, {t})"))
        .c(fma.replace("a1", "a0").replace("a2", "a1"))
        .interp(interp)
        .with(Gen::Pairs),
    );
    for (name, c) in [
        ("minnum", "fmin"),
        ("maxnum", "fmax"),
        ("minimum", "fminimum"),
        ("maximum", "fmaximum"),
        ("minimumnum", "fminimum_num"),
        ("maximumnum", "fmaximum_num"),
    ] {
        let f = intrinsic(&format!("{name}_{s}"), &format!("{name}.{s}"), ty, &[ty, ty])
            .c(c_call(ty, c, 2))
            .with(Gen::Pairs);
        // llc 22 cannot select `minimum`/`maximum` of `fp128`.
        let f = if ty == Q && matches!(name, "minimum" | "maximum") { f.llc_crashes() } else { f };
        out.push(f);
    }
    out
}

/// `lround`, `llround`, `lrint`, `llrint` to `i64` and `i32`, of values
/// whose result is in range.
fn to_int_ops(ty: Ty) -> Vec<Func> {
    let s = ty.suffix();
    let mut out = Vec::new();
    for (name, c) in
        [("lround", "lround"), ("llround", "llround"), ("lrint", "lrint"), ("llrint", "llrint")]
    {
        for bits in [64, 32] {
            // llc 22 cannot select `llround`/`llrint` to `i32` (but
            // of `fp128`, a call).
            let crashes = ty != Q && bits == 32 && name.starts_with("ll");
            let f = intrinsic(
                &format!("{name}_{s}_i{bits}"),
                &format!("{name}.i{bits}.{s}"),
                I(bits),
                &[ty],
            )
            .c(format!("(uint{bits}_t){}", c_call(ty, c, 1)))
            .with(Gen::Small);
            out.push(if crashes { f.llc_crashes() } else { f });
        }
    }
    out
}

/// `fptosi.sat`/`fptoui.sat` at every width, against the model.
fn saturating_ops(ty: Ty) -> Vec<Func> {
    let s = ty.suffix();
    let mut out = Vec::new();
    for width in [1, 2, 7, 8, 16, 17, 31, 32, 33, 53, 63, 64] {
        for signed in [true, false] {
            let op = if signed { "fptosi" } else { "fptoui" };
            out.push(
                intrinsic(
                    &format!("{op}_sat_{s}_i{width}"),
                    &format!("{op}.sat.i{width}.{s}"),
                    I(width),
                    &[ty],
                )
                .model(move |a| saturate(ty, a[0], width, signed)),
            );
        }
    }
    out
}

/// The called intrinsics: libm, `powi`, `ldexp`, `frexp`.
fn call_ops(ty: Ty) -> Vec<Func> {
    let s = ty.suffix();
    let t = ty.ll();
    let mut out = Vec::new();
    for name in [
        "sin", "cos", "tan", "asin", "acos", "atan", "sinh", "cosh", "tanh", "exp", "exp2",
        "exp10", "log", "log2", "log10",
    ] {
        out.push(
            intrinsic(&format!("{name}_{s}"), &format!("{name}.{s}"), ty, &[ty])
                .c(c_call(ty, name, 1)),
        );
    }
    for name in ["atan2", "pow"] {
        out.push(
            intrinsic(&format!("{name}_{s}"), &format!("{name}.{s}"), ty, &[ty, ty])
                .c(c_call(ty, name, 2))
                .with(Gen::Pairs),
        );
    }
    let x = if ty == H { "(float)a0" } else { "a0" };
    let powi = match ty {
        H | F => "__builtin_powif",
        D => "__builtin_powi",
        _ => "__builtin_powil",
    };
    out.push(
        intrinsic(&format!("powi_{s}"), &format!("powi.{s}.i32"), ty, &[ty, I(32)])
            .c(format!("{powi}({x}, (int32_t)a1)"))
            .with(Gen::Exp),
    );
    out.push(
        intrinsic(&format!("powi_{s}_i16"), &format!("powi.{s}.i16"), ty, &[ty, I(16)])
            .c(format!("{powi}({x}, (int16_t)a1)"))
            .with(Gen::Exp),
    );
    let ldexp = ty.libm("ldexp");
    out.push(
        intrinsic(&format!("ldexp_{s}"), &format!("ldexp.{s}.i32"), ty, &[ty, I(32)])
            .c(format!("{ldexp}({x}, (int32_t)a1)"))
            .with(Gen::Exp),
    );
    out.push(
        intrinsic(&format!("ldexp_{s}_i16"), &format!("ldexp.{s}.i16"), ty, &[ty, I(16)])
            .c(format!("{ldexp}({x}, (int16_t)a1)"))
            .with(Gen::Exp),
    );
    // An `i64` exponent: clamped to the `int` range (llc truncates it,
    // so it is compared only where it is in range).
    let clamped = format!(
        "{ldexp}({x}, (int64_t)a1 > 0x7fffffff ? 0x7fffffff : (int64_t)a1 < -0x7fffffffll - 1 ? -0x7fffffff - 1 : (int)(int64_t)a1)"
    );
    out.push(
        intrinsic(&format!("ldexp_{s}_i64"), &format!("ldexp.{s}.i64"), ty, &[ty, I(64)])
            .c(clamped.clone())
            .with(Gen::Exp)
            .no_llc(),
    );
    out.push(
        intrinsic(&format!("ldexp_{s}_i64_int"), &format!("ldexp.{s}.i64"), ty, &[ty, I(64)])
            .c(clamped)
            .with(Gen::ExpInt),
    );
    // `frexp`: the fraction and the exponent, each used.
    let frexp = ty.libm("frexp");
    let decl = format!("declare {{ {t}, i32 }} @llvm.frexp.{s}.i32({t})");
    let call = format!("  %s = call {{ {t}, i32 }} @llvm.frexp.{s}.i32({t} %a0)\n");
    out.push(
        Func::new(
            format!("frexp_m_{s}"),
            ty,
            &[ty],
            format!("{call}  %r = extractvalue {{ {t}, i32 }} %s, 0\n  ret {t} %r"),
        )
        .decl(decl.clone())
        .c(format!("({{ int e; {frexp}({x}, &e); }})")),
    );
    out.push(
        Func::new(
            format!("frexp_e_{s}"),
            I(32),
            &[ty],
            format!("{call}  %r = extractvalue {{ {t}, i32 }} %s, 1\n  ret i32 %r"),
        )
        .decl(decl)
        .c(format!("({{ int e; {frexp}({x}, &e); (uint32_t)e; }})")),
    );
    out
}

/// Every form at every type.
fn every_form() -> Vec<Func> {
    let mut out = Vec::new();
    for ty in FLOATS {
        out.extend(sign_ops(ty));
        out.extend(unary_ops(ty));
        out.extend(binary_ops(ty));
        out.extend(to_int_ops(ty));
        out.extend(call_ops(ty));
        if ty != Q {
            out.extend(saturating_ops(ty));
        }
    }
    out
}

/// Each form compiles by itself (a failure names every function).
#[test]
fn every_form_compiles() {
    let failed: Vec<String> = every_form()
        .iter()
        .filter_map(|f| {
            let text = format!("{}\n{}", f.decls.join("\n"), f.text_of(""));
            compile_ll(&text).err().map(|e| format!("{}: {e}", f.name))
        })
        .collect();
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

#[test]
fn every_form_agrees_with_llc_c_and_the_interpreter() {
    let funcs = every_form();
    let report = run_native("a64-fpintr-forms", &funcs, 24, 70);
    assert!(report.interp > 0 && report.c > 0 && report.model > 0, "{report:?}");
}

/// `is.fpclass` with every mask: a function per type and 32 masks
/// returns the 32 tests as bits; one value of each class and random ones.
#[test]
fn every_fpclass_mask_agrees() {
    let mut funcs = Vec::new();
    for ty in FLOATS {
        let (t, s) = (ty.ll(), ty.suffix());
        for group in 0..32u32 {
            let mut body = String::new();
            let mut acc = "0".to_string();
            for k in 0..32u32 {
                let mask = group * 32 + k;
                let _ = writeln!(
                    body,
                    "  %c{k} = call i1 @llvm.is.fpclass.{s}({t} %a0, i32 {mask})\n  \
                     %z{k} = zext i1 %c{k} to i32\n  %h{k} = shl i32 %z{k}, {k}\n  \
                     %o{k} = or i32 {acc}, %h{k}"
                );
                acc = format!("%o{k}");
            }
            let _ = write!(body, "  ret i32 {acc}");
            funcs.push(
                Func::new(format!("fpclass_{s}_{group}"), I(32), &[ty], body)
                    .decl(format!("declare i1 @llvm.is.fpclass.{s}({t}, i32)"))
                    .model(move |a| {
                        let bit = fp_class(ty, a[0]);
                        (0..32u32)
                            .filter(|k| (group * 32 + k) >> bit & 1 == 1)
                            .map(|k| 1u128 << k)
                            .sum()
                    }),
            );
        }
        // The `i1` result returned as it is.
        for mask in [0x3u32, 0x204, 0x90, 0x3fc, 0x1f8, 0x60] {
            funcs.push(
                Func::new(
                    format!("fpclass_{s}_r{mask}"),
                    I(1),
                    &[ty],
                    format!(
                        "  %r = call i1 @llvm.is.fpclass.{s}({t} %a0, i32 {mask})\n  ret i1 %r"
                    ),
                )
                .decl(format!("declare i1 @llvm.is.fpclass.{s}({t}, i32)"))
                .model(move |a| u128::from(mask >> fp_class(ty, a[0]) & 1)),
            );
        }
    }
    let report = run_native("a64-fpintr-class", &funcs, 40, 71);
    assert_eq!(report.model, report.calls);
}

/// Many integer and floating-point values live across each called
/// intrinsic, plain and stressed.
#[test]
fn live_values_survive_called_intrinsics() {
    let mut funcs = Vec::new();
    for ty in FLOATS {
        let (t, s) = (ty.ll(), ty.suffix());
        let calls: [(&str, String, String); 6] = [
            (
                "sin",
                format!("call {t} @llvm.sin.{s}({t} %x)"),
                c_call(ty, "sin", 1).replace("a0", "x"),
            ),
            (
                "pow",
                format!("call {t} @llvm.pow.{s}({t} %x, {t} %y)"),
                c_call(ty, "pow", 2).replace("a0", "x").replace("a1", "y"),
            ),
            ("powi", format!("call {t} @llvm.powi.{s}.i32({t} %x, i32 %n)"), {
                let x = if ty == H { "(float)x" } else { "x" };
                let f = match ty {
                    H | F => "__builtin_powif",
                    D => "__builtin_powi",
                    _ => "__builtin_powil",
                };
                format!("{f}({x}, n)")
            }),
            ("ldexp", format!("call {t} @llvm.ldexp.{s}.i32({t} %x, i32 %n)"), {
                let x = if ty == H { "(float)x" } else { "x" };
                format!("{}({x}, n)", ty.libm("ldexp"))
            }),
            (
                "fma",
                format!("call {t} @llvm.fma.{s}({t} %x, {t} %y, {t} %x)"),
                if ty == H {
                    "fma((double)x, (double)y, (double)x)".into()
                } else {
                    format!("{}(x, y, x)", ty.libm("fma"))
                },
            ),
            (
                "maxnum",
                format!("call {t} @llvm.maxnum.{s}({t} %x, {t} %y)"),
                c_call(ty, "fmax", 2).replace("a0", "x").replace("a1", "y"),
            ),
        ];
        for (name, call, c) in calls {
            // Parameters: x, y (ty), d (double), n (i32), k (i64).
            // Before the call: ten values of each class; after it, all
            // of them summed in a fixed order into the result (double).
            let mut body = String::new();
            let mut cexpr = String::from("({ ");
            let ext = |v: &str| -> String {
                match ty {
                    D => format!("fadd double {v}, 0.0"),
                    Q => format!("fptrunc {t} {v} to double"),
                    _ => format!("fpext {t} {v} to double"),
                }
            };
            let _ = writeln!(body, "  %x = fadd {t} %a0, %a1\n  %y = fsub {t} %a0, %a1");
            let _ = writeln!(body, "  %n = and i32 %a3, 63");
            let _ = write!(
                cexpr,
                "{0} x = ({0})(a0 + a1); {0} y = ({0})(a0 - a1); int n = (int)(a3 & 63); ",
                ty.c()
            );
            for k in 0..10 {
                let _ = writeln!(body, "  %d{k} = fmul double %a2, {k}.0");
                let _ = writeln!(body, "  %i{k} = mul i64 %a4, {}", k + 3);
                let _ = write!(cexpr, "double d{k} = a2 * {k}.0; uint64_t i{k} = a4 * {}; ", k + 3);
            }
            let _ = writeln!(body, "  %c = {call}");
            let _ = write!(cexpr, "{} cr = ({})({c}); ", ty.c(), ty.c());
            let _ = writeln!(body, "  %c0 = {}", ext("%c"));
            let _ = write!(cexpr, "double acc = (double)cr; uint64_t iacc = 0; ");
            let mut acc = "%c0".to_string();
            let mut iacc = "0".to_string();
            for k in 0..10 {
                let _ = writeln!(body, "  %s{k} = fadd double {acc}, %d{k}");
                let _ = writeln!(body, "  %j{k} = xor i64 {iacc}, %i{k}");
                let _ = write!(cexpr, "acc = acc + d{k}; iacc = iacc ^ i{k}; ");
                acc = format!("%s{k}");
                iacc = format!("%j{k}");
            }
            let _ = writeln!(body, "  %if = uitofp i64 {iacc} to double");
            let _ = writeln!(body, "  %r = fadd double {acc}, %if\n  ret double %r");
            let _ = write!(cexpr, "acc + (double)iacc; }})");
            let decl = match name {
                "sin" => format!("declare {t} @llvm.sin.{s}({t})"),
                "pow" => format!("declare {t} @llvm.pow.{s}({t}, {t})"),
                "powi" => format!("declare {t} @llvm.powi.{s}.i32({t}, i32)"),
                "ldexp" => format!("declare {t} @llvm.ldexp.{s}.i32({t}, i32)"),
                "fma" => format!("declare {t} @llvm.fma.{s}({t}, {t}, {t})"),
                _ => format!("declare {t} @llvm.maxnum.{s}({t}, {t})"),
            };
            funcs.push(
                Func::new(format!("live_{name}_{s}"), D, &[ty, ty, D, I(32), I(64)], body)
                    .decl(decl)
                    .c(cexpr),
            );
        }
    }
    let report = run_native("a64-fpintr-live", &funcs, 30, 72);
    assert_eq!(report.c, report.calls);
}

/// What is not lowered is refused by name; the forms next to it compile.
#[test]
fn what_is_not_lowered_is_refused_by_name() {
    assert_refused(&[
        (
            "declare bfloat @llvm.fabs.bf16(bfloat)\n\
             define bfloat @f(bfloat %a) {\n  %r = call bfloat @llvm.fabs.bf16(bfloat %a)\n  ret bfloat %r\n}\n",
            fpintr::BFLOAT,
        ),
        (
            "declare i32 @llvm.fptosi.sat.i32.f128(fp128)\n\
             define i32 @f(fp128 %a) {\n  %r = call i32 @llvm.fptosi.sat.i32.f128(fp128 %a)\n  ret i32 %r\n}\n",
            fpintr::FP128,
        ),
        (
            "declare double @llvm.powi.f64.i64(double, i64)\n\
             define double @f(double %a, i64 %n) {\n  %r = call double @llvm.powi.f64.i64(double %a, i64 %n)\n  ret double %r\n}\n",
            intr::INTRINSIC,
        ),
        (
            "declare i32 @llvm.eh.typeid.for.p0(ptr)\n\
             define i32 @f(ptr %p) {\n  %r = call i32 @llvm.eh.typeid.for.p0(ptr %p)\n  ret i32 %r\n}\n",
            intr::TYPEID,
        ),
    ]);
    for ok in [
        "declare fp128 @llvm.fabs.f128(fp128)\n\
         define fp128 @f(fp128 %a) {\n  %r = call fp128 @llvm.fabs.f128(fp128 %a)\n  ret fp128 %r\n}\n",
        "declare i32 @llvm.fptosi.sat.i32.f64(double)\n\
         define i32 @f(double %a) {\n  %r = call i32 @llvm.fptosi.sat.i32.f64(double %a)\n  ret i32 %r\n}\n",
        "declare void @llvm.clear_cache(ptr, ptr)\n\
         define void @f(ptr %a, ptr %b) {\n  call void @llvm.clear_cache(ptr %a, ptr %b)\n  ret void\n}\n",
    ] {
        assert_eq!(compile_ll(ok), Ok(()), "{ok}");
    }
}
