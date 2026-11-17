//! Vectors in a function without SSE (`spec/codegen-x86.md` section V,
//! WP-338). **Owner: WP-338.** Tests: `crates/hod/tests/i386_vecscalar.rs`.
//!
//! `llc` scalarizes every vector there. hod keeps §17's scheme: a vector
//! is of class `Mem` with its memory image in its home, and an operation
//! goes lane by lane. §17's integer forms use no `xmm` register and no
//! `cmov` and stay in `vector.rs`; [`lower`] takes the forms that would
//! (floating lanes on the x87, inserts of lanes wider than a register,
//! an `i1` insert at a variable index) and refuses what is not lowered.
//!
//! Floating lanes: `fld` from the operand, operate, `fstp` to the result
//! at the lane's width, so a lane rounds at its store. `llc` keeps a
//! computed lane at 80 bits for its uses in the block (section V): such
//! a use is refused ([`EXCESS`]), never computed from the rounded lane.
//! Nothing here branches or calls: no construct is an x87 barrier.
//!
//! Registers written: `eax`, `ecx`, `edx`, the x87 stack, the flags.

use hod_cg::{CgResult, FnCx};
use hod_ir::{
    FloatPredicate, Inst, InstKind, Intrinsic, Opcode, TypeKind, Value, ValueKind, X86Features,
};
use hod_mc_x64 as mc;
use hod_mc_x64::{Cond, Mem, Size, St, X87Size};

use crate::common::{EAX, ECX, EDX, frame_inst, gp, put, unsupported, value_ty};
use crate::target::X86;
use crate::vector::{E, Shape};

/// A floating lane that `llc` would still hold at 80 bits where it is
/// read (section V "Excess precision").
pub const EXCESS: &str = "x86:vector:no-sse2:excess-precision";
/// A floating operation on constant lanes, which `llc` folds.
pub const FOLD: &str = "x86:vector:no-sse2:fold";
/// A cast from or to floating lanes.
pub const CAST: &str = "x86:vector:no-sse2:cast";
/// A vector `select` without `cmov` (`llc` branches per lane).
pub const SELECT: &str = "x86:vector:no-sse2:select";
/// A `min`/`max` reduction without `cmov` (`llc` branches).
pub const REDUCE: &str = "x86:vector:no-sse2:reduce";
/// An `<N x i1>` or `inreg` vector at the boundary (`abi.rs`).
pub const ABI: &str = "x86:vector:no-sse2:abi";

/// How deep [`wide`] and [`constant`] look through operands.
const DEPTH: u32 = 8;

/// Lowers `inst` when it is one of this module's forms; `None` leaves it
/// to `vector.rs`, whose code for it uses neither `xmm` nor `cmov`.
pub fn lower<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>) -> Option<CgResult<()>> {
    let cmov = cx.target.features().has(X86Features::CMOV);
    let fp_vec = |cx: &FnCx<'_, X86>, v: Value| fp_lanes(cx, v).is_some();
    if returned_wide(cx, inst) {
        return Some(unsupported(EXCESS));
    }
    match inst.kind() {
        InstKind::Binary { lhs, rhs } if fp_vec(cx, lhs) => Some(binary(cx, inst, lhs, rhs)),
        InstKind::FCmp { pred, lhs, rhs } => Some(fcmp(cx, inst, pred, lhs, rhs)),
        InstKind::Cast { arg }
            if !matches!(inst.opcode(), Opcode::BitCast | Opcode::AddrSpaceCast)
                && (fp_vec(cx, arg) || is_fp_vec_ty(cx, inst)) =>
        {
            Some(unsupported(CAST))
        }
        InstKind::ExtractElement { vector, .. } if fp_vec(cx, vector) => {
            if wide(cx, vector, inst, DEPTH) { Some(unsupported(EXCESS)) } else { None }
        }
        InstKind::InsertElement { vector, value, index } => {
            let gpr = crate::abi::is_gpr(cx.types(), value_ty(cx, value));
            let variable = !matches!(index.kind(), ValueKind::Const(_));
            let mask = matches!(cx.types().kind(value_ty(cx, value)), TypeKind::Int(1));
            if gpr && !(mask && variable) {
                return None;
            }
            Some(insert(cx, inst, vector, value, index))
        }
        InstKind::Select { .. } if !cmov => Some(unsupported(SELECT)),
        InstKind::Call(site) if !cmov => match site.intrinsic() {
            Some(
                Intrinsic::VectorReduceSmax
                | Intrinsic::VectorReduceSmin
                | Intrinsic::VectorReduceUmax
                | Intrinsic::VectorReduceUmin,
            ) => Some(unsupported(REDUCE)),
            _ => None,
        },
        _ => None,
    }
}

