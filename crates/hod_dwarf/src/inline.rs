//! Inlined subroutines, abstract instances and functions' statics
//! (`spec/dwarf.md` §6.9, WP-085).
//!
//! `llc`'s `LexicalScopes` keys a function's scopes by `(scope,
//! inlinedAt)`: an inlined subprogram's scope has the call's scope as
//! its parent, and every scope reached through an `inlinedAt` also gets
//! an *abstract* scope, keyed by the scope alone. [`crate::func`] walks
//! the keys; this module keeps the abstract scopes of the function being
//! added ([`FnInline`]) and, per unit ([`Inline`]), the abstract DIEs:
//! one per inlined subprogram, written once, by the first function that
//! inlines it (`constructAbstractSubprogramScopeDIE`), with the
//! parameters, locals, labels and blocks that function's inlined code
//! has. Concrete DIEs name them by `DW_AT_abstract_origin`, which the
//! writer decides when it writes them: `llc` decides at the end of the
//! module (`finishSubprogramDefinitions`, `finishEntityDefinitions`).
//!
//! Nothing here is touched by a function that inlines nothing, beyond an
//! empty map lookup per entity.

use std::collections::{HashMap, HashSet};

use hod_ir::{DiFlags, DiId, DiKind, DiNode, LocId, Module, SpFlags, SymbolKind};

use crate::Error;
use crate::abbrev::{code, fam};
use crate::die::{Kind, NONE, UnitPlan, Writer, bad, unsupported};
use crate::func::{FnData, LocalData, LocalWhat};
use crate::leb::write_uleb;

/// An abstract instance of a member function whose declaration is not
/// one hod writes yet (WP-059's out-of-line definitions).
pub const DEBUG_INLINED_MEMBER: &str = "debug:inlined-member";
/// A call whose location is a block file with a discriminator: `llc`
/// writes `DW_AT_GNU_discriminator` on the inlined subroutine.
pub const DEBUG_INLINED_DISCRIMINATOR: &str = "debug:inlined-discriminator";
/// Code inlined from a subprogram of another compile unit.
pub const DEBUG_INLINED_FOREIGN_UNIT: &str = "debug:inlined-foreign-unit";
/// An inlined subprogram with `retainedNodes`: `llc` makes abstract
/// entities of them that no record names.
pub const DEBUG_RETAINED_NODES: &str = "debug:retained-nodes";
/// A type, or a variable a type names, local to a function that has an
/// abstract instance: where `llc` puts it is not written yet.
pub const DEBUG_INLINED_LOCAL_TYPE: &str = "debug:inlined-local-type";
/// A function's `static` that is scoped in a block, or in a subprogram
/// that is not a definition of this unit, or whose function has no code.
pub const DEBUG_LOCAL_STATIC: &str = "debug:local-static";

/// An abstract entity that has no DIE: made by a function after its
/// subprogram's abstract instance was written.
const NO_DIE: u32 = u32::MAX - 1;
/// `DW_INL_inlined`.
const DW_INL_INLINED: u8 = 1;
const DW_RLE_BASE_ADDRESSX: u8 = 0x01;
const DW_RLE_OFFSET_PAIR: u8 = 0x04;

