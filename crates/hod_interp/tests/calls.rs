//! Tests of calls (`spec/interp.md` §5.8), integer and float intrinsics
//! (§5.6), host functions and the trace (§6), limits (§7) and what a run
//! refuses as not well formed (§2).

mod common;

use std::cell::RefCell;

use common::{
    DOUBLE, FLOAT, Fx, I1, I8, I16, I32, I64, PTR, VOID, al, call, eval, icall, int, sint, trap,
    unsupported, value,
};
use hod_interp::{Config, Event, Interp, Program, Stop, Trap, Val, host};
use hod_ir::{CallConv, CallDesc, IntPredicate, Module, Opcode, SymId, TypeId, Value};

/// `i64 fact(i64 n)`, recursive, and `i64 down(i64 n)`, which calls itself
/// `n` times and returns 0.
fn recursive(fx: &mut Fx) -> (SymId, SymId) {
    let fact = fx.declare("fact", I64, &[I64]);
    fx.define(fact, |m, b| {
        let (base, step) = (b.create_block()?, b.create_block()?);
        let (zero, one) = (int(m, I64, 0), int(m, I64, 1));
        let done = b.icmp(IntPredicate::Eq, I1, b.arg(0)?, zero)?;
        b.cond_br(done, base, step)?;
        b.begin_block(base)?;
        b.ret(Some(one))?;
        b.begin_block(step)?;
        let less = b.binary(Opcode::Sub, I64, b.arg(0)?, one)?;
        let inner = call(m, b, fact, &[less])?;
        let product = b.binary(Opcode::Mul, I64, inner, b.arg(0)?)?;
        b.ret(Some(product))?;
        Ok(())
    });
    let down = fx.declare("down", I64, &[I64]);
    fx.define(down, |m, b| {
        let (base, step) = (b.create_block()?, b.create_block()?);
        let (zero, one) = (int(m, I64, 0), int(m, I64, 1));
        let done = b.icmp(IntPredicate::Eq, I1, b.arg(0)?, zero)?;
        b.cond_br(done, base, step)?;
        b.begin_block(base)?;
        b.ret(Some(zero))?;
        b.begin_block(step)?;
        let less = b.binary(Opcode::Sub, I64, b.arg(0)?, one)?;
        let inner = call(m, b, down, &[less])?;
        b.ret(Some(inner))?;
        Ok(())
    });
    (fact, down)
}

/// §5.8: direct calls pass arguments and return results; each call has
/// its own frame.
#[test]
fn direct_calls_and_recursion() {
    let mut fx = Fx::new();
    let (fact, _) = recursive(&mut fx);
    let outcome = fx.run(fact, &[Val::i64(10)]);
    assert_eq!(outcome.max_depth, 11);
    assert_eq!(value(outcome), Val::i64(3_628_800));

    // Ten arguments of mixed types, each weighted by its position.
    let mut fx = Fx::new();
    let params = [I8, I16, I32, I64, DOUBLE, I1, PTR, I64, I32, I8];
    let many = fx.func("many", I64, &params, |m, b| {
        let mut sum = int(m, I64, 0);
        for (index, &ty) in params.iter().enumerate() {
            let arg = b.arg(index as u32)?;
            let wide = match ty {
                DOUBLE => b.cast(Opcode::FPToSI, I64, arg)?,
                PTR => b.load(I64, arg, al(8))?,
                I64 => arg,
                _ => b.cast(Opcode::ZExt, I64, arg)?,
            };
            let weight = int(m, I64, index as u64 + 1);
            let term = b.binary(Opcode::Mul, I64, wide, weight)?;
            sum = b.binary(Opcode::Add, I64, sum, term)?;
        }
        b.ret(Some(sum))?;
        Ok(())
    });
    let f = fx.func("f", I64, &[], |m, b| {
        let slot = b.add_slot(8, 8)?;
        let p = b.slot(slot)?;
        let seven = int(m, I64, 7);
        b.store(seven, p, al(8))?;
        let args = [
            int(m, I8, 1),
            int(m, I16, 2),
            int(m, I32, 3),
            int(m, I64, 4),
            common::double(m, 5.0),
            int(m, I1, 1),
            p,
            int(m, I64, 8),
            int(m, I32, 9),
            int(m, I8, 10),
        ];
        let result = call(m, b, many, &args)?;
        b.ret(Some(result))?;
        Ok(())
    });
    // 1 + 4 + 9 + 16 + 25 + 6 + 49 + 64 + 81 + 100.
    assert_eq!(value(fx.run(f, &[])), Val::i64(355));
}

