//! The floating-point intrinsics (`spec/ir.md` §5.5) of scalar `half`,
//! `float`, `double` and `fp128` in SSE2 (`spec/codegen.md` §5.6.7,
//! WP-127): inline where SSE2 has the operation, the libm, libgcc or C23
//! function `llc` calls otherwise.
//!
//! **Owner: WP-127.** Tests: `tests/fpintr.rs`. The dispatch sends here
//! what [`is_fp`] accepts (the same set as AArch64's). The decisions:
//!
//! - `float` and `double`: `sqrt` is `sqrtss`/`sqrtsd`; the rounding
//!   family (`floor` .. `rint`), `fma`, `lround` and `llround` call libm
//!   (`floorf`, `fmaf`, `lroundf`, ...: SSE2 has no `roundss`, which is
//!   never emitted); `lrint`/`llrint` are `cvtss2si` (the current mode);
//!   `fmuladd` is `mulss` then `addss` (unfused, as `llc -O0`);
//!   `canonicalize` is `x * 1.0`. `fabs` and `copysign` are bit operations
//!   that leave a NaN's payload alone.
//! - The six minimum/maximum intrinsics are `llc`'s sequences:
//!   `minnum`/`maxnum` `minss` of the operands swapped, the second operand
//!   taken when the first is a NaN (`cmpunordss` mask); `minimum`,
//!   `maximum`, `minimumnum`, `maximumnum` order the operands by the
//!   first one's sign (so that `-0 < +0`), then `minss`/`maxss` and a NaN
//!   check.
//! - `fptosi.sat`/`fptoui.sat`: `cvttss2si` at 64 bits (the `fptoui`
//!   sequence for `u64`), then `cmov`s of the bounds after `ucomiss`
//!   against them and of 0 for a NaN.
//! - `is.fpclass` with a constant mask: the magnitude counted against the
//!   bounds of the classes into a kind, then a bit of a table chosen by
//!   the sign (as AArch64's).
//! - `half` is computed in `float` (`__extendhfsf2` in, `__truncsfhf2`
//!   out), calling the `float` routine (`fma` in `double`, as `llc` 22);
//!   `fabs` and `copysign` act on bit 15. `fp128` calls the `...f128`
//!   functions, `__powitf2` and `__multf3`; `fabs`/`copysign`/`is.fpclass`
//!   are inline, `fptosi.sat`/`fptoui.sat` are [`FP128`].
//! - `bfloat` is [`BFLOAT`]; vectors are [`crate::vector::VECTOR`].
//!
//! # Registers
//!
//! Besides the operands' registers, a lowering here writes `rax`, `rcx`,
//! `rdx`, `r11`, `xmm14` and `xmm15`, and every register a call writes.

#![allow(clippy::many_single_char_names, reason = "the operands are LangRef's x, a, b, c")]

use hod_cg::sym::{FpOp, FpType, fp_routine};
use hod_cg::{CallArg, CgResult, Class, FnCx, FrameRef, PReg, RuntimeFn};
use hod_ir::{Arch, Const, Inst, Intrinsic, TypeId, TypeKind, Value};
use hod_mc_x64 as mc;
use hod_mc_x64::{Cond, FpCmp, FpSize, Reg, Size, Xmm};

use crate::common::{
    R11, XMM14, XMM15, branch_cond, deferred_first, gp, load_fpr, load_gpr, mov_imm, put,
    store_fpr, store_gpr, unsupported, value_ty, xm,
};
use crate::float::{Shape, fp_to_int, give_back, half_in, half_out, runtime, xmm_imm};
use crate::int::{Ext, extend_to, size_for_bits};
use crate::target::X64;
use crate::vector::VECTOR;

/// A floating-point intrinsic with no lowering in this form: an operand
/// list or type the intrinsic does not take (invalid IR), a non-constant
/// `is.fpclass` mask, a `powi` exponent wider than 32 bits.
pub const INTRINSIC: &str = "x64:fp-intrinsic";
/// A floating-point intrinsic of `bfloat` (as AArch64's
/// `a64:intrinsic-bfloat`): no routine in the table.
pub const BFLOAT: &str = "x64:intrinsic-bfloat";
/// `fptosi.sat` or `fptoui.sat` of `fp128` (as AArch64's): `llc` compares
/// with the bounds through `__getf2`/`__unordtf2`; not written.
pub const FP128: &str = "x64:intrinsic-fp128";
/// An integer result or operand wider than 64 bits.
pub const WIDE: &str = "x64:intrinsic-wide";

const RAX: Reg = Reg::Rax;
const RCX: Reg = Reg::Rcx;
const RDX: Reg = Reg::Rdx;

/// Whether `which` is lowered here.
pub fn is_fp(which: Intrinsic<'_>) -> bool {
    use Intrinsic as I;
    matches!(
        which,
        I::Fabs
            | I::Sqrt
            | I::Floor
            | I::Ceil
            | I::Trunc
            | I::Round
            | I::RoundEven
            | I::NearbyInt
            | I::Rint
            | I::CopySign
            | I::Fma
            | I::FmulAdd
            | I::MinNum
            | I::MaxNum
            | I::Minimum
            | I::Maximum
            | I::MinimumNum
            | I::MaximumNum
            | I::Canonicalize
            | I::ArithmeticFence
            | I::Powi
            | I::Ldexp
            | I::Frexp
            | I::Lround
            | I::Llround
            | I::Lrint
            | I::Llrint
            | I::FptosiSat
            | I::FptouiSat
            | I::IsFpClass
    ) || libm(which).is_some()
}

