//! WP-203: 256-bit vectors in `ymm` under AVX (`spec/abi-x64.md` §1.3,
//! §3.2, §4.3, §5.1, §8.7, §9.1), run. Two translation units, a callee
//! and a caller, each built by `llc` and by hod (plain and under every
//! stress mode) and mixed every way; each program links with every
//! linker and runs under the emulator's richest CPU, and prints what the
//! all-`llc` build prints.
//!
//! - C++ as clang 19 and 22 write it for `-mavx2`, `-march=haswell` and
//!   `-march=znver3`: `struct { __m256i }`, a pair of them, a union with
//!   one, `std::array<__m256, 2>`, `__m256d` arguments past the eighth
//!   vector register, a variadic callee reading `__m256d` with `va_arg`,
//!   an indirect call.
//! - IR: what clang does not write: `{<4 x double>, <4 x double>}`,
//!   vectors widened to 32 bytes (`<6 x float>`, `<3 x double>`), split
//!   into two `ymm` (`<16 x float>`), passed element by element
//!   (`<12 x float>`), `invoke`, a `musttail` call, `byval` and `sret`.

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

#[path = "common/aggr_gen.rs"]
mod aggr_gen;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use aggr_gen::{llc_of, parse, pic};
use hod_cg::Stress;
use hod_testutil::{Scratch, run_with};
use linking::Toolchain;

const CALLEE_CXX: &str = r"
#include <immintrin.h>
#include <array>
#include <cstdarg>
struct S1 { __m256i v; };
struct S2 { __m256i a, b; };
union U { __m256i v; long long q[4]; };
S1 f1(S1 s) { s.v = _mm256_add_epi64(s.v, _mm256_set1_epi64x(1)); return s; }
S2 f2(S2 s) { S2 r = { _mm256_sub_epi64(s.b, s.a), _mm256_add_epi64(s.a, s.b) }; return r; }
U f3(U u) { u.q[0] += 100; u.q[3] -= 7; return u; }
std::array<__m256, 2> f4(std::array<__m256, 2> a) {
  return { _mm256_mul_ps(a[0], a[1]), _mm256_add_ps(a[0], a[1]) };
}
__m256d f5(__m256d a, __m256d b) { return _mm256_sub_pd(a, b); }
__m256d f6(__m256d a0, __m256d a1, __m256d a2, __m256d a3, __m256d a4, __m256d a5,
           __m256d a6, __m256d a7, __m256d a8, int k, __m128 x, __m256d a9) {
  __m256d s = _mm256_add_pd(_mm256_add_pd(a0, a1), _mm256_add_pd(a2, a3));
  s = _mm256_add_pd(s, _mm256_add_pd(_mm256_add_pd(a4, a5), _mm256_add_pd(a6, a7)));
  s = _mm256_mul_pd(s, a8);
  s = _mm256_sub_pd(s, a9);
  return _mm256_add_pd(s, _mm256_set1_pd(k + x[1]));
}
double vsum(int n, ...) {
  va_list ap;
  va_start(ap, n);
  __m256d s = _mm256_setzero_pd();
  for (int i = 0; i < n; i++) s = _mm256_add_pd(s, va_arg(ap, __m256d));
  double d = va_arg(ap, double);
  va_end(ap);
  return s[0] + 2 * s[1] + 3 * s[2] + 4 * s[3] + d;
}
static __m256i twice(__m256i v) { return _mm256_add_epi32(v, v); }
__m256i (*pick())(__m256i) { return twice; }
";

const CALLER_CXX: &str = r#"
#include <immintrin.h>
#include <array>
#include <cstdio>
struct S1 { __m256i v; };
struct S2 { __m256i a, b; };
union U { __m256i v; long long q[4]; };
S1 f1(S1 s);
S2 f2(S2 s);
U f3(U u);
std::array<__m256, 2> f4(std::array<__m256, 2> a);
__m256d f5(__m256d a, __m256d b);
__m256d f6(__m256d, __m256d, __m256d, __m256d, __m256d, __m256d, __m256d, __m256d, __m256d,
           int, __m128, __m256d);
