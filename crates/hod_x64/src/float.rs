//! Floating-point and vector-register (`Fpr`, SSE2) instructions:
//! WP-127 (X-float). Every entry point refuses as [`FLOAT`] until then.
//!
//! What WP-127 finds in `common`: [`load_fpr`](crate::common::load_fpr),
//! [`store_fpr`](crate::common::store_fpr), `xmm14`/`xmm15` as scratch.
//! Constants are built through a general register (`movabs` + `movq`),
//! no island, except vectors and `fp128` (design 0006 §2.6, §2.10).

use hod_cg::{CgResult, FnCx};
use hod_ir::{FloatPredicate, Inst, IntPredicate, Value};
use hod_mc_x64::Xmm;

use crate::common::unsupported;
use crate::target::X64;

/// The group's refusal until WP-127.
pub const FLOAT: &str = "x64:todo:float";

/// A binary operation of class `Fpr`.
///
/// # Errors
/// [`FLOAT`].
pub fn binary<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, lhs: Value, rhs: Value) -> CgResult<()> {
    let _ = (cx, inst, lhs, rhs);
    unsupported(FLOAT)
}

/// `fneg` of class `Fpr`.
///
/// # Errors
/// [`FLOAT`].
pub fn fneg<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let _ = (cx, inst, arg);
    unsupported(FLOAT)
}

/// `freeze` of class `Fpr`.
///
/// # Errors
/// [`FLOAT`].
pub fn freeze<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let _ = (cx, inst, arg);
    unsupported(FLOAT)
}

/// A cast with an `Fpr` side (the other `Gpr` or `Fpr`).
///
/// # Errors
/// [`FLOAT`].
pub fn cast<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let _ = (cx, inst, arg);
    unsupported(FLOAT)
}

/// `fcmp` of `Fpr` operands.
///
/// # Errors
/// [`FLOAT`].
pub fn fcmp<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    pred: FloatPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    let _ = (cx, inst, pred, lhs, rhs);
    unsupported(FLOAT)
}

/// `icmp` of `Fpr` (vector) operands.
///
/// # Errors
/// [`FLOAT`].
pub fn vector_icmp<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    pred: IntPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    let _ = (cx, inst, pred, lhs, rhs);
    unsupported(FLOAT)
}

/// `select` of an `Fpr` value.
///
/// # Errors
/// [`FLOAT`].
pub fn select<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    cond: Value,
    if_true: Value,
    if_false: Value,
) -> CgResult<()> {
    let _ = (cx, inst, cond, if_true, if_false);
    unsupported(FLOAT)
}

/// Glue (`Target::materialize` for a vector register): a constant of
/// class `Fpr` in `rd`.
///
/// # Errors
/// [`FLOAT`].
pub fn materialize(cx: &mut FnCx<'_, X64>, rd: Xmm, value: Value) -> CgResult<()> {
    let _ = (cx, rd, value);
    unsupported(FLOAT)
}
