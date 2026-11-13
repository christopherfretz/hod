//! Variables described by `#dbg_value` records (WP-162, `spec/dwarf.md`
//! §6.23, `spec/codegen.md` §9.5): the ranges of code where hod can
//! prove where a variable's value is.
//!
//! A record stands before an instruction and says that from there on the
//! variable has the record's value. hod is an `-O0` code generator: a
//! value that has a home (§4.1) is stored there at its definition and
//! the home is never written again (a `phi`'s home is written at the head
//! of its block only; the edges write its incoming slot, §4.5). So a
//! range is proved, without any analysis of the code, from the record to
//! the earlier of the next record for the same variable instance and the
//! end of its block. A branch (`br`, `switch`, `indirectbr`, `invoke`)
//! keeps the frame and writes no home, so a range reaches the end of its
//! code; a return (whose exit sequence tears the frame down), a
//! `musttail` call and the other terminators end the ranges at their
//! start.
//!
//! In that range the variable is:
//! - in its value's home ([`ValueAt::Home`], then [`ValueAt::Frame`] once
//!   the frame is final), when the value is an argument or an
//!   instruction result with a home that is written before the range
//!   starts: an argument (stored by the entry code), an instruction of
//!   the same block before the record, or of a block that dominates the
//!   record's block, and its type is one whose home holds exactly its
//!   bytes (an integer of 8, 16, 32 or 64 bits, a pointer, `float`,
//!   `double`);
//! - a constant ([`ValueAt::Const`]): an integer of at most 64 bits or a
//!   null pointer;
//! - nowhere hod can prove ([`ValueAt::None`], `<optimized out>`)
//!   otherwise: a register value (fused into its one use), a dead value,
//!   `undef`, `poison`, an empty location, a slot's or a symbol's address,
//!   a floating-point or wide constant.
//!
//! Ranges are never extended across blocks (LLVM's `LiveDebugValues`
//! does that; follow-up in the landed record): where `llc` shows a value
//! hod may show `<optimized out>`, never another value.
//!
//! Each range is one [`VarRow`] with [`VarPlace::Value`]: the row's
//! variable, location (its `inlinedAt` says which instance) and
//! expression are the record's. A record whose range is empty still has
//! a row, so that the variable gets its DIE. `hod_dwarf` groups the rows
//! of one instance into a location list. Every offset goes through the
//! same mapping as the line table when x86-64 code shrinks
//! ([`FnCx::map_value_ranges`], `spec/codegen.md` §5.6.15).

use hod_ir::{
    BlockId, Const, DbgRecord, DiId, ExprId, Inst, InstId, LocId, Opcode, TailKind, TypeKind,
    Value, ValueKind,
};

use crate::cx::FnCx;
use crate::error::CgResult;
use crate::frame::FrameRef;
use crate::loc::{Key, Kind, Loc};
use crate::record::{VarPlace, VarRow};
use crate::target::Target;

/// A `#dbg_value` whose location is a `DIArgList` (several values
/// combined by `DW_OP_LLVM_arg`): not written yet.
pub const DEBUG_DBG_VALUE_ARGLIST: &str = "debug:dbg-value-arglist";

/// A `#dbg_value` in a coroutine body (WP-108): its frame is not the
/// function's alone, and no producer of the corpus writes one.
pub const DEBUG_DBG_VALUE_COROUTINE: &str = "debug:dbg-value-coroutine";

/// The end of a range not closed yet.
const OPEN: u32 = u32::MAX;

/// Where a variable described by value records is over one range.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ValueAt {
    /// In this frame slot, the value's home, while the frame grows.
    Home(FrameRef, u8),
    /// This many bytes from the function's DWARF frame base; the value
    /// is that many bytes wide.
    Frame(i64, u8),
    /// This constant: an integer `width` bits wide, zero-extended.
    Const {
        /// The bits, those above `width` clear.
        bits: u64,
        /// The width in bits, 1 to 64.
        width: u8,
    },
    /// Nowhere hod can prove (`<optimized out>`).
    None,
}