/// The x87 width of the lanes of `v`'s vector type, if they are `float`
/// or `double`.
fn fp_lanes(cx: &FnCx<'_, X86>, v: Value) -> Option<X87Size> {
    fp_of_ty(cx, value_ty(cx, v))
}

fn fp_of_ty(cx: &FnCx<'_, X86>, ty: hod_ir::TypeId) -> Option<X87Size> {
    let types = cx.types();
    let TypeKind::Vector { elem, .. } = types.kind(ty) else { return None };
    match types.kind(elem) {
        TypeKind::Float => Some(X87Size::S),
        TypeKind::Double => Some(X87Size::D),
        _ => None,
    }
}

fn is_fp_vec_ty(cx: &FnCx<'_, X86>, inst: Inst<'_>) -> bool {
    fp_of_ty(cx, inst.ty()).is_some()
}

/// Whether `ty` is `float`, `double`, or a vector of them.
fn is_fp_ty(cx: &FnCx<'_, X86>, ty: hod_ir::TypeId) -> bool {
    matches!(cx.types().kind(ty), TypeKind::Float | TypeKind::Double) || fp_of_ty(cx, ty).is_some()
}

/// Whether instruction `def` comes before `user` in the same block.
fn same_block(cx: &FnCx<'_, X86>, def: hod_ir::InstId, user: Inst<'_>) -> bool {
    def < user.id()
        && (def.raw()..user.id().raw())
            .all(|raw| !cx.body.inst(hod_ir::InstId::new(raw)).is_terminator())
}

/// Whether `llc` may hold floating value `v` (a scalar or a vector) at 80
/// bits where `user` reads it: `v` is defined earlier in `user`'s block
/// by an instruction that computes a floating value, or moves lanes of
/// such a value. Loads, `phi`s, arguments, constants and `bitcast`s hold
/// the value rounded. A call between them is not looked at (hod refuses
/// where `llc` would read the value rounded).
fn wide(cx: &FnCx<'_, X86>, v: Value, user: Inst<'_>, depth: u32) -> bool {
    let Some(id) = v.as_inst() else { return false };
    if !is_fp_ty(cx, value_ty(cx, v)) || !same_block(cx, id, user) {
        return false;
    }
    if depth == 0 {
        return true;
    }
    let def = cx.body.inst(id);
    let any = |vs: &[Value]| vs.iter().any(|&o| wide(cx, o, user, depth - 1));
    match def.kind() {
        InstKind::Load { .. } | InstKind::Phi { .. } => false,
        InstKind::Cast { .. } if def.opcode() == Opcode::BitCast => false,
        InstKind::InsertElement { vector, value, .. } => any(&[vector, value]),
        InstKind::ExtractElement { vector, .. } => any(&[vector]),
        InstKind::ShuffleVector { first, second, .. } => any(&[first, second]),
        InstKind::Select { if_true, if_false, .. } => any(&[if_true, if_false]),
        InstKind::Unary { arg } => any(&[arg]),
        _ => true,
    }
}

