//! `br`, `ret` and `unreachable` (`spec/codegen.md` §5.4, §6.2): WP-033.

use hod_cg::{CgResult, Class, FnCx};
use hod_ir::{BlockId, Inst, Value};

use crate::common::{Cursor, X16, X17, branch, copy_bytes, deferred_first, mem_bytes, value_ty};
use crate::frame::ret_memory_address;
use crate::target::A64;

/// `br label %target`: nothing when the target is emitted next.
///
/// # Errors
/// What the branch's fixup reports (§6.3).
pub fn br(cx: &mut FnCx<'_, A64>, target: BlockId) -> CgResult<()> {
    if cx.falls_through(target) {
        return Ok(());
    }
    branch(cx, cx.block_label(target))
}

/// `ret`: the value to where the function returns it, then to the
/// epilogue unless it comes next (§5.4). A value returned in memory is
/// copied to the buffer whose address arrived in `x8` (AAPCS64 does not
/// ask for the address back).
///
/// # Errors
/// What placing the value or the branch reports.
pub fn ret<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, value: Option<Value>) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    if let Some(value) = value {
        if cx.state.frame.ret_memory.is_some() {
            let bytes = mem_bytes(cx, value_ty(cx, value))?;
            let from = if cx.class_of(value) == Class::Mem {
                cx.use_mem(value)?
            } else {
                return cx.bug("a register value returned in memory");
            };
            ret_memory_address(cx, X17)?;
            let mut to = Cursor::at_reg(X17, 0, X17);
            let mut src = Cursor::frame(cx, from, X16);
            copy_bytes(cx, &mut to, &mut src, bytes);
        } else {
            cx.place_return(value)?;
        }
    }
    if cx.ret_falls_through() {
        return Ok(());
    }
    branch(cx, cx.epilogue())
}

/// `unreachable`: the trap (§4.7).
///
/// # Errors
/// None; a `Result` for the dispatch.
#[allow(clippy::unnecessary_wraps, reason = "every entry point of the dispatch returns CgResult")]
pub fn unreachable(cx: &mut FnCx<'_, A64>) -> CgResult<()> {
    cx.trap();
    Ok(())
}
