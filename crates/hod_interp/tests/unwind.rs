//! Tests of the model of unwinding (`spec/interp.md` §5.9): `invoke`,
//! `landingpad`, `resume`, `llvm.eh.typeid.for`, and the Itanium C++
//! personality's search and cleanup phases.

mod common;

use common::{Fx, I1, I32, I64, PTR, VOID, int, trap, value};
use hod_interp::eh::{self, Eh};
use hod_interp::{Stop, Trap, Val};
use hod_ir::{
    BlockId, CallDesc, Clause, FnAttrs, FunctionBuilder, FunctionDesc, IrResult, Module, Opcode,
    SymId, TypeId, Value,
};

type Build<'x> = Box<dyn FnOnce(&mut Module, &mut FunctionBuilder, &Eh) -> IrResult<()> + 'x>;

/// A module with the standard hosts and the exception model declared.
fn fixture() -> (Fx, Eh) {
    let mut fx = Fx::new();
    let eh = eh::declare(&mut fx.m).unwrap();
    (fx, eh)
}

/// Declares `ret name(params)` with the C++ personality, `nounwind` if
/// asked, and gives it the body `build`.
fn func(
    fx: &mut Fx,
    eh: &Eh,
    name: &str,
    ret: TypeId,
    params: &[TypeId],
    nounwind: bool,
    build: Build<'_>,
) -> SymId {
    let ty = fx.m.types.function(ret, params, false).unwrap();
    let mut attrs = FnAttrs::new();
    if nounwind {
        attrs.set(FnAttrs::NOUNWIND).unwrap();
    }
    let attrs = fx.m.intern_fn_attrs(&attrs).unwrap();
    let desc = FunctionDesc { attrs, personality: Some(eh.personality), ..FunctionDesc::new(ty) };
    let sym = fx.m.add_function(name.as_bytes(), &desc).unwrap();
    fx.define(sym, |m, b| build(m, b, eh));
    sym
}

fn ti(eh: &Eh, index: usize) -> Value {
    Value::sym(eh.type_infos[index]).unwrap()
}

fn ti_const(m: &mut Module, eh: &Eh, index: usize) -> hod_ir::ConstId {
    m.consts.addr(eh.type_infos[index], 0).unwrap()
}

/// `call hod_throw(type, payload)`, plain or `nounwind`, then
/// `unreachable`.
fn throw(
    m: &mut Module,
    b: &mut FunctionBuilder,
    eh: &Eh,
    ty: usize,
    payload: u64,
    nounwind: bool,
) {
    let payload = int(m, I64, payload);
    let args = [ti(eh, ty), payload];
    let desc =
        CallDesc { nounwind, ..CallDesc::new(Value::sym(eh.throw).unwrap(), eh.throw_ty, &args) };
    b.call(VOID, &desc).unwrap();
    b.unreachable().unwrap();
}

/// `invoke hod_throw(type, payload)` unwinding to `pad`.
fn invoke_throw(
    m: &mut Module,
    b: &mut FunctionBuilder,
    eh: &Eh,
    ty: usize,
    payload: u64,
    pad: BlockId,
) {
    let normal = b.create_block().unwrap();
    let payload = int(m, I64, payload);
    let args = [ti(eh, ty), payload];
    b.invoke(VOID, &CallDesc::new(Value::sym(eh.throw).unwrap(), eh.throw_ty, &args), normal, pad)
        .unwrap();
    b.begin_block(normal).unwrap();
    b.unreachable().unwrap();
}

/// `invoke callee()` returning `ret`, unwinding to `pad`; continues in the
/// normal destination.
fn invoke(m: &Module, b: &mut FunctionBuilder, callee: SymId, ret: TypeId, pad: BlockId) -> Value {
    let normal = b.create_block().unwrap();
    let ty = m.symbol(callee).ty();
    let v =
        b.invoke(ret, &CallDesc::new(Value::sym(callee).unwrap(), ty, &[]), normal, pad).unwrap();
    b.begin_block(normal).unwrap();
    v
}

