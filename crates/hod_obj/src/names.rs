//! Interning of section names. Contract: `spec/object.md` "Section names".
//!
//! Section names repeat (`.text` once per function under
//! `-ffunction-sections` with COMDAT, `.group` aside), so they are stored
//! once. The table is one byte arena, one span per distinct name and an
//! open-addressing index; interning allocates only when one of those three
//! vectors grows. The hash is fixed, so behavior does not vary between runs.

/// One distinct name: where its bytes are, and their hash.
#[derive(Clone, Copy, Debug)]
struct Span {
    start: u32,
    len: u32,
    hash: u32,
}

/// Distinct byte strings, numbered from zero in first-interned order.
#[derive(Debug, Default)]
pub(crate) struct NameTable {
    bytes: Vec<u8>,
    spans: Vec<Span>,
    /// Open addressing, linear probing. `0` is empty, else span index + 1.
    /// The length is zero or a power of two, and at most half is occupied.
    slots: Vec<u32>,
}

impl NameTable {
    /// The number of `name`, adding it if it is new. `None` when the table
    /// cannot address another name (4 GiB of bytes, or 2^32 - 1 names).
    pub(crate) fn intern(&mut self, name: &[u8]) -> Option<u32> {
        let hash = hash(name);
        if let Some(found) = self.find(name, hash) {
            return Some(found);
        }
        let start = u32::try_from(self.bytes.len()).ok()?;
        let len = u32::try_from(name.len()).ok()?;
        start.checked_add(len)?;
        let id = u32::try_from(self.spans.len()).ok().filter(|&id| id != u32::MAX)?;
        self.bytes.extend_from_slice(name);
        self.spans.push(Span { start, len, hash });
        if self.spans.len() * 2 > self.slots.len() {
            self.grow();
        } else {
            self.place(hash, id);
        }
        Some(id)
    }

    /// The bytes of name `id`.
    ///
    /// # Panics
    /// Panics if `id` was not returned by [`Self::intern`] on this table.
    pub(crate) fn get(&self, id: u32) -> &[u8] {
        let span = self.spans[id as usize];
        &self.bytes[span.start as usize..(span.start + span.len) as usize]
    }

    /// How many distinct names there are.
    pub(crate) fn len(&self) -> usize {
        self.spans.len()
    }

    /// All names, in id order.
    pub(crate) fn iter(&self) -> impl Iterator<Item = &[u8]> {
        self.spans.iter().map(|s| &self.bytes[s.start as usize..(s.start + s.len) as usize])
    }

    fn find(&self, name: &[u8], hash: u32) -> Option<u32> {
        if self.slots.is_empty() {
            return None;
        }
        let mask = self.slots.len() - 1;
        let mut at = hash as usize & mask;
        loop {
            let slot = self.slots[at];
            if slot == 0 {
                return None;
            }
            let id = slot - 1;
            if self.spans[id as usize].hash == hash && self.get(id) == name {
                return Some(id);
            }
            at = (at + 1) & mask;
        }
    }

    /// Records `id` in the first free slot of its probe sequence.
    fn place(&mut self, hash: u32, id: u32) {
        let mask = self.slots.len() - 1;
        let mut at = hash as usize & mask;
        while self.slots[at] != 0 {
            at = (at + 1) & mask;
        }
        self.slots[at] = id + 1;
    }

    /// Doubles the index and re-places every name.
    fn grow(&mut self) {
        let size = (self.slots.len() * 2).max(16);
        self.slots.clear();
        self.slots.resize(size, 0);
        for id in 0..self.spans.len() {
            self.place(self.spans[id].hash, id as u32);
        }
    }
}

/// A fixed multiplicative hash over eight-byte words.
fn hash(bytes: &[u8]) -> u32 {
    const K: u64 = 0x9e37_79b9_7f4a_7c15;
    let mix = |h: u64, word: u64| (h.rotate_left(5) ^ word).wrapping_mul(K);
    let (words, rest) = bytes.as_chunks::<8>();
    let mut h = bytes.len() as u64;
    for word in words {
        h = mix(h, u64::from_le_bytes(*word));
    }
    if !rest.is_empty() {
        let mut last = [0u8; 8];
        last[..rest.len()].copy_from_slice(rest);
        h = mix(h, u64::from_le_bytes(last));
    }
    (h >> 32) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `spec/object.md` "Section names": equal names share one entry.
    #[test]
    fn equal_names_share_an_id() {
        let mut table = NameTable::default();
        let text = table.intern(b".text").unwrap();
        let data = table.intern(b".data").unwrap();
        assert_eq!((text, data), (0, 1));
        assert_eq!(table.intern(b".text"), Some(text));
        assert_eq!(table.intern(b""), Some(2));
        assert_eq!(table.intern(b""), Some(2));
        assert_eq!(table.len(), 3);
        assert_eq!(table.get(text), b".text");
        assert_eq!(table.get(2), b"");
        assert_eq!(table.iter().collect::<Vec<_>>(), [&b".text"[..], b".data", b""]);
    }

    /// Ids stay valid and distinct across many index growths.
    #[test]
    fn many_names_survive_growth() {
        let mut table = NameTable::default();
        let name = |i: u32| format!(".text._ZN3fooILi{i}EE3barEv");
        for i in 0..5000 {
            assert_eq!(table.intern(name(i).as_bytes()), Some(i));
        }
        for i in 0..5000 {
            assert_eq!(table.intern(name(i).as_bytes()), Some(i));
            assert_eq!(table.get(i), name(i).as_bytes());
        }
        assert_eq!(table.len(), 5000);
    }

    /// Names that differ only in a short tail, or only in length, differ.
    #[test]
    fn near_names_are_distinct() {
        let mut table = NameTable::default();
        let names: [&[u8]; 6] = [b"a", b"a\0", b"aaaaaaaa", b"aaaaaaaaa", b"aaaaaaab", b"b"];
        let ids: Vec<_> = names.iter().map(|n| table.intern(n).unwrap()).collect();
        assert_eq!(ids, [0, 1, 2, 3, 4, 5]);
    }
}