/// The routine operation of a libm intrinsic.
fn libm(which: Intrinsic<'_>) -> Option<FpOp> {
    use FpOp as O;
    use Intrinsic as I;
    Some(match which {
        I::Sin => O::Sin,
        I::Cos => O::Cos,
        I::Tan => O::Tan,
        I::Asin => O::Asin,
        I::Acos => O::Acos,
        I::Atan => O::Atan,
        I::Atan2 => O::Atan2,
        I::Sinh => O::Sinh,
        I::Cosh => O::Cosh,
        I::Tanh => O::Tanh,
        I::Exp => O::Exp,
        I::Exp2 => O::Exp2,
        I::Exp10 => O::Exp10,
        I::Log => O::Log,
        I::Log2 => O::Log2,
        I::Log10 => O::Log10,
        I::Pow => O::Pow,
        _ => return None,
    })
}

/// The routine operation of the rounding family.
fn rounding_op(which: Intrinsic<'_>) -> Option<FpOp> {
    use FpOp as O;
    use Intrinsic as I;
    Some(match which {
        I::Floor => O::Floor,
        I::Ceil => O::Ceil,
        I::Trunc => O::Trunc,
        I::Round => O::Round,
        I::RoundEven => O::RoundEven,
        I::NearbyInt => O::NearbyInt,
        I::Rint => O::Rint,
        _ => return None,
    })
}

/// The shape of the scalar operand type `ty`, or the refusal.
fn scalar(cx: &FnCx<'_, X64>, ty: TypeId) -> CgResult<Shape> {
    match cx.types().kind(ty) {
        TypeKind::Half => Ok(Shape::Half),
        TypeKind::Float => Ok(Shape::Float),
        TypeKind::Double => Ok(Shape::Double),
        TypeKind::Fp128 => Ok(Shape::Quad),
        TypeKind::BFloat => unsupported(BFLOAT),
        TypeKind::Vector { .. } | TypeKind::ScalableVector { .. } => unsupported(VECTOR),
        _ => unsupported(INTRINSIC),
    }
}

/// The routine of `op` for `shape`, or the refusal where the table has
/// none.
fn routine(shape: Shape, op: FpOp) -> CgResult<RuntimeFn> {
    let ty = match shape {
        Shape::Half => FpType::Half,
        Shape::Double => FpType::Double,
        Shape::Quad => FpType::Fp128,
        _ => FpType::Float,
    };
    fp_routine(op, ty, Arch::X86_64).map_or_else(|| unsupported(INTRINSIC), Ok)
}

/// The width of the integer result of `inst` (1 to 64), or the refusal.
fn int_result(cx: &FnCx<'_, X64>, inst: Inst<'_>) -> CgResult<u32> {
    match cx.types().kind(inst.ty()) {
        TypeKind::Int(bits) if (1..=64).contains(&bits) => Ok(bits),
        TypeKind::Int(_) => unsupported(WIDE),
        TypeKind::Vector { .. } | TypeKind::ScalableVector { .. } => unsupported(VECTOR),
        _ => unsupported(INTRINSIC),
    }
}

/// The value of an integer constant operand.
fn const_int(cx: &FnCx<'_, X64>, value: Value) -> Option<u64> {
    let id = value.as_const()?;
    match cx.module.consts.get(id) {
        Const::Int { value, .. } => Some(value),
        Const::Null(_) | Const::Zero(_) | Const::Undef(_) | Const::Poison(_) => Some(0),
        _ => None,
    }
}

/// A call of floating-point intrinsic `which` (one [`is_fp`] accepts).
///
/// # Errors
/// [`VECTOR`] for a vector, [`BFLOAT`], [`FP128`], [`WIDE`] for an
/// integer wider than 64 bits, [`INTRINSIC`] for a form with no lowering;
/// what the operands and the runtime calls report.
pub fn intrinsic<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    use Intrinsic as I;
    let Some(&first) = args.first() else { return unsupported(INTRINSIC) };
    let f = scalar(cx, value_ty(cx, first))?;
    deferred_first(cx, inst, None)?;
    if let Some(op) = libm(which) {
        if args.len() != if which == I::Pow || which == I::Atan2 { 2 } else { 1 } {
            return unsupported(INTRINSIC);
        }
        return call_math(cx, inst, f, routine(f, op)?, args, None);
    }
    if let (Some(op), &[x]) = (rounding_op(which), args) {
        return call_math(cx, inst, f, routine(f, op)?, &[x], None);
    }
    match (which, args) {
        (I::Sqrt, &[x]) => sqrt(cx, inst, f, x),
        (I::Canonicalize, &[x]) => canonicalize(cx, inst, f, x),
        (I::ArithmeticFence, &[x]) => {
            let r = cx.use_reg(x)?;
            cx.finish(inst, r)
        }
        (I::Fabs, &[x]) => fabs(cx, inst, f, x),
        (I::CopySign, &[a, b]) => copysign(cx, inst, f, a, b),
        (I::Fma, &[a, b, c]) => fma(cx, inst, f, [a, b, c]),
        (I::FmulAdd, &[a, b, c]) => fmuladd(cx, inst, f, [a, b, c]),
        (
            I::MinNum | I::MaxNum | I::Minimum | I::Maximum | I::MinimumNum | I::MaximumNum,
            &[a, b],
        ) => min_max(cx, inst, which, f, a, b),
        (I::Lround | I::Llround, &[x]) => lround(cx, inst, which, f, x),
        (I::Lrint | I::Llrint, &[x]) => lrint(cx, inst, which, f, x),
        (I::FptosiSat | I::FptouiSat, &[x]) => saturate(cx, inst, which == I::FptosiSat, f, x),
        (I::IsFpClass, &[x, mask]) => fp_class(cx, inst, f, x, mask),
        (I::Powi, &[x, n]) => call_math(cx, inst, f, routine(f, FpOp::Powi)?, &[x], Some(n)),
        (I::Ldexp, &[x, n]) => call_math(cx, inst, f, routine(f, FpOp::Ldexp)?, &[x], Some(n)),
        (I::Frexp, &[x]) => frexp(cx, inst, f, x),
        _ => unsupported(INTRINSIC),
    }
}

