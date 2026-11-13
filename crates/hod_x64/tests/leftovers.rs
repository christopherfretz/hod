//! The x86-64 leftovers between the instruction groups (WP-144,
//! `spec/codegen.md` §5.6.14): floating-point members of aggregates,
//! conversions between floating point and integers of 65 to 128 bits that
//! are not `i128`, the floating-point `atomicrmw` operations, and the
//! other small forms the groups left. Each runs under the emulator against
//! `hod_interp` (where it has the type) and `llc -O0 -fast-isel=0` of
//! LLVM 22 and 19 (`common/float_gen.rs`).

#![allow(
    clippy::too_many_lines,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "tables of functions and values"
)]

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

#[path = "common/float_gen.rs"]
mod float_gen;

#[path = "common/aggr_gen.rs"]
mod aggr_gen;

use float_gen::{Check, Rng, Suite, T, cases, check};

const MAJORS: [u32; 2] = [22, 19];

/// The floating-point types, with who checks them.
const FP_TYPES: [(T, Check); 5] = [
    (T::F, Check::Interp),
    (T::D, Check::Interp),
    (T::H, Check::Llc),
    (T::B, Check::Llc),
    (T::Q, Check::Llc),
];

/// A short name of a type, for function names.
fn tag(t: T) -> String {
    match t {
        T::H => "h".into(),
        T::B => "b".into(),
        T::F => "f".into(),
        T::D => "d".into(),
        T::Q => "q".into(),
        T::I(bits) => format!("i{bits}"),
    }
}

/// A constant of `t` that is not zero (1.5).
fn one_and_a_half(t: T) -> &'static str {
    match t {
        T::H => "0xH3E00",
        T::B => "0xR3FC0",
        T::F | T::D => "1.5",
        T::Q => "0xL00000000000000003FFF800000000000",
        T::I(_) => "3",
    }
}

// ---- 1. floating-point members of aggregates ----

