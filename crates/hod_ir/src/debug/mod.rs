//! Debug information (`spec/ir.md` §6): the module's tables of debug
//! nodes, expressions and source locations.
//!
//! [`DebugInfo`] is the field `debug` of a [`crate::Module`]. A module
//! without debug information has an empty one, which has allocated
//! nothing. The tables follow the crate's rules: `u32` ids into arenas,
//! strings and expressions interned, no allocation per node.
//!
//! | module | contents |
//! |---|---|
//! | `node` | the kinds and their typed records, how a record is stored, the by-name setter |
//! | `expr` | expressions: `ExprId`, `DiOp` |
//! | `names` | DWARF and LLVM vocabulary: tags, encodings, flags |
//! | `map` | the field table: what becomes of every LLVM kind and field |

pub(crate) mod expr;
pub(crate) mod map;
pub(crate) mod names;
pub(crate) mod node;

use crate::error::{IrErrorKind, IrResult, err};
use crate::ids::{DiId, LocId};
use crate::intern::{Hasher64, InternTable, next_id, pool_start};
use crate::strings::StrPool;

use expr::{DiOp, DiOps, ExprId, ExprTable, order_error};
use node::{Dec, DiKind, DiNode, Enc, class};

/// A node's fixed part. Eight bytes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Head {
    /// Where the node's cells start: the mask, then the stored fields.
    at: u32,
    /// `DiKind as u8`; 0 while the node is only reserved.
    kind: u8,
    flags: u8,
    tag: u16,
}

const DISTINCT: u8 = 1;

/// A source location (§6.5): LLVM's `DILocation`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct DiLocation {
    /// The line; 0 for "no line", which compiler-generated code has.
    pub line: u32,
    /// The column; 0 for none. LLVM keeps 16 bits and stores a larger
    /// column as 0, and so must a producer.
    pub column: u16,
    /// The innermost scope: a subprogram or a block of one.
    pub scope: DiId,
    /// For an instruction inlined from another function: the location
    /// of the call it was inlined at. [`LocId::NONE`] otherwise.
    pub inlined_at: LocId,
}

/// A stored location. Sixteen bytes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct LocRec {
    line: u32,
    scope: DiId,
    inlined_at: LocId,
    column: u16,
    state: u16,
}

/// Reserved and not defined.
const LOC_RESERVED: u16 = 0;
/// Defined and distinct: never the answer to a lookup.
const LOC_DISTINCT: u16 = 1;
/// Defined and interned.
const LOC_INTERNED: u16 = 2;
/// Defined after a reservation, not interned and not distinct.
const LOC_UNIQUED: u16 = 3;

/// What the debug tables hold, for the size reports of §6.10.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct DebugStats {
    /// Nodes, reserved ones included.
    pub nodes: usize,
    /// Bytes of node storage: eight per node and four per cell.
    pub node_bytes: usize,
    /// Bytes of interned strings, with their spans.
    pub string_bytes: usize,
    /// Expressions, the empty one included.
    pub exprs: usize,
    /// Bytes of expression storage.
    pub expr_bytes: usize,
    /// Locations.
    pub locs: usize,
    /// Bytes of location storage: sixteen per location.
    pub loc_bytes: usize,
}

/// The debug tables of a module (§6): nodes behind [`DiId`], expressions
/// behind [`ExprId`], source locations behind [`LocId`], and the list of
/// compile units.
///
/// Append-only like every table of the module, with two exceptions that
/// both happen before anything reads the node: a reserved node is
/// defined once, and a forward-declared composite type is completed
/// once.
#[derive(Clone, Debug, Default)]
pub struct DebugInfo {
    heads: Vec<Head>,
    /// Masks, numbers, string ids and node ids, as the nodes' fields
    /// need them. A node list is stored in place, so that it is read as
    /// a slice.
    cells: Vec<DiId>,
    /// The number of strings in `strings` and the bytes they take, for
    /// the size report.
    string_count: u32,
    string_bytes: usize,
    strings: Option<StrPool>,
    /// The uniqued nodes.
    interner: Option<InternTable>,
    /// Nodes reserved and not defined.
    undefined: u32,
    /// String ids of the kind names of `DiOther` nodes, each once.
    other_kinds: Vec<u32>,
    exprs: ExprTable,
    locs: Vec<LocRec>,
    loc_interner: Option<InternTable>,
    undefined_locs: u32,
    units: Vec<DiId>,
}

