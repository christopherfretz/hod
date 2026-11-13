//! Element-wise operations on vectors, one lane at a time
//! (`spec/codegen.md` §5.5.1 "Vectors, the lane loop (WP-080)"; design
//! note `docs/design/0003-vectors.md` package V3).
//!
//! **Owner: WP-080.** Tests: `tests/lanes.rs`. The groups hook in where
//! they refused a vector: `aggr::binary` and `aggr::cast` (vectors of
//! class `Mem`), `float::binary` and `float::cast` (the vectors of class
//! `Fpr` without a one-instruction form), the `fneg` arm of the dispatch
//! in `target.rs` (a `Mem` vector), and the top of
//! `intrinsic::intrinsic`.
//!
//! # The loop
//!
//! Every operation here is one loop, unrolled at emit time over the
//! lanes (at most [`vector::MAX_LANES`]). The loop itself is `hod_cg`'s
//! (`hod_cg::lanes`, `spec/codegen.md` §7.3 "The lane loop", WP-129):
//! `A64` implements its `LaneKernels` below, and each operation passes
//! its scalar kernel to `each_lane`. The steps:
//!
//! 1. the shapes are checked ([`vector::lanes`] and its refusals) before
//!    anything is emitted;
//! 2. each vector operand's image is found once: the slot of a `Mem`
//!    vector, or the register of an `Fpr` vector stored into a scratch
//!    image (and given back at once, so that the loop has the whole pool);
//!    a scalar operand (`powi`'s exponent) is put into a register in
//!    every lane;
//! 3. the result is written lane by lane into its slot (`Mem`), or into
//!    a scratch image that is loaded into the result's register at the
//!    end (`Fpr`): the next free image, or the first operand's when the
//!    three are taken (three operands of the result's type, read lane by
//!    lane before the lane is written);
//! 4. per lane: pool temporaries of the element's class, the operands'
//!    lanes loaded into them (zero-extended), the **scalar lowering's own
//!    register kernel** (`int::lane_binary`, `float::lane_binary`,
//!    `intrinsic::lane_unary`, `fpintr::lane_inline`, ...: never a second
//!    copy of a scalar rule), the result lane stored, the temporaries
//!    given back.
//!
//! What a lane computes is therefore what the scalar instruction
//! computes, poison and undefined behaviour included: a lane of `udiv`
//! by zero is `udiv`'s result for it (0), a shift by the width or more is
//! the register shift's.
//!
//! # Refusals
//!
//! The shapes [`vector::lanes`] refuses keep its names
//! ([`vector::LENGTH`], [`vector::WIDE_ELEMENT`], [`vector::ODD_ELEMENT`]);
//! a vector of `bfloat` is [`vector::BFLOAT`]. Vectors of `i1` are
//! WP-081's: a binary operation or a cast of one stays [`MEM_VECTOR`],
//! an intrinsic of one is left to `intrinsic.rs` (`a64:intrinsic-vector`).
//! `frexp` and `is.fpclass` of a vector stay `a64:intrinsic-vector`.
//!
//! # Registers
//!
//! `x16`, `x17`, `v31` and what the kernels write (each says); at most
//! three general and two vector pool temporaries at once, the third
//! floating-point operand of `fma`/`fmuladd` in `v31`.

use hod_cg::lanes::{self as shared, Geometry, LaneKernels};
use hod_cg::{CgResult, Class, FnCx, FrameRef, PReg};
use hod_ir::{Inst, Intrinsic, Opcode, TypeId, Value};

use crate::common::{Cursor, V31, X16, deferred_first, load_fpr_at, unsupported, value_ty, vr, xr};
use crate::fpintr::{self, LaneFp};
use crate::target::A64;
use crate::vector::{self, Elem, Lanes, MEM_VECTOR, SCRATCH_IMAGES, Shape};
use crate::{float, int, intrinsic};