/// Whether `inst` makes a vector of one or two floating lanes that a
/// `ret` of its block returns while `llc` holds it at 80 bits (in `st0`).
fn returned_wide(cx: &FnCx<'_, X86>, inst: Inst<'_>) -> bool {
    let types = cx.types();
    let small = matches!(types.kind(inst.ty()), TypeKind::Vector { len: 1 | 2, .. });
    if !small || fp_of_ty(cx, inst.ty()).is_none() {
        return false;
    }
    let mut raw = inst.id().raw() + 1;
    let term = loop {
        if raw >= cx.body.inst_count() as u32 {
            return false;
        }
        let q = cx.body.inst(hod_ir::InstId::new(raw));
        if q.is_terminator() {
            break q;
        }
        raw += 1;
    };
    let v = inst.result();
    matches!(term.kind(), InstKind::Ret { value: Some(r) } if r == v) && wide(cx, v, term, DEPTH)
}

/// Whether `v` is a constant or computed from constants only in `user`'s
/// block (`llc`'s DAG folds such an operation).
fn constant(cx: &FnCx<'_, X86>, v: Value, user: Inst<'_>, depth: u32) -> bool {
    match v.kind() {
        ValueKind::Const(_) => true,
        _ => v.as_inst().is_some_and(|id| {
            let def = cx.body.inst(id);
            depth > 0
                && same_block(cx, id, user)
                && !matches!(def.kind(), InstKind::Load { .. } | InstKind::Phi { .. })
                && !matches!(def.kind(), InstKind::Call(_))
                && def.operands().iter().all(|&o| constant(cx, o, user, depth - 1))
        }),
    }
}

/// `fadd`, `fsub`, `fmul`, `fdiv` lane by lane on the x87 (as §11's
/// scalar: `fld a`, `fld b`, `f<op>p st1`, `fstp`).
fn binary<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, lhs: Value, rhs: Value) -> CgResult<()> {
    let s = crate::vector::shape(cx, inst.ty())?;
    let Some(fp) = fp_of_ty(cx, inst.ty()) else {
        return cx.bug("a floating vector operation without floating lanes");
    };
    let op = match inst.opcode() {
        Opcode::FAdd => mc::faddp(St::St1),
        Opcode::FSub => mc::fsubp(St::St1),
        Opcode::FMul => mc::fmulp(St::St1),
        Opcode::FDiv => mc::fdivp(St::St1),
        Opcode::FRem => return unsupported(crate::vector::FREM),
        _ => return cx.bug("an integer opcode on floating-point lanes"),
    };
    if constant(cx, lhs, inst, DEPTH) && constant(cx, rhs, inst, DEPTH) {
        return unsupported(FOLD);
    }
    if wide(cx, lhs, inst, DEPTH) || wide(cx, rhs, inst, DEPTH) {
        return unsupported(EXCESS);
    }
    let a = cx.use_mem(lhs)?;
    let b = cx.use_mem(rhs)?;
    let d = cx.def_mem(inst)?;
    for lane in 0..s.n {
        let at = s.at(lane);
        frame_inst(cx, a.offset_by(at), |m| mc::fld_m(fp, m));
        frame_inst(cx, b.offset_by(at), |m| mc::fld_m(fp, m));
        put(cx, op);
        frame_inst(cx, d.offset_by(at), |m| mc::fstp_m(fp, m));
    }
    Ok(())
}

/// How a predicate reads the flags of `st0 = x` compared with `st1 = y`
/// (§11's table): whether the operands swap, and the condition(s).
#[derive(Clone, Copy)]
enum FpCond {
    One(Cond),
    All(Cond, Cond),
    Any(Cond, Cond),
}