double vsum(int n, ...);
__m256i (*pick())(__m256i);
static void q(const char *name, __m256i v) {
  long long w[4];
  _mm256_storeu_si256((__m256i *)w, v);
  printf("%s %lld %lld %lld %lld\n", name, w[0], w[1], w[2], w[3]);
}
static void d(const char *name, __m256d v) {
  double w[4];
  _mm256_storeu_pd(w, v);
  printf("%s %.17g %.17g %.17g %.17g\n", name, w[0], w[1], w[2], w[3]);
}
int main() {
  __m256i a = _mm256_set_epi64x(4, -3, 2, -1), b = _mm256_set_epi64x(40, 30, -20, 10);
  S1 s1 = { a };
  q("f1", f1(s1).v);
  S2 s2 = { a, b };
  S2 r2 = f2(s2);
  q("f2a", r2.a);
  q("f2b", r2.b);
  U u;
  u.v = b;
  q("f3", f3(u).v);
  std::array<__m256, 2> x = { _mm256_set_ps(8, 7, 6, 5, 4, 3, 2, 1),
                              _mm256_set1_ps(0.5f) };
  std::array<__m256, 2> y = f4(x);
  d("f4", _mm256_cvtps_pd(_mm256_castps256_ps128(y[0])));
  d("f4", _mm256_cvtps_pd(_mm256_extractf128_ps(y[1], 1)));
  __m256d e = _mm256_set_pd(1.5, -2.25, 3.0, 0.125), g = _mm256_set1_pd(0.75);
  d("f5", f5(e, g));
  d("f6", f6(e, g, e, g, e, g, e, g, _mm256_set1_pd(2.0), 3, _mm_set_ps(0, 0, 9.5f, 0), e));
  printf("vsum %.17g\n", vsum(3, e, g, e, 0.0625));
  q("pick", pick()(a));
  return 0;
}
"#;

/// The IR callee: what clang does not write.
const CALLEE_LL: &str = r"
define { <4 x double>, <4 x double> } @c_pair(<4 x double> %a, <4 x double> %b) #0 {
  %s = fadd <4 x double> %a, %b
  %d = fsub <4 x double> %a, %b
  %r0 = insertvalue { <4 x double>, <4 x double> } poison, <4 x double> %s, 0
  %r1 = insertvalue { <4 x double>, <4 x double> } %r0, <4 x double> %d, 1
  ret { <4 x double>, <4 x double> } %r1
}
define <6 x float> @c_odd(<6 x float> %a, <3 x double> %b, i32 %n) #0 {
  %b1 = extractelement <3 x double> %b, i32 2
  %f = fptrunc double %b1 to float
  %r = insertelement <6 x float> %a, float %f, i32 5
  ret <6 x float> %r
}
define <16 x float> @c_split(<16 x float> %a, <16 x float> %b) #0 {
  %r = fmul <16 x float> %a, %b
  ret <16 x float> %r
}
define <12 x float> @c_elem(<12 x float> %a, float %f) #0 {
  %r = insertelement <12 x float> %a, float %f, i32 11
  ret <12 x float> %r
}
define <4 x i64> @c_many(<4 x i64> %a0, <4 x i64> %a1, <4 x i64> %a2, <4 x i64> %a3,
    <4 x i64> %a4, <4 x i64> %a5, <4 x i64> %a6, <4 x i64> %a7, i8 %c,
    <4 x i64> %a8, <2 x i64> %n, <4 x i64> %a9) #0 {
  %s0 = add <4 x i64> %a0, %a1
  %s1 = add <4 x i64> %a2, %a3
  %s2 = add <4 x i64> %a4, %a5
  %s3 = add <4 x i64> %a6, %a7
  %s4 = add <4 x i64> %s0, %s1
  %s5 = add <4 x i64> %s2, %s3
  %s6 = add <4 x i64> %s4, %s5
  %s7 = mul <4 x i64> %s6, %a8
  %s8 = sub <4 x i64> %s7, %a9
  %c64 = sext i8 %c to i64
  %n0 = extractelement <2 x i64> %n, i32 1
  %k = add i64 %c64, %n0
  %r = insertelement <4 x i64> %s8, i64 %k, i32 2
  ret <4 x i64> %r
}
define void @c_sret(ptr sret({ <4 x i64>, i32 }) align 32 %out, ptr byval({ <4 x i64>, <4 x i64> }) align 32 %in, <8 x i32> %v) #0 {
  %a = load <4 x i64>, ptr %in, align 32
  %pb = getelementptr i8, ptr %in, i64 32
  %b = load <4 x i64>, ptr %pb, align 32
  %s = sub <4 x i64> %a, %b
  store <4 x i64> %s, ptr %out, align 32
  %e = extractelement <8 x i32> %v, i32 6
  %pe = getelementptr i8, ptr %out, i64 32
  store i32 %e, ptr %pe, align 4
  ret void
}
define <8 x i32> @c_maybe(<8 x i32> %v) #0 {
  %r = shl <8 x i32> %v, <i32 1, i32 2, i32 3, i32 4, i32 5, i32 6, i32 7, i32 8>
  ret <8 x i32> %r
}
define <8 x i32> @c_tail(<8 x i32> %v) #0 {
  %w = add <8 x i32> %v, %v
  %r = musttail call <8 x i32> @c_maybe(<8 x i32> %w)
  ret <8 x i32> %r
}
";

