//! Arithmetic on vectors of `i128` and `x86_fp80` lanes and
//! `llvm.x86.sse2.pause` (WP-161, `spec/codegen.md` §5.6.19), against
//! `hod_interp` and `llc -O0 -fast-isel=0` of LLVM 22 and 19 under the
//! emulator (`common/float_gen.rs`; hod plain and stressed):
//!
//! - `i128` lanes: every integer binary operator with both operands
//!   built from the arguments and with a constant vector of edge values
//!   on either side; shifts by variable amounts, by constant amounts at
//!   the boundaries (lanes at 128 and beyond are poison and not hashed),
//!   and by a variable amount of 128 or more in one lane next to defined
//!   lanes; the casts to and from 8- to 64-bit lanes, pointers, `float`
//!   and `double`; `select` on a mask;
//! - `x86_fp80` lanes: `fadd`, `fsub`, `fmul`, `fdiv`, `fneg`, every
//!   `fcmp` predicate (NaN, infinities and zeros among the lanes) and
//!   `select`;
//! - `pause`;
//! - the refusals that stay, by name.

#![allow(
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    reason = "tables of functions and values"
)]

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

#[path = "common/float_gen.rs"]
mod float_gen;

use std::fmt::Write as _;

use float_gen::{Check, Rng, Suite, T, check, disassembly, ll_header, mnemonic, parse, pic};
use hod_cg::{CgError, Stress};

const MAJORS: [u32; 2] = [22, 19];
const I64: T = T::I(64);
const SEED: &str = "-3750763034362895579";

/// 64-bit words at the edges of carries, borrows, signs and shifts.
const EDGES: [u64; 14] = [
    0,
    1,
    u64::MAX,
    1 << 63,
    (1 << 63) - 1,
    1 << 32,
    0xffff_ffff,
    2,
    63,
    64,
    65,
    127,
    128,
    u64::MAX - 1,
];

/// Argument lists of four words that make lane 0 of the built operands
/// (`lo`, `hi` of `x`, then of `y`) an interesting pair: a carry out of
/// the low half, a borrow into it, signed overflow, equal values.
const PAIRS: [[u64; 4]; 8] = [
    [u64::MAX, 0, 1, 0],
    [0, 0, 1, 0],
    [0, 1 << 63, 0, 1 << 63],
    [u64::MAX, u64::MAX, u64::MAX, u64::MAX],
    [u64::MAX, (1 << 63) - 1, 1, 0],
    [0, 1 << 63, u64::MAX, u64::MAX],
    [5, 0, 3, 0],
    [7, u64::MAX, 2, 0],
];

/// Edge values of `i128` (constant lanes, by index modulo the length).
const WIDE_EDGES: [i128; 12] = [
    0,
    1,
    -1,
    i128::MIN,
    i128::MAX,
    0xffff_ffff_ffff_ffff,
    1 << 64,
    -(1 << 64),
    1 << 63,
    3,
    -2,
    0x1234_5678_9abc_def0_0fed_cba9_8765_4321,
];

/// Compiles `text` (functions after the module header) with hod and
/// returns the error, if any.
fn compile(text: &str) -> Result<(), CgError> {
    let (module, bodies) = parse(&format!("{}{text}", ll_header()));
    match hod_x64::compile_to_object(&module, &bodies, &pic(Stress::default())) {
        Ok(_) => Ok(()),
        Err(hod_x64::ModuleError::Function { error, .. }) => Err(error),
        Err(other) => panic!("{other}"),
    }
}

/// Asserts that hod refuses `text` as `name`.
fn refused(text: &str, name: &'static str) {
    assert_eq!(compile(text), Err(CgError::unsupported(name)), "{text}");
}

