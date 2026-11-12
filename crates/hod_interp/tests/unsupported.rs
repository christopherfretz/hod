//! Tests of what the interpreter does not implement (`spec/interp.md`
//! §8): every reason is provoked, and nothing is refused before it is
//! reached.

mod common;

use common::{DOUBLE, Fx, I1, I32, I64, PTR, VOID, al, call, icall, int, unsupported, value};
use hod_interp::{Outcome, Val};
use hod_ir::{
    AliasDesc, AtomicOrdering, Bundle, CallDesc, ConstOp, FunctionBuilder, FunctionDesc, IFuncDesc,
    InlineAsm, IrResult, Linkage, Module, Opcode, ParamAttrs, ParamMem, RmwOp, SyncScope, TlsModel,
    TypeId, Value, VariableDesc,
};

/// Runs `void f()` whose body is `build`, which leaves its last block for
/// a `ret void`. Says whether `Body::verify` accepted the body.
fn stops(build: impl FnOnce(&mut Module, &mut FunctionBuilder) -> IrResult<()>) -> (Outcome, bool) {
    let mut fx = Fx::new();
    let (f, verified) = fx.func_unverified("f", VOID, &[], |m, b| {
        build(m, b)?;
        if b.current_block().is_some() {
            b.ret(None)?;
        }
        Ok(())
    });
    (fx.run(f, &[]), verified)
}

/// The reason a verified body is unsupported.
#[track_caller]
fn reason(build: impl FnOnce(&mut Module, &mut FunctionBuilder) -> IrResult<()>) -> String {
    let (outcome, verified) = stops(build);
    assert!(verified, "the body does not verify");
    unsupported(&outcome)
}

/// The reason a body is unsupported, verified or not.
#[track_caller]
fn loose_reason(build: impl FnOnce(&mut Module, &mut FunctionBuilder) -> IrResult<()>) -> String {
    unsupported(&stops(build).0)
}

fn slot(b: &mut FunctionBuilder) -> IrResult<Value> {
    let slot = b.add_slot(64, 16)?;
    b.slot(slot)
}

/// §8: types. A value of an unsupported type stops the run where it
/// would be computed.
#[test]
fn types() {
    let frozen = |make: fn(&mut Module) -> IrResult<TypeId>| {
        reason(|m, b| {
            let ty = make(m)?;
            let undef = Value::constant(m.consts.undef(&m.types, ty)?)?;
            b.freeze(ty, undef)?;
            Ok(())
        })
    };
    assert_eq!(frozen(|_| Ok(TypeId::HALF)), "type:half");
    assert_eq!(frozen(|_| Ok(TypeId::BFLOAT)), "type:bfloat");
    assert_eq!(frozen(|_| Ok(TypeId::FP128)), "type:fp128");
    assert_eq!(frozen(|_| Ok(TypeId::X86_FP80)), "type:x86_fp80");
    assert_eq!(frozen(|m| m.types.vector(I32, 4)), "type:vector");
    assert_eq!(frozen(|m| m.types.scalable_vector(I32, 4)), "type:scalable-vector");
    assert_eq!(frozen(|m| m.types.int(129)), "type:int-over-128");
    assert_eq!(frozen(|m| m.types.target_ext("spirv.Image")), "type:target-ext");
    // Inside an aggregate too.
    assert_eq!(
        frozen(|m| {
            let vector = m.types.vector(I32, 4)?;
            m.types.struct_type(&[I32, vector], false)
        }),
        "type:vector"
    );
    let loaded = reason(|_, b| {
        let p = slot(b)?;
        b.load(TypeId::HALF, p, al(2))?;
        Ok(())
    });
    assert_eq!(loaded, "type:half");
    let element = reason(|m, b| {
        let vector = m.types.vector(I32, 4)?;
        let undef = Value::constant(m.consts.undef(&m.types, vector)?)?;
        let zero = int(m, I32, 0);
        b.extract_element(I32, undef, zero)?;
        Ok(())
    });
    assert_eq!(element, "type:vector");
    let token = loose_reason(|m, b| {
        let none = Value::constant(m.consts.none_token()?)?;
        b.freeze(TypeId::TOKEN, none)?;
        Ok(())
    });
    assert_eq!(token, "type:token");
}

