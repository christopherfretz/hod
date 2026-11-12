//! The call group (WP-038, `spec/codegen.md` §5.5): calls, `invoke`,
//! `musttail`, landing pads, `resume`, variadic definitions and `va_arg`.
//! Until the package lands, each entry point is refused by its stable
//! name.

mod common;

use common::assert_refused;
use hod_a64::call;

#[test]
fn the_group_is_refused_by_name_until_it_lands() {
    assert_refused(&[
        ("declare i64 @g()\ndefine i64 @f() { %r = call i64 @g()\n ret i64 %r }", call::CALL),
        ("define i64 @f(...) { ret i64 0 }", call::VARIADIC),
        ("define i64 @f(ptr %ap) { %r = va_arg ptr %ap, i64\n ret i64 %r }", call::VA_ARG),
        (
            "declare void @llvm.va_copy.p0(ptr, ptr)\ndefine void @f(ptr %a, ptr %b) { call void @llvm.va_copy.p0(ptr %a, ptr %b)\n ret void }",
            call::VA_INTRINSIC,
        ),
        ("define void @f(ptr %e) { resume { ptr, i32 } zeroinitializer }", call::RESUME),
    ]);
}
