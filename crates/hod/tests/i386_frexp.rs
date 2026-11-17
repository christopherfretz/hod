//! WP-341 (`spec/codegen-x86.md` "Floating-point classes and `modf`"):
//! `llvm.frexp` of `float`, `double` and `x86_fp80` on i386, run against
//! `llc` (`-O0 -global-isel=0 -fast-isel=0`, LLVM 19 and 22) under
//! `qemu-i386`, plain and under `Stress::ALL`, static and PIE, at CPU
//! `generic` (no `cmov`, no SSE), `i686` (`cmov`) and `pentium4` (SSE2):
//! - the fraction's bits and the exponent of zeros, subnormals, normals,
//!   powers of two, the largest finite, infinities and NaNs (quiet and
//!   signalling), both signs;
//! - the call as an x87 barrier: a product computed before it and read
//!   at 80 bits after it is the product rounded to its type; a `frexp`
//!   of a constant, which `llc` folds (no call), is no barrier;
//! - a C program through clang's `__builtin_frexp*`.
//!
//! A missing tool is skipped with a note, unless `HOD_REQUIRE_TOOLS=1`.

use std::fmt::Write as _;

use hod_testutil::Scratch;

mod i386_i11b;
mod i386_support;
use i386_i11b::{Build, STATIC, Source, lane, refusal};

/// QEMU's features of an SSE2 build (`pentium4`).
const P4: &[&str] = &["mmx", "sse", "sse2"];

/// The CPUs of the runs: the function attribute (so `Stress::ALL`
/// applies too) and the QEMU features its code needs.
const CPUS: [(Option<&str>, &[&str]); 3] =
    [(None, &[]), (Some("i686"), &[]), (Some("pentium4"), P4)];

/// `float` patterns, both signs.
const F32: [u32; 10] = [
    0,
    1,
    0x0040_0000,
    0x007f_ffff,
    0x0080_0000,
    0x3f80_0000,
    0x3fc0_0000,
    0x4100_0000,
    0x7f7f_ffff,
    0x7f80_0000,
];

/// `float` NaNs (one sign each is enough: the sign rides along).
const F32_NAN: [u32; 3] = [0x7fc0_0000, 0xffc0_0001, 0x7fa0_0000];

/// `double` patterns, both signs.
const F64: [u64; 10] = [
    0,
    1,
    0x0000_0001_0000_0000,
    0x000f_ffff_ffff_ffff,
    0x0010_0000_0000_0000,
    0x3ff0_0000_0000_0000,
    0x3ff8_0000_0000_0000,
    0x4020_0000_0000_0000,
    0x7fef_ffff_ffff_ffff,
    0x7ff0_0000_0000_0000,
];

/// `double` NaNs.
const F64_NAN: [u64; 3] = [0x7ff8_0000_0000_0000, 0xfff8_0000_0000_0001, 0x7ff4_0000_0000_0000];

/// `x86_fp80` patterns (sign and exponent, significand), both signs.
const F80: [(u16, u64); 10] = [
    (0, 0),
    (0, 1),
    (0, 1 << 62),
    (0, (1 << 63) - 1),
    (1, 1 << 63),
    (0x3fff, 1 << 63),
    (0x3fff, 0xc000_0000_0000_0000),
    (0x4002, 1 << 63),
    (0x7ffe, u64::MAX),
    (0x7fff, 1 << 63),
];

/// `x86_fp80` NaNs.
const F80_NAN: [(u16, u64); 3] = [
    (0x7fff, 0xc000_0000_0000_0000),
    (0xffff, 0xc000_0000_0000_0001),
    (0x7fff, 0xa000_0000_0000_0000),
];

