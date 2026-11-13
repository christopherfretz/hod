//! The x86-64 `x86_fp80` group (WP-131, `spec/codegen.md` §5.6.12):
//! `fadd` .. `frem`, `fneg`, `fcmp` (as a value and fused into `select`
//! and `br`), every conversion, `select`, `phi`, `freeze`, the constants,
//! `load` and `store`, the intrinsics, and the calling convention
//! (arguments in memory, results in `st0`/`st1`), checked against
//! `hod_interp` and `llc -O0 -fast-isel=0` (LLVM 22 and 19) under the
//! emulator (`common/x87_gen.rs`), against C, and with objects of hod and
//! `llc` linked together; the flags rule and the empty x87 stack on the
//! disassembly; what is refused, by name.

#![allow(
    clippy::too_many_lines,
    clippy::field_reassign_with_default,
    reason = "a test is a list of cases"
)]

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

#[path = "common/x87_gen.rs"]
mod x87_gen;

use hod_cg::{CgError, Stress};
use x87_gen::{Check, Rng, Suite, T, cases, check, disassembly, ll_header, mnemonic, parse, pic};

const MAJORS: [u32; 2] = [22, 19];

const X: T = T::X;

/// The 16 predicates of `fcmp`.
const PREDICATES: [&str; 16] = [
    "false", "oeq", "ogt", "oge", "olt", "ole", "one", "ord", "ueq", "ugt", "uge", "ult", "ule",
    "une", "uno", "true",
];

/// A short name of a type for function names.
fn tag(t: T) -> String {
    match t {
        T::X => "x".into(),
        T::H => "h".into(),
        T::B => "b".into(),
        T::F => "f".into(),
        T::D => "d".into(),
        T::Q => "q".into(),
        T::I(bits) => format!("i{bits}"),
    }
}

/// `fadd`, `fsub`, `fmul`, `fdiv`, `frem` and `fneg`, every pair of edge
/// values and random ones; an operation whose operands are one value.
#[test]
fn every_binary_operation_agrees() {
    let mut rng = Rng(310);
    let mut suite = Suite::default();
    for op in ["fadd", "fsub", "fmul", "fdiv", "frem"] {
        let f =
            suite.add(op, &[X, X], X, &format!("  %r = {op} x86_fp80 %a0, %a1\n"), Check::Interp);
        cases(&mut suite, &mut rng, f, &[X, X], 5000, 600);
        let f = suite.add(
            &format!("{op}_same"),
            &[X],
            X,
            &format!("  %r = {op} x86_fp80 %a0, %a0\n"),
            Check::Interp,
        );
        cases(&mut suite, &mut rng, f, &[X], 100, 20);
    }
    let f = suite.add("fneg", &[X], X, "  %r = fneg x86_fp80 %a0\n", Check::Interp);
    cases(&mut suite, &mut rng, f, &[X], 100, 50);
    // A chain: the stack is empty between the instructions.
    let f = suite.add(
        "chain",
        &[X, X, X],
        X,
        "  %s = fmul x86_fp80 %a0, %a1\n  %t = fsub x86_fp80 %s, %a2\n  \
         %u = fdiv x86_fp80 %t, %a0\n  %v = fneg x86_fp80 %u\n  %r = fadd x86_fp80 %v, %s\n",
        Check::Interp,
    );
    suite.funcs[f].llc_nan = true;
    cases(&mut suite, &mut rng, f, &[X, X, X], 400, 200);
    check("binary", &suite, &MAJORS);
}

/// Every predicate: as a value, fused into `select` and `br`, and used
/// twice (as a value and by a `select`).
#[test]
fn every_compare_agrees() {
    check("compare", &compare_suite(&mut Rng(311)), &MAJORS);
}

fn compare_suite(rng: &mut Rng) -> Suite {
    let mut suite = Suite::default();
    for p in PREDICATES {
        let value = suite.add(
            &format!("fcmp_{p}"),
            &[X, X],
            T::I(1),
            &format!("  %r = fcmp {p} x86_fp80 %a0, %a1\n"),
            Check::Interp,
        );
        let select = suite.add(
            &format!("select_{p}"),
            &[X, X],
            X,
            &format!(
                "  %c = fcmp {p} x86_fp80 %a0, %a1\n  %r = select i1 %c, x86_fp80 %a0, x86_fp80 %a1\n"
            ),
            Check::Interp,
        );
        let br = suite.add(
            &format!("br_{p}"),
            &[X, X],
            X,
            &format!(
                "  %c = fcmp {p} x86_fp80 %a0, %a1\n  br i1 %c, label %yes, label %no\n\
                 yes:\n  br label %join\nno:\n  br label %join\n\
                 join:\n  %r = phi x86_fp80 [ %a1, %yes ], [ %a0, %no ]\n"
            ),
            Check::Interp,
        );
        let twice = suite.add(
            &format!("twice_{p}"),
            &[X, X],
            T::I(32),
            &format!(
                "  %c = fcmp {p} x86_fp80 %a0, %a1\n  %z = zext i1 %c to i32\n  \
                 %s = select i1 %c, i32 7, i32 3\n  %r = add i32 %z, %s\n"
            ),
            Check::Interp,
        );
        for f in [value, select, br, twice] {
            cases(&mut suite, rng, f, &[X, X], 400, 40);
        }
    }
    suite
}

