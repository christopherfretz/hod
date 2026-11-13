//! The x86-64 masks (`<N x i1>`, WP-132, `spec/codegen.md` §5.6.13):
//! every compare predicate on every lane type (NaN, signed zeros,
//! infinities, `half` and `bfloat` without conversions), the bit rules of
//! element-wise operations, `select`, the casts, element access,
//! shuffles, stores of every byte count and the reductions, checked
//! against `hod_interp` and `llc` under the emulator.

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

#[path = "common/aggr_gen.rs"]
mod aggr_gen;

#[path = "common/vector_gen.rs"]
mod vector_gen;

use std::fmt::Write as _;

use aggr_gen::{header, parse, pic};
use hod_cg::{CgError, Stress};
use vector_gen::{Tally, assert_agrees, check_functions};

/// Floating-point lane types: IR name, bits, interesting bit patterns
/// (zeros, ones, infinities, NaNs, a subnormal, ordinary values).
const FLOATS: [(&str, u32, [u64; 10]); 4] = [
    ("half", 16, [0x0000, 0x8000, 0x3c00, 0xbc00, 0x7c00, 0xfc00, 0x7e00, 0x0001, 0x4500, 0xc100]),
    (
        "bfloat",
        16,
        [0x0000, 0x8000, 0x3f80, 0xbf80, 0x7f80, 0xff80, 0x7fc0, 0x0001, 0x4120, 0xc020],
    ),
    (
        "float",
        32,
        [
            0,
            0x8000_0000,
            0x3f80_0000,
            0xbf80_0000,
            0x7f80_0000,
            0xff80_0000,
            0x7fc0_0000,
            1,
            0x4120_0000,
            0xc020_0000,
        ],
    ),
    (
        "double",
        64,
        [
            0,
            0x8000_0000_0000_0000,
            0x3ff0_0000_0000_0000,
            0xbff0_0000_0000_0000,
            0x7ff0_0000_0000_0000,
            0xfff0_0000_0000_0000,
            0x7ff8_0000_0000_0000,
            1,
            0x4024_0000_0000_0000,
            0xc004_0000_0000_0000,
        ],
    ),
];

const FCMP: [&str; 16] = [
    "false", "oeq", "ogt", "oge", "olt", "ole", "one", "ord", "uno", "ueq", "ugt", "uge", "ult",
    "ule", "une", "true",
];

const ICMP: [&str; 10] = ["eq", "ne", "ugt", "uge", "ult", "ule", "sgt", "sge", "slt", "sle"];

/// A vector constant of `n` lanes of the `bits`-bit integer type from
/// `lane(i)` (bit patterns).
fn ints(bits: u32, n: u32, lane: impl Fn(u32) -> u64) -> String {
    let list: Vec<String> = (0..n)
        .map(|i| {
            let v = lane(i);
            let shift = 64 - bits;
            format!("i{bits} {}", ((v << shift) as i64) >> shift)
        })
        .collect();
    format!("<{n} x i{bits}> <{}>", list.join(", "))
}

/// The functions of a suite, numbered from `k`.
struct Fns {
    k: u64,
    out: Vec<(u64, Option<String>)>,
}

impl Fns {
    /// A function that loads `a` and `b` (integer lane images of
    /// `<n x T>`, bitcast to `t`), runs `body` (defining `%r`, a mask of
    /// `rn` lanes) and emits `%r` zero-extended to bytes, then stored as a
    /// mask (its image).
    fn mask_result(
        &mut self,
        t: &str,
        ints_a: &str,
        ints_b: &str,
        int_t: &str,
        rn: u32,
        body: &str,
    ) {
        self.k += 1;
        let k = self.k;
        let text = format!(
            "@t{k}.a = global {ints_a}, align 64\n@t{k}.b = global {ints_b}, align 64\n\
             @t{k}.o = global [{rn} x i8] zeroinitializer, align 64\n\
             @t{k}.m = global [8 x i8] zeroinitializer, align 64\n\
             define void @t{k}() {{\nentry:\n  %ai = load {int_t}, ptr @t{k}.a\n  \
             %a = bitcast {int_t} %ai to {t}\n  %bi = load {int_t}, ptr @t{k}.b\n  \
             %b = bitcast {int_t} %bi to {t}\n{body}  \
             %z = zext <{rn} x i1> %r to <{rn} x i8>\n  store <{rn} x i8> %z, ptr @t{k}.o\n  \
             call void @emit(ptr @t{k}.o, i64 {rn})\n  store <{rn} x i1> %r, ptr @t{k}.m\n  \
             call void @emit(ptr @t{k}.m, i64 8)\n  ret void\n}}\n\n"
        );
        self.out.push((k, Some(text)));
    }

