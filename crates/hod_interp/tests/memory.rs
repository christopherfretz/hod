//! Tests of memory (`spec/interp.md` §4) and of the instructions and
//! intrinsics that touch it (§5.4, §5.6).

mod common;

use common::{
    DOUBLE, Fx, I1, I8, I16, I32, I64, PTR, VOID, al, call, eval, icall, int, trap, value,
};
use hod_interp::{Config, Outcome, Trap, Val};
use hod_ir::{
    AtomicOrdering, FunctionBuilder, IrResult, MemFlags, Module, Opcode, RmwOp, SyncScope, TypeId,
    Value, VariableDesc,
};

fn slot(b: &mut FunctionBuilder, size: u64, align: u64) -> IrResult<Value> {
    let slot = b.add_slot(size, align)?;
    b.slot(slot)
}

/// §4: memory is little-endian bytes; a scalar occupies its store size.
#[test]
fn scalars_are_little_endian_bytes() {
    let bytes = eval(I32, &[], &[], |m, b| {
        let p = slot(b, 8, 8)?;
        let wide = int(m, I64, 0x1122_3344_5566_7788);
        b.store(wide, p, al(8))?;
        let third = b.ptr_add(PTR, p, 2, &[])?;
        let low = b.load(I8, p, al(1))?;
        let mid = b.load(I16, third, al(2))?;
        let (low, mid) = (b.cast(Opcode::ZExt, I32, low)?, b.cast(Opcode::ZExt, I32, mid)?);
        let shift = int(m, I32, 8);
        let high = b.binary(Opcode::Shl, I32, mid, shift)?;
        b.binary(Opcode::Or, I32, high, low)
    });
    assert_eq!(value(bytes), Val::i32(0x0055_6688));
    let float = eval(I64, &[], &[], |m, b| {
        let p = slot(b, 8, 8)?;
        let one = common::double(m, 1.0);
        b.store(one, p, al(8))?;
        b.load(I64, p, al(8))
    });
    assert_eq!(value(float), Val::i64(0x3ff0_0000_0000_0000));
    let pointer = eval(I1, &[], &[], |_, b| {
        let p = slot(b, 8, 8)?;
        b.store(p, p, al(8))?;
        let back = b.load(PTR, p, al(8))?;
        b.icmp(hod_ir::IntPredicate::Eq, I1, back, p)
    });
    assert_eq!(value(pointer), Val::i1(true));
}

/// §10.1: the function that `llc -O0` miscompiles for AArch64. A loaded
/// `i16` with its top bit set, zero-extended to `i32`, is not negative
/// when that is sign-extended.
#[test]
fn zero_extended_load_then_sign_extended() {
    let result = eval(I64, &[], &[], |m, b| {
        let p = slot(b, 2, 2)?;
        let stored = int(m, I16, 0xeae0);
        b.store(stored, p, al(2))?;
        let i33 = m.types.int(33)?;
        let loaded = b.load(I16, p, al(2))?;
        let word = b.cast(Opcode::ZExt, I32, loaded)?;
        let odd = b.cast(Opcode::ZExt, i33, word)?;
        let signed = b.cast(Opcode::SExt, I64, word)?;
        let unsigned = b.cast(Opcode::ZExt, I64, odd)?;
        b.binary(Opcode::Or, I64, signed, unsigned)
    });
    assert_eq!(value(result), Val::i64(0xeae0));
}

