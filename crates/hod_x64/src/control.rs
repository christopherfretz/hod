//! `condbr`, `switch`, `indirectbr` and `blockaddress`: WP-124
//! (X-control). Every entry point refuses as [`CONTROL`] until then.
//!
//! What WP-124 finds in `common`: [`branch`](crate::common::branch) and
//! [`branch_cond`](crate::common::branch_cond) ("forward `rel32`,
//! backward shortest"), the fixup kinds [`REL32`](crate::common::REL32)
//! and [`REL8`](crate::common::REL8). A fused `icmp` is WP-122's; between
//! its `cmp` and the `jcc` only flag-preserving glue runs (§5.6.3).

use hod_cg::{CgResult, FnCx};
use hod_ir::{BlockId, Inst, Switch, Value};

use crate::common::unsupported;
use crate::target::X64;

/// The group's refusal until WP-124 (also `blockaddress` constants).
pub const CONTROL: &str = "x64:todo:control";

/// `br i1 %cond, label %t, label %f`.
///
/// # Errors
/// [`CONTROL`].
pub fn cond_br<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    cond: Value,
    if_true: BlockId,
    if_false: BlockId,
) -> CgResult<()> {
    let _ = (cx, inst, cond, if_true, if_false);
    unsupported(CONTROL)
}

/// `switch`.
///
/// # Errors
/// [`CONTROL`].
pub fn switch<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, switch: &Switch<'a>) -> CgResult<()> {
    let _ = (cx, inst, switch);
    unsupported(CONTROL)
}

/// `indirectbr`.
///
/// # Errors
/// [`CONTROL`].
pub fn indirect_br<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, addr: Value) -> CgResult<()> {
    let _ = (cx, inst, addr);
    unsupported(CONTROL)
}