/// §5.6.14 (1): `insertvalue` and `extractvalue` of a `half`, `bfloat`,
/// `float`, `double` or `fp128` member, in aligned, packed (misaligned),
/// nested and array aggregates; built from `undef`, `zeroinitializer` and
/// `poison`, overwritten, filled with constants, and carried through
/// memory, a `select` and a `phi`. Every result is a member's bits (NaN
/// payloads and signalling NaNs included).
#[test]
fn floating_point_members_of_aggregates_agree() {
    let mut rng = Rng(144);
    let mut suite = Suite::default();
    for (t, check) in FP_TYPES {
        let tl = t.ll();
        // (aggregate, the member read, another member and what is
        // written there).
        let shapes = [
            (format!("{{ i8, {tl}, i32 }}"), "1", "2", "i32 7".to_string()),
            (format!("[3 x {tl}]"), "0", "2", format!("{tl} %a1")),
            (format!("<{{ i8, {tl}, i8, {tl} }}>"), "1", "3", format!("{tl} %a1")),
            (format!("{{ i16, {{ {tl}, i8 }}, {tl} }}"), "1, 0", "2", format!("{tl} %a1")),
            (format!("{{ {tl}, i128, {tl} }}"), "2", "0", format!("{tl} %a1")),
        ];
        for (k, (agg, p, q, other)) in shapes.iter().enumerate() {
            let name = |what: &str| format!("{what}{k}_{}", tag(t));
            let k15 = one_and_a_half(t);
            for base in ["undef", "zeroinitializer", "poison"] {
                // The first member written, the second written over it,
                // the first read back.
                let f = suite.add(
                    &name(&format!("first_{}_", &base[..1])),
                    &[t, t],
                    t,
                    &format!(
                        "  %g0 = insertvalue {agg} {base}, {tl} %a0, {p}\n  \
                         %g1 = insertvalue {agg} %g0, {other}, {q}\n  \
                         %r = extractvalue {agg} %g1, {p}\n"
                    ),
                    check,
                );
                cases(&mut suite, &mut rng, f, &[t, t], 120, 20);
            }
            // A member overwritten: the second value is read.
            let f = suite.add(
                &name("over"),
                &[t, t],
                t,
                &format!(
                    "  %g0 = insertvalue {agg} zeroinitializer, {tl} %a0, {p}\n  \
                     %g1 = insertvalue {agg} %g0, {tl} %a1, {p}\n  \
                     %r = extractvalue {agg} %g1, {p}\n"
                ),
                check,
            );
            cases(&mut suite, &mut rng, f, &[t, t], 120, 20);
            // A constant member, and the zero one beside it.
            for (what, value) in [("k", k15), ("z", "zeroinitializer")] {
                let f = suite.add(
                    &name(what),
                    &[t],
                    t,
                    &format!(
                        "  %g0 = insertvalue {agg} undef, {tl} %a0, {p}\n  \
                         %g1 = insertvalue {agg} %g0, {tl} {value}, {p}\n  \
                         %r = extractvalue {agg} %g1, {p}\n"
                    ),
                    check,
                );
                cases(&mut suite, &mut rng, f, &[t], 4, 2);
            }
            // Through memory: stored whole, loaded whole, read.
            let f = suite.add(
                &name("mem"),
                &[t, t],
                t,
                &format!(
                    "  %m = alloca {agg}, align 16\n  \
                     %g0 = insertvalue {agg} zeroinitializer, {tl} %a0, {p}\n  \
                     store {agg} %g0, ptr %m\n  %l = load {agg}, ptr %m\n  \
                     %g1 = insertvalue {agg} %l, {tl} %a1, {p}\n  \
                     store {agg} %g1, ptr %m\n  %l2 = load {agg}, ptr %m\n  \
                     %r = extractvalue {agg} %l2, {p}\n"
                ),
                check,
            );
            cases(&mut suite, &mut rng, f, &[t, t], 120, 20);
            // Chosen by a `select` on the second value's bits, merged by a
            // `phi`.
            let f = suite.add(
                &name("sel"),
                &[t, t],
                t,
                &format!(
                    "  %x = insertvalue {agg} undef, {tl} %a0, {p}\n  \
                     %y = insertvalue {agg} undef, {tl} %a1, {p}\n  \
                     %c = fcmp ord {tl} %a1, %a1\n  \
                     br label %k\nk:\n  \
                     %s = select i1 %c, {agg} %y, {agg} %x\n  br label %j\nj:\n  \
                     %m = phi {agg} [ %s, %k ]\n  %r = extractvalue {agg} %m, {p}\n"
                ),
                check,
            );
            if t == T::B {
                // `llc` quiets a signalling `bfloat` NaN that a `phi`
                // carries (WP-127's record): a model instead.
                suite.funcs[f].llc = false;
                suite.funcs[f].model =
                    Some(
                        |a| if T::B.is_nan(a[1] & 0xffff) { a[0] & 0xffff } else { a[1] & 0xffff },
                    );
            }
            cases(&mut suite, &mut rng, f, &[t, t], 120, 20);
        }
        // Read a member, compute with it, write it back.
        if matches!(t, T::F | T::D | T::H | T::Q) {
            let agg = format!("{{ {tl}, i64 }}");
            let f = suite.add(
                &format!("arith_{}", tag(t)),
                &[t, t],
                t,
                &format!(
                    "  %g0 = insertvalue {agg} undef, {tl} %a0, 0\n  \
                     %x = extractvalue {agg} %g0, 0\n  %y = fadd {tl} %x, %a1\n  \
                     %g1 = insertvalue {agg} %g0, {tl} %y, 0\n  \
                     %r = extractvalue {agg} %g1, 0\n"
                ),
                check,
            );
            cases(&mut suite, &mut rng, f, &[t, t], 200, 20);
        }
    }
    check("fp-members", &suite, &MAJORS);
}

// ---- 2. conversions with integers of 65 to 127 bits ----

