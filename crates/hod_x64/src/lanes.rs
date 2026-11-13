//! Element-wise operations on vectors (`spec/codegen.md` §5.6.13,
//! "Vectors (WP-132)"): SSE2's packed instruction where one computes the
//! operation on a 128- or 64-bit vector, else one lane at a time over the
//! scalar kernels (`hod_cg::lanes`, §7.3).
//!
//! **Owner: WP-132.** Tests: `tests/lanes.rs`. The groups hook in where
//! they refused a vector: `float::binary`, `float::fneg`, `float::cast`
//! (class `Fpr`), the top of `intrinsic::intrinsic` and of
//! `fpintr::intrinsic` ([`intrinsic`]); the dispatch sends `Mem`-class
//! vectors here directly.
//!
//! # The loop
//!
//! `X64` implements `LaneKernels` below: a lane is a frame place (`rbp`
//! relative), loaded zero-extended into a general register or as the
//! scalar view of an `xmm` (a 2-byte lane through `r11`). Each operation
//! hands `each_lane` a kernel that computes one lane as the scalar
//! instruction does (the scalar groups' register helpers where they have
//! one: `intrinsic::unary_reg`, `two_reg`, `funnel_reg`,
//! `fpintr::min_max_reg`, `saturate_reg`, `float::fp_to_int`,
//! `int_to_fp`, `negate`, the `half`/`bfloat` conversions), so that a
//! lane is what the scalar instruction gives, poison and undefined
//! behaviour included (a lane of `udiv` by zero traps, as the scalar
//! one).
//!
//! # Packed forms
//!
//! On an `Fpr` vector: `padd*`/`psub*` (8- to 64-bit lanes), `pand`,
//! `por`, `pxor`, `pmullw` (16-bit lanes), `addps` .. `divpd` (`float`
//! and `double` lanes), `fneg` as `xorps` with the sign of each lane.
//!
//! # Refusals
//!
//! The shapes `vector::lanes` refuses keep its names; `bfloat` lanes of
//! the floating-point intrinsics are `fpintr::BFLOAT` (as the scalar);
//! `frexp`, `is.fpclass`, `sshl.sat`/`ushl.sat` of vectors stay
//! `intrinsic.rs`'s refusals; a reduction whose operands are not the
//! intrinsic's is [`REDUCE`].
//!
//! # Registers
//!
//! `rax`, `rcx`, `rdx`, `r10`, `r11`, `xmm14`, `xmm15` and what the
//! kernels' calls write; at most four pool temporaries of a class at once.

#![allow(clippy::many_single_char_names, reason = "the operands are LangRef's a, b, c")]

use hod_cg::lanes::{self as shared, Geometry, LaneKernels};
use hod_cg::sym::{FpOp, FpType, fp_ext_routine, fp_routine};
use hod_cg::{CallArg, CgResult, Class, FnCx, FrameRef, PReg, RuntimeFn};
use hod_ir::{Arch, Const, Inst, Intrinsic, Opcode, TypeId, Value, ValueKind};
use hod_mc_x64 as mc;
use hod_mc_x64::{Cond, FpSize, Reg, Size, Xmm};

use crate::common::{
    R11, XMM15, copy_frame, deferred_first, frame_inst, gp, load_fpr, mov_imm, put, store_fpr,
    unsupported, value_ty, xm,
};
use crate::float::{
    Shape as FShape, bfloat_narrow, bfloat_widen, double_to_float_odd, fp_to_int, half_out,
    int_to_fp, narrow_call, negate, runtime, xmm_imm,
};
use crate::fpintr::{min_max_reg, saturate_reg};
use crate::int::{Ext, extend_to, size_for_bits};
use crate::intrinsic::{funnel_reg, two_reg, unary_reg};
use crate::reg;
use crate::target::X64;
use crate::vector::{self, At, Elem, Lanes, SCRATCH_IMAGES};

/// The refusal kept by this module's callers for the names they owned.
pub const LANES: &str = vector::MASK_WIDE;
/// A `llvm.vector.reduce.*` call whose operands are not the intrinsic's
/// (a floating-point fold of integers, an integer fold of floats).
pub const REDUCE: &str = "x64:vector-reduce";

const RAX: Reg = Reg::Rax;
const RCX: Reg = Reg::Rcx;
const RDX: Reg = Reg::Rdx;

impl LaneKernels for X64 {
    type Lanes = Lanes;
    const SCRATCH_IMAGES: u32 = SCRATCH_IMAGES;

    fn geometry(lanes: &Lanes) -> Geometry {
        lanes.geometry()
    }

    fn deferred_first<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>) -> CgResult<()> {
        deferred_first(cx, inst, None)
    }

    fn scratch(cx: &mut FnCx<'_, X64>, k: u32) -> CgResult<FrameRef> {
        vector::scratch(cx, k)
    }

    fn spill(cx: &mut FnCx<'_, X64>, reg: PReg, lanes: &Lanes, k: u32) -> CgResult<FrameRef> {
        vector::spill(cx, xm(reg), lanes, k)
    }

    fn reload(cx: &mut FnCx<'_, X64>, reg: PReg, lanes: &Lanes, image: FrameRef) {
        vector::reload(cx, xm(reg), lanes, image);
    }

    fn load_lane(cx: &mut FnCx<'_, X64>, lanes: &Lanes, at: FrameRef, rt: PReg) {
        vector::load_lane(cx, lanes, At::Frame(at), rt);
    }

    fn store_lane(cx: &mut FnCx<'_, X64>, lanes: &Lanes, at: FrameRef, rt: PReg) {
        vector::store_lane(cx, lanes, At::Frame(at), rt);
    }
}

/// An operand of a lane loop.
type Arg = shared::Arg<Lanes>;
/// The operands and the result of a lane loop.
type Loop = shared::LaneLoop<Lanes>;

/// The lanes of vector type `ty`, or the refusal.
fn lanes_of(cx: &FnCx<'_, X64>, ty: TypeId) -> CgResult<Lanes> {
    vector::lanes(cx, ty)
}

/// Checks the shapes of `inst` and its `operands` (at most three), then
/// finds the operands' images and the result's.
fn setup<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, operands: &[Value]) -> CgResult<Loop> {
    shared::setup(cx, inst, operands, |cx, ty| lanes_of(cx, ty))
}

/// The end of a lane loop: an `Fpr` result's image loaded into its
/// register.
fn done<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, lp: &Loop) -> CgResult<()> {
    shared::finish(cx, inst, &lp.result, lp.dst)
}

/// The scalar shape of a floating-point element.
fn fshape(elem: Elem) -> FShape {
    match elem {
        Elem::Half => FShape::Half,
        Elem::BFloat => FShape::BFloat,
        Elem::Double => FShape::Double,
        _ => FShape::Float,
    }
}

/// The routine `func` of `op` for `float` or `double`.
fn routine(cx: &FnCx<'_, X64>, op: FpOp, size: FpSize) -> CgResult<RuntimeFn> {
    let ty = if size == FpSize::D { FpType::Double } else { FpType::Float };
    match fp_routine(op, ty, Arch::X86_64) {
        Some(func) => Ok(func),
        None => cx.bug("a floating-point lane operation without a routine"),
    }
}

