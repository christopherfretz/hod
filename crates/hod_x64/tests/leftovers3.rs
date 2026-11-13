//! The x86-64 leftovers of the first libc++ run (WP-152,
//! `spec/codegen.md` §5.6.18, `spec/abi-x64.md` §4.3, §6.2):
//!
//! - lanes of `i128` and `x86_fp80` (`insertelement`, `extractelement`,
//!   `shufflevector`, memory), against `hod_interp` and `llc -O0
//!   -fast-isel=0` of LLVM 22 and 19 (`common/float_gen.rs`);
//! - vectors of `i128` and `x86_fp80` as arguments and results, and
//!   `byval` arguments aligned above 16: a two-module program whose
//!   callers and callees each come from hod (plain, stressed) or `llc`
//!   (22, 19), every pairing, linked by bfd, lld and mold;
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
use std::path::PathBuf;

use float_gen::{Check, Rng, Suite, T, check, ll_header, parse, pic, text_size};
use hod_cg::{CgError, Stress};
use hod_testutil::Scratch;

const MAJORS: [u32; 2] = [22, 19];
const I64: T = T::I(64);

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

// ---- 1. wide lanes ----

/// A lane type of the wide vectors.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Wide {
    I128,
    X86Fp80,
}

impl Wide {
    fn ll(self) -> &'static str {
        match self {
            Wide::I128 => "i128",
            Wide::X86Fp80 => "x86_fp80",
        }
    }

    /// The bytes between lanes in memory (LLVM packs vector elements).
    fn stride(self) -> u64 {
        match self {
            Wide::I128 => 16,
            Wide::X86Fp80 => 10,
        }
    }

    /// The lane counts the tests take.
    fn lengths(self) -> &'static [u32] {
        match self {
            Wide::I128 => &[1, 2, 3, 4, 8],
            Wide::X86Fp80 => &[1, 2, 3, 4],
        }
    }

    /// A constant lane `i`: distinct bits per lane, a normal number for
    /// `x86_fp80`.
    fn lane_const(self, i: u32) -> String {
        match self {
            Wide::I128 => {
                let v = (u128::from(i) + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15_f39c_c060_5ced_c835);
                format!("i128 {}", v as i128)
            }
            Wide::X86Fp80 => {
                let exp = 0x3fff + i + if i % 2 == 1 { 0x8000 } else { 0 };
                let sig =
                    0x8000_0000_0000_0000_u64 | u64::from(i + 1).wrapping_mul(0x1234_5678_9abc);
                format!("x86_fp80 0xK{exp:04X}{sig:016X}")
            }
        }
    }

    /// A constant `<n x E>`.
    fn vec_const(self, n: u32) -> String {
        let lanes: Vec<String> = (0..n).map(|i| self.lane_const(i)).collect();
        format!("<{}>", lanes.join(", "))
    }

    /// Lines that build the lane `%{name}` from the `i64` values `x`
    /// and `y` (an `x86_fp80`: significand `x` with its integer bit,
    /// sign and exponent from `y`, kept normal).
    fn lane(self, name: &str, x: &str, y: &str) -> String {
        match self {
            Wide::I128 => format!(
                "  %{name}.l = zext i64 {x} to i128\n  %{name}.y = zext i64 {y} to i128\n  \
                 %{name}.h = shl i128 %{name}.y, 64\n  %{name} = or i128 %{name}.h, %{name}.l\n"
            ),
            Wide::X86Fp80 => format!(
                "  %{name}.s = or i64 {x}, -9223372036854775808\n  \
                 %{name}.e = and i64 {y}, 36863\n  %{name}.f = or i64 %{name}.e, 12288\n  \
                 %{name}.l = zext i64 %{name}.s to i80\n  %{name}.g = zext i64 %{name}.f to i80\n  \
                 %{name}.h = shl i80 %{name}.g, 64\n  %{name}.b = or i80 %{name}.h, %{name}.l\n  \
                 %{name} = bitcast i80 %{name}.b to x86_fp80\n"
            ),
        }
    }

    /// Lines that mix the bits of lane `%{e}` into the hash `acc`,
    /// defining `%{e}.z`.
    fn mix(self, e: &str, acc: &str) -> String {
        let (bits, int) = match self {
            Wide::I128 => (128, format!("  %{e}.b = add i128 %{e}, 0\n")),
            Wide::X86Fp80 => (80, format!("  %{e}.b = bitcast x86_fp80 %{e} to i80\n")),
        };
        format!(
            "{int}  %{e}.l = trunc i{bits} %{e}.b to i64\n  %{e}.s = lshr i{bits} %{e}.b, 64\n  \
             %{e}.u = trunc i{bits} %{e}.s to i64\n  %{e}.m = mul i64 {acc}, 1099511628211\n  \
             %{e}.x = xor i64 %{e}.m, %{e}.l\n  %{e}.n = mul i64 %{e}.x, 1099511628211\n  \
             %{e}.z = xor i64 %{e}.n, %{e}.u\n"
        )
    }
}

/// Lines that hash the lanes `lanes` of `<n x E>` `%{v}` into `acc`;
/// returns them and the new hash's name.
fn fold(ty: Wide, n: u32, v: &str, lanes: &[u32], acc: &str) -> (String, String) {
    let mut text = String::new();
    let mut acc = acc.to_string();
    let vt = format!("<{n} x {}>", ty.ll());
    for &i in lanes {
        let e = format!("{v}.e{i}");
        writeln!(text, "  %{e} = extractelement {vt} %{v}, i32 {i}").unwrap();
        text.push_str(&ty.mix(&e, &acc));
        acc = format!("%{e}.z");
    }
    (text, acc)
}

/// Every lane of `n`.
fn all(n: u32) -> Vec<u32> {
    (0..n).collect()
}

/// Lines that build a `<n x E>` `%{v}` lane by lane from `%a0`, `%a1`
/// (each lane a different value), starting from `poison`.
fn chain(ty: Wide, n: u32, v: &str) -> String {
    let vt = format!("<{n} x {}>", ty.ll());
    let mut text = String::new();
    let mut prev = "poison".to_string();
    for i in 0..n {
        writeln!(
            text,
            "  %{v}.x{i} = add i64 %a0, {}\n  %{v}.y{i} = xor i64 %a1, {}",
            i64::from(i) * 0x0101_0101,
            i64::from(i) * 0x0a0b
        )
        .unwrap();
        text.push_str(&ty.lane(&format!("{v}.k{i}"), &format!("%{v}.x{i}"), &format!("%{v}.y{i}")));
        let next = if i + 1 == n { format!("%{v}") } else { format!("%{v}.c{i}") };
        writeln!(text, "  {next} = insertelement {vt} {prev}, {} %{v}.k{i}, i32 {i}", ty.ll())
            .unwrap();
        prev = next;
    }
    text
}

