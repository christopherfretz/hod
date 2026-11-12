//! The per-function output record: `spec/codegen.md` §9.1 to §9.6.
//!
//! A worker compiles a run of consecutive functions into a [`Chunk`]:
//! flat, append-only tables that one function after another adds to, plus
//! one [`FuncRecord`] per function holding ranges into them. Nothing in a
//! chunk is allocated per function. **Every offset in a row is relative
//! to the start of its function's code**, so a function's rows do not
//! depend on which chunk it is in, nor on what else is in it.

use std::ops::Range;

use hod_ir::{BlockId, DiId, ExprId, LocId, SymId};
use hod_obj::RelocKind;

use crate::frame::FrameRef;
use crate::sym::SymRef;

/// A relocation in a function's code (§9.2).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Reloc {
    /// The offset of the field.
    pub offset: u32,
    /// The psABI relocation, chosen by the target.
    pub kind: RelocKind,
    /// What it refers to.
    pub sym: SymRef,
    /// The addend.
    pub addend: i64,
}

/// A DWARF call-frame instruction, as data (§9.3): `hod_eh`'s [`Op`](hod_eh::Op),
/// the one row type of the code generator and the unwind-table writer
/// (`spec/eh.md` §1). Registers are DWARF register numbers; offsets are
/// bytes.
pub use hod_eh::Op as CfiOp;
/// A call-frame instruction and the code offset, from the function's
/// first byte, from which it holds: `hod_eh`'s [`Row`](hod_eh::Row).
pub use hod_eh::Row as CfiRow;

/// "No landing pad" in [`CallSiteRow::pad`].
pub const NO_PAD: u32 = u32::MAX;

/// One call instruction (§9.4), in code order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CallSiteRow {
    /// The offset of the start of what `Target::emit_call` emitted for
    /// the call: the row covers all of it.
    pub start: u32,
    /// Its length in bytes.
    pub len: u32,
    /// The index of the landing pad in the function's range of
    /// [`Chunk::pads`], which gives the row's action; [`NO_PAD`] for a
    /// plain `call` or a runtime call.
    pub pad: u32,
    /// The landing address, used only with a pad: the offset of the pad
    /// stub of an `invoke` with stack arguments (§5.3), or 0 for the
    /// pad's own [`PadRow::offset`]. 0 without a pad. (`spec/eh.md` §9.3
    /// rule 1 reads it the same way.)
    pub landing: u32,
    /// The callee cannot unwind: a direct call of a function declared
    /// `nounwind`. A row with this flag and no pad gets no entry in the
    /// call-site table, as `llc` (`EHStreamer::callToNoUnwindFunction`);
    /// with a pad (an `invoke`) it is covered as any other.
    pub nounwind: bool,
}

/// One landing pad (§9.4).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PadRow {
    /// The landing-pad block.
    pub block: BlockId,
    /// The code offset of the block.
    pub offset: u32,
    /// Whether the pad runs cleanups.
    pub cleanup: bool,
    /// Its clauses, a range of the function's range of
    /// [`Chunk::clauses`].
    pub clauses: Range<u32>,
}

/// One clause of a landing pad (§9.4): `hod_eh`'s [`Clause`](hod_eh::Clause),
/// `Catch(type id)` or `Filter(range)`. Type ids are indices + 1 into the
/// function's range of [`Chunk::type_ids`], the values
/// `llvm.eh.typeid.for` yields; a filter's range is of the function's
/// range of [`Chunk::filter_ids`].
pub use hod_eh::Clause as ClauseRow;

/// A line-table transition (§9.5): from `offset` on, the instructions
/// have location `loc`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LineRow {
    /// The code offset.
    pub offset: u32,
    /// The location; [`LocId::NONE`] for none.
    pub loc: LocId,
    /// The row's instruction is the first that emitted code in its
    /// block, and the block is not the entry block (`spec/dwarf.md`
    /// §5.3): a row without a location is then "no line", not the line
    /// of whatever was emitted before the block.
    pub block_start: bool,
}

/// Where the address a `#dbg_declare` names is, for the whole function
/// (§9.5); or, for a row that is not a declaration, what else it is.
///
/// The driver records [`VarPlace::Slot`] and [`VarPlace::Indirect`]
/// while the frame grows, and once the frame is final rewrites them to
/// [`VarPlace::Frame`] and [`VarPlace::FrameIndirect`]: a finished
/// record has no `Slot` or `Indirect` row.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VarPlace {
    /// The variable is in this frame slot.
    Slot(FrameRef),
    /// This frame slot holds the variable's address: an over-aligned slot
    /// (§5.1), or the home of a value that is not a slot.
    Indirect(FrameRef),
    /// The variable is this many bytes from the function's DWARF frame
    /// base ([`FrameDesc::frame_base`]).
    Frame(i64),
    /// The variable's address is this many bytes from the frame base.
    FrameIndirect(i64),
    /// A `#dbg_label` at this code offset; the row's `var` is the label.
    Label(u32),
    /// The row's variable or label cannot be described exactly: a full
    /// debug unit refuses the function under this stable name, a unit
    /// that writes no variables ignores it.
    Refused(&'static str),
}

