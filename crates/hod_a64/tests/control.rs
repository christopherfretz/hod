//! The control group (WP-037, `spec/codegen.md` §5.5): `br i1`, `switch`,
//! `indirectbr`, `blockaddress`. Until the package lands, each entry point
//! is refused by its stable name.

mod common;

use common::assert_refused;
use hod_a64::control;

#[test]
fn the_group_is_refused_by_name_until_it_lands() {
    assert_refused(&[
        (
            "define i64 @f(i1 %c) {\n br i1 %c, label %a, label %b\na:\n ret i64 1\nb:\n ret i64 2\n}",
            control::COND_BR,
        ),
        (
            "define i64 @f(i64 %x) {\n switch i64 %x, label %a [ i64 1, label %b ]\na:\n ret i64 1\nb:\n ret i64 2\n}",
            control::SWITCH,
        ),
        (
            "define i64 @f(ptr %p) {\n indirectbr ptr %p, [label %a]\na:\n ret i64 1\n}",
            control::INDIRECT_BR,
        ),
    ]);
}
