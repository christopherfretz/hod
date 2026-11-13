//! The floating-point intrinsics (`spec/ir.md` §5.5) of scalar `half`,
//! `float`, `double` and `fp128`: inline where ARMv8.0 has the
//! operation, calls of libm, libgcc or C23 functions otherwise.
//!
//! **Owner: WP-070.** Tests: `tests/fpintr.rs`. `intrinsic.rs` dispatches
//! here. The decisions are `spec/codegen.md` §5.5.1, "Intrinsics, the
//! second set (WP-070)":
//!
//! - `float` and `double` use the instruction (`fsqrt`, the `frint*`
//!   family, `fmadd`, `fminnm`/`fmaxnm`/`fmin`/`fmax`, `fcvtas`,
//!   `frintx` + `fcvtzs`, `fcvtz*` with a clamp); `fabs`, `copysign` and
//!   `is.fpclass` are bit operations in `x16`/`x17`, NaNs untouched.
//! - `half` is computed as `llc` does it: widened to `float` (`double`
//!   for `fma`), the operation, narrowed; `fmuladd` of `half` is a
//!   rounded `fmul` then a rounded `fadd`, `fabs`/`copysign` act on bit 15.
//! - `fp128` calls the `long double` function of libm (`sqrtl`, `fmal`,
//!   `floorl`, `fminl`, the C23 `fminimuml`/`fminimum_numl` family,
//!   `lroundl`, ...), `__powitf2`, `__multf3` (`canonicalize` is `x * 1`,
//!   `fmuladd` `__multf3` + `__addtf3`); `fabs`, `copysign` and
//!   `is.fpclass` are inline. `fptosi.sat`/`fptoui.sat` of `fp128` are
//!   refused ([`FP128`]).
//! - The libm intrinsics (`sin` .. `log10`, `pow`, `atan2`), `powi`,
//!   `ldexp` and `frexp` are calls: `sinf`/`sin`/`sinl`, `__powisf2`,
//!   `ldexpf`, `frexpf`, ...; `half` through the `float` function, the
//!   result narrowed (as `llc`).

#![allow(
    clippy::many_single_char_names,
    reason = "the operands are LangRef's x, a, b, c; f the format"
)]

use hod_cg::sym::{FpOp, FpType, fp_routine};
use hod_cg::{CallArg, CgResult, Class, FnCx, PReg, RuntimeFn};
use hod_ir::{Arch, Inst, Intrinsic, TypeId, TypeKind, Value};
use hod_mc_a64 as mc;
use hod_mc_a64::{Cond, FpFormat, FpSize, Reg, Size, VMemSize, VReg, VecBits};

use crate::common::{
    Cursor, V31, X16, X17, deferred_first, load_gpr_at, mov_imm, put, size_for_bits, store_fpr_at,
    store_gpr_at, unsupported, value_ty, vr, xr,
};
use crate::intrinsic::{INTRINSIC, VECTOR, WIDE};
use crate::target::A64;

/// A floating-point intrinsic of `bfloat`: the base architecture has no
/// `bfloat` arithmetic (as `float::BFLOAT`).
pub const BFLOAT: &str = "a64:intrinsic-bfloat";
/// `fptosi.sat` or `fptoui.sat` of `fp128`: `llc` compares with the
/// bounds through `__getf2`/`__gttf2`/`__unordtf2`; not written.
pub const FP128: &str = "a64:intrinsic-fp128";

/// A three-register floating-point instruction's encoder.
type Op3 = fn(FpSize, VReg, VReg, VReg) -> u32;

/// The scalar formats.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fmt {
    H,
    S,
    D,
    Q,
}

impl Fmt {
    /// The size an inline operation computes in (`half` in single).
    fn size(self) -> FpSize {
        if self == Fmt::D { FpSize::D } else { FpSize::S }
    }

    /// The register view of the value.
    fn view(self) -> VMemSize {
        match self {
            Fmt::H => VMemSize::H,
            Fmt::S => VMemSize::S,
            Fmt::D => VMemSize::D,
            Fmt::Q => VMemSize::Q,
        }
    }

    /// The routine `op` of this format calls (`hod_cg::sym`'s table;
    /// `half` calls the `float` one).
    ///
    /// # Errors
    /// [`INTRINSIC`] where the table has no call.
    fn routine(self, op: FpOp) -> CgResult<RuntimeFn> {
        let ty = match self {
            Fmt::H => FpType::Half,
            Fmt::S => FpType::Float,
            Fmt::D => FpType::Double,
            Fmt::Q => FpType::Fp128,
        };
        fp_routine(op, ty, Arch::Aarch64).map_or_else(|| unsupported(INTRINSIC), Ok)
    }
}

/// The format of `ty`, or the refusal.
fn fmt_of(cx: &FnCx<'_, A64>, ty: TypeId) -> CgResult<Fmt> {
    match cx.types().kind(ty) {
        TypeKind::Half => Ok(Fmt::H),
        TypeKind::Float => Ok(Fmt::S),
        TypeKind::Double => Ok(Fmt::D),
        TypeKind::Fp128 => Ok(Fmt::Q),
        TypeKind::BFloat => unsupported(BFLOAT),
        TypeKind::Vector { .. } | TypeKind::ScalableVector { .. } => unsupported(VECTOR),
        _ => unsupported(INTRINSIC),
    }
}

