//! The short tail, third part (WP-101, `spec/codegen.md` §5.5.1
//! "WP-101"): vectors of `i128` and `fp128` lanes and their ABI,
//! `bfloat` arrays and vectors, the `bfloat` NEON intrinsics, a number
//! minus an address and `preserve_allcc`.
//!
//! Each table of functions is compiled by hod (plain, every stress
//! option, far mode) and by `llc -O0 -global-isel=0 -fast-isel=0` of
//! LLVM 22 and of LLVM 19, linked with a C driver under every linker
//! present and run. A table split over two modules (a caller and a
//! callee) is also linked with one module from hod and the other from
//! each `llc`, so that the two sides of every call are checked against
//! each other. The reference is `hod_interp` where it models the
//! construct, else the `llc` 22 build.

#![allow(
    clippy::too_many_lines,
    clippy::too_many_arguments,
    clippy::similar_names,
    reason = "tables of `.ll` text built by hand"
)]

mod common;

use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use common::linking::Toolchain;
use common::{assert_refused, ll_header, parse, write_object};
use hod_a64::{compile_module, tail3};
use hod_cg::{Options, RelocModel, Stress};
use hod_interp::{Interp, Program, Val};

/// A function of the table and its six arguments.
type Call = (String, [u64; 6]);

const DEFAULT: Options = Options {
    reloc_model: RelocModel::Static,
    code_model: None,
    function_sections: false,
    data_sections: false,
    stress: Stress { min_pool: false, far: false, displace: false },
    debug_compression: hod_obj::Compression::None,
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
            u64::MAX,
            1 << 63,
            (1 << 63) - 1,
            0xffff_ffff,
            0x3ff0_0000_0000_0000,
            0x7ff0_0000_0000_0000,
            0x7ff8_0000_0000_0001,
            0xbff0_0000_0000_0000,
        ];
        let r = self.next();
        if r.is_multiple_of(3) {
            EDGES[(r >> 8) as usize % EDGES.len()]
        } else {
            self.next() >> (r % 64)
        }
    }
}

/// `count` calls of each of `names` with words from `rng`, and for each
/// name calls whose pairs of arguments are equal (so that compares see
/// equal lanes).
fn calls_of(names: &[String], rng: &mut Rng, count: usize) -> Vec<Call> {
    let mut calls = Vec::new();
    for name in names {
        for i in 0..count {
            let mut args = [(); 6].map(|()| rng.word());
            if i % 4 == 0 {
                args[2] = args[0];
                args[3] = args[1];
            }
            calls.push((name.clone(), args));
        }
    }
    calls
}

/// The `llc` of LLVM `major`, if installed.
fn llc_of(major: u32) -> Option<PathBuf> {
    let installed = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/llc"));
    if installed.is_file() {
        return Some(installed);
    }
    let found = hod_testutil::find_tool(&format!("llc-{major}"))?;
    found.is_file().then_some(found)
}

/// Compiles `text` with `llc` of LLVM `major` into `dir/name.o`.
fn llc_build(dir: &hod_testutil::Scratch, major: u32, name: &str, body: &str) -> Option<PathBuf> {
    let llc = llc_of(major)?;
    let input = dir.write(&format!("{name}.ll"), format!("{}{body}", ll_header(major)));
    let output = dir.join(format!("{name}.o"));
    let mut command = Command::new(llc);
    command
        .args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj"])
        .arg(&input)
        .arg("-o")
        .arg(&output);
    let outcome = hod_testutil::run_with(&mut command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "llc {major} failed:\n{}", outcome.stderr_text());
    Some(output)
}

/// The C driver that calls each of `calls` through a table and prints
/// the results, one line each.
fn driver(calls: &[Call]) -> String {
    let mut c = String::from(
        "#include <stdio.h>\n#include <stdint.h>\n\
         typedef uint64_t (*fn6)(uint64_t, uint64_t, uint64_t, uint64_t, uint64_t, uint64_t);\n",
    );
    let mut declared: Vec<&str> = Vec::new();
    for (name, _) in calls {
        if !declared.contains(&name.as_str()) {
            writeln!(c, "uint64_t {name}(void);").unwrap();
            declared.push(name);
        }
    }
    c.push_str("static const struct { fn6 f; uint64_t a[6]; } calls[] = {\n");
    for (name, args) in calls {
        let list: Vec<String> = args.iter().map(|a| format!("{a}ULL")).collect();
        writeln!(c, "  {{ (fn6){name}, {{ {} }} }},", list.join(", ")).unwrap();
    }
    c.push_str(
        "};\nint main(void) {\n  for (unsigned i = 0; i < sizeof calls / sizeof calls[0]; i++) {\n    \
         const uint64_t *a = calls[i].a;\n    \
         printf(\"%llu\\n\", (unsigned long long)calls[i].f(a[0], a[1], a[2], a[3], a[4], a[5]));\n  \
         }\n  return 0;\n}\n",
    );
    c
}

/// Builds the modules `bodies` (one or two: a caller and a callee) by hod
/// in every mode and by `llc` 22 and 19, and the mixed builds of two
/// modules (each side from hod plain and the other from each `llc`);
/// runs `calls` under every linker and checks each build against `want`
/// (or the `llc` 22 build). Returns the number of builds checked (0
/// without the tools).
fn run(label: &str, bodies: &[&str], calls: &[Call], want: Option<&str>) -> usize {
    build_and_run(label, bodies, calls, want, true)
}

/// [`run`] with hod's builds only, against `want`.
fn run_hod(label: &str, body: &str, calls: &[Call], want: &str) -> usize {
    build_and_run(label, &[body], calls, Some(want), false)
}

fn build_and_run(
    label: &str,
    bodies: &[&str],
    calls: &[Call],
    want: Option<&str>,
    with_llc: bool,
) -> usize {
    let dir = hod_testutil::Scratch::new(label);
    let Some(toolchain) = Toolchain::host(&dir) else { return 0 };
    let driver = toolchain.compile_c(&dir, "driver", &driver(calls), &[]);
    // builds[k] = (name, one object per module)
    let mut builds: Vec<(String, Vec<PathBuf>)> = Vec::new();
    for major in if with_llc { &[22, 19][..] } else { &[] } {
        let major = *major;
        let mut objects = Vec::new();
        for (i, body) in bodies.iter().enumerate() {
            match llc_build(&dir, major, &format!("llc{major}-{i}"), body) {
                Some(object) => objects.push(object),
                None => break,
            }
        }
        if objects.len() == bodies.len() {
            builds.push((format!("llc{major}"), objects));
        }
    }
    if with_llc && builds.is_empty() {
        return 0;
    }
    let mut hod_plain = Vec::new();
    for (mode, options) in MODES {
        let mut objects = Vec::new();
        for (i, body) in bodies.iter().enumerate() {
            let (module, fbodies) = parse(&format!("{}{body}", ll_header(22)));
            let emitted = compile_module(&module, &fbodies, &options)
                .unwrap_or_else(|e| panic!("{label} {mode} module {i}: {e}"));
            objects.push(write_object(&dir, &format!("{mode}-{i}.o"), &emitted));
        }
        if mode == "plain" {
            hod_plain.clone_from(&objects);
        }
        builds.push((mode.to_owned(), objects));
    }
    if bodies.len() == 2 {
        let llc_builds: Vec<(String, Vec<PathBuf>)> =
            builds.iter().filter(|(n, _)| n.starts_with("llc")).cloned().collect();
        for (name, objects) in llc_builds {
            builds.push((
                format!("hod-caller+{name}"),
                vec![hod_plain[0].clone(), objects[1].clone()],
            ));
            builds.push((
                format!("{name}-caller+hod"),
                vec![objects[0].clone(), hod_plain[1].clone()],
            ));
        }
    }
    let mut reference = want.map(str::to_owned);
    for linker in &toolchain.linkers {
        for (name, objects) in &builds {
            let exe = dir.join(format!("{name}-{}", linker.name));
            let mut inputs = vec![driver.as_path()];
            inputs.extend(objects.iter().map(PathBuf::as_path));
            // `frem` of `fp128` calls `fmodl`; the library follows the objects.
            inputs.push(std::path::Path::new("-lm"));
            let ran = toolchain.link_and_run(linker, &[], &inputs, &exe);
            assert!(ran.success(), "{label} {name} under {}: {}", linker.name, ran.stderr_text());
            let got = ran.stdout_text();
            let reference = reference.get_or_insert_with(|| got.clone());
            if &got != reference {
                let at = got.lines().zip(reference.lines()).position(|(g, w)| g != w).unwrap_or(0);
                let (function, args) = &calls[at.min(calls.len() - 1)];
                let g = got.lines().nth(at).unwrap_or_default();
                let w = reference.lines().nth(at).unwrap_or_default();
                panic!(
                    "{label}: {name} under {}: call {at} @{function}{args:?} gives {g}, the reference {w}",
                    linker.name
                );
            }
        }
    }
    eprintln!("{label}: {} calls agree in {} builds", calls.len(), builds.len());
    builds.len()
}

