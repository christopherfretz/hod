//! The x86-64 `Mem`-class group (WP-126, `spec/codegen.md` §5.6.6):
//! integers wider than 64 bits, aggregates, `Mem` vectors and the
//! overflow intrinsics, checked against `hod_interp` and `llc` 19 and 22
//! under the emulated runner (`common/aggr_gen.rs`); constants with
//! addresses (`constaddr.rs`); values passed and returned through the
//! real convention (register pairs, `byval`, results in memory); what is
//! refused, by name.

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

#[path = "common/aggr_gen.rs"]
mod aggr_gen;

use std::path::{Path, PathBuf};
use std::time::Duration;

use aggr_gen::{PIE_FLAGS, Report, Rng, Suite, T, assert_agrees, header, llc, parse, pic, run};
use hod_cg::{CgError, Stress};
use hod_testutil::{Scratch, run_with};
use hod_x64::{aggr, constaddr};
use linking::{Toolchain, exit_code};

/// Prints what a suite's run measured.
#[allow(clippy::cast_precision_loss, reason = "rates to print")]
fn summary(name: &str, suite: &Suite, report: &Report) {
    eprintln!(
        "x64 aggr {name}: {} functions, {} cases: {} against the interpreter, {} against llc only, \
         {} undefined (not compared), {} native with garbage above the widths ({} returned in \
         memory); legs {:?}; {} IR instructions compiled in {:?} ({:.0} ns each); text hod {} B, \
         llc 22 {} B (ratio {:.2}); tools {:?}",
        suite.funcs.len(),
        suite.cases.len(),
        report.interp,
        report.llc_only,
        report.undefined,
        report.native,
        report.in_memory,
        report.legs,
        report.insts,
        report.compile,
        report.compile.as_nanos() as f64 / report.insts.max(1) as f64,
        report.text.0,
        report.text.1,
        report.text.0 as f64 / report.text.1.max(1) as f64,
        report.tools,
    );
}

fn check(name: &str, suite: &Suite) {
    let report = run(name, suite);
    summary(name, suite, &report);
    assert_agrees(&report);
}

/// Every binary operation at every wide width (division through the
/// runtime up to 128 bits), register and constant operands, every shift
/// amount the brief names.
#[test]
fn every_wide_binary_operation_agrees() {
    check("binary", &aggr_gen::binary_table(&mut Rng(126)));
}

/// The bits above the width that producers leave behind, under every
/// consumer that depends on them.
#[test]
fn consumers_of_unextended_results_agree() {
    check("chain", &aggr_gen::chain_table(&mut Rng(127)));
}

/// Every predicate at every wide width; `select`, `freeze`, `phi`.
#[test]
fn every_wide_compare_and_select_agrees() {
    check("compare", &aggr_gen::compare_table(&mut Rng(128)));
}

/// Extensions and truncations between narrow and wide widths, pointer
/// casts, bitcasts between wide integers and vectors, conversions
/// between floating point and wide integers.
#[test]
fn every_wide_cast_agrees() {
    check("cast", &aggr_gen::cast_table(&mut Rng(129)));
}

/// Aggregates of every layout (padding, packed, nested, empty,
/// zero-length) built, read, selected, merged, loaded and stored;
/// constant aggregates; `Mem` vectors.
#[test]
fn every_aggregate_form_agrees() {
    check("aggregate", &aggr_gen::aggregate_table(&mut Rng(130)));
}

/// The six overflow intrinsics at every width.
#[test]
fn every_overflow_intrinsic_agrees() {
    check("overflow", &aggr_gen::overflow_table(&mut Rng(131)));
}

/// Random straight-line wide-integer functions. `HOD_AGGR_FUNCTIONS=<n>`
/// sets how many (default 600).
#[test]
fn random_wide_functions_agree() {
    let count =
        std::env::var("HOD_AGGR_FUNCTIONS").ok().and_then(|text| text.parse().ok()).unwrap_or(600);
    check("random", &aggr_gen::random_functions(&mut Rng(0x0126), count));
}

