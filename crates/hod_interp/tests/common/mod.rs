//! What the interpreter's tests share: a module under construction, its
//! bodies, and one-line ways to build a function and run it.

// Each test file uses its own part of this.
#![allow(dead_code)]

use hod_interp::host::{self, Standard};
use hod_interp::{Config, Interp, Outcome, Program, Trap, Val};
use hod_ir::{
    Body, CallDesc, FunctionBuilder, FunctionDesc, IrResult, MemFlags, Module, SymId, TargetSpec,
    TypeId, Value, resolve_intrinsic,
};

pub const I1: TypeId = TypeId::I1;
pub const I8: TypeId = TypeId::I8;
pub const I16: TypeId = TypeId::I16;
pub const I32: TypeId = TypeId::I32;
pub const I64: TypeId = TypeId::I64;
pub const I128: TypeId = TypeId::I128;
pub const FLOAT: TypeId = TypeId::FLOAT;
pub const DOUBLE: TypeId = TypeId::DOUBLE;
pub const PTR: TypeId = TypeId::PTR;
pub const VOID: TypeId = TypeId::VOID;

/// A module with the standard host functions declared, and the bodies
/// built so far.
pub struct Fx {
    pub m: Module,
    pub bodies: Vec<Body>,
    pub std: Standard,
}

impl Fx {
    pub fn new() -> Fx {
        let mut m = Module::new(TargetSpec::aarch64_linux_gnu());
        let std = host::declare(&mut m).unwrap();
        Fx { m, bodies: Vec::new(), std }
    }

    /// Declares `ret name(params)`.
    pub fn declare(&mut self, name: &str, ret: TypeId, params: &[TypeId]) -> SymId {
        let ty = self.m.types.function(ret, params, false).unwrap();
        self.m.add_function(name.as_bytes(), &FunctionDesc::new(ty)).unwrap()
    }

    /// Gives `sym` a body: `build` is called with the entry block open.
    pub fn define(
        &mut self,
        sym: SymId,
        build: impl FnOnce(&mut Module, &mut FunctionBuilder) -> IrResult<()>,
    ) {
        let ty = self.m.symbol(sym).ty();
        let mut b = FunctionBuilder::new(&self.m, sym, ty).unwrap();
        let entry = b.create_block().unwrap();
        b.begin_block(entry).unwrap();
        build(&mut self.m, &mut b).unwrap();
        let body = b.finish().unwrap();
        body.verify(&self.m).unwrap();
        self.m.set_has_body(sym).unwrap();
        self.bodies.push(body);
    }

    /// As [`Fx::func`], for a body `Body::verify` may refuse. Returns
    /// whether it accepts this one.
    pub fn func_unverified(
        &mut self,
        name: &str,
        ret: TypeId,
        params: &[TypeId],
        build: impl FnOnce(&mut Module, &mut FunctionBuilder) -> IrResult<()>,
    ) -> (SymId, bool) {
        let sym = self.declare(name, ret, params);
        let ty = self.m.symbol(sym).ty();
        let mut b = FunctionBuilder::new(&self.m, sym, ty).unwrap();
        let entry = b.create_block().unwrap();
        b.begin_block(entry).unwrap();
        build(&mut self.m, &mut b).unwrap();
        let body = b.finish().unwrap();
        let verified = body.verify(&self.m).is_ok();
        self.m.set_has_body(sym).unwrap();
        self.bodies.push(body);
        (sym, verified)
    }

    /// Declares and defines `ret name(params)`.
    pub fn func(
        &mut self,
        name: &str,
        ret: TypeId,
        params: &[TypeId],
        build: impl FnOnce(&mut Module, &mut FunctionBuilder) -> IrResult<()>,
    ) -> SymId {
        let sym = self.declare(name, ret, params);
        self.define(sym, build);
        sym
    }

    /// Runs `sym` with the standard hosts and the default limits.
    pub fn run(&self, sym: SymId, args: &[Val]) -> Outcome {
        self.run_with(sym, args, Config::default())
    }

    pub fn run_with(&self, sym: SymId, args: &[Val], config: Config) -> Outcome {
        let program = Program::with_bodies(&self.m, &self.bodies).unwrap();
        let mut interp = Interp::new(&program).with_config(config);
        host::register(&mut interp);
        interp.run(sym, args)
    }
}

/// Builds `ret f(params)` whose body is `build` followed by a `ret` of
/// what `build` returned, and runs it on `args`.
pub fn eval(
    ret: TypeId,
    params: &[TypeId],
    args: &[Val],
    build: impl FnOnce(&mut Module, &mut FunctionBuilder) -> IrResult<Value>,
) -> Outcome {
    let mut fx = Fx::new();
    let f = fx.func("f", ret, params, |m, b| {
        let value = build(m, b)?;
        b.ret(Some(value))?;
        Ok(())
    });
    fx.run(f, args)
}

/// The value a run returned.
#[track_caller]
pub fn value(outcome: Outcome) -> Val {
    match outcome.result {
        Ok(Some(value)) => value,
        other => panic!("expected a value, got {other:?}"),
    }
}

/// The trap a run stopped with.
#[track_caller]
pub fn trap(outcome: &Outcome) -> Trap {
    match &outcome.result {
        Err(stop) if stop.trap().is_some() => stop.trap().unwrap(),
        other => panic!("expected a trap, got {other:?}"),
    }
}

/// The reason a run was unsupported.
#[track_caller]
pub fn unsupported(outcome: &Outcome) -> String {
    match &outcome.result {
        Err(stop) if stop.unsupported().is_some() => stop.unsupported().unwrap().to_owned(),
        other => panic!("expected unsupported, got {other:?}"),
    }
}

/// An integer constant.
pub fn int(m: &mut Module, ty: TypeId, value: u64) -> Value {
    Value::constant(m.consts.int(&m.types, ty, value).unwrap()).unwrap()
}

/// A signed integer constant.
pub fn sint(m: &mut Module, ty: TypeId, value: i64) -> Value {
    Value::constant(m.consts.int_signed(&m.types, ty, value).unwrap()).unwrap()
}

/// A `double` constant.
pub fn double(m: &mut Module, value: f64) -> Value {
    Value::constant(m.consts.f64(value).unwrap()).unwrap()
}

/// A `float` constant.
pub fn float(m: &mut Module, value: f32) -> Value {
    Value::constant(m.consts.f32(value).unwrap()).unwrap()
}

/// Memory flags of an access aligned to `bytes`.
pub fn al(bytes: u64) -> MemFlags {
    MemFlags::aligned(bytes).unwrap()
}

/// A call of the intrinsic `name`, which is `ret name(params)`.
pub fn icall(
    m: &mut Module,
    b: &mut FunctionBuilder,
    name: &str,
    ret: TypeId,
    params: &[TypeId],
    args: &[Value],
) -> IrResult<Value> {
    let fn_ty = m.types.function(ret, params, false)?;
    let found = resolve_intrinsic(&m.types, name, fn_ty);
    b.call(ret, &CallDesc::intrinsic(found.intrinsic, found.overloads(), fn_ty, args))
}

/// A direct C call of `callee`.
pub fn call(m: &Module, b: &mut FunctionBuilder, callee: SymId, args: &[Value]) -> IrResult<Value> {
    let fn_ty = m.symbol(callee).ty();
    let hod_ir::TypeKind::Function { ret, .. } = m.types.kind(fn_ty) else {
        unreachable!("a function symbol has a function type")
    };
    b.call(ret, &CallDesc::new(Value::sym(callee)?, fn_ty, args))
}
