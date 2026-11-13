//! CET indirect-branch tracking (WP-184, `spec/codegen.md` §5.6.25): the
//! `endbr64` landing instructions of a module with the module flag
//! `cf-protection-branch`, where `llc` 19 and 22 put them
//! (`X86IndirectBranchTracking`), and the one form hod refuses.
//!
//! - Every function entry but a `nocf_check` function's ([`entry`],
//!   called once, first, from `frame::begin`). `llc` also leaves out
//!   local functions whose address is never taken; hod marks those too,
//!   which is harmless: `endbr64` is a `nop` (a hint in the reserved-`nop`
//!   space) on every x86-64 processor, and marking a place only permits
//!   an indirect branch there that the program never makes.
//! - Every block whose address is taken and every landing pad, before the
//!   block's `phi` loads, at the offset recorded for it (`hod_cg`'s
//!   `Target::branch_target` hook, [`landing`]).
//! - The resume point of `llvm.eh.sjlj.setjmp` (`llc` takes its address
//!   as a block's), and each coroutine entry stub (`F.resume`,
//!   `F.destroy`: `llc` writes those as functions whose address is
//!   taken) ([`landing`]).
//! - After a direct call of a function declared `returns_twice` (the
//!   callee's declaration, not the call site, decides, as in `llc`)
//!   ([`after_call`]): a `longjmp` may return there by an indirect jump.
//! - Not at `callbr` indirect targets: `llc` 19 and 22 mark none (the
//!   assembly branches there directly).
//!
//! hod makes no jump table (`control`), so no `notrack` jump. An indirect
//! call with `nocf_check` needs the `notrack` prefix (`3E`), which the
//! encoder does not have: it is refused ([`NOCF_CHECK_INDIRECT_CALL`]);
//! without the prefix it would fault under IBT at a callee without
//! `endbr64`. The property note is written by the module emitter
//! (`hod_cg`'s `gnu_property`).

use hod_cg::{CallTarget, CgResult, FnCx, SymRef};
use hod_ir::{BranchProtection, CallSite, FnAttrs, Module, SymId, SymbolKind, ValueKind};
use hod_mc_x64 as mc;

use crate::X64;
use crate::common::{put, unsupported};

/// An indirect call (or `musttail` call) with `nocf_check` in a
/// `cf-protection-branch` module: it needs a `notrack` prefix, which the
/// encoder lacks (`llc` 19 and 22 abort on the `musttail` form).
pub const NOCF_CHECK_INDIRECT_CALL: &str = "x64:nocf-check-indirect-call";

/// Every name this module refuses with.
pub const UNSUPPORTED: &[&str] = &[NOCF_CHECK_INDIRECT_CALL];

/// Whether the module asks for indirect-branch tracking
/// (`cf-protection-branch`, any value: `spec/ir.md` §2.1).
#[inline]
pub fn tracks(module: &Module) -> bool {
    module.props.branch_protection.has(BranchProtection::CET_IBT)
}

/// Whether the function symbol `sym` has the function flag `flag`.
fn has_flag(module: &Module, sym: SymId, flag: u32) -> bool {
    match module.symbol(sym).kind() {
        SymbolKind::Function { attrs, .. } => module.fn_attrs(attrs).has(flag),
        _ => false,
    }
}

/// The function's entry: `endbr64` unless the function is `nocf_check`.
/// The prologue's first instruction (`frame::begin` calls this before
/// anything else it emits).
pub fn entry(cx: &mut FnCx<'_, X64>) {
    if tracks(cx.module) && !has_flag(cx.module, cx.body.symbol(), FnAttrs::NO_CF_CHECK) {
        put(cx, mc::endbr64());
    }
}

/// A place an indirect branch or the unwinder reaches inside a function:
/// `endbr64` when the module tracks indirect branches.
pub fn landing(cx: &mut FnCx<'_, X64>) {
    if tracks(cx.module) {
        put(cx, mc::endbr64());
    }
}

/// Right after a call instruction to `target` (after its return address
/// is recorded): `endbr64` when the callee is a function declared
/// `returns_twice`.
pub fn after_call(cx: &mut FnCx<'_, X64>, target: CallTarget) {
    if let CallTarget::Sym(SymRef::Module(sym)) = target
        && tracks(cx.module)
        && has_flag(cx.module, sym, FnAttrs::RETURNS_TWICE)
    {
        put(cx, mc::endbr64());
    }
}

/// Refuses a call the tracking cannot express: an indirect `nocf_check`
/// call ([`NOCF_CHECK_INDIRECT_CALL`]). A direct one is an ordinary call.
///
/// # Errors
/// [`NOCF_CHECK_INDIRECT_CALL`].
pub fn check_call(cx: &FnCx<'_, X64>, site: &CallSite<'_>) -> CgResult<()> {
    if site.is_nocf_check()
        && tracks(cx.module)
        && !matches!(site.callee_value().kind(), ValueKind::Sym(_))
    {
        return unsupported(NOCF_CHECK_INDIRECT_CALL);
    }
    Ok(())
}
