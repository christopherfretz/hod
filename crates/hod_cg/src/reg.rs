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

    /// The `count` lowest registers of the set (all of it if it has
    /// fewer).
    #[must_use]
    pub const fn lowest_n(self, count: u32) -> RegSet {
        let mut rest = self.0;
        let mut out = 0;
        let mut taken = 0;
        while rest != 0 && taken < count {
            let bit = rest & rest.wrapping_neg();
            out |= bit;
            rest &= !bit;
            taken += 1;
        }
        RegSet(out)
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

/// The least number of general registers a target's pool has (§2.1). A
/// lowering pins at most this many general registers at once.
pub const MIN_POOL_GPR: u32 = 4;

/// The least number of vector registers a target's pool has (§2.1).
pub const MIN_POOL_FPR: u32 = 2;

/// A target's registers, described once (`spec/codegen.md` §2.1).
///
/// Invariants, checked by [`RegInfo::check`]: `pool` is a subset of
/// `holdable`; `holdable` and `scratch` are disjoint; every one of them is
/// inside `gpr` or `fpr`, which are disjoint; the pool has at least
/// [`MIN_POOL_GPR`] general and [`MIN_POOL_FPR`] vector registers;
/// `scratch` has at least two general and one vector register, `callee`
/// among the general ones.
#[derive(Clone, Copy, Debug)]
pub struct RegInfo {
    /// The general registers.
    pub gpr: RegSet,
    /// The floating-point/vector registers.
    pub fpr: RegSet,
    /// Registers a fresh value may be allocated to. None is written by
    /// argument placement or by a return under any supported calling
    /// convention; each is caller-saved or in the target's save set.
    pub pool: RegSet,
    /// Registers a value may rest in: `pool` plus the registers argument
    /// placement and returns write.
    pub holdable: RegSet,
    /// Registers that never hold a value across instructions: they belong
    /// to whatever is being emitted right now (glue, large offsets, `phi`
    /// copies, stack-argument placement).
    pub scratch: RegSet,
    /// The two registers a landing pad receives: the exception pointer
    /// and the selector.
    pub eh_data: [PReg; 2],
    /// The general scratch register an indirect callee's address is
    /// brought into, last, by the call sequence (§7.1). The target's exit
    /// sequences leave it alone, so a `musttail` exit can branch through
    /// it.
    pub callee: PReg,
    /// The bytes of a general register: the size and alignment of a home
    /// or spill slot of class `Gpr` (§2.2).
    pub gpr_bytes: u8,
    /// The bytes of a vector register that a home or spill slot of class
    /// `Fpr` holds.
    pub fpr_bytes: u8,
    /// Each register's DWARF number, for CFI rows (§9.3).
    pub dwarf: fn(PReg) -> u16,
}

impl RegInfo {
    /// Whether the description satisfies the invariants above.
    pub fn check(&self) -> Result<(), &'static str> {
        let all = self.gpr.union(self.fpr);
        if !self.gpr.intersection(self.fpr).is_empty() {
            return Err("gpr and fpr overlap");
        }
        if !self.pool.difference(self.holdable).is_empty() {
            return Err("pool is not a subset of holdable");
        }
        if !self.holdable.intersection(self.scratch).is_empty() {
            return Err("holdable and scratch overlap");
        }
        if !self.holdable.union(self.scratch).difference(all).is_empty() {
            return Err("a holdable or scratch register is in no class");
        }
        if self.pool.intersection(self.gpr).len() < MIN_POOL_GPR
            || self.pool.intersection(self.fpr).len() < MIN_POOL_FPR
        {
            return Err("the pool is below the minimum of four general and two vector registers");
        }
        if self.scratch.intersection(self.gpr).len() < 2
            || self.scratch.intersection(self.fpr).is_empty()
        {
            return Err("scratch needs two general and one vector register");
        }
        if !self.scratch.intersection(self.gpr).contains(self.callee) {
            return Err("the callee register is not a general scratch register");
        }
        if !self.holdable.contains(self.eh_data[0]) || !self.holdable.contains(self.eh_data[1]) {
            return Err("an eh_data register is not holdable");
        }
        if !self.gpr_bytes.is_power_of_two() || !self.fpr_bytes.is_power_of_two() {
            return Err("a register size is not a power of two");
        }
        Ok(())
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
        assert_eq!(set.with(b).lowest_n(2), RegSet::of(&[a, b]));
        assert_eq!(set.lowest_n(5), set);
        assert_eq!(set.lowest_n(0), RegSet::EMPTY);
    }

    #[test]
    #[should_panic(expected = "a register index is below 64")]
    fn an_index_of_64_panics() {
        let _ = PReg::new(64);
    }
}
