//! The `Mem`-class group (WP-040, `spec/codegen.md` §5.5.1 "Mem-class
//! values"): integers wider than 64 bits, aggregates, `Mem` vectors and
//! the overflow intrinsics, checked natively against `hod_interp` and
//! `llc -O0 -global-isel=0 -fast-isel=0` (`common/aggr_gen.rs`); what is
//! refused, by name.

mod common;

#[path = "common/aggr_gen.rs"]
mod aggr_gen;

use aggr_gen::{Report, Rng, Suite, T, assert_agrees, run};
use common::{assert_refused, compile_ll};
use hod_a64::aggr;

/// Prints what a suite's run measured.
#[allow(clippy::cast_precision_loss, reason = "rates to print")]
fn summary(name: &str, suite: &Suite, report: &Report) {
    eprintln!(
        "a64 aggr {name}: {} functions, {} cases: {} against the interpreter, {} against llc only, \
         {} undefined (not compared), {} with garbage above the widths in registers; \
         {} IR instructions compiled in {:?} ({:.0} ns each); tools {:?}",
        suite.funcs.len(),
        suite.cases.len(),
        report.interp,
        report.llc_only,
        report.undefined,
        report.regs,
        report.insts,
        report.compile,
        report.compile.as_nanos() as f64 / report.insts.max(1) as f64,
        report.tools,
    );
}

fn check(name: &str, suite: &Suite) {
    let report = run(name, suite);
    summary(name, suite, &report);
    assert_agrees(&report);
}

/// Every binary operation at every wide width (division through the
/// runtime up to 128 bits), register and constant operands.
#[test]
fn every_wide_binary_operation_agrees() {
    check("binary", &aggr_gen::binary_table(&mut Rng(40)));
}

/// The bits above the width that producers leave behind, under every
/// consumer that depends on them.
#[test]
fn consumers_of_unextended_results_agree() {
    check("chain", &aggr_gen::chain_table(&mut Rng(41)));
}

/// Every predicate at every wide width; `select`, `freeze`, `phi`.
#[test]
fn every_wide_compare_and_select_agrees() {
    check("compare", &aggr_gen::compare_table(&mut Rng(42)));
}

/// Extensions and truncations between narrow and wide widths, pointer
/// casts, bitcasts between wide integers and vectors.
#[test]
fn every_wide_cast_agrees() {
    check("cast", &aggr_gen::cast_table(&mut Rng(43)));
}

/// Aggregates of every layout (padding, packed, nested, empty,
/// zero-length) built, read, selected, merged, loaded and stored;
/// constant aggregates; `Mem` vectors.
#[test]
fn every_aggregate_form_agrees() {
    check("aggregate", &aggr_gen::aggregate_table(&mut Rng(44)));
}

/// The six overflow intrinsics at every width up to 64, add and sub
/// wider.
#[test]
fn every_overflow_intrinsic_agrees() {
    check("overflow", &aggr_gen::overflow_table(&mut Rng(45)));
}

/// Random straight-line wide-integer functions. `HOD_AGGR_FUNCTIONS=<n>`
/// sets how many (default 400).
#[test]
fn random_wide_functions_agree() {
    let count =
        std::env::var("HOD_AGGR_FUNCTIONS").ok().and_then(|text| text.parse().ok()).unwrap_or(400);
    check("random", &aggr_gen::random_functions(&mut Rng(0x0400), count));
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

/// The layouts the generator's masks are computed from are `hod_ir`'s.
#[test]
fn the_generator_layouts_are_hod_irs() {
    let mut m = common::module();
    let v = |n, e| T::V(n, Box::new(e));
    let mut all = aggr_gen::aggregates();
    all.extend([
        v(3, T::I(32)),
        v(8, T::I(1)),
        v(5, T::I(1)),
        v(4, T::I(64)),
        T::I(65),
        T::I(129),
        T::I(17),
    ]);
    for ty in all {
        let id = type_id(&mut m, &ty);
        assert_eq!(m.types.size_of(id), ty.store(), "store size of {}", ty.ll());
        assert_eq!(m.types.alloc_size_of(id), ty.alloc(), "alloc size of {}", ty.ll());
        assert_eq!(m.types.align_of(id), ty.align(), "align of {}", ty.ll());
        if let T::S(..) = ty {
            assert_eq!(m.types.field_offsets(id).unwrap(), ty.offsets().as_slice(), "{}", ty.ll());
        }
    }
}

#[test]
fn what_is_refused_is_refused_by_name() {
    assert_refused(&[
        (
            "define i256 @f(i256 %a, i256 %b) { %r = udiv i256 %a, %b\n ret i256 %r }",
            aggr::WIDE_DIVISION,
        ),
        (
            "define i129 @f(i129 %a, i129 %b) { %r = srem i129 %a, %b\n ret i129 %r }",
            aggr::WIDE_DIVISION,
        ),
        (
            "declare { i128, i1 } @llvm.umul.with.overflow.i128(i128, i128)\ndefine { i128, i1 } @f(i128 %a) { %r = call { i128, i1 } @llvm.umul.with.overflow.i128(i128 %a, i128 3)\n ret { i128, i1 } %r }",
            aggr::WIDE_MUL_OVERFLOW,
        ),
        (
            "define i1 @f(ptr %p) { %v = load <3 x i32>, ptr %p\n %c = icmp eq <3 x i32> %v, zeroinitializer\n %r = extractelement <3 x i1> %c, i32 0\n ret i1 %r }",
            aggr::MEM_VECTOR,
        ),
        (
            "define double @f(i128 %a) { %r = sitofp i128 %a to double\n ret double %r }",
            aggr::WIDE_FP_CAST,
        ),
        (
            "define i128 @f(double %a) { %r = fptoui double %a to i128\n ret i128 %r }",
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
    ]);
    // Linear forms have no limit.
    let text = "define void @f(ptr %p, i8000 %a) { %s = lshr i8000 %a, 4001\n %r = add i8000 %s, %a\n store i8000 %r, ptr %p\n ret void }";
    assert_eq!(compile_ll(text), Ok(()));
}
