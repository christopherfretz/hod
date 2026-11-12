//! Hash-consing support: a deterministic hasher and an id-only hash table.
//!
//! The tables of `spec/ir.md` §3 and §4 keep their entries in arenas and
//! variable-length parts in shared pools, so an entry cannot be a
//! `HashMap` key without storing it twice. [`InternTable`] stores only
//! `(hash, id)` pairs; the owner supplies the equality test against its
//! arena. Nothing here depends on addresses or random state, so ids are
//! assigned deterministically.

use crate::error::{IrErrorKind, IrResult, err};

/// An FxHash-style multiplicative hasher over 64-bit words.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Hasher64 {
    state: u64,
}

impl Hasher64 {
    const K: u64 = 0x517c_c1b7_2722_0a95;

    /// A hasher seeded with `tag` (the entry kind).
    #[inline]
    pub(crate) fn new(tag: u64) -> Self {
        let mut h = Self { state: 0 };
        h.word(tag);
        h
    }

    /// Mixes one word in.
    #[inline]
    pub(crate) fn word(&mut self, w: u64) {
        self.state = (self.state.rotate_left(5) ^ w).wrapping_mul(Self::K);
    }

    /// Mixes two 32-bit values in.
    #[inline]
    pub(crate) fn pair(&mut self, a: u32, b: u32) {
        self.word(u64::from(a) | (u64::from(b) << 32));
    }

    /// Mixes a byte string in, length included.
    pub(crate) fn bytes(&mut self, bytes: &[u8]) {
        self.word(bytes.len() as u64);
        let mut chunks = bytes.chunks_exact(8);
        for chunk in &mut chunks {
            let mut w = [0u8; 8];
            w.copy_from_slice(chunk);
            self.word(u64::from_le_bytes(w));
        }
        let rest = chunks.remainder();
        if !rest.is_empty() {
            let mut w = [0u8; 8];
            w[..rest.len()].copy_from_slice(rest);
            self.word(u64::from_le_bytes(w));
        }
    }

    /// The 32-bit hash. The multiplication pushes entropy upward, so the
    /// high half is the well-mixed one.
    #[inline]
    pub(crate) fn finish(self) -> u32 {
        (self.state >> 32) as u32
    }
}

/// One slot: the entry's hash and id. `id == EMPTY` marks a free slot.
#[derive(Clone, Copy, Debug)]
struct Slot {
    hash: u32,
    id: u32,
}

const EMPTY: u32 = u32::MAX;

/// The largest number of entries a table with `u32` ids can hold
/// (`u32::MAX` itself is the free-slot marker).
pub(crate) const MAX_ENTRIES: usize = (u32::MAX - 1) as usize;

/// The id for the next entry of an arena holding `len` entries.
pub(crate) fn next_id(len: usize, what: &str) -> IrResult<u32> {
    if len >= MAX_ENTRIES {
        return err(IrErrorKind::TableFull, format!("the {what} table is full"));
    }
    Ok(len as u32)
}

/// The start index for `extra` more elements in a pool holding `len`:
/// pool positions are `u32`.
pub(crate) fn pool_start(len: usize, extra: usize, what: &str) -> IrResult<u32> {
    match len.checked_add(extra) {
        Some(end) if end <= MAX_ENTRIES => Ok(len as u32),
        _ => err(IrErrorKind::TableFull, format!("the {what} pool is full")),
    }
}

/// An open-addressing hash set of ids, keyed by caller-supplied hashes.
#[derive(Clone, Debug)]
pub(crate) struct InternTable {
    slots: Vec<Slot>,
    len: usize,
}

impl Default for InternTable {
    fn default() -> Self {
        Self::new()
    }
}

impl InternTable {
    const INITIAL: usize = 64;

    pub(crate) fn new() -> Self {
        Self { slots: vec![Slot { hash: 0, id: EMPTY }; Self::INITIAL], len: 0 }
    }

    /// The id of an entry with `hash` for which `eq` holds.
    #[inline]
    pub(crate) fn find(&self, hash: u32, mut eq: impl FnMut(u32) -> bool) -> Option<u32> {
        let mask = self.slots.len() - 1;
        let mut at = hash as usize & mask;
        loop {
            let slot = self.slots[at];
            if slot.id == EMPTY {
                return None;
            }
            if slot.hash == hash && eq(slot.id) {
                return Some(slot.id);
            }
            at = (at + 1) & mask;
        }
    }

    /// Adds `id` under `hash`. The caller has checked with [`Self::find`]
    /// that no equal entry is present.
    pub(crate) fn insert(&mut self, hash: u32, id: u32) {
        debug_assert_ne!(id, EMPTY);
        // Grow at 3/4 load.
        if (self.len + 1) * 4 > self.slots.len() * 3 {
            let old = std::mem::replace(&mut self.slots, vec![Slot { hash: 0, id: EMPTY }; 0]);
            self.slots = vec![Slot { hash: 0, id: EMPTY }; old.len() * 2];
            for slot in old {
                if slot.id != EMPTY {
                    self.place(slot);
                }
            }
        }
        self.place(Slot { hash, id });
        self.len += 1;
    }

    fn place(&mut self, slot: Slot) {
        let mask = self.slots.len() - 1;
        let mut at = slot.hash as usize & mask;
        while self.slots[at].id != EMPTY {
            at = (at + 1) & mask;
        }
        self.slots[at] = slot;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §1: interning finds what was inserted, across growth and collisions.
    #[test]
    fn table_finds_inserted_ids_across_growth() {
        let mut table = InternTable::new();
        let keys: Vec<u64> = (0..5000).map(|i| i * 7919).collect();
        for (id, key) in keys.iter().enumerate() {
            // A deliberately poor hash (few distinct values) to force probing.
            let hash = (*key % 97) as u32;
            assert_eq!(table.find(hash, |other| keys[other as usize] == *key), None);
            table.insert(hash, id as u32);
        }
        for (id, key) in keys.iter().enumerate() {
            let hash = (*key % 97) as u32;
            assert_eq!(table.find(hash, |other| keys[other as usize] == *key), Some(id as u32));
        }
        assert_eq!(table.find(5, |_| false), None);
    }

    /// The hasher is a pure function of its input (principle 5: deterministic).
    #[test]
    fn hasher_is_deterministic_and_length_sensitive() {
        let hash = |bytes: &[u8]| {
            let mut h = Hasher64::new(1);
            h.bytes(bytes);
            h.finish()
        };
        assert_eq!(hash(b"abc"), hash(b"abc"));
        assert_ne!(hash(b"abc"), hash(b"abd"));
        assert_ne!(hash(b"abc\0"), hash(b"abc"));
        assert_ne!(hash(b"12345678"), hash(b"123456789"));
        let mut a = Hasher64::new(1);
        a.pair(1, 2);
        let mut b = Hasher64::new(1);
        b.pair(2, 1);
        assert_ne!(a.finish(), b.finish());
    }

    /// §1 `TableFull`: ids and pool positions stop at the `u32` limit.
    #[test]
    fn id_space_exhaustion_is_an_error() {
        assert_eq!(next_id(10, "type").unwrap(), 10);
        assert_eq!(next_id(MAX_ENTRIES, "type").unwrap_err().kind, IrErrorKind::TableFull);
        assert_eq!(pool_start(10, 5, "field").unwrap(), 10);
        assert_eq!(
            pool_start(MAX_ENTRIES - 1, 2, "field").unwrap_err().kind,
            IrErrorKind::TableFull
        );
        assert_eq!(pool_start(usize::MAX, 2, "field").unwrap_err().kind, IrErrorKind::TableFull);
    }
}
