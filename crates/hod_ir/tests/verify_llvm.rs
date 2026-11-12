//! `spec/ir.md` §5.8: `Body::verify` and the module's checks refuse what
//! LLVM's verifier refuses. Each case builds the offending body or module
//! through `hod_ir` and its nearest valid variant, checks hod's verdict,
//! prints both for LLVM 22 and LLVM 19, and requires `llvm-as` (with its
//! verifier) to agree: refused when hod refuses, accepted when hod
//! accepts. A crash of LLVM's verifier counts as a refusal (§5.8 records
//! the one case where LLVM 22 crashes).
#![expect(clippy::many_single_char_names, reason = "short names for the values of a body")]
#![allow(clippy::similar_names, clippy::type_complexity, reason = "tables of cases")]

mod print_support;

use hod_ir::{
    Body, Bundle, CallConv, CallDesc, Clause, Dialect, FunctionDesc, InlineAsm, IntPredicate,
    Intrinsic, IrErrorKind, Module, Opcode, ParamAttrs, ParamMem, PrintError, SymId, TailKind,
    TargetSpec, TypeId, Value,
};
use print_support::{Fx, Llvm, declare, define, printed};

fn module() -> Module {
    Module::new(TargetSpec::x86_64_linux_gnu())
}

/// Checks hod's verdict on `body` of `m` against `refused`, then LLVM's.
fn check(what: &str, m: &Module, body: &Body, refused: Option<IrErrorKind>) {
    let verdict = body.verify(m).err().map(|error| error.kind);
    assert_eq!(verdict, refused, "{what}: hod's verdict");
    for major in [22, 19] {
        let Some(llvm) = Llvm::new(major) else { continue };
        let text = printed(m, llvm.dialect(), std::slice::from_ref(body));
        let objection = llvm.objection(&text);
        assert_eq!(
            objection.is_some(),
            refused.is_some(),
            "{what}: LLVM {major} {}:\n{}\n{text}",
            if objection.is_some() { "refuses" } else { "accepts" },
            objection.as_deref().unwrap_or(""),
        );
    }
}

/// A function `f` of type `ret (params...)` with a body to be written,
/// and the external `i32 @g()`; `f` has a personality when `eh`.
fn setup(m: &mut Module, ret: TypeId, params: &[TypeId], eh: bool) -> (SymId, SymId) {
    let g = declare(m, "g", TypeId::I32, &[], false);
    let f = if eh {
        let personality = declare(m, "__gxx_personality_v0", TypeId::I32, &[], true);
        let ty = m.types.function(ret, params, false).unwrap();
        let desc = FunctionDesc {
            has_body: true,
            personality: Some(personality),
            ..FunctionDesc::new(ty)
        };
        m.add_function(b"f", &desc).unwrap()
    } else {
        define(m, "f", ret, params)
    };
    (f, g)
}

/// `invoke i32 @g()`.
fn invoke_g(x: &mut Fx<'_>, g: SymId, normal: hod_ir::BlockId, unwind: hod_ir::BlockId) -> Value {
    let fn_ty = x.m.symbol(g).ty();
    x.b.invoke(TypeId::I32, &CallDesc::new(x.sym(g), fn_ty, &[]), normal, unwind).unwrap()
}

/// A cleanup landing pad that resumes.
fn cleanup(x: &mut Fx<'_>) {
    let pair = x.m.types.struct_type(&[TypeId::PTR, TypeId::I32], false).unwrap();
    let pad = x.b.landing_pad(pair, true, &[] as &[Clause]).unwrap();
    x.b.resume(pad).unwrap();
}

// ---- dominance ----

/// A value defined on one side of a branch, used after the join: refused;
/// through a `phi`: accepted.
#[test]
fn dominance_across_a_branch() {
    for with_phi in [false, true] {
        let mut m = module();
        let (f, _) = setup(&mut m, TypeId::I32, &[TypeId::I32], false);
        let mut x = Fx::new(&mut m, f);
        let n = x.arg(0);
        let zero = x.i32(0);
        let one = x.i32(1);
        let c = x.icmp(IntPredicate::Eq, n, zero);
        let (a, b, join) = (x.block(), x.block(), x.block());
        x.cond_br(c, a, b);
        x.at(a);
        let v = x.bin(Opcode::Add, TypeId::I32, n, one);
        x.br(join);
        x.at(b);
        x.br(join);
        x.at(join);
        let result = if with_phi { x.phi(TypeId::I32, &[(v, a), (n, b)]) } else { v };
        x.ret(result);
        let body = x.finish();
        let refused = (!with_phi).then_some(IrErrorKind::NotDominated);
        check(&format!("use across a branch, phi {with_phi}"), &m, &body, refused);
    }
}

/// An instruction that uses its own result, and a use before the
/// definition in the same block: refused where control reaches the
/// block, accepted in a block it does not reach.
#[test]
fn dominance_within_a_block() {
    for self_use in [true, false] {
        for reachable in [true, false] {
            let mut m = module();
            let (f, _) = setup(&mut m, TypeId::I32, &[TypeId::I32], false);
            let mut x = Fx::new(&mut m, f);
            let n = x.arg(0);
            let one = x.i32(1);
            if !reachable {
                x.ret(n);
                let dead = x.block();
                x.at(dead);
            }
            let r = x.b.reserve(TypeId::I32).unwrap();
            let result = if self_use {
                let v = x.bin(Opcode::Add, TypeId::I32, r, one);
                x.b.resolve(r, v).unwrap();
                v
            } else {
                let early = x.bin(Opcode::Add, TypeId::I32, r, one);
                let late = x.bin(Opcode::Mul, TypeId::I32, n, one);
                x.b.resolve(r, late).unwrap();
                early
            };
            x.ret(result);
            let body = x.finish();
            let refused = reachable.then_some(IrErrorKind::NotDominated);
            check(&format!("self use {self_use}, reachable {reachable}"), &m, &body, refused);
        }
    }
}

