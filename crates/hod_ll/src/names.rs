//! A map from byte-string names to small integers (`spec/ll.md` §5.2):
//! open addressing over an arena of the keys, so that a lookup neither
//! allocates nor hashes with a keyed hash.

/// A map from names to `u32` values. Keys are copied into the map once.
#[derive(Clone, Debug, Default)]
pub(crate) struct NameMap {
    bytes: Vec<u8>,
    /// `(start in bytes, length, value)`.
    entries: Vec<(u32, u32, u32)>,
    /// Entry index plus one, or zero; a power of two long.
    slots: Vec<u32>,
}

fn hash(key: &[u8]) -> u64 {
    let mut h = 0x9E37_79B9_7F4A_7C15 ^ key.len() as u64;
    for chunk in key.chunks(8) {
        let mut word = [0u8; 8];
        word[..chunk.len()].copy_from_slice(chunk);
        h = (h ^ u64::from_le_bytes(word)).wrapping_mul(0x2545_F491_4F6C_DD1D);
        h ^= h >> 29;
    }
    h
}

impl NameMap {
    fn key(&self, entry: usize) -> &[u8] {
        let (start, len, _) = self.entries[entry];
        &self.bytes[start as usize..(start + len) as usize]
    }

    /// The slot `key` is in or would go in.
    fn slot(&self, key: &[u8]) -> usize {
        let mask = self.slots.len() - 1;
        let mut at = hash(key) as usize & mask;
        loop {
            match self.slots[at] {
                0 => return at,
                entry if self.key(entry as usize - 1) == key => return at,
                _ => at = (at + 1) & mask,
            }
        }
    }

    /// The value of `key`.
    pub(crate) fn get(&self, key: &[u8]) -> Option<u32> {
        if self.slots.is_empty() {
            return None;
        }
        match self.slots[self.slot(key)] {
            0 => None,
            entry => Some(self.entries[entry as usize - 1].2),
        }
    }

    /// Adds `key` with `value`; `Err` is the value it already has.
    pub(crate) fn insert(&mut self, key: &[u8], value: u32) -> Result<(), u32> {
        if (self.entries.len() + 1) * 2 > self.slots.len() {
            let size = (self.slots.len() * 2).max(16);
            self.slots = vec![0; size];
            for entry in 0..self.entries.len() {
                let at = self.slot(self.key(entry));
                self.slots[at] = entry as u32 + 1;
            }
        }
        let at = self.slot(key);
        if let Some(entry) = self.slots[at].checked_sub(1) {
            return Err(self.entries[entry as usize].2);
        }
        let start = self.bytes.len() as u32;
        self.bytes.extend_from_slice(key);
        self.entries.push((start, key.len() as u32, value));
        self.slots[at] = self.entries.len() as u32;
        Ok(())
    }

    /// Replaces the value of every key by `f` of it.
    pub(crate) fn map_values(&mut self, mut f: impl FnMut(u32) -> u32) {
        for entry in &mut self.entries {
            entry.2 = f(entry.2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §5.2: every key comes back with its value, through growth, and a
    /// second insertion is refused with the first value.
    #[test]
    fn insert_and_get() {
        let mut map = NameMap::default();
        assert_eq!(map.get(b"a"), None);
        for i in 0..5000u32 {
            let key = format!("struct.name{i}");
            assert_eq!(map.insert(key.as_bytes(), i), Ok(()));
        }
        assert_eq!(map.insert(b"", 7), Ok(()));
        for i in 0..5000u32 {
            let key = format!("struct.name{i}");
            assert_eq!(map.get(key.as_bytes()), Some(i));
            assert_eq!(map.insert(key.as_bytes(), 0), Err(i));
        }
        assert_eq!(map.get(b""), Some(7));
        assert_eq!(map.get(b"struct.name"), None);
        map.map_values(|v| v + 1);
        assert_eq!(map.get(b"struct.name41"), Some(42));
    }
}
