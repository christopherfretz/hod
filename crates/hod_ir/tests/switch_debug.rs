//! `Switch`'s `Debug` (WP-064): the cases as `(constant, block)` pairs of
//! typed ids, not the raw words of the body's tables.

use hod_ir::{FunctionBuilder, FunctionDesc, InstKind, Module, TargetSpec, TypeId};

#[test]
fn a_switch_debugs_as_typed_cases() {
    let mut m = Module::new(TargetSpec::aarch64_linux_gnu());
    let fn_ty = m.types.function(TypeId::VOID, &[TypeId::I32], false).unwrap();
    let f =
        m.add_function(b"f", &FunctionDesc { has_body: true, ..FunctionDesc::new(fn_ty) }).unwrap();
    // Constants ahead of the cases', so that their ids are not 0 and 1.
    for value in 10..13 {
        m.consts.int(&m.types, TypeId::I64, value).unwrap();
    }
    let seven = m.consts.int(&m.types, TypeId::I32, 7).unwrap();
    let nine = m.consts.int(&m.types, TypeId::I32, 9).unwrap();
    let mut b = FunctionBuilder::new(&m, f, fn_ty).unwrap();
    let [entry, a, other] = [(); 3].map(|()| b.create_block().unwrap());
    b.begin_block(entry).unwrap();
    let x = b.arg(0).unwrap();
    b.switch(x, other, &[(seven, a), (nine, other)]).unwrap();
    for block in [a, other] {
        b.begin_block(block).unwrap();
        b.ret(None).unwrap();
    }
    let body = b.finish().unwrap();
    body.verify(&m).unwrap();
    let term = body.block(entry).terminator();
    let InstKind::Switch(switch) = term.kind() else { panic!("no switch") };
    let text = format!("{switch:?}");
    let want = format!(
        "Switch {{ cond: {x:?}, default: {other:?}, cases: [({seven:?}, {a:?}), ({nine:?}, {other:?})] }}"
    );
    assert_eq!(text, want);
    assert_eq!(text, "Switch { cond: arg0, default: bb2, cases: [(c3, bb1), (c4, bb2)] }");
    // The alternate form lists one case per line.
    assert!(format!("{switch:#?}").contains("(\n            c3,\n            bb1,\n        ),"));
}