/// §5.6.14 (2): `fptosi`, `fptoui`, `sitofp`, `uitofp` between `half`,
/// `float`, `double`, `fp128` and integers of 65 to 127 bits (`i128` is
/// WP-127's): the 128-bit routines, the operand extended to 128 bits.
/// An integer is made of two `i64` words (low, high); a result is folded
/// into 64 bits.
#[test]
fn conversions_with_odd_wide_integers_agree() {
    let mut rng = Rng(1442);
    let mut suite = Suite::default();
    for (t, check) in
        [(T::F, Check::Interp), (T::D, Check::Interp), (T::H, Check::Llc), (T::Q, Check::Llc)]
    {
        let tl = t.ll();
        for bits in [65, 80, 96, 100, 127] {
            let int = format!("i{bits}");
            for op in ["sitofp", "uitofp"] {
                let f = suite.add(
                    &format!("{op}_i{bits}_{}", tag(t)),
                    &[T::I(64), T::I(64)],
                    t,
                    &format!(
                        "  %wl = zext i64 %a0 to {int}\n  %wz = zext i64 %a1 to {int}\n  \
                         %ws = shl {int} %wz, 64\n  %v = or {int} %wl, %ws\n  \
                         %r = {op} {int} %v to {tl}\n"
                    ),
                    check,
                );
                cases(&mut suite, &mut rng, f, &[T::I(64), T::I(64)], 300, 100);
            }
            for op in ["fptosi", "fptoui"] {
                let f = suite.add(
                    &format!("{op}_{}_i{bits}", tag(t)),
                    &[t],
                    T::I(64),
                    &format!(
                        "  %v = {op} {tl} %a0 to {int}\n  %wl = trunc {int} %v to i64\n  \
                         %ws = lshr {int} %v, 64\n  %wh = trunc {int} %ws to i64\n  \
                         %wm = mul i64 %wh, -7046029254386353131\n  %r = xor i64 %wm, %wl\n"
                    ),
                    check,
                );
                // In range for the type (the others are poison).
                let max = f64::from(bits - u32::from(op == "fptosi")).exp2();
                let add = |suite: &mut Suite, x: f64| {
                    let fits = x.abs() < max && (op == "fptosi" || x > -1.0);
                    if !fits {
                        return;
                    }
                    let words = match t {
                        T::F => vec![u64::from((x as f32).to_bits())],
                        T::D => vec![x.to_bits()],
                        T::H => vec![half_bits(x)],
                        _ => {
                            let (lo, hi) = float_gen::quad_of(x);
                            vec![lo, hi]
                        }
                    };
                    suite.case(f, &words);
                };
                for x in [0.0, -0.0, 0.5, -0.5, 0.99, 1.0, -1.0, 1.5, -2.5, 65504.0, -65504.0] {
                    add(&mut suite, x);
                }
                for k in [31, 32, 52, 53, 63, 64, 65, 79, 95, 99, 100, 126, 127] {
                    for x in [f64::from(k).exp2(), -f64::from(k).exp2()] {
                        add(&mut suite, x * 0.75);
                        add(&mut suite, x);
                    }
                }
                for _ in 0..200 {
                    let e = (rng.below(u64::from(bits) + 4) as i32) - 4;
                    let m = (rng.next() >> 11) as f64 / (1u64 << 53) as f64 + 0.5;
                    let x = if rng.below(2) == 0 {
                        m * f64::from(e).exp2()
                    } else {
                        -m * f64::from(e).exp2()
                    };
                    // `half` holds only up to 65504.
                    let x = if t == T::H { x.clamp(-65504.0, 65504.0) } else { x };
                    add(&mut suite, x);
                }
            }
        }
    }
    check("wide-casts", &suite, &MAJORS);
}