/// The wide-lane functions of `ty` (`spec/codegen.md` §5.6.18 (1)).
fn wide_lane_functions(suite: &mut Suite, rng: &mut Rng, ty: Wide) {
    let e = ty.ll();
    let seed = "-3750763034362895579";
    // The interpreter has no `x86_fp80`: `llc` alone is the oracle.
    let by = if ty == Wide::I128 { Check::Interp } else { Check::Llc };
    for &n in ty.lengths() {
        let vt = format!("<{n} x {e}>");
        let cst = ty.vec_const(n);
        let name = format!("{e}x{n}");
        let mut funcs = Vec::new();
        // A constant index into a constant vector.
        for k in 0..n {
            let mut body = ty.lane("x", "%a0", "%a1");
            writeln!(body, "  %v = insertelement {vt} {cst}, {e} %x, i32 {k}").unwrap();
            let (f, h) = fold(ty, n, "v", &all(n), seed);
            body.push_str(&f);
            writeln!(body, "  %r = add i64 {h}, 0").unwrap();
            funcs.push(suite.add(&format!("insc_{name}_{k}"), &[I64, I64], I64, &body, by));
        }
        // Variable indices: insert at `a2 % n`, extract at `a3 % n`.
        {
            let mut body = ty.lane("x", "%a0", "%a1");
            write!(
                body,
                "  %i = urem i64 %a2, {n}\n  %j = urem i64 %a3, {n}\n  \
                 %v = insertelement {vt} {cst}, {e} %x, i64 %i\n  \
                 %w = extractelement {vt} %v, i64 %j\n"
            )
            .unwrap();
            let (f, h) = fold(ty, n, "v", &all(n), seed);
            body.push_str(&f);
            body.push_str(&ty.mix("w", &h));
            writeln!(body, "  %r = add i64 %w.z, %j").unwrap();
            funcs.push(suite.add(&format!("insv_{name}"), &[I64; 4], I64, &body, by));
        }
        // Lane by lane from `poison`; a variable extract of a built vector.
        {
            let mut body = chain(ty, n, "v");
            writeln!(body, "  %j = urem i64 %a2, {n}\n  %w = extractelement {vt} %v, i64 %j")
                .unwrap();
            let (f, h) = fold(ty, n, "v", &all(n), seed);
            body.push_str(&f);
            body.push_str(&ty.mix("w", &h));
            writeln!(body, "  %r = add i64 %w.z, 0").unwrap();
            funcs.push(suite.add(&format!("chain_{name}"), &[I64; 3], I64, &body, by));
        }
        // Shuffles of a built vector with a constant one: reversed, every
        // other lane of both, a broadcast, and one with an undefined lane
        // (not observed).
        let reverse: Vec<i64> = (0..n).rev().map(i64::from).collect();
        let both: Vec<i64> = (0..n.min(4)).flat_map(|i| [i64::from(i), i64::from(n + i)]).collect();
        let broadcast: Vec<i64> = vec![i64::from(n - 1); 3];
        let holes: Vec<i64> =
            (0..=n).map(|i| if i == 1 { -1 } else { i64::from(i % (2 * n)) }).collect();
        for (k, mask) in [reverse, both, broadcast, holes].into_iter().enumerate() {
            let m = mask.len() as u32;
            let text: Vec<String> = mask
                .iter()
                .map(|&l| if l < 0 { "i32 poison".into() } else { format!("i32 {l}") })
                .collect();
            let mut body = chain(ty, n, "v");
            writeln!(
                body,
                "  %s = shufflevector {vt} %v, {vt} {cst}, <{m} x i32> <{}>",
                text.join(", ")
            )
            .unwrap();
            let kept: Vec<u32> = (0..m).filter(|&i| mask[i as usize] >= 0).collect();
            let (f, h) = fold(ty, m, "s", &kept, seed);
            body.push_str(&f);
            writeln!(body, "  %r = add i64 {h}, 0").unwrap();
            funcs.push(suite.add(&format!("shuf{k}_{name}"), &[I64, I64], I64, &body, by));
        }
        // Memory: stored over zeros, the bytes read back as `i64` words
        // at every 2-byte offset (the packed layout), the vector loaded
        // back.
        {
            let store = u64::from(n) * ty.stride();
            let words = store.div_ceil(8) + 1;
            let mut body = chain(ty, n, "v");
            writeln!(body, "  %m = alloca [{words} x i64], align 16").unwrap();
            for w in 0..words {
                writeln!(
                    body,
                    "  %z{w} = getelementptr i8, ptr %m, i64 {}\n  store i64 0, ptr %z{w}, align 8",
                    w * 8
                )
                .unwrap();
            }
            writeln!(body, "  store {vt} %v, ptr %m, align 16").unwrap();
            let mut acc = seed.to_string();
            let mut off = 0;
            while off + 8 <= words * 8 {
                writeln!(
                    body,
                    "  %mp{off} = getelementptr i8, ptr %m, i64 {off}\n  \
                     %mq{off} = load i64, ptr %mp{off}, align 2\n  \
                     %mh{off} = mul i64 {acc}, 1099511628211\n  %mg{off} = xor i64 %mh{off}, %mq{off}"
                )
                .unwrap();
                acc = format!("%mg{off}");
                off += 2;
            }
            writeln!(body, "  %u = load {vt}, ptr %m, align 16").unwrap();
            let (f, h) = fold(ty, n, "u", &all(n), &acc);
            body.push_str(&f);
            writeln!(body, "  %r = add i64 {h}, 0").unwrap();
            funcs.push(suite.add(&format!("mem_{name}"), &[I64, I64], I64, &body, by));
        }
        for f in funcs {
            for _ in 0..4 {
                let args: Vec<u64> = (0..suite.funcs[f].ops.len()).map(|_| rng.next()).collect();
                suite.case(f, &args);
            }
            let edge: Vec<u64> =
                (0..suite.funcs[f].ops.len() as u64).map(|i| u64::MAX - i).collect();
            suite.case(f, &edge);
        }
    }
}

/// §5.6.18 (1): lanes of `i128` and `x86_fp80`, with constant and
/// variable indices, shuffles, and memory, agree with the interpreter
/// and with `llc` 22 and 19 (hod plain and stressed).
#[test]
fn wide_lanes_agree() {
    let mut suite = Suite::default();
    let mut rng = Rng(0x152);
    wide_lane_functions(&mut suite, &mut rng, Wide::I128);
    wide_lane_functions(&mut suite, &mut rng, Wide::X86Fp80);
    let report = check("wide-lanes", &suite, &MAJORS);
    assert!(report.interpreted > 0 || report.legs.is_empty(), "{report:?}");
}

/// Lines that hash the mask `<n x i1>` `%{m}` into `acc` lane by lane;
/// returns them and the new hash's name.
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