/// §4: an integer whose width is not a whole number of bytes is stored
/// zero-extended to its store size, and a load of one from bytes with
/// other bits set has no one result (`spec/codegen.md` §2.2).
#[test]
fn odd_width_integers_in_memory() {
    let run = |bits: u32, stored: u64| {
        let mut fx = Fx::new();
        let narrow = fx.m.types.int(bits).unwrap();
        let f = fx.func("f", I64, &[I64], |m, b| {
            let p = slot(b, 8, 8)?;
            let ones = int(m, I64, u64::MAX);
            b.store(ones, p, al(8))?;
            let cut = b.cast(Opcode::Trunc, narrow, b.arg(0)?)?;
            b.store(cut, p, al(8))?;
            let whole = b.load(I64, p, al(8))?;
            b.ret(Some(whole))?;
            Ok(())
        });
        value(fx.run(f, &[Val::i64(stored)]))
    };
    // An i33 is five bytes: the top three keep the old ones.
    assert_eq!(run(33, u64::MAX), Val::i64(0xffff_ff01_ffff_ffff));
    assert_eq!(run(7, 0xff), Val::i64(0xffff_ffff_ffff_ff7f));
    assert_eq!(run(1, 3), Val::i64(0xffff_ffff_ffff_ff01));

    let load = |bits: u32, byte: u64| {
        let mut fx = Fx::new();
        let narrow = fx.m.types.int(bits).unwrap();
        let f = fx.func("f", narrow, &[], |m, b| {
            let p = slot(b, 8, 8)?;
            let stored = int(m, I64, byte);
            b.store(stored, p, al(8))?;
            let loaded = b.load(narrow, p, al(1))?;
            b.ret(Some(loaded))?;
            Ok(())
        });
        fx.run(f, &[])
    };
    assert_eq!(value(load(7, 0x7f)), Val::int(7, 0x7f));
    assert_eq!(trap(&load(7, 0x80)), Trap::NonZeroPadding);
    assert_eq!(value(load(1, 1)), Val::i1(true));
    assert_eq!(trap(&load(1, 2)), Trap::NonZeroPadding);
    assert_eq!(value(load(33, 0x1_0000_0001)), Val::int(33, 0x1_0000_0001));
    assert_eq!(trap(&load(33, 0x2_0000_0000)), Trap::NonZeroPadding);
}

fn access(build: impl FnOnce(&mut Module, &mut FunctionBuilder) -> IrResult<Value>) -> Outcome {
    eval(I32, &[I64], &[Val::i64(4)], build)
}

/// §4: an access must lie wholly inside one live allocation; the
/// addresses between allocations belong to none.
#[test]
fn out_of_bounds() {
    let seven = |m: &mut Module| int(m, I32, 7);
    let at = |offset: i64, store: bool| {
        access(|m, b| {
            let p = slot(b, 8, 4)?;
            let other = slot(b, 8, 4)?;
            let zero = int(m, I64, 0);
            b.store(zero, p, al(4))?;
            b.store(zero, other, al(4))?;
            let q = b.ptr_add(PTR, p, offset, &[])?;
            if store {
                let v = seven(m);
                b.store(v, q, al(1))?;
            }
            b.load(I32, q, al(1))
        })
    };
    assert_eq!(value(at(4, false)), Val::i32(0));
    assert_eq!(value(at(4, true)), Val::i32(7));
    for offset in [5, 8, -1, -4, 1 << 40] {
        assert_eq!(trap(&at(offset, false)), Trap::OutOfBounds, "load at {offset}");
        assert_eq!(trap(&at(offset, true)), Trap::OutOfBounds, "store at {offset}");
    }
    // A scaled index: the argument is 4, the element 4 bytes.
    let indexed = |scale: u64| {
        access(|m, b| {
            let p = slot(b, 20, 4)?;
            let zero = int(m, I32, 9);
            let last = b.ptr_add(PTR, p, 16, &[])?;
            b.store(zero, last, al(4))?;
            let q = b.ptr_add(PTR, p, 0, &[(b.arg(0)?, scale)])?;
            b.load(I32, q, al(4))
        })
    };
    assert_eq!(value(indexed(4)), Val::i32(9));
    assert_eq!(trap(&indexed(5)), Trap::OutOfBounds);
    // Null, and an integer that is no address at all.
    let null = access(|m, b| {
        let null = Value::constant(m.consts.null(&m.types, PTR)?)?;
        b.load(I32, null, al(4))
    });
    assert_eq!(trap(&null), Trap::OutOfBounds);
    let forged = access(|_, b| {
        let p = b.cast(Opcode::IntToPtr, PTR, b.arg(0)?)?;
        b.load(I32, p, al(4))
    });
    assert_eq!(trap(&forged), Trap::OutOfBounds);
    // A function's address is not data.
    let mut fx = Fx::new();
    let f = fx.func("f", I8, &[], |m, b| {
        let me = Value::sym(m.find_symbol(b"f").unwrap())?;
        let byte = b.load(I8, me, al(1))?;
        b.ret(Some(byte))?;
        Ok(())
    });
    assert_eq!(trap(&fx.run(f, &[])), Trap::OutOfBounds);
}