/// The `half` bits of `x` (a value `half` holds exactly, or rounded
/// toward zero).
fn half_bits(x: f64) -> u64 {
    let sign = if x.is_sign_negative() { 0x8000 } else { 0 };
    let a = x.abs();
    if a == 0.0 {
        return sign;
    }
    let mut e = a.log2().floor() as i32;
    if e < -14 {
        e = -14;
    }
    let m = (a / f64::from(e).exp2() * 1024.0) as u64;
    let biased = if m < 1024 { 0 } else { (e + 15) as u64 };
    sign | (biased << 10) | (m & 0x3ff)
}

// ---- 4. floating-point atomicrmw ----

/// §5.6.14 (4): `atomicrmw` `fadd`, `fsub`, `fmax`, `fmin`, `fmaximum`,
/// `fminimum` of `half`, `float` and `double` (a `lock cmpxchg` loop;
/// `half` computed in `float` with inline conversions): the old value,
/// the new value in memory, and the new value when the result is dead.
/// `fmaximum`/`fminimum` are LLVM 22's (`llc` 19 does not read them).
#[test]
fn floating_point_atomicrmw_agrees() {
    let mut rng = Rng(1444);
    let mut suite = Suite::default();
    for (t, check) in [(T::F, Check::Interp), (T::D, Check::Interp), (T::H, Check::Llc)] {
        let tl = t.ll();
        for op in ["fadd", "fsub", "fmax", "fmin", "fmaximum", "fminimum"] {
            for (what, ordering) in [("old", "seq_cst"), ("new", "monotonic"), ("dead", "acq_rel")]
            {
                let rmw = format!("atomicrmw {op} ptr %cell, {tl} %a1 {ordering}");
                let body = match what {
                    "old" => format!("  %r = {rmw}\n"),
                    "new" => format!("  %o = {rmw}\n  %r = load {tl}, ptr %cell\n"),
                    _ => format!("  {rmw}\n  %r = load {tl}, ptr %cell\n"),
                };
                let f = suite.add(
                    &format!("rmw_{op}_{what}_{}", tag(t)),
                    &[t, t],
                    t,
                    &format!("  %cell = alloca {tl}, align 8\n  store {tl} %a0, ptr %cell\n{body}"),
                    // The interpreter has no `fmax` .. `fminimum`.
                    if op.contains("max") || op.contains("min") { Check::Llc } else { check },
                );
                if op.starts_with("fmaximum") || op.starts_with("fminimum") {
                    suite.funcs[f].since = 22;
                }
                cases(&mut suite, &mut rng, f, &[t, t], 800, 200);
                if t == T::H && !op.starts_with("fadd") && !op.starts_with("fsub") {
                    // `llc` computes a `half` minimum or maximum of the
                    // operand and the old value in the other order: where
                    // both are NaNs (the payload is open) or both zeros
                    // (`maxnum`/`minnum` may return either) it differs.
                    let open = |a: u64, b: u64| {
                        (T::H.is_nan(a) && T::H.is_nan(b))
                            || (op.ends_with("max") || op.ends_with("min"))
                                && ((a | b) & 0x7fff).trailing_zeros() >= 15
                    };
                    suite.cases.retain(|c| c.func != f || !open(c.args[0], c.args[1]));
                }
            }
        }
    }
    check("atomic-float", &suite, &MAJORS);
}

// ---- 16-byte atomics without cx16: libatomic ----

/// The `atomicrmw` operations of `i128`.
const WIDE_OPS: [&str; 15] = [
    "xchg",
    "add",
    "sub",
    "and",
    "nand",
    "or",
    "xor",
    "max",
    "min",
    "umax",
    "umin",
    "uinc_wrap",
    "udec_wrap",
    "usub_cond",
    "usub_sat",
];
/// The orderings of an `atomicrmw` or a `cmpxchg` success.
const WIDE_ORDERS: [&str; 5] = ["monotonic", "acquire", "release", "acq_rel", "seq_cst"];
/// The orderings of a `cmpxchg` failure.
const WIDE_FAIL: [&str; 3] = ["monotonic", "acquire", "seq_cst"];