/// The width of the integer result of `inst` (1 to 64), or the refusal.
fn int_result(cx: &FnCx<'_, A64>, inst: Inst<'_>) -> CgResult<u32> {
    match cx.types().kind(inst.ty()) {
        TypeKind::Int(bits) if (1..=64).contains(&bits) => Ok(bits),
        TypeKind::Int(_) => unsupported(WIDE),
        TypeKind::Vector { .. } | TypeKind::ScalableVector { .. } => unsupported(VECTOR),
        _ => unsupported(INTRINSIC),
    }
}

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
            | I::Pow
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

/// A call of floating-point intrinsic `which` (one [`is_fp`] accepts).
///
/// # Errors
/// [`VECTOR`] for a vector, [`BFLOAT`], [`FP128`], [`WIDE`] for an
/// integer result wider than 64 bits, [`INTRINSIC`] for a form with no
/// lowering; what the operands and the runtime calls report.
pub fn fp_intrinsic<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    use Intrinsic as I;
    let Some(&first) = args.first() else { return unsupported(INTRINSIC) };
    let f = fmt_of(cx, value_ty(cx, first))?;
    if let Some(op) = libm(which) {
        return call_math(cx, inst, f, f.routine(op)?, args, None);
    }
    match (which, args) {
        (
            I::Sqrt
            | I::Floor
            | I::Ceil
            | I::Trunc
            | I::Round
            | I::RoundEven
            | I::NearbyInt
            | I::Rint
            | I::Canonicalize,
            &[x],
        ) => rounding(cx, inst, which, f, x),
        (I::ArithmeticFence, &[x]) => {
            deferred_first(cx, inst, None)?;
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
        (I::Lround | I::Llround | I::Lrint | I::Llrint, &[x]) => to_int(cx, inst, which, f, x),
        (I::FptosiSat | I::FptouiSat, &[x]) => saturate(cx, inst, which == I::FptosiSat, f, x),
        (I::IsFpClass, &[x, mask]) => fp_class(cx, inst, f, x, mask),
        (I::Powi, &[_, n]) => {
            let func = f.routine(FpOp::Powi)?;
            call_math(cx, inst, f, func, &args[..1], Some(n))
        }
        (I::Ldexp, &[_, n]) => {
            let func = f.routine(FpOp::Ldexp)?;
            call_math(cx, inst, f, func, &args[..1], Some(n))
        }
        (I::Frexp, &[x]) => frexp(cx, inst, f, x),
        _ => unsupported(INTRINSIC),
    }
}

// ---- one lane of a vector (WP-080) ----

/// How [`lanes.rs`](crate::lanes) computes one lane of a floating-point
/// intrinsic on vectors (WP-080).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LaneFp {
    /// Inline, by [`lane_inline`].
    Inline,
    /// A call of the function, by [`lane_call`]; `n` is the width of the
    /// `int` operand (`powi`, `ldexp`), else 0.
    Call {
        /// The function.
        func: RuntimeFn,
        /// The width of the `int` operand, 0 if there is none.
        n: u32,
    },
}

/// How one lane of `which` on elements of `elem` (`half`, `float`,
/// `double`) is computed; `n_elem` is the type of the `int` operand of
/// `powi`/`ldexp` (its element, for a vector one).
///
/// # Errors
/// [`VECTOR`] for an intrinsic with no element-wise form here (`frexp`,
/// `is.fpclass`, whose results are not one vector of lanes); what the
/// scalar form refuses for the element.
pub fn lane_plan(
    cx: &FnCx<'_, A64>,
    which: Intrinsic<'_>,
    elem: TypeId,
    n_elem: Option<TypeId>,
) -> CgResult<LaneFp> {
    use Intrinsic as I;
    let f = fmt_of(cx, elem)?;
    if f == Fmt::Q {
        return cx.bug("a vector lane of fp128");
    }
    if let Some(op) = libm(which) {
        return Ok(LaneFp::Call { func: f.routine(op)?, n: 0 });
    }
    let (op, clamp) = match which {
        I::Powi => (FpOp::Powi, false),
        I::Ldexp => (FpOp::Ldexp, true),
        I::Frexp | I::IsFpClass => return unsupported(VECTOR),
        _ => return Ok(LaneFp::Inline),
    };
    let Some(n_elem) = n_elem else { return unsupported(INTRINSIC) };
    let n = int_arg_bits(cx.types().int_bits(n_elem), clamp)?;
    Ok(LaneFp::Call { func: f.routine(op)?, n })
}

