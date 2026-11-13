//! `Phi::value_for` and the address-taken mark (WP-064, `spec/ir.md`
//! §4.2, §5.9): the value of a predecessor's edge, the first listed when
//! a `switch` gives it several; the blocks whose `blockaddress` was
//! interned, once each, in id order, per function.

use hod_ir::{
    BlockId, ConstId, FunctionBuilder, FunctionDesc, InstKind, Module, SymId, TargetSpec, TypeId,
    Value,
};

fn module() -> (Module, SymId, SymId) {
    let mut m = Module::new(TargetSpec::aarch64_linux_gnu());
    let fn_ty = m.types.function(TypeId::I32, &[TypeId::I32], false).unwrap();
    let f =
        m.add_function(b"f", &FunctionDesc { has_body: true, ..FunctionDesc::new(fn_ty) }).unwrap();
    let g =
        m.add_function(b"g", &FunctionDesc { has_body: true, ..FunctionDesc::new(fn_ty) }).unwrap();
    (m, f, g)
}

// §5.9: `value_for` gives each predecessor's value; a predecessor with
// two edges (a `switch`) is listed twice with the same value, and the
// first is answered; a block that is no predecessor has none.
#[test]
fn value_for_answers_each_predecessor() {
    let (mut m, f, _) = module();
    let one = m.consts.int(&m.types, TypeId::I32, 1).unwrap();
    let two = m.consts.int(&m.types, TypeId::I32, 2).unwrap();
    let seven = Value::constant(m.consts.int(&m.types, TypeId::I32, 7).unwrap()).unwrap();
    let fn_ty = m.symbol(f).ty();
    let mut b = FunctionBuilder::new(&m, f, fn_ty).unwrap();
    let [entry, other, join] = [(); 3].map(|()| b.create_block().unwrap());
    b.begin_block(entry).unwrap();
    let x = b.arg(0).unwrap();
    b.switch(x, other, &[(one, join), (two, join)]).unwrap();
    b.begin_block(other).unwrap();
    b.br(join).unwrap();
    b.begin_block(join).unwrap();
    let phi = b.phi(TypeId::I32, &[(x, entry), (x, entry), (seven, other)]).unwrap();
    b.ret(Some(phi)).unwrap();
    let body = b.finish().unwrap();
    body.verify(&m).unwrap();
    let phi_inst = body.block(join).phis().next().unwrap();
    let InstKind::Phi(list) = phi_inst.kind() else { panic!("no phi") };
    assert_eq!(list.value_for(entry), Some(x));
    assert_eq!(list.value_for(other), Some(seven));
    assert_eq!(list.value_for(join), None);
    for (value, pred) in list.incoming() {
        assert_eq!(list.value_for(pred), Some(value));
    }
}

// §4.2: the mark lists each `blockaddress` once, in id order; it tells
// the functions apart, and other constants do not enter it.
#[test]
fn the_address_taken_mark_lists_each_blockaddress_once() {
    let (mut m, f, g) = module();
    assert_eq!(m.consts.block_addresses().count(), 0);
    assert!(!m.consts.has_address_taken_block(f));
    let b3 = BlockId::new(3);
    let b1 = BlockId::new(1);
    let first = m.consts.block_addr(f, b3).unwrap();
    m.consts.int(&m.types, TypeId::I32, 5).unwrap();
    m.consts.addr(g, 0).unwrap();
    let second = m.consts.block_addr(g, b1).unwrap();
    // Interned again: the same constant, not listed again.
    assert_eq!(m.consts.block_addr(f, b3).unwrap(), first);
    let third = m.consts.block_addr(f, b1).unwrap();
    let listed: Vec<(ConstId, SymId, BlockId)> = m.consts.block_addresses().collect();
    assert_eq!(listed, [(first, f, b3), (second, g, b1), (third, f, b1)]);
    assert!(m.consts.is_address_taken(f, b3) && m.consts.is_address_taken(f, b1));
    assert!(m.consts.is_address_taken(g, b1) && !m.consts.is_address_taken(g, b3));
    assert!(!m.consts.is_address_taken(f, BlockId::new(0)));
    assert!(m.consts.has_address_taken_block(f) && m.consts.has_address_taken_block(g));
    // A clone of the module keeps the mark.
    let copy = m.clone();
    assert_eq!(copy.consts.block_addresses().collect::<Vec<_>>(), listed);
}