/// §5.4: `ptradd` sign-extends or truncates each index to 64 bits and
/// wraps; only an access through the result is checked.
#[test]
fn ptr_add_indices() {
    let run = |index_ty: TypeId, index: Val, scale: u64| {
        let mut fx = Fx::new();
        let f = fx.func("f", I32, &[index_ty], |m, b| {
            let p = slot(b, 16, 4)?;
            for at in 0..4 {
                let v = int(m, I32, 10 + at);
                let q = b.ptr_add(PTR, p, (at * 4) as i64, &[])?;
                b.store(v, q, al(4))?;
            }
            let end = b.ptr_add(PTR, p, 16, &[])?;
            let q = b.ptr_add(PTR, end, 0, &[(b.arg(0)?, scale)])?;
            let loaded = b.load(I32, q, al(4))?;
            b.ret(Some(loaded))?;
            Ok(())
        });
        fx.run(f, &[index])
    };
    // An i8 of 0xff is -1: one element back from the end.
    assert_eq!(value(run(I8, Val::i8(0xff), 4)), Val::i32(13));
    assert_eq!(value(run(I32, Val::i32(0xffff_fffc), 4)), Val::i32(10));
    assert_eq!(value(run(I64, Val::i64(u64::MAX - 1), 4)), Val::i32(12));
    // 2^64 + (2^64 - 2) truncates to -2.
    assert_eq!(
        value(run(common::I128, Val::i128((1 << 64) + u128::from(u64::MAX - 1)), 4)),
        Val::i32(12)
    );
    assert_eq!(trap(&run(I8, Val::i8(1), 4)), Trap::OutOfBounds);
    // Out and back in again without an access is fine.
    let roundabout = eval(I32, &[], &[], |m, b| {
        let p = slot(b, 4, 4)?;
        let v = int(m, I32, 5);
        b.store(v, p, al(4))?;
        let far = b.ptr_add(PTR, p, 1 << 20, &[])?;
        let back = b.ptr_add(PTR, far, -(1 << 20), &[])?;
        b.load(I32, back, al(4))
    });
    assert_eq!(value(roundabout), Val::i32(5));
}

/// §4: a byte can be loaded only after it was stored.
#[test]
fn uninitialized_loads() {
    let fresh = access(|_, b| {
        let p = slot(b, 4, 4)?;
        b.load(I32, p, al(4))
    });
    assert_eq!(trap(&fresh), Trap::UninitializedLoad);
    let partial = access(|m, b| {
        let p = slot(b, 4, 4)?;
        let v = int(m, I16, 1);
        b.store(v, p, al(2))?;
        b.load(I32, p, al(4))
    });
    assert_eq!(trap(&partial), Trap::UninitializedLoad);
    let alloca = access(|m, b| {
        let one = int(m, I64, 1);
        let p = b.alloca(PTR, one, 4, 4)?;
        b.load(I32, p, al(4))
    });
    assert_eq!(trap(&alloca), Trap::UninitializedLoad);
}

/// §4: an access may state only an alignment the allocation guarantees
/// and the address has.
#[test]
fn alignment() {
    let run = |slot_align: u64, offset: i64, access_align: u64| {
        access(|m, b| {
            let p = slot(b, 16, slot_align)?;
            let zero = int(m, I64, 0);
            let q = b.ptr_add(PTR, p, 8, &[])?;
            b.store(zero, p, al(1))?;
            b.store(zero, q, al(1))?;
            let at = b.ptr_add(PTR, p, offset, &[])?;
            b.load(I32, at, al(access_align))
        })
    };
    assert_eq!(value(run(4, 0, 4)), Val::i32(0));
    assert_eq!(value(run(4, 4, 4)), Val::i32(0));
    assert_eq!(value(run(4, 1, 1)), Val::i32(0));
    assert_eq!(value(run(1, 0, 1)), Val::i32(0));
    assert_eq!(trap(&run(4, 2, 4)), Trap::Misaligned);
    assert_eq!(trap(&run(4, 1, 2)), Trap::Misaligned);
    // The slot's address may happen to be a multiple of 8; nothing says
    // so, so an access may not rely on it.
    assert_eq!(trap(&run(4, 0, 8)), Trap::Misaligned);
    assert_eq!(trap(&run(1, 0, 2)), Trap::Misaligned);
    assert_eq!(value(run(16, 8, 8)), Val::i32(0));
    // Bounds come first.
    assert_eq!(trap(&run(4, 14, 4)), Trap::OutOfBounds);
}

