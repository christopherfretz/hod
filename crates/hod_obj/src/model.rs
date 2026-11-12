//! The object model. Contract: `spec/object.md` "Model".
//!
//! An [`Object`] is a set of flat tables addressed by `u32` ids: sections,
//! symbols, relocations, groups and two string arenas. Nothing in it points
//! at anything else, and building one allocates only when a table grows.

use crate::error::{Error, Limit};
use crate::names::NameTable;
use crate::reloc::RelocKind;

/// "No entry" in the index chains.
pub(crate) const NONE: u32 = u32::MAX;

/// The target architecture of an object.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Arch {
    /// 64-bit Arm, little-endian.
    AArch64,
    /// x86-64.
    X86_64,
}

macro_rules! id_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(pub(crate) u32);

        impl $name {
            /// The id with the given position in creation order.
            pub const fn from_index(index: u32) -> Self {
                Self(index)
            }

            /// The position in creation order, from zero.
            pub const fn index(self) -> u32 {
                self.0
            }
        }
    };
}

id_type! {
    /// A section of one [`Object`]. Ids count up from zero in the order
    /// sections are added.
    SectionId
}
id_type! {
    /// A symbol of one [`Object`]. Ids count up from zero in the order
    /// symbols are added.
    SymbolId
}
id_type! {
    /// A section group of one [`Object`]. Ids count up from zero in the
    /// order groups are added.
    GroupId
}

/// A symbol name stored in an [`Object`] by [`Object::add_string`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StrId(pub(crate) u32);

impl StrId {
    /// The empty name.
    pub const EMPTY: Self = Self(0);
}

/// What a section holds, which decides its type and flags in the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SectionKind {
    /// Executable code.
    Text,
    /// Writable initialized data.
    Data,
    /// Read-only data.
    ReadOnlyData,
    /// Writable zero-initialized data. Occupies no file space.
    Bss,
    /// Initialized thread-local data.
    Tls,
    /// Zero-initialized thread-local data. Occupies no file space.
    TlsBss,
    /// Pointers to functions run before `main`.
    InitArray,
    /// Pointers to functions run at exit.
    FiniArray,
    /// A note that is part of the loaded image.
    Note,
    /// Debugging information: not part of the loaded image.
    Debug,
    /// Read-only NUL-terminated strings the linker may merge.
    MergeableStrings {
        /// The size of one character in bytes; not zero.
        char_size: u32,
    },
    /// Read-only fixed-size constants the linker may merge.
    MergeableConstants {
        /// The size of one constant in bytes; not zero.
        entry_size: u32,
    },
    /// Anything else, by its raw ELF section header fields. The constants
    /// are in [`crate::elf`].
    Elf {
        /// `sh_type`.
        sh_type: u32,
        /// `sh_flags`, without `SHF_GROUP` and `SHF_INFO_LINK`.
        sh_flags: u64,
        /// `sh_entsize`.
        sh_entsize: u64,
    },
}

/// A symbol's binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Binding {
    /// Visible only inside the object.
    Local,
    /// Visible to all objects; at most one definition.
    Global,
    /// Like global, but yields to a global definition.
    Weak,
}

/// A symbol's visibility outside the linked component.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Visibility {
    /// As the binding says.
    Default,
    /// Not visible to other components.
    Hidden,
    /// Visible, but references from inside the component bind to it.
    Protected,
}

/// What a symbol names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    /// Unspecified.
    NoType,
    /// A function.
    Func,
    /// A data object.
    Object,
    /// A section. Use [`Object::section_symbol`].
    Section,
    /// The source file. Use [`Object::add_file_symbol`].
    File,
    /// A thread-local object.
    Tls,
    /// A function that returns the address to call (`STT_GNU_IFUNC`).
    GnuIfunc,
}

/// Where a symbol is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SymbolPlace {
    /// Defined elsewhere.
    Undefined,
    /// `value` is absolute, not relative to a section.
    Absolute,
    /// A common block: `value` is its alignment, `size` its size.
    Common,
    /// `value` is an offset into this section.
    Section(SectionId),
}

/// One symbol. Plain data: change it through [`Object::symbol_mut`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Symbol {
    /// The name.
    pub name: StrId,
    /// The offset in the section, absolute value, or common alignment.
    pub value: u64,
    /// The size in bytes; zero when unknown.
    pub size: u64,
    /// Where the symbol is.
    pub place: SymbolPlace,
    /// The binding.
    pub binding: Binding,
    /// What the symbol names.
    pub kind: SymbolKind,
    /// The visibility.
    pub visibility: Visibility,
    /// Processor-specific bits of `st_other`, such as
    /// `STO_AARCH64_VARIANT_PCS`. The low two bits must be zero.
    pub other: u8,
}

