//! Coroutine bodies at the facade (WP-106, `spec/codegen.md` §7.6): a body
//! with `presplitcoroutine` that `classify` accepted is accepted when the
//! coroutine plan accepts it, and refused by the plan's `coro:*` name
//! otherwise, before code generation. With debug information, its
//! variables are described on the frame word (WP-108, `spec/dwarf.md`
//! §6.16).

use hod_ir::{Body, Module};

/// The name `body` is refused by, if it is a coroutine hod does not
/// compile.
pub(crate) fn refusal(module: &Module, body: &Body) -> Option<&'static str> {
    match hod_xform::coro::plan(module, body) {
        Err(refusal) => Some(refusal.construct),
        Ok(_) => None,
    }
}