/// What `hod_interp` prints for each call of `body`'s functions.
fn interp_lines(body: &str, calls: &[Call]) -> String {
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

// ---- generated functions ----

/// The parameters of every table function.
const PARAMS: &str = "i64 %a, i64 %b, i64 %c, i64 %d, i64 %e, i64 %f";

/// `%x`, `%y`, `%z`: `i128`s made of the pairs `(a, b)`, `(c, d)`,
/// `(e, f)`.
const I128_ARGS: &str = "
  %az = zext i64 %a to i128
  %bz = zext i64 %b to i128
  %bs = shl i128 %bz, 64
  %x = or i128 %az, %bs
  %cz = zext i64 %c to i128
  %dz = zext i64 %d to i128
  %ds = shl i128 %dz, 64
  %y = or i128 %cz, %ds
  %ez = zext i64 %e to i128
  %fz = zext i64 %f to i128
  %fs = shl i128 %fz, 64
  %z = or i128 %ez, %fs
";

/// `%x`, `%y`, `%z`: `fp128`s, the doubles `a`, `c`, `e` widened.
const FP128_ARGS: &str = "
  %ad = bitcast i64 %a to double
  %x = fpext double %ad to fp128
  %cd = bitcast i64 %c to double
  %y = fpext double %cd to fp128
  %ed = bitcast i64 %e to double
  %z = fpext double %ed to fp128
";

/// Builds `%name` = `<n x elem>` whose lane `i` is `lanes[i % 3]` (of
/// `%x`, `%y`, `%z`), by `insertelement` with constant indices.
fn build(out: &mut String, name: &str, n: u32, elem: &str, lanes: [&str; 3]) {
    let ty = format!("<{n} x {elem}>");
    let mut prev = "poison".to_owned();
    for i in 0..n {
        let next = if i + 1 == n { format!("%{name}") } else { format!("%{name}.{i}") };
        writeln!(
            out,
            "  {next} = insertelement {ty} {prev}, {elem} %{}, i32 {i}",
            lanes[i as usize % 3]
        )
        .unwrap();
        prev = next;
    }
}

/// Folds the `<n x i128>` `%name` into `%h{name}`, an `i64` that depends
/// on every bit of every lane.
fn fold(out: &mut String, name: &str, n: u32) {
    let ty = format!("<{n} x i128>");
    let mut acc = "0".to_owned();
    for i in 0..n {
        writeln!(
            out,
            "  %{name}.e{i} = extractelement {ty} %{name}, i32 {i}
  %{name}.l{i} = trunc i128 %{name}.e{i} to i64
  %{name}.s{i} = lshr i128 %{name}.e{i}, 64
  %{name}.h{i} = trunc i128 %{name}.s{i} to i64
  %{name}.m{i} = mul i64 %{name}.h{i}, -7046029254386353131
  %{name}.x{i} = xor i64 %{name}.m{i}, %{name}.l{i}
  %{name}.r{i} = mul i64 {acc}, 1000003
  %{name}.a{i} = add i64 %{name}.r{i}, %{name}.x{i}"
        )
        .unwrap();
        acc = format!("%{name}.a{i}");
    }
    writeln!(out, "  %h{name} = add i64 {acc}, 0").unwrap();
}

/// `%name` (`<n x fp128>`) bitcast to `<n x i128>` as `%name.i`, folded.
fn fold_fp(out: &mut String, name: &str, n: u32) {
    writeln!(out, "  %{name}.i = bitcast <{n} x fp128> %{name} to <{n} x i128>").unwrap();
    fold(out, &format!("{name}.i"), n);
    writeln!(out, "  %h{name} = add i64 %h{name}.i, 0").unwrap();
}

/// The mask `%name` (`<n x i1>`) as `%h{name}`, an `i64`.
fn mask_bits(out: &mut String, name: &str, n: u32) {
    writeln!(
        out,
        "  %{name}.b = bitcast <{n} x i1> %{name} to i{n}\n  %h{name} = zext i{n} %{name}.b to i64"
    )
    .unwrap();
}

/// The lane counts every operation is tried at: a register vector, a
/// power of two in memory, an odd count.
const COUNTS: [u32; 3] = [1, 2, 3];

/// The table of `i128`-lane functions, and their names.
fn i128_table() -> (String, Vec<String>) {
    let mut body = String::new();
    let mut names = Vec::new();
    let mut def = |body: &mut String, name: String, text: &str, result: &str| {
        writeln!(body, "define i64 @{name}({PARAMS}) {{{I128_ARGS}{text}  ret i64 {result}\n}}\n")
            .unwrap();
        names.push(name);
    };
    for n in COUNTS {
        let ty = format!("<{n} x i128>");
        for op in ["add", "sub", "mul", "and", "or", "xor"] {
            let mut t = String::new();
            build(&mut t, "va", n, "i128", ["x", "y", "z"]);
            build(&mut t, "vb", n, "i128", ["z", "x", "y"]);
            writeln!(t, "  %r = {op} {ty} %va, %vb").unwrap();
            writeln!(
                t,
                "  %s = {op} {ty} %r, splat (i128 -170141183460469231731687303715884105727)"
            )
            .unwrap();
            writeln!(t, "  %u = {op} {ty} %s, %s").unwrap();
            fold(&mut t, "r", n);
            fold(&mut t, "u", n);
            writeln!(t, "  %res = xor i64 %hr, %hu").unwrap();
            def(&mut body, format!("bin_{op}_{n}"), &t, "%res");
        }
        for pred in ["eq", "ne", "ult", "ule", "ugt", "uge", "slt", "sle", "sgt", "sge"] {
            let mut t = String::new();
            build(&mut t, "va", n, "i128", ["x", "y", "z"]);
            build(&mut t, "vb", n, "i128", ["y", "x", "y"]);
            writeln!(t, "  %m = icmp {pred} {ty} %va, %vb").unwrap();
            writeln!(t, "  %k = icmp {pred} {ty} %va, zeroinitializer").unwrap();
            mask_bits(&mut t, "m", n);
            mask_bits(&mut t, "k", n);
            writeln!(t, "  %ks = shl i64 %hk, 8\n  %res = or i64 %hm, %ks").unwrap();
            def(&mut body, format!("icmp_{pred}_{n}"), &t, "%res");
        }
        // Masks to and from lanes, select on a mask.
        let mut t = String::new();
        build(&mut t, "va", n, "i128", ["x", "y", "z"]);
        build(&mut t, "vb", n, "i128", ["y", "z", "x"]);
        writeln!(
            t,
            "  %m = icmp ult {ty} %va, %vb
  %se = sext <{n} x i1> %m to {ty}
  %ze = zext <{n} x i1> %m to {ty}
  %sl = select <{n} x i1> %m, {ty} %va, {ty} %vb
  %tr = trunc {ty} %va to <{n} x i1>"
        )
        .unwrap();
        fold(&mut t, "se", n);
        fold(&mut t, "ze", n);
        fold(&mut t, "sl", n);
        mask_bits(&mut t, "tr", n);
        writeln!(
            t,
            "  %r1 = xor i64 %hse, %hze\n  %r2 = add i64 %r1, %hsl\n  %res = add i64 %r2, %htr"
        )
        .unwrap();
        def(&mut body, format!("mask_{n}"), &t, "%res");
        // Integer lanes to and from i128 lanes.
        for (narrow, bits) in [("i8", 8), ("i32", 32), ("i64", 64)] {
            let nty = format!("<{n} x {narrow}>");
            let mut t = String::new();
            let op = if bits == 64 { "add i64 0," } else { "trunc i64" };
            let to = if bits == 64 { String::new() } else { format!(" to {narrow}") };
            for v in ["a", "c", "e"] {
                writeln!(t, "  %t{v} = {op} %{v}{to}").unwrap();
            }
            build(&mut t, "na", n, narrow, ["ta", "tc", "te"]);
            build(&mut t, "va", n, "i128", ["x", "y", "z"]);
            writeln!(
                t,
                "  %sx = sext {nty} %na to {ty}
  %zx = zext {nty} %na to {ty}
  %tn = trunc {ty} %va to {nty}
  %tz = zext {nty} %tn to {ty}"
            )
            .unwrap();
            fold(&mut t, "sx", n);
            fold(&mut t, "zx", n);
            fold(&mut t, "tz", n);
            writeln!(t, "  %r1 = xor i64 %hsx, %hzx\n  %res = add i64 %r1, %htz").unwrap();
            def(&mut body, format!("cast_{bits}_{n}"), &t, "%res");
        }
        // insertelement and extractelement with variable indices.
        let mut t = String::new();
        build(&mut t, "va", n, "i128", ["x", "y", "z"]);
        let index = if n.is_power_of_two() {
            format!("and i64 %b, {}", n - 1)
        } else {
            format!("urem i64 %b, {n}")
        };
        writeln!(
            t,
            "  %i = {index}
  %j = {}
  %v1 = insertelement {ty} %va, i128 %z, i64 %i
  %v2 = insertelement {ty} %v1, i128 %y, i64 %j
  %v3 = insertelement {ty} %v2, i128 7, i32 0
  %g = extractelement {ty} %v2, i64 %j
  %gl = trunc i128 %g to i64",
            index.replace("%b", "%d")
        )
        .unwrap();
        fold(&mut t, "v2", n);
        fold(&mut t, "v3", n);
        writeln!(t, "  %r1 = xor i64 %hv2, %hv3\n  %res = add i64 %r1, %gl").unwrap();
        def(&mut body, format!("elem_{n}"), &t, "%res");
    }
    // shufflevector between lane counts, with an undefined lane.
    let mut t = String::new();
    build(&mut t, "va", 2, "i128", ["x", "y", "z"]);
    build(&mut t, "vb", 2, "i128", ["z", "x", "y"]);
    build(&mut t, "v1", 1, "i128", ["y", "x", "z"]);
    writeln!(
        t,
        "  %s3 = shufflevector <2 x i128> %va, <2 x i128> %vb, <3 x i32> <i32 3, i32 2, i32 0>
  %s1 = shufflevector <2 x i128> %va, <2 x i128> poison, <1 x i32> <i32 1>
  %s2 = shufflevector <1 x i128> %v1, <1 x i128> %s1, <2 x i32> <i32 1, i32 0>"
    )
    .unwrap();
    fold(&mut t, "s3", 3);
    fold(&mut t, "s1", 1);
    fold(&mut t, "s2", 2);
    writeln!(t, "  %r1 = xor i64 %hs3, %hs1\n  %res = add i64 %r1, %hs2").unwrap();
    def(&mut body, "shuffle".to_owned(), &t, "%res");
    (body, names)
}

#[test]
fn i128_lanes_agree_with_the_interpreter_and_llc() {
    let (body, names) = i128_table();
    let mut rng = Rng(0x0101_0128);
    let calls = calls_of(&names, &mut rng, 8);
    let want = interp_lines(&body, &calls);
    run("a64-tail3-i128", &[&body], &calls, Some(&want));
}

/// The table of `fp128`-lane functions, and their names.
fn fp128_table() -> (String, Vec<String>) {
    let mut body = String::new();
    let mut names = Vec::new();
    let mut def = |body: &mut String, name: String, text: &str, result: &str| {
        writeln!(body, "define i64 @{name}({PARAMS}) {{{FP128_ARGS}{text}  ret i64 {result}\n}}\n")
            .unwrap();
        names.push(name);
    };
    for n in COUNTS {
        let ty = format!("<{n} x fp128>");
        for op in ["fadd", "fsub", "fmul", "fdiv", "frem"] {
            let mut t = String::new();
            build(&mut t, "va", n, "fp128", ["x", "y", "z"]);
            build(&mut t, "vb", n, "fp128", ["z", "x", "y"]);
            writeln!(
                t,
                "  %r = {op} {ty} %va, %vb
  %s = {op} {ty} %r, splat (fp128 0xL00000000000000003FFF000000000000)
  %u = fneg {ty} %s"
            )
            .unwrap();
            fold_fp(&mut t, "r", n);
            fold_fp(&mut t, "u", n);
            writeln!(t, "  %res = xor i64 %hr, %hu").unwrap();
            def(&mut body, format!("fbin_{op}_{n}"), &t, "%res");
        }
        let mut t = String::new();
        build(&mut t, "va", n, "fp128", ["x", "y", "z"]);
        build(&mut t, "vb", n, "fp128", ["y", "x", "y"]);
        let preds = [
            "false", "oeq", "ogt", "oge", "olt", "ole", "one", "ord", "ueq", "ugt", "uge", "ult",
            "ule", "une", "uno", "true",
        ];
        let mut acc = "0".to_owned();
        for (k, pred) in preds.iter().enumerate() {
            writeln!(t, "  %m{k} = fcmp {pred} {ty} %va, %vb").unwrap();
            mask_bits(&mut t, &format!("m{k}"), n);
            writeln!(t, "  %sh{k} = shl i64 %hm{k}, {}\n  %acc{k} = or i64 {acc}, %sh{k}", 3 * k)
                .unwrap();
            acc = format!("%acc{k}");
        }
        def(&mut body, format!("fcmp_{n}"), &t, &acc);
        let mut t = String::new();
        build(&mut t, "va", n, "fp128", ["x", "y", "z"]);
        let index = if n.is_power_of_two() {
            format!("and i64 %b, {}", n - 1)
        } else {
            format!("urem i64 %b, {n}")
        };
        writeln!(
            t,
            "  %i = {index}
  %v1 = insertelement {ty} %va, fp128 %z, i64 %i
  %g = extractelement {ty} %v1, i64 %i
  %k = extractelement {ty} %va, i32 0
  %gs = fadd fp128 %g, %k
  %gi = bitcast fp128 %gs to i128
  %gl = trunc i128 %gi to i64
  %m = fcmp olt {ty} %va, zeroinitializer
  %sl = select <{n} x i1> %m, {ty} %va, {ty} %v1
  %sh = shufflevector {ty} %v1, {ty} %va, <2 x i32> <i32 {n}, i32 0>"
        )
        .unwrap();
        fold_fp(&mut t, "v1", n);
        fold_fp(&mut t, "sl", n);
        fold_fp(&mut t, "sh", 2);
        writeln!(
            t,
            "  %r1 = xor i64 %hv1, %gl\n  %r2 = add i64 %r1, %hsl\n  %res = xor i64 %r2, %hsh"
        )
        .unwrap();
        def(&mut body, format!("felem_{n}"), &t, "%res");
    }
    (body, names)
}

#[test]
fn fp128_lanes_agree_with_llc() {
    let (body, names) = fp128_table();
    let mut rng = Rng(0x0101_f128);
    let calls = calls_of(&names, &mut rng, 8);
    run("a64-tail3-fp128", &[&body], &calls, None);
}

// ---- the ABI of i128 lanes ----

/// The callee module: functions taking and returning vectors of `i128`
/// and arrays and structures of them, at even and odd register numbers
/// and on the stack (a lone `<1 x i128>` is `abi:vector-element`).
const ABI_CALLEE: &str = "
define <2 x i128> @k_v2(i64 %a, <2 x i128> %x, i64 %b, <2 x i128> %y) noinline {
  %s = sub <2 x i128> %x, %y
  %e = zext i64 %a to i128
  %f = zext i64 %b to i128
  %v0 = insertelement <2 x i128> poison, i128 %e, i32 0
  %v = insertelement <2 x i128> %v0, i128 %f, i32 1
  %r = xor <2 x i128> %s, %v
  ret <2 x i128> %r
}

define <3 x i128> @k_v3(<3 x i128> %x, i64 %a) noinline {
  %e = zext i64 %a to i128
  %v = insertelement <3 x i128> %x, i128 %e, i32 2
  ret <3 x i128> %v
}

define i64 @k_arr0([1 x <1 x i128>] alignstack(16) %x, i64 %a) noinline {
  %v = extractvalue [1 x <1 x i128>] %x, 0
  %e = extractelement <1 x i128> %v, i32 0
  %h = lshr i128 %e, 64
  %s = add i128 %e, %h
  %t = trunc i128 %s to i64
  %r = xor i64 %t, %a
  ret i64 %r
}

define i64 @k_arr2(i64 %a, i64 %b, [1 x <1 x i128>] alignstack(16) %x) noinline {
  %v = extractvalue [1 x <1 x i128>] %x, 0
  %e = extractelement <1 x i128> %v, i32 0
  %h = lshr i128 %e, 64
  %s = sub i128 %e, %h
  %t = trunc i128 %s to i64
  %u = add i64 %t, %a
  %r = xor i64 %u, %b
  ret i64 %r
}

define i64 @k_arr7(i64 %a, i64 %b, i64 %c, i64 %d, i64 %e, i64 %f, i64 %g, [1 x <1 x i128>] alignstack(16) %x, i64 %h) noinline {
  %v = extractvalue [1 x <1 x i128>] %x, 0
  %w = extractelement <1 x i128> %v, i32 0
  %hi = lshr i128 %w, 64
  %s = mul i128 %w, %hi
  %t = trunc i128 %s to i64
  %u = add i64 %t, %g
  %r = xor i64 %u, %h
  ret i64 %r
}

define i64 @k_pair(i64 %a, [2 x <1 x i128>] %x) noinline {
  %v0 = extractvalue [2 x <1 x i128>] %x, 0
  %v1 = extractvalue [2 x <1 x i128>] %x, 1
  %s = add <1 x i128> %v0, %v1
  %e = extractelement <1 x i128> %s, i32 0
  %h = lshr i128 %e, 64
  %m = xor i128 %e, %h
  %t = trunc i128 %m to i64
  %r = add i64 %t, %a
  ret i64 %r
}

define { <1 x i128> } @k_struct({ <1 x i128> } %x, i64 %a) noinline {
  %v = extractvalue { <1 x i128> } %x, 0
  %e = zext i64 %a to i128
  %w = insertelement <1 x i128> poison, i128 %e, i32 0
  %s = mul <1 x i128> %v, %w
  %r = insertvalue { <1 x i128> } poison, <1 x i128> %s, 0
  ret { <1 x i128> } %r
}
";

/// The caller module: one table function per callee.
fn abi_caller() -> (String, Vec<String>) {
    let mut body = String::from(
        "
declare <2 x i128> @k_v2(i64, <2 x i128>, i64, <2 x i128>)
declare <3 x i128> @k_v3(<3 x i128>, i64)
declare i64 @k_arr0([1 x <1 x i128>] alignstack(16), i64)
declare i64 @k_arr2(i64, i64, [1 x <1 x i128>] alignstack(16))
declare i64 @k_arr7(i64, i64, i64, i64, i64, i64, i64, [1 x <1 x i128>] alignstack(16), i64)
declare i64 @k_pair(i64, [2 x <1 x i128>])
declare { <1 x i128> } @k_struct({ <1 x i128> }, i64)
",
    );
    let mut names = Vec::new();
    let mut def = |body: &mut String, name: &str, text: &str, result: &str| {
        writeln!(body, "define i64 @{name}({PARAMS}) {{{I128_ARGS}{text}  ret i64 {result}\n}}\n")
            .unwrap();
        names.push(name.to_owned());
    };
    let mut t = String::new();
    build(&mut t, "va", 2, "i128", ["x", "y", "z"]);
    build(&mut t, "vb", 2, "i128", ["z", "y", "x"]);
    writeln!(t, "  %r = call <2 x i128> @k_v2(i64 %a, <2 x i128> %va, i64 %f, <2 x i128> %vb)")
        .unwrap();
    fold(&mut t, "r", 2);
    def(&mut body, "c_v2", &t, "%hr");
    let mut t = String::new();
    build(&mut t, "va", 3, "i128", ["x", "y", "z"]);
    writeln!(t, "  %r = call <3 x i128> @k_v3(<3 x i128> %va, i64 %b)").unwrap();
    fold(&mut t, "r", 3);
    def(&mut body, "c_v3", &t, "%hr");
    let arr = "  %v = insertelement <1 x i128> poison, i128 %y, i32 0
  %ar = insertvalue [1 x <1 x i128>] poison, <1 x i128> %v, 0
";
    def(
        &mut body,
        "c_arr0",
        &format!("{arr}  %r = call i64 @k_arr0([1 x <1 x i128>] alignstack(16) %ar, i64 %a)\n"),
        "%r",
    );
    def(
        &mut body,
        "c_arr2",
        &format!(
            "{arr}  %r = call i64 @k_arr2(i64 %a, i64 %b, [1 x <1 x i128>] alignstack(16) %ar)\n"
        ),
        "%r",
    );
    def(
        &mut body,
        "c_arr7",
        &format!(
            "{arr}  %r = call i64 @k_arr7(i64 %a, i64 %b, i64 %c, i64 %d, i64 %e, i64 %f, i64 %a, [1 x <1 x i128>] alignstack(16) %ar, i64 %f)\n"
        ),
        "%r",
    );
    def(
        &mut body,
        "c_pair",
        &format!(
            "{arr}  %w = insertelement <1 x i128> poison, i128 %z, i32 0
  %pr = insertvalue [2 x <1 x i128>] poison, <1 x i128> %v, 0
  %pp = insertvalue [2 x <1 x i128>] %pr, <1 x i128> %w, 1
  %r = call i64 @k_pair(i64 %c, [2 x <1 x i128>] %pp)\n"
        ),
        "%r",
    );
    let mut t = String::from(
        "  %v = insertelement <1 x i128> poison, i128 %x, i32 0
  %st = insertvalue { <1 x i128> } poison, <1 x i128> %v, 0
  %rs = call { <1 x i128> } @k_struct({ <1 x i128> } %st, i64 %c)
  %r = extractvalue { <1 x i128> } %rs, 0
",
    );
    fold(&mut t, "r", 1);
    def(&mut body, "c_struct", &t, "%hr");
    (body, names)
}

#[test]
fn i128_lanes_cross_calls_as_llc_passes_them() {
    let (caller, names) = abi_caller();
    let mut rng = Rng(0x0101_0abc);
    let calls = calls_of(&names, &mut rng, 6);
    let builds = run("a64-tail3-abi", &[&caller, ABI_CALLEE], &calls, None);
    eprintln!("{builds} builds");
}

// ---- the whole libc++ shapes ----

/// A reduction of what `std::experimental::simd<__int128>` compiles to:
/// a storage struct of `<1 x i128>` returned in registers, the
/// `[1 x <1 x i128>]` argument, a mask stored as bytes, the unary
/// operators.
const SIMD_SHAPES: &str = "
%storage = type { <1 x i128> }

define %storage @set(i64 %i, i128 %v, [1 x <1 x i128>] alignstack(16) %s) noinline {
  %old = extractvalue [1 x <1 x i128>] %s, 0
  %n = insertelement <1 x i128> %old, i128 %v, i64 %i
  %r = insertvalue %storage poison, <1 x i128> %n, 0
  ret %storage %r
}

define void @store_mask([1 x <1 x i128>] alignstack(16) %s, ptr %p) noinline {
  %v = extractvalue [1 x <1 x i128>] %s, 0
  %e = extractelement <1 x i128> %v, i64 0
  %b = icmp ne i128 %e, 0
  %z = zext i1 %b to i8
  store i8 %z, ptr %p
  ret void
}

define i64 @simd(i64 %a, i64 %b, i64 %c, i64 %d, i64 %e, i64 %f) {
  %p = alloca i8
  %az = zext i64 %a to i128
  %bz = zext i64 %b to i128
  %bs = shl i128 %bz, 64
  %x = or i128 %az, %bs
  %v0 = insertelement <1 x i128> poison, i128 %x, i32 0
  %s0 = insertvalue [1 x <1 x i128>] poison, <1 x i128> %v0, 0
  %st = call %storage @set(i64 0, i128 %x, [1 x <1 x i128>] alignstack(16) %s0)
  %v = extractvalue %storage %st, 0
  %not = xor <1 x i128> %v, splat (i128 -1)
  %neg = sub <1 x i128> zeroinitializer, %not
  %inc = add <1 x i128> %neg, splat (i128 1)
  %m = icmp eq <1 x i128> %inc, zeroinitializer
  %mx = sext <1 x i1> %m to <1 x i128>
  %ms = insertvalue [1 x <1 x i128>] poison, <1 x i128> %mx, 0
  call void @store_mask([1 x <1 x i128>] alignstack(16) %ms, ptr %p)
  %byte = load i8, ptr %p
  %bw = zext i8 %byte to i64
  %w = extractelement <1 x i128> %inc, i32 0
  %lo = trunc i128 %w to i64
  %hi128 = lshr i128 %w, 64
  %hi = trunc i128 %hi128 to i64
  %h = mul i64 %hi, 31
  %r0 = add i64 %h, %lo
  %r = add i64 %r0, %bw
  ret i64 %r
}
";

#[test]
fn simd_shapes_agree_with_the_interpreter() {
    let mut rng = Rng(0x0101_51ad);
    let mut calls = calls_of(&["simd".to_owned()], &mut rng, 40);
    calls.push(("simd".to_owned(), [0, 0, 0, 0, 0, 0]));
    calls.push(("simd".to_owned(), [1, 0, 0, 0, 0, 0]));
    let want = interp_lines(SIMD_SHAPES, &calls);
    run("a64-tail3-simd", &[SIMD_SHAPES], &calls, Some(&want));
}

// ---- refusals ----

#[test]
fn wide_lane_refusals_are_named() {
    assert_refused(&[
        (
            "define void @f(ptr %p) {
  %a = load <2 x i128>, ptr %p
  %r = shl <2 x i128> %a, %a
  store <2 x i128> %r, ptr %p
  ret void
}",
            tail3::I128_OP,
        ),
        (
            "define void @f(ptr %p) {
  %a = load <1 x i128>, ptr %p
  %r = udiv <1 x i128> %a, %a
  store <1 x i128> %r, ptr %p
  ret void
}",
            tail3::I128_OP,
        ),
        (
            "define void @f(<2 x i128> %a, ptr %p) {
  %r = sitofp <2 x i128> %a to <2 x double>
  store <2 x double> %r, ptr %p
  ret void
}",
            tail3::WIDE_CAST,
        ),
        (
            "define void @f(<2 x double> %a, ptr %p) {
  %r = fpext <2 x double> %a to <2 x fp128>
  store <2 x fp128> %r, ptr %p
  ret void
}",
            tail3::WIDE_CAST,
        ),
        (
            "define void @f(ptr %p) {
  %a = load <65 x i128>, ptr %p
  %r = icmp eq <65 x i128> %a, zeroinitializer
  store <65 x i1> %r, ptr %p
  ret void
}",
            "a64:mask-wide",
        ),
        (
            "define i64 @f(<1 x i128> %x) {
  ret i64 0
}",
            "abi:vector-element",
        ),
        (
            "define i64 @f(i64 %a, [1 x <1 x i128>] %x) {
  ret i64 %a
}",
            "abi:wide-int-array",
        ),
        (
            "define i64 @f(i64 %a, i64 %b, i64 %c, i64 %d, i64 %e, [2 x <1 x i128>] %x) {
  ret i64 %a
}",
            "abi:wide-int-array",
        ),
    ]);
}