/// A value defined in a block control does not reach, used where it
/// does: refused; reaching the use through a `phi` entry for the
/// unreachable predecessor: accepted.
#[test]
fn dominance_from_an_unreachable_block() {
    for with_phi in [false, true] {
        let mut m = module();
        let (f, _) = setup(&mut m, TypeId::I32, &[TypeId::I32], false);
        let mut x = Fx::new(&mut m, f);
        let entry = x.here();
        let n = x.arg(0);
        let one = x.i32(1);
        let (dead, join) = (x.block(), x.block());
        x.br(join);
        x.at(dead);
        let v = x.bin(Opcode::Add, TypeId::I32, n, one);
        x.br(join);
        x.at(join);
        let result = if with_phi { x.phi(TypeId::I32, &[(n, entry), (v, dead)]) } else { v };
        x.ret(result);
        let body = x.finish();
        let refused = (!with_phi).then_some(IrErrorKind::NotDominated);
        check(&format!("unreachable definition, phi {with_phi}"), &m, &body, refused);
    }
}

/// An `invoke`'s result exists on its normal edge only: refused in the
/// unwind destination, accepted in the normal one.
#[test]
fn invoke_result_off_the_normal_edge() {
    for in_unwind in [true, false] {
        let mut m = module();
        let (f, g) = setup(&mut m, TypeId::I32, &[], true);
        let mut x = Fx::new(&mut m, f);
        let (normal, unwind) = (x.block(), x.block());
        let r = invoke_g(&mut x, g, normal, unwind);
        x.at(normal);
        x.ret(r);
        x.at(unwind);
        if in_unwind {
            let pair = x.m.types.struct_type(&[TypeId::PTR, TypeId::I32], false).unwrap();
            x.b.landing_pad(pair, true, &[]).unwrap();
            x.ret(r);
        } else {
            cleanup(&mut x);
        }
        let body = x.finish();
        let refused = in_unwind.then_some(IrErrorKind::NotDominated);
        check(&format!("invoke result in unwind dest {in_unwind}"), &m, &body, refused);
    }
}

/// An `invoke` whose normal destination has another predecessor: its
/// result does not dominate that block, except as the `phi` entry for the
/// invoke's own edge.
#[test]
fn invoke_result_over_a_critical_edge() {
    for with_phi in [false, true] {
        let mut m = module();
        let (f, g) = setup(&mut m, TypeId::I32, &[TypeId::I1], true);
        let mut x = Fx::new(&mut m, f);
        let entry = x.here();
        let c = x.arg(0);
        let zero = x.i32(0);
        let (call, normal, unwind) = (x.block(), x.block(), x.block());
        x.cond_br(c, call, normal);
        x.at(call);
        let r = invoke_g(&mut x, g, normal, unwind);
        x.at(normal);
        let result = if with_phi { x.phi(TypeId::I32, &[(zero, entry), (r, call)]) } else { r };
        x.ret(result);
        x.at(unwind);
        cleanup(&mut x);
        let body = x.finish();
        let refused = (!with_phi).then_some(IrErrorKind::NotDominated);
        check(&format!("invoke over a critical edge, phi {with_phi}"), &m, &body, refused);
    }
}

/// Accepted: an `invoke` whose normal destination is also entered by a
/// back edge from a block it dominates, and an `invoke` that loops to its
/// own block, using its result in that block's `phi`.
#[test]
fn invoke_result_with_back_edges() {
    for self_loop in [false, true] {
        let mut m = module();
        let (f, g) = setup(&mut m, TypeId::I32, &[TypeId::I1], true);
        let mut x = Fx::new(&mut m, f);
        let entry = x.here();
        let c = x.arg(0);
        let zero = x.i32(0);
        let one = x.i32(1);
        let (normal, unwind, exit) = (x.block(), x.block(), x.block());
        if self_loop {
            let lp = normal;
            x.br(lp);
            x.at(lp);
            let r = x.b.reserve(TypeId::I32).unwrap();
            let p = x.phi(TypeId::I32, &[(zero, entry), (r, lp)]);
            let _ = p;
            let v = invoke_g(&mut x, g, lp, unwind);
            x.b.resolve(r, v).unwrap();
            x.at(exit);
            x.ret(zero);
        } else {
            let r = invoke_g(&mut x, g, normal, unwind);
            x.at(normal);
            let v = x.bin(Opcode::Add, TypeId::I32, r, one);
            let _ = v;
            x.cond_br(c, normal, exit);
            x.at(exit);
            x.ret(r);
        }
        x.at(unwind);
        cleanup(&mut x);
        let body = x.finish();
        check(&format!("invoke with a back edge, self loop {self_loop}"), &m, &body, None);
    }
}

