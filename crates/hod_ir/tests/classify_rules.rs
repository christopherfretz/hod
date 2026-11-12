//! The rules of `classify` that no matrix changes (`spec/ir.md` §8.5 to
//! §8.7): each structural rule with a program that triggers it and the
//! nearest program that does not, the names built from the input, and
//! the malformed-input outcomes. Everything here is classified with
//! every row of the matrix switched on, so that what is refused is
//! refused by a rule.

mod classify_support;

use classify_support::{
    DOUBLE, I32, I64, NO_STACK, PTR, Program, Stack, VOID, align, atomic, body, constant, declare,
    define, define_as, defined, everything, fx, module, sym, variable, verdict,
};
use hod_ir::{
    Arch, AsmId, AtomicOrdering, BlockId, Bundle, CallConv, CallDesc, CallSite, Classifier, Clause,
    ConstId, FlagValue, FnAttrs, FnAttrsId, FunctionDesc, INVALID_RULES, Intrinsic, Module, Opcode,
    Rejection, RejectionKind, RmwOp, STRUCTURAL_RULES, StackArgs, Support, SymId, Symbol, TailKind,
    TlsModel, TypeId, TypeKind, Value, VariableDesc,
};

const ARCH: Arch = Arch::Aarch64;

/// The verdict with everything implemented.
fn check(program: &Program) -> Result<(), Rejection> {
    verdict(&everything(program.m.target().arch()), program)
}

#[track_caller]
fn refused(program: &Program, construct: &str) -> Rejection {
    let rejection = check(program).expect_err(construct);
    assert_eq!(rejection.construct, construct);
    assert_eq!(rejection.kind, RejectionKind::Unsupported, "{rejection}");
    rejection
}

#[track_caller]
fn malformed(program: &Program, construct: &str) -> Rejection {
    let rejection = check(program).expect_err(construct);
    assert_eq!(rejection.construct, construct);
    assert_eq!(rejection.kind, RejectionKind::Invalid, "{rejection}");
    assert!(INVALID_RULES.iter().any(|rule| rule.name == construct), "{construct} is not listed");
    rejection
}

#[track_caller]
fn accepted(program: &Program) {
    assert_eq!(check(program), Ok(()));
}

fn rule(name: &str) -> &'static str {
    STRUCTURAL_RULES.iter().find(|rule| rule.name == name).expect(name).name
}

fn thread_local(m: &mut Module, name: &str) -> SymId {
    let desc = VariableDesc { tls: TlsModel::GeneralDynamic, ..VariableDesc::new(I32) };
    m.add_variable(name.as_bytes(), &desc).unwrap()
}

// ---- §8.6: thread-local storage ----

/// §8.6 `tls:direct-operand`: a thread-local symbol is an operand of
/// `llvm.threadlocal.address` and of nothing else.
#[test]
fn a_thread_local_symbol_is_only_the_operand_of_threadlocal_address() {
    let direct = |through_intrinsic: bool, stored: bool| {
        let mut fx = fx(ARCH);
        let t = thread_local(&mut fx.m, "t");
        let address = fx.m.types.function(PTR, &[PTR], false).unwrap();
        let args = [sym(t)];
        fx.with(|b, _| {
            let a = b.arg(0).unwrap();
            let ptr = if through_intrinsic {
                let desc =
                    CallDesc::intrinsic(Intrinsic::ThreadLocalAddress, &[PTR], address, &args);
                b.call(PTR, &desc).unwrap()
            } else {
                sym(t)
            };
            if stored {
                // As the value stored, not the address stored to.
                b.store(ptr, b.arg(1).unwrap(), align(8)).unwrap();
            } else {
                b.store(a, ptr, align(4)).unwrap();
            }
        })
    };
    let rejection = refused(&direct(false, false), rule("tls:direct-operand"));
    assert!(rejection.symbol.is_some() && rejection.inst.is_some());
    refused(&direct(false, true), "tls:direct-operand");
    accepted(&direct(true, false));
    accepted(&direct(true, true));

    // The same symbol in any other argument position, and as the
    // argument of any other call, is refused.
    let mut fx = fx(ARCH);
    let t = thread_local(&mut fx.m, "t");
    let (take, take_ty) = declare(&mut fx.m, "take", VOID, &[PTR]);
    let args = [sym(t)];
    let program = fx.with(|b, _| {
        b.call(VOID, &CallDesc::new(sym(take), take_ty, &args)).unwrap();
    });
    refused(&program, "tls:direct-operand");

    // A symbol that is not thread-local is an operand like any other.
    let plain = classify_support::fx(ARCH);
    let g = plain.g;
    accepted(&plain.with(|b, _| {
        b.store(b.arg(0).unwrap(), sym(g), align(4)).unwrap();
    }));
}

