//! Key instructions (WP-149, `spec/dwarf.md` §5.9): which line rows of a
//! function whose subprogram has `keyInstructions` are statements.
//!
//! clang 22 gives the instructions of each source atom (a statement, an
//! assignment, a branch) an `atomGroup` and an `atomRank` (1 the
//! highest), and `llc` 22 marks as statements only the rows of the
//! atoms' *key* instructions instead of every row that changes the line
//! (`DwarfDebug::computeKeyInstructions`). This module applies that rule
//! to hod's rows. In a key-instructions function the driver records a
//! row for every piece of code it emits (not only for a change of
//! location), so that each row stands for the instructions `llc` would
//! have selected from one IR instruction, and marks the rows whose code
//! ends with a call ([`ROW_CALL`]) and those that continue a row emitted
//! elsewhere ([`ROW_CONT`]). Once the function is complete,
//! [`key_rows`] walks the rows in code order as `llc` walks its machine
//! instructions:
//!
//! - a row whose location has no line (none, or line 0) is skipped;
//! - the *buoy* is the first row of a run of rows with one line in one
//!   block; a new block, a new line, a call, or an atom other than the
//!   buoy's starts a new one;
//! - a row with an atom makes the buoy a candidate of its atom `(inlined
//!   at, group)` when its rank is the best seen for the atom, after
//!   dropping the candidates of a worse rank (an equal rank drops
//!   nothing: `llc` 22's `remove_if` is not followed by an `erase`);
//! - a call makes the buoy a key instruction outright, ends the buoy, and
//!   with an atom drops the atom's candidates of a worse rank without
//!   becoming one itself;
//! - at the end every candidate is a key instruction.
//!
//! The rows that are key instructions are [`Stmt::Key`], the others of
//! the function [`Stmt::NotKey`]; `hod_dwarf::function_rows` writes the
//! first with `is_stmt` and the others without (the prologue end has it
//! anyway). A function with locations of both kinds of subprogram (code
//! inlined from a function compiled without key instructions, or the
//! other way round) is refused, [`DEBUG_KEY_INSTRUCTIONS_MIXED`]; so is a
//! coroutine body with key instructions,
//! [`DEBUG_KEY_INSTRUCTIONS_COROUTINE`] (`llc` sees it only after
//! `CoroSplit`, so there is nothing to measure the rows against).

use std::collections::HashMap;

use hod_ir::{DiAtom, DiId, SymbolKind};

use crate::cx::FnCx;
use crate::error::{CgError, CgResult};
use crate::record::{LineRow, ROW_CALL, ROW_CONT, Stmt};
use crate::sym::{RuntimeFn, SymRef};
use crate::target::{CallTarget, Target};

/// A function whose locations come from subprograms with and without
/// key instructions (§5.9 K7).
pub const DEBUG_KEY_INSTRUCTIONS_MIXED: &str = "debug:key-instructions-mixed";
/// A coroutine body (WP-106) with key instructions (§5.9 K7).
pub const DEBUG_KEY_INSTRUCTIONS_COROUTINE: &str = "debug:key-instructions-coroutine";

/// The pass's tables, kept across functions.
#[derive(Debug, Default)]
pub(crate) struct KeyScratch {
    /// Per atom `(inlined at, group)`: the best rank seen and its
    /// candidates (row indices).
    candidates: HashMap<(u32, u64), (u8, Vec<u32>)>,
    /// The candidate lists of atoms of earlier functions, for reuse.
    spare: Vec<Vec<u32>>,
    /// Per row of the function: whether it is a key instruction.
    keys: Vec<bool>,
}

impl KeyScratch {
    pub(crate) fn allocated_bytes(&self) -> usize {
        self.keys.capacity()
            + self.candidates.capacity() * size_of::<((u32, u64), (u8, Vec<u32>))>()
            + self.spare.iter().map(|v| v.capacity() * 4).sum::<usize>()
    }
}

/// Whether the subprogram `sp` has `keyInstructions`: a flag read, no
/// decode of the node (WP-170).
fn is_key(debug: &hod_ir::DebugInfo, sp: Option<DiId>) -> bool {
    sp.is_some_and(|sp| debug.has_key_instructions(sp))
}