/// A `callbr`'s result is defined at the end of its block, so it may be
/// used in an indirect destination the block dominates; not in a block
/// with another predecessor.
#[test]
fn callbr_result_dominance() {
    for other_pred in [false, true] {
        let mut m = module();
        let (f, _) = setup(&mut m, TypeId::I32, &[TypeId::I1], false);
        let fn_ty = m.types.function(TypeId::I32, &[], false).unwrap();
        let asm = InlineAsm {
            template: b"",
            constraints: b"=r,!i",
            flags: InlineAsm::SIDE_EFFECT,
            ty: fn_ty,
        };
        let callee = Value::asm(m.inline_asm(&asm).unwrap()).unwrap();
        let mut x = Fx::new(&mut m, f);
        let c = x.arg(0);
        let (call, fallthrough, indirect) = (x.block(), x.block(), x.block());
        if other_pred {
            x.cond_br(c, call, indirect);
        } else {
            x.br(call);
        }
        x.at(call);
        let r =
            x.b.callbr(TypeId::I32, &CallDesc::new(callee, fn_ty, &[]), fallthrough, &[indirect])
                .unwrap();
        x.at(fallthrough);
        x.ret(r);
        x.at(indirect);
        x.ret(r);
        let body = x.finish();
        let refused = other_pred.then_some(IrErrorKind::NotDominated);
        check(&format!("callbr result, other predecessor {other_pred}"), &m, &body, refused);
    }
}

// ---- calls ----

/// Like [`check`], for a body hod refuses whose text LLVM cannot even
/// parse: the printer must refuse it as a construct no dialect has.
fn check_unprintable(what: &str, m: &Module, body: &Body, refused: IrErrorKind) {
    assert_eq!(body.verify(m).err().map(|error| error.kind), Some(refused), "{what}");
    for dialect in Dialect::ALL {
        let mut text = String::new();
        let printed = m.print(&mut text, dialect, [body]);
        assert!(matches!(printed, Err(PrintError::Unsupported { .. })), "{what}: {printed:?}");
    }
}

/// An inline-assembly callee of type `fn_ty`.
fn asm(m: &mut Module, constraints: &[u8], flags: u8, fn_ty: TypeId) -> Value {
    let asm = InlineAsm { template: b"", constraints, flags, ty: fn_ty };
    Value::asm(m.inline_asm(&asm).unwrap()).unwrap()
}

/// `callbr` calls inline assembly with as many label constraints as it
/// has indirect destinations, which does not unwind; LLVM 22 crashes on
/// a `callbr` of a function (§5.8).
#[test]
fn callbr_callees() {
    // (what, callee constraints or a function, indirect destinations, flags)
    let cases: [(&str, Option<&[u8]>, usize, u8, bool); 6] = [
        ("asm with one label", Some(b"!i"), 1, 0, true),
        ("asm with two labels", Some(b"!i,!i"), 2, 0, true),
        ("a function", None, 1, 0, false),
        ("asm with too few labels", Some(b"!i"), 2, 0, false),
        ("asm with too many labels", Some(b"!i,!i"), 1, 0, false),
        ("asm that unwinds", Some(b"!i"), 1, InlineAsm::UNWIND, false),
    ];
    for (what, constraints, dests, flags, ok) in cases {
        let mut m = module();
        let (f, _) = setup(&mut m, TypeId::VOID, &[], false);
        let fn_ty = m.types.function(TypeId::VOID, &[], false).unwrap();
        let callee = match constraints {
            Some(constraints) => asm(&mut m, constraints, flags, fn_ty),
            None => Value::sym(declare(&mut m, "h", TypeId::VOID, &[], false)).unwrap(),
        };
        let mut x = Fx::new(&mut m, f);
        let fallthrough = x.block();
        let indirect: Vec<_> = (0..dests).map(|_| x.block()).collect();
        x.b.callbr(TypeId::VOID, &CallDesc::new(callee, fn_ty, &[]), fallthrough, &indirect)
            .unwrap();
        for block in [fallthrough].into_iter().chain(indirect) {
            x.at(block);
            x.ret_void();
        }
        let body = x.finish();
        check(&format!("callbr of {what}"), &m, &body, (!ok).then_some(IrErrorKind::InvalidCall));
    }
}

/// Label constraints are for `callbr`: refused on a `call`.
#[test]
fn label_constraints_on_a_call() {
    for constraints in [&b"!i"[..], b""] {
        let mut m = module();
        let (f, _) = setup(&mut m, TypeId::VOID, &[], false);
        let fn_ty = m.types.function(TypeId::VOID, &[], false).unwrap();
        let callee = asm(&mut m, constraints, InlineAsm::SIDE_EFFECT, fn_ty);
        let mut x = Fx::new(&mut m, f);
        x.b.call(TypeId::VOID, &CallDesc::new(callee, fn_ty, &[])).unwrap();
        x.ret_void();
        let body = x.finish();
        let refused = (!constraints.is_empty()).then_some(IrErrorKind::InvalidCall);
        check(&format!("call of asm {constraints:?}"), &m, &body, refused);
    }
}