/// One function: its core (defining `%o`, `%n` of `i128` and `%k` of
/// `i8` from `%a`, `%b`, `%c`) and its cases.
struct Wide {
    core: String,
    cases: Vec<[u128; 3]>,
}

fn wide_word(w: &[u64]) -> u128 {
    u128::from(w[0]) | (u128::from(w[1]) << 64)
}

/// Every operation under every ordering (result used, and unused),
/// the operations that compare on the edges, constant operands, loads,
/// stores, `cmpxchg` strong and weak under every pair of orderings.
fn wide_table(rng: &mut aggr_gen::Rng) -> Vec<Wide> {
    let e: Vec<u128> = aggr_gen::edges(128).iter().map(|w| wide_word(w)).collect();
    let mut cases = |same: bool| {
        let mut c: Vec<[u128; 3]> = Vec::new();
        for _ in 0..4 {
            let a = wide_word(&aggr_gen::value(rng, 128));
            let b = if same && rng.chance(50) { a } else { wide_word(&aggr_gen::value(rng, 128)) };
            c.push([a, b, wide_word(&aggr_gen::value(rng, 128))]);
        }
        c.push([*rng.pick(&e), *rng.pick(&e), 0]);
        c
    };
    let rmw = |op: &str, operand: &str, order: &str| {
        format!(
            "  store i128 %a, ptr %p, align 16\n  \
             %o = atomicrmw {op} ptr %p, i128 {operand} {order}, align 16\n  \
             %n = load i128, ptr %p, align 16\n  %k = add i8 0, 0\n"
        )
    };
    let mut out = Vec::new();
    for op in WIDE_OPS {
        for order in WIDE_ORDERS {
            out.push(Wide { core: rmw(op, "%b", order), cases: cases(true) });
        }
        out.push(Wide {
            core: format!(
                "  store i128 %a, ptr %p, align 16\n  \
                 %u = atomicrmw {op} ptr %p, i128 %b acquire, align 16\n  \
                 %n = load i128, ptr %p, align 16\n  %o = add i128 0, 0\n  %k = add i8 0, 0\n"
            ),
            cases: cases(true),
        });
    }
    for op in &WIDE_OPS[7..] {
        let mut c = Vec::new();
        for &x in e.iter().take(10) {
            for &y in e.iter().take(10) {
                c.push([x, y, 0]);
            }
        }
        out.push(Wide { core: rmw(op, "%b", "acq_rel"), cases: c });
    }
    for (op, k) in [
        ("add", "1"),
        ("sub", "-1"),
        ("xchg", "18446744073709551616"),
        ("umax", "18446744073709551616"),
        ("udec_wrap", "0"),
    ] {
        out.push(Wide { core: rmw(op, k, "seq_cst"), cases: cases(false) });
    }
    for order in ["unordered", "monotonic", "acquire", "seq_cst"] {
        out.push(Wide {
            core: format!(
                "  store i128 %a, ptr %p, align 16\n  \
                 %o = load atomic i128, ptr %p {order}, align 16\n  \
                 %n = load i128, ptr %p, align 16\n  %k = add i8 0, 0\n"
            ),
            cases: cases(false),
        });
    }
    for order in ["unordered", "monotonic", "release", "seq_cst"] {
        out.push(Wide {
            core: format!(
                "  store i128 %a, ptr %p, align 16\n  \
                 store atomic i128 %b, ptr %p {order}, align 16\n  \
                 %n = load i128, ptr %p, align 16\n  %o = add i128 0, 0\n  %k = add i8 0, 0\n"
            ),
            cases: cases(false),
        });
    }
    for weak in ["", "weak "] {
        for success in WIDE_ORDERS {
            for failure in WIDE_FAIL {
                out.push(Wide {
                    core: format!(
                        "  store i128 %a, ptr %p, align 16\n  \
                         %x = cmpxchg {weak}ptr %p, i128 %b, i128 %c {success} {failure}, align 16\n  \
                         %o = extractvalue {{ i128, i1 }} %x, 0\n  \
                         %f = extractvalue {{ i128, i1 }} %x, 1\n  \
                         %k = zext i1 %f to i8\n  %n = load i128, ptr %p, align 16\n"
                    ),
                    cases: cases(true),
                });
            }
        }
    }
    out
}