// ---- half operands ----

/// The `half` operands `values` (at most three) widened to `float`, each
/// kept in a 4-byte frame slot across the calls that follow (a value goes
/// to one call only: a repeated operand shares its slot).
fn half_slots(cx: &mut FnCx<'_, X64>, values: &[Value]) -> CgResult<[Option<FrameRef>; 3]> {
    let mut slots = [None; 3];
    for (i, &value) in values.iter().enumerate().take(3) {
        if let Some(j) = values[..i].iter().position(|&v| v == value) {
            slots[i] = slots[j];
            continue;
        }
        let r = half_in(cx, value)?;
        let slot = cx.alloc_slot(4, 4)?;
        store_fpr(cx, xm(r), slot, 4);
        cx.free_temp(r);
        slots[i] = Some(slot);
    }
    Ok(slots)
}

/// A pool temporary loaded with the `float` at `slot`.
fn load_temp(cx: &mut FnCx<'_, X64>, slot: Option<FrameRef>) -> CgResult<PReg> {
    let Some(slot) = slot else { return cx.bug("a half operand without its slot") };
    let t = cx.temp(Class::Fpr)?;
    load_fpr(cx, xm(t), slot, 4);
    Ok(t)
}

/// The result register `r` of a call moved to a pool temporary, the
/// place a call argument must be in.
fn to_pool(cx: &mut FnCx<'_, X64>, r: PReg) -> CgResult<PReg> {
    let t = cx.temp(Class::Fpr)?;
    put(cx, mc::movap_rr(FpSize::S, xm(t), xm(r)));
    cx.free_temp(r);
    Ok(t)
}

// ---- inline operations ----

/// `sqrt`: `sqrtss`/`sqrtsd` (`half` in single precision, rounded once
/// more to `half`: exact, single has more than twice half's precision);
/// `fp128` calls `sqrtf128`.
fn sqrt<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, f: Shape, x: Value) -> CgResult<()> {
    match f {
        Shape::Quad => call_math(cx, inst, f, routine(f, FpOp::Sqrt)?, &[x], None),
        Shape::Half => {
            let r = half_in(cx, x)?;
            put(cx, mc::sqrts_rr(FpSize::S, xm(r), xm(r)));
            let r = half_out(cx, r)?;
            cx.finish(inst, r)
        }
        _ => {
            let r = cx.use_reg(x)?;
            put(cx, mc::sqrts_rr(f.size(), xm(r), xm(r)));
            cx.finish(inst, r)
        }
    }
}

/// `canonicalize`: `x * 1.0` (`mulss`/`mulsd`, `__multf3`; a signalling
/// NaN quieted, everything else unchanged, as `llc`); `half` widened and
/// narrowed.
fn canonicalize<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, f: Shape, x: Value) -> CgResult<()> {
    match f {
        Shape::Half => {
            let r = half_in(cx, x)?;
            let r = half_out(cx, r)?;
            cx.finish(inst, r)
        }
        Shape::Quad => {
            let one = cx.temp(Class::Fpr)?;
            xmm_imm(cx, xm(one), 0x3fff_0000_0000_0000, 64);
            // The low quadword to the high one.
            put(cx, mc::shufp_rri(FpSize::S, xm(one), xm(one), 0x4e));
            let r = runtime(cx, RuntimeFn::MulTf3, &[CallArg::Value(x), CallArg::Reg(one)])?;
            cx.finish(inst, r)
        }
        _ => {
            let r = cx.use_reg(x)?;
            let one = if f == Shape::Double { 0x3ff0_0000_0000_0000 } else { 0x3f80_0000 };
            xmm_imm(cx, XMM15, one, if f == Shape::Double { 64 } else { 32 });
            put(cx, mc::muls_rr(f.size(), xm(r), XMM15));
            cx.finish(inst, r)
        }
    }
}

/// The word of the scalar in `x` that holds its sign, in `rd`: the low
/// 32 bits for `half` and `float`, 64 for `double`, the upper quadword of
/// an `fp128` (through `xmm15`). The size of that view and its sign bit.
fn sign_word(cx: &mut FnCx<'_, X64>, f: Shape, rd: Reg, x: Xmm, via: Xmm) -> (Size, u8) {
    match f {
        Shape::Half => {
            put(cx, mc::movd_rx(Size::D, rd, x));
            (Size::D, 15)
        }
        Shape::Double => {
            put(cx, mc::movd_rx(Size::Q, rd, x));
            (Size::Q, 63)
        }
        Shape::Quad => {
            put(cx, mc::movhlps_rr(via, x));
            put(cx, mc::movd_rx(Size::Q, rd, via));
            (Size::Q, 63)
        }
        _ => {
            put(cx, mc::movd_rx(Size::D, rd, x));
            (Size::D, 31)
        }
    }
}