/// The IR caller: each callee with fixed values, the results stored to
/// `@out` in order.
const CALLER_LL: &str = r"
@out = global [512 x i8] zeroinitializer, align 64
declare { <4 x double>, <4 x double> } @c_pair(<4 x double>, <4 x double>)
declare <6 x float> @c_odd(<6 x float>, <3 x double>, i32)
declare <16 x float> @c_split(<16 x float>, <16 x float>)
declare <12 x float> @c_elem(<12 x float>, float)
declare <4 x i64> @c_many(<4 x i64>, <4 x i64>, <4 x i64>, <4 x i64>, <4 x i64>, <4 x i64>,
    <4 x i64>, <4 x i64>, i8, <4 x i64>, <2 x i64>, <4 x i64>)
declare void @c_sret(ptr sret({ <4 x i64>, i32 }) align 32, ptr byval({ <4 x i64>, <4 x i64> }) align 32, <8 x i32>)
declare <8 x i32> @c_maybe(<8 x i32>)
declare <8 x i32> @c_tail(<8 x i32>)
declare i32 @c_personality(...)

define void @run() #0 personality ptr @c_personality {
  %p = call { <4 x double>, <4 x double> } @c_pair(<4 x double> <double 1.5, double -2.0, double 3.25, double 8.0>, <4 x double> <double 0.5, double 4.0, double -1.0, double 2.0>)
  store { <4 x double>, <4 x double> } %p, ptr @out, align 64
  %o = call <6 x float> @c_odd(<6 x float> <float 1.0, float 2.0, float 3.0, float 4.0, float 5.0, float 6.0>, <3 x double> <double 7.0, double 8.0, double 9.5>, i32 3)
  %po = getelementptr i8, ptr @out, i64 64
  store <6 x float> %o, ptr %po, align 1
  %s = call <16 x float> @c_split(<16 x float> <float 1.0, float 2.0, float 3.0, float 4.0, float 5.0, float 6.0, float 7.0, float 8.0, float 9.0, float 10.0, float 11.0, float 12.0, float 13.0, float 14.0, float 15.0, float 16.0>, <16 x float> <float 0.5, float 0.5, float 0.5, float 0.5, float 2.0, float 2.0, float 2.0, float 2.0, float -1.0, float -1.0, float -1.0, float -1.0, float 3.0, float 3.0, float 3.0, float 3.0>)
  %ps = getelementptr i8, ptr @out, i64 96
  store <16 x float> %s, ptr %ps, align 1
  %e = call <12 x float> @c_elem(<12 x float> <float 1.0, float 2.0, float 3.0, float 4.0, float 5.0, float 6.0, float 7.0, float 8.0, float 9.0, float 10.0, float 11.0, float 12.0>, float -7.5)
  %pe = getelementptr i8, ptr @out, i64 160
  store <12 x float> %e, ptr %pe, align 1
  %m = call <4 x i64> @c_many(<4 x i64> <i64 1, i64 2, i64 3, i64 4>, <4 x i64> <i64 5, i64 6, i64 7, i64 8>, <4 x i64> <i64 9, i64 10, i64 11, i64 12>, <4 x i64> <i64 13, i64 14, i64 15, i64 16>, <4 x i64> <i64 17, i64 18, i64 19, i64 20>, <4 x i64> <i64 21, i64 22, i64 23, i64 24>, <4 x i64> <i64 25, i64 26, i64 27, i64 28>, <4 x i64> <i64 29, i64 30, i64 31, i64 32>, i8 -5, <4 x i64> <i64 2, i64 3, i64 -1, i64 7>, <2 x i64> <i64 99, i64 1000>, <4 x i64> <i64 100, i64 200, i64 300, i64 400>)
  %pm = getelementptr i8, ptr @out, i64 224
  store <4 x i64> %m, ptr %pm, align 1
  %in = alloca { <4 x i64>, <4 x i64> }, align 32
  store <4 x i64> <i64 50, i64 60, i64 70, i64 80>, ptr %in, align 32
  %pin = getelementptr i8, ptr %in, i64 32
  store <4 x i64> <i64 5, i64 6, i64 7, i64 8>, ptr %pin, align 32
  %r = alloca { <4 x i64>, i32 }, align 32
  call void @c_sret(ptr sret({ <4 x i64>, i32 }) align 32 %r, ptr byval({ <4 x i64>, <4 x i64> }) align 32 %in, <8 x i32> <i32 1, i32 2, i32 3, i32 4, i32 5, i32 6, i32 77, i32 8>)
  %rv = load { <4 x i64>, i32 }, ptr %r, align 32
  %pr = getelementptr i8, ptr @out, i64 256
  %rv0 = extractvalue { <4 x i64>, i32 } %rv, 0
  store <4 x i64> %rv0, ptr %pr, align 32
  %rv1 = extractvalue { <4 x i64>, i32 } %rv, 1
  %pr1 = getelementptr i8, ptr @out, i64 288
  store i32 %rv1, ptr %pr1, align 4
  %t = invoke <8 x i32> @c_tail(<8 x i32> <i32 1, i32 -1, i32 2, i32 -2, i32 3, i32 -3, i32 4, i32 -4>)
      to label %ok unwind label %pad
