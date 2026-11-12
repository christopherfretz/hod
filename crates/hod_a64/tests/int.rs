//! The integer group (WP-035, `spec/codegen.md` §5.5): binary operations,
//! casts, `icmp`, `select`, `freeze` on `Gpr`-class values. Until the
//! package lands, each entry point is refused by its stable name.

mod common;

use common::assert_refused;
use hod_a64::int;

#[test]
fn the_group_is_refused_by_name_until_it_lands() {
    assert_refused(&[
        ("define i64 @f(i64 %a) { %r = add i64 %a, 1\n ret i64 %r }", int::BINARY),
        ("define i32 @f(i32 %a) { %r = freeze i32 %a\n ret i32 %r }", int::FREEZE),
        ("define i64 @f(i32 %a) { %r = sext i32 %a to i64\n ret i64 %r }", int::CAST),
        ("define i1 @f(i64 %a) { %r = icmp eq i64 %a, 0\n ret i1 %r }", int::ICMP),
        ("define i64 @f(i1 %c, i64 %a) { %r = select i1 %c, i64 %a, i64 0\n ret i64 %r }", int::SELECT),
    ]);
}