const fn conditions(pred: FloatPredicate) -> (bool, FpCond) {
    use FloatPredicate as P;
    match pred {
        P::Oeq => (false, FpCond::All(Cond::E, Cond::Np)),
        P::Ogt => (false, FpCond::One(Cond::A)),
        P::Oge => (false, FpCond::One(Cond::Ae)),
        P::Olt => (true, FpCond::One(Cond::A)),
        P::Ole => (true, FpCond::One(Cond::Ae)),
        P::One | P::False => (false, FpCond::One(Cond::Ne)),
        P::Ord => (false, FpCond::One(Cond::Np)),
        P::Ueq | P::True => (false, FpCond::One(Cond::E)),
        P::Ugt => (true, FpCond::One(Cond::B)),
        P::Uge => (true, FpCond::One(Cond::Be)),
        P::Ult => (false, FpCond::One(Cond::B)),
        P::Ule => (false, FpCond::One(Cond::Be)),
        P::Une => (false, FpCond::Any(Cond::Ne, Cond::P)),
        P::Uno => (false, FpCond::One(Cond::P)),
    }
}

/// `fcmp` lane by lane on the x87 into an `i1` vector built in a slot of
/// its own (§11's compare: `fucomip` with `cmov`, else `fucompp`,
/// `fnstsw ax`, `sahf`).
fn fcmp<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    pred: FloatPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    let s = crate::vector::shape(cx, value_ty(cx, lhs))?;
    let r = crate::vector::shape(cx, inst.ty())?;
    let Some(fp) = fp_lanes(cx, lhs) else {
        return cx.bug("an fcmp of vectors that are no floating-point vectors");
    };
    let trivial = matches!(pred, FloatPredicate::True | FloatPredicate::False);
    if !trivial {
        if constant(cx, lhs, inst, DEPTH) && constant(cx, rhs, inst, DEPTH) {
            return unsupported(FOLD);
        }
        if wide(cx, lhs, inst, DEPTH) || wide(cx, rhs, inst, DEPTH) {
            return unsupported(EXCESS);
        }
    }
    let cmov = cx.target.features().has(X86Features::CMOV);
    let a = cx.use_mem(lhs)?;
    let b = cx.use_mem(rhs)?;
    let t = crate::vector::begin(cx, r)?;
    let (swap, cond) = conditions(pred);
    let (x, y) = if swap { (b, a) } else { (a, b) };
    for lane in 0..s.n {
        let at = s.at(lane);
        if trivial {
            let one = i64::from(pred == FloatPredicate::True);
            put(cx, mc::mov_ri(Size::D, EAX, one));
        } else {
            if lhs == rhs {
                frame_inst(cx, x.offset_by(at), |m| mc::fld_m(fp, m));
                put(cx, mc::fld_st(St::St0));
            } else {
                frame_inst(cx, y.offset_by(at), |m| mc::fld_m(fp, m));
                frame_inst(cx, x.offset_by(at), |m| mc::fld_m(fp, m));
            }
            if cmov {
                put(cx, mc::fucomip(St::St1));
                put(cx, mc::fstp_st(St::St0));
            } else {
                put(cx, mc::fucompp());
                put(cx, mc::fnstsw_ax());
                put(cx, mc::sahf());
            }
            match cond {
                FpCond::One(c) => put(cx, mc::setcc_r(c, EAX)),
                FpCond::All(c1, c2) | FpCond::Any(c1, c2) => {
                    put(cx, mc::setcc_r(c1, EAX));
                    put(cx, mc::setcc_r(c2, ECX));
                    let all = matches!(cond, FpCond::All(..));
                    put(
                        cx,
                        if all {
                            mc::and_rr(Size::B, EAX, ECX)
                        } else {
                            mc::or_rr(Size::B, EAX, ECX)
                        },
                    );
                }
            }
            put(cx, mc::movzx_rr(Size::D, EAX, Size::B, EAX));
        }
        crate::vector::lane_out(cx, r, t, lane);
    }
    crate::vector::end(cx, inst, t, r)
}

