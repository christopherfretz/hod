//! Physical registers: `spec/codegen.md` §2.1.

/// A physical register: an index below 64 into the target's register
/// file. The numbering is the target's (`hod_a64`: `x0`..`x30` are 0..=30,
/// `sp` is 31, `v0`..`v31` are 32..=63).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct PReg(u8);

impl PReg {
    /// The register with index `index`.
    ///
    /// # Panics
    /// Panics if `index` is 64 or more.
    pub const fn new(index: u8) -> PReg {
        assert!(index < 64, "a register index is below 64");
        PReg(index)
    }

    /// The index.
    pub const fn index(self) -> u8 {
        self.0
    }
}

/// A set of physical registers.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct RegSet(u64);

impl RegSet {
    /// The empty set.
    pub const EMPTY: RegSet = RegSet(0);

    /// The set of the registers in `regs`.
    pub const fn of(regs: &[PReg]) -> RegSet {
        let mut bits = 0;
        let mut at = 0;
        while at < regs.len() {
            bits |= 1 << regs[at].0;
            at += 1;
        }
        RegSet(bits)
    }

    /// Whether `reg` is in the set.
    pub const fn contains(self, reg: PReg) -> bool {
        self.0 >> reg.0 & 1 != 0
    }

    /// The set with `reg` added.
    #[must_use]
    pub const fn with(self, reg: PReg) -> RegSet {
        RegSet(self.0 | 1 << reg.0)
    }

    /// The set with `reg` removed.
    #[must_use]
    pub const fn without(self, reg: PReg) -> RegSet {
        RegSet(self.0 & !(1 << reg.0))
    }

    /// The registers in either set.
    #[must_use]
    pub const fn union(self, other: RegSet) -> RegSet {
        RegSet(self.0 | other.0)
    }

    /// The registers in both sets.
    #[must_use]
    pub const fn intersection(self, other: RegSet) -> RegSet {
        RegSet(self.0 & other.0)
    }

    /// The registers of this set that are not in `other`.
    #[must_use]
    pub const fn difference(self, other: RegSet) -> RegSet {
        RegSet(self.0 & !other.0)
    }

    /// Whether the set is empty.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The number of registers.
    pub const fn len(self) -> u32 {
        self.0.count_ones()
    }

    /// The register with the lowest index, if any.
    pub const fn lowest(self) -> Option<PReg> {
        if self.0 == 0 { None } else { Some(PReg(self.0.trailing_zeros() as u8)) }
    }

    /// The registers, lowest index first.
    pub fn iter(self) -> impl Iterator<Item = PReg> {
        let mut bits = self.0;
        std::iter::from_fn(move || {
            let reg = RegSet(bits).lowest()?;
            bits &= bits - 1;
            Some(reg)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `spec/codegen.md` §2.1: set operations and iteration order.
    #[test]
    fn sets() {
        let (a, b, c) = (PReg::new(0), PReg::new(31), PReg::new(63));
        let set = RegSet::of(&[c, a]);
        assert!(set.contains(a) && set.contains(c) && !set.contains(b));
        assert_eq!(set.len(), 2);
        assert_eq!(set.iter().collect::<Vec<_>>(), [a, c]);
        assert_eq!(set.with(b).without(a).lowest(), Some(b));
        assert_eq!(set.union(RegSet::of(&[b])).len(), 3);
        assert_eq!(set.intersection(RegSet::of(&[a, b])), RegSet::of(&[a]));
        assert_eq!(set.difference(RegSet::of(&[a])), RegSet::of(&[c]));
        assert!(RegSet::EMPTY.is_empty() && RegSet::EMPTY.lowest().is_none());
    }

    #[test]
    #[should_panic(expected = "a register index is below 64")]
    fn an_index_of_64_panics() {
        let _ = PReg::new(64);
    }
}