/// The module: each wrapper `@w<k>(ptr %out, ptr %in)` and, with `twins`,
/// each `@w<k>_v(i128, i128, i128) -> { i128, i128, i8 }`. No function
/// has `+cx16`.
fn wide_module(funcs: &[Wide], twins: bool, keep: impl Fn(&Wide) -> bool) -> String {
    use std::fmt::Write as _;
    let mut text = aggr_gen::header();
    for (k, f) in funcs.iter().enumerate().filter(|(_, f)| keep(f)) {
        write!(
            text,
            "define void @w{k}(ptr %out, ptr %in) {{\nentry:\n  \
             %p = alloca i128, align 16\n  %a = load i128, ptr %in, align 16\n  \
             %ib = getelementptr i8, ptr %in, i64 16\n  %b = load i128, ptr %ib, align 16\n  \
             %ic = getelementptr i8, ptr %in, i64 32\n  %c = load i128, ptr %ic, align 16\n\
             {}  store i128 %o, ptr %out, align 16\n  \
             %out.n = getelementptr i8, ptr %out, i64 16\n  store i128 %n, ptr %out.n, align 16\n  \
             %out.k = getelementptr i8, ptr %out, i64 32\n  store i8 %k, ptr %out.k, align 1\n  \
             ret void\n}}\n\n",
            f.core
        )
        .unwrap();
        if twins {
            write!(
                text,
                "define {{ i128, i128, i8 }} @w{k}_v(i128 %a, i128 %b, i128 %c) {{\nentry:\n  \
                 %p = alloca i128, align 16\n{}  \
                 %r0 = insertvalue {{ i128, i128, i8 }} poison, i128 %o, 0\n  \
                 %r1 = insertvalue {{ i128, i128, i8 }} %r0, i128 %n, 1\n  \
                 %r = insertvalue {{ i128, i128, i8 }} %r1, i8 %k, 2\n  \
                 ret {{ i128, i128, i8 }} %r\n}}\n\n",
                f.core
            )
            .unwrap();
        }
    }
    text.push_str(aggr_gen::PIE_FLAGS);
    text
}

/// The bytes of a result: `old` (16), `new` (16), the flag (1), in hex.
const WIDE_RESULT: usize = 33;

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, b| {
        write!(out, "{b:02x}").unwrap();
        out
    })
}

/// The interpreter's line of every case, in order.
fn wide_expected(funcs: &[Wide]) -> Vec<String> {
    use hod_interp::{Interp, Program, Val};
    let (module, bodies) = aggr_gen::parse(&wide_module(funcs, true, |_| true));
    let program = Program::with_bodies(&module, &bodies).expect("a valid program");
    let mut interp = Interp::new(&program);
    let mut out = Vec::new();
    for (k, f) in funcs.iter().enumerate() {
        let sym = module.find_symbol(format!("w{k}_v").as_bytes()).expect("a twin");
        for case in &f.cases {
            let args: Vec<Val> = case.iter().map(|&v| Val::int(128, v)).collect();
            match interp.run(sym, &args).result {
                Ok(Some(Val::Agg(agg))) => out.push(hex(&agg.bytes()[..WIDE_RESULT])),
                other => panic!("w{k}: {other:?}\n{}", f.core),
            }
        }
    }
    out
}

