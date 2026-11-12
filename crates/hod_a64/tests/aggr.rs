//! The `Mem`-class group (WP-040, `spec/codegen.md` §5.5): wide integers,
//! aggregates, and the overflow intrinsics. Until the package lands, each
//! entry point is refused by its stable name.

mod common;

use common::assert_refused;
use hod_a64::aggr;

#[test]
fn the_group_is_refused_by_name_until_it_lands() {
    assert_refused(&[
        ("define i128 @f(i128 %a) { %r = add i128 %a, 1\n ret i128 %r }", aggr::BINARY),
        ("define i128 @f(i128 %a) { %r = freeze i128 %a\n ret i128 %r }", aggr::FREEZE),
        ("define i128 @f(i64 %a) { %r = sext i64 %a to i128\n ret i128 %r }", aggr::CAST),
        ("define i1 @f(i128 %a) { %r = icmp eq i128 %a, 0\n ret i1 %r }", aggr::ICMP),
        ("define i128 @f(ptr %p) { %r = load i128, ptr %p\n ret i128 %r }", aggr::LOAD),
        ("define void @f(ptr %p, i128 %a) { store i128 %a, ptr %p\n ret void }", aggr::STORE),
        (
            "define i128 @f(i1 %c, i128 %a) { %r = select i1 %c, i128 %a, i128 0\n ret i128 %r }",
            aggr::SELECT,
        ),
        (
            "define i64 @f({ i64, i64 } %a) { %r = extractvalue { i64, i64 } %a, 1\n ret i64 %r }",
            aggr::EXTRACT_VALUE,
        ),
        (
            "define { i64, i64 } @f({ i64, i64 } %a) { %r = insertvalue { i64, i64 } %a, i64 1, 0\n ret { i64, i64 } %r }",
            aggr::INSERT_VALUE,
        ),
        (
            "declare { i64, i1 } @llvm.sadd.with.overflow.i64(i64, i64)\ndefine { i64, i1 } @f(i64 %a) { %r = call { i64, i1 } @llvm.sadd.with.overflow.i64(i64 %a, i64 1)\n ret { i64, i1 } %r }",
            aggr::OVERFLOW,
        ),
    ]);
}
