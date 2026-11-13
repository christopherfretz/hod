//! Atomic `load` and `store`, `atomicrmw`, `cmpxchg`, `fence`: WP-133
//! (X-atomic; design 0006 §2.7, §10.2). Refuses as [`ATOMIC`] until then.

use hod_cg::{CgResult, FnCx};
use hod_ir::{AtomicOrdering, Inst, MemFlags, RmwOp, SyncScope, Value};

use crate::common::unsupported;
use crate::target::X64;

/// The group's refusal until WP-133.
pub const ATOMIC: &str = "x64:todo:atomic";

/// An atomic `load`.
///
/// # Errors
/// [`ATOMIC`].
pub fn load<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, ptr: Value, mem: MemFlags) -> CgResult<()> {
    let _ = (cx, inst, ptr, mem);
    unsupported(ATOMIC)
}

/// An atomic `store`.
///
/// # Errors
/// [`ATOMIC`].
pub fn store<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    value: Value,
    ptr: Value,
    mem: MemFlags,
) -> CgResult<()> {
    let _ = (cx, inst, value, ptr, mem);
    unsupported(ATOMIC)
}

/// `fence`.
///
/// # Errors
/// [`ATOMIC`].
pub fn fence(cx: &mut FnCx<'_, X64>, ordering: AtomicOrdering, scope: SyncScope) -> CgResult<()> {
    let _ = (cx, ordering, scope);
    unsupported(ATOMIC)
}

/// `cmpxchg`.
///
/// # Errors
/// [`ATOMIC`].
#[allow(
    clippy::too_many_arguments,
    reason = "the instruction's operands, as the dispatch has them"
)]
pub fn cmpxchg<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    ptr: Value,
    expected: Value,
    replacement: Value,
    mem: MemFlags,
    failure: AtomicOrdering,
    weak: bool,
) -> CgResult<()> {
    let _ = (cx, inst, ptr, expected, replacement, mem, failure, weak);
    unsupported(ATOMIC)
}

/// `atomicrmw`.
///
/// # Errors
/// [`ATOMIC`].
pub fn rmw<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    op: RmwOp,
    ptr: Value,
    value: Value,
    mem: MemFlags,
) -> CgResult<()> {
    let _ = (cx, inst, op, ptr, value, mem);
    unsupported(ATOMIC)
}