/// Lines that build `<n x i128>` `%{v}` lane by lane from the `i64`
/// values `lo` and `hi`: lane `i` is `(lo + i·K) | (hi ^ i·L) << 64`, so
/// lane 0 is exactly `(lo, hi)`.
fn build(n: u32, v: &str, lo: &str, hi: &str) -> String {
    let vt = format!("<{n} x i128>");
    let mut text = String::new();
    let mut prev = "poison".to_string();
    for i in 0..n {
        let (k, l) = (i64::from(i) * 0x0101_0101_0101, i64::from(i) * 0x0a0b_0c0d);
        writeln!(
            text,
            "  %{v}.x{i} = add i64 {lo}, {k}\n  %{v}.y{i} = xor i64 {hi}, {l}\n  \
             %{v}.l{i} = zext i64 %{v}.x{i} to i128\n  %{v}.u{i} = zext i64 %{v}.y{i} to i128\n  \
             %{v}.h{i} = shl i128 %{v}.u{i}, 64\n  %{v}.k{i} = or i128 %{v}.h{i}, %{v}.l{i}"
        )
        .unwrap();
        let next = if i + 1 == n { format!("%{v}") } else { format!("%{v}.c{i}") };
        writeln!(text, "  {next} = insertelement {vt} {prev}, i128 %{v}.k{i}, i32 {i}").unwrap();
        prev = next;
    }
    text
}

/// Lines that hash the lanes `lanes` of `<n x E>` `%{v}` (`E` an integer
/// of `bits` bits, a `ptr`, or `x86_fp80`) into `acc`; returns them and
/// the new hash's name.
fn fold(n: u32, elem: &str, v: &str, lanes: &[u32], acc: &str) -> (String, String) {
    let mut text = String::new();
    let mut acc = acc.to_string();
    for &i in lanes {
        let e = format!("{v}.e{i}");
        writeln!(text, "  %{e} = extractelement <{n} x {elem}> %{v}, i32 {i}").unwrap();
        // The lane as an `i128` %{e}.b.
        match elem {
            "i128" => writeln!(text, "  %{e}.b = add i128 %{e}, 0").unwrap(),
            "x86_fp80" => writeln!(
                text,
                "  %{e}.c = bitcast x86_fp80 %{e} to i80\n  %{e}.b = zext i80 %{e}.c to i128"
            )
            .unwrap(),
            "ptr" => writeln!(
                text,
                "  %{e}.c = ptrtoint ptr %{e} to i64\n  %{e}.b = zext i64 %{e}.c to i128"
            )
            .unwrap(),
            "float" => writeln!(
                text,
                "  %{e}.c = bitcast float %{e} to i32\n  %{e}.b = zext i32 %{e}.c to i128"
            )
            .unwrap(),
            "double" => writeln!(
                text,
                "  %{e}.c = bitcast double %{e} to i64\n  %{e}.b = zext i64 %{e}.c to i128"
            )
            .unwrap(),
            int => writeln!(text, "  %{e}.b = zext {int} %{e} to i128").unwrap(),
        }
        writeln!(
            text,
            "  %{e}.l = trunc i128 %{e}.b to i64\n  %{e}.s = lshr i128 %{e}.b, 64\n  \
             %{e}.u = trunc i128 %{e}.s to i64\n  %{e}.m = mul i64 {acc}, 1099511628211\n  \
             %{e}.x = xor i64 %{e}.m, %{e}.l\n  %{e}.n = mul i64 %{e}.x, 1099511628211\n  \
             %{e}.z = xor i64 %{e}.n, %{e}.u"
        )
        .unwrap();
        acc = format!("%{e}.z");
    }
    (text, acc)
}

/// Lines that hash the mask `<n x i1>` `%{m}` into `acc`.
fn fold_mask(n: u32, m: &str, acc: &str) -> (String, String) {
    let mut text = String::new();
    let mut acc = acc.to_string();
    for i in 0..n {
        writeln!(
            text,
            "  %{m}.e{i} = extractelement <{n} x i1> %{m}, i32 {i}\n  \
             %{m}.z{i} = zext i1 %{m}.e{i} to i64\n  %{m}.s{i} = shl i64 %{m}.z{i}, {i}\n  \
             %{m}.h{i} = mul i64 {acc}, 1099511628211\n  %{m}.x{i} = xor i64 %{m}.h{i}, %{m}.s{i}"
        )
        .unwrap();
        acc = format!("%{m}.x{i}");
    }
    (text, acc)
}