/// §8.6 `tls:address-in-constant`: the address of a thread-local symbol
/// is not a constant: not in an initializer, not as a constant operand.
#[test]
fn a_thread_local_address_is_not_a_constant() {
    let init = |tls: bool| {
        let mut m = module(ARCH);
        let target = if tls { thread_local(&mut m, "t") } else { variable(&mut m, "t", I32) };
        let address = m.consts.addr(target, 0).unwrap();
        let pair = m.types.struct_type(&[I64, PTR], false).unwrap();
        let zero = m.consts.int(&m.types, I64, 0).unwrap();
        let init = m.consts.aggregate(&m.types, pair, &[zero, address]).unwrap();
        defined(&mut m, "v", init);
        Program { m, bodies: Vec::new() }
    };
    let rejection = refused(&init(true), rule("tls:address-in-constant"));
    assert_eq!(rejection.symbol, init(true).m.find_symbol(b"v"));
    assert_eq!(rejection.inst, None);
    accepted(&init(false));

    let operand = |tls: bool, through_intrinsic: bool| {
        let mut fx = fx(ARCH);
        let target = if tls { thread_local(&mut fx.m, "t") } else { fx.g };
        let address = constant(fx.m.consts.addr(target, 4).unwrap());
        let ty = fx.m.types.function(PTR, &[PTR], false).unwrap();
        let args = [address];
        fx.with(|b, _| {
            if through_intrinsic {
                let desc = CallDesc::intrinsic(Intrinsic::ThreadLocalAddress, &[PTR], ty, &args);
                b.call(PTR, &desc).unwrap();
            } else {
                b.store(b.arg(0).unwrap(), address, align(4)).unwrap();
            }
        })
    };
    refused(&operand(true, false), "tls:address-in-constant");
    accepted(&operand(false, false));
    // Decided conservatively: `llvm.threadlocal.address` takes the
    // symbol itself; a constant expression over it is refused.
    refused(&operand(true, true), "tls:address-in-constant");

    // A difference of thread-local addresses is no constant either.
    let mut m = module(ARCH);
    let (t, g) = (thread_local(&mut m, "t"), variable(&mut m, "g", I32));
    let diff = m.consts.sym_diff(&m.types, I64, g, t, 0, false).unwrap();
    defined(&mut m, "v", diff);
    refused(&Program { m, bodies: Vec::new() }, "tls:address-in-constant");
}

// ---- §8.6: `blockaddress` ----

/// §8.6 `blockaddress:other-function`: a body may take the address of
/// its own blocks only. An initializer may take any function's.
#[test]
fn a_body_takes_the_address_of_its_own_blocks_only() {
    let in_body = |own: bool| {
        let mut fx = fx(ARCH);
        let other = define(&mut fx.m, "other", VOID, &[]);
        let other_body = body(&fx.m, other, |_, _| {});
        let function = if own { fx.f.0 } else { other.0 };
        let address = constant(fx.m.consts.block_addr(function, BlockId::new(0)).unwrap());
        let mut program = fx.with(|b, _| {
            b.store(address, b.arg(1).unwrap(), align(8)).unwrap();
        });
        program.bodies.push(other_body);
        program
    };
    let rejection = refused(&in_body(false), rule("blockaddress:other-function"));
    assert!(rejection.inst.is_some());
    accepted(&in_body(true));

    let mut m = module(ARCH);
    let f = define(&mut m, "f", VOID, &[]);
    let f_body = body(&m, f, |_, _| {});
    let address = m.consts.block_addr(f.0, BlockId::new(0)).unwrap();
    defined(&mut m, "table", address);
    accepted(&Program { m, bodies: vec![f_body] });
}

// ---- §8.6: what is refused by design and has a row ----

