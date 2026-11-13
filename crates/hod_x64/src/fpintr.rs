//! The floating-point intrinsics (`llvm.fabs`, `llvm.sqrt`, rounding,
//! `llvm.minnum` and friends, the libm functions): WP-127/WP-128.
//! Refuses as [`FPINTR`] until then. [`is_fp`] is the set the dispatch
//! sends here, the same as AArch64's.

use hod_cg::{CgResult, FnCx};
use hod_ir::{Inst, Intrinsic, Value};

use crate::common::unsupported;
use crate::target::X64;

/// The group's refusal until WP-127/WP-128.
pub const FPINTR: &str = "x64:todo:fpintr";

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
            | I::Sin
            | I::Cos
            | I::Tan
            | I::Asin
            | I::Acos
            | I::Atan
            | I::Atan2
            | I::Sinh
            | I::Cosh
            | I::Tanh
            | I::Exp
            | I::Exp2
            | I::Exp10
            | I::Log
            | I::Log2
            | I::Log10
    )
}

/// A call of floating-point intrinsic `which`.
///
/// # Errors
/// [`FPINTR`].
pub fn intrinsic<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    let _ = (cx, inst, which, args);
    unsupported(FPINTR)
}
