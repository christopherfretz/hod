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

use hod_ir::{
    Body, DebugInfo, DiId, DiKind, DiNode, DiSubprogram, EmissionKind, ExprId, Module, SpFlags,
    SymbolKind, Value,
};

use crate::Error;
use crate::abbrev::tag;
use crate::die::{UnitPlan, unsupported};
use crate::func::{DEF_FLAGS, DEF_SP_FLAGS, check_expr};
use crate::inline::DEBUG_LOCAL_STATIC;
use crate::scope::{IMPORT, IMPORT_SCOPE};

/// A function-local type whose function's DIE cannot be made where `llc`
/// makes it (§6.4, §6.12).
pub const LOCAL_TYPE_SCOPE: &str = "debug:local-type-scope";

impl UnitPlan<'_> {
    /// The parent DIE of type `of`, local to subprogram `scope`, which has
    /// no DIE yet: the function's early DIE.
    pub(crate) fn local_type_parent(&mut self, of: DiId, scope: DiId) -> Result<u32, Error> {
        // WP-098: a type local to a function that is only inlined, reached
        // once its abstract instance exists, goes under it (§6.14).
        if let Some(d) = crate::local_type::abstract_parent(self, scope) {
            return Ok(d);
        }
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

impl UnitPlan<'_> {
    /// What `applySubprogramDefinitionAttributes` gives an abstract
    /// instance when `constructAbstractSubprogramScopeDIE` makes it, before
    /// its return type and its children: the template parameters (WP-087;
    /// a concrete definition gets them last, [`UnitPlan::seal_definition`]).
    /// A definition whose return type differs from its declaration's is
    /// refused: the abstract member's family has no `DW_AT_type`.
    pub(crate) fn abstract_template_params(
        &mut self,
        sp_id: DiId,
        sp: &DiSubprogram<'_>,
        die: u32,
    ) -> Result<(), Error> {
        if self.definition_type(sp_id, sp)?.is_some() {
            return Err(unsupported(sp_id, "debug:specification-type"));
        }
        self.add_template_params(die, sp.template_params)
    }
}

/// The function-DIE refusals that depend on the metadata alone (WP-094,
/// `spec/dwarf.md` §6.13), for every function with a body whose
/// subprogram is in a `FullDebug` unit: the same conditions
/// [`UnitPlan::add_function`] and [`UnitPlan::seal`] refuse, checked
/// before any code is generated, so that a module hod declines costs no
/// code generation. `Err((index in bodies, stable name))` for the first
/// body that has one. What needs the code (which scopes it covers, what
/// is inlined where, the frame) stays where it is; `spec/dwarf.md`
/// §6.13 lists it.
///
/// # Errors
/// As above.
pub fn check_functions(module: &Module, bodies: &[Body]) -> Result<(), (usize, &'static str)> {
    let debug = &module.debug;
    if debug.compile_units().is_empty() {
        return Ok(());
    }
    let mut seen: Vec<(u32, usize)> = Vec::with_capacity(bodies.len());
    for (i, body) in bodies.iter().enumerate() {
        let SymbolKind::Function { subprogram: Some(sp_id), .. } =
            module.symbol(body.symbol()).kind()
        else {
            continue;
        };
        let DiNode::Subprogram(sp) = debug.node(sp_id) else { continue };
        if !full_debug_unit(debug, &sp) {
            continue;
        }
        check_subprogram(&sp).map_err(|name| (i, name))?;
        seen.push((sp_id.raw(), i));
        check_records(debug, body).map_err(|name| (i, name))?;
        check_imports(debug, sp_id, &sp).map_err(|name| (i, name))?;
    }
    // Two bodies of one subprogram: the second is refused.
    seen.sort_unstable();
    if let Some(w) = seen.windows(2).filter(|w| w[0].0 == w[1].0).map(|w| w[1].1).min() {
        return Err((w, "debug:subprogram-twice"));
    }
    Ok(())
}

