//! `.debug_str` and `.debug_str_offsets` (`spec/dwarf.md` §6.3).

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::hash::{BuildHasherDefault, Hasher};

use hod_ir::{DebugInfo, DiStr};

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

/// Appends `s` and its NUL to `.debug_str`; its offset there.
fn add(data: &mut Vec<u8>, s: &[u8]) -> Result<u32, Error> {
    if s.contains(&0) {
        return Err(Error::NulInName);
    }
    let offset = u32::try_from(data.len()).map_err(|_| Error::TooLarge)?;
    if data.len() + s.len() >= u32::MAX as usize {
        return Err(Error::TooLarge);
    }
    data.extend_from_slice(s);
    data.push(0);
    Ok(offset)
}

#[derive(Clone, Copy, Debug, Default)]
struct Slot {
    /// The offset in `.debug_str`.
    offset: u32,
    /// The unit that last named the string (0: none yet), and its index
    /// there.
    unit: u32,
    index: u32,
}

/// The strings of an object's DIEs: `.debug_str` pooled across units
/// (each distinct string once), and per unit the `.debug_str_offsets`
/// entries in order of first use (§6.3).
///
/// A string's entry remembers the last unit that named it and its index
/// there, so a lookup is one probe whether the string is new to the
/// object, new to the unit, or neither. The entries of the strings of the
/// debug store ([`DebugStrings::use_store`]) are a table indexed by the
/// store's string id ([`DiStr`]), so a string read from a node is found
/// without hashing it; the other strings (made up by the writer) are a
/// hash table keyed by the bytes. A string is in one of the two, never
/// both: a string asked for by its bytes is first looked for in the store
/// (one hash), and if the store holds it, it is the store's entry
/// (WP-318). So both ways of asking give one index per string.
#[derive(Debug)]
pub struct DebugStrings<'a> {
    data: Vec<u8>,
    /// The strings the store does not hold.
    map: HashMap<&'a [u8], Slot, FxBuild>,
    unit: u32,
    offsets: Vec<u32>,
    /// The debug store whose strings are keyed by id, and whether the
    /// current unit reads that store.
    store: Option<&'a DebugInfo>,
    same: bool,
    /// By the store's string id; empty until the first id is asked for.
    by_id: Vec<Slot>,
}