/// `invoke` of an intrinsic: LLVM's list (the coroutine resumers,
/// `llvm.donothing`, the patchpoints ...) and nothing else.
#[test]
fn invoke_of_intrinsics() {
    let cases: [(Intrinsic<'_>, bool); 5] = [
        (Intrinsic::Other("llvm.donothing"), true),
        (Intrinsic::CoroResume, true),
        (Intrinsic::CoroDestroy, true),
        (Intrinsic::Trap, false),
        (Intrinsic::Other("llvm.hod.unknown"), false),
    ];
    for (intrinsic, ok) in cases {
        let mut m = module();
        let (f, _) = setup(&mut m, TypeId::VOID, &[], true);
        let coro = matches!(intrinsic, Intrinsic::CoroResume | Intrinsic::CoroDestroy);
        let params: &[TypeId] = if coro { &[TypeId::PTR] } else { &[] };
        let fn_ty = m.types.function(TypeId::VOID, params, false).unwrap();
        let mut x = Fx::new(&mut m, f);
        let null = x.null();
        let args: &[Value] = if coro { &[null] } else { &[] };
        let (normal, unwind) = (x.block(), x.block());
        let desc = CallDesc::intrinsic(intrinsic, &[], fn_ty, args);
        x.b.invoke(TypeId::VOID, &desc, normal, unwind).unwrap();
        x.at(normal);
        x.ret_void();
        x.at(unwind);
        cleanup(&mut x);
        let body = x.finish();
        let refused = (!ok).then_some(IrErrorKind::InvalidCall);
        check(&format!("invoke of {}", intrinsic.name()), &m, &body, refused);
    }
}

/// A tail marker on `invoke` or `callbr`: refused; LLVM's text has no
/// way to write one.
#[test]
fn tail_markers_on_terminators() {
    for tail in [TailKind::Tail, TailKind::MustTail, TailKind::NoTail] {
        for callbr in [false, true] {
            let mut m = module();
            let (f, g) = setup(&mut m, TypeId::VOID, &[], true);
            let fn_ty = m.types.function(TypeId::VOID, &[], false).unwrap();
            let callee = if callbr { asm(&mut m, b"!i", 0, fn_ty) } else { Value::sym(g).unwrap() };
            let mut x = Fx::new(&mut m, f);
            let (a, b) = (x.block(), x.block());
            let desc = CallDesc { tail, ..CallDesc::new(callee, fn_ty, &[]) };
            if callbr {
                x.b.callbr(TypeId::VOID, &desc, a, &[b]).unwrap();
            } else {
                let i32_fn = x.m.symbol(g).ty();
                let desc = CallDesc { tail, ..CallDesc::new(callee, i32_fn, &[]) };
                x.b.invoke(TypeId::I32, &desc, a, b).unwrap();
            }
            x.at(a);
            x.ret_void();
            x.at(b);
            if callbr {
                x.ret_void();
            } else {
                cleanup(&mut x);
            }
            let body = x.finish();
            let what = format!("{} on callbr {callbr}", tail.name());
            check_unprintable(&what, &m, &body, IrErrorKind::InvalidCall);
        }
    }
}

/// A function signature for the `musttail` cases.
#[derive(Clone, Copy)]
struct Sig<'a> {
    ret: TypeId,
    params: &'a [TypeId],
    variadic: bool,
    cc: CallConv,
    attrs: &'a [ParamAttrs],
}

const I32_SIG: Sig<'static> =
    Sig { ret: TypeId::I32, params: &[TypeId::I32], variadic: false, cc: CallConv::C, attrs: &[] };

/// What follows the `musttail` call.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Then {
    /// `ret` of the result, or `ret void`.
    Ret,
    /// `ret poison`.
    RetPoison,
    /// `ret` of the first argument.
    RetOther,
    /// An `add`, then `ret` of the result.
    Gap,
    /// A `bitcast` of the result, then `ret` of it.
    BitCast,
}

/// What the `musttail` call calls.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Callee {
    Function,
    Trap,
    Asm,
}

/// `caller` calling `callee` with `musttail`, then `then`.
fn must_tail(what: &str, ok: bool, caller: Sig<'_>, callee: Sig<'_>, kind: Callee, then: Then) {
    let mut m = module();
    let caller_ty = m.types.function(caller.ret, caller.params, caller.variadic).unwrap();
    let desc = FunctionDesc {
        cc: caller.cc,
        param_attrs: caller.attrs,
        has_body: true,
        ..FunctionDesc::new(caller_ty)
    };
    let f = m.add_function(b"f", &desc).unwrap();
    let callee_ty = m.types.function(callee.ret, callee.params, callee.variadic).unwrap();
    let desc = FunctionDesc { cc: callee.cc, ..FunctionDesc::new(callee_ty) };
    let g = m.add_function(b"g", &desc).unwrap();
    let constraints = if callee.ret == TypeId::VOID { "" } else { "=r" };
    let inline = asm(&mut m, constraints.as_bytes(), 0, callee_ty);
    let mut x = Fx::new(&mut m, f);
    let args: Vec<Value> = callee
        .params
        .iter()
        .map(|&ty| if ty == TypeId::PTR { x.null() } else { x.int(ty, 1) })
        .collect();
    let base = match kind {
        Callee::Function => CallDesc::new(Value::sym(g).unwrap(), callee_ty, &args),
        Callee::Trap => CallDesc::intrinsic(Intrinsic::Trap, &[], callee_ty, &args),
        Callee::Asm => CallDesc::new(inline, callee_ty, &args),
    };
    let desc =
        CallDesc { tail: TailKind::MustTail, cc: callee.cc, arg_attrs: callee.attrs, ..base };
    let r = x.b.call(callee.ret, &desc).unwrap();
    let returned = match then {
        Then::Ret => r,
        Then::RetPoison => {
            Value::constant(x.m.consts.poison(&x.m.types, caller.ret).unwrap()).unwrap()
        }
        Then::RetOther => x.arg(0),
        Then::Gap => {
            x.bin(Opcode::Add, TypeId::I32, r, r);
            r
        }
        Then::BitCast => x.cast(Opcode::BitCast, callee.ret, r),
    };
    if caller.ret == TypeId::VOID {
        x.ret_void();
    } else {
        x.ret(returned);
    }
    let body = x.finish();
    let what = format!("musttail: {what}");
    check(&what, &m, &body, (!ok).then_some(IrErrorKind::InvalidCall));
}

