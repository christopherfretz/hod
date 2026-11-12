//! Tests of the instructions that compute and that transfer control
//! (`spec/interp.md` §5.1 to §5.3, §5.5, §5.7), each with an expectation
//! worked out by hand.

mod common;

use common::{
    DOUBLE, FLOAT, Fx, I1, I8, I16, I32, I64, I128, PTR, al, double, eval, int, trap, value,
};
use hod_interp::{Config, NanPolicy, Outcome, Trap, Val};
use hod_ir::{FloatPredicate, IntPredicate, Opcode, TypeId, Value};

fn bin(op: Opcode, bits: u32, a: u128, b: u128) -> Outcome {
    let mut fx = Fx::new();
    let ty = fx.m.types.int(bits).unwrap();
    let f = fx.func("f", ty, &[ty, ty], |_, b| {
        let result = b.binary(op, ty, b.arg(0)?, b.arg(1)?)?;
        b.ret(Some(result))?;
        Ok(())
    });
    fx.run(f, &[Val::int(bits, a), Val::int(bits, b)])
}

fn fbin(op: Opcode, a: f64, b: f64, nan: NanPolicy) -> Outcome {
    let mut fx = Fx::new();
    let f = fx.func("f", DOUBLE, &[DOUBLE, DOUBLE], |_, b| {
        let result = b.binary(op, DOUBLE, b.arg(0)?, b.arg(1)?)?;
        b.ret(Some(result))?;
        Ok(())
    });
    fx.run_with(f, &[Val::f64(a), Val::f64(b)], Config { nan, ..Config::default() })
}

fn cast(op: Opcode, from: TypeId, to: TypeId, arg: Val) -> Outcome {
    eval(to, &[from], &[arg], |_, b| b.cast(op, to, b.arg(0)?))
}

/// §5.1: integer arithmetic wraps at the type's width, at every width
/// from 1 to 128.
#[test]
fn integer_arithmetic_wraps_at_the_width() {
    use Opcode::{AShr, Add, And, LShr, Mul, Or, SDiv, SRem, Shl, Sub, UDiv, URem, Xor};
    let cases: &[(Opcode, u32, u128, u128, u128)] = &[
        (Add, 8, 200, 100, 44),
        (Sub, 33, 0, 1, (1 << 33) - 1),
        (Mul, 64, u128::from(u64::MAX), 2, u128::from(u64::MAX) - 1),
        (Mul, 128, 1 << 64, 1 << 64, 0),
        (Mul, 128, (1 << 64) + 1, 1 << 63, (1 << 127) + (1 << 63)),
        (UDiv, 7, 100, 7, 14),
        // 100 is -28 as an i7; -28 / 7 = -4, which is 124.
        (SDiv, 7, 100, 7, 124),
        (URem, 7, 100, 7, 2),
        // -28 rem 5 = -3, which is 125.
        (SRem, 7, 100, 5, 125),
        (Shl, 16, 0x00ff, 12, 0xf000),
        (LShr, 16, 0x8000, 15, 1),
        (AShr, 16, 0x8000, 15, 0xffff),
        (And, 1, 1, 0, 0),
        (Or, 1, 1, 0, 1),
        (Xor, 1, 1, 1, 0),
        (Add, 1, 1, 1, 0),
        (Xor, 128, u128::MAX, 1, u128::MAX - 1),
    ];
    for &(op, bits, a, b, expected) in cases {
        assert_eq!(value(bin(op, bits, a, b)), Val::int(bits, expected), "{op:?} i{bits} {a} {b}");
    }
}

/// §5.1: the three integer traps: division by zero, the one signed
/// division that overflows, a shift by the width or more.
#[test]
fn integer_traps() {
    use Opcode::{AShr, LShr, SDiv, SRem, Shl, UDiv, URem};
    for op in [UDiv, SDiv, URem, SRem] {
        assert_eq!(trap(&bin(op, 32, 5, 0)), Trap::DivisionByZero, "{op:?}");
    }
    assert_eq!(trap(&bin(SDiv, 32, 0x8000_0000, 0xffff_ffff)), Trap::DivisionOverflow);
    assert_eq!(trap(&bin(SRem, 7, 0x40, 0x7f)), Trap::DivisionOverflow);
    assert_eq!(value(bin(SDiv, 32, 0x8000_0001, 0xffff_ffff)), Val::i32(0x7fff_ffff));
    for op in [Shl, LShr, AShr] {
        assert_eq!(trap(&bin(op, 8, 1, 8)), Trap::ShiftTooLarge, "{op:?}");
        assert_eq!(trap(&bin(op, 33, 1, 1 << 32)), Trap::ShiftTooLarge, "{op:?}");
        assert!(bin(op, 8, 1, 7).result.is_ok(), "{op:?}");
    }
    let stopped = bin(UDiv, 32, 5, 0);
    assert_eq!(stopped.stopped_at.map(|(_, inst)| inst.index()), Some(0));
    assert_eq!(stopped.steps, 1);
}

