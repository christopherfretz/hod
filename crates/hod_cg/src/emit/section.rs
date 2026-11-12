//! Which section a function or variable goes to, as `llc` chooses it
//! (`spec/codegen.md` §9.7): LLVM's `TargetLoweringObjectFile::
//! getKindForGlobal`, `TargetLoweringObjectFileELF::SelectSectionForGlobal`
//! and `getExplicitSectionGlobal`, and `MCContext`'s rules for which
//! sections of one name are one section.

use std::collections::HashMap;

use hod_obj::elf::{
    SHF_ALLOC, SHF_EXECINSTR, SHF_GNU_RETAIN, SHF_MERGE, SHF_STRINGS, SHF_TLS, SHF_WRITE,
    SHT_FINI_ARRAY, SHT_INIT_ARRAY, SHT_NOBITS, SHT_NOTE, SHT_PREINIT_ARRAY, SHT_PROGBITS,
};
use hod_obj::{SectionId, SectionKind};

/// LLVM's `SectionKind`, as far as ELF output tells the kinds apart.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Kind {
    /// Code.
    Text,
    /// Read-only data.
    ReadOnly,
    /// NUL-terminated strings of this character size, mergeable.
    CString(u8),
    /// Constants of this size, mergeable.
    Const(u8),
    /// Read-only after the dynamic linker has relocated it.
    ReadOnlyWithRel,
    /// Writable data.
    Data,
    /// Zero-initialized writable data; `local` for a symbol with local
    /// linkage (which gets at least one byte in `.bss`).
    Bss {
        /// Local linkage.
        local: bool,
    },
    /// Initialized thread-local data.
    ThreadData,
    /// Zero-initialized thread-local data.
    ThreadBss,
}

impl Kind {
    /// `getELFSectionFlags`.
    pub(crate) const fn flags(self) -> u64 {
        SHF_ALLOC
            | match self {
                Kind::Text => SHF_EXECINSTR,
                Kind::ReadOnly => 0,
                Kind::CString(_) => SHF_MERGE | SHF_STRINGS,
                Kind::Const(_) => SHF_MERGE,
                Kind::ReadOnlyWithRel | Kind::Data | Kind::Bss { .. } => SHF_WRITE,
                Kind::ThreadData | Kind::ThreadBss => SHF_WRITE | SHF_TLS,
            }
    }

    /// `getEntrySizeForKind`.
    pub(crate) const fn entry_size(self) -> u64 {
        match self {
            Kind::CString(size) | Kind::Const(size) => size as u64,
            _ => 0,
        }
    }

    /// Without file space.
    pub(crate) const fn is_nobits(self) -> bool {
        matches!(self, Kind::Bss { .. } | Kind::ThreadBss)
    }

    /// `getSectionPrefixForGlobal` (small code model).
    const fn prefix(self) -> &'static [u8] {
        match self {
            Kind::Text => b".text",
            Kind::ReadOnly | Kind::CString(_) | Kind::Const(_) => b".rodata",
            Kind::Bss { .. } => b".bss",
            Kind::ThreadData => b".tdata",
            Kind::ThreadBss => b".tbss",
            Kind::Data => b".data",
            Kind::ReadOnlyWithRel => b".data.rel.ro",
        }
    }
}

/// `getELFKindForNamedSection`: an explicit section's name may say more
/// than the symbol's kind.
pub(crate) fn named_kind(name: &[u8], kind: Kind) -> Kind {
    let is = |exact: &[u8], prefixes: &[&[u8]]| {
        name == exact || prefixes.iter().any(|prefix| name.starts_with(prefix))
    };
    if is(b".bss", &[b".bss.", b".gnu.linkonce.b.", b".llvm.linkonce.b."])
        || is(b".sbss", &[b".sbss.", b".gnu.linkonce.sb.", b".llvm.linkonce.sb."])
    {
        return Kind::Bss { local: false };
    }
    if is(b".tdata", &[b".tdata.", b".gnu.linkonce.td.", b".llvm.linkonce.td."]) {
        return Kind::ThreadData;
    }
    if is(b".tbss", &[b".tbss.", b".gnu.linkonce.tb.", b".llvm.linkonce.tb."]) {
        return Kind::ThreadBss;
    }
    kind
}