/// §5.8: a call through a pointer reaches the function whose address it
/// is, and only with that function's type and calling convention.
#[test]
fn indirect_calls() {
    let build = |how: u32| {
        let mut fx = Fx::new();
        let double = fx.func("double", I32, &[I32], |_, b| {
            let sum = b.binary(Opcode::Add, I32, b.arg(0)?, b.arg(0)?)?;
            b.ret(Some(sum))?;
            Ok(())
        });
        let negate = fx.func("negate", I32, &[I32], |m, b| {
            let zero = int(m, I32, 0);
            let diff = b.binary(Opcode::Sub, I32, zero, b.arg(0)?)?;
            b.ret(Some(diff))?;
            Ok(())
        });
        let f = fx.func("f", I32, &[I1], |m, b| {
            let fn_ty = m.symbol(double).ty();
            let picked =
                b.select(PTR, b.arg(0)?, Value::sym(double)?, Value::sym(negate)?)?;
            let twenty_one = int(m, I32, 21);
            let args = [twenty_one];
            let good = CallDesc::new(picked, fn_ty, &args);
            let result = match how {
                0 => b.call(I32, &good)?,
                1 => {
                    let off = b.ptr_add(PTR, picked, 1, &[])?;
                    b.call(I32, &CallDesc::new(off, fn_ty, &args))?
                }
                2 => {
                    let slot = b.add_slot(8, 8)?;
                    let data = b.slot(slot)?;
                    b.call(I32, &CallDesc::new(data, fn_ty, &args))?
                }
                3 => {
                    let null = Value::constant(m.consts.null(&m.types, PTR)?)?;
                    b.call(I32, &CallDesc::new(null, fn_ty, &args))?
                }
                4 => {
                    let other = m.types.function(I32, &[I64], false)?;
                    let wide = [int(m, I64, 21)];
                    b.call(I32, &CallDesc::new(picked, other, &wide))?
                }
                5 => b.call(I32, &CallDesc { cc: CallConv::FAST, ..good })?,
                6 => b.call(I32, &CallDesc { noreturn: true, ..good })?,
                _ => {
                    // Through an integer and back.
                    let addr = b.cast(Opcode::PtrToInt, I64, picked)?;
                    let back = b.cast(Opcode::IntToPtr, PTR, addr)?;
                    b.call(I32, &CallDesc::new(back, fn_ty, &args))?
                }
            };
            b.ret(Some(result))?;
            Ok(())
        });
        (fx, f)
    };
    let (fx, f) = build(0);
    assert_eq!(value(fx.run(f, &[Val::i1(true)])), Val::i32(42));
    assert_eq!(value(fx.run(f, &[Val::i1(false)])), Val::i32((-21i32) as u32));
    let (fx, f) = build(7);
    assert_eq!(value(fx.run(f, &[Val::i1(true)])), Val::i32(42));
    for (how, expected) in [
        (1, Trap::NotAFunction),
        (2, Trap::NotAFunction),
        (3, Trap::NotAFunction),
        (4, Trap::FunctionTypeMismatch),
        (5, Trap::CallConvMismatch),
        (6, Trap::NoReturnReturned),
    ] {
        let (fx, f) = build(how);
        assert_eq!(trap(&fx.run(f, &[Val::i1(true)])), expected, "case {how}");
    }
}

/// §6.1: the standard host functions, and the trace they leave: `sink`
/// records its argument, `source` returns a fixed sequence.
#[test]
fn standard_hosts_and_the_trace() {
    let mut fx = Fx::new();
    let std = fx.std;
    let f = fx.func("f", I64, &[I64], |m, b| {
        let first = call(m, b, std.source, &[])?;
        call(m, b, std.sink, &[b.arg(0)?])?;
        let second = call(m, b, std.source, &[])?;
        call(m, b, std.sink, &[first])?;
        let sum = b.binary(Opcode::Xor, I64, first, second)?;
        b.ret(Some(sum))?;
        Ok(())
    });
    let outcome = fx.run(f, &[Val::i64(77)]);
    let (a, c) = (host::source_value(0), host::source_value(1));
    assert_eq!(
        outcome.trace,
        [
            Event { name: "source".into(), args: vec![], result: Some(Val::i64(a)) },
            Event { name: "sink".into(), args: vec![Val::i64(77)], result: None },
            Event { name: "source".into(), args: vec![], result: Some(Val::i64(c)) },
            Event { name: "sink".into(), args: vec![Val::i64(a)], result: None },
        ]
    );
    assert_eq!(
        outcome.trace_text(),
        format!("source() = i64 {a}\nsink(i64 77)\nsource() = i64 {c}\nsink(i64 {a})\n")
    );
    assert_eq!(outcome.result, Ok(Some(Val::i64(a ^ c))));
    // A second run starts the sequence again.
    assert_eq!(fx.run(f, &[Val::i64(77)]), outcome);
}

