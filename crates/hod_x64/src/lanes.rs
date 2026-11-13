//! Element-wise operations on vectors of class `Mem` (wider than 128
//! bits, or of an odd shape), one lane at a time over the scalar kernels:
//! WP-132 (X-vector). Refuses as [`LANES`] until then.

use hod_cg::{CgResult, FnCx};
use hod_ir::{Inst, Value};

use crate::common::unsupported;
use crate::target::X64;

/// The group's refusal until WP-132.
pub const LANES: &str = "x64:todo:lanes";

/// A binary operation on a `Mem`-class vector.
///
/// # Errors
/// [`LANES`].
pub fn binary<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, lhs: Value, rhs: Value) -> CgResult<()> {
    let _ = (cx, inst, lhs, rhs);
    unsupported(LANES)
}

/// `fneg` of a `Mem`-class vector.
///
/// # Errors
/// [`LANES`].
pub fn fneg<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let _ = (cx, inst, arg);
    unsupported(LANES)
}

/// A cast to a `Mem`-class vector.
///
/// # Errors
/// [`LANES`].
pub fn cast<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let _ = (cx, inst, arg);
    unsupported(LANES)
}
