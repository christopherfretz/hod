//! `llvm.modf` on x86-64 and AArch64 (WP-340, `spec/codegen.md` §5.5.1
//! "Calls", §5.6.12 and the x86-64 intrinsics), by execution: x86-64
//! under `qemu-x86_64` on this host, AArch64 natively.
//!
//! clang 22 writes `llvm.modf.*` for `modf`, `modff` and `modfl` (LLVM 19
//! does not know the intrinsic: this file is LLVM 22 only). The module is
//! clang 22's (`-O0 -Xclang -disable-llvm-passes`) of [`MODULE_C`], with
//! a `half` function appended on AArch64 (C has no `half` `modf`);
//! [`DRIVER_C`] calls it on zeros of both signs, integers, values with a
//! fractional part, the largest values, subnormals, infinities and NaNs
//! (quiet and signalling, with payloads), and for `x86_fp80` its odd
//! encodings, and prints the bits of both parts. hod (plain and every
//! function under `Stress::ALL`) and `llc` 22 (`-O0 -global-isel=0
//! -fast-isel=0`), static and PIC, print the same.

#![allow(dead_code, reason = "the linking support is shared with other tests")]

#[path = "../../hod_obj/tests/common/linking.rs"]
mod linking;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hod::{Options, OutputKind, RelocModel, Report, Settings};
use hod_testutil::{Scratch, Target, run_with, tool};
use linking::{Toolchain, exit_code};

/// The functions of the module: each type's `modf` through the pair.
const MODULE_C: &str = r"
double modf(double, double *);
float modff(float, float *);
long double modfl(long double, long double *);
void md(double x, double *f, double *i) { *f = modf(x, i); }
void mf(float x, float *f, float *i) { *f = modff(x, i); }
void ml(long double x, long double *f, long double *i) { *f = modfl(x, i); }
";

/// `modf` of `half` (AArch64): through `modff`, as `llc` 22.
const HALF_LL: &str = "
declare { half, half } @llvm.modf.f16(half)
define void @mh(half %x, ptr %f, ptr %i) {
  %r = call { half, half } @llvm.modf.f16(half %x)
  %a = extractvalue { half, half } %r, 0
  %b = extractvalue { half, half } %r, 1
  store half %a, ptr %f
  store half %b, ptr %i
  ret void
}
";

/// The driver: every value, both parts' bits in hex.
const DRIVER_C: &str = r#"
#include <stdio.h>
#include <stdint.h>
#include <string.h>
void md(double, double *, double *);
void mf(float, float *, float *);
void ml(long double, long double *, long double *);
static const uint64_t ds[] = {
  0, 1ull << 63, 0x3ff0000000000000ull, 0xbff8000000000000ull, 0x4005ffffffffffffull,
  0xc01c000000000000ull, 0x3fe0000000000000ull, 0xbfdfffffffffffffull,
  0x4330000000000001ull, 0x432fffffffffffffull, 0xc32fffffffffffffull, 0x4340000000000000ull,
  0x7fefffffffffffffull, 0xffefffffffffffffull, 1, 0x800fffffffffffffull,
  0x0010000000000000ull, 0x7ff0000000000000ull, 0xfff0000000000000ull,
  0x7ff8000000000000ull, 0xfff8000000000123ull, 0x7ff0000000000001ull, 0xfff4000000000abcull,
  0x400921fb54442d18ull, 0xc1d2a05f20000000ull};
static const uint32_t fs[] = {
  0, 0x80000000u, 0x3f800000u, 0xbfc00000u, 0x4b000001u, 0x4affffffu, 0xcaffffffu,
  0x7f7fffffu, 0xff7fffffu, 1, 0x807fffffu, 0x00800000u, 0x7f800000u, 0xff800000u,
  0x7fc00000u, 0xffc00123u, 0x7f800001u, 0xffa00abcu, 0x40490fdbu, 0x3effffffu};
