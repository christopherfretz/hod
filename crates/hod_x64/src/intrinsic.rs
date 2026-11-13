//! The intrinsics no other group takes (bit counting, `memcpy` and
//! friends, `llvm.trap`, `llvm.expect`, ...): WP-128 (X-intrinsic). The
//! dispatch (`target.rs`) has already sent the variadic, stack,
//! overflow, thread-local, coroutine and floating-point intrinsics to
//! their modules. Refuses as [`INTRINSIC`] until then.

use hod_cg::{CgResult, FnCx};
use hod_ir::{Inst, Intrinsic, Value};

use crate::common::unsupported;
use crate::target::X64;

/// The group's refusal until WP-128.
pub const INTRINSIC: &str = "x64:todo:intrinsic";

/// A call of intrinsic `which` with `args`.
///
/// # Errors
/// [`INTRINSIC`].
pub fn intrinsic<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    let _ = (cx, inst, which, args);
    unsupported(INTRINSIC)
}