/// Whether `sp`'s unit is one of the module's and writes DIEs for it.
fn full_debug_unit(debug: &DebugInfo, sp: &DiSubprogram<'_>) -> bool {
    let Some(unit) = sp.unit else { return false };
    debug.compile_units().contains(&unit)
        && matches!(debug.node(unit), DiNode::CompileUnit(cu) if cu.emission_kind == EmissionKind::FullDebug)
}

/// `UnitPlan::subprogram_parent`'s refusals of the subprogram itself.
fn check_subprogram(sp: &DiSubprogram<'_>) -> Result<(), &'static str> {
    if !sp.sp_flags.contains(SpFlags::DEFINITION) {
        return Err("debug:subprogram-not-definition");
    }
    crate::call_sites::check(sp)?; // WP-098: §6.14
    if !sp.thrown_types.is_empty()
        || sp.sp_flags.0 & !DEF_SP_FLAGS != 0
        || (sp.declaration.is_none() && sp.flags.0 & !DEF_FLAGS != 0)
    {
        return Err("debug:subprogram-attribute");
    }
    Ok(())
}

/// `UnitPlan::collect_entities`' refusals of a record, made whether or
/// not its scope has code.
fn check_records(debug: &DebugInfo, body: &Body) -> Result<(), &'static str> {
    let declares = body.declares().iter().filter(|d| d.address != Value::NONE);
    let vars = body.slot_vars().iter().map(|v| (v.var, v.expr));
    for (var, expr) in vars.chain(declares.map(|d| (d.var, d.expr))) {
        check_variable(debug, var, expr)?;
    }
    // WP-098: every label is written (§6.14); none is refused here.
    Ok(())
}

fn check_variable(debug: &DebugInfo, var: DiId, expr: ExprId) -> Result<(), &'static str> {
    if debug.find_kind(var) != Some(DiKind::LocalVariable) {
        return Ok(());
    }
    let DiNode::LocalVariable(n) = debug.node(var) else { return Ok(()) };
    if n.ty.is_none() {
        return Err("debug:variable-without-type");
    }
    if n.align != 0 && (n.arg != 0 || n.name.is_empty()) {
        return Err("debug:variable-alignment");
    }
    check_expr(debug, expr)
}

/// `UnitPlan::import_parent`'s and `import`'s refusals of a function's
/// imports that do not depend on its code: an import with no scope, or
/// scoped in a block file or in another function, and an import in the
/// function's own scope that is not one hod writes. (One in a block
/// without code is dropped before its shape is looked at, and one of a
/// function with an abstract instance depends on what is inlined.)
fn check_imports(
    debug: &DebugInfo,
    sp_id: DiId,
    sp: &DiSubprogram<'_>,
) -> Result<(), &'static str> {
    for &n in sp.retained_nodes {
        if debug.find_kind(n) != Some(DiKind::ImportedEntity) {
            continue;
        }
        let DiNode::ImportedEntity(e) = debug.node(n) else { continue };
        let Some(scope) = e.scope else { return Err(IMPORT_SCOPE) };
        if scope == sp_id {
            let named_module = e.tag == tag::IMPORTED_MODULE && !e.name.is_empty();
            if !matches!(e.tag, tag::IMPORTED_MODULE | tag::IMPORTED_DECLARATION)
                || !e.elements.is_empty()
                || named_module
                || (e.line != 0 && e.file.is_none())
            {
                return Err(IMPORT);
            }
        } else if debug.find_kind(scope) != Some(DiKind::LexicalBlock)
            || owner(debug, scope) != Some(sp_id)
        {
            return Err(IMPORT_SCOPE);
        }
    }
    Ok(())
}

/// The subprogram block `s` is in, through blocks and block files;
/// `None` for a broken chain (the plan reports it).
fn owner(debug: &DebugInfo, mut s: DiId) -> Option<DiId> {
    for _ in 0..debug.node_count() {
        match debug.node(s) {
            DiNode::Subprogram(_) => return Some(s),
            DiNode::LexicalBlock(b) => s = b.scope?,
            DiNode::LexicalBlockFile(f) => s = f.scope?,
            _ => return None,
        }
    }
    None
}