/// Writes the word in `rs` back to `x` (the counterpart of [`sign_word`];
/// for `fp128` the low quadword is kept). Writes `xmm15`.
fn put_sign_word(cx: &mut FnCx<'_, X64>, f: Shape, x: Xmm, rs: Reg) {
    match f {
        Shape::Double => put(cx, mc::movd_xr(Size::Q, x, rs)),
        Shape::Quad => {
            put(cx, mc::movd_xr(Size::Q, XMM15, rs));
            put(cx, mc::movlhps_rr(x, XMM15));
        }
        _ => put(cx, mc::movd_xr(Size::D, x, rs)),
    }
}

/// `fabs`: the sign bit cleared, nothing else (NaNs included).
fn fabs<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, f: Shape, x: Value) -> CgResult<()> {
    let r = cx.use_reg(x)?;
    let (size, sign) = sign_word(cx, f, RAX, xm(r), XMM15);
    put(cx, mc::btr_ri(size, RAX, sign));
    put_sign_word(cx, f, xm(r), RAX);
    cx.finish(inst, r)
}

/// `copysign a, b`: the sign bit of `b` with the other bits of `a`.
fn copysign<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    f: Shape,
    a: Value,
    b: Value,
) -> CgResult<()> {
    let ra = cx.use_reg(a)?;
    let rb = cx.use_reg(b)?;
    let (size, sign) = sign_word(cx, f, RAX, xm(ra), XMM15);
    sign_word(cx, f, RCX, xm(rb), XMM14);
    put(cx, mc::btr_ri(size, RAX, sign));
    put(cx, mc::shr_ri(size, RCX, sign));
    put(cx, mc::and_ri(size, RCX, 1));
    put(cx, mc::shl_ri(size, RCX, sign));
    put(cx, mc::or_rr(size, RAX, RCX));
    put_sign_word(cx, f, xm(ra), RAX);
    cx.finish(inst, ra)
}

/// `fma`: `fmaf`/`fma`/`fmaf128` (SSE2 has no fused instruction); `half`
/// in `double`, then `__truncdfhf2`. `llc` 22 calls `fma` in `double`;
/// the product of two `half`s is exact in `double` (22 bits), so
/// `mulsd` + `addsd` rounds once, as that `fma` does, in two registers.
fn fma<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, f: Shape, abc: [Value; 3]) -> CgResult<()> {
    if f != Shape::Half {
        let func = routine(f, FpOp::Fma)?;
        let args = abc.map(CallArg::Value);
        let r = runtime(cx, func, &args)?;
        return cx.finish(inst, r);
    }
    let slots = half_slots(cx, &abc)?;
    let p = load_temp(cx, slots[0])?;
    let t = load_temp(cx, slots[1])?;
    put(cx, mc::cvtss2sd_rr(xm(p), xm(p)));
    put(cx, mc::cvtss2sd_rr(xm(t), xm(t)));
    put(cx, mc::muls_rr(FpSize::D, xm(p), xm(t)));
    let Some(slot) = slots[2] else { return cx.bug("a half operand without its slot") };
    load_fpr(cx, xm(t), slot, 4);
    put(cx, mc::cvtss2sd_rr(xm(t), xm(t)));
    put(cx, mc::adds_rr(FpSize::D, xm(p), xm(t)));
    cx.free_temp(t);
    let r = crate::float::narrow_call(cx, Shape::Double, CallArg::Reg(p))?;
    cx.finish(inst, r)
}

/// `fmuladd`, as `llc -O0` makes it: a rounded multiplication, then a
/// rounded addition (`mulss` + `addss`; `half` rounded to `half` after
/// each; `fp128` `__multf3` + `__addtf3`).
fn fmuladd<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    f: Shape,
    [a, b, c]: [Value; 3],
) -> CgResult<()> {
    match f {
        Shape::Quad => {
            let p = runtime(cx, RuntimeFn::MulTf3, &[CallArg::Value(a), CallArg::Value(b)])?;
            let t = to_pool(cx, p)?;
            let r = runtime(cx, RuntimeFn::AddTf3, &[CallArg::Reg(t), CallArg::Value(c)])?;
            cx.finish(inst, r)
        }
        Shape::Half => {
            let slots = half_slots(cx, &[a, b, c])?;
            let ta = load_temp(cx, slots[0])?;
            let tb = load_temp(cx, slots[1])?;
            put(cx, mc::muls_rr(FpSize::S, xm(ta), xm(tb)));
            cx.free_temp(tb);
            let p = half_out(cx, ta)?;
            let t = to_pool(cx, p)?;
            let Some(ext) = hod_cg::sym::fp_ext_routine(FpType::Half, FpType::Float, Arch::X86_64)
            else {
                return cx.bug("no half extension routine on x86-64");
            };
            let p = runtime(cx, ext, &[CallArg::Reg(t)])?;
            let tc = load_temp(cx, slots[2])?;
            put(cx, mc::adds_rr(FpSize::S, xm(p), xm(tc)));
            cx.free_temp(tc);
            let r = half_out(cx, p)?;
            cx.finish(inst, r)
        }
        _ => {
            // The addend first: `c` may be `a`, whose register the product
            // overwrites.
            cx.put(c, crate::reg::xmm(XMM15))?;
            let ra = cx.use_reg(a)?;
            let rb = cx.use_reg(b)?;
            put(cx, mc::muls_rr(f.size(), xm(ra), xm(rb)));
            put(cx, mc::adds_rr(f.size(), xm(ra), XMM15));
            cx.finish(inst, ra)
        }
    }
}