/// `musttail` (§5.8): LLVM's `verifyMustTailCall`, each condition kept
/// and broken once.
#[test]
fn musttail_conditions() {
    use Callee::{Asm, Function, Trap};
    use Then::{BitCast, Gap, Ret, RetOther, RetPoison};
    let i32_sig = I32_SIG;
    let i64_sig = Sig { params: &[TypeId::I64], ..i32_sig };
    let ptr_sig = Sig { ret: TypeId::PTR, params: &[TypeId::PTR], ..i32_sig };
    let void_sig = Sig { ret: TypeId::VOID, ..i32_sig };
    let variadic = Sig { variadic: true, ..i32_sig };
    let fast = Sig { cc: CallConv::FAST, ..i32_sig };
    let inreg = [ParamAttrs::NONE.with(ParamAttrs::INREG).unwrap()];
    let signext = [ParamAttrs::NONE.with(ParamAttrs::SIGNEXT).unwrap()];
    let with_inreg = Sig { attrs: &inreg, ..i32_sig };
    let with_signext = Sig { attrs: &signext, ..i32_sig };
    let trap = Sig { ret: TypeId::VOID, params: &[], ..i32_sig };
    let tail = Sig { cc: CallConv::TAIL, ..i32_sig };
    let tail_two = Sig { params: &[TypeId::I64, TypeId::I64], ..tail };
    let tail_inreg = Sig { attrs: &inreg, ..tail };
    let tail_variadic = Sig { variadic: true, ..tail };
    let cases = [
        ("the same signature", true, i32_sig, i32_sig, Function, Ret),
        ("ret poison", true, i32_sig, i32_sig, Function, RetPoison),
        ("a bitcast between", true, ptr_sig, ptr_sig, Function, BitCast),
        ("ret of something else", false, i32_sig, i32_sig, Function, RetOther),
        ("an instruction between", false, i32_sig, i32_sig, Function, Gap),
        ("other parameters", false, i32_sig, i64_sig, Function, Ret),
        ("another return type", false, void_sig, i32_sig, Function, Ret),
        ("one side variadic", false, variadic, i32_sig, Function, Ret),
        ("both variadic", true, variadic, variadic, Function, Ret),
        ("another convention", false, i32_sig, fast, Function, Ret),
        ("inline assembly", false, i32_sig, i32_sig, Asm, Ret),
        ("inreg on one side", false, with_inreg, i32_sig, Function, Ret),
        ("inreg on both", true, with_inreg, with_inreg, Function, Ret),
        ("signext on one side", true, with_signext, i32_sig, Function, Ret),
        ("an intrinsic with other parameters", true, void_sig, trap, Trap, Ret),
        (
            "an intrinsic, inreg on the caller",
            false,
            Sig { attrs: &inreg, ..void_sig },
            trap,
            Trap,
            Ret,
        ),
        ("tailcc with other parameters", true, tail, tail_two, Function, Ret),
        ("tailcc with inreg", false, tail_inreg, tail_inreg, Function, Ret),
        ("tailcc and variadic", false, tail_variadic, tail_variadic, Function, Ret),
    ];
    for (what, ok, caller, callee, kind, then) in cases {
        must_tail(what, ok, caller, callee, kind, then);
    }
}

/// An `immarg` argument (§5.8) is a constant integer: not a value, not
/// `undef` or `poison`, in LLVM 19 as in LLVM 22.
#[test]
fn immediate_arguments() {
    for (what, ok) in
        [("a constant", true), ("an argument", false), ("undef", false), ("poison", false)]
    {
        let mut m = module();
        let (f, _) = setup(&mut m, TypeId::VOID, &[TypeId::PTR, TypeId::I1], false);
        let memcpy_ty = m
            .types
            .function(TypeId::VOID, &[TypeId::PTR, TypeId::PTR, TypeId::I64, TypeId::I1], false)
            .unwrap();
        let mut x = Fx::new(&mut m, f);
        let (p, c) = (x.arg(0), x.arg(1));
        let flag = match what {
            "a constant" => x.int(TypeId::I1, 0),
            "an argument" => c,
            "undef" => Value::constant(x.m.consts.undef(&x.m.types, TypeId::I1).unwrap()).unwrap(),
            _ => Value::constant(x.m.consts.poison(&x.m.types, TypeId::I1).unwrap()).unwrap(),
        };
        let size = x.i64(8);
        let overloads = [TypeId::PTR, TypeId::PTR, TypeId::I64];
        let args = [p, p, size, flag];
        x.b.call(
            TypeId::VOID,
            &CallDesc::intrinsic(Intrinsic::Memcpy, &overloads, memcpy_ty, &args),
        )
        .unwrap();
        x.ret_void();
        let body = x.finish();
        check(
            &format!("memcpy's flag {what}"),
            &m,
            &body,
            (!ok).then_some(IrErrorKind::InvalidCall),
        );
    }
}