/// Ends `body` with `%r`, the hash of the masks `%c` and `%d` of `n`
/// lanes.
fn hash_masks(body: &mut String, n: u32) {
    let (first, hash) = fold_mask(n, "c", SEED);
    body.push_str(&first);
    let (second, hash) = fold_mask(n, "d", &hash);
    body.push_str(&second);
    writeln!(body, "  %r = add i64 {hash}, 0").unwrap();
}

/// Every lane of `n`.
fn all(n: u32) -> Vec<u32> {
    (0..n).collect()
}

/// A constant `<n x i128>` of edge values, starting at `WIDE_EDGES[k]`;
/// zeros replaced by 7 for a divisor (`nonzero`).
fn edge_vector(n: u32, k: usize, nonzero: bool) -> String {
    let lanes: Vec<String> = (0..n as usize)
        .map(|i| {
            let v = WIDE_EDGES[(k + i) % WIDE_EDGES.len()];
            format!("i128 {}", if nonzero && v == 0 { 7 } else { v })
        })
        .collect();
    format!("<{}>", lanes.join(", "))
}

/// Ends `body` with `%r`, the hash of `<n x elem>` `%{v}`.
fn hashed(mut body: String, n: u32, elem: &str, v: &str, lanes: &[u32]) -> String {
    let (f, h) = fold(n, elem, v, lanes, SEED);
    body.push_str(&f);
    writeln!(body, "  %r = add i64 {h}, 0").unwrap();
    body
}