/// The C driver: each case's operands, its wrapper called, the result
/// printed in hex, one line per case.
fn wide_driver(funcs: &[Wide]) -> String {
    use std::fmt::Write as _;
    let mut c = String::from(
        "#include <stdio.h>\n#include <string.h>\ntypedef void (*fn)(void *, const void *);\n",
    );
    for k in 0..funcs.len() {
        writeln!(c, "void w{k}(void *, const void *);").unwrap();
    }
    c.push_str("static const struct { fn f; unsigned char in[48]; } cases[] = {\n");
    for (k, f) in funcs.iter().enumerate() {
        for case in &f.cases {
            let bytes: Vec<String> = case
                .iter()
                .flat_map(|v| aggr_gen::bytes_of(&[*v as u64, (v >> 64) as u64], 128))
                .map(|b| format!("{b:#04x}"))
                .collect();
            writeln!(c, "  {{ w{k}, {{ {} }} }},", bytes.join(", ")).unwrap();
        }
    }
    write!(
        c,
        "}};\nint main(void) {{\n  \
         for (unsigned i = 0; i < sizeof cases / sizeof cases[0]; i++) {{\n    \
         _Alignas(16) unsigned char in[48], out[48];\n    \
         memcpy(in, cases[i].in, 48);\n    memset(out, 0xa5, sizeof out);\n    \
         cases[i].f(out, in);\n    \
         for (int b = 0; b < {WIDE_RESULT}; b++) printf(\"%02x\", out[b]);\n    printf(\"\\n\");\n  }}\n  \
         return 0;\n}}\n"
    )
    .unwrap();
    c
}

/// Whether the x86-64 cross toolchain has libatomic.
fn has_libatomic() -> bool {
    let target = hod_testutil::target::Target::X86_64;
    let mut command = std::process::Command::new(target.c_driver());
    command.arg("-print-file-name=libatomic.so");
    let ran = hod_testutil::run_with(&mut command, &[], std::time::Duration::from_mins(1));
    ran.success() && ran.stdout_text().trim().starts_with('/')
}

/// §5.6.14 (5): `i128` atomics in functions without `+cx16` call
/// libatomic's `__atomic_*_16`, as `llc` does: every leg (hod plain and
/// under every stress option, `llc` 22 and 19) linked with `-latomic`
/// and run on the baseline CPU model (no `cmpxchg16b`: libatomic takes
/// its lock path), compared byte for byte with `hod_interp`.
#[test]
fn wide_atomics_without_cx16_call_libatomic() {
    use std::path::{Path, PathBuf};
    let funcs = wide_table(&mut aggr_gen::Rng(0x0144_0016));
    let want = wide_expected(&funcs);
    let text = wide_module(&funcs, false, |_| true);
    let (m, bodies) = aggr_gen::parse(&text);
    let scratch = hod_testutil::Scratch::new("x64-leftovers-libatomic");
    let mut legs: Vec<(String, Vec<PathBuf>)> = Vec::new();
    for (mode, stress) in [("plain", hod_cg::Stress::default()), ("stress", hod_cg::Stress::ALL)] {
        let bytes = hod_x64::compile_to_object(&m, &bodies, &aggr_gen::pic(stress))
            .unwrap_or_else(|e| panic!("{mode}: {e}"));
        legs.push((format!("hod-{mode}"), vec![scratch.write(&format!("hod-{mode}.o"), bytes)]));
    }
    let new_ops = |f: &Wide| f.core.contains("usub_");
    let mut ratio = None;
    if let Some(object) = aggr_gen::llc(&scratch, 22, "llc22", &text) {
        if let (Some(h), Some(l)) =
            (aggr_gen::text_bytes(&legs[0].1[0]), aggr_gen::text_bytes(&object))
        {
            ratio = Some(h as f64 / l as f64);
        }
        legs.push(("llc22".into(), vec![object]));
    }
    if let (Some(old), Some(new)) = (
        aggr_gen::llc(&scratch, 19, "llc19", &wide_module(&funcs, false, |f| !new_ops(f))),
        aggr_gen::llc(&scratch, 22, "llc22-new", &wide_module(&funcs, false, new_ops)),
    ) {
        legs.push(("llc19".into(), vec![old, new]));
    }
    if !has_libatomic() {
        eprintln!("note: the x86-64 cross toolchain has no libatomic: not linked or run");
        return;
    }
    let Some(toolchain) = linking::Toolchain::x86_64(&scratch) else { return };
    let driver = toolchain.compile_c(&scratch, "driver", &wide_driver(&funcs), &["-fPIE"]);
    let mut wrong = Vec::new();
    for linker in &toolchain.linkers {
        for (leg, objects) in &legs {
            let program = scratch.join(format!("{leg}-{}", linker.name));
            let mut inputs: Vec<&Path> = vec![driver.as_path()];
            inputs.extend(objects.iter().map(PathBuf::as_path));
            inputs.push(Path::new("-latomic"));
            toolchain.link_ok(linker, &["-pie"], &inputs, &program);
            let ran = toolchain.run(linker, &program);
            assert!(ran.success(), "{leg}: {:?}\n{}", ran.status, ran.stderr_text());
            let out = ran.stdout_text();
            let lines: Vec<&str> = out.lines().collect();
            assert_eq!(lines.len(), want.len(), "{leg}: one line per case");
            let mut i = 0;
            for (k, f) in funcs.iter().enumerate() {
                for case in &f.cases {
                    if lines[i] != want[i] {
                        wrong.push(format!(
                            "{leg}/{}: w{k}({case:x?}): expected {}, got {}\n{}",
                            linker.name, want[i], lines[i], f.core
                        ));
                    }
                    i += 1;
                }
            }
        }
    }
    eprintln!(
        "x64 atomic 16 bytes without cx16: {} functions, {} cases, legs {:?}, text hod/llc 22 {:.2?}",
        funcs.len(),
        want.len(),
        legs.iter().map(|(l, _)| l.as_str()).collect::<Vec<_>>(),
        ratio
    );
    assert!(
        wrong.is_empty(),
        "{} disagreements; the first:\n{}",
        wrong.len(),
        wrong[..wrong.len().min(6)].join("\n")
    );
}

