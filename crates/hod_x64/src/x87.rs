//! `x86_fp80` arithmetic, `fneg`, casts and `fcmp`: WP-131 (X-x87).
//! `x86_fp80` is class `Mem` (a 16-byte slot, 10 bytes used); every
//! operation loads its operands onto the x87 stack, operates and stores,
//! so no x87 register lives across a lowering (design 0006 §2.6). Its
//! loads, stores, `select`s and `phi`s are `Mem`-class copies. Refuses as
//! [`X87`] until then.

use hod_cg::{CgResult, FnCx};
use hod_ir::{FloatPredicate, Inst, Value};

use crate::common::unsupported;
use crate::target::X64;

/// The group's refusal until WP-131.
pub const X87: &str = "x64:todo:x87";

/// A binary operation on `x86_fp80`.
///
/// # Errors
/// [`X87`].
pub fn binary<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, lhs: Value, rhs: Value) -> CgResult<()> {
    let _ = (cx, inst, lhs, rhs);
    unsupported(X87)
}

/// `fneg` of an `x86_fp80`.
///
/// # Errors
/// [`X87`].
pub fn fneg<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let _ = (cx, inst, arg);
    unsupported(X87)
}

/// A cast from or to `x86_fp80`.
///
/// # Errors
/// [`X87`].
pub fn cast<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let _ = (cx, inst, arg);
    unsupported(X87)
}

/// `fcmp` of `x86_fp80` operands.
///
/// # Errors
/// [`X87`].
pub fn fcmp<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    pred: FloatPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    let _ = (cx, inst, pred, lhs, rhs);
    unsupported(X87)
}