/// The lane loop's moves on AArch64 (`hod_cg::lanes`): a lane is reached
/// through a frame cursor that rebases into `x16`; a register vector's
/// image is its `D`/`Q` register stored to a scratch image of 16 bytes.
impl LaneKernels for A64 {
    type Lanes = Lanes;

    const SCRATCH_IMAGES: u32 = SCRATCH_IMAGES;

    fn geometry(lanes: &Lanes) -> Geometry {
        lanes.geometry()
    }

    fn deferred_first<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>) -> CgResult<()> {
        deferred_first(cx, inst, None)
    }

    fn scratch(cx: &mut FnCx<'_, A64>, k: u32) -> CgResult<FrameRef> {
        vector::scratch(cx, k)
    }

    fn spill(cx: &mut FnCx<'_, A64>, reg: PReg, lanes: &Lanes, k: u32) -> CgResult<FrameRef> {
        vector::spill(cx, vr(reg), &lanes.shape, k)
    }

    fn reload(cx: &mut FnCx<'_, A64>, reg: PReg, lanes: &Lanes, image: FrameRef) {
        vector::reload(cx, vr(reg), &lanes.shape, image);
    }

    fn load_lane(cx: &mut FnCx<'_, A64>, lanes: &Lanes, at: FrameRef, rt: PReg) {
        let mut at = Cursor::frame(cx, at, X16);
        vector::load_lane(cx, lanes, rt, &mut at);
    }

    fn store_lane(cx: &mut FnCx<'_, A64>, lanes: &Lanes, at: FrameRef, rt: PReg) {
        let mut at = Cursor::frame(cx, at, X16);
        vector::store_lane(cx, lanes, rt, &mut at);
    }
}

/// An operand of a lane loop.
type Arg = shared::Arg<Lanes>;

/// The operands and the result of a lane loop.
type Loop = shared::LaneLoop<Lanes>;

/// The lanes of vector type `ty`, or the refusal.
fn lanes_of(cx: &FnCx<'_, A64>, ty: TypeId) -> CgResult<Lanes> {
    match Shape::of(cx.types(), ty) {
        // WP-081's, under the name the groups give it.
        Some(shape) if shape.elem == Elem::Int(1) => unsupported(MEM_VECTOR),
        Some(shape) if shape.elem == Elem::BFloat => unsupported(vector::BFLOAT),
        Some(shape) => shape.lanes(),
        None => unsupported(MEM_VECTOR),
    }
}

/// Whether `ty` is a vector of `i1` (a mask: WP-081's).
fn is_mask(cx: &FnCx<'_, A64>, ty: TypeId) -> bool {
    Shape::of(cx.types(), ty).is_some_and(|shape| shape.elem == Elem::Int(1))
}

/// Checks the shapes of `inst` and its `operands` (at most three), then
/// finds the operands' images and the result's (`hod_cg::lanes::setup`).
fn setup<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, operands: &[Value]) -> CgResult<Loop> {
    shared::setup(cx, inst, operands, |cx, ty| lanes_of(cx, ty))
}

/// Loads lane `lane` of the floating-point vector `arg` into `v31`.
fn load_v31(cx: &mut FnCx<'_, A64>, arg: Arg, lane: u32) -> CgResult<()> {
    let Arg::Vector(image, lanes) = arg else {
        return cx.bug("a scalar third operand of a floating-point lane operation");
    };
    let mut at = Cursor::frame(cx, vector::offset(image, lanes.offset(lane)), X16);
    load_fpr_at(cx, lanes.fpr_view(), V31, &mut at);
    Ok(())
}

/// The class of a lane of `arg` (of a scalar operand: its own).
fn class_of(cx: &FnCx<'_, A64>, arg: Arg) -> Class {
    shared::class_of(cx, arg)
}

/// The end of a lane loop: an `Fpr` result's image loaded into its
/// register.
fn done<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, lp: &Loop) -> CgResult<()> {
    shared::finish(cx, inst, &lp.result, lp.dst)
}

// ---- instructions ----