/// §8: nothing is refused for being in the program: an unsupported
/// instruction on a path not taken does not stop the run.
#[test]
fn only_what_is_reached() {
    let mut fx = Fx::new();
    let f = fx.func("f", I32, &[I1], |m, b| {
        let (odd, plain) = (b.create_block()?, b.create_block()?);
        b.cond_br(b.arg(0)?, odd, plain)?;
        b.begin_block(odd)?;
        let p = slot(b)?;
        let next = b.va_arg(I32, p)?;
        b.ret(Some(next))?;
        b.begin_block(plain)?;
        let one = int(m, I32, 1);
        b.ret(Some(one))?;
        Ok(())
    });
    assert_eq!(value(fx.run(f, &[Val::i1(false)])), Val::i32(1));
    let outcome = fx.run(f, &[Val::i1(true)]);
    assert_eq!(unsupported(&outcome), "inst:va_arg");
    assert_eq!(outcome.stopped_at.map(|(sym, inst)| (sym, inst.index())), Some((f, 1)));
}

/// §8: instructions.
#[test]
fn instructions() {
    // §5.9: unwinding through a function whose personality is not the
    // Itanium C++ one.
    let foreign = reason(|m, b| {
        let eh = hod_interp::eh::declare(m)?;
        let exn = eh.pad_ty;
        let (pad, normal) = (b.create_block()?, b.create_block()?);
        let tag = Value::sym(eh.type_infos[0])?;
        let payload = int(m, I64, 1);
        let args = [tag, payload];
        b.invoke(VOID, &CallDesc::new(Value::sym(eh.throw)?, eh.throw_ty, &args), normal, pad)?;
        b.begin_block(pad)?;
        let caught = b.landing_pad(exn, true, &[])?;
        b.resume(caught)?;
        b.begin_block(normal)?;
        Ok(())
    });
    assert_eq!(foreign, "eh:personality");
    let callbr = loose_reason(|m, b| {
        let void_ty = m.types.function(VOID, &[], false)?;
        let asm =
            m.inline_asm(&InlineAsm { template: b"nop", constraints: b"", flags: 0, ty: void_ty })?;
        let after = b.create_block()?;
        b.callbr(VOID, &CallDesc::new(Value::asm(asm)?, void_ty, &[]), after, &[after])?;
        b.begin_block(after)?;
        Ok(())
    });
    assert_eq!(callbr, "inst:callbr");
    let cast = reason(|m, b| {
        let other = m.types.ptr(1)?;
        let p = slot(b)?;
        b.cast(Opcode::AddrSpaceCast, other, p)?;
        Ok(())
    });
    assert_eq!(cast, "inst:addrspacecast");
    for (op, name) in [
        (Opcode::CleanupPad, "inst:cleanuppad"),
        (Opcode::CatchPad, "inst:catchpad"),
        (Opcode::CatchSwitch, "inst:catchswitch"),
        (Opcode::CatchRet, "inst:catchret"),
        (Opcode::CleanupRet, "inst:cleanupret"),
    ] {
        let funclet = loose_reason(|_, b| {
            let ty = if op.is_terminator() { VOID } else { TypeId::TOKEN };
            b.funclet(op, ty)?;
            Ok(())
        });
        assert_eq!(funclet, name);
    }
    for (op, name) in [
        (RmwOp::FMax, "inst:atomicrmw-fmax"),
        (RmwOp::FMin, "inst:atomicrmw-fmin"),
        (RmwOp::FMaximum, "inst:atomicrmw-fmaximum"),
        (RmwOp::FMinimum, "inst:atomicrmw-fminimum"),
    ] {
        let rmw = reason(|m, b| {
            let p = slot(b)?;
            let one = common::double(m, 1.0);
            let mem = al(8).with_atomic(AtomicOrdering::SeqCst, SyncScope::System);
            b.store(one, p, mem)?;
            b.atomic_rmw(op, DOUBLE, p, one, mem)?;
            Ok(())
        });
        assert_eq!(rmw, name);
    }
}