/// The subprograms of `module`'s functions that have bodies, sorted: a
/// static of one goes under its concrete DIE, of any other under its
/// abstract instance (`llc` 22's `getOrCreateContextDIE`).
pub(crate) fn bodies(module: &Module) -> Vec<u32> {
    let mut out: Vec<u32> = module
        .symbols()
        .filter_map(|s| match s.kind() {
            SymbolKind::Function { subprogram: Some(sp), has_body: true, .. } => Some(sp.raw()),
            _ => None,
        })
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// The unit's abstract instances (WP-085).
#[derive(Clone, Debug, Default)]
pub(crate) struct Inline {
    /// Per subprogram, block, variable or label node: its abstract DIE,
    /// or `NO_DIE` for an abstract entity without one.
    abs: HashMap<u32, u32>,
    /// The subprograms whose abstract instance is written.
    populated: HashSet<u32>,
    /// Concrete subprogram DIEs made for statics before their function.
    early: HashSet<u32>,
    /// See [`bodies`]; `None` for a plan without a module.
    bodies: Option<Vec<u32>>,
    /// `(abstract subprogram DIE, its `this` DIE)`, sorted when sealed.
    objptr: Vec<(u32, u32)>,
    /// `(subprogram DIE, function index)`, sorted when sealed.
    fn_by_die: Vec<(u32, u32)>,
    /// The functions' variable DIEs in `llc`'s `ConcreteEntities` order
    /// (WP-094): by function, then by record, not by DIE.
    entities: Vec<u32>,
}

impl Inline {
    pub(crate) fn new(bodies: Option<Vec<u32>>) -> Self {
        Self { bodies, ..Self::default() }
    }

    /// The abstract DIE of `node`, if it has one.
    #[inline]
    pub(crate) fn die(&self, node: DiId) -> Option<u32> {
        if self.abs.is_empty() {
            return None;
        }
        self.abs.get(&node.raw()).copied().filter(|&d| d != NO_DIE)
    }

    /// Whether the module has the code of subprogram `node`.
    pub(crate) fn has_body(&self, node: DiId) -> bool {
        self.bodies.as_ref().is_some_and(|b| b.binary_search(&node.raw()).is_ok())
    }

    /// Whether `die` was made early for statics; it is the function's
    /// from now on.
    pub(crate) fn take_early(&mut self, die: u32) -> bool {
        die != NONE && self.early.remove(&die)
    }

    /// The index in `UnitPlan::funcs` of the function of subprogram DIE
    /// `die` (sealed plans).
    pub(crate) fn fn_index(&self, die: u32) -> Option<usize> {
        let i = self.fn_by_die.binary_search_by_key(&die, |&(d, _)| d).ok()?;
        Some(self.fn_by_die[i].1 as usize)
    }

    fn object_pointer(&self, die: u32) -> Option<u32> {
        let i = self.objptr.binary_search_by_key(&die, |&(d, _)| d).ok()?;
        Some(self.objptr[i].1)
    }
}

/// The function being added: its inlined scopes' keys and its abstract
/// scopes (`LexicalScopes`' `InlinedLexicalScopeMap`,
/// `AbstractScopeMap` and `AbstractScopesList`).
#[derive(Clone, Debug, Default)]
pub(crate) struct FnInline {
    /// `(node, inlinedAt)` to the index of the scope in the function's.
    keys: HashMap<(u32, u32), u32>,
    pub(crate) kpath: Vec<(DiId, LocId)>,
    apath: Vec<DiId>,
    /// The abstract scopes, in creation order: `(node, parent)`.
    scopes: Vec<(DiId, u32)>,
    index: HashMap<u32, u32>,
    /// The abstract subprogram scopes, in creation order.
    list: Vec<u32>,
    /// The abstract entities made: `(scope, class, key, node)`, as
    /// `FnScratch::ents`.
    ents: Vec<(u32, u8, u32, DiId)>,
    /// `(variable, inlinedAt)` seen.
    seen: HashSet<(u32, u32)>,
    child_start: Vec<u32>,
    children: Vec<u32>,
    vars: Vec<bool>,
    walk: Vec<(u32, u32, u32)>,
}

impl FnInline {
    pub(crate) fn clear(&mut self) {
        self.keys.clear();
        self.scopes.clear();
        self.index.clear();
        self.list.clear();
        self.ents.clear();
        self.seen.clear();
    }

    #[inline]
    pub(crate) fn scope(&self, node: DiId, ia: LocId) -> u32 {
        self.keys.get(&(node.raw(), ia.raw())).copied().unwrap_or(NONE)
    }

    pub(crate) fn set_scope(&mut self, node: DiId, ia: LocId, index: u32) {
        self.keys.insert((node.raw(), ia.raw()), index);
    }

    /// Whether this is the first entity `(node, ia)` of the function.
    pub(crate) fn first_of(&mut self, node: DiId, ia: LocId) -> bool {
        self.seen.insert((node.raw(), ia.raw()))
    }
}

const fn has(flags: DiFlags, f: DiFlags) -> bool {
    flags.0 & f.0 != 0
}

impl UnitPlan<'_> {
    /// `getOrCreateAbstractScope`: the abstract scope of `scope` and of
    /// the blocks around it up to its subprogram, outermost first.
    pub(crate) fn abstract_scope(&self, s: &mut FnInline, scope: DiId) -> Result<(), Error> {
        let mut at = self.non_file(scope)?;
        s.apath.clear();
        let top = loop {
            if let Some(&i) = s.index.get(&at.raw()) {
                break i;
            }
            s.apath.push(at);
            if s.apath.len() > self.slot.len() {
                return Err(bad(at, "scope"));
            }
            match self.parent_scope(at)? {
                Some(p) => at = p,
                None => break NONE,
            }
        };
        let mut parent = top;
        for k in (0..s.apath.len()).rev() {
            let node = s.apath[k];
            let i = s.scopes.len() as u32;
            s.scopes.push((node, parent));
            s.index.insert(node.raw(), i);
            if parent == NONE {
                self.check_inlined(node)?;
                s.list.push(i);
            }
            parent = i;
        }
        Ok(())
    }

    /// What an inlined subprogram must be for its instances to be
    /// written.
    fn check_inlined(&self, sp_id: DiId) -> Result<(), Error> {
        let DiNode::Subprogram(sp) = self.node_of(sp_id, "scope", sp_id)? else {
            return Err(bad(sp_id, "scope"));
        };
        if sp.unit != Some(self.unit) {
            return Err(unsupported(sp_id, DEBUG_INLINED_FOREIGN_UNIT));
        }
        if !sp.retained_nodes.is_empty() {
            return Err(unsupported(sp_id, DEBUG_RETAINED_NODES));
        }
        Ok(())
    }

    /// The key of the scope an inlined subprogram's scope `(sp, ia)` is
    /// in: its call's. When the key is `new`, the call's scope gets its
    /// abstract scope first if it is itself inlined
    /// (`getOrCreateLexicalScope(InlinedAt)`).
    pub(crate) fn call_parent(
        &self,
        s: &mut FnInline,
        sp: DiId,
        ia: LocId,
        new: bool,
    ) -> Result<(DiId, LocId), Error> {
        let call = self.debug.location(ia).ok_or(bad(sp, "inlinedAt"))?;
        if let Some(DiKind::LexicalBlockFile) = self.debug.find_kind(call.scope)
            && let DiNode::LexicalBlockFile(f) = self.debug.node(call.scope)
            && f.discriminator != 0
        {
            return Err(unsupported(sp, DEBUG_INLINED_DISCRIMINATOR));
        }
        if new && call.inlined_at != LocId::NONE {
            self.abstract_scope(s, call.scope)?;
        }
        Ok((self.non_file(call.scope)?, call.inlined_at))
    }

    /// The subprogram whose code an inlined location's outermost call is
    /// in.
    pub(crate) fn call_root(&self, mut ia: LocId) -> Result<DiId, Error> {
        for _ in 0..=self.debug.loc_count() {
            let Some(l) = self.debug.location(ia) else { break };
            if l.inlined_at == LocId::NONE {
                return self.owner_of(self.non_file(l.scope)?);
            }
            ia = l.inlined_at;
        }
        Err(bad(self.unit, "inlinedAt"))
    }

    /// `ensureAbstractEntityIsCreatedIfScoped`: an entity whose scope has
    /// an abstract scope in this function gets an abstract entity, once
    /// per unit.
    pub(crate) fn abstract_entity(
        &mut self,
        s: &mut FnInline,
        node: DiId,
        scope: DiId,
        class: u8,
        key: u32,
    ) {
        let Some(&i) = s.index.get(&scope.raw()) else { return };
        if self.inline.abs.contains_key(&node.raw()) {
            return;
        }
        self.inline.abs.insert(node.raw(), NO_DIE);
        s.ents.push((i, class, key, node));
    }

    /// The abstract instances of the subprograms the function inlines,
    /// in the order their abstract scopes were made, each written once
    /// per unit (`constructAbstractSubprogramScopeDIE`).
    pub(crate) fn abstract_instances(&mut self, s: &mut FnInline) -> Result<(), Error> {
        if s.list.is_empty() {
            return Ok(());
        }
        s.ents.sort_unstable();
        // Children of each abstract scope, in creation order, and
        // whether it has variables (`needToEmitLexicalScope`).
        let n = s.scopes.len();
        s.child_start.clear();
        s.child_start.resize(n + 1, 0);
        for &(_, p) in &s.scopes {
            if p != NONE {
                s.child_start[p as usize + 1] += 1;
            }
        }
        for i in 1..=n {
            s.child_start[i] += s.child_start[i - 1];
        }
        s.children.clear();
        s.children.resize(s.child_start[n] as usize, 0);
        let mut fill = s.child_start[..n].to_vec();
        for (i, &(_, p)) in s.scopes.iter().enumerate() {
            if p != NONE {
                s.children[fill[p as usize] as usize] = i as u32;
                fill[p as usize] += 1;
            }
        }
        s.vars.clear();
        s.vars.resize(n, false);
        for &(sc, class, ..) in &s.ents {
            if class != 2 {
                s.vars[sc as usize] = true;
            }
        }
        for k in 0..s.list.len() {
            let root = s.list[k];
            let sp = s.scopes[root as usize].0;
            if !self.inline.populated.insert(sp.raw()) {
                continue;
            }
            let die = match self.inline.die(sp) {
                Some(d) => d,
                None => self.create_abstract(sp)?,
            };
            self.abstract_entities(s, root, die, die)?;
            s.walk.clear();
            s.walk.push((root, die, s.child_start[root as usize]));
            while let Some(&(sc, parent, cursor)) = s.walk.last() {
                if cursor == s.child_start[sc as usize + 1] {
                    s.walk.pop();
                    continue;
                }
                if let Some(top) = s.walk.last_mut() {
                    top.2 += 1;
                }
                let child = s.children[cursor as usize];
                let node = s.scopes[child as usize].0;
                let d = if s.vars[child as usize] {
                    let b = self.push_anon(node, parent, Kind::AbstractBlock);
                    self.inline.abs.insert(node.raw(), b);
                    b
                } else {
                    parent
                };
                self.abstract_entities(s, child, d, die)?;
                s.walk.push((child, d, s.child_start[child as usize]));
            }
        }
        Ok(())
    }

    /// The abstract DIEs of abstract scope `sc`'s entities, under `die`:
    /// each with its type, made now (`applyCommonDbgVariableAttributes`).
    fn abstract_entities(
        &mut self,
        s: &FnInline,
        sc: u32,
        die: u32,
        sp_die: u32,
    ) -> Result<(), Error> {
        let mut e = s.ents.partition_point(|x| x.0 < sc);
        while let Some(&(scope, class, _, node)) = s.ents.get(e) {
            if scope != sc {
                break;
            }
            e += 1;
            let kind = match class {
                0 => Kind::AbstractParam,
                1 => Kind::AbstractVar,
                _ => Kind::AbstractLabel,
            };
            // WP-098: the type first (`constructVariableDIE` adds the
            // variable to its scope after its attributes), so that a type
            // local to the function precedes the variable (§6.14).
            if let DiNode::LocalVariable(v) = self.debug.node(node) {
                self.reference_type(node, "type", v.ty.unwrap_or(DiId::NONE))?;
            }
            let d = self.push_anon(node, die, kind);
            self.inline.abs.insert(node.raw(), d);
            if let DiNode::LocalVariable(v) = self.debug.node(node)
                && class == 0
                && has(v.flags, DiFlags::OBJECT_POINTER)
            {
                if self.inline.objptr.last().is_some_and(|&(p, _)| p == sp_die) {
                    return Err(unsupported(node, "debug:object-pointer"));
                }
                self.inline.objptr.push((sp_die, d));
            }
        }
        Ok(())
    }

    /// `createAbstractSubprogramDIE`: the DIE in its context (a member
    /// function's after its class), its return type made now.
    fn create_abstract(&mut self, sp_id: DiId) -> Result<u32, Error> {
        let DiNode::Subprogram(sp) = self.node_of(sp_id, "scope", sp_id)? else {
            return Err(bad(sp_id, "scope"));
        };
        let parent =
            self.subprogram_parent(sp_id, &sp).map_err(|e| match e {
                Error::Unsupported {
                    node,
                    what:
                        "debug:specification"
                        | "debug:specification-type"
                        | "debug:out-of-line-definition",
                } => Error::Unsupported { node, what: DEBUG_INLINED_MEMBER },
                e => e,
            })?;
        let die = self.push_anon(sp_id, parent, Kind::AbstractSubprogram);
        self.inline.abs.insert(sp_id.raw(), die);
        // A `LineTablesOnly` unit's has only its name (WP-094, §6.13).
        if self.cu.emission_kind != hod_ir::EmissionKind::FullDebug {
            return Ok(die);
        }
        self.abstract_template_params(sp_id, &sp, die)?;
        if sp.declaration.is_none()
            && let Some(t) = self.return_type(sp_id, &sp)?
        {
            self.reference_type(sp_id, "type", t)?;
        }
        Ok(die)
    }

    /// The parent DIE of a function's static `var` scoped `scope`, made
    /// when it is new (`llc` 22, `beginModule`): the function's concrete
    /// DIE, empty until the function is added, when the module has its
    /// code; else its abstract instance.
    pub(crate) fn static_parent(&mut self, var: DiId, scope: DiId) -> Result<u32, Error> {
        if self.debug.find_kind(scope) != Some(DiKind::Subprogram) {
            return Err(unsupported(var, DEBUG_LOCAL_STATIC));
        }
        let DiNode::Subprogram(sp) = self.debug.node(scope) else {
            return Err(bad(var, "scope"));
        };
        if !sp.sp_flags.contains(SpFlags::DEFINITION) || sp.unit != Some(self.unit) {
            return Err(unsupported(var, DEBUG_LOCAL_STATIC));
        }
        if let Some(d) = self.inline.die(scope) {
            return Ok(d);
        }
        let seen = self.slot[scope.index()];
        if seen != NONE {
            return Ok(seen);
        }
        let Some(bodies) = &self.inline.bodies else {
            return Err(unsupported(var, DEBUG_LOCAL_STATIC));
        };
        if bodies.binary_search(&scope.raw()).is_ok() {
            let parent = self.subprogram_parent(scope, &sp)?;
            let die = self.push(scope, parent, Kind::Subprogram);
            self.inline.early.insert(die);
            Ok(die)
        } else {
            self.check_inlined(scope)?;
            self.create_abstract(scope)
        }
    }

    /// Records the variable DIEs the function just added made, in the
    /// order of its records (WP-094, §6.13): `llc`'s `collectEntityInfo`
    /// makes its concrete entities in the order of the frame table,
    /// which is the order of the declarations, and
    /// `finishEntityDefinitions` makes their types in that order.
    pub(crate) fn record_entities(&mut self, f: &crate::func::FunctionInput<'_>, added: bool) {
        let Some(func) = self.funcs.len().checked_sub(1).filter(|_| added) else { return };
        let start = self.locals.iter().rposition(|l| l.func as usize != func).map_or(0, |i| i + 1);
        // By node: the instances of one variable inlined twice have one
        // type, so which record is which instance does not matter.
        let mut tail: Vec<(u32, u32)> = self.locals[start..]
            .iter()
            .filter(|l| matches!(l.what, LocalWhat::Var { .. } | LocalWhat::CoroVar { .. }))
            .map(|l| (self.dies[l.die as usize].node.raw(), l.die))
            .collect();
        tail.sort_unstable();
        let mut done = vec![false; tail.len()];
        for v in f.vars {
            let node = v.node.raw();
            let mut i = tail.partition_point(|&(n, _)| n < node);
            while i < tail.len() && tail[i].0 == node && done[i] {
                i += 1;
            }
            if i < tail.len() && tail[i].0 == node {
                done[i] = true;
                self.inline.entities.push(tail[i].1);
            }
        }
        self.record_retained(f.subprogram, start);
    }

    /// After the declared variables, the retained ones without a record
    /// (WP-111), in `retainedNodes` order: `llc`'s `collectEntityInfo`
    /// makes their entities last.
    fn record_retained(&mut self, sp: DiId, start: usize) {
        let DiNode::Subprogram(sp) = self.debug.node(sp) else { return };
        for &n in sp.retained_nodes {
            let found = self.locals[start..].iter().find(|l| {
                matches!(l.what, LocalWhat::Retained) && self.dies[l.die as usize].node == n
            });
            if let Some(l) = found {
                self.inline.entities.push(l.die);
            }
        }
    }

    /// `finishEntityDefinitions`: the variables' types, in the order of
    /// [`Self::record_entities`]. One with an abstract instance names
    /// it instead (WP-085).
    pub(crate) fn finish_entities(&mut self) -> Result<(), Error> {
        for i in 0..self.inline.entities.len() {
            let d = self.dies[self.inline.entities[i] as usize];
            if let (Kind::LocalParam | Kind::LocalVar, DiNode::LocalVariable(v)) =
                (d.kind, self.debug.node(d.node))
                && self.inline.die(d.node).is_none()
            {
                self.reference_type(d.node, "type", v.ty.unwrap_or(DiId::NONE))?;
            }
        }
        Ok(())
    }

    /// What sealing checks and sorts (WP-085).
    pub(crate) fn seal_inline(&mut self) -> Result<(), Error> {
        if let Some(&die) = self.inline.early.iter().min() {
            return Err(unsupported(self.dies[die as usize].node, DEBUG_LOCAL_STATIC));
        }
        if !self.inline.abs.is_empty() {
            self.check_local_types()?;
        }
        self.inline.objptr.sort_unstable();
        self.inline.fn_by_die.clear();
        self.inline.fn_by_die.extend(self.funcs.iter().enumerate().map(|(i, f)| (f.die, i as u32)));
        self.inline.fn_by_die.sort_unstable();
        Ok(())
    }

    /// A local type of a function that has an abstract instance, or a
    /// variable of one named by a type, is refused.
    fn check_local_types(&self) -> Result<(), Error> {
        for d in &self.dies[1..] {
            let p = self.dies[d.parent as usize];
            let owner = match p.kind {
                Kind::Subprogram => p.node,
                Kind::Block => match self.debug.subprogram_of(p.node) {
                    Some(sp) => sp,
                    None => continue,
                },
                _ => continue,
            };
            let local = matches!(
                d.kind,
                Kind::LocalParam
                    | Kind::LocalVar
                    | Kind::Label
                    | Kind::Block
                    | Kind::Unspecified
                    | Kind::Inlined
                    | Kind::Variable
            );
            if !local && self.inline.die(owner).is_some() {
                return Err(unsupported(d.node, DEBUG_INLINED_LOCAL_TYPE));
            }
        }
        for &(_, target) in &self.aux {
            let d = self.dies[target as usize];
            if matches!(d.kind, Kind::LocalParam | Kind::LocalVar)
                && self.inline.die(d.node).is_some()
            {
                return Err(unsupported(d.node, DEBUG_INLINED_LOCAL_TYPE));
            }
        }
        Ok(())
    }

    /// The file of a local scope.
    fn scope_file(&self, scope: DiId) -> Option<DiId> {
        match self.debug.node(scope) {
            DiNode::Subprogram(s) => s.file,
            DiNode::LexicalBlock(b) => b.file,
            DiNode::LexicalBlockFile(f) => f.file,
            _ => None,
        }
    }
}

