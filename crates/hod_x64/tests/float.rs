//! The x86-64 scalar floating-point group (WP-127, `spec/codegen.md`
//! §5.6.7): `fadd` .. `frem`, `fneg`, `fcmp` (as a value and fused into
//! `select` and `br`), the conversions, `select`, `phi`, `freeze` and the
//! constants of `half`, `bfloat`, `float`, `double` and `fp128`, checked
//! against `hod_interp` and `llc -O0 -fast-isel=0` (LLVM 22 and 19) under
//! the emulator (`common/float_gen.rs`); the flags rule, the fusions and
//! the absence of any instruction above SSE2 on the disassembly; what is
//! refused, by name.

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

#[path = "common/float_gen.rs"]
mod float_gen;

use float_gen::{
    Check, Rng, Suite, T, above_sse2, cases, check, disassembly, edges, ll_header, mnemonic, parse,
    pic,
};
use hod_cg::{CgError, Stress};

const MAJORS: [u32; 2] = [22, 19];

/// The types of the group, with who checks them.
const TYPES: [(T, Check); 5] = [
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

/// The 16 predicates.
const PREDICATES: [&str; 16] = [
    "false", "oeq", "ogt", "oge", "olt", "ole", "one", "ord", "ueq", "ugt", "uge", "ult", "ule",
    "une", "uno", "true",
];

/// Every binary operation and `fneg` of every type, every pair of edge
/// values (signed zeros, subnormals, infinities, NaNs with payloads) and
/// random ones.
#[test]
fn every_binary_operation_agrees() {
    let mut rng = Rng(127);
    let mut suite = Suite::default();
    for (t, check) in TYPES {
        let ty = t.ll();
        for op in ["fadd", "fsub", "fmul", "fdiv", "frem"] {
            let f = suite.add(
                &format!("{op}_{}", tag(t)),
                &[t, t],
                t,
                &format!("  %r = {op} {ty} %a0, %a1\n"),
                check,
            );
            cases(&mut suite, &mut rng, f, &[t, t], 1200, 200);
        }
        let f = suite.add(
            &format!("fneg_{}", tag(t)),
            &[t],
            t,
            &format!("  %r = fneg {ty} %a0\n"),
            check,
        );
        cases(&mut suite, &mut rng, f, &[t], 100, 30);
        // The same operand twice.
        let f = suite.add(
            &format!("square_{}", tag(t)),
            &[t],
            t,
            &format!("  %m = fmul {ty} %a0, %a0\n  %r = fsub {ty} %m, %a0\n"),
            check,
        );
        cases(&mut suite, &mut rng, f, &[t], 100, 30);
    }
    check("binary", &suite, &MAJORS);
}

/// Every predicate of every type: as a value, fused into `select`, fused
/// into `br` (to a `phi` of the operands), and used twice (not fused).
#[test]
fn every_compare_agrees() {
    let mut rng = Rng(128);
    let suite = compare_suite(&mut rng);
    check("compare", &suite, &MAJORS);
}

fn compare_suite(rng: &mut Rng) -> Suite {
    let mut suite = Suite::default();
    for (t, check) in TYPES {
        let ty = t.ll();
        for p in PREDICATES {
            let n = tag(t);
            let value = suite.add(
                &format!("fcmp_{p}_{n}"),
                &[t, t],
                T::I(1),
                &format!("  %r = fcmp {p} {ty} %a0, %a1\n"),
                check,
            );
            let select = suite.add(
                &format!("select_{p}_{n}"),
                &[t, t],
                t,
                &format!(
                    "  %c = fcmp {p} {ty} %a0, %a1\n  %r = select i1 %c, {ty} %a0, {ty} %a1\n"
                ),
                check,
            );
            let br = suite.add(
                &format!("br_{p}_{n}"),
                &[t, t],
                t,
                &format!(
                    "  %c = fcmp {p} {ty} %a0, %a1\n  br i1 %c, label %yes, label %no\n\
                     yes:\n  br label %join\nno:\n  br label %join\n\
                     join:\n  %r = phi {ty} [ %a1, %yes ], [ %a0, %no ]\n"
                ),
                check,
            );
            let twice = suite.add(
                &format!("twice_{p}_{n}"),
                &[t, t],
                T::I(32),
                &format!(
                    "  %c = fcmp {p} {ty} %a0, %a1\n  %z = zext i1 %c to i32\n  \
                     %s = select i1 %c, i32 7, i32 3\n  %r = add i32 %z, %s\n"
                ),
                check,
            );
            // `llc` 22 quiets a signalling `bfloat` NaN that a `phi`
            // passes (the value is moved through `float`); hod keeps its
            // bits, as `phi` must.
            if t == T::B {
                suite.funcs[br].llc = false;
            }
            for f in [value, select, br, twice] {
                cases(&mut suite, rng, f, &[t, t], 150, 20);
            }
        }
    }
    suite
}

/// `fpext` and `fptrunc` between every pair of types (`bfloat` and
/// `fp128` apart: refused), every edge value.
#[test]
fn every_floating_conversion_agrees() {
    let mut rng = Rng(129);
    let mut suite = Suite::default();
    let order = [T::H, T::B, T::F, T::D, T::Q];
    for (i, &from) in order.iter().enumerate() {
        for (j, &to) in order.iter().enumerate() {
            if i == j
                || from.bits() == to.bits()
                || matches!((from, to), (T::B, T::Q) | (T::Q, T::B))
            {
                continue;
            }
            let op = if from.bits() < to.bits() { "fpext" } else { "fptrunc" };
            let check = if matches!((from, to), (T::F, T::D) | (T::D, T::F)) {
                Check::Interp
            } else {
                Check::Llc
            };
            let f = suite.add(
                &format!("{op}_{}_{}", tag(from), tag(to)),
                &[from],
                to,
                &format!("  %r = {op} {} %a0 to {}\n", from.ll(), to.ll()),
                check,
            );
            cases(&mut suite, &mut rng, f, &[from], 200, 300);
        }
    }
    check("fpconv", &suite, &MAJORS);
}

/// The value of `bits` of `t`, approximately for `fp128` (its upper word
/// only), `None` for an infinity or a NaN.
fn value_of(t: T, bits: &[u64]) -> Option<f64> {
    let value = match t {
        T::H => {
            let half = bits[0] & 0xffff;
            let exponent = ((half >> 10) & 0x1f) as i32;
            let fraction = f64::from((half & 0x3ff) as u32);
            let magnitude = if exponent == 0 {
                fraction * 2f64.powi(-24)
            } else {
                (1024.0 + fraction) * 2f64.powi(exponent - 25)
            };
            if exponent == 0x1f {
                f64::NAN
            } else if half & 0x8000 != 0 {
                -magnitude
            } else {
                magnitude
            }
        }
        T::B => f64::from(f32::from_bits(((bits[0] & 0xffff) as u32) << 16)),
        T::F => f64::from(f32::from_bits(bits[0] as u32)),
        T::D => f64::from_bits(bits[0]),
        T::Q => {
            let high = bits[1];
            let exponent = ((high >> 48) & 0x7fff) as i32;
            // The top 32 of the 48 fraction bits of the high word.
            let fraction = f64::from(((high >> 16) & 0xffff_ffff) as u32);
            let magnitude = if exponent == 0x7fff {
                f64::NAN
            } else if exponent == 0 {
                0.0
            } else {
                (4_294_967_296.0 + fraction) * 2f64.powi(exponent - 16383 - 32)
            };
            if high >> 63 == 1 { -magnitude } else { magnitude }
        }
        T::I(_) => return None,
    };
    value.is_finite().then_some(value)
}

/// Whether `fptosi`/`fptoui` (`signed`) of `x` to `bits` bits is defined
/// with a margin (the boundaries themselves are the interpreter's cases).
fn in_range(x: f64, bits: u32, signed: bool) -> bool {
    let top = 2f64.powi(bits as i32 - i32::from(signed));
    let x = x.trunc();
    if signed { x > -top && x < top } else { x >= 0.0 && x < top }
}

/// The `bfloat` of the magnitude `m` with sign `negative`, rounded once
/// to nearest even.
fn bfloat_of(m: u64, negative: bool) -> u64 {
    let sign = u64::from(negative) << 15;
    if m == 0 {
        return sign;
    }
    let p = 63 - u64::from(m.leading_zeros());
    let (mut frac, mut exp) = if p <= 7 {
        (m << (7 - p), p)
    } else {
        let shift = p - 7;
        let kept = m >> shift;
        let rest = m & ((1 << shift) - 1);
        let half = 1 << (shift - 1);
        let up = rest > half || (rest == half && kept & 1 == 1);
        (kept + u64::from(up), p)
    };
    if frac == 0x100 {
        frac = 0x80;
        exp += 1;
    }
    sign | (exp + 127) << 7 | (frac & 0x7f)
}

/// The model of `uitofp`/`sitofp` of `BITS` bits to `bfloat`.
fn bfloat_model<const BITS: u32, const SIGNED: bool>(args: &[u64]) -> u64 {
    let x = args[0] & float_gen::mask(BITS);
    if SIGNED && x >> (BITS - 1) == 1 {
        let m = (x | !float_gen::mask(BITS)).wrapping_neg();
        bfloat_of(m, true)
    } else {
        bfloat_of(x, false)
    }
}

macro_rules! models {
    ($signed:literal: $($bits:literal)*) => {
        [$(bfloat_model::<$bits, $signed> as fn(&[u64]) -> u64),*]
    };
}

/// The models of the widths 25 to 64.
const BITS_FROM_25: usize = 40;
static BFLOAT_SIGNED: [fn(&[u64]) -> u64; BITS_FROM_25] = models!(true: 25 26 27 28 29 30 31 32 33 34 35 36 37 38 39 40 41 42 43 44 45 46 47 48 49 50 51 52 53 54 55 56 57 58 59 60 61 62 63 64);
static BFLOAT_UNSIGNED: [fn(&[u64]) -> u64; BITS_FROM_25] = models!(false: 25 26 27 28 29 30 31 32 33 34 35 36 37 38 39 40 41 42 43 44 45 46 47 48 49 50 51 52 53 54 55 56 57 58 59 60 61 62 63 64);

/// `fptosi`/`fptoui` of every type to the widths around 32 and 64 (the
/// conversion boundaries `±2^31`, `2^32`, `±2^63`, `2^64` and their
/// neighbours: out of range is poison, not run), `sitofp`/`uitofp` of
/// the widths' edge values.
#[test]
fn every_integer_conversion_agrees() {
    let mut rng = Rng(130);
    let mut suite = Suite::default();
    let widths = [1, 8, 16, 17, 31, 32, 33, 53, 54, 63, 64];
    for (t, check) in TYPES {
        for bits in widths {
            for signed in [true, false] {
                let op = if signed { "fptosi" } else { "fptoui" };
                let f = suite.add(
                    &format!("{op}_{}_{bits}", tag(t)),
                    &[t],
                    T::I(bits),
                    &format!("  %r = {op} {} %a0 to i{bits}\n", t.ll()),
                    check,
                );
                if check == Check::Interp {
                    cases(&mut suite, &mut rng, f, &[t], 200, 100);
                } else {
                    let mut values: Vec<Vec<u64>> = float_gen::operands(t);
                    for _ in 0..400 {
                        values.push(float_gen::random(&mut rng, t));
                    }
                    for v in values {
                        if value_of(t, &v).is_some_and(|x| in_range(x, bits, signed)) {
                            suite.case(f, &v);
                        }
                    }
                }
                let op = if signed { "sitofp" } else { "uitofp" };
                let f = suite.add(
                    &format!("{op}_{bits}_{}", tag(t)),
                    &[T::I(bits)],
                    t,
                    &format!("  %r = {op} i{bits} %a0 to {}\n", t.ll()),
                    check,
                );
                // `llc` rounds an integer of more than 24 bits to `float`
                // first, then to `bfloat`: twice (`0x1feffffff` gives
                // `0x5000`, not `0x4fff`). The model rounds once.
                if t == T::B && bits > 24 {
                    suite.funcs[f].llc = false;
                    suite.funcs[f].model = Some(if signed {
                        BFLOAT_SIGNED[(bits - 25) as usize]
                    } else {
                        BFLOAT_UNSIGNED[(bits - 25) as usize]
                    });
                }
                cases(&mut suite, &mut rng, f, &[T::I(bits)], 200, 100);
            }
        }
    }
    check("intconv", &suite, &MAJORS);
}

/// `select` on an `i1` argument, `freeze` (of a value and of `poison`),
/// a `phi` across a loop, and constants of every type (zeros, NaNs,
/// values that need a data island).
#[test]
fn selects_phis_freezes_and_constants_agree() {
    let mut rng = Rng(131);
    let mut suite = Suite::default();
    for (t, check) in TYPES {
        let ty = t.ll();
        let n = tag(t);
        let f = suite.add(
            &format!("sel_{n}"),
            &[T::I(1), t, t],
            t,
            &format!("  %r = select i1 %a0, {ty} %a1, {ty} %a2\n"),
            check,
        );
        cases(&mut suite, &mut rng, f, &[T::I(1), t, t], 60, 20);
        let f = suite.add(
            &format!("freeze_{n}"),
            &[t],
            t,
            &format!("  %f = freeze {ty} %a0\n  %r = fadd {ty} %f, %f\n"),
            check,
        );
        cases(&mut suite, &mut rng, f, &[t], 60, 10);
        let f = suite.add(
            &format!("freeze_poison_{n}"),
            &[t],
            t,
            &format!("  %f = freeze {ty} poison\n  %r = fsub {ty} %a0, %f\n"),
            check,
        );
        // LLVM leaves the value of a frozen `poison` open.
        suite.funcs[f].llc = false;
        if check == Check::Llc {
            suite.funcs.pop();
            suite.text.truncate(suite.text.rfind("define").unwrap_or(0));
        } else {
            cases(&mut suite, &mut rng, f, &[t], 30, 0);
        }
        // A loop: x = x * a1 + a0, three times.
        let f = suite.add(
            &format!("loop_{n}"),
            &[t, t],
            t,
            &format!(
                "  br label %head\nhead:\n  %i = phi i32 [ 0, %0 ], [ %j, %head ]\n  \
                 %x = phi {ty} [ %a0, %0 ], [ %y, %head ]\n  %m = fmul {ty} %x, %a1\n  \
                 %y = fadd {ty} %m, %a0\n  %j = add i32 %i, 1\n  %c = icmp ult i32 %j, 3\n  \
                 br i1 %c, label %head, label %exit\nexit:\n  %r = fadd {ty} %y, %x\n"
            ),
            check,
        );
        // `llc` computes a `bfloat` `fmul` in `float` with the operands
        // commuted: of two NaNs it returns the other one (either is
        // allowed).
        if t == T::B {
            suite.funcs[f].llc = false;
        }
        cases(&mut suite, &mut rng, f, &[t, t], 80, 20);
        let consts: &[&str] = match t {
            T::H => &["0xH0000", "0xH8000", "0xH3C00", "0xH7E01", "0xH7BFF", "0xH0001"],
            T::B => &["0xR0000", "0xR8000", "0xR3F80", "0xR7FC1", "0xR7F7F", "0xR0001"],
            T::F => &[
                "0.0",
                "-0.0",
                "1.5",
                "0x7FF8000000000000",
                "0x36A0000000000000",
                "0x47EFFFFFE0000000",
            ],
            T::D => &["0.0", "-0.0", "1.5", "0x7FF8000012345678", "1.0e-310", "1.0e300"],
            _ => &[
                "0xL00000000000000000000000000000000",
                "0xL00000000000000008000000000000000",
                "0xL00000000000000003FFF800000000000",
                "0xL00000000000000017FFF800000000000",
                "0xL123456789ABCDEF03FFF123456789ABC",
                "0xL0000000000000001C001000000000000",
            ],
        };
        for (k, c) in consts.iter().enumerate() {
            let f = suite.add(
                &format!("const{k}_{n}"),
                &[t],
                t,
                &format!("  %r = fadd {ty} %a0, {c}\n"),
                check,
            );
            // `llc` folds `x + -0.0` to `x`, a signalling NaN unquieted
            // (allowed); hod adds.
            if k == 1 {
                suite.funcs[f].llc = false;
            }
            cases(&mut suite, &mut rng, f, &[t], 30, 5);
            let f = suite.add(
                &format!("ret{k}_{n}"),
                &[t],
                t,
                &format!("  %r = fadd {ty} {c}, {c}\n"),
                check,
            );
            suite.case(f, &[0, 0]);
        }
    }
    check("select", &suite, &MAJORS);
}

// ---- the code itself ----

/// Whether `m` reads the flags: `setcc`, `cmovcc`, `jcc`.
fn reads_flags(m: &str) -> bool {
    m.starts_with("set") || m.starts_with("cmov") || (m.starts_with('j') && m != "jmp")
}

/// What may run between a flag setter and its reader (§5.6 "Flags").
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
            | "movd"
            | "movq"
            | "movaps"
            | "movapd"
            | "movss"
            | "movsd"
            | "movups"
    ) || reads_flags(m)
}