/// Lines that build `<n x i128>` `%x` from `%a0`, `%a1` ([`chain`]) and
/// `%y.c{n-1}` from it lane by lane: lane `i` equal (`i % 4 == 0`), its low
/// half changed by `%a2` (1), its sign bit flipped (2), both halves
/// changed by `%a2` and `%a3` (3), so that every predicate sees equal
/// lanes, lanes ordered by the low half alone and by the sign.
fn compare_operands(n: u32) -> String {
    let vt = format!("<{n} x i128>");
    let mut text = chain(Wide::I128, n, "x");
    let mut prev = "poison".to_string();
    for i in 0..n {
        let (lo, hi) = match i % 4 {
            0 => ("0".to_string(), "0".to_string()),
            1 => ("%a2".to_string(), "0".to_string()),
            2 => ("0".to_string(), "-9223372036854775808".to_string()),
            _ => ("%a2".to_string(), "%a3".to_string()),
        };
        text.push_str(&Wide::I128.lane(&format!("y.d{i}"), &lo, &hi));
        writeln!(
            text,
            "  %y.k{i} = xor i128 %x.k{i}, %y.d{i}\n  \
             %y.c{i} = insertelement {vt} {prev}, i128 %y.k{i}, i32 {i}"
        )
        .unwrap();
        prev = format!("%y.c{i}");
    }
    text
}

/// The mask functions of `i128` lanes (`spec/codegen.md` §5.6.18 (5)):
/// every predicate on built operands, against a constant, an operand
/// with itself; `zext`/`sext` of a mask to `i128` lanes and `trunc`
/// back.
fn wide_mask_functions(suite: &mut Suite, rng: &mut Rng) {
    let seed = "-3750763034362895579";
    let preds = ["eq", "ne", "ugt", "uge", "ult", "ule", "sgt", "sge", "slt", "sle"];
    for &n in Wide::I128.lengths() {
        let vt = format!("<{n} x i128>");
        let mut funcs = Vec::new();
        for p in preds {
            let mut body = compare_operands(n);
            let y = format!("%y.c{}", n - 1);
            writeln!(body, "  %c = icmp {p} {vt} %x, {y}\n  %d = icmp {p} {vt} {y}, %x").unwrap();
            let (f, h) = fold_mask(n, "c", seed);
            body.push_str(&f);
            let (g, h) = fold_mask(n, "d", &h);
            body.push_str(&g);
            writeln!(body, "  %r = add i64 {h}, 0").unwrap();
            funcs.push(suite.add(&format!("cmp_{p}_{n}"), &[I64; 4], I64, &body, Check::Interp));
        }
        {
            let cst = Wide::I128.vec_const(n);
            let mut body = chain(Wide::I128, n, "x");
            writeln!(
                body,
                "  %c = icmp slt {vt} %x, {cst}\n  %d = icmp ule {vt} %x, %x\n  \
                 %e = icmp ne {vt} %x, zeroinitializer"
            )
            .unwrap();
            let mut h = seed.to_string();
            for m in ["c", "d", "e"] {
                let (f, k) = fold_mask(n, m, &h);
                body.push_str(&f);
                h = k;
            }
            writeln!(body, "  %r = add i64 {h}, 0").unwrap();
            funcs.push(suite.add(&format!("cmpc_{n}"), &[I64, I64], I64, &body, Check::Interp));
        }
        for ext in ["zext", "sext"] {
            let mut body = format!(
                "  %t = trunc i64 %a0 to i{n}\n  %m = bitcast i{n} %t to <{n} x i1>\n  \
                 %v = {ext} <{n} x i1> %m to {vt}\n"
            );
            let (f, h) = fold(Wide::I128, n, "v", &all(n), seed);
            body.push_str(&f);
            writeln!(body, "  %r = add i64 {h}, 0").unwrap();
            funcs.push(suite.add(&format!("{ext}_{n}"), &[I64], I64, &body, Check::Interp));
        }
        {
            let mut body = chain(Wide::I128, n, "x");
            writeln!(body, "  %m = trunc {vt} %x to <{n} x i1>").unwrap();
            let (f, h) = fold_mask(n, "m", seed);
            body.push_str(&f);
            writeln!(body, "  %r = add i64 {h}, 0").unwrap();
            funcs.push(suite.add(&format!("trunc_{n}"), &[I64, I64], I64, &body, Check::Interp));
        }
        for f in funcs {
            for _ in 0..6 {
                let args: Vec<u64> = (0..suite.funcs[f].ops.len()).map(|_| rng.next()).collect();
                suite.case(f, &args);
            }
            let zero = vec![0; suite.funcs[f].ops.len()];
            suite.case(f, &zero);
        }
    }
}

/// §5.6.18 (5): compares of `i128` lanes (every predicate; equal lanes,
/// lanes that differ in the low half alone, in the sign) and the casts
/// between masks and `i128` lanes agree with the interpreter and with
/// `llc` 22 and 19 (hod plain and stressed).
#[test]
fn wide_masks_agree() {
    let mut suite = Suite::default();
    let mut rng = Rng(0x1525);
    wide_mask_functions(&mut suite, &mut rng);
    let report = check("wide-masks", &suite, &MAJORS);
    assert!(report.interpreted > 0 || report.legs.is_empty(), "{report:?}");
}

/// §5.6.18 (1): what stays refused: a wide lane of a vector of more than
/// `MAX_LANES` lanes, a cast of a mask to `x86_fp80` lanes, and a
/// conversion between `i128` and `half` lanes.
#[test]
fn wide_lane_refusals_by_name() {
    let many = hod_x64::vector::MAX_LANES + 1;
    refused(
        &format!(
            "define i128 @f(ptr %p) {{\n  %v = load <{many} x i128>, ptr %p\n  \
             %e = extractelement <{many} x i128> %v, i32 1\n  ret i128 %e\n}}\n"
        ),
        hod_x64::vector::LENGTH,
    );
    refused(
        "define void @f(<2 x i1> %c, ptr %p) {\n  %v = uitofp <2 x i1> %c to <2 x x86_fp80>\n  \
         store <2 x x86_fp80> %v, ptr %p\n  ret void\n}\n",
        hod_x64::vector::WIDE_ELEMENT,
    );
    // Arithmetic on `i128` lanes and their casts compile since WP-161
    // (§5.6.19, `tests/wide_arith.rs`); a conversion to `half` lanes stays
    // refused.
    refused(
        "define void @f(<2 x i128> %v, ptr %p) {\n  %w = sitofp <2 x i128> %v to <2 x half>\n  \
         store <2 x half> %w, ptr %p\n  ret void\n}\n",
        hod_x64::vector::WIDE_ELEMENT,
    );
}

// ---- 2. the calling convention ----