impl<'a> Writer<'a> {
    /// Writes a DIE of an abstract instance or an inlined subroutine.
    pub(crate) fn inline_die(
        &mut self,
        plan: &UnitPlan<'a>,
        die: u32,
        kind: Kind,
        node: DiId,
        children: bool,
    ) -> Result<(), Error> {
        let b = u32::from;
        match (kind, plan.debug.node(node)) {
            (Kind::AbstractSubprogram, DiNode::Subprogram(sp)) => {
                self.abstract_subprogram(plan, die, node, &sp, children)?;
            }
            (Kind::AbstractParam | Kind::AbstractVar, DiNode::LocalVariable(v)) => {
                let decl = Writer::decl_of(plan, node, v.file, v.line)?;
                let art = has(v.flags, DiFlags::ARTIFICIAL);
                if kind == Kind::AbstractParam {
                    let mask = b(!v.name.is_empty()) << 1 | b(decl.is_some()) << 2 | b(art) << 3;
                    self.abbrev(code(fam::FORMAL_PARAMETER, mask, false));
                    self.opt_name(v.name)?;
                } else {
                    // WP-098: a named local's alignment is written in the
                    // abstract instance too (family 88 without `location`,
                    // `llc`'s order); an unnamed one's is refused as for a
                    // concrete local (§6.4).
                    let aligned = v.align >= 8;
                    if v.align != 0 && v.name.is_empty() {
                        return Err(unsupported(node, "debug:variable-alignment"));
                    }
                    let mask = b(decl.is_some()) << 1 | b(art) << 2;
                    // WP-087: an unnamed variable has no `DW_AT_name`.
                    let family = match (v.name.is_empty(), aligned) {
                        (true, _) => fam::VARIABLE_UNNAMED,
                        (false, true) => fam::VARIABLE_ALIGNED,
                        (false, false) => fam::VARIABLE,
                    };
                    self.abbrev(code(family, mask, false));
                    if !v.name.is_empty() {
                        self.strx(v.name)?;
                    }
                    if aligned {
                        write_uleb(&mut self.info, u64::from(v.align / 8));
                    }
                }
                self.decl(decl);
                self.ref_to(plan, node, v.ty.unwrap_or(DiId::NONE))?;
            }
            (Kind::AbstractLabel, DiNode::Label(n)) => {
                if !crate::label::is_plain(&n) {
                    return self.label_any(plan, node, &n, None);
                }
                let decl = Writer::decl_of(plan, node, n.file, n.line)?;
                if let Some(d) = decl
                    && n.column != 0
                {
                    self.abbrev(code(fam::LABEL_COLUMN, 0, false));
                    self.strx(n.name)?;
                    self.decl(Some(d));
                    write_uleb(&mut self.info, u64::from(n.column));
                } else {
                    self.abbrev(code(fam::LABEL, b(decl.is_some()), false));
                    self.strx(n.name)?;
                    self.decl(decl);
                }
            }
            (Kind::AbstractBlock, DiNode::LexicalBlock(_)) => {
                self.abbrev(code(fam::LEXICAL_BLOCK_ABSTRACT, 0, true));
            }
            (Kind::Inlined, DiNode::Subprogram(_)) => {
                let l = *plan.local_data(die).ok_or(bad(node, "subprogram"))?;
                let LocalWhat::Inlined { first, count, call } = l.what else {
                    return Err(bad(node, "subprogram"));
                };
                let origin = plan.inline.die(node).ok_or(bad(node, "subprogram"))?;
                let c = plan.debug.location(call).ok_or(bad(node, "inlinedAt"))?;
                let file = plan
                    .scope_file(c.scope)
                    .and_then(|f| plan.file_index(f))
                    .ok_or(bad(c.scope, "file"))?;
                let mask = b(count == 1) | b(count != 1) << 1 | b(c.column != 0) << 2;
                self.abbrev(code(fam::INLINED_SUBROUTINE, mask, children));
                self.ref4(origin);
                self.scope_ranges(plan, &l, first, count);
                write_uleb(&mut self.info, u64::from(file));
                write_uleb(&mut self.info, u64::from(c.line));
                if c.column != 0 {
                    write_uleb(&mut self.info, u64::from(c.column));
                }
            }
            _ => return Err(bad(node, "scope")),
        }
        Ok(())
    }