/// `getELFSectionType`.
fn section_type(name: &[u8], kind: Kind) -> u32 {
    let has_prefix = |prefix: &[u8]| {
        name.strip_prefix(prefix).is_some_and(|rest| rest.is_empty() || rest[0] == b'.')
    };
    if name.starts_with(b".note") {
        SHT_NOTE
    } else if has_prefix(b".init_array") {
        SHT_INIT_ARRAY
    } else if has_prefix(b".fini_array") {
        SHT_FINI_ARRAY
    } else if has_prefix(b".preinit_array") {
        SHT_PREINIT_ARRAY
    } else if kind.is_nobits() {
        SHT_NOBITS
    } else {
        SHT_PROGBITS
    }
}

/// The `hod_obj` kind of a section with these header fields.
pub(crate) fn obj_kind(sh_type: u32, flags: u64, entry_size: u64) -> SectionKind {
    const A: u64 = SHF_ALLOC;
    const W: u64 = SHF_WRITE;
    match (sh_type, flags, entry_size) {
        (SHT_PROGBITS, f, 0) if f == A | SHF_EXECINSTR => SectionKind::Text,
        (SHT_PROGBITS, f, 0) if f == A | W => SectionKind::Data,
        (SHT_PROGBITS, A, 0) => SectionKind::ReadOnlyData,
        (SHT_NOBITS, f, 0) if f == A | W => SectionKind::Bss,
        (SHT_PROGBITS, f, 0) if f == A | W | SHF_TLS => SectionKind::Tls,
        (SHT_NOBITS, f, 0) if f == A | W | SHF_TLS => SectionKind::TlsBss,
        (SHT_INIT_ARRAY, f, 0) if f == A | W => SectionKind::InitArray,
        (SHT_FINI_ARRAY, f, 0) if f == A | W => SectionKind::FiniArray,
        (SHT_NOTE, A, 0) => SectionKind::Note,
        (SHT_PROGBITS, f, e) if f == A | SHF_MERGE | SHF_STRINGS && e > 0 && e <= 4 => {
            SectionKind::MergeableStrings { char_size: e as u32 }
        }
        (SHT_PROGBITS, f, e) if f == A | SHF_MERGE && e > 0 && e <= 32 => {
            SectionKind::MergeableConstants { entry_size: e as u32 }
        }
        _ => SectionKind::Elf { sh_type, sh_flags: flags, sh_entsize: entry_size },
    }
}

/// "Not a unique section" (`MCSection::NonUniqueID`).
const GENERIC: u32 = u32::MAX;

/// "No group".
pub(crate) const NO_GROUP: u32 = u32::MAX;

/// A section to create: its name is the caller's.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct NewSection {
    /// `hod_obj`'s kind.
    pub(crate) kind: SectionKind,
    /// The group to join, or [`NO_GROUP`].
    pub(crate) group: u32,
}

/// What a lookup found.
pub(crate) enum Found {
    /// This existing section.
    Old(SectionId),
    /// Nothing: create this section, then [`Sections::created`].
    New(NewSection, Pending),
}

/// A section being created, to be registered once it has an id.
#[derive(Debug)]
pub(crate) struct Pending {
    /// A section of the global's own (`<prefix>.<symbol>`): not the
    /// generic section of its kind.
    own: bool,
    id: u32,
    flags: u64,
    entry_size: u64,
    group: u32,
}