/// In a pad: `selector * 1000 + hod_catch(exn)`, returned as an i64.
fn ret_caught(m: &mut Module, b: &mut FunctionBuilder, eh: &Eh, pair: Value) {
    let exn = b.extract_value(PTR, pair, 0).unwrap();
    let sel = b.extract_value(I32, pair, 8).unwrap();
    let sel = b.cast(Opcode::SExt, I64, sel).unwrap();
    let k = int(m, I64, 1000);
    let scaled = b.binary(Opcode::Mul, I64, sel, k).unwrap();
    let args = [exn];
    let payload =
        b.call(I64, &CallDesc::new(Value::sym(eh.catch).unwrap(), eh.catch_ty, &args)).unwrap();
    let sum = b.binary(Opcode::Add, I64, scaled, payload).unwrap();
    b.ret(Some(sum)).unwrap();
}

fn sink(m: &mut Module, b: &mut FunctionBuilder, fx_sink: (SymId, TypeId), tag: u64) {
    let tag = int(m, I64, tag);
    b.call(VOID, &CallDesc::new(Value::sym(fx_sink.0).unwrap(), fx_sink.1, &[tag])).unwrap();
}

/// `i64 f()` throwing `ty` with payload 42 into a pad with `clauses`.
fn catch_with(
    ty: usize,
    cleanup: bool,
    clauses: fn(&mut Module, &Eh) -> Vec<Clause>,
) -> Result<Option<Val>, Stop> {
    let (mut fx, eh) = fixture();
    let f = func(
        &mut fx,
        &eh,
        "f",
        I64,
        &[],
        false,
        Box::new(move |m, b, eh| {
            let pad = b.create_block()?;
            invoke_throw(m, b, eh, ty, 42, pad);
            b.begin_block(pad)?;
            let clauses = clauses(m, eh);
            let pair = b.landing_pad(eh.pad_ty, cleanup, &clauses)?;
            ret_caught(m, b, eh, pair);
            Ok(())
        }),
    );
    fx.run(f, &[]).result
}

/// §5.9: catch clauses are tried in order; type ids are numbered in order
/// of first mention, from 1.
#[test]
fn catch_clauses_in_order() {
    let long_then_int = |m: &mut Module, eh: &Eh| {
        vec![Clause::Catch(ti_const(m, eh, 1)), Clause::Catch(ti_const(m, eh, 0))]
    };
    // `int` is the second type mentioned.
    assert_eq!(catch_with(0, false, long_then_int), Ok(Some(Val::i64(2042))));
    assert_eq!(catch_with(1, false, long_then_int), Ok(Some(Val::i64(1042))));
    // A type no clause names: no handler anywhere.
    assert_eq!(catch_with(2, false, long_then_int), Err(Stop::Trap(Trap::UncaughtException)));
    // `catch ptr null` takes everything.
    let all = |m: &mut Module, _: &Eh| vec![Clause::Catch(m.consts.null(&m.types, PTR).unwrap())];
    assert_eq!(catch_with(3, false, all), Ok(Some(Val::i64(1042))));
}

/// §5.9: a filter is a handler for every type it does not list; its
/// selector is negative. The empty filter takes everything.
#[test]
fn filters() {
    fn filter_of(m: &mut Module, eh: &Eh, types: &[usize]) -> Clause {
        let elems: Vec<_> = types.iter().map(|&t| ti_const(m, eh, t)).collect();
        let ty = m.types.array(PTR, elems.len() as u64).unwrap();
        let c = if elems.is_empty() {
            m.consts.zero(&m.types, ty).unwrap()
        } else {
            m.consts.aggregate(&m.types, ty, &elems).unwrap()
        };
        Clause::Filter(c)
    }
    let int_only = |m: &mut Module, eh: &Eh| vec![filter_of(m, eh, &[0])];
    assert_eq!(catch_with(1, false, int_only), Ok(Some(Val::i64((-1000_i64 + 42) as u64))));
    assert_eq!(catch_with(0, false, int_only), Err(Stop::Trap(Trap::UncaughtException)));
    let empty_second =
        |m: &mut Module, eh: &Eh| vec![filter_of(m, eh, &[0, 1]), filter_of(m, eh, &[])];
    assert_eq!(catch_with(0, false, empty_second), Ok(Some(Val::i64((-2000_i64 + 42) as u64))));
    assert_eq!(catch_with(2, false, empty_second), Ok(Some(Val::i64((-1000_i64 + 42) as u64))));
}