/// `fpext` to and `fptrunc` from `x86_fp80`, from and to `half`,
/// `bfloat` (`fpext` only), `float`, `double` and `fp128`.
#[test]
fn every_floating_conversion_agrees() {
    let mut rng = Rng(312);
    let mut suite = Suite::default();
    for t in [T::H, T::B, T::F, T::D, T::Q] {
        let up = t.bits() < 80;
        let check = if matches!(t, T::F | T::D) { Check::Interp } else { Check::Llc };
        let f = suite.add(
            &format!("from_{}", tag(t)),
            &[t],
            X,
            &format!(
                "  %r = {} {} %a0 to x86_fp80\n",
                if up { "fpext" } else { "fptrunc" },
                t.ll()
            ),
            check,
        );
        cases(&mut suite, &mut rng, f, &[t], 200, 300);
        if t == T::B {
            continue;
        }
        let f = suite.add(
            &format!("to_{}", tag(t)),
            &[X],
            t,
            &format!(
                "  %r = {} x86_fp80 %a0 to {}\n",
                if up { "fptrunc" } else { "fpext" },
                t.ll()
            ),
            check,
        );
        cases(&mut suite, &mut rng, f, &[X], 200, 600);
    }
    check("fpconv", &suite, &MAJORS);
}

/// The integer widths of the conversions.
const WIDTHS: [u32; 11] = [1, 8, 16, 31, 32, 33, 63, 64, 65, 100, 128];

/// `fptosi`, `fptoui`, `sitofp`, `uitofp` with every width: the
/// interpreter decides which conversions are poison (not run).
#[test]
fn every_integer_conversion_agrees() {
    let mut rng = Rng(313);
    let mut suite = Suite::default();
    for bits in WIDTHS {
        let i = T::I(bits);
        for op in ["fptosi", "fptoui"] {
            let f = suite.add(
                &format!("{op}_{bits}"),
                &[X],
                i,
                &format!("  %r = {op} x86_fp80 %a0 to i{bits}\n"),
                Check::Interp,
            );
            cases(&mut suite, &mut rng, f, &[X], 200, 300);
            // Values in range: an integer converted there and back.
            for _ in 0..200 {
                let n = x87_gen::random(&mut rng, i);
                suite.case(f, &int_as_x(&n, bits, op == "fptosi"));
            }
        }
        for op in ["sitofp", "uitofp"] {
            let f = suite.add(
                &format!("{op}_{bits}"),
                &[i],
                X,
                &format!("  %r = {op} i{bits} %a0 to x86_fp80\n"),
                Check::Interp,
            );
            cases(&mut suite, &mut rng, f, &[i], 200, 300);
        }
    }
    check("intconv", &suite, &MAJORS);
}

/// The `x86_fp80` (words) of the integer `n` (words, `bits` wide, signed
/// or not) truncated to 64 significant bits, plus a fraction below one
/// (`0.75` of the last place where there is room): a value in range.
fn int_as_x(n: &[u64], bits: u32, signed: bool) -> Vec<u64> {
    let lo = u128::from(n[0]);
    let hi = u128::from(*n.get(1).unwrap_or(&0));
    let mut v = if bits > 64 { lo | hi << 64 } else { lo };
    let mut negative = false;
    if signed && bits > 0 && v >> (bits - 1) & 1 == 1 {
        let full = if bits == 128 { u128::MAX } else { (1u128 << bits) - 1 };
        v = (!v & full) + 1;
        negative = true;
    }
    if v == 0 {
        return vec![0xc000_0000_0000_0000, 0x3ffe | u64::from(negative) << 15];
    }
    let top = v.ilog2();
    let (m, e) = if top >= 63 {
        ((v >> (top - 63)) as u64, top)
    } else {
        // Room for a fraction: 0.5 below the integer.
        let m = (v << (63 - top)) as u64 | (1 << (62 - top));
        (m, top)
    };
    vec![m, (16383 + u64::from(e)) | u64::from(negative) << 15]
}