impl<T: Target> FnCx<'_, T> {
    /// Whether the function's rows follow key instructions: it has line
    /// rows and its subprogram has `keyInstructions` (§5.9).
    pub(crate) fn key_function(&self) -> bool {
        if !self.lines {
            return false;
        }
        let SymbolKind::Function { subprogram, .. } = self.module.symbol(self.body.symbol()).kind()
        else {
            return false;
        };
        is_key(&self.module.debug, subprogram)
    }

    /// The default [`Stmt`] of a row pushed now.
    #[inline]
    pub(crate) fn row_stmt(&self) -> Stmt {
        if self.key_lines { Stmt::NotKey } else { Stmt::Line }
    }

    /// A call of `target` was emitted for the open segment and returns
    /// here (§5.9 K4): in a key-instructions function, the segment's row
    /// ends here, marked as ending with a call, and the code after the
    /// call is a row of its own. `_Unwind_Resume` is no call there: `llc`
    /// makes it from a `resume` without a location (`DwarfEHPrepare`).
    pub(crate) fn key_call_returns(&mut self, target: CallTarget) {
        if !self.key_lines || target == CallTarget::Sym(SymRef::Runtime(RuntimeFn::UnwindResume)) {
            return;
        }
        let loc = self.line_seg.1;
        self.line_call = true;
        self.line_end();
        self.line_begin(loc);
    }

    /// A `musttail` call is about to branch to its exit: the open
    /// segment's row ends with a call (§5.9 K4).
    pub(crate) fn key_tail_call(&mut self) {
        if self.key_lines {
            self.line_call = true;
        }
    }

    /// Decides [`LineRow::stmt`] of the function's rows (§5.9), once
    /// every row is recorded; checks that a function without key
    /// instructions has no location of a subprogram with them.
    ///
    /// # Errors
    /// [`DEBUG_KEY_INSTRUCTIONS_MIXED`], [`DEBUG_KEY_INSTRUCTIONS_COROUTINE`].
    pub(crate) fn key_rows(&mut self) -> CgResult<()> {
        if !self.lines || !(self.key_lines || self.module.debug.has_atoms()) {
            return Ok(());
        }
        let debug = &self.module.debug;
        let first = self.mark.lines as usize;
        let rows = &mut self.out.lines[first..];
        // K7: every location's subprogram is of the function's kind.
        let mut last: Option<(DiId, bool)> = None;
        for row in rows.iter() {
            let Some(loc) = debug.location(row.loc) else { continue };
            let key = match last {
                Some((scope, key)) if scope == loc.scope => key,
                _ => {
                    let key = is_key(debug, debug.subprogram_of(loc.scope));
                    last = Some((loc.scope, key));
                    key
                }
            };
            if key != self.key_lines {
                return Err(CgError::unsupported(DEBUG_KEY_INSTRUCTIONS_MIXED));
            }
        }
        if !self.key_lines {
            return Ok(());
        }
        if self.coro.is_some() {
            return Err(CgError::unsupported(DEBUG_KEY_INSTRUCTIONS_COROUTINE));
        }
        let ks = &mut self.s.key_rows;
        decide(debug, rows, ks);
        Ok(())
    }
}

/// The key instructions of `rows` (§5.9 K1 to K6), as `llc` 22 chooses
/// them over its machine instructions; sets every row's [`Stmt`].
fn decide(debug: &hod_ir::DebugInfo, rows: &mut [LineRow], ks: &mut KeyScratch) {
    for (_, (_, mut list)) in ks.candidates.drain() {
        list.clear();
        ks.spare.push(list);
    }
    ks.keys.clear();
    ks.keys.resize(rows.len(), false);
    // The buoy: its row and line; the group of the atom it serves (0
    // for none yet).
    let mut buoy: Option<(u32, u32)> = None;
    let mut buoy_atom = 0u64;
    for (i, row) in rows.iter().enumerate() {
        let i = i as u32;
        if row.marks & ROW_CONT != 0 {
            continue; // K6
        }
        if row.block_start {
            buoy = None; // K2: a new block
            buoy_atom = 0;
        }
        let Some(loc) = debug.location(row.loc) else { continue };
        if loc.line == 0 {
            continue; // K1
        }
        if buoy.is_none_or(|(_, line)| line != loc.line) {
            buoy = Some((i, loc.line));
            buoy_atom = 0;
        }
        let atom = debug.atom(row.loc);
        let key = (loc.inlined_at.raw(), atom.group);
        if atom.is_some() {
            // K3: an atom other than the buoy's starts a buoy here.
            if buoy_atom != 0 && buoy_atom != atom.group {
                buoy = Some((i, loc.line));
                buoy_atom = atom.group;
            }
            let at = buoy.map_or(i, |(b, _)| b);
            if candidate(ks, key, atom, Some(at)) {
                buoy_atom = atom.group;
            }
        }
        if row.marks & ROW_CALL != 0 {
            // K4: the call is key through its buoy, and nothing floats
            // up to it.
            if let Some((b, _)) = buoy {
                ks.keys[b as usize] = true;
            }
            buoy = None;
            buoy_atom = 0;
            if atom.is_some() {
                candidate(ks, key, atom, None);
            }
        }
    }
    for (rank, list) in ks.candidates.values() {
        debug_assert!(*rank != 0 || list.is_empty(), "a candidate without a rank");
        for &i in list {
            ks.keys[i as usize] = true;
        }
    }
    for (row, &key) in rows.iter_mut().zip(&ks.keys) {
        row.stmt = if key { Stmt::Key } else { Stmt::NotKey };
    }
}