// ---- arrays of `bfloat` ----

/// `(floats before, members)` of the `[n x bfloat]` arguments every LLVM
/// configuration passes alike: all members in registers.
const BFLOAT_ARRAYS: [(u32, u32); 6] = [(0, 1), (0, 2), (0, 4), (5, 3), (6, 2), (7, 1)];

/// The callee and caller modules of [`BFLOAT_ARRAYS`], and the caller's
/// table functions.
fn bfloat_arrays() -> (String, String, Vec<String>) {
    let (mut callee, mut caller, mut names) = (String::new(), String::new(), Vec::new());
    for (floats, n) in BFLOAT_ARRAYS {
        let name = format!("bfa_{floats}_{n}");
        let params = (0..floats).fold(String::new(), |mut p, i| {
            write!(p, "float %f{i}, ").unwrap();
            p
        });
        let types: String = (0..floats).map(|_| "float, ").collect();
        // The callee folds each member's bits and each float's bits.
        let mut body =
            format!("define i64 @k_{name}({params}[{n} x bfloat] %a, i64 %s) noinline {{\n");
        let mut acc = "%s".to_owned();
        for i in 0..n {
            writeln!(
                body,
                "  %e{i} = extractvalue [{n} x bfloat] %a, {i}
  %b{i} = bitcast bfloat %e{i} to i16
  %z{i} = zext i16 %b{i} to i64
  %m{i} = mul i64 {acc}, 65599
  %a{i} = add i64 %m{i}, %z{i}"
            )
            .unwrap();
            acc = format!("%a{i}");
        }
        for i in 0..floats {
            writeln!(
                body,
                "  %fb{i} = bitcast float %f{i} to i32
  %fz{i} = zext i32 %fb{i} to i64
  %fm{i} = mul i64 {acc}, 31
  %fa{i} = add i64 %fm{i}, %fz{i}"
            )
            .unwrap();
            acc = format!("%fa{i}");
        }
        writeln!(body, "  ret i64 {acc}\n}}\n").unwrap();
        callee.push_str(&body);
        writeln!(caller, "declare i64 @k_{name}({types}[{n} x bfloat], i64)").unwrap();
        let mut t = format!("define i64 @{name}({PARAMS}) {{\n");
        let words = ["%a", "%b", "%c", "%d"];
        let mut prev = "poison".to_owned();
        for i in 0..n {
            let shift = 16 * (i % 4);
            writeln!(
                t,
                "  %w{i} = lshr i64 {}, {shift}
  %h{i} = trunc i64 %w{i} to i16
  %v{i} = bitcast i16 %h{i} to bfloat
  %r{i} = insertvalue [{n} x bfloat] {prev}, bfloat %v{i}, {i}",
                words[i as usize / 4]
            )
            .unwrap();
            prev = format!("%r{i}");
        }
        let mut args = String::new();
        for i in 0..floats {
            writeln!(t, "  %ft{i} = trunc i64 %e to i32\n  %fx{i} = xor i32 %ft{i}, {i}\n  %f{i} = bitcast i32 %fx{i} to float").unwrap();
            write!(args, "float %f{i}, ").unwrap();
        }
        writeln!(
            t,
            "  %r = call i64 @k_{name}({args}[{n} x bfloat] {prev}, i64 %f)\n  ret i64 %r\n}}\n"
        )
        .unwrap();
        caller.push_str(&t);
        names.push(name);
    }
    (caller, callee, names)
}

