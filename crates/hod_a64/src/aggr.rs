//! Instructions on values of class `Mem` (`spec/codegen.md` §2.2): wide
//! integers, first-class aggregates and wide vectors, slot to slot —
//! binary operations, `freeze`, casts, `icmp`, `load`, `store`, `select`,
//! `extractvalue`, `insertvalue`, and the `*.with.overflow` intrinsics,
//! whose result is a `{iN, i1}`.
//!
//! **Owner: WP-040.** Tests: `tests/aggr.rs`. Until the package lands each
//! entry point refuses with its stable name below; the dispatch in
//! `target.rs` is final. Frame-to-frame copies are
//! `crate::common::copy_bytes`; `Mem`-class constants are written by
//! `crate::common::materialize_mem_const` (WP-033).

use hod_cg::{CgResult, FnCx};
use hod_ir::{Inst, IntPredicate, Intrinsic, MemFlags, Value};

use crate::common::unsupported;
use crate::target::A64;

/// A binary operation on `Mem`-class values (`i128` and wider).
pub const BINARY: &str = "a64:mem-binary";
/// `freeze` of a `Mem`-class value.
pub const FREEZE: &str = "a64:mem-freeze";
/// A cast with a `Mem`-class side.
pub const CAST: &str = "a64:mem-cast";
/// `icmp` of `Mem`-class values.
pub const ICMP: &str = "a64:mem-icmp";
/// `load` of a `Mem`-class value.
pub const LOAD: &str = "a64:mem-load";
/// `store` of a `Mem`-class value.
pub const STORE: &str = "a64:mem-store";
/// `select` of `Mem`-class values.
pub const SELECT: &str = "a64:mem-select";
/// `extractvalue`.
pub const EXTRACT_VALUE: &str = "a64:extractvalue";
/// `insertvalue`.
pub const INSERT_VALUE: &str = "a64:insertvalue";
/// `llvm.{s,u}{add,sub,mul}.with.overflow`.
pub const OVERFLOW: &str = "a64:overflow-intrinsic";

/// `%r = <op> %lhs, %rhs` of class `Mem`.
pub fn binary<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, lhs: Value, rhs: Value) -> CgResult<()> {
    let _ = (cx, inst, lhs, rhs);
    unsupported(BINARY)
}

/// `%r = freeze %arg` of class `Mem`.
pub fn freeze<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let _ = (cx, inst, arg);
    unsupported(FREEZE)
}

/// `%r = <cast> %arg` with a side of class `Mem`.
pub fn cast<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let _ = (cx, inst, arg);
    unsupported(CAST)
}

/// `%r = icmp <pred> %lhs, %rhs` of `Mem`-class operands.
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

/// `%r = load <ty>, ptr %ptr` of class `Mem`.
pub fn load<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, ptr: Value, mem: MemFlags) -> CgResult<()> {
    let _ = (cx, inst, ptr, mem);
    unsupported(LOAD)
}

/// `store <ty> %value, ptr %ptr` of class `Mem`.
pub fn store<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    value: Value,
    ptr: Value,
    mem: MemFlags,
) -> CgResult<()> {
    let _ = (cx, inst, value, ptr, mem);
    unsupported(STORE)
}

/// `%r = select i1 %cond, %if_true, %if_false` of class `Mem`.
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

/// `%r = extractvalue %agg, <offset>` (the field at byte `offset`).
pub fn extract_value<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    agg: Value,
    offset: u64,
) -> CgResult<()> {
    let _ = (cx, inst, agg, offset);
    unsupported(EXTRACT_VALUE)
}

/// `%r = insertvalue %agg, %value, <offset>`.
pub fn insert_value<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    agg: Value,
    value: Value,
    offset: u64,
) -> CgResult<()> {
    let _ = (cx, inst, agg, value, offset);
    unsupported(INSERT_VALUE)
}

/// `llvm.{s,u}{add,sub,mul}.with.overflow`.
pub fn overflow_intrinsic<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    let _ = (cx, inst, which, args);
    unsupported(OVERFLOW)
}