/// §8.6: `naked` functions, `callbr` and `va_arg` are refused by
/// design. They are rows of the matrix whose status is `rejected` for
/// both targets, so no `Support` accepts them.
#[test]
fn naked_callbr_and_va_arg_are_refused_by_design() {
    for arch in [Arch::Aarch64, Arch::X86_64] {
        for name in ["attr:naked", "inst:callbr", "inst:va_arg"] {
            assert!(everything(arch).with(name).is_none(), "{name}");
            assert_eq!(everything(arch).supports(name), Some(false), "{name}");
        }
    }

    let with_attr = |flag: u32, has_body: bool| {
        let mut m = module(ARCH);
        let mut attrs = FnAttrs::new();
        attrs.set(flag).unwrap();
        let ty = m.types.function(VOID, &[], false).unwrap();
        let desc =
            FunctionDesc { attrs: m.intern_fn_attrs(&attrs).unwrap(), ..FunctionDesc::new(ty) };
        if has_body {
            let f = define_as(&mut m, "f", &desc);
            let body = body(&m, f, |_, _| {});
            Program { m, bodies: vec![body] }
        } else {
            m.add_function(b"f", &desc).unwrap();
            Program { m, bodies: Vec::new() }
        }
    };
    let rejection = refused(&with_attr(FnAttrs::NAKED, true), "attr:naked");
    assert_eq!(rejection.inst, None);
    accepted(&with_attr(FnAttrs::NORETURN, true));
    // The attribute of a declaration is the callee's business.
    accepted(&with_attr(FnAttrs::NAKED, false));

    let branch = |callbr: bool| {
        let fx = fx(ARCH);
        let call = fx.call();
        fx.with(|b, _| {
            let (then, other) = (b.create_block().unwrap(), b.create_block().unwrap());
            if callbr {
                b.callbr(VOID, &call, then, &[other]).unwrap();
            } else {
                b.call(VOID, &call).unwrap();
                let truth = b.icmp(
                    hod_ir::IntPredicate::Eq,
                    TypeId::I1,
                    b.arg(0).unwrap(),
                    b.arg(0).unwrap(),
                );
                b.cond_br(truth.unwrap(), then, other).unwrap();
            }
            b.begin_block(then).unwrap();
            b.ret(None).unwrap();
            b.begin_block(other).unwrap();
            b.ret(None).unwrap();
        })
    };
    refused(&branch(true), "inst:callbr");
    accepted(&branch(false));

    let variadic = |va_arg: bool| {
        let mut fx = fx(ARCH);
        let start = fx.m.types.function(VOID, &[PTR], false).unwrap();
        fx.with(|b, _| {
            let list = b.arg(1).unwrap();
            if va_arg {
                b.va_arg(I32, list).unwrap();
            } else {
                let args = [list];
                b.call(VOID, &CallDesc::intrinsic(Intrinsic::VaStart, &[PTR], start, &args))
                    .unwrap();
            }
        })
    };
    refused(&variadic(true), "inst:va_arg");
    accepted(&variadic(false));
}

// ---- §8.6: `musttail` ----

/// A `StackArgs` that puts every argument on the stack, 8 bytes each.
struct EightEach;

impl StackArgs for EightEach {
    fn of_function(&self, module: &Module, function: Symbol<'_>) -> Option<u64> {
        match module.types.kind(function.ty()) {
            TypeKind::Function { params, .. } => Some(8 * params.len() as u64),
            _ => None,
        }
    }

    fn of_call(&self, _: &Module, call: &CallSite<'_>) -> Option<u64> {
        Some(8 * call.args().len() as u64)
    }
}

/// `void f(i32, ptr, double)` making a call marked `tail` of a function
/// of `params` parameters with convention `cc`.
fn tail_call(tail: TailKind, cc: CallConv, params: usize) -> Program {
    let mut fx = fx(ARCH);
    let types = vec![I32; params];
    let ty = fx.m.types.function(VOID, &types, false).unwrap();
    let desc = FunctionDesc { cc, ..FunctionDesc::new(ty) };
    let callee = fx.m.add_function(b"target", &desc).unwrap();
    fx.with(|b, _| {
        let args = vec![b.arg(0).unwrap(); params];
        let call = CallDesc { tail, cc, ..CallDesc::new(sym(callee), ty, &args) };
        b.call(VOID, &call).unwrap();
        b.ret(None).unwrap();
    })
}

fn with_hook(program: &Program, hook: Option<&dyn StackArgs>) -> Result<(), String> {
    let support = everything(ARCH);
    let mut classifier = Classifier::new(&support);
    if let Some(hook) = hook {
        classifier = classifier.with_stack_args(hook);
    }
    classifier.module(&program.m).unwrap();
    classifier.body(&program.m, &program.bodies[0]).map_err(|rejection| {
        assert_eq!(rejection.kind, RejectionKind::Unsupported);
        assert!(rejection.inst.is_some());
        rejection.construct.into_owned()
    })
}