ok:
  %pt = getelementptr i8, ptr @out, i64 320
  store <8 x i32> %t, ptr %pt, align 1
  %fp = load ptr, ptr @maybe, align 8
  %i = call <8 x i32> %fp(<8 x i32> %t)
  %pi = getelementptr i8, ptr @out, i64 352
  store <8 x i32> %i, ptr %pi, align 1
  ret void
pad:
  %l = landingpad { ptr, i32 } cleanup
  resume { ptr, i32 } %l
}
@maybe = global ptr @c_maybe, align 8
";

const MAIN_C: &str = r#"
#include <stdio.h>
extern unsigned char out[512];
void run(void);
int c_personality(void) { return 0; }
int main(void) {
  run();
  for (int i = 0; i < 384; i++) printf("%02x%s", out[i], i % 32 == 31 ? "\n" : "");
  return 0;
}
"#;

/// The machines, as clang's flag and as the IR's attributes.
const MACHINES: [(&str, &str); 3] = [
    ("-mavx2", "\"target-cpu\"=\"x86-64\" \"target-features\"=\"+avx,+avx2,+sse4.2\""),
    ("-march=haswell", "\"target-cpu\"=\"haswell\""),
    ("-march=znver3", "\"target-cpu\"=\"znver3\""),
];

/// One translation unit: its IR, and the objects of each builder.
struct Unit {
    name: String,
    ir: PathBuf,
}

/// `llc` of LLVM `major` at `-O0` on `ir`.
fn llc_object(major: u32, ir: &Path) -> PathBuf {
    let llc = llc_of(major).expect("llc");
    let object = ir.with_extension(format!("llc{major}.o"));
    let mut command = Command::new(llc);
    command.args(["-O0", "-relocation-model=pic", "-filetype=obj"]).arg(ir).arg("-o").arg(&object);
    let outcome = run_with(&mut command, &[], Duration::from_mins(3));
    assert!(outcome.success(), "llc {major} {}: {}", ir.display(), outcome.stderr_text());
    object
}

/// hod (under `stress`) on `ir`, as `hod --strict` builds it.
fn hod_object(ir: &Path, stress: Stress, tag: &str) -> PathBuf {
    let text = std::fs::read_to_string(ir).expect("the IR");
    let (module, bodies) = parse(&text);
    let bytes = hod_x64::compile_to_object(&module, &bodies, &pic(stress))
        .unwrap_or_else(|e| panic!("hod refused {}: {e:?}", ir.display()));
    let object = ir.with_extension(format!("{tag}.o"));
    std::fs::write(&object, bytes).expect("the object");
    object
}

