//! The declarations a definition's `DW_AT_specification` refers to, and
//! the definitions' late attributes (`spec/dwarf.md` §6.9, WP-059), as
//! `llc`'s `getOrCreateSubprogramDIE` and
//! `applySubprogramDefinitionAttributes` write them:
//!
//! - a member function's declaration that its class does not list (a
//!   template's instance, an implicit member) is made when its definition
//!   is, appended to the class;
//! - a declaration scoped in a namespace or the unit (an out-of-line
//!   definition's, an imported function's) is made in that scope, with
//!   the member declaration's attributes;
//! - a definition whose return type differs from its declaration's has
//!   `DW_AT_type` before `DW_AT_decl_file`;
//! - a definition's template parameters are its last children, made when
//!   the plan is sealed (`finishSubprogramDefinitions`).

use hod_ir::{DiId, DiKind, DiNode, DiSubprogram, SpFlags};

use crate::Error;
use crate::abbrev::{code, fam};
use crate::die::{Kind, NONE, UnitPlan, Writer, bad, unsupported};
use crate::leb::write_uleb;
use crate::scope::DECLARATION_SCOPE;
use crate::types::{Task, Want};

impl UnitPlan<'_> {
    /// The DIE of function declaration `decl`, made in its context if it
    /// has none (`getOrCreateSubprogramDIE` of a declaration); `from`
    /// names the reference.
    pub(crate) fn declaration_die(&mut self, from: DiId, decl: DiId) -> Result<u32, Error> {
        if self.slot[decl.index()] != NONE {
            return Ok(self.slot[decl.index()]);
        }
        let DiNode::Subprogram(d) = self.node_of(from, "declaration", decl)? else {
            return Err(bad(from, "declaration"));
        };
        match d.scope.map(|s| (s, self.debug.find_kind(s))) {
            Some((class, Some(DiKind::CompositeType))) => {
                // The class (with the members it lists), then the
                // declaration if it is not among them, appended.
                self.reference_type(from, "scope", class)?;
                if self.slot[decl.index()] == NONE {
                    self.reference_method(from, decl)?;
                }
            }
            Some((_, Some(DiKind::Namespace | DiKind::CompileUnit | DiKind::File))) | None => {
                if d.sp_flags.contains(SpFlags::DEFINITION)
                    || d.sp_flags.0 & 3 != 0
                    || d.declaration.is_some()
                    || !d.thrown_types.is_empty()
                {
                    return Err(unsupported(decl, DECLARATION_SCOPE));
                }
                if let Some(ty) = d.ty
                    && self.debug.find_kind(ty) != Some(DiKind::SubroutineType)
                {
                    return Err(bad(decl, "type"));
                }
                let mut chain = Vec::new();
                let parent = self.scope_parent(decl, d.scope, &mut chain)?;
                let die = self.push(decl, parent, Kind::Method);
                let mut work = std::mem::take(&mut self.build.work);
                work.clear();
                self.open(die, &mut work);
                Self::template_tasks(die, d.template_params, &mut work);
                let result = self.drain(&mut work);
                self.build.work = work;
                result?;
            }
            Some(_) => return Err(unsupported(decl, DECLARATION_SCOPE)),
        }
        Ok(self.slot[decl.index()])
    }

    /// A member function's declaration that its class does not list: the
    /// class's last child.
    pub(crate) fn reference_method(&mut self, from: DiId, decl: DiId) -> Result<(), Error> {
        self.ensure_want(from, decl, Want::Method).map(|_| ())
    }

    /// A static member's declaration (`getOrCreateStaticMemberDIE`): its
    /// DIE, appended to its class if the class does not list it.
    pub(crate) fn reference_static_member(&mut self, from: DiId, decl: DiId) -> Result<u32, Error> {
        self.ensure_want(from, decl, Want::StaticMember)
    }

    fn ensure_want(&mut self, from: DiId, node: DiId, want: Want) -> Result<u32, Error> {
        let mut work = std::mem::take(&mut self.build.work);
        work.clear();
        work.push(Task::Ensure { from, field: "declaration", node, want });
        let result = self.drain(&mut work);
        self.build.work = work;
        result?;
        Ok(self.slot[node.index()])
    }

    /// The return type a definition writes as `DW_AT_type` beside its
    /// `DW_AT_specification`: its own when it differs from the
    /// declaration's (`applySubprogramDefinitionAttributes`).
    pub(crate) fn definition_type(
        &self,
        sp_id: DiId,
        sp: &DiSubprogram<'_>,
    ) -> Result<Option<DiId>, Error> {
        let Some(decl) = sp.declaration else { return Ok(None) };
        let DiNode::Subprogram(d) = self.node_of(sp_id, "declaration", decl)? else {
            return Err(bad(sp_id, "declaration"));
        };
        let types = |s: &DiSubprogram<'_>| match s.ty.map(|t| self.debug.node(t)) {
            Some(DiNode::SubroutineType(t)) => t.types.first().copied(),
            _ => None,
        };
        Ok(match (types(sp), types(&d)) {
            (Some(def), Some(dec)) if def != DiId::NONE && def != dec => Some(def),
            _ => None,
        })
    }

    /// What `finishSubprogramDefinitions` adds to definition `i` of the
    /// plan before its return type: its type beside a specification, then
    /// its template parameters (§6.9).
    pub(crate) fn seal_definition(&mut self, i: usize) -> Result<(), Error> {
        let die = self.funcs[i].die;
        let sp_id = self.dies[die as usize].node;
        let DiNode::Subprogram(sp) = self.debug.node(sp_id) else { return Ok(()) };
        if let Some(t) = self.definition_type(sp_id, &sp)? {
            self.reference_type(sp_id, "type", t)?;
        }
        self.add_template_params(die, sp.template_params)
    }
}

