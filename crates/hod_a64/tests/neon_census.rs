//! The `<arm_neon.h>`/`<arm_fp16.h>`/`<arm_acle.h>` names WP-205 builds
//! (`spec/codegen.md` §5.6.34; the census of `data/neon_census`).
//!
//! Each case is a function `void @g<k>(ptr %in, ptr %out)`: operand `i`
//! is loaded from `%in + 32 * i`, a pointer operand is `%in + 128` (for a
//! load) or `%out` (for a store), and what the call returns is stored at
//! `%out`. A C driver fills `%in` from pools of edge bit patterns chosen
//! by each operand's element type (NaNs, infinities, denormals, ±2^31,
//! ±2^63, saturation bounds, halves), with every fourth round random,
//! calls each case [`ROUNDS`] times and prints a hash of `%out`. Every
//! case is compiled by hod (plain, every stress option, far mode) and by
//! `llc -O0 -global-isel=0 -fast-isel=0` of LLVM 19 and 22, linked under
//! every linker present, and every build must print what the first prints.
//! The half-to-`i16` conversions are checked against `llc` 22 alone (19
//! cannot select them); SM3/SM4, which the host lacks, run under
//! `qemu-aarch64 -cpu max`.

mod common;

use std::fmt::Write as _;
use std::path::Path;

use common::linking::Toolchain;
use common::{assert_refused, ll_header, parse, write_object};
use hod_a64::compile_module;
use hod_cg::{Options, RelocModel, Stress};

/// The rounds each case runs (the pools are at most 48 long; every fourth
/// round is random).
const ROUNDS: u32 = 64;

const DEFAULT: Options = Options {
    reloc_model: RelocModel::Static,
    code_model: None,
    function_sections: false,
    data_sections: false,
    stress: Stress { min_pool: false, far: false, displace: false, coro_frame: false },
    debug_compression: hod_obj::Compression::None,
    split_dwarf: hod_dwarf::SplitDwarf::Off,
};

const MODES: [(&str, Options); 3] = [
    ("plain", DEFAULT),
    ("stress", Options { stress: Stress::ALL, ..DEFAULT }),
    (
        "far",
        Options {
            stress: Stress { min_pool: false, far: true, displace: false, coro_frame: false },
            ..DEFAULT
        },
    ),
];

/// Where a group of cases runs and what it is checked against.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Group {
    /// On the host, against `llc` 19 and 22.
    Native,
    /// On the host, against `llc` 22 only.
    Llc22,
    /// Under `qemu-aarch64 -cpu max`, against `llc` 19 and 22.
    Emulated,
}

const GROUPS: [Group; 3] = [Group::Native, Group::Llc22, Group::Emulated];

