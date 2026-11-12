//! The memory group (WP-036, `spec/codegen.md` §5.5): `load`, `store`,
//! `alloca`, `ptradd`, `llvm.stacksave`/`llvm.stackrestore`. Until the
//! package lands, each entry point is refused by its stable name.

mod common;

use common::assert_refused;
use hod_a64::mem;

#[test]
fn the_group_is_refused_by_name_until_it_lands() {
    assert_refused(&[
        ("define i64 @f(ptr %p) { %r = load i64, ptr %p\n ret i64 %r }", mem::LOAD),
        ("define void @f(ptr %p) { store i64 1, ptr %p\n ret void }", mem::STORE),
        ("define ptr @f(i64 %n) { %r = alloca i8, i64 %n\n ret ptr %r }", mem::ALLOCA),
        (
            "define ptr @f(ptr %p, i64 %i) { %r = getelementptr i8, ptr %p, i64 %i\n ret ptr %r }",
            mem::PTRADD,
        ),
        (
            "declare ptr @llvm.stacksave.p0()\ndefine ptr @f() { %r = call ptr @llvm.stacksave.p0()\n ret ptr %r }",
            mem::STACK_INTRINSIC,
        ),
    ]);
}
