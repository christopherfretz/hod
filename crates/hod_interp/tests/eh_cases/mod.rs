//! Hand-written programs that throw (`spec/interp.md` §5.9) or branch
//! through block addresses (§5.5), as [`Generated`] programs for the LLVM
//! oracle: the shapes of C++ exception handling and of computed gotos at
//! the IR level, each run on several arguments.
//!
//! A program compares selectors only with `llvm.eh.typeid.for` and with
//! 0: the numbers themselves are the personality's (LLVM numbers type
//! ids in another order than the interpreter).

#![allow(clippy::many_single_char_names)]

use hod_interp::Val;
use hod_interp::eh::{self, Eh};
use hod_interp::generate::{Coverage, Generated};
use hod_interp::host::{self, Standard};
use hod_ir::{
    CallDesc, Clause, ConstId, FunctionBuilder, FunctionDesc, IntPredicate, IrResult, Module,
    Opcode, SymId, TargetSpec, TypeId, Value, resolve_intrinsic,
};

const I32: TypeId = TypeId::I32;
const I64: TypeId = TypeId::I64;
const PTR: TypeId = TypeId::PTR;
const VOID: TypeId = TypeId::VOID;

struct Cx {
    m: Module,
    bodies: Vec<hod_ir::Body>,
    eh: Eh,
    std: Standard,
}

type Build<'x> = &'x dyn Fn(&mut Module, &mut FunctionBuilder, &Eh, &Standard) -> IrResult<()>;

impl Cx {
    fn new(target: &TargetSpec) -> Cx {
        let mut m = Module::new(target.clone());
        let std = host::declare(&mut m).unwrap();
        let eh = eh::declare(&mut m).unwrap();
        Cx { m, bodies: Vec::new(), eh, std }
    }

    /// `i64 name(i64, i64)` with the C++ personality.
    fn declare(&mut self, name: &str) -> SymId {
        let ty = self.m.types.function(I64, &[I64, I64], false).unwrap();
        let desc = FunctionDesc { personality: Some(self.eh.personality), ..FunctionDesc::new(ty) };
        self.m.add_function(name.as_bytes(), &desc).unwrap()
    }

    fn define(&mut self, sym: SymId, build: Build<'_>) {
        let ty = self.m.symbol(sym).ty();
        let mut b = FunctionBuilder::new(&self.m, sym, ty).unwrap();
        let entry = b.create_block().unwrap();
        b.begin_block(entry).unwrap();
        build(&mut self.m, &mut b, &self.eh, &self.std).unwrap();
        let body = b.finish().unwrap();
        body.verify(&self.m).unwrap();
        self.m.set_has_body(sym).unwrap();
        self.bodies.push(body);
    }

    fn finish(self, entry: SymId, a: u64, b: u64) -> Generated {
        Generated {
            module: self.m,
            bodies: self.bodies,
            entry,
            args: vec![Val::i64(a), Val::i64(b)],
            fuel_bound: 100_000,
            hosts: Some(self.std),
            coverage: Coverage::default(),
        }
    }
}

fn int(m: &mut Module, ty: TypeId, value: u64) -> Value {
    Value::constant(m.consts.int(&m.types, ty, value).unwrap()).unwrap()
}

fn ti(m: &mut Module, eh: &Eh, index: usize) -> ConstId {
    m.consts.addr(eh.type_infos[index], 0).unwrap()
}

/// `call i64 callee(x, y)`, or an `invoke` unwinding to `pad`.
fn call2(
    m: &Module,
    b: &mut FunctionBuilder,
    callee: SymId,
    x: Value,
    y: Value,
    pad: Option<hod_ir::BlockId>,
) -> IrResult<Value> {
    let ty = m.symbol(callee).ty();
    let args = [x, y];
    let desc = CallDesc::new(Value::sym(callee)?, ty, &args);
    match pad {
        None => b.call(I64, &desc),
        Some(pad) => {
            let normal = b.create_block()?;
            let v = b.invoke(I64, &desc, normal, pad)?;
            b.begin_block(normal)?;
            Ok(v)
        }
    }
}