/// `%r = <op> %lhs, %rhs` element-wise on vectors: the integer
/// operations (`add` .. `srem`, the shifts) through `int::lane_binary`,
/// `fadd` .. `fdiv` through `float::lane_binary`, `frem` through
/// `float::lane_frem` (a call per lane).
///
/// # Errors
/// What [`vector::lanes`] refuses; [`vector::BFLOAT`]; [`MEM_VECTOR`]
/// for vectors of `i1` (WP-081); what the operands and the calls report.
pub fn binary<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, lhs: Value, rhs: Value) -> CgResult<()> {
    use Opcode as O;
    let op = inst.opcode();
    let lp = setup(cx, inst, &[lhs, rhs])?;
    let elem = lp.result.shape.elem;
    let elem_ty = lp.result.shape.elem_ty;
    let both = |class| [(0, class), (1, class)];
    match op {
        O::FAdd | O::FSub | O::FMul | O::FDiv if elem.is_float() => {
            shared::each_lane(cx, &lp, &both(Class::Fpr), |cx, _, t| {
                float::lane_binary(cx, op, elem_ty, vr(t[0]), vr(t[1]))?;
                Ok(t[0])
            })?;
        }
        O::FRem if elem.is_float() => {
            shared::each_lane(cx, &lp, &both(Class::Fpr), |cx, _, t| {
                float::lane_frem(cx, elem_ty, t[0], t[1])
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
                int::lane_binary(cx, op, bits, xr(t[0]), xr(t[1]));
                Ok(t[0])
            })?;
        }
        _ => return cx.bug("a binary opcode that does not fit the vector's elements"),
    }
    done(cx, inst, &lp)
}

/// `%r = fneg %arg` of a vector of class `Mem`: the sign bit of each
/// lane flipped (`float::lane_fneg`).
///
/// # Errors
/// As [`binary`].
pub fn fneg<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let lp = setup(cx, inst, &[arg])?;
    if !lp.result.shape.elem.is_float() {
        return unsupported(float::FNEG);
    }
    let elem_ty = lp.result.shape.elem_ty;
    shared::each_lane(cx, &lp, &[(0, Class::Fpr)], |cx, _, t| {
        float::lane_fneg(cx, elem_ty, vr(t[0]))?;
        Ok(t[0])
    })?;
    done(cx, inst, &lp)
}

/// `%r = <cast> %arg` element-wise between vectors (not `bitcast`): the
/// integer casts and `ptrtoint`/`ptrtoaddr`/`inttoptr` through
/// `int::lane_cast`, the conversions with a floating-point side through
/// `float::lane_convert`. The two vectors may be of different classes.
///
/// # Errors
/// As [`binary`]; `float::CAST` for a conversion `float::cast` refuses.
pub fn cast<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    use Opcode as O;
    let op = inst.opcode();
    if !matches!(
        op,
        O::Trunc
            | O::ZExt
            | O::SExt
            | O::PtrToInt
            | O::PtrToAddr
            | O::IntToPtr
            | O::FPExt
            | O::FPTrunc
            | O::FPToUI
            | O::FPToSI
            | O::UIToFP
            | O::SIToFP
    ) {
        return cx.bug("an element-wise cast of an opcode that is none");
    }
    let lp = setup(cx, inst, &[arg])?;
    let Arg::Vector(_, from) = lp.args[0] else {
        return cx.bug("an element-wise cast of a scalar");
    };
    let to = lp.result;
    if matches!(op, O::Trunc | O::ZExt | O::SExt | O::PtrToInt | O::PtrToAddr | O::IntToPtr) {
        shared::each_lane(cx, &lp, &[(0, Class::Gpr)], |cx, _, t| {
            int::lane_cast(cx, op, from.shape.elem.bits(), to.shape.elem.bits(), xr(t[0]));
            Ok(t[0])
        })?;
    } else {
        shared::each_lane(cx, &lp, &[(0, from.elem_class())], |cx, _, t| {
            let src = t[0];
            let dst =
                if from.elem_class() == to.elem_class() { src } else { cx.temp(to.elem_class())? };
            float::lane_convert(cx, op, from.shape.elem_ty, to.shape.elem_ty, src, dst)?;
            Ok(dst)
        })?;
    }
    done(cx, inst, &lp)
}