/// The callees (`@b_*`): each hashes everything it got, or builds its
/// result from its arguments.
const CALLEES: &str = "
declare void @llvm.trap()
define i64 @mix(i64 %h, i64 %x) {
  %m = mul i64 %h, 1099511628211
  %r = xor i64 %m, %x
  ret i64 %r
}
define i64 @mix128(i64 %h, i128 %x) {
  %l = trunc i128 %x to i64
  %s = lshr i128 %x, 64
  %u = trunc i128 %s to i64
  %a = call i64 @mix(i64 %h, i64 %l)
  %b = call i64 @mix(i64 %a, i64 %u)
  ret i64 %b
}
define i64 @mix80(i64 %h, x86_fp80 %x) {
  %i = bitcast x86_fp80 %x to i80
  %w = zext i80 %i to i128
  %r = call i64 @mix128(i64 %h, i128 %w)
  ret i64 %r
}

define i64 @b_v1(i64 %x, <1 x i128> %v, i64 %y) {
  %e = extractelement <1 x i128> %v, i32 0
  %a = call i64 @mix(i64 1, i64 %x)
  %b = call i64 @mix128(i64 %a, i128 %e)
  %c = call i64 @mix(i64 %b, i64 %y)
  ret i64 %c
}
define i64 @b_v1s(i64 %p0, i64 %p1, i64 %p2, i64 %p3, i64 %p4, i64 %p5, <1 x i128> %v, i64 %y) {
  %e = extractelement <1 x i128> %v, i32 0
  %a = call i64 @mix(i64 %p0, i64 %p5)
  %b = call i64 @mix128(i64 %a, i128 %e)
  %c = call i64 @mix(i64 %b, i64 %y)
  ret i64 %c
}
define i64 @b_v2(<2 x i128> %v, i64 %y) {
  %e0 = extractelement <2 x i128> %v, i32 0
  %e1 = extractelement <2 x i128> %v, i32 1
  %a = call i64 @mix128(i64 2, i128 %e0)
  %b = call i64 @mix128(i64 %a, i128 %e1)
  %c = call i64 @mix(i64 %b, i64 %y)
  ret i64 %c
}
define i64 @b_v2s(i64 %p0, i64 %p1, i64 %p2, i64 %p3, i64 %p4, i64 %p5, <2 x i128> %v, i64 %y) {
  %e0 = extractelement <2 x i128> %v, i32 0
  %e1 = extractelement <2 x i128> %v, i32 1
  %a = call i64 @mix(i64 %p1, i64 %p4)
  %b = call i64 @mix128(i64 %a, i128 %e0)
  %c = call i64 @mix128(i64 %b, i128 %e1)
  %d = call i64 @mix(i64 %c, i64 %y)
  ret i64 %d
}
define i64 @b_v3(<3 x i128> %v) {
  %e0 = extractelement <3 x i128> %v, i32 0
  %e1 = extractelement <3 x i128> %v, i32 1
  %e2 = extractelement <3 x i128> %v, i32 2
  %a = call i64 @mix128(i64 3, i128 %e0)
  %b = call i64 @mix128(i64 %a, i128 %e1)
  %c = call i64 @mix128(i64 %b, i128 %e2)
  ret i64 %c
}
define i64 @b_v3s(i64 %p0, i64 %p1, i64 %p2, i64 %p3, i64 %p4, i64 %p5, <3 x i128> %v, i64 %y) {
  %e0 = extractelement <3 x i128> %v, i32 0
  %e1 = extractelement <3 x i128> %v, i32 1
  %e2 = extractelement <3 x i128> %v, i32 2
  %a = call i64 @mix(i64 %p2, i64 %p3)
  %b = call i64 @mix128(i64 %a, i128 %e0)
  %c = call i64 @mix128(i64 %b, i128 %e1)
  %d = call i64 @mix128(i64 %c, i128 %e2)
  %f = call i64 @mix(i64 %d, i64 %y)
  ret i64 %f
}
define <1 x i128> @b_r1(i64 %x, i64 %y) {
  %l = zext i64 %x to i128
  %u = zext i64 %y to i128
  %h = shl i128 %u, 64
  %w = or i128 %h, %l
  %v = insertelement <1 x i128> poison, i128 %w, i32 0
  ret <1 x i128> %v
}
define <2 x i128> @b_r2(i64 %x, i64 %y) {
  %l = zext i64 %x to i128
  %u = zext i64 %y to i128
  %h = shl i128 %u, 64
  %w = or i128 %h, %l
  %z = xor i128 %w, -1
  %v0 = insertelement <2 x i128> poison, i128 %w, i32 0
  %v = insertelement <2 x i128> %v0, i128 %z, i32 1
  ret <2 x i128> %v
}
define i64 @b_f1(i64 %x, <1 x x86_fp80> %v, i64 %y) {
  %e = extractelement <1 x x86_fp80> %v, i32 0
  %a = call i64 @mix(i64 4, i64 %x)
  %b = call i64 @mix80(i64 %a, x86_fp80 %e)
  %c = call i64 @mix(i64 %b, i64 %y)
  ret i64 %c
}
define i64 @b_f2(i64 %x, <2 x x86_fp80> %v, x86_fp80 %s, i64 %y) {
  %e0 = extractelement <2 x x86_fp80> %v, i32 0
  %e1 = extractelement <2 x x86_fp80> %v, i32 1
  %a = call i64 @mix(i64 5, i64 %x)
  %b = call i64 @mix80(i64 %a, x86_fp80 %e0)
  %c = call i64 @mix80(i64 %b, x86_fp80 %e1)
  %d = call i64 @mix80(i64 %c, x86_fp80 %s)
  %f = call i64 @mix(i64 %d, i64 %y)
  ret i64 %f
}
define i64 @b_f3(<3 x x86_fp80> %v, i64 %y) {
  %e0 = extractelement <3 x x86_fp80> %v, i32 0
  %e1 = extractelement <3 x x86_fp80> %v, i32 1
  %e2 = extractelement <3 x x86_fp80> %v, i32 2
  %b = call i64 @mix80(i64 6, x86_fp80 %e0)
  %c = call i64 @mix80(i64 %b, x86_fp80 %e1)
  %d = call i64 @mix80(i64 %c, x86_fp80 %e2)
  %f = call i64 @mix(i64 %d, i64 %y)
  ret i64 %f
}
define <1 x x86_fp80> @b_g1(i64 %x, i64 %y) {
  %e = call x86_fp80 @lane80(i64 %x, i64 %y)
  %v = insertelement <1 x x86_fp80> poison, x86_fp80 %e, i32 0
  ret <1 x x86_fp80> %v
}
define <2 x x86_fp80> @b_g2(i64 %x, i64 %y) {
  %e = call x86_fp80 @lane80(i64 %x, i64 %y)
  %z = xor i64 %x, 255
  %f = call x86_fp80 @lane80(i64 %z, i64 %x)
  %v0 = insertelement <2 x x86_fp80> poison, x86_fp80 %e, i32 0
  %v = insertelement <2 x x86_fp80> %v0, x86_fp80 %f, i32 1
  ret <2 x x86_fp80> %v
}
define <3 x x86_fp80> @b_g3(i64 %x, i64 %y) {
  %e = call x86_fp80 @lane80(i64 %x, i64 %y)
  %z = xor i64 %x, 255
  %f = call x86_fp80 @lane80(i64 %z, i64 %x)
  %w = add i64 %y, 3
  %g = call x86_fp80 @lane80(i64 %w, i64 %z)
  %v0 = insertelement <3 x x86_fp80> poison, x86_fp80 %e, i32 0
  %v1 = insertelement <3 x x86_fp80> %v0, x86_fp80 %f, i32 1
  %v = insertelement <3 x x86_fp80> %v1, x86_fp80 %g, i32 2
  ret <3 x x86_fp80> %v
}
define x86_fp80 @lane80(i64 %x, i64 %y) {
  %s = or i64 %x, -9223372036854775808
  %e = and i64 %y, 36863
  %f = or i64 %e, 12288
  %l = zext i64 %s to i80
  %g = zext i64 %f to i80
  %h = shl i80 %g, 64
  %b = or i80 %h, %l
  %r = bitcast i80 %b to x86_fp80
  ret x86_fp80 %r
}

