//! Shrinking a function's code once it is complete (`spec/codegen.md`
//! §5.6.15): byte ranges the target cuts out (the unused part of a prologue
//! emitted at its largest), and branches relaxed to a short form when
//! the final layout puts their label in reach. Every code offset the
//! function's labels, references and rows hold moves with the bytes,
//! and every reference to a bound label is patched again.
//!
//! It runs after the exit sequences, the coroutine stubs and the trap,
//! before the data islands, so an island is aligned in the final layout.
//! A function that asked for neither costs nothing.

use crate::cx::FnCx;
use crate::error::{CgResult, internal};
use crate::label::FixupKind;
use crate::record::VarPlace;
use crate::target::Target;

/// A branch that may take a short form (§5.6.15).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Relax {
    /// The reference (`Labels::refs`) of its field.
    pub(crate) reference: u32,
    /// The offset of its first byte.
    pub(crate) start: u32,
    /// Its length in the long form: it ends with its field.
    pub(crate) long: u8,
    /// The opcode of the short form `[short, rel8]`.
    pub(crate) short: u8,
    /// The fixup kind of the short form's one-byte field.
    pub(crate) kind: FixupKind,
}

/// The length of a relaxed branch: an opcode and a `rel8`.
const SHORT: u32 = 2;

/// At most this many passes look for branches that reach (§5.6.15): each
/// pass only adds, and any set found is valid.
const MAX_PASSES: u32 = 8;

/// The edits of one function: byte ranges removed, in offset order, with
/// the bytes removed before each.
#[derive(Default)]
struct Edits {
    /// `(at, len)`, ascending, disjoint.
    ranges: Vec<(u32, u32)>,
    /// `before[i]`: the bytes the ranges before `ranges[i]` remove.
    before: Vec<u32>,
}

impl Edits {
    fn build(&mut self, cuts: &[(u32, u32)], relax: &[Relax], chosen: &[bool]) {
        self.ranges.clear();
        self.before.clear();
        let mut relaxed = relax
            .iter()
            .zip(chosen)
            .filter(|&(_, &c)| c)
            .map(|(r, _)| (r.start + SHORT, u32::from(r.long) - SHORT))
            .peekable();
        let mut cut = cuts.iter().copied().peekable();
        loop {
            let next = match (cut.peek(), relaxed.peek()) {
                (Some(&a), Some(&b)) if a.0 <= b.0 => cut.next(),
                (Some(_), None) => cut.next(),
                (_, Some(_)) => relaxed.next(),
                (None, None) => break,
            };
            if let Some(range) = next {
                self.ranges.push(range);
            }
        }
        let mut total = 0;
        for &(_, len) in &self.ranges {
            self.before.push(total);
            total += len;
        }
    }

    /// Where offset `x` of the code before the edits is after them; an
    /// offset inside a removed range goes to its start.
    fn map(&self, x: u32) -> u32 {
        let i = self.ranges.partition_point(|&(at, _)| at < x);
        if i == 0 {
            return x;
        }
        let (at, len) = self.ranges[i - 1];
        x - self.before[i - 1] - (x - at).min(len)
    }
}