// ---- intrinsics ----

/// The families of intrinsics with an element-wise form.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Family {
    /// `ctlz`, `cttz`, `ctpop`, `bswap`, `bitreverse`, `abs`.
    Unary,
    /// `smax`, `smin`, `umax`, `umin`.
    MinMax,
    /// `{s,u}{add,sub}.sat`.
    Saturating,
    /// `scmp`, `ucmp`.
    ThreeWay,
    /// `fshl`, `fshr`.
    Funnel,
    /// `ptrmask`.
    Ptrmask,
    /// The floating-point intrinsics (`fpintr`).
    Fp,
}

/// A call of intrinsic `which` with a vector result, element-wise; `None`
/// for any other intrinsic, which `intrinsic.rs` lowers or refuses as
/// before (masks, `frexp`, `is.fpclass`, the reductions, the target
/// intrinsics).
pub fn intrinsic<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> Option<CgResult<()>> {
    use Intrinsic as I;
    let family = match which {
        I::Ctlz | I::Cttz | I::Ctpop | I::Bswap | I::BitReverse | I::Abs => Family::Unary,
        I::Smax | I::Smin | I::Umax | I::Umin => Family::MinMax,
        I::SaddSat | I::UaddSat | I::SsubSat | I::UsubSat => Family::Saturating,
        I::Scmp | I::Ucmp => Family::ThreeWay,
        I::Fshl | I::Fshr => Family::Funnel,
        I::Ptrmask => Family::Ptrmask,
        I::Frexp | I::IsFpClass => return None,
        _ if fpintr::is_fp(which) => Family::Fp,
        _ => return None,
    };
    if !cx.types().is_vector(inst.ty())
        || is_mask(cx, inst.ty())
        || args.iter().any(|&a| is_mask(cx, value_ty(cx, a)))
    {
        return None;
    }
    Some(lane_intrinsic(cx, inst, which, family, args))
}

/// [`intrinsic`] of `family`.
#[allow(
    clippy::many_single_char_names,
    reason = "the operands are LangRef's a, b, c; x, y, z their lanes"
)]
fn lane_intrinsic<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    family: Family,
    args: &'a [Value],
) -> CgResult<()> {
    let arity = match family {
        Family::Unary => 1,
        Family::Funnel => 3,
        Family::Fp => args.len(),
        _ => 2,
    };
    if args.len() < arity || arity == 0 {
        return unsupported(intrinsic::INTRINSIC);
    }
    if family == Family::Fp {
        return fp_lanes(cx, inst, which, &args[..arity]);
    }
    if which == Intrinsic::Bswap
        && Shape::of(cx.types(), inst.ty()).is_some_and(|s| !s.elem.bits().is_multiple_of(16))
    {
        // The verifier rejects it: an odd number of bytes.
        return unsupported(intrinsic::INTRINSIC);
    }
    let lp = setup(cx, inst, &args[..arity])?;
    let bits = lp.result.shape.elem.bits();
    let [a, b, _] = lp.args;
    match family {
        Family::Unary => shared::each_lane(cx, &lp, &[(0, Class::Gpr)], |cx, _, t| {
            intrinsic::lane_unary(cx, which, xr(t[0]), bits)?;
            Ok(t[0])
        })?,
        Family::Funnel => {
            let takes = [(0, Class::Gpr), (1, Class::Gpr), (2, Class::Gpr)];
            let left = which == Intrinsic::Fshl;
            shared::each_lane(cx, &lp, &takes, |cx, _, t| {
                let first = intrinsic::lane_funnel(cx, left, [xr(t[0]), xr(t[1]), xr(t[2])], bits);
                Ok(if first { t[0] } else { t[1] })
            })?;
        }
        _ => {
            let operand_bits = match (family, a) {
                (Family::ThreeWay, Arg::Vector(_, operand)) => operand.shape.elem.bits(),
                (Family::ThreeWay, Arg::Scalar(_)) => {
                    return cx.bug("scmp/ucmp of a scalar into a vector");
                }
                _ => 0,
            };
            let takes = [(0, Class::Gpr), (1, class_of(cx, b))];
            shared::each_lane(cx, &lp, &takes, |cx, _, t| {
                let (x, y) = (xr(t[0]), xr(t[1]));
                match family {
                    Family::MinMax => intrinsic::lane_min_max(cx, which, x, y, bits),
                    Family::Saturating => intrinsic::lane_saturating(cx, which, x, y, bits),
                    Family::ThreeWay => {
                        let signed = which == Intrinsic::Scmp;
                        intrinsic::lane_three_way(cx, signed, x, y, operand_bits);
                    }
                    _ => int::lane_binary(cx, Opcode::And, 64, x, y),
                }
                Ok(t[0])
            })?;
        }
    }
    done(cx, inst, &lp)
}

