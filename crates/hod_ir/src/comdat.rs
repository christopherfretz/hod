//! The COMDAT table (`spec/ir.md` §2): a name and a selection kind per
//! [`ComdatId`].
//!
//! COMDAT names are a name space of their own (`$name` in LLVM's text), so
//! a COMDAT and a symbol may share a name, and usually do.

use crate::error::IrResult;
use crate::ids::ComdatId;
use crate::intern::{InternTable, next_id, pool_start};
use crate::strings::hash_bytes;

/// How a linker chooses among COMDATs of one name (LLVM language reference, "Comdats").
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Default)]
#[repr(u8)]
pub enum ComdatKind {
    /// `any`: keep any one.
    #[default]
    Any,
    /// `exactmatch`: all must have the same contents.
    ExactMatch,
    /// `largest`: keep the largest.
    Largest,
    /// `nodeduplicate`: keep them all; the group is only a unit of
    /// garbage collection.
    NoDeduplicate,
    /// `samesize`: all must have the same size.
    SameSize,
}

impl ComdatKind {
    /// The five kinds.
    pub const ALL: [ComdatKind; 5] = [
        ComdatKind::Any,
        ComdatKind::ExactMatch,
        ComdatKind::Largest,
        ComdatKind::NoDeduplicate,
        ComdatKind::SameSize,
    ];

    /// The keyword LLVM's text uses.
    pub const fn keyword(self) -> &'static str {
        match self {
            ComdatKind::Any => "any",
            ComdatKind::ExactMatch => "exactmatch",
            ComdatKind::Largest => "largest",
            ComdatKind::NoDeduplicate => "nodeduplicate",
            ComdatKind::SameSize => "samesize",
        }
    }
}

/// A COMDAT, read through its module (`Module::comdat`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Comdat<'a> {
    /// The name, without LLVM's `$`.
    pub name: &'a [u8],
    /// The selection kind.
    pub kind: ComdatKind,
}

#[derive(Clone, Copy, Debug)]
struct Entry {
    name_start: u32,
    name_len: u32,
    kind: ComdatKind,
}

/// The COMDATs of a module.
#[derive(Clone, Debug, Default)]
pub(crate) struct ComdatTable {
    entries: Vec<Entry>,
    names: Vec<u8>,
    by_name: InternTable,
}

impl ComdatTable {
    fn name(&self, index: u32) -> &[u8] {
        let entry = &self.entries[index as usize];
        &self.names[entry.name_start as usize..(entry.name_start + entry.name_len) as usize]
    }

    /// The COMDAT called `name`, if there is one.
    pub(crate) fn find(&self, name: &[u8]) -> Option<ComdatId> {
        self.by_name.find(hash_bytes(name), |id| self.name(id) == name).map(ComdatId::new)
    }

    /// The COMDAT called `name`, created with kind `any` if new.
    pub(crate) fn named(&mut self, name: &[u8]) -> IrResult<ComdatId> {
        if let Some(found) = self.find(name) {
            return Ok(found);
        }
        let id = next_id(self.entries.len(), "COMDAT")?;
        let name_start = pool_start(self.names.len(), name.len(), "COMDAT name")?;
        self.names.extend_from_slice(name);
        self.entries.push(Entry { name_start, name_len: name.len() as u32, kind: ComdatKind::Any });
        self.by_name.insert(hash_bytes(name), id);
        Ok(ComdatId::new(id))
    }

    /// Sets the selection kind of `id`.
    pub(crate) fn set_kind(&mut self, id: ComdatId, kind: ComdatKind) {
        self.entries[id.index()].kind = kind;
    }

    /// The COMDAT `id`.
    pub(crate) fn get(&self, id: ComdatId) -> Comdat<'_> {
        Comdat { name: self.name(id.raw()), kind: self.entries[id.index()].kind }
    }

    /// Number of COMDATs.
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §2: a COMDAT is a name and one of five selection kinds; a name is
    /// one COMDAT.
    #[test]
    fn comdats_are_named_and_have_a_kind() {
        assert_eq!(size_of::<Entry>(), 12);
        let mut table = ComdatTable::default();
        assert_eq!(table.find(b"_ZN1AD2Ev"), None);
        let a = table.named(b"_ZN1AD2Ev").unwrap();
        let b = table.named(b"").unwrap();
        assert_ne!(a, b);
        assert_eq!(table.named(b"_ZN1AD2Ev").unwrap(), a);
        assert_eq!(table.find(b"_ZN1AD2Ev"), Some(a));
        assert_eq!(table.len(), 2);
        assert_eq!(table.get(a), Comdat { name: b"_ZN1AD2Ev", kind: ComdatKind::Any });
        for kind in ComdatKind::ALL {
            table.set_kind(a, kind);
            assert_eq!(table.get(a).kind, kind);
            assert_eq!(table.get(b).kind, ComdatKind::Any);
        }
        let keywords: Vec<&str> = ComdatKind::ALL.iter().map(|kind| kind.keyword()).collect();
        assert_eq!(keywords, ["any", "exactmatch", "largest", "nodeduplicate", "samesize"]);
        assert_eq!(ComdatKind::default(), ComdatKind::Any);
    }
}