    /// A function of its own text, `@t<k>` with `{k}` substituted.
    fn raw(&mut self, text: &str) {
        self.k += 1;
        self.out.push((self.k, Some(text.replace("{k}", &self.k.to_string()))));
    }
}

/// Runs `fns`, prints the tally, requires agreement and that nothing was
/// refused.
fn run(name: &str, fns: Fns) {
    let mut tally = Tally::default();
    check_functions(name, fns.out, &mut tally);
    eprintln!(
        "x64 masks {name}: {} generated, {} compared on {:?}, {} undefined {:?}, {} poison \
         bytes, refused {:?}, llc disagreements {:?}, against llc {:?}",
        tally.generated,
        tally.run,
        tally.legs,
        tally.trapped,
        tally.traps,
        tally.poison_bytes,
        tally.refused,
        tally.llc_wrong,
        tally.by_llc
    );
    assert_agrees(&tally);
    assert!(tally.refused.is_empty(), "{:?}", tally.refused);
    assert!(tally.run * 10 >= tally.generated * 9, "too few compared: {tally:?}");
}

/// Every `fcmp` predicate on every floating-point lane type, both in a
/// register vector and in memory, on every pair of interesting values.
#[test]
fn every_fcmp_predicate_agrees() {
    let mut fns = Fns { k: 0, out: Vec::new() };
    for (name, bits, values) in FLOATS {
        // Every ordered pair of the ten values: 100 lanes in chunks.
        let pairs: Vec<(u64, u64)> =
            values.iter().flat_map(|&a| values.iter().map(move |&b| (a, b))).collect();
        let n = 128 / bits;
        for (c, chunk) in pairs.chunks(n as usize).enumerate() {
            let len = chunk.len() as u32;
            // A register vector when the chunk fills one, else in memory
            // (an odd count).
            let n = if len == n && c % 2 == 0 { len } else { len.min(n - 1).max(1) };
            let chunk = &chunk[..n as usize];
            let ia = ints(bits, n, |i| chunk[i as usize].0);
            let ib = ints(bits, n, |i| chunk[i as usize].1);
            let int_t = format!("<{n} x i{bits}>");
            let t = format!("<{n} x {name}>");
            for pred in FCMP {
                fns.mask_result(
                    &t,
                    &ia,
                    &ib,
                    &int_t,
                    n,
                    &format!("  %r = fcmp {pred} {t} %a, %b\n"),
                );
            }
        }
    }
    run("fcmp", fns);
}

/// Every `icmp` predicate on every integer lane width and on masks.
#[test]
fn every_icmp_predicate_agrees() {
    let mut fns = Fns { k: 0, out: Vec::new() };
    let edges = |bits: u32| -> Vec<u64> {
        let top = 1u64 << (bits - 1);
        let all = if bits == 64 { u64::MAX } else { (1 << bits) - 1 };
        vec![0, 1, top - 1, top, all, 2, top + 1, all - 1]
    };
    for bits in [8, 16, 32, 64] {
        let e = edges(bits);
        let pairs: Vec<(u64, u64)> =
            e.iter().flat_map(|&a| e.iter().map(move |&b| (a, b))).collect();
        for n in [128 / bits, 3, 5] {
            for (c, chunk) in pairs.chunks(n as usize).enumerate().take(6) {
                let n = chunk.len() as u32;
                let ia = ints(bits, n, |i| chunk[i as usize].0);
                let ib = ints(bits, n, |i| chunk[i as usize].1);
                let t = format!("<{n} x i{bits}>");
                for pred in ICMP {
                    if c % 2 == 0 || pred.starts_with('s') {
                        fns.mask_result(
                            &t,
                            &ia,
                            &ib,
                            &t,
                            n,
                            &format!("  %r = icmp {pred} {t} %a, %b\n"),
                        );
                    }
                }
            }
        }
    }
    // Masks: compares of masks made by compares.
    for pred in ICMP {
        let t = "<8 x i8>";
        let ia = ints(8, 8, |i| [0, 1, 0, 1, 5, 0, 7, 0][i as usize]);
        let ib = ints(8, 8, |i| [0, 0, 1, 1, 9, 0, 0, 3][i as usize]);
        fns.mask_result(
            t,
            &ia,
            &ib,
            t,
            8,
            &format!(
                "  %ma = icmp ne {t} %a, zeroinitializer\n  %mb = icmp ne {t} %b, zeroinitializer\n  \
                 %r = icmp {pred} <8 x i1> %ma, %mb\n"
            ),
        );
    }
    run("icmp", fns);
}

