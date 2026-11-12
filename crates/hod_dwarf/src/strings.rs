//! `.debug_str` and `.debug_str_offsets` (`spec/dwarf.md` §6.3).

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::hash::{BuildHasherDefault, Hasher};

use crate::Error;

/// A fast hash for byte strings: eight bytes per multiply. Not
/// DoS-resistant, which a compiler's own output does not need.
#[derive(Default, Clone, Copy, Debug)]
pub(crate) struct FxHasher(u64);

const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

impl FxHasher {
    #[inline]
    fn add(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(SEED);
    }
}

impl Hasher for FxHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for c in &mut chunks {
            self.add(u64::from_le_bytes(c.try_into().unwrap_or([0; 8])));
        }
        let rest = chunks.remainder();
        if !rest.is_empty() {
            let mut last = [0u8; 8];
            last[..rest.len()].copy_from_slice(rest);
            self.add(u64::from_le_bytes(last));
        }
    }

    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add(u64::from(i));
    }

    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }

    /// A last mix: the product's high bits are its best mixed, and the
    /// table takes the bucket from the low ones and a tag from the top
    /// seven, so both must depend on every input bit. (Without it,
    /// strings that differ only in the high bytes of their last word,
    /// `v12345` and `v12346`, share a bucket.)
    #[inline]
    fn finish(&self) -> u64 {
        let h = self.0 ^ (self.0 >> 32);
        let h = h.wrapping_mul(0x9e37_79b9_7f4a_7c15);
        h ^ (h >> 29)
    }
}

pub(crate) type FxBuild = BuildHasherDefault<FxHasher>;

#[derive(Clone, Copy, Debug)]
struct Slot {
    /// The offset in `.debug_str`.
    offset: u32,
    /// The unit that last named the string, and its index there.
    unit: u32,
    index: u32,
}

/// The strings of an object's DIEs: `.debug_str` pooled across units
/// (each distinct string once), and per unit the `.debug_str_offsets`
/// entries in order of first use (§6.3).
///
/// One hash table keyed by the bytes serves both: a string's entry
/// remembers the last unit that named it and its index there, so a
/// lookup is one probe whether the string is new to the object, new to
/// the unit, or neither.
#[derive(Debug)]
pub struct DebugStrings<'a> {
    data: Vec<u8>,
    map: HashMap<&'a [u8], Slot, FxBuild>,
    unit: u32,
    offsets: Vec<u32>,
}

impl Default for DebugStrings<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> DebugStrings<'a> {
    /// An empty pool, in its first unit.
    pub fn new() -> Self {
        Self { data: Vec::new(), map: HashMap::default(), unit: 1, offsets: Vec::new() }
    }

    /// Starts the next unit: indices start again from 0.
    pub fn begin_unit(&mut self) {
        self.unit += 1;
        self.offsets.clear();
    }

    /// The `DW_FORM_strx` index of `s` in the current unit, adding it to
    /// the unit's offsets and to `.debug_str` when it is new.
    ///
    /// # Errors
    /// `NulInName` if `s` contains a NUL; `TooLarge` if `.debug_str`
    /// would reach 4 GiB.
    #[inline]
    pub fn index(&mut self, s: &'a [u8]) -> Result<u32, Error> {
        let next = self.offsets.len() as u32;
        match self.map.entry(s) {
            Entry::Occupied(mut o) => {
                let e = o.get_mut();
                if e.unit != self.unit {
                    e.unit = self.unit;
                    e.index = next;
                    self.offsets.push(e.offset);
                }
                Ok(e.index)
            }
            Entry::Vacant(v) => {
                if s.contains(&0) {
                    return Err(Error::NulInName);
                }
                let offset = u32::try_from(self.data.len()).map_err(|_| Error::TooLarge)?;
                if self.data.len() + s.len() >= u32::MAX as usize {
                    return Err(Error::TooLarge);
                }
                self.data.extend_from_slice(s);
                self.data.push(0);
                self.offsets.push(offset);
                v.insert(Slot { offset, unit: self.unit, index: next });
                Ok(next)
            }
        }
    }

    /// The current unit's `.debug_str` offsets, by index.
    pub fn unit_offsets(&self) -> &[u32] {
        &self.offsets
    }

    /// `.debug_str` so far.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub(crate) fn into_data(self) -> Vec<u8> {
        self.data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The hash spreads names that differ in a few digits over the
    /// buckets (by the low bits) and the tags (the top seven) as a random
    /// function would: about `1 - 1/e` of `n` buckets used by `n` keys.
    #[test]
    fn hash_spreads() {
        use std::hash::BuildHasher;
        let n = 1usize << 16;
        for shape in ["v{}", "_ZN4top13mid24low1v{}E", "{}x", "a_rather_long_prefix_{}_and_suffix"]
        {
            let keys: Vec<String> = (0..n).map(|i| shape.replace("{}", &i.to_string())).collect();
            let mut buckets = vec![false; n];
            let mut tags = [0usize; 128];
            for k in &keys {
                let h = FxBuild::default().hash_one(k.as_bytes());
                buckets[(h as usize) & (n - 1)] = true;
                tags[(h >> 57) as usize] += 1;
            }
            let used = buckets.iter().filter(|&&b| b).count();
            eprintln!(
                "{shape}: {used} of {n} buckets, tags {}..{}",
                tags.iter().min().unwrap(),
                tags.iter().max().unwrap()
            );
            assert!(used > n * 6 / 10, "{shape}: {used} of {n} buckets");
            assert!(*tags.iter().max().unwrap() < 2 * n / 128, "{shape}: tags");
        }
    }

    /// §6.3: one copy per object, indices per unit in order of first use.
    #[test]
    fn pooling() {
        let mut p = DebugStrings::new();
        assert_eq!(p.index(b"int").unwrap(), 0);
        assert_eq!(p.index(b"x").unwrap(), 1);
        assert_eq!(p.index(b"int").unwrap(), 0);
        assert_eq!(p.unit_offsets(), [0, 4]);
        p.begin_unit();
        assert_eq!(p.index(b"x").unwrap(), 0);
        assert_eq!(p.index(b"y").unwrap(), 1);
        assert_eq!(p.index(b"int").unwrap(), 2);
        assert_eq!(p.unit_offsets(), [4, 6, 0]);
        assert_eq!(p.data(), b"int\0x\0y\0");
        assert_eq!(p.index(b"a\0b"), Err(Error::NulInName));
        // A long string and the empty one.
        let long = vec![b'q'; 10_000];
        let long: &'static [u8] = Box::leak(long.into_boxed_slice());
        assert_eq!(p.index(long).unwrap(), 3);
        assert_eq!(p.index(b"").unwrap(), 4);
        assert_eq!(p.data().len(), 8 + 10_001 + 1);
    }
}