impl Symbol {
    /// A global, default-visibility reference to a symbol defined elsewhere.
    pub const fn undefined(name: StrId) -> Self {
        Self {
            name,
            value: 0,
            size: 0,
            place: SymbolPlace::Undefined,
            binding: Binding::Global,
            kind: SymbolKind::NoType,
            visibility: Visibility::Default,
            other: 0,
        }
    }

    /// A global, default-visibility symbol at `value` in `section`.
    pub const fn defined(
        name: StrId,
        kind: SymbolKind,
        section: SectionId,
        value: u64,
        size: u64,
    ) -> Self {
        Self {
            name,
            value,
            size,
            place: SymbolPlace::Section(section),
            binding: Binding::Global,
            kind,
            visibility: Visibility::Default,
            other: 0,
        }
    }

    /// The same symbol with another binding.
    #[must_use]
    pub const fn with_binding(mut self, binding: Binding) -> Self {
        self.binding = binding;
        self
    }

    /// The same symbol with another visibility.
    #[must_use]
    pub const fn with_visibility(mut self, visibility: Visibility) -> Self {
        self.visibility = visibility;
        self
    }
}

/// One relocation, as [`Object::relocations`] reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Relocation {
    /// The offset in the section of the place to patch.
    pub offset: u64,
    /// The kind.
    pub kind: RelocKind,
    /// The symbol referred to.
    pub symbol: SymbolId,
    /// The addend.
    pub addend: i64,
}

/// Whether a group is deduplicated by the linker.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GroupKind {
    /// A COMDAT group: of all groups with the same signature the linker
    /// keeps one.
    Comdat,
    /// Sections that are kept or discarded together.
    Plain,
}

/// The stack marker section, `.note.GNU-stack`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StackNote {
    /// The code does not need an executable stack. The default.
    NonExecutable,
    /// The code needs an executable stack.
    Executable,
    /// No marker; the linker's default applies.
    Omit,
}

/// A section's record.
#[derive(Debug)]
pub(crate) struct SectionData {
    /// Index in the section name table.
    pub(crate) name: u32,
    pub(crate) kind: SectionKind,
    pub(crate) align: u64,
    pub(crate) data: Vec<u8>,
    /// The size of a section that occupies no file space.
    pub(crate) nobits_size: u64,
    /// First and last relocation of the section's chain, and its length.
    pub(crate) reloc_head: u32,
    pub(crate) reloc_tail: u32,
    pub(crate) reloc_count: u32,
    /// The largest relocation offset.
    pub(crate) reloc_max: u64,
    /// Whether some relocation was added below `reloc_max`.
    pub(crate) relocs_unsorted: bool,
    /// The group the section is in, and the next member of that group.
    pub(crate) group: u32,
    pub(crate) next_in_group: u32,
    /// The section's symbol, once asked for.
    pub(crate) symbol: u32,
}

impl SectionData {
    pub(crate) fn size(&self) -> u64 {
        if self.kind.is_nobits() { self.nobits_size } else { self.data.len() as u64 }
    }
}

impl SectionKind {
    /// Whether a section of this kind occupies no file space.
    pub const fn is_nobits(self) -> bool {
        matches!(self, Self::Bss | Self::TlsBss | Self::Elf { sh_type: crate::elf::SHT_NOBITS, .. })
    }
}

/// A relocation's record: 32 bytes, chained per section.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RelocEntry {
    pub(crate) offset: u64,
    pub(crate) addend: i64,
    pub(crate) symbol: u32,
    pub(crate) r_type: u32,
    /// The next relocation of the same section.
    pub(crate) next: u32,
}

/// A group's record.
#[derive(Clone, Copy, Debug)]
pub(crate) struct GroupData {
    pub(crate) signature: u32,
    pub(crate) kind: GroupKind,
    /// First and last member section.
    pub(crate) head: u32,
    pub(crate) tail: u32,
}