#[test]
fn bfloat_arrays_cross_calls_as_llc_passes_them() {
    let (caller, callee, names) = bfloat_arrays();
    let mut rng = Rng(0x0101_bf16);
    let calls = calls_of(&names, &mut rng, 6);
    run("a64-tail3-bfloat-array", &[&caller, &callee], &calls, None);
}

#[test]
fn bfloat_arrays_that_llc_versions_place_apart_are_refused() {
    // llc 22 puts the three on the stack, llc 19 one in `h7` (§3.3).
    assert_refused(&[(
        "define i64 @f(float %a, float %b, float %c, float %d, float %e, float %f, float %g, [3 x bfloat] %x) {
  ret i64 0
}",
        "abi:bfloat-array",
    )]);
}

// ---- vectors of `bfloat` ----

/// `%name` = `<n x bfloat>` whose lane `i` is 16 bits of the arguments
/// (`a`, `b`, `c`, `d`, four lanes each, then again).
fn build_bf(out: &mut String, name: &str, n: u32, words: [&str; 4]) {
    let mut prev = "poison".to_owned();
    for i in 0..n {
        let next = if i + 1 == n { format!("%{name}") } else { format!("%{name}.{i}") };
        writeln!(
            out,
            "  %{name}.w{i} = lshr i64 %{}, {}
  %{name}.t{i} = trunc i64 %{name}.w{i} to i16
  %{name}.b{i} = bitcast i16 %{name}.t{i} to bfloat
  {next} = insertelement <{n} x bfloat> {prev}, bfloat %{name}.b{i}, i32 {i}",
            words[(i as usize / 4) % 4],
            16 * (i % 4)
        )
        .unwrap();
        prev = next;
    }
}