/// The `kcfi` bundle has one `i32` constant; a call has at most one of
/// each bundle LLVM knows of (`deopt`, `kcfi`, ...), and any number of
/// one it does not.
#[test]
fn operand_bundles() {
    for case in 0..7 {
        let mut m = module();
        let (f, g) = setup(&mut m, TypeId::VOID, &[TypeId::I32], false);
        let mut x = Fx::new(&mut m, f);
        let n = x.arg(0);
        let (k32, k64) = (x.i32(7), x.i64(7));
        let (one, two): ([Value; 1], [Value; 2]) = ([k32], [k32, k32]);
        let (wide, variable) = ([k64], [n]);
        let (what, ok, bundles): (&str, bool, Vec<Bundle<'_>>) = match case {
            0 => ("kcfi with an i32", true, vec![Bundle { tag: "kcfi", inputs: &one }]),
            1 => ("kcfi without operands", false, vec![Bundle { tag: "kcfi", inputs: &[] }]),
            2 => ("kcfi with two", false, vec![Bundle { tag: "kcfi", inputs: &two }]),
            3 => ("kcfi with an i64", false, vec![Bundle { tag: "kcfi", inputs: &wide }]),
            4 => ("kcfi with an argument", false, vec![Bundle { tag: "kcfi", inputs: &variable }]),
            5 => {
                let deopt = Bundle { tag: "deopt", inputs: &one };
                ("two deopt", false, vec![deopt, deopt])
            }
            _ => {
                let other = Bundle { tag: "hod.other", inputs: &one };
                ("two of an unknown tag", true, vec![other, other])
            }
        };
        let fn_ty = x.m.symbol(g).ty();
        let desc = CallDesc { bundles: &bundles, ..CallDesc::new(x.sym(g), fn_ty, &[]) };
        x.b.call(TypeId::I32, &desc).unwrap();
        x.ret_void();
        let body = x.finish();
        check(what, &m, &body, (!ok).then_some(IrErrorKind::InvalidCall));
    }
}

// ---- attributes ----

/// An argument of type `ty`: a constant of it.
fn arg_of(x: &mut Fx<'_>, ty: TypeId) -> Value {
    if ty == TypeId::PTR {
        return x.null();
    }
    if x.m.types.is_int(ty) {
        return x.int(ty, 1);
    }
    if ty == TypeId::FLOAT {
        return x.f32(1.0);
    }
    Value::constant(x.m.consts.zero(&x.m.types, ty).unwrap()).unwrap()
}

/// A call of `@h` of type `ret (params..., ...?)` with arguments of
/// `args` (the parameters, then variable arguments), carrying `attrs`
/// and `ret_attrs`; of `llvm.va_end` instead when `intrinsic`.
#[derive(Clone, Copy)]
struct AttrCase<'a> {
    ret: TypeId,
    ret_attrs: ParamAttrs,
    params: &'a [TypeId],
    variadic: bool,
    args: &'a [TypeId],
    attrs: &'a [ParamAttrs],
    intrinsic: bool,
}

/// The types of the attribute cases past the built-in ones: `<2 x i32>`,
/// `<2 x ptr>` and an opaque struct. A module made the same way has
/// them with the same ids.
fn extra_types(m: &mut Module) -> (TypeId, TypeId, TypeId) {
    let v2i32 = m.types.vector(TypeId::I32, 2).unwrap();
    let v2ptr = m.types.vector(TypeId::PTR, 2).unwrap();
    (v2i32, v2ptr, m.types.opaque_struct().unwrap())
}

fn attr_case(what: &str, ok: bool, case: AttrCase<'_>) {
    let mut m = module();
    extra_types(&mut m);
    let (f, _) = setup(&mut m, TypeId::VOID, &[], false);
    let fn_ty = m.types.function(case.ret, case.params, case.variadic).unwrap();
    let h = m.add_function(b"h", &FunctionDesc::new(fn_ty)).unwrap();
    let mut x = Fx::new(&mut m, f);
    let args: Vec<Value> = case.args.iter().map(|&ty| arg_of(&mut x, ty)).collect();
    let base = if case.intrinsic {
        CallDesc::intrinsic(Intrinsic::VaEnd, &[TypeId::PTR], fn_ty, &args)
    } else {
        CallDesc::new(x.sym(h), fn_ty, &args)
    };
    let mut attrs = case.attrs.to_vec();
    attrs.resize(args.len(), ParamAttrs::NONE);
    let desc = CallDesc { arg_attrs: &attrs, ret_attrs: case.ret_attrs, ..base };
    x.b.call(case.ret, &desc).unwrap();
    x.ret_void();
    let body = x.finish();
    check(
        &format!("attributes: {what}"),
        &m,
        &body,
        (!ok).then_some(IrErrorKind::InvalidAttribute),
    );
}

