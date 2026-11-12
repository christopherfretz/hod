//! The short tail of first refusals (WP-084, `spec/codegen.md` §5.5.1
//! "WP-084"): `fp128` and `half` fields of aggregates, conversions between
//! floating point and integers of 65 to 128 bits, `{s,u}mul.with.overflow`
//! wider than 64 bits, `switch` on wide integers, `readcyclecounter`,
//! `{s,u}shl.sat`, `aarch64.crc32*`, `is.constant` and an `invoke` whose
//! result has odd integer leaves.
//!
//! Each table of functions is compiled by hod (plain, every stress
//! option, far mode) and by `llc -O0 -global-isel=0 -fast-isel=0`, linked
//! with a C driver under every linker present and run (`common::native`).
//! The reference is `hod_interp` where it models the types (integers up
//! to 128 bits, `float`, `double`), a model in this file for the
//! saturating shifts, else the `llc` build.

mod common;
#[path = "common/native.rs"]
mod native;

use std::cmp::Ordering;
use std::fmt::Write as _;

use common::{assert_refused, compile_ll, ll_header, parse};
use hod_a64::aggr;
use hod_cg::{Options, RelocModel, Stress};
use hod_interp::{Interp, Program, Val};

const DEFAULT: Options = Options {
    reloc_model: RelocModel::Static,
    code_model: None,
    function_sections: false,
    data_sections: false,
    stress: Stress { min_pool: false, far: false, displace: false },
    debug_compression: hod_obj::Compression::None,
    split_dwarf: hod_dwarf::SplitDwarf::Off,
};

const MODES: [(&str, Options); 3] = [
    ("plain", DEFAULT),
    ("stress", Options { stress: Stress::ALL, ..DEFAULT }),
    ("far", Options { stress: Stress { min_pool: false, far: true, displace: false }, ..DEFAULT }),
];

/// xorshift64*: a fixed sequence, so that every run checks the same calls.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// An edge value or a random word.
    fn word(&mut self) -> u64 {
        const EDGES: [u64; 10] = [
            0,
            1,
            2,
            3,
            u64::MAX,
            u64::MAX - 1,
            1 << 63,
            (1 << 63) - 1,
            0xffff_ffff,
            0x1_0000_0000,
        ];
        let r = self.next();
        if r.is_multiple_of(3) {
            EDGES[(r >> 8) as usize % EDGES.len()]
        } else {
            self.next() >> (r % 64)
        }
    }
}

/// IR that makes `%out` of type `iN` from the `i64` parameters `p`
/// (two for `N <= 128`, built in `i128` so that the interpreter models
/// it; three otherwise, built in `i192` and sign-extended above it).
fn build(n: u32, p: &[&str], out: &str) -> String {
    let w = if n <= 128 { 128 } else { 192 };
    let mut s = String::new();
    let mut acc = String::new();
    for (k, name) in p.iter().take((w / 64) as usize).enumerate() {
        writeln!(s, "  %{out}.z{k} = zext i64 %{name} to i{w}").unwrap();
        writeln!(s, "  %{out}.s{k} = shl i{w} %{out}.z{k}, {}", 64 * k).unwrap();
        if k == 0 {
            acc = format!("%{out}.s0");
        } else {
            writeln!(s, "  %{out}.o{k} = or i{w} {acc}, %{out}.s{k}").unwrap();
            acc = format!("%{out}.o{k}");
        }
    }
    let conv = match n.cmp(&w) {
        Ordering::Less => "trunc",
        Ordering::Equal => "",
        Ordering::Greater => "sext",
    };
    if conv.is_empty() {
        writeln!(s, "  %{out} = or i{w} {acc}, 0").unwrap();
    } else {
        writeln!(s, "  %{out} = {conv} i{w} {acc} to i{n}").unwrap();
    }
    s
}

/// IR that folds `%v` of type `iN` into the `i64` `%out`: every limb
/// (zero-extended from the width) mixed in.
fn fold(n: u32, v: &str, out: &str) -> String {
    let mut s = String::new();
    let mut acc = String::from("0");
    for k in 0..n.div_ceil(64) {
        match n.cmp(&64) {
            Ordering::Greater => {
                writeln!(s, "  %{out}.r{k} = lshr i{n} %{v}, {}", 64 * k).unwrap();
                writeln!(s, "  %{out}.t{k} = trunc i{n} %{out}.r{k} to i64").unwrap();
            }
            Ordering::Less => writeln!(s, "  %{out}.t{k} = zext i{n} %{v} to i64").unwrap(),
            Ordering::Equal => writeln!(s, "  %{out}.t{k} = or i64 %{v}, 0").unwrap(),
        }
        writeln!(s, "  %{out}.m{k} = mul i64 {acc}, -7046029254386353131").unwrap();
        writeln!(s, "  %{out}.a{k} = add i64 %{out}.m{k}, %{out}.t{k}").unwrap();
        acc = format!("%{out}.a{k}");
    }
    writeln!(s, "  %{out} = or i64 {acc}, 0").unwrap();
    s
}