/// §4: a frame's slots and dynamic allocations die when it returns, and
/// their addresses are not used again.
#[test]
fn use_after_return() {
    let run = |dynamic: bool, write: bool| {
        let mut fx = Fx::new();
        let leak = fx.func("leak", PTR, &[], |m, b| {
            let p = if dynamic {
                let one = int(m, I64, 1);
                b.alloca(PTR, one, 4, 4)?
            } else {
                slot(b, 4, 4)?
            };
            let v = int(m, I32, 1);
            b.store(v, p, al(4))?;
            b.ret(Some(p))?;
            Ok(())
        });
        let f = fx.func("f", I32, &[], |m, b| {
            let dangling = call(m, b, leak, &[])?;
            // A second call gets new addresses, not the dead ones.
            let _again = call(m, b, leak, &[])?;
            if write {
                let v = int(m, I32, 2);
                b.store(v, dangling, al(4))?;
            }
            let loaded = b.load(I32, dangling, al(4))?;
            b.ret(Some(loaded))?;
            Ok(())
        });
        trap(&fx.run(f, &[]))
    };
    for dynamic in [false, true] {
        for write in [false, true] {
            assert_eq!(run(dynamic, write), Trap::UseAfterFree, "{dynamic} {write}");
        }
    }
}

/// §5.4: `alloca` makes `count * elem_size` bytes that live until the
/// function returns.
#[test]
fn dynamic_allocation() {
    let run = |count: u64, index: u64| {
        eval(I32, &[I64, I64], &[Val::i64(count), Val::i64(index)], |m, b| {
            let p = b.alloca(PTR, b.arg(0)?, 4, 4)?;
            let q = b.ptr_add(PTR, p, 0, &[(b.arg(1)?, 4)])?;
            let v = int(m, I32, 42);
            b.store(v, q, al(4))?;
            b.load(I32, q, al(4))
        })
    };
    assert_eq!(value(run(3, 0)), Val::i32(42));
    assert_eq!(value(run(3, 2)), Val::i32(42));
    assert_eq!(trap(&run(3, 3)), Trap::OutOfBounds);
    assert_eq!(trap(&run(0, 0)), Trap::OutOfBounds);
    assert_eq!(trap(&run(u64::MAX, 0)), Trap::Memory);
    // Two allocations of nothing are still two addresses.
    let distinct = eval(I1, &[], &[], |m, b| {
        let zero = int(m, I64, 0);
        let (p, q) = (b.alloca(PTR, zero, 4, 4)?, b.alloca(PTR, zero, 4, 4)?);
        b.icmp(hod_ir::IntPredicate::Ne, I1, p, q)
    });
    assert_eq!(value(distinct), Val::i1(true));
}

/// A module with `@n = global i64 5`, `@table = constant [3 x i32]`,
/// `@pair = global { i8, i32, ptr, ptr } { 9, 77, @table + 4, @callee }`
/// and `i32 callee()` returning 31.
fn globals() -> (Fx, [hod_ir::SymId; 4], TypeId) {
    let mut fx = Fx::new();
    let m = &mut fx.m;
    let five = m.consts.int(&m.types, I64, 5).unwrap();
    let n = m.add_variable(b"n", &VariableDesc { init: Some(five), ..VariableDesc::new(I64) });
    let table_ty = m.types.array(I32, 3).unwrap();
    let elems: Vec<_> = [100, 200, 300].map(|v| m.consts.int(&m.types, I32, v).unwrap()).into();
    let table_init = m.consts.aggregate(&m.types, table_ty, &elems).unwrap();
    let table = m.add_variable(
        b"table",
        &VariableDesc { init: Some(table_init), constant: true, ..VariableDesc::new(table_ty) },
    );
    let (n, table) = (n.unwrap(), table.unwrap());
    let callee = fx.func("callee", I32, &[], |m, b| {
        let v = int(m, I32, 31);
        b.ret(Some(v))?;
        Ok(())
    });
    let m = &mut fx.m;
    let pair_ty = m.types.struct_type(&[I8, I32, PTR, PTR], false).unwrap();
    let fields = [
        m.consts.int(&m.types, I8, 9).unwrap(),
        m.consts.int(&m.types, I32, 77).unwrap(),
        m.consts.addr(table, 4).unwrap(),
        m.consts.addr(callee, 0).unwrap(),
    ];
    let pair_init = m.consts.aggregate(&m.types, pair_ty, &fields).unwrap();
    let pair = m
        .add_variable(
            b"pair",
            &VariableDesc { init: Some(pair_init), ..VariableDesc::new(pair_ty) },
        )
        .unwrap();
    (fx, [n, table, pair, callee], pair_ty)
}