/// The sections of one name that are distinct (`MCContext`'s section map
/// key is name, group and unique id; its flags are the first creator's).
#[derive(Default, Debug)]
struct ByName {
    sections: Vec<(u32, u32, SectionId)>,
    /// `MCContext::ELFSeenGenericMergeableSections`: a generic section of
    /// this name exists.
    seen: bool,
    /// `MCContext::ELFEntrySizeMap` for this name: flags and entry size to
    /// unique id, first registration wins.
    entry_sizes: Vec<(u64, u64, u32)>,
}

/// The state of section selection for one object: the named sections
/// that exist, in the shape `MCContext` keeps them.
#[derive(Debug)]
pub(crate) struct Sections {
    by_name: HashMap<Box<[u8]>, ByName>,
    next_unique: u32,
    /// The generic section of each standard kind, once created: `Text`,
    /// `ReadOnly`, `ReadOnlyWithRel`, `Data`, `Bss`, `ThreadData`,
    /// `ThreadBss`, `Const(4, 8, 16, 32)`.
    standard: [Option<SectionId>; 11],
    /// The generic `.rodata.str<size>.<align>` sections: (size, align,
    /// section).
    strings: Vec<(u8, u64, SectionId)>,
}

impl Sections {
    /// The state `MCObjectFileInfo` leaves: its standard sections exist as
    /// generic sections (whether or not they are ever written).
    pub(crate) fn new() -> Sections {
        let mut sections = Sections {
            by_name: HashMap::new(),
            next_unique: 0,
            standard: [None; 11],
            strings: Vec::new(),
        };
        let standard: [(&[u8], Kind); 11] = [
            (b".text", Kind::Text),
            (b".data", Kind::Data),
            (b".bss", Kind::Bss { local: false }),
            (b".rodata", Kind::ReadOnly),
            (b".tdata", Kind::ThreadData),
            (b".tbss", Kind::ThreadBss),
            (b".data.rel.ro", Kind::ReadOnlyWithRel),
            (b".rodata.cst4", Kind::Const(4)),
            (b".rodata.cst8", Kind::Const(8)),
            (b".rodata.cst16", Kind::Const(16)),
            (b".rodata.cst32", Kind::Const(32)),
        ];
        for (name, kind) in standard {
            sections.record(name, kind.flags(), GENERIC, kind.entry_size());
        }
        sections
    }

    fn standard_index(kind: Kind) -> Option<usize> {
        Some(match kind {
            Kind::Text => 0,
            Kind::ReadOnly => 1,
            Kind::ReadOnlyWithRel => 2,
            Kind::Data => 3,
            Kind::Bss { .. } => 4,
            Kind::ThreadData => 5,
            Kind::ThreadBss => 6,
            Kind::Const(4) => 7,
            Kind::Const(8) => 8,
            Kind::Const(16) => 9,
            Kind::Const(32) => 10,
            Kind::Const(_) | Kind::CString(_) => return None,
        })
    }

    /// `MCContext::recordELFMergeableSectionInfo`.
    fn record(&mut self, name: &[u8], flags: u64, id: u32, entry_size: u64) {
        let entry = self.entry(name);
        let mergeable = flags & SHF_MERGE != 0 || id == GENERIC;
        if id == GENERIC {
            entry.seen = true;
        }
        if (mergeable || entry.seen || is_implicit_mergeable(name))
            && !entry.entry_sizes.iter().any(|&(f, e, _)| f == flags && e == entry_size)
        {
            entry.entry_sizes.push((flags, entry_size, id));
        }
    }

    fn entry(&mut self, name: &[u8]) -> &mut ByName {
        if !self.by_name.contains_key(name) {
            self.by_name.insert(name.into(), ByName::default());
        }
        self.by_name.get_mut(name).expect("record: just inserted")
    }