/// One lane of an inline floating-point intrinsic ([`LaneFp::Inline`])
/// on elements of `elem`, as the scalar form computes it: the operands
/// in `v` (as many as the intrinsic has; the third may be `v31`), the
/// result in `v[0]`, or in `rd` at `bits` bits for `lround`..`llrint`
/// and `fptosi.sat`/`fptoui.sat`. Writes the operands, `x16` and `x17`.
///
/// # Errors
/// [`INTRINSIC`] for an intrinsic that is not inline.
pub fn lane_inline(
    cx: &mut FnCx<'_, A64>,
    which: Intrinsic<'_>,
    elem: TypeId,
    v: [VReg; 3],
    rd: Reg,
    bits: u32,
) -> CgResult<()> {
    use Intrinsic as I;
    let f = fmt_of(cx, elem)?;
    match which {
        I::Sqrt
        | I::Floor
        | I::Ceil
        | I::Trunc
        | I::Round
        | I::RoundEven
        | I::NearbyInt
        | I::Rint
        | I::Canonicalize => rounding_reg(cx, which, f, v[0]),
        I::ArithmeticFence => {}
        I::Fabs => fabs_reg(cx, f, v[0]),
        I::CopySign => copysign_reg(cx, f, v[0], v[1]),
        I::Fma => fma_reg(cx, f, v[0], v[1], v[2]),
        I::FmulAdd => fmuladd_reg(cx, f, v[0], v[1], v[2]),
        I::MinNum | I::MaxNum | I::Minimum | I::Maximum | I::MinimumNum | I::MaximumNum => {
            min_max_reg(cx, which, f, v[0], v[1]);
        }
        I::Lround | I::Llround | I::Lrint | I::Llrint => to_int_reg(cx, which, f, v[0], rd, bits),
        I::FptosiSat | I::FptouiSat => saturate_reg(cx, which == I::FptosiSat, f, v[0], rd, bits),
        _ => return unsupported(INTRINSIC),
    }
    Ok(())
}

/// One lane of a floating-point intrinsic that is a call
/// ([`LaneFp::Call`]): `func` of the elements of `elem` in the pool
/// temporaries `fps` (one or two) and of the `int` of `n.1` bits in the
/// temporary `n.0`, as the scalar form calls it (`half` widened to
/// `float`, the result narrowed). The result's register (every
/// temporary has ended at the call).
///
/// # Errors
/// What the call reports.
pub fn lane_call(
    cx: &mut FnCx<'_, A64>,
    func: RuntimeFn,
    elem: TypeId,
    fps: &[PReg],
    n: Option<(PReg, u32)>,
) -> CgResult<PReg> {
    let f = fmt_of(cx, elem)?;
    let mut list = [CallArg::Value(Value::NONE); 3];
    let mut count = 0;
    for &x in fps.iter().take(2) {
        if f == Fmt::H {
            widen(cx, vr(x));
        }
        list[count] = CallArg::Reg(x);
        count += 1;
    }
    if let Some((t, bits)) = n {
        int_arg_reg(cx, xr(t), xr(t), bits);
        list[count] = CallArg::Reg(t);
        count += 1;
    }
    let Some(r) = cx.call_runtime(func, &list[..count], None)? else {
        return cx.bug("a runtime function returned nothing");
    };
    if f == Fmt::H {
        narrow(cx, vr(r));
    }
    Ok(r)
}

// ---- inline operations ----

/// The scalar instruction of a one-operand intrinsic.
type Fp2 = fn(FpSize, VReg, VReg) -> u32;

/// `half` in `v` widened to single precision in place.
fn widen(cx: &mut FnCx<'_, A64>, v: VReg) {
    put(cx, mc::fcvt(FpFormat::S, v, FpFormat::H, v));
}

/// The single in `v` narrowed to `half` in place.
fn narrow(cx: &mut FnCx<'_, A64>, v: VReg) {
    put(cx, mc::fcvt(FpFormat::H, v, FpFormat::S, v));
}

/// `canonicalize` as an instruction: `fminnm x, x` (a signalling NaN
/// quieted, everything else unchanged, as `llc`).
fn canon(sz: FpSize, rd: VReg, rn: VReg) -> u32 {
    mc::fminnm(sz, rd, rn, rn)
}

/// `sqrt`, the rounding family, `canonicalize`.
fn rounding<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    f: Fmt,
    x: Value,
) -> CgResult<()> {
    use FpOp as O;
    use Intrinsic as I;
    deferred_first(cx, inst, None)?;
    if f == Fmt::Q {
        let op = match which {
            I::Sqrt => O::Sqrt,
            I::Floor => O::Floor,
            I::Ceil => O::Ceil,
            I::Trunc => O::Trunc,
            I::Round => O::Round,
            I::RoundEven => O::RoundEven,
            I::NearbyInt => O::NearbyInt,
            I::Rint => O::Rint,
            // `x * 1.0`, as `llc`.
            _ => return quad_canonicalize(cx, inst, x),
        };
        let func = f.routine(op)?;
        return finish_call(cx, inst, func, &[CallArg::Value(x)], false);
    }
    let r = cx.use_reg(x)?;
    rounding_reg(cx, which, f, vr(r));
    cx.finish(inst, r)
}