/// One range of a variable described by value records: code offsets
/// `start..end` of the function, and where the variable is there.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ValueRange {
    /// The first byte of the range.
    pub start: u32,
    /// One past its last byte; equal to `start` for an empty range.
    pub end: u32,
    /// Where the variable is.
    pub at: ValueAt,
}

/// A `#dbg_value` waiting for its instruction.
#[derive(Clone, Copy, Debug)]
struct Pending {
    before: u32,
    var: DiId,
    loc: LocId,
    expr: ExprId,
    value: Value,
    /// The instance: the record location's `inlinedAt`.
    ia: LocId,
}

/// The per-function state (part of [`crate::Scratch`]), reset by
/// [`FnCx::value_records`].
#[derive(Debug, Default)]
pub(crate) struct ValueScratch {
    records: Vec<Pending>,
    /// The ranges not closed: variable, instance, index in `Chunk::vars`.
    open: Vec<(DiId, LocId, u32)>,
    /// The block being emitted, as `BlockId` raw + 1 (0: none yet).
    block: u32,
    /// Its first byte (before its `phi` loads), while no instruction of
    /// it has been emitted; `OPEN` after.
    head: u32,
    /// By instruction: its block. Filled when a record names a value of
    /// another block.
    block_of: Vec<u32>,
    /// By block: its immediate dominator's index, `u32::MAX` for the
    /// entry and unreachable blocks; and its reverse post-order number.
    idom: Vec<u32>,
    rpo: Vec<u32>,
    order: Vec<u32>,
    dom_ready: bool,
}

impl ValueScratch {
    /// The bytes the tables have allocated.
    pub(crate) fn allocated_bytes(&self) -> usize {
        fn bytes<T>(v: &Vec<T>) -> usize {
            v.capacity() * size_of::<T>()
        }
        bytes(&self.records)
            + bytes(&self.open)
            + bytes(&self.block_of)
            + bytes(&self.idom)
            + bytes(&self.rpo)
            + bytes(&self.order)
    }
}

impl<'a, T: Target> FnCx<'a, T> {
    /// Before the first instruction (`layout_slots`): the function's
    /// value records, in position order. A record hod cannot describe
    /// at all is a refused row (`debug:dbg-value-arglist`,
    /// `debug:dbg-value-coroutine`).
    pub(crate) fn value_records(&mut self) {
        let v = &mut self.s.dbg_values;
        v.records.clear();
        v.open.clear();
        v.block = 0;
        v.head = OPEN;
        v.dom_ready = false;
        let coroutine = self.coro.is_some();
        for record in self.body.dbg_records() {
            if record.kind != hod_ir::DbgRecordKind::Value {
                continue;
            }
            let refused = if coroutine {
                Some(DEBUG_DBG_VALUE_COROUTINE)
            } else if record.arg_list || record.values.len() != 1 {
                Some(DEBUG_DBG_VALUE_ARGLIST)
            } else {
                None
            };
            if let Some(name) = refused {
                self.out.vars.push(refused_row(&record, name));
                continue;
            }
            let ia = self.module.debug.location(record.loc).map_or(LocId::NONE, |l| l.inlined_at);
            self.s.dbg_values.records.push(Pending {
                before: record.before.raw(),
                var: record.var,
                loc: record.loc,
                expr: record.expr,
                value: record.values[0],
                ia,
            });
        }
    }

    /// Whether the function has value records to place.
    #[inline]
    pub(crate) fn has_value_records(&self) -> bool {
        !self.s.dbg_values.records.is_empty()
    }