/// What `hod_interp` prints for each call of `body`'s functions: the
/// reference of a table whose types it models.
fn interp_lines(body: &str, calls: &[native::Call]) -> String {
    let text = format!("{}{body}", ll_header(22));
    let (module, bodies) = parse(&text);
    let program = Program::with_bodies(&module, &bodies).expect("a program");
    let mut lines = String::new();
    for (name, args) in calls {
        let sym = module.find_symbol(name.as_bytes()).expect("a function");
        let fb = bodies.iter().find(|b| b.symbol() == sym).expect("a body");
        let vals: Vec<Val> =
            args[..fb.arg_count() as usize].iter().map(|&a| Val::int(64, u128::from(a))).collect();
        let outcome = Interp::new(&program).run(sym, &vals);
        let Ok(Some(Val::Int { value, .. })) = outcome.result else {
            panic!("the interpreter on @{name}{args:?}: {:?}", outcome.result);
        };
        writeln!(lines, "{}", value as u64).unwrap();
    }
    lines
}

/// Runs `calls` of `body` natively against `want` (or `llc`); returns
/// the number of calls checked.
fn run(label: &str, body: &str, calls: &[native::Call], want: Option<&str>) -> usize {
    let count = native::run_table(label, body, calls, &MODES, want);
    eprintln!("{label}: {count} calls agree");
    count
}

/// `count` calls of `name` with words from `rng`.
fn calls_of(name: &str, rng: &mut Rng, count: usize) -> Vec<native::Call> {
    (0..count).map(|_| (name.to_owned(), [(); 6].map(|()| rng.word()))).collect()
}

// ---- fp128 and half fields of aggregates ----

const AGGREGATES: &str = "
define { fp128, fp128 } @mk(fp128 %x, fp128 %y) {
  %s0 = insertvalue { fp128, fp128 } undef, fp128 %x, 0
  %s = insertvalue { fp128, fp128 } %s0, fp128 %y, 1
  ret { fp128, fp128 } %s
}

define i64 @complex_fp128(i64 %a, i64 %b) {
  %x = sitofp i64 %a to fp128
  %y = sitofp i64 %b to fp128
  %s = call { fp128, fp128 } @mk(fp128 %x, fp128 %y)
  %re = extractvalue { fp128, fp128 } %s, 0
  %im = extractvalue { fp128, fp128 } %s, 1
  %t = insertvalue [2 x fp128] zeroinitializer, fp128 %im, 1
  %u = insertvalue [2 x fp128] %t, fp128 %re, 0
  %p = alloca [2 x fp128]
  store [2 x fp128] %u, ptr %p
  %v = load [2 x fp128], ptr %p
  %w0 = extractvalue [2 x fp128] %v, 0
  %w1 = extractvalue [2 x fp128] %v, 1
  %d = fsub fp128 %w1, %w0
  %h = fmul fp128 %d, 0xL00000000000000004000000000000000
  %r = fptosi fp128 %h to i64
  ret i64 %r
}

define i64 @half_fields(i64 %a, i64 %b) {
  %x = sitofp i64 %a to half
  %y = sitofp i64 %b to half
  %s0 = insertvalue { half, i8, half, fp128 } undef, half %x, 0
  %s1 = insertvalue { half, i8, half, fp128 } %s0, i8 7, 1
  %s2 = insertvalue { half, i8, half, fp128 } %s1, half %y, 2
  %q = fpext half %y to fp128
  %s = insertvalue { half, i8, half, fp128 } %s2, fp128 %q, 3
  %e0 = extractvalue { half, i8, half, fp128 } %s, 0
  %e2 = extractvalue { half, i8, half, fp128 } %s, 2
  %e3 = extractvalue { half, i8, half, fp128 } %s, 3
  %b0 = bitcast half %e0 to i16
  %b2 = bitcast half %e2 to i16
  %z0 = zext i16 %b0 to i64
  %z2 = zext i16 %b2 to i64
  %sh = shl i64 %z2, 16
  %o = or i64 %z0, %sh
  %t = fptosi fp128 %e3 to i64
  %t2 = shl i64 %t, 32
  %r = xor i64 %o, %t2
  ret i64 %r
}
";