/// §8: intrinsics the interpreter does not implement are named by their
/// base name; an unknown `llvm.*` name by its full name.
#[test]
fn intrinsics() {
    let one = |name: &'static str, ret: TypeId, params: &'static [TypeId]| {
        reason(move |m, b| {
            let mut args = Vec::new();
            for &param in params {
                args.push(Value::constant(m.consts.zero(&m.types, param)?)?);
            }
            icall(m, b, name, ret, params, &args)?;
            Ok(())
        })
    };
    assert_eq!(one("llvm.stacksave.p0", PTR, &[]), "intrinsic:llvm.stacksave");
    assert_eq!(one("llvm.va_start.p0", VOID, &[PTR]), "intrinsic:llvm.va_start");
    assert_eq!(one("llvm.fmuladd.f64", DOUBLE, &[DOUBLE; 3]), "intrinsic:llvm.fmuladd");
    assert_eq!(one("llvm.sin.f64", DOUBLE, &[DOUBLE]), "intrinsic:llvm.sin");
    assert_eq!(one("llvm.pow.f64", DOUBLE, &[DOUBLE; 2]), "intrinsic:llvm.pow");
    assert_eq!(one("llvm.minnum.f64", DOUBLE, &[DOUBLE; 2]), "intrinsic:llvm.minnum");
    assert_eq!(one("llvm.debugtrap", VOID, &[]), "intrinsic:llvm.debugtrap");
    assert_eq!(one("llvm.aarch64.hint", VOID, &[I32]), "intrinsic:llvm.aarch64.hint");
    assert_eq!(one("llvm.returnaddress", PTR, &[I32]), "intrinsic:llvm.returnaddress");
}

fn sym_f(m: &Module) -> hod_ir::SymId {
    m.find_symbol(b"f").unwrap()
}

fn sym_sink(m: &Module) -> hod_ir::SymId {
    m.find_symbol(b"sink").unwrap()
}

/// `add (ptrtoint @f), 1`.
fn sum(m: &mut Module) -> IrResult<hod_ir::ConstId> {
    let one = m.consts.int(&m.types, I64, 1)?;
    let addr = m.consts.addr(sym_f(m), 0)?;
    let int = m.consts.expr(ConstOp::PtrToInt, I64, None, &[addr])?;
    m.consts.expr(ConstOp::Add, I64, None, &[int, one])
}

/// `[1 x i64] [elem]`.
fn array(m: &mut Module, elem: hod_ir::ConstId) -> IrResult<(TypeId, hod_ir::ConstId)> {
    let ty = m.types.array(I64, 1)?;
    Ok((ty, m.consts.aggregate(&m.types, ty, &[elem])?))
}

/// §8: constants without a value here.
#[test]
fn constants() {
    let operand = |make: fn(&mut Module) -> IrResult<(TypeId, hod_ir::ConstId)>| {
        loose_reason(|m, b| {
            let (ty, constant) = make(m)?;
            b.freeze(ty, Value::constant(constant)?)?;
            Ok(())
        })
    };
    assert_eq!(
        operand(|m| Ok((I64, m.consts.sym_diff(&m.types, I64, sym_f(m), sym_sink(m), 0, false)?))),
        "const:symdiff"
    );
    assert_eq!(operand(|m| Ok((I64, sum(m)?))), "const-expr:add");
    assert_eq!(
        operand(|m| {
            let elem = sum(m)?;
            array(m, elem)
        }),
        "const:aggregate"
    );
    assert_eq!(
        operand(|m| {
            let elem = m.consts.sym_diff(&m.types, I64, sym_f(m), sym_sink(m), 0, false)?;
            array(m, elem)
        }),
        "const:symdiff"
    );

    // In an initializer the reason comes when the module is loaded, which
    // is before the entry function starts.
    let mut fx = Fx::new();
    let f = fx.func("f", VOID, &[], |_, b| {
        b.ret(None)?;
        Ok(())
    });
    assert_eq!(fx.run(f, &[]).result, Ok(None));
    let m = &mut fx.m;
    let (ty, init) = {
        let elem = sum(m).unwrap();
        array(m, elem).unwrap()
    };
    m.add_variable(b"g", &VariableDesc { init: Some(init), ..VariableDesc::new(ty) }).unwrap();
    let outcome = fx.run(f, &[]);
    assert_eq!(unsupported(&outcome), "global:initializer");
    assert_eq!(outcome.stopped_at, None);
}