fn type_id(m: &mut hod_ir::Module, ty: &T) -> hod_ir::TypeId {
    let t = &mut m.types;
    match ty {
        T::I(b) => t.int(*b).unwrap(),
        T::F => hod_ir::TypeId::FLOAT,
        T::D => hod_ir::TypeId::DOUBLE,
        T::P => t.ptr(0).unwrap(),
        T::S(fields, packed) => {
            let ids: Vec<_> = fields.iter().map(|f| type_id(m, f)).collect();
            m.types.struct_type(&ids, *packed).unwrap()
        }
        T::A(n, e) => {
            let e = type_id(m, e);
            m.types.array(e, *n).unwrap()
        }
        T::V(n, e) => {
            let e = type_id(m, e);
            m.types.vector(e, u32::try_from(*n).unwrap()).unwrap()
        }
    }
}

/// The layouts and classes the generator's masks and wrappers are
/// computed from are `hod_ir`'s and `hod_x64`'s.
#[test]
fn the_generator_layouts_and_classes_are_hods() {
    let (mut m, _) = parse(&format!("{}define void @f() {{\n  ret void\n}}\n", header()));
    let v = |n, e| T::V(n, Box::new(e));
    let mut all = aggr_gen::aggregates();
    all.extend([
        v(3, T::I(32)),
        v(8, T::I(1)),
        v(128, T::I(1)),
        v(4, T::I(64)),
        v(2, T::I(64)),
        v(4, T::F),
        v(5, T::I(8)),
        T::I(65),
        T::I(128),
        T::I(129),
        T::I(17),
        T::I(64),
        T::F,
        T::P,
    ]);
    for ty in all {
        let id = type_id(&mut m, &ty);
        assert_eq!(m.types.size_of(id), ty.store(), "store size of {}", ty.ll());
        assert_eq!(m.types.alloc_size_of(id), ty.alloc(), "alloc size of {}", ty.ll());
        assert_eq!(m.types.align_of(id), ty.align(), "align of {}", ty.ll());
        let class = hod_cg::Target::class_of(&hod_x64::X64, &m.types, id);
        assert_eq!(class == hod_cg::Class::Mem, ty.is_mem(), "class of {}", ty.ll());
        if let T::S(..) = ty {
            assert_eq!(m.types.field_offsets(id).unwrap(), ty.offsets().as_slice(), "{}", ty.ll());
        }
    }
}

/// What compiling `.ll` functions (the header added) reports.
fn compile_ll(functions: &str) -> Result<(), CgError> {
    let (module, bodies) = parse(&format!("{}{functions}{PIE_FLAGS}", header()));
    match hod_x64::compile_to_object(&module, &bodies, &pic(Stress::default())) {
        Ok(_) => Ok(()),
        Err(hod_x64::ModuleError::Function { error, .. }) => Err(error),
        Err(other) => panic!("{other}"),
    }
}