#[test]
fn fp128_and_half_fields_of_aggregates() {
    let mut calls = Vec::new();
    for (a, b) in [(0, 0), (1, 2), (-5i64 as u64, 9), (1 << 40, (-3i64 << 50) as u64), (77, 77)] {
        calls.push(("complex_fp128".to_owned(), [a, b, 0, 0, 0, 0]));
    }
    for (a, b) in [(0, 0), (1, 2), (-5i64 as u64, 9), (2048, (-60000i64) as u64), (3, 65504)] {
        calls.push(("half_fields".to_owned(), [a, b, 0, 0, 0, 0]));
    }
    run("a64-tail-aggregates", AGGREGATES, &calls, None);
}

// ---- multiplication with overflow, wider than 64 bits ----

fn mul_overflow_body(widths: &[u32]) -> String {
    let mut body = String::new();
    for &n in widths {
        for op in ["smul", "umul"] {
            let mut f =
                format!("define i64 @{op}{n}(i64 %a, i64 %b, i64 %c, i64 %d, i64 %e, i64 %f) {{\n");
            f += &build(n, &["a", "b", "c"], "x");
            f += &build(n, &["d", "e", "f"], "y");
            writeln!(
                f,
                "  %r = call {{ i{n}, i1 }} @llvm.{op}.with.overflow.i{n}(i{n} %x, i{n} %y)\n  \
                 %v = extractvalue {{ i{n}, i1 }} %r, 0\n  \
                 %o = extractvalue {{ i{n}, i1 }} %r, 1\n  \
                 %oz = zext i1 %o to i64"
            )
            .unwrap();
            f += &fold(n, "v", "h");
            f += "  %h2 = shl i64 %h, 1\n  %res = or i64 %h2, %oz\n  ret i64 %res\n}\n";
            writeln!(body, "declare {{ i{n}, i1 }} @llvm.{op}.with.overflow.i{n}(i{n}, i{n})\n{f}")
                .unwrap();
        }
    }
    body
}

fn mul_overflow_calls(widths: &[u32], rng: &mut Rng, per: usize) -> Vec<native::Call> {
    let mut calls = Vec::new();
    for &n in widths {
        for op in ["smul", "umul"] {
            let name = format!("{op}{n}");
            calls.extend(calls_of(&name, rng, per));
            // Small operands (no overflow), and the signed edge MIN * -1.
            for (x, y) in [(3, 5), (u64::MAX, u64::MAX), (u64::MAX, 2), (0, u64::MAX)] {
                calls.push((name.clone(), [x, 0, 0, y, 0, 0]));
                calls.push((name.clone(), [x, x, x, y, y, y]));
            }
            let min_top = if n <= 128 { 1u64 << ((n - 1) % 64) } else { 0 };
            let min = if n <= 64 {
                [min_top, 0, 0]
            } else if n <= 128 {
                [0, min_top, 0]
            } else {
                [0, 0, 1 << 63]
            };
            calls.push((name.clone(), [min[0], min[1], min[2], u64::MAX, u64::MAX, u64::MAX]));
            calls.push((name.clone(), [min[0], min[1], min[2], 1, 0, 0]));
        }
    }
    calls
}

/// Against the interpreter (and `llc`): the widths it models.
#[test]
fn wide_mul_overflow_agrees_with_the_interpreter() {
    let widths = [65, 100, 127, 128];
    let body = mul_overflow_body(&widths);
    let calls = mul_overflow_calls(&widths, &mut Rng(0x0841_0084), 40);
    let want = interp_lines(&body, &calls);
    run("a64-tail-mulo", &body, &calls, Some(&want));
}

/// Wider than the interpreter models: against `llc`.
#[test]
fn wide_mul_overflow_agrees_with_llc() {
    let widths = [129, 192, 200, 256];
    let body = mul_overflow_body(&widths);
    let calls = mul_overflow_calls(&widths, &mut Rng(0x0842_0084), 25);
    run("a64-tail-mulo-wide", &body, &calls, None);
}

// ---- conversions between floating point and 65 to 128 bits ----