/// Every mix of the callee's and the caller's builders prints what the
/// all-`llc` build prints, with every linker.
fn check_mixes(
    toolchain: &Toolchain,
    scratch: &Scratch,
    label: &str,
    units: [&Unit; 2],
    major: u32,
    extra: &[&Path],
) -> usize {
    let builds = |unit: &Unit| {
        vec![
            ("llc", llc_object(major, &unit.ir)),
            ("hod", hod_object(&unit.ir, Stress::default(), "hod")),
            ("stress", hod_object(&unit.ir, Stress::ALL, "stress")),
        ]
    };
    let built = [builds(units[0]), builds(units[1])];
    let mut expected: Option<String> = None;
    let mut runs = 0;
    for (callee_by, callee) in &built[0] {
        for (caller_by, caller) in &built[1] {
            let tag = format!("{label}-{}-{callee_by}-{caller_by}", units[0].name);
            let mut objects: Vec<&Path> = vec![callee, caller];
            objects.extend(extra);
            let mut seen: Option<String> = None;
            for linker in &toolchain.linkers {
                let exe = scratch.join(format!("{tag}-{}", linker.name));
                let ran = toolchain.link_and_run(linker, &["-pie"], &objects, &exe);
                assert!(ran.success(), "{tag} ({}): {}", linker.name, ran.stderr_text());
                let out = ran.stdout_text();
                if let Some(seen) = &seen {
                    assert_eq!(&out, seen, "{tag}: {} differs", linker.name);
                }
                seen = Some(out);
                let _ = std::fs::remove_file(&exe);
                runs += 1;
            }
            let out = seen.unwrap_or_default();
            match &expected {
                // The first build is llc's callee with llc's caller.
                None => expected = Some(out),
                Some(want) => assert_eq!(
                    &out, want,
                    "{label}: callee by {callee_by}, caller by {caller_by} against llc {major}"
                ),
            }
        }
    }
    runs
}

/// The C++ of clang 19 and 22 for each machine, mixed every way.
#[test]
fn clang_wide_vectors_mix() {
    let scratch = Scratch::new("x64-wp203-cxx");
    let Some(toolchain) = Toolchain::x86_64(&scratch) else { return };
    let toolchain = toolchain.with_emulated_cpu("max");
    let mut runs = 0;
    for major in [19, 22] {
        let clang = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/clang++"));
        if !clang.is_file() || llc_of(major).is_none() {
            continue;
        }
        for (index, (flag, _)) in MACHINES.iter().enumerate() {
            let label = format!("c{major}-{index}");
            let units = ["callee", "caller"].map(|name| {
                let source = scratch.write(
                    &format!("{label}-{name}.cpp"),
                    if name == "callee" { CALLEE_CXX } else { CALLER_CXX },
                );
                let ir = scratch.join(format!("{label}-{name}.ll"));
                let mut command = Command::new(&clang);
                command
                    .args(["--target=x86_64-unknown-linux-gnu", "-O0", "-S", "-emit-llvm"])
                    .args(["-std=c++20", "-fPIE", "-fno-exceptions", *flag])
                    .arg(&source)
                    .arg("-o")
                    .arg(&ir);
                let outcome = run_with(&mut command, &[], Duration::from_mins(3));
                assert!(outcome.success(), "clang {major}: {}", outcome.stderr_text());
                Unit { name: name.to_string(), ir }
            });
            runs += check_mixes(&toolchain, &scratch, &label, [&units[0], &units[1]], major, &[]);
        }
    }
    eprintln!("wp203 C++: {runs} runs agree with llc");
}

/// The IR shapes clang does not write, for each machine.
#[test]
fn ir_wide_vectors_mix() {
    let scratch = Scratch::new("x64-wp203-ir");
    let Some(toolchain) = Toolchain::x86_64(&scratch) else { return };
    let toolchain = toolchain.with_emulated_cpu("max");
    let main = toolchain.compile_c(&scratch, "main", MAIN_C, &["-fPIE"]);
    let mut runs = 0;
    for major in [19, 22] {
        if llc_of(major).is_none() {
            continue;
        }
        for (index, (_, attrs)) in MACHINES.iter().enumerate() {
            let label = format!("ir{major}-{index}");
            let units = ["callee", "caller"].map(|name| {
                let body = if name == "callee" { CALLEE_LL } else { CALLER_LL };
                let text = format!(
                    "{}{body}\nattributes #0 = {{ {attrs} }}\n{}",
                    aggr_gen::header(),
                    aggr_gen::PIE_FLAGS
                );
                let ir = scratch.write(&format!("{label}-{name}.ll"), text);
                Unit { name: name.to_string(), ir }
            });
            runs +=
                check_mixes(&toolchain, &scratch, &label, [&units[0], &units[1]], major, &[&main]);
        }
    }
    eprintln!("wp203 IR: {runs} runs agree with llc");
}