impl<T: Target> FnCx<'_, T> {
    /// Removes the `len` bytes at offset `at` once the function's code is
    /// complete (§5.6.15): nothing may refer to them, and no row or label may
    /// be inside them. Cuts are made in offset order and do not overlap.
    #[inline]
    pub fn cut(&mut self, at: u32, len: u32) {
        if len != 0 {
            self.s.cuts.push((at, len));
        }
    }

    /// The reference [`FnCx::fixup`] just recorded is the field of a
    /// branch that starts at `start` and ends with the field: once the
    /// code is complete, if the label is bound in reach of one signed
    /// byte, the branch becomes `[short, rel8]` with a field of `kind`
    /// (§5.6.15). Call it right after that `fixup`.
    #[inline]
    pub fn relaxable(&mut self, start: u32, short: u8, kind: FixupKind) {
        let reference = self.s.labels.ref_count().wrapping_sub(1);
        let Some(r) = self.s.labels.refs().last() else { return };
        let Ok(long) = u8::try_from(r.at + 4 - start) else { return };
        self.s.relax.push(Relax { reference, start, long, short, kind });
    }

    /// §5.6.15: the branches that reach, the cuts, every offset moved.
    pub(crate) fn shrink(&mut self) -> CgResult<()> {
        if self.s.cuts.is_empty() && self.s.relax.is_empty() {
            return Ok(());
        }
        let mut edits = Edits::default();
        let mut chosen = std::mem::take(&mut self.s.chosen);
        chosen.clear();
        chosen.resize(self.s.relax.len(), false);
        let mut cuts = std::mem::take(&mut self.s.cuts);
        cuts.sort_unstable();
        let relax = std::mem::take(&mut self.s.relax);
        for _ in 0..MAX_PASSES {
            edits.build(&cuts, &relax, &chosen);
            let mut changed = false;
            for (r, chosen) in relax.iter().zip(chosen.iter_mut()) {
                if *chosen {
                    continue;
                }
                let refs = self.s.labels.refs();
                let Some(target) = refs
                    .get(r.reference as usize)
                    .and_then(|reference| self.s.labels.offset(reference.label))
                else {
                    continue;
                };
                let (s, t) = (i64::from(edits.map(r.start)), i64::from(edits.map(target)));
                // Forward: the target moves by what the branch itself
                // loses, so the distance from the end of either form is
                // the same. Backward: counted from the short form's end.
                let disp = if target > r.start { t - s - i64::from(r.long) } else { t - s - 2 };
                if i8::try_from(disp).is_ok() {
                    *chosen = true;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        edits.build(&cuts, &relax, &chosen);

        // The short forms: the opcode now, the field when patched below.
        let base = self.mark.code as usize;
        for (r, _) in relax.iter().zip(&chosen).filter(|&(_, &c)| c) {
            self.out.code[base + r.start as usize] = r.short;
            self.s.labels.retarget_ref(r.reference as usize, r.start + 1, r.kind);
        }
        // The bytes.
        let mut to = base;
        let mut from = base;
        for &(at, len) in &edits.ranges {
            let end = base + at as usize;
            if end < from {
                return internal("overlapping code edits");
            }
            self.out.code.copy_within(from..end, to);
            to += end - from;
            from = end + len as usize;
        }
        let end = self.out.code.len();
        self.out.code.copy_within(from..end, to);
        self.out.code.truncate(to + (end - from));

        self.move_rows(&edits);

        // Every reference to a bound label, patched again.
        let code = &mut self.out.code[base..];
        for r in self.s.labels.refs() {
            let Some(target) = self.s.labels.offset(r.label) else { continue };
            if !T::patch_branch(code, r.at, r.kind, i64::from(target) - i64::from(r.at)) {
                return internal("a reference out of reach after shrinking the code");
            }
        }
        cuts.clear();
        self.s.cuts = cuts;
        self.s.chosen = chosen;
        let mut relax = relax;
        relax.clear();
        self.s.relax = relax;
        Ok(())
    }

    /// Every offset row of the function, moved by `edits` (§5.6.15).
    fn move_rows(&mut self, edits: &Edits) {
        let map = |x: u32| edits.map(x);
        self.s.labels.map_offsets(map);
        let m = self.mark;
        for row in &mut self.out.relocs[m.relocs as usize..] {
            row.offset = map(row.offset);
        }
        for row in &mut self.out.cfi[m.cfi as usize..] {
            row.offset = map(row.offset);
        }
        for row in &mut self.out.call_sites[m.call_sites as usize..] {
            let end = map(row.start + row.len);
            row.start = map(row.start);
            row.len = end - row.start;
            if row.landing != 0 {
                row.landing = map(row.landing);
            }
        }
        for row in &mut self.out.pads[m.pads as usize..] {
            row.offset = map(row.offset);
        }
        for row in &mut self.out.lines[m.lines as usize..] {
            row.offset = map(row.offset);
        }
        for row in &mut self.out.vars[m.vars as usize..] {
            if let VarPlace::Label(offset) = &mut row.place {
                *offset = map(*offset);
            }
        }
        for row in &mut self.out.islands[m.islands as usize..] {
            row.offset = map(row.offset);
        }
        for row in &mut self.out.block_offsets[m.block_offsets as usize..] {
            row.offset = map(row.offset);
        }
        self.map_coro_offsets(map);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §5.6.15: an offset moves by the bytes removed before it; one inside
    /// a removed range goes to the range's start.
    #[test]
    fn offsets_move_by_what_is_removed_before_them() {
        let mut edits = Edits::default();
        let relax = [Relax { reference: 0, start: 20, long: 6, short: 0x74, kind: FixupKind(1) }];
        edits.build(&[(4, 3), (30, 2)], &relax, &[true]);
        assert_eq!(edits.ranges, [(4, 3), (22, 4), (30, 2)]);
        let cases = [
            (0, 0),
            (4, 4),
            (5, 4),
            (7, 4),
            (10, 7),
            (22, 19),
            (24, 19),
            (26, 19),
            (30, 23),
            (40, 31),
        ];
        for (x, y) in cases {
            assert_eq!(edits.map(x), y, "{x}");
        }
        edits.build(&[(4, 3)], &relax, &[false]);
        assert_eq!(edits.ranges, [(4, 3)]);
        assert_eq!(edits.map(26), 23);
    }
}