%A32 = type { i64, i64, i64, i64 }
%A64 = type { [8 x i64] }
%A128 = type { [3 x i64] }

define i64 @b_by32(i64 %x, ptr byval(%A32) align 32 %p, i64 %y) {
  %a = call i64 @mix(i64 7, i64 %x)
  %b = call i64 @by_words(i64 %a, ptr %p, i64 4, i64 32)
  %c = call i64 @mix(i64 %b, i64 %y)
  ret i64 %c
}
define i64 @b_by64s(i64 %p0, i64 %p1, i64 %p2, i64 %p3, i64 %p4, i64 %p5, i64 %s0,
                    ptr byval(%A64) align 64 %p, i64 %s1, ptr byval(%A32) align 32 %q, i64 %s2) {
  %a = call i64 @mix(i64 %p0, i64 %p5)
  %b = call i64 @mix(i64 %a, i64 %s0)
  %c = call i64 @by_words(i64 %b, ptr %p, i64 8, i64 64)
  %d = call i64 @mix(i64 %c, i64 %s1)
  %e = call i64 @by_words(i64 %d, ptr %q, i64 4, i64 32)
  %f = call i64 @mix(i64 %e, i64 %s2)
  ret i64 %f
}
define i64 @b_by128(ptr byval(%A128) align 128 %p, i64 %y) {
  %b = call i64 @by_words(i64 8, ptr %p, i64 3, i64 128)
  %c = call i64 @mix(i64 %b, i64 %y)
  ret i64 %c
}
define i64 @b_byaddr(ptr byval(%A64) align 64 %p) {
  %i = ptrtoint ptr %p to i64
  ret i64 %i
}
; The hash of `n` words at `p`, and whether `p` is `align`-aligned.
define i64 @by_words(i64 %h, ptr %p, i64 %n, i64 %align) {
entry:
  %i = ptrtoint ptr %p to i64
  %m = sub i64 %align, 1
  %low = and i64 %i, %m
  %ok = icmp eq i64 %low, 0
  br i1 %ok, label %loop, label %bad
bad:
  call void @llvm.trap()
  unreachable
loop:
  %k = phi i64 [ 0, %entry ], [ %k1, %loop ]
  %acc = phi i64 [ %h, %entry ], [ %acc1, %loop ]
  %q = getelementptr i64, ptr %p, i64 %k
  %w = load i64, ptr %q
  %acc1 = call i64 @mix(i64 %acc, i64 %w)
  %k1 = add i64 %k, 1
  %more = icmp ult i64 %k1, %n
  br i1 %more, label %loop, label %done
done:
  ret i64 %acc1
}
";

/// The callers (`@d_*`, called by the C driver with a seed): each builds
/// its arguments from the seed, calls a callee and hashes what came back.
const CALLERS: &str = "
declare i64 @mix(i64, i64)
declare i64 @mix128(i64, i128)
declare i64 @mix80(i64, x86_fp80)
declare x86_fp80 @lane80(i64, i64)
declare i64 @b_v1(i64, <1 x i128>, i64)
declare i64 @b_v1s(i64, i64, i64, i64, i64, i64, <1 x i128>, i64)
declare i64 @b_v2(<2 x i128>, i64)
declare i64 @b_v2s(i64, i64, i64, i64, i64, i64, <2 x i128>, i64)
declare i64 @b_v3(<3 x i128>)
declare i64 @b_v3s(i64, i64, i64, i64, i64, i64, <3 x i128>, i64)
declare <1 x i128> @b_r1(i64, i64)
declare <2 x i128> @b_r2(i64, i64)
declare i64 @b_f1(i64, <1 x x86_fp80>, i64)
declare i64 @b_f2(i64, <2 x x86_fp80>, x86_fp80, i64)
declare i64 @b_f3(<3 x x86_fp80>, i64)
declare <1 x x86_fp80> @b_g1(i64, i64)
declare <2 x x86_fp80> @b_g2(i64, i64)
declare <3 x x86_fp80> @b_g3(i64, i64)

%A32 = type { i64, i64, i64, i64 }
%A64 = type { [8 x i64] }
%A128 = type { [3 x i64] }
declare i64 @b_by32(i64, ptr byval(%A32) align 32, i64)
declare i64 @b_by64s(i64, i64, i64, i64, i64, i64, i64, ptr byval(%A64) align 64, i64,
                     ptr byval(%A32) align 32, i64)
declare i64 @b_by128(ptr byval(%A128) align 128, i64)
declare i64 @b_byaddr(ptr byval(%A64) align 64)

define <1 x i128> @vec1(i64 %s) {
  %a = mul i64 %s, 3
  %b = xor i64 %s, 81985529216486895
  %l = zext i64 %a to i128
  %u = zext i64 %b to i128
  %h = shl i128 %u, 64
  %w = or i128 %h, %l
  %v = insertelement <1 x i128> poison, i128 %w, i32 0
  ret <1 x i128> %v
}

