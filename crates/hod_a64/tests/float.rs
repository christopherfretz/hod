//! The floating-point and vector group (WP-039, `spec/codegen.md` §5.5):
//! instructions on `Fpr`-class values. Until the package lands, each entry
//! point is refused by its stable name.

mod common;

use common::assert_refused;
use hod_a64::float;

#[test]
fn the_group_is_refused_by_name_until_it_lands() {
    assert_refused(&[
        ("define double @f(double %a) { %r = fadd double %a, 1.0\n ret double %r }", float::BINARY),
        ("define <4 x i32> @f(<4 x i32> %a) { %r = add <4 x i32> %a, %a\n ret <4 x i32> %r }", float::BINARY),
        ("define double @f(double %a) { %r = fneg double %a\n ret double %r }", float::FNEG),
        ("define double @f(double %a) { %r = freeze double %a\n ret double %r }", float::FREEZE),
        ("define double @f(i64 %a) { %r = sitofp i64 %a to double\n ret double %r }", float::CAST),
        ("define i1 @f(double %a) { %r = fcmp olt double %a, 1.0\n ret i1 %r }", float::FCMP),
        (
            "define <4 x i32> @f(<4 x i32> %a) { %c = icmp eq <4 x i32> %a, %a\n %r = sext <4 x i1> %c to <4 x i32>\n ret <4 x i32> %r }",
            float::VECTOR_ICMP,
        ),
        ("define double @f(i1 %c, double %a) { %r = select i1 %c, double %a, double 0.0\n ret double %r }", float::SELECT),
    ]);
}