/// §6: a host function is a closure named like a function symbol without
/// a body; it gets the arguments and the memory, and its state is its
/// own.
#[test]
fn host_functions() {
    let mut fx = Fx::new();
    let sum = fx.declare("sum", I64, &[PTR, I64]);
    let fill = fx.declare("fill", VOID, &[PTR, I32]);
    let bad = fx.declare("bad", I32, &[]);
    let refuse = fx.declare("refuse", VOID, &[]);
    let nothing = fx.declare("nothing", VOID, &[]);
    let missing = fx.declare("missing", VOID, &[]);
    let f = fx.func("f", I64, &[I32], |m, b| {
        let slot = b.add_slot(16, 8)?;
        let p = b.slot(slot)?;
        let (bbs, done) = ([(); 5].map(|()| b.create_block().unwrap()), b.create_block()?);
        let case = |m: &mut Module, v: u64| m.consts.int(&m.types, I32, v);
        let cases = [
            (case(m, 1)?, bbs[1]),
            (case(m, 2)?, bbs[2]),
            (case(m, 3)?, bbs[3]),
            (case(m, 4)?, bbs[4]),
        ];
        b.switch(b.arg(0)?, bbs[0], &cases)?;
        b.begin_block(bbs[0])?;
        // fill writes 4 bytes of 0x01010101 * n at p and p + 4.
        let three = int(m, I32, 3);
        call(m, b, fill, &[p, three])?;
        let eight = int(m, I64, 8);
        let total = call(m, b, sum, &[p, eight])?;
        b.ret(Some(total))?;
        b.begin_block(bbs[1])?;
        // Reads 16 bytes of which none is written.
        let sixteen = int(m, I64, 16);
        call(m, b, sum, &[p, sixteen])?;
        b.br(done)?;
        b.begin_block(bbs[2])?;
        call(m, b, bad, &[])?;
        b.br(done)?;
        b.begin_block(bbs[3])?;
        call(m, b, refuse, &[])?;
        b.br(done)?;
        b.begin_block(bbs[4])?;
        call(m, b, nothing, &[])?;
        call(m, b, missing, &[])?;
        b.br(done)?;
        b.begin_block(done)?;
        let zero = int(m, I64, 0);
        b.ret(Some(zero))?;
        Ok(())
    });
    let program = Program::with_bodies(&fx.m, &fx.bodies).unwrap();
    let calls = RefCell::new(Vec::new());
    let mut interp = Interp::new(&program);
    interp
        .host("sum", |cx, args| {
            let [Val::Ptr(addr), Val::Int { value: len, .. }] = args else {
                return Err(Trap::Host);
            };
            let bytes = cx.memory.read(*addr, *len as u64)?;
            Ok(Some(Val::i64(bytes.iter().map(|&byte| u64::from(byte)).sum())))
        })
        .host("fill", |cx, args| {
            let [Val::Ptr(addr), Val::Int { value, .. }] = args else {
                return Err(Trap::Host);
            };
            calls.borrow_mut().push(cx.index);
            let word = (0x0101_0101 * *value as u32).to_le_bytes();
            cx.memory.write(*addr, &word)?;
            cx.memory.write(*addr + 4, &word)?;
            Ok(None)
        })
        .host("bad", |_, _| Ok(Some(Val::i64(1))))
        .host("refuse", |_, _| Err(Trap::Host))
        .host("nothing", |_, _| Ok(Some(Val::i8(0))))
        .host("nothing", |_, _| Ok(None));
    let outcome = interp.run(f, &[Val::i32(0)]);
    assert_eq!(outcome.result, Ok(Some(Val::i64(24))));
    assert_eq!(outcome.trace.len(), 2);
    assert_eq!(outcome.trace[1].to_string(), format!("sum({}, i64 8) = i64 24", outcome.trace[1].args[0]));
    assert!(matches!(outcome.trace[0].args[0], Val::Ptr(addr) if addr != 0));

    // A read of unwritten memory by a host is the program's trap.
    let outcome = interp.run(f, &[Val::i32(1)]);
    assert_eq!(trap(&outcome), Trap::UninitializedLoad);
    assert!(outcome.trace.is_empty());
    // A host that returns the wrong kind of value is a broken test.
    assert!(matches!(interp.run(f, &[Val::i32(2)]).result, Err(Stop::Invalid(_))));
    assert_eq!(trap(&interp.run(f, &[Val::i32(3)])), Trap::Host);
    // The later registration of a name is the one called; a function with
    // neither a body nor a host is unsupported, by name.
    let outcome = interp.run(f, &[Val::i32(4)]);
    assert_eq!(unsupported(&outcome), "extern:missing");
    assert_eq!(outcome.trace.len(), 1);
    assert_eq!(outcome.trace[0].to_string(), "nothing()");
    // The closure's state outlives a run; the per-run index does not.
    interp.run(f, &[Val::i32(0)]);
    drop(interp);
    assert_eq!(calls.into_inner(), [0, 0]);
}

