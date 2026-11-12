//! The DIEs of functions (`spec/dwarf.md` §6.4): one subprogram per
//! placed function, its parameters, locals and labels, and its lexical
//! blocks, with the ranges `llc`'s `LexicalScopes` gives them.
//!
//! A plan made by [`UnitPlan::for_functions`] is open: the code generator
//! adds each placed function, in module order, with
//! [`UnitPlan::add_function`], then seals it ([`UnitPlan::seal`]). Sealing
//! makes the types the functions name in `llc`'s order (return types,
//! then variable types, as `finishSubprogramDefinitions` and
//! `finishEntityDefinitions` do) and inverts the tree.
//!
//! Nothing is allocated per DIE: a function's scopes, ranges and
//! entities live in scratch vectors reused across functions, and what
//! the writer needs is three side tables sorted by DIE.

use hod_ir::{
    DiFlags, DiId, DiKind, DiLocation, DiNode, DiOp, DiSubprogram, ExprId, LocId, SpFlags,
};

use crate::Error;
use crate::abbrev::{code, fam};
use crate::die::{CodeSpan, Kind, NONE, UnitPlan, Writer, bad, uleb_len, unsupported};
use crate::leb::{write_sleb, write_uleb};

/// `DW_OP_fbreg`.
const DW_OP_FBREG: u8 = 0x91;
/// `DW_OP_reg0`; `DW_OP_regx` beyond 31.
const DW_OP_REG0: u8 = 0x50;
const DW_OP_REGX: u8 = 0x90;
const DW_OP_DEREF: u8 = 0x06;

/// One line-table transition of a placed function, as the code generator
/// recorded it (`spec/codegen.md` §9.5): from `offset` on, the code has
/// location `loc` ([`LocId::NONE`] for none), up to the next row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScopeRow {
    /// The code offset, from the function's first byte.
    pub offset: u32,
    /// The location.
    pub loc: LocId,
}

/// Where a declared variable is, or what else a [`LocalRow`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalPlace {
    /// The variable is this many bytes from the frame base.
    Frame(i64),
    /// The variable's address is stored this many bytes from the frame
    /// base.
    FrameIndirect(i64),
    /// The row is a label at this code offset.
    Label(u32),
    /// The code generator could not describe the row: the function is
    /// refused under this stable name.
    Refused(&'static str),
}

/// One variable or label of a placed function.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalRow {
    /// The `DILocalVariable`, or the `DILabel`.
    pub node: DiId,
    /// The location of the declaration or the label: its scope says
    /// which block the DIE belongs to.
    pub loc: LocId,
    /// How the variable is found from its declared address.
    pub expr: ExprId,
    /// Where it is.
    pub place: LocalPlace,
}

/// One placed function (§6.4).
#[derive(Clone, Copy, Debug)]
pub struct FunctionInput<'p> {
    /// The function's `DISubprogram`.
    pub subprogram: DiId,
    /// Its code.
    pub code: CodeSpan,
    /// The DWARF register of its frame base.
    pub frame_base: u16,
    /// Its line-table transitions, in offset order.
    pub scopes: &'p [ScopeRow],
    /// Its variables and labels.
    pub vars: &'p [LocalRow],
}

/// A function's facts for the writer, by its subprogram DIE.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FnData {
    pub(crate) die: u32,
    pub(crate) code: CodeSpan,
    pub(crate) frame_base: u16,
    /// The `this` parameter's DIE, or `NONE`.
    pub(crate) object_pointer: u32,
}

/// What a local DIE is located by.
#[derive(Clone, Copy, Debug)]
pub(crate) enum LocalWhat {
    /// A variable: frame offset, indirect, expression.
    Var { offset: i64, indirect: bool, expr: ExprId },
    /// A block: its ranges in `UnitPlan::ranges`.
    Block { first: u32, count: u32 },
    /// A label at a code offset.
    Label { offset: u32 },
    /// An inlined subroutine (WP-085): its ranges, and the call's
    /// location (its `inlinedAt`).
    Inlined { first: u32, count: u32, call: LocId },
}

/// A local DIE's facts for the writer.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LocalData {
    pub(crate) die: u32,
    /// The index of its function in `UnitPlan::funcs`.
    pub(crate) func: u32,
    pub(crate) what: LocalWhat,
}

/// One scope of the function being added.
#[derive(Clone, Copy, Debug)]
struct Scope {
    node: DiId,
    /// The `inlinedAt` of the scope's key (`LocId::NONE` for the
    /// function's own scopes): `llc` keys a scope by both.
    ia: LocId,
    parent: u32,
    open: bool,
    first: u32,
    /// Whether the scope has variables (a block without is not written).
    vars: bool,
}

