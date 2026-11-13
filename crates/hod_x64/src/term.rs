//! `br`, `ret` and `unreachable` (`spec/codegen.md` §5.4, §5.6, §6.2):
//! WP-116.

use hod_cg::{CgResult, FnCx};
use hod_ir::{BlockId, Inst, Value};

use crate::common::{branch, deferred_first, unsupported};
use crate::target::X64;

/// `br label %target`: nothing when the target is emitted next.
///
/// # Errors
/// What the branch's fixup reports (§6.3).
pub fn br(cx: &mut FnCx<'_, X64>, target: BlockId) -> CgResult<()> {
    if cx.falls_through(target) {
        return Ok(());
    }
    branch(cx, cx.block_label(target))
}

/// `ret`: the value to where the function returns it (§7.2, through
/// [`FnCx::place_return`]), then to the epilogue unless it comes next
/// (§5.4).
///
/// # Errors
/// `x64:todo:abi` for a value returned in memory (WP-118, WP-126); what
/// placing the value or the branch reports.
pub fn ret<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, value: Option<Value>) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    if let Some(value) = value {
        if cx.fn_assignment().ret_memory.is_some() {
            return unsupported(crate::abi::ABI);
        }
        cx.place_return(value)?;
    }
    if cx.ret_falls_through() {
        return Ok(());
    }
    branch(cx, cx.epilogue())
}

/// `unreachable`: the trap, `ud2` (§4.7).
///
/// # Errors
/// None; a `Result` for the dispatch.
#[allow(clippy::unnecessary_wraps, reason = "every entry point of the dispatch returns CgResult")]
pub fn unreachable(cx: &mut FnCx<'_, X64>) -> CgResult<()> {
    cx.trap();
    Ok(())
}