/// The calls of every case, as `<ret> @<name>(<args>)`: an argument
/// `<ty> %` is loaded from the input, `ptr in`/`ptr out` are the input's
/// pointer region and the output, anything else is a constant.
fn cases() -> Vec<(Group, String)> {
    let mut out = Vec::new();
    let mut add = |group, call: String| out.push((group, call));
    // `vqshlub_n_s8`/`vqshluh_n_s16`: clang's shift has poison lanes.
    for (lanes, elem, bits) in [(8, "i8", 8), (4, "i16", 16)] {
        for shift in [0, 1, bits / 2, bits - 1] {
            let rest = format!(", {elem} poison").repeat(lanes - 1);
            add(
                Group::Native,
                format!(
                    "<{lanes} x {elem}> @llvm.aarch64.neon.sqshlu.v{lanes}{elem}(<{lanes} x {elem}> %, <{lanes} x {elem}> <{elem} {shift}{rest}>)"
                ),
            );
        }
    }
    // The half scalar conversions of `<arm_fp16.h>`.
    for op in ["as", "au", "ms", "mu", "ns", "nu", "ps", "pu", "zs", "zu"] {
        add(Group::Llc22, format!("i16 @llvm.aarch64.neon.fcvt{op}.i16.f16(half %)"));
        for int in ["i32", "i64"] {
            add(Group::Native, format!("{int} @llvm.aarch64.neon.fcvt{op}.{int}.f16(half %)"));
        }
    }
    for sign in ["s", "u"] {
        for int in ["i32", "i64"] {
            for fbits in [1, 8, 15, 16] {
                add(
                    Group::Native,
                    format!(
                        "{int} @llvm.aarch64.neon.vcvtfp2fx{sign}.{int}.f16(half %, i32 {fbits})"
                    ),
                );
                add(
                    Group::Native,
                    format!(
                        "half @llvm.aarch64.neon.vcvtfx{sign}2fp.f16.{int}({int} %, i32 {fbits})"
                    ),
                );
            }
        }
    }
    for op in ["fmax", "fmin", "fmaxnm", "fminnm"] {
        add(Group::Native, format!("half @llvm.aarch64.neon.{op}.f16(half %, half %)"));
    }
    // FEAT_FRINTTS and FEAT_JSCVT.
    for op in ["frint32x", "frint32z", "frint64x", "frint64z"] {
        add(Group::Native, format!("float @llvm.aarch64.{op}.f32(float %)"));
        add(Group::Native, format!("double @llvm.aarch64.{op}.f64(double %)"));
    }
    add(Group::Native, "i32 @llvm.aarch64.fjcvtzs(double %)".to_owned());
    // SM3 and SM4.
    let v = "<4 x i32> %";
    for op in ["sm3partw1", "sm3partw2", "sm3ss1"] {
        add(Group::Emulated, format!("<4 x i32> @llvm.aarch64.crypto.{op}({v}, {v}, {v})"));
    }
    for op in ["sm3tt1a", "sm3tt1b", "sm3tt2a", "sm3tt2b"] {
        for index in 0..4 {
            add(
                Group::Emulated,
                format!("<4 x i32> @llvm.aarch64.crypto.{op}({v}, {v}, {v}, i64 {index})"),
            );
        }
    }
    for op in ["sm4e", "sm4ekey"] {
        add(Group::Emulated, format!("<4 x i32> @llvm.aarch64.crypto.{op}({v}, {v})"));
    }
    // The bfloat structure loads and stores.
    for lanes in [4, 8] {
        let ty = format!("<{lanes} x bfloat>");
        let suffix = format!("v{lanes}bf16.p0");
        for n in 2..=4 {
            let ret = format!("{{ {} }}", vec![ty.as_str(); n].join(", "));
            let vs = vec![format!("{ty} %"); n].join(", ");
            for load in [format!("ld1x{n}"), format!("ld{n}"), format!("ld{n}r")] {
                add(Group::Native, format!("{ret} @llvm.aarch64.neon.{load}.{suffix}(ptr in)"));
            }
            for store in [format!("st1x{n}"), format!("st{n}")] {
                add(
                    Group::Native,
                    format!("void @llvm.aarch64.neon.{store}.{suffix}({vs}, ptr out)"),
                );
            }
            for lane in [0, lanes - 1] {
                add(
                    Group::Native,
                    format!(
                        "{ret} @llvm.aarch64.neon.ld{n}lane.{suffix}({vs}, i64 {lane}, ptr in)"
                    ),
                );
                add(
                    Group::Native,
                    format!(
                        "void @llvm.aarch64.neon.st{n}lane.{suffix}({vs}, i64 {lane}, ptr out)"
                    ),
                );
            }
        }
    }
    out
}