/// `sqrt`, the rounding family or `canonicalize` of the `half`, `float`
/// or `double` in `v`, in place.
fn rounding_reg(cx: &mut FnCx<'_, A64>, which: Intrinsic<'_>, f: Fmt, v: VReg) {
    use Intrinsic as I;
    let op: Fp2 = match which {
        I::Sqrt => mc::fsqrt,
        I::Floor => mc::frintm,
        I::Ceil => mc::frintp,
        I::Trunc => mc::frintz,
        // Ties away from zero.
        I::Round => mc::frinta,
        // Ties to even.
        I::RoundEven => mc::frintn,
        // The current mode, inexact not raised.
        I::NearbyInt => mc::frinti,
        // The current mode, inexact raised.
        I::Rint => mc::frintx,
        _ => canon,
    };
    if f == Fmt::H {
        widen(cx, v);
    }
    put(cx, op(f.size(), v, v));
    if f == Fmt::H {
        narrow(cx, v);
    }
}

/// `canonicalize fp128`: `__multf3(x, 1.0)`.
fn quad_canonicalize<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, x: Value) -> CgResult<()> {
    let one = cx.temp(Class::Fpr)?;
    mov_imm(cx, Size::X, X16, 0x3fff_0000_0000_0000);
    put(cx, mc::fmov_gpr_to_fp(Size::X, vr(one), hod_mc_a64::RegZr::ZR));
    put(cx, mc::fmov_gpr_to_hi(vr(one), X16.zr()));
    finish_call(cx, inst, RuntimeFn::MulTf3, &[CallArg::Value(x), CallArg::Reg(one)], false)
}

/// The bits of the scalar in `v` with its sign bit at bit `sign` of
/// `rd` (`fmov` of the `W` or `X` view, or the upper half of an
/// `fp128`), and the size of the general view.
fn sign_word(cx: &mut FnCx<'_, A64>, f: Fmt, rd: Reg, v: VReg) -> (Size, u32) {
    match f {
        Fmt::H => {
            put(cx, mc::fmov_fp_to_gpr(Size::W, rd.zr(), v));
            (Size::W, 15)
        }
        Fmt::S => {
            put(cx, mc::fmov_fp_to_gpr(Size::W, rd.zr(), v));
            (Size::W, 31)
        }
        Fmt::D => {
            put(cx, mc::fmov_fp_to_gpr(Size::X, rd.zr(), v));
            (Size::X, 63)
        }
        Fmt::Q => {
            put(cx, mc::fmov_hi_to_gpr(rd.zr(), v));
            (Size::X, 63)
        }
    }
}

/// Writes the general view in `rs` back to `v` (the counterpart of
/// [`sign_word`]).
fn put_sign_word(cx: &mut FnCx<'_, A64>, f: Fmt, v: VReg, rs: Reg) {
    match f {
        Fmt::H | Fmt::S => put(cx, mc::fmov_gpr_to_fp(Size::W, v, rs.zr())),
        Fmt::D => put(cx, mc::fmov_gpr_to_fp(Size::X, v, rs.zr())),
        Fmt::Q => put(cx, mc::fmov_gpr_to_hi(v, rs.zr())),
    }
}

/// `fabs`: the sign bit cleared, nothing else (NaNs included).
fn fabs<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, f: Fmt, x: Value) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    let r = cx.use_reg(x)?;
    fabs_reg(cx, f, vr(r));
    cx.finish(inst, r)
}

/// `fabs` of the value in `v`, in place. Writes `x16`.
fn fabs_reg(cx: &mut FnCx<'_, A64>, f: Fmt, v: VReg) {
    match f {
        Fmt::S | Fmt::D => put(cx, mc::fabs(f.size(), v, v)),
        Fmt::H | Fmt::Q => {
            let (sz, sign) = sign_word(cx, f, X16, v);
            put(cx, mc::and_imm(sz, X16.sp(), X16.zr(), (1u64 << sign) - 1));
            put_sign_word(cx, f, v, X16);
        }
    }
}

/// `copysign a, b`: the sign bit of `b` with the other bits of `a`.
fn copysign<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    f: Fmt,
    a: Value,
    b: Value,
) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    let ra = cx.use_reg(a)?;
    let rb = cx.use_reg(b)?;
    copysign_reg(cx, f, vr(ra), vr(rb));
    cx.finish(inst, ra)
}

/// `va = copysign(va, vb)`. Writes `x16` and `x17`.
fn copysign_reg(cx: &mut FnCx<'_, A64>, f: Fmt, va: VReg, vb: VReg) {
    let (sz, sign) = sign_word(cx, f, X16, va);
    sign_word(cx, f, X17, vb);
    // x17 keeps its sign bit (and what is above it) and takes the bits
    // below from x16.
    put(cx, mc::bfxil(sz, X17.zr(), X16.zr(), 0, sign));
    put_sign_word(cx, f, va, X17);
}

/// The multiplicands `a` and `b` of `fma`/`fmuladd` in registers and
/// the addend `c` in `v31` (put there last: nothing runs between it and
/// the instruction), so that two vector registers are pinned at most
/// (`Stress::min_pool` has two).
fn addend_last(cx: &mut FnCx<'_, A64>, a: Value, b: Value, c: Value) -> CgResult<(PReg, PReg)> {
    let ra = cx.use_reg(a)?;
    let rb = cx.use_reg(b)?;
    cx.put(c, crate::reg::V31)?;
    Ok((ra, rb))
}