/// The scratch of [`UnitPlan::add_function`], reused across functions.
#[derive(Clone, Debug, Default)]
pub(crate) struct FnScratch {
    /// Per node: the index of its scope in `scopes`, or `NONE`.
    scope_of: Vec<u32>,
    scopes: Vec<Scope>,
    /// The open scopes, outermost first.
    stack: Vec<u32>,
    path: Vec<DiId>,
    /// `(scope, start, end)`, in the order they close.
    ranges: Vec<(u32, u32, u32)>,
    /// Per scope: its first range in `sorted`; one more than `scopes`.
    range_start: Vec<u32>,
    sorted: Vec<(u32, u32)>,
    /// Per scope: its first child in `children`.
    child_start: Vec<u32>,
    children: Vec<u32>,
    /// `(scope, class, key, row)`: class 0 a parameter (key its
    /// number), 1 a local, 2 a label (key the row).
    ents: Vec<(u32, u8, u32, u32)>,
    walk: Vec<(u32, u32, u32)>,
    fill: Vec<u32>,
    /// The inlined scopes' keys and the abstract scopes (WP-085).
    inl: crate::inline::FnInline,
}

/// The index of the scope keyed `(node, ia)`, or `NONE`.
fn find(s: &FnScratch, node: DiId, ia: LocId) -> u32 {
    if ia == LocId::NONE {
        s.scope_of.get(node.index()).copied().unwrap_or(NONE)
    } else {
        s.inl.scope(node, ia)
    }
}

impl FnScratch {
    /// The ranges grouped by scope, and the children of each scope: two
    /// counting sorts, both stable.
    fn group(&mut self) {
        let n = self.scopes.len();
        self.range_start.clear();
        self.range_start.resize(n + 1, 0);
        for &(sc, ..) in &self.ranges {
            self.range_start[sc as usize + 1] += 1;
        }
        for i in 1..=n {
            self.range_start[i] += self.range_start[i - 1];
        }
        self.sorted.clear();
        self.sorted.resize(self.ranges.len(), (0, 0));
        self.fill.clear();
        self.fill.extend_from_slice(&self.range_start[..n]);
        for &(sc, a, b) in &self.ranges {
            let at = &mut self.fill[sc as usize];
            self.sorted[*at as usize] = (a, b);
            *at += 1;
        }
        self.child_start.clear();
        self.child_start.resize(n + 1, 0);
        for sc in self.scopes.iter().skip(1) {
            self.child_start[sc.parent as usize + 1] += 1;
        }
        for i in 1..=n {
            self.child_start[i] += self.child_start[i - 1];
        }
        self.children.clear();
        self.children.resize(n.saturating_sub(1), 0);
        self.fill.clear();
        self.fill.extend_from_slice(&self.child_start[..n]);
        for (i, sc) in self.scopes.iter().enumerate().skip(1) {
            let at = &mut self.fill[sc.parent as usize];
            self.children[*at as usize] = i as u32;
            *at += 1;
        }
        self.walk.clear();
    }
}

const fn has(flags: DiFlags, f: DiFlags) -> bool {
    flags.0 & f.0 != 0
}

/// The flags a definition may have: anything else is written by `llc`
/// in a way hod does not yet write, and is refused.
/// `DIFlagThunk` (kiln's virtual thunks) changes nothing in `llc`'s DIE
/// (WP-087, checked with `llc` 19 and 22).
pub(crate) const DEF_FLAGS: u32 =
    DiFlags::PROTOTYPED.0 | DiFlags::ARTIFICIAL.0 | DiFlags::NO_RETURN.0 | DiFlags::THUNK.0;
pub(crate) const DEF_SP_FLAGS: u32 =
    SpFlags::DEFINITION.0 | SpFlags::LOCAL_TO_UNIT.0 | SpFlags::OPTIMIZED.0;

/// `dwarf::isC`, as the type graph has it.
const fn is_c(language: u16) -> bool {
    matches!(language, 0x01 | 0x02 | 0x0c | 0x10 | 0x1d | 0x2c)
}

/// The expression ops of a declared location after `DW_OP_fbreg`, or
/// the stable name of why there are none (§6.4). Each op is a standard
/// DWARF operation written as itself; LLVM's own operations are refused.
pub(crate) fn check_expr(debug: &hod_ir::DebugInfo, expr: ExprId) -> Result<(), &'static str> {
    for op in debug.ops(expr) {
        match op {
            DiOp::Fragment(..) => return Err("debug:fragment"),
            DiOp::StackValue
            | DiOp::Convert(..)
            | DiOp::TagOffset(_)
            | DiOp::EntryValue(_)
            | DiOp::ImplicitPointer
            | DiOp::Arg(_)
            | DiOp::ExtractBitsSext(..)
            | DiOp::ExtractBitsZext(..)
            | DiOp::PushObjectAddress
            | DiOp::Xderef
            | DiOp::Other(_) => return Err("debug:expression"),
            DiOp::DerefSize(n) if n > 0xff => return Err("debug:expression"),
            _ => {}
        }
    }
    Ok(())
}

/// Writes the ops of `expr` (already checked) to `out`.
fn write_ops(plan: &UnitPlan<'_>, expr: ExprId, out: &mut Vec<u8>) {
    for op in plan.debug.ops(expr) {
        out.push(op.code() as u8);
        match op {
            DiOp::Constu(v) | DiOp::PlusUconst(v) => write_uleb(out, v),
            DiOp::Consts(v) => write_sleb(out, v as i64),
            DiOp::DerefSize(n) => out.push(n as u8),
            _ => {}
        }
    }
}

