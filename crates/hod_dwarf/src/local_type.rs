//! Types local to a function that is only inlined (`spec/dwarf.md`
//! §6.14, WP-098): the lambda of an `always_inline` member, say. When
//! the type is reached, `llc`'s `getOrCreateContextDIE` finds the
//! function's abstract instance (`getAbstractScopeDIEs`) if it exists and
//! makes the type its child; `llc` 19 and 22 agree. In the abstract
//! instance a variable's type is made before the variable is added to
//! its scope (`constructVariableDIE`), so the type precedes it. A type
//! reached before the abstract instance exists (`llc` would make another
//! subprogram DIE) stays `debug:local-type-scope`.

use hod_ir::DiId;

use crate::die::UnitPlan;

/// The abstract instance of subprogram `scope`, when it has one: the
/// parent of a type local to it.
pub(crate) fn abstract_parent(plan: &UnitPlan<'_>, scope: DiId) -> Option<u32> {
    plan.inline.die(scope)
}