/// §7: fuel bounds the instructions executed, exactly; running out is its
/// own trap.
#[test]
fn fuel() {
    let mut fx = Fx::new();
    let spin = fx.func("spin", VOID, &[], |_, b| {
        let again = b.create_block()?;
        b.br(again)?;
        b.begin_block(again)?;
        b.br(again)?;
        Ok(())
    });
    let outcome = fx.run_with(spin, &[], Config { fuel: 1000, ..Config::default() });
    assert_eq!(trap(&outcome), Trap::Fuel);
    assert_eq!(outcome.steps, 1000);
    assert!(Trap::Fuel.is_limit());

    let (fact, _) = recursive(&mut fx);
    let whole = fx.run(fact, &[Val::i64(5)]);
    // Five times icmp, condbr, sub, call, then mul, ret; once icmp,
    // condbr, ret.
    assert_eq!(whole.steps, 5 * 6 + 3);
    let exact = fx.run_with(fact, &[Val::i64(5)], Config { fuel: 33, ..Config::default() });
    assert_eq!(exact.result, Ok(Some(Val::i64(120))));
    let short = fx.run_with(fact, &[Val::i64(5)], Config { fuel: 32, ..Config::default() });
    assert_eq!(trap(&short), Trap::Fuel);
    assert_eq!(short.steps, 32);
}

/// §7: call depth is bounded by the configuration and not by the host's
/// stack: a hundred thousand nested calls run in a test thread.
#[test]
fn call_depth() {
    let mut fx = Fx::new();
    let (_, down) = recursive(&mut fx);
    let config = Config { max_call_depth: 10, ..Config::default() };
    let fits = fx.run_with(down, &[Val::i64(9)], config);
    assert_eq!((fits.max_depth, &fits.result), (10, &Ok(Some(Val::i64(0)))));
    let over = fx.run_with(down, &[Val::i64(10)], config);
    assert_eq!(trap(&over), Trap::CallDepth);
    assert_eq!(over.max_depth, 10);
    assert_eq!(trap(&fx.run(down, &[Val::i64(u64::MAX)])), Trap::CallDepth);
    let config = Config { max_call_depth: 200_000, ..Config::default() };
    let deep = fx.run_with(down, &[Val::i64(100_000)], config);
    assert_eq!((deep.max_depth, &deep.result), (100_001, &Ok(Some(Val::i64(0)))));
}