/// The doubles converted to integers: in range for every width tested
/// (an out-of-range conversion is poison).
const DOUBLES: [f64; 12] =
    [0.0, -0.0, 0.5, 1.0, -1.0, 123.75, -98765.4321, 1e18, -1e18, 3.0e19, 1.0e29, -6.02e28];

fn to_int_body() -> String {
    let mut body = String::new();
    for n in [65, 100, 128] {
        for (op, sign) in [("fptosi", "s"), ("fptoui", "u")] {
            for fp in ["half", "float", "double", "fp128"] {
                let cast = match fp {
                    "double" => {
                        "  %x = or i64 %a, 0\n  %xd = bitcast i64 %x to double\n".to_owned()
                    }
                    "fp128" => {
                        "  %xb = bitcast i64 %a to double\n  %xd = fpext double %xb to fp128\n"
                            .to_owned()
                    }
                    _ => format!(
                        "  %xb = bitcast i64 %a to double\n  %xd = fptrunc double %xb to {fp}\n"
                    ),
                };
                let mut f = format!("define i64 @{sign}{fp}_i{n}(i64 %a) {{\n{cast}");
                writeln!(f, "  %v = {op} {fp} %xd to i{n}").unwrap();
                f += &fold(n, "v", "h");
                f += "  ret i64 %h\n}\n";
                body += &f;
            }
        }
    }
    body
}

fn from_int_body() -> String {
    let mut body = String::new();
    for n in [65, 100, 128] {
        for (op, sign) in [("sitofp", "s"), ("uitofp", "u")] {
            for (fp, int) in [("half", 16), ("float", 32), ("double", 64), ("fp128", 128)] {
                let mut f = format!("define i64 @{sign}i{n}_{fp}(i64 %a, i64 %b) {{\n");
                f += &build(n, &["a", "b"], "x");
                writeln!(f, "  %v = {op} i{n} %x to {fp}\n  %bits = bitcast {fp} %v to i{int}")
                    .unwrap();
                f += &fold(int, "bits", "h");
                f += "  ret i64 %h\n}\n";
                body += &f;
            }
        }
    }
    body
}

#[test]
fn conversions_to_wide_integers() {
    let body = to_int_body();
    let mut calls = Vec::new();
    for n in [65, 100, 128] {
        for sign in ["s", "u"] {
            for fp in ["half", "float", "double", "fp128"] {
                for d in DOUBLES {
                    let d = if fp == "half" { d.clamp(-60000.0, 60000.0) } else { d };
                    let d = if sign == "u" { d.abs() } else { d };
                    let d = if n == 65 && d.abs() >= 1.8e19 { d / 1e11 } else { d };
                    calls.push((format!("{sign}{fp}_i{n}"), [d.to_bits(), 0, 0, 0, 0, 0]));
                }
            }
        }
    }
    run("a64-tail-fptoi", &body, &calls, None);
    // The interpreter on the types it models.
    let keep: Vec<native::Call> = calls
        .into_iter()
        .filter(|(name, _)| name.contains("float") || name.contains("double"))
        .collect();
    let want = interp_lines(&body, &keep);
    run("a64-tail-fptoi-interp", &body, &keep, Some(&want));
}

#[test]
fn conversions_from_wide_integers() {
    let body = from_int_body();
    let mut rng = Rng(0x0843_0084);
    let mut calls = Vec::new();
    for n in [65, 100, 128] {
        for sign in ["s", "u"] {
            for fp in ["half", "float", "double", "fp128"] {
                calls.extend(calls_of(&format!("{sign}i{n}_{fp}"), &mut rng, 24));
            }
        }
    }
    run("a64-tail-itofp", &body, &calls, None);
    let keep: Vec<native::Call> = calls
        .into_iter()
        .filter(|(name, _)| name.ends_with("float") || name.ends_with("double"))
        .collect();
    let want = interp_lines(&body, &keep);
    run("a64-tail-itofp-interp", &body, &keep, Some(&want));
}

// ---- switch on wide integers ----