/// The parts of `text` between commas outside brackets.
fn split_top(text: &str) -> Vec<&str> {
    let (mut out, mut depth, mut start) = (Vec::new(), 0i32, 0);
    for (i, ch) in text.char_indices() {
        match ch {
            '<' | '{' | '(' => depth += 1,
            '>' | '}' | ')' => depth -= 1,
            ',' if depth == 0 => {
                out.push(text[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    if !text[start..].trim().is_empty() {
        out.push(text[start..].trim());
    }
    out
}

/// The driver's pool of an element type: `h` half (and bfloat), `s`
/// float, `d` double, `b`/`w`/`i`/`l` 8/16/32/64-bit integers.
fn kind(ty: &str) -> char {
    if ty.contains("half") || ty.contains("bfloat") {
        'h'
    } else if ty.contains("float") {
        's'
    } else if ty.contains("double") {
        'd'
    } else if ty.contains("i8") {
        'b'
    } else if ty.contains("i16") {
        'w'
    } else if ty.contains("i32") {
        'i'
    } else {
        'l'
    }
}

/// The function `@g<k>` of `call`, its declaration, and the pools of its
/// four operand slots and pointer region.
fn case(k: usize, call: &str) -> (String, String, String) {
    let (ret, callee) = call.split_once(" @").expect("a call");
    let (name, args) = callee.split_once('(').expect("arguments");
    let args = split_top(args.strip_suffix(')').expect("arguments"));
    let mut body = format!("define void @g{k}(ptr %in, ptr %out) #0 {{\n");
    let (mut types, mut values) = (Vec::new(), Vec::new());
    let mut kinds = String::new();
    for (i, arg) in args.iter().enumerate() {
        if let Some(ty) = arg.strip_suffix(" %") {
            let slot = kinds.len();
            writeln!(body, "  %q{i} = getelementptr i8, ptr %in, i64 {}", 32 * slot).unwrap();
            writeln!(body, "  %x{i} = load {ty}, ptr %q{i}, align 1").unwrap();
            values.push(format!("{ty} %x{i}"));
            types.push(ty);
            kinds.push(kind(ty));
        } else if *arg == "ptr in" {
            writeln!(body, "  %x{i} = getelementptr i8, ptr %in, i64 128").unwrap();
            values.push(format!("ptr %x{i}"));
            types.push("ptr");
        } else if *arg == "ptr out" {
            values.push("ptr %out".to_owned());
            types.push("ptr");
        } else {
            let (ty, _) = arg.split_once(" <").or_else(|| arg.rsplit_once(' ')).unwrap();
            values.push((*arg).to_owned());
            types.push(ty);
        }
    }
    while kinds.len() < 4 {
        kinds.push('l');
    }
    // The pointer region holds what the loads read: bfloat.
    kinds.push(if name.contains("bf16") { 'h' } else { 'l' });
    let call = format!("@{name}({})", values.join(", "));
    if ret == "void" {
        writeln!(body, "  call void {call}").unwrap();
    } else if let Some(fields) = ret.strip_prefix("{ ").and_then(|r| r.strip_suffix(" }")) {
        writeln!(body, "  %res = call {ret} {call}").unwrap();
        for (j, field) in split_top(fields).into_iter().enumerate() {
            writeln!(
                body,
                "  %f{j} = extractvalue {ret} %res, {j}\n  \
                 %o{j} = getelementptr i8, ptr %out, i64 {}\n  \
                 store {field} %f{j}, ptr %o{j}, align 1",
                16 * j
            )
            .unwrap();
        }
    } else {
        writeln!(body, "  %res = call {ret} {call}\n  store {ret} %res, ptr %out, align 1")
            .unwrap();
    }
    body += "  ret void\n}\n";
    (body, format!("declare {ret} @{name}({})\n", types.join(", ")), kinds)
}

const FEATURES: &str =
    "attributes #0 = { \"target-features\"=\"+neon,+fullfp16,+bf16,+sm4,+v8.5a,+jsconv\" }\n";

/// The module of `group`'s cases (of case `only` alone, if given) without
/// the header, and their names, calls and pools.
fn module(group: Group, only: Option<usize>) -> (String, Vec<(String, String, String)>) {
    let mut text = String::from(FEATURES);
    let mut declared = std::collections::HashSet::new();
    let mut names = Vec::new();
    for (k, (g, call)) in cases().into_iter().enumerate() {
        if g != group || only.is_some_and(|o| o != k) {
            continue;
        }
        let (body, declaration, kinds) = case(k, &call);
        if declared.insert(declaration.clone()) {
            text += &declaration;
        }
        text += &body;
        names.push((format!("g{k}"), call, kinds));
    }
    (text, names)
}

/// The C driver of `names`: it prints one hash per case and round.
fn driver(names: &[(String, String, String)]) -> String {
    let mut c = String::from(DRIVER_HEAD);
    for (name, _, _) in names {
        writeln!(c, "void {name}(uint8_t *, uint8_t *);").unwrap();
    }
    c +=
        "static const struct { void (*f)(uint8_t *, uint8_t *); const char *kinds; } cases[] = {\n";
    for (name, _, kinds) in names {
        writeln!(c, "  {{ {name}, \"{kinds}\" }},").unwrap();
    }
    write!(c, "}};\n#define ROUNDS {ROUNDS}\n{DRIVER_MAIN}").unwrap();
    c
}

const DRIVER_HEAD: &str = r"#include <stdio.h>
#include <stdint.h>
#include <string.h>

static const uint64_t H[] = { 0x0000, 0x8000, 0x3c00, 0xbc00, 0x3800, 0xb800, 0x3e00, 0xbe00,
  0x4100, 0xc100, 0x4500, 0x7bff, 0xfbff, 0x7c00, 0xfc00, 0x7e00, 0xfe00, 0x7d00, 0x0001,
  0x8001, 0x03ff, 0x0400, 0x7800, 0xf800, 0x77ff, 0x5c00, 0xdc00, 0x3555, 0x4248, 0x3bff,
  0x7a00, 0xfa00, 0x6400, 0x1400 };
static const uint64_t S[] = { 0x00000000, 0x80000000, 0x3f800000, 0xbf800000, 0x3f000000,
  0xbf000000, 0x3fc00000, 0x40200000, 0xc0200000, 0x4f000000, 0xcf000000, 0x4effffff,
  0xcf000001, 0x5f000000, 0xdf000000, 0x5effffff, 0x7f800000, 0xff800000, 0x7fc00000,
  0xffc00000, 0x7fa00000, 0x00000001, 0x80800000, 0x7f7fffff, 0x4b000001, 0x4b400001,
  0x3effffff, 0x3f7fffff, 0x47000000, 0xc7000000, 0x5f800000, 0x4f800000 };
static const uint64_t D[] = { 0x0000000000000000ULL, 0x8000000000000000ULL,
  0x3ff0000000000000ULL, 0xbff0000000000000ULL, 0x3fe0000000000000ULL, 0xbfe0000000000000ULL,
  0x3ff8000000000000ULL, 0x4004000000000000ULL, 0xc004000000000000ULL, 0x41e0000000000000ULL,
  0x41dfffffffc00000ULL, 0xc1e0000000000000ULL, 0xc1e0000000200000ULL, 0x41f0000000000000ULL,
  0x41f0000000080000ULL, 0x43e0000000000000ULL, 0xc3e0000000000000ULL, 0x43dfffffffffffffULL,
  0x7ff0000000000000ULL, 0xfff0000000000000ULL, 0x7ff8000000000000ULL, 0x7ff4000000000000ULL,
  0x0000000000000001ULL, 0x7fefffffffffffffULL, 0x4330000000000001ULL, 0x4202a05f20000000ULL,
  0xc202a05f20000000ULL, 0x44b52d02c7e14af6ULL, 0x3fdfffffffffffffULL, 0x41e0000000100000ULL,
  0xc1dfffffffe00000ULL, 0x4340000000000001ULL };
static const uint64_t B[] = { 0, 1, 0xff, 0x80, 0x7f, 0x81, 0x7e, 0x40, 0x3f, 0x20, 0xc0, 0xfe,
  0x55, 0xaa, 0x10, 0x0f };
static const uint64_t W[] = { 0, 1, 0xffff, 0x8000, 0x7fff, 0x8001, 0x7ffe, 0x4000, 0x3fff,
  0xc000, 0x00ff, 0xff00, 0x5555, 0xaaaa, 0x0100, 0x1234 };
static const uint64_t I[] = { 0, 1, 0xffffffff, 0x80000000, 0x7fffffff, 0x80000001, 0x7ffffffe,
  0x0000ffff, 0x00010000, 0xffff0000, 0x00008000, 0xffff8000, 0x00007fff, 0x55555555,
  0xaaaaaaaa, 0x12345678, 0x40000000, 0xc0000000 };
static const uint64_t L[] = { 0, 1, 0xffffffffffffffffULL, 0x8000000000000000ULL,
  0x7fffffffffffffffULL, 0x8000000000000001ULL, 0x00000000ffffffffULL, 0xffffffff00000000ULL,
  0x0000000080000000ULL, 0xffffffff80000000ULL, 0x000000000000ffffULL, 0xffffffffffff8000ULL,
  0x0000000000007fffULL, 0x5555555555555555ULL, 0x0123456789abcdefULL, 0x4000000000000000ULL };

static uint64_t state = 0x0205020502050205ULL;
static uint64_t next(void) {
  state ^= state >> 12;
  state ^= state << 25;
  state ^= state >> 27;
  return state * 0x2545f4914f6cdd1dULL;
}

static void fill(uint8_t *p, char kind, unsigned round, unsigned slot) {
  const uint64_t *pool;
  unsigned n, size;
  switch (kind) {
  case 'h': pool = H; n = sizeof H / 8; size = 2; break;
  case 's': pool = S; n = sizeof S / 8; size = 4; break;
  case 'd': pool = D; n = sizeof D / 8; size = 8; break;
  case 'b': pool = B; n = sizeof B / 8; size = 1; break;
  case 'w': pool = W; n = sizeof W / 8; size = 2; break;
  case 'i': pool = I; n = sizeof I / 8; size = 4; break;
  default: pool = L; n = sizeof L / 8; size = 8; break;
  }
  for (unsigned j = 0; j < (slot == 4 ? 128 : 32) / size; j++) {
    uint64_t v = round % 4 == 3 ? next() : pool[(round - round / 4 + j * 7 + slot * 11) % n];
    memcpy(p + j * size, &v, size);
  }
}

";

const DRIVER_MAIN: &str = r#"
int main(void) {
  static uint8_t in[256], out[256];
  for (unsigned c = 0; c < sizeof cases / sizeof cases[0]; c++) {
    for (unsigned round = 0; round < ROUNDS; round++) {
      for (unsigned slot = 0; slot < 5; slot++)
        fill(in + 32 * slot, cases[c].kinds[slot], round, slot);
      memset(out, 0, sizeof out);
      cases[c].f(in, out);
      uint64_t h = 0;
      for (unsigned i = 0; i < sizeof out; i += 8) {
        uint64_t w;
        memcpy(&w, out + i, 8);
        h = ((h << 13) | (h >> 51)) ^ w;
        h *= 0x9e3779b97f4a7c15ULL;
      }
      printf("%016llx\n", (unsigned long long)h);
    }
    fflush(stdout);
  }
  return 0;
}
"#;

/// Every case compiles in every mode, each by itself.
#[test]
fn every_case_compiles() {
    let mut failed = Vec::new();
    for (k, (group, call)) in cases().into_iter().enumerate() {
        let (text, _) = module(group, Some(k));
        let (module, bodies) = parse(&format!("{}{text}", ll_header(22)));
        for (mode, options) in &MODES {
            if let Err(e) = compile_module(&module, &bodies, options) {
                failed.push(format!("g{k} ({call}) {mode}: {e}"));
            }
        }
    }
    assert!(failed.is_empty(), "{} failures:\n{}", failed.len(), failed.join("\n"));
}

/// Runs `program`, under `qemu-aarch64 -cpu max` for [`Group::Emulated`]
/// on a native host.
fn run(
    toolchain: &Toolchain,
    linker: &common::linking::Linker,
    group: Group,
    exe: &Path,
) -> hod_testutil::Outcome {
    if group != Group::Emulated || toolchain.emulated() {
        return toolchain.run(linker, exe);
    }
    let Some(qemu) = hod_testutil::tool("qemu-aarch64") else {
        panic!("SM3/SM4 need qemu-aarch64, which the host lacks");
    };
    let mut command = std::process::Command::new(qemu);
    command.args(["-cpu", "max"]).arg(exe);
    hod_testutil::run_with(&mut command, &[], std::time::Duration::from_mins(5))
}

fn check_group(group: Group) {
    let (all, names) = module(group, None);
    let dir = hod_testutil::Scratch::new(&format!("a64-neon-census-{group:?}"));
    let Some(toolchain) = Toolchain::aarch64(&dir) else { return };
    let toolchain = toolchain.with_emulated_cpu("max");
    let driver = toolchain.compile_c(&dir, "driver", &driver(&names), &[]);
    let mut objects = Vec::new();
    let majors: &[u32] = if group == Group::Llc22 { &[22] } else { &[19, 22] };
    for &major in majors {
        let Some(llc) = hod_testutil::tool(&format!("llc-{major}")) else { continue };
        let input = dir.write(&format!("llc{major}.ll"), format!("{}{all}", ll_header(major)));
        let output = dir.join(format!("llc{major}.o"));
        let mut command = std::process::Command::new(llc);
        command
            .args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj"])
            .arg(&input)
            .arg("-o")
            .arg(&output);
        let outcome = hod_testutil::run_with(&mut command, &[], std::time::Duration::from_mins(5));
        assert!(outcome.success(), "llc-{major}: {}", outcome.stderr_text());
        objects.push((format!("llc-{major}"), output));
    }
    if objects.is_empty() {
        return;
    }
    let (module, bodies) = parse(&format!("{}{all}", ll_header(22)));
    for (name, options) in &MODES {
        let emitted =
            compile_module(&module, &bodies, options).unwrap_or_else(|e| panic!("{name}: {e}"));
        objects.push(((*name).to_owned(), write_object(&dir, &format!("{name}.o"), &emitted)));
    }
    let lines = names.len() * ROUNDS as usize;
    let mut reference: Option<(String, String)> = None;
    for linker in &toolchain.linkers {
        for (name, object) in &objects {
            let exe = dir.join(format!("{name}-{}", linker.name));
            toolchain.link_ok(linker, &[], &[&driver, object], &exe);
            let ran = run(&toolchain, linker, group, &exe);
            let got = ran.stdout_text();
            let at = got.lines().count() / ROUNDS as usize;
            assert!(
                ran.success(),
                "{name} under {}: {} died: {}",
                linker.name,
                names.get(at).map_or("", |n| n.1.as_str()),
                ran.stderr_text()
            );
            let (first, want) = reference.get_or_insert_with(|| (name.clone(), got.clone()));
            let wrong: Vec<String> = got
                .lines()
                .zip(want.lines())
                .enumerate()
                .filter(|(_, (g, w))| g != w)
                .map(|(i, (g, w))| {
                    let n = &names[i / ROUNDS as usize];
                    format!("@{} {} round {}: {g}, {first} {w}", n.0, n.1, i % ROUNDS as usize)
                })
                .collect();
            assert!(
                wrong.is_empty() && got.lines().count() == lines,
                "{name} under {}: {} rounds differ from {first}:\n{}",
                linker.name,
                wrong.len(),
                wrong.join("\n")
            );
        }
    }
    eprintln!(
        "neon census {group:?}: {} cases, {lines} rounds agree in {} builds under {} linkers",
        names.len(),
        objects.len(),
        toolchain.linkers.len()
    );
}

#[test]
fn native_cases_match_llc() {
    check_group(Group::Native);
}

#[test]
fn half_to_i16_matches_llc22() {
    check_group(Group::Llc22);
}

#[test]
fn sm3_sm4_match_llc_under_emulation() {
    check_group(Group::Emulated);
}

#[test]
fn every_group_has_cases() {
    for group in GROUPS {
        assert!(!module(group, None).1.is_empty(), "{group:?}");
    }
}

/// Immediates out of range stay refused by name.
#[test]
fn bad_immediates_are_refused() {
    let immediate = hod_a64::neon::IMMEDIATE;
    assert_refused(&[
        (
            "declare <4 x i32> @llvm.aarch64.crypto.sm3tt1a(<4 x i32>, <4 x i32>, <4 x i32>, i64)\n\
             define <4 x i32> @f(<4 x i32> %a, <4 x i32> %b, <4 x i32> %c) {\n  \
             %r = call <4 x i32> @llvm.aarch64.crypto.sm3tt1a(<4 x i32> %a, <4 x i32> %b, <4 x i32> %c, i64 4)\n  \
             ret <4 x i32> %r\n}\n",
            immediate,
        ),
        (
            "declare i32 @llvm.aarch64.neon.vcvtfp2fxs.i32.f16(half, i32)\n\
             define i32 @f(half %a) {\n  \
             %r = call i32 @llvm.aarch64.neon.vcvtfp2fxs.i32.f16(half %a, i32 0)\n  \
             ret i32 %r\n}\n",
            immediate,
        ),
    ]);
}
