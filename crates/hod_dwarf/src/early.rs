//! What WP-087 adds to the plan for whole `-g` translation units
//! (`spec/dwarf.md` §6.12).
//!
//! **A function-local type reached before its function** (a retained
//! type of the unit, the explicit casts clang lists in `retainedTypes`,
//! or a type another function names): `llc`'s `getOrCreateContextDIE`
//! makes the function's DIE there and then (`getOrCreateSubprogramDIE`),
//! and the type is its child; the function, placed later, fills that DIE.
//! That is the early DIE functions' statics already get (WP-085,
//! [`UnitPlan::static_parent`]) when the module has the function's code.
//! A type local to a function without code here (`llc` 22 puts it under
//! the abstract instance of a function that is only inlined, `llc` 19
//! under an empty subprogram of its own, and a local type of an abstract
//! instance is refused anyway, `debug:inlined-local-type`), to a block, or
//! to a function that is not a definition of this unit stays refused as
//! [`LOCAL_TYPE_SCOPE`].

use hod_ir::DiId;

use crate::Error;
use crate::die::{UnitPlan, unsupported};
use crate::inline::DEBUG_LOCAL_STATIC;

/// A function-local type whose function's DIE cannot be made where `llc`
/// makes it (§6.4, §6.12).
pub const LOCAL_TYPE_SCOPE: &str = "debug:local-type-scope";

impl UnitPlan<'_> {
    /// The parent DIE of type `of`, local to subprogram `scope`, which has
    /// no DIE yet: the function's early DIE.
    pub(crate) fn local_type_parent(&mut self, of: DiId, scope: DiId) -> Result<u32, Error> {
        if !self.inline.has_body(scope) {
            return Err(unsupported(of, LOCAL_TYPE_SCOPE));
        }
        self.static_parent(of, scope).map_err(|e| match e {
            Error::Unsupported { what: DEBUG_LOCAL_STATIC, .. } => {
                unsupported(of, LOCAL_TYPE_SCOPE)
            }
            e => e,
        })
    }
}