/// `fma`: `fmadd` (`half` in double precision, as `llc`), `fmal`.
fn fma<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, f: Fmt, [a, b, c]: [Value; 3]) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    if f == Fmt::Q {
        let args = [CallArg::Value(a), CallArg::Value(b), CallArg::Value(c)];
        return finish_call(cx, inst, f.routine(FpOp::Fma)?, &args, false);
    }
    let (ra, rb) = addend_last(cx, a, b, c)?;
    fma_reg(cx, f, vr(ra), vr(rb), V31);
    cx.finish(inst, ra)
}

/// `va = fma(va, vb, vc)` of `half`, `float` or `double`.
fn fma_reg(cx: &mut FnCx<'_, A64>, f: Fmt, va: VReg, vb: VReg, vc: VReg) {
    if f == Fmt::H {
        for v in [va, vb, vc] {
            put(cx, mc::fcvt(FpFormat::D, v, FpFormat::H, v));
        }
        put(cx, mc::fmadd(FpSize::D, va, va, vb, vc));
        put(cx, mc::fcvt(FpFormat::H, va, FpFormat::D, va));
    } else {
        put(cx, mc::fmadd(f.size(), va, va, vb, vc));
    }
}

/// `va = fmuladd(va, vb, vc)` of `half` (a rounded `fmul`, then a
/// rounded `fadd`, as `llc -O0`); `float` and `double` are [`fma_reg`].
fn fmuladd_reg(cx: &mut FnCx<'_, A64>, f: Fmt, va: VReg, vb: VReg, vc: VReg) {
    if f != Fmt::H {
        return fma_reg(cx, f, va, vb, vc);
    }
    widen(cx, va);
    widen(cx, vb);
    put(cx, mc::fmul(FpSize::S, va, va, vb));
    narrow(cx, va);
    widen(cx, va);
    widen(cx, vc);
    put(cx, mc::fadd(FpSize::S, va, va, vc));
    narrow(cx, va);
}

/// `fmuladd`, as `llc -O0` makes it: fused (`fmadd`) for `float` and
/// `double`; `half` a `fmul` rounded to `half`, then a `fadd` rounded;
/// `fp128` `__multf3` then `__addtf3`.
fn fmuladd<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    f: Fmt,
    [a, b, c]: [Value; 3],
) -> CgResult<()> {
    match f {
        Fmt::S | Fmt::D => fma(cx, inst, f, [a, b, c]),
        Fmt::H => {
            deferred_first(cx, inst, None)?;
            let (ra, rb) = addend_last(cx, a, b, c)?;
            fmuladd_reg(cx, f, vr(ra), vr(rb), V31);
            cx.finish(inst, ra)
        }
        Fmt::Q => {
            deferred_first(cx, inst, None)?;
            let Some(p) =
                cx.call_runtime(RuntimeFn::MulTf3, &[CallArg::Value(a), CallArg::Value(b)], None)?
            else {
                return cx.bug("a runtime function returned nothing");
            };
            // The product moves from the return register to a pool
            // temporary, which a call argument must be.
            let t = cx.temp(Class::Fpr)?;
            put(cx, mc::orr_vec(VecBits::V128, vr(t), vr(p), vr(p)));
            cx.free_temp(p);
            finish_call(cx, inst, RuntimeFn::AddTf3, &[CallArg::Reg(t), CallArg::Value(c)], false)
        }
    }
}

/// `minnum`/`maxnum` (`fminnm`/`fmaxnm`), `minimum`/`maximum`
/// (`fmin`/`fmax`), `minimumnum`/`maximumnum` (`fminnm`/`fmaxnm` of
/// the operands quieted first, as `llc`: a signalling NaN is then a
/// missing operand like a quiet one).
fn min_max<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    f: Fmt,
    a: Value,
    b: Value,
) -> CgResult<()> {
    use FpOp as O;
    use Intrinsic as I;
    deferred_first(cx, inst, None)?;
    if f == Fmt::Q {
        let func = f.routine(match which {
            I::MinNum => O::MinNum,
            I::MaxNum => O::MaxNum,
            I::Minimum => O::Minimum,
            I::Maximum => O::Maximum,
            I::MinimumNum => O::MinimumNum,
            _ => O::MaximumNum,
        })?;
        return finish_call(cx, inst, func, &[CallArg::Value(a), CallArg::Value(b)], false);
    }
    let ra = cx.use_reg(a)?;
    let rb = cx.use_reg(b)?;
    min_max_reg(cx, which, f, vr(ra), vr(rb));
    cx.finish(inst, ra)
}