/// `__extendhfsf2`.
fn half_ext(cx: &FnCx<'_, X64>) -> CgResult<RuntimeFn> {
    match fp_ext_routine(FpType::Half, FpType::Float, Arch::X86_64) {
        Some(func) => Ok(func),
        None => cx.bug("no half extension routine on x86-64"),
    }
}

// ---- packed forms ----

/// The packed instruction of `op` on an `Fpr` vector of `lanes`, if SSE2
/// has one.
fn packed(op: Opcode, lanes: &Lanes, x: Xmm, y: Xmm) -> Option<mc::Inst> {
    use Opcode as O;
    let elem = lanes.shape.elem;
    if elem.is_float() {
        let size = match elem {
            Elem::Float => FpSize::S,
            Elem::Double => FpSize::D,
            _ => return None,
        };
        return Some(match op {
            O::FAdd => mc::addp_rr(size, x, y),
            O::FSub => mc::subp_rr(size, x, y),
            O::FMul => mc::mulp_rr(size, x, y),
            O::FDiv => mc::divp_rr(size, x, y),
            _ => return None,
        });
    }
    Some(match (op, lanes.log2) {
        (O::And, _) => mc::pand_rr(x, y),
        (O::Or, _) => mc::por_rr(x, y),
        (O::Xor, _) => mc::pxor_rr(x, y),
        (O::Add, 0) => mc::paddb_rr(x, y),
        (O::Add, 1) => mc::paddw_rr(x, y),
        (O::Add, 2) => mc::paddd_rr(x, y),
        (O::Add, _) => mc::paddq_rr(x, y),
        (O::Sub, 0) => mc::psubb_rr(x, y),
        (O::Sub, 1) => mc::psubw_rr(x, y),
        (O::Sub, 2) => mc::psubd_rr(x, y),
        (O::Sub, _) => mc::psubq_rr(x, y),
        (O::Mul, 1) => mc::pmullw_rr(x, y),
        _ => return None,
    })
}

/// The shift amount every lane of the constant `value` holds, if it is
/// one below the lane width (a larger one makes the lane poison: the
/// lane loop's business).
fn splat_amount(cx: &FnCx<'_, X64>, value: Value, lanes: &Lanes) -> Option<u8> {
    let ValueKind::Const(id) = value.kind() else { return None };
    let int = |id| match cx.module.consts.get(id) {
        Const::Int { value, .. } => Some(value),
        Const::Zero(_) => Some(0),
        _ => None,
    };
    let amount = match cx.module.consts.get(id) {
        Const::Zero(_) => 0,
        Const::Splat { elem, .. } => int(elem)?,
        Const::Aggregate { elems, .. } => {
            let first = int(*elems.first()?)?;
            elems.iter().all(|&e| int(e) == Some(first)).then_some(first)?
        }
        _ => return None,
    };
    u8::try_from(amount).ok().filter(|&a| u32::from(a) < lanes.shape.elem.bits())
}

/// The packed shift of every lane of `x` by `amount`, if SSE2 has one
/// (no byte lanes, no arithmetic shift of quadwords).
fn shift_packed(op: Opcode, lanes: &Lanes, x: Xmm, amount: u8) -> Option<mc::Inst> {
    use Opcode as O;
    if lanes.shape.elem.is_float() {
        return None;
    }
    Some(match (op, lanes.log2) {
        (O::Shl, 1) => mc::psllw_ri(x, amount),
        (O::Shl, 2) => mc::pslld_ri(x, amount),
        (O::Shl, 3) => mc::psllq_ri(x, amount),
        (O::LShr, 1) => mc::psrlw_ri(x, amount),
        (O::LShr, 2) => mc::psrld_ri(x, amount),
        (O::LShr, 3) => mc::psrlq_ri(x, amount),
        (O::AShr, 1) => mc::psraw_ri(x, amount),
        (O::AShr, 2) => mc::psrad_ri(x, amount),
        _ => return None,
    })
}

/// [`binary`] by one packed instruction: the left operand's register
/// (the right one's too for `%a op %a`).
fn binary_packed<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    lhs: Value,
    rhs: Value,
) -> Option<CgResult<()>> {
    let lanes = vector::lanes(cx, inst.ty()).ok()?;
    if lanes.shape.class != Class::Fpr {
        return None;
    }
    if let Some(amount) = splat_amount(cx, rhs, &lanes) {
        shift_packed(inst.opcode(), &lanes, Xmm::Xmm0, amount)?;
        return Some((|| {
            deferred_first(cx, inst, None)?;
            let a = cx.use_reg(lhs)?;
            if let Some(word) = shift_packed(inst.opcode(), &lanes, xm(a), amount) {
                put(cx, word);
            }
            cx.finish(inst, a)
        })());
    }
    packed(inst.opcode(), &lanes, Xmm::Xmm0, Xmm::Xmm0)?;
    Some((|| {
        deferred_first(cx, inst, None)?;
        let a = cx.use_reg(lhs)?;
        let b = if rhs == lhs { a } else { cx.use_reg(rhs)? };
        if let Some(word) = packed(inst.opcode(), &lanes, xm(a), xm(b)) {
            put(cx, word);
        }
        cx.finish(inst, a)
    })())
}

// ---- kernels ----

/// `a = a <op> b` on one integer lane of `bits` bits (both loaded
/// zero-extended): the scalar instruction at 32 or 64 bits, a shift
/// through `cl`, a division through `rax`/`rdx`. Writes `rax`, `rcx`,
/// `rdx` and the flags.
fn int_lane(cx: &mut FnCx<'_, X64>, op: Opcode, bits: u32, a: Reg, b: Reg) -> CgResult<()> {
    use Opcode as O;
    let size = size_for_bits(bits);
    let inst = match op {
        O::Add => mc::add_rr(size, a, b),
        O::Sub => mc::sub_rr(size, a, b),
        O::Mul => mc::imul_rr(size, a, b),
        O::And => mc::and_rr(size, a, b),
        O::Or => mc::or_rr(size, a, b),
        O::Xor => mc::xor_rr(size, a, b),
        O::Shl | O::LShr | O::AShr => {
            if op == O::AShr {
                extend_to(cx, a, bits, Ext::Sign, size);
            }
            put(cx, mc::mov_rr(Size::D, RCX, b));
            match op {
                O::Shl => mc::shl_rcl(size, a),
                O::LShr => mc::shr_rcl(size, a),
                _ => mc::sar_rcl(size, a),
            }
        }
        O::UDiv | O::URem | O::SDiv | O::SRem => {
            let signed = matches!(op, O::SDiv | O::SRem);
            if signed {
                extend_to(cx, a, bits, Ext::Sign, size);
                extend_to(cx, b, bits, Ext::Sign, size);
            }
            put(cx, mc::mov_rr(size, RAX, a));
            if signed {
                put(cx, if size == Size::Q { mc::cqo() } else { mc::cdq() });
                put(cx, mc::idiv_r(size, b));
            } else {
                put(cx, mc::mov_ri(Size::D, RDX, 0));
                put(cx, mc::div_r(size, b));
            }
            let from = if matches!(op, O::UDiv | O::SDiv) { RAX } else { RDX };
            mc::mov_rr(Size::Q, a, from)
        }
        _ => return cx.bug("a binary opcode that does not fit integer lanes"),
    };
    put(cx, inst);
    Ok(())
}