/// §2: a run that is not well formed is refused, not guessed at.
#[test]
fn invalid_runs() {
    let mut fx = Fx::new();
    let (fact, _) = recursive(&mut fx);
    let invalid = |outcome: hod_interp::Outcome| match outcome.result {
        Err(Stop::Invalid(what)) => what,
        other => panic!("expected invalid, got {other:?}"),
    };
    assert!(invalid(fx.run(fact, &[])).contains("0 arguments for 1 parameters"));
    assert!(invalid(fx.run(fact, &[Val::i32(1)])).contains("argument 0"));
    assert!(invalid(fx.run(fact, &[Val::Ptr(1)])).contains("argument 0"));
    assert!(invalid(fx.run(fx.std.sink, &[Val::i64(1)])).contains("no body"));

    // A body is added once, for a function of its module and type.
    let mut program = Program::new(&fx.m);
    program.add_body(&fx.bodies[0]).unwrap();
    assert!(matches!(program.add_body(&fx.bodies[0]), Err(Stop::Invalid(_))));
    assert_eq!(program.bodies().count(), 1);
    assert!(program.body(fact).is_some() && program.body(fx.std.sink).is_none());
    program.verify().unwrap();
    let other = Fx::new();
    let mut elsewhere = Program::new(&other.m);
    assert!(matches!(elsewhere.add_body(&fx.bodies[0]), Err(Stop::Invalid(_))));
}

fn unary(name: &str, ty: TypeId, arg: Val) -> hod_interp::Outcome {
    eval(ty, &[ty], &[arg], |m, b| icall(m, b, name, ty, &[ty], &[b.arg(0)?]))
}

fn binary(name: &str, ty: TypeId, a: Val, c: Val) -> hod_interp::Outcome {
    eval(ty, &[ty, ty], &[a, c], |m, b| icall(m, b, name, ty, &[ty, ty], &[b.arg(0)?, b.arg(1)?]))
}

/// With the `i1` that makes a zero (or the smallest value) poison.
fn flagged(name: &str, ty: TypeId, arg: Val, poison: bool) -> hod_interp::Outcome {
    eval(ty, &[ty], &[arg], |m, b| {
        let flag = int(m, I1, u64::from(poison));
        icall(m, b, name, ty, &[ty, I1], &[b.arg(0)?, flag])
    })
}

/// §5.6: the pure integer intrinsics.
#[test]
fn integer_intrinsics() {
    assert_eq!(value(unary("llvm.ctpop.i32", I32, Val::i32(0xf0f0_0001))), Val::i32(9));
    assert_eq!(value(unary("llvm.bswap.i32", I32, Val::i32(0x1122_3344))), Val::i32(0x4433_2211));
    assert_eq!(value(unary("llvm.bswap.i16", I16, Val::i16(0x1122))), Val::i16(0x2211));
    assert_eq!(value(unary("llvm.bitreverse.i8", I8, Val::i8(0b1101_0000))), Val::i8(0b0000_1011));
    assert_eq!(value(flagged("llvm.ctlz.i32", I32, Val::i32(1), false)), Val::i32(31));
    assert_eq!(value(flagged("llvm.ctlz.i32", I32, Val::i32(0), false)), Val::i32(32));
    assert_eq!(trap(&flagged("llvm.ctlz.i32", I32, Val::i32(0), true)), Trap::PoisonIntrinsic);
    assert_eq!(value(flagged("llvm.cttz.i16", I16, Val::i16(0x100), true)), Val::i16(8));
    assert_eq!(value(flagged("llvm.cttz.i16", I16, Val::i16(0), false)), Val::i16(16));
    assert_eq!(trap(&flagged("llvm.cttz.i16", I16, Val::i16(0), true)), Trap::PoisonIntrinsic);
    assert_eq!(value(flagged("llvm.abs.i8", I8, Val::i8(0xfb), true)), Val::i8(5));
    assert_eq!(value(flagged("llvm.abs.i8", I8, Val::i8(0x80), false)), Val::i8(0x80));
    assert_eq!(trap(&flagged("llvm.abs.i8", I8, Val::i8(0x80), true)), Trap::PoisonIntrinsic);
    let (minus, plus) = (Val::i8(0xfe), Val::i8(3));
    assert_eq!(value(binary("llvm.smax.i8", I8, minus.clone(), plus.clone())), plus);
    assert_eq!(value(binary("llvm.smin.i8", I8, minus.clone(), plus.clone())), minus);
    assert_eq!(value(binary("llvm.umax.i8", I8, minus.clone(), plus.clone())), minus);
    assert_eq!(value(binary("llvm.umin.i8", I8, minus.clone(), plus.clone())), plus);
    assert_eq!(value(binary("llvm.sadd.sat.i8", I8, Val::i8(100), Val::i8(100))), Val::i8(127));
    assert_eq!(value(binary("llvm.uadd.sat.i8", I8, Val::i8(200), Val::i8(100))), Val::i8(255));
    assert_eq!(value(binary("llvm.ssub.sat.i8", I8, Val::i8(0x80), Val::i8(1))), Val::i8(0x80));
    assert_eq!(value(binary("llvm.usub.sat.i8", I8, Val::i8(3), Val::i8(5))), Val::i8(0));

    let funnel = |name: &str, a: u8, c: u8, by: u8| {
        value(eval(I8, &[I8, I8, I8], &[Val::i8(a), Val::i8(c), Val::i8(by)], |m, b| {
            icall(m, b, name, I8, &[I8, I8, I8], &[b.arg(0)?, b.arg(1)?, b.arg(2)?])
        }))
    };
    assert_eq!(funnel("llvm.fshl.i8", 0b1000_0001, 0b1100_0000, 2), Val::i8(0b0000_0111));
    assert_eq!(funnel("llvm.fshr.i8", 0b1000_0001, 0b1100_0000, 2), Val::i8(0b0111_0000));
    // The amount is taken modulo the width.
    assert_eq!(funnel("llvm.fshl.i8", 0x12, 0x34, 8), Val::i8(0x12));
    assert_eq!(funnel("llvm.fshr.i8", 0x12, 0x34, 16), Val::i8(0x34));

    let three_way = |name: &str, a: u8, c: u8| {
        value(eval(I8, &[I8, I8], &[Val::i8(a), Val::i8(c)], |m, b| {
            icall(m, b, name, I8, &[I8, I8], &[b.arg(0)?, b.arg(1)?])
        }))
    };
    assert_eq!(three_way("llvm.scmp.i8.i8", 0xff, 1), Val::i8(0xff));
    assert_eq!(three_way("llvm.ucmp.i8.i8", 0xff, 1), Val::i8(1));
    assert_eq!(three_way("llvm.ucmp.i8.i8", 4, 4), Val::i8(0));
}