/// §5.6 "Flags": in every compare function, plain and under every stress
/// option, each flag reader is reached from a `ucomiss`/`ucomisd`, a
/// `cmp` or a `test` through flag-keeping glue only (another reader of
/// the same flags included: `jp` then `jne`).
#[test]
fn every_compare_keeps_the_flags() {
    let suite = compare_suite(&mut Rng(132));
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
                matches!(
                    setter,
                    "cmp" | "test" | "ucomiss" | "ucomisd" | "or" | "and" | "bt" | "sar"
                ),
                "{line} is reached from {setter}:\n{}",
                lines[at.saturating_sub(12)..=at].join("\n")
            );
        }
        assert!(readers > 400, "{readers} readers");
    }
}

/// No instruction above SSE2: no `roundss` (SSE4.1), no blend, no AVX
/// (`v...`), no x87, in any function of the group.
#[test]
fn nothing_above_sse2_is_emitted() {
    let mut rng = Rng(133);
    let mut text = compare_suite(&mut rng).text;
    for (t, check) in TYPES {
        for op in ["fadd", "fdiv", "frem"] {
            let mut s = Suite::default();
            s.add("f", &[t, t], t, &format!("  %r = {op} {} %a0, %a1\n", t.ll()), check);
            text.push_str(&s.text.replace("@f(", &format!("@{op}_{}(", tag(t))));
        }
    }
    let Some(lines) = disassembly(&text, Stress::default()) else { return };
    for line in &lines {
        assert!(!above_sse2(mnemonic(line)), "{line}");
    }
}