/// `select` on a value, `freeze`, `phi` in a loop, the constants, `load`
/// and `store` (one through the other), a value used after a call.
#[test]
fn selects_phis_freezes_constants_and_memory_agree() {
    let mut rng = Rng(314);
    let mut suite = Suite::default();
    let f = suite.add(
        "sel",
        &[T::I(1), X, X],
        X,
        "  %r = select i1 %a0, x86_fp80 %a1, x86_fp80 %a2\n",
        Check::Interp,
    );
    cases(&mut suite, &mut rng, f, &[T::I(1), X, X], 200, 20);
    let f = suite.add(
        "freeze",
        &[X],
        X,
        "  %f = freeze x86_fp80 %a0\n  %r = fadd x86_fp80 %f, %f\n",
        Check::Interp,
    );
    cases(&mut suite, &mut rng, f, &[X], 100, 10);
    let f = suite.add(
        "loop",
        &[X, X],
        X,
        "  br label %head\nhead:\n  %i = phi i32 [ 0, %0 ], [ %j, %head ]\n  \
         %x = phi x86_fp80 [ %a0, %0 ], [ %y, %head ]\n  %m = fmul x86_fp80 %x, %a1\n  \
         %y = fadd x86_fp80 %m, %a0\n  %j = add i32 %i, 1\n  %c = icmp ult i32 %j, 3\n  \
         br i1 %c, label %head, label %exit\nexit:\n  %r = fadd x86_fp80 %y, %x\n",
        Check::Interp,
    );
    cases(&mut suite, &mut rng, f, &[X, X], 200, 40);
    // A swap through a `phi` pair: both values move at once.
    let f = suite.add(
        "swap",
        &[X, X],
        X,
        "  br label %head\nhead:\n  %i = phi i32 [ 0, %0 ], [ %j, %head ]\n  \
         %x = phi x86_fp80 [ %a0, %0 ], [ %y, %head ]\n  \
         %y = phi x86_fp80 [ %a1, %0 ], [ %x, %head ]\n  %j = add i32 %i, 1\n  \
         %c = icmp ult i32 %j, 3\n  br i1 %c, label %head, label %exit\n\
         exit:\n  %r = fsub x86_fp80 %x, %y\n",
        Check::Interp,
    );
    cases(&mut suite, &mut rng, f, &[X, X], 100, 10);
    let consts = [
        "0xK00000000000000000000",
        "0xK80000000000000000000",
        "0xK3FFF8000000000000000",
        "0xK3FFFC000000000000000",
        "0xK4000C90FDAA22168C235",
        "0xK7FFFC000000000000001",
        "0xK7FFF8000000000000000",
        "0xK00000000000000000001",
        "0xK7FFEFFFFFFFFFFFFFFFF",
    ];
    for (k, c) in consts.iter().enumerate() {
        let f = suite.add(
            &format!("const{k}"),
            &[X],
            X,
            &format!("  %r = fmul x86_fp80 %a0, {c}\n"),
            Check::Interp,
        );
        // `llc` folds `x * 1.0` to `x`, a signalling NaN unquieted and a
        // pseudo-denormal kept (allowed); hod multiplies.
        suite.funcs[f].llc_nan = true;
        cases(&mut suite, &mut rng, f, &[X], 30, 5);
        suite.cases.retain(|c| c.func != f || !pseudo_denormal(c.args[0], c.args[1]));
        let f = suite.add(
            &format!("ret{k}"),
            &[],
            X,
            &format!("  %r = fadd x86_fp80 {c}, 0xK00000000000000000000\n"),
            Check::Interp,
        );
        suite.case(f, &[]);
        let f = suite.add(
            &format!("bits{k}"),
            &[],
            X,
            &format!("  %r = select i1 true, x86_fp80 {c}, x86_fp80 0xK00000000000000000000\n"),
            Check::Interp,
        );
        suite.case(f, &[]);
    }
    let f = suite.add(
        "memory",
        &[X, X],
        X,
        "  %p = alloca [3 x x86_fp80], align 16\n  \
         %q = getelementptr [3 x x86_fp80], ptr %p, i64 0, i64 1\n  \
         store x86_fp80 %a0, ptr %p, align 16\n  store x86_fp80 %a1, ptr %q, align 16\n  \
         %u = load x86_fp80, ptr %q, align 16\n  %v = load x86_fp80, ptr %p, align 16\n  \
         %r = fdiv x86_fp80 %u, %v\n",
        Check::Interp,
    );
    cases(&mut suite, &mut rng, f, &[X, X], 100, 20);
    // Unaligned and packed: the 10 bytes at offset 3.
    let f = suite.add(
        "packed",
        &[X],
        X,
        "  %p = alloca [32 x i8], align 16\n  %q = getelementptr i8, ptr %p, i64 3\n  \
         store x86_fp80 %a0, ptr %q, align 1\n  %r = load x86_fp80, ptr %q, align 1\n",
        Check::Interp,
    );
    cases(&mut suite, &mut rng, f, &[X], 100, 20);
    check("select", &suite, &MAJORS);
}