impl<'a> UnitPlan<'a> {
    /// The scope a `DILexicalBlockFile` stands for: its nearest enclosing
    /// scope that is not one.
    pub(crate) fn non_file(&self, mut s: DiId) -> Result<DiId, Error> {
        let mut steps = 0;
        while let Some(DiKind::LexicalBlockFile) = self.debug.find_kind(s) {
            let DiNode::LexicalBlockFile(f) = self.debug.node(s) else { break };
            s = f.scope.ok_or(bad(s, "scope"))?;
            steps += 1;
            if steps > self.slot.len() {
                return Err(bad(s, "scope"));
            }
        }
        Ok(s)
    }

    /// The parent scope of a block, or `None` for a subprogram.
    pub(crate) fn parent_scope(&self, s: DiId) -> Result<Option<DiId>, Error> {
        match self.debug.find_kind(s) {
            Some(DiKind::Subprogram) => Ok(None),
            Some(DiKind::LexicalBlock) => {
                let DiNode::LexicalBlock(b) = self.debug.node(s) else {
                    return Err(bad(s, "scope"));
                };
                let p = b.scope.ok_or(bad(s, "scope"))?;
                self.non_file(p).map(Some)
            }
            _ => Err(bad(s, "scope")),
        }
    }

    /// Adds function `f` to an open plan (§6.4): its subprogram DIE, the
    /// lexical blocks its code is in, its variables and labels. Functions
    /// are added in module order.
    ///
    /// # Errors
    /// `Unsupported` with a stable `debug:*` name for what is not written
    /// exactly (`spec/dwarf.md` §6.4); `BadReference`; `Unsupported` if
    /// the plan is sealed.
    pub fn add_function(&mut self, f: &FunctionInput<'_>) -> Result<(), Error> {
        let sp_id = f.subprogram;
        if self.sealed {
            return Err(unsupported(sp_id, "a function added to a sealed plan"));
        }
        if self.debug.find_kind(sp_id) != Some(DiKind::Subprogram) {
            return Err(bad(sp_id, "subprogram"));
        }
        let DiNode::Subprogram(sp) = self.debug.node(sp_id) else {
            return Err(bad(sp_id, "subprogram"));
        };
        // WP-061 (§6.10); a `LineTablesOnly` function with inlined code
        // goes on in minimal form (WP-094, §6.13).
        if self.cu.emission_kind != hod_ir::EmissionKind::FullDebug
            && crate::reduced::add_function(self, f)?
        {
            return Ok(());
        }
        // A DIE made before its function, for its statics (WP-085), is
        // the function's.
        let early = self.inline.take_early(self.slot[sp_id.index()]);
        if self.slot[sp_id.index()] != NONE && !early {
            return Err(unsupported(sp_id, "debug:subprogram-twice"));
        }
        if u32::try_from(f.code.size).is_err() {
            return Err(Error::TooLarge);
        }
        let variadic = self.variadic(sp_id, &sp)?;

        let mut s = std::mem::take(&mut self.fn_scratch);
        let result = self.function_body(f, &sp, &mut s);
        // A variadic function's last child (`llc`'s
        // `constructSubprogramScopeDIE`).
        if let Ok(die) = result
            && variadic
        {
            self.push_anon(sp_id, die, Kind::Unspecified);
        }
        for sc in &s.scopes {
            s.scope_of[sc.node.index()] = NONE;
        }
        self.fn_scratch = s;
        self.record_entities(f, result.is_ok()); // WP-094: §6.13
        result.map(|_| ())
    }

    /// The parent DIE of a definition (§6.4): the unit for a member
    /// function's (after its class, as `llc` builds the declaration
    /// first), else its namespace or the unit.
    pub(crate) fn subprogram_parent(
        &mut self,
        sp_id: DiId,
        sp: &DiSubprogram<'a>,
    ) -> Result<u32, Error> {
        if self.cu.emission_kind != hod_ir::EmissionKind::FullDebug {
            return crate::reduced::minimal_parent(self, sp_id, sp); // WP-094: §6.13
        }
        if !sp.sp_flags.contains(SpFlags::DEFINITION) || sp.unit != Some(self.unit) {
            return Err(unsupported(sp_id, "debug:subprogram-not-definition"));
        }
        if !sp.thrown_types.is_empty() || sp.sp_flags.0 & !DEF_SP_FLAGS != 0 {
            return Err(unsupported(sp_id, "debug:subprogram-attribute"));
        }
        let Some(decl) = sp.declaration else {
            if sp.flags.0 & !DEF_FLAGS != 0 {
                return Err(unsupported(sp_id, "debug:subprogram-attribute"));
            }
            let mut chain = std::mem::take(&mut self.fn_scratch.path);
            let parent = self.scope_parent(sp_id, sp.scope, &mut chain);
            self.fn_scratch.path = chain;
            return parent.map_err(|e| match e {
                Error::Unsupported { node, .. } => {
                    Error::Unsupported { node, what: "debug:subprogram-scope" }
                }
                e => e,
            });
        };
        // A member function's definition: the class is made (with the
        // declaration among its members), then the definition goes to
        // the unit.
        // Made on demand, in its class or its scope (§6.11).
        let spec = self.declaration_die(sp_id, decl)?;
        if spec == NONE || self.dies[spec as usize].kind != Kind::Method {
            return Err(unsupported(sp_id, "debug:specification"));
        }
        Ok(0)
    }