    /// At the start of instruction `inst`'s code: the ranges its records
    /// open, and those it closes (§6.23): a record closes its instance's
    /// open range; a terminator or a `musttail` call closes every range
    /// of the block.
    pub(crate) fn value_rows(&mut self, inst: Inst<'a>) -> CgResult<()> {
        let offset = self.offset();
        let block = self.block.raw() + 1;
        if self.s.dbg_values.block != block {
            // Every range closes at its block's end or before its
            // terminator; one still open (a block left early) claims
            // nothing.
            self.close_all(None);
            self.s.dbg_values.block = block;
            self.s.dbg_values.head = OPEN;
        }
        let head = std::mem::replace(&mut self.s.dbg_values.head, OPEN);
        // A branch keeps the frame and writes no home (its edges write
        // the incoming slots, §4.5): ranges reach the block's end
        // (`value_block_end`). A return, a `musttail` call and the other
        // terminators tear the frame down or leave: ranges end before
        // them.
        let branch = matches!(
            inst.opcode(),
            Opcode::Br | Opcode::CondBr | Opcode::Switch | Opcode::IndirectBr | Opcode::Invoke
        );
        let ends = (inst.is_terminator() && !branch)
            || (inst.opcode() == Opcode::Call
                && inst.call().is_some_and(|c| c.tail() == TailKind::MustTail));
        if ends {
            self.close_all(Some(offset));
        }
        let raw = inst.id().raw();
        let records = &self.s.dbg_values.records;
        let mut at = records.partition_point(|r| r.before < raw);
        while let Some(&r) = self.s.dbg_values.records.get(at) {
            if r.before != raw {
                break;
            }
            at += 1;
            // The instance's range so far ends here.
            if let Some(k) =
                self.s.dbg_values.open.iter().position(|&(var, ia, _)| var == r.var && ia == r.ia)
            {
                let (_, _, row) = self.s.dbg_values.open.swap_remove(k);
                self.set_end(row, offset);
            }
            let place = if ends { ValueAt::None } else { self.value_at(r.value, inst)? };
            let end = if ends { offset } else { OPEN };
            // A record before a block's first instruction holds from the
            // block's first byte, unless its value is a `phi` of the
            // block, whose home the `phi` loads there write (§4.5).
            let start =
                if head != OPEN && !ends && !self.phi_here(r.value) { head } else { offset };
            let row = self.out.vars.len() as u32;
            self.out.vars.push(VarRow {
                var: r.var,
                loc: r.loc,
                expr: r.expr,
                place: VarPlace::Value(ValueRange { start, end, at: place }),
            });
            if !ends {
                self.s.dbg_values.open.push((r.var, r.ia, row));
            }
        }
        Ok(())
    }

    /// Before a block's `phi` loads (not the entry block's, whose code
    /// before its first instruction stores the arguments): its first
    /// byte.
    pub(crate) fn value_block_start(&mut self) {
        self.close_all(None);
        self.s.dbg_values.block = self.block.raw() + 1;
        self.s.dbg_values.head = if self.block == self.body.entry() { OPEN } else { self.offset() };
    }

    /// Whether `value` is a `phi` of the block being emitted.
    fn phi_here(&self, value: Value) -> bool {
        let ValueKind::Inst(def) = value.kind() else { return false };
        self.body.inst(def).opcode() == Opcode::Phi
            && self.body.block(self.block).inst_range().contains(&def.raw())
    }

    /// After a block's last instruction: its ranges end here.
    pub(crate) fn value_block_end(&mut self) {
        let offset = self.offset();
        self.close_all(Some(offset));
    }

    /// Closes every open range at `offset`, or, for `None`, at its own
    /// start (it claims nothing).
    fn close_all(&mut self, offset: Option<u32>) {
        let mut open = std::mem::take(&mut self.s.dbg_values.open);
        for &(_, _, row) in &open {
            match offset {
                Some(offset) => self.set_end(row, offset),
                None => {
                    if let Some(VarRow { place: VarPlace::Value(r), .. }) =
                        self.out.vars.get_mut(row as usize)
                    {
                        r.end = r.start;
                    }
                }
            }
        }
        open.clear();
        self.s.dbg_values.open = open;
    }

    fn set_end(&mut self, row: u32, offset: u32) {
        if let Some(VarRow { place: VarPlace::Value(r), .. }) = self.out.vars.get_mut(row as usize)
        {
            r.end = offset.max(r.start);
        }
    }

