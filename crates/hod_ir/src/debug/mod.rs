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
use crate::intern::{Hasher64, InternTable, Layered, Layering, NoLayer, Seg, next_id, pool_start};
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

/// A location's key-instruction atom (LLVM 21 and later, WP-149,
/// `spec/ir.md` §6.5, `spec/dwarf.md` §5.9): the source atom
/// (`atomGroup`, unique within the function and its inlined-at chain) the
/// instruction belongs to, and its precedence in it (`atomRank`, 1 the
/// highest). [`DiAtom::NONE`] (both 0) for none, which is every location
/// of LLVM 19 and of a function without `keyInstructions`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct DiAtom {
    /// `atomGroup`; below 2^61 (LLVM stores 61 bits).
    pub group: u64,
    /// `atomRank`; at most 7 (LLVM stores 3 bits).
    pub rank: u8,
}

impl DiAtom {
    /// No atom.
    pub const NONE: DiAtom = DiAtom { group: 0, rank: 0 };
    /// The largest `atomGroup` LLVM stores (61 bits).
    pub const MAX_GROUP: u64 = (1 << 61) - 1;
    /// The largest `atomRank` LLVM stores (3 bits).
    pub const MAX_RANK: u8 = 7;

    /// Whether the location takes part in key instructions: LLVM's
    /// `getAtomGroup() && getAtomRank()`.
    #[inline]
    pub fn is_some(self) -> bool {
        self.group != 0 && self.rank != 0
    }
}

/// A location's atom in the side table, behind [`LOC_ATOM`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct AtomRec {
    loc: u32,
    rank: u8,
    group: u64,
}

/// A stored location. Sixteen bytes. An atom (WP-149) is not in it: the
/// state's [`LOC_ATOM`] bit says the side table has one, so a module
/// without atoms pays nothing.
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
/// The bits of the state above: the rest are flags.
const LOC_KIND: u16 = 3;
/// Flag: the location has an atom in the side table (WP-149).
const LOC_ATOM: u16 = 4;

/// The mask bit of a subprogram's `keyInstructions`, its 17th field
/// (`DiKind::Subprogram.fields()[16]`).
const SUBPROGRAM_KEY_INSTRUCTIONS: u32 = 1 << 16;

impl LocRec {
    #[inline]
    fn kind(&self) -> u16 {
        self.state & LOC_KIND
    }
}

fn atom_hash(loc: u32) -> u32 {
    let mut hasher = Hasher64::new(0xa70);
    hasher.pair(loc, 0);
    hasher.finish()
}

/// The atom of location `id` in `locs`, `atoms` and `index`: the side
/// table, looked up only when the record says it has one.
#[inline]
fn atom_of<L: Layering>(
    locs: &Seg<LocRec, L>,
    atoms: &Seg<AtomRec, L>,
    index: Option<&InternTable<L>>,
    id: u32,
) -> DiAtom {
    match (locs.get(id as usize), index) {
        (Some(record), Some(index)) if record.state & LOC_ATOM != 0 => index
            .find(atom_hash(id), |at| atoms[at as usize].loc == id)
            .map_or(DiAtom::NONE, |at| {
                let a = atoms[at as usize];
                DiAtom { group: a.group, rank: a.rank }
            }),
        _ => DiAtom::NONE,
    }
}

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
    /// Bytes of location storage: sixteen per location, and the side
    /// table of atoms (WP-149).
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
#[derive(Clone, Debug)]
pub struct DebugInfo<L: Layering = NoLayer> {
    heads: Seg<Head, L>,
    /// Masks, numbers, string ids and node ids, as the nodes' fields
    /// need them. A node list is stored in place, so that it is read as
    /// a slice.
    cells: Seg<DiId, L>,
    /// The number of strings in `strings` and the bytes they take, for
    /// the size report.
    string_count: u32,
    string_bytes: usize,
    /// Boxed: the tables of a module without debug information are a
    /// fixed number of words.
    strings: Option<Box<StrPool<L>>>,
    /// The uniqued nodes.
    interner: Option<InternTable<L>>,
    /// Nodes reserved and not defined.
    undefined: u32,
    /// String ids of the kind names of `DiOther` nodes, each once.
    other_kinds: Vec<u32>,
    exprs: ExprTable<L>,
    locs: Seg<LocRec, L>,
    loc_interner: Option<InternTable<L>>,
    undefined_locs: u32,
    /// The atoms of the locations with [`LOC_ATOM`] (WP-149), and their
    /// index by location id; none at all in a module without atoms.
    atoms: Seg<AtomRec, L>,
    atom_index: Option<InternTable<L>>,
    units: Vec<DiId>,
    /// Entries changed in place (a reserved node defined, a declaration
    /// completed, a reserved location defined): with the counts, what
    /// tells whether forks still read this table (`spec/ir.md` §5.1).
    edits: u32,
}