    /// An abstract subprogram: a definition's attributes without code,
    /// then `DW_AT_inline` (`applySubprogramAttributesToDefinition`).
    fn abstract_subprogram(
        &mut self,
        plan: &UnitPlan<'a>,
        die: u32,
        node: DiId,
        sp: &hod_ir::DiSubprogram<'a>,
        children: bool,
    ) -> Result<(), Error> {
        let b = u32::from;
        // A `LineTablesOnly` unit's: `name`, `DW_AT_inline` (WP-094,
        // `applySubprogramAttributes` with `SkipSPAttributes`).
        if plan.cu.emission_kind != hod_ir::EmissionKind::FullDebug {
            let name = !sp.name.is_empty();
            self.abbrev(code(fam::SUBPROGRAM_ABSTRACT, b(name) << 1, children));
            self.opt_name(sp.name)?;
            self.info.push(DW_INL_INLINED);
            return Ok(());
        }
        let objptr = plan.inline.object_pointer(die);
        if let Some(decl) = sp.declaration {
            let DiNode::Subprogram(d) = plan.debug.node(decl) else {
                return Err(bad(node, "declaration"));
            };
            let spec = plan.die_of(decl).ok_or(bad(node, "declaration"))?;
            let file = sp.file.and_then(|f| plan.file_index(f));
            let decl_file = d.file.and_then(|f| plan.file_index(f));
            let new_file = file != decl_file;
            let new_line = sp.line != d.line;
            let link = !sp.linkage_name.is_empty() && sp.linkage_name != d.linkage_name;
            let mask = b(new_file) | b(new_line) << 1 | b(link) << 2 | b(objptr.is_some()) << 3;
            self.abbrev(code(fam::SUBPROGRAM_ABSTRACT_MEMBER, mask, children));
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
            self.info.push(DW_INL_INLINED);
            if let Some(p) = objptr {
                self.ref4(p);
            }
            return Ok(());
        }
        let decl = Writer::decl_of(plan, node, sp.file, sp.line)?;
        let proto = has(sp.flags, DiFlags::PROTOTYPED)
            && matches!(plan.cu.language, 0x01 | 0x02 | 0x0c | 0x10 | 0x1d | 0x2c);
        let ty = plan.return_type(node, sp)?;
        let art = has(sp.flags, DiFlags::ARTIFICIAL);
        let ext = !sp.sp_flags.contains(SpFlags::LOCAL_TO_UNIT);
        let nr = has(sp.flags, DiFlags::NO_RETURN);
        let link = !sp.linkage_name.is_empty();
        let name = !sp.name.is_empty();
        if objptr.is_some() {
            return Err(unsupported(node, "debug:object-pointer"));
        }
        let mask = b(link)
            | b(name) << 1
            | b(decl.is_some()) << 2
            | b(proto) << 3
            | b(ty.is_some()) << 4
            | b(art) << 5
            | b(ext) << 6
            | b(nr) << 7;
        self.abbrev(code(fam::SUBPROGRAM_ABSTRACT, mask, children));
        if link {
            self.strx(sp.linkage_name)?;
        }
        if name {
            self.strx(sp.name)?;
        }
        self.decl(decl);
        if let Some(t) = ty {
            self.ref_to(plan, node, t)?;
        }
        self.info.push(DW_INL_INLINED);
        Ok(())
    }