/// One relocatable object file in memory.
///
/// Methods that take an id panic when the id was not issued by this object.
#[derive(Debug)]
pub struct Object {
    pub(crate) arch: Arch,
    pub(crate) flags: u32,
    pub(crate) stack_note: StackNote,
    /// Symbol names, each followed by a NUL, after one leading NUL: the
    /// image of the string table.
    pub(crate) strings: Vec<u8>,
    pub(crate) section_names: NameTable,
    pub(crate) sections: Vec<SectionData>,
    pub(crate) symbols: Vec<Symbol>,
    /// One bit per symbol: a relocation or a group refers to it, or it was
    /// retained.
    pub(crate) referenced: Vec<u64>,
    pub(crate) relocs: Vec<RelocEntry>,
    pub(crate) groups: Vec<GroupData>,
    /// The first error of a call that does not return one.
    pub(crate) deferred: Option<Error>,
}

impl Object {
    /// An empty object for `arch`.
    pub fn new(arch: Arch) -> Self {
        Self {
            arch,
            flags: 0,
            stack_note: StackNote::NonExecutable,
            strings: vec![0],
            section_names: NameTable::default(),
            sections: Vec::new(),
            symbols: Vec::new(),
            referenced: Vec::new(),
            relocs: Vec::new(),
            groups: Vec::new(),
            deferred: None,
        }
    }

    /// The architecture.
    pub fn arch(&self) -> Arch {
        self.arch
    }

    /// Sets the processor-specific flags of the file header (`e_flags`).
    /// Zero by default, which is right for both architectures.
    pub fn set_header_flags(&mut self, flags: u32) {
        self.flags = flags;
    }

    /// Chooses the stack marker.
    pub fn set_stack_note(&mut self, note: StackNote) {
        self.stack_note = note;
    }

    /// Reserves room for this many more sections, symbols, relocations and
    /// bytes of symbol names.
    pub fn reserve(
        &mut self,
        sections: usize,
        symbols: usize,
        relocations: usize,
        string_bytes: usize,
    ) {
        self.sections.reserve(sections);
        self.symbols.reserve(symbols);
        self.referenced.reserve(symbols / 64 + 1);
        self.relocs.reserve(relocations);
        self.strings.reserve(string_bytes);
    }

    fn defer(&mut self, error: Error) {
        self.deferred.get_or_insert(error);
    }

    // ---- strings ----

    /// Stores a symbol name and returns its id. Equal names are not
    /// merged: a caller that uses one name many times adds it once and
    /// reuses the id. The empty name is [`StrId::EMPTY`] and takes no room.
    ///
    /// # Errors
    /// [`Error::NulInName`] if `name` contains a NUL byte;
    /// [`Error::TooLarge`] if the names would exceed 4 GiB.
    pub fn add_string(&mut self, name: &[u8]) -> Result<StrId, Error> {
        if name.is_empty() {
            return Ok(StrId::EMPTY);
        }
        if name.contains(&0) {
            return Err(Error::NulInName);
        }
        let id = u32::try_from(self.strings.len())
            .ok()
            .filter(|&start| u32::try_from(name.len()).is_ok_and(|len| len < u32::MAX - start))
            .ok_or(Error::TooLarge(Limit::StringBytes))?;
        self.strings.extend_from_slice(name);
        self.strings.push(0);
        Ok(StrId(id))
    }

    /// The bytes of a stored name. Never panics: for an id this object did
    /// not issue the result is unspecified (empty when past the end).
    pub fn string(&self, id: StrId) -> &[u8] {
        let tail = self.strings.get(id.0 as usize..).unwrap_or_default();
        let len = tail.iter().position(|&b| b == 0).unwrap_or(tail.len());
        &tail[..len]
    }

    // ---- sections ----

    /// Adds an empty section. Several sections may have the same name.
    /// `align` is the section's alignment in bytes, a power of two.
    ///
    /// # Errors
    /// [`Error::NulInName`], [`Error::BadAlignment`],
    /// [`Error::BadEntrySize`] for a mergeable kind with a zero entry size,
    /// [`Error::ReservedSectionType`] and [`Error::ReservedSectionFlags`]
    /// for a [`SectionKind::Elf`] that claims what the writer generates,
    /// and [`Error::TooLarge`].
    pub fn add_section(
        &mut self,
        name: &[u8],
        kind: SectionKind,
        align: u64,
    ) -> Result<SectionId, Error> {
        if name.contains(&0) {
            return Err(Error::NulInName);
        }
        if !align.is_power_of_two() {
            return Err(Error::BadAlignment { align });
        }
        crate::elf::check_kind(kind)?;
        // The last index is `NONE`.
        let id = u32::try_from(self.sections.len())
            .ok()
            .filter(|&id| id != NONE)
            .ok_or(Error::TooLarge(Limit::Sections))?;
        let name =
            self.section_names.intern(name).ok_or(Error::TooLarge(Limit::SectionNameBytes))?;
        self.sections.push(SectionData {
            name,
            kind,
            align,
            data: Vec::new(),
            nobits_size: 0,
            reloc_head: NONE,
            reloc_tail: NONE,
            reloc_count: 0,
            reloc_max: 0,
            relocs_unsorted: false,
            group: NONE,
            next_in_group: NONE,
            symbol: NONE,
        });
        Ok(SectionId(id))
    }