/// One instruction of atom `key` with `atom`'s rank (§5.9 K3): its buoy
/// `at` becomes a candidate when the rank is not worse than the best
/// seen, after the candidates of a worse rank are dropped; a call
/// (`at` none) only drops them. Returns whether `at` became one.
fn candidate(ks: &mut KeyScratch, key: (u32, u64), atom: DiAtom, at: Option<u32>) -> bool {
    let spare = &mut ks.spare;
    let (rank, list) =
        ks.candidates.entry(key).or_insert_with(|| (0, spare.pop().unwrap_or_default()));
    if *rank != 0 && *rank < atom.rank {
        return false;
    }
    if *rank > atom.rank {
        list.clear();
    }
    if let Some(at) = at {
        list.push(at);
        *rank = atom.rank;
        true
    } else {
        if list.is_empty() {
            *rank = 0;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hod_ir::{DebugInfo, DiFieldValue, DiKind, DiLocation, DiNode, LocId};

    const CALL: u8 = ROW_CALL;

    /// A table with one subprogram and a location per `(line, group,
    /// rank)`.
    fn table(locs: &[(u32, u64, u8)]) -> (DebugInfo, Vec<LocId>) {
        let mut debug = DebugInfo::default();
        let mut sp = DiNode::llvm_default(DiKind::Subprogram);
        sp.set(b"keyInstructions", DiFieldValue::Bool(true)).unwrap();
        let sp = debug.add_distinct(&sp).unwrap();
        let ids = locs
            .iter()
            .map(|&(line, group, rank)| {
                let loc = DiLocation { line, column: 0, scope: sp, inlined_at: LocId::NONE };
                debug.loc_in_atom(loc, DiAtom { group, rank }).unwrap()
            })
            .collect();
        (debug, ids)
    }

    fn rows(ids: &[LocId], spec: &[(usize, bool, u8)]) -> Vec<LineRow> {
        spec.iter()
            .enumerate()
            .map(|(i, &(loc, block_start, marks))| LineRow {
                marks,
                ..LineRow::new(i as u32 * 4, ids[loc], block_start)
            })
            .collect()
    }

    fn keys(debug: &DebugInfo, rows: &mut [LineRow]) -> Vec<usize> {
        decide(debug, rows, &mut KeyScratch::default());
        rows.iter().enumerate().filter(|(_, r)| r.stmt == Stmt::Key).map(|(i, _)| i).collect()
    }

    /// §5.9 K2, K3: `int y = x + 1;` (a load without an atom, the add of
    /// rank 2, the store of rank 1): the buoy is the load, the first row
    /// of the line; `return y;` likewise.
    #[test]
    fn the_buoy_is_the_first_row_of_the_line() {
        let (debug, ids) = table(&[(11, 0, 0), (11, 1, 2), (11, 1, 1), (12, 2, 2), (12, 2, 1)]);
        let mut r = rows(
            &ids,
            &[(0, false, 0), (1, false, 0), (2, false, 0), (3, false, 0), (4, false, 0)],
        );
        assert_eq!(keys(&debug, &mut r), [0, 3]);
    }

    /// §5.9 K4: `int r = hop(x);` (the call of rank 2, the store of
    /// rank 1): the call makes its buoy key and the store after it is a
    /// buoy of its own, key at the return address.
    #[test]
    fn a_call_ends_the_buoy() {
        let (debug, ids) = table(&[(20, 0, 0), (20, 1, 2), (20, 1, 1), (21, 0, 0), (21, 2, 1)]);
        let mut r = rows(
            &ids,
            &[(0, false, 0), (1, false, CALL), (2, false, 0), (3, false, 0), (4, false, 0)],
        );
        assert_eq!(keys(&debug, &mut r), [0, 2, 3]);
    }

    /// §5.9 K3: a rank of the same group better than the candidates'
    /// drops them; an equal one adds a candidate (in another block too).
    #[test]
    fn ranks_and_blocks() {
        let (debug, ids) = table(&[(5, 1, 2), (6, 1, 1), (7, 1, 1)]);
        let mut r = rows(&ids, &[(0, false, 0), (1, false, 0), (2, true, 0)]);
        assert_eq!(keys(&debug, &mut r), [1, 2]);
    }

    /// §5.9 K3: on one line, a second atom starts a buoy of its own.
    #[test]
    fn another_atom_on_the_line() {
        let (debug, ids) = table(&[(3, 0, 0), (3, 1, 1), (3, 0, 0), (3, 2, 1)]);
        let mut r = rows(&ids, &[(0, false, 0), (1, false, 0), (2, false, 0), (3, false, 0)]);
        assert_eq!(keys(&debug, &mut r), [0, 3]);
    }

    /// §5.9 K4: a call of a better rank drops the candidates and is not
    /// one; K6: a continuation row is no instruction.
    #[test]
    fn a_call_of_a_better_rank_and_continuations() {
        let (debug, ids) = table(&[(8, 4, 2), (9, 0, 0), (9, 4, 1), (10, 5, 1)]);
        let mut r =
            rows(&ids, &[(0, false, 0), (1, false, 0), (2, false, CALL), (3, false, ROW_CONT)]);
        // Row 1 is key by the call (its buoy); row 0's candidate is gone.
        assert_eq!(keys(&debug, &mut r), [1]);
    }
}