/// `x = x <op> y` in `size`, for `fadd` .. `fdiv`.
fn arith(cx: &mut FnCx<'_, X64>, op: Opcode, size: FpSize, x: Xmm, y: Xmm) -> CgResult<()> {
    let inst = match op {
        Opcode::FAdd => mc::adds_rr(size, x, y),
        Opcode::FSub => mc::subs_rr(size, x, y),
        Opcode::FMul => mc::muls_rr(size, x, y),
        Opcode::FDiv => mc::divs_rr(size, x, y),
        _ => return cx.bug("a floating-point lane opcode that is none"),
    };
    put(cx, inst);
    Ok(())
}

/// Three 4-byte frame slots for `half` operands widened across calls.
type HalfSlots = [FrameRef; 3];

/// Slots for [`widen_halves`].
fn half_slots(cx: &mut FnCx<'_, X64>) -> CgResult<HalfSlots> {
    Ok([cx.alloc_slot(4, 4)?, cx.alloc_slot(4, 4)?, cx.alloc_slot(4, 4)?])
}

/// The `half` lanes in `regs` (pool temporaries of either class)
/// widened to `float` by `__extendhfsf2`, each kept in its slot across
/// the calls (all are stored before the first call, each call takes its
/// operand from its slot), then the first two loaded into new
/// temporaries; a third stays in `slots[2]` (the minimal pool has two
/// vector registers).
fn widen_halves(cx: &mut FnCx<'_, X64>, regs: &[PReg], slots: &HalfSlots) -> CgResult<[PReg; 2]> {
    let ext = half_ext(cx)?;
    for (&r, &slot) in regs.iter().zip(slots) {
        if reg::is_fpr(r) {
            store_fpr(cx, xm(r), slot, 4);
        } else {
            frame_inst(cx, slot, |m| mc::mov_mr(Size::D, m, gp(r)));
        }
    }
    for &slot in &slots[..regs.len()] {
        let w = runtime(cx, ext, &[CallArg::Mem(slot)])?;
        store_fpr(cx, xm(w), slot, 4);
        cx.free_temp(w);
    }
    let mut out = [PReg::new(0); 2];
    for (o, &slot) in out.iter_mut().zip(&slots[..regs.len().min(2)]) {
        let t = cx.temp(Class::Fpr)?;
        load_fpr(cx, xm(t), slot, 4);
        *o = t;
    }
    Ok(out)
}

/// One lane of `fadd` .. `frem` of `elem` on `x`, `y`: the result's
/// register.
fn fp_binary_lane(
    cx: &mut FnCx<'_, X64>,
    op: Opcode,
    elem: Elem,
    x: PReg,
    y: PReg,
    slots: Option<&HalfSlots>,
) -> CgResult<PReg> {
    match elem {
        Elem::Float | Elem::Double => {
            let size = if elem == Elem::Double { FpSize::D } else { FpSize::S };
            if op == Opcode::FRem {
                let func = routine(cx, FpOp::Rem, size)?;
                return runtime(cx, func, &[CallArg::Reg(x), CallArg::Reg(y)]);
            }
            arith(cx, op, size, xm(x), xm(y))?;
            Ok(x)
        }
        Elem::BFloat => {
            bfloat_widen(cx, xm(x));
            bfloat_widen(cx, xm(y));
            let r = if op == Opcode::FRem {
                let func = routine(cx, FpOp::Rem, FpSize::S)?;
                runtime(cx, func, &[CallArg::Reg(x), CallArg::Reg(y)])?
            } else {
                arith(cx, op, FpSize::S, xm(x), xm(y))?;
                x
            };
            bfloat_narrow(cx, xm(r));
            Ok(r)
        }
        _ => {
            let Some(slots) = slots else { return cx.bug("half lanes without their slots") };
            let [a, b] = widen_halves(cx, &[x, y], slots)?;
            let r = if op == Opcode::FRem {
                let func = routine(cx, FpOp::Rem, FpSize::S)?;
                runtime(cx, func, &[CallArg::Reg(a), CallArg::Reg(b)])?
            } else {
                arith(cx, op, FpSize::S, xm(a), xm(b))?;
                cx.free_temp(b);
                a
            };
            half_out(cx, r)
        }
    }
}

// ---- instructions ----

/// `%r = <op> %lhs, %rhs` element-wise on vectors: a packed instruction
/// where SSE2 has one, else the lane loop over [`int_lane`] or
/// [`fp_binary_lane`].
///
/// # Errors
/// What `vector::lanes` refuses; what the operands and the calls report.
pub fn binary<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, lhs: Value, rhs: Value) -> CgResult<()> {
    use Opcode as O;
    if let Some(done) = binary_packed(cx, inst, lhs, rhs) {
        return done;
    }
    let op = inst.opcode();
    let lp = setup(cx, inst, &[lhs, rhs])?;
    let elem = lp.result.shape.elem;
    let both = |class| [(0, class), (1, class)];
    match op {
        O::FAdd | O::FSub | O::FMul | O::FDiv | O::FRem if elem.is_float() => {
            let slots = if elem == Elem::Half { Some(half_slots(cx)?) } else { None };
            shared::each_lane(cx, &lp, &both(Class::Fpr), |cx, _, t| {
                fp_binary_lane(cx, op, elem, t[0], t[1], slots.as_ref())
            })?;
        }
        O::Add
        | O::Sub
        | O::Mul
        | O::And
        | O::Or
        | O::Xor
        | O::Shl
        | O::LShr
        | O::AShr
        | O::UDiv
        | O::SDiv
        | O::URem
        | O::SRem
            if !elem.is_float() =>
        {
            let bits = elem.bits();
            shared::each_lane(cx, &lp, &both(Class::Gpr), |cx, _, t| {
                int_lane(cx, op, bits, gp(t[0]), gp(t[1]))?;
                Ok(t[0])
            })?;
        }
        _ => return cx.bug("a binary opcode that does not fit the vector's elements"),
    }
    done(cx, inst, &lp)
}

/// `%r = fneg %arg` of a vector: of an `Fpr` vector of `float` or
/// `double`, `xorps` with the sign bit of every lane (built in `xmm15`
/// through `r11`); otherwise each lane's sign flipped (`float::negate`).
///
/// # Errors
/// As [`binary`].
pub fn fneg<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let lanes = vector::lanes(cx, inst.ty())?;
    if !lanes.shape.elem.is_float() {
        return cx.bug("fneg of integer lanes");
    }
    if lanes.shape.class == Class::Fpr {
        deferred_first(cx, inst, None)?;
        let a = cx.use_reg(arg)?;
        let (sign, bits) = match lanes.shape.elem {
            Elem::Double => (1u64 << 63, 64),
            Elem::Float => (0x8000_0000_8000_0000, 64),
            _ => (0x8000_8000_8000_8000, 64),
        };
        xmm_imm(cx, XMM15, sign, bits);
        put(cx, mc::punpcklqdq_rr(XMM15, XMM15));
        put(cx, mc::xorp_rr(FpSize::S, xm(a), XMM15));
        return cx.finish(inst, a);
    }
    let lp = setup(cx, inst, &[arg])?;
    let shape = fshape(lp.result.shape.elem);
    shared::each_lane(cx, &lp, &[(0, Class::Fpr)], |cx, _, t| {
        negate(cx, shape, xm(t[0]));
        Ok(t[0])
    })?;
    done(cx, inst, &lp)
}