/// §5.1: float arithmetic is IEEE in the default environment, bit for
/// bit; `fneg` flips the sign bit and nothing else.
#[test]
fn float_arithmetic_is_ieee() {
    use Opcode::{FAdd, FDiv, FMul, FRem, FSub};
    let cases: &[(Opcode, f64, f64, f64)] = &[
        (FAdd, 1.5, 2.25, 3.75),
        (FSub, 1.0, 0.75, 0.25),
        (FMul, 1.5, -2.0, -3.0),
        (FDiv, 1.0, 4.0, 0.25),
        (FDiv, 1.0, 0.0, f64::INFINITY),
        (FDiv, -1.0, 0.0, f64::NEG_INFINITY),
        (FRem, 7.5, 2.0, 1.5),
        (FRem, -7.5, 2.0, -1.5),
        (FMul, -0.0, 5.0, -0.0),
    ];
    for &(op, a, b, expected) in cases {
        assert_eq!(value(fbin(op, a, b, NanPolicy::Trap)), Val::f64(expected), "{op:?} {a} {b}");
    }
    // 0.1f + 0.2f rounds to 0x3e99999a in binary32.
    let sum = eval(FLOAT, &[FLOAT, FLOAT], &[Val::f32(0.1), Val::f32(0.2)], |_, b| {
        b.binary(FAdd, FLOAT, b.arg(0)?, b.arg(1)?)
    });
    assert_eq!(value(sum), Val::Float(0x3e99_999a));
    let nan = Val::Double(0x7ff8_0000_0000_0001);
    let negated = eval(DOUBLE, &[DOUBLE], &[nan], |_, b| b.fneg(DOUBLE, b.arg(0)?));
    assert_eq!(value(negated), Val::Double(0xfff8_0000_0000_0001));
    let negated = eval(FLOAT, &[FLOAT], &[Val::f32(0.0)], |_, b| b.fneg(FLOAT, b.arg(0)?));
    assert_eq!(value(negated), Val::Float(0x8000_0000));
}

/// §5.1: an operation that computes a NaN has no one result: it traps,
/// unless the run asked for the host's NaN.
#[test]
fn computed_nan_traps_by_default() {
    use Opcode::{FAdd, FDiv, FMul, FRem, FSub};
    let inf = f64::INFINITY;
    for (op, a, b) in [
        (FDiv, 0.0, 0.0),
        (FSub, inf, inf),
        (FMul, 0.0, inf),
        (FRem, 1.0, 0.0),
        (FAdd, f64::NAN, 1.0),
    ] {
        assert_eq!(trap(&fbin(op, a, b, NanPolicy::Trap)), Trap::NanResult, "{op:?}");
        let Val::Double(bits) = value(fbin(op, a, b, NanPolicy::Host)) else { panic!() };
        assert!(f64::from_bits(bits).is_nan(), "{op:?}");
    }
    let narrowed = cast(Opcode::FPTrunc, DOUBLE, FLOAT, Val::f64(f64::NAN));
    assert_eq!(trap(&narrowed), Trap::NanResult);
    let widened = cast(Opcode::FPExt, FLOAT, DOUBLE, Val::f32(f32::NAN));
    assert_eq!(trap(&widened), Trap::NanResult);
}