/// The intrinsics: the inline ones (`fabs`, `copysign`, `sqrt`,
/// `canonicalize`, `fmuladd`, `lrint`, `llrint`, `is.fpclass`, the
/// saturating conversions) and those that call the runtime (`fmodl`'s
/// family: the rounding functions, `fma`, `minnum`, the transcendental
/// functions, `powi`, `ldexp`, `frexp`, `lround`).
#[test]
fn every_intrinsic_agrees() {
    let mut rng = Rng(315);
    let mut suite = Suite::default();
    let unary = [
        ("fabs", Check::Interp),
        ("sqrt", Check::Interp),
        ("canonicalize", Check::Llc),
        ("arithmetic.fence", Check::Interp),
        ("floor", Check::Interp),
        ("ceil", Check::Interp),
        ("trunc", Check::Interp),
        ("round", Check::Interp),
        ("roundeven", Check::Interp),
        ("rint", Check::Interp),
        ("nearbyint", Check::Interp),
        ("sin", Check::Llc),
        ("cos", Check::Llc),
        ("tan", Check::Llc),
        ("exp", Check::Llc),
        ("exp2", Check::Llc),
        ("log", Check::Llc),
        ("log2", Check::Llc),
        ("log10", Check::Llc),
        ("asin", Check::Llc),
        ("acos", Check::Llc),
        ("atan", Check::Llc),
        ("sinh", Check::Llc),
        ("cosh", Check::Llc),
        ("tanh", Check::Llc),
        ("exp10", Check::Llc),
    ];
    for (name, check) in unary {
        let ty = "x86_fp80";
        suite.declare(&format!("declare {ty} @llvm.{name}.f80({ty})"));
        let f = suite.add(
            &format!("i_{}", name.replace('.', "_")),
            &[X],
            X,
            &format!("  %r = call {ty} @llvm.{name}.f80({ty} %a0)\n"),
            check,
        );
        // LLVM 19 cannot select `canonicalize` of `x86_fp80`.
        if name == "canonicalize" {
            suite.funcs[f].since = 22;
        }
        cases(&mut suite, &mut rng, f, &[X], 100, 100);
    }
    for (name, check) in [
        ("copysign", Check::Interp),
        ("minnum", Check::Interp),
        ("maxnum", Check::Interp),
        ("pow", Check::Llc),
        ("atan2", Check::Llc),
        ("minimum", Check::Interp),
        ("maximum", Check::Interp),
        ("minimumnum", Check::Interp),
        ("maximumnum", Check::Interp),
    ] {
        suite.declare(&format!("declare x86_fp80 @llvm.{name}.f80(x86_fp80, x86_fp80)"));
        let f = suite.add(
            &format!("i_{name}"),
            &[X, X],
            X,
            &format!("  %r = call x86_fp80 @llvm.{name}.f80(x86_fp80 %a0, x86_fp80 %a1)\n"),
            check,
        );
        match name {
            // `llc` 19 calls `llvm.atan2.f80` by that name.
            "atan2" => suite.funcs[f].since = 22,
            // `llc` cannot select them ("Cannot select: X86ISD::FCMP").
            "minimum" | "maximum" => suite.funcs[f].llc = false,
            // `llc` 19 calls `llvm.minimumnum.f80`; 22 is inline, and
            // may return another NaN than the C library.
            "minimumnum" | "maximumnum" => {
                suite.funcs[f].since = 22;
                suite.funcs[f].llc_nan = true;
            }
            _ => {}
        }
        cases(&mut suite, &mut rng, f, &[X, X], 300, 100);
        // `llc` 22's inline sequence passes an invalid encoding through
        // where the C library returns the default NaN.
        if matches!(name, "minimumnum" | "maximumnum") {
            suite.cases.retain(|c| {
                c.func != f || !(invalid(c.args[0], c.args[1]) || invalid(c.args[2], c.args[3]))
            });
        }
    }
    for name in ["fma", "fmuladd"] {
        suite.declare(&format!("declare x86_fp80 @llvm.{name}.f80(x86_fp80, x86_fp80, x86_fp80)"));
        let f = suite.add(
            &format!("i_{name}"),
            &[X, X, X],
            X,
            &format!(
                "  %r = call x86_fp80 @llvm.{name}.f80(x86_fp80 %a0, x86_fp80 %a1, x86_fp80 %a2)\n"
            ),
            // `fmuladd` is unfused here as in `llc`; the interpreter may
            // fuse it.
            if name == "fma" { Check::Interp } else { Check::Llc },
        );
        cases(&mut suite, &mut rng, f, &[X, X, X], 300, 100);
    }
    for (name, bits) in
        [("lrint", 64), ("llrint", 64), ("lround", 64), ("llround", 64), ("lrint", 32)]
    {
        suite.declare(&format!("declare i{bits} @llvm.{name}.i{bits}.f80(x86_fp80)"));
        let f = suite.add(
            &format!("i_{name}{bits}"),
            &[X],
            T::I(bits),
            &format!("  %r = call i{bits} @llvm.{name}.i{bits}.f80(x86_fp80 %a0)\n"),
            Check::Llc,
        );
        let in_range: Vec<[u64; 2]> = x87_gen::x_edges()
            .into_iter()
            .filter(|w| w[1] & 0x7fff < 16383 + u64::from(bits) - 1)
            .collect();
        for w in in_range {
            suite.case(f, &w);
        }
    }
    for op in ["fptosi", "fptoui"] {
        for bits in [1, 8, 16, 32, 33, 64] {
            suite.declare(&format!("declare i{bits} @llvm.{op}.sat.i{bits}.f80(x86_fp80)"));
            let f = suite.add(
                &format!("i_{op}_sat{bits}"),
                &[X],
                T::I(bits),
                &format!("  %r = call i{bits} @llvm.{op}.sat.i{bits}.f80(x86_fp80 %a0)\n"),
                Check::Interp,
            );
            cases(&mut suite, &mut rng, f, &[X], 200, 200);
        }
    }
    for mask in [0x3ff, 1, 2, 3, 4, 8, 0x10, 0x20, 0x40, 0x80, 0x100, 0x200, 0x204, 0x1f8] {
        suite.declare("declare i1 @llvm.is.fpclass.f80(x86_fp80, i32)");
        let f = suite.add(
            &format!("i_fpclass{mask}"),
            &[X],
            T::I(1),
            &format!("  %r = call i1 @llvm.is.fpclass.f80(x86_fp80 %a0, i32 {mask})\n"),
            Check::Interp,
        );
        cases(&mut suite, &mut rng, f, &[X], 100, 20);
        // A pseudo-denormal's class in `llc` depends on the mask's
        // expansion and its version (`spec/codegen.md` §5.6.12).
        suite.cases.retain(|c| c.func != f || !pseudo_denormal(c.args[0], c.args[1]));
    }
    suite.declare("declare x86_fp80 @llvm.powi.f80.i32(x86_fp80, i32)");
    let f = suite.add(
        "i_powi",
        &[X, T::I(32)],
        X,
        "  %r = call x86_fp80 @llvm.powi.f80.i32(x86_fp80 %a0, i32 %a1)\n",
        Check::Llc,
    );
    for _ in 0..200 {
        let mut x = x87_gen::x_random(&mut rng).to_vec();
        x.push(rng.below(41).wrapping_sub(20) & 0xffff_ffff);
        suite.case(f, &x);
    }
    suite.declare("declare x86_fp80 @llvm.ldexp.f80.i32(x86_fp80, i32)");
    let f = suite.add(
        "i_ldexp",
        &[X, T::I(32)],
        X,
        "  %r = call x86_fp80 @llvm.ldexp.f80.i32(x86_fp80 %a0, i32 %a1)\n",
        Check::Interp,
    );
    cases(&mut suite, &mut rng, f, &[X, T::I(32)], 400, 200);
    suite.declare("declare { x86_fp80, i32 } @llvm.frexp.f80.i32(x86_fp80)");
    for (k, ret) in [(0, X), (1, T::I(32))] {
        let f = suite.add(
            &format!("i_frexp{k}"),
            &[X],
            ret,
            &format!(
                "  %p = call {{ x86_fp80, i32 }} @llvm.frexp.f80.i32(x86_fp80 %a0)\n  \
                 %r = extractvalue {{ x86_fp80, i32 }} %p, {k}\n"
            ),
            Check::Llc,
        );
        cases(&mut suite, &mut rng, f, &[X], 100, 100);
    }
    // The calls of the C library (`spec/codegen.md` §5.6.12).
    let inline = ["fabs", "sqrt", "canonicalize", "arithmetic_fence", "copysign", "fmuladd"];
    for f in &mut suite.funcs {
        let name = f.name.trim_start_matches("i_");
        f.libm = !inline.contains(&name)
            && !name.starts_with("lrint")
            && !name.starts_with("llrint")
            && !name.contains("_sat")
            && !name.starts_with("fpclass");
    }
    check("intrinsic", &suite, &MAJORS);
}