    /// Whether a subprogram's type ends in a null parameter type.
    fn variadic(&self, of: DiId, sp: &DiSubprogram<'_>) -> Result<bool, Error> {
        let Some(ty) = sp.ty else { return Ok(false) };
        let DiNode::SubroutineType(t) = self.node_of(of, "type", ty)? else {
            return Err(bad(of, "type"));
        };
        Ok(t.types.len() > 1 && t.types.last() == Some(&DiId::NONE))
    }

    /// The return type of a subprogram, `None` for `void` or no type.
    pub(crate) fn return_type(
        &self,
        of: DiId,
        sp: &DiSubprogram<'_>,
    ) -> Result<Option<DiId>, Error> {
        let Some(ty) = sp.ty else { return Ok(None) };
        let DiNode::SubroutineType(t) = self.node_of(of, "type", ty)? else {
            return Err(bad(of, "type"));
        };
        Ok(t.types.first().copied().filter(|&t| t != DiId::NONE))
    }

    /// `llc`'s `endFunctionImpl`: the scopes and entities, the abstract
    /// instances of what the function inlines (WP-085), then the
    /// function's DIE and its scopes'. The subprogram DIE.
    fn function_body(
        &mut self,
        f: &FunctionInput<'_>,
        sp: &DiSubprogram<'a>,
        s: &mut FnScratch,
    ) -> Result<u32, Error> {
        let sp_id = f.subprogram;
        if s.scope_of.len() < self.slot.len() {
            s.scope_of.resize(self.slot.len(), NONE);
        }
        s.scopes.clear();
        s.stack.clear();
        s.ranges.clear();
        s.ents.clear();
        s.inl.clear();
        self.scope_ranges(f, s)?;

        self.collect_entities(f, s)?;
        s.group();
        self.abstract_instances(&mut s.inl)?;

        let die = match self.slot[sp_id.index()] {
            NONE => {
                let parent = self.subprogram_parent(sp_id, sp)?;
                self.push(sp_id, parent, Kind::Subprogram)
            }
            // Made for a local type its own inlined code reached
            // (WP-094, §6.13): the function's from now on.
            early => {
                self.inline.take_early(early);
                early
            }
        };
        let func = self.funcs.len() as u32;
        self.funcs.push(FnData {
            die,
            code: f.code,
            frame_base: f.frame_base,
            object_pointer: NONE,
        });
        self.code_addrs += 1;
        self.scope_dies(f, func, die, s)?;
        Ok(die)
    }

    /// The function's variables and labels, by scope (§6.4).
    fn collect_entities(&mut self, f: &FunctionInput<'_>, s: &mut FnScratch) -> Result<(), Error> {
        let sp_id = f.subprogram;
        // A variable or label whose scope has no code is dropped, as
        // `llc` drops it.
        for (row, v) in f.vars.iter().enumerate() {
            let label = matches!(v.place, LocalPlace::Label(_));
            if let LocalPlace::Refused(name) = v.place {
                return Err(unsupported(v.node, name));
            }
            let want = if label { DiKind::Label } else { DiKind::LocalVariable };
            if self.debug.find_kind(v.node) != Some(want) {
                return Err(bad(v.node, "variable"));
            }
            let (scope, class, key) = match self.debug.node(v.node) {
                DiNode::LocalVariable(n) => {
                    if n.ty.is_none() {
                        return Err(unsupported(v.node, "debug:variable-without-type"));
                    }
                    // WP-087: a named local's alignment is written; a
                    // parameter's, or an unnamed one's, is not yet.
                    if n.align != 0 && (n.arg != 0 || n.name.is_empty()) {
                        return Err(unsupported(v.node, "debug:variable-alignment"));
                    }
                    if let LocalPlace::Frame(_) | LocalPlace::FrameIndirect(_) = v.place
                        && let Err(name) = check_expr(self.debug, v.expr)
                    {
                        return Err(unsupported(v.node, name));
                    }
                    let (class, key) =
                        if n.arg != 0 { (0, u32::from(n.arg)) } else { (1, row as u32) };
                    (n.scope, class, key)
                }
                DiNode::Label(n) => {
                    if n.name.is_empty() || n.is_artificial || n.coro_suspend_idx.is_some() {
                        return Err(unsupported(v.node, "debug:label-attribute"));
                    }
                    (n.scope, 2, row as u32)
                }
                _ => return Err(bad(v.node, "variable")),
            };
            let (scope, ia) = match self.debug.location(v.loc) {
                Some(DiLocation { scope, inlined_at, .. }) => (scope, inlined_at),
                None => (scope.ok_or(bad(v.node, "scope"))?, LocId::NONE),
            };
            let scope = self.non_file(scope)?;
            let index = find(s, scope, ia);
            if index == NONE {
                let owner =
                    if ia == LocId::NONE { self.owner_of(scope)? } else { self.call_root(ia)? };
                if owner != sp_id {
                    return Err(unsupported(v.node, "debug:foreign-scope"));
                }
                continue;
            }
            if ia != LocId::NONE && !s.inl.first_of(v.node, ia) {
                return Err(unsupported(v.node, "debug:variable-twice"));
            }
            s.ents.push((index, class, key, row as u32));
            self.abstract_entity(&mut s.inl, v.node, scope, class, key);
        }
        s.ents.sort_unstable();
        // `llc`'s `needToEmitLexicalScope`: a block is written when it
        // has variables; its labels alone do not make it.
        for &(scope, class, ..) in &s.ents {
            if class != 2 {
                s.scopes[scope as usize].vars = true;
            }
        }
        // A block with an imported entity is written too (WP-094, §6.13).
        if let DiNode::Subprogram(sp) = self.debug.node(sp_id) {
            self.import_blocks(&sp, |b| match find(s, b, LocId::NONE) {
                NONE => {}
                i => s.scopes[i as usize].vars = true,
            });
        }

        Ok(())
    }