/// The `i128`-lane functions of `n` lanes.
fn int_functions(suite: &mut Suite, n: u32) -> Vec<usize> {
    let vt = format!("<{n} x i128>");
    let mut funcs = Vec::new();
    let ops = ["add", "sub", "mul", "and", "or", "xor", "udiv", "sdiv", "urem", "srem"];
    for (k, op) in ops.iter().enumerate() {
        let divides = op.ends_with("div") || op.ends_with("rem");
        // Both built from the arguments (a zero divisor lane is
        // undefined: the interpreter traps and the case is not run).
        let mut body = build(n, "x", "%a0", "%a1");
        body.push_str(&build(n, "y", "%a2", "%a3"));
        writeln!(body, "  %v = {op} {vt} %x, %y").unwrap();
        let body = hashed(body, n, "i128", "v", &all(n));
        funcs.push(suite.add(&format!("{op}_vv_{n}"), &[I64; 4], I64, &body, Check::Interp));
        // A constant on the right, then on the left.
        let c = edge_vector(n, k, divides);
        let mut body = build(n, "x", "%a0", "%a1");
        writeln!(body, "  %v = {op} {vt} %x, {c}").unwrap();
        let body = hashed(body, n, "i128", "v", &all(n));
        funcs.push(suite.add(&format!("{op}_vc_{n}"), &[I64; 2], I64, &body, Check::Interp));
        let c = edge_vector(n, k + 3, false);
        let mut body = build(n, "x", "%a0", "%a1");
        writeln!(body, "  %v = {op} {vt} {c}, %x").unwrap();
        let body = hashed(body, n, "i128", "v", &all(n));
        funcs.push(suite.add(&format!("{op}_cv_{n}"), &[I64; 2], I64, &body, Check::Interp));
    }
    let amounts = [0u32, 1, 63, 64, 65, 127, 100, 128, 200, 37];
    for op in ["shl", "lshr", "ashr"] {
        // Variable amounts below 128.
        let mut body = build(n, "x", "%a0", "%a1");
        body.push_str(&build(n, "y", "%a2", "%a3"));
        writeln!(body, "  %s = and {vt} %y, splat (i128 127)\n  %v = {op} {vt} %x, %s").unwrap();
        let body = hashed(body, n, "i128", "v", &all(n));
        funcs.push(suite.add(&format!("{op}_var_{n}"), &[I64; 4], I64, &body, Check::Interp));
        // Constant amounts: the lanes at 128 and beyond are poison.
        for start in [0, 5] {
            let lanes: Vec<u32> = (0..n as usize).map(|i| amounts[(start + i) % 10]).collect();
            let text: Vec<String> = lanes.iter().map(|a| format!("i128 {a}")).collect();
            let defined: Vec<u32> = (0..n).filter(|&i| lanes[i as usize] < 128).collect();
            let mut body = build(n, "x", "%a0", "%a1");
            writeln!(body, "  %v = {op} {vt} %x, <{}>", text.join(", ")).unwrap();
            let body = hashed(body, n, "i128", "v", &defined);
            let name = format!("{op}_const{start}_{n}");
            funcs.push(suite.add(&name, &[I64; 2], I64, &body, Check::Interp));
        }
        // A splat amount.
        let mut body = build(n, "x", "%a0", "%a1");
        writeln!(body, "  %v = {op} {vt} %x, splat (i128 65)").unwrap();
        let body = hashed(body, n, "i128", "v", &all(n));
        funcs.push(suite.add(&format!("{op}_splat_{n}"), &[I64; 2], I64, &body, Check::Interp));
        // A variable amount that may be 128 or more in lane 0 (poison,
        // not hashed) beside defined variable lanes: the other lanes and
        // the frame survive it.
        if n > 1 {
            let mut body = build(n, "x", "%a0", "%a1");
            body.push_str(&build(n, "y", "%a2", "%a3"));
            writeln!(
                body,
                "  %s0 = and {vt} %y, splat (i128 127)\n  %b = and i64 %a2, 255\n  \
                 %bw = zext i64 %b to i128\n  %s = insertelement {vt} %s0, i128 %bw, i32 0\n  \
                 %v = {op} {vt} %x, %s"
            )
            .unwrap();
            let body = hashed(body, n, "i128", "v", &(1..n).collect::<Vec<_>>());
            funcs.push(suite.add(&format!("{op}_wild_{n}"), &[I64; 4], I64, &body, Check::Interp));
        }
    }
    // Casts with integer and pointer lanes.
    for w in [8u32, 16, 32, 64] {
        let nt = format!("<{n} x i{w}>");
        let mut source = String::new();
        let mut prev = "poison".to_string();
        for i in 0..n {
            let k = i64::from(i) * 0x0123_4567_89ab_cdef_u64 as i64;
            let narrow = if w == 64 {
                "add i64 %n.x{i}, 0".to_string()
            } else {
                format!("trunc i64 %n.x{i} to i{w}")
            };
            let narrow = narrow.replace("{i}", &i.to_string());
            writeln!(source, "  %n.x{i} = xor i64 %a0, {k}\n  %n.t{i} = {narrow}").unwrap();
            let next = if i + 1 == n { "%n".to_string() } else { format!("%n.c{i}") };
            writeln!(source, "  {next} = insertelement {nt} {prev}, i{w} %n.t{i}, i32 {i}")
                .unwrap();
            prev = next;
        }
        for ext in ["zext", "sext"] {
            let body = format!("{source}  %v = {ext} {nt} %n to {vt}\n");
            let body = hashed(body, n, "i128", "v", &all(n));
            funcs.push(suite.add(&format!("{ext}{w}_{n}"), &[I64], I64, &body, Check::Interp));
        }
        let mut body = build(n, "x", "%a0", "%a1");
        writeln!(body, "  %v = trunc {vt} %x to {nt}").unwrap();
        let body = hashed(body, n, &format!("i{w}"), "v", &all(n));
        funcs.push(suite.add(&format!("trunc{w}_{n}"), &[I64; 2], I64, &body, Check::Interp));
    }
    {
        let pt = format!("<{n} x ptr>");
        let mut body = String::new();
        let mut prev = "poison".to_string();
        for i in 0..n {
            writeln!(
                body,
                "  %p.x{i} = add i64 %a0, {}\n  %p.k{i} = inttoptr i64 %p.x{i} to ptr",
                i64::from(i) * 4099
            )
            .unwrap();
            let next = if i + 1 == n { "%p".to_string() } else { format!("%p.c{i}") };
            writeln!(body, "  {next} = insertelement {pt} {prev}, ptr %p.k{i}, i32 {i}").unwrap();
            prev = next;
        }
        writeln!(body, "  %v = ptrtoint {pt} %p to {vt}").unwrap();
        let body = hashed(body, n, "i128", "v", &all(n));
        funcs.push(suite.add(&format!("ptrtoint_{n}"), &[I64], I64, &body, Check::Interp));
        let mut body = build(n, "x", "%a0", "%a1");
        writeln!(body, "  %v = inttoptr {vt} %x to {pt}").unwrap();
        let body = hashed(body, n, "ptr", "v", &all(n));
        funcs.push(suite.add(&format!("inttoptr_{n}"), &[I64; 2], I64, &body, Check::Interp));
    }
    // Conversions with `float` and `double` lanes.
    for fp in ["float", "double"] {
        let ft = format!("<{n} x {fp}>");
        for conv in ["sitofp", "uitofp"] {
            let mut body = build(n, "x", "%a0", "%a1");
            writeln!(body, "  %v = {conv} {vt} %x to {ft}").unwrap();
            let body = hashed(body, n, fp, "v", &all(n));
            funcs.push(suite.add(
                &format!("{conv}_{fp}_{n}"),
                &[I64; 2],
                I64,
                &body,
                Check::Interp,
            ));
        }
        // In range: an `i64` converted, scaled by 2^40 or 2^-10.
        for (conv, from) in [("fptosi", "sitofp"), ("fptoui", "uitofp")] {
            for (tag, scale) in [("big", "0x4270000000000000"), ("frac", "0x3F50000000000000")] {
                let mut body = String::new();
                let mut prev = "poison".to_string();
                for i in 0..n {
                    writeln!(
                        body,
                        "  %f.x{i} = add i64 %a0, {}\n  %f.d{i} = {from} i64 %f.x{i} to {fp}\n  \
                         %f.k{i} = fmul {fp} %f.d{i}, {scale}",
                        i64::from(i) * 0x0101_0101
                    )
                    .unwrap();
                    let next = if i + 1 == n { "%f".to_string() } else { format!("%f.c{i}") };
                    writeln!(body, "  {next} = insertelement {ft} {prev}, {fp} %f.k{i}, i32 {i}")
                        .unwrap();
                    prev = next;
                }
                writeln!(body, "  %v = {conv} {ft} %f to {vt}").unwrap();
                let body = hashed(body, n, "i128", "v", &all(n));
                let name = format!("{conv}_{fp}_{tag}_{n}");
                funcs.push(suite.add(&name, &[I64], I64, &body, Check::Interp));
            }
        }
    }
    // `select` on a mask of the argument's bits.
    {
        let c = edge_vector(n, 1, false);
        let mut body = build(n, "x", "%a0", "%a1");
        writeln!(
            body,
            "  %t = trunc i64 %a2 to i{n}\n  %m = bitcast i{n} %t to <{n} x i1>\n  \
             %v = select <{n} x i1> %m, {vt} %x, {vt} {c}"
        )
        .unwrap();
        let body = hashed(body, n, "i128", "v", &all(n));
        funcs.push(suite.add(&format!("select_{n}"), &[I64; 3], I64, &body, Check::Interp));
    }
    funcs
}