    /// Where `value` is from the start of `inst`'s code to the end of the
    /// range (module doc).
    fn value_at(&mut self, value: Value, inst: Inst<'a>) -> CgResult<ValueAt> {
        let key = match value.kind() {
            ValueKind::Const(id) => {
                return Ok(match self.module.consts.get(id) {
                    Const::Int { ty, value } => match self.types().kind(ty) {
                        TypeKind::Int(width @ 1..=64) => {
                            ValueAt::Const { bits: value, width: width as u8 }
                        }
                        _ => ValueAt::None,
                    },
                    Const::Null(_) => ValueAt::Const { bits: 0, width: 64 },
                    _ => ValueAt::None,
                });
            }
            ValueKind::Inst(_) | ValueKind::Arg(_) => match Key::of(value) {
                Some(key) => key,
                None => return Ok(ValueAt::None),
            },
            _ => return Ok(ValueAt::None),
        };
        let size = match self.types().kind(self.body.value_type(self.module, value)) {
            TypeKind::Int(bits @ (8 | 16 | 32 | 64)) => (bits / 8) as u8,
            TypeKind::Ptr { addr_space: 0 } | TypeKind::Double => 8,
            TypeKind::Float => 4,
            _ => return Ok(ValueAt::None),
        };
        let state = self.state(key)?;
        let (Kind::Home, Loc::Frame(home)) = (state.kind, state.loc) else {
            return Ok(ValueAt::None);
        };
        if let ValueKind::Inst(def) = value.kind() {
            let d = self.body.inst(def);
            if d.is_terminator() || !self.home_written(def, inst.id()) {
                return Ok(ValueAt::None);
            }
        }
        Ok(ValueAt::Home(home, size))
    }

    /// Whether instruction `def`'s home is written before the code of
    /// `at`, in the block being emitted, starts: `def` is earlier in the
    /// same block (a `phi` is loaded at the block's head), or its block
    /// dominates this one.
    fn home_written(&mut self, def: InstId, at: InstId) -> bool {
        let here = self.block;
        let range = self.body.block(here).inst_range();
        if range.contains(&def.raw()) {
            return def.raw() < at.raw();
        }
        if !self.s.dbg_values.dom_ready {
            self.dominators();
        }
        let v = &self.s.dbg_values;
        let Some(&from) = v.block_of.get(def.index()) else { return false };
        let mut b = here.raw();
        // Up the dominator tree from this block: `from` dominates it if
        // it is on the way to the entry.
        loop {
            let Some(&up) = v.idom.get(b as usize) else { return false };
            if up == u32::MAX {
                return false;
            }
            if up == from {
                return true;
            }
            // `idom` numbers decrease in reverse post-order: stop below
            // `from`'s number.
            if v.rpo[up as usize] < v.rpo[from as usize] {
                return false;
            }
            b = up;
        }
    }

    /// The dominator tree of the reachable blocks (Cooper, Harvey and
    /// Kennedy's iteration over reverse post-order) and each
    /// instruction's block.
    fn dominators(&mut self) {
        let body = self.body;
        let n = body.block_count();
        let v = &mut self.s.dbg_values;
        v.dom_ready = true;
        v.block_of.clear();
        v.block_of.resize(body.inst_count(), u32::MAX);
        for b in 0..n {
            let range = body.block(BlockId::new(b as u32)).inst_range();
            for i in range {
                if let Some(slot) = v.block_of.get_mut(i as usize) {
                    *slot = b as u32;
                }
            }
        }
        // Post-order by an explicit stack: (block, next successor).
        v.rpo.clear();
        v.rpo.resize(n, u32::MAX);
        v.order.clear();
        let entry = body.entry().raw();
        let mut seen = vec![false; n];
        seen[entry as usize] = true;
        let mut stack = vec![(entry, body.block(body.entry()).successors())];
        while let Some((b, succs)) = stack.last_mut() {
            if let Some(s) = succs.next() {
                if !seen[s.index()] {
                    seen[s.index()] = true;
                    stack.push((s.raw(), body.block(s).successors()));
                }
            } else {
                v.order.push(*b);
                stack.pop();
            }
        }
        v.order.reverse();
        for (k, &b) in v.order.iter().enumerate() {
            v.rpo[b as usize] = k as u32;
        }
        // Predecessors, as (block, pred) pairs sorted by block.
        let mut preds: Vec<(u32, u32)> = Vec::new();
        for &b in &v.order {
            for s in body.block(BlockId::new(b)).successors() {
                preds.push((s.raw(), b));
            }
        }
        preds.sort_unstable();
        v.idom.clear();
        v.idom.resize(n, u32::MAX);
        v.idom[entry as usize] = entry;
        let mut changed = true;
        while changed {
            changed = false;
            for k in 1..v.order.len() {
                let b = v.order[k];
                let first = preds.partition_point(|&(s, _)| s < b);
                let mut new = u32::MAX;
                for &(s, p) in &preds[first..] {
                    if s != b {
                        break;
                    }
                    if v.idom[p as usize] == u32::MAX {
                        continue;
                    }
                    new = if new == u32::MAX { p } else { intersect(&v.idom, &v.rpo, p, new) };
                }
                if new != u32::MAX && v.idom[b as usize] != new {
                    v.idom[b as usize] = new;
                    changed = true;
                }
            }
        }
        // The entry has no dominator.
        v.idom[entry as usize] = u32::MAX;
    }

