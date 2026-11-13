//! The x86-64 leftovers between the instruction groups (WP-144,
//! `spec/codegen.md` §5.6.14): floating-point members of aggregates,
//! conversions between floating point and integers of 65 to 128 bits that
//! are not `i128`, the floating-point `atomicrmw` operations, and the
//! other small forms the groups left. Each runs under the emulator against
//! `hod_interp` (where it has the type) and `llc -O0 -fast-isel=0` of
//! LLVM 22 and 19 (`common/float_gen.rs`).

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

#[path = "common/float_gen.rs"]
mod float_gen;

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
                let mut add = |suite: &mut Suite, x: f64| {
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