/// Folds the floating-point `<n x elem>` `%name` (elements of `bits`
/// bits) into `%h{name}` through its integer lanes, every NaN counted as
/// one value (which NaN an operation of two NaNs gives depends on the
/// order of its registers, which LLVM does not specify).
fn fold_lanes(out: &mut String, name: &str, n: u32, elem: &str, bits: u32) {
    writeln!(out, "  %{name}.iv = bitcast <{n} x {elem}> %{name} to <{n} x i{bits}>").unwrap();
    let (abs, inf): (u64, u64) = match bits {
        16 => (0x7fff, 0x7f80),
        32 => (0x7fff_ffff, 0x7f80_0000),
        _ => (0x7fff_ffff_ffff_ffff, 0x7ff0_0000_0000_0000),
    };
    let mut acc = "0".to_owned();
    for i in 0..n {
        let ext = if bits == 64 {
            format!("add i64 %{name}.e{i}, 0")
        } else {
            format!("zext i{bits} %{name}.e{i} to i64")
        };
        writeln!(
            out,
            "  %{name}.e{i} = extractelement <{n} x i{bits}> %{name}.iv, i32 {i}
  %{name}.x{i} = {ext}
  %{name}.b{i} = and i64 %{name}.x{i}, {abs}
  %{name}.n{i} = icmp ugt i64 %{name}.b{i}, {inf}
  %{name}.z{i} = select i1 %{name}.n{i}, i64 -1, i64 %{name}.x{i}
  %{name}.m{i} = mul i64 {acc}, 1000003
  %{name}.a{i} = add i64 %{name}.m{i}, %{name}.z{i}"
        )
        .unwrap();
        acc = format!("%{name}.a{i}");
    }
    writeln!(out, "  %h{name} = add i64 {acc}, 0").unwrap();
}

/// `%dst = <instr> <n x from> <args>` (to `<n x to>`), or, `scalar`, the
/// same lane by lane on scalars (extract, operate, insert), which is the
/// reference form for `llc`. `instr` is a binary operator or an `fcmp`
/// (two arguments), `fneg` (one), or a cast (one, `to` its type).
fn lanewise(
    t: &mut String,
    scalar: bool,
    n: u32,
    dst: &str,
    instr: &str,
    args: &[&str],
    from: &str,
    to: &str,
) {
    let cast = matches!(instr, "fpext" | "fptrunc");
    let text = |ty: &str, a: &[String], to_ty: &str| match (a, cast) {
        ([x], true) => format!("{instr} {ty} {x} to {to_ty}"),
        ([x], false) => format!("{instr} {ty} {x}"),
        ([x, y], _) => format!("{instr} {ty} {x}, {y}"),
        _ => unreachable!(),
    };
    if !scalar {
        let a: Vec<String> = args.iter().map(|x| format!("%{x}")).collect();
        writeln!(
            t,
            "  %{dst} = {}",
            text(&format!("<{n} x {from}>"), &a, &format!("<{n} x {to}>"))
        )
        .unwrap();
        return;
    }
    let mut prev = "poison".to_owned();
    for i in 0..n {
        let mut a = Vec::new();
        for x in args {
            writeln!(t, "  %{dst}.{x}{i} = extractelement <{n} x {from}> %{x}, i32 {i}").unwrap();
            a.push(format!("%{dst}.{x}{i}"));
        }
        writeln!(t, "  %{dst}.s{i} = {}", text(from, &a, to)).unwrap();
        let next = if i + 1 == n { format!("%{dst}") } else { format!("%{dst}.v{i}") };
        writeln!(t, "  {next} = insertelement <{n} x {to}> {prev}, {to} %{dst}.s{i}, i32 {i}")
            .unwrap();
        prev = next;
    }
}