/// §5.2: `icmp` on integers and on pointers (addresses, unsigned), `fcmp`
/// with a NaN operand, and `select`.
#[test]
fn compares_and_select() {
    let lt = |pred: IntPredicate, a: u8, b: u8| {
        value(eval(I1, &[I8, I8], &[Val::i8(a), Val::i8(b)], |_, b| {
            b.icmp(pred, I1, b.arg(0)?, b.arg(1)?)
        }))
    };
    assert_eq!(lt(IntPredicate::Slt, 0x80, 0), Val::i1(true));
    assert_eq!(lt(IntPredicate::Ult, 0x80, 0), Val::i1(false));
    assert_eq!(lt(IntPredicate::Eq, 3, 3), Val::i1(true));
    assert_eq!(lt(IntPredicate::Sge, 0x7f, 0x80), Val::i1(true));

    // Two slots are two addresses; a pointer into a slot is above its base.
    let pointers = eval(I32, &[], &[], |m, b| {
        let (p, q) = (b.add_slot(8, 8)?, b.add_slot(8, 8)?);
        let (p, q) = (b.slot(p)?, b.slot(q)?);
        let inside = b.ptr_add(PTR, p, 4, &[])?;
        let ne = b.icmp(IntPredicate::Ne, I1, p, q)?;
        let below = b.icmp(IntPredicate::Ult, I1, p, inside)?;
        let same = b.icmp(IntPredicate::Eq, I1, p, p)?;
        let null = Value::constant(m.consts.null(&m.types, PTR)?)?;
        let non_null = b.icmp(IntPredicate::Ne, I1, p, null)?;
        let mut bits = int(m, I32, 0);
        for flag in [ne, below, same, non_null] {
            let wide = b.cast(Opcode::ZExt, I32, flag)?;
            let two = int(m, I32, 2);
            bits = b.binary(Opcode::Mul, I32, bits, two)?;
            bits = b.binary(Opcode::Or, I32, bits, wide)?;
        }
        Ok(bits)
    });
    assert_eq!(value(pointers), Val::i32(0b1111));

    let fcmp = |pred: FloatPredicate, a: f64, b: f64| {
        value(eval(I1, &[DOUBLE, DOUBLE], &[Val::f64(a), Val::f64(b)], |_, b| {
            b.fcmp(pred, I1, b.arg(0)?, b.arg(1)?)
        }))
    };
    assert_eq!(fcmp(FloatPredicate::Olt, 1.0, 2.0), Val::i1(true));
    assert_eq!(fcmp(FloatPredicate::Oeq, f64::NAN, f64::NAN), Val::i1(false));
    assert_eq!(fcmp(FloatPredicate::Uno, f64::NAN, 1.0), Val::i1(true));
    assert_eq!(fcmp(FloatPredicate::Une, f64::NAN, 1.0), Val::i1(true));
    assert_eq!(fcmp(FloatPredicate::Oeq, 0.0, -0.0), Val::i1(true));
    let floats = eval(I1, &[FLOAT, FLOAT], &[Val::f32(2.0), Val::f32(1.0)], |_, b| {
        b.fcmp(FloatPredicate::Ogt, I1, b.arg(0)?, b.arg(1)?)
    });
    assert_eq!(value(floats), Val::i1(true));

    let select = |cond: bool| {
        let args = [Val::i1(cond), Val::f64(1.0), Val::f64(2.0)];
        value(eval(DOUBLE, &[I1, DOUBLE, DOUBLE], &args, |_, b| {
            b.select(DOUBLE, b.arg(0)?, b.arg(1)?, b.arg(2)?)
        }))
    };
    assert_eq!(select(true), Val::f64(1.0));
    assert_eq!(select(false), Val::f64(2.0));
}

/// §5.7: `undef` and `poison` constants are zero and `freeze` is a copy
/// (`spec/ir.md` §4.1, `spec/codegen.md` §4).
#[test]
fn undef_is_zero_and_freeze_copies() {
    let undef = eval(I32, &[I32], &[Val::i32(5)], |m, b| {
        let undef = Value::constant(m.consts.undef(&m.types, I32)?)?;
        let poison = Value::constant(m.consts.poison(&m.types, I32)?)?;
        let sum = b.binary(Opcode::Add, I32, b.arg(0)?, undef)?;
        let frozen = b.freeze(I32, poison)?;
        b.binary(Opcode::Add, I32, sum, frozen)
    });
    assert_eq!(value(undef), Val::i32(5));
    let frozen = eval(I64, &[I64], &[Val::i64(77)], |_, b| b.freeze(I64, b.arg(0)?));
    assert_eq!(value(frozen), Val::i64(77));
    let pointer = eval(PTR, &[], &[], |m, b| {
        let undef = Value::constant(m.consts.undef(&m.types, PTR)?)?;
        b.freeze(PTR, undef)
    });
    assert_eq!(value(pointer), Val::Ptr(0));
    let float = eval(DOUBLE, &[], &[], |m, b| {
        let undef = Value::constant(m.consts.undef(&m.types, DOUBLE)?)?;
        b.freeze(DOUBLE, undef)
    });
    assert_eq!(value(float), Val::f64(0.0));
}