/// Adds random and edge cases of every function of `funcs`.
fn add_cases(suite: &mut Suite, rng: &mut Rng, funcs: &[usize], random: usize) {
    for &f in funcs {
        let arity = suite.funcs[f].ops.len();
        for _ in 0..random {
            let args: Vec<u64> = (0..arity).map(|_| rng.next()).collect();
            suite.case(f, &args);
        }
        for j in 0..6 {
            let args: Vec<u64> = (0..arity).map(|k| EDGES[(j * 3 + k * 5) % EDGES.len()]).collect();
            suite.case(f, &args);
        }
        if arity == 4 {
            for pair in PAIRS {
                suite.case(f, &pair);
            }
        } else if arity == 2 {
            for pair in &PAIRS[..4] {
                suite.case(f, &pair[..2]);
            }
        }
    }
}

/// §5.6.19 (1): the integer binary operators, shifts, casts and `select`
/// of `i128` lanes agree with the interpreter and with `llc` 22 and 19
/// (hod plain and stressed).
#[test]
fn i128_lanes_agree() {
    let mut suite = Suite::default();
    let mut rng = Rng(0x161);
    for n in [1, 2, 3, 4, 8] {
        let funcs = int_functions(&mut suite, n);
        add_cases(&mut suite, &mut rng, &funcs, 4);
    }
    // The long vectors of the libc++ module (`<32 x i128>`).
    {
        let n = 32;
        let vt = format!("<{n} x i128>");
        let mut funcs = Vec::new();
        for op in ["add", "sub", "xor", "mul", "lshr", "sdiv"] {
            let mut body = build(n, "x", "%a0", "%a1");
            body.push_str(&build(n, "y", "%a2", "%a3"));
            let rhs = match op {
                "lshr" => "%s".to_string(),
                "sdiv" => edge_vector(n, 2, true),
                _ => "%y".to_string(),
            };
            writeln!(body, "  %s = and {vt} %y, splat (i128 127)\n  %v = {op} {vt} %x, {rhs}")
                .unwrap();
            let body = hashed(body, n, "i128", "v", &all(n));
            funcs.push(suite.add(&format!("{op}_vv_{n}"), &[I64; 4], I64, &body, Check::Interp));
        }
        add_cases(&mut suite, &mut rng, &funcs, 2);
    }
    let report = check("wide-arith-i128", &suite, &MAJORS);
    assert!(report.interpreted > 0, "{report:?}");
}