/// §4.6: an `fcmp` whose only use is the next `select` or `br` is fused:
/// `ucomiss` then the branches, no `setcc`; `oeq` is two conditions
/// (`jne` and `jp` to the false side).
#[test]
fn compares_fuse_into_select_and_br() {
    let Some(select) = disassembly(
        "define float @f(float %a, float %b) {\n  %c = fcmp olt float %a, %b\n  \
         %r = select i1 %c, float %a, float %b\n  ret float %r\n}\n",
        Stress::default(),
    ) else {
        return;
    };
    let has = |lines: &[String], m: &str| lines.iter().any(|l| mnemonic(l) == m);
    let starts = |lines: &[String], m: &str| lines.iter().any(|l| mnemonic(l).starts_with(m));
    assert!(has(&select, "ucomiss") && !starts(&select, "set"), "{select:#?}");
    let br = disassembly(
        "define i32 @f(double %a, double %b) {\n  %c = fcmp oeq double %a, %b\n  \
         br i1 %c, label %t, label %e\nt:\n  ret i32 1\ne:\n  ret i32 2\n}\n",
        Stress::default(),
    )
    .unwrap();
    assert!(has(&br, "ucomisd") && !starts(&br, "set"), "{br:#?}");
    assert!(has(&br, "jp") && has(&br, "jne"), "{br:#?}");
    let apart = disassembly(
        "define i32 @f(double %a, double %b) {\n  %c = fcmp une double %a, %b\n  \
         %z = zext i1 %c to i32\n  ret i32 %z\n}\n",
        Stress::default(),
    )
    .unwrap();
    assert!(has(&apart, "setne") && has(&apart, "setp") && has(&apart, "or"), "{apart:#?}");
}