    /// The DIEs of the function's scopes, depth first (§6.4).
    fn scope_dies(
        &mut self,
        f: &FunctionInput<'_>,
        func: u32,
        sp_die: u32,
        s: &mut FnScratch,
    ) -> Result<(), Error> {
        let n = s.scopes.len();
        // The DIEs, depth first: a scope's variables and labels, then its
        // blocks; a block without variables is not written, and its
        // labels and blocks go to its parent (§6.4).
        if n == 0 {
            return Ok(());
        }
        self.entities(f, func, 0, sp_die, s)?;
        s.walk.push((0, sp_die, s.child_start[0]));
        while let Some(&(sc, die, cursor)) = s.walk.last() {
            if cursor == s.child_start[sc as usize + 1] {
                s.walk.pop();
                continue;
            }
            if let Some(top) = s.walk.last_mut() {
                top.2 += 1;
            }
            let child = s.children[cursor as usize];
            let c = s.scopes[child as usize];
            // An inlined subroutine is always written (WP-085).
            let inlined = c.ia != LocId::NONE && self.parent_scope(c.node)?.is_none();
            let parent = if c.vars || inlined {
                let kind = if inlined { Kind::Inlined } else { Kind::Block };
                let block = if c.ia == LocId::NONE {
                    self.push(c.node, die, kind)
                } else {
                    self.push_anon(c.node, die, kind)
                };
                let (first, end) =
                    (s.range_start[child as usize], s.range_start[child as usize + 1]);
                let first_range = self.ranges.len() as u32;
                self.ranges.extend_from_slice(&s.sorted[first as usize..end as usize]);
                let count = end - first;
                if count == 1 {
                    self.code_addrs += 1;
                }
                let what = if inlined {
                    LocalWhat::Inlined { first: first_range, count, call: c.ia }
                } else {
                    LocalWhat::Block { first: first_range, count }
                };
                self.locals.push(LocalData { die: block, func, what });
                self.entities(f, func, child, block, s)?;
                block
            } else {
                // Its labels go to the parent, where `llc` puts them.
                self.entities(f, func, child, die, s)?;
                die
            };
            s.walk.push((child, parent, s.child_start[child as usize]));
        }
        Ok(())
    }

    /// The subprogram a local scope belongs to.
    pub(crate) fn owner_of(&self, mut scope: DiId) -> Result<DiId, Error> {
        let mut steps = 0;
        while let Some(p) = self.parent_scope(scope)? {
            scope = p;
            steps += 1;
            if steps > self.slot.len() {
                return Err(bad(scope, "scope"));
            }
        }
        Ok(scope)
    }

    /// The DIEs of scope `sc`'s variables and labels, under `die`.
    #[allow(clippy::many_single_char_names, reason = "the walk's state")]
    fn entities(
        &mut self,
        f: &FunctionInput<'_>,
        func: u32,
        sc: u32,
        die: u32,
        s: &FnScratch,
    ) -> Result<(), Error> {
        // The entities are sorted by scope, and scopes are visited in
        // an order that is not their index order: find the run.
        let mut e = s.ents.partition_point(|x| x.0 < sc);
        while let Some(&(scope, class, _, row)) = s.ents.get(e) {
            if scope != sc {
                break;
            }
            e += 1;
            let v = f.vars[row as usize];
            // An inlined instance's entities are not keyed (WP-085).
            let inlined = s.scopes[sc as usize].ia != LocId::NONE;
            if !inlined && self.slot[v.node.index()] != NONE {
                return Err(unsupported(v.node, "debug:variable-twice"));
            }
            let kind = match class {
                0 => Kind::LocalParam,
                1 => Kind::LocalVar,
                _ => Kind::Label,
            };
            let d = if inlined {
                self.push_anon(v.node, die, kind)
            } else {
                self.push(v.node, die, kind)
            };
            let what = match v.place {
                LocalPlace::Frame(offset) => {
                    LocalWhat::Var { offset, indirect: false, expr: v.expr }
                }
                LocalPlace::FrameIndirect(offset) => {
                    LocalWhat::Var { offset, indirect: true, expr: v.expr }
                }
                LocalPlace::Label(offset) => {
                    self.code_addrs += 1;
                    LocalWhat::Label { offset }
                }
                LocalPlace::Refused(name) => return Err(unsupported(v.node, name)),
            };
            if class == 0
                && !inlined
                && let DiNode::LocalVariable(n) = self.debug.node(v.node)
                && has(n.flags, DiFlags::OBJECT_POINTER)
            {
                let fd = &mut self.funcs[func as usize];
                if fd.object_pointer != NONE {
                    return Err(unsupported(v.node, "debug:object-pointer"));
                }
                fd.object_pointer = d;
            }
            self.locals.push(LocalData { die: d, func, what });
        }
        Ok(())
    }