/// §4: module variables start as their initializers, with the addresses
/// in them resolved; a `constant` one cannot be stored to; the padding of
/// an initializer was never written.
#[test]
fn module_variables() {
    let (mut fx, [n, table, pair, _], _) = globals();
    let bump = fx.func("bump", I64, &[], |m, b| {
        let n = Value::sym(n)?;
        let old = b.load(I64, n, al(8))?;
        let one = int(m, I64, 1);
        let new = b.binary(Opcode::Add, I64, old, one)?;
        b.store(new, n, al(8))?;
        b.ret(Some(new))?;
        Ok(())
    });
    // Every run starts from the initializers again.
    assert_eq!(value(fx.run(bump, &[])), Val::i64(6));
    assert_eq!(value(fx.run(bump, &[])), Val::i64(6));

    let through = fx.func("through", I32, &[], |m, b| {
        let pair = Value::sym(pair)?;
        let field = b.ptr_add(PTR, pair, 8, &[])?;
        let into_table = b.load(PTR, field, al(8))?;
        let second = b.load(I32, into_table, al(4))?;
        let field = b.ptr_add(PTR, pair, 16, &[])?;
        let callee = b.load(PTR, field, al(8))?;
        let fn_ty = m.types.function(I32, &[], false)?;
        let returned = b.call(I32, &hod_ir::CallDesc::new(callee, fn_ty, &[]))?;
        let field = b.ptr_add(PTR, pair, 4, &[])?;
        let middle = b.load(I32, field, al(4))?;
        let sum = b.binary(Opcode::Add, I32, second, returned)?;
        let sum = b.binary(Opcode::Add, I32, sum, middle)?;
        b.ret(Some(sum))?;
        Ok(())
    });
    assert_eq!(value(fx.run(through, &[])), Val::i32(200 + 31 + 77));

    let scribble = fx.func("scribble", VOID, &[], |m, b| {
        let v = int(m, I32, 1);
        b.store(v, Value::sym(table)?, al(4))?;
        b.ret(None)?;
        Ok(())
    });
    assert_eq!(trap(&fx.run(scribble, &[])), Trap::WriteToConstant);

    let padding = fx.func("padding", I8, &[I64], |_, b| {
        let at = b.ptr_add(PTR, Value::sym(pair)?, 0, &[(b.arg(0)?, 1)])?;
        let byte = b.load(I8, at, al(1))?;
        b.ret(Some(byte))?;
        Ok(())
    });
    assert_eq!(value(fx.run(padding, &[Val::i64(0)])), Val::i8(9));
    assert_eq!(trap(&fx.run(padding, &[Val::i64(1)])), Trap::UninitializedLoad);
    assert_eq!(value(fx.run(padding, &[Val::i64(4)])), Val::i8(77));
    assert_eq!(trap(&fx.run(padding, &[Val::i64(24)])), Trap::OutOfBounds);
}