/// What the group does not lower, by name, one construct per program:
/// a conversion between `bfloat` and `fp128` (vectors are WP-132's,
/// `tests/vector.rs`).
#[test]
fn what_is_not_lowered_is_refused_by_name() {
    for (text, name) in [
        (
            "define bfloat @f(fp128 %a) {\n  %r = fptrunc fp128 %a to bfloat\n  ret bfloat %r\n}\n",
            hod_x64::float::BFLOAT_FP128,
        ),
        (
            "define fp128 @f(bfloat %a) {\n  %r = fpext bfloat %a to fp128\n  ret fp128 %r\n}\n",
            hod_x64::float::BFLOAT_FP128,
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

/// The edge lists hold what the module documentation promises.
#[test]
fn the_edges_hold_the_boundaries() {
    let f = edges(T::F);
    for bits in [
        0x4f00_0000u64,
        0x4f80_0000,
        0x5f00_0000,
        0x5f80_0000,
        0xdf00_0000,
        0x7fa0_0000,
        0x8000_0000,
    ] {
        assert!(f.contains(&bits), "{bits:#x}");
    }
    let d = edges(T::D);
    for bits in [0x41e0_0000_0000_0000u64, 0x43e0_0000_0000_0000, 0x43f0_0000_0000_0000, 1] {
        assert!(d.contains(&bits), "{bits:#x}");
    }
    assert_eq!(float_gen::quad_of(1.0), (0, 0x3fff_0000_0000_0000));
    assert_eq!(float_gen::quad_of(5e-324), (0, 0x3bcd_0000_0000_0000));
}