/// §8.6: a `musttail` call is accepted when the callee does not pop its
/// own arguments and its stack arguments fit the caller's own area,
/// which only a `StackArgs` knows.
#[test]
fn musttail_needs_the_stack_argument_areas() {
    let err = |name: &str| Err(rule(name).to_owned());
    let must = tail_call(TailKind::MustTail, CallConv::C, 2);

    // Without the hook, or when it cannot tell, the call is refused.
    assert_eq!(with_hook(&must, None), err("musttail:stack-args-unknown"));
    let no_function = Stack { function: None, call: Some(0) };
    assert_eq!(with_hook(&must, Some(&no_function)), err("musttail:stack-args-unknown"));
    let no_call = Stack { function: Some(0), call: None };
    assert_eq!(with_hook(&must, Some(&no_call)), err("musttail:stack-args-unknown"));
    assert_eq!(with_hook(&must, Some(&NO_STACK)), Ok(()));
    // `tail` and `notail` are not `musttail`: no hook is asked.
    for kind in [TailKind::None, TailKind::Tail, TailKind::NoTail] {
        assert_eq!(with_hook(&tail_call(kind, CallConv::C, 2), None), Ok(()), "{kind:?}");
    }

    // The callee's area may be as large as the caller's, not larger.
    let grow = |function, call| {
        with_hook(&must, Some(&Stack { function: Some(function), call: Some(call) }))
    };
    assert_eq!(grow(16, 32), err("musttail:stack-args-grow"));
    assert_eq!(grow(0, 8), err("musttail:stack-args-grow"));
    assert_eq!(grow(16, 16), Ok(()));
    assert_eq!(grow(32, 16), Ok(()));
    // The hook is given the function of the body and the call itself:
    // `f` has three parameters.
    assert_eq!(with_hook(&tail_call(TailKind::MustTail, CallConv::C, 3), Some(&EightEach)), Ok(()));
    assert_eq!(
        with_hook(&tail_call(TailKind::MustTail, CallConv::C, 4), Some(&EightEach)),
        err("musttail:stack-args-grow")
    );

    // A convention whose callee pops its arguments, whatever the areas.
    for cc in [CallConv::TAIL, CallConv::SWIFT_TAIL] {
        let pop = tail_call(TailKind::MustTail, cc, 2);
        assert_eq!(with_hook(&pop, Some(&NO_STACK)), err("musttail:callee-pop"), "{cc:?}");
        assert_eq!(with_hook(&pop, None), err("musttail:callee-pop"), "{cc:?}");
        assert_eq!(with_hook(&tail_call(TailKind::Tail, cc, 2), None), Ok(()), "{cc:?}");
    }
    assert_eq!(
        with_hook(&tail_call(TailKind::MustTail, CallConv::FAST, 2), Some(&NO_STACK)),
        Ok(())
    );
}

// ---- §8.6: atomics of scalable vectors ----

/// §8.6 `atomic:scalable-vector`: an atomic access of a scalable vector
/// has no meaning anywhere. It is its own reason, ahead of the type's.
#[test]
fn an_atomic_scalable_vector_is_its_own_reason() {
    #[derive(Clone, Copy, PartialEq, Debug)]
    enum Access {
        Load,
        Store,
        Rmw,
        CmpXchg,
    }
    let access = |kind: Access, scalable: bool, is_atomic: bool| {
        let mut fx = fx(ARCH);
        let m = &mut fx.m;
        let ty = if scalable { m.types.scalable_vector(I32, 4).unwrap() } else { I32 };
        let value = constant(m.consts.zero(&m.types, ty).unwrap());
        let pair = m.types.struct_type(&[ty, TypeId::I1], false).unwrap();
        let mem = if is_atomic { atomic(4) } else { align(4) };
        fx.with(|b, _| {
            let p = b.arg(1).unwrap();
            match kind {
                Access::Load => b.load(ty, p, mem),
                Access::Store => b.store(value, p, mem),
                Access::Rmw => b.atomic_rmw(RmwOp::Xchg, ty, p, value, mem),
                Access::CmpXchg => {
                    b.cmpxchg(pair, p, value, value, mem, AtomicOrdering::SeqCst, false)
                }
            }
            .unwrap();
        })
    };
    for kind in [Access::Load, Access::Store, Access::Rmw, Access::CmpXchg] {
        refused(&access(kind, true, true), rule("atomic:scalable-vector"));
        accepted(&access(kind, false, true));
    }
    for kind in [Access::Load, Access::Store] {
        refused(&access(kind, true, false), "type:scalable-vector");
        accepted(&access(kind, false, false));
    }
}

// ---- §8.6: address spaces ----

/// §8.6: only address space 0 exists. A pointer type, a global, a
/// function and an `alloca` in another one each have their own reason.
#[test]
fn only_address_space_zero_exists() {
    let pointer = |space: u32| {
        let mut m = module(ARCH);
        let ptr = m.types.ptr(space).unwrap();
        let f = define(&mut m, "f", VOID, &[ptr]);
        let body = body(&m, f, |_, _| {});
        Program { m, bodies: vec![body] }
    };
    refused(&pointer(1), rule("addrspace:pointer"));
    accepted(&pointer(0));

    let loaded = |space: u32| {
        let mut fx = fx(ARCH);
        let ptr = fx.m.types.ptr(space).unwrap();
        fx.with(|b, _| {
            b.load(ptr, b.arg(1).unwrap(), align(8)).unwrap();
        })
    };
    let rejection = refused(&loaded(7), "addrspace:pointer");
    assert!(rejection.inst.is_some());
    accepted(&loaded(0));

    let global = |space: u32| {
        let mut m = module(ARCH);
        m.add_variable(b"g", &VariableDesc { addr_space: space, ..VariableDesc::new(I32) })
            .unwrap();
        Program { m, bodies: Vec::new() }
    };
    let rejection = refused(&global(1), rule("addrspace:global"));
    assert_eq!(rejection.symbol, global(1).m.find_symbol(b"g"));
    accepted(&global(0));

    let function = |space: u32| {
        let mut m = module(ARCH);
        let ty = m.types.function(VOID, &[], false).unwrap();
        m.add_function(b"f", &FunctionDesc { addr_space: space, ..FunctionDesc::new(ty) }).unwrap();
        Program { m, bodies: Vec::new() }
    };
    refused(&function(1), rule("addrspace:function"));
    accepted(&function(0));

    let alloca = |space: u32| {
        let mut fx = fx(ARCH);
        let ptr = fx.m.types.ptr(space).unwrap();
        fx.with(|b, _| {
            b.alloca(ptr, b.arg(0).unwrap(), 4, 4).unwrap();
        })
    };
    refused(&alloca(5), rule("addrspace:alloca"));
    accepted(&alloca(0));
}