// ---- refusals ----

/// What compiling `.ll` functions (the header added) reports.
fn compile_ll(functions: &str) -> Result<(), hod_cg::CgError> {
    let (module, bodies) = float_gen::parse(&format!("{}{functions}", float_gen::ll_header()));
    match hod_x64::compile_to_object(&module, &bodies, &float_gen::pic(hod_cg::Stress::default())) {
        Ok(_) => Ok(()),
        Err(hod_x64::ModuleError::Function { error, .. }) => Err(error),
        Err(other) => panic!("{other}"),
    }
}

/// Asserts that each `.ll` function of `cases` is refused with `reason`.
fn assert_refused(cases: &[(&str, &'static str)]) {
    for (functions, reason) in cases {
        assert_eq!(compile_ll(functions), Err(hod_cg::CgError::unsupported(reason)), "{functions}");
    }
}

/// §5.6.14 (2): what stays refused, by AArch64's rule: an integer wider
/// than 128 bits, and `bfloat` with an integer wider than 64 bits.
#[test]
fn wide_conversions_refused_by_name() {
    use hod_x64::aggr::WIDE_FP_CAST;
    assert_refused(&[
        (
            "define double @f(i129 %a) { %r = uitofp i129 %a to double\n ret double %r }",
            WIDE_FP_CAST,
        ),
        (
            "define void @f(ptr %p, float %a) { %r = fptosi float %a to i200\n store i200 %r, ptr %p\n ret void }",
            WIDE_FP_CAST,
        ),
        ("define bfloat @f(i96 %a) { %r = sitofp i96 %a to bfloat\n ret bfloat %r }", WIDE_FP_CAST),
        (
            "define void @f(ptr %p, bfloat %a) { %r = fptoui bfloat %a to i65\n store i65 %r, ptr %p\n ret void }",
            WIDE_FP_CAST,
        ),
    ]);
}