    /// How many sections have been added.
    pub fn section_count(&self) -> u32 {
        self.sections.len() as u32
    }

    /// A section's name.
    pub fn section_name(&self, section: SectionId) -> &[u8] {
        self.section_names.get(self.sections[section.0 as usize].name)
    }

    /// A section's kind.
    pub fn section_kind(&self, section: SectionId) -> SectionKind {
        self.sections[section.0 as usize].kind
    }

    /// A section's alignment in bytes.
    pub fn section_align(&self, section: SectionId) -> u64 {
        self.sections[section.0 as usize].align
    }

    /// Raises a section's alignment to at least `align`, a power of two.
    /// Any other value is reported by the writer as
    /// [`Error::BadAlignment`].
    pub fn raise_section_align(&mut self, section: SectionId, align: u64) {
        if align.is_power_of_two() {
            let sec = &mut self.sections[section.0 as usize];
            sec.align = sec.align.max(align);
        } else {
            self.defer(Error::BadAlignment { align });
        }
    }

    /// A section's size in bytes.
    pub fn section_size(&self, section: SectionId) -> u64 {
        self.sections[section.0 as usize].size()
    }

    /// Appends bytes to a section and returns the offset they start at.
    pub fn append(&mut self, section: SectionId, bytes: &[u8]) -> u64 {
        let data = &mut self.sections[section.0 as usize].data;
        let offset = data.len() as u64;
        data.extend_from_slice(bytes);
        offset
    }

    /// Appends an owned buffer to a section and returns the offset it
    /// starts at. An empty section takes the buffer over without copying.
    pub fn append_owned(&mut self, section: SectionId, mut bytes: Vec<u8>) -> u64 {
        let data = &mut self.sections[section.0 as usize].data;
        let offset = data.len() as u64;
        if data.is_empty() {
            *data = bytes;
        } else {
            data.append(&mut bytes);
        }
        offset
    }

    /// Appends `len` zero bytes and returns the offset they start at. For a
    /// section that occupies no file space this only grows its size.
    pub fn append_zeros(&mut self, section: SectionId, len: u64) -> u64 {
        let sec = &mut self.sections[section.0 as usize];
        let offset = sec.size();
        if sec.kind.is_nobits() {
            sec.nobits_size = sec.nobits_size.saturating_add(len);
        } else {
            sec.data.resize(sec.data.len().saturating_add(len as usize), 0);
        }
        offset
    }

    /// Pads a section with `fill` bytes to a multiple of `align`, a power
    /// of two, raises the section's alignment to at least `align`, and
    /// returns the section's new size. Any other `align` changes nothing
    /// and is reported by the writer as [`Error::BadAlignment`].
    pub fn align_to(&mut self, section: SectionId, align: u64, fill: u8) -> u64 {
        let sec = &mut self.sections[section.0 as usize];
        if !align.is_power_of_two() {
            let size = sec.size();
            self.defer(Error::BadAlignment { align });
            return size;
        }
        sec.align = sec.align.max(align);
        let size = sec.size();
        let padded = size.saturating_add(align - 1) & !(align - 1);
        if sec.kind.is_nobits() {
            sec.nobits_size = padded;
        } else {
            sec.data.resize(padded as usize, fill);
        }
        padded
    }

    /// Reserves room for `additional` more bytes in a section.
    pub fn reserve_data(&mut self, section: SectionId, additional: usize) {
        self.sections[section.0 as usize].data.reserve(additional);
    }

    /// A section's bytes. Empty for a section that occupies no file space.
    pub fn data(&self, section: SectionId) -> &[u8] {
        &self.sections[section.0 as usize].data
    }

    /// A section's buffer, to build or patch contents in place.
    pub fn data_mut(&mut self, section: SectionId) -> &mut Vec<u8> {
        &mut self.sections[section.0 as usize].data
    }

    // ---- symbols ----

