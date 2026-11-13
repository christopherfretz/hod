//! `Mem`-class values: integers wider than 64 bits, aggregates, `Mem`
//! constants, the `*.with.overflow` intrinsics: WP-126 (X-aggr). Every
//! entry point refuses as [`AGGR`] until then.
//!
//! What WP-126 finds in `common`: [`copy_frame`](crate::common::copy_frame),
//! [`copy_from_ptr`](crate::common::copy_from_ptr),
//! [`frame_inst`](crate::common::frame_inst) (every access to a local goes
//! through it), [`pieces`](crate::common::pieces).

use hod_cg::{CgResult, FnCx, FrameRef};
use hod_ir::{Inst, IntPredicate, Intrinsic, MemFlags, Value};

use crate::common::unsupported;
use crate::target::X64;

/// The group's refusal until WP-126.
pub const AGGR: &str = "x64:todo:aggr";

/// A binary operation of class `Mem` that is not a vector's or an
/// `x86_fp80`'s: a wide integer.
///
/// # Errors
/// [`AGGR`].
pub fn binary<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, lhs: Value, rhs: Value) -> CgResult<()> {
    let _ = (cx, inst, lhs, rhs);
    unsupported(AGGR)
}

/// `freeze` of class `Mem`.
///
/// # Errors
/// [`AGGR`].
pub fn freeze<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let _ = (cx, inst, arg);
    unsupported(AGGR)
}

/// A cast with a `Mem` side that is not a vector's or an `x86_fp80`'s.
///
/// # Errors
/// [`AGGR`].
pub fn cast<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let _ = (cx, inst, arg);
    unsupported(AGGR)
}

/// `icmp` of `Mem` operands.
///
/// # Errors
/// [`AGGR`].
pub fn icmp<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    pred: IntPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    let _ = (cx, inst, pred, lhs, rhs);
    unsupported(AGGR)
}

/// A non-atomic `load` of a `Mem` value.
///
/// # Errors
/// [`AGGR`].
pub fn load<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, ptr: Value, mem: MemFlags) -> CgResult<()> {
    let _ = (cx, inst, ptr, mem);
    unsupported(AGGR)
}

/// A non-atomic `store` of a `Mem` value.
///
/// # Errors
/// [`AGGR`].
pub fn store<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    value: Value,
    ptr: Value,
    mem: MemFlags,
) -> CgResult<()> {
    let _ = (cx, inst, value, ptr, mem);
    unsupported(AGGR)
}

/// `extractvalue`.
///
/// # Errors
/// [`AGGR`].
pub fn extract_value<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    agg: Value,
    offset: u64,
) -> CgResult<()> {
    let _ = (cx, inst, agg, offset);
    unsupported(AGGR)
}

/// `insertvalue`.
///
/// # Errors
/// [`AGGR`].
pub fn insert_value<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    agg: Value,
    value: Value,
    offset: u64,
) -> CgResult<()> {
    let _ = (cx, inst, agg, value, offset);
    unsupported(AGGR)
}

/// `select` of a `Mem` value.
///
/// # Errors
/// [`AGGR`].
pub fn select<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    cond: Value,
    if_true: Value,
    if_false: Value,
) -> CgResult<()> {
    let _ = (cx, inst, cond, if_true, if_false);
    unsupported(AGGR)
}

/// `llvm.{s,u}{add,sub,mul}.with.overflow`.
///
/// # Errors
/// [`AGGR`].
pub fn overflow_intrinsic<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    let _ = (cx, inst, which, args);
    unsupported(AGGR)
}

/// Glue (`Target::materialize_mem`): the in-memory image of a `Mem`
/// constant at `dst`.
///
/// # Errors
/// [`AGGR`].
pub fn materialize_mem(cx: &mut FnCx<'_, X64>, dst: FrameRef, value: Value) -> CgResult<()> {
    let _ = (cx, dst, value);
    unsupported(AGGR)
}
