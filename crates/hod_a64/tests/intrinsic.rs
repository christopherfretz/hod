//! The other intrinsics (WP-041, `spec/codegen.md` §5.5). Until the
//! package lands, they are refused by the group's stable name.

mod common;

use common::assert_refused;
use hod_a64::intrinsic;

#[test]
fn the_group_is_refused_by_name_until_it_lands() {
    assert_refused(&[
        (
            "declare void @llvm.trap()\ndefine void @f() { call void @llvm.trap()\n unreachable }",
            intrinsic::INTRINSIC,
        ),
        (
            "declare i64 @llvm.ctpop.i64(i64)\ndefine i64 @f(i64 %a) { %r = call i64 @llvm.ctpop.i64(i64 %a)\n ret i64 %r }",
            intrinsic::INTRINSIC,
        ),
    ]);
}