/// §8: calls.
#[test]
fn calls() {
    let asm = loose_reason(|m, b| {
        let void_ty = m.types.function(VOID, &[], false)?;
        let asm =
            m.inline_asm(&InlineAsm { template: b"nop", constraints: b"", flags: 0, ty: void_ty })?;
        b.call(VOID, &CallDesc::new(Value::asm(asm)?, void_ty, &[]))?;
        Ok(())
    });
    assert_eq!(asm, "call:inline-asm");
    let bundle = reason(|m, b| {
        let sink = m.find_symbol(b"sink").unwrap();
        let zero = int(m, I64, 0);
        let (args, inputs) = ([zero], [zero]);
        let bundles = [Bundle { tag: "deopt", inputs: &inputs }];
        let desc = CallDesc::new(Value::sym(sink)?, m.symbol(sink).ty(), &args);
        b.call(VOID, &CallDesc { bundles: &bundles, ..desc })?;
        Ok(())
    });
    assert_eq!(bundle, "call:operand-bundle");
    let variadic = reason(|m, b| {
        let ty = m.types.function(I32, &[PTR], true)?;
        let printf = m.add_function(b"printf", &FunctionDesc::new(ty))?;
        let p = slot(b)?;
        let one = int(m, I32, 1);
        b.call(I32, &CallDesc::new(Value::sym(printf)?, ty, &[p, one]))?;
        Ok(())
    });
    assert_eq!(variadic, "call:variadic");
    for (mem, name) in [
        (ParamMem::ByVal, "attr:byval"),
        (ParamMem::InAlloca, "attr:inalloca"),
        (ParamMem::Preallocated, "attr:preallocated"),
    ] {
        let passed = loose_reason(|m, b| {
            let ty = m.types.function(VOID, &[PTR], false)?;
            let attrs = [ParamAttrs::NONE.with_mem(mem, I64)?];
            let desc = FunctionDesc { param_attrs: &attrs, ..FunctionDesc::new(ty) };
            let callee = m.add_function(b"callee", &desc)?;
            let p = slot(b)?;
            let args = [p];
            let desc = CallDesc::new(Value::sym(callee)?, ty, &args);
            b.call(VOID, &CallDesc { arg_attrs: &attrs, ..desc })?;
            Ok(())
        });
        assert_eq!(passed, name);
    }
    let external = reason(|m, b| {
        let ty = m.types.function(I32, &[], false)?;
        let getpid = m.add_function(b"getpid", &FunctionDesc::new(ty))?;
        call(m, b, getpid, &[])?;
        Ok(())
    });
    assert_eq!(external, "extern:getpid");
}

/// §8: symbols without an address here.
#[test]
fn symbols() {
    let address_of = |make: fn(&mut Module) -> IrResult<hod_ir::SymId>| {
        reason(|m, b| {
            let sym = make(m)?;
            b.freeze(PTR, Value::sym(sym)?)?;
            Ok(())
        })
    };
    // A declared variable has an address (§4) but nothing to read.
    let read = reason(|m, b| {
        let errno = m.add_variable(b"errno", &VariableDesc::new(I32))?;
        b.load(I32, Value::sym(errno)?, al(4))?;
        Ok(())
    });
    assert_eq!(read, "extern:errno");
    assert_eq!(
        address_of(|m| {
            let zero = m.consts.zero(&m.types, I32)?;
            let desc = VariableDesc {
                init: Some(zero),
                tls: TlsModel::GeneralDynamic,
                ..VariableDesc::new(I32)
            };
            m.add_variable(b"t", &desc)
        }),
        "global:thread-local"
    );
    assert_eq!(
        address_of(|m| {
            let target = m.consts.addr(m.find_symbol(b"f").unwrap(), 0)?;
            m.add_alias(b"a", &AliasDesc::new(m.symbol(m.find_symbol(b"f").unwrap()).ty(), target))
        }),
        "global:alias"
    );
    assert_eq!(
        address_of(|m| {
            let f = m.find_symbol(b"f").unwrap();
            m.add_ifunc(b"i", &IFuncDesc::new(m.symbol(f).ty(), f))
        }),
        "global:ifunc"
    );
    assert_eq!(
        address_of(|m| {
            let ty = m.types.function(VOID, &[], false)?;
            let desc = FunctionDesc { linkage: Linkage::ExternWeak, ..FunctionDesc::new(ty) };
            m.add_function(b"maybe", &desc)
        }),
        "global:extern-weak"
    );
    // A declared function that is not `extern_weak` has an address; only
    // a call of it is unsupported.
    let declared = stops(|m, b| {
        let ty = m.types.function(VOID, &[], false)?;
        let other = m.add_function(b"other", &FunctionDesc::new(ty))?;
        b.freeze(PTR, Value::sym(other)?)?;
        Ok(())
    });
    assert_eq!(declared.0.result, Ok(None));
}