/// The element-wise bit rules, `select` of masks and of lanes, and
/// constant mask operands.
#[test]
#[allow(clippy::too_many_lines, reason = "one table of cases")]
fn bit_rules_and_selects_agree() {
    let mut fns = Fns { k: 0, out: Vec::new() };
    // `sdiv` and `srem` of `i1` lanes overflow whenever the dividend is
    // true (-1 / -1): undefined, not tested.
    let ops = ["add", "sub", "mul", "and", "or", "xor", "shl", "lshr", "ashr", "udiv", "urem"];
    for n in [4, 13, 64] {
        let t = format!("<{n} x i8>");
        let ia = ints(8, n, |i| u64::from(i % 3 == 0));
        let ib = ints(8, n, |i| u64::from(i % 2 == 0) * u64::from(i + 1));
        let masks = format!(
            "  %ma = icmp ne {t} %a, zeroinitializer\n  %mb = icmp ne {t} %b, zeroinitializer\n"
        );
        let m = format!("<{n} x i1>");
        for op in ops {
            if matches!(op, "udiv" | "urem") {
                // A division by a zero lane is undefined: divide by ones.
                fns.mask_result(
                    &t,
                    &ia,
                    &ib,
                    &t,
                    n,
                    &format!("{masks}  %r = {op} {m} %ma, splat (i1 true)\n"),
                );
                continue;
            }
            if matches!(op, "shl" | "lshr" | "ashr") {
                fns.mask_result(
                    &t,
                    &ia,
                    &ib,
                    &t,
                    n,
                    &format!("{masks}  %r = {op} {m} %ma, zeroinitializer\n"),
                );
                continue;
            }
            fns.mask_result(&t, &ia, &ib, &t, n, &format!("{masks}  %r = {op} {m} %ma, %mb\n"));
            fns.mask_result(&t, &ia, &ib, &t, n, &format!("{masks}  %r = {op} {m} %ma, %ma\n"));
        }
        // Constant operands.
        let aggregate: Vec<&str> =
            (0..n).map(|i| if i % 5 == 1 { "i1 1" } else { "i1 0" }).collect();
        fns.mask_result(
            &t,
            &ia,
            &ib,
            &t,
            n,
            &format!("{masks}  %r = xor {m} %ma, <{}>\n", aggregate.join(", ")),
        );
        fns.mask_result(
            &t,
            &ia,
            &ib,
            &t,
            n,
            &format!("{masks}  %r = and {m} splat (i1 true), %mb\n"),
        );
        // `select` of masks, of `%ma` twice, and on lanes.
        fns.mask_result(
            &t,
            &ia,
            &ib,
            &t,
            n,
            &format!("{masks}  %x = icmp ugt {t} %b, %a\n  %r = select {m} %ma, {m} %mb, {m} %x\n"),
        );
        fns.mask_result(
            &t,
            &ia,
            &ib,
            &t,
            n,
            &format!("{masks}  %r = select {m} %ma, {m} %ma, {m} %mb\n"),
        );
        fns.mask_result(&t, &ia, &ib, &t, n, &format!(
            "{masks}  %s = select {m} %ma, {t} %a, {t} %b\n  %r = icmp ne {t} %s, zeroinitializer\n"));
    }
    // `select` on lanes of every width, the values emitted.
    for (e, bits) in [
        ("i8", 8),
        ("i16", 16),
        ("i32", 32),
        ("i64", 64),
        ("float", 32),
        ("double", 64),
        ("ptr", 64),
    ] {
        for n in [2, 3, 4, 16] {
            fns.raw(&format!(
                "@t{{k}}.a = global <{n} x {e}> zeroinitializer, align 64\n\
                 @t{{k}}.b = global {}, align 64\n\
                 @t{{k}}.c = global {}, align 64\n\
                 @t{{k}}.o = global [{} x i8] zeroinitializer, align 64\n\
                 define void @t{{k}}() {{\nentry:\n  %c = load <{n} x i8>, ptr @t{{k}}.c\n  \
                 %m = trunc <{n} x i8> %c to <{n} x i1>\n  %bi = load <{n} x i{bits}>, ptr @t{{k}}.b\n  \
                 %b = bitcast <{n} x i{bits}> %bi to <{n} x {e}>\n  \
                 %a = load <{n} x {e}>, ptr @t{{k}}.a\n  \
                 %r = select <{n} x i1> %m, <{n} x {e}> %b, <{n} x {e}> %a\n  \
                 store <{n} x {e}> %r, ptr @t{{k}}.o\n  call void @emit(ptr @t{{k}}.o, i64 {})\n  \
                 ret void\n}}\n\n",
                ints(bits, n, |i| 0x1111_2222_3333_4444u64.wrapping_mul(u64::from(i) + 3)),
                ints(8, n, |i| u64::from(i * 7 % 3 == 1) | 6),
                n * bits / 8,
                n * bits / 8,
            ).replace("bitcast <2 x i64> %bi to <2 x ptr>", "inttoptr <2 x i64> %bi to <2 x ptr>")
             .replace(&format!("bitcast <{n} x i64> %bi to <{n} x ptr>"), &format!("inttoptr <{n} x i64> %bi to <{n} x ptr>")));
        }
    }
    run("bits", fns);
}