/// §3, §5.4: first-class aggregates are images; a load or store of one
/// moves its scalars and leaves padding unwritten.
#[test]
fn aggregates() {
    let (mut fx, [_, table, pair, _], pair_ty) = globals();
    let table_ty = fx.m.types.array(I32, 3).unwrap();
    let whole = fx.func("whole", I32, &[], |m, b| {
        let loaded = b.load(table_ty, Value::sym(table)?, al(4))?;
        let v = int(m, I32, 5);
        let changed = b.insert_value(table_ty, loaded, v, 4)?;
        let (first, second) =
            (b.extract_value(I32, changed, 0)?, b.extract_value(I32, changed, 4)?);
        let third = b.extract_value(I32, loaded, 8)?;
        let sum = b.binary(Opcode::Add, I32, first, second)?;
        let sum = b.binary(Opcode::Add, I32, sum, third)?;
        b.ret(Some(sum))?;
        Ok(())
    });
    assert_eq!(value(fx.run(whole, &[])), Val::i32(100 + 5 + 300));

    let copy = fx.func("copy", I32, &[I64], |m, b| {
        let p = slot(b, 24, 8)?;
        let ones = int(m, I64, u64::MAX);
        b.store(ones, p, al(8))?;
        let loaded = b.load(pair_ty, Value::sym(pair)?, al(8))?;
        b.store(loaded, p, al(8))?;
        let at = b.ptr_add(PTR, p, 0, &[(b.arg(0)?, 1)])?;
        let byte = b.load(I8, at, al(1))?;
        let wide = b.cast(Opcode::ZExt, I32, byte)?;
        b.ret(Some(wide))?;
        Ok(())
    });
    assert_eq!(value(fx.run(copy, &[Val::i64(0)])), Val::i32(9));
    assert_eq!(value(fx.run(copy, &[Val::i64(4)])), Val::i32(77));
    // The store of the struct made the three bytes after the i8 padding
    // again, whatever was there.
    assert_eq!(trap(&fx.run(copy, &[Val::i64(2)])), Trap::UninitializedLoad);

    let returned = fx.func("returned", pair_ty, &[], |_, b| {
        let loaded = b.load(pair_ty, Value::sym(pair)?, al(8))?;
        b.ret(Some(loaded))?;
        Ok(())
    });
    let Val::Agg(agg) = value(fx.run(returned, &[])) else { panic!() };
    assert_eq!(agg.ty(), pair_ty);
    assert_eq!(agg.bytes().len(), 24);
    assert_eq!(agg.bytes()[..8], [9, 0, 0, 0, 77, 0, 0, 0]);
}

fn mem_intrinsic(name: &str, dst: i64, src: i64, len: u64, read_at: i64, written: u64) -> Outcome {
    eval(I8, &[], &[], |m, b| {
        let p = slot(b, 16, 8)?;
        // Bytes 0..written hold 1, 2, 3, ...
        for at in 0..written {
            let v = int(m, I8, at + 1);
            let q = b.ptr_add(PTR, p, at as i64, &[])?;
            b.store(v, q, al(1))?;
        }
        let (to, from) = (b.ptr_add(PTR, p, dst, &[])?, b.ptr_add(PTR, p, src, &[])?);
        let len = int(m, I64, len);
        let no = int(m, I1, 0);
        if name.starts_with("llvm.memset") {
            let byte = int(m, I8, 0xaa);
            icall(m, b, name, VOID, &[PTR, I8, I64, I1], &[to, byte, len, no])?;
        } else {
            icall(m, b, name, VOID, &[PTR, PTR, I64, I1], &[to, from, len, no])?;
        }
        let at = b.ptr_add(PTR, p, read_at, &[])?;
        b.load(I8, at, al(1))
    })
}

