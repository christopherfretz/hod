//! A cheap hasher for maps keyed by ids the compiler assigns (`SymId`,
//! `hod_obj::SymbolId`): one multiply a word instead of `SipHash`.
//!
//! Not resistant to hash flooding, which keys the compiler numbers itself
//! do not need: the input cannot choose them. A map keyed by input bytes
//! (a section or symbol name) or by an input integer keeps the standard
//! hasher. No state is random, so a map's iteration order is the same
//! from run to run; nothing that writes output may depend on it anyway.
//!
//! `hod_ir` and `hod_dwarf` have hashers of the same family, private to
//! their crates (`hod_ir::intern::Hasher64` is not a `Hasher`,
//! `hod_dwarf`'s `FxHasher` is crate-private), hence this one.

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// The multiplier of `FxHash` (`hod_ir::intern` and `hod_dwarf` use it too).
const K: u64 = 0x517c_c1b7_2722_0a95;

/// An FxHash-style hasher over words, with a final mix.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct IdHasher(u64);

impl IdHasher {
    #[inline]
    fn add(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(K);
    }
}

impl Hasher for IdHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for chunk in &mut chunks {
            let mut word = [0u8; 8];
            word.copy_from_slice(chunk);
            self.add(u64::from_le_bytes(word));
        }
        let rest = chunks.remainder();
        if !rest.is_empty() {
            let mut word = [0u8; 8];
            word[..rest.len()].copy_from_slice(rest);
            self.add(u64::from_le_bytes(word));
        }
    }

    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add(u64::from(i));
    }

    #[inline]
    fn write_u16(&mut self, i: u16) {
        self.add(u64::from(i));
    }

    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add(u64::from(i));
    }

    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }

    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }

    /// The table takes the bucket from the low bits and a tag from the
    /// top seven, so both must depend on every bit of the key: the
    /// product alone leaves its low bits a function of the key's low
    /// bits.
    #[inline]
    fn finish(&self) -> u64 {
        let h = self.0 ^ (self.0 >> 32);
        let h = h.wrapping_mul(0x9e37_79b9_7f4a_7c15);
        h ^ (h >> 29)
    }
}

/// A `HashMap` keyed by compiler-assigned ids.
pub(crate) type IdMap<K, V> = HashMap<K, V, BuildHasherDefault<IdHasher>>;

#[cfg(test)]
mod tests {
    use std::hash::BuildHasher;

    use super::*;

    /// Equal keys hash equally, from one hasher to the next (no random
    /// state); a map behaves as a map.
    #[test]
    fn deterministic_and_a_map() {
        let build = BuildHasherDefault::<IdHasher>::default();
        assert_eq!(build.hash_one(7u32), BuildHasherDefault::<IdHasher>::default().hash_one(7u32));
        assert_ne!(build.hash_one(7u32), build.hash_one(8u32));
        let mut map: IdMap<u32, u32> = IdMap::default();
        for i in 0..10_000 {
            map.insert(i, i * 3);
        }
        assert_eq!(map.len(), 10_000);
        assert!((0..10_000).all(|i| map.get(&i) == Some(&(i * 3))));
        assert_eq!(map.get(&10_000), None);
    }

    /// Consecutive ids spread over the buckets and over the tag bits.
    #[test]
    fn consecutive_ids_spread() {
        let build = BuildHasherDefault::<IdHasher>::default();
        let mut low = [0u32; 64];
        let mut top = [0u32; 128];
        for i in 0..64 * 128u32 {
            let h = build.hash_one(i);
            low[(h & 63) as usize] += 1;
            top[(h >> 57) as usize] += 1;
        }
        assert!(low.iter().all(|&n| (64..=192).contains(&n)), "{low:?}");
        assert!(top.iter().all(|&n| (16..=112).contains(&n)), "{top:?}");
    }
}