define i64 @d_v1(i64 %s) {
  %v = call <1 x i128> @vec1(i64 %s)
  %x = add i64 %s, 11
  %r = call i64 @b_v1(i64 %s, <1 x i128> %v, i64 %x)
  ret i64 %r
}
define i64 @d_v1s(i64 %s) {
  %v = call <1 x i128> @vec1(i64 %s)
  %x = add i64 %s, 12
  %r = call i64 @b_v1s(i64 %s, i64 1, i64 2, i64 3, i64 4, i64 %x, <1 x i128> %v, i64 99)
  ret i64 %r
}
define i64 @d_v2(i64 %s) {
  %v = call <1 x i128> @vec1(i64 %s)
  %e = extractelement <1 x i128> %v, i32 0
  %f = mul i128 %e, 1000003
  %w0 = insertelement <2 x i128> poison, i128 %e, i32 0
  %w = insertelement <2 x i128> %w0, i128 %f, i32 1
  %r = call i64 @b_v2(<2 x i128> %w, i64 %s)
  %t = call i64 @b_v2s(i64 1, i64 %s, i64 3, i64 4, i64 77, i64 6, <2 x i128> %w, i64 -5)
  %o = xor i64 %r, %t
  ret i64 %o
}
define i64 @d_v3(i64 %s) {
  %v = call <1 x i128> @vec1(i64 %s)
  %e = extractelement <1 x i128> %v, i32 0
  %f = mul i128 %e, 1000003
  %g = sub i128 0, %e
  %w0 = insertelement <3 x i128> poison, i128 %e, i32 0
  %w1 = insertelement <3 x i128> %w0, i128 %f, i32 1
  %w = insertelement <3 x i128> %w1, i128 %g, i32 2
  %r = call i64 @b_v3(<3 x i128> %w)
  %t = call i64 @b_v3s(i64 1, i64 2, i64 %s, i64 4, i64 5, i64 6, <3 x i128> %w, i64 -6)
  %o = xor i64 %r, %t
  ret i64 %o
}
define i64 @d_r1(i64 %s) {
  %x = add i64 %s, 13
  %v = call <1 x i128> @b_r1(i64 %s, i64 %x)
  %e = extractelement <1 x i128> %v, i32 0
  %r = call i64 @mix128(i64 9, i128 %e)
  ret i64 %r
}
define i64 @d_r2(i64 %s) {
  %x = add i64 %s, 14
  %v = call <2 x i128> @b_r2(i64 %s, i64 %x)
  %e0 = extractelement <2 x i128> %v, i32 0
  %e1 = extractelement <2 x i128> %v, i32 1
  %a = call i64 @mix128(i64 10, i128 %e0)
  %r = call i64 @mix128(i64 %a, i128 %e1)
  ret i64 %r
}
define i64 @d_f(i64 %s) {
  %x = add i64 %s, 15
  %e = call x86_fp80 @lane80(i64 %s, i64 %x)
  %y = mul i64 %s, 7
  %f = call x86_fp80 @lane80(i64 %y, i64 %s)
  %g = call x86_fp80 @lane80(i64 %x, i64 %y)
  %v1 = insertelement <1 x x86_fp80> poison, x86_fp80 %e, i32 0
  %a = call i64 @b_f1(i64 %s, <1 x x86_fp80> %v1, i64 %x)
  %v20 = insertelement <2 x x86_fp80> poison, x86_fp80 %e, i32 0
  %v2 = insertelement <2 x x86_fp80> %v20, x86_fp80 %f, i32 1
  %b = call i64 @b_f2(i64 %a, <2 x x86_fp80> %v2, x86_fp80 %g, i64 %y)
  %v30 = insertelement <3 x x86_fp80> poison, x86_fp80 %e, i32 0
  %v31 = insertelement <3 x x86_fp80> %v30, x86_fp80 %g, i32 1
  %v3 = insertelement <3 x x86_fp80> %v31, x86_fp80 %f, i32 2
  %c = call i64 @b_f3(<3 x x86_fp80> %v3, i64 %b)
  ret i64 %c
}
define i64 @d_g(i64 %s) {
  %x = add i64 %s, 16
  %v1 = call <1 x x86_fp80> @b_g1(i64 %s, i64 %x)
  %e = extractelement <1 x x86_fp80> %v1, i32 0
  %a = call i64 @mix80(i64 11, x86_fp80 %e)
  %v2 = call <2 x x86_fp80> @b_g2(i64 %x, i64 %s)
  %f0 = extractelement <2 x x86_fp80> %v2, i32 0
  %f1 = extractelement <2 x x86_fp80> %v2, i32 1
  %b = call i64 @mix80(i64 %a, x86_fp80 %f0)
  %c = call i64 @mix80(i64 %b, x86_fp80 %f1)
  %v3 = call <3 x x86_fp80> @b_g3(i64 %s, i64 %c)
  %g0 = extractelement <3 x x86_fp80> %v3, i32 0
  %g1 = extractelement <3 x x86_fp80> %v3, i32 1
  %g2 = extractelement <3 x x86_fp80> %v3, i32 2
  %d = call i64 @mix80(i64 %c, x86_fp80 %g0)
  %f = call i64 @mix80(i64 %d, x86_fp80 %g1)
  %g = call i64 @mix80(i64 %f, x86_fp80 %g2)
  ret i64 %g
}

; Fills `n` words at `p` from the seed.
define void @fill(ptr %p, i64 %n, i64 %s) {
entry:
  br label %loop
loop:
  %k = phi i64 [ 0, %entry ], [ %k1, %loop ]
  %q = getelementptr i64, ptr %p, i64 %k
  %m = mul i64 %k, 6364136223846793005
  %w = xor i64 %m, %s
  store i64 %w, ptr %q
  %k1 = add i64 %k, 1
  %more = icmp ult i64 %k1, %n
  br i1 %more, label %loop, label %done
done:
  ret void
}
define i64 @d_by(i64 %s) {
  %a = alloca %A32, align 32
  %b = alloca %A64, align 64
  %c = alloca %A128, align 128
  call void @fill(ptr %a, i64 4, i64 %s)
  %t = add i64 %s, 1
  call void @fill(ptr %b, i64 8, i64 %t)
  %u = add i64 %s, 2
  call void @fill(ptr %c, i64 3, i64 %u)
  %r0 = call i64 @b_by32(i64 %s, ptr byval(%A32) align 32 %a, i64 %t)
  %r1 = call i64 @b_by64s(i64 1, i64 2, i64 3, i64 4, i64 5, i64 %s, i64 %t,
                          ptr byval(%A64) align 64 %b, i64 %u, ptr byval(%A32) align 32 %a, i64 -7)
  %r2 = call i64 @b_by128(ptr byval(%A128) align 128 %c, i64 %u)
  %x = call i64 @mix(i64 %r0, i64 %r1)
  %y = call i64 @mix(i64 %x, i64 %r2)
  ret i64 %y
}
; The byval copy is at the same address every time round a loop: the
; realignment does not move the stack.
define i64 @d_byloop(i64 %s) {
entry:
  %b = alloca %A64, align 64
  call void @fill(ptr %b, i64 8, i64 %s)
  %first = call i64 @b_byaddr(ptr byval(%A64) align 64 %b)
  br label %loop
loop:
  %k = phi i64 [ 0, %entry ], [ %k1, %loop ]
  %acc = phi i64 [ 0, %entry ], [ %acc1, %loop ]
  %at = call i64 @b_byaddr(ptr byval(%A64) align 64 %b)
  %d = xor i64 %at, %first
  %acc1 = or i64 %acc, %d
  %k1 = add i64 %k, 1
  %more = icmp ult i64 %k1, 50
  br i1 %more, label %loop, label %done
done:
  %low = and i64 %first, 63
  %r = or i64 %acc1, %low
  ret i64 %r
}
";