/// Casts between masks and lanes; `extractelement`, `insertelement`
/// (constant and variable indices) and shuffles of masks; stores of
/// every byte count; reductions.
#[test]
#[allow(clippy::too_many_lines, reason = "one table of cases")]
fn casts_access_shuffles_stores_and_reductions_agree() {
    let mut fns = Fns { k: 0, out: Vec::new() };
    for n in [2, 3, 8, 13, 16, 33, 64] {
        let t = format!("<{n} x i8>");
        let m = format!("<{n} x i1>");
        let ia = ints(8, n, |i| u64::from(i % 3 != 1) * 3 + u64::from(i % 7 == 0));
        let ib = ints(8, n, |i| u64::from(i % 4 == 2));
        let masks = format!(
            "  %ma = icmp ne {t} %a, zeroinitializer\n  %mb = icmp ne {t} %b, zeroinitializer\n"
        );
        // Casts to lanes and back.
        for (op, to) in [
            ("zext", "i8"),
            ("sext", "i16"),
            ("zext", "i64"),
            ("sext", "i32"),
            ("uitofp", "float"),
            ("sitofp", "double"),
            ("uitofp", "half"),
            ("sitofp", "bfloat"),
        ] {
            let (back, cmp) = if matches!(to, "float" | "double" | "half" | "bfloat") {
                ("fptosi", "")
            } else {
                ("trunc", "")
            };
            let _ = cmp;
            fns.mask_result(&t, &ia, &ib, &t, n, &format!(
                "{masks}  %w = {op} {m} %ma to <{n} x {to}>\n  %r = {back} <{n} x {to}> %w to {m}\n"));
        }
        // `fptoui` of lanes that are 0 or 1.
        fns.mask_result(&t, &ia, &ib, &t, n, &format!(
            "{masks}  %w = uitofp {m} %ma to <{n} x float>\n  %r = fptoui <{n} x float> %w to {m}\n"));
        // `trunc` keeps bit 0 only.
        fns.mask_result(&t, &ia, &ib, &t, n, &format!("  %r = trunc {t} %a to {m}\n"));
        // Element access.
        for i in [0, 1, n - 1, n, n + 9] {
            fns.mask_result(
                &t,
                &ia,
                &ib,
                &t,
                n,
                &format!(
                    "{masks}  %e = extractelement {m} %ma, i32 {i}\n  \
                 %r = insertelement {m} %mb, i1 %e, i64 {}\n",
                    (i + 1) % n
                ),
            );
        }
        for i in [0u32, 2, n - 1] {
            fns.raw(&format!(
                "@t{{k}}.a = global {ia}, align 64\n@t{{k}}.i = global i32 {i}\n\
                 @t{{k}}.o = global [8 x i8] zeroinitializer, align 64\n\
                 define void @t{{k}}() {{\nentry:\n  %a = load {t}, ptr @t{{k}}.a\n  \
                 %i = load i32, ptr @t{{k}}.i\n  %ma = icmp ne {t} %a, zeroinitializer\n  \
                 %e = extractelement {m} %ma, i32 %i\n  %f = xor i1 %e, true\n  \
                 %r = insertelement {m} %ma, i1 %f, i32 %i\n  store {m} %r, ptr @t{{k}}.o\n  \
                 call void @emit(ptr @t{{k}}.o, i64 8)\n  %z = zext i1 %e to i8\n  \
                 store i8 %z, ptr @t{{k}}.o\n  call void @emit(ptr @t{{k}}.o, i64 1)\n  \
                 ret void\n}}\n\n"
            ));
        }
        // Shuffles: reverse, interleave of two, undefined entries, `%a,
        // %a`, longer and shorter.
        let entries = |list: Vec<Option<u32>>| {
            list.iter()
                .map(|e| e.map_or("i32 poison".to_string(), |e| format!("i32 {e}")))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let shuffles: Vec<(Vec<Option<u32>>, bool)> = vec![
            ((0..n).rev().map(Some).collect(), false),
            ((0..n).map(|j| Some(if j % 2 == 0 { j / 2 } else { n + j / 2 })).collect(), false),
            ((0..n).map(|j| (j % 3 != 2).then_some((j * 7) % (2 * n))).collect(), false),
            ((0..n).map(|j| Some((j + 1) % (2 * n))).collect(), true),
            ((0..(2 * n).min(64)).map(|j| Some(j % (2 * n))).collect(), false),
            ((0..n.div_ceil(2)).map(|j| Some(n + 2 * j % n)).collect(), false),
        ];
        for (list, same) in shuffles {
            let len = list.len() as u32;
            let second = if same { "%ma" } else { "%mb" };
            fns.mask_result(
                &t,
                &ia,
                &ib,
                &t,
                len,
                &format!(
                    "{masks}  %r = shufflevector {m} %ma, {m} {second}, <{len} x i32> <{}>\n",
                    entries(list)
                ),
            );
        }
        // Reductions, as a mask of one lane.
        for red in ["or", "and", "xor", "add", "mul", "umax", "umin", "smax", "smin"] {
            fns.raw(&format!(
                "declare i1 @llvm.vector.reduce.{red}.v{n}i1({m})\n\
                 @t{{k}}.a = global {ia}, align 64\n@t{{k}}.b = global {ib}, align 64\n\
                 @t{{k}}.o = global [1 x i8] zeroinitializer, align 64\n\
                 define void @t{{k}}() {{\nentry:\n  %a = load {t}, ptr @t{{k}}.a\n  \
                 %b = load {t}, ptr @t{{k}}.b\n{masks}  \
                 %x = call i1 @llvm.vector.reduce.{red}.v{n}i1({m} %ma)\n  \
                 %y = call i1 @llvm.vector.reduce.{red}.v{n}i1({m} %mb)\n  \
                 %u = call i1 @llvm.vector.reduce.{red}.v{n}i1({m} zeroinitializer)\n  \
                 %v = call i1 @llvm.vector.reduce.{red}.v{n}i1({m} splat (i1 true))\n  \
                 %x8 = zext i1 %x to i8\n  %y8 = zext i1 %y to i8\n  %u8 = zext i1 %u to i8\n  \
                 %v8 = zext i1 %v to i8\n  %s1 = shl i8 %y8, 1\n  %s2 = shl i8 %u8, 2\n  \
                 %s3 = shl i8 %v8, 3\n  %o1 = or i8 %x8, %s1\n  %o2 = or i8 %o1, %s2\n  \
                 %o3 = or i8 %o2, %s3\n  store i8 %o3, ptr @t{{k}}.o\n  \
                 call void @emit(ptr @t{{k}}.o, i64 1)\n  ret void\n}}\n\n"
            ));
        }
    }
    // The declarations are repeated per function: keep the first.
    let mut seen = std::collections::BTreeSet::new();
    for (_, text) in &mut fns.out {
        let Some(text) = text else { continue };
        let mut kept = String::new();
        for line in text.lines() {
            if line.starts_with("declare") && !seen.insert(line.to_string()) {
                continue;
            }
            let _ = writeln!(kept, "{line}");
        }
        *text = kept;
    }
    run("access", fns);
}

/// What is refused, by name: an intrinsic of `i1` lanes other than a
/// reduction; a lane operation on a mask of more than 64 lanes.
#[test]
fn what_is_refused_is_refused_by_name() {
    let cases = [
        (
            "declare <4 x i1> @llvm.umax.v4i1(<4 x i1>, <4 x i1>)\n\
             define void @f(ptr %p) {\n  %v = load <4 x i1>, ptr %p\n  \
             %r = call <4 x i1> @llvm.umax.v4i1(<4 x i1> %v, <4 x i1> %v)\n  \
             store <4 x i1> %r, ptr %p\n  ret void\n}\n",
            hod_x64::mask::MASK,
        ),
        (
            "define void @f(ptr %p) {\n  %v = load <65 x i8>, ptr %p\n  \
             %r = icmp eq <65 x i8> %v, zeroinitializer\n  store <65 x i1> %r, ptr %p\n  \
             ret void\n}\n",
            hod_x64::vector::MASK_WIDE,
        ),
    ];
    for (text, name) in cases {
        let (module, bodies) = parse(&format!("{}{text}", header()));
        match hod_x64::compile_to_object(&module, &bodies, &pic(Stress::default())) {
            Err(hod_x64::ModuleError::Function { error, .. }) => {
                assert_eq!(error, CgError::unsupported(name), "{text}");
            }
            other => panic!("not refused: {text}: {:?}", other.map(|o| o.len())),
        }
    }
}