/// `i64 thrower(k, payload)`: throws `int`, `long`, `unsigned`, `short`
/// for `k & 7` from 0 to 3, else returns `payload * 3`.
fn thrower(cx: &mut Cx) -> SymId {
    let sym = cx.declare("thrower");
    cx.define(sym, &|m, b, eh, _| {
        let seven = int(m, I64, 7);
        let k = b.binary(Opcode::And, I64, b.arg(0)?, seven)?;
        let other = b.create_block()?;
        let mut cases = Vec::new();
        for t in 0..4 {
            cases.push((m.consts.int(&m.types, I64, t)?, b.create_block()?));
        }
        b.switch(k, other, &cases)?;
        for (t, &(_, block)) in cases.iter().enumerate() {
            b.begin_block(block)?;
            let args = [Value::sym(eh.type_infos[t])?, b.arg(1)?];
            b.call(VOID, &CallDesc::new(Value::sym(eh.throw)?, eh.throw_ty, &args))?;
            b.unreachable()?;
        }
        b.begin_block(other)?;
        let three = int(m, I64, 3);
        let r = b.binary(Opcode::Mul, I64, b.arg(1)?, three)?;
        b.ret(Some(r))?;
        Ok(())
    });
    sym
}

/// `llvm.eh.typeid.for(type)`.
fn typeid(m: &mut Module, b: &mut FunctionBuilder, ty: ConstId) -> IrResult<Value> {
    let fn_ty = m.types.function(I32, &[PTR], false)?;
    let found = resolve_intrinsic(&m.types, "llvm.eh.typeid.for.p0", fn_ty);
    b.call(
        I32,
        &CallDesc::intrinsic(found.intrinsic, found.overloads(), fn_ty, &[Value::constant(ty)?]),
    )
}

fn catch(b: &mut FunctionBuilder, eh: &Eh, exn: Value) -> IrResult<Value> {
    b.call(I64, &CallDesc::new(Value::sym(eh.catch)?, eh.catch_ty, &[exn]))
}

/// Through a cleanup in `mid` and a frame without a pad, to a pad with
/// catches, a filter and a catch-all in the entry function.
fn classify(target: &TargetSpec, a: u64, b_arg: u64) -> Generated {
    let mut cx = Cx::new(target);
    let thrower = thrower(&mut cx);
    let plain = cx.declare("plain");
    cx.define(plain, &|m, b, _, _| {
        let v = call2(m, b, thrower, b.arg(0)?, b.arg(1)?, None)?;
        b.ret(Some(v))?;
        Ok(())
    });
    let mid = cx.declare("mid");
    cx.define(mid, &|m, b, eh, std| {
        let pad = b.create_block()?;
        let v = call2(m, b, plain, b.arg(0)?, b.arg(1)?, Some(pad))?;
        let one = int(m, I64, 1);
        let v = b.binary(Opcode::Add, I64, v, one)?;
        b.ret(Some(v))?;
        b.begin_block(pad)?;
        let pair = b.landing_pad(eh.pad_ty, true, &[])?;
        b.call(VOID, &CallDesc::new(Value::sym(std.sink)?, std.sink_ty, &[b.arg(0)?]))?;
        b.resume(pair)?;
        Ok(())
    });
    let entry = cx.declare("classify");
    cx.define(entry, &|m, b, eh, _| {
        let pad = b.create_block()?;
        let v = call2(m, b, mid, b.arg(0)?, b.arg(1)?, Some(pad))?;
        b.ret(Some(v))?;
        b.begin_block(pad)?;
        let (int_ti, long_ti, unsigned_ti) = (ti(m, eh, 0), ti(m, eh, 1), ti(m, eh, 2));
        let filter_ty = m.types.array(PTR, 1)?;
        let filter = m.consts.aggregate(&m.types, filter_ty, &[unsigned_ti])?;
        let null = m.consts.null(&m.types, PTR)?;
        let clauses = [
            Clause::Catch(int_ti),
            Clause::Catch(long_ti),
            Clause::Filter(filter),
            Clause::Catch(null),
        ];
        let pair = b.landing_pad(eh.pad_ty, false, &clauses)?;
        let exn = b.extract_value(PTR, pair, 0)?;
        let sel = b.extract_value(I32, pair, 8)?;
        let is_int = typeid(m, b, int_ti)?;
        let is_int = b.icmp(IntPredicate::Eq, I1, sel, is_int)?;
        let is_long = typeid(m, b, long_ti)?;
        let is_long = b.icmp(IntPredicate::Eq, I1, sel, is_long)?;
        let zero = int(m, I32, 0);
        let filtered = b.icmp(IntPredicate::Slt, I1, sel, zero)?;
        let codes: Vec<Value> = [100, 200, 300, 400].iter().map(|&c| int(m, I64, c)).collect();
        let code = b.select(I64, filtered, codes[2], codes[3])?;
        let code = b.select(I64, is_long, codes[1], code)?;
        let code = b.select(I64, is_int, codes[0], code)?;
        let payload = catch(b, eh, exn)?;
        let r = b.binary(Opcode::Add, I64, code, payload)?;
        b.ret(Some(r))?;
        Ok(())
    });
    cx.finish(entry, a, b_arg)
}

