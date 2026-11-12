//! Calls and what goes with them (`spec/codegen.md` §7.1, `abi-a64.md`
//! §8): calls and `invoke` through `hod_cg`'s call sequence, `musttail`,
//! landing pads and `resume`, the variadic save area, `va_arg` and the
//! `llvm.va_*` intrinsics.
//!
//! **Owner: WP-038.** Tests: `tests/call.rs`. Until the package lands each
//! entry point refuses with its stable name below; the dispatch in
//! `target.rs` is final. The call instruction itself (`bl` with
//! `R_AARCH64_CALL26`, `blr`) and the tail exits are emitted by
//! `crate::frame` (WP-033), which `hod_cg`'s call sequence drives.

use hod_cg::{CgResult, FnCx};
use hod_ir::{Inst, Intrinsic, LandingPad, Value};

use crate::common::unsupported;
use crate::target::A64;

/// A call or `invoke`.
pub const CALL: &str = "a64:call";
/// `llvm.va_start`, `llvm.va_end`, `llvm.va_copy`.
pub const VA_INTRINSIC: &str = "a64:va-intrinsic";
/// `va_arg`.
pub const VA_ARG: &str = "a64:va_arg";
/// `landingpad`.
pub const LANDING_PAD: &str = "a64:landingpad";
/// `resume`.
pub const RESUME: &str = "a64:resume";
/// The definition of a variadic function: its register save area.
pub const VARIADIC: &str = "a64:variadic-definition";

/// What a variadic function's frame holds for `va_start` (the save
/// area's offsets), filled by [`reserve_variadic`]. Part of
/// `crate::target::FnState`.
#[derive(Clone, Copy, Default, Debug)]
pub struct VaState {}

/// Called by the prologue of a variadic function, after the frame record
/// and the incoming base are reserved: reserves and fills the register
/// save area (`abi-a64.md` §8). Writes scratch registers only.
pub fn reserve_variadic(cx: &mut FnCx<'_, A64>) -> CgResult<()> {
    let _ = cx;
    unsupported(VARIADIC)
}

/// A call or `invoke` of a function, not an intrinsic: `cx.call(inst)`
/// once the package has checked what it supports.
pub fn call<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>) -> CgResult<()> {
    let _ = (cx, inst);
    unsupported(CALL)
}

/// `llvm.va_start`, `llvm.va_end`, `llvm.va_copy`.
pub fn va_intrinsic<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    let _ = (cx, inst, which, args);
    unsupported(VA_INTRINSIC)
}

/// `%r = va_arg ptr %list, <ty>`.
pub fn va_arg<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, list: Value) -> CgResult<()> {
    let _ = (cx, inst, list);
    unsupported(VA_ARG)
}

/// `%r = landingpad <ty> [clauses]`.
pub fn landing_pad<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    pad: &LandingPad<'a>,
) -> CgResult<()> {
    let _ = (cx, inst, pad);
    unsupported(LANDING_PAD)
}

/// `resume <ty> %value`.
pub fn resume<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, value: Value) -> CgResult<()> {
    let _ = (cx, inst, value);
    unsupported(RESUME)
}