/// `%r = <cast> %arg` element-wise between vectors (not `bitcast`), lane
/// by lane: the integer casts on the general register (a lane is loaded
/// zero-extended, so `zext`, `trunc`, `ptrtoint`, `inttoptr` are the
/// store of its low bytes; `sext` extends first), the conversions with a
/// floating-point side as the scalar conversion of `float.rs`. The two
/// vectors may be of different classes.
///
/// # Errors
/// As [`binary`]; `float::CAST` for a cast that is no conversion between
/// the elements; [`vector::BFLOAT`] for 64-bit integers to `bfloat`.
pub fn cast<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    use Opcode as O;
    let op = inst.opcode();
    let from = vector::lanes(cx, value_ty(cx, arg))?;
    let to = vector::lanes(cx, inst.ty())?;
    let (fe, te) = (from.shape.elem, to.shape.elem);
    let (fb, tb) = (fe.bits(), te.bits());
    let ok = match op {
        O::Trunc | O::ZExt | O::SExt | O::PtrToInt | O::PtrToAddr | O::IntToPtr => {
            !fe.is_float() && !te.is_float()
        }
        O::FPExt | O::FPTrunc => {
            fe.is_float() && te.is_float() && fe != te && (fb != tb || op == O::FPExt)
        }
        O::FPToSI | O::FPToUI => fe.is_float() && !te.is_float(),
        O::SIToFP | O::UIToFP => !fe.is_float() && te.is_float(),
        _ => false,
    };
    if !ok || (fb == tb && matches!(op, O::FPExt | O::FPTrunc)) {
        return unsupported(crate::float::CAST);
    }
    if te == Elem::BFloat && matches!(op, O::SIToFP | O::UIToFP) && fb > 32 {
        return unsupported(vector::BFLOAT);
    }
    let lp = setup(cx, inst, &[arg])?;
    match op {
        O::Trunc | O::ZExt | O::PtrToInt | O::PtrToAddr | O::IntToPtr | O::SExt => {
            shared::each_lane(cx, &lp, &[(0, Class::Gpr)], |cx, _, t| {
                if op == O::SExt {
                    crate::mem::sign_extend(cx, gp(t[0]), fb);
                }
                Ok(t[0])
            })?;
        }
        O::FPExt | O::FPTrunc => {
            shared::each_lane(cx, &lp, &[(0, Class::Fpr)], |cx, _, t| {
                fp_convert_lane(cx, fe, te, t[0])
            })?;
        }
        O::FPToSI | O::FPToUI => {
            let signed = op == O::FPToSI;
            shared::each_lane(cx, &lp, &[(0, Class::Fpr)], |cx, _, t| {
                let (x, size) = widened(cx, fe, t[0])?;
                let rd = cx.temp(Class::Gpr)?;
                fp_to_int(cx, size, xm(x), tb, signed, gp(rd));
                Ok(rd)
            })?;
        }
        _ => {
            let signed = op == O::SIToFP;
            shared::each_lane(cx, &lp, &[(0, Class::Gpr)], |cx, _, t| {
                int_to_fp_lane(cx, te, gp(t[0]), fb, signed)
            })?;
        }
    }
    done(cx, inst, &lp)
}

/// The floating-point lane in `x` as a `float` or `double` to compute
/// with: `half` widened by a call (the result's register comes back),
/// `bfloat` widened in place.
fn widened(cx: &mut FnCx<'_, X64>, elem: Elem, x: PReg) -> CgResult<(PReg, FpSize)> {
    match elem {
        Elem::Half => {
            let ext = half_ext(cx)?;
            Ok((runtime(cx, ext, &[CallArg::Reg(x)])?, FpSize::S))
        }
        Elem::BFloat => {
            bfloat_widen(cx, xm(x));
            Ok((x, FpSize::S))
        }
        Elem::Double => Ok((x, FpSize::D)),
        _ => Ok((x, FpSize::S)),
    }
}

/// One lane of `fpext`/`fptrunc` from `from` to `to`.
fn fp_convert_lane(cx: &mut FnCx<'_, X64>, from: Elem, to: Elem, x: PReg) -> CgResult<PReg> {
    let (w, size) = widened(cx, from, x)?;
    match (size, to) {
        (FpSize::S, Elem::Double) => {
            put(cx, mc::cvtss2sd_rr(xm(w), xm(w)));
            Ok(w)
        }
        (FpSize::D, Elem::Float) => {
            put(cx, mc::cvtsd2ss_rr(xm(w), xm(w)));
            Ok(w)
        }
        (_, Elem::Half) => {
            let from = if size == FpSize::D { FShape::Double } else { FShape::Float };
            let t = if w == x {
                w
            } else {
                let t = cx.temp(Class::Fpr)?;
                put(cx, mc::movap_rr(FpSize::S, xm(t), xm(w)));
                cx.free_temp(w);
                t
            };
            narrow_call(cx, from, CallArg::Reg(t))
        }
        (_, Elem::BFloat) => {
            if size == FpSize::D {
                double_to_float_odd(cx, xm(w))?;
            }
            bfloat_narrow(cx, xm(w));
            Ok(w)
        }
        _ => Ok(w),
    }
}

/// One lane of `sitofp`/`uitofp` of the `bits`-bit integer in `r` to
/// `to`: `float.rs`'s conversion; `half` through `float` and
/// `__truncsfhf2`; `bfloat` through `float` (up to 16 bits, exact) or
/// `double` rounded to odd (up to 32).
fn int_to_fp_lane(
    cx: &mut FnCx<'_, X64>,
    to: Elem,
    r: Reg,
    bits: u32,
    signed: bool,
) -> CgResult<PReg> {
    let xd = cx.temp(Class::Fpr)?;
    match to {
        Elem::Double => int_to_fp(cx, r, bits, signed, FpSize::D, xm(xd))?,
        Elem::Float => int_to_fp(cx, r, bits, signed, FpSize::S, xm(xd))?,
        Elem::Half => {
            int_to_fp(cx, r, bits, signed, FpSize::S, xm(xd))?;
            return half_out(cx, xd);
        }
        _ if bits <= 16 => {
            int_to_fp(cx, r, bits, signed, FpSize::S, xm(xd))?;
            bfloat_narrow(cx, xm(xd));
        }
        _ => {
            int_to_fp(cx, r, bits, signed, FpSize::D, xm(xd))?;
            double_to_float_odd(cx, xm(xd))?;
            bfloat_narrow(cx, xm(xd));
        }
    }
    Ok(xd)
}

// ---- intrinsics ----

/// The families of intrinsics with an element-wise form.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Family {
    /// `ctlz`, `cttz`, `ctpop`, `bswap`, `bitreverse`, `abs`.
    Unary,
    /// `smax` .. `umin`, the saturating additions and subtractions.
    Two,
    /// `scmp`, `ucmp`.
    ThreeWay,
    /// `fshl`, `fshr`.
    Funnel,
    /// `ptrmask`.
    Ptrmask,
    /// The floating-point intrinsics.
    Fp,
    /// `llvm.vector.reduce.*`.
    Reduce,
    /// `llvm.vector.extract`, `llvm.vector.insert`.
    Part,
}