/// Lines that build `<n x x86_fp80>` `%{v}` from the `i64` values `x`
/// and `y` (significand `x` with its integer bit, sign and exponent from
/// `y`, kept normal and in a range where no lane overflows).
fn build_fp80(n: u32, v: &str, x: &str, y: &str) -> String {
    let vt = format!("<{n} x x86_fp80>");
    let mut text = String::new();
    let mut prev = "poison".to_string();
    for i in 0..n {
        writeln!(
            text,
            "  %{v}.a{i} = add i64 {x}, {}\n  %{v}.b{i} = xor i64 {y}, {}\n  \
             %{v}.s{i} = or i64 %{v}.a{i}, -9223372036854775808\n  \
             %{v}.e{i} = and i64 %{v}.b{i}, 32831\n  %{v}.f{i} = or i64 %{v}.e{i}, 16320\n  \
             %{v}.l{i} = zext i64 %{v}.s{i} to i80\n  %{v}.g{i} = zext i64 %{v}.f{i} to i80\n  \
             %{v}.h{i} = shl i80 %{v}.g{i}, 64\n  %{v}.o{i} = or i80 %{v}.h{i}, %{v}.l{i}\n  \
             %{v}.k{i} = bitcast i80 %{v}.o{i} to x86_fp80",
            i64::from(i) * 0x0101_0101,
            i64::from(i) * 0x8003
        )
        .unwrap();
        let next = if i + 1 == n { format!("%{v}") } else { format!("%{v}.c{i}") };
        writeln!(text, "  {next} = insertelement {vt} {prev}, x86_fp80 %{v}.k{i}, i32 {i}")
            .unwrap();
        prev = next;
    }
    text
}

/// Special `x86_fp80` lanes: NaN, infinities, zeros, 1, -1, the largest
/// finite, the smallest denormal.
const FP80_SPECIAL: [&str; 9] = [
    "0xKFFFFC000000000000000",
    "0xK7FFF8000000000000000",
    "0xKFFFF8000000000000000",
    "0xK00000000000000000000",
    "0xK80000000000000000000",
    "0xK3FFF8000000000000000",
    "0xKBFFF8000000000000000",
    "0xK7FFEFFFFFFFFFFFFFFFF",
    "0xK00000000000000000001",
];