#[cold]
fn invalid<T>(message: String) -> IrResult<T> {
    err(IrErrorKind::InvalidDebugInfo, message)
}

impl DebugInfo {
    /// Whether the module has no debug information at all: no node, no
    /// location, no expression but the empty one. Such a table has
    /// allocated nothing.
    pub fn is_empty(&self) -> bool {
        self.heads.is_empty() && self.locs.is_empty() && self.exprs.len() == 1
    }

    // ---- nodes: building ----

    /// A new id whose node is defined later, by [`DebugInfo::define`] or
    /// [`DebugInfo::define_distinct`]. This is how cycles are built (a
    /// class names its members and each member names the class) and how
    /// a producer refers to a node before it has read it.
    ///
    /// # Errors
    /// `TableFull`.
    pub fn reserve(&mut self) -> IrResult<DiId> {
        let id = next_id(self.heads.len(), "debug node")?;
        self.heads.push(Head { at: 0, kind: 0, flags: 0, tag: 0 });
        self.undefined += 1;
        Ok(DiId::new(id))
    }

    /// Defines the reserved node `id` as `node`, a node LLVM would
    /// unique. Its id was handed out before its content was known, so it
    /// is never the answer to a later [`DebugInfo::add`].
    ///
    /// # Errors
    /// `UndefinedDebugNode` if `id` was never reserved, or `node` names
    /// an id that was never reserved or added; `DebugRedefined` if `id`
    /// is defined already; `InvalidDebugInfo` if `node` is
    /// [`DiNode::Undefined`] or must be distinct; `TableFull`.
    pub fn define(&mut self, id: DiId, node: &DiNode<'_>) -> IrResult<()> {
        self.define_as(id, node, 0)
    }

    /// Defines the reserved node `id` as the distinct node `node`.
    ///
    /// # Errors
    /// As [`DebugInfo::define`], without the distinctness rule.
    pub fn define_distinct(&mut self, id: DiId, node: &DiNode<'_>) -> IrResult<()> {
        self.define_as(id, node, DISTINCT)
    }

    fn define_as(&mut self, id: DiId, node: &DiNode<'_>, flags: u8) -> IrResult<()> {
        match self.heads.get(id.index()) {
            None => {
                return err(IrErrorKind::UndefinedDebugNode, format!("{id:?} was never reserved"));
            }
            Some(head) if head.kind != 0 => {
                let kind = DiKind::from_code(head.kind).llvm_name();
                return err(
                    IrErrorKind::DebugRedefined,
                    format!("{id:?} is defined already, as a {kind}"),
                );
            }
            Some(_) => {}
        }
        let head = self.store(node, flags)?;
        self.heads[id.index()] = head;
        self.undefined -= 1;
        Ok(())
    }

    /// The id of `node`, a node LLVM would unique: an equal node that
    /// was added before is returned again. Equal means the same kind
    /// and the same fields; distinct nodes and nodes defined through
    /// [`DebugInfo::reserve`] are never returned.
    ///
    /// # Errors
    /// `UndefinedDebugNode` if `node` names an id that was never
    /// reserved or added; `InvalidDebugInfo` if `node` is
    /// [`DiNode::Undefined`] or must be distinct; `TableFull`.
    pub fn add(&mut self, node: &DiNode<'_>) -> IrResult<DiId> {
        let id = next_id(self.heads.len(), "debug node")?;
        let head = self.store(node, 0)?;
        let cells = &self.cells[head.at as usize..];
        let mut hasher = Hasher64::new(u64::from(head.kind) | (u64::from(head.tag) << 8));
        for cell in cells {
            hasher.word(u64::from(cell.raw()));
        }
        let hash = hasher.finish();
        let interner = self.interner.get_or_insert_with(InternTable::new);
        let found = interner.find(hash, |other| {
            let other = self.heads[other as usize];
            // Equal masks and list lengths make equal extents, so the
            // comparison never needs the other node's length.
            other.kind == head.kind
                && other.tag == head.tag
                && self.cells[other.at as usize..]
                    .get(..cells.len())
                    .is_some_and(|theirs| theirs == cells)
        });
        if let Some(found) = found {
            self.cells.truncate(head.at as usize);
            return Ok(DiId::new(found));
        }
        interner.insert(hash, id);
        self.heads.push(head);
        Ok(DiId::new(id))
    }