/// `insertelement` of a lane in memory (`i64`, `float`, `double`: copied
/// through `eax`) or of an `i1` lane at a variable index (the byte and
/// bit through `eax`, `ecx`, `edx`; the value read again from its
/// register, which is a pool register).
fn insert<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    vector: Value,
    value: Value,
    index: Value,
) -> CgResult<()> {
    let s: Shape = crate::vector::shape(cx, inst.ty())?;
    let img = cx.use_mem(vector)?;
    let gpr = crate::abi::is_gpr(cx.types(), value_ty(cx, value));
    let (vreg, vmem) =
        if gpr { (Some(cx.use_reg(value)?), None) } else { (None, Some(cx.use_mem(value)?)) };
    let asked = crate::vector::ask_index(cx, index, s.n)?;
    let d = cx.def_mem(inst)?;
    crate::vector::copy(cx, d, img, s.alloc);
    match (asked, vmem, vreg) {
        (Ok(lane), Some(m), _) => crate::vector::copy(cx, d.offset_by(s.at(lane)), m, s.lane()),
        (Err(asked), Some(m), _) => {
            crate::vector::place_index(cx, asked, s.n, d);
            for k in (0..s.lane()).step_by(4) {
                crate::vector::ld(cx, EAX, 4, false, m.offset_by(k));
                put(cx, mc::mov_mr(Size::D, crate::vector::var_mem(s, k as i32), EAX));
            }
        }
        (Err(asked), None, Some(r)) if s.e == E::Mask => {
            crate::vector::place_index(cx, asked, s.n, d);
            // edx = &byte, ecx = bit; the bit cleared, then the value's.
            put(cx, mc::mov_rr(Size::D, EAX, ECX));
            put(cx, mc::shr_ri(Size::D, EAX, 3));
            put(cx, mc::add_rr(Size::D, EDX, EAX));
            put(cx, mc::and_ri(Size::D, ECX, 7));
            put(cx, mc::mov_ri(Size::D, EAX, 1));
            put(cx, mc::shl_rcl(Size::D, EAX));
            put(cx, mc::not_r(Size::D, EAX));
            put(cx, mc::and_mr(Size::B, Mem::base(EDX, 0), EAX));
            put(cx, mc::mov_rr(Size::D, EAX, gp(r)));
            put(cx, mc::and_ri(Size::D, EAX, 1));
            put(cx, mc::shl_rcl(Size::D, EAX));
            put(cx, mc::or_mr(Size::B, Mem::base(EDX, 0), EAX));
        }
        _ => return cx.bug("an insertelement vecscalar does not take"),
    }
    Ok(())
}

/// `ret` of a vector returned in memory (section V): its store size
/// copied to the hidden result pointer (incoming slot 0) through `eax`,
/// the pointer left in `eax` (`spec/abi-x86.md` §5.2).
///
/// # Errors
/// The vector's refusal; what asking for the value reports.
pub fn ret_memory(cx: &mut FnCx<'_, X86>, value: Value) -> CgResult<()> {
    let s = crate::vector::shape(cx, value_ty(cx, value))?;
    let v = cx.use_mem(value)?;
    crate::common::load_gpr(cx, ECX, hod_cg::FrameRef::Incoming(0), 4);
    let mut at = 0;
    while at < s.size {
        let w = match s.size - at {
            4.. => 4,
            2 | 3 => 2,
            _ => 1,
        };
        crate::vector::ld(cx, EAX, w, false, v.offset_by(at));
        let size = match w {
            4 => Size::D,
            2 => Size::W,
            _ => Size::B,
        };
        put(cx, mc::mov_mr(size, Mem::base(ECX, at as i32), EAX));
        at += w;
    }
    put(cx, mc::mov_rr(Size::D, EAX, ECX));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_narrower_than_no_sse2() {
        for name in [EXCESS, FOLD, CAST, SELECT, REDUCE, ABI] {
            assert!(name.starts_with(crate::vector::NO_SSE2), "{name}");
        }
    }

    #[test]
    fn swapped_predicates() {
        assert!(conditions(FloatPredicate::Olt).0);
        assert!(!conditions(FloatPredicate::Oeq).0);
    }
}