/// A constant `<n x x86_fp80>` starting at `FP80_SPECIAL[k]`, or of
/// ordinary numbers (`special` false: 1.5, -3, 0.25, ... ).
fn fp80_vector(n: u32, k: usize, special: bool) -> String {
    const PLAIN: [&str; 5] = [
        "0xK3FFFC000000000000000",
        "0xKC000C000000000000000",
        "0xK3FFD8000000000000000",
        "0xK4005C800000000000000",
        "0xKBFFEA000000000000000",
    ];
    let lanes: Vec<String> = (0..n as usize)
        .map(|i| {
            let v = if special {
                FP80_SPECIAL[(k + i) % FP80_SPECIAL.len()]
            } else {
                PLAIN[(k + i) % PLAIN.len()]
            };
            format!("x86_fp80 {v}")
        })
        .collect();
    format!("<{}>", lanes.join(", "))
}

/// The `x86_fp80`-lane functions of `n` lanes.
fn fp80_functions(suite: &mut Suite, n: u32) -> Vec<usize> {
    let vt = format!("<{n} x x86_fp80>");
    let mut funcs = Vec::new();
    for (k, op) in ["fadd", "fsub", "fmul", "fdiv"].iter().enumerate() {
        let mut body = build_fp80(n, "x", "%a0", "%a1");
        body.push_str(&build_fp80(n, "y", "%a2", "%a3"));
        writeln!(body, "  %v = {op} {vt} %x, %y").unwrap();
        let body = hashed(body, n, "x86_fp80", "v", &all(n));
        funcs.push(suite.add(&format!("{op}_vv_{n}"), &[I64; 4], I64, &body, Check::Llc));
        let c = fp80_vector(n, k, false);
        let mut body = build_fp80(n, "x", "%a0", "%a1");
        writeln!(body, "  %v = {op} {vt} {c}, %x").unwrap();
        let body = hashed(body, n, "x86_fp80", "v", &all(n));
        funcs.push(suite.add(&format!("{op}_cv_{n}"), &[I64; 2], I64, &body, Check::Llc));
    }
    {
        let mut body = build_fp80(n, "x", "%a0", "%a1");
        writeln!(body, "  %v = fneg {vt} %x").unwrap();
        let body = hashed(body, n, "x86_fp80", "v", &all(n));
        funcs.push(suite.add(&format!("fneg_{n}"), &[I64; 2], I64, &body, Check::Llc));
        // `fneg` of special values (a NaN keeps its payload).
        let c = fp80_vector(n, 2, true);
        let body = format!("  %v = fneg {vt} {c}\n  %z = add i64 %a0, 0\n");
        let body = hashed(body, n, "x86_fp80", "v", &all(n));
        funcs.push(suite.add(&format!("fneg_special_{n}"), &[I64], I64, &body, Check::Llc));
    }
    let preds = [
        "false", "oeq", "ogt", "oge", "olt", "ole", "one", "ord", "uno", "ueq", "ugt", "uge",
        "ult", "ule", "une", "true",
    ];
    for p in preds {
        // Built operands, equal in lane 0 when the arguments agree.
        let mut body = build_fp80(n, "x", "%a0", "%a1");
        body.push_str(&build_fp80(n, "y", "%a2", "%a3"));
        writeln!(body, "  %c = fcmp {p} {vt} %x, %y\n  %d = fcmp {p} {vt} %x, %x").unwrap();
        hash_masks(&mut body, n);
        funcs.push(suite.add(&format!("fcmp_{p}_{n}"), &[I64; 4], I64, &body, Check::Llc));
        // Against the special values, both ways round.
        let special = fp80_vector(n, n as usize, true);
        let mut body = build_fp80(n, "x", "%a0", "%a1");
        writeln!(body, "  %c = fcmp {p} {vt} %x, {special}\n  %d = fcmp {p} {vt} {special}, %x")
            .unwrap();
        hash_masks(&mut body, n);
        funcs.push(suite.add(&format!("fcmpc_{p}_{n}"), &[I64; 2], I64, &body, Check::Llc));
    }
    {
        let c = fp80_vector(n, 0, true);
        let mut body = build_fp80(n, "x", "%a0", "%a1");
        writeln!(
            body,
            "  %t = trunc i64 %a2 to i{n}\n  %m = bitcast i{n} %t to <{n} x i1>\n  \
             %v = select <{n} x i1> %m, {vt} {c}, {vt} %x"
        )
        .unwrap();
        let body = hashed(body, n, "x86_fp80", "v", &all(n));
        funcs.push(suite.add(&format!("fselect_{n}"), &[I64; 3], I64, &body, Check::Llc));
    }
    funcs
}