/// The six minimum/maximum intrinsics: inline for `float` and `double`
/// and for `half` in single precision ([`min_max_reg`]); `fminf`/`fmaxf`
/// for `minnum`/`maxnum` of `half` (as `llc`); the `fp128` functions.
fn min_max<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    f: Shape,
    a: Value,
    b: Value,
) -> CgResult<()> {
    use FpOp as O;
    use Intrinsic as I;
    let op = match which {
        I::MinNum => O::MinNum,
        I::MaxNum => O::MaxNum,
        I::Minimum => O::Minimum,
        I::Maximum => O::Maximum,
        I::MinimumNum => O::MinimumNum,
        _ => O::MaximumNum,
    };
    if f == Shape::Quad || (f == Shape::Half && matches!(op, O::MinNum | O::MaxNum)) {
        return call_math(cx, inst, f, routine(f, op)?, &[a, b], None);
    }
    if f == Shape::Half {
        let slots = half_slots(cx, &[a, b])?;
        let ta = load_temp(cx, slots[0])?;
        if a == b {
            let r = half_out(cx, ta)?;
            return cx.finish(inst, r);
        }
        let tb = load_temp(cx, slots[1])?;
        min_max_reg(cx, which, FpSize::S, xm(ta), xm(tb))?;
        cx.free_temp(tb);
        let r = half_out(cx, ta)?;
        return cx.finish(inst, r);
    }
    let ra = cx.use_reg(a)?;
    if a == b {
        // Each of the six is `a` of `a, a` (`llc`'s sequences too).
        return cx.finish(inst, ra);
    }
    let rb = cx.use_reg(b)?;
    min_max_reg(cx, which, f.size(), xm(ra), xm(rb))?;
    cx.finish(inst, ra)
}

/// `ra = min|max(ra, rb)` in `size` for each of the six intrinsics, as
/// `llc` computes them (`ra` and `rb` distinct). Writes `rax`, `xmm14`,
/// `xmm15` and the flags.
///
/// - `minnum`/`maxnum`: `m = minss(b, a)` (`a` when either is a NaN),
///   then `b` where `a` is a NaN.
/// - `minimum`/`maximum`: `(x, y)` = the operands in an order by `a`'s
///   sign, then `x` if it is a NaN, else `minss(x, y)` (`y` for a NaN).
/// - `minimumnum`/`maximumnum`: `(q, p)` in an order by `a`'s sign, `m =
///   minss(q, p)`, then `q` if `m` is a NaN.
///
/// # Errors
/// What binding a label reports.
pub fn min_max_reg(
    cx: &mut FnCx<'_, X64>,
    which: Intrinsic<'_>,
    size: FpSize,
    ra: Xmm,
    rb: Xmm,
) -> CgResult<()> {
    use Intrinsic as I;
    let is_min = matches!(which, I::MinNum | I::Minimum | I::MinimumNum);
    let op =
        |dst, src| if is_min { mc::mins_rr(size, dst, src) } else { mc::maxs_rr(size, dst, src) };
    if matches!(which, I::MinNum | I::MaxNum) {
        put(cx, mc::movap_rr(size, XMM15, rb));
        put(cx, op(XMM15, ra));
        put(cx, mc::movap_rr(size, XMM14, ra));
        put(cx, mc::cmps_rr(FpCmp::Unord, size, XMM14, XMM14));
        put(cx, mc::movap_rr(size, ra, XMM14));
        put(cx, mc::andnp_rr(size, XMM14, XMM15));
        put(cx, mc::andp_rr(size, ra, rb));
        put(cx, mc::orp_rr(size, ra, XMM14));
        return Ok(());
    }
    // The first of the pair is `a` when `a` is not negative.
    let a_first = matches!(which, I::Minimum | I::MinimumNum);
    let (first, second) = if a_first { (ra, rb) } else { (rb, ra) };
    let word = if size == FpSize::D { Size::Q } else { Size::D };
    let keep = cx.new_label();
    let done = cx.new_label();
    put(cx, mc::movap_rr(size, XMM15, first));
    put(cx, mc::movap_rr(size, XMM14, second));
    put(cx, mc::movd_rx(word, RAX, ra));
    put(cx, mc::test_rr(word, RAX, RAX));
    branch_cond(cx, Cond::Ns, keep)?;
    put(cx, mc::movap_rr(size, XMM15, second));
    put(cx, mc::movap_rr(size, XMM14, first));
    cx.bind(keep)?;
    put(cx, mc::movap_rr(size, ra, XMM15));
    if matches!(which, I::Minimum | I::Maximum) {
        put(cx, mc::ucomis_rr(size, XMM15, XMM15));
        branch_cond(cx, Cond::P, done)?;
        put(cx, op(ra, XMM14));
    } else {
        put(cx, op(ra, XMM14));
        put(cx, mc::ucomis_rr(size, ra, ra));
        branch_cond(cx, Cond::Np, done)?;
        put(cx, mc::movap_rr(size, ra, XMM15));
    }
    cx.bind(done)
}

