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
//!   with inlined code (WP-094, `spec/dwarf.md` §6.13) goes through the
//!   functions' path with `llc`'s `includeMinimalInlineScopes`: every
//!   DIE is the unit's child; the function's subprogram DIE has only
//!   `low_pc`, `high_pc` and `name` (or `DW_AT_abstract_origin`), an
//!   abstract subprogram per inlined function only `name` and
//!   `DW_AT_inline`, its inlined subroutines are WP-085's, and no lexical
//!   block is written. What no producer of such a unit writes is refused
//!   as [`LINE_TABLES_INLINED`].
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

use hod_ir::{
    DebugInfo, DiCompileUnit, DiFlags, DiId, DiNode, DiSubprogram, EmissionKind, LocId, SpFlags,
};

use crate::Error;
use crate::abbrev::{code, fam};
use crate::die::{UnitPlan, Writer, unsupported};
use crate::func::FunctionInput;

/// A `LineTablesOnly` or `DebugDirectivesOnly` unit that lists globals,
/// enumerations, retained types or imported entities.
pub const REDUCED_UNIT_ENTITIES: &str = "debug:reduced-unit-entities";
/// A function of a `LineTablesOnly` unit with variable or label records.
pub const LINE_TABLES_VARIABLES: &str = "debug:line-tables-variables";
/// A function of a `LineTablesOnly` unit with code inlined into it whose
/// subprogram has a declaration, template parameters, a subroutine type
/// with elements or retained nodes, none of which `-gline-tables-only`
/// writes (`spec/dwarf.md` §6.13; the plain shape is written since
/// WP-094).
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

/// [`UnitPlan::add_function`] for a function of a reduced unit: `true`
/// when it is done with it (no DIE: its code is in the unit's ranges
/// through the [`crate::CodeSpan`]s the caller gives
/// [`crate::write_debug_info`]); `false` for a `LineTablesOnly` function
/// with inlined code, which goes on along the functions' path in minimal
/// form (WP-094, §6.13).
pub(crate) fn add_function(plan: &UnitPlan<'_>, f: &FunctionInput<'_>) -> Result<bool, Error> {
    if plan.cu.emission_kind != EmissionKind::LineTablesOnly {
        // `DebugDirectivesOnly`: no DIEs at all, whatever the function is.
        return Ok(true);
    }
    if !f.vars.is_empty() {
        return Err(unsupported(f.subprogram, LINE_TABLES_VARIABLES));
    }
    if has_inlined_code(plan.debug, f).is_none() {
        return Ok(true);
    }
    // What `seal` would make for the function beyond its name: a return
    // type, template parameters, a specification, imports, a
    // variadic function's `DW_TAG_unspecified_parameters`. `llc` writes
    // none of them in minimal form; no producer writes them in such a
    // unit, so they are refused rather than special-cased.
    let DiNode::Subprogram(sp) = plan.debug.node(f.subprogram) else {
        return Err(unsupported(f.subprogram, LINE_TABLES_INLINED));
    };
    let typed = sp.ty.is_some_and(|t| match plan.debug.node(t) {
        DiNode::SubroutineType(t) => !t.types.is_empty(),
        _ => true,
    });
    if typed
        || sp.declaration.is_some()
        || !sp.template_params.is_empty()
        || !sp.retained_nodes.is_empty()
    {
        return Err(unsupported(f.subprogram, LINE_TABLES_INLINED));
    }
    Ok(false)
}

/// The flags a minimal subprogram may have; anything else is refused as
/// `debug:subprogram-attribute`, as for a full unit.
/// `DIFlagAllCallsDescribed` changes nothing in a function without a DIE
/// (WP-113); one with inlined code gets its call sites in minimal form, as
/// `llc` writes them (WP-150, §6.21).
const MINIMAL_FLAGS: u32 = DiFlags::PROTOTYPED.0
    | DiFlags::ARTIFICIAL.0
    | DiFlags::NO_RETURN.0
    | DiFlags::THUNK.0
    | DiFlags::ALL_CALLS_DESCRIBED.0;
const MINIMAL_SP_FLAGS: u32 =
    SpFlags::DEFINITION.0 | SpFlags::LOCAL_TO_UNIT.0 | SpFlags::OPTIMIZED.0;

/// The parent of a subprogram DIE in a `LineTablesOnly` unit: the unit
/// (`getOrCreateSubprogramDIE(SP, Minimal)`,
/// `constructAbstractSubprogramScopeDIE`), whatever its scope.
pub(crate) fn minimal_parent(
    plan: &UnitPlan<'_>,
    sp_id: DiId,
    sp: &DiSubprogram<'_>,
) -> Result<u32, Error> {
    if !sp.sp_flags.contains(SpFlags::DEFINITION) || sp.unit != Some(plan.unit) {
        return Err(unsupported(sp_id, "debug:subprogram-not-definition"));
    }
    if !sp.thrown_types.is_empty()
        || sp.sp_flags.0 & !MINIMAL_SP_FLAGS != 0
        || sp.flags.0 & !MINIMAL_FLAGS != 0
    {
        return Err(unsupported(sp_id, "debug:subprogram-attribute"));
    }
    Ok(0)
}

impl<'a> Writer<'a> {
    /// A function's subprogram DIE in a `LineTablesOnly` unit (family 89):
    /// `low_pc`, `high_pc`, then `name` (`applySubprogramAttributes` with
    /// `SkipSPAttributes`), or `DW_AT_abstract_origin` when the function
    /// also has an abstract instance (`finishSubprogramDefinition`).
    /// `code` is the function's address index and size.
    pub(crate) fn minimal_subprogram(
        &mut self,
        plan: &UnitPlan<'a>,
        sp_id: DiId,
        sp: &DiSubprogram<'a>,
        (index, size): (u32, u64),
        calls: bool,
        children: bool,
    ) -> Result<(), Error> {
        let origin = plan.inline.die(sp_id);
        let name = origin.is_none() && !sp.name.is_empty();
        let mask = u32::from(name) | u32::from(origin.is_some()) << 1;
        // WP-150: `DW_AT_call_all_calls` (§6.21) for one that describes
        // its calls.
        let family = crate::call_sites::family(fam::SUBPROGRAM_MINIMAL, calls);
        self.abbrev(code(family, mask, children));
        self.addr_attrs(index, size);
        // With `calls`, `DW_AT_call_all_calls` is `DW_FORM_flag_present`:
        // no bytes.
        if name {
            self.strx(sp.name)?;
        }
        if let Some(o) = origin {
            self.ref4(o);
        }
        Ok(())
    }
}