/// §5.9: `llvm.eh.typeid.for` is the selector of its type, also of a type
/// only it mentions.
#[test]
fn typeid_for() {
    let (mut fx, eh) = fixture();
    let f = func(
        &mut fx,
        &eh,
        "f",
        I64,
        &[],
        false,
        Box::new(|m, b, eh| {
            let mut ids = Vec::new();
            for t in [2, 1, 2] {
                let args = [ti(eh, t)];
                ids.push(common::icall(m, b, "llvm.eh.typeid.for.p0", I32, &[PTR], &args)?);
            }
            let ten = int(m, I32, 10);
            let a = b.binary(Opcode::Mul, I32, ids[0], ten)?;
            let a = b.binary(Opcode::Add, I32, a, ids[1])?;
            let a = b.binary(Opcode::Mul, I32, a, ten)?;
            let a = b.binary(Opcode::Add, I32, a, ids[2])?;
            let a = b.cast(Opcode::ZExt, I64, a)?;
            b.ret(Some(a))?;
            Ok(())
        }),
    );
    assert_eq!(value(fx.run(f, &[])), Val::i64(121));
}

/// §5.9: a cleanup pad is entered on the way to the handler, and `resume`
/// continues into the caller's pad; frames without pads are left.
#[test]
fn cleanup_then_resume() {
    let (mut fx, eh) = fixture();
    let sinkf = (fx.std.sink, fx.std.sink_ty);
    let leaf = func(
        &mut fx,
        &eh,
        "leaf",
        VOID,
        &[],
        false,
        Box::new(|m, b, eh| {
            throw(m, b, eh, 1, 7, false);
            Ok(())
        }),
    );
    let plain = func(
        &mut fx,
        &eh,
        "plain",
        VOID,
        &[],
        false,
        Box::new(move |m, b, _| {
            let ty = m.symbol(leaf).ty();
            b.call(VOID, &CallDesc::new(Value::sym(leaf)?, ty, &[]))?;
            b.ret(None)?;
            Ok(())
        }),
    );
    let middle = func(
        &mut fx,
        &eh,
        "middle",
        VOID,
        &[],
        false,
        Box::new(move |m, b, eh| {
            let pad = b.create_block()?;
            invoke(m, b, plain, VOID, pad);
            b.ret(None)?;
            b.begin_block(pad)?;
            let pair = b.landing_pad(eh.pad_ty, true, &[])?;
            sink(m, b, sinkf, 5);
            b.resume(pair)?;
            Ok(())
        }),
    );
    let f = func(
        &mut fx,
        &eh,
        "f",
        I64,
        &[],
        false,
        Box::new(move |m, b, eh| {
            let pad = b.create_block()?;
            invoke(m, b, middle, VOID, pad);
            let zero = int(m, I64, 0);
            b.ret(Some(zero))?;
            b.begin_block(pad)?;
            let long = ti_const(m, eh, 1);
            let pair = b.landing_pad(eh.pad_ty, false, &[Clause::Catch(long)])?;
            ret_caught(m, b, eh, pair);
            Ok(())
        }),
    );
    let outcome = fx.run(f, &[]);
    assert_eq!(outcome.trace_text(), "sink(i64 5)\n");
    assert_eq!(outcome.result, Ok(Some(Val::i64(1007))));
}

/// §5.9: without a handler the search fails and no cleanup runs.
#[test]
fn uncaught_runs_no_cleanup() {
    let (mut fx, eh) = fixture();
    let sinkf = (fx.std.sink, fx.std.sink_ty);
    let f = func(
        &mut fx,
        &eh,
        "f",
        VOID,
        &[],
        false,
        Box::new(move |m, b, eh| {
            let pad = b.create_block()?;
            invoke_throw(m, b, eh, 0, 1, pad);
            b.begin_block(pad)?;
            let pair = b.landing_pad(eh.pad_ty, true, &[])?;
            sink(m, b, sinkf, 5);
            b.resume(pair)?;
            Ok(())
        }),
    );
    let outcome = fx.run(f, &[]);
    assert_eq!(trap(&outcome), Trap::UncaughtException);
    assert_eq!(outcome.trace_text(), "");
}