/// Whether the words `m`, `e` are an unnormal, a pseudo-zero, a
/// pseudo-infinity or a pseudo-NaN: a non-zero exponent without the
/// integer bit.
fn invalid(m: u64, e: u64) -> bool {
    e & 0x7fff != 0 && m & x87_gen::J == 0
}

/// Whether the words `m`, `e` are a pseudo-denormal.
fn pseudo_denormal(m: u64, e: u64) -> bool {
    e.trailing_zeros() >= 15 && m & x87_gen::J != 0
}

/// `llvm.is.fpclass` with every one of the 1024 masks, on one value of
/// each class and each encoding only the x87 has but the
/// pseudo-denormal: the classes as `llc` computes them, and
/// `x87::invalid_in` for the invalid encodings.
#[test]
fn is_fpclass_of_every_mask_agrees() {
    let values: [[u64; 2]; 18] = [
        [0, 0],
        [0, 0x8000],
        [1, 0],
        [1, 0x8000],
        [x87_gen::J, 0x3fff],
        [x87_gen::J, 0xbfff],
        [x87_gen::J, 0x7fff],
        [x87_gen::J, 0xffff],
        [0xc000_0000_0000_0000, 0x7fff],
        [x87_gen::J | 1, 0xffff],
        [0x4000_0000_0000_0000, 0x3fff],
        [0x4000_0000_0000_0000, 0xbfff],
        [0, 0x3fff],
        [0, 0x7fff],
        [0, 0xffff],
        [0x4000_0000_0000_0000, 0x7fff],
        [1, 0xffff],
        [0x7fff_ffff_ffff_ffff, 0x0001],
    ];
    let mut suite = Suite::default();
    suite.declare("declare i1 @llvm.is.fpclass.f80(x86_fp80, i32)");
    for mask in 0..1024 {
        let f = suite.add(
            &format!("c{mask}"),
            &[X],
            T::I(1),
            &format!("  %r = call i1 @llvm.is.fpclass.f80(x86_fp80 %a0, i32 {mask})\n"),
            Check::Llc,
        );
        for v in values {
            suite.case(f, &v);
        }
    }
    check("fpclass", &suite, &MAJORS);
}