/// §5.3: the integer casts, at odd widths too.
#[test]
fn integer_casts() {
    use Opcode::{SExt, Trunc, ZExt};
    let icast = |op: Opcode, from: u32, to: u32, arg: u128| {
        let mut fx = Fx::new();
        let (from_ty, to_ty) = (fx.m.types.int(from).unwrap(), fx.m.types.int(to).unwrap());
        let f = fx.func("f", to_ty, &[from_ty], |_, b| {
            let result = b.cast(op, to_ty, b.arg(0)?)?;
            b.ret(Some(result))?;
            Ok(())
        });
        value(fx.run(f, &[Val::int(from, arg)]))
    };
    assert_eq!(icast(Trunc, 64, 7, 0x1234_5678_9abc_def0), Val::int(7, 0x70));
    assert_eq!(icast(ZExt, 7, 64, 0x7f), Val::i64(127));
    assert_eq!(icast(SExt, 7, 64, 0x7f), Val::i64(u64::MAX));
    assert_eq!(icast(SExt, 7, 64, 0x3f), Val::i64(63));
    assert_eq!(icast(SExt, 7, 33, 0x40), Val::int(33, 0x1_ffff_ffc0));
    assert_eq!(icast(SExt, 1, 32, 1), Val::i32(u32::MAX));
    assert_eq!(icast(ZExt, 1, 8, 1), Val::i8(1));
    assert_eq!(icast(SExt, 64, 128, 1 << 63), Val::i128(0xffff_ffff_ffff_ffff_8000_0000_0000_0000));
    assert_eq!(icast(Trunc, 128, 16, (1 << 100) + 0x1_2345), Val::i16(0x2345));
}

/// §5.3: the float casts: conversions round to nearest, float to integer
/// rounds toward zero and traps when the result does not fit.
#[test]
fn float_casts() {
    use Opcode::{BitCast, FPExt, FPToSI, FPToUI, FPTrunc, SIToFP, UIToFP};
    assert_eq!(value(cast(FPExt, FLOAT, DOUBLE, Val::f32(1.5))), Val::f64(1.5));
    assert_eq!(value(cast(FPTrunc, DOUBLE, FLOAT, Val::f64(0.1))), Val::Float(0x3dcc_cccd));
    assert_eq!(value(cast(FPTrunc, DOUBLE, FLOAT, Val::f64(1e300))), Val::f32(f32::INFINITY));
    assert_eq!(value(cast(FPToSI, DOUBLE, I32, Val::f64(-3.7))), Val::i32(0xffff_fffd));
    assert_eq!(value(cast(FPToUI, DOUBLE, I8, Val::f64(3.9))), Val::i8(3));
    assert_eq!(value(cast(FPToUI, DOUBLE, I8, Val::f64(-0.5))), Val::i8(0));
    assert_eq!(value(cast(FPToSI, FLOAT, I8, Val::f32(-128.9))), Val::i8(0x80));
    assert_eq!(value(cast(FPToUI, DOUBLE, I1, Val::f64(1.0))), Val::i1(true));
    for (op, to, x) in [
        (FPToUI, I8, -1.0),
        (FPToUI, I8, 256.0),
        (FPToSI, I8, 128.0),
        (FPToSI, I8, -129.0),
        (FPToSI, I64, 9_223_372_036_854_775_808.0),
        (FPToSI, I32, f64::NAN),
        (FPToUI, I32, f64::INFINITY),
        (FPToSI, I1, 1.0),
    ] {
        let converted = cast(op, DOUBLE, to, Val::f64(x));
        assert_eq!(trap(&converted), Trap::FloatToIntOverflow, "{op:?} {x}");
    }
    assert_eq!(
        value(cast(UIToFP, I64, DOUBLE, Val::i64(u64::MAX))),
        Val::f64(18_446_744_073_709_551_616.0)
    );
    assert_eq!(value(cast(SIToFP, I64, DOUBLE, Val::i64(u64::MAX))), Val::f64(-1.0));
    assert_eq!(value(cast(UIToFP, I1, DOUBLE, Val::i1(true))), Val::f64(1.0));
    assert_eq!(value(cast(SIToFP, I1, DOUBLE, Val::i1(true))), Val::f64(-1.0));
    // 2^24 + 1 is halfway between two floats; the even one is 2^24.
    assert_eq!(value(cast(UIToFP, I32, FLOAT, Val::i32(16_777_217))), Val::f32(16_777_216.0));
    assert_eq!(value(cast(BitCast, I32, FLOAT, Val::i32(0x3f80_0000))), Val::f32(1.0));
    assert_eq!(value(cast(BitCast, DOUBLE, I64, Val::f64(-2.0))), Val::i64(0xc000_0000_0000_0000));
    assert_eq!(value(cast(BitCast, I64, DOUBLE, Val::i64(1 << 63))), Val::f64(-0.0));
    assert_eq!(value(cast(BitCast, FLOAT, I32, Val::Float(0x7fc0_0001))), Val::i32(0x7fc0_0001));
}