/// The C driver: every caller with a few seeds.
const DRIVER: &str = r#"
#include <stdio.h>
#include <stdint.h>
typedef int64_t (*fn)(int64_t);
int64_t d_v1(int64_t), d_v1s(int64_t), d_v2(int64_t), d_v3(int64_t), d_r1(int64_t),
    d_r2(int64_t), d_f(int64_t), d_g(int64_t), d_by(int64_t), d_byloop(int64_t);
int main(void) {
    fn fns[] = {d_v1, d_v1s, d_v2, d_v3, d_r1, d_r2, d_f, d_g, d_by, d_byloop};
    int64_t seeds[] = {0, 1, -1, 0x123456789abcdefLL, (int64_t)0x8000000000000000ULL};
    for (unsigned f = 0; f < sizeof fns / sizeof *fns; f++) {
        for (unsigned s = 0; s < sizeof seeds / sizeof *seeds; s++)
            printf("%llx ", (unsigned long long)fns[f](seeds[s]));
        printf("\n");
        fflush(stdout);
    }
    return 0;
}
"#;

/// An object of one side: who compiled it, and where it is.
type Side = (String, PathBuf);

/// hod's object of `text` (header added) with `stress`.
fn hod_object(scratch: &Scratch, name: &str, text: &str, stress: Stress) -> PathBuf {
    let (module, bodies) = parse(&format!("{}{text}", ll_header()));
    let object = hod_x64::compile_to_object(&module, &bodies, &pic(stress))
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    scratch.write(&format!("{name}.o"), object)
}

/// The objects of one side: hod plain, hod stressed, `llc` 22 and 19.
fn sides(scratch: &Scratch, tag: &str, text: &str) -> Vec<Side> {
    let mut out = vec![
        ("hod".to_string(), hod_object(scratch, &format!("{tag}-hod"), text, Stress::default())),
        (
            "hod-stress".to_string(),
            hod_object(scratch, &format!("{tag}-stress"), text, Stress::ALL),
        ),
    ];
    for major in MAJORS {
        let full = format!("{}{text}", ll_header());
        if let Some(o) = float_gen::llc(scratch, major, &format!("{tag}-llc{major}"), &full) {
            out.push((format!("llc{major}"), o));
        }
    }
    out
}

/// §4.3, §6.2 and §5.6.18 (2), (3): vectors of `i128` and `x86_fp80` as
/// arguments (in registers, on the stack) and results (registers, `st0`
/// and `st1`, memory), and `byval` arguments aligned to 32, 64 and 128
/// (alone, after stack arguments, in a loop): every pairing of the
/// callers with the callees, each from hod plain, hod stressed, `llc` 22
/// and `llc` 19, linked by every linker, prints what `llc` 22 alone
/// prints, and the callees saw aligned copies.
#[test]
fn wide_vectors_and_aligned_byval_interoperate() {
    let scratch = Scratch::new("x64-wp152-abi");
    let users = sides(&scratch, "callers", CALLERS);
    let callees = sides(&scratch, "callees", CALLEES);
    let hod_bytes = text_size(&users[0].1) + text_size(&callees[0].1);
    if let (Some(c), Some(d)) =
        (users.iter().find(|s| s.0 == "llc22"), callees.iter().find(|s| s.0 == "llc22"))
    {
        let llc_bytes = text_size(&c.1) + text_size(&d.1);
        eprintln!(
            "wp152 abi: .text hod {hod_bytes}, llc 22 {llc_bytes} (hod/llc {:.2})",
            hod_bytes as f64 / llc_bytes.max(1) as f64
        );
    }
    let Some(toolchain) = linking::Toolchain::x86_64(&scratch) else { return };
    let driver = toolchain.compile_c(&scratch, "abi-driver", DRIVER, &[]);
    // `llc` 22 alone first (hod alone without it): the reference.
    let mut pairs: Vec<(&Side, &Side)> =
        users.iter().flat_map(|a| callees.iter().map(move |b| (a, b))).collect();
    pairs.sort_by_key(|(a, b)| !(a.0 == "llc22" && b.0 == "llc22"));
    let mut expected: Option<String> = None;
    let mut runs = 0;
    for ((a, caller), (b, callee)) in pairs {
        for linker in &toolchain.linkers {
            let program = scratch.join(format!("abi-{a}-{b}-{}", linker.name));
            let ran = toolchain.link_and_run(linker, &[], &[&driver, caller, callee], &program);
            assert!(
                ran.success(),
                "{a} + {b} [{}]: {:?}\n{}{}",
                linker.name,
                ran.status,
                ran.stdout_text(),
                ran.stderr_text()
            );
            let out = ran.stdout_text();
            if let Some(want) = &expected {
                assert_eq!(&out, want, "callers {a}, callees {b} [{}]", linker.name);
            } else {
                // `d_byloop`: the copy at one 64-aligned address each time.
                let last = out.lines().last().unwrap_or_default();
                assert!(last.split(' ').all(|w| w.is_empty() || w == "0"), "{last}");
                eprintln!("wp152 abi: the reference is {a} + {b}");
                expected = Some(out);
            }
            runs += 1;
        }
    }
    eprintln!("wp152 abi: {} callers x {} callees, {runs} programs", users.len(), callees.len());
}

/// The refusals that stay (`spec/abi-x64.md` §4.3, §6.2): a vector of
/// `i128` whose pieces would straddle the last register and the stack, or
/// start the stack at an offset not a multiple of 16 (LLVM 19 and 22
/// disagree there), and a `byval` alignment above 2^30 or not a power
/// of two.
#[test]
fn wide_vector_and_byval_refusals_by_name() {
    let split = hod_x64::abi::I128_VECTOR_SPLIT;
    refused(
        "define i64 @f(i64 %a, i64 %b, i64 %c, i64 %d, i64 %e, <1 x i128> %v) {\n  ret i64 %a\n}\n",
        split,
    );
    refused("define i64 @f(i64 %a, i64 %b, i64 %c, <2 x i128> %v) {\n  ret i64 %a\n}\n", split);
    refused(
        "define i64 @f(i64 %a, i64 %b, i64 %c, i64 %d, i64 %e, i64 %g, i64 %s, <2 x i128> %v) {\n  \
         ret i64 %a\n}\n",
        split,
    );
    // The same through a call: the caller's side.
    refused(
        "declare void @g(i64, i64, i64, i64, i64, <1 x i128>)\n\
         define void @f(<1 x i128> %v) {\n  call void @g(i64 1, i64 2, i64 3, i64 4, i64 5, \
         <1 x i128> %v)\n  ret void\n}\n",
        split,
    );
    // Whole on the stack from offset 0, or all in registers: accepted.
    assert_eq!(
        compile(
            "define i64 @f(i64 %a, i64 %b, i64 %c, i64 %d, i64 %e, i64 %g, <2 x i128> %v) {\n  \
             ret i64 %a\n}\n"
        ),
        Ok(())
    );
    assert_eq!(
        compile("define i64 @f(i64 %a, i64 %b, <2 x i128> %v) {\n  ret i64 %a\n}\n"),
        Ok(())
    );
    let byval = "abi:byval-align";
    refused("define i64 @f(ptr byval([4 x i64]) align 2147483648 %p) {\n  ret i64 0\n}\n", byval);
    assert_eq!(
        compile("define i64 @f(ptr byval([4 x i64]) align 1073741824 %p) {\n  ret i64 0\n}\n"),
        Ok(())
    );
}