/// The table of `bfloat`-lane functions of `counts` lanes, and their
/// names; with `scalar`, each lane operation is written on scalars.
fn bfloat_table(counts: &[u32], scalar: bool) -> (String, Vec<String>) {
    let mut body = String::new();
    let mut names = Vec::new();
    let mut def = |body: &mut String, name: String, text: &str, result: &str| {
        writeln!(body, "define i64 @{name}({PARAMS}) {{\n{text}  ret i64 {result}\n}}\n").unwrap();
        names.push(name);
    };
    for &n in counts {
        for op in ["fadd", "fsub", "fmul", "fdiv", "frem"] {
            let mut t = String::new();
            build_bf(&mut t, "va", n, ["a", "b", "c", "d"]);
            build_bf(&mut t, "vb", n, ["c", "d", "a", "b"]);
            lanewise(&mut t, scalar, n, "r", op, &["va", "vb"], "bfloat", "bfloat");
            lanewise(&mut t, scalar, n, "u", "fneg", &["r"], "bfloat", "bfloat");
            fold_lanes(&mut t, "r", n, "bfloat", 16);
            fold_lanes(&mut t, "u", n, "bfloat", 16);
            writeln!(t, "  %res = xor i64 %hr, %hu").unwrap();
            def(&mut body, format!("bf_{op}_{n}"), &t, "%res");
        }
        let mut t = String::new();
        build_bf(&mut t, "va", n, ["a", "b", "c", "d"]);
        build_bf(&mut t, "vb", n, ["a", "d", "c", "b"]);
        let preds = [
            "false", "oeq", "ogt", "oge", "olt", "ole", "one", "ord", "ueq", "ugt", "uge", "ult",
            "ule", "une", "uno", "true",
        ];
        let mut acc = "0".to_owned();
        for (k, pred) in preds.iter().enumerate() {
            lanewise(
                &mut t,
                scalar,
                n,
                &format!("m{k}"),
                &format!("fcmp {pred}"),
                &["va", "vb"],
                "bfloat",
                "i1",
            );
            writeln!(
                t,
                "  %mb{k} = bitcast <{n} x i1> %m{k} to i{n}
  %mz{k} = zext i{n} %mb{k} to i64
  %mm{k} = mul i64 {acc}, 131
  %acc{k} = add i64 %mm{k}, %mz{k}"
            )
            .unwrap();
            acc = format!("%acc{k}");
        }
        def(&mut body, format!("bf_fcmp_{n}"), &t, &acc);
        // fpext and fptrunc, with `float` and `double` lanes.
        let mut t = String::new();
        build_bf(&mut t, "va", n, ["a", "b", "c", "d"]);
        lanewise(&mut t, scalar, n, "ef", "fpext", &["va"], "bfloat", "float");
        lanewise(&mut t, scalar, n, "ed", "fpext", &["va"], "bfloat", "double");
        // Floats and doubles made of the arguments' bits.
        let mut fprev = "poison".to_owned();
        let mut dprev = "poison".to_owned();
        let words = ["a", "b", "c", "d", "e", "f"];
        for i in 0..n {
            let (fnext, dnext) = if i + 1 == n {
                ("%fv".to_owned(), "%dv".to_owned())
            } else {
                (format!("%fv.{i}"), format!("%dv.{i}"))
            };
            writeln!(
                t,
                "  %fw{i} = lshr i64 %{w}, {s}
  %ft{i} = trunc i64 %fw{i} to i32
  %fb{i} = bitcast i32 %ft{i} to float
  {fnext} = insertelement <{n} x float> {fprev}, float %fb{i}, i32 {i}
  %dr{i} = call i64 @llvm.fshl.i64(i64 %{w}, i64 %{w}, i64 {r})
  %db{i} = bitcast i64 %dr{i} to double
  {dnext} = insertelement <{n} x double> {dprev}, double %db{i}, i32 {i}",
                w = words[i as usize % 6],
                s = 32 * ((i / 6) % 2),
                r = 7 * (i / 6)
            )
            .unwrap();
            fprev = fnext;
            dprev = dnext;
        }
        lanewise(&mut t, scalar, n, "tf", "fptrunc", &["fv"], "float", "bfloat");
        lanewise(&mut t, scalar, n, "td", "fptrunc", &["dv"], "double", "bfloat");
        fold_lanes(&mut t, "ef", n, "float", 32);
        fold_lanes(&mut t, "ed", n, "double", 64);
        fold_lanes(&mut t, "tf", n, "bfloat", 16);
        fold_lanes(&mut t, "td", n, "bfloat", 16);
        writeln!(
            t,
            "  %r1 = xor i64 %hef, %hed\n  %r2 = add i64 %r1, %htf\n  %res = xor i64 %r2, %htd"
        )
        .unwrap();
        def(&mut body, format!("bf_cast_{n}"), &t, "%res");
        // Lanes moved: shuffle, select on a mask, insert and extract.
        let mut t = String::new();
        build_bf(&mut t, "va", n, ["a", "b", "c", "d"]);
        build_bf(&mut t, "vb", n, ["d", "c", "b", "a"]);
        let mask: Vec<String> = (0..n).map(|i| format!("i32 {}", (3 * i + 1) % (2 * n))).collect();
        writeln!(
            t,
            "  %sh = shufflevector <{n} x bfloat> %va, <{n} x bfloat> %vb, <{n} x i32> <{}>
  %ei = urem i64 %e, {n}
  %ins = insertelement <{n} x bfloat> %sh, bfloat 0xR3F80, i64 %ei
  %fb = trunc i64 %f to i{n}
  %fm = bitcast i{n} %fb to <{n} x i1>
  %sel = select <{n} x i1> %fm, <{n} x bfloat> %ins, <{n} x bfloat> %va
  %ex = extractelement <{n} x bfloat> %sel, i64 %ei
  %exi = bitcast bfloat %ex to i16
  %exz = zext i16 %exi to i64",
            mask.join(", ")
        )
        .unwrap();
        fold_lanes(&mut t, "sel", n, "bfloat", 16);
        writeln!(t, "  %res = add i64 %hsel, %exz").unwrap();
        def(&mut body, format!("bf_move_{n}"), &t, "%res");
    }
    body.push_str("declare i64 @llvm.fshl.i64(i64, i64, i64)\n");
    (body, names)
}

/// Random words with `bfloat` NaNs, infinities, zeros and denormals in
/// their 16-bit pieces, and `float` ones in their halves.
fn bf_word(rng: &mut Rng) -> u64 {
    const PIECES: [u16; 10] =
        [0x7fc0, 0x7f81, 0xffff, 0x7f80, 0xff80, 0x0000, 0x8000, 0x0001, 0x3f80, 0x7f7f];
    let mut w = rng.word();
    for k in 0..4 {
        let r = rng.next();
        if r.is_multiple_of(3) {
            let piece = u64::from(PIECES[(r >> 8) as usize % PIECES.len()]);
            w = (w & !(0xffff << (16 * k))) | piece << (16 * k);
        }
    }
    w
}

/// Twelve calls of each of `names` with [`bf_word`]s.
fn bf_calls(names: &[String], seed: u64) -> Vec<Call> {
    let mut rng = Rng(seed);
    let mut calls = Vec::new();
    for name in names {
        for _ in 0..12 {
            let args = [(); 6].map(|()| bf_word(&mut rng));
            calls.push((name.clone(), args));
        }
    }
    calls
}

/// What `llc` 22 and 19 (which must agree) compute for `calls` of
/// `body`, or `None` without the tools.
fn llc_reference(label: &str, body: &str, calls: &[Call]) -> Option<String> {
    let dir = hod_testutil::Scratch::new(label);
    let toolchain = Toolchain::host(&dir)?;
    let driver = toolchain.compile_c(&dir, "driver", &driver(calls), &[]);
    let mut reference: Option<String> = None;
    for major in [22, 19] {
        let Some(object) = llc_build(&dir, major, &format!("llc{major}"), body) else { continue };
        let exe = dir.join(format!("llc{major}"));
        let inputs = [driver.as_path(), object.as_path(), std::path::Path::new("-lm")];
        let ran = toolchain.link_and_run(&toolchain.linkers[0], &[], &inputs, &exe);
        assert!(ran.success(), "{}", ran.stderr_text());
        let got = ran.stdout_text();
        let reference = reference.get_or_insert_with(|| got.clone());
        assert!(&got == reference, "{label}: llc {major} differs from llc 22");
    }
    reference
}

/// `llc -O0` 19 and 22 miscompile arithmetic on `bfloat` vectors without
/// `+bf16`: the rounding reads bit 16 of an operand's image instead of
/// the result's (`ushr v1.4s, v0.4s, #16` of the loaded operand), so a
/// product or sum whose low half is a tie or `0x7fff` comes out one ulp
/// off. hod rounds each lane as the scalar `bfloat` operation does; the
/// reference is `llc`'s build of the same functions written lane by lane
/// on scalars.
#[test]
fn bfloat_lanes_agree_with_llc_on_scalars() {
    let counts = [2, 3, 4, 5, 8, 16];
    let (body, names) = bfloat_table(&counts, false);
    let (reference, _) = bfloat_table(&counts, true);
    let calls = bf_calls(&names, 0x0101_bf16);
    let Some(want) = llc_reference("a64-tail3-bfloat-ref", &reference, &calls) else { return };
    run_hod("a64-tail3-bfloat", &body, &calls, &want);
}

#[test]
fn bfloat_lane_refusals_are_named() {
    assert_refused(&[
        (
            "define void @f(ptr %p) {
  %a = load <4 x i32>, ptr %p
  %r = sitofp <4 x i32> %a to <4 x bfloat>
  store <4 x bfloat> %r, ptr %p
  ret void
}",
            tail3::BFLOAT_CAST,
        ),
        (
            "define void @f(ptr %p) {
  %a = load <4 x bfloat>, ptr %p
  %r = fptoui <4 x bfloat> %a to <4 x i16>
  store <4 x i16> %r, ptr %p
  ret void
}",
            tail3::BFLOAT_CAST,
        ),
    ]);
}

// ---- the `bfloat` NEON intrinsics ----

/// `%name` = `<n x float>` whose lanes are halves of the arguments.
fn build_f(out: &mut String, name: &str, n: u32, words: [&str; 2]) {
    let mut prev = "poison".to_owned();
    for i in 0..n {
        let next = if i + 1 == n { format!("%{name}") } else { format!("%{name}.{i}") };
        writeln!(
            out,
            "  %{name}.w{i} = lshr i64 %{}, {}
  %{name}.t{i} = trunc i64 %{name}.w{i} to i32
  %{name}.b{i} = bitcast i32 %{name}.t{i} to float
  {next} = insertelement <{n} x float> {prev}, float %{name}.b{i}, i32 {i}",
            words[(i as usize / 2) % 2],
            32 * (i % 2)
        )
        .unwrap();
        prev = next;
    }
}