/// The functions of one type: `@t<s>` prints the bits of the fraction
/// and the exponent of the value at its pointer.
const TYPES: &str = r#"
declare i32 @printf(ptr, ...)
@fmt32 = private constant [9 x i8] c"%08x %d\0A\00"
@fmt64 = private constant [12 x i8] c"%016llx %d\0A\00"
@fmt80 = private constant [16 x i8] c"%04x%016llx %d\0A\00"
@fmtp = private constant [8 x i8] c"%La %d\0A\00"
declare { float, i32 } @llvm.frexp.f32.i32(float)
declare { double, i32 } @llvm.frexp.f64.i32(double)
declare { x86_fp80, i32 } @llvm.frexp.f80.i32(x86_fp80)
define void @tf(ptr %p) #0 {
  %x = load float, ptr %p
  %r = call { float, i32 } @llvm.frexp.f32.i32(float %x)
  %f = extractvalue { float, i32 } %r, 0
  %e = extractvalue { float, i32 } %r, 1
  %b = bitcast float %f to i32
  %u = call i32 (ptr, ...) @printf(ptr @fmt32, i32 %b, i32 %e)
  ret void
}
define void @td(ptr %p) #0 {
  %x = load double, ptr %p
  %r = call { double, i32 } @llvm.frexp.f64.i32(double %x)
  %f = extractvalue { double, i32 } %r, 0
  %e = extractvalue { double, i32 } %r, 1
  %b = bitcast double %f to i64
  %u = call i32 (ptr, ...) @printf(ptr @fmt64, i64 %b, i32 %e)
  ret void
}
define void @tl(ptr %p) #0 {
  %s = alloca x86_fp80, align 4
  %x = load x86_fp80, ptr %p
  %r = call { x86_fp80, i32 } @llvm.frexp.f80.i32(x86_fp80 %x)
  %f = extractvalue { x86_fp80, i32 } %r, 0
  %e = extractvalue { x86_fp80, i32 } %r, 1
  store x86_fp80 %f, ptr %s
  %m = load i64, ptr %s
  %hp = getelementptr i8, ptr %s, i32 8
  %h = load i16, ptr %hp
  %hz = zext i16 %h to i32
  %u = call i32 (ptr, ...) @printf(ptr @fmt80, i32 %hz, i64 %m, i32 %e)
  ret void
}
define void @bf(float %a, float %b) #0 {
  %p = fmul float %a, %b
  %r = call { float, i32 } @llvm.frexp.f32.i32(float %p)
  %e = extractvalue { float, i32 } %r, 1
  %w = fpext float %p to x86_fp80
  %u = call i32 (ptr, ...) @printf(ptr @fmtp, x86_fp80 %w, i32 %e)
  ret void
}
define void @bd(double %a, double %b) #0 {
  %p = fmul double %a, %b
  %r = call { double, i32 } @llvm.frexp.f64.i32(double %p)
  %e = extractvalue { double, i32 } %r, 1
  %w = fpext double %p to x86_fp80
  %u = call i32 (ptr, ...) @printf(ptr @fmtp, x86_fp80 %w, i32 %e)
  ret void
}
define void @cd(double %a, double %b) #0 {
  %p = fmul double %a, %b
  %r = call { double, i32 } @llvm.frexp.f64.i32(double 12.0)
  %e = extractvalue { double, i32 } %r, 1
  %w = fpext double %p to x86_fp80
  %u = call i32 (ptr, ...) @printf(ptr @fmtp, x86_fp80 %w, i32 %e)
  ret void
}
define void @cl() #0 {
  %r = call { x86_fp80, i32 } @llvm.frexp.f80.i32(x86_fp80 0xK7FFF8000000000000000)
  %f = extractvalue { x86_fp80, i32 } %r, 0
  %e = extractvalue { x86_fp80, i32 } %r, 1
  %u = call i32 (ptr, ...) @printf(ptr @fmtp, x86_fp80 %f, i32 %e)
  ret void
}
"#;

/// The program for `cpu`: every pattern through its type's function,
/// then the barrier shapes.
fn program(cpu: Option<&str>) -> String {
    let mut s = format!("target triple = \"{}\"\n{TYPES}", i386_support::TRIPLE);
    let mut calls = String::new();
    let f32s = F32.iter().flat_map(|&p| [p, p | 0x8000_0000]).chain(F32_NAN);
    for (n, bits) in f32s.enumerate() {
        writeln!(s, "@f{n} = private constant i32 {}", bits as i32).unwrap();
        writeln!(calls, "  call void @tf(ptr @f{n})").unwrap();
    }
    let f64s = F64.iter().flat_map(|&p| [p, p | (1 << 63)]).chain(F64_NAN);
    for (n, bits) in f64s.enumerate() {
        writeln!(s, "@d{n} = private constant i64 {}", bits as i64).unwrap();
        writeln!(calls, "  call void @td(ptr @d{n})").unwrap();
    }
    let f80s = F80.iter().flat_map(|&(e, m)| [(e, m), (e | 0x8000, m)]).chain(F80_NAN);
    for (n, (e, m)) in f80s.enumerate() {
        writeln!(
            s,
            "@l{n} = private constant {{ i64, i16 }} {{ i64 {}, i16 {} }}",
            m as i64, e as i16
        )
        .unwrap();
        writeln!(calls, "  call void @tl(ptr @l{n})").unwrap();
    }
    // Products that round at their type (overflow, an inexact product):
    // read after the call, they are rounded (the call is a barrier).
    calls.push_str(
        "  call void @bf(float 0x46293E5940000000, float 0x46293E5940000000)
  call void @bf(float 0x3FF19999A0000000, float 0x3FF3333340000000)
  call void @bd(double 1.0e300, double 1.0e10)
  call void @bd(double 0x3FF199999999999A, double 0x3FF3333333333333)
  call void @cd(double 1.0e300, double 1.0e10)
  call void @cd(double 0x3FF199999999999A, double 0x3FF3333333333333)
  call void @cl()
",
    );
    write!(s, "define i32 @main() #0 {{\n{calls}  ret i32 0\n}}\n").unwrap();
    match cpu {
        Some(c) => writeln!(s, "attributes #0 = {{ \"target-cpu\"=\"{c}\" }}").unwrap(),
        None => s.push_str("attributes #0 = { nounwind }\n"),
    }
    s
}