    /// A scope's `low_pc` and `high_pc` (one range), or its range list:
    /// `DW_RLE_base_addressx` of the function, then one
    /// `DW_RLE_offset_pair` per range, as a block's (§6.4).
    fn scope_ranges(&mut self, plan: &UnitPlan<'a>, l: &LocalData, first: u32, count: u32) {
        let fd = plan.funcs[l.func as usize];
        if count == 1 {
            let (start, end) = plan.ranges[first as usize];
            let index = self.push_addr(fd.code.symbol, fd.code.addend + i64::from(start));
            self.addr_attrs(index, u64::from(end - start));
            return;
        }
        let base = self.fn_addr.get(l.func as usize).copied().unwrap_or(0);
        let at = self.block_lists.len();
        self.block_lists.push(DW_RLE_BASE_ADDRESSX);
        write_uleb(&mut self.block_lists, u64::from(base));
        for &(start, end) in &plan.ranges[first as usize..(first + count) as usize] {
            self.block_lists.push(DW_RLE_OFFSET_PAIR);
            write_uleb(&mut self.block_lists, u64::from(start));
            write_uleb(&mut self.block_lists, u64::from(end));
        }
        self.block_lists.push(0);
        let offset = self.block_lists_base + at;
        self.rnglists_offset(offset as i64);
    }