#ifdef __x86_64__
#define LD_BYTES 10
/* x86_fp80: (sign and exponent, significand), the odd encodings included. */
static const struct { uint16_t se; uint64_t m; } ls[] = {
  {0x403e, 0xffffffffffffffffull}, {0xc03d, 0xffffffffffffffffull},
  {0x403f, 0x8000000000000001ull}, {0x3ffe, 0xffffffffffffffffull},
  {0x0000, 0x0000000000000001ull}, {0x8000, 0x7fffffffffffffffull},
  {0x0000, 0x8000000000000000ull}, {0x8000, 0xc000000000000000ull},
  {0x7ffe, 0xffffffffffffffffull}, {0x7fff, 0x8000000000000000ull},
  {0xffff, 0xc000000000000001ull}, {0x7fff, 0xa000000000000000ull},
  {0x4000, 0x4000000000000000ull}, {0x7fff, 0x0000000000000000ull},
  {0x7fff, 0x4000000000000000ull}};
#else
#define LD_BYTES 16
/* fp128: (high word, low word). */
static const struct { uint64_t hi, lo; } ls[] = {
  {0x406f000000000000ull, 1}, {0xc06effffffffffffull, ~0ull},
  {0x3ffe000000000000ull, 0}, {0x0000ffffffffffffull, ~0ull}, {0x8000000000000000ull, 1},
  {0x7ffeffffffffffffull, ~0ull}, {0x7fff000000000000ull, 0},
  {0xffff800000000000ull, 5}, {0x7fff400000000000ull, 0}, {0x4000921fb54442d1ull, 0x8469898cc51701b8ull}};
void mh(_Float16, _Float16 *, _Float16 *);
static const uint16_t hs[] = {0, 0x8000, 0x3c00, 0xbe00, 0x6401, 0x63ff, 0xe3ff, 0x7bff, 1,
  0x83ff, 0x0400, 0x7c00, 0xfc00, 0x7e00, 0xfe12, 0x7c01, 0xfd0b, 0x4248, 0x37ff};
#endif
static void bytes(const void *p, int n) {
  const unsigned char *b = p;
  for (int k = n - 1; k >= 0; k--) printf("%02x", b[k]);
  printf(" ");
}
int main(void) {
  for (unsigned k = 0; k < sizeof ds / sizeof ds[0]; k++) {
    double x, f, i;
    memcpy(&x, &ds[k], 8);
    md(x, &f, &i);
    bytes(&f, 8); bytes(&i, 8); printf("d\n");
    long double lx = x, lf, li;
    ml(lx, &lf, &li);
    bytes(&lf, LD_BYTES); bytes(&li, LD_BYTES); printf("l\n");
  }
  for (unsigned k = 0; k < sizeof fs / sizeof fs[0]; k++) {
    float x, f, i;
    memcpy(&x, &fs[k], 4);
    mf(x, &f, &i);
    bytes(&f, 4); bytes(&i, 4); printf("f\n");
  }
  for (unsigned k = 0; k < sizeof ls / sizeof ls[0]; k++) {
    long double x, f, i;
    unsigned char b[sizeof x];
    memset(b, 0, sizeof b);
#ifdef __x86_64__
    memcpy(b, &ls[k].m, 8);
    memcpy(b + 8, &ls[k].se, 2);
#else
    memcpy(b, &ls[k].lo, 8);
    memcpy(b + 8, &ls[k].hi, 8);
#endif
    memcpy(&x, b, sizeof x);
    ml(x, &f, &i);
    bytes(&f, LD_BYTES); bytes(&i, LD_BYTES); printf("L\n");
  }
#ifdef __aarch64__
  for (unsigned k = 0; k < sizeof hs / sizeof hs[0]; k++) {
    _Float16 x, f, i;
    memcpy(&x, &hs[k], 2);
    mh(x, &f, &i);
    bytes(&f, 2); bytes(&i, 2); printf("h\n");
  }
#endif
  return 0;
}
"#;

/// The targets of this file.
const TARGETS: [Target; 2] = [Target::X86_64, Target::Aarch64];

