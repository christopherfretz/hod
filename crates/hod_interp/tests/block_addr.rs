//! Tests of `blockaddress` and `indirectbr` (`spec/interp.md` §5.5).

mod common;

use common::{Fx, I1, I64, PTR, al, int, trap, value};
use hod_interp::{Trap, Val};
use hod_ir::{BlockId, IntPredicate, Opcode, Value, VariableDesc};

/// `i64 f(i64 k)`: a computed goto through a constant table of three
/// block addresses, indexed by `k`; each target returns its number plus
/// 10 if the loaded address is not `null`, or, with `equal`, if it equals
/// `blockaddress(@f, %two)`. With `wild`, the `indirectbr` lists only the
/// first target.
fn computed_goto(wild: bool, equal: bool) -> (Fx, hod_ir::SymId) {
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
        let null = Value::constant(m.consts.null(&m.types, PTR)?)?;
        let is_two = if equal {
            b.icmp(IntPredicate::Eq, I1, addr, Value::constant(two)?)?
        } else {
            b.icmp(IntPredicate::Ne, I1, addr, null)?
        };
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
/// and is not `null`.
#[test]
fn computed_goto_through_a_table() {
    let (fx, f) = computed_goto(false, false);
    assert_eq!(value(fx.run(f, &[Val::i64(0)])), Val::i64(11));
    assert_eq!(value(fx.run(f, &[Val::i64(1)])), Val::i64(12));
    assert_eq!(value(fx.run(f, &[Val::i64(2)])), Val::i64(13));
    // Past the table: the access traps before the branch.
    assert_eq!(trap(&fx.run(f, &[Val::i64(3)])), Trap::OutOfBounds);
}

/// §5.5: LLVM defines no comparison of block addresses but with `null`
/// (`llc -O2` folds an equality of two to what it likes), and no
/// conversion of one to an integer.
#[test]
fn inspecting_a_block_address() {
    let (fx, f) = computed_goto(false, true);
    assert_eq!(trap(&fx.run(f, &[Val::i64(1)])), Trap::BlockAddressInspected);
    let mut fx = Fx::new();
    let f = fx.declare("f", I64, &[]);
    let addr = fx.m.consts.block_addr(f, BlockId::new(1)).unwrap();
    fx.define(f, |_, b| {
        let next = b.create_block()?;
        let bits = b.cast(Opcode::PtrToInt, I64, Value::constant(addr)?)?;
        b.br(next)?;
        b.begin_block(next)?;
        b.ret(Some(bits))?;
        Ok(())
    });
    assert_eq!(trap(&fx.run(f, &[])), Trap::BlockAddressInspected);
}

/// §5.5: an `indirectbr` to an address it does not list is a trap.
#[test]
fn indirectbr_off_its_list() {
    let (fx, f) = computed_goto(true, false);
    assert_eq!(value(fx.run(f, &[Val::i64(0)])), Val::i64(11));
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