// ---- values live across `llvm.eh.sjlj.setjmp` (§5.6.18 (4)) ----

/// The setter: register values of the entry block live across the
/// `setjmp` (an instruction result `%x`, a floating-point `%f`, the
/// argument `%b`), read after both returns; after the first, sixteen
/// values live at once force spills, which would land in the slots the
/// reloads of `%x`, `%f` and `%b` released if they were reused.
const SJLJ_SETTER: &str = "
declare i32 @llvm.eh.sjlj.setjmp(ptr)
declare ptr @llvm.frameaddress.p0(i32)
declare ptr @llvm.stacksave.p0()
declare void @g_jump(ptr, i64)

define i64 @live(ptr %buf, i64 %a, i64 %b) {
entry:
  %x = mul i64 %a, 7
  %f = sitofp i64 %a to double
  %fa = call ptr @llvm.frameaddress.p0(i32 0)
  store ptr %fa, ptr %buf, align 8
  %ss = call ptr @llvm.stacksave.p0()
  %p2 = getelementptr ptr, ptr %buf, i64 2
  store ptr %ss, ptr %p2, align 8
  %r = call i32 @llvm.eh.sjlj.setjmp(ptr %buf)
  %s0 = add i64 %x, %b
  %fi = fptosi double %f to i64
  %s1 = add i64 %s0, %fi
  %rz = zext i32 %r to i64
  %u = add i64 %s1, %rz
  %z = icmp eq i32 %r, 0
  br i1 %z, label %first, label %second
first:
SPILLS  call void @g_jump(ptr %buf, i64 %t16)
  unreachable
second:
  %p3 = getelementptr ptr, ptr %buf, i64 3
  %w = load i64, ptr %p3, align 8
  %ret = add i64 %u, %w
  ret i64 %ret
}
";

/// G: stores its value in word 3 of the buffer (unused by the layout)
/// and jumps.
const SJLJ_JUMPER: &str = "
declare void @llvm.eh.sjlj.longjmp(ptr)

define void @g_jump(ptr %buf, i64 %v) {
  %p3 = getelementptr ptr, ptr %buf, i64 3
  store i64 %v, ptr %p3, align 8
  call void @llvm.eh.sjlj.longjmp(ptr %buf)
  unreachable
}
";

/// The caller of the setter, with the buffer in its frame.
const SJLJ_CALLER: &str = "
declare i64 @live(ptr, i64, i64)

define i64 @call_live(i64 %a, i64 %b) {
  %buf = alloca [5 x ptr], align 16
  %r = call i64 @live(ptr %buf, i64 %a, i64 %b)
  ret i64 %r
}
";

/// The arguments the driver passes.
const SJLJ_ARGS: [(i64, i64); 4] = [(3, 10), (-5, -77), (1 << 40, 9), (12_345, 0)];

/// [`SJLJ_SETTER`] with its sixteen products of `%u` and their sum `%t16`.
fn sjlj_setter() -> String {
    let mut spills = String::new();
    for i in 1..=16 {
        let _ = writeln!(spills, "  %v{i} = mul i64 %u, {i}");
    }
    let _ = writeln!(spills, "  %t1 = add i64 %v1, 0");
    for i in 2..=16 {
        let _ = writeln!(spills, "  %t{i} = add i64 %t{}, %v{i}", i - 1);
    }
    SJLJ_SETTER.replace("SPILLS", &spills)
}

/// What `live(a, b)` returns: `u = 8a + b` at the first return, the sum
/// of `u · i` through G, `u + 1` plus that at the second.
fn sjlj_expected() -> String {
    let mut out = String::new();
    for (a, b) in SJLJ_ARGS {
        let u = a.wrapping_mul(8).wrapping_add(b);
        let w = u.wrapping_mul(136);
        let _ = writeln!(out, "{:x}", u.wrapping_add(1).wrapping_add(w) as u64);
    }
    out
}

/// §5.6.18 (4): values live across `llvm.eh.sjlj.setjmp` are read from
/// memory after both returns, and no slot they are in is reused: every
/// triple of setter, G and caller, each from hod plain, hod stressed,
/// `llc` 22 and 19, linked by every linker, returns the expected values.
#[test]
fn sjlj_live_values_survive_the_second_return() {
    let scratch = Scratch::new("x64-wp152-sjlj");
    let setters = sides(&scratch, "set", &sjlj_setter());
    let jumpers = sides(&scratch, "g", SJLJ_JUMPER);
    let callers = sides(&scratch, "caller", SJLJ_CALLER);
    let Some(toolchain) = linking::Toolchain::x86_64(&scratch) else { return };
    let mut driver = String::from(
        "#include <stdio.h>\n#include <stdint.h>\nuint64_t call_live(uint64_t, uint64_t);\n\
         int main(void) {\n",
    );
    for (a, b) in SJLJ_ARGS {
        let _ = writeln!(
            driver,
            "  printf(\"%llx\\n\", (unsigned long long)call_live((uint64_t){a}LL, (uint64_t){b}LL));"
        );
    }
    driver.push_str("  return 0;\n}\n");
    let driver = toolchain.compile_c(&scratch, "sjlj-driver", &driver, &[]);
    let expected = sjlj_expected();
    let mut runs = 0;
    for (s, set) in &setters {
        for (g, jump) in &jumpers {
            for (c, caller) in &callers {
                for linker in &toolchain.linkers {
                    let program = scratch.join(format!("sjlj-{s}-{g}-{c}-{}", linker.name));
                    let ran = toolchain.link_and_run(
                        linker,
                        &[],
                        &[&driver, set, jump, caller],
                        &program,
                    );
                    assert!(
                        ran.success(),
                        "{s} + {g} + {c} [{}]: {:?}\n{}",
                        linker.name,
                        ran.status,
                        ran.stderr_text()
                    );
                    assert_eq!(
                        ran.stdout_text(),
                        expected,
                        "setter {s}, G {g}, caller {c} [{}]",
                        linker.name
                    );
                    runs += 1;
                }
            }
        }
    }
    eprintln!("wp152 sjlj: {runs} programs");
}