/// §5.3: pointers are 64-bit addresses: `ptrtoint` truncates or zero
/// extends, `inttoptr` is its inverse, and a pointer that went through an
/// integer still reaches its allocation.
#[test]
fn pointer_casts() {
    let through = eval(I32, &[], &[], |m, b| {
        let slot = b.add_slot(4, 4)?;
        let p = b.slot(slot)?;
        let seven = int(m, I32, 7);
        b.store(seven, p, al(4))?;
        let addr = b.cast(Opcode::PtrToInt, I64, p)?;
        let one = int(m, I64, 1);
        let moved = b.binary(Opcode::Add, I64, addr, one)?;
        let back = b.binary(Opcode::Sub, I64, moved, one)?;
        let q = b.cast(Opcode::IntToPtr, PTR, back)?;
        b.load(I32, q, al(4))
    });
    assert_eq!(value(through), Val::i32(7));
    let low = eval(I1, &[], &[], |_, b| {
        let slot = b.add_slot(4, 4)?;
        let p = b.slot(slot)?;
        let narrow = b.cast(Opcode::PtrToInt, I16, p)?;
        let wide = b.cast(Opcode::PtrToInt, I64, p)?;
        let cut = b.cast(Opcode::Trunc, I16, wide)?;
        b.icmp(IntPredicate::Eq, I1, narrow, cut)
    });
    assert_eq!(value(low), Val::i1(true));
    let widened = cast(Opcode::IntToPtr, I8, PTR, Val::i8(0xff));
    assert_eq!(value(widened), Val::Ptr(0xff));
    let narrowed = cast(Opcode::IntToPtr, I128, PTR, Val::i128((1 << 64) + 5));
    assert_eq!(value(narrowed), Val::Ptr(5));
    let addr = cast(Opcode::PtrToAddr, PTR, I64, Val::Ptr(0x1234));
    assert_eq!(value(addr), Val::i64(0x1234));
}

/// The loop of §5.5's test: `a` and `b` trade places on every back edge.
/// Returns `a * 1000 + b` after `n` entries of the loop block, which is
/// its own successor.
fn swap_loop(n: u64) -> Outcome {
    let mut fx = Fx::new();
    let f = fx.func("f", I64, &[I64, I64, I64], |m, b| {
        let (looping, exit) = (b.create_block()?, b.create_block()?);
        let entry = b.current_block().unwrap();
        b.br(looping)?;
        b.begin_block(looping)?;
        let zero = int(m, I64, 0);
        let x = b.phi(I64, &[(b.arg(0)?, entry)])?;
        let y = b.phi(I64, &[(b.arg(1)?, entry)])?;
        let i = b.phi(I64, &[(zero, entry)])?;
        let one = int(m, I64, 1);
        let next = b.binary(Opcode::Add, I64, i, one)?;
        b.add_incoming(x, y, looping)?;
        b.add_incoming(y, x, looping)?;
        b.add_incoming(i, next, looping)?;
        let again = b.icmp(IntPredicate::Ult, I1, next, b.arg(2)?)?;
        b.cond_br(again, looping, exit)?;
        b.begin_block(exit)?;
        let thousand = int(m, I64, 1000);
        let scaled = b.binary(Opcode::Mul, I64, x, thousand)?;
        let result = b.binary(Opcode::Add, I64, scaled, y)?;
        b.ret(Some(result))?;
        Ok(())
    });
    fx.run(f, &[Val::i64(1), Val::i64(2), Val::i64(n)])
}