    /// The id of a new distinct node: never equal to another id,
    /// whatever its content.
    ///
    /// # Errors
    /// As [`DebugInfo::add`], without the distinctness rule.
    pub fn add_distinct(&mut self, node: &DiNode<'_>) -> IrResult<DiId> {
        let id = next_id(self.heads.len(), "debug node")?;
        let head = self.store(node, DISTINCT)?;
        self.heads.push(head);
        Ok(DiId::new(id))
    }

    /// Replaces the forward declaration `id` by its definition: LLVM's
    /// replaceable composite type. `id` must be a composite type with
    /// `DIFlagFwdDecl` and an `identifier`, and `node` a composite type
    /// with the same identifier. Every reference to `id` then names the
    /// definition. Whether the node is distinct does not change.
    ///
    /// # Errors
    /// `UndefinedDebugNode` if `id` does not exist or `node` names an id
    /// that does not; `InvalidDebugInfo` if either is not such a
    /// composite type; `TableFull`.
    pub fn complete(&mut self, id: DiId, node: &DiNode<'_>) -> IrResult<()> {
        let Some(&old) = self.heads.get(id.index()) else {
            return err(IrErrorKind::UndefinedDebugNode, format!("{id:?} does not exist"));
        };
        let (DiNode::CompositeType(declared), DiNode::CompositeType(defined)) =
            (self.node(id), node)
        else {
            return invalid(format!("only a composite type completes one, and {id:?} is not one"));
        };
        if !declared.flags.contains(names::DiFlags::FWD_DECL)
            || declared.identifier.is_empty()
            || declared.identifier != defined.identifier
        {
            return invalid(format!(
                "{id:?} is not a forward declaration with the identifier {:?}",
                String::from_utf8_lossy(defined.identifier)
            ));
        }
        let head = self.store(node, old.flags & DISTINCT)?;
        self.heads[id.index()] = head;
        Ok(())
    }

    /// Appends the cells of `node` and returns its head.
    fn store(&mut self, node: &DiNode<'_>, flags: u8) -> IrResult<Head> {
        if flags & DISTINCT == 0 && node.must_be_distinct() {
            return invalid(format!("a {} must be distinct", describe(node)));
        }
        let at = pool_start(self.cells.len(), 1, "debug node cell")?;
        self.cells.push(DiId::new(0));
        let mut enc = Enc {
            cells: &mut self.cells,
            strings: &mut self.strings,
            string_count: &mut self.string_count,
            string_bytes: &mut self.string_bytes,
            nodes: self.heads.len() as u32,
            exprs: self.exprs.len() as u32,
        };
        let encoded = node.encode(&mut enc).and_then(|done| {
            pool_start(self.cells.len(), 0, "debug node cell")?;
            Ok(done)
        });
        let (tag, mask) = match encoded {
            Ok(done) => done,
            Err(error) => {
                self.cells.truncate(at as usize);
                return Err(error);
            }
        };
        self.cells[at as usize] = DiId::new(mask);
        if let DiNode::Other(other) = node {
            // Interned by `encode` unless the name is empty.
            let name = self.strings.as_ref().and_then(|strings| strings.find(other.kind));
            let raw = name.map_or(u32::MAX, crate::strings::StrId::raw);
            if !self.other_kinds.contains(&raw) {
                self.other_kinds.push(raw);
            }
        }
        Ok(Head { at, kind: node.kind() as u8, flags, tag })
    }

    // ---- nodes: reading ----

    /// The number of node ids handed out; ids are `0..node_count()`.
    #[inline]
    pub fn node_count(&self) -> usize {
        self.heads.len()
    }

    /// Whether `id` is a node id of this table.
    #[inline]
    pub fn contains(&self, id: DiId) -> bool {
        id.index() < self.heads.len()
    }

    /// The kind of node `id`: one byte read. [`DiKind::Undefined`] for a
    /// reserved node that is not defined.
    ///
    /// # Panics
    /// If `id` is not a node of this table.
    #[inline]
    pub fn kind(&self, id: DiId) -> DiKind {
        DiKind::from_code(self.heads[id.index()].kind)
    }