impl<'a> Writer<'a> {
    /// Writes a definition with a declaration whose return type differs
    /// (§6.9); `index` is its `.debug_addr` entry, its frame base is in
    /// `self.expr`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn typed_definition_die(
        &mut self,
        plan: &UnitPlan<'a>,
        sp_id: DiId,
        sp: &DiSubprogram<'a>,
        ty: DiId,
        index: u32,
        size: u64,
        object_pointer: u32,
        children: bool,
    ) -> Result<(), Error> {
        let b = u32::from;
        let decl = sp.declaration.ok_or(bad(sp_id, "declaration"))?;
        let DiNode::Subprogram(d) = plan.debug.node(decl) else {
            return Err(bad(sp_id, "declaration"));
        };
        let spec = plan.die_of(decl).ok_or(bad(sp_id, "declaration"))?;
        let file = sp.file.and_then(|f| plan.file_index(f));
        let new_file = file != d.file.and_then(|f| plan.file_index(f));
        let new_line = sp.line != d.line;
        let link = !sp.linkage_name.is_empty() && sp.linkage_name != d.linkage_name;
        let op = object_pointer != NONE;
        if new_file {
            let mask = b(op) | 1 << 1 | 1 << 2 | b(new_line) << 3 | b(link) << 4;
            self.abbrev(code(fam::SUBPROGRAM_SPEC_ANY, mask, children));
        } else {
            self.abbrev(code(
                fam::SUBPROGRAM_SPEC_TYPED,
                b(op) | b(new_line) << 1 | b(link) << 2,
                children,
            ));
        }
        write_uleb(&mut self.info, u64::from(index));
        self.info.extend_from_slice(&(size as u32).to_le_bytes());
        write_uleb(&mut self.info, self.expr.len() as u64);
        self.info.extend_from_slice(&self.expr);
        if op {
            self.ref4(object_pointer);
        }
        self.ref_to(plan, sp_id, ty)?;
        if new_file {
            write_uleb(&mut self.info, u64::from(file.unwrap_or(0)));
        }
        if new_line {
            write_uleb(&mut self.info, u64::from(sp.line));
        }
        if link {
            self.strx(sp.linkage_name)?;
        }
        self.ref4(spec);
        Ok(())
    }
}