/// `va = min|max(va, vb)` of `half`, `float` or `double`, for each of
/// the six intrinsics.
fn min_max_reg(cx: &mut FnCx<'_, A64>, which: Intrinsic<'_>, f: Fmt, va: VReg, vb: VReg) {
    use Intrinsic as I;
    let (op, quiet): (Op3, bool) = match which {
        I::MinNum => (mc::fminnm, false),
        I::MaxNum => (mc::fmaxnm, false),
        I::Minimum => (mc::fmin, false),
        I::Maximum => (mc::fmax, false),
        I::MinimumNum => (mc::fminnm, true),
        _ => (mc::fmaxnm, true),
    };
    let sz = f.size();
    if f == Fmt::H {
        // The widening quiets a signalling NaN.
        widen(cx, va);
        widen(cx, vb);
    } else if quiet {
        put(cx, canon(sz, va, va));
        put(cx, canon(sz, vb, vb));
    }
    put(cx, op(sz, va, va, vb));
    if f == Fmt::H {
        narrow(cx, va);
    }
}

/// `lround`/`llround` (`fcvtas`) and `lrint`/`llrint` (`frintx` +
/// `fcvtzs`) to an integer of 1 to 64 bits; `fp128` calls `lroundl`,
/// `llroundl`, `lrintl`, `llrintl`. An out-of-range result is
/// unspecified (`LangRef`), an in-range one exact at any width.
fn to_int<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    f: Fmt,
    x: Value,
) -> CgResult<()> {
    use Intrinsic as I;
    let bits = int_result(cx, inst)?;
    deferred_first(cx, inst, None)?;
    if f == Fmt::Q {
        let func = f.routine(match which {
            I::Lround => FpOp::Lround,
            I::Llround => FpOp::Llround,
            I::Lrint => FpOp::Lrint,
            _ => FpOp::Llrint,
        })?;
        return finish_call(cx, inst, func, &[CallArg::Value(x)], false);
    }
    let a = cx.use_reg(x)?;
    let rd = cx.def_reg(inst)?;
    to_int_reg(cx, which, f, vr(a), xr(rd), bits);
    cx.finish(inst, rd)
}

/// `rd = lround|llround|lrint|llrint(v)` to `bits` bits. Writes `v`.
fn to_int_reg(cx: &mut FnCx<'_, A64>, which: Intrinsic<'_>, f: Fmt, v: VReg, rd: Reg, bits: u32) {
    use Intrinsic as I;
    if f == Fmt::H {
        widen(cx, v);
    }
    let sz = size_for_bits(bits);
    if matches!(which, I::Lround | I::Llround) {
        put(cx, mc::fcvtas(sz, rd.zr(), f.size(), v));
    } else {
        put(cx, mc::frintx(f.size(), v, v));
        put(cx, mc::fcvtzs(sz, rd.zr(), f.size(), v));
    }
}

/// `fptosi.sat`/`fptoui.sat` to `i1`..`i64`: `fcvtzs`/`fcvtzu` at the
/// register width saturate and give 0 for a NaN; a width other than 32
/// and 64 is clamped to its range by `cmp` + `csel` (as `llc`).
fn saturate<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    signed: bool,
    f: Fmt,
    x: Value,
) -> CgResult<()> {
    let bits = int_result(cx, inst)?;
    if f == Fmt::Q {
        return unsupported(FP128);
    }
    deferred_first(cx, inst, None)?;
    let a = cx.use_reg(x)?;
    let rd = cx.def_reg(inst)?;
    saturate_reg(cx, signed, f, vr(a), xr(rd), bits);
    cx.finish(inst, rd)
}

