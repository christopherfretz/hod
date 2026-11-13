//! Coroutine bodies at the facade (WP-106, `spec/codegen.md` §7.6): a body
//! with `presplitcoroutine` that `classify` accepted is accepted when the
//! coroutine plan accepts it, and refused by the plan's `coro:*` name
//! otherwise, before code generation; a coroutine in a module that writes
//! debug information is refused as [`DEBUG_INFO`] until its frame
//! variables are described (WP-108).

use hod_ir::{Body, DiNode, EmissionKind, Module};

/// A coroutine in a module that writes debug information.
pub(crate) const DEBUG_INFO: &str = "coro:debug-info";

/// The name `body` is refused by, if it is a coroutine hod does not
/// compile.
pub(crate) fn refusal(module: &Module, body: &Body) -> Option<&'static str> {
    match hod_xform::coro::plan(module, body) {
        Err(refusal) => Some(refusal.construct),
        Ok(Some(_)) if writes_debug_info(module) => Some(DEBUG_INFO),
        Ok(_) => None,
    }
}

/// Whether `module` has a compile unit debug information is written for
/// (as `classify` decides it).
fn writes_debug_info(module: &Module) -> bool {
    let debug = &module.debug;
    debug.compile_units().iter().any(|&unit| {
        !matches!(debug.node(unit), DiNode::CompileUnit(cu) if cu.emission_kind == EmissionKind::NoDebug)
    })
}
