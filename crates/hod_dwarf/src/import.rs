//! Imported entities (`spec/dwarf.md` §6.11, WP-059): `using namespace`
//! (`DW_TAG_imported_module`), `using` declarations and namespace aliases
//! (`DW_TAG_imported_declaration`), as `llc`'s
//! `getOrCreateImportedEntityDIE` writes them at the end of the module: a
//! unit's (`imports:`) in its namespace or the unit, then each function's
//! (its `retainedNodes`) as the function's last children, before the
//! template parameters `finishSubprogramDefinitions` adds. The context is
//! made first, then the entity's DIE if it has none, then the import's.

use hod_ir::{DiId, DiKind, DiNode, DiSubprogram, ExprId, SpFlags};

use crate::Error;
use crate::abbrev::{code, fam, tag};
use crate::die::{Kind, NONE, UnitPlan, Writer, bad, unsupported};
use crate::scope::{IMPORT, IMPORT_ENTITY, IMPORT_SCOPE};

/// Where a function's import goes ([`UnitPlan::import_parent`]).
enum BlockOf {
    Function,
    /// The DIE of its block.
    Block(u32),
    /// Its block has no code: the import is not written.
    NoCode,
}

impl UnitPlan<'_> {
    /// The imported entities of the unit, then of each function, in
    /// `llc`'s order (§6.11): the first step of [`UnitPlan::seal`].
    pub(crate) fn seal_imports(&mut self) -> Result<(), Error> {
        let mut chain = Vec::new();
        for &ie in self.cu.imports {
            self.import(ie, None, &mut chain)?;
        }
        let mut in_blocks = Vec::new();
        for i in 0..self.funcs.len() {
            let die = self.funcs[i].die;
            let sp = self.dies[die as usize].node;
            let DiNode::Subprogram(s) = self.debug.node(sp) else { continue };
            // `DeferredLocalDecls`: the function's own scope first, then
            // its blocks depth first, which is the order of their DIEs
            // (WP-094, §6.13); in each, in `retainedNodes` order.
            in_blocks.clear();
            for (at, &n) in s.retained_nodes.iter().enumerate() {
                if self.debug.find_kind(n) != Some(DiKind::ImportedEntity) {
                    continue;
                }
                match self.import_parent(n, sp)? {
                    BlockOf::Function => {
                        self.import(n, Some((sp, die)), &mut chain)?;
                    }
                    BlockOf::Block(b) => in_blocks.push((b, at, n)),
                    BlockOf::NoCode => {}
                }
            }
            in_blocks.sort_unstable();
            for &(b, _, n) in &in_blocks {
                self.import(n, Some((self.dies[b as usize].node, b)), &mut chain)?;
            }
        }
        Ok(())
    }

    /// Where an import that function `sp` retains goes (WP-094, §6.13):
    /// the function's DIE when it is scoped in the function; the DIE of
    /// its lexical block (written because it has an import,
    /// [`UnitPlan::import_blocks`]); nowhere when that block has no code
    /// (`llc` defers the imports of the scopes it visits only). An import
    /// scoped in a block file, in another function's block, or in a
    /// function with an abstract instance is refused.
    fn import_parent(&self, ie: DiId, sp: DiId) -> Result<BlockOf, Error> {
        let DiNode::ImportedEntity(e) = self.node_of(sp, "retainedNodes", ie)? else {
            return Err(bad(sp, "retainedNodes"));
        };
        let Some(scope) = e.scope else { return Err(unsupported(ie, IMPORT_SCOPE)) };
        if scope == sp {
            return Ok(BlockOf::Function);
        }
        if self.debug.find_kind(scope) != Some(DiKind::LexicalBlock)
            || self.owner_of(scope)? != sp
            || self.inline.die(sp).is_some()
        {
            return Err(unsupported(ie, IMPORT_SCOPE));
        }
        Ok(match self.slot[scope.index()] {
            NONE => BlockOf::NoCode,
            b => BlockOf::Block(b),
        })
    }

    /// Calls `mark` with each lexical block that function `sp` has an
    /// import in: `llc`'s `skipLexicalScope` writes such a block even
    /// without variables (WP-094, §6.13). The hook of
    /// `UnitPlan::collect_entities`.
    pub(crate) fn import_blocks(&self, sp: &DiSubprogram<'_>, mut mark: impl FnMut(DiId)) {
        for &n in sp.retained_nodes {
            if let Some(DiKind::ImportedEntity) = self.debug.find_kind(n)
                && let DiNode::ImportedEntity(e) = self.debug.node(n)
                && let Some(scope) = e.scope
                && self.debug.find_kind(scope) == Some(DiKind::LexicalBlock)
            {
                mark(scope);
            }
        }
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
        // `getOrCreateContextDIE(scope)`: the function's or its block's
        // DIE ([`UnitPlan::import_parent`]).
        let parent = match local {
            Some((scope, die)) if e.scope == Some(scope) => die,
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
    /// Writes an imported entity (§6.11).
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