/// `lround`/`llround`: `lroundf`/`lround`/`lroundf128` (`llroundf`, ...;
/// SSE2 has no rounding to nearest with ties away); `half` through
/// `roundf` then `cvttss2si` (as `llc`: the rounded value is an integer,
/// exactly converted).
fn lround<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    f: Shape,
    x: Value,
) -> CgResult<()> {
    let bits = int_result(cx, inst)?;
    let r = if f == Shape::Half {
        let r = half_in(cx, x)?;
        let t = to_pool(cx, r)?;
        let r = runtime(cx, RuntimeFn::Roundf, &[CallArg::Reg(t)])?;
        let rd = cx.def_reg(inst)?;
        put(cx, mc::cvtts2si_rr(size_for_bits(bits), gp(rd), FpSize::S, xm(r)));
        cx.free_temp(r);
        return cx.finish(inst, rd);
    } else {
        let op = if which == Intrinsic::Lround { FpOp::Lround } else { FpOp::Llround };
        runtime(cx, routine(f, op)?, &[CallArg::Value(x)])?
    };
    finish_gpr(cx, inst, r)
}

/// `inst`'s integer result, in the return register `r` of a call, moved
/// to its own register.
fn finish_gpr<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, r: PReg) -> CgResult<()> {
    let rd = cx.def_reg(inst)?;
    put(cx, mc::mov_rr(Size::Q, gp(rd), gp(r)));
    cx.free_temp(r);
    cx.finish(inst, rd)
}

/// `lrint`/`llrint`: `cvtss2si`/`cvtsd2si` (the current rounding mode;
/// `half` widened first), `lrintf128`/`llrintf128`.
fn lrint<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    f: Shape,
    x: Value,
) -> CgResult<()> {
    let bits = int_result(cx, inst)?;
    if f == Shape::Quad {
        let op = if which == Intrinsic::Lrint { FpOp::Lrint } else { FpOp::Llrint };
        let r = runtime(cx, routine(f, op)?, &[CallArg::Value(x)])?;
        return finish_gpr(cx, inst, r);
    }
    let r = if f == Shape::Half { half_in(cx, x)? } else { cx.use_reg(x)? };
    let rd = cx.def_reg(inst)?;
    put(cx, mc::cvts2si_rr(size_for_bits(bits), gp(rd), f.size(), xm(r)));
    if f == Shape::Half {
        cx.free_temp(r);
    } else {
        give_back(cx, x, r);
    }
    cx.finish(inst, rd)
}

/// The bits of `±2^k` in `size`.
fn pow2(size: FpSize, k: u32, negative: bool) -> u64 {
    if size == FpSize::D {
        (u64::from(1023 + k) << 52) | (u64::from(negative) << 63)
    } else {
        (u64::from(127 + k) << 23) | (u64::from(negative) << 31)
    }
}

/// `fptosi.sat`/`fptoui.sat` to `i1`..`i64` (`fp128` is [`FP128`]).
fn saturate<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    signed: bool,
    f: Shape,
    x: Value,
) -> CgResult<()> {
    let bits = int_result(cx, inst)?;
    if f == Shape::Quad {
        return unsupported(FP128);
    }
    let r = if f == Shape::Half { half_in(cx, x)? } else { cx.use_reg(x)? };
    let rd = cx.def_reg(inst)?;
    saturate_reg(cx, signed, f.size(), xm(r), gp(rd), bits);
    if f == Shape::Half {
        cx.free_temp(r);
    } else {
        give_back(cx, x, r);
    }
    cx.finish(inst, rd)
}

/// `rd = fptosi.sat|fptoui.sat(x)` (`signed`) to `bits` bits, `x` of
/// `size`: the truncation at 64 bits (exact in range), then the maximum
/// where `x >= 2^(bits-1)` (`2^bits` unsigned), the minimum where `x <
/// -2^(bits-1)` (0 where `x < 0` unsigned, a NaN included: unordered sets
/// `CF`), and 0 for a NaN. Writes `rcx`, `rdx`, `r11`, `xmm14`, `xmm15`
/// and the flags.
pub fn saturate_reg(
    cx: &mut FnCx<'_, X64>,
    signed: bool,
    size: FpSize,
    x: Xmm,
    rd: Reg,
    bits: u32,
) {
    let fbits = if size == FpSize::D { 64 } else { 32 };
    if signed || bits < 64 {
        put(cx, mc::cvtts2si_rr(Size::Q, rd, size, x));
    } else {
        fp_to_int(cx, size, x, 64, false, rd);
    }
    let (top, max) =
        if signed { (bits - 1, (1u64 << (bits - 1)) - 1) } else { (bits, u64::MAX >> (64 - bits)) };
    xmm_imm(cx, XMM15, pow2(size, top, false), fbits);
    mov_imm(cx, RCX, max, 64);
    put(cx, mc::ucomis_rr(size, x, XMM15));
    put(cx, mc::cmovcc_rr(Cond::Ae, Size::Q, rd, RCX));
    if signed {
        xmm_imm(cx, XMM15, pow2(size, bits - 1, true), fbits);
        mov_imm(cx, RCX, !max, 64);
    } else {
        xmm_imm(cx, XMM15, 0, fbits);
        mov_imm(cx, RCX, 0, 64);
    }
    put(cx, mc::ucomis_rr(size, x, XMM15));
    put(cx, mc::cmovcc_rr(Cond::B, Size::Q, rd, RCX));
    if signed {
        mov_imm(cx, RCX, 0, 64);
        put(cx, mc::ucomis_rr(size, x, x));
        put(cx, mc::cmovcc_rr(Cond::P, Size::Q, rd, RCX));
    }
}