/// §5.6: the checked arithmetic family returns the wrapped result and
/// whether it overflowed.
#[test]
fn checked_arithmetic() {
    let checked = |name: &str, a: u8, c: u8| {
        let mut fx = Fx::new();
        let pair = fx.m.types.struct_type(&[I8, I1], false).unwrap();
        let f = fx.func("f", I32, &[I8, I8], |m, b| {
            let result = icall(m, b, name, pair, &[I8, I8], &[b.arg(0)?, b.arg(1)?])?;
            let (wrapped, flag) = (b.extract_value(I8, result, 0)?, b.extract_value(I1, result, 1)?);
            // Returns `flag * 256 + wrapped`.
            let (wrapped, flag) =
                (b.cast(Opcode::ZExt, I32, wrapped)?, b.cast(Opcode::ZExt, I32, flag)?);
            let k = int(m, I32, 256);
            let high = b.binary(Opcode::Mul, I32, flag, k)?;
            let both = b.binary(Opcode::Or, I32, high, wrapped)?;
            b.ret(Some(both))?;
            Ok(())
        });
        value(fx.run(f, &[Val::i8(a), Val::i8(c)]))
    };
    assert_eq!(checked("llvm.sadd.with.overflow.i8", 100, 27), Val::i32(127));
    assert_eq!(checked("llvm.sadd.with.overflow.i8", 100, 28), Val::i32(256 + 128));
    assert_eq!(checked("llvm.uadd.with.overflow.i8", 200, 55), Val::i32(255));
    assert_eq!(checked("llvm.uadd.with.overflow.i8", 200, 56), Val::i32(256));
    assert_eq!(checked("llvm.ssub.with.overflow.i8", 0x80, 1), Val::i32(256 + 127));
    assert_eq!(checked("llvm.usub.with.overflow.i8", 3, 5), Val::i32(256 + 254));
    assert_eq!(checked("llvm.usub.with.overflow.i8", 5, 3), Val::i32(2));
    assert_eq!(checked("llvm.smul.with.overflow.i8", 16, 8), Val::i32(256 + 128));
    assert_eq!(checked("llvm.smul.with.overflow.i8", 0xf0, 8), Val::i32(128));
    assert_eq!(checked("llvm.umul.with.overflow.i8", 16, 15), Val::i32(240));
    assert_eq!(checked("llvm.umul.with.overflow.i8", 16, 16), Val::i32(256));
}

