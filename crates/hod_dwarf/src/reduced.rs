//! Units whose `emissionKind` is not `FullDebug` (`spec/dwarf.md` §6.10,
//! WP-061): `LineTablesOnly` (`-gline-tables-only`, `-g1`) and
//! `DebugDirectivesOnly` (`-gline-directives-only`).
//!
//! What `llc` 19 and 22 write for them, read from their output:
//!
//! - `LineTablesOnly`: the line table as for `FullDebug`, and a unit DIE
//!   with the same attributes as a full unit's (producer, language, name,
//!   the string-offsets base, `DW_AT_stmt_list`, the directory, the code
//!   as `low_pc`/`high_pc` or `ranges`, the address and range-list bases).
//!   A function gets **no DIE** unless code was inlined into it
//!   (`DwarfDebug::endFunctionImpl` returns early when the function has no
//!   abstract scopes): its code is only in the unit's ranges. A function
//!   with inlined code gets a subprogram DIE with only `low_pc`,
//!   `high_pc` and `name`, an abstract subprogram per inlined function
//!   (`name`, `DW_AT_inline`) and `DW_TAG_inlined_subroutine` children;
//!   inlined subroutines are not written yet, so such a function is
//!   refused as [`LINE_TABLES_INLINED`].
//! - `DebugDirectivesOnly`: the line table only. No `.debug_info`, no
//!   `.debug_abbrev`, no strings: the unit is in `.debug_line` and nowhere
//!   else. The bridge (`hod_cg`) leaves such a unit out of
//!   [`crate::write_debug_info`]; its plan still numbers the files of its
//!   line table.
//!
//! What no producer writes is refused rather than guessed: a reduced unit
//! that lists globals, enumerations, retained types or imported entities
//! ([`REDUCED_UNIT_ENTITIES`]: `llc` 22 crashes on such a `LineTablesOnly`
//! unit and writes a stray `.debug_addr` for such a `DebugDirectivesOnly`
//! one), and a function of a `LineTablesOnly` unit with variable or label
//! records ([`LINE_TABLES_VARIABLES`]: `llc` 22 crashes).

use hod_ir::{DebugInfo, DiCompileUnit, DiId, EmissionKind, LocId};

use crate::Error;
use crate::die::{UnitPlan, unsupported};
use crate::func::FunctionInput;

/// A `LineTablesOnly` or `DebugDirectivesOnly` unit that lists globals,
/// enumerations, retained types or imported entities.
pub const REDUCED_UNIT_ENTITIES: &str = "debug:reduced-unit-entities";
/// A function of a `LineTablesOnly` unit with variable or label records.
pub const LINE_TABLES_VARIABLES: &str = "debug:line-tables-variables";
/// A function of a `LineTablesOnly` unit with code inlined into it: its
/// subprogram DIE needs inlined subroutines (`spec/dwarf.md` §6.10).
pub const LINE_TABLES_INLINED: &str = "debug:line-tables-inlined";

/// Whether a unit of this kind gets DIEs: `FullDebug` and
/// `LineTablesOnly` do, `DebugDirectivesOnly` (only a line table) and
/// `NoDebug` (nothing) do not.
pub const fn has_dies(kind: EmissionKind) -> bool {
    matches!(kind, EmissionKind::FullDebug | EmissionKind::LineTablesOnly)
}

/// The check [`UnitPlan`] makes of a unit that is not `FullDebug`: it is
/// `LineTablesOnly` or `DebugDirectivesOnly`, and lists nothing of what
/// only a full unit describes.
pub(crate) fn check_unit(unit: DiId, cu: &DiCompileUnit<'_>) -> Result<(), Error> {
    match cu.emission_kind {
        EmissionKind::LineTablesOnly | EmissionKind::DebugDirectivesOnly => {}
        EmissionKind::FullDebug | EmissionKind::NoDebug => {
            return Err(unsupported(unit, "a compile unit whose emissionKind is NoDebug"));
        }
    }
    if !(cu.globals.is_empty()
        && cu.enums.is_empty()
        && cu.retained_types.is_empty()
        && cu.imports.is_empty())
    {
        return Err(unsupported(unit, REDUCED_UNIT_ENTITIES));
    }
    Ok(())
}

/// Whether the code of `f` holds an inlined location: a transition with
/// `inlinedAt` that covers at least one byte (what makes `llc`'s
/// `LexicalScopes` create an abstract scope).
fn has_inlined_code(debug: &DebugInfo, f: &FunctionInput<'_>) -> Option<DiId> {
    let size = u32::try_from(f.code.size).unwrap_or(u32::MAX);
    f.scopes.iter().enumerate().find_map(|(i, row)| {
        let end = f.scopes.get(i + 1).map_or(size, |r| r.offset).min(size);
        let loc = debug.location(row.loc)?;
        (end > row.offset && loc.inlined_at != LocId::NONE).then_some(loc.scope)
    })
}

/// [`UnitPlan::add_function`] for a function of a reduced unit: no DIE.
/// Its code is in the unit's ranges through the [`crate::CodeSpan`]s the
/// caller gives [`crate::write_debug_info`].
pub(crate) fn add_function(plan: &UnitPlan<'_>, f: &FunctionInput<'_>) -> Result<(), Error> {
    if plan.cu.emission_kind != EmissionKind::LineTablesOnly {
        // `DebugDirectivesOnly`: no DIEs at all, whatever the function is.
        return Ok(());
    }
    if !f.vars.is_empty() {
        return Err(unsupported(f.subprogram, LINE_TABLES_VARIABLES));
    }
    if let Some(scope) = has_inlined_code(plan.debug, f) {
        return Err(unsupported(scope, LINE_TABLES_INLINED));
    }
    Ok(())
}