fn switch_body(n: u32) -> String {
    let mut f = format!("define i64 @switch{n}(i64 %a, i64 %b, i64 %c) {{\nentry:\n");
    f += &build(n, &["a", "b", "c"], "x");
    let m = if n > 128 { 192 } else { n };
    let big = |hi: u128, lo: u128| (hi << 64) | lo;
    let mut cases = vec![
        ("0".to_owned(), "z"),
        ("1".to_owned(), "one"),
        ("-1".to_owned(), "m1"),
        (big(1, 0).to_string(), "hi"),
        (big(1, 1).to_string(), "hilo"),
        ((1u128 << 64).wrapping_sub(1).to_string(), "lo"),
        ("5".to_owned(), "entry_dup"),
    ];
    if m > 100 {
        cases.push((format!("-{}", big(0, 1 << 40)), "neg"));
    }
    write!(f, "  switch i{n} %x, label %def [").unwrap();
    for (value, label) in &cases {
        let label = if *label == "entry_dup" { "def" } else { label };
        write!(f, "\n    i{n} {value}, label %{label}").unwrap();
    }
    f += "\n    i";
    write!(f, "{n} 2, label %z\n  ]\n").unwrap();
    for (k, (_, label)) in cases.iter().enumerate() {
        if *label != "entry_dup" {
            writeln!(f, "{label}:\n  ret i64 {}", 100 + k).unwrap();
        }
    }
    f += "def:\n  ret i64 7\n}\n";
    f
}

#[test]
fn switch_on_wide_integers() {
    let mut body = String::new();
    let mut calls = Vec::new();
    let mut rng = Rng(0x0844_0084);
    for n in [65, 100, 128, 129, 256] {
        body += &switch_body(n);
        let name = format!("switch{n}");
        let words = [0, 1, 2, 5, u64::MAX, 1 << 40, (1u64 << 40).wrapping_neg()];
        for &a in &words {
            for &b in &[0, 1, u64::MAX] {
                for &c in &[0, u64::MAX] {
                    calls.push((name.clone(), [a, b, c, 0, 0, 0]));
                }
            }
        }
        calls.extend(calls_of(&name, &mut rng, 10));
    }
    run("a64-tail-switch", &body, &calls, None);
    let keep: Vec<native::Call> = calls
        .into_iter()
        .filter(|(name, _)| ["switch65", "switch100", "switch128"].contains(&name.as_str()))
        .collect();
    let want = interp_lines(&body, &keep);
    run("a64-tail-switch-interp", &body, &keep, Some(&want));
}

/// A constant condition branches to its case directly.
#[test]
fn switch_on_a_wide_constant() {
    let body = "define i64 @k() {\n  switch i128 18446744073709551617, label %a [\n    \
                i128 1, label %b\n    i128 18446744073709551617, label %c\n  ]\n\
                a:\n  ret i64 1\nb:\n  ret i64 2\nc:\n  ret i64 3\n}\n";
    run("a64-tail-switch-const", body, &[("k".to_owned(), [0; 6])], Some("3\n"));
}

// ---- readcyclecounter ----

#[test]
fn readcyclecounter_counts_up() {
    let body = "declare i64 @llvm.readcyclecounter()\n\
                define i64 @ticks() {\n  %a = call i64 @llvm.readcyclecounter()\n  \
                %b = call i64 @llvm.readcyclecounter()\n  %up = icmp uge i64 %b, %a\n  \
                %nz = icmp ne i64 %a, 0\n  %both = and i1 %up, %nz\n  \
                %r = zext i1 %both to i64\n  ret i64 %r\n}\n";
    run("a64-tail-cntvct", body, &[("ticks".to_owned(), [0; 6])], Some("1\n"));
}

// ---- {s,u}shl.sat ----

/// The model: `a << b` in `bits` bits, saturated when shifting back does
/// not give `a`.
fn shl_sat(signed: bool, bits: u32, a: u64, b: u64) -> u64 {
    let mask = if bits == 64 { u64::MAX } else { (1 << bits) - 1 };
    let a = a & mask;
    let shifted = (a << b) & mask;
    if signed {
        let sext = |v: u64| ((v << (64 - bits)) as i64) >> (64 - bits);
        let back = sext(shifted) >> b;
        if back == sext(a) {
            shifted
        } else if sext(a) < 0 {
            (1 << (bits - 1)) & mask
        } else {
            mask >> 1
        }
    } else if shifted >> b == a {
        shifted
    } else {
        mask
    }
}