/// §5.6: the float intrinsics that IEEE defines exactly, `llvm.trap`, and
/// the intrinsics that are no code.
#[test]
fn float_and_other_intrinsics() {
    let d = |name: &str, x: f64| value(unary(name, DOUBLE, Val::f64(x)));
    assert_eq!(d("llvm.fabs.f64", -2.5), Val::f64(2.5));
    assert_eq!(d("llvm.sqrt.f64", 2.25), Val::f64(1.5));
    assert_eq!(d("llvm.floor.f64", -1.5), Val::f64(-2.0));
    assert_eq!(d("llvm.ceil.f64", -1.5), Val::f64(-1.0));
    assert_eq!(d("llvm.trunc.f64", -1.5), Val::f64(-1.0));
    assert_eq!(d("llvm.round.f64", 2.5), Val::f64(3.0));
    assert_eq!(d("llvm.roundeven.f64", 2.5), Val::f64(2.0));
    assert_eq!(d("llvm.rint.f64", 3.5), Val::f64(4.0));
    assert_eq!(d("llvm.nearbyint.f64", -0.4), Val::f64(-0.0));
    assert_eq!(trap(&unary("llvm.sqrt.f64", DOUBLE, Val::f64(-1.0))), Trap::NanResult);
    assert_eq!(value(unary("llvm.fabs.f32", FLOAT, Val::Float(0xffc0_0001))), Val::Float(0x7fc0_0001));
    assert_eq!(value(unary("llvm.sqrt.f32", FLOAT, Val::f32(9.0))), Val::f32(3.0));
    let copied = binary("llvm.copysign.f64", DOUBLE, Val::f64(3.0), Val::f64(-0.0));
    assert_eq!(value(copied), Val::f64(-3.0));
    let fused = eval(DOUBLE, &[], &[], |m, b| {
        // 2^53 + 1 is not a double: the product is exact only when fused.
        let (a, c) = (common::double(m, 134_217_729.0), common::double(m, 134_217_727.0));
        let minus = common::double(m, -18_014_398_509_481_984.0);
        icall(m, b, "llvm.fma.f64", DOUBLE, &[DOUBLE; 3], &[a, c, minus])
    });
    assert_eq!(value(fused), Val::f64(-1.0));

    let trapped = eval(I32, &[], &[], |m, b| {
        icall(m, b, "llvm.trap", VOID, &[], &[])?;
        Ok(int(m, I32, 0))
    });
    assert_eq!(trap(&trapped), Trap::TrapIntrinsic);

    let no_code = eval(I64, &[I64], &[Val::i64(9)], |m, b| {
        let slot = b.add_slot(8, 8)?;
        let p = b.slot(slot)?;
        icall(m, b, "llvm.lifetime.start.p0", VOID, &[PTR], &[p])?;
        icall(m, b, "llvm.donothing", VOID, &[], &[])?;
        let yes = int(m, I1, 1);
        icall(m, b, "llvm.assume", VOID, &[I1], &[yes])?;
        let likely = sint(m, I64, -1);
        let expected = icall(m, b, "llvm.expect.i64", I64, &[I64, I64], &[b.arg(0)?, likely])?;
        let q = icall(m, b, "llvm.launder.invariant.group.p0", PTR, &[PTR], &[p])?;
        b.store(expected, q, al(8))?;
        icall(m, b, "llvm.lifetime.end.p0", VOID, &[PTR], &[p])?;
        b.load(I64, p, al(8))
    });
    assert_eq!(value(no_code), Val::i64(9));
}

/// §5.8: a `void` call has no result; a call's result may be ignored.
#[test]
fn void_calls_and_unused_results() {
    let mut fx = Fx::new();
    let std = fx.std;
    let noisy = fx.func("noisy", I64, &[I64], |m, b| {
        call(m, b, std.sink, &[b.arg(0)?])?;
        b.ret(Some(b.arg(0)?))?;
        Ok(())
    });
    let f = fx.func("f", VOID, &[], |m, b| {
        let (one, two) = (int(m, I64, 1), int(m, I64, 2));
        call(m, b, noisy, &[one])?;
        call(m, b, noisy, &[two])?;
        b.ret(None)?;
        Ok(())
    });
    let outcome = fx.run(f, &[]);
    assert_eq!(outcome.result, Ok(None));
    assert_eq!(outcome.trace_text(), "sink(i64 1)\nsink(i64 2)\n");
}