    /// `llc`'s `LexicalScopes` over the function's transitions (§6.4):
    /// a scope's range stays open while the code's scope is inside it,
    /// and closes at the first code outside it. Code without a location
    /// belongs to whatever range is open around it. Linear in the rows
    /// plus the scopes opened.
    fn scope_ranges(&self, f: &FunctionInput<'_>, s: &mut FnScratch) -> Result<(), Error> {
        let size = f.code.size as u32;
        let mut last_end = 0;
        for (i, row) in f.scopes.iter().enumerate() {
            let Some(loc) = self.debug.location(row.loc) else { continue };
            let end = f.scopes.get(i + 1).map_or(size, |r| r.offset).min(size);
            if end <= row.offset {
                continue;
            }
            // An inlined location: `llc` makes the abstract scope of
            // its scope first (WP-085).
            if loc.inlined_at != LocId::NONE {
                self.abstract_scope(&mut s.inl, loc.scope)?;
            }
            // The scopes from the row's out to the first one open, keyed
            // `(scope, inlinedAt)`; an inlined subprogram's parent is
            // its call's scope.
            s.inl.kpath.clear();
            let mut at = (self.non_file(loc.scope)?, loc.inlined_at);
            let bound = self.slot.len() + self.debug.loc_count();
            let ancestor = loop {
                let index = find(s, at.0, at.1);
                if index != NONE && s.scopes[index as usize].open {
                    break index;
                }
                s.inl.kpath.push(at);
                if s.inl.kpath.len() > bound {
                    return Err(bad(at.0, "scope"));
                }
                match self.parent_scope(at.0)? {
                    Some(p) => at.0 = p,
                    None if at.1 != LocId::NONE => {
                        at = self.call_parent(&mut s.inl, at.0, at.1, index == NONE)?;
                    }
                    None if at.0 == f.subprogram => break NONE,
                    None => return Err(unsupported(at.0, "debug:foreign-scope")),
                }
            };
            // Close what does not contain the row's scope.
            while let Some(&top) = s.stack.last() {
                if top == ancestor {
                    break;
                }
                s.stack.pop();
                let sc = &mut s.scopes[top as usize];
                sc.open = false;
                s.ranges.push((top, sc.first, last_end));
            }
            // Open the new ones, outermost first.
            for k in (0..s.inl.kpath.len()).rev() {
                let (node, ia) = s.inl.kpath[k];
                let parent = s.stack.last().copied().unwrap_or(NONE);
                let mut index = find(s, node, ia);
                if index == NONE {
                    index = s.scopes.len() as u32;
                    if ia == LocId::NONE {
                        s.scope_of[node.index()] = index;
                    } else {
                        s.inl.set_scope(node, ia, index);
                    }
                    s.scopes.push(Scope { node, ia, parent, open: false, first: 0, vars: false });
                }
                let sc = &mut s.scopes[index as usize];
                sc.open = true;
                sc.first = row.offset;
                s.stack.push(index);
            }
            last_end = end;
        }
        while let Some(top) = s.stack.pop() {
            let sc = &mut s.scopes[top as usize];
            sc.open = false;
            s.ranges.push((top, sc.first, last_end));
        }
        Ok(())
    }

    /// Seals an open plan: the types its functions name, in `llc`'s
    /// order, the side tables sorted, the tree inverted. A sealed plan
    /// takes no more functions; sealing twice does nothing.
    ///
    /// # Errors
    /// What the type graph refuses (§6.8).
    pub fn seal(&mut self) -> Result<(), Error> {
        if self.sealed {
            return Ok(());
        }
        self.seal_imports()?;
        // `finishSubprogramDefinitions`: the return types, by function.
        for i in 0..self.funcs.len() {
            let die = self.funcs[i].die;
            let sp_id = self.dies[die as usize].node;
            let DiNode::Subprogram(sp) = self.debug.node(sp_id) else { continue };
            self.seal_definition(i)?; // before the return type (WP-094: §6.13)
            // One with an abstract instance names it instead (WP-085).
            if sp.declaration.is_none()
                && self.inline.die(sp_id).is_none()
                && let Some(t) = self.return_type(sp_id, &sp)?
            {
                self.reference_type(sp_id, "type", t)?;
            }
        }
        self.finish_entities()?; // `finishEntityDefinitions` (WP-094: §6.13)
        self.finish_types()?;
        self.seal_inline()?;
        self.seal_tables();
        Ok(())
    }

