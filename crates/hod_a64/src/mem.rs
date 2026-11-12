//! Memory instructions (`spec/codegen.md` §5.2): `load` and `store` of
//! `Gpr`- and `Fpr`-class values, `alloca`, `ptradd`,
//! `llvm.stacksave` and `llvm.stackrestore`.
//!
//! **Owner: WP-036.** Tests: `tests/mem.rs`. Until the package lands each
//! entry point refuses with its stable name below; the dispatch in
//! `target.rs` is final. An entry point starts with
//! [`deferred_first`](crate::common::deferred_first), naming the address
//! operand when it fuses a deferred `ptradd` into the addressing mode.
//! A lowering that leaves the stack pointer below the frame sets
//! `cx.state.frame.dynamic` (`crate::frame::FrameState::dynamic`).

use hod_cg::{CgResult, FnCx};
use hod_ir::{Inst, Intrinsic, MemFlags, PtrAdd, Value};

use crate::common::unsupported;
use crate::target::A64;

/// `load` of a register value.
pub const LOAD: &str = "a64:load";
/// `store` of a register value.
pub const STORE: &str = "a64:store";
/// `alloca`.
pub const ALLOCA: &str = "a64:alloca";
/// `ptradd`.
pub const PTRADD: &str = "a64:ptradd";
/// `llvm.stacksave`, `llvm.stackrestore`.
pub const STACK_INTRINSIC: &str = "a64:stack-intrinsic";

/// `%r = load <ty>, ptr %ptr` of class `Gpr` or `Fpr`.
pub fn load<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, ptr: Value, mem: MemFlags) -> CgResult<()> {
    let _ = (cx, inst, ptr, mem);
    unsupported(LOAD)
}

/// `store <ty> %value, ptr %ptr` of class `Gpr` or `Fpr`.
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

/// `%r = alloca <elem_size>, %count, align <align>` (§5.2).
pub fn alloca<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    count: Value,
    elem_size: u64,
    align: u64,
) -> CgResult<()> {
    let _ = (cx, inst, count, elem_size, align);
    unsupported(ALLOCA)
}

/// `%r = ptradd %base, <offset> + Σ index × stride`.
pub fn ptr_add<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, add: &PtrAdd<'a>) -> CgResult<()> {
    let _ = (cx, inst, add);
    unsupported(PTRADD)
}

/// `llvm.stacksave` and `llvm.stackrestore`.
pub fn stack_intrinsic<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    let _ = (cx, inst, which, args);
    unsupported(STACK_INTRINSIC)
}