impl Default for DebugInfo {
    fn default() -> Self {
        DebugInfo {
            heads: Seg::default(),
            cells: Seg::default(),
            string_count: 0,
            string_bytes: 0,
            strings: None,
            interner: None,
            undefined: 0,
            other_kinds: Vec::new(),
            exprs: ExprTable::default(),
            locs: Seg::default(),
            loc_interner: None,
            undefined_locs: 0,
            atoms: Seg::default(),
            atom_index: None,
            units: Vec::new(),
            edits: 0,
        }
    }
}

#[cold]
fn invalid<T>(message: String) -> IrResult<T> {
    err(IrErrorKind::InvalidDebugInfo, message)
}

/// An atom LLVM stores: a group below 2^61, a rank of at most 7.
#[inline]
fn check_atom(atom: DiAtom) -> IrResult<()> {
    if atom.group > DiAtom::MAX_GROUP || atom.rank > DiAtom::MAX_RANK {
        return invalid(format!(
            "a location's atom (group {}, rank {}) does not fit LLVM's 61 and 3 bits",
            atom.group, atom.rank
        ));
    }
    Ok(())
}

impl<L: Layering> DebugInfo<L> {
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
        self.edits = self.edits.wrapping_add(1);
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
        let cells = self.cells.from(head.at as usize);
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
                && self
                    .cells
                    .from(other.at as usize)
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
        self.edits = self.edits.wrapping_add(1);
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
            let name = self.strings.as_deref().and_then(|strings| strings.find(other.kind));
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
        let mut dec = Dec { cells: self.cells.from(at), at: 1, strings: self.strings.as_deref() };
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

    /// Whether node `id` is a subprogram with `keyInstructions`
    /// (WP-149): what `matches!(self.node(id), DiNode::Subprogram(s) if
    /// s.key_instructions)` says, from two array reads instead of a
    /// decode. A `bool` field is its bit in the record's mask and no
    /// word.
    ///
    /// # Panics
    /// If `id` is not a node of this table.
    #[inline]
    pub fn has_key_instructions(&self, id: DiId) -> bool {
        let head = self.heads[id.index()];
        DiKind::from_code(head.kind) == DiKind::Subprogram
            && self.cells[head.at as usize].raw() & SUBPROGRAM_KEY_INSTRUCTIONS != 0
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
        self.loc_without_atom(loc)
    }

    /// [`DebugInfo::loc`] of a location with an atom (WP-149): LLVM's
    /// `atomGroup` and `atomRank`, part of the location's identity (two
    /// locations that differ only in their atoms are two locations).
    /// [`DiAtom::NONE`] makes exactly what [`DebugInfo::loc`] makes.
    ///
    /// # Errors
    /// As [`DebugInfo::loc`]; `InvalidDebugInfo` for an atom LLVM does
    /// not store (a group of 2^61 or more, a rank above 7).
    pub fn loc_in_atom(&mut self, loc: DiLocation, atom: DiAtom) -> IrResult<LocId> {
        if atom == DiAtom::NONE {
            return self.loc_without_atom(loc);
        }
        self.check_loc(loc)?;
        check_atom(atom)?;
        let mut hasher = Hasher64::new(0x10c);
        hasher.pair(loc.line, u32::from(loc.column));
        hasher.pair(loc.scope.raw(), loc.inlined_at.raw());
        let with = atom != DiAtom::NONE;
        if with {
            hasher.word(atom.group);
            hasher.pair(u32::from(atom.rank), 0);
        }
        let hash = hasher.finish();
        let record = LocRec {
            line: loc.line,
            scope: loc.scope,
            inlined_at: loc.inlined_at,
            column: loc.column,
            state: LOC_INTERNED | if with { LOC_ATOM } else { 0 },
        };
        let interner = self.loc_interner.get_or_insert_with(InternTable::new);
        let (locs, atoms, index) = (&self.locs, &self.atoms, self.atom_index.as_ref());
        if let Some(found) = interner.find(hash, |id| {
            locs[id as usize] == record && (!with || atom_of(locs, atoms, index, id) == atom)
        }) {
            return Ok(LocId::new(found));
        }
        let id = next_id(self.locs.len(), "location")?;
        interner.insert(hash, id);
        self.locs.push(record);
        if with {
            self.push_atom(id, atom);
        }
        Ok(LocId::new(id))
    }

