//! The line-table transitions of a function as it is emitted:
//! `spec/codegen.md` §9.5, `spec/dwarf.md` §5.8.
//!
//! The driver opens a *segment* at the start of each instruction with the
//! instruction's row location; a lowering that emits code of ANOTHER
//! instruction in the middle of its own (a deferred instruction asked
//! for, or fused: a compare emitted by its branch) switches the segment
//! to that instruction's location and back with [`FnCx::line_switch`].
//! A segment that emitted code becomes a transition ([`LineRow`]) when
//! its location differs from the last one recorded; so the code of each
//! IR instruction is at its own location, wherever the lowering placed
//! it, which is where `llc` puts it (its machine instructions keep the
//! location of the IR instruction they were selected from). Nothing here
//! changes a byte of code.

use hod_ir::{Inst, InstKind, LocId, Opcode};

use crate::cx::FnCx;
use crate::record::{LineRow, ROW_CALL};
use crate::target::Target;

/// The location the code of `inst` is recorded at (`spec/dwarf.md`
/// §5.8): its own, except for two instructions whose code has none, so
/// that it stays in the row before it (a line-0 row when it is the first
/// code of its block, §5.3):
///
/// - `unreachable`: `llc` writes no code for it, so its line has no row
///   in `llc`'s table; hod's trap is not a statement of its own.
/// - a `ptradd` with offset 0 and no index (a zero-offset
///   `getelementptr`): its value is its base's, so its code is a copy or
///   reload of the base or the materialization of a frame or symbol
///   address, which in `llc`'s program are folded into the user or are
///   instructions without a location (register-allocator reloads, frame
///   index materializations).
#[inline]
pub(crate) fn row_loc(inst: Inst<'_>) -> LocId {
    match inst.opcode() {
        Opcode::Unreachable => LocId::NONE,
        Opcode::PtrAdd => match inst.kind() {
            InstKind::PtrAdd(add) if add.offset == 0 && add.index_count() == 0 => LocId::NONE,
            _ => inst.loc(),
        },
        _ => inst.loc(),
    }
}

impl<'a, T: Target> FnCx<'a, T> {
    /// Opens the segment of an instruction at the current offset.
    #[inline]
    pub(crate) fn line_begin(&mut self, loc: LocId) {
        self.line_seg = (self.offset(), loc);
        self.line_call = false;
    }

    /// Closes the open segment: a transition if it emitted code and its
    /// location is not the last one recorded, or if it is the first code
    /// of a block (other than the entry block) and has no location
    /// (`spec/dwarf.md` §5.3).
    #[inline]
    pub(crate) fn line_end(&mut self) {
        let (offset, loc) = self.line_seg;
        if !self.lines || self.offset() == offset {
            return;
        }
        let block_start = self.line_block_start;
        // §5.9: in a key-instructions function every segment is a row
        // (`key_rows` chooses among them), marked when it ends with a call.
        if self.key_lines || self.last_loc != Some(loc) || (block_start && loc == LocId::NONE) {
            self.last_loc = Some(loc);
            let marks = if self.line_call { ROW_CALL } else { 0 };
            self.out.lines.push(LineRow {
                stmt: self.row_stmt(),
                marks,
                ..LineRow::new(offset, loc, block_start)
            });
        }
        self.line_call = false;
        self.line_block_start = false;
    }

    /// The code emitted from here on, until the next switch, is at `loc`:
    /// a lowering that emits another instruction's code inside its own
    /// switches to that instruction's location ([`row_loc`] of it) before
    /// it and back to [`FnCx::line_loc`]'s value after it.
    #[inline]
    pub fn line_switch(&mut self, loc: LocId) {
        if !self.lines || self.line_seg.1 == loc {
            return;
        }
        self.line_end();
        self.line_begin(loc);
    }

    /// The location of the open segment.
    #[inline]
    pub fn line_loc(&self) -> LocId {
        self.line_seg.1
    }

    /// The row location of `inst` ([`row_loc`]).
    #[inline]
    pub fn row_loc_of(&self, inst: Inst<'a>) -> LocId {
        row_loc(inst)
    }
}
