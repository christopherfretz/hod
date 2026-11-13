//! Coroutine handle operations in any function: `llvm.coro.resume`,
//! `destroy`, `done`, `promise`, `noop`: WP-137 (C7). Refuses as
//! [`CORO`] until then.

use hod_cg::{CgResult, FnCx};
use hod_ir::{Inst, Intrinsic, Value};

use crate::common::unsupported;
use crate::target::X64;

/// The group's refusal until WP-137.
pub const CORO: &str = "x64:todo:coro";

/// A coroutine handle intrinsic.
///
/// # Errors
/// [`CORO`].
pub fn intrinsic<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    let _ = (cx, inst, which, args);
    unsupported(CORO)
}