/// A call of intrinsic `which` with a vector among its result and
/// operands, element-wise; `None` for any other call and for the forms
/// left to `intrinsic.rs`/`fpintr.rs` (masks are `mask.rs`'s, which the
/// dispatch has asked first; `frexp`, `is.fpclass`, `sshl.sat`,
/// `ushl.sat`).
pub fn intrinsic<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> Option<CgResult<()>> {
    use Intrinsic as I;
    let types = cx.types();
    let vector =
        types.is_vector(inst.ty()) || args.iter().any(|&a| types.is_vector(value_ty(cx, a)));
    if !vector {
        return None;
    }
    let family = match which {
        I::Ctlz | I::Cttz | I::Ctpop | I::Bswap | I::BitReverse | I::Abs => Family::Unary,
        I::Smax
        | I::Smin
        | I::Umax
        | I::Umin
        | I::SaddSat
        | I::UaddSat
        | I::SsubSat
        | I::UsubSat => Family::Two,
        I::Scmp | I::Ucmp => Family::ThreeWay,
        I::Fshl | I::Fshr => Family::Funnel,
        I::Ptrmask => Family::Ptrmask,
        I::VectorReduceAdd
        | I::VectorReduceMul
        | I::VectorReduceAnd
        | I::VectorReduceOr
        | I::VectorReduceXor
        | I::VectorReduceSmax
        | I::VectorReduceSmin
        | I::VectorReduceUmax
        | I::VectorReduceUmin
        | I::VectorReduceFmax
        | I::VectorReduceFmin
        | I::VectorReduceFadd
        | I::VectorReduceFmul => Family::Reduce,
        I::VectorExtract | I::VectorInsert => Family::Part,
        I::Frexp | I::IsFpClass => return None,
        _ if crate::fpintr::is_fp(which) => Family::Fp,
        _ => return None,
    };
    Some(match family {
        Family::Reduce => reduce(cx, inst, which, args),
        Family::Part => part(cx, inst, which, args),
        Family::Fp => fp_lanes(cx, inst, which, args),
        _ => int_lanes(cx, inst, which, family, args),
    })
}

/// The integer families of [`intrinsic`].
fn int_lanes<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    family: Family,
    args: &'a [Value],
) -> CgResult<()> {
    let arity = match family {
        Family::Unary => 1,
        Family::Funnel => 3,
        _ => 2,
    };
    if args.len() < arity || !cx.types().is_vector(inst.ty()) {
        return unsupported(crate::intrinsic::INTRINSIC);
    }
    let lp = setup(cx, inst, &args[..arity])?;
    let elem = lp.result.shape.elem;
    if elem.is_float() {
        return unsupported(crate::intrinsic::INTRINSIC);
    }
    let bits = elem.bits();
    match family {
        Family::Unary => shared::each_lane(cx, &lp, &[(0, Class::Gpr)], |cx, _, t| {
            unary_reg(cx, which, gp(t[0]), bits)?;
            Ok(t[0])
        })?,
        Family::Funnel => {
            let takes = [(0, Class::Gpr), (1, Class::Gpr), (2, Class::Gpr)];
            let left = which == Intrinsic::Fshl;
            shared::each_lane(cx, &lp, &takes, |cx, _, t| {
                let first = funnel_reg(cx, left, [gp(t[0]), gp(t[1]), gp(t[2])], bits);
                Ok(if first { t[0] } else { t[1] })
            })?;
        }
        _ => {
            let operand_bits = match lp.args[0] {
                Arg::Vector(_, operand) => operand.shape.elem.bits(),
                Arg::Scalar(_) => bits,
            };
            let takes = [(0, Class::Gpr), (1, shared::class_of(cx, lp.args[1]))];
            shared::each_lane(cx, &lp, &takes, |cx, _, t| {
                let (x, y) = (gp(t[0]), gp(t[1]));
                match family {
                    Family::Two => two_reg(cx, which, x, y, bits)?,
                    Family::ThreeWay => {
                        three_way(cx, which == Intrinsic::Scmp, x, y, operand_bits);
                    }
                    _ => put(cx, mc::and_rr(Size::Q, x, y)),
                }
                Ok(t[0])
            })?;
        }
    }
    done(cx, inst, &lp)
}

/// `x = scmp|ucmp(x, y)` of `bits`-bit lanes (loaded zero-extended): -1,
/// 0 or 1. Writes `rax`, `rcx` and the flags.
fn three_way(cx: &mut FnCx<'_, X64>, signed: bool, x: Reg, y: Reg, bits: u32) {
    if signed {
        crate::mem::sign_extend(cx, x, bits);
        crate::mem::sign_extend(cx, y, bits);
    }
    put(cx, mc::cmp_rr(Size::Q, x, y));
    put(cx, mc::setcc_r(if signed { Cond::G } else { Cond::A }, RAX));
    put(cx, mc::setcc_r(if signed { Cond::L } else { Cond::B }, RCX));
    put(cx, mc::sub_rr(Size::B, RAX, RCX));
    put(cx, mc::movsx_rr(Size::Q, x, Size::B, RAX));
}

// ---- floating-point intrinsics ----

/// The libm operation of `which` with its arity, if it is one.
fn libm(which: Intrinsic<'_>) -> Option<(FpOp, usize)> {
    use FpOp as O;
    use Intrinsic as I;
    Some(match which {
        I::Sin => (O::Sin, 1),
        I::Cos => (O::Cos, 1),
        I::Tan => (O::Tan, 1),
        I::Asin => (O::Asin, 1),
        I::Acos => (O::Acos, 1),
        I::Atan => (O::Atan, 1),
        I::Atan2 => (O::Atan2, 2),
        I::Sinh => (O::Sinh, 1),
        I::Cosh => (O::Cosh, 1),
        I::Tanh => (O::Tanh, 1),
        I::Exp => (O::Exp, 1),
        I::Exp2 => (O::Exp2, 1),
        I::Exp10 => (O::Exp10, 1),
        I::Log => (O::Log, 1),
        I::Log2 => (O::Log2, 1),
        I::Log10 => (O::Log10, 1),
        I::Pow => (O::Pow, 2),
        I::Floor => (O::Floor, 1),
        I::Ceil => (O::Ceil, 1),
        I::Trunc => (O::Trunc, 1),
        I::Round => (O::Round, 1),
        I::RoundEven => (O::RoundEven, 1),
        I::NearbyInt => (O::NearbyInt, 1),
        I::Rint => (O::Rint, 1),
        I::Fma => (O::Fma, 3),
        I::Lround => (O::Lround, 1),
        I::Llround => (O::Llround, 1),
        I::Powi => (O::Powi, 1),
        I::Ldexp => (O::Ldexp, 1),
        _ => return None,
    })
}

/// The number of floating-point operands of `which`.
fn fp_arity(which: Intrinsic<'_>) -> usize {
    use Intrinsic as I;
    match which {
        I::CopySign
        | I::MinNum
        | I::MaxNum
        | I::Minimum
        | I::Maximum
        | I::MinimumNum
        | I::MaximumNum => 2,
        I::FmulAdd => 3,
        _ => libm(which).map_or(1, |(_, n)| n),
    }
}

