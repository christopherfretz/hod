//! Tests of `blockaddress` and `indirectbr` (`spec/interp.md` §5.5).

mod common;

use common::{Fx, I1, I64, PTR, al, int, trap, value};
use hod_interp::{Trap, Val};
use hod_ir::{BlockId, IntPredicate, Opcode, Value, VariableDesc};

/// `i64 f(i64 k)`: a computed goto through a constant table of three
/// block addresses, indexed by `k`; each target returns its number plus
/// 10 if the loaded address equals `blockaddress(@f, %two)`. With `wild`,
/// the `indirectbr` lists only the first target.
fn computed_goto(wild: bool) -> (Fx, hod_ir::SymId) {
    let mut fx = Fx::new();
    let f = fx.declare("f", I64, &[I64]);
    let m = &mut fx.m;
    // Blocks 1 to 3 are created first in the body.
    let addrs: Vec<_> = (1..=3).map(|b| m.consts.block_addr(f, BlockId::new(b)).unwrap()).collect();
    let table_ty = m.types.array(PTR, 3).unwrap();
    let init = m.consts.aggregate(&m.types, table_ty, &addrs).unwrap();
    let desc = VariableDesc { init: Some(init), constant: true, ..VariableDesc::new(table_ty) };
    let table = m.add_variable(b"table", &desc).unwrap();
    let two = addrs[1];
    fx.define(f, |m, b| {
        let targets = [b.create_block()?, b.create_block()?, b.create_block()?];
        assert_eq!(targets[0], BlockId::new(1));
        let offset = b.ptr_add(PTR, Value::sym(table)?, 0, &[(b.arg(0)?, 8)])?;
        let addr = b.load(PTR, offset, al(8))?;
        let is_two = b.icmp(IntPredicate::Eq, I1, addr, Value::constant(two)?)?;
        let is_two = b.cast(Opcode::ZExt, I64, is_two)?;
        let ten = int(m, I64, 10);
        let bonus = b.binary(Opcode::Mul, I64, is_two, ten)?;
        b.indirect_br(addr, if wild { &targets[..1] } else { &targets })?;
        for (n, &target) in targets.iter().enumerate() {
            b.begin_block(target)?;
            let k = int(m, I64, n as u64 + 1);
            let r = b.binary(Opcode::Add, I64, bonus, k)?;
            b.ret(Some(r))?;
        }
        Ok(())
    });
    (fx, f)
}

/// §5.5: a `blockaddress` in a global table is a target of `indirectbr`
/// and equals the same constant in code.
#[test]
fn computed_goto_through_a_table() {
    let (fx, f) = computed_goto(false);
    assert_eq!(value(fx.run(f, &[Val::i64(0)])), Val::i64(1));
    assert_eq!(value(fx.run(f, &[Val::i64(1)])), Val::i64(12));
    assert_eq!(value(fx.run(f, &[Val::i64(2)])), Val::i64(3));
    // Past the table: the access traps before the branch.
    assert_eq!(trap(&fx.run(f, &[Val::i64(3)])), Trap::OutOfBounds);
}

/// §5.5: an `indirectbr` to an address it does not list is a trap.
#[test]
fn indirectbr_off_its_list() {
    let (fx, f) = computed_goto(true);
    assert_eq!(value(fx.run(f, &[Val::i64(0)])), Val::i64(1));
    assert_eq!(trap(&fx.run(f, &[Val::i64(1)])), Trap::IndirectBrTarget);
    // To something that is not a block at all.
    let mut fx = Fx::new();
    let f = fx.func("f", I64, &[], |m, b| {
        let target = b.create_block()?;
        let slot = b.add_slot(8, 8)?;
        b.indirect_br(b.slot(slot)?, &[target])?;
        b.begin_block(target)?;
        let one = int(m, I64, 1);
        b.ret(Some(one))?;
        Ok(())
    });
    assert_eq!(trap(&fx.run(f, &[])), Trap::IndirectBrTarget);
    // To a block of another function.
    let mut fx = Fx::new();
    let g = fx.func("g", I64, &[], |m, b| {
        let other = b.create_block()?;
        b.br(other)?;
        b.begin_block(other)?;
        let two = int(m, I64, 2);
        b.ret(Some(two))?;
        Ok(())
    });
    let f = fx.func("f", I64, &[], |m, b| {
        let target = b.create_block()?;
        let elsewhere = Value::constant(m.consts.block_addr(g, BlockId::new(1))?)?;
        b.indirect_br(elsewhere, &[target])?;
        b.begin_block(target)?;
        let one = int(m, I64, 1);
        b.ret(Some(one))?;
        Ok(())
    });
    assert_eq!(trap(&fx.run(f, &[])), Trap::IndirectBrTarget);
}