/// The calling convention: `x86_fp80` arguments in memory (after the
/// registers run out and among them), results in `st0`, `{x86_fp80,
/// x86_fp80}` in `st0` and `st1`, `{x86_fp80, double}` in `st0` and
/// `xmm0`, a variadic call; callees by hod and `llc` in one module, apart
/// (hod calling `llc` and the reverse), and in C.
#[test]
fn calls_agree_with_llc_and_c() {
    let mut rng = Rng(316);
    let mut suite = Suite::default();
    suite.callees = "\
define x86_fp80 @c_mix(i64 %i, x86_fp80 %a, double %d, x86_fp80 %b, i32 %j) noinline {
  %ai = sitofp i64 %i to x86_fp80
  %dx = fpext double %d to x86_fp80
  %jx = sitofp i32 %j to x86_fp80
  %s = fadd x86_fp80 %a, %ai
  %t = fmul x86_fp80 %s, %b
  %u = fsub x86_fp80 %t, %dx
  %r = fadd x86_fp80 %u, %jx
  ret x86_fp80 %r
}

define { x86_fp80, x86_fp80 } @c_pair(x86_fp80 %a, x86_fp80 %b) noinline {
  %s = fadd x86_fp80 %a, %b
  %d = fsub x86_fp80 %a, %b
  %p = insertvalue { x86_fp80, x86_fp80 } poison, x86_fp80 %s, 0
  %q = insertvalue { x86_fp80, x86_fp80 } %p, x86_fp80 %d, 1
  ret { x86_fp80, x86_fp80 } %q
}

define { x86_fp80, double } @c_wide(x86_fp80 %a, double %d) noinline {
  %t = fptrunc x86_fp80 %a to double
  %e = fadd double %t, %d
  %p = insertvalue { x86_fp80, double } poison, x86_fp80 %a, 0
  %q = insertvalue { x86_fp80, double } %p, double %e, 1
  ret { x86_fp80, double } %q
}

define x86_fp80 @c_many(x86_fp80 %a, x86_fp80 %b, x86_fp80 %c, x86_fp80 %d, i64 %e, i64 %f, i64 %g, i64 %h, i64 %i, i64 %j, i64 %k) noinline {
  %s = fsub x86_fp80 %a, %b
  %t = fmul x86_fp80 %s, %c
  %u = fdiv x86_fp80 %t, %d
  %v = add i64 %e, %k
  %w = sitofp i64 %v to x86_fp80
  %r = fadd x86_fp80 %u, %w
  ret x86_fp80 %r
}