const I1: TypeId = TypeId::I1;

/// A pad shared by two invokes with a `phi`, an exception thrown from a
/// handler, and a cleanup that resumes into the caller.
fn nested(target: &TargetSpec, a: u64, b_arg: u64) -> Generated {
    let mut cx = Cx::new(target);
    let thrower = thrower(&mut cx);
    let inner = cx.declare("inner");
    cx.define(inner, &|m, b, eh, std| {
        let (left, right, pad, pad2) =
            (b.create_block()?, b.create_block()?, b.create_block()?, b.create_block()?);
        let one = int(m, I64, 1);
        let low = b.binary(Opcode::And, I64, b.arg(0)?, one)?;
        let zero = int(m, I64, 0);
        let odd = b.icmp(IntPredicate::Ne, I1, low, zero)?;
        b.cond_br(odd, left, right)?;
        b.begin_block(left)?;
        let v = call2(m, b, thrower, zero, b.arg(1)?, Some(pad))?;
        b.ret(Some(v))?;
        b.begin_block(right)?;
        let v = call2(m, b, thrower, one, b.arg(1)?, Some(pad))?;
        b.ret(Some(v))?;
        b.begin_block(pad)?;
        let (ten, twenty) = (int(m, I64, 10), int(m, I64, 20));
        let from = b.phi(I64, &[(ten, left), (twenty, right)])?;
        let null = m.consts.null(&m.types, PTR)?;
        let pair = b.landing_pad(eh.pad_ty, false, &[Clause::Catch(null)])?;
        let exn = b.extract_value(PTR, pair, 0)?;
        let payload = catch(b, eh, exn)?;
        let x = b.binary(Opcode::Add, I64, from, payload)?;
        let two = int(m, I64, 2);
        let shifted = b.binary(Opcode::LShr, I64, b.arg(0)?, one)?;
        let kind = b.binary(Opcode::And, I64, shifted, two)?;
        let v = call2(m, b, thrower, kind, x, Some(pad2))?;
        b.ret(Some(v))?;
        b.begin_block(pad2)?;
        let int_ti = ti(m, eh, 0);
        let pair = b.landing_pad(eh.pad_ty, true, &[Clause::Catch(int_ti)])?;
        let exn = b.extract_value(PTR, pair, 0)?;
        let sel = b.extract_value(I32, pair, 8)?;
        let (caught, cleanup) = (b.create_block()?, b.create_block()?);
        let want = typeid(m, b, int_ti)?;
        let is_int = b.icmp(IntPredicate::Eq, I1, sel, want)?;
        b.cond_br(is_int, caught, cleanup)?;
        b.begin_block(caught)?;
        let payload = catch(b, eh, exn)?;
        let doubled = b.binary(Opcode::Add, I64, payload, payload)?;
        b.ret(Some(doubled))?;
        b.begin_block(cleanup)?;
        b.call(VOID, &CallDesc::new(Value::sym(std.sink)?, std.sink_ty, &[x]))?;
        b.resume(pair)?;
        Ok(())
    });
    let entry = cx.declare("nested");
    cx.define(entry, &|m, b, eh, _| {
        let pad = b.create_block()?;
        let v = call2(m, b, inner, b.arg(0)?, b.arg(1)?, Some(pad))?;
        b.ret(Some(v))?;
        b.begin_block(pad)?;
        let null = m.consts.null(&m.types, PTR)?;
        let pair = b.landing_pad(eh.pad_ty, false, &[Clause::Catch(null)])?;
        let exn = b.extract_value(PTR, pair, 0)?;
        let payload = catch(b, eh, exn)?;
        let k = int(m, I64, 1000);
        let r = b.binary(Opcode::Add, I64, payload, k)?;
        b.ret(Some(r))?;
        Ok(())
    });
    cx.finish(entry, a, b_arg)
}