    /// [`DebugInfo::kind`] without the panic: `None` for an id that is
    /// not a node of this table.
    #[inline]
    pub fn find_kind(&self, id: DiId) -> Option<DiKind> {
        self.heads.get(id.index()).map(|head| DiKind::from_code(head.kind))
    }

    /// The DWARF tag of node `id`; 0 for a kind without one.
    ///
    /// # Panics
    /// If `id` is not a node of this table.
    #[inline]
    pub fn tag(&self, id: DiId) -> u16 {
        self.heads[id.index()].tag
    }

    /// Whether node `id` was defined as distinct.
    ///
    /// # Panics
    /// If `id` is not a node of this table.
    #[inline]
    pub fn is_distinct(&self, id: DiId) -> bool {
        self.heads[id.index()].flags & DISTINCT != 0
    }

    /// Node `id`, borrowing its strings and lists from the table.
    /// [`DiNode::Undefined`] for a reserved node that is not defined.
    ///
    /// # Panics
    /// If `id` is not a node of this table.
    pub fn node(&self, id: DiId) -> DiNode<'_> {
        let head = self.heads[id.index()];
        let kind = DiKind::from_code(head.kind);
        if kind == DiKind::Undefined {
            return DiNode::Undefined;
        }
        let at = head.at as usize;
        let mask = self.cells[at].raw();
        let mut dec = Dec { cells: &self.cells, at: at + 1, strings: self.strings.as_ref() };
        DiNode::decode(kind, head.tag, mask, &mut dec)
    }

    /// The scope that encloses the local scope `id`: for a lexical block
    /// or block file, its `scope`; `None` for anything else, a
    /// subprogram included. Two array reads.
    ///
    /// # Panics
    /// If `id` is not a node of this table.
    #[inline]
    pub fn enclosing_block_scope(&self, id: DiId) -> Option<DiId> {
        let head = self.heads[id.index()];
        if !matches!(DiKind::from_code(head.kind), DiKind::LexicalBlock | DiKind::LexicalBlockFile)
        {
            return None;
        }
        // `scope` is the first field of both records.
        let at = head.at as usize;
        (self.cells[at].raw() & 1 != 0).then(|| self.cells[at + 1])
    }

    /// The subprogram the local scope `scope` belongs to: `scope` itself
    /// if it is one, else the subprogram its chain of enclosing blocks
    /// ends at. `None` if `scope` is not a node, or the chain does not
    /// end at a subprogram.
    pub fn subprogram_of(&self, scope: DiId) -> Option<DiId> {
        let mut at = scope;
        // A cycle of blocks is as long as the table at most.
        for _ in 0..=self.heads.len() {
            match self.find_kind(at)? {
                DiKind::Subprogram => return Some(at),
                _ => at = self.enclosing_block_scope(at)?,
            }
        }
        None
    }

    // ---- expressions ----

    /// The expression with these operations; equal expressions have
    /// equal ids, and the empty one is [`ExprId::EMPTY`].
    ///
    /// # Errors
    /// `TableFull`.
    pub fn expr(&mut self, ops: &[DiOp]) -> IrResult<ExprId> {
        self.exprs.add_ops(ops)
    }

    /// The expression with these elements, as LLVM stores them:
    /// operation codes, each followed by its operands. An element that
    /// is not a known operation is kept as [`DiOp::Other`].
    ///
    /// # Errors
    /// `InvalidDebugInfo` if a known operation lacks operands;
    /// `TableFull`.
    pub fn expr_from_elements(&mut self, elements: &[u64]) -> IrResult<ExprId> {
        self.exprs.add_elements(elements)
    }

    /// The operations of expression `id`. None for an id that names no
    /// expression.
    #[inline]
    pub fn ops(&self, id: ExprId) -> DiOps<'_> {
        self.exprs.ops(id)
    }

    /// The elements of expression `id`, as LLVM stores them.
    #[inline]
    pub fn expr_elements(&self, id: ExprId) -> &[u64] {
        self.exprs.elements(id)
    }

    /// The number of expressions; ids are `0..expr_count()`, and id 0,
    /// the empty expression, always exists.
    #[inline]
    pub fn expr_count(&self) -> usize {
        self.exprs.len()
    }

    // ---- locations ----

    /// The id of `loc`, interned: an equal location that was interned
    /// before is returned again. One hash of four words and, usually,
    /// one probe.
    ///
    /// # Errors
    /// `UndefinedDebugNode` if the scope or the inlined-at location does
    /// not exist; `TableFull`.
    pub fn loc(&mut self, loc: DiLocation) -> IrResult<LocId> {
        self.check_loc(loc)?;
        let mut hasher = Hasher64::new(0x10c);
        hasher.pair(loc.line, u32::from(loc.column));
        hasher.pair(loc.scope.raw(), loc.inlined_at.raw());
        let hash = hasher.finish();
        let record = LocRec {
            line: loc.line,
            scope: loc.scope,
            inlined_at: loc.inlined_at,
            column: loc.column,
            state: LOC_INTERNED,
        };
        let interner = self.loc_interner.get_or_insert_with(InternTable::new);
        if let Some(found) = interner.find(hash, |id| self.locs[id as usize] == record) {
            return Ok(LocId::new(found));
        }
        let id = next_id(self.locs.len(), "location")?;
        interner.insert(hash, id);
        self.locs.push(record);
        Ok(LocId::new(id))
    }

    /// A new location that is never equal to another: LLVM's `distinct
    /// !DILocation`, which `clang` uses for the call a function was
    /// inlined at, so that two calls on one line stay two inlined
    /// instances. Also the cheapest way in for a producer whose
    /// locations are unique already: one push, no hash.
    ///
    /// # Errors
    /// As [`DebugInfo::loc`].
    pub fn distinct_loc(&mut self, loc: DiLocation) -> IrResult<LocId> {
        self.check_loc(loc)?;
        let id = next_id(self.locs.len(), "location")?;
        self.locs.push(LocRec {
            line: loc.line,
            scope: loc.scope,
            inlined_at: loc.inlined_at,
            column: loc.column,
            state: LOC_DISTINCT,
        });
        Ok(LocId::new(id))
    }

    /// A new location id that [`DebugInfo::define_loc`] defines later:
    /// for a producer that meets `!dbg !7` before `!7`.
    ///
    /// # Errors
    /// `TableFull`.
    pub fn reserve_loc(&mut self) -> IrResult<LocId> {
        let id = next_id(self.locs.len(), "location")?;
        self.locs.push(LocRec {
            line: 0,
            scope: DiId::NONE,
            inlined_at: LocId::NONE,
            column: 0,
            state: LOC_RESERVED,
        });
        self.undefined_locs += 1;
        Ok(LocId::new(id))
    }

    /// Defines the reserved location `id`. It is not interned (a later
    /// [`DebugInfo::loc`] does not return it), and it is not distinct:
    /// the printer writes it as LLVM's uniqued `!DILocation`, as it was
    /// read. [`DebugInfo::define_distinct_loc`] defines a distinct one.
    ///
    /// # Errors
    /// `UndefinedDebugNode` if `id` was never reserved, or the scope or
    /// the inlined-at location does not exist; `DebugRedefined` if `id`
    /// is defined already.
    pub fn define_loc(&mut self, id: LocId, loc: DiLocation) -> IrResult<()> {
        self.define_loc_as(id, loc, LOC_UNIQUED)
    }

    /// [`DebugInfo::define_loc`] of LLVM's `distinct !DILocation`.
    ///
    /// # Errors
    /// As [`DebugInfo::define_loc`].
    pub fn define_distinct_loc(&mut self, id: LocId, loc: DiLocation) -> IrResult<()> {
        self.define_loc_as(id, loc, LOC_DISTINCT)
    }

    /// Whether location `id` is LLVM's `distinct !DILocation`: made by
    /// [`DebugInfo::distinct_loc`] or [`DebugInfo::define_distinct_loc`].
    #[inline]
    pub fn is_distinct_loc(&self, id: LocId) -> bool {
        self.locs.get(id.index()).is_some_and(|record| record.state == LOC_DISTINCT)
    }

    fn define_loc_as(&mut self, id: LocId, loc: DiLocation, state: u16) -> IrResult<()> {
        match self.locs.get(id.index()) {
            None => {
                return err(IrErrorKind::UndefinedDebugNode, format!("{id:?} was never reserved"));
            }
            Some(record) if record.state != LOC_RESERVED => {
                return err(IrErrorKind::DebugRedefined, format!("{id:?} is defined already"));
            }
            Some(_) => {}
        }
        self.check_loc(loc)?;
        self.locs[id.index()] = LocRec {
            line: loc.line,
            scope: loc.scope,
            inlined_at: loc.inlined_at,
            column: loc.column,
            state,
        };
        self.undefined_locs -= 1;
        Ok(())
    }

    fn check_loc(&self, loc: DiLocation) -> IrResult<()> {
        if !self.contains(loc.scope) {
            return err(
                IrErrorKind::UndefinedDebugNode,
                format!("a location's scope {:?} does not exist", loc.scope),
            );
        }
        if loc.inlined_at != LocId::NONE && loc.inlined_at.index() >= self.locs.len() {
            return err(
                IrErrorKind::UndefinedDebugNode,
                format!("a location is inlined at {:?}, which does not exist", loc.inlined_at),
            );
        }
        Ok(())
    }

    /// The number of location ids handed out; ids are `0..loc_count()`.
    #[inline]
    pub fn loc_count(&self) -> usize {
        self.locs.len()
    }

    /// Location `id`: one array read. `None` for [`LocId::NONE`], an id
    /// that is not a location of this table, and a reserved location
    /// that is not defined.
    #[inline]
    pub fn location(&self, id: LocId) -> Option<DiLocation> {
        let record = self.locs.get(id.index())?;
        (record.state != LOC_RESERVED).then_some(DiLocation {
            line: record.line,
            column: record.column,
            scope: record.scope,
            inlined_at: record.inlined_at,
        })
    }

    /// The subprogram whose code an instruction at `id` is part of: the
    /// subprogram of the scope at the end of the inlined-at chain, which
    /// is the function the instruction is in. `None` if `id` is not a
    /// defined location or the chain is broken.
    pub fn function_of(&self, id: LocId) -> Option<DiId> {
        let mut loc = self.location(id)?;
        for _ in 0..self.locs.len() {
            if loc.inlined_at == LocId::NONE {
                return self.subprogram_of(loc.scope);
            }
            loc = self.location(loc.inlined_at)?;
        }
        None
    }

    // ---- compile units ----

    /// Appends `unit` to the module's compile units: one operand of
    /// `!llvm.dbg.cu`. A unit that is not listed is not emitted.
    ///
    /// # Errors
    /// `UndefinedDebugNode` if `unit` does not exist; `InvalidDebugInfo`
    /// if it is defined and not a compile unit, or listed already.
    pub fn add_compile_unit(&mut self, unit: DiId) -> IrResult<()> {
        match self.find_kind(unit) {
            None => {
                return err(IrErrorKind::UndefinedDebugNode, format!("{unit:?} does not exist"));
            }
            Some(DiKind::CompileUnit | DiKind::Undefined) => {}
            Some(kind) => {
                return invalid(format!("{unit:?} is a {}, not a compile unit", kind.llvm_name()));
            }
        }
        if self.units.contains(&unit) {
            return invalid(format!("{unit:?} is listed as a compile unit already"));
        }
        self.units.push(unit);
        Ok(())
    }

    /// The compile units, in the order of `!llvm.dbg.cu`.
    #[inline]
    pub fn compile_units(&self) -> &[DiId] {
        &self.units
    }

    // ---- checks ----

    /// Checks that everything reserved was defined: nodes and
    /// locations. Two compares when it holds.
    ///
    /// # Errors
    /// `UndefinedDebugNode`, naming the first id that is not defined.
    pub fn check_defined(&self) -> IrResult<()> {
        if self.undefined != 0
            && let Some(index) = self.heads.iter().position(|head| head.kind == 0)
        {
            return err(
                IrErrorKind::UndefinedDebugNode,
                format!("{:?} was reserved and never defined", DiId::new(index as u32)),
            );
        }
        if self.undefined_locs != 0
            && let Some(index) = self.locs.iter().position(|loc| loc.state == LOC_RESERVED)
        {
            return err(
                IrErrorKind::UndefinedDebugNode,
                format!("{:?} was reserved and never defined", LocId::new(index as u32)),
            );
        }
        Ok(())
    }

    /// Checks the structure of the tables (§6.8): one pass over the
    /// nodes and the locations.
    ///
    /// - everything reserved was defined;
    /// - every reference names a node of a kind its field allows;
    /// - every listed compile unit is one;
    /// - a location's scope is a local scope whose chain of blocks ends
    ///   at a subprogram, and its inlined-at chain ends;
    /// - an expression's operations are placed as LLVM requires.
    ///
    /// # Errors
    /// `UndefinedDebugNode` for the first rule, `InvalidDebugInfo` for
    /// the others: the first violation found.
    pub fn verify(&self) -> IrResult<()> {
        self.check_defined()?;
        for index in 0..self.heads.len() {
            let id = DiId::new(index as u32);
            let node = self.node(id);
            let mut wrong = None;
            node.refs(&mut |field, allowed, target| {
                let kind = self.find_kind(target);
                if wrong.is_none() && kind.is_none_or(|kind| class::of(kind) & allowed == 0) {
                    wrong = Some((field, target, kind));
                }
            });
            if let Some((field, target, kind)) = wrong {
                let found = kind.map_or("no node", DiKind::llvm_name);
                return invalid(format!(
                    "{id:?}: {}.{field} names {target:?}, which is {found}",
                    node.kind().llvm_name()
                ));
            }
        }
        if let Some(unit) = self.units.iter().find(|&&unit| self.kind(unit) != DiKind::CompileUnit)
        {
            return invalid(format!("{unit:?} is listed as a compile unit and is not one"));
        }
        for index in 0..self.locs.len() {
            let id = LocId::new(index as u32);
            let record = self.locs[index];
            let local = self
                .find_kind(record.scope)
                .is_some_and(|kind| class::of(kind) & class::LOCAL_SCOPE != 0);
            if !local {
                return invalid(format!(
                    "{id:?}: the scope {:?} is not a subprogram or a block",
                    record.scope
                ));
            }
            if self.function_of(id).is_none() {
                return invalid(format!(
                    "{id:?}: its scopes and inlined-at locations do not end at a subprogram"
                ));
            }
        }
        for index in 1..self.exprs.len() {
            let id = ExprId::new(index as u32);
            if let Some(what) = order_error(self.exprs.ops(id)) {
                return invalid(format!("{id:?}: {what}"));
            }
        }
        Ok(())
    }

    /// The construct names `classify` reports for what the tables hold
    /// and hod cannot use: `debug:<kind>` for each kind of
    /// [`node::DiOther`] node, `debug:DW_OP_<name>` for each
    /// [`DiOp::Other`] element. Each once, in the order met.
    pub fn unsupported_constructs(&self) -> Vec<String> {
        let mut names = Vec::new();
        for &raw in &self.other_kinds {
            let kind = match (&self.strings, crate::strings::StrId::from_raw(raw)) {
                (Some(strings), Some(id)) => String::from_utf8_lossy(strings.get(id)).into_owned(),
                _ => String::new(),
            };
            names.push(format!("debug:{kind}"));
        }
        for &element in self.exprs.others() {
            let name = names::dw_op_name(element).unwrap_or_else(|| format!("DW_OP_{element:#x}"));
            names.push(format!("debug:{name}"));
        }
        names
    }

    /// What the tables hold and what it costs.
    pub fn stats(&self) -> DebugStats {
        DebugStats {
            nodes: self.heads.len(),
            node_bytes: size_of_val(&self.heads[..]) + size_of_val(&self.cells[..]),
            string_bytes: self.string_bytes,
            exprs: self.exprs.len(),
            expr_bytes: self.exprs.bytes(),
            locs: self.locs.len(),
            loc_bytes: size_of_val(&self.locs[..]),
        }
    }
}

/// "`DISubprogram` that is a definition", for messages.
fn describe(node: &DiNode<'_>) -> &'static str {
    match node {
        DiNode::Subprogram(_) => "DISubprogram that is a definition",
        other => other.kind().llvm_name(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §6.10: the record sizes, and an empty table has allocated
    /// nothing.
    #[test]
    fn record_sizes() {
        assert_eq!(size_of::<Head>(), 8);
        assert_eq!(size_of::<LocRec>(), 16);
        assert_eq!(size_of::<DiLocation>(), 16);
        assert_eq!(size_of::<ExprId>(), 4);
        let empty = DebugInfo::default();
        assert!(empty.is_empty());
        assert_eq!(empty.stats(), DebugStats { exprs: 1, ..DebugStats::default() });
        assert_eq!(empty.heads.capacity() + empty.cells.capacity() + empty.locs.capacity(), 0);
        assert!(
            empty.strings.is_none() && empty.interner.is_none() && empty.loc_interner.is_none()
        );
        empty.verify().unwrap();
    }
}