/// §5.5: the `phi`s of a block are one parallel copy: a swap cycle swaps,
/// in a block that is its own successor; an instruction is one step and
/// so is each `phi` on an edge taken.
#[test]
fn phis_are_a_parallel_copy() {
    let once = swap_loop(1);
    // br; three phis; add, icmp, condbr; mul, add, ret.
    assert_eq!(once.steps, 10);
    assert_eq!(value(once), Val::i64(1002));
    let twice = swap_loop(2);
    assert_eq!(twice.steps, 16);
    assert_eq!(value(twice), Val::i64(2001));
    assert_eq!(value(swap_loop(3)), Val::i64(1002));
    assert_eq!(value(swap_loop(4)), Val::i64(2001));
}

/// §5.5: `switch` goes to the first case equal to the condition, else the
/// default; a block that is a successor twice has one `phi` entry per
/// edge.
#[test]
fn switch_and_repeated_successors() {
    let run = |x: u32| {
        let mut fx = Fx::new();
        let f = fx.func("f", I32, &[I32], |m, b| {
            let entry = b.current_block().unwrap();
            let (shared, five, other) = (b.create_block()?, b.create_block()?, b.create_block()?);
            let join = b.create_block()?;
            let case = |m: &mut hod_ir::Module, v: u64| m.consts.int(&m.types, I32, v);
            let cases =
                [(case(m, 1)?, shared), (case(m, 5)?, five), (case(m, 0x8000_0000)?, shared)];
            b.switch(b.arg(0)?, other, &cases)?;
            b.begin_block(shared)?;
            // Entered by two edges of the one switch: two entries.
            let hundred = int(m, I32, 100);
            let seen = b.phi(I32, &[(hundred, entry), (hundred, entry)])?;
            b.br(join)?;
            b.begin_block(five)?;
            let yes = Value::constant(m.consts.int(&m.types, I1, 1)?)?;
            // Both ways to one block.
            b.cond_br(yes, join, join)?;
            b.begin_block(other)?;
            b.br(join)?;
            b.begin_block(join)?;
            let (two, three) = (int(m, I32, 200), int(m, I32, 300));
            let result = b.phi(I32, &[(seen, shared), (two, five), (two, five), (three, other)])?;
            b.ret(Some(result))?;
            Ok(())
        });
        value(fx.run(f, &[Val::i32(x)]))
    };
    assert_eq!(run(1), Val::i32(100));
    assert_eq!(run(0x8000_0000), Val::i32(100));
    assert_eq!(run(5), Val::i32(200));
    assert_eq!(run(0), Val::i32(300));
    assert_eq!(run(6), Val::i32(300));
}

/// §5.5: `unreachable` traps, and so does the use of an instruction that
/// did not run (a definition that does not dominate its use).
#[test]
fn unreachable_and_undefined_values() {
    let run = |through: bool| {
        let mut fx = Fx::new();
        let f = fx.func("f", I32, &[I1, I1], |m, b| {
            let (side, join, dead) = (b.create_block()?, b.create_block()?, b.create_block()?);
            b.cond_br(b.arg(0)?, side, join)?;
            b.begin_block(side)?;
            let one = int(m, I32, 1);
            let computed = b.binary(Opcode::Add, I32, one, one)?;
            b.cond_br(b.arg(1)?, dead, join)?;
            b.begin_block(join)?;
            b.ret(Some(computed))?;
            b.begin_block(dead)?;
            b.unreachable()?;
            Ok(())
        });
        (fx.run(f, &[Val::i1(through), Val::i1(false)]), fx.run(f, &[Val::i1(true), Val::i1(true)]))
    };
    let (joined, dead) = run(true);
    assert_eq!(value(joined), Val::i32(2));
    assert_eq!(trap(&dead), Trap::Unreachable);
    let (skipped, _) = run(false);
    assert_eq!(trap(&skipped), Trap::UndefinedValue);
}

/// §5.3: a narrow value is what its type says wherever it came from: the
/// bits above an `i16` computed in an `i64` do not survive a `trunc`.
#[test]
fn narrow_values_stay_narrow() {
    let narrow = eval(I64, &[I64], &[Val::i64(0x1_ffff)], |m, b| {
        let cut = b.cast(Opcode::Trunc, I16, b.arg(0)?)?;
        let one = int(m, I16, 1);
        let wrapped = b.binary(Opcode::Add, I16, cut, one)?;
        b.cast(Opcode::ZExt, I64, wrapped)
    });
    assert_eq!(value(narrow), Val::i64(0));
    let d = eval(DOUBLE, &[], &[], |m, b| {
        let half = double(m, 0.5);
        b.binary(Opcode::FAdd, DOUBLE, half, half)
    });
    assert_eq!(value(d), Val::f64(1.0));
}