/// clang 22's module of [`MODULE_C`] for `target` (with [`HALF_LL`] on
/// AArch64); `None` without clang 22.
fn module(dir: &Scratch, target: Target) -> Option<String> {
    let clang = tool("clang-22")?;
    let source = dir.write("modf.c", MODULE_C);
    let mut command = Command::new(clang);
    command.arg(format!("--target={}", target.triple()));
    command.args(["-O0", "-Xclang", "-disable-llvm-passes", "-S", "-emit-llvm", "-o", "-"]);
    command.arg(source);
    let ran = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(ran.success(), "clang 22: {}", ran.stderr_text());
    let mut text = ran.stdout_text();
    assert_eq!(text.matches("@llvm.modf.").count(), 6, "{text}");
    assert!(text.contains("@llvm.modf.f64(") && text.contains("@llvm.modf.f32("), "{text}");
    let long = if target == Target::X86_64 { "@llvm.modf.f80(" } else { "@llvm.modf.f128(" };
    assert!(text.contains(long), "{text}");
    if target == Target::Aarch64 {
        text.push_str(HALF_LL);
    }
    Some(text)
}

/// hod's object through the session.
fn hod_object(text: &str, pic: bool) -> Vec<u8> {
    let options = Options {
        output_kind: OutputKind::Object,
        relocation_model: Some(if pic { RelocModel::Pic } else { RelocModel::Static }),
        ..Options::default()
    };
    hod::compile_with(text.as_bytes(), &options, &Settings::default(), &mut Report::default())
        .unwrap_or_else(|e| panic!("hod: {e}"))
}

/// The back end's object of every function under `Stress::ALL`.
fn stressed_object(text: &str, target: Target, pic: bool) -> Vec<u8> {
    let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
        .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    let side = parsed.text;
    let mut module = parsed.module;
    let mut bodies = Vec::new();
    hod_ll::parse_bodies(&mut module, &side, |_, read| {
        bodies.push(read.body.expect("every body is read").clone());
        Ok::<(), hod_ll::LlError>(())
    })
    .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    let options = hod_cg::Options {
        reloc_model: if pic { hod_cg::RelocModel::Pic } else { hod_cg::RelocModel::Static },
        stress: hod_cg::Stress::ALL,
        ..hod_cg::Options::default()
    };
    match target {
        Target::X86_64 => hod_x64::compile_to_object(&module, &bodies, &options)
            .unwrap_or_else(|e| panic!("stress: {e}")),
        Target::Aarch64 => hod_a64::compile_to_object(&module, &bodies, &options)
            .unwrap_or_else(|e| panic!("stress: {e}")),
        Target::I686 => unreachable!("not in TARGETS"),
    }
}

/// `llc` 22's object, the reference; `None` without it.
fn llc_object(dir: &Scratch, text: &str, pic: bool, name: &str) -> Option<PathBuf> {
    let llc = tool("llc-22")?;
    let object = dir.join(name);
    let mut command = Command::new(llc);
    command.args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj"]);
    command.arg(if pic { "-relocation-model=pic" } else { "-relocation-model=static" });
    command.arg("-o").arg(&object);
    let ran = run_with(&mut command, text.as_bytes(), Duration::from_mins(2));
    assert!(ran.success(), "llc 22: {}", ran.stderr_text());
    Some(object)
}