/// Asserts that each `.ll` function of `cases` is refused with `reason`.
fn assert_refused(cases: &[(&str, &'static str)]) {
    for (functions, reason) in cases {
        assert_eq!(compile_ll(functions), Err(CgError::unsupported(reason)), "{functions}");
    }
}

/// Each refusal of this group, by name, in a function of one construct;
/// the same limits as AArch64's.
#[test]
fn what_is_refused_is_refused_by_name() {
    assert_refused(&[
        (
            "define void @f(ptr %p, i256 %a, i256 %b) { %r = udiv i256 %a, %b\n store i256 %r, ptr %p\n ret void }",
            aggr::WIDE_DIVISION,
        ),
        (
            "define void @f(ptr %p, i129 %a, i129 %b) { %r = srem i129 %a, %b\n store i129 %r, ptr %p\n ret void }",
            aggr::WIDE_DIVISION,
        ),
        (
            "declare { i4096, i1 } @llvm.umul.with.overflow.i4096(i4096, i4096)\ndefine void @f(ptr %p, i4096 %a) { %r = call { i4096, i1 } @llvm.umul.with.overflow.i4096(i4096 %a, i4096 3)\n store { i4096, i1 } %r, ptr %p\n ret void }",
            aggr::WIDE_MUL_OVERFLOW,
        ),
        (
            "define void @f(ptr %p, i256 %a) { %r = sitofp i256 %a to double\n %s = insertvalue { double } undef, double %r, 0\n store { double } %s, ptr %p\n ret void }",
            aggr::WIDE_FP_CAST,
        ),
        (
            "define void @f(ptr %p, double %a) { %r = fptoui double %a to i256\n store i256 %r, ptr %p\n ret void }",
            aggr::WIDE_FP_CAST,
        ),
        (
            "define void @f(ptr %p, bfloat %a) { %r = fptosi bfloat %a to i100\n store i100 %r, ptr %p\n ret void }",
            aggr::WIDE_FP_CAST,
        ),
        (
            "define void @f(ptr %p, i8000 %a) { %r = mul i8000 %a, %a\n store i8000 %r, ptr %p\n ret void }",
            aggr::WIDE_MUL,
        ),
        (
            "define void @f(ptr %p, i8000 %a, i8000 %k) { %r = shl i8000 %a, %k\n store i8000 %r, ptr %p\n ret void }",
            aggr::WIDE_SHIFT,
        ),
        (
            "define void @f(ptr %p, ptr %q) { %v = load atomic i128, ptr %q seq_cst, align 16\n store i128 %v, ptr %p\n ret void }",
            hod_x64::atomic::ATOMIC,
        ),
        (
            "define void @f(ptr %p) { b:\n store { ptr } { ptr blockaddress(@f, %b) }, ptr %p\n ret void }",
            hod_x64::control::CONTROL,
        ),
    ]);
    // Linear forms have no limit.
    let text = "define void @f(ptr %p, i8000 %a) { %s = lshr i8000 %a, 4001\n %r = add i8000 %s, %a\n store i8000 %r, ptr %p\n ret void }";
    assert_eq!(compile_ll(text), Ok(()));
    let _ = constaddr::CONSTANT_RELOC;
}

/// Links `objects` with the C `main` and runs it.
fn link_and_run(
    toolchain: &Toolchain,
    dir: &Scratch,
    name: &str,
    c_main: &str,
    objects: &[PathBuf],
) -> Option<i32> {
    let linker = toolchain.linkers.first()?;
    let main = toolchain.compile_c(dir, &format!("{name}-main"), c_main, &["-fPIE"]);
    let mut inputs: Vec<&Path> = vec![&main];
    inputs.extend(objects.iter().map(PathBuf::as_path));
    let program = dir.join(name);
    toolchain.link_ok(linker, &["-pie"], &inputs, &program);
    let ran = run_with(&mut toolchain.runner_command(&program), &[], Duration::from_mins(5));
    assert!(!ran.timed_out(), "{name} did not finish");
    if exit_code(&ran) != Some(42) {
        eprintln!("{name}: {}{}", ran.stdout_text(), ran.stderr_text());
    }
    exit_code(&ran)
}

/// Compiles `ll` (the header added) by hod (plain and stressed) and by
/// `llc` 22 and 19, links each with `c_main` and requires exit 42.
fn every_leg_exits_42(name: &str, ll: &str, c_main: &str) {
    let text = format!("{}{ll}{PIE_FLAGS}", header());
    let (module, bodies) = parse(&text);
    let dir = Scratch::new(&format!("x64-aggr-{name}"));
    let mut legs = Vec::new();
    for (mode, stress) in [("plain", Stress::default()), ("stress", Stress::ALL)] {
        let bytes = hod_x64::compile_to_object(&module, &bodies, &pic(stress))
            .unwrap_or_else(|e| panic!("{name} {mode}: {e}"));
        legs.push((format!("hod-{mode}"), dir.write(&format!("{name}-{mode}.o"), bytes)));
    }
    for major in [22, 19] {
        if let Some(object) = llc(&dir, major, &format!("{name}-llc{major}"), &text) {
            legs.push((format!("llc{major}"), object));
        }
    }
    let Some(toolchain) = Toolchain::x86_64(&dir) else { return };
    for (leg, object) in legs {
        let code = link_and_run(&toolchain, &dir, &format!("{name}-{leg}"), c_main, &[object]);
        assert_eq!(code, Some(42), "{name}: {leg}");
    }
}

/// Constants of class `Mem` with addresses in them (`constaddr.rs`):
/// symbol addresses at aligned and unaligned offsets, of 8 and 4 bytes,
/// with addends, local and preemptible (GOT), functions; differences of
/// symbols as `Gpr` operands and inside images, truncated to 32 bits; a
/// number minus an address.
#[test]
fn constants_with_addresses_are_computed_in_code() {
    let ll = r"
@g = global [4 x i32] zeroinitializer
@h = dso_local global i64 0
@x = external global i8

define void @c1(ptr %out) {
  store { ptr, i32, ptr } { ptr @g, i32 7, ptr getelementptr (i8, ptr @h, i64 4) }, ptr %out
  ret void
}

define void @c2(ptr %out) {
  store <{ i8, ptr, i16, ptr }> <{ i8 1, ptr @x, i16 515, ptr getelementptr (i8, ptr @g, i64 -8) }>, ptr %out
  ret void
}

define void @c3(ptr %out) {
  store { i64, i32, [2 x ptr] } { i64 sub (i64 ptrtoint (ptr @g to i64), i64 ptrtoint (ptr @h to i64)), i32 trunc (i64 sub (i64 ptrtoint (ptr @x to i64), i64 ptrtoint (ptr @g to i64)) to i32), [2 x ptr] [ptr @c1, ptr null] }, ptr %out
  ret void
}

define void @c4(ptr %out) {
  %d = insertvalue { i64, i64 } undef, i64 sub (i64 ptrtoint (ptr @x to i64), i64 ptrtoint (ptr @h to i64)), 0
  %r = insertvalue { i64, i64 } %d, i64 sub (i64 100, i64 ptrtoint (ptr @h to i64)), 1
  store { i64, i64 } %r, ptr %out
  ret void
}
";
    let c_main = r"
#include <stdint.h>
#include <string.h>
extern int g[4];
extern long h;
char x;
void c1(void *); void c2(void *); void c3(void *); void c4(void *);
int main(void) {
  struct { void *a; int b; void *c; } s1;
  c1(&s1);
  if (s1.a != (void *)g || s1.b != 7 || s1.c != (char *)&h + 4) return 1;
  unsigned char s2[1 + 8 + 2 + 8];
  c2(s2);
  void *p; uint16_t k;
  memcpy(&p, s2 + 1, 8);
  if (s2[0] != 1 || p != (void *)&x) return 2;
  memcpy(&k, s2 + 9, 2);
  memcpy(&p, s2 + 11, 8);
  if (k != 515 || p != (char *)g - 8) return 3;
  struct { int64_t d; int32_t e; void *f[2]; } s3;
  c3(&s3);
  if (s3.d != (char *)g - (char *)&h || s3.e != (int32_t)((char *)&x - (char *)g)) return 4;
  if (s3.f[0] != (void *)c1 || s3.f[1] != 0) return 5;
  int64_t s4[2];
  c4(s4);
  if (s4[0] != (char *)&x - (char *)&h || s4[1] != 100 - (intptr_t)&h) return 6;
  return 42;
}
";
    every_leg_exits_42("constaddr", ll, c_main);
}

/// Values through the real convention, called from C: an aggregate
/// passed `byval` and returned in memory (`ret_memory`, the address back
/// in `rax`), constants returned in memory, `i128` pairs at every
/// register position and on the stack, wide integers split into pieces
/// between the registers and the stack, a struct of four pieces
/// returned in memory.
#[test]
fn values_pass_and_return_through_the_convention() {
    let ll = r"
define [5 x i64] @arr(ptr byval([5 x i64]) align 8 %p, i64 %k) {
  %v = load [5 x i64], ptr %p
  %r = insertvalue [5 x i64] %v, i64 %k, 2
  ret [5 x i64] %r
}

define [5 x i64] @arr_const(i64 %unused) {
  ret [5 x i64] [i64 1, i64 -2, i64 3, i64 4294967296, i64 5]
}

define { i64, i64, i64, i64 } @four(i128 %a, i128 %b) {
  %a0 = trunc i128 %a to i64
  %a1s = lshr i128 %a, 64
  %a1 = trunc i128 %a1s to i64
  %b0 = trunc i128 %b to i64
  %b1s = lshr i128 %b, 64
  %b1 = trunc i128 %b1s to i64
  %r0 = insertvalue { i64, i64, i64, i64 } undef, i64 %b1, 0
  %r1 = insertvalue { i64, i64, i64, i64 } %r0, i64 %a0, 1
  %r2 = insertvalue { i64, i64, i64, i64 } %r1, i64 %a1, 2
  %r3 = insertvalue { i64, i64, i64, i64 } %r2, i64 %b0, 3
  ret { i64, i64, i64, i64 } %r3
}

define i128 @pair_last(i64 %a, i64 %b, i64 %c, i64 %d, i64 %e, i128 %x) {
  %r = add i128 %x, 1
  ret i128 %r
}

define i128 @pair_after(i64 %a, i128 %x, i128 %y) {
  %r = sub i128 %x, %y
  ret i128 %r
}

define i256 @split(i64 %a, i64 %b, i64 %c, i256 %x, i256 %y) {
  %r = add i256 %x, %y
  ret i256 %r
}

define i200 @ret_poison(i64 %a) {
  ret i200 poison
}

define i256 @ret_const() {
  ret i256 u0x123456789abcdef0fedcba98765432100011223344556677
}
";
    let c_main = r"
#include <stdint.h>
typedef unsigned __int128 u128;
typedef struct { uint64_t w[5]; } W5;
typedef struct { uint64_t w[4]; } W4;
W5 arr(W5, uint64_t);
W5 arr_const(uint64_t);
W4 four(u128, u128);
u128 pair_last(uint64_t, uint64_t, uint64_t, uint64_t, uint64_t, u128);
u128 pair_after(uint64_t, u128, u128);
W4 split(uint64_t, uint64_t, uint64_t, uint64_t, uint64_t, uint64_t, uint64_t,
         uint64_t, uint64_t, uint64_t, uint64_t);
W4 ret_const(void);
int main(void) {
  W5 in = { { 10, 11, 12, 13, 14 } };
  W5 r = arr(in, 99);
  if (r.w[0] != 10 || r.w[1] != 11 || r.w[2] != 99 || r.w[3] != 13 || r.w[4] != 14) return 1;
  W5 k = arr_const(7);
  if (k.w[0] != 1 || k.w[1] != (uint64_t)-2 || k.w[2] != 3 || k.w[3] != 4294967296ULL || k.w[4] != 5) return 2;
  u128 a = ((u128)0x1111 << 64) | 0x2222, b = ((u128)0x3333 << 64) | 0x4444;
  W4 f = four(a, b);
  if (f.w[0] != 0x3333 || f.w[1] != 0x2222 || f.w[2] != 0x1111 || f.w[3] != 0x4444) return 3;
  u128 m = ((u128)5 << 64) | UINT64_MAX;
  if (pair_last(1, 2, 3, 4, 5, m) != ((u128)6 << 64)) return 4;
  if (pair_after(1, m, 1) != m - 1) return 5;
  W4 s = split(0, 0, 0, UINT64_MAX, UINT64_MAX, 7, 8, 1, 0, 0, 1);
  if (s.w[0] != 0 || s.w[1] != 0 || s.w[2] != 8 || s.w[3] != 9) return 6;
  W4 c = ret_const();
  if (c.w[0] != 0x0011223344556677ULL || c.w[1] != 0xfedcba9876543210ULL ||
      c.w[2] != 0x123456789abcdef0ULL || c.w[3] != 0) return 7;
  return 42;
}
";
    every_leg_exits_42("convention", ll, c_main);
}