"
    .into();
    suite.externs = "\
declare x86_fp80 @c_mix(i64, x86_fp80, double, x86_fp80, i32)
declare { x86_fp80, x86_fp80 } @c_pair(x86_fp80, x86_fp80)
declare { x86_fp80, double } @c_wide(x86_fp80, double)
declare x86_fp80 @c_many(x86_fp80, x86_fp80, x86_fp80, x86_fp80, i64, i64, i64, i64, i64, i64, i64)
"
    .into();
    // C callees: the same functions, the C compiler's convention.
    suite.c = "\
long double cc_mix(long i, long double a, double d, long double b, int j) {
  return (a + (long double)i) * b - (long double)d + (long double)j;
}
/* `_Complex long double` is returned in `st0` and `st1`. */
_Complex long double cc_pair(long double a, long double b) {
  _Complex long double r;
  __real__ r = a + b;
  __imag__ r = a - b;
  return r;
}
#include <stdarg.h>
long double cc_sum(int n, ...) {
  va_list ap;
  va_start(ap, n);
  long double s = 0;
  for (int i = 0; i < n; i++) {
    long double x = va_arg(ap, long double);
    s = s * 2 + x;
  }
  va_end(ap);
  return s;
}
"
    .into();
    suite.declare("declare x86_fp80 @cc_mix(i64, x86_fp80, double, x86_fp80, i32)");
    suite.declare("declare { x86_fp80, x86_fp80 } @cc_pair(x86_fp80, x86_fp80)");
    suite.declare("declare x86_fp80 @cc_sum(i32, ...)");
    let mixed = |callee: &str| {
        format!(
            "  %r = call x86_fp80 @{callee}(i64 7, x86_fp80 %a0, double 0.5, x86_fp80 %a1, i32 -3)\n"
        )
    };
    // Called both ways round: hod's callers of hod's (and llc's) callees.
    for callee in ["c_mix", "cc_mix"] {
        let check = if callee == "c_mix" { Check::Interp } else { Check::Llc };
        let f = suite.add(&format!("t_{callee}"), &[X, X], X, &mixed(callee), check);
        cases(&mut suite, &mut rng, f, &[X, X], 100, 50);
    }
    for (callee, k) in [("c_pair", 0), ("c_pair", 1), ("cc_pair", 0), ("cc_pair", 1)] {
        let check = if callee == "c_pair" { Check::Interp } else { Check::Llc };
        let f = suite.add(
            &format!("t_{callee}{k}"),
            &[X, X],
            X,
            &format!(
                "  %p = call {{ x86_fp80, x86_fp80 }} @{callee}(x86_fp80 %a0, x86_fp80 %a1)\n  \
                 %r = extractvalue {{ x86_fp80, x86_fp80 }} %p, {k}\n"
            ),
            check,
        );
        cases(&mut suite, &mut rng, f, &[X, X], 100, 50);
    }
    for (k, ret) in [(0, X), (1, T::D)] {
        let ty = ret.ll();
        let f = suite.add(
            &format!("t_c_wide{k}"),
            &[X, T::D],
            ret,
            &format!(
                "  %p = call {{ x86_fp80, double }} @c_wide(x86_fp80 %a0, double %a1)\n  \
                 %r = extractvalue {{ x86_fp80, double }} %p, {k}\n  %unused = fadd {ty} %r, %r\n"
            ),
            Check::Interp,
        );
        cases(&mut suite, &mut rng, f, &[X, T::D], 100, 50);
    }
    let f = suite.add(
        "t_c_many",
        &[X, X, T::I(64)],
        X,
        "  %m = fneg x86_fp80 %a1\n  \
         %r = call x86_fp80 @c_many(x86_fp80 %a0, x86_fp80 %a1, x86_fp80 %m, x86_fp80 %a0, \
         i64 %a2, i64 1, i64 2, i64 3, i64 4, i64 5, i64 %a2)\n",
        Check::Interp,
    );
    cases(&mut suite, &mut rng, f, &[X, X, T::I(64)], 100, 50);
    let f = suite.add(
        "t_cc_sum",
        &[X, X],
        X,
        "  %r = call x86_fp80 (i32, ...) @cc_sum(i32 3, x86_fp80 %a0, x86_fp80 %a1, x86_fp80 %a0)\n",
        Check::Llc,
    );
    cases(&mut suite, &mut rng, f, &[X, X], 100, 50);
    // A value live across a call (its slot survives), and a recursive
    // call whose result is the operand of the next.
    let f = suite.add(
        "across",
        &[X, X],
        X,
        "  %s = fmul x86_fp80 %a0, %a1\n  \
         %c = call x86_fp80 @c_mix(i64 1, x86_fp80 %s, double 2.0, x86_fp80 %a1, i32 0)\n  \
         %d = call x86_fp80 @c_mix(i64 2, x86_fp80 %c, double 1.0, x86_fp80 %s, i32 1)\n  \
         %r = fsub x86_fp80 %d, %s\n",
        Check::Interp,
    );
    cases(&mut suite, &mut rng, f, &[X, X], 100, 50);
    let report = check("calls", &suite, &MAJORS);
    if !report.legs.is_empty() {
        assert!(report.legs.iter().any(|l| l == "mix hod/llc"), "{:?}", report.legs);
    }
}

// ---- the code itself ----

/// Whether `m` reads the flags: `setcc`, `cmovcc`, `fcmovcc`, `jcc`.
fn reads_flags(m: &str) -> bool {
    m.starts_with("set")
        || m.starts_with("cmov")
        || m.starts_with("fcmov")
        || (m.starts_with('j') && m != "jmp")
}

/// What may run between a flag setter and its reader (§5.6 "Flags",
/// §5.6.12: the x87 moves and `fstp` keep `EFLAGS`).
fn keeps_flags(m: &str) -> bool {
    matches!(
        m,
        "mov"
            | "movabs"
            | "lea"
            | "movzx"
            | "movsx"
            | "movsxd"
            | "push"
            | "pop"
            | "fld"
            | "fstp"
            | "fxch"
    ) || reads_flags(m)
}

/// §5.6 "Flags": in every compare function, plain and under every stress
/// option, each flag reader is reached from a `fucomi`/`fucomip`, a
/// `cmp`, a `test`, or another flag setter of the group, through
/// flag-keeping glue only.
#[test]
fn every_compare_keeps_the_flags() {
    let suite = compare_suite(&mut Rng(317));
    for stress in [Stress::default(), Stress::ALL] {
        let Some(lines) = disassembly(&suite.text, stress) else { return };
        let mut readers = 0;
        for (at, line) in lines.iter().enumerate() {
            if !reads_flags(mnemonic(line)) {
                continue;
            }
            readers += 1;
            let setter = lines[..at]
                .iter()
                .rev()
                .find(|l| !keeps_flags(mnemonic(l)))
                .map_or("", |l| mnemonic(l));
            assert!(
                matches!(setter, "cmp" | "test" | "fucomi" | "fucompi" | "or" | "and" | "bt"),
                "{line} is reached from {setter}:\n{}",
                lines[at.saturating_sub(12)..=at].join("\n")
            );
        }
        assert!(readers > 40, "{readers} readers");
    }
}

