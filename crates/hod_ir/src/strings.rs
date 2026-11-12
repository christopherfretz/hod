//! Interned byte strings for the module-level tables (`spec/ir.md` §2):
//! section names, attribute values, COMDAT names, construct names.
//!
//! One contiguous byte buffer and a span per string; equal strings share
//! one [`StrId`]. Symbol names do not live here: the symbol table keeps
//! its own buffer so that a name lookup lands on the symbol directly.

use crate::error::IrResult;
use crate::intern::{Hasher64, InternTable, next_id, pool_start};

/// An interned byte string of one [`StrPool`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub(crate) struct StrId(u32);

impl StrId {
    /// "No string", for optional fields stored as a raw `u32`.
    pub(crate) const NONE_RAW: u32 = u32::MAX;

    /// The raw value, never [`StrId::NONE_RAW`].
    #[inline]
    pub(crate) const fn raw(self) -> u32 {
        self.0
    }

    /// The id stored as `raw`, or `None` for [`StrId::NONE_RAW`].
    #[inline]
    pub(crate) const fn from_raw(raw: u32) -> Option<StrId> {
        if raw == Self::NONE_RAW { None } else { Some(StrId(raw)) }
    }
}

/// The hash of a byte string, as every name table of this crate uses it.
#[inline]
pub(crate) fn hash_bytes(bytes: &[u8]) -> u32 {
    let mut h = Hasher64::new(0x5eed);
    h.bytes(bytes);
    h.finish()
}

/// A set of interned byte strings.
#[derive(Clone, Debug, Default)]
pub(crate) struct StrPool {
    bytes: Vec<u8>,
    /// `(start, len)` in `bytes`.
    spans: Vec<(u32, u32)>,
    interner: InternTable,
}

impl StrPool {
    /// The id of `text`, added if new.
    pub(crate) fn intern(&mut self, text: &[u8]) -> IrResult<StrId> {
        let hash = hash_bytes(text);
        if let Some(found) = self.lookup(hash, text) {
            return Ok(found);
        }
        let id = next_id(self.spans.len(), "string")?;
        let start = pool_start(self.bytes.len(), text.len(), "string byte")?;
        self.bytes.extend_from_slice(text);
        self.spans.push((start, text.len() as u32));
        self.interner.insert(hash, id);
        Ok(StrId(id))
    }

    /// The id of `text` if it has been interned.
    pub(crate) fn find(&self, text: &[u8]) -> Option<StrId> {
        self.lookup(hash_bytes(text), text)
    }

    fn lookup(&self, hash: u32, text: &[u8]) -> Option<StrId> {
        self.interner.find(hash, |id| self.span(id) == text).map(StrId)
    }

    fn span(&self, id: u32) -> &[u8] {
        let (start, len) = self.spans[id as usize];
        &self.bytes[start as usize..start as usize + len as usize]
    }

    /// The string `id`.
    #[inline]
    pub(crate) fn get(&self, id: StrId) -> &[u8] {
        self.span(id.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §2: equal strings are one id; the empty string and embedded zero
    /// bytes are ordinary strings.
    #[test]
    fn strings_are_interned_by_content() {
        let mut pool = StrPool::default();
        let a = pool.intern(b".text.hot").unwrap();
        let b = pool.intern(b".data").unwrap();
        let empty = pool.intern(b"").unwrap();
        let nul = pool.intern(b"a\0b").unwrap();
        assert_eq!(pool.intern(b".text.hot").unwrap(), a);
        assert_eq!(pool.intern(b"").unwrap(), empty);
        assert_ne!(a, b);
        assert_ne!(pool.intern(b"a\0").unwrap(), nul);
        assert_eq!(pool.get(a), b".text.hot");
        assert_eq!(pool.get(empty), b"");
        assert_eq!(pool.get(nul), b"a\0b");
        assert_eq!(pool.find(b".data"), Some(b));
        assert_eq!(pool.find(b".bss"), None);
        assert_eq!(StrId::from_raw(a.raw()), Some(a));
        assert_eq!(StrId::from_raw(StrId::NONE_RAW), None);
        for i in 0..2000u32 {
            let text = format!("section.{i}");
            let id = pool.intern(text.as_bytes()).unwrap();
            assert_eq!(pool.get(id), text.as_bytes());
        }
        assert_eq!(pool.get(a), b".text.hot");
    }
}