/// `rd = fptosi.sat|fptoui.sat(v)` (`signed`) to `bits` bits. Writes `v`
/// and `x16`.
fn saturate_reg(cx: &mut FnCx<'_, A64>, signed: bool, f: Fmt, v: VReg, rd: Reg, bits: u32) {
    if f == Fmt::H {
        widen(cx, v);
    }
    let r = rd.zr();
    let sz = size_for_bits(bits);
    let word = if signed { mc::fcvtzs } else { mc::fcvtzu };
    put(cx, word(sz, r, f.size(), v));
    if bits != 32 && bits != 64 {
        let reg_mask = if sz == Size::W { u64::from(u32::MAX) } else { u64::MAX };
        if signed {
            let max = (1u64 << (bits - 1)) - 1;
            let min = !max & reg_mask;
            mov_imm(cx, sz, X16, max);
            put(cx, mc::cmp(sz, r, X16.zr()));
            put(cx, mc::csel(sz, r, r, X16.zr(), Cond::Lt));
            mov_imm(cx, sz, X16, min);
            put(cx, mc::cmp(sz, r, X16.zr()));
            put(cx, mc::csel(sz, r, r, X16.zr(), Cond::Gt));
        } else {
            mov_imm(cx, sz, X16, (1u64 << bits) - 1);
            put(cx, mc::cmp(sz, r, X16.zr()));
            put(cx, mc::csel(sz, r, r, X16.zr(), Cond::Lo));
        }
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

/// The exponent and fraction widths of a format; for `fp128` those of
/// its upper half (15 and 48: the lower 64 fraction bits are folded
/// into bit 0 of it, which none of the bounds uses).
fn fields(f: Fmt) -> (u32, u32, u32) {
    match f {
        Fmt::H => (16, 5, 10),
        Fmt::S => (32, 8, 23),
        Fmt::D => (64, 11, 52),
        Fmt::Q => (64, 15, 48),
    }
}

/// `is.fpclass x, mask` (`mask` a constant): the bits moved to `x16`
/// with the sign at bit 63 (an `fp128`'s low half folded into bit 0 of
/// its high half, a sticky bit), the magnitude `x17 = x16 << 1` counted
/// against the bounds into a kind 0..5, then bit `kind` of the table of
/// the sign ([`class_tables`]).
fn fp_class<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    f: Fmt,
    x: Value,
    mask: Value,
) -> CgResult<()> {
    let Some(mask) = crate::intrinsic::const_int(cx, mask) else {
        return unsupported(INTRINSIC);
    };
    let mask = mask & 0x3ff;
    let bits = int_result(cx, inst)?;
    let _ = bits;
    deferred_first(cx, inst, None)?;
    let rd = cx.def_reg(inst)?;
    let r = xr(rd);
    if mask == 0 || mask == 0x3ff {
        mov_imm(cx, Size::W, r, u64::from(mask != 0));
        return cx.finish(inst, rd);
    }
    let a = cx.use_reg(x)?;
    let v = vr(a);
    let (width, exp, frac) = fields(f);
    match f {
        Fmt::H | Fmt::S => {
            put(cx, mc::fmov_fp_to_gpr(Size::W, X16.zr(), v));
            put(cx, mc::lsl_imm(Size::X, X16.zr(), X16.zr(), 64 - width));
        }
        Fmt::D => put(cx, mc::fmov_fp_to_gpr(Size::X, X16.zr(), v)),
        Fmt::Q => {
            put(cx, mc::fmov_fp_to_gpr(Size::X, X17.zr(), v));
            put(cx, mc::cmp_imm(Size::X, X17.sp(), 0));
            put(cx, mc::cset(Size::X, X17.zr(), Cond::Ne));
            put(cx, mc::fmov_hi_to_gpr(X16.zr(), v));
            put(cx, mc::orr(Size::X, X16.zr(), X16.zr(), X17.zr()));
        }
    }
    let up = 65 - width;
    let inf = ((1u64 << exp) - 1) << (frac + up);
    let min_normal = 1u64 << (frac + up);
    let quiet = inf | (1u64 << (frac - 1 + up));
    let t = xr(cx.temp(Class::Gpr)?);
    // r = kind: the bounds passed.
    put(cx, mc::lsl_imm(Size::X, X17.zr(), X16.zr(), 1));
    put(cx, mc::cmp_imm(Size::X, X17.sp(), 0));
    put(cx, mc::cset(Size::X, r.zr(), Cond::Ne));
    for (bound, cond) in
        [(min_normal, Cond::Hs), (inf, Cond::Hs), (inf, Cond::Hi), (quiet, Cond::Hs)]
    {
        mov_imm(cx, Size::X, t, bound);
        put(cx, mc::cmp(Size::X, X17.zr(), t.zr()));
        put(cx, mc::cinc(Size::X, r.zr(), r, cond));
    }
    let (positive, negative) = class_tables(mask);
    mov_imm(cx, Size::X, t, positive);
    mov_imm(cx, Size::X, X17, negative);
    put(cx, mc::cmp_imm(Size::X, X16.sp(), 0));
    put(cx, mc::csel(Size::X, t.zr(), X17.zr(), t.zr(), Cond::Lt));
    put(cx, mc::lsrv(Size::X, r.zr(), t.zr(), r.zr()));
    put(cx, mc::and_imm(Size::W, r.sp(), r.zr(), 1));
    cx.finish(inst, rd)
}

// ---- calls ----

/// Calls `func` with `args` and finishes `inst` with its result, first
/// narrowed from single precision to `half` if `narrow_half`.
fn finish_call<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    func: RuntimeFn,
    args: &[CallArg],
    narrow_half: bool,
) -> CgResult<()> {
    let Some(r) = cx.call_runtime(func, args, None)? else {
        return cx.bug("a runtime function returned nothing");
    };
    if narrow_half {
        narrow(cx, vr(r));
    }
    cx.finish(inst, r)
}

/// A floating-point argument of a call: as it is, or for `half` widened
/// to `float` in a temporary (through `v31`, so that one register is
/// pinned besides the operand's).
fn fp_arg(cx: &mut FnCx<'_, A64>, f: Fmt, x: Value) -> CgResult<CallArg> {
    if f != Fmt::H {
        return Ok(CallArg::Value(x));
    }
    let a = cx.use_reg(x)?;
    put(cx, mc::fcvt(FpFormat::S, V31, FpFormat::H, vr(a)));
    cx.release(x);
    cx.free_temp(a);
    let t = cx.temp(Class::Fpr)?;
    put(cx, mc::fmov(FpSize::S, vr(t), V31));
    Ok(CallArg::Reg(t))
}

/// An `int` argument of a call: as it is when it is 32 bits wide, else
/// sign-extended (or truncated) to 32 bits in a temporary.
fn int_arg(cx: &mut FnCx<'_, A64>, n: Value, clamp: bool) -> CgResult<CallArg> {
    let bits = int_arg_bits(cx.types().int_bits(value_ty(cx, n)), clamp)?;
    if bits == 32 {
        return Ok(CallArg::Value(n));
    }
    let a = cx.use_reg(n)?;
    let t = cx.temp(Class::Gpr)?;
    int_arg_reg(cx, xr(t), xr(a), bits);
    cx.release(n);
    cx.free_temp(a);
    Ok(CallArg::Reg(t))
}