    /// The index and facts of the function whose subprogram DIE is
    /// `die`.
    pub(crate) fn fn_data(&self, die: u32) -> Option<(usize, FnData)> {
        let i = self.inline.fn_index(die)?;
        Some((i, self.funcs[i]))
    }

    pub(crate) fn local_data(&self, die: u32) -> Option<&LocalData> {
        let i = self.locals.binary_search_by_key(&die, |l| l.die).ok()?;
        self.locals.get(i)
    }

    /// The number of `.debug_addr` entries the functions' DIEs take.
    pub(crate) fn code_addrs(&self) -> u32 {
        self.code_addrs
    }
}

impl<'a> Writer<'a> {
    pub(crate) fn exprloc(&mut self) {
        write_uleb(&mut self.info, self.expr.len() as u64);
        self.info.extend_from_slice(&self.expr);
    }

    /// Writes a subprogram definition (§6.4).
    pub(crate) fn subprogram_die(
        &mut self,
        plan: &UnitPlan<'a>,
        die: u32,
        sp_id: DiId,
        sp: &DiSubprogram<'a>,
        children: bool,
    ) -> Result<(), Error> {
        let b = u32::from;
        let (func, fd) = plan.fn_data(die).ok_or(bad(sp_id, "subprogram"))?;
        let index = self.push_addr(fd.code.symbol, fd.code.addend);
        if let Some(a) = self.fn_addr.get_mut(func) {
            *a = index;
        }
        if plan.cu.emission_kind != hod_ir::EmissionKind::FullDebug {
            return self.minimal_subprogram(plan, sp_id, sp, index, fd.code.size, children); // WP-094
        }
        // The frame base: `DW_OP_reg<n>`.
        self.expr.clear();
        if fd.frame_base < 32 {
            self.expr.push(DW_OP_REG0 + fd.frame_base as u8);
        } else {
            self.expr.push(DW_OP_REGX);
            write_uleb(&mut self.expr, u64::from(fd.frame_base));
        }
        if let Some(origin) = plan.inline.die(sp_id) {
            self.subprogram_of_abstract(index, &fd, origin, children);
            return Ok(());
        }
        let decl_of = |file: Option<DiId>, line: u32| Writer::decl_of(plan, sp_id, file, line);
        if let Some(decl) = sp.declaration {
            let DiNode::Subprogram(d) = plan.debug.node(decl) else {
                return Err(bad(sp_id, "declaration"));
            };
            if let Some(t) = plan.definition_type(sp_id, sp)? {
                let (size, op) = (fd.code.size, fd.object_pointer);
                return self.typed_definition_die(plan, sp_id, sp, t, index, size, op, children);
            }
            let spec = plan.die_of(decl).ok_or(bad(sp_id, "declaration"))?;
            let file = sp.file.and_then(|f| plan.file_index(f));
            let decl_file = d.file.and_then(|f| plan.file_index(f));
            let new_file = file != decl_file;
            let new_line = sp.line != d.line;
            // `llc`: the definition's linkage name unless it is the
            // declaration's.
            let link = !sp.linkage_name.is_empty() && sp.linkage_name != d.linkage_name;
            let mask =
                b(fd.object_pointer != NONE) | b(new_file) << 1 | b(new_line) << 2 | b(link) << 3;
            self.abbrev(code(fam::SUBPROGRAM_MEMBER, mask, children));
            self.addr_attrs(index, fd.code.size);
            self.exprloc();
            if fd.object_pointer != NONE {
                self.ref4(fd.object_pointer);
            }
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
            return Ok(());
        }
        let decl = decl_of(sp.file, sp.line)?;
        let proto = has(sp.flags, DiFlags::PROTOTYPED) && is_c(plan.cu.language);
        let ty = plan.return_type(sp_id, sp)?;
        let art = has(sp.flags, DiFlags::ARTIFICIAL);
        let ext = !sp.sp_flags.contains(SpFlags::LOCAL_TO_UNIT);
        let nr = has(sp.flags, DiFlags::NO_RETURN);
        let link = !sp.linkage_name.is_empty();
        let name = !sp.name.is_empty();
        if !proto && !art && !nr && name {
            let mask = b(link) | b(decl.is_some()) << 1 | b(ty.is_some()) << 2 | b(ext) << 3;
            self.abbrev(code(fam::SUBPROGRAM, mask, children));
        } else {
            let mask = b(link)
                | b(name) << 1
                | b(decl.is_some()) << 2
                | b(proto) << 3
                | b(ty.is_some()) << 4
                | b(art) << 5
                | b(ext) << 6
                | b(nr) << 7;
            self.abbrev(code(fam::SUBPROGRAM_ANY, mask, children));
        }
        self.addr_attrs(index, fd.code.size);
        self.exprloc();
        if link {
            self.strx(sp.linkage_name)?;
        }
        if name {
            self.strx(sp.name)?;
        }
        self.decl(decl);
        if let Some(t) = ty {
            self.ref_to(plan, sp_id, t)?;
        }
        Ok(())
    }