/// The test of `is.fpclass` for the 6 kinds of magnitude (zero,
/// subnormal, normal, infinity, signalling NaN, quiet NaN), positive
/// and negative: bit `k` of each word says whether kind `k` is in
/// `mask` (`LangRef`'s 10 bits: 0 sNaN, 1 qNaN, 2 -inf, 3 -normal, 4
/// -subnormal, 5 -zero, 6 +zero, 7 +subnormal, 8 +normal, 9 +inf).
fn class_tables(mask: u64) -> (u64, u64) {
    let bit = |k: u32| (mask >> k) & 1;
    let nan = (bit(0) << 4) | (bit(1) << 5);
    let positive = bit(6) | (bit(7) << 1) | (bit(8) << 2) | (bit(9) << 3) | nan;
    let negative = bit(5) | (bit(4) << 1) | (bit(3) << 2) | (bit(2) << 3) | nan;
    (positive, negative)
}

/// The width, exponent and fraction widths of a format; for `fp128`
/// those of its upper half (the lower 64 fraction bits are folded into
/// bit 0 of it, which none of the bounds uses).
fn fields(f: Shape) -> (u32, u32, u32) {
    match f {
        Shape::Half => (16, 5, 10),
        Shape::Double => (64, 11, 52),
        Shape::Quad => (64, 15, 48),
        _ => (32, 8, 23),
    }
}

/// `is.fpclass x, mask` (`mask` a constant): the bits moved to `rax`
/// with the sign at bit 63 (an `fp128`'s low half folded into bit 0 of
/// its high half, a sticky bit), the magnitude `rdx = rax << 1` counted
/// against the bounds into a kind 0..5, then bit `kind` of the table of
/// the sign ([`class_tables`]).
fn fp_class<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    f: Shape,
    x: Value,
    mask: Value,
) -> CgResult<()> {
    let Some(mask) = const_int(cx, mask) else { return unsupported(INTRINSIC) };
    let mask = mask & 0x3ff;
    int_result(cx, inst)?;
    if mask == 0 || mask == 0x3ff {
        let rd = cx.def_reg(inst)?;
        mov_imm(cx, gp(rd), u64::from(mask != 0), 32);
        return cx.finish(inst, rd);
    }
    let a = cx.use_reg(x)?;
    let v = xm(a);
    let (width, exp, frac) = fields(f);
    match f {
        Shape::Half | Shape::Float => {
            put(cx, mc::movd_rx(Size::D, RAX, v));
            put(cx, mc::shl_ri(Size::Q, RAX, (64 - width) as u8));
        }
        Shape::Double => put(cx, mc::movd_rx(Size::Q, RAX, v)),
        _ => {
            put(cx, mc::movd_rx(Size::Q, RCX, v));
            mov_imm(cx, R11, 0, 32);
            put(cx, mc::test_rr(Size::Q, RCX, RCX));
            put(cx, mc::setcc_r(Cond::Ne, R11));
            put(cx, mc::movhlps_rr(XMM15, v));
            put(cx, mc::movd_rx(Size::Q, RAX, XMM15));
            put(cx, mc::or_rr(Size::Q, RAX, R11));
        }
    }
    give_back(cx, x, a);
    let rd = cx.def_reg(inst)?;
    let r = gp(rd);
    let up = 65 - width;
    let inf = ((1u64 << exp) - 1) << (frac + up);
    let min_normal = 1u64 << (frac + up);
    let quiet = inf | (1u64 << (frac - 1 + up));
    put(cx, mc::mov_rr(Size::Q, RDX, RAX));
    put(cx, mc::shl_ri(Size::Q, RDX, 1));
    mov_imm(cx, r, 0, 32);
    // kind = the number of bounds the magnitude is above.
    for bound in [0, min_normal - 1, inf - 1, inf, quiet - 1] {
        mov_imm(cx, R11, bound, 64);
        mov_imm(cx, RCX, 0, 32);
        put(cx, mc::cmp_rr(Size::Q, RDX, R11));
        put(cx, mc::setcc_r(Cond::A, RCX));
        put(cx, mc::add_rr(Size::D, r, RCX));
    }
    let (positive, negative) = class_tables(mask);
    mov_imm(cx, R11, positive, 64);
    mov_imm(cx, RCX, negative, 64);
    put(cx, mc::test_rr(Size::Q, RAX, RAX));
    put(cx, mc::cmovcc_rr(Cond::S, Size::Q, R11, RCX));
    put(cx, mc::bt_rr(Size::Q, R11, r));
    put(cx, mc::setcc_r(Cond::B, r));
    put(cx, mc::movzx_rr(Size::D, r, Size::B, r));
    cx.finish(inst, rd)
}

// ---- calls ----

