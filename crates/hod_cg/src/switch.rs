//! The `switch` plan (`spec/codegen.md` §7.3, "`switch`", WP-117): the
//! cases' canonical order and the shape of the search over them. Pure;
//! each target emits the compares and branches of the plan.

use hod_ir::BlockId;

/// A run of at most this many sorted cases is a linear chain; a longer
/// one is split at its middle case.
pub const LINEAR_CASES: usize = 4;

/// The mask of a `switch` condition of `bits` bits: case values and the
/// condition are compared zero-extended (`(1 << bits) - 1`, all ones
/// from 64 bits).
pub fn case_mask(bits: u32) -> u64 {
    if bits >= 64 { u64::MAX } else { (1u64 << bits) - 1 }
}

/// The block a `switch` of the constant condition `value` goes to: the
/// first case of that value, else `default`.
pub fn constant_target(
    mut cases: impl Iterator<Item = (u64, BlockId)>,
    value: u64,
    default: BlockId,
) -> BlockId {
    cases.find(|&(case, _)| case == value).map_or(default, |(_, block)| block)
}

/// Fills `out` (cleared first) with the cases of a `switch` the search
/// tests: those not to `default`, in increasing unsigned order, one per
/// value. Of two cases of one value (invalid IR), the first of those not
/// to `default` is kept (the sort is stable). The sort is skipped when the
/// cases come sorted, so compile time is linear for a frontend's usual
/// order and `n log n` otherwise.
pub fn canonical_cases(
    out: &mut Vec<(u64, BlockId)>,
    cases: impl Iterator<Item = (u64, BlockId)>,
    default: BlockId,
) {
    out.clear();
    out.extend(cases.filter(|&(_, block)| block != default));
    if !out.is_sorted_by_key(|&(value, _)| value) {
        out.sort_by_key(|&(value, _)| value);
    }
    out.dedup_by_key(|&mut (value, _)| value);
}

/// One step of the search over sorted, distinct cases.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step<'c> {
    /// At most [`LINEAR_CASES`] (and at least one) cases: compare with
    /// each in order, branch to its block on equality, then go to the
    /// default.
    Chain(&'c [(u64, BlockId)]),
    /// Compare with `mid`: equal goes to its block, above to the search
    /// of `upper`, else (below) the search of `lower`.
    Node {
        /// The cases below the middle one.
        lower: &'c [(u64, BlockId)],
        /// The middle case, at `len / 2`.
        mid: (u64, BlockId),
        /// The cases above the middle one.
        upper: &'c [(u64, BlockId)],
    },
}

/// The step of the search over `cases` (sorted, distinct, not empty).
pub fn search_step(cases: &[(u64, BlockId)]) -> Step<'_> {
    if cases.len() <= LINEAR_CASES {
        return Step::Chain(cases);
    }
    let mid = cases.len() / 2;
    Step::Node { lower: &cases[..mid], mid: cases[mid], upper: &cases[mid + 1..] }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(n: u32) -> BlockId {
        BlockId::new(n)
    }

    /// §7.3 "`switch`": cases to the default go, the rest are sorted
    /// unsigned and deduplicated, the first of a value winning.
    #[test]
    fn duplicate_cases_first_wins() {
        let mut out = Vec::new();
        let cases = [(7, b(1)), (3, b(2)), (7, b(3)), (u64::MAX, b(4)), (5, b(0)), (3, b(5))];
        canonical_cases(&mut out, cases.into_iter(), b(0));
        assert_eq!(out, [(3, b(2)), (7, b(1)), (u64::MAX, b(4))]);
        // A first duplicate to the default is dropped before the
        // deduplication, so the next one of that value is kept.
        canonical_cases(&mut out, [(9, b(0)), (9, b(6))].into_iter(), b(0));
        assert_eq!(out, [(9, b(6))]);
        // The constant fold takes the first of the value, default or not.
        assert_eq!(constant_target(cases.into_iter(), 7, b(0)), b(1));
        assert_eq!(constant_target([(9, b(0)), (9, b(6))].into_iter(), 9, b(0)), b(0));
        assert_eq!(constant_target(cases.into_iter(), 8, b(0)), b(0));
    }

    #[test]
    fn masks() {
        assert_eq!(case_mask(1), 1);
        assert_eq!(case_mask(32), 0xffff_ffff);
        assert_eq!(case_mask(64), u64::MAX);
    }

    /// The search shape: chains of at most four, split at `len / 2`.
    #[test]
    fn search_shape() {
        let cases: Vec<_> = (0..10).map(|v| (v, b(v as u32 + 1))).collect();
        assert_eq!(search_step(&cases[..4]), Step::Chain(&cases[..4]));
        assert_eq!(search_step(&cases[..1]), Step::Chain(&cases[..1]));
        assert_eq!(
            search_step(&cases[..5]),
            Step::Node { lower: &cases[..2], mid: cases[2], upper: &cases[3..5] }
        );
        assert_eq!(
            search_step(&cases),
            Step::Node { lower: &cases[..5], mid: cases[5], upper: &cases[6..] }
        );
    }
}