/// The forms not lowered are refused by the classifier's row: on x86-64
/// `half` (`llc` 22 cannot select it) and `fp128` (`llc` calls
/// `modff128`); `bfloat` and vectors on both targets.
#[test]
fn the_other_types_are_refused_by_the_row() {
    let forms: [(Target, &str, &str); 6] = [
        (Target::X86_64, "half", "f16"),
        (Target::X86_64, "fp128", "f128"),
        (Target::X86_64, "bfloat", "bf16"),
        (Target::X86_64, "<2 x float>", "v2f32"),
        (Target::Aarch64, "bfloat", "bf16"),
        (Target::Aarch64, "<2 x float>", "v2f32"),
    ];
    for (target, ty, suffix) in forms {
        let text = format!(
            "target triple = \"{}\"\n\
             declare {{ {ty}, {ty} }} @llvm.modf.{suffix}({ty})\n\
             define void @f({ty} %x, ptr %p) {{\n  \
             %r = call {{ {ty}, {ty} }} @llvm.modf.{suffix}({ty} %x)\n  \
             store {{ {ty}, {ty} }} %r, ptr %p\n  ret void\n}}\n",
            target.triple()
        );
        let options = Options { output_kind: OutputKind::Object, ..Options::default() };
        let error = hod::compile_with(
            text.as_bytes(),
            &options,
            &Settings::default(),
            &mut Report::default(),
        )
        .expect_err(&text)
        .to_string();
        assert!(error.contains("intrinsic:llvm.modf"), "{} {ty}: {error}", target.name());
    }
}

/// The forms lowered compile on each target (no toolchain needed).
#[test]
fn the_lowered_types_compile() {
    let forms: [(Target, &[(&str, &str)]); 2] = [
        (Target::X86_64, &[("float", "f32"), ("double", "f64"), ("x86_fp80", "f80")]),
        (
            Target::Aarch64,
            &[("half", "f16"), ("float", "f32"), ("double", "f64"), ("fp128", "f128")],
        ),
    ];
    for (target, types) in forms {
        for &(ty, suffix) in types {
            let text = format!(
                "target triple = \"{}\"\n\
                 declare {{ {ty}, {ty} }} @llvm.modf.{suffix}({ty})\n\
                 define {{ {ty}, {ty} }} @f({ty} %x) {{\n  \
                 %r = call {{ {ty}, {ty} }} @llvm.modf.{suffix}({ty} %x)\n  \
                 ret {{ {ty}, {ty} }} %r\n}}\n",
                target.triple()
            );
            hod_object(&text, true);
            stressed_object(&text, target, false);
        }
    }
}

#[test]
fn modf_from_c_runs_as_llc_22() {
    let mut runs = 0;
    for target in TARGETS {
        let dir = Scratch::new(&format!("hod-modf64-{}", target.name()));
        let Some(toolchain) = Toolchain::for_target(&dir, target) else { continue };
        let Some(text) = module(&dir, target) else { continue };
        let main = toolchain.compile_c(&dir, "driver", DRIVER_C, &["-O0", "-fPIE"]);
        let linker = &toolchain.linkers[0];
        for pic in [false, true] {
            let kind = if pic { "pic" } else { "static" };
            let Some(reference) = llc_object(&dir, &text, pic, &format!("llc-{kind}.o")) else {
                return;
            };
            let objects = [
                ("llc", reference),
                ("hod", dir.write(&format!("hod-{kind}.o"), hod_object(&text, pic))),
                (
                    "stress",
                    dir.write(&format!("stress-{kind}.o"), stressed_object(&text, target, pic)),
                ),
            ];
            let mut want: Option<String> = None;
            for (build, object) in &objects {
                let who = format!("{} {kind} {build}", target.name());
                let program = dir.join(format!("{build}-{kind}"));
                let inputs: [&Path; 3] = [&main, object, Path::new("-lm")];
                let flags: &[&str] = if pic { &["-pie"] } else { &["-no-pie"] };
                let ran = toolchain.link_and_run(linker, flags, &inputs, &program);
                assert_eq!(exit_code(&ran), Some(0), "{who}: {}", ran.stderr_text());
                let out = ran.stdout_text();
                match &want {
                    None => want = Some(out),
                    Some(want) => assert_eq!(&out, want, "{who} against llc 22"),
                }
                runs += 1;
            }
            let out = want.unwrap_or_default();
            let lines = 25 * 2 + 20 + if target == Target::X86_64 { 15 } else { 10 + 19 };
            assert_eq!(out.lines().count(), lines, "{}:\n{out}", target.name());
        }
    }
    eprintln!("modf: {runs} runs print what llc 22's do");
}
