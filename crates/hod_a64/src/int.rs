//! Integer instructions of class `Gpr` (`spec/codegen.md` §2.2): binary
//! operations, `freeze`, integer and pointer casts, `icmp`, `select`.
//!
//! **Owner: WP-035.** Tests: `tests/int.rs`. Until the package lands each
//! entry point refuses with its stable name below; the dispatch in
//! `target.rs` is final. An entry point starts with
//! [`deferred_first`](crate::common::deferred_first), naming the operand
//! it fuses (an `icmp` that may be deferred fuses nothing and defers
//! itself; see `hod_cg::FnCx::may_defer`).

use hod_cg::{CgResult, FnCx};
use hod_ir::{Inst, IntPredicate, Value};

use crate::common::unsupported;
use crate::target::A64;

/// A binary operation on integers or pointers in general registers.
pub const BINARY: &str = "a64:int-binary";
/// `freeze` of a general-register value.
pub const FREEZE: &str = "a64:int-freeze";
/// A cast between general-register types.
pub const CAST: &str = "a64:int-cast";
/// `icmp` of general-register values.
pub const ICMP: &str = "a64:icmp";
/// `select` of general-register values.
pub const SELECT: &str = "a64:int-select";

/// `%r = <op> %lhs, %rhs`.
pub fn binary<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, lhs: Value, rhs: Value) -> CgResult<()> {
    let _ = (cx, inst, lhs, rhs);
    unsupported(BINARY)
}

/// `%r = freeze %arg`.
pub fn freeze<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let _ = (cx, inst, arg);
    unsupported(FREEZE)
}

/// `%r = <cast> %arg` with both sides of class `Gpr`.
pub fn cast<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let _ = (cx, inst, arg);
    unsupported(CAST)
}

/// `%r = icmp <pred> %lhs, %rhs`.
pub fn icmp<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    pred: IntPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    let _ = (cx, inst, pred, lhs, rhs);
    unsupported(ICMP)
}

/// `%r = select i1 %cond, %if_true, %if_false` of class `Gpr`.
pub fn select<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    cond: Value,
    if_true: Value,
    if_false: Value,
) -> CgResult<()> {
    let _ = (cx, inst, cond, if_true, if_false);
    unsupported(SELECT)
}
