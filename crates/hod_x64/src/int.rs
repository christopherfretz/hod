//! Integer binary operations, casts, `icmp`, `select` and `freeze` of
//! class `Gpr`: WP-122 (X-int). Every entry point refuses as [`INT`]
//! until then.
//!
//! What WP-122 finds in `common`: [`mov_imm`](crate::common::mov_imm),
//! [`materialize_gpr`](crate::common::materialize_gpr), the scratch
//! registers, and the two-operand rule of `spec/codegen.md` §5.6.3:
//! `rax`, `rcx`, `rdx` are free to write (division, `cl` shifts), and
//! only flag-preserving glue may come between a compare and its reader.

use hod_cg::{CgResult, FnCx};
use hod_ir::{Inst, IntPredicate, Value};

use crate::common::unsupported;
use crate::target::X64;

/// The group's refusal until WP-122.
pub const INT: &str = "x64:todo:int";

/// An integer binary operation.
///
/// # Errors
/// [`INT`].
pub fn binary<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, lhs: Value, rhs: Value) -> CgResult<()> {
    let _ = (cx, inst, lhs, rhs);
    unsupported(INT)
}

/// `freeze` of a `Gpr` value.
///
/// # Errors
/// [`INT`].
pub fn freeze<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let _ = (cx, inst, arg);
    unsupported(INT)
}

/// A cast between two `Gpr` types.
///
/// # Errors
/// [`INT`].
pub fn cast<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let _ = (cx, inst, arg);
    unsupported(INT)
}

/// `icmp` of `Gpr` operands.
///
/// # Errors
/// [`INT`].
pub fn icmp<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    pred: IntPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    let _ = (cx, inst, pred, lhs, rhs);
    unsupported(INT)
}

/// `select` of a `Gpr` value.
///
/// # Errors
/// [`INT`].
pub fn select<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    cond: Value,
    if_true: Value,
    if_false: Value,
) -> CgResult<()> {
    let _ = (cx, inst, cond, if_true, if_false);
    unsupported(INT)
}
