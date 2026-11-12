//! `spec/ir.md` §5.8: `Body::verify` and the module's checks refuse what
//! LLVM's verifier refuses. Each case builds the offending body or module
//! through `hod_ir` and its nearest valid variant, checks hod's verdict,
//! prints both for LLVM 22 and LLVM 19, and requires `llvm-as` (with its
//! verifier) to agree: refused when hod refuses, accepted when hod
//! accepts. A crash of LLVM's verifier counts as a refusal (§5.8 records
//! the one case where LLVM 22 crashes).
#![expect(clippy::many_single_char_names, reason = "short names for the values of a body")]

mod print_support;

use hod_ir::{
    Body, CallDesc, Clause, FunctionDesc, InlineAsm, IntPredicate, IrErrorKind, Module, Opcode,
    SymId, TargetSpec, TypeId, Value,
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