/// §5.6.19 (2): `fadd` .. `fdiv`, `fneg`, `fcmp` and `select` of
/// `x86_fp80` lanes agree with `llc` 22 and 19 (hod plain and stressed;
/// the interpreter has no `x86_fp80`).
#[test]
fn x86_fp80_lanes_agree() {
    let mut suite = Suite::default();
    let mut rng = Rng(0x1612);
    for n in [1, 2, 3, 4] {
        let funcs = fp80_functions(&mut suite, n);
        add_cases(&mut suite, &mut rng, &funcs, 4);
    }
    let report = check("wide-arith-fp80", &suite, &MAJORS);
    assert!(report.compared > 0, "{report:?}");
}

/// §5.6.19 (3): `llvm.x86.sse2.pause` is the `pause` instruction
/// (`F3 90`), as `llc`'s, and the function around it computes what
/// `llc`'s does.
#[test]
fn pause_is_the_instruction() {
    let mut suite = Suite::default();
    suite.declare("declare void @llvm.x86.sse2.pause()");
    let body = "  call void @llvm.x86.sse2.pause()\n  %b = mul i64 %a0, 3\n  \
                call void @llvm.x86.sse2.pause()\n  %r = add i64 %b, %a1\n";
    let f = suite.add("spin", &[I64; 2], I64, body, Check::Llc);
    for args in [[0, 0], [1, 2], [u64::MAX, 7]] {
        suite.case(f, &args);
    }
    check("wide-arith-pause", &suite, &MAJORS);
    let text = format!("{}\n{}", suite.decls, suite.text);
    if let Some(lines) = disassembly(&text, Stress::default()) {
        let pauses = lines.iter().filter(|l| mnemonic(l) == "pause").count();
        assert_eq!(pauses, 2, "{lines:?}");
    }
}

/// What stays refused on these lanes, by name: `frem` of `x86_fp80`
/// lanes, conversions between `i128` lanes and `half` or `x86_fp80`
/// lanes, the integer intrinsics of `i128` lanes, arithmetic on `fp128`
/// lanes.
#[test]
fn wide_arith_refusals_by_name() {
    let wide = hod_x64::vector::WIDE_ELEMENT;
    refused(
        "define <2 x x86_fp80> @f(<2 x x86_fp80> %a, <2 x x86_fp80> %b) {\n  \
         %r = frem <2 x x86_fp80> %a, %b\n  ret <2 x x86_fp80> %r\n}\n",
        wide,
    );
    refused(
        "define void @f(<2 x i128> %a, ptr %p) {\n  %r = sitofp <2 x i128> %a to <2 x half>\n  \
         store <2 x half> %r, ptr %p\n  ret void\n}\n",
        wide,
    );
    refused(
        "define void @f(<2 x i128> %a, ptr %p) {\n  \
         %r = uitofp <2 x i128> %a to <2 x x86_fp80>\n  store <2 x x86_fp80> %r, ptr %p\n  \
         ret void\n}\n",
        wide,
    );
    refused(
        "define void @f(<2 x float> %a, ptr %p) {\n  %r = fpext <2 x float> %a to <2 x x86_fp80>\n  \
         store <2 x x86_fp80> %r, ptr %p\n  ret void\n}\n",
        wide,
    );
    refused(
        "declare <2 x i128> @llvm.smax.v2i128(<2 x i128>, <2 x i128>)\n\
         define void @f(ptr %p) {\n  %a = load <2 x i128>, ptr %p\n  \
         %r = call <2 x i128> @llvm.smax.v2i128(<2 x i128> %a, <2 x i128> %a)\n  \
         store <2 x i128> %r, ptr %p\n  ret void\n}\n",
        wide,
    );
    refused(
        "define void @f(ptr %p) {\n  %a = load <2 x fp128>, ptr %p\n  \
         %r = fadd <2 x fp128> %a, %a\n  store <2 x fp128> %r, ptr %p\n  ret void\n}\n",
        wide,
    );
}