    /// `DW_AT_low_pc` (addrx `index`) and `DW_AT_high_pc` (`size`).
    pub(crate) fn addr_attrs(&mut self, index: u32, size: u64) {
        write_uleb(&mut self.info, u64::from(index));
        self.info.extend_from_slice(&(size as u32).to_le_bytes());
    }

    /// Writes a parameter, a local variable, a block or a label (§6.4).
    pub(crate) fn local_die(
        &mut self,
        plan: &UnitPlan<'a>,
        die: u32,
        kind: Kind,
        node: DiId,
    ) -> Result<(), Error> {
        let b = u32::from;
        let l = *plan.local_data(die).ok_or(bad(node, "variable"))?;
        let fd = plan.funcs[l.func as usize];
        match (l.what, plan.debug.node(node)) {
            (LocalWhat::Var { offset, indirect, expr }, DiNode::LocalVariable(v)) => {
                self.expr.clear();
                self.expr.push(DW_OP_FBREG);
                write_sleb(&mut self.expr, offset);
                if indirect {
                    self.expr.push(DW_OP_DEREF);
                }
                let mut ops = std::mem::take(&mut self.expr);
                write_ops(plan, expr, &mut ops);
                self.expr = ops;
                if let Some(origin) = plan.inline.die(node) {
                    self.var_of_abstract(kind, origin);
                    return Ok(());
                }
                let decl = Writer::decl_of(plan, node, v.file, v.line)?;
                let art = has(v.flags, DiFlags::ARTIFICIAL);
                if kind == Kind::LocalParam {
                    let mask =
                        1 | b(!v.name.is_empty()) << 1 | b(decl.is_some()) << 2 | b(art) << 3;
                    self.abbrev(code(fam::FORMAL_PARAMETER, mask, false));
                    self.exprloc();
                    self.opt_name(v.name)?;
                } else {
                    let mask = 1 | b(decl.is_some()) << 1 | b(art) << 2;
                    // WP-087: an unnamed variable has no `DW_AT_name`; an
                    // aligned one has `DW_AT_alignment` after it.
                    let family = match (v.name.is_empty(), v.align != 0) {
                        (true, _) => fam::VARIABLE_UNNAMED,
                        (false, true) => fam::VARIABLE_ALIGNED,
                        (false, false) => fam::VARIABLE,
                    };
                    self.abbrev(code(family, mask, false));
                    self.exprloc();
                    if !v.name.is_empty() {
                        self.strx(v.name)?;
                    }
                    if v.align != 0 {
                        write_uleb(&mut self.info, u64::from(v.align / 8));
                    }
                }
                self.decl(decl);
                self.ref_to(plan, node, v.ty.unwrap_or(DiId::NONE))?;
            }
            (LocalWhat::Block { first, count }, DiNode::LexicalBlock(_)) => {
                if let Some(origin) = plan.inline.die(node) {
                    self.block_of_abstract(plan, &l, first, count, origin);
                } else if count == 1 {
                    let (start, end) = plan.ranges[first as usize];
                    self.abbrev(code(fam::LEXICAL_BLOCK, 1, true));
                    let index = self.push_addr(fd.code.symbol, fd.code.addend + i64::from(start));
                    self.addr_attrs(index, u64::from(end - start));
                } else {
                    self.abbrev(code(fam::LEXICAL_BLOCK, 2, true));
                    // `DW_RLE_base_addressx` of the function, then one
                    // `DW_RLE_offset_pair` per range.
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
            }
            (LocalWhat::Label { offset }, DiNode::Label(n)) => {
                if let Some(origin) = plan.inline.die(node) {
                    self.label_of_abstract(&fd, offset, origin);
                    return Ok(());
                }
                let decl = Writer::decl_of(plan, node, n.file, n.line)?;
                if let Some(d) = decl
                    && n.column != 0
                {
                    // `llc` 21 and later write the column of a label.
                    self.abbrev(code(fam::LABEL_COLUMN, 1, false));
                    self.strx(n.name)?;
                    self.decl(Some(d));
                    write_uleb(&mut self.info, u64::from(n.column));
                } else {
                    self.abbrev(code(fam::LABEL, b(decl.is_some()) | 2, false));
                    self.strx(n.name)?;
                    self.decl(decl);
                }
                let index = self.push_addr(fd.code.symbol, fd.code.addend + i64::from(offset));
                write_uleb(&mut self.info, u64::from(index));
            }
            _ => return Err(bad(node, "variable")),
        }
        Ok(())
    }
}

const DW_RLE_BASE_ADDRESSX: u8 = 0x01;
const DW_RLE_OFFSET_PAIR: u8 = 0x04;

/// The bytes of the CU's own range list for `spans` entries starting at
/// address index `base`: what precedes the blocks' lists.
pub(crate) fn cu_list_len(base: u32, sizes: impl Iterator<Item = u64>) -> usize {
    let mut n = 1;
    for (k, size) in sizes.enumerate() {
        n += 1 + uleb_len(u64::from(base) + k as u64) + uleb_len(size);
    }
    n
}
