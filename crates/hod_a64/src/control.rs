//! Control flow beyond `br` (`spec/codegen.md` §6): `condbr` (with a
//! deferred compare fused), `switch`, `indirectbr`, and the `blockaddress`
//! constants it consumes ([`crate::common::BLOCKADDRESS`]).
//!
//! **Owner: WP-037.** Tests: `tests/control.rs`. Until the package lands
//! each entry point refuses with its stable name below; the dispatch in
//! `target.rs` is final. An entry point starts with
//! [`deferred_first`](crate::common::deferred_first), naming the
//! condition it fuses. Branches use `crate::common::branch*`, which know
//! far mode.

use hod_cg::{CgResult, FnCx};
use hod_ir::{BlockId, Inst, Switch, Value};

use crate::common::unsupported;
use crate::target::A64;

/// `br i1 %cond, label %t, label %f`.
pub const COND_BR: &str = "a64:condbr";
/// `switch`.
pub const SWITCH: &str = "a64:switch";
/// `indirectbr`.
pub const INDIRECT_BR: &str = "a64:indirectbr";

/// `br i1 %cond, label %if_true, label %if_false`.
pub fn cond_br<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    cond: Value,
    if_true: BlockId,
    if_false: BlockId,
) -> CgResult<()> {
    let _ = (cx, inst, cond, if_true, if_false);
    unsupported(COND_BR)
}

/// `switch <ty> %cond, label %default [cases]`.
pub fn switch<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, switch: &Switch<'a>) -> CgResult<()> {
    let _ = (cx, inst, switch);
    unsupported(SWITCH)
}

/// `indirectbr ptr %addr, [labels]`.
pub fn indirect_br<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, addr: Value) -> CgResult<()> {
    let _ = (cx, inst, addr);
    unsupported(INDIRECT_BR)
}