/// One `#dbg_declare`, or one `#dbg_label` (§9.5).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VarRow {
    /// The variable (a local-variable debug node), or the label.
    pub var: DiId,
    /// Where it is declared.
    pub loc: LocId,
    /// How the variable is found from the declared address:
    /// [`ExprId::EMPTY`] when it is simply there.
    pub expr: ExprId,
    /// Where the declared address is.
    pub place: VarPlace,
}

/// A data island inside a function's code (§7.5), for mapping symbols.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IslandRow {
    /// The offset of its first byte.
    pub offset: u32,
    /// Its length in bytes.
    pub len: u32,
}

/// The code offset of a block (§9.6): of the block itself if it is
/// reachable, of the trap that stands for it otherwise.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BlockOffset {
    /// The block.
    pub block: BlockId,
    /// Its offset.
    pub offset: u32,
}

/// How to unwind a function's frame (§9.3).
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct FrameDesc {
    /// The final frame size in bytes, as the target's prologue allocates
    /// it.
    pub frame_size: u32,
    /// The DWARF register of the frame pointer, which is also the
    /// function's DWARF frame base: variable locations are offsets from
    /// it.
    pub frame_base: u16,
}

/// One compiled function: its symbol and its ranges in the chunk's
/// tables (§9.1).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FuncRecord {
    /// The function.
    pub sym: SymId,
    /// The alignment its code needs: the target's function alignment, or
    /// more for a data island that asks for more.
    pub align: u32,
    /// How to unwind its frame.
    pub frame: FrameDesc,
    /// Whether it has landing pads, and so an LSDA.
    pub has_pads: bool,
    /// Its personality function (`hod_ir` `SymbolKind::Function {
    /// personality }`), if it names one.
    pub personality: Option<SymId>,
    /// The function is `nounwind`.
    pub nounwind: bool,
    /// The function has `uwtable` (of either kind).
    pub uwtable: bool,
    /// Its bytes in [`Chunk::code`].
    pub code: Range<u32>,
    /// Its rows in [`Chunk::relocs`].
    pub relocs: Range<u32>,
    /// Its rows in [`Chunk::cfi`].
    pub cfi: Range<u32>,
    /// Its rows in [`Chunk::call_sites`].
    pub call_sites: Range<u32>,
    /// Its rows in [`Chunk::pads`].
    pub pads: Range<u32>,
    /// Its rows in [`Chunk::clauses`].
    pub clauses: Range<u32>,
    /// Its rows in [`Chunk::type_ids`].
    pub type_ids: Range<u32>,
    /// Its rows in [`Chunk::filter_ids`].
    pub filter_ids: Range<u32>,
    /// Its rows in [`Chunk::lines`].
    pub lines: Range<u32>,
    /// Its rows in [`Chunk::vars`].
    pub vars: Range<u32>,
    /// Its rows in [`Chunk::islands`].
    pub islands: Range<u32>,
    /// Its rows in [`Chunk::block_offsets`].
    pub block_offsets: Range<u32>,
}

impl FuncRecord {
    /// Whether the function gets an `.eh_frame` entry (`spec/eh.md`
    /// §1.1): LLVM's `Function::needsUnwindTableEntry`, true unless it is
    /// `nounwind` without `uwtable` and without a personality.
    #[inline]
    pub fn needs_unwind_entry(&self) -> bool {
        self.uwtable || !self.nounwind || self.personality.is_some()
    }
}

/// The output of a run of function compiles (§9.1).
///
/// `#dbg_label`s are rows of [`Chunk::vars`] ([`VarPlace::Label`]); a
/// `#dbg_value` or `#dbg_assign` is a [`VarPlace::Refused`] row.
#[derive(Clone, PartialEq, Eq, Default, Debug)]
pub struct Chunk {
    /// The functions' instructions and data islands, one function after
    /// another without padding.
    pub code: Vec<u8>,
    /// Relocations (§9.2).
    pub relocs: Vec<Reloc>,
    /// Call-frame instructions (§9.3).
    pub cfi: Vec<CfiRow>,
    /// Call sites (§9.4).
    pub call_sites: Vec<CallSiteRow>,
    /// Landing pads (§9.4).
    pub pads: Vec<PadRow>,
    /// Landing-pad clauses (§9.4).
    pub clauses: Vec<ClauseRow>,
    /// The functions' type-info tables (§9.4): `None` is catch-all.
    pub type_ids: Vec<Option<SymId>>,
    /// The type ids of `filter` clauses (§9.4).
    pub filter_ids: Vec<u32>,
    /// Line-table transitions (§9.5).
    pub lines: Vec<LineRow>,
    /// Declared variables and labels (§9.5).
    pub vars: Vec<VarRow>,
    /// Data islands (§7.5).
    pub islands: Vec<IslandRow>,
    /// Block offsets (§9.6).
    pub block_offsets: Vec<BlockOffset>,
    /// One record per function, in the order they were compiled.
    pub funcs: Vec<FuncRecord>,
}