impl Default for DebugStrings<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> DebugStrings<'a> {
    /// An empty pool, in its first unit.
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
            map: HashMap::default(),
            unit: 1,
            offsets: Vec::new(),
            store: None,
            same: false,
            by_id: Vec::new(),
        }
    }

    /// Starts the next unit: indices start again from 0.
    pub fn begin_unit(&mut self) {
        self.unit += 1;
        self.offsets.clear();
    }

    /// The current unit reads `store`: [`DebugStrings::index_str`] takes
    /// its strings by id. The first store given is the pool's; a unit of
    /// another store is served by the bytes of its strings.
    pub fn use_store(&mut self, store: &'a DebugInfo) {
        let pool = *self.store.get_or_insert(store);
        self.same = std::ptr::eq(pool, store);
    }

    /// The `DW_FORM_strx` index of `s` in the current unit, adding it to
    /// the unit's offsets and to `.debug_str` when it is new.
    ///
    /// # Errors
    /// `NulInName` if `s` contains a NUL; `TooLarge` if `.debug_str`
    /// would reach 4 GiB.
    #[inline]
    pub fn index(&mut self, s: &'a [u8]) -> Result<u32, Error> {
        if let Some(found) = self.store.and_then(|store| store.find_string(s)) {
            return self.by_store(found);
        }
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
                let offset = add(&mut self.data, s)?;
                self.offsets.push(offset);
                v.insert(Slot { offset, unit: self.unit, index: next });
                Ok(next)
            }
        }
    }

    /// [`DebugStrings::index`] of a string read from a node of the
    /// current unit's store: by its id, without hashing it, when that
    /// store is the pool's.
    ///
    /// # Errors
    /// As [`DebugStrings::index`].
    #[inline]
    pub fn index_str(&mut self, s: DiStr<'a>) -> Result<u32, Error> {
        if self.same && s.id().is_some() { self.by_store(s) } else { self.index(s.text()) }
    }

    /// The index of `s`, a string of the pool's store.
    #[inline]
    fn by_store(&mut self, s: DiStr<'a>) -> Result<u32, Error> {
        if self.by_id.is_empty() {
            let count = self.store.map_or(0, DebugInfo::string_count);
            self.by_id = vec![Slot::default(); count];
        }
        let next = self.offsets.len() as u32;
        let Some(e) = s.id().and_then(|id| self.by_id.get_mut(id as usize)) else {
            unreachable!("a string of the pool's store has an id below its count");
        };
        if e.unit != self.unit {
            if e.unit == 0 {
                e.offset = add(&mut self.data, s.text())?;
            }
            e.unit = self.unit;
            e.index = next;
            self.offsets.push(e.offset);
        }
        Ok(e.index)
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
    use hod_ir::{DiFieldValue, DiId, DiKind, DiNamespace, DiNode};

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

    /// A store with namespaces named `names`, and each name's string read
    /// with its id.
    fn store(names: &[&'static [u8]]) -> (DebugInfo, Vec<DiId>) {
        let mut d = DebugInfo::default();
        let ids = names
            .iter()
            .map(|&name| {
                let mut n = DiNode::new(DiKind::Namespace);
                n.set(b"name", DiFieldValue::Str(name)).unwrap();
                d.add(&n).unwrap()
            })
            .collect();
        (d, ids)
    }

    fn name(d: &DebugInfo, n: DiId) -> DiStr<'_> {
        d.view::<DiNamespace>(n).unwrap().ids().name()
    }

    /// WP-318: a string of the store asked for by its id or by its bytes
    /// gets one index, whichever way comes first, in every unit; strings
    /// the store does not hold are pooled by their bytes alone; the
    /// indices and `.debug_str` are what the pool by bytes alone gives.
    #[test]
    #[allow(clippy::many_single_char_names, reason = "two pools and two stores, side by side")]
    fn by_id_and_by_bytes_agree() {
        let (d, n) = store(&[b"int", b"x", b"y", b"a\0b"]);
        let mut p = DebugStrings::new();
        p.use_store(&d);
        let mut q = DebugStrings::new();
        assert_eq!(p.index_str(name(&d, n[0])).unwrap(), 0);
        assert_eq!(q.index(b"int").unwrap(), 0);
        assert_eq!(p.index(b"int").unwrap(), 0);
        assert_eq!(p.index(b"made up").unwrap(), 1);
        assert_eq!(q.index(b"made up").unwrap(), 1);
        assert_eq!(p.index(b"x").unwrap(), 2);
        assert_eq!(q.index(b"x").unwrap(), 2);
        assert_eq!(p.index_str(name(&d, n[1])).unwrap(), 2);
        assert_eq!(p.index_str(DiStr::EMPTY).unwrap(), 3);
        assert_eq!(q.index(b"").unwrap(), 3);
        assert_eq!(p.unit_offsets(), q.unit_offsets());
        p.begin_unit();
        q.begin_unit();
        p.use_store(&d);
        assert_eq!(p.index_str(name(&d, n[2])).unwrap(), 0);
        assert_eq!(q.index(b"y").unwrap(), 0);
        assert_eq!(p.index(b"x").unwrap(), 1);
        assert_eq!(q.index(b"x").unwrap(), 1);
        assert_eq!(p.index_str(name(&d, n[0])).unwrap(), 2);
        assert_eq!(q.index(b"int").unwrap(), 2);
        assert_eq!(p.unit_offsets(), q.unit_offsets());
        assert_eq!(p.data(), q.data());
        assert_eq!(p.index_str(name(&d, n[3])), Err(Error::NulInName));
        // A unit of another store is served by its strings' bytes.
        let (other, m) = store(&[b"z", b"int"]);
        p.begin_unit();
        p.use_store(&other);
        assert_eq!(p.index_str(name(&other, m[0])).unwrap(), 0);
        assert_eq!(p.index_str(name(&other, m[1])).unwrap(), 1);
        assert_eq!(p.index(b"int").unwrap(), 1);
        assert_eq!(p.data(), b"int\0made up\0x\0\0y\0z\0");
    }
}