/// The table of the seven intrinsics, and its names.
fn bf16_intrinsics() -> (String, Vec<String>) {
    let mut body = String::from(
        "declare bfloat @llvm.aarch64.neon.bfcvt(float)
declare <8 x bfloat> @llvm.aarch64.neon.bfcvtn(<4 x float>)
declare <8 x bfloat> @llvm.aarch64.neon.bfcvtn2(<8 x bfloat>, <4 x float>)
declare <2 x float> @llvm.aarch64.neon.bfdot.v2f32.v4bf16(<2 x float>, <4 x bfloat>, <4 x bfloat>)
declare <4 x float> @llvm.aarch64.neon.bfdot.v4f32.v8bf16(<4 x float>, <8 x bfloat>, <8 x bfloat>)
declare <4 x float> @llvm.aarch64.neon.bfmlalb(<4 x float>, <8 x bfloat>, <8 x bfloat>)
declare <4 x float> @llvm.aarch64.neon.bfmlalt(<4 x float>, <8 x bfloat>, <8 x bfloat>)
declare <4 x float> @llvm.aarch64.neon.bfmmla(<4 x float>, <8 x bfloat>, <8 x bfloat>)
",
    );
    let mut names = Vec::new();
    let mut def = |body: &mut String, name: &str, text: &str, result: &str| {
        writeln!(
            body,
            "define i64 @{name}({PARAMS}) \"target-features\"=\"+bf16\" {{\n{text}  ret i64 {result}\n}}\n"
        )
        .unwrap();
        names.push(name.to_owned());
    };
    let mut t = String::new();
    writeln!(
        t,
        "  %t = trunc i64 %a to i32
  %x = bitcast i32 %t to float
  %r = call bfloat @llvm.aarch64.neon.bfcvt(float %x)
  %rv = insertelement <1 x bfloat> poison, bfloat %r, i32 0"
    )
    .unwrap();
    fold_lanes(&mut t, "rv", 1, "bfloat", 16);
    def(&mut body, "i_bfcvt", &t, "%hrv");
    let mut t = String::new();
    build_f(&mut t, "vf", 4, ["a", "b"]);
    build_f(&mut t, "vg", 4, ["c", "d"]);
    writeln!(
        t,
        "  %lo = call <8 x bfloat> @llvm.aarch64.neon.bfcvtn(<4 x float> %vf)
  %hi = call <8 x bfloat> @llvm.aarch64.neon.bfcvtn2(<8 x bfloat> %lo, <4 x float> %vg)
  %lo2 = call <8 x bfloat> @llvm.aarch64.neon.bfcvtn(<4 x float> %vg)"
    )
    .unwrap();
    fold_lanes(&mut t, "hi", 8, "bfloat", 16);
    fold_lanes(&mut t, "lo2", 8, "bfloat", 16);
    writeln!(t, "  %res = xor i64 %hhi, %hlo2").unwrap();
    def(&mut body, "i_bfcvtn", &t, "%res");
    for (name, call, n) in [
        ("i_bfdot2", "<2 x float> @llvm.aarch64.neon.bfdot.v2f32.v4bf16", 2),
        ("i_bfdot4", "<4 x float> @llvm.aarch64.neon.bfdot.v4f32.v8bf16", 4),
        ("i_bfmlalb", "<4 x float> @llvm.aarch64.neon.bfmlalb", 4),
        ("i_bfmlalt", "<4 x float> @llvm.aarch64.neon.bfmlalt", 4),
        ("i_bfmmla", "<4 x float> @llvm.aarch64.neon.bfmmla", 4),
    ] {
        let m = 2 * n;
        let mut t = String::new();
        build_f(&mut t, "acc", n, ["e", "f"]);
        build_bf(&mut t, "x", m, ["a", "b", "c", "d"]);
        build_bf(&mut t, "y", m, ["c", "d", "b", "a"]);
        writeln!(
            t,
            "  %r = call {call}(<{n} x float> %acc, <{m} x bfloat> %x, <{m} x bfloat> %y)
  %s = call {call}(<{n} x float> %r, <{m} x bfloat> %x, <{m} x bfloat> %x)"
        )
        .unwrap();
        fold_lanes(&mut t, "r", n, "float", 32);
        fold_lanes(&mut t, "s", n, "float", 32);
        writeln!(t, "  %res = xor i64 %hr, %hs").unwrap();
        def(&mut body, name, &t, "%res");
    }
    (body, names)
}

#[test]
fn bf16_intrinsics_agree_with_llc() {
    let (body, names) = bf16_intrinsics();
    let calls = bf_calls(&names, 0x0101_bfd0);
    run("a64-tail3-bf16-intrinsics", &[&body], &calls, None);
}

// ---- a number minus an address ----

/// Functions whose operands are a number minus an address
/// (`sub (i64 k, ptrtoint @s)`), each adding the address back at run
/// time so that the result does not depend on where the program loads:
/// a local array (direct), an external function (through the GOT under
/// PIC), an offset into the array, and the forms clang and GCC's torture
/// suite write (`and (sub 0, @s), 63`).
fn neg_addr_body() -> (String, Vec<String>) {
    let body = "@nb = internal global [64 x i8] zeroinitializer, align 64
@ng = global [64 x i8] zeroinitializer, align 64
declare i32 @puts(ptr)
define i64 @na(i64 %a) {
  %s = ptrtoint ptr @nb to i64
  %x = add i64 sub (i64 4, i64 ptrtoint (ptr @nb to i64)), %s
  %r = add i64 %x, %a
  ret i64 %r
}
define i64 @nc(i64 %a) {
  %s = ptrtoint ptr @ng to i64
  %x = and i64 sub nsw (i64 0, i64 ptrtoint (ptr getelementptr (i8, ptr @ng, i64 5) to i64)), 63
  %y = add i64 sub (i64 4, i64 ptrtoint (ptr getelementptr (i8, ptr @ng, i64 12) to i64)), %s
  %r = add i64 %x, %y
  %q = xor i64 %r, %a
  ret i64 %q
}
define i64 @nd(i64 %a) {
  %s = ptrtoint ptr @puts to i64
  %x = add i64 add (i64 sub (i64 123456789012, i64 ptrtoint (ptr @puts to i64)), i64 -5), %s
  %r = mul i64 %x, %a
  ret i64 %r
}
define i64 @ne(i64 %a) {
  %s = ptrtoint ptr @ng to i64
  %x = add i64 sub (i64 sub (i64 0, i64 ptrtoint (ptr getelementptr (i8, ptr @ng, i64 33) to i64)), i64 -70000000000), %s
  %c = icmp ult i64 %a, sub (i64 -1, i64 ptrtoint (ptr @nb to i64))
  %z = zext i1 %c to i64
  %r = add i64 %x, %z
  ret i64 %r
}
";
    (body.to_owned(), ["na", "nc", "nd", "ne"].map(str::to_owned).to_vec())
}

#[test]
fn numbers_minus_addresses_agree_with_llc() {
    let (body, names) = neg_addr_body();
    let calls = calls_of(&names, &mut Rng(0x0101_5ab0), 8);
    run("a64-tail3-neg-addr", &[&body], &calls, None);
}

/// Data and an aggregate operand's image have no relocation for a
/// number minus an address (`llc` reports "Cannot represent a difference
/// across sections" for data); a thread-local symbol is no constant.
#[test]
fn numbers_minus_addresses_in_images_are_refused() {
    let classify = |text: &str| {
        let (module, bodies) = parse(&format!("{}{text}", ll_header(22)));
        let module_pass = hod_ir::classify(&module).err();
        let body_pass = bodies.iter().find_map(|b| hod_ir::classify_body(&module, b).err());
        module_pass.or(body_pass).map(|r| r.construct.into_owned())
    };
    let cases = [
        (
            "@a = global i64 0
@g = global i64 sub (i64 4, i64 ptrtoint (ptr @a to i64))",
            "init:neg-addr",
        ),
        (
            "@a = global i64 0
define void @f(ptr %p) {
  store <2 x i64> <i64 1, i64 sub (i64 4, i64 ptrtoint (ptr @a to i64))>, ptr %p
  ret void
}",
            "init:neg-addr",
        ),
        (
            "@t = thread_local global i64 0
define i64 @f() {
  ret i64 sub (i64 4, i64 ptrtoint (ptr @t to i64))
}",
            "tls:address-in-constant",
        ),
        (
            "@a = global i64 0
define i64 @f() {
  ret i64 sub (i64 4, i64 ptrtoint (ptr getelementptr (i8, ptr @a, i64 3) to i64))
}",
            "",
        ),
        (
            "@a = global i64 0
define i64 @f() {
  ret i64 add (i64 4, i64 ptrtoint (ptr @a to i64))
}",
            "",
        ),
        (
            "@a = global i64 0
define i64 @f() {
  ret i64 sub (i64 ptrtoint (ptr @a to i64), i64 ptrtoint (ptr @a to i64))
}",
            "",
        ),
        (
            "@a = global i64 0
define i64 @f() {
  ret i64 sub (i64 0, i64 sub (i64 ptrtoint (ptr @a to i64), i64 ptrtoint (ptr @b to i64)))
}
@b = global i64 0",
            "",
        ),
        (
            "@a = global i64 0
define i64 @f() {
  ret i64 sub (i64 0, i64 add (i64 ptrtoint (ptr @a to i64), i64 ptrtoint (ptr @a to i64)))
}",
            "const-expr:sub",
        ),
        (
            "@a = global i64 0
define i32 @f() {
  ret i32 sub (i32 0, i32 ptrtoint (ptr @a to i32))
}",
            "const-expr:sub",
        ),
    ];
    for (text, want) in cases {
        assert_eq!(classify(text).unwrap_or_default(), want, "{text}");
    }
}

// ---- preserve_allcc ----

/// `@pa` is `preserve_allcc` and calls `clobber` (C, writing every
/// register a `ccc` callee may) and does vector work of its own; `@cpa`
/// calls it as `preserve_allcc`.
const PRESERVE_ALL: &str = "declare void @clobber()
define preserve_allcc i64 @pa(i64 %a, i64 %b, <2 x double> %v) {
  call void @clobber()
  %w = fadd <2 x double> %v, %v
  %x = bitcast <2 x double> %w to <2 x i64>
  %e = extractelement <2 x i64> %x, i32 1
  %s = add i64 %a, %b
  %r = xor i64 %s, %e
  call void @clobber()
  ret i64 %r
}
define i64 @cpa(i64 %a, i64 %b) {
  %r = call preserve_allcc i64 @pa(i64 %a, i64 %b, <2 x double> <double 1.0, double 2.0>)
  %q = add i64 %r, 1
  ret i64 %q
}
";