// ---- §8.6: what a producer could not say ----

/// §8.6: a construct a producer recorded as unrepresentable
/// (`Module::record_unsupported`) is refused under the name the
/// producer gave it, at the symbol it gave.
#[test]
fn producer_records_pass_through() {
    let program = |record: Option<(&str, bool)>| {
        let mut m = module(ARCH);
        let (f, _) = declare(&mut m, "f", VOID, &[]);
        if let Some((construct, at_symbol)) = record {
            m.record_unsupported(construct, at_symbol.then_some(f));
        }
        Program { m, bodies: Vec::new() }
    };
    accepted(&program(None));
    let at_symbol = program(Some(("attr:jumptable", true)));
    let rejection = refused(&at_symbol, "attr:jumptable");
    assert_eq!((rejection.symbol, rejection.inst), (at_symbol.m.find_symbol(b"f"), None));
    let rejection = refused(&program(Some(("global:partition", false))), "global:partition");
    assert_eq!(rejection.symbol, None);

    // In the order recorded, and the shipped matrix refuses them too.
    let mut m = module(ARCH);
    m.record_unsupported("inst:first", None);
    m.record_unsupported("inst:second", None);
    assert_eq!(hod_ir::classify(&m).unwrap_err().construct, "inst:first");
}

/// §8.6: a module flag hod does not know and may not ignore is refused
/// as `module-flag:<name>`. A flag with the `Require` behavior is never
/// ignorable.
#[test]
fn unknown_module_flags_are_refused_by_name() {
    let flagged = |behavior: u32, name: &[u8], value: FlagValue<'_>| {
        let mut m = module(ARCH);
        m.set_module_flag(behavior, name, value).unwrap();
        Program { m, bodies: Vec::new() }
    };
    let rejection = refused(&flagged(1, b"mystery", FlagValue::Int(1)), "module-flag:mystery");
    assert_eq!((rejection.symbol, rejection.inst), (None, None));
    refused(&flagged(2, b"CG Profile", FlagValue::Other), "module-flag:CG Profile");
    for ignorable in hod_ir::IGNORABLE_MODULE_FLAGS {
        accepted(&flagged(1, ignorable.as_bytes(), FlagValue::Int(4)));
    }
    refused(&flagged(3, b"wchar_size", FlagValue::Other), "module-flag:wchar_size");
    // A flag hod knows is a property, decided by its row.
    let known = flagged(8, b"PIC Level", FlagValue::Int(2));
    accepted(&known);
    let no_pic = everything(ARCH).without("module:pic").unwrap();
    assert_eq!(verdict(&no_pic, &known).unwrap_err().construct, "module:pic");
}

// ---- §8.6: limits and mismatches ----

/// A constant of `depth` structs around an address.
fn nested(m: &mut Module, target: SymId, depth: usize) -> ConstId {
    let mut id = m.consts.addr(target, 0).unwrap();
    for _ in 0..depth {
        let ty = m.consts.ty(id);
        let ty = m.types.struct_type(&[ty], false).unwrap();
        id = m.consts.aggregate(&m.types, ty, &[id]).unwrap();
    }
    id
}

/// §8.6 `limit:constant-nesting`: a constant nested more than 64 deep
/// is refused, in an initializer and as an operand.
#[test]
fn a_constant_nests_at_most_64_deep() {
    let init = |depth: usize| {
        let mut m = module(ARCH);
        let g = variable(&mut m, "g", I32);
        let init = nested(&mut m, g, depth);
        defined(&mut m, "v", init);
        Program { m, bodies: Vec::new() }
    };
    accepted(&init(10));
    accepted(&init(64));
    let rejection = refused(&init(65), rule("limit:constant-nesting"));
    assert_eq!(rejection.symbol, init(65).m.find_symbol(b"v"));
    refused(&init(300), "limit:constant-nesting");

    let operand = |depth: usize| {
        let mut fx = fx(ARCH);
        let g = fx.g;
        let value = constant(nested(&mut fx.m, g, depth));
        fx.with(|b, _| {
            b.store(value, b.arg(1).unwrap(), align(8)).unwrap();
        })
    };
    accepted(&operand(64));
    refused(&operand(65), "limit:constant-nesting");
}