/// A computed goto (§5.5): a constant table of block addresses, an
/// `indirectbr` through it, and an address compared with a constant.
fn computed_goto(target: &TargetSpec, a: u64) -> Generated {
    let mut cx = Cx::new(target);
    let entry = cx.declare("goto");
    let m = &mut cx.m;
    let addrs: Vec<_> =
        (1..=3).map(|b| m.consts.block_addr(entry, hod_ir::BlockId::new(b)).unwrap()).collect();
    let table_ty = m.types.array(PTR, 3).unwrap();
    let init = m.consts.aggregate(&m.types, table_ty, &addrs).unwrap();
    let desc = hod_ir::VariableDesc {
        init: Some(init),
        constant: true,
        ..hod_ir::VariableDesc::new(table_ty)
    };
    let table = m.add_variable(b"table", &desc).unwrap();
    cx.define(entry, &|m, b, _, _| {
        let targets = [b.create_block()?, b.create_block()?, b.create_block()?];
        let three = int(m, I64, 3);
        let k = b.binary(Opcode::URem, I64, b.arg(0)?, three)?;
        let at = b.ptr_add(PTR, Value::sym(table)?, 0, &[(k, 8)])?;
        let addr = b.load(PTR, at, hod_ir::MemFlags::aligned(8)?)?;
        // `null` is the one thing a block address may be compared with.
        let null = Value::constant(m.consts.null(&m.types, PTR)?)?;
        let is_two = b.icmp(IntPredicate::Ne, I1, addr, null)?;
        let is_two = b.cast(Opcode::ZExt, I64, is_two)?;
        b.indirect_br(addr, &targets)?;
        for (n, &block) in targets.iter().enumerate() {
            b.begin_block(block)?;
            let k = int(m, I64, 100 * (n as u64 + 1));
            let r = b.binary(Opcode::Add, I64, is_two, k)?;
            let r = b.binary(Opcode::Add, I64, r, b.arg(1)?)?;
            b.ret(Some(r))?;
        }
        Ok(())
    });
    cx.finish(entry, a, 5)
}

/// The programs [`super`] checks, each on several arguments.
pub fn cases(target: &TargetSpec) -> Vec<Generated> {
    let mut cases = Vec::new();
    for a in 0..3 {
        cases.push(computed_goto(target, a));
    }
    for a in 0..5 {
        cases.push(classify(target, a, 40 + a));
    }
    for a in 0..4 {
        cases.push(nested(target, a, 7 + a));
    }
    cases
}

/// An exception that only cleanups see: `std::terminate`.
pub fn uncaught(target: &TargetSpec) -> Generated {
    let mut cx = Cx::new(target);
    let thrower = thrower(&mut cx);
    let entry = cx.declare("uncaught");
    cx.define(entry, &|m, b, eh, std| {
        let pad = b.create_block()?;
        let v = call2(m, b, thrower, b.arg(0)?, b.arg(1)?, Some(pad))?;
        b.ret(Some(v))?;
        b.begin_block(pad)?;
        let pair = b.landing_pad(eh.pad_ty, true, &[])?;
        b.call(VOID, &CallDesc::new(Value::sym(std.sink)?, std.sink_ty, &[b.arg(1)?]))?;
        b.resume(pair)?;
        Ok(())
    });
    cx.finish(entry, 0, 5)
}