/// The C side: `clobber` writes `x9`..`x15`, `x16`, `x17` and every
/// vector register; `check` puts known values in `x9`..`x15` and
/// `q8`..`q31`, calls `pa` by `bl` and compares them afterwards.
const PRESERVE_ALL_C: &str = r#"#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <unwind.h>
uint64_t cpa(uint64_t, uint64_t);
void pa(void); /* called only from the assembly below */
/* What the unwinder finds in `x9`..`x15` in the frame that called `pa`. */
static uint64_t seen[7];
static int after_pa;
static _Unwind_Reason_Code step(struct _Unwind_Context *ctx, void *arg) {
  (void)arg;
  if (after_pa) {
    for (int r = 0; r < 7; r++) seen[r] = _Unwind_GetGR(ctx, 9 + r);
    after_pa = 0;
    return _URC_END_OF_STACK;
  }
  after_pa = _Unwind_FindEnclosingFunction((void *)_Unwind_GetIP(ctx)) == (void *)pa;
  return _URC_NO_REASON;
}
__attribute__((noinline)) void clobber(void) {
  /* The second call finds `x9`..`x15` clobbered by the first: only
     `pa`'s rules say where the caller's values are. */
  _Unwind_Backtrace(step, 0);
  __asm__ volatile(
    "mov x9, #-1\n mov x10, #-1\n mov x11, #-1\n mov x12, #-1\n mov x13, #-1\n"
    "mov x14, #-1\n mov x15, #-1\n mov x16, #-1\n mov x17, #-1\n"
    "movi v0.16b, #0xee\n mov v1.16b, v0.16b\n mov v2.16b, v0.16b\n mov v3.16b, v0.16b\n"
    "mov v4.16b, v0.16b\n mov v5.16b, v0.16b\n mov v6.16b, v0.16b\n mov v7.16b, v0.16b\n"
    "mov v16.16b, v0.16b\n mov v17.16b, v0.16b\n mov v18.16b, v0.16b\n mov v19.16b, v0.16b\n"
    "mov v20.16b, v0.16b\n mov v21.16b, v0.16b\n mov v22.16b, v0.16b\n mov v23.16b, v0.16b\n"
    "mov v24.16b, v0.16b\n mov v25.16b, v0.16b\n mov v26.16b, v0.16b\n mov v27.16b, v0.16b\n"
    "mov v28.16b, v0.16b\n mov v29.16b, v0.16b\n mov v30.16b, v0.16b\n mov v31.16b, v0.16b\n"
    "mov v8.d[1], v0.d[0]\n mov v9.d[1], v0.d[0]\n mov v10.d[1], v0.d[0]\n mov v11.d[1], v0.d[0]\n"
    "mov v12.d[1], v0.d[0]\n mov v13.d[1], v0.d[0]\n mov v14.d[1], v0.d[0]\n mov v15.d[1], v0.d[0]\n"
    ::: "x9", "x10", "x11", "x12", "x13", "x14", "x15", "x16", "x17",
        "v0", "v1", "v2", "v3", "v4", "v5", "v6", "v7", "v16", "v17", "v18", "v19",
        "v20", "v21", "v22", "v23", "v24", "v25", "v26", "v27", "v28", "v29", "v30", "v31",
        "memory");
}
static int check(uint64_t a, uint64_t b, uint64_t want) {
  uint64_t xin[8], xout[8], res;
  uint8_t vin[24 * 16], vout[24 * 16];
  for (int i = 0; i < 8; i++) xin[i] = 0x0909090909090909ULL * (uint64_t)(i + 1) + a;
  for (int i = 0; i < 24 * 16; i++) vin[i] = (uint8_t)(i * 7 + 3 + b);
  double two[2] = {1.5, -2.25};
  __asm__ volatile(
    "ldp x9, x10, [%[xin]]\n ldp x11, x12, [%[xin], #16]\n ldp x13, x14, [%[xin], #32]\n"
    "ldr x15, [%[xin], #48]\n"
    "ldp q8, q9, [%[vin]]\n ldp q10, q11, [%[vin], #32]\n ldp q12, q13, [%[vin], #64]\n"
    "ldp q14, q15, [%[vin], #96]\n ldp q16, q17, [%[vin], #128]\n ldp q18, q19, [%[vin], #160]\n"
    "ldp q20, q21, [%[vin], #192]\n ldp q22, q23, [%[vin], #224]\n ldp q24, q25, [%[vin], #256]\n"
    "ldp q26, q27, [%[vin], #288]\n ldp q28, q29, [%[vin], #320]\n ldp q30, q31, [%[vin], #352]\n"
    "mov x0, %[a]\n mov x1, %[b]\n ldr q0, [%[two]]\n"
    "bl pa\n"
    "str x0, [%[res]]\n"
    "stp x9, x10, [%[xout]]\n stp x11, x12, [%[xout], #16]\n stp x13, x14, [%[xout], #32]\n"
    "str x15, [%[xout], #48]\n"
    "stp q8, q9, [%[vout]]\n stp q10, q11, [%[vout], #32]\n stp q12, q13, [%[vout], #64]\n"
    "stp q14, q15, [%[vout], #96]\n stp q16, q17, [%[vout], #128]\n stp q18, q19, [%[vout], #160]\n"
    "stp q20, q21, [%[vout], #192]\n stp q22, q23, [%[vout], #224]\n stp q24, q25, [%[vout], #256]\n"
    "stp q26, q27, [%[vout], #288]\n stp q28, q29, [%[vout], #320]\n stp q30, q31, [%[vout], #352]\n"
    :
    : [xin] "r"(xin), [vin] "r"(vin), [xout] "r"(xout), [vout] "r"(vout), [res] "r"(&res),
      [a] "r"(a), [b] "r"(b), [two] "r"(two)
    : "x0", "x1", "x2", "x3", "x4", "x5", "x6", "x7", "x8", "x9", "x10", "x11", "x12", "x13",
      "x14", "x15", "x16", "x17", "x18", "x30",
      "v0", "v1", "v2", "v3", "v4", "v5", "v6", "v7", "v8", "v9", "v10", "v11", "v12", "v13",
      "v14", "v15", "v16", "v17", "v18", "v19", "v20", "v21", "v22", "v23", "v24", "v25", "v26",
      "v27", "v28", "v29", "v30", "v31", "cc", "memory");
  if (memcmp(xin, xout, 7 * 8) != 0) { printf("general registers changed\n"); return 1; }
  if (memcmp(xin, seen, 7 * 8) != 0) { printf("the unwinder finds other x9..x15\n"); return 1; }
  if (memcmp(vin, vout, sizeof vin) != 0) {
    for (int i = 0; i < 24 * 16; i++)
      if (vin[i] != vout[i]) { printf("q%d changed\n", 8 + i / 16); return 1; }
  }
  if (res != want) { printf("pa gave %llx, want %llx\n", (unsigned long long)res, (unsigned long long)want); return 1; }
  return 0;
}
int main(void) {
  uint64_t three = 0xc012000000000000ULL; /* the bits of -4.5 */
  int bad = 0;
  for (uint64_t a = 0; a < 3; a++)
    bad |= check(a * 1000, a + 5, (a * 1000 + a + 5) ^ three);
  printf("%llu\n", (unsigned long long)cpa(7, 8));
  return bad;
}
"#;

/// A `preserve_allcc` function keeps `x9`..`x15` and the whole of
/// `q8`..`q31` for its caller (checked by C's inline assembly around a
/// `bl`), as `llc` 22 and 19 builds do; a `ccc` caller calls it.
#[test]
fn preserve_all_functions_keep_every_register_llc_keeps() {
    let dir = hod_testutil::Scratch::new("a64-tail3-preserve-all");
    let Some(toolchain) = Toolchain::host(&dir) else { return };
    let c = toolchain.compile_c(&dir, "check", PRESERVE_ALL_C, &[]);
    let mut builds: Vec<(String, PathBuf)> = Vec::new();
    for major in [22, 19] {
        if let Some(object) = llc_build(&dir, major, &format!("llc{major}"), PRESERVE_ALL) {
            builds.push((format!("llc{major}"), object));
        }
    }
    for (mode, options) in MODES {
        let (module, bodies) = parse(&format!("{}{PRESERVE_ALL}", ll_header(22)));
        let emitted = compile_module(&module, &bodies, &options)
            .unwrap_or_else(|e| panic!("preserve_all {mode}: {e}"));
        builds.push((mode.to_owned(), write_object(&dir, &format!("{mode}.o"), &emitted)));
    }
    let mut reference: Option<String> = None;
    for linker in &toolchain.linkers {
        for (name, object) in &builds {
            let exe = dir.join(format!("{name}-{}", linker.name));
            let ran = toolchain.link_and_run(linker, &[], &[c.as_path(), object.as_path()], &exe);
            assert!(ran.success(), "{name} under {}: {}", linker.name, ran.stdout_text());
            let got = ran.stdout_text();
            assert_eq!(&got, reference.get_or_insert_with(|| got.clone()), "{name}");
        }
    }
    // The unwinder above reads general registers only: the vector ones'
    // rules are compared with `llc`'s, register by register.
    let rules = builds.iter().filter_map(|(name, object)| Some((name, cfi_registers(object)?)));
    let rules: Vec<_> = rules.collect();
    for (name, set) in &rules {
        assert_eq!(set, &rules[0].1, "{name} describes other registers than {}", rules[0].0);
    }
}

/// The registers any CFI row of `object` gives a rule (`W9`, `B8`, ..),
/// by `llvm-dwarfdump`; `None` without it.
fn cfi_registers(object: &std::path::Path) -> Option<std::collections::BTreeSet<String>> {
    let tool = hod_testutil::find_tool("llvm-dwarfdump")?;
    let mut command = Command::new(tool);
    command.arg("--eh-frame").arg(object);
    let outcome = hod_testutil::run_with(&mut command, &[], Duration::from_mins(1));
    assert!(outcome.success(), "llvm-dwarfdump: {}", outcome.stderr_text());
    let text = outcome.stdout_text();
    let rows = text.lines().filter_map(|line| line.trim().split_once(": CFA=")).map(|(_, r)| r);
    let rules = rows.flat_map(|row| row.split(": ").nth(1).unwrap_or_default().split(", "));
    Some(rules.filter_map(|rule| Some(rule.split_once('=')?.0.to_owned())).collect())
}