    /// [`DebugInfo::loc_in_atom`] of [`DiAtom::NONE`]: what `loc` was
    /// before atoms (WP-149). The record's state is [`LOC_INTERNED`]
    /// without [`LOC_ATOM`], so a location with an atom never compares
    /// equal to it and needs no atom compare; the hash is the one
    /// `loc_in_atom` makes for no atom.
    #[inline]
    fn loc_without_atom(&mut self, loc: DiLocation) -> IrResult<LocId> {
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
        let locs = &self.locs;
        if let Some(found) = interner.find(hash, |id| locs[id as usize] == record) {
            return Ok(LocId::new(found));
        }
        let id = next_id(self.locs.len(), "location")?;
        interner.insert(hash, id);
        self.locs.push(record);
        Ok(LocId::new(id))
    }

    fn push_atom(&mut self, id: u32, atom: DiAtom) {
        let at = self.atoms.len() as u32;
        self.atoms.push(AtomRec { loc: id, rank: atom.rank, group: atom.group });
        self.atom_index.get_or_insert_with(InternTable::new).insert(atom_hash(id), at);
    }

    /// The atom of location `id` (WP-149); [`DiAtom::NONE`] for a
    /// location without one, [`LocId::NONE`] and an id that is no
    /// location. One flag test for a location without an atom.
    #[inline]
    pub fn atom(&self, id: LocId) -> DiAtom {
        atom_of(&self.locs, &self.atoms, self.atom_index.as_ref(), id.raw())
    }

    /// Whether any location has an atom (WP-149): `false` for every
    /// module LLVM 19 wrote, and for one without key instructions.
    #[inline]
    pub fn has_atoms(&self) -> bool {
        !self.atoms.is_empty()
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
        self.distinct_loc_in_atom(loc, DiAtom::NONE)
    }

    /// [`DebugInfo::distinct_loc`] with an atom (WP-149).
    ///
    /// # Errors
    /// As [`DebugInfo::loc_in_atom`].
    pub fn distinct_loc_in_atom(&mut self, loc: DiLocation, atom: DiAtom) -> IrResult<LocId> {
        self.check_loc(loc)?;
        check_atom(atom)?;
        let id = next_id(self.locs.len(), "location")?;
        let with = atom != DiAtom::NONE;
        self.locs.push(LocRec {
            line: loc.line,
            scope: loc.scope,
            inlined_at: loc.inlined_at,
            column: loc.column,
            state: LOC_DISTINCT | if with { LOC_ATOM } else { 0 },
        });
        if with {
            self.push_atom(id, atom);
        }
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
        self.define_loc_as(id, loc, LOC_UNIQUED, DiAtom::NONE)
    }

    /// [`DebugInfo::define_loc`] of LLVM's `distinct !DILocation`.
    ///
    /// # Errors
    /// As [`DebugInfo::define_loc`].
    pub fn define_distinct_loc(&mut self, id: LocId, loc: DiLocation) -> IrResult<()> {
        self.define_loc_as(id, loc, LOC_DISTINCT, DiAtom::NONE)
    }

    /// [`DebugInfo::define_loc`] (or, with `distinct`,
    /// [`DebugInfo::define_distinct_loc`]) of a location with an atom
    /// (WP-149).
    ///
    /// # Errors
    /// As [`DebugInfo::define_loc`]; `InvalidDebugInfo` for an atom LLVM
    /// does not store.
    pub fn define_loc_in_atom(
        &mut self,
        id: LocId,
        loc: DiLocation,
        atom: DiAtom,
        distinct: bool,
    ) -> IrResult<()> {
        check_atom(atom)?;
        self.define_loc_as(id, loc, if distinct { LOC_DISTINCT } else { LOC_UNIQUED }, atom)
    }