#[test]
fn saturating_shifts() {
    let widths = [1, 7, 8, 16, 31, 32, 33, 63, 64];
    let mut body = String::new();
    let mut calls = Vec::new();
    let mut want = String::new();
    let mut rng = Rng(0x0845_0084);
    for &n in &widths {
        for op in ["sshl", "ushl"] {
            let (down, up) = if n == 64 { ("or", "or") } else { ("trunc", "zext") };
            let (to_n, to_64) = if n == 64 { (", 0", ", 0") } else { (" to i{n}", " to i64") };
            let to_n = to_n.replace("{n}", &n.to_string());
            writeln!(
                body,
                "declare i{n} @llvm.{op}.sat.i{n}(i{n}, i{n})\n\
                 define i64 @{op}{n}(i64 %a, i64 %b) {{\n  %x = {down} i64 %a{to_n}\n  \
                 %y = {down} i64 %b{to_n}\n  %r = call i{n} @llvm.{op}.sat.i{n}(i{n} %x, i{n} %y)\n  \
                 %z = {up} i{n} %r{to_64}\n  ret i64 %z\n}}"
            )
            .unwrap();
            for _ in 0..30 {
                let a = rng.word();
                // Junk above the amount's width: the callee must not read it.
                let b = (rng.next() % u64::from(n))
                    | ((rng.next() << n.min(63)) & !((1 << n.min(63)) - 1));
                let b = if n == 64 { b % 64 } else { b };
                calls.push((format!("{op}{n}"), [a, b, 0, 0, 0, 0]));
                let mask = if n == 64 { u64::MAX } else { (1 << n) - 1 };
                writeln!(want, "{}", shl_sat(op == "sshl", n, a, b & mask)).unwrap();
            }
        }
    }
    run("a64-tail-shlsat", &body, &calls, Some(&want));
}

// ---- crc32 ----

#[test]
fn crc32_matches_llc() {
    let mut body = String::new();
    let mut calls = Vec::new();
    let mut rng = Rng(0x0846_0084);
    for (kind, ty) in [
        ("b", "i32"),
        ("h", "i32"),
        ("w", "i32"),
        ("x", "i64"),
        ("cb", "i32"),
        ("ch", "i32"),
        ("cw", "i32"),
        ("cx", "i64"),
    ] {
        let data = if ty == "i64" { "%b".to_owned() } else { "%bt".to_owned() };
        writeln!(
            body,
            "declare i32 @llvm.aarch64.crc32{kind}(i32, {ty})\n\
             define i64 @crc{kind}(i64 %a, i64 %b) #0 {{\n  %at = trunc i64 %a to i32\n  \
             %bt = trunc i64 %b to i32\n  %r = call i32 @llvm.aarch64.crc32{kind}(i32 %at, {ty} {data})\n  \
             %z = zext i32 %r to i64\n  ret i64 %z\n}}"
        )
        .unwrap();
        calls.extend(calls_of(&format!("crc{kind}"), &mut rng, 12));
    }
    body += "attributes #0 = { \"target-features\"=\"+crc\" }\n";
    run("a64-tail-crc32", &body, &calls, None);
}

// ---- is.constant ----