/// A floating-point intrinsic element-wise: per lane, the operands of a
/// `half` vector widened to `float` (calls), then the `float`/`double`
/// kernel ([`fp_kernel`]), then a `half` result narrowed. `fabs` and
/// `copysign` act on the sign bit of any element directly.
///
/// # Errors
/// `fpintr::BFLOAT` for `bfloat` lanes; `fpintr::INTRINSIC` for an
/// operand list that is not the intrinsic's or a `powi` exponent wider
/// than 32 bits; what `vector::lanes` refuses; what the calls report.
fn fp_lanes<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    use Intrinsic as I;
    let fps = fp_arity(which);
    let exponent = matches!(which, I::Powi | I::Ldexp);
    let arity = fps + usize::from(exponent);
    if args.len() != arity || !cx.types().is_vector(inst.ty()) {
        return unsupported(crate::fpintr::INTRINSIC);
    }
    let src = vector::lanes(cx, value_ty(cx, args[0]))?;
    let elem = src.shape.elem;
    if elem == Elem::BFloat {
        return unsupported(crate::fpintr::BFLOAT);
    }
    if !elem.is_float() {
        return unsupported(crate::fpintr::INTRINSIC);
    }
    let n_bits = if exponent {
        let ty = value_ty(cx, args[1]);
        let n_ty = vector::Shape::of(cx.types(), ty).map_or(ty, |s| s.elem_ty);
        match cx.types().int_bits(n_ty) {
            Some(bits @ 1..=32) => bits,
            Some(bits @ 33..=64) if which == I::Ldexp => bits,
            _ => return unsupported(crate::fpintr::INTRINSIC),
        }
    } else {
        0
    };
    let lp = setup(cx, inst, args)?;
    let res_bits = lp.result.shape.elem.bits();
    let int_result = !lp.result.shape.elem.is_float();
    let half = elem == Elem::Half;
    let slots = if half { Some(half_slots(cx)?) } else { None };
    let n_slot = if half && exponent { Some(cx.alloc_slot(8, 8)?) } else { None };
    // The minimal pool has two vector registers: `half` lanes are taken
    // in general registers (they go to slots anyway, and the widened
    // operands and the kernel need the vector registers) but for
    // `fabs`/`copysign`, which work on the lane's bits in place; the
    // third `float`/`double` operand is not taken but read from its
    // image (into `xmm14`, or by the call).
    let three = fps == 3;
    let in_place = matches!(which, I::Fabs | I::CopySign);
    let lane_class = if half && !in_place { Class::Gpr } else { Class::Fpr };
    let mut takes = [(0, Class::Fpr); 4];
    for (i, take) in takes.iter_mut().enumerate().take(fps) {
        *take = (i, lane_class);
    }
    if exponent {
        takes[fps] = (fps, shared::class_of(cx, lp.args[fps]));
    }
    let used = if three && !half { 2 } else { arity };
    let third = lp.args[2];
    shared::each_lane(cx, &lp, &takes[..used], |cx, lane, t| {
        let mut x = [t[0]; 3];
        x[..fps.min(t.len())].copy_from_slice(&t[..fps.min(t.len())]);
        let mut c_at = None;
        if three
            && !half
            && let shared::Arg::Vector(image, l) = third
        {
            let at = image.offset_by(l.offset(lane));
            let s = cx.scratch_reg(Class::Fpr);
            vector::load_lane(cx, &l, At::Frame(at), s);
            x[2] = s;
            c_at = Some(at);
        }
        let mut n = exponent.then(|| gp(t[fps]));
        if half && matches!(which, I::Fabs | I::CopySign) {
            sign_op(cx, which, FpSize::S, 15, xm(x[0]), xm(x[1]));
            return Ok(x[0]);
        }
        if let (Some(slot), Some(r)) = (n_slot, n) {
            frame_inst(cx, slot, |m| mc::mov_mr(Size::Q, m, r));
        }
        if let Some(slots) = &slots {
            let w = widen_halves(cx, &t[..fps], slots)?;
            let k = fps.min(2);
            x[..k].copy_from_slice(&w[..k]);
            // Unused operands name a vector register too (the kernel
            // reads only its own; a third `half` one from `slots`).
            for unused in x.iter_mut().take(3).skip(fps.max(1)) {
                *unused = w[0];
            }
            if let Some(slot) = n_slot {
                let r = cx.temp(Class::Gpr)?;
                frame_inst(cx, slot, |m| mc::mov_rm(Size::Q, gp(r), m));
                n = Some(gp(r));
            }
        }
        let size = if elem == Elem::Double { FpSize::D } else { FpSize::S };
        let n = n.map(|r| (r, n_bits));
        let r = fp_kernel(cx, which, size, half, x, c_at, n, res_bits, slots.as_ref())?;
        if half && !int_result && !matches!(which, I::Fma) { half_out(cx, r) } else { Ok(r) }
    })?;
    done(cx, inst, &lp)
}

/// `fabs` (`which`) or `copysign` on the scalar view of `x` (`y` the sign
/// source) of `size` with the sign at bit `sign`: through `rax`, `rcx`.
/// Writes `rax`, `rcx` and the flags.
fn sign_op(cx: &mut FnCx<'_, X64>, which: Intrinsic<'_>, size: FpSize, sign: u8, x: Xmm, y: Xmm) {
    let s = if size == FpSize::D { Size::Q } else { Size::D };
    put(cx, mc::movd_rx(s, RAX, x));
    put(cx, mc::btr_ri(s, RAX, sign));
    if which == Intrinsic::CopySign {
        put(cx, mc::movd_rx(s, RCX, y));
        put(cx, mc::shr_ri(s, RCX, sign));
        put(cx, mc::and_ri(s, RCX, 1));
        put(cx, mc::shl_ri(s, RCX, sign));
        put(cx, mc::or_rr(s, RAX, RCX));
    }
    put(cx, mc::movd_xr(s, x, RAX));
}