/// §5.9: an exception that would leave a `nounwind` call or function is a
/// trap, found by the search.
#[test]
fn nounwind_violations() {
    let (mut fx, eh) = fixture();
    let f = func(
        &mut fx,
        &eh,
        "f",
        VOID,
        &[],
        false,
        Box::new(|m, b, eh| {
            throw(m, b, eh, 0, 1, true);
            Ok(())
        }),
    );
    assert_eq!(trap(&fx.run(f, &[])), Trap::UnwindFromNounwind);

    let (mut fx, eh) = fixture();
    let g = func(
        &mut fx,
        &eh,
        "g",
        VOID,
        &[],
        true,
        Box::new(|m, b, eh| {
            throw(m, b, eh, 0, 1, false);
            Ok(())
        }),
    );
    let f = func(
        &mut fx,
        &eh,
        "f",
        I64,
        &[],
        false,
        Box::new(move |m, b, eh| {
            let pad = b.create_block()?;
            invoke(m, b, g, VOID, pad);
            let zero = int(m, I64, 0);
            b.ret(Some(zero))?;
            b.begin_block(pad)?;
            let null = m.consts.null(&m.types, PTR)?;
            let pair = b.landing_pad(eh.pad_ty, false, &[Clause::Catch(null)])?;
            ret_caught(m, b, eh, pair);
            Ok(())
        }),
    );
    assert_eq!(trap(&fx.run(f, &[])), Trap::UnwindFromNounwind);

    // A handler inside the nounwind function: nothing crosses it.
    let (mut fx, eh) = fixture();
    let f = func(
        &mut fx,
        &eh,
        "f",
        I64,
        &[],
        true,
        Box::new(|m, b, eh| {
            let pad = b.create_block()?;
            invoke_throw(m, b, eh, 0, 9, pad);
            b.begin_block(pad)?;
            let null = m.consts.null(&m.types, PTR)?;
            let pair = b.landing_pad(eh.pad_ty, false, &[Clause::Catch(null)])?;
            ret_caught(m, b, eh, pair);
            Ok(())
        }),
    );
    assert_eq!(value(fx.run(f, &[])), Val::i64(1009));
}

/// §5.9: a landing pad shared by two invokes, with `phi`s, and a second
/// exception thrown and caught from a handler.
#[test]
fn shared_pad_and_nested() {
    let (mut fx, eh) = fixture();
    let f = func(
        &mut fx,
        &eh,
        "f",
        I64,
        &[I1],
        false,
        Box::new(|m, b, eh| {
            let (one, two, pad, inner) =
                (b.create_block()?, b.create_block()?, b.create_block()?, b.create_block()?);
            b.cond_br(b.arg(0)?, one, two)?;
            b.begin_block(one)?;
            invoke_throw(m, b, eh, 0, 1, pad);
            b.begin_block(two)?;
            invoke_throw(m, b, eh, 1, 2, pad);
            b.begin_block(pad)?;
            let (ten, twenty) = (int(m, I64, 10), int(m, I64, 20));
            let from = b.phi(I64, &[(ten, one), (twenty, two)])?;
            let null = m.consts.null(&m.types, PTR)?;
            let pair = b.landing_pad(eh.pad_ty, false, &[Clause::Catch(null)])?;
            let exn = b.extract_value(PTR, pair, 0)?;
            let args = [exn];
            let first = b.call(I64, &CallDesc::new(Value::sym(eh.catch)?, eh.catch_ty, &args))?;
            let sum = b.binary(Opcode::Add, I64, from, first)?;
            // Thrown again from the handler, caught by another pad.
            let normal = b.create_block()?;
            let args = [ti(eh, 2), sum];
            b.invoke(
                VOID,
                &CallDesc::new(Value::sym(eh.throw)?, eh.throw_ty, &args),
                normal,
                inner,
            )?;
            b.begin_block(normal)?;
            b.unreachable()?;
            b.begin_block(inner)?;
            let unsigned = ti_const(m, eh, 2);
            let pair = b.landing_pad(eh.pad_ty, false, &[Clause::Catch(unsigned)])?;
            ret_caught(m, b, eh, pair);
            Ok(())
        }),
    );
    // `null` is type 1, `unsigned` type 2.
    assert_eq!(value(fx.run(f, &[Val::i1(true)])), Val::i64(2011));
    assert_eq!(value(fx.run(f, &[Val::i1(false)])), Val::i64(2022));
}