/// `llvm.is.constant` of literal aggregates and expressions of data is
/// true, of anything holding an address false (`llc`'s
/// `isManifestConstant`); `objectsize` folds as before.
#[test]
fn is_constant_of_aggregates() {
    let body = "
@g = global i32 0
declare i1 @llvm.is.constant.sl_i32i32s({ i32, i32 })
declare i1 @llvm.is.constant.a2i32([2 x i32])
declare i1 @llvm.is.constant.a3i8([3 x i8])
declare i1 @llvm.is.constant.v4i32(<4 x i32>)
declare i1 @llvm.is.constant.p0(ptr)
declare i1 @llvm.is.constant.sl_i32p0s({ i32, ptr })
declare i1 @llvm.is.constant.i64(i64)
define i64 @isc(i64 %a) {
  %c0 = call i1 @llvm.is.constant.sl_i32i32s({ i32, i32 } { i32 1, i32 2 })
  %c1 = call i1 @llvm.is.constant.a2i32([2 x i32] [i32 1, i32 2])
  %c2 = call i1 @llvm.is.constant.a3i8([3 x i8] c\"ab\\00\")
  %c3 = call i1 @llvm.is.constant.v4i32(<4 x i32> <i32 1, i32 2, i32 3, i32 4>)
  %c4 = call i1 @llvm.is.constant.v4i32(<4 x i32> splat (i32 9))
  %c5 = call i1 @llvm.is.constant.p0(ptr inttoptr (i64 64 to ptr))
  %c6 = call i1 @llvm.is.constant.p0(ptr @g)
  %c7 = call i1 @llvm.is.constant.sl_i32p0s({ i32, ptr } { i32 1, ptr @g })
  %c8 = call i1 @llvm.is.constant.i64(i64 ptrtoint (ptr @g to i64))
  %c9 = call i1 @llvm.is.constant.i64(i64 %a)
  %c10 = call i1 @llvm.is.constant.sl_i32i32s({ i32, i32 } zeroinitializer)
  %r0 = zext i1 %c0 to i64
  %r1 = zext i1 %c1 to i64
  %r2 = zext i1 %c2 to i64
  %r3 = zext i1 %c3 to i64
  %r4 = zext i1 %c4 to i64
  %r5 = zext i1 %c5 to i64
  %r6 = zext i1 %c6 to i64
  %r7 = zext i1 %c7 to i64
  %r8 = zext i1 %c8 to i64
  %r9 = zext i1 %c9 to i64
  %r10 = zext i1 %c10 to i64
  %s1 = shl i64 %r1, 1
  %s2 = shl i64 %r2, 2
  %s3 = shl i64 %r3, 3
  %s4 = shl i64 %r4, 4
  %s5 = shl i64 %r5, 5
  %s6 = shl i64 %r6, 6
  %s7 = shl i64 %r7, 7
  %s8 = shl i64 %r8, 8
  %s9 = shl i64 %r9, 9
  %s10 = shl i64 %r10, 10
  %o1 = or i64 %r0, %s1
  %o2 = or i64 %o1, %s2
  %o3 = or i64 %o2, %s3
  %o4 = or i64 %o3, %s4
  %o5 = or i64 %o4, %s5
  %o6 = or i64 %o5, %s6
  %o7 = or i64 %o6, %s7
  %o8 = or i64 %o7, %s8
  %o9 = or i64 %o8, %s9
  %o10 = or i64 %o9, %s10
  ret i64 %o10
}
";
    // 0..=5 and 10 true; 6..=9 false.
    let want = format!("{}\n", 0b100_0011_1111);
    run("a64-tail-isconst", body, &[("isc".to_owned(), [0; 6])], Some(&want));
}

// ---- invoke with an odd integer result ----

/// An `invoke` whose `{ i7, i64 }` result the callee leaves junk above:
/// the normal edge sees the bits above `i7` clear, in a `phi` and in
/// memory.
#[test]
fn invoke_with_an_odd_result() {
    let body = "
define i32 @pers(...) {
  ret i32 0
}
define { i64, i64 } @junk(i64 %a) {
  %s0 = insertvalue { i64, i64 } undef, i64 %a, 0
  %s = insertvalue { i64, i64 } %s0, i64 5, 1
  ret { i64, i64 } %s
}
define i64 @inv(i64 %a, i64 %c) personality ptr @pers {
entry:
  %p = alloca { i7, i64 }
  %t = trunc i64 %c to i1
  br i1 %t, label %call, label %other
call:
  %r = invoke { i7, i64 } @junk(i64 %a) to label %ok unwind label %pad
other:
  br label %ok
ok:
  %v = phi { i7, i64 } [ %r, %call ], [ { i7 3, i64 4 }, %other ]
  store { i7, i64 } %v, ptr %p
  %byte = load i8, ptr %p
  %z = zext i8 %byte to i64
  %f = getelementptr i8, ptr %p, i64 8
  %w = load i64, ptr %f
  %w8 = shl i64 %w, 8
  %res = or i64 %z, %w8
  ret i64 %res
pad:
  %l = landingpad { ptr, i32 } cleanup
  ret i64 0
}
";
    let calls: Vec<native::Call> = [(u64::MAX, 1), (0xff, 1), (0x80, 1), (0x7f, 1), (0, 0)]
        .into_iter()
        .map(|(a, c)| ("inv".to_owned(), [a, c, 0, 0, 0, 0]))
        .collect();
    let want = "1407\n1407\n1280\n1407\n1027\n";
    run("a64-tail-invoke", body, &calls, Some(want));
}

// ---- what stays refused ----

#[test]
fn what_stays_refused_is_refused_by_name() {
    assert_refused(&[
        (
            "define double @f(i129 %a) { %r = sitofp i129 %a to double\n ret double %r }",
            aggr::WIDE_FP_CAST,
        ),
        (
            "define i256 @f(double %a) { %r = fptoui double %a to i256\n ret i256 %r }",
            aggr::WIDE_FP_CAST,
        ),
        (
            "define bfloat @f(i128 %a) { %r = sitofp i128 %a to bfloat\n ret bfloat %r }",
            aggr::WIDE_FP_CAST,
        ),
        (
            "declare { i2049, i1 } @llvm.umul.with.overflow.i2049(i2049, i2049)\n\
             define { i2049, i1 } @f(i2049 %a) { %r = call { i2049, i1 } @llvm.umul.with.overflow.i2049(i2049 %a, i2049 3)\n ret { i2049, i1 } %r }",
            aggr::WIDE_MUL_OVERFLOW,
        ),
        (
            "declare i128 @llvm.sshl.sat.i128(i128, i128)\n\
             define i128 @f(i128 %a, i128 %b) { %r = call i128 @llvm.sshl.sat.i128(i128 %a, i128 %b)\n ret i128 %r }",
            hod_a64::intrinsic::WIDE,
        ),
    ]);
    // At the limit: lowered.
    compile_ll(
        "declare { i2048, i1 } @llvm.smul.with.overflow.i2048(i2048, i2048)\n\
         define { i2048, i1 } @f(i2048 %a) { %r = call { i2048, i1 } @llvm.smul.with.overflow.i2048(i2048 %a, i2048 3)\n ret { i2048, i1 } %r }",
    )
    .expect("i2048 is lowered");
}

// ---- the operands of folded is.constant and objectsize ----

/// `llc` folds `is.constant` and `objectsize` before instruction
/// selection, and the operand chains nothing else uses in their block go
/// with them: a load through an invalid pointer (GCC's `bcp-1.c`) is not
/// run. Called with a null pointer; every build must return normally.
const FOLDED_OPERANDS: &str = "
declare i1 @llvm.is.constant.i8(i8)
declare i1 @llvm.is.constant.i64(i64)
declare i64 @llvm.objectsize.i64.p0(ptr, i1, i1, i1)

; WP-086's reproducer, without `convergent` on the declaration.
define i64 @bcp(i64 %a) {
  %p = inttoptr i64 %a to ptr
  %q = getelementptr inbounds i8, ptr %p, i64 123456
  %v = load i8, ptr %q, align 1
  %c = call i1 @llvm.is.constant.i8(i8 %v)
  %r = zext i1 %c to i64
  ret i64 %r
}

; The chain in another block than the call: its only use is the call.
define i64 @cross(i64 %a) {
entry:
  %p = inttoptr i64 %a to ptr
  %v = load i64, ptr %p
  %w = mul i64 %v, 3
  br label %next
next:
  %c = call i1 @llvm.is.constant.i64(i64 %w)
  %r = zext i1 %c to i64
  ret i64 %r
}

; objectsize of a pointer loaded through an invalid one.
define i64 @osize(i64 %a) {
  %pp = inttoptr i64 %a to ptr
  %p = load ptr, ptr %pp
  %s = call i64 @llvm.objectsize.i64.p0(ptr %p, i1 false, i1 false, i1 false)
  ret i64 %s
}

; A dead chain beside a folded call: deleted too.
define i64 @beside(i64 %a) {
  %p = inttoptr i64 %a to ptr
  %d = load i64, ptr %p
  %e = add i64 %d, 1
  %c = call i1 @llvm.is.constant.i64(i64 7)
  %r = zext i1 %c to i64
  ret i64 %r
}

; An operand that something else uses is computed.
define i64 @kept(i64 %a) {
  %v = add i64 %a, 40
  %c = call i1 @llvm.is.constant.i64(i64 %v)
  %z = zext i1 %c to i64
  %r = add i64 %v, %z
  ret i64 %r
}

; Used in another block by an instruction that is itself dead there:
; still exported, so computed (here through a valid pointer).
define i64 @exported(i64 %a) {
entry:
  %p = alloca i64
  store i64 %a, ptr %p
  %v = load i64, ptr %p
  %c = call i1 @llvm.is.constant.i64(i64 %v)
  br label %next
next:
  %dead = mul i64 %v, 5
  %r = zext i1 %c to i64
  ret i64 %r
}
";

#[test]
fn folded_operands_are_not_evaluated() {
    let calls: Vec<native::Call> = ["bcp", "cross", "osize", "beside", "kept", "exported"]
        .into_iter()
        .map(|name| {
            (
                name.to_owned(),
                [if name == "kept" || name == "exported" { 2 } else { 0 }, 0, 0, 0, 0, 0],
            )
        })
        .collect();
    let want = format!("0\n0\n{}\n1\n42\n0\n", u64::MAX);
    run("a64-tail-folded", FOLDED_OPERANDS, &calls, Some(&want));
}