    /// The section a global without an explicit section goes to, by
    /// `SelectSectionForGlobal`: the generic section of its kind, or, when
    /// `unique` (a COMDAT member, `-ffunction-sections` or
    /// `-fdata-sections` for a kind that is not mergeable, or a retained
    /// symbol), a section of its own named `<prefix>.<symbol>`. `name` is
    /// a scratch buffer that receives the name to create. `align` is the
    /// global's preferred alignment, which names a string section.
    #[expect(clippy::too_many_arguments, reason = "the inputs of `SelectSectionForGlobal`")]
    pub(crate) fn implicit(
        &mut self,
        kind: Kind,
        unique: bool,
        retain: bool,
        group: u32,
        symbol: &[u8],
        align: u64,
        name: &mut Vec<u8>,
    ) -> Found {
        name.clear();
        name.extend_from_slice(kind.prefix());
        match kind {
            Kind::CString(size) => {
                use std::io::Write as _;
                let _ = write!(name, ".str{size}.{align}");
            }
            Kind::Const(size) => {
                use std::io::Write as _;
                let _ = write!(name, ".cst{size}");
            }
            _ => {}
        }
        let mut flags = kind.flags();
        if retain {
            flags |= SHF_GNU_RETAIN;
        }
        let entry_size = kind.entry_size();
        // An implicit name never starts like a typed one (`.note`,
        // `.init_array`): the type is the kind's.
        let obj = obj_kind(section_type(name, kind), flags, entry_size);
        let new = |group| NewSection { kind: obj, group };
        if unique || retain {
            name.push(b'.');
            name.extend_from_slice(symbol);
            let pending = Pending { own: true, id: GENERIC, flags, entry_size, group };
            return match self.find(name, group, GENERIC) {
                Some(section) => Found::Old(section),
                None => Found::New(new(group), pending),
            };
        }
        let cached = match kind {
            Kind::CString(size) => self
                .strings
                .iter()
                .find(|&&(s, a, _)| s == size && a == align)
                .map(|&(_, _, section)| section),
            _ => Sections::standard_index(kind).and_then(|index| self.standard[index]),
        };
        if let Some(section) = cached {
            return Found::Old(section);
        }
        if let Some(section) = self.find(name, NO_GROUP, GENERIC) {
            self.cache(kind, align, section);
            return Found::Old(section);
        }
        Found::New(
            new(NO_GROUP),
            Pending { own: false, id: GENERIC, flags, entry_size, group: NO_GROUP },
        )
    }

    /// The section a global with the explicit section `name` goes to
    /// (`selectExplicitSectionGlobal`, `calcUniqueIDUpdateFlagsAndSize`).
    /// `kind` is the global's own kind (before the name refines it);
    /// `align` its preferred alignment.
    pub(crate) fn explicit(
        &mut self,
        name: &[u8],
        kind: Kind,
        retain: bool,
        group: u32,
        align: u64,
    ) -> Found {
        let kind = named_kind(name, kind);
        let mut flags = kind.flags();
        let entry_size = kind.entry_size();
        let mergeable = flags & SHF_MERGE != 0;
        let id = if retain {
            flags |= SHF_GNU_RETAIN;
            self.unique()
        } else {
            let entry = self.by_name.get(name);
            let seen = entry.is_some_and(|entry| entry.seen) || is_implicit_mergeable(name);
            let previous = entry.and_then(|entry| {
                entry.entry_sizes.iter().find(|&&(f, e, _)| f == flags && e == entry_size)
            });
            if !mergeable && !seen {
                GENERIC
            } else if let Some(&(_, _, id)) = previous {
                id
            } else if mergeable
                && is_implicit_mergeable(name)
                && name.starts_with(&stem(kind, align))
            {
                GENERIC
            } else {
                self.unique()
            }
        };
        if let Some(section) = self.find(name, group, id) {
            return Found::Old(section);
        }
        let new = NewSection { kind: obj_kind(section_type(name, kind), flags, entry_size), group };
        Found::New(new, Pending { own: false, id, flags, entry_size, group })
    }

    fn unique(&mut self) -> u32 {
        let id = self.next_unique;
        self.next_unique += 1;
        id
    }

