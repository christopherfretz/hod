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
use crate::label::{FixupKind, RefId};
use crate::record::VarPlace;
use crate::target::Target;

/// A branch that may take a short form (§5.6.15).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Relax {
    /// The reference of its field.
    pub(crate) reference: RefId,
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

/// How many ranges [`Edits::index_near`] steps over before it searches.
const STEPS: usize = 8;

/// The edits of one function: byte ranges removed, in offset order, with
/// the bytes removed before each. It lives in the worker's `Scratch`, so a
/// warm one allocates nothing (WP-153).
#[derive(Default, Debug)]
pub(crate) struct Edits {
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
    #[cfg(test)]
    fn map(&self, x: u32) -> u32 {
        self.moved(self.ranges.partition_point(|&(at, _)| at < x), x)
    }

    /// [`Edits::map`] for offsets that come in about ascending order:
    /// `hint` is the index the last offset found, and is moved to this
    /// one's. The answer is the same; finding it costs a few steps from
    /// the last one instead of a search of every range (WP-153).
    #[inline]
    fn map_near(&self, hint: &mut usize, x: u32) -> u32 {
        *hint = self.index_near(*hint, x);
        self.moved(*hint, x)
    }

    /// The first range that starts at or after `x`, looked for from
    /// `hint`: at most [`STEPS`] ranges one by one, then a binary search
    /// of the side that is left.
    #[inline]
    fn index_near(&self, hint: usize, x: u32) -> usize {
        let r = &self.ranges;
        let mut i = hint.min(r.len());
        if i > 0 && r[i - 1].0 >= x {
            // Backward: `r[i]` starts at or after `x` from here on.
            for _ in 0..STEPS {
                i -= 1;
                if i == 0 || r[i - 1].0 < x {
                    return i;
                }
            }
            return r[..i].partition_point(|&(at, _)| at < x);
        }
        // Forward: every range before `i` starts before `x`.
        for _ in 0..STEPS {
            if i == r.len() || r[i].0 >= x {
                return i;
            }
            i += 1;
        }
        i + r[i..].partition_point(|&(at, _)| at < x)
    }

    /// `x` moved by the ranges before index `i` (the first range that
    /// starts at or after `x`).
    #[inline]
    fn moved(&self, i: usize, x: u32) -> u32 {
        if i == 0 {
            return x;
        }
        let (at, len) = self.ranges[i - 1];
        x - self.before[i - 1] - (x - at).min(len)
    }

    /// The bytes its tables have allocated.
    pub(crate) fn allocated_bytes(&self) -> usize {
        self.ranges.capacity() * size_of::<(u32, u32)>() + self.before.capacity() * size_of::<u32>()
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
        let Some(reference) = self.s.labels.last_ref() else { return };
        let Some(r) = self.s.labels.get_ref(reference) else { return };
        let Ok(long) = u8::try_from(r.at + 4 - start) else { return };
        self.s.relax.push(Relax { reference, start, long, short, kind });
    }

    /// §5.6.15: the branches that reach, the cuts, every offset moved.
    pub(crate) fn shrink(&mut self) -> CgResult<()> {
        if self.s.cuts.is_empty() && self.s.relax.is_empty() {
            return Ok(());
        }
        let mut edits = std::mem::take(&mut self.s.edits);
        let mut chosen = std::mem::take(&mut self.s.chosen);
        chosen.clear();
        chosen.resize(self.s.relax.len(), false);
        let mut cuts = std::mem::take(&mut self.s.cuts);
        cuts.sort_unstable();
        let relax = std::mem::take(&mut self.s.relax);
        // Whether `edits` was built from the final choice.
        let mut current = false;
        for _ in 0..MAX_PASSES {
            edits.build(&cuts, &relax, &chosen);
            let mut changed = false;
            // The branches are in offset order: each start is found from
            // the last, each target from its branch.
            let mut near = 0;
            for (r, chosen) in relax.iter().zip(chosen.iter_mut()) {
                if *chosen {
                    continue;
                }
                let labels = &self.s.labels;
                let Some(target) = labels
                    .get_ref(r.reference)
                    .and_then(|reference| labels.offset(reference.label))
                else {
                    continue;
                };
                let s = i64::from(edits.map_near(&mut near, r.start));
                let mut from = near;
                let t = i64::from(edits.map_near(&mut from, target));
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
                current = true;
                break;
            }
        }
        if !current {
            edits.build(&cuts, &relax, &chosen);
        }

        // The short forms: the opcode now, the field when patched below.
        let base = self.mark.code as usize;
        for (r, _) in relax.iter().zip(&chosen).filter(|&(_, &c)| c) {
            self.out.code[base + r.start as usize] = r.short;
            self.s.labels.retarget_ref(r.reference, r.start + 1, r.kind);
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
        self.s.edits = edits;
        Ok(())
    }

    /// Every offset row of the function, moved by `edits` (§5.6.15).
    fn move_rows(&mut self, edits: &Edits) {
        // Each table's offsets ascend, or nearly: each is found from the
        // last (a table's first one searches back).
        let near = std::cell::Cell::new(0);
        let map = |x: u32| {
            let mut at = near.get();
            let moved = edits.map_near(&mut at, x);
            near.set(at);
            moved
        };
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
        self.map_debug_calls(map); // WP-113: `spec/dwarf.md` §6.19
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
        let relax =
            [Relax { reference: RefId(0), start: 20, long: 6, short: 0x74, kind: FixupKind(1) }];
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

    /// WP-153: an offset found from any hint moves as one found by a
    /// search of every range, in any order, and the hint becomes the
    /// searched index.
    #[test]
    fn a_hint_finds_what_a_search_finds() {
        let mut edits = Edits::default();
        let cuts: Vec<(u32, u32)> =
            (0..40).map(|i| (7 * i * i % 997 + 10 * i, 1 + i % 4)).collect();
        let mut cuts = cuts;
        cuts.sort_unstable();
        cuts.dedup_by_key(|c| c.0);
        let mut last = 0;
        cuts.retain(|&(at, len)| {
            let keep = at >= last;
            if keep {
                last = at + len;
            }
            keep
        });
        edits.build(&cuts, &[], &[]);
        let mut seed = 12345u32;
        let mut hint = 0;
        for _ in 0..5000 {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            let x = (seed >> 8) % 1400;
            let start = (seed as usize >> 4) % (edits.ranges.len() + 2);
            let mut from = start;
            assert_eq!(edits.map_near(&mut from, x), edits.map(x), "{x} from {start}");
            assert_eq!(from, edits.ranges.partition_point(|&(at, _)| at < x));
            assert_eq!(edits.map_near(&mut hint, x), edits.map(x));
        }
    }
}