#[test]
fn frexp_of_every_class_as_llc() {
    let Some(lane) = lane() else { return };
    let scratch = Scratch::new("i386-frexp-ll");
    let mut runs = 0;
    for (cpu, features) in CPUS {
        let text = program(cpu);
        for pic in [false, true] {
            let build = Build { pic, features, link: &["-lm"], ..STATIC };
            let label = format!("frexp-{}", cpu.unwrap_or("generic"));
            runs += lane.check(&scratch, &label, &Source::Ll(&text), build);
        }
    }
    assert!(runs > 0, "no run was made");
    eprintln!("i386 frexp: {runs} runs equal llc's");
}

/// clang's `__builtin_frexp*` (`llvm.frexp` at `-O0`, clang 19 and 22).
const FREXP_C: &str = r#"
#include <stdio.h>
#include <string.h>
#include <math.h>

static void f32(float x) {
  int e = -99;
  float f = __builtin_frexpf(x, &e);
  unsigned b;
  memcpy(&b, &f, 4);
  printf("%08x %d\n", b, e);
}

static void f64(double x) {
  int e = -99;
  double f = __builtin_frexp(x, &e);
  unsigned long long b;
  memcpy(&b, &f, 8);
  printf("%016llx %d\n", b, e);
}

static void f80(long double x) {
  int e = -99;
  long double f = __builtin_frexpl(x, &e);
  unsigned char b[10];
  memcpy(b, &f, 10);
  for (int i = 9; i >= 0; i--) printf("%02x", b[i]);
  printf(" %d\n", e);
}

static void barrier(double a, double b) {
  double p = a * b;
  int e;
  double f = __builtin_frexp(p, &e);
  long double w = p;
  printf("%La %a %d\n", w, f, e);
}

volatile float vf[] = {0.0f, -0.0f, 1e-45f, -1e-40f, 1.17549435e-38f, 1.0f, -1.0f,
                       0.75f, 8.0f, -3.4028235e38f, INFINITY, -INFINITY, NAN, -NAN};
volatile double vd[] = {0.0, -0.0, 4.9e-324, -2.2e-310, 2.2250738585072014e-308, 1.0,
                        -2.0, 0.1, 1024.0, 1.7976931348623157e308, INFINITY, -INFINITY,
                        NAN, -NAN};
volatile long double vl[] = {0.0L, -0.0L, 3.6e-4951L, -1e-4940L, 1.0L, -0.5L, 0.1L,
                             65536.0L, 1.18e4932L, INFINITY, -INFINITY, NAN, -NAN};

int main(void) {
  for (unsigned i = 0; i < sizeof vf / sizeof vf[0]; i++) f32(vf[i]);
  for (unsigned i = 0; i < sizeof vd / sizeof vd[0]; i++) f64(vd[i]);
  for (unsigned i = 0; i < sizeof vl / sizeof vl[0]; i++) f80(vl[i]);
  barrier(1e300, 1e10);
  barrier(1.1, 1.2);
  return 0;
}
"#;

#[test]
fn frexp_from_c() {
    let Some(lane) = lane() else { return };
    let scratch = Scratch::new("i386-frexp-c");
    let source = scratch.write("frexp.c", FREXP_C);
    let mut runs = 0;
    for (flags, features) in
        [(&[][..], &[][..]), (&["-march=i586"][..], &[][..]), (&["-march=pentium4"][..], P4)]
    {
        for pic in [false, true] {
            let build = Build { pic, features, link: &["-lm"], ..STATIC };
            let name = format!("frexp-{}", flags.first().unwrap_or(&"i686"));
            runs += lane.check(&scratch, &name, &Source::C(&source, flags), build);
        }
    }
    assert!(runs > 0, "no run was made");
    eprintln!("i386 frexp from C: {runs} runs equal llc's");
}

#[test]
fn vectors_and_other_types_stay_refused() {
    let vector = "declare { <2 x double>, <2 x i32> } @llvm.frexp.v2f64.v2i32(<2 x double>)
define void @f(ptr %q, ptr %p) {
  %x = load <2 x double>, ptr %q
  %r = call { <2 x double>, <2 x i32> } @llvm.frexp.v2f64.v2i32(<2 x double> %x)
  %a = extractvalue { <2 x double>, <2 x i32> } %r, 0
  store <2 x double> %a, ptr %p
  ret void
}";
    let error = refusal(vector, None).expect("a vector frexp compiles");
    assert!(error.contains("intrinsic:llvm.frexp/vector"), "{error}");
    for cpu in [None, Some("pentium4")] {
        let half = "declare { half, i32 } @llvm.frexp.f16.i32(half)
define void @f(ptr %q, ptr %p) {
  %x = load half, ptr %q
  %r = call { half, i32 } @llvm.frexp.f16.i32(half %x)
  %a = extractvalue { half, i32 } %r, 0
  store half %a, ptr %p
  ret void
}";
        let error = refusal(half, cpu).expect("a half frexp compiles");
        assert!(error.contains("intrinsic:llvm.frexp/half"), "{error}");
    }
}