    /// Adds a symbol.
    pub fn add_symbol(&mut self, symbol: Symbol) -> SymbolId {
        // The last index is `NONE`.
        let Some(id) = u32::try_from(self.symbols.len()).ok().filter(|&id| id != NONE) else {
            self.defer(Error::TooLarge(Limit::Symbols));
            return SymbolId(0);
        };
        if id.is_multiple_of(64) {
            self.referenced.push(0);
        }
        self.symbols.push(symbol);
        SymbolId(id)
    }

    /// How many symbols have been added.
    pub fn symbol_count(&self) -> u32 {
        self.symbols.len() as u32
    }

    /// A symbol.
    pub fn symbol(&self, symbol: SymbolId) -> &Symbol {
        &self.symbols[symbol.0 as usize]
    }

    /// A symbol, to define or change it.
    pub fn symbol_mut(&mut self, symbol: SymbolId) -> &mut Symbol {
        &mut self.symbols[symbol.0 as usize]
    }

    /// The symbol that stands for a section itself, created on the first
    /// call and reused after.
    pub fn section_symbol(&mut self, section: SectionId) -> SymbolId {
        let existing = self.sections[section.0 as usize].symbol;
        if existing != NONE {
            return SymbolId(existing);
        }
        let id = self.add_symbol(Symbol {
            name: StrId::EMPTY,
            value: 0,
            size: 0,
            place: SymbolPlace::Section(section),
            binding: Binding::Local,
            kind: SymbolKind::Section,
            visibility: Visibility::Default,
            other: 0,
        });
        self.sections[section.0 as usize].symbol = id.0;
        id
    }

    /// Adds the symbol that names the source file.
    ///
    /// # Errors
    /// As [`Self::add_string`].
    pub fn add_file_symbol(&mut self, name: &[u8]) -> Result<SymbolId, Error> {
        let name = self.add_string(name)?;
        Ok(self.add_symbol(Symbol {
            name,
            value: 0,
            size: 0,
            place: SymbolPlace::Absolute,
            binding: Binding::Local,
            kind: SymbolKind::File,
            visibility: Visibility::Default,
            other: 0,
        }))
    }

    /// Makes the writer keep an undefined symbol that nothing refers to.
    pub fn retain_symbol(&mut self, symbol: SymbolId) {
        self.mark_referenced(symbol);
    }

    fn mark_referenced(&mut self, symbol: SymbolId) {
        assert!(symbol.0 < self.symbol_count(), "symbol id not issued by this object");
        self.referenced[(symbol.0 / 64) as usize] |= 1 << (symbol.0 % 64);
    }

    /// Whether a relocation or a group refers to the symbol, or it was
    /// retained.
    pub fn is_referenced(&self, symbol: SymbolId) -> bool {
        assert!(symbol.0 < self.symbol_count(), "symbol id not issued by this object");
        self.referenced[(symbol.0 / 64) as usize] & (1 << (symbol.0 % 64)) != 0
    }

    // ---- relocations ----

    /// Adds a relocation to a section: patch the place at `offset` with a
    /// value computed, as `kind` says, from `symbol` and `addend`.
    ///
    /// A kind of another architecture than the object's is not added and is
    /// reported by the writer as [`Error::RelocationArchMismatch`].
    pub fn add_relocation(
        &mut self,
        section: SectionId,
        offset: u64,
        kind: impl Into<RelocKind>,
        symbol: SymbolId,
        addend: i64,
    ) {
        let kind = kind.into();
        if kind.arch() != self.arch {
            self.defer(Error::RelocationArchMismatch { section, kind });
            return;
        }
        // The last index is `NONE`.
        let Some(index) = u32::try_from(self.relocs.len()).ok().filter(|&i| i != NONE) else {
            self.defer(Error::TooLarge(Limit::Relocations));
            return;
        };
        self.mark_referenced(symbol);
        let sec = &mut self.sections[section.0 as usize];
        if sec.reloc_count == 0 {
            sec.reloc_head = index;
        } else {
            self.relocs[sec.reloc_tail as usize].next = index;
        }
        sec.reloc_tail = index;
        sec.reloc_count += 1;
        if offset < sec.reloc_max {
            sec.relocs_unsorted = true;
        } else {
            sec.reloc_max = offset;
        }
        self.relocs.push(RelocEntry {
            offset,
            addend,
            symbol: symbol.0,
            r_type: kind.elf_type(),
            next: NONE,
        });
    }

    /// How many relocations a section has.
    pub fn relocation_count(&self, section: SectionId) -> u32 {
        self.sections[section.0 as usize].reloc_count
    }