/// §8.6 `target:support-mismatch`: a matrix decides for its own target.
#[test]
fn a_matrix_is_of_one_target() {
    for (module_arch, support_arch) in
        [(Arch::Aarch64, Arch::X86_64), (Arch::X86_64, Arch::Aarch64)]
    {
        let program = fx(module_arch).with(|_, _| {});
        let support = everything(support_arch);
        let classifier = Classifier::new(&support);
        let name = rule("target:support-mismatch");
        let module_reason = classifier.module(&program.m).unwrap_err();
        assert_eq!((module_reason.construct.as_ref(), module_reason.symbol), (name, None));
        let body_reason = classifier.body(&program.m, &program.bodies[0]).unwrap_err();
        assert_eq!((body_reason.construct.as_ref(), body_reason.symbol), (name, None));
        accepted(&program);
    }
}

// ---- §8.5: names built from the input ----

/// §8.5: what has no row of its own is named after itself: a calling
/// convention by its number, an operand bundle by its tag, an intrinsic
/// by its name. None of them can be switched on.
#[test]
fn what_has_no_row_is_named_after_itself() {
    let convention = |number: u32| {
        let cc = CallConv::from_llvm(number).unwrap();
        let mut m = module(ARCH);
        let ty = m.types.function(VOID, &[], false).unwrap();
        let f = define_as(&mut m, "f", &FunctionDesc { cc, ..FunctionDesc::new(ty) });
        let body = body(&m, f, |_, _| {});
        Program { m, bodies: vec![body] }
    };
    refused(&convention(77), "cc:77");
    accepted(&convention(8));
    // On a call as well.
    let fx77 = fx(ARCH);
    let call = CallDesc { cc: CallConv::from_llvm(1023).unwrap(), ..fx77.call() };
    refused(
        &fx77.with(|b, _| {
            b.call(VOID, &call).unwrap();
        }),
        "cc:1023",
    );

    let bundle = |tag: &str| {
        let fx = fx(ARCH);
        let bundles = [Bundle { tag, inputs: &[] }];
        let call = CallDesc { bundles: &bundles, ..fx.call() };
        fx.with(|b, _| {
            b.call(VOID, &call).unwrap();
        })
    };
    refused(&bundle("mystery"), "bundle:mystery");
    accepted(&bundle("deopt"));

    let intrinsic = |intrinsic: Intrinsic<'static>| {
        let fx = fx(ARCH);
        let overloads = [I32; 4];
        let overloads = &overloads[..intrinsic.overload_count()];
        let call = CallDesc::intrinsic(intrinsic, overloads, fx.callee.1, &[]);
        fx.with(|b, _| {
            b.call(VOID, &call).unwrap();
        })
    };
    refused(&intrinsic(Intrinsic::Other("llvm.foo.bar")), "intrinsic:llvm.foo.bar");
    accepted(&intrinsic(Intrinsic::Trap));
    for name in ["cc:77", "bundle:mystery", "intrinsic:llvm.foo.bar", "module-flag:mystery"] {
        assert_eq!(everything(ARCH).supports(name), None, "{name}");
        assert!(everything(ARCH).with(name).is_none(), "{name}");
    }
}

// ---- §8.6: what is not an error ----

/// §8.6: a direct call whose function type is not the callee's is legal
/// IR and is not refused: the call is made with the call's own type.
#[test]
fn a_direct_call_through_another_type_is_accepted() {
    let mut fx = fx(ARCH);
    let other = fx.m.types.function(I32, &[I32, DOUBLE], false).unwrap();
    let callee = fx.callee.0;
    accepted(&fx.with(|b, _| {
        let args = [b.arg(0).unwrap(), b.arg(2).unwrap()];
        b.call(I32, &CallDesc::new(sym(callee), other, &args)).unwrap();
    }));

    // And a call of a variable, or of a number, is a call.
    let mut fx = classify_support::fx(ARCH);
    let g = fx.g;
    let ty = fx.callee.1;
    let number = constant(fx.m.consts.addr_of_int(4096).unwrap());
    accepted(&fx.with(|b, _| {
        b.call(VOID, &CallDesc::new(sym(g), ty, &[])).unwrap();
        b.call(VOID, &CallDesc::new(number, ty, &[])).unwrap();
    }));
}

// ---- §8.7: malformed input ----