/// One lane of floating-point intrinsic `which` on `float`/`double`
/// operands `x` (pool temporaries; a third one in `xmm14`, its lane at
/// `c_at`, which a call reads), of `size`, the integer exponent `n`
/// (and its width) of `powi`/`ldexp`: the register of the result (a
/// floating-point one of `size`, or an integer of `res_bits` bits).
/// `half` says the lanes were `half` (for `fma` in `double` and the
/// narrowing it does itself, `fmuladd` rounded after the product, the
/// addend reloaded from `slots`, and `minnum`/`maxnum` by
/// `fminf`/`fmaxf`, as the scalar lowering).
#[allow(clippy::too_many_arguments, reason = "the lane's operands and shapes")]
fn fp_kernel(
    cx: &mut FnCx<'_, X64>,
    which: Intrinsic<'_>,
    size: FpSize,
    half: bool,
    [a, b, c]: [PReg; 3],
    c_at: Option<FrameRef>,
    n: Option<(Reg, u32)>,
    res_bits: u32,
    slots: Option<&HalfSlots>,
) -> CgResult<PReg> {
    use Intrinsic as I;
    let (xa, xb) = (xm(a), xm(b));
    match which {
        I::Sqrt => put(cx, mc::sqrts_rr(size, xa, xa)),
        I::ArithmeticFence => {}
        I::Canonicalize => {
            let one = if size == FpSize::D { 0x3ff0_0000_0000_0000 } else { 0x3f80_0000 };
            xmm_imm(cx, XMM15, one, if size == FpSize::D { 64 } else { 32 });
            put(cx, mc::muls_rr(size, xa, XMM15));
        }
        I::Fabs | I::CopySign => {
            let sign = if size == FpSize::D { 63 } else { 31 };
            sign_op(cx, which, size, sign, xa, xb);
        }
        I::FmulAdd if half => {
            // `half` rounds after the multiplication too; the addend is
            // in its slot as a `float`.
            let Some(slots) = slots else { return cx.bug("half lanes without their slots") };
            put(cx, mc::muls_rr(FpSize::S, xa, xb));
            cx.free_temp(b);
            let p = half_out(cx, a)?;
            let t = cx.temp(Class::Fpr)?;
            put(cx, mc::movap_rr(FpSize::S, xm(t), xm(p)));
            cx.free_temp(p);
            let ext = half_ext(cx)?;
            let w = runtime(cx, ext, &[CallArg::Reg(t)])?;
            let tc = cx.temp(Class::Fpr)?;
            load_fpr(cx, xm(tc), slots[2], 4);
            put(cx, mc::adds_rr(FpSize::S, xm(w), xm(tc)));
            cx.free_temp(tc);
            return Ok(w);
        }
        I::FmulAdd => {
            put(cx, mc::muls_rr(size, xa, xb));
            put(cx, mc::adds_rr(size, xa, xm(c)));
        }
        I::Fma if half => {
            put(cx, mc::cvtss2sd_rr(xa, xa));
            put(cx, mc::cvtss2sd_rr(xb, xb));
            put(cx, mc::muls_rr(FpSize::D, xa, xb));
            let Some(slots) = slots else { return cx.bug("half lanes without their slots") };
            load_fpr(cx, XMM15, slots[2], 4);
            put(cx, mc::cvtss2sd_rr(XMM15, XMM15));
            put(cx, mc::adds_rr(FpSize::D, xa, XMM15));
            return narrow_call(cx, FShape::Double, CallArg::Reg(a));
        }
        I::MinNum | I::MaxNum if half => {
            let func = if which == I::MinNum { RuntimeFn::Fminf } else { RuntimeFn::Fmaxf };
            return runtime(cx, func, &[CallArg::Reg(a), CallArg::Reg(b)]);
        }
        I::MinNum | I::MaxNum | I::Minimum | I::Maximum | I::MinimumNum | I::MaximumNum => {
            min_max_reg(cx, which, size, xa, xb)?;
            if half {
                // Given back for the narrowing's register (the minimal
                // pool has two).
                cx.free_temp(b);
            }
        }
        I::Lrint | I::Llrint => {
            let rd = cx.temp(Class::Gpr)?;
            put(cx, mc::cvts2si_rr(size_for_bits(res_bits), gp(rd), size, xa));
            return Ok(rd);
        }
        I::FptosiSat | I::FptouiSat => {
            let rd = cx.temp(Class::Gpr)?;
            saturate_reg(cx, which == I::FptosiSat, size, xa, gp(rd), res_bits);
            return Ok(rd);
        }
        _ => {
            let Some((op, count)) = libm(which) else {
                return unsupported(crate::fpintr::INTRINSIC);
            };
            let func = routine(cx, op, size)?;
            let mut list = [CallArg::Reg(a); 3];
            list[1] = CallArg::Reg(b);
            // The third operand (`fma`) from its image.
            list[2] = c_at.map_or(CallArg::Reg(c), CallArg::Mem);
            let mut count = count;
            if let Some((r, bits)) = n {
                let t = exponent(cx, r, bits)?;
                list[1] = CallArg::Reg(t);
                count = 2;
            }
            let r = runtime(cx, func, &list[..count])?;
            if matches!(which, I::Lround | I::Llround) {
                let rd = cx.temp(Class::Gpr)?;
                put(cx, mc::mov_rr(Size::Q, gp(rd), gp(r)));
                cx.free_temp(r);
                return Ok(rd);
            }
            return Ok(r);
        }
    }
    Ok(a)
}

/// The exponent lane of `powi`/`ldexp` (`bits` wide, loaded
/// zero-extended in `r`) as the C `int` argument in a pool temporary:
/// sign-extended below 32 bits, clamped to the `int` range above (as
/// `fpintr.rs`). Writes `r11` and the flags.
fn exponent(cx: &mut FnCx<'_, X64>, r: Reg, bits: u32) -> CgResult<PReg> {
    let t = cx.temp(Class::Gpr)?;
    let tr = gp(t);
    put(cx, mc::mov_rr(Size::Q, tr, r));
    if bits < 32 {
        extend_to(cx, tr, bits, Ext::Sign, Size::D);
    } else if bits > 32 {
        extend_to(cx, tr, bits, Ext::Sign, Size::Q);
        mov_imm(cx, R11, 0x7fff_ffff, 64);
        put(cx, mc::cmp_rr(Size::Q, tr, R11));
        put(cx, mc::cmovcc_rr(Cond::G, Size::Q, tr, R11));
        mov_imm(cx, R11, 0xffff_ffff_8000_0000, 64);
        put(cx, mc::cmp_rr(Size::Q, tr, R11));
        put(cx, mc::cmovcc_rr(Cond::L, Size::Q, tr, R11));
    }
    Ok(t)
}

// ---- reductions and parts ----

