//! A fixed-size set of ids, one bit each (`spec/xform.md` §2).

use std::fmt;
use std::marker::PhantomData;

use hod_ir::{BlockId, InstId, SlotId};

/// An id a [`BitSet`] can hold: a dense index.
pub trait Idx: Copy {
    /// The index.
    fn index(self) -> usize;
    /// The id of an index.
    fn from_index(index: usize) -> Self;
}

macro_rules! idx {
    ($($ty:ty),*) => {$(
        impl Idx for $ty {
            #[inline]
            fn index(self) -> usize {
                <$ty>::index(self)
            }
            #[inline]
            fn from_index(index: usize) -> Self {
                <$ty>::new(index as u32)
            }
        }
    )*};
}
idx!(BlockId, InstId, SlotId);

/// A set of the ids `0..len` of one kind.
#[derive(Clone, PartialEq, Eq)]
pub struct BitSet<T> {
    words: Vec<u64>,
    len: usize,
    kind: PhantomData<T>,
}

impl<T: Idx> BitSet<T> {
    /// The empty set of ids below `len`.
    pub fn new(len: usize) -> BitSet<T> {
        BitSet { words: vec![0; len.div_ceil(64)], len, kind: PhantomData }
    }

    /// The bound: every member is below it.
    #[inline]
    pub fn domain(&self) -> usize {
        self.len
    }

    /// Whether `id` is a member; false for an id past the bound.
    #[inline]
    pub fn contains(&self, id: T) -> bool {
        let index = id.index();
        index < self.len && self.words[index / 64] >> (index % 64) & 1 != 0
    }

    /// Adds `id`; true if it was not a member.
    ///
    /// # Panics
    /// If `id` is past the bound.
    #[inline]
    pub fn insert(&mut self, id: T) -> bool {
        let index = id.index();
        assert!(index < self.len, "id {index} past the bound {}", self.len);
        let word = &mut self.words[index / 64];
        let bit = 1 << (index % 64);
        let new = *word & bit == 0;
        *word |= bit;
        new
    }

    /// Removes `id`; true if it was a member.
    #[inline]
    pub fn remove(&mut self, id: T) -> bool {
        let index = id.index();
        if index >= self.len {
            return false;
        }
        let word = &mut self.words[index / 64];
        let bit = 1 << (index % 64);
        let old = *word & bit != 0;
        *word &= !bit;
        old
    }

    /// The number of members.
    pub fn count(&self) -> usize {
        self.words.iter().map(|word| word.count_ones() as usize).sum()
    }

    /// Whether there is no member.
    pub fn is_empty(&self) -> bool {
        self.words.iter().all(|&word| word == 0)
    }

    /// The members, in increasing order.
    pub fn iter(&self) -> impl Iterator<Item = T> + '_ {
        self.words.iter().enumerate().flat_map(|(at, &word)| {
            let mut rest = word;
            std::iter::from_fn(move || {
                if rest == 0 {
                    return None;
                }
                let bit = rest.trailing_zeros() as usize;
                rest &= rest - 1;
                Some(T::from_index(at * 64 + bit))
            })
        })
    }
}

impl<T: Idx> fmt::Debug for BitSet<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.iter().map(Idx::index)).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn members() {
        let mut set: BitSet<SlotId> = BitSet::new(130);
        assert!(set.is_empty());
        assert!(set.insert(SlotId::new(0)));
        assert!(set.insert(SlotId::new(129)));
        assert!(set.insert(SlotId::new(64)));
        assert!(!set.insert(SlotId::new(64)));
        assert!(set.contains(SlotId::new(129)));
        assert!(!set.contains(SlotId::new(1)));
        assert!(!set.contains(SlotId::new(1000)));
        assert_eq!(set.count(), 3);
        let members: Vec<usize> = set.iter().map(Idx::index).collect();
        assert_eq!(members, [0, 64, 129]);
        assert!(set.remove(SlotId::new(64)));
        assert!(!set.remove(SlotId::new(64)));
        assert_eq!(format!("{set:?}"), "{0, 129}");
    }
}