/// §8.7: a body that is not the body of a function of this module is
/// not walked at all.
#[test]
fn a_body_must_belong_to_its_module() {
    let built = fx(ARCH).with(|b, _| {
        b.binary(Opcode::Add, I32, b.arg(0).unwrap(), b.arg(0).unwrap()).unwrap();
    });
    accepted(&built);
    let foreign = |m: Module| Program { m, bodies: vec![built.bodies[0].clone()] };

    let rejection = malformed(&foreign(module(ARCH)), "invalid:body-symbol");
    assert_eq!((rejection.symbol, rejection.inst), (None, None));

    let mut m = module(ARCH);
    variable(&mut m, "f", I32);
    let rejection = malformed(&foreign(m), "invalid:body-not-function");
    assert_eq!(rejection.symbol, Some(built.bodies[0].symbol()));

    // A function of another type.
    let mut m = module(ARCH);
    m.types.struct_type(&[I32, I32], false).unwrap();
    let (f, ty) = declare(&mut m, "f", VOID, &[I32]);
    assert_ne!(ty, built.bodies[0].fn_type());
    let rejection = malformed(&foreign(m), "invalid:body-type");
    assert_eq!(rejection.symbol, Some(f));

    // A function whose type has the body's id and is another type: the
    // body has three arguments, the type one parameter.
    let mut m = module(ARCH);
    let (_, ty) = declare(&mut m, "f", VOID, &[I32]);
    assert_eq!(ty, built.bodies[0].fn_type());
    malformed(&foreign(m), "invalid:body");

    // In the census they end the walk too: nothing after them counts.
    let support = everything(ARCH);
    let mut census = hod_ir::Census::new();
    Classifier::new(&support).census_body(&module(ARCH), &built.bodies[0], &mut census);
    assert_eq!(census.len(), 1);
    assert_eq!(census.entries()[0].kind, RejectionKind::Invalid);
    assert_eq!(census.entries()[0].construct, "invalid:body-symbol");
}

/// §8.7: ids that name nothing: a symbol, a constant, an inline
/// assembly, a function's attributes.
#[test]
fn ids_must_name_something() {
    let nothing = SymId::new(99);
    let fx0 = fx(ARCH);
    let rejection = malformed(
        &fx0.with(|b, _| {
            b.store(b.arg(0).unwrap(), sym(nothing), align(4)).unwrap();
        }),
        "invalid:symbol",
    );
    assert!(rejection.inst.is_some());

    // As a callee.
    let fx1 = fx(ARCH);
    let call = CallDesc::new(sym(nothing), fx1.callee.1, &[]);
    malformed(
        &fx1.with(|b, _| {
            b.call(VOID, &call).unwrap();
        }),
        "invalid:symbol",
    );

    // In a constant, as an operand and in an initializer.
    let mut fx2 = fx(ARCH);
    let address = fx2.m.consts.addr(nothing, 0).unwrap();
    malformed(
        &fx2.with(|b, _| {
            b.store(constant(address), b.arg(1).unwrap(), align(8)).unwrap();
        }),
        "invalid:symbol",
    );
    let mut m = module(ARCH);
    let address = m.consts.addr(nothing, 0).unwrap();
    defined(&mut m, "v", address);
    let rejection = malformed(&Program { m, bodies: Vec::new() }, "invalid:symbol");
    assert_eq!(rejection.symbol, Some(SymId::new(0)));

    // A constant.
    let fx3 = fx(ARCH);
    let missing = ConstId::new(9999);
    malformed(
        &fx3.with(|b, _| {
            b.store(constant(missing), b.arg(1).unwrap(), align(8)).unwrap();
        }),
        "invalid:constant",
    );
    let mut fx4 = fx(ARCH);
    let pad = fx4.m.types.struct_type(&[PTR, I32], false).unwrap();
    malformed(
        &fx4.with(|b, _| {
            b.landing_pad(pad, false, &[Clause::Catch(missing)]).unwrap();
        }),
        "invalid:constant",
    );
    let fx5 = fx(ARCH);
    malformed(
        &fx5.with(|b, _| {
            let (then, other) = (b.create_block().unwrap(), b.create_block().unwrap());
            b.switch(b.arg(0).unwrap(), other, &[(missing, then)]).unwrap();
            b.begin_block(then).unwrap();
            b.ret(None).unwrap();
            b.begin_block(other).unwrap();
            b.ret(None).unwrap();
        }),
        "invalid:constant",
    );

    // An inline assembly.
    let fx6 = fx(ARCH);
    let asm = Value::asm(AsmId::new(9)).unwrap();
    let call = CallDesc::new(asm, fx6.callee.1, &[]);
    malformed(
        &fx6.with(|b, _| {
            b.call(VOID, &call).unwrap();
        }),
        "invalid:inline-asm",
    );

    // A function's attributes: the module pass and the body pass.
    let mut m = module(ARCH);
    let ty = m.types.function(VOID, &[], false).unwrap();
    let desc = FunctionDesc { attrs: FnAttrsId::new(77), ..FunctionDesc::new(ty) };
    let f = define_as(&mut m, "f", &desc);
    let body = body(&m, f, |_, _| {});
    let program = Program { m, bodies: vec![body] };
    let rejection = malformed(&program, "invalid:function-attributes");
    assert_eq!((rejection.symbol, rejection.inst), (Some(f.0), None));
    let support = everything(ARCH);
    let rejection = Classifier::new(&support).body(&program.m, &program.bodies[0]).unwrap_err();
    assert_eq!(rejection.construct, "invalid:function-attributes");
    assert_eq!(rejection.kind, RejectionKind::Invalid);
    assert_eq!(
        rejection.to_string(),
        format!("malformed input: invalid:function-attributes at {:?}", f.0)
    );
}