/// The width of an `int` argument of `bits` bits that [`int_arg_reg`]
/// takes, or the refusal.
fn int_arg_bits(bits: Option<u32>, clamp: bool) -> CgResult<u32> {
    match bits {
        Some(bits @ 1..=32) => Ok(bits),
        Some(bits @ 33..=64) if clamp => Ok(bits),
        // `powi`'s exponent is the C `int` of `__powidf2`; a wider one
        // has no call (as `llc`).
        Some(1..=64) => unsupported(INTRINSIC),
        _ => unsupported(WIDE),
    }
}

/// `t` = the `int` argument made of the `bits`-bit integer in `a` (`bits`
/// as [`int_arg_bits`] accepts it): sign-extended to 32 bits, or a wider
/// exponent of `ldexp` clamped to the `int` range (beyond it every result
/// is already 0, infinite or `x`). Writes `x16`.
fn int_arg_reg(cx: &mut FnCx<'_, A64>, t: Reg, a: Reg, bits: u32) {
    let tz = t.zr();
    match bits {
        32 if t == a => {}
        32 => put(cx, mc::mov(Size::W, tz, a.zr())),
        1..=31 => put(cx, mc::sbfx(Size::W, tz, a.zr(), 0, bits)),
        _ => {
            put(cx, mc::sbfx(Size::X, tz, a.zr(), 0, bits));
            mov_imm(cx, Size::X, X16, 0x7fff_ffff);
            put(cx, mc::cmp(Size::X, tz, X16.zr()));
            put(cx, mc::csel(Size::X, tz, tz, X16.zr(), Cond::Lt));
            mov_imm(cx, Size::X, X16, 0xffff_ffff_8000_0000);
            put(cx, mc::cmp(Size::X, tz, X16.zr()));
            put(cx, mc::csel(Size::X, tz, tz, X16.zr(), Cond::Gt));
        }
    }
}

/// `inst` as a call of `func` with the floating-point operands `fps`
/// (one or two) and the `int` operand `n`; `half` through the `float`
/// function.
fn call_math<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    f: Fmt,
    func: RuntimeFn,
    fps: &[Value],
    n: Option<Value>,
) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    let mut list = [CallArg::Value(Value::NONE); 3];
    let mut count = 0;
    for &x in fps {
        list[count] = fp_arg(cx, f, x)?;
        count += 1;
    }
    if let Some(n) = n {
        let clamp = matches!(func, RuntimeFn::Ldexpf | RuntimeFn::Ldexp | RuntimeFn::Ldexpl);
        list[count] = int_arg(cx, n, clamp)?;
        count += 1;
    }
    finish_call(cx, inst, func, &list[..count], f == Fmt::H)
}

/// `frexp x` into its `{ T, i32 }` result (class `Mem`): `frexpf`,
/// `frexp`, `frexpl` with the exponent written to a frame slot, then
/// both fields written to the result's slot.
fn frexp<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, f: Fmt, x: Value) -> CgResult<()> {
    let ty = inst.ty();
    let fields = cx.types().struct_fields(ty);
    if fields.map(<[TypeId]>::len) != Some(2)
        || fields.is_some_and(|fs| cx.types().int_bits(fs[1]) != Some(32))
    {
        return unsupported(INTRINSIC);
    }
    let exp_at = cx.types().field_offset(ty, 1) as u32;
    deferred_first(cx, inst, None)?;
    let func = f.routine(FpOp::Frexp)?;
    let slot = cx.alloc_slot(4, 4)?;
    let arg = fp_arg(cx, f, x)?;
    let Some(r) = cx.call_runtime(func, &[arg, CallArg::Addr(slot)], None)? else {
        return cx.bug("a runtime function returned nothing");
    };
    if f == Fmt::H {
        narrow(cx, vr(r));
    }
    let dst = cx.def_mem(inst)?;
    let mut at = Cursor::frame(cx, dst, X16);
    store_fpr_at(cx, f.view(), vr(r), &mut at);
    let mut from = Cursor::frame(cx, slot, X16);
    load_gpr_at(cx, hod_mc_a64::Load::W, X17, &mut from);
    let mut at = Cursor::frame(cx, dst, X16);
    at.advance(exp_at);
    store_gpr_at(cx, hod_mc_a64::MemSize::W, X17.zr(), &mut at);
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

    /// The bounds of `is.fpclass` in the shifted layout.
    #[test]
    fn fpclass_bounds() {
        let (width, exp, frac) = fields(Fmt::H);
        let up = 65 - width;
        assert_eq!(((1u64 << exp) - 1) << (frac + up), 0xf800_0000_0000_0000);
        let (width, exp, frac) = fields(Fmt::D);
        assert_eq!(((1u64 << exp) - 1) << (frac + 65 - width), 0xffe0_0000_0000_0000);
    }
}
