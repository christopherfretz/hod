//! Instructions on values of class `Fpr` (`spec/codegen.md` §2.2):
//! floating-point binary operations, `fneg`, `freeze`, casts with a
//! floating-point or vector side, `fcmp`, `icmp` of vectors, `select`.
//!
//! **Owner: WP-039.** Tests: `tests/float.rs`. Until the package lands each
//! entry point refuses with its stable name below; the dispatch in
//! `target.rs` is final. An entry point starts with
//! [`deferred_first`](crate::common::deferred_first).

use hod_cg::{CgResult, FnCx};
use hod_ir::{FloatPredicate, Inst, IntPredicate, Value};

use crate::common::unsupported;
use crate::target::A64;

/// A binary operation on vector-register values.
pub const BINARY: &str = "a64:fp-binary";
/// `fneg`.
pub const FNEG: &str = "a64:fneg";
/// `freeze` of a vector-register value.
pub const FREEZE: &str = "a64:fp-freeze";
/// A cast with a vector-register side.
pub const CAST: &str = "a64:fp-cast";
/// `fcmp`.
pub const FCMP: &str = "a64:fcmp";
/// `icmp` of vector-register values.
pub const VECTOR_ICMP: &str = "a64:vector-icmp";
/// `select` of vector-register values.
pub const SELECT: &str = "a64:fp-select";

/// `%r = <op> %lhs, %rhs` of class `Fpr`.
pub fn binary<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, lhs: Value, rhs: Value) -> CgResult<()> {
    let _ = (cx, inst, lhs, rhs);
    unsupported(BINARY)
}

/// `%r = fneg %arg`.
pub fn fneg<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let _ = (cx, inst, arg);
    unsupported(FNEG)
}

/// `%r = freeze %arg` of class `Fpr`.
pub fn freeze<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let _ = (cx, inst, arg);
    unsupported(FREEZE)
}

/// `%r = <cast> %arg` with a side of class `Fpr` and none of class `Mem`.
pub fn cast<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let _ = (cx, inst, arg);
    unsupported(CAST)
}

/// `%r = fcmp <pred> %lhs, %rhs`.
pub fn fcmp<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    pred: FloatPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    let _ = (cx, inst, pred, lhs, rhs);
    unsupported(FCMP)
}

/// `%r = icmp <pred> %lhs, %rhs` of vectors in vector registers.
pub fn vector_icmp<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    pred: IntPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    let _ = (cx, inst, pred, lhs, rhs);
    unsupported(VECTOR_ICMP)
}

/// `%r = select i1 %cond, %if_true, %if_false` of class `Fpr`.
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