/// The integer fold of a reduction, as a binary opcode or a min/max
/// intrinsic.
fn int_fold(which: Intrinsic<'_>) -> Option<Result<Opcode, Intrinsic<'static>>> {
    use Intrinsic as I;
    Some(match which {
        I::VectorReduceAdd => Ok(Opcode::Add),
        I::VectorReduceMul => Ok(Opcode::Mul),
        I::VectorReduceAnd => Ok(Opcode::And),
        I::VectorReduceOr => Ok(Opcode::Or),
        I::VectorReduceXor => Ok(Opcode::Xor),
        I::VectorReduceSmax => Err(I::Smax),
        I::VectorReduceSmin => Err(I::Smin),
        I::VectorReduceUmax => Err(I::Umax),
        I::VectorReduceUmin => Err(I::Umin),
        _ => return None,
    })
}

/// `llvm.vector.reduce.*` of integer or floating-point lanes: lane 0,
/// then one scalar kernel per lane (`fadd`/`fmul` in lane order from the
/// start value, which is right with and without `reassoc`; `fmax`/`fmin`
/// as `maxnum`/`minnum` lane after lane).
///
/// # Errors
/// [`REDUCE`] for operands that are not the intrinsic's; what
/// `vector::lanes` refuses; `fpintr::BFLOAT` for `bfloat` lanes;
/// `vector::BFLOAT`... for `half` lanes of a floating-point fold
/// [`REDUCE`]; what asking for the operands reports.
fn reduce<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    use Intrinsic as I;
    let ordered = matches!(which, I::VectorReduceFadd | I::VectorReduceFmul);
    let ((true, &[_, vec]) | (false, &[vec])) = (ordered, args) else {
        return unsupported(REDUCE);
    };
    let l = vector::lanes(cx, value_ty(cx, vec))?;
    let elem = l.shape.elem;
    let fold = int_fold(which);
    if elem.is_float() == fold.is_some() {
        return unsupported(REDUCE);
    }
    if elem == Elem::BFloat {
        return unsupported(REDUCE);
    }
    deferred_first(cx, inst, None)?;
    if elem == Elem::Half {
        return half_reduce(cx, inst, which, ordered.then(|| args[0]), vec, &l);
    }
    let image = vector::image(cx, vec, &l, 0)?;
    let lane = |k: u32| At::Frame(image.offset_by(l.offset(k)));
    if let Some(fold) = fold {
        let bits = elem.bits();
        let rd = cx.def_reg(inst)?;
        let t = cx.temp(Class::Gpr)?;
        vector::load_lane(cx, &l, lane(0), rd);
        for k in 1..l.count() {
            vector::load_lane(cx, &l, lane(k), t);
            match fold {
                Ok(op) => int_lane(cx, op, bits, gp(rd), gp(t))?,
                Err(min_max) => two_reg(cx, min_max, gp(rd), gp(t), bits)?,
            }
        }
        cx.free_temp(t);
        return cx.finish(inst, rd);
    }
    let size = if elem == Elem::Double { FpSize::D } else { FpSize::S };
    let (acc, first) = if ordered {
        let acc = cx.temp(Class::Fpr)?;
        cx.put(args[0], acc)?;
        (acc, 0)
    } else {
        let acc = cx.temp(Class::Fpr)?;
        vector::load_lane(cx, &l, lane(0), acc);
        (acc, 1)
    };
    let t = cx.temp(Class::Fpr)?;
    for k in first..l.count() {
        vector::load_lane(cx, &l, lane(k), t);
        match which {
            I::VectorReduceFadd => put(cx, mc::adds_rr(size, xm(acc), xm(t))),
            I::VectorReduceFmul => put(cx, mc::muls_rr(size, xm(acc), xm(t))),
            I::VectorReduceFmax => min_max_reg(cx, I::MaxNum, size, xm(acc), xm(t))?,
            _ => min_max_reg(cx, I::MinNum, size, xm(acc), xm(t))?,
        }
    }
    cx.free_temp(t);
    cx.finish(inst, acc)
}

/// A floating-point reduction of `half` lanes, as `llc` promotes it: each
/// step widens the accumulator and the lane to `float`, computes there
/// (`fminf`/`fmaxf` for `fmin`/`fmax`, as the scalar lowering) and
/// rounds back to `half`. The accumulator lives in a frame slot across
/// the calls; `start` is the ordered reductions' first operand.
///
/// # Errors
/// What asking for the operands and the calls report.
fn half_reduce<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    start: Option<Value>,
    vec: Value,
    l: &Lanes,
) -> CgResult<()> {
    use Intrinsic as I;
    let image = vector::image(cx, vec, l, 0)?;
    let lane = |k: u32| At::Frame(image.offset_by(l.offset(k)));
    let acc = At::Frame(cx.alloc_slot(2, 2)?);
    let slots = half_slots(cx)?;
    let first = if let Some(start) = start {
        let t = cx.temp(Class::Fpr)?;
        cx.put(start, t)?;
        vector::store_lane(cx, l, acc, t);
        cx.free_temp(t);
        0
    } else {
        let t = cx.temp(Class::Gpr)?;
        vector::load_lane(cx, l, lane(0), t);
        vector::store_lane(cx, l, acc, t);
        cx.free_temp(t);
        1
    };
    for k in first..l.count() {
        let a = cx.temp(Class::Gpr)?;
        let b = cx.temp(Class::Gpr)?;
        vector::load_lane(cx, l, acc, a);
        vector::load_lane(cx, l, lane(k), b);
        let [x, y] = widen_halves(cx, &[a, b], &slots)?;
        let r = match which {
            I::VectorReduceFadd | I::VectorReduceFmul => {
                let op = if which == I::VectorReduceFadd { Opcode::FAdd } else { Opcode::FMul };
                arith(cx, op, FpSize::S, xm(x), xm(y))?;
                cx.free_temp(y);
                x
            }
            _ => {
                let func =
                    if which == I::VectorReduceFmax { RuntimeFn::Fmaxf } else { RuntimeFn::Fminf };
                runtime(cx, func, &[CallArg::Reg(x), CallArg::Reg(y)])?
            }
        };
        let h = half_out(cx, r)?;
        vector::store_lane(cx, l, acc, h);
        cx.free_temp(h);
    }
    let rd = cx.def_reg(inst)?;
    vector::load_lane(cx, l, acc, rd);
    cx.finish(inst, rd)
}

/// `llvm.vector.extract(%v, idx)` and `llvm.vector.insert(%v, %sub, idx)`
/// of fixed vectors: bytes copied between the images (the index is a
/// constant multiple of the part's length, verified).
///
/// # Errors
/// `intrinsic::INTRINSIC` for operands that are not the intrinsic's (a
/// variable or out-of-range index); what `vector::lanes` refuses.
fn part<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    let insert = which == Intrinsic::VectorInsert;
    let (vec, sub, at) = match (insert, args) {
        (false, &[v, i]) => (v, None, i),
        (true, &[v, s, i]) => (v, Some(s), i),
        _ => return unsupported(crate::intrinsic::INTRINSIC),
    };
    let Some(idx) = crate::intrinsic::const_int(cx, at) else {
        return unsupported(crate::intrinsic::INTRINSIC);
    };
    let whole = vector::lanes(cx, value_ty(cx, vec))?;
    let piece = match sub {
        Some(s) => vector::lanes(cx, value_ty(cx, s))?,
        None => vector::lanes(cx, inst.ty())?,
    };
    if idx.saturating_add(u64::from(piece.count())) > u64::from(whole.count()) {
        return unsupported(crate::intrinsic::INTRINSIC);
    }
    let off = whole.offset(idx as u32);
    let piece_bytes = piece.count() * piece.bytes();
    deferred_first(cx, inst, None)?;
    let src = vector::image(cx, vec, &whole, 0)?;
    let result = if insert { whole } else { piece };
    let out =
        if result.shape.class == Class::Fpr { vector::scratch(cx, 2)? } else { cx.def_mem(inst)? };
    if let Some(s) = sub {
        let sub_image = if s == vec { src } else { vector::image(cx, s, &piece, 1)? };
        copy_frame(cx, out, src, vector::store_bytes(&whole)?);
        copy_frame(cx, out.offset_by(off), sub_image, piece_bytes);
    } else {
        copy_frame(cx, out, src.offset_by(off), piece_bytes);
    }
    if result.shape.class == Class::Fpr {
        let rd = cx.def_reg(inst)?;
        vector::reload(cx, xm(rd), &result, out);
        return cx.finish(inst, rd);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arities() {
        assert_eq!(fp_arity(Intrinsic::Fma), 3);
        assert_eq!(fp_arity(Intrinsic::FmulAdd), 3);
        assert_eq!(fp_arity(Intrinsic::CopySign), 2);
        assert_eq!(fp_arity(Intrinsic::Powi), 1);
        assert_eq!(fp_arity(Intrinsic::Sqrt), 1);
    }
}