/// The element type of `v`: its own type for a scalar.
fn elem_ty(cx: &FnCx<'_, A64>, v: Value) -> TypeId {
    let ty = value_ty(cx, v);
    Shape::of(cx.types(), ty).map_or(ty, |shape| shape.elem_ty)
}

/// A floating-point intrinsic element-wise: inline
/// (`fpintr::lane_inline`) or a call per lane (`fpintr::lane_call`).
fn fp_lanes<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    use Intrinsic as I;
    let elem = elem_ty(cx, args[0]);
    let n_elem = if matches!(which, I::Powi | I::Ldexp) {
        args.get(1).map(|&n| elem_ty(cx, n))
    } else {
        None
    };
    if Shape::of(cx.types(), value_ty(cx, args[0])).is_some_and(|s| s.elem == Elem::BFloat) {
        return unsupported(vector::BFLOAT);
    }
    let plan = fpintr::lane_plan(cx, which, elem, n_elem)?;
    let lp = setup(cx, inst, args)?;
    match plan {
        LaneFp::Inline
            if matches!(
                which,
                I::Lround | I::Llround | I::Lrint | I::Llrint | I::FptosiSat | I::FptouiSat
            ) =>
        {
            let bits = lp.result.shape.elem.bits();
            shared::each_lane(cx, &lp, &[(0, Class::Fpr)], |cx, _, t| {
                let rd = cx.temp(Class::Gpr)?;
                fpintr::lane_inline(cx, which, elem, [vr(t[0]), V31, V31], xr(rd), bits)?;
                Ok(rd)
            })?;
        }
        LaneFp::Inline => {
            let takes = [(0, Class::Fpr), (1, Class::Fpr)];
            let takes = &takes[..args.len().min(2)];
            shared::each_lane(cx, &lp, takes, |cx, lane, t| {
                if args.len() > 2 {
                    load_v31(cx, lp.args[2], lane)?;
                }
                let regs = [vr(t[0]), t.get(1).map_or(V31, |&y| vr(y)), V31];
                fpintr::lane_inline(cx, which, elem, regs, X16, 0)?;
                Ok(t[0])
            })?;
        }
        LaneFp::Call { func, n } => {
            let fps = if n == 0 { args.len() } else { 1 };
            let mut takes = [(0, Class::Fpr); 3];
            let mut count = 1;
            if fps > 1 {
                takes[count] = (1, Class::Fpr);
                count += 1;
            }
            if n != 0 {
                takes[count] = (1, class_of(cx, lp.args[1]));
                count += 1;
            }
            shared::each_lane(cx, &lp, &takes[..count], |cx, _, t| {
                let regs = if fps > 1 { [t[0], t[1]] } else { [t[0], t[0]] };
                let exponent = if n == 0 { None } else { Some((t[count - 1], n)) };
                fpintr::lane_call(cx, func, elem, &regs[..fps.min(2)], exponent)
            })?;
        }
    }
    done(cx, inst, &lp)
}
