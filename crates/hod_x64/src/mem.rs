//! `load`, `store`, `alloca`, `ptradd`, `llvm.stacksave` and
//! `llvm.stackrestore` of class `Gpr` and `Fpr`: WP-123 (X-mem). Every
//! entry point refuses as [`MEM`] until then.
//!
//! What WP-123 finds in `common` and `frame`:
//! [`slot_address`](crate::common::slot_address),
//! [`frame_address`](crate::common::frame_address),
//! [`gpr_load`](crate::common::gpr_load), [`fpr_load`](crate::common::fpr_load),
//! [`frame::dynamic_slot`](crate::frame::dynamic_slot)'s sequence for a
//! dynamic `alloca` (the epilogue restores `rsp` from `rbp`, so nothing
//! else is needed). An `alloca` aligned above 16 is `x64:stackrealign`
//! (design 0006 §2.9).

use hod_cg::{CgResult, FnCx};
use hod_ir::{Inst, Intrinsic, MemFlags, PtrAdd, Value};

use crate::common::unsupported;
use crate::target::X64;

/// The group's refusal until WP-123.
pub const MEM: &str = "x64:todo:mem";

/// A non-atomic `load` of a `Gpr` or `Fpr` value.
///
/// # Errors
/// [`MEM`].
pub fn load<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, ptr: Value, mem: MemFlags) -> CgResult<()> {
    let _ = (cx, inst, ptr, mem);
    unsupported(MEM)
}

/// A non-atomic `store` of a `Gpr` or `Fpr` value.
///
/// # Errors
/// [`MEM`].
pub fn store<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    value: Value,
    ptr: Value,
    mem: MemFlags,
) -> CgResult<()> {
    let _ = (cx, inst, value, ptr, mem);
    unsupported(MEM)
}

/// A dynamic `alloca`.
///
/// # Errors
/// [`MEM`].
pub fn alloca<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    count: Value,
    elem_size: u64,
    align: u64,
) -> CgResult<()> {
    let _ = (cx, inst, count, elem_size, align);
    unsupported(MEM)
}

/// `ptradd` (`getelementptr`).
///
/// # Errors
/// [`MEM`].
pub fn ptr_add<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, add: &PtrAdd<'a>) -> CgResult<()> {
    let _ = (cx, inst, add);
    unsupported(MEM)
}

/// `llvm.stacksave`, `llvm.stackrestore`.
///
/// # Errors
/// [`MEM`].
pub fn stack_intrinsic<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    let _ = (cx, inst, which, args);
    unsupported(MEM)
}