/// The lengths of a chunk's tables at one moment: where a function
/// began, and what a failed compile is truncated to (§1).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Mark {
    pub(crate) code: u32,
    pub(crate) relocs: u32,
    pub(crate) cfi: u32,
    pub(crate) call_sites: u32,
    pub(crate) pads: u32,
    pub(crate) clauses: u32,
    pub(crate) type_ids: u32,
    pub(crate) filter_ids: u32,
    pub(crate) lines: u32,
    pub(crate) vars: u32,
    pub(crate) islands: u32,
    pub(crate) block_offsets: u32,
    pub(crate) funcs: u32,
}

impl Chunk {
    /// An empty chunk.
    pub fn new() -> Chunk {
        Chunk::default()
    }

    /// Empties every table, keeping the allocations.
    pub fn clear(&mut self) {
        self.truncate(Mark {
            code: 0,
            relocs: 0,
            cfi: 0,
            call_sites: 0,
            pads: 0,
            clauses: 0,
            type_ids: 0,
            filter_ids: 0,
            lines: 0,
            vars: 0,
            islands: 0,
            block_offsets: 0,
            funcs: 0,
        });
    }

    /// The code of function record `func`.
    pub fn code_of(&self, func: &FuncRecord) -> &[u8] {
        &self.code[func.code.start as usize..func.code.end as usize]
    }

    /// The bytes the tables hold, not counting spare capacity.
    pub fn table_bytes(&self) -> usize {
        self.code.len()
            + size_of_val(self.relocs.as_slice())
            + size_of_val(self.cfi.as_slice())
            + size_of_val(self.call_sites.as_slice())
            + size_of_val(self.pads.as_slice())
            + size_of_val(self.clauses.as_slice())
            + size_of_val(self.type_ids.as_slice())
            + size_of_val(self.filter_ids.as_slice())
            + size_of_val(self.lines.as_slice())
            + size_of_val(self.vars.as_slice())
            + size_of_val(self.islands.as_slice())
            + size_of_val(self.block_offsets.as_slice())
            + size_of_val(self.funcs.as_slice())
    }

    pub(crate) fn mark(&self) -> Mark {
        Mark {
            code: self.code.len() as u32,
            relocs: self.relocs.len() as u32,
            cfi: self.cfi.len() as u32,
            call_sites: self.call_sites.len() as u32,
            pads: self.pads.len() as u32,
            clauses: self.clauses.len() as u32,
            type_ids: self.type_ids.len() as u32,
            filter_ids: self.filter_ids.len() as u32,
            lines: self.lines.len() as u32,
            vars: self.vars.len() as u32,
            islands: self.islands.len() as u32,
            block_offsets: self.block_offsets.len() as u32,
            funcs: self.funcs.len() as u32,
        }
    }

    pub(crate) fn truncate(&mut self, mark: Mark) {
        self.code.truncate(mark.code as usize);
        self.relocs.truncate(mark.relocs as usize);
        self.cfi.truncate(mark.cfi as usize);
        self.call_sites.truncate(mark.call_sites as usize);
        self.pads.truncate(mark.pads as usize);
        self.clauses.truncate(mark.clauses as usize);
        self.type_ids.truncate(mark.type_ids as usize);
        self.filter_ids.truncate(mark.filter_ids as usize);
        self.lines.truncate(mark.lines as usize);
        self.vars.truncate(mark.vars as usize);
        self.islands.truncate(mark.islands as usize);
        self.block_offsets.truncate(mark.block_offsets as usize);
        self.funcs.truncate(mark.funcs as usize);
    }

    /// The record of everything from `mark` to now, for function `sym`.
    pub(crate) fn record(&self, mark: Mark, sym: SymId) -> FuncRecord {
        let now = self.mark();
        FuncRecord {
            sym,
            align: 1,
            frame: FrameDesc::default(),
            has_pads: false,
            personality: None,
            nounwind: false,
            uwtable: false,
            code: mark.code..now.code,
            relocs: mark.relocs..now.relocs,
            cfi: mark.cfi..now.cfi,
            call_sites: mark.call_sites..now.call_sites,
            pads: mark.pads..now.pads,
            clauses: mark.clauses..now.clauses,
            type_ids: mark.type_ids..now.type_ids,
            filter_ids: mark.filter_ids..now.filter_ids,
            lines: mark.lines..now.lines,
            vars: mark.vars..now.vars,
            islands: mark.islands..now.islands,
            block_offsets: mark.block_offsets..now.block_offsets,
        }
    }
}