/// §5.6: `memcpy` (ranges equal or apart), `memmove` (any) and `memset`;
/// a copy carries unwritten bytes as unwritten.
#[test]
fn memory_intrinsics() {
    let copy = "llvm.memcpy.p0.p0.i64";
    let mv = "llvm.memmove.p0.p0.i64";
    let set = "llvm.memset.p0.i64";
    assert_eq!(value(mem_intrinsic(copy, 8, 0, 4, 10, 8)), Val::i8(3));
    assert_eq!(value(mem_intrinsic(copy, 4, 0, 4, 4, 8)), Val::i8(1));
    assert_eq!(value(mem_intrinsic(copy, 0, 0, 8, 7, 8)), Val::i8(8));
    assert_eq!(trap(&mem_intrinsic(copy, 2, 0, 4, 2, 8)), Trap::OverlappingCopy);
    assert_eq!(trap(&mem_intrinsic(copy, 0, 3, 4, 2, 8)), Trap::OverlappingCopy);
    // Forwards and backwards overlap: the source is read before written.
    assert_eq!(value(mem_intrinsic(mv, 2, 0, 6, 7, 8)), Val::i8(6));
    assert_eq!(value(mem_intrinsic(mv, 0, 2, 6, 0, 8)), Val::i8(3));
    assert_eq!(value(mem_intrinsic(mv, 2, 0, 6, 2, 8)), Val::i8(1));
    assert_eq!(value(mem_intrinsic(set, 4, 0, 8, 11, 0)), Val::i8(0xaa));
    assert_eq!(trap(&mem_intrinsic(set, 4, 0, 8, 12, 0)), Trap::UninitializedLoad);
    assert_eq!(trap(&mem_intrinsic(set, 9, 0, 8, 0, 8)), Trap::OutOfBounds);
    assert_eq!(trap(&mem_intrinsic(copy, 12, 0, 8, 0, 8)), Trap::OutOfBounds);
    assert_eq!(trap(&mem_intrinsic(copy, 0, 12, 8, 0, 8)), Trap::OutOfBounds);
    // Bytes 4..8 were never written; the copy is fine, the load is not.
    assert_eq!(value(mem_intrinsic(copy, 8, 0, 8, 11, 4)), Val::i8(4));
    assert_eq!(trap(&mem_intrinsic(copy, 8, 0, 8, 12, 4)), Trap::UninitializedLoad);
    // Nothing is touched by a length of zero, wherever it points.
    assert_eq!(value(mem_intrinsic(copy, 1 << 30, -5, 0, 0, 8)), Val::i8(1));
    assert_eq!(value(mem_intrinsic(set, 1 << 30, 0, 0, 0, 8)), Val::i8(1));
    assert_eq!(value(mem_intrinsic("llvm.memcpy.inline.p0.p0.i64", 8, 0, 4, 9, 8)), Val::i8(2));
    assert_eq!(value(mem_intrinsic("llvm.memset.inline.p0.i64", 0, 0, 2, 1, 8)), Val::i8(0xaa));
}

fn atomic() -> MemFlags {
    al(8).with_atomic(AtomicOrdering::SeqCst, SyncScope::System)
}