/// Parameter attributes on call arguments (§5.8), which are checked as a
/// function's parameters are: each kept and broken.
#[test]
#[expect(clippy::too_many_lines, reason = "a table of cases")]
fn parameter_attributes() {
    let flag = |flag| ParamAttrs::NONE.with(flag).unwrap();
    let mem = |mem, ty| ParamAttrs::NONE.with_mem(mem, ty).unwrap();
    let align = ParamAttrs::NONE.with_align(8).unwrap();
    let none = ParamAttrs::NONE;
    let (v2i32, v2ptr, opaque) = extra_types(&mut module());
    let one = |ty: TypeId| [ty];
    let base = AttrCase {
        ret: TypeId::VOID,
        ret_attrs: none,
        params: &[],
        variadic: false,
        args: &[],
        attrs: &[],
        intrinsic: false,
    };
    let signext = [flag(ParamAttrs::SIGNEXT)];
    let nest = [flag(ParamAttrs::NEST)];
    let swifterror = [flag(ParamAttrs::SWIFT_ERROR)];
    let aligned = [align];
    let byval = [mem(ParamMem::ByVal, TypeId::I32)];
    let byval_opaque = [mem(ParamMem::ByVal, opaque)];
    let byref_opaque = [mem(ParamMem::ByRef, opaque)];
    let inalloca_opaque = [mem(ParamMem::InAlloca, opaque)];
    let preallocated_opaque = [mem(ParamMem::Preallocated, opaque)];
    let byval_inreg = [mem(ParamMem::ByVal, TypeId::I32).with(ParamAttrs::INREG).unwrap()];
    let sret_inreg = [mem(ParamMem::Sret, TypeId::I32).with(ParamAttrs::INREG).unwrap()];
    let nest_inreg = [flag(ParamAttrs::NEST).with(ParamAttrs::INREG).unwrap()];
    let sret = mem(ParamMem::Sret, TypeId::I32);
    let inalloca = mem(ParamMem::InAlloca, TypeId::I32);
    let swiftself = flag(ParamAttrs::SWIFT_SELF);
    let element = [mem(ParamMem::ElementType, TypeId::I32)];
    let (i32s, floats, vectors) = (one(TypeId::I32), one(TypeId::FLOAT), one(v2i32));
    let (ptrs, i64s, ptr_vectors) = (one(TypeId::PTR), one(TypeId::I64), one(v2ptr));
    let three = [TypeId::PTR; 3];
    let two = [TypeId::PTR; 2];
    for (what, ok, case) in [
        ("signext on i32", true, AttrCase { params: &i32s, args: &i32s, attrs: &signext, ..base }),
        (
            "signext on float",
            false,
            AttrCase { params: &floats, args: &floats, attrs: &signext, ..base },
        ),
        (
            "signext on a vector",
            false,
            AttrCase { params: &vectors, args: &vectors, attrs: &signext, ..base },
        ),
        ("nest on ptr", true, AttrCase { params: &ptrs, args: &ptrs, attrs: &nest, ..base }),
        ("nest on i32", false, AttrCase { params: &i32s, args: &i32s, attrs: &nest, ..base }),
        (
            "swifterror on i32",
            false,
            AttrCase { params: &i32s, args: &i32s, attrs: &swifterror, ..base },
        ),
        ("align on ptr", true, AttrCase { params: &ptrs, args: &ptrs, attrs: &aligned, ..base }),
        (
            "align on a pointer vector",
            true,
            AttrCase { params: &ptr_vectors, args: &ptr_vectors, attrs: &aligned, ..base },
        ),
        ("align on i64", false, AttrCase { params: &i64s, args: &i64s, attrs: &aligned, ..base }),
        ("byval on ptr", true, AttrCase { params: &ptrs, args: &ptrs, attrs: &byval, ..base }),
        ("byval on i64", false, AttrCase { params: &i64s, args: &i64s, attrs: &byval, ..base }),
        (
            "byval of an opaque struct",
            false,
            AttrCase { params: &ptrs, args: &ptrs, attrs: &byval_opaque, ..base },
        ),
        (
            "byref of an opaque struct",
            false,
            AttrCase { params: &ptrs, args: &ptrs, attrs: &byref_opaque, ..base },
        ),
        (
            "inalloca of an opaque struct",
            false,
            AttrCase { params: &ptrs, args: &ptrs, attrs: &inalloca_opaque, ..base },
        ),
        (
            "preallocated of an opaque struct",
            false,
            AttrCase { params: &ptrs, args: &ptrs, attrs: &preallocated_opaque, ..base },
        ),
        (
            "byval with inreg",
            false,
            AttrCase { params: &ptrs, args: &ptrs, attrs: &byval_inreg, ..base },
        ),
        (
            "sret with inreg",
            true,
            AttrCase { params: &ptrs, args: &ptrs, attrs: &sret_inreg, ..base },
        ),
        (
            "nest with inreg",
            false,
            AttrCase { params: &ptrs, args: &ptrs, attrs: &nest_inreg, ..base },
        ),
        (
            "sret on the second parameter",
            true,
            AttrCase { params: &three, args: &three, attrs: &[none, sret], ..base },
        ),
        (
            "sret on the third parameter",
            false,
            AttrCase { params: &three, args: &three, attrs: &[none, none, sret], ..base },
        ),
        (
            "sret twice",
            false,
            AttrCase { params: &three, args: &three, attrs: &[sret, sret], ..base },
        ),
        (
            "sret on a variable argument",
            false,
            AttrCase { params: &[], variadic: true, args: &ptrs, attrs: &[sret], ..base },
        ),
        (
            "inalloca on the last parameter",
            true,
            AttrCase { params: &two, args: &two, attrs: &[none, inalloca], ..base },
        ),
        (
            "inalloca on another parameter",
            false,
            AttrCase { params: &two, args: &two, attrs: &[inalloca, none], ..base },
        ),
        (
            "swiftself once",
            true,
            AttrCase { params: &two, args: &two, attrs: &[swiftself, none], ..base },
        ),
        (
            "swiftself twice",
            false,
            AttrCase { params: &two, args: &two, attrs: &[swiftself, swiftself], ..base },
        ),
        (
            "swiftself twice, one a variable argument",
            true,
            AttrCase {
                params: &ptrs,
                variadic: true,
                args: &two,
                attrs: &[swiftself, swiftself],
                ..base
            },
        ),
        (
            "nest twice, one a variable argument",
            false,
            AttrCase {
                params: &ptrs,
                variadic: true,
                args: &two,
                attrs: &[nest[0], nest[0]],
                ..base
            },
        ),
        (
            "elementtype on a function's argument",
            false,
            AttrCase { params: &ptrs, args: &ptrs, attrs: &element, ..base },
        ),
        (
            "elementtype on an intrinsic's argument",
            true,
            AttrCase { params: &ptrs, args: &ptrs, attrs: &element, intrinsic: true, ..base },
        ),
        (
            "signext on an i32 result",
            true,
            AttrCase { ret: TypeId::I32, ret_attrs: signext[0], ..base },
        ),
        (
            "signext on a float result",
            false,
            AttrCase { ret: TypeId::FLOAT, ret_attrs: signext[0], ..base },
        ),
        (
            "align on a pointer result",
            true,
            AttrCase { ret: TypeId::PTR, ret_attrs: align, ..base },
        ),
    ] {
        attr_case(what, ok, case);
    }
}