    /// A concrete subprogram with an abstract instance: its code, frame
    /// base and `this`, then `DW_AT_abstract_origin`. The frame base is
    /// in `self.expr`.
    pub(crate) fn subprogram_of_abstract(
        &mut self,
        index: u32,
        fd: &FnData,
        origin: u32,
        children: bool,
    ) {
        let objptr = fd.object_pointer != NONE;
        self.abbrev(code(fam::SUBPROGRAM_OF_ABSTRACT, u32::from(objptr), children));
        self.addr_attrs(index, fd.code.size);
        self.exprloc();
        if objptr {
            self.ref4(fd.object_pointer);
        }
        self.ref4(origin);
    }

    /// A concrete parameter or variable with an abstract entity: its
    /// location (in `self.expr`), then `DW_AT_abstract_origin`.
    pub(crate) fn var_of_abstract(&mut self, kind: Kind, origin: u32) {
        let family = if kind == Kind::LocalParam {
            fam::FORMAL_PARAMETER_OF_ABSTRACT
        } else {
            fam::VARIABLE_OF_ABSTRACT
        };
        self.abbrev(code(family, 0, false));
        self.exprloc();
        self.ref4(origin);
    }

    /// A concrete block with an abstract one: its ranges, then
    /// `DW_AT_abstract_origin` (`llc` 21 and later,
    /// `attachLexicalScopesAbstractOrigins`; §6.7).
    pub(crate) fn block_of_abstract(
        &mut self,
        plan: &UnitPlan<'a>,
        l: &LocalData,
        first: u32,
        count: u32,
        origin: u32,
    ) {
        let mask = if count == 1 { 1 } else { 2 };
        self.abbrev(code(fam::LEXICAL_BLOCK_OF_ABSTRACT, mask, true));
        self.scope_ranges(plan, l, first, count);
        self.ref4(origin);
    }

    /// A concrete label with an abstract one.
    pub(crate) fn label_of_abstract(&mut self, fd: &FnData, offset: u32, origin: u32) {
        self.abbrev(code(fam::LABEL_OF_ABSTRACT, 0, false));
        self.ref4(origin);
        let index = self.push_addr(fd.code.symbol, fd.code.addend + i64::from(offset));
        write_uleb(&mut self.info, u64::from(index));
    }
}