    /// Once the frame is final (`finish_vars`): every home made an
    /// offset from the frame base, and any range still open closed at
    /// its start.
    pub(crate) fn finish_value_ranges(&mut self, start: usize) {
        if !self.has_value_records() {
            return;
        }
        self.close_all(None);
        let t = self.target;
        let desc = self.frame_desc;
        for row in self.out.vars.get_mut(start..).unwrap_or_default() {
            if let VarPlace::Value(r) = &mut row.place
                && let ValueAt::Home(home, size) = r.at
            {
                r.at = match t.frame_base_offset(&desc, home) {
                    Some(offset) => ValueAt::Frame(offset, size),
                    None => ValueAt::None,
                };
            }
        }
    }

    /// The ranges' offsets after the function's code shrank
    /// (`spec/codegen.md` §5.6.15), by the mapping every table uses.
    pub(crate) fn map_value_ranges(&mut self, map: impl Fn(u32) -> u32) {
        if !self.has_value_records() {
            return;
        }
        let first = self.mark.vars as usize;
        for row in self.out.vars.get_mut(first..).unwrap_or_default() {
            if let VarPlace::Value(r) = &mut row.place {
                let end = if r.end == OPEN { OPEN } else { map(r.end) };
                r.start = map(r.start);
                r.end = if end == OPEN { end } else { end.max(r.start) };
            }
        }
    }
}

/// The dominator two blocks have in common (Cooper, Harvey, Kennedy).
fn intersect(idom: &[u32], rpo: &[u32], mut a: u32, mut b: u32) -> u32 {
    while a != b {
        while rpo[a as usize] > rpo[b as usize] {
            a = idom[a as usize];
        }
        while rpo[b as usize] > rpo[a as usize] {
            b = idom[b as usize];
        }
    }
    a
}

/// The row of a record hod refuses under `name`.
fn refused_row(record: &DbgRecord<'_>, name: &'static str) -> VarRow {
    VarRow { var: record.var, loc: record.loc, expr: record.expr, place: VarPlace::Refused(name) }
}

/// The bridge's place of a value row (`emit/lines.rs`): a home that was
/// never resolved to the frame base is not a place the writer can name.
pub(crate) fn local_place(r: ValueRange) -> hod_dwarf::LocalPlace {
    use hod_dwarf::{LocalPlace, ValueLoc};
    let at = match r.at {
        ValueAt::Frame(offset, size) => ValueLoc::Frame { offset, size },
        ValueAt::Const { bits, width } => ValueLoc::Const { bits, width },
        ValueAt::None => ValueLoc::None,
        ValueAt::Home(..) => return LocalPlace::Refused(crate::error::DEBUG_FRAME_PLACE),
    };
    LocalPlace::Value { start: r.start, end: r.end, at }
}