/// §8.7: a symbol that was named and never given a kind.
#[test]
fn a_symbol_must_be_resolved() {
    let mut m = module(ARCH);
    let x = m.symbol_named(b"x").unwrap();
    let unresolved = Program { m, bodies: Vec::new() };
    let rejection = malformed(&unresolved, "invalid:unresolved-symbol");
    assert_eq!(rejection.symbol, Some(x));
    // The shipped matrix says the same: it is not a question of support.
    assert_eq!(hod_ir::classify(&unresolved.m), Err(rejection));

    let mut m = unresolved.m;
    m.set_variable(x, &VariableDesc::new(I32)).unwrap();
    accepted(&Program { m, bodies: Vec::new() });
}

/// §8.7 `invalid:blockaddress`: the address of a block that does not
/// exist, or of a block of a function that has no body.
#[test]
fn a_blockaddress_must_name_a_block() {
    let in_body = |block: u32| {
        let mut fx = fx(ARCH);
        let address = constant(fx.m.consts.block_addr(fx.f.0, BlockId::new(block)).unwrap());
        fx.with(|b, _| {
            b.store(address, b.arg(1).unwrap(), align(8)).unwrap();
        })
    };
    malformed(&in_body(7), "invalid:blockaddress");
    accepted(&in_body(0));

    let in_init = |has_body: bool| {
        let mut m = module(ARCH);
        let f =
            if has_body { define(&mut m, "f", VOID, &[]) } else { declare(&mut m, "f", VOID, &[]) };
        let address = m.consts.block_addr(f.0, BlockId::new(0)).unwrap();
        defined(&mut m, "table", address);
        Program { m, bodies: Vec::new() }
    };
    malformed(&in_init(false), "invalid:blockaddress");
    accepted(&in_init(true));

    // Of a variable.
    let mut m = module(ARCH);
    let g = variable(&mut m, "g", I32);
    let address = m.consts.block_addr(g, BlockId::new(0)).unwrap();
    defined(&mut m, "table", address);
    malformed(&Program { m, bodies: Vec::new() }, "invalid:blockaddress");
}

/// §8.7 `invalid:type`: a value of a type that has no values, and a
/// call through a type that is not a function type.
#[test]
fn a_value_has_a_value_type() {
    let call = |through_function: bool| {
        let fx = fx(ARCH);
        let ty = if through_function { fx.callee.1 } else { I32 };
        let call = CallDesc::new(sym(fx.callee.0), ty, &[]);
        fx.with(|b, _| {
            b.call(VOID, &call).unwrap();
        })
    };
    malformed(&call(false), "invalid:type");
    accepted(&call(true));

    let load = |ty: TypeId| {
        fx(ARCH).with(|b, _| {
            b.load(ty, b.arg(1).unwrap(), align(4)).unwrap();
        })
    };
    malformed(&load(TypeId::LABEL), "invalid:type");
    malformed(&load(TypeId::METADATA), "invalid:type");
    malformed(&load(TypeId::new(60000)), "invalid:type");
    let mut opaque = fx(ARCH);
    let ty = opaque.m.types.opaque_struct().unwrap();
    malformed(
        &opaque.with(|b, _| {
            b.load(ty, b.arg(1).unwrap(), align(4)).unwrap();
        }),
        "invalid:type",
    );
    accepted(&load(I32));
}

/// §8.5, §8.7: every structural rule and every malformed-input outcome
/// this file triggers is in the published lists, and the lists have
/// nothing this file or the defensive checks do not name.
#[test]
fn the_rule_lists_are_the_rules() {
    let structural: Vec<&str> = STRUCTURAL_RULES.iter().map(|rule| rule.name).collect();
    assert_eq!(
        structural,
        [
            "tls:direct-operand",
            "tls:address-in-constant",
            "blockaddress:other-function",
            "musttail:callee-pop",
            "musttail:stack-args-grow",
            "musttail:stack-args-unknown",
            "atomic:scalable-vector",
            "addrspace:pointer",
            "addrspace:global",
            "addrspace:function",
            "addrspace:alloca",
            "limit:constant-nesting",
            "target:support-mismatch",
        ]
    );
    let invalid: Vec<&str> = INVALID_RULES.iter().map(|rule| rule.name).collect();
    assert_eq!(
        invalid,
        [
            "invalid:body-symbol",
            "invalid:body-not-function",
            "invalid:body-type",
            "invalid:body",
            "invalid:symbol",
            "invalid:unresolved-symbol",
            "invalid:inline-asm",
            "invalid:constant",
            "invalid:type",
            "invalid:blockaddress",
            "invalid:initializer",
            "invalid:function-attributes",
        ]
    );
    // None of them is a row: no `Support` switches a rule off.
    let support = Support::everything_implemented(ARCH);
    for name in structural.iter().chain(&invalid) {
        assert_eq!(support.supports(name), None, "{name}");
    }
}
