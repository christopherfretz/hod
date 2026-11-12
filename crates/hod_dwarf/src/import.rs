//! Imported entities (`spec/dwarf.md` §6.9, WP-059): `using namespace`
//! (`DW_TAG_imported_module`), `using` declarations and namespace aliases
//! (`DW_TAG_imported_declaration`), as `llc`'s
//! `getOrCreateImportedEntityDIE` writes them at the end of the module: a
//! unit's (`imports:`) in its namespace or the unit, then each function's
//! (its `retainedNodes`) as the function's last children, before the
//! template parameters `finishSubprogramDefinitions` adds. The context is
//! made first, then the entity's DIE if it has none, then the import's.

use hod_ir::{DiId, DiKind, DiNode, ExprId, SpFlags};

use crate::Error;
use crate::abbrev::{code, fam, tag};
use crate::die::{Kind, NONE, UnitPlan, Writer, bad, unsupported};
use crate::scope::{IMPORT, IMPORT_ENTITY, IMPORT_SCOPE};

impl UnitPlan<'_> {
    /// The imported entities of the unit, then of each function, in
    /// `llc`'s order (§6.9): the first step of [`UnitPlan::seal`].
    pub(crate) fn seal_imports(&mut self) -> Result<(), Error> {
        let mut chain = Vec::new();
        for &ie in self.cu.imports {
            self.import(ie, None, &mut chain)?;
        }
        for i in 0..self.funcs.len() {
            let die = self.funcs[i].die;
            let sp = self.dies[die as usize].node;
            let DiNode::Subprogram(s) = self.debug.node(sp) else { continue };
            for &n in s.retained_nodes {
                if self.debug.find_kind(n) == Some(DiKind::ImportedEntity) {
                    self.import(n, Some((sp, die)), &mut chain)?;
                }
            }
        }
        Ok(())
    }

    /// The DIE of imported entity `ie`: of the function `local` (its node
    /// and DIE) for a function's.
    fn import(
        &mut self,
        ie: DiId,
        local: Option<(DiId, u32)>,
        chain: &mut Vec<DiId>,
    ) -> Result<u32, Error> {
        if self.slot[ie.index()] != NONE {
            return Ok(self.slot[ie.index()]);
        }
        let from = local.map_or(self.unit, |l| l.0);
        let DiNode::ImportedEntity(e) = self.node_of(from, "imports", ie)? else {
            return Err(bad(from, "imports"));
        };
        let named_module = e.tag == tag::IMPORTED_MODULE && !e.name.is_empty();
        if !matches!(e.tag, tag::IMPORTED_MODULE | tag::IMPORTED_DECLARATION)
            || !e.elements.is_empty()
            || named_module
            || (e.line != 0 && e.file.is_none())
        {
            return Err(unsupported(ie, IMPORT));
        }
        // `getOrCreateContextDIE(scope)`.
        let parent = match local {
            Some((sp, die)) if e.scope == Some(sp) => die,
            Some(_) => return Err(unsupported(ie, IMPORT_SCOPE)),
            None => self.scope_parent(ie, e.scope, chain).map_err(|e| match e {
                Error::Unsupported { node, .. } => Error::Unsupported { node, what: IMPORT_SCOPE },
                e => e,
            })?,
        };
        let entity = e.entity.ok_or(bad(ie, "entity"))?;
        self.import_target(ie, entity, chain)?;
        Ok(self.push(ie, parent, Kind::Imported))
    }

    /// The entity's DIE, made if it has none (`constructImportedEntityDIE`).
    fn import_target(
        &mut self,
        ie: DiId,
        entity: DiId,
        chain: &mut Vec<DiId>,
    ) -> Result<(), Error> {
        let kind = self.debug.find_kind(entity).ok_or(bad(ie, "entity"))?;
        if self.slot[entity.index()] != NONE {
            return Ok(());
        }
        match (kind, self.debug.node(entity)) {
            (DiKind::Namespace, _) => {
                self.scope_parent(ie, Some(entity), chain)?;
            }
            (DiKind::BasicType | DiKind::DerivedType | DiKind::CompositeType, _) => {
                self.reference_type(ie, "entity", entity)?;
            }
            (DiKind::Subprogram, DiNode::Subprogram(sp))
                if !sp.sp_flags.contains(SpFlags::DEFINITION) =>
            {
                self.declaration_die(ie, entity)?;
            }
            (DiKind::GlobalVariable, DiNode::GlobalVariable(v)) => {
                if let Some(s) = v.scope
                    && matches!(
                        self.debug.find_kind(s),
                        Some(DiKind::Subprogram | DiKind::LexicalBlock | DiKind::LexicalBlockFile)
                    )
                {
                    return Err(unsupported(ie, IMPORT_ENTITY));
                }
                // `getOrCreateGlobalVariableDIE(GV, {})`: no expression.
                self.global_die(entity, &v, ExprId::EMPTY, false, chain)?;
            }
            _ => return Err(unsupported(ie, IMPORT_ENTITY)),
        }
        Ok(())
    }
}

impl<'a> Writer<'a> {
    /// Writes an imported entity (§6.9).
    pub(crate) fn imported_die(&mut self, plan: &UnitPlan<'a>, node: DiId) -> Result<(), Error> {
        let DiNode::ImportedEntity(e) = plan.debug.node(node) else {
            return Err(bad(node, "imports"));
        };
        let decl = Writer::decl_of(plan, node, e.file, e.line)?;
        let b = u32::from;
        if e.tag == tag::IMPORTED_MODULE {
            self.abbrev(code(fam::IMPORTED_MODULE, b(decl.is_some()), false));
        } else {
            let mask = b(decl.is_some()) | b(!e.name.is_empty()) << 1;
            self.abbrev(code(fam::IMPORTED_DECLARATION, mask, false));
        }
        self.decl(decl);
        self.ref_to(plan, node, e.entity.unwrap_or(DiId::NONE))?;
        self.opt_name(e.name)?;
        Ok(())
    }
}
