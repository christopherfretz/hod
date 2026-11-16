//! CET on i386 (`spec/codegen-x86.md` §17, WP-324): the `endbr32` landing
//! instructions of a module with the module flag `cf-protection-branch`,
//! where `llc` 19 and 22 put them (`X86IndirectBranchTracking`, the same
//! places as on x86-64, `hod_x64`'s `cet`), and the one form refused.
//!
//! - Every function entry but a `nocf_check` function's ([`entry`],
//!   first in `frame::begin`). `llc` also leaves out local functions
//!   whose address is never taken; hod marks those too, which is
//!   harmless (`endbr32` is a `nop` in the reserved-`nop` space).
//! - Every block whose address is taken and every landing pad (and an
//!   `invoke`'s pad stub), before the block's `phi` loads (`hod_cg`'s
//!   `Target::branch_target`, [`landing`]).
//! - After a direct call of a function declared `returns_twice`
//!   ([`after_call`]).
//!
//! hod makes no jump table, so no `notrack` jump. An indirect call with
//! `nocf_check` needs the `notrack` prefix, which the encoder does not
//! have: refused ([`NOCF_CHECK_INDIRECT_CALL`]), as on x86-64.
//! `cf-protection-return` changes no code (hod returns only to the
//! address its call pushed). The property note is the module emitter's
//! (`hod_cg`'s `gnu_property`, ELF32 padding).

use hod_cg::{CallTarget, CgResult, FnCx, SymRef};
use hod_ir::{BranchProtection, CallSite, FnAttrs, Module, SymId, SymbolKind, ValueKind};
use hod_mc_x64 as mc;

use crate::common::{put, unsupported};
use crate::target::X86;

/// An indirect call with `nocf_check` in a `cf-protection-branch` module:
/// it needs a `notrack` prefix, which the encoder lacks.
pub const NOCF_CHECK_INDIRECT_CALL: &str = "x86:nocf-check-indirect-call";

/// Whether the module asks for indirect-branch tracking.
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

/// The function's entry: `endbr32` unless the function is `nocf_check`.
pub fn entry(cx: &mut FnCx<'_, X86>) {
    if tracks(cx.module) && !has_flag(cx.module, cx.body.symbol(), FnAttrs::NO_CF_CHECK) {
        put(cx, mc::endbr32());
    }
}

/// A place an indirect branch or the unwinder reaches inside a function.
pub fn landing(cx: &mut FnCx<'_, X86>) {
    if tracks(cx.module) {
        put(cx, mc::endbr32());
    }
}

/// Right after a call instruction to `target`: `endbr32` when the callee
/// is a function declared `returns_twice`.
pub fn after_call(cx: &mut FnCx<'_, X86>, target: CallTarget) {
    if let CallTarget::Sym(SymRef::Module(sym)) = target
        && tracks(cx.module)
        && has_flag(cx.module, sym, FnAttrs::RETURNS_TWICE)
    {
        put(cx, mc::endbr32());
    }
}

/// Refuses an indirect `nocf_check` call ([`NOCF_CHECK_INDIRECT_CALL`]).
///
/// # Errors
/// [`NOCF_CHECK_INDIRECT_CALL`].
pub fn check_call(cx: &FnCx<'_, X86>, site: &CallSite<'_>) -> CgResult<()> {
    if site.is_nocf_check()
        && tracks(cx.module)
        && !matches!(site.callee_value().kind(), ValueKind::Sym(_))
    {
        return unsupported(NOCF_CHECK_INDIRECT_CALL);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use hod_mc_x64::Mode;

    /// `llvm-mc -triple=i686`'s `endbr32`.
    #[test]
    fn endbr32_bytes() {
        assert_eq!(
            Mode::X86.admit(super::mc::endbr32()).unwrap().bytes(),
            [0xf3, 0x0f, 0x1e, 0xfb]
        );
    }
}