    fn find(&self, name: &[u8], group: u32, id: u32) -> Option<SectionId> {
        let entry = self.by_name.get(name)?;
        entry.sections.iter().find(|&&(g, i, _)| g == group && i == id).map(|&(_, _, s)| s)
    }

    /// Registers a section created after a lookup returned
    /// [`Found::New`]. `name` is its name; `kind` and `align` as given to
    /// the lookup (they key the standard-section cache).
    pub(crate) fn created(
        &mut self,
        name: &[u8],
        pending: &Pending,
        section: SectionId,
        cache: Option<(Kind, u64)>,
    ) {
        self.entry(name).sections.push((pending.group, pending.id, section));
        self.record(name, pending.flags, pending.id, pending.entry_size);
        if let Some((kind, align)) =
            cache.filter(|_| !pending.own && pending.group == NO_GROUP && pending.id == GENERIC)
        {
            self.cache(kind, align, section);
        }
    }

    /// The generic section of a standard kind, if it was created.
    pub(crate) fn generic(&self, kind: Kind) -> Option<SectionId> {
        Sections::standard_index(kind).and_then(|index| self.standard[index])
    }

    fn cache(&mut self, kind: Kind, align: u64, section: SectionId) {
        match kind {
            Kind::CString(size) => self.strings.push((size, align, section)),
            _ => {
                if let Some(index) = Sections::standard_index(kind) {
                    self.standard[index] = Some(section);
                }
            }
        }
    }
}

/// `MCContext::isELFImplicitMergeableSectionNamePrefix`.
fn is_implicit_mergeable(name: &[u8]) -> bool {
    name.starts_with(b".rodata.str") || name.starts_with(b".rodata.cst")
}

/// The name `SelectSectionForGlobal` would give a mergeable kind.
fn stem(kind: Kind, align: u64) -> Vec<u8> {
    use std::io::Write as _;
    let mut name = kind.prefix().to_vec();
    match kind {
        Kind::CString(size) => {
            let _ = write!(name, ".str{size}.{align}");
        }
        Kind::Const(size) => {
            let _ = write!(name, ".cst{size}");
        }
        _ => {}
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §9.7: the section kinds `getELFKindForNamedSection` refines, and
    /// the ELF types that names imply.
    #[test]
    fn names_refine_kinds_and_types() {
        assert_eq!(named_kind(b".bss.x", Kind::Data), Kind::Bss { local: false });
        assert_eq!(named_kind(b".tbss", Kind::Data), Kind::ThreadBss);
        assert_eq!(named_kind(b".bssx", Kind::Data), Kind::Data);
        assert_eq!(section_type(b".init_array.5", Kind::Data), SHT_INIT_ARRAY);
        assert_eq!(section_type(b".init_arrayx", Kind::Data), SHT_PROGBITS);
        assert_eq!(section_type(b".note.x", Kind::Data), SHT_NOTE);
    }

    /// §9.7: explicit sections of one name with different flags are
    /// distinct sections; equal flags share one.
    #[test]
    fn explicit_sections_share_by_flags() {
        let mut sections = Sections::new();
        let mut next = 0;
        let mut get = |sections: &mut Sections, name: &[u8], kind| match sections
            .explicit(name, kind, false, NO_GROUP, 1)
        {
            Found::Old(section) => section,
            Found::New(_, pending) => {
                let section = SectionId::from_index(next);
                next += 1;
                sections.created(name, &pending, section, None);
                section
            }
        };
        let data = get(&mut sections, b"foo", Kind::Data);
        assert_eq!(data, get(&mut sections, b"foo", Kind::Data));
        let strings = get(&mut sections, b"foo", Kind::CString(1));
        assert_ne!(data, strings);
        assert_eq!(strings, get(&mut sections, b"foo", Kind::CString(1)));
        // `.data` exists before anything is placed: a read-only variable
        // that names it gets a section of its own.
        let writable = get(&mut sections, b".data", Kind::Data);
        assert_ne!(writable, get(&mut sections, b".data", Kind::ReadOnly));
    }
}