/// An `int` argument of a call: as it is when it is 32 bits wide, else
/// sign-extended to 32 bits in a temporary, or for `ldexp` (`clamp`) a
/// wider exponent clamped to the `int` range (beyond it every result is
/// already 0, infinite or `x`).
fn int_arg(cx: &mut FnCx<'_, X64>, n: Value, clamp: bool) -> CgResult<CallArg> {
    let bits = match cx.types().int_bits(value_ty(cx, n)) {
        Some(bits @ 1..=32) => bits,
        Some(bits @ 33..=64) if clamp => bits,
        // `powi`'s exponent is the C `int` of `__powisf2`; a wider one
        // has no call (as `llc`).
        Some(1..=64) => return unsupported(INTRINSIC),
        _ => return unsupported(WIDE),
    };
    if bits == 32 {
        return Ok(CallArg::Value(n));
    }
    let a = cx.use_reg(n)?;
    let t = cx.temp(Class::Gpr)?;
    let tr = gp(t);
    put(cx, mc::mov_rr(Size::Q, tr, gp(a)));
    give_back(cx, n, a);
    if bits < 32 {
        extend_to(cx, tr, bits, Ext::Sign, Size::D);
    } else {
        extend_to(cx, tr, bits, Ext::Sign, Size::Q);
        mov_imm(cx, R11, 0x7fff_ffff, 64);
        put(cx, mc::cmp_rr(Size::Q, tr, R11));
        put(cx, mc::cmovcc_rr(Cond::G, Size::Q, tr, R11));
        mov_imm(cx, R11, 0xffff_ffff_8000_0000, 64);
        put(cx, mc::cmp_rr(Size::Q, tr, R11));
        put(cx, mc::cmovcc_rr(Cond::L, Size::Q, tr, R11));
    }
    Ok(CallArg::Reg(t))
}

/// `inst` as a call of `func` with the floating-point operands `fps`
/// (one or two) and the `int` operand `n`; `half` through the `float`
/// function, the result narrowed.
fn call_math<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    f: Shape,
    func: RuntimeFn,
    fps: &[Value],
    n: Option<Value>,
) -> CgResult<()> {
    let mut list = [CallArg::Value(Value::NONE); 3];
    let mut count = 0;
    if f == Shape::Half {
        let slots = half_slots(cx, fps)?;
        for &slot in slots.iter().take(fps.len()) {
            list[count] = CallArg::Reg(load_temp(cx, slot)?);
            count += 1;
        }
    } else {
        for &x in fps {
            list[count] = CallArg::Value(x);
            count += 1;
        }
    }
    if let Some(n) = n {
        let clamp = matches!(func, RuntimeFn::Ldexpf | RuntimeFn::Ldexp | RuntimeFn::Ldexpf128);
        list[count] = int_arg(cx, n, clamp)?;
        count += 1;
    }
    let r = runtime(cx, func, &list[..count])?;
    let r = if f == Shape::Half { half_out(cx, r)? } else { r };
    cx.finish(inst, r)
}

/// `frexp x` into its `{ T, i32 }` result (class `Mem`): `frexpf`,
/// `frexp`, `frexpf128` with the exponent written to a frame slot, then
/// both fields written to the result's slot; `half` through `frexpf`
/// (exact: the fraction of a `half` is one).
fn frexp<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, f: Shape, x: Value) -> CgResult<()> {
    let ty = inst.ty();
    let fields = cx.types().struct_fields(ty);
    if fields.map(<[TypeId]>::len) != Some(2)
        || fields.is_some_and(|fs| cx.types().int_bits(fs[1]) != Some(32))
    {
        return unsupported(INTRINSIC);
    }
    let exp_at = cx.types().field_offset(ty, 1) as u32;
    let func = routine(if f == Shape::Half { Shape::Float } else { f }, FpOp::Frexp)?;
    let slot = cx.alloc_slot(4, 4)?;
    let arg = if f == Shape::Half {
        let r = half_in(cx, x)?;
        CallArg::Reg(to_pool(cx, r)?)
    } else {
        CallArg::Value(x)
    };
    let r = runtime(cx, func, &[arg, CallArg::Addr(slot)])?;
    let r = if f == Shape::Half { half_out(cx, r)? } else { r };
    let dst = cx.def_mem(inst)?;
    match f {
        Shape::Half => {
            put(cx, mc::movd_rx(Size::D, RAX, xm(r)));
            store_gpr(cx, RAX, dst, 2);
        }
        Shape::Double => store_fpr(cx, xm(r), dst, 8),
        Shape::Quad => store_fpr(cx, xm(r), dst, 16),
        _ => store_fpr(cx, xm(r), dst, 4),
    }
    load_gpr(cx, RAX, slot, 4);
    store_gpr(cx, RAX, dst.offset_by(exp_at), 4);
    cx.free_temp(r);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tables of `is.fpclass` for single classes.
    #[test]
    fn class_tables_place_each_class() {
        assert_eq!(class_tables(1 << 0), (0b01_0000, 0b01_0000));
        assert_eq!(class_tables(1 << 1), (0b10_0000, 0b10_0000));
        assert_eq!(class_tables(1 << 2), (0, 0b1000));
        assert_eq!(class_tables(1 << 5), (0, 0b0001));
        assert_eq!(class_tables(1 << 6), (0b0001, 0));
        assert_eq!(class_tables(1 << 9), (0b1000, 0));
    }

    /// The powers of two of the saturation bounds.
    #[test]
    fn pow2_bits() {
        assert_eq!(pow2(FpSize::S, 31, false), 0x4f00_0000);
        assert_eq!(pow2(FpSize::S, 63, true), 0xdf00_0000);
        assert_eq!(pow2(FpSize::D, 64, false), 0x43f0_0000_0000_0000);
        assert_eq!(pow2(FpSize::D, 0, true), 0xbff0_0000_0000_0000);
    }
}