    /// A section's relocations, in the order they were added.
    pub fn relocations(&self, section: SectionId) -> impl Iterator<Item = Relocation> + '_ {
        let mut next = self.sections[section.0 as usize].reloc_head;
        std::iter::from_fn(move || {
            let entry = self.relocs.get(next as usize)?;
            next = entry.next;
            Some(Relocation {
                offset: entry.offset,
                kind: RelocKind::from_elf_type(self.arch, entry.r_type),
                symbol: SymbolId(entry.symbol),
                addend: entry.addend,
            })
        })
    }

    // ---- groups ----

    /// Adds an empty section group whose signature is `signature`.
    pub fn add_group(&mut self, signature: SymbolId, kind: GroupKind) -> GroupId {
        self.mark_referenced(signature);
        // The last index is `NONE`.
        let Some(id) = u32::try_from(self.groups.len()).ok().filter(|&id| id != NONE) else {
            self.defer(Error::TooLarge(Limit::Groups));
            return GroupId(0);
        };
        self.groups.push(GroupData { signature: signature.0, kind, head: NONE, tail: NONE });
        GroupId(id)
    }

    /// How many groups have been added.
    pub fn group_count(&self) -> u32 {
        self.groups.len() as u32
    }

    /// Adds a section to a group. The section's relocations belong to the
    /// group with it.
    ///
    /// # Errors
    /// [`Error::SectionAlreadyInGroup`] if the section is in a group.
    pub fn add_group_member(&mut self, group: GroupId, section: SectionId) -> Result<(), Error> {
        let grp = &mut self.groups[group.0 as usize];
        let sec = &mut self.sections[section.0 as usize];
        if sec.group != NONE {
            return Err(Error::SectionAlreadyInGroup { section });
        }
        sec.group = group.0;
        let tail = std::mem::replace(&mut grp.tail, section.0);
        if grp.head == NONE {
            grp.head = section.0;
        } else {
            self.sections[tail as usize].next_in_group = section.0;
        }
        Ok(())
    }

    /// The group a section is in.
    pub fn section_group(&self, section: SectionId) -> Option<GroupId> {
        let group = self.sections[section.0 as usize].group;
        (group != NONE).then_some(GroupId(group))
    }

    /// A group's signature symbol.
    pub fn group_signature(&self, group: GroupId) -> SymbolId {
        SymbolId(self.groups[group.0 as usize].signature)
    }

    /// A group's member sections, in the order they were added.
    pub fn group_members(&self, group: GroupId) -> impl Iterator<Item = SectionId> + '_ {
        let mut next = self.groups[group.0 as usize].head;
        std::iter::from_fn(move || {
            let section = self.sections.get(next as usize)?;
            let id = SectionId(next);
            next = section.next_in_group;
            Some(id)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reloc::{A64Reloc, X64Reloc};

    fn object() -> (Object, SectionId) {
        let mut obj = Object::new(Arch::AArch64);
        let text = obj.add_section(b".text", SectionKind::Text, 4).unwrap();
        (obj, text)
    }

    /// `spec/object.md` "Performance": the records the hot path stores.
    #[test]
    fn record_sizes() {
        assert_eq!(size_of::<Symbol>(), 32);
        assert_eq!(size_of::<RelocEntry>(), 32);
        assert_eq!(size_of::<SectionId>(), 4);
    }

    /// "Ids": ids count up from zero in creation order, per table.
    #[test]
    fn ids_are_dense_and_in_creation_order() {
        let (mut obj, text) = object();
        let data = obj.add_section(b".data", SectionKind::Data, 8).unwrap();
        assert_eq!((text.index(), data.index()), (0, 1));
        assert_eq!(SectionId::from_index(1), data);
        let first = obj.add_symbol(Symbol::undefined(StrId::EMPTY));
        let section = obj.section_symbol(data);
        let third = obj.add_symbol(Symbol::undefined(StrId::EMPTY));
        assert_eq!([first, section, third].map(SymbolId::index), [0, 1, 2]);
        assert_eq!((obj.section_count(), obj.symbol_count()), (2, 3));
        let a = obj.add_group(first, GroupKind::Comdat);
        let b = obj.add_group(first, GroupKind::Plain);
        assert_eq!((a.index(), b.index(), obj.group_count()), (0, 1, 2));
        assert_eq!(obj.arch(), Arch::AArch64);
    }

    /// "Strings".
    #[test]
    fn strings() {
        let mut obj = Object::new(Arch::X86_64);
        assert_eq!(obj.add_string(b""), Ok(StrId::EMPTY));
        let main = obj.add_string(b"main").unwrap();
        let again = obj.add_string(b"main").unwrap();
        assert_ne!(main, again, "equal names are not merged");
        assert_eq!(obj.string(main), b"main");
        assert_eq!(obj.string(again), b"main");
        assert_eq!(obj.string(StrId::EMPTY), b"");
        assert_eq!(obj.string(StrId(1000)), b"");
        assert_eq!(obj.add_string(b"a\0"), Err(Error::NulInName));
        assert_eq!(obj.strings, b"\0main\0main\0");
    }

    /// "Section contents": appending returns offsets and keeps sizes.
    #[test]
    fn append_returns_offsets() {
        let (mut obj, text) = object();
        assert_eq!(obj.section_size(text), 0);
        assert_eq!(obj.append(text, &[1, 2, 3]), 0);
        assert_eq!(obj.append(text, &[4]), 3);
        assert_eq!(obj.append_zeros(text, 2), 4);
        assert_eq!(obj.append_owned(text, vec![7, 8]), 6);
        assert_eq!(obj.data(text), [1, 2, 3, 4, 0, 0, 7, 8]);
        assert_eq!(obj.section_size(text), 8);
        obj.data_mut(text)[0] = 9;
        obj.reserve_data(text, 100);
        assert_eq!(obj.data(text)[0], 9);
        assert!(obj.data_mut(text).capacity() >= 108);
    }

    /// "Section contents": an empty section takes a buffer over as it is.
    #[test]
    fn append_owned_does_not_copy_into_an_empty_section() {
        let (mut obj, text) = object();
        let buffer = vec![0xaau8; 4096];
        let address = buffer.as_ptr();
        assert_eq!(obj.append_owned(text, buffer), 0);
        assert_eq!(obj.data(text).as_ptr(), address);
        assert_eq!(obj.section_size(text), 4096);
    }

    /// "Section contents": padding, and the alignment it implies.
    #[test]
    fn align_to_pads_and_raises_alignment() {
        let (mut obj, text) = object();
        assert_eq!(obj.align_to(text, 16, 0xcc), 0);
        assert_eq!(obj.section_align(text), 16);
        obj.append(text, &[1, 2, 3]);
        assert_eq!(obj.align_to(text, 8, 0xcc), 8);
        assert_eq!(obj.data(text), [1, 2, 3, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc]);
        assert_eq!(obj.align_to(text, 8, 0), 8);
        assert_eq!(obj.section_align(text), 16, "alignment never drops");
        obj.raise_section_align(text, 4);
        assert_eq!(obj.section_align(text), 16);
        obj.raise_section_align(text, 64);
        assert_eq!(obj.section_align(text), 64);
        // A bad alignment changes nothing and is reported at write time.
        assert_eq!(obj.align_to(text, 3, 0), 8);
        assert_eq!(obj.section_align(text), 64);
        assert_eq!(obj.deferred, Some(Error::BadAlignment { align: 3 }));
    }

    /// "Section contents": a section without file space has a size only.
    #[test]
    fn nobits_sections_track_size_only() {
        let mut obj = Object::new(Arch::AArch64);
        let bss = obj.add_section(b".bss", SectionKind::Bss, 8).unwrap();
        assert_eq!(obj.append_zeros(bss, 5), 0);
        assert_eq!(obj.align_to(bss, 16, 0xff), 16);
        assert_eq!(obj.append_zeros(bss, 1 << 40), 16);
        assert_eq!(obj.section_size(bss), (1 << 40) + 16);
        assert!(obj.data(bss).is_empty());
        assert_eq!(obj.section_align(bss), 16);
        assert!(SectionKind::TlsBss.is_nobits() && !SectionKind::Tls.is_nobits());
    }

    /// "Symbols": a section symbol is made once; symbols are plain data.
    #[test]
    fn symbols() {
        let (mut obj, text) = object();
        let name = obj.add_string(b"f").unwrap();
        let id = obj.add_symbol(Symbol::undefined(name));
        assert_eq!(obj.symbol(id).place, SymbolPlace::Undefined);
        *obj.symbol_mut(id) = Symbol::defined(name, SymbolKind::Func, text, 4, 8)
            .with_binding(Binding::Weak)
            .with_visibility(Visibility::Hidden);
        let symbol = obj.symbol(id);
        assert_eq!((symbol.value, symbol.size), (4, 8));
        assert_eq!((symbol.binding, symbol.visibility), (Binding::Weak, Visibility::Hidden));
        let section = obj.section_symbol(text);
        assert_eq!(obj.section_symbol(text), section);
        let record = obj.symbol(section);
        assert_eq!((record.kind, record.binding), (SymbolKind::Section, Binding::Local));
        assert_eq!((record.place, record.name), (SymbolPlace::Section(text), StrId::EMPTY));
        let file = obj.add_file_symbol(b"a.cc").unwrap();
        let record = obj.symbol(file);
        assert_eq!((record.kind, record.place), (SymbolKind::File, SymbolPlace::Absolute));
        assert_eq!(obj.string(record.name), b"a.cc");
    }

    /// "Relocations": each section keeps its relocations in the order
    /// added, however additions to different sections interleave, and a
    /// relocation marks its symbol as referenced.
    #[test]
    fn relocations_chain_per_section() {
        let (mut obj, text) = object();
        let data = obj.add_section(b".data", SectionKind::Data, 8).unwrap();
        let symbols: Vec<_> =
            (0..130).map(|_| obj.add_symbol(Symbol::undefined(StrId::EMPTY))).collect();
        obj.add_relocation(text, 8, A64Reloc::Call26, symbols[0], 0);
        obj.add_relocation(data, 0, A64Reloc::Abs64, symbols[64], -1);
        obj.add_relocation(text, 4, A64Reloc::Other(999), symbols[129], 7);
        assert_eq!((obj.relocation_count(text), obj.relocation_count(data)), (2, 1));
        let text_relocs: Vec<_> = obj.relocations(text).collect();
        assert_eq!(
            text_relocs,
            [
                Relocation {
                    offset: 8,
                    kind: A64Reloc::Call26.into(),
                    symbol: symbols[0],
                    addend: 0
                },
                Relocation {
                    offset: 4,
                    kind: A64Reloc::Other(999).into(),
                    symbol: symbols[129],
                    addend: 7
                },
            ]
        );
        assert_eq!(obj.relocations(data).count(), 1);
        for (index, &symbol) in symbols.iter().enumerate() {
            assert_eq!(obj.is_referenced(symbol), [0, 64, 129].contains(&index), "{index}");
        }
        assert!(obj.sections[text.0 as usize].relocs_unsorted);
        assert!(!obj.sections[data.0 as usize].relocs_unsorted);
    }

    /// "Relocations": a kind of the other architecture is not added.
    #[test]
    fn relocation_of_another_architecture_is_deferred() {
        let (mut obj, text) = object();
        let symbol = obj.add_symbol(Symbol::undefined(StrId::EMPTY));
        obj.add_relocation(text, 0, X64Reloc::Pc32, symbol, 0);
        assert_eq!(obj.relocation_count(text), 0);
        assert!(!obj.is_referenced(symbol));
        assert_eq!(
            obj.deferred,
            Some(Error::RelocationArchMismatch { section: text, kind: X64Reloc::Pc32.into() })
        );
        // The first deferred error is the one reported.
        obj.align_to(text, 3, 0);
        assert!(matches!(obj.deferred, Some(Error::RelocationArchMismatch { .. })));
    }

    /// "Section groups": members in the order added; a section joins one group.
    #[test]
    fn groups() {
        let (mut obj, text) = object();
        let data = obj.add_section(b".data", SectionKind::Data, 8).unwrap();
        let other = obj.add_section(b".text", SectionKind::Text, 4).unwrap();
        let signature = obj.add_symbol(Symbol::undefined(StrId::EMPTY));
        let group = obj.add_group(signature, GroupKind::Comdat);
        assert!(obj.is_referenced(signature));
        assert_eq!(obj.group_members(group).count(), 0);
        obj.add_group_member(group, data).unwrap();
        obj.add_group_member(group, text).unwrap();
        assert_eq!(obj.group_members(group).collect::<Vec<_>>(), [data, text]);
        assert_eq!(obj.section_group(text), Some(group));
        assert_eq!(obj.section_group(other), None);
        assert_eq!(obj.group_signature(group), signature);
        let second = obj.add_group(signature, GroupKind::Plain);
        assert_eq!(
            obj.add_group_member(second, text),
            Err(Error::SectionAlreadyInGroup { section: text })
        );
        assert_eq!(obj.group_members(second).count(), 0);
    }

    #[test]
    #[should_panic(expected = "symbol id not issued by this object")]
    fn a_foreign_symbol_id_panics() {
        let (mut obj, text) = object();
        obj.add_relocation(text, 0, A64Reloc::Call26, SymbolId::from_index(0), 0);
    }
}