/// §5.4: atomic instructions have their sequential meaning.
#[test]
fn atomics_are_sequential() {
    let rmw = |op: RmwOp, old: u64, operand: u64| {
        value(eval(I64, &[I64, I64], &[Val::i64(old), Val::i64(operand)], |m, b| {
            let p = slot(b, 8, 8)?;
            b.store(b.arg(0)?, p, atomic())?;
            b.fence(AtomicOrdering::SeqCst, SyncScope::System)?;
            let seen = b.atomic_rmw(op, I64, p, b.arg(1)?, atomic())?;
            let now = b.load(I64, p, atomic())?;
            // Returns `seen * 1000 + now`.
            let k = int(m, I64, 1000);
            let scaled = b.binary(Opcode::Mul, I64, seen, k)?;
            b.binary(Opcode::Add, I64, scaled, now)
        }))
    };
    let both = |seen: u64, now: u64| Val::i64(seen.wrapping_mul(1000).wrapping_add(now));
    assert_eq!(rmw(RmwOp::Xchg, 5, 9), both(5, 9));
    assert_eq!(rmw(RmwOp::Add, 5, 9), both(5, 14));
    assert_eq!(rmw(RmwOp::Sub, 5, 9), both(5, (-4i64) as u64));
    assert_eq!(rmw(RmwOp::And, 12, 10), both(12, 8));
    assert_eq!(rmw(RmwOp::Nand, 12, 10), both(12, !8));
    assert_eq!(rmw(RmwOp::Or, 12, 10), both(12, 14));
    assert_eq!(rmw(RmwOp::Xor, 12, 10), both(12, 6));
    assert_eq!(rmw(RmwOp::Max, u64::MAX, 3), both(u64::MAX, 3));
    assert_eq!(rmw(RmwOp::Min, u64::MAX, 3), both(u64::MAX, u64::MAX));
    assert_eq!(rmw(RmwOp::UMax, u64::MAX, 3), both(u64::MAX, u64::MAX));
    assert_eq!(rmw(RmwOp::UMin, u64::MAX, 3), both(u64::MAX, 3));
    assert_eq!(rmw(RmwOp::UIncWrap, 3, 5), both(3, 4));
    assert_eq!(rmw(RmwOp::UIncWrap, 5, 5), both(5, 0));
    assert_eq!(rmw(RmwOp::UDecWrap, 3, 5), both(3, 2));
    assert_eq!(rmw(RmwOp::UDecWrap, 0, 5), both(0, 5));
    assert_eq!(rmw(RmwOp::UDecWrap, 9, 5), both(9, 5));
    assert_eq!(rmw(RmwOp::USubCond, 9, 5), both(9, 4));
    assert_eq!(rmw(RmwOp::USubCond, 3, 5), both(3, 3));
    assert_eq!(rmw(RmwOp::USubSat, 3, 5), both(3, 0));
    assert_eq!(rmw(RmwOp::USubSat, 9, 5), both(9, 4));

    let float = eval(DOUBLE, &[], &[], |m, b| {
        let p = slot(b, 8, 8)?;
        let (a, c) = (common::double(m, 1.5), common::double(m, 0.25));
        b.store(a, p, atomic())?;
        b.atomic_rmw(RmwOp::FAdd, DOUBLE, p, c, atomic())?;
        b.atomic_rmw(RmwOp::FSub, DOUBLE, p, a, atomic())?;
        b.load(DOUBLE, p, atomic())
    });
    assert_eq!(value(float), Val::f64(0.25));

    let cmpxchg = |expected: u64| {
        let mut fx = Fx::new();
        let pair = fx.m.types.struct_type(&[I64, I1], false).unwrap();
        let f = fx.func("f", I64, &[I64], |m, b| {
            let p = slot(b, 8, 8)?;
            let five = int(m, I64, 5);
            b.store(five, p, atomic())?;
            let nine = int(m, I64, 9);
            let result =
                b.cmpxchg(pair, p, b.arg(0)?, nine, atomic(), AtomicOrdering::SeqCst, false)?;
            let (old, ok) = (b.extract_value(I64, result, 0)?, b.extract_value(I1, result, 8)?);
            let now = b.load(I64, p, atomic())?;
            // Returns `ok * 10000 + old * 100 + now`.
            let ok = b.cast(Opcode::ZExt, I64, ok)?;
            let (k, h) = (int(m, I64, 10000), int(m, I64, 100));
            let ok = b.binary(Opcode::Mul, I64, ok, k)?;
            let old = b.binary(Opcode::Mul, I64, old, h)?;
            let sum = b.binary(Opcode::Add, I64, ok, old)?;
            let sum = b.binary(Opcode::Add, I64, sum, now)?;
            b.ret(Some(sum))?;
            Ok(())
        });
        value(fx.run(f, &[Val::i64(expected)]))
    };
    assert_eq!(cmpxchg(5), Val::i64(10000 + 500 + 9));
    assert_eq!(cmpxchg(6), Val::i64(500 + 5));
}

/// §7: live memory is bounded; module variables count.
#[test]
fn memory_limit() {
    let (mut fx, _, _) = globals();
    let f = fx.func("f", I64, &[I64], |_, b| {
        let p = b.alloca(PTR, b.arg(0)?, 1, 1)?;
        let addr = b.cast(Opcode::PtrToInt, I64, p)?;
        b.ret(Some(addr))?;
        Ok(())
    });
    // The variables are 8 + 12 + 24 bytes.
    let config = Config { max_memory: 100, ..Config::default() };
    let fits = fx.run_with(f, &[Val::i64(56)], config);
    assert_eq!(fits.peak_memory, 100);
    assert!(fits.result.is_ok());
    assert_eq!(trap(&fx.run_with(f, &[Val::i64(57)], config)), Trap::Memory);
    let config = Config { max_memory: 43, ..Config::default() };
    let outcome = fx.run_with(f, &[Val::i64(0)], config);
    assert_eq!(trap(&outcome), Trap::Memory);
    assert_eq!(outcome.stopped_at, None);
}
