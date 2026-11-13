//! Inline assembly (`call` of an `asm` callee, `callbr`): the templates
//! that assemble to nothing (compiler barriers) first, WP-128. Refuses as
//! [`BARRIER`] until then; an x86 assembler for real templates is not
//! planned yet.

use hod_cg::{CgResult, FnCx};
use hod_ir::Inst;

use crate::common::unsupported;
use crate::target::X64;

/// The group's refusal until WP-128.
pub const BARRIER: &str = "x64:todo:barrier";

/// A call whose callee is inline assembly.
///
/// # Errors
/// [`BARRIER`].
pub fn asm<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>) -> CgResult<()> {
    let _ = (cx, inst);
    unsupported(BARRIER)
}