    /// Whether location `id` is LLVM's `distinct !DILocation`: made by
    /// [`DebugInfo::distinct_loc`] or [`DebugInfo::define_distinct_loc`].
    #[inline]
    pub fn is_distinct_loc(&self, id: LocId) -> bool {
        self.locs.get(id.index()).is_some_and(|record| record.kind() == LOC_DISTINCT)
    }

    fn define_loc_as(
        &mut self,
        id: LocId,
        loc: DiLocation,
        state: u16,
        atom: DiAtom,
    ) -> IrResult<()> {
        match self.locs.get(id.index()) {
            None => {
                return err(IrErrorKind::UndefinedDebugNode, format!("{id:?} was never reserved"));
            }
            Some(record) if record.kind() != LOC_RESERVED => {
                return err(IrErrorKind::DebugRedefined, format!("{id:?} is defined already"));
            }
            Some(_) => {}
        }
        self.check_loc(loc)?;
        self.edits = self.edits.wrapping_add(1);
        self.locs[id.index()] = LocRec {
            line: loc.line,
            scope: loc.scope,
            inlined_at: loc.inlined_at,
            column: loc.column,
            state: state | if atom == DiAtom::NONE { 0 } else { LOC_ATOM },
        };
        if atom != DiAtom::NONE {
            self.push_atom(id.raw(), atom);
        }
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
        (record.kind() != LOC_RESERVED).then_some(DiLocation {
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
            && let Some(index) = self.locs.iter().position(|loc| loc.kind() == LOC_RESERVED)
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
            node_bytes: self.heads.bytes() + self.cells.bytes(),
            string_bytes: self.string_bytes,
            exprs: self.exprs.len(),
            expr_bytes: self.exprs.bytes(),
            locs: self.locs.len(),
            loc_bytes: self.locs.bytes() + self.atoms.bytes(),
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

/// The sizes of the debug tables a body does not add to, for telling
/// whether a fork changed them (`spec/ir.md` §5.1).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct DebugCounts {
    heads: usize,
    cells: usize,
    strings: usize,
    other_kinds: usize,
    units: usize,
    undefined: u32,
    undefined_locs: u32,
}

/// What one body added to a fork's debug tables: expressions and
/// locations, at their fork-local ids from the mark on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DebugDelta {
    exprs: Vec<Box<[u64]>>,
    locs: Vec<LocRec>,
    /// The atoms of the `locs` with [`LOC_ATOM`], in their order.
    atoms: Vec<DiAtom>,
}

/// Fork-local expression and location ids to a module's.
#[derive(Clone, Debug, Default)]
pub(crate) struct DebugRemap {
    exprs_base: usize,
    exprs: Vec<ExprId>,
    locs_base: usize,
    locs: Vec<LocId>,
}

impl DebugRemap {
    pub(crate) fn is_identity(&self) -> bool {
        self.exprs.is_empty() && self.locs.is_empty()
    }

    #[inline]
    pub(crate) fn expr(&self, id: ExprId) -> ExprId {
        // The empty expression is id 0 everywhere, made or not.
        if id == ExprId::EMPTY {
            return id;
        }
        match id.index().checked_sub(self.exprs_base) {
            Some(at) => self.exprs[at],
            None => id,
        }
    }

    #[inline]
    pub(crate) fn loc(&self, id: LocId) -> LocId {
        if id == LocId::NONE {
            return id;
        }
        match id.index().checked_sub(self.locs_base) {
            Some(at) => self.locs[at],
            None => id,
        }
    }
}

impl DebugInfo {
    // ---- `spec/ir.md` §5.1: reading bodies in parallel ----

    /// The base of the forks of parallel reading: a copy.
    pub(crate) fn seed(&self) -> DebugInfo<Layered> {
        DebugInfo {
            heads: self.heads.seed(),
            cells: self.cells.seed(),
            string_count: self.string_count,
            string_bytes: self.string_bytes,
            strings: self.strings.as_ref().map(|strings| Box::new(strings.seed())),
            interner: self.interner.as_ref().map(InternTable::seed),
            undefined: self.undefined,
            other_kinds: self.other_kinds.clone(),
            exprs: self.exprs.seed(),
            locs: self.locs.seed(),
            loc_interner: self.loc_interner.as_ref().map(InternTable::seed),
            undefined_locs: self.undefined_locs,
            atoms: self.atoms.seed(),
            atom_index: self.atom_index.as_ref().map(InternTable::seed),
            units: self.units.clone(),
            edits: 0,
        }
    }

    /// Adds `delta`'s expressions and locations as the fork made them,
    /// with their ids translated, and extends `map` with the ids they
    /// became.
    pub(crate) fn adopt(&mut self, delta: &DebugDelta, map: &mut DebugRemap) -> IrResult<()> {
        for elements in &delta.exprs {
            let id = self.exprs.add_elements(elements)?;
            map.exprs.push(id);
        }
        let mut atoms = delta.atoms.iter();
        for record in &delta.locs {
            let loc = DiLocation {
                line: record.line,
                column: record.column,
                scope: record.scope,
                inlined_at: map.loc(record.inlined_at),
            };
            let atom = if record.state & LOC_ATOM != 0 {
                atoms.next().copied().unwrap_or(DiAtom::NONE)
            } else {
                DiAtom::NONE
            };
            let id = if record.kind() == LOC_DISTINCT {
                self.distinct_loc_in_atom(loc, atom)?
            } else {
                self.loc_in_atom(loc, atom)?
            };
            map.locs.push(id);
        }
        Ok(())
    }

    /// The entries changed in place so far.
    pub(crate) fn edits(&self) -> u32 {
        self.edits
    }
}

impl DebugInfo<Layered> {
    pub(crate) fn fork(&self) -> DebugInfo<Layered> {
        DebugInfo {
            heads: self.heads.fork(),
            cells: self.cells.fork(),
            string_count: self.string_count,
            string_bytes: self.string_bytes,
            strings: self.strings.as_ref().map(|strings| Box::new(strings.fork())),
            interner: self.interner.as_ref().map(InternTable::fork),
            undefined: self.undefined,
            other_kinds: self.other_kinds.clone(),
            exprs: self.exprs.fork(),
            locs: self.locs.fork(),
            loc_interner: self.loc_interner.as_ref().map(InternTable::fork),
            undefined_locs: self.undefined_locs,
            atoms: self.atoms.fork(),
            atom_index: self.atom_index.as_ref().map(InternTable::fork),
            units: self.units.clone(),
            edits: 0,
        }
    }

    /// Whether an entry of the shared base was changed in place.
    pub(crate) fn touched(&self) -> bool {
        self.heads.touched() || self.cells.touched() || self.locs.touched()
    }

    /// Whether the tables a body does not add to changed since `origin`.
    pub(crate) fn changed_since(&self, origin: &DebugCounts) -> bool {
        self.counts() != *origin || self.touched()
    }

    /// The expressions and locations from `mark` on; `None` if a
    /// location was reserved or defined rather than interned or made
    /// distinct, which only the module itself can do in order.
    pub(crate) fn delta(&self, (exprs, locs): (usize, usize)) -> Option<DebugDelta> {
        let first = locs;
        let locs: Vec<LocRec> = (locs..self.locs.len()).map(|id| self.locs[id]).collect();
        if locs.iter().any(|loc| loc.kind() != LOC_INTERNED && loc.kind() != LOC_DISTINCT) {
            return None;
        }
        let atoms = (first..self.locs.len())
            .filter(|&id| self.locs[id].state & LOC_ATOM != 0)
            .map(|id| self.atom(LocId::new(id as u32)))
            .collect();
        Some(DebugDelta { exprs: self.exprs.owned_since(exprs), locs, atoms })
    }
}

impl<L: Layering> DebugInfo<L> {
    pub(crate) fn counts(&self) -> DebugCounts {
        DebugCounts {
            heads: self.heads.len(),
            cells: self.cells.len(),
            strings: self.strings.as_ref().map_or(0, |strings| strings.len()),
            other_kinds: self.other_kinds.len(),
            units: self.units.len(),
            undefined: self.undefined,
            undefined_locs: self.undefined_locs,
        }
    }

    /// `(expressions, locations)`: where a fork's next ones go.
    pub(crate) fn mark(&self) -> (usize, usize) {
        (self.exprs.mark(), self.locs.len())
    }

    /// A remap for a fork of this table as it is now.
    pub(crate) fn remap(&self) -> DebugRemap {
        let (exprs_base, locs_base) = self.mark();
        DebugRemap { exprs_base, exprs: Vec::new(), locs_base, locs: Vec::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DiFieldValue;

    /// §6.10: the record sizes, and an empty table has allocated
    /// nothing.
    #[test]
    fn record_sizes() {
        assert_eq!(size_of::<Head>(), 8);
        assert_eq!(size_of::<LocRec>(), 16);
        assert_eq!(size_of::<DiLocation>(), 16);
        assert_eq!(size_of::<ExprId>(), 4);
        let empty = <DebugInfo>::default();
        assert!(empty.is_empty());
        assert_eq!(empty.stats(), DebugStats { exprs: 1, ..DebugStats::default() });
        assert_eq!(empty.heads.capacity() + empty.cells.capacity() + empty.locs.capacity(), 0);
        assert!(
            empty.strings.is_none() && empty.interner.is_none() && empty.loc_interner.is_none()
        );
        empty.verify().unwrap();
    }

    /// WP-170: `has_key_instructions` says what the decoded node says,
    /// for subprograms with and without the field, with every other
    /// field set or not, and for a node of another kind.
    #[test]
    fn key_instructions_from_the_mask() {
        let mut debug = <DebugInfo>::default();
        let mut ids = Vec::new();
        for key in [false, true] {
            for full in [false, true] {
                let mut sp = DiNode::llvm_default(DiKind::Subprogram);
                if full {
                    sp.set(b"name", DiFieldValue::Str(b"f")).unwrap();
                    sp.set(b"line", DiFieldValue::UInt(3)).unwrap();
                    sp.set(b"virtualIndex", DiFieldValue::UInt(2)).unwrap();
                }
                sp.set(b"keyInstructions", DiFieldValue::Bool(key)).unwrap();
                ids.push(debug.add_distinct(&sp).unwrap());
            }
        }
        let mut ty = DiNode::llvm_default(DiKind::CompositeType);
        for field in [&b"name"[..], b"identifier"] {
            ty.set(field, DiFieldValue::Str(b"S")).unwrap();
        }
        for field in [&b"line"[..], b"size", b"align", b"offset", b"runtimeLang"] {
            ty.set(field, DiFieldValue::UInt(8)).unwrap();
        }
        ids.push(debug.add(&ty).unwrap());
        for id in ids {
            let decoded = matches!(debug.node(id), DiNode::Subprogram(s) if s.key_instructions);
            assert_eq!(debug.has_key_instructions(id), decoded, "{id:?}");
        }
        assert_eq!(DiKind::Subprogram.fields()[16], "keyInstructions");
    }

    /// WP-170: `loc` (the path without an atom) and `loc_in_atom` share
    /// one interner: equal locations are one id, a location with an atom
    /// is another, and `loc` after it still finds the one without.
    #[test]
    fn locations_with_and_without_atoms() {
        let mut debug = <DebugInfo>::default();
        let sp = debug.add_distinct(&DiNode::llvm_default(DiKind::Subprogram)).unwrap();
        let at = DiLocation { line: 4, column: 2, scope: sp, inlined_at: LocId::NONE };
        let plain = debug.loc(at).unwrap();
        assert_eq!(debug.loc_in_atom(at, DiAtom::NONE).unwrap(), plain);
        let atom = DiAtom { group: 1, rank: 1 };
        let with = debug.loc_in_atom(at, atom).unwrap();
        assert_ne!(with, plain);
        assert_eq!(debug.loc_in_atom(at, atom).unwrap(), with);
        assert_eq!(debug.loc(at).unwrap(), plain);
        assert_eq!(debug.atom(plain), DiAtom::NONE);
        assert_eq!(debug.atom(with), atom);
        let other = debug.loc(DiLocation { line: 5, ..at }).unwrap();
        assert_ne!(other, plain);
        assert!(debug.loc(DiLocation { scope: DiId::new(99), ..at }).is_err());
        debug.verify().unwrap();
    }
}