/// Attributes LLVM's text cannot put on a return value: refused, and the
/// printer writes what LLVM then refuses (LLVM 22 takes `alignstack`
/// there; LLVM 19 does not parse it, so hod refuses it).
#[test]
fn return_attributes() {
    let nest = ParamAttrs::NONE.with(ParamAttrs::NEST).unwrap();
    let stack = ParamAttrs::NONE.with_stack_align(16).unwrap();
    for (what, attrs, refused_by_22) in [("nest", nest, true), ("alignstack", stack, false)] {
        let mut m = module();
        let (f, _) = setup(&mut m, TypeId::VOID, &[], false);
        let fn_ty = m.types.function(TypeId::PTR, &[], false).unwrap();
        let h = m.add_function(b"h", &FunctionDesc::new(fn_ty)).unwrap();
        let mut x = Fx::new(&mut m, f);
        let desc = CallDesc { ret_attrs: attrs, ..CallDesc::new(x.sym(h), fn_ty, &[]) };
        x.b.call(TypeId::PTR, &desc).unwrap();
        x.ret_void();
        let body = x.finish();
        assert_eq!(body.verify(&m).unwrap_err().kind, IrErrorKind::InvalidAttribute, "{what}");
        for (major, refused) in [(19, true), (22, refused_by_22)] {
            let Some(llvm) = Llvm::new(major) else { continue };
            let text = printed(&m, llvm.dialect(), std::slice::from_ref(&body));
            assert_eq!(
                llvm.objection(&text).is_some(),
                refused,
                "{what} on a result, LLVM {major}"
            );
        }
    }
}

/// A function's own signature is checked when it is described (§5.7):
/// `Module::add_function` refuses what a call's arguments may not have,
/// and `elementtype`, which LLVM allows on a call's argument alone.
#[test]
fn function_signatures() {
    let mut m = module();
    let ty =
        m.types.function(TypeId::VOID, &[TypeId::PTR, TypeId::PTR, TypeId::PTR], false).unwrap();
    let sret = ParamAttrs::NONE.with_mem(ParamMem::Sret, TypeId::I32).unwrap();
    let signext = ParamAttrs::NONE.with(ParamAttrs::SIGNEXT).unwrap();
    let element = ParamAttrs::NONE.with_mem(ParamMem::ElementType, TypeId::I32).unwrap();
    let none = ParamAttrs::NONE;
    for (what, attrs, ok) in [
        ("sret on the second parameter", [none, sret, none], true),
        ("sret on the third parameter", [none, none, sret], false),
        ("signext on a pointer", [signext, none, none], false),
        ("elementtype", [element, none, none], false),
    ] {
        let desc = FunctionDesc { param_attrs: &attrs, ..FunctionDesc::new(ty) };
        // A refused description leaves its name unresolved: each is
        // tried in a module of its own, where the type has the same id.
        let made = if ok {
            m.add_function(b"accepted", &desc)
        } else {
            let mut fresh = module();
            let params = [TypeId::PTR; 3];
            assert_eq!(fresh.types.function(TypeId::VOID, &params, false).unwrap(), ty);
            fresh.add_function(b"h", &desc)
        };
        assert_eq!(made.is_ok(), ok, "{what}: {made:?}");
        if !ok {
            assert_eq!(made.unwrap_err().kind, IrErrorKind::InvalidAttribute);
        }
    }
    // LLVM takes the accepted declaration, and refuses the text of each
    // refused one.
    for llvm in [22, 19].into_iter().filter_map(Llvm::new) {
        let text = printed(&m, llvm.dialect(), &[]);
        llvm.accept(&text, "the accepted declaration");
        let line = "declare void @accepted(ptr, ptr sret(i32), ptr)";
        assert!(text.contains(line), "{text}");
        for refused in [
            "declare void @accepted(ptr, ptr, ptr sret(i32))",
            "declare void @accepted(ptr signext, ptr, ptr)",
            "declare void @accepted(ptr elementtype(i32), ptr, ptr)",
        ] {
            let edited = text.replace(line, refused);
            assert!(llvm.objection(&edited).is_some(), "LLVM {}: {refused}", llvm.major);
        }
    }
}

/// `sanitize_realtime` with `sanitize_realtime_blocking`: `FnAttrs::set`
/// refuses the pair, and LLVM refuses the text with both.
#[test]
fn realtime_function_attributes() {
    let mut attrs = hod_ir::FnAttrs::new();
    attrs.set(hod_ir::FnAttrs::SANITIZE_REALTIME).unwrap();
    let error = attrs.set(hod_ir::FnAttrs::SANITIZE_REALTIME_BLOCKING).unwrap_err();
    assert_eq!(error.kind, IrErrorKind::InvalidAttribute);
    let mut m = module();
    let id = m.intern_fn_attrs(&attrs).unwrap();
    let ty = m.types.function(TypeId::VOID, &[], false).unwrap();
    m.add_function(b"h", &FunctionDesc { attrs: id, ..FunctionDesc::new(ty) }).unwrap();
    for llvm in [22].into_iter().filter_map(Llvm::new) {
        let text = printed(&m, llvm.dialect(), &[]);
        llvm.accept(&text, "sanitize_realtime alone");
        let both =
            text.replace("sanitize_realtime", "sanitize_realtime sanitize_realtime_blocking");
        assert!(llvm.objection(&both).is_some(), "{both}");
    }
}