/// The x87 stack is empty between IR instructions: in straight-line
/// code of every group, each `fld`/`fild` is matched by a pop before the
/// next non-x87 instruction that is not glue, counted on the
/// disassembly (pushes: `fld*`, `fild`; pops: `fstp`, `fistp`, the `p`
/// arithmetic, `fucomip`).
#[test]
fn the_stack_is_empty_between_instructions() {
    let mut suite = Suite::default();
    for op in ["fadd", "fsub", "fmul", "fdiv"] {
        suite.add(op, &[X, X], X, &format!("  %r = {op} x86_fp80 %a0, %a1\n"), Check::Interp);
    }
    for (op, to) in
        [("fptosi", T::I(64)), ("fptoui", T::I(64)), ("fptoui", T::I(32)), ("fptrunc", T::D)]
    {
        suite.add(
            &format!("{op}_{}", tag(to)),
            &[X],
            to,
            &format!("  %r = {op} x86_fp80 %a0 to {}\n", to.ll()),
            Check::Interp,
        );
    }
    suite.add("u64", &[T::I(64)], X, "  %r = uitofp i64 %a0 to x86_fp80\n", Check::Interp);
    let mut text = suite.text.clone();
    text.push_str(&compare_suite(&mut Rng(318)).text);
    let Some(lines) = disassembly(&text, Stress::default()) else { return };
    let mut depth: i32 = 0;
    for line in &lines {
        let m = mnemonic(line);
        let push = (m.starts_with("fld") && m != "fldcw") || m == "fild";
        let pop = m == "fstp"
            || m == "fistp"
            || m == "fucompi"
            || (m.starts_with('f') && m.ends_with('p') && m.len() <= 6 && m != "fxch");
        depth += i32::from(push) - i32::from(pop);
        assert!((0..=2).contains(&depth), "depth {depth} at {line}");
        if m == "ret" || m == "call" {
            assert!(depth == 0 || m == "ret", "the stack is not empty at {line}");
        }
    }
}

/// No load folding into arithmetic beyond the x87 memory forms, no SSE
/// above SSE2, no `fcomi` (the signalling compare): the compares are
/// quiet.
#[test]
fn compares_are_quiet() {
    let Some(lines) = disassembly(&compare_suite(&mut Rng(319)).text, Stress::default()) else {
        return;
    };
    for line in &lines {
        let m = mnemonic(line);
        assert!(!matches!(m, "fcomi" | "fcompi" | "fcom" | "fcomp"), "{line}");
        assert!(!m.starts_with('v'), "{line}");
    }
}

/// What the group does not lower, by name, one construct per program.
#[test]
fn what_is_not_lowered_is_refused_by_name() {
    for (text, name) in [
        (
            "define bfloat @f(x86_fp80 %a) {\n  %r = fptrunc x86_fp80 %a to bfloat\n  \
             ret bfloat %r\n}\n",
            hod_x64::x87::BFLOAT,
        ),
        (
            "declare x86_fp80 @llvm.powi.f80.i64(x86_fp80, i64)\n\
             define x86_fp80 @f(x86_fp80 %a, i64 %n) {\n  \
             %r = call x86_fp80 @llvm.powi.f80.i64(x86_fp80 %a, i64 %n)\n  ret x86_fp80 %r\n}\n",
            hod_x64::x87::INTRINSIC,
        ),
        (
            "declare i128 @llvm.lrint.i128.f80(x86_fp80)\n\
             define i128 @f(x86_fp80 %a) {\n  \
             %r = call i128 @llvm.lrint.i128.f80(x86_fp80 %a)\n  ret i128 %r\n}\n",
            hod_x64::x87::INTRINSIC,
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

/// The edge values hold what `x87_gen` documents.
#[test]
fn the_edges_hold_the_boundaries() {
    let e = x87_gen::x_edges();
    for w in [
        [0, 0x8000],
        [1, 0],
        [0x8000_0000_0000_0001, 0],
        [0x4000_0000_0000_0000, 0x3fff],
        [0x8000_0000_0000_0000, 0x7fff],
        [0xc000_0000_0000_0000, 0xffff],
        [0x8000_0000_0000_0001, 0x7fff],
        [0, 0x7fff],
        [0x8000_0000_0000_0000, 16383 + 63],
        [u64::MAX, 16383 + 62],
    ] {
        assert!(e.contains(&w), "{w:#x?}");
    }
    assert_eq!(x87_gen::x_of(1.0), [0x8000_0000_0000_0000, 0x3fff]);
    assert_eq!(x87_gen::x_of(f64::from_bits(1)), [0x8000_0000_0000_0000, 16383 - 1074]);
    assert_eq!(int_as_x(&[1], 64, false), vec![0xc000_0000_0000_0000, 0x3fff]);
}