/// §5.9: an `invoke` that returns continues at its normal destination,
/// for a body, a host and an intrinsic alike.
#[test]
fn invoke_returns() {
    let (mut fx, eh) = fixture();
    let source = fx.std.source;
    let g = func(
        &mut fx,
        &eh,
        "g",
        I64,
        &[],
        false,
        Box::new(|m, b, _| {
            let five = int(m, I64, 5);
            b.ret(Some(five))?;
            Ok(())
        }),
    );
    let f = func(
        &mut fx,
        &eh,
        "f",
        I64,
        &[],
        false,
        Box::new(move |m, b, eh| {
            let pad = b.create_block()?;
            let a = invoke(m, b, g, I64, pad);
            let s = invoke(m, b, source, I64, pad);
            let sum = b.binary(Opcode::Add, I64, a, s)?;
            b.ret(Some(sum))?;
            b.begin_block(pad)?;
            let pair = b.landing_pad(eh.pad_ty, true, &[])?;
            b.resume(pair)?;
            Ok(())
        }),
    );
    let outcome = fx.run(f, &[]);
    let Ok(Some(Val::Int { bits: 64, .. })) = outcome.result else {
        panic!("{:?}", outcome.result);
    };
    assert_eq!(outcome.trace.len(), 1);
}

/// §5.9: what is not an exception's life cycle: a `resume` of something
/// else, a `resume` at or past the handler, `hod_catch` of an exception
/// in a cleanup, a `landingpad` reached by a branch.
#[test]
fn misuse() {
    // `resume` in the handler's own pad.
    let (mut fx, eh) = fixture();
    let f = func(
        &mut fx,
        &eh,
        "f",
        VOID,
        &[],
        false,
        Box::new(|m, b, eh| {
            let pad = b.create_block()?;
            invoke_throw(m, b, eh, 0, 1, pad);
            b.begin_block(pad)?;
            let null = m.consts.null(&m.types, PTR)?;
            let pair = b.landing_pad(eh.pad_ty, false, &[Clause::Catch(null)])?;
            b.resume(pair)?;
            Ok(())
        }),
    );
    assert_eq!(trap(&fx.run(f, &[])), Trap::EhPhaseMismatch);

    // `resume` of a pair that holds no exception.
    let (mut fx, eh) = fixture();
    let f = func(
        &mut fx,
        &eh,
        "f",
        VOID,
        &[],
        false,
        Box::new(|m, b, eh| {
            let zero = Value::constant(m.consts.zero(&m.types, eh.pad_ty)?)?;
            b.resume(zero)?;
            Ok(())
        }),
    );
    assert_eq!(trap(&fx.run(f, &[])), Trap::NotAnException);

    // `hod_catch` in a cleanup pad.
    let (mut fx, eh) = fixture();
    let g = func(
        &mut fx,
        &eh,
        "g",
        I64,
        &[],
        false,
        Box::new(|m, b, eh| {
            let pad = b.create_block()?;
            invoke_throw(m, b, eh, 0, 1, pad);
            b.begin_block(pad)?;
            let pair = b.landing_pad(eh.pad_ty, true, &[])?;
            ret_caught(m, b, eh, pair);
            Ok(())
        }),
    );
    let f = func(
        &mut fx,
        &eh,
        "f",
        I64,
        &[],
        false,
        Box::new(move |m, b, eh| {
            let pad = b.create_block()?;
            let v = invoke(m, b, g, I64, pad);
            b.ret(Some(v))?;
            b.begin_block(pad)?;
            let null = m.consts.null(&m.types, PTR)?;
            let pair = b.landing_pad(eh.pad_ty, false, &[Clause::Catch(null)])?;
            ret_caught(m, b, eh, pair);
            Ok(())
        }),
    );
    assert_eq!(trap(&fx.run(f, &[])), Trap::NotAnException);

    // A `landingpad` reached by a branch: not a program (§5.9).
    let (mut fx, eh) = fixture();
    let (f, _) = fx.func_unverified("f", VOID, &[], |_, b| {
        let pad = b.create_block()?;
        b.br(pad)?;
        b.begin_block(pad)?;
        let pair = b.landing_pad(eh.pad_ty, true, &[])?;
        b.resume(pair)?;
        Ok(())
    });
    assert!(matches!(fx.run(f, &[]).result, Err(Stop::Invalid(_))));
}

/// §5.9: `hod_throw` with a null type is the host's refusal.
#[test]
fn null_type() {
    let (mut fx, eh) = fixture();
    let f = func(
        &mut fx,
        &eh,
        "f",
        VOID,
        &[],
        false,
        Box::new(|m, b, eh| {
            let null = Value::constant(m.consts.null(&m.types, PTR)?)?;
            let one = int(m, I64, 1);
            b.call(VOID, &CallDesc::new(Value::sym(eh.throw)?, eh.throw_ty, &[null, one]))?;
            b.ret(None)?;
            Ok(())
        }),
    );
    assert_eq!(trap(&fx.run(f, &[])), Trap::Host);
}
