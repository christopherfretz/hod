//! A cheap hasher for maps keyed by ids the compiler assigns (`SymId`,
//! `hod_obj::SymbolId`): one multiply a word instead of `SipHash`.
//!
//! Not resistant to hash flooding, which keys the compiler numbers itself
//! do not need: the input cannot choose them. A map keyed by input bytes
//! or by an input integer keeps the standard hasher, with one exception
//! (WP-321): the section names of `emit::section`, where an input built
//! to collide could only cost time, never change the object, and
//! `SipHash` cost about 1% of a large `-g` input. No state is random, so a
//! map's iteration order is the same from run to run; nothing that writes
//! output may depend on it anyway.
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

    /// WP-321: section names (`.text.<mangled>`, long common prefixes,
    /// differing in a few bytes late in the name) do not collapse: 10,000
    /// of them give at least 9,990 distinct hashes, and the low 16 bits
    /// (the buckets) and the top 7 (the tags) are spread.
    #[test]
    fn section_names_spread() {
        use std::collections::HashSet;
        let build = BuildHasherDefault::<IdHasher>::default();
        let mut hashes = HashSet::new();
        let mut low = [0u32; 256];
        let mut low16 = HashSet::new();
        let mut top = [0u32; 128];
        for i in 0..10_000u32 {
            let name = format!(
                ".text._ZNSt3__16vectorIN12_GLOBAL__N_14ItemENS_9allocatorIS2_EEE{}E{}Ev",
                i % 100,
                i / 100
            );
            let h = build.hash_one(name.as_bytes());
            hashes.insert(h);
            low[(h & 255) as usize] += 1;
            low16.insert(h & 0xffff);
            top[(h >> 57) as usize] += 1;
        }
        assert!(hashes.len() >= 9_990, "{} distinct", hashes.len());
        // 10,000 keys over 65,536 values: about 9,300 distinct when spread.
        assert!(low16.len() >= 9_000, "{} distinct low 16 bits", low16.len());
        // About 39 per byte value, 78 per tag.
        assert!(low.iter().all(|&n| (10..=80).contains(&n)), "{low:?}");
        assert!(top.iter().all(|&n| (30..=140).contains(&n)), "{top:?}");
    }
}
