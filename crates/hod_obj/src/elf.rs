//! The ELF64 little-endian relocatable writer. Contract: `spec/object.md`
//! "ELF writer".
//!
//! [`write`] and [`to_vec`] first check the object and compute the whole
//! file layout ([`Layout`]), then emit the file front to back. Nothing is
//! emitted for an object that fails the checks.

mod conventions;

use std::convert::Infallible;
use std::io::Write;

pub use conventions::{
    DEFAULT_INIT_PRIORITY, GNU_PROPERTY_AARCH64_FEATURE_1_BTI, GNU_PROPERTY_AARCH64_FEATURE_1_GCS,
    GNU_PROPERTY_AARCH64_FEATURE_1_PAC, GNU_PROPERTY_X86_FEATURE_1_IBT,
    GNU_PROPERTY_X86_FEATURE_1_SHSTK, add_fini_array, add_gnu_property_note, add_init_array,
    gnu_property_note,
};

use crate::error::{Error, Limit, SymbolProblem, WriteError};
use crate::model::{
    Arch, Binding, GroupKind, NONE, Object, RelocEntry, SectionData, SectionId, SectionKind,
    StackNote, Symbol, SymbolId, SymbolKind, SymbolPlace, Visibility,
};

// Section types (`sh_type`), ELF gABI "Sections".
/// Inactive section header.
pub const SHT_NULL: u32 = 0;
/// Contents defined by the program.
pub const SHT_PROGBITS: u32 = 1;
/// The symbol table.
pub const SHT_SYMTAB: u32 = 2;
/// A string table.
pub const SHT_STRTAB: u32 = 3;
/// Relocations with addends.
pub const SHT_RELA: u32 = 4;
/// A symbol hash table.
pub const SHT_HASH: u32 = 5;
/// Dynamic linking information.
pub const SHT_DYNAMIC: u32 = 6;
/// Notes.
pub const SHT_NOTE: u32 = 7;
/// Occupies no file space.
pub const SHT_NOBITS: u32 = 8;
/// Relocations without addends.
pub const SHT_REL: u32 = 9;
/// The dynamic symbol table.
pub const SHT_DYNSYM: u32 = 11;
/// Pointers to initialization functions.
pub const SHT_INIT_ARRAY: u32 = 14;
/// Pointers to termination functions.
pub const SHT_FINI_ARRAY: u32 = 15;
/// Pointers to pre-initialization functions.
pub const SHT_PREINIT_ARRAY: u32 = 16;
/// A section group.
pub const SHT_GROUP: u32 = 17;
/// Section indices of symbols whose `st_shndx` is `SHN_XINDEX`.
pub const SHT_SYMTAB_SHNDX: u32 = 18;
/// Unwind tables (x86-64 psABI): the type of `.eh_frame` on x86-64.
pub const SHT_X86_64_UNWIND: u32 = 0x7000_0001;

// Section flags (`sh_flags`), ELF gABI "Sections".
/// Writable at run time.
pub const SHF_WRITE: u64 = 0x1;
/// Occupies memory at run time.
pub const SHF_ALLOC: u64 = 0x2;
/// Executable instructions.
pub const SHF_EXECINSTR: u64 = 0x4;
/// May be merged to remove duplicates.
pub const SHF_MERGE: u64 = 0x10;
/// NUL-terminated strings.
pub const SHF_STRINGS: u64 = 0x20;
/// `sh_info` holds a section index.
pub const SHF_INFO_LINK: u64 = 0x40;
/// Ordered after the section `sh_link` names.
pub const SHF_LINK_ORDER: u64 = 0x80;
/// Member of a section group.
pub const SHF_GROUP: u64 = 0x200;
/// Thread-local storage.
pub const SHF_TLS: u64 = 0x400;
/// Compressed contents.
pub const SHF_COMPRESSED: u64 = 0x800;
/// Not garbage-collected by the linker (GNU extension).
pub const SHF_GNU_RETAIN: u64 = 0x20_0000;
/// Not part of an executable or shared object.
pub const SHF_EXCLUDE: u64 = 0x8000_0000;
/// May be placed beyond 2 GiB (x86-64 psABI).
pub const SHF_X86_64_LARGE: u64 = 0x1000_0000;

/// `st_other` bit of a function that does not follow the base procedure
/// call standard ("ELF for the Arm 64-bit Architecture", "Symbol Table").
pub const STO_AARCH64_VARIANT_PCS: u8 = 0x80;

/// The first reserved section index. Section counts and indices at or above
/// it are stored through the escapes of ELF gABI "ELF Header" and
/// "Symbol Table".
const SHN_LORESERVE: u32 = 0xff00;
const SHN_ABS: u16 = 0xfff1;
const SHN_COMMON: u16 = 0xfff2;
const SHN_XINDEX: u16 = 0xffff;
const GRP_COMDAT: u32 = 1;
const ELFOSABI_GNU: u8 = 3;
const EM_X86_64: u16 = 62;
const EM_AARCH64: u16 = 183;

const EHDR_SIZE: u64 = 64;
const SHDR_SIZE: u64 = 64;
const SYM_SIZE: u64 = 24;
const RELA_SIZE: u64 = 24;

/// Offsets of the writer's own names at the start of `.shstrtab`.
const SHSTRTAB_HEAD: &[u8] = b"\0.symtab\0.strtab\0.shstrtab\0";
const NAME_SYMTAB: u32 = 1;
const NAME_STRTAB: u32 = 9;
const NAME_SHSTRTAB: u32 = 17;
const RELA_PREFIX: &[u8] = b".rela";

/// Rejects a section kind the writer cannot honor.
pub(crate) fn check_kind(kind: SectionKind) -> Result<(), Error> {
    match kind {
        SectionKind::MergeableStrings { char_size: 0 }
        | SectionKind::MergeableConstants { entry_size: 0 } => {
            Err(Error::BadEntrySize { section: None })
        }
        SectionKind::Elf {
            sh_type:
                sh_type @ (SHT_NULL | SHT_SYMTAB | SHT_RELA | SHT_HASH | SHT_DYNAMIC | SHT_REL
                | SHT_DYNSYM | SHT_GROUP | SHT_SYMTAB_SHNDX),
            ..
        } => Err(Error::ReservedSectionType { sh_type }),
        SectionKind::Elf { sh_flags, .. }
            if sh_flags & (SHF_GROUP | SHF_INFO_LINK | SHF_LINK_ORDER) != 0 =>
        {
            Err(Error::ReservedSectionFlags {
                sh_flags: sh_flags & (SHF_GROUP | SHF_INFO_LINK | SHF_LINK_ORDER),
            })
        }
        _ => Ok(()),
    }
}

/// `(sh_type, sh_flags, sh_entsize)` of a section kind.
const fn header_fields(kind: SectionKind) -> (u32, u64, u64) {
    match kind {
        SectionKind::Text => (SHT_PROGBITS, SHF_ALLOC | SHF_EXECINSTR, 0),
        SectionKind::Data => (SHT_PROGBITS, SHF_ALLOC | SHF_WRITE, 0),
        SectionKind::ReadOnlyData => (SHT_PROGBITS, SHF_ALLOC, 0),
        SectionKind::Bss => (SHT_NOBITS, SHF_ALLOC | SHF_WRITE, 0),
        SectionKind::Tls => (SHT_PROGBITS, SHF_ALLOC | SHF_WRITE | SHF_TLS, 0),
        SectionKind::TlsBss => (SHT_NOBITS, SHF_ALLOC | SHF_WRITE | SHF_TLS, 0),
        SectionKind::InitArray => (SHT_INIT_ARRAY, SHF_ALLOC | SHF_WRITE, 0),
        SectionKind::FiniArray => (SHT_FINI_ARRAY, SHF_ALLOC | SHF_WRITE, 0),
        SectionKind::Note => (SHT_NOTE, SHF_ALLOC, 0),
        SectionKind::Debug => (SHT_PROGBITS, 0, 0),
        SectionKind::MergeableStrings { char_size } => {
            (SHT_PROGBITS, SHF_ALLOC | SHF_MERGE | SHF_STRINGS, char_size as u64)
        }
        SectionKind::MergeableConstants { entry_size } => {
            (SHT_PROGBITS, SHF_ALLOC | SHF_MERGE, entry_size as u64)
        }
        SectionKind::Elf { sh_type, sh_flags, sh_entsize } => (sh_type, sh_flags, sh_entsize),
    }
}

/// The part of the symbol table a symbol is written in, in file order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    File,
    Local,
    Global,
}

const CLASSES: [Class; 3] = [Class::File, Class::Local, Class::Global];

fn class_of(symbol: &Symbol) -> Class {
    match (symbol.binding, symbol.kind) {
        (Binding::Local, SymbolKind::File) => Class::File,
        (Binding::Local, _) => Class::Local,
        _ => Class::Global,
    }
}

/// Section contents: where they go, and which sections have relocations.
#[derive(Debug)]
struct SectionPlan {
    /// File offset of each section's contents.
    offsets: Vec<u64>,
    /// ELF index of each section's relocation section; zero when it has no
    /// relocations.
    rela_index: Vec<u32>,
    /// Per distinct section name: whether a section of that name has
    /// relocations, so that `.rela` is emitted in front of the name.
    name_has_rela: Vec<bool>,
    rela_count: u64,
    reloc_total: u64,
    /// File offset just after the last section's contents.
    end: u64,
    uses_gnu_abi: bool,
}

/// The symbol table: which symbols are written, and in what order.
#[derive(Debug)]
struct SymbolPlan {
    /// `.symtab` index of each symbol; zero when it is omitted.
    index: Vec<u32>,
    /// Entries in `.symtab`, with the null symbol.
    count: u32,
    /// `.symtab` index of the first symbol that is not local.
    first_global: u32,
    /// Whether some symbol's section index needs `.symtab_shndx`.
    need_shndx: bool,
    uses_gnu_abi: bool,
}

/// The section name string table.
#[derive(Debug)]
struct NamePlan {
    /// The names the writer adds, before the sections' own.
    head: Vec<u8>,
    shndx: u32,
    group: u32,
    stack: u32,
    /// Offset in `.shstrtab` of each distinct section name.
    offsets: Vec<u32>,
    size: u64,
}

/// Everything about the file that is decided before the first byte is
/// emitted.
#[derive(Debug)]
struct Layout {
    os_abi: u8,
    /// ELF index of `SectionId(0)`; the groups come before it.
    first_section: u32,
    /// ELF index of `.note.GNU-stack`, when it is emitted.
    stack_index: Option<u32>,
    symtab_index: u32,
    /// ELF index of `.symtab_shndx`, when it is emitted.
    shndx_index: Option<u32>,
    strtab_index: u32,
    shstrtab_index: u32,
    shnum: u32,
    sections: SectionPlan,
    symbols: SymbolPlan,
    names: NamePlan,
    groups_off: u64,
    relas_off: u64,
    symtab_off: u64,
    shndx_off: u64,
    strtab_off: u64,
    shstrtab_off: u64,
    shdr_off: u64,
    file_size: u64,
}

/// `value` rounded up to a multiple of `align`; an error when the file
/// would pass 2^64 bytes.
fn align_up(value: u64, align: u64) -> Result<u64, Error> {
    value.checked_next_multiple_of(align).ok_or(Error::TooLarge(Limit::FileBytes))
}

/// Checks the sections and places their contents after the file header.
/// `first_rela` is the ELF index the first relocation section will have.
fn plan_sections(obj: &Object, first_rela: u64) -> Result<SectionPlan, Error> {
    let mut plan = SectionPlan {
        offsets: Vec::with_capacity(obj.sections.len()),
        rela_index: Vec::with_capacity(obj.sections.len()),
        name_has_rela: vec![false; obj.section_names.len()],
        rela_count: 0,
        reloc_total: 0,
        end: EHDR_SIZE,
        uses_gnu_abi: false,
    };
    for (index, sec) in obj.sections.iter().enumerate() {
        check_section(SectionId(index as u32), sec)?;
        if sec.reloc_count == 0 {
            plan.rela_index.push(0);
        } else {
            plan.rela_index.push((first_rela + plan.rela_count) as u32);
            plan.rela_count += 1;
            plan.reloc_total += u64::from(sec.reloc_count);
            plan.name_has_rela[sec.name as usize] = true;
        }
        if let SectionKind::Elf { sh_flags, .. } = sec.kind {
            plan.uses_gnu_abi |= sh_flags & SHF_GNU_RETAIN != 0;
        }
        // A section without file space is not aligned in the file: its
        // offset only says where it would be.
        if !sec.kind.is_nobits() {
            plan.end = align_up(plan.end, sec.align())?;
        }
        plan.offsets.push(plan.end);
        plan.end =
            plan.end.checked_add(sec.data.len() as u64).ok_or(Error::TooLarge(Limit::FileBytes))?;
    }
    Ok(plan)
}

/// Checks the symbols that will be written and gives each its `.symtab`
/// index: file symbols, the other locals, then the rest, each in id order.
/// `first_section` is the ELF index of `SectionId(0)`.
fn plan_symbols(obj: &Object, first_section: u64) -> Result<SymbolPlan, Error> {
    let mut counts = [0u32; 3];
    let mut need_shndx = false;
    let mut uses_gnu_abi = false;
    let mut index = vec![0u32; obj.symbols.len()];
    for (at, symbol) in obj.symbols.iter().enumerate() {
        let id = SymbolId(at as u32);
        if symbol.place == SymbolPlace::Undefined && !obj.is_referenced(id) {
            continue;
        }
        check_symbol(obj, symbol).map_err(|problem| Error::BadSymbol { symbol: id, problem })?;
        if let SymbolPlace::Section(section) = symbol.place {
            need_shndx |= first_section + u64::from(section.0) >= u64::from(SHN_LORESERVE);
        }
        uses_gnu_abi |= symbol.kind == SymbolKind::GnuIfunc;
        let class = class_of(symbol) as usize;
        counts[class] += 1;
        // Provisional: the position within the class, from one.
        index[at] = counts[class];
    }
    let emitted: u64 = counts.iter().map(|&n| u64::from(n)).sum();
    let Ok(count) = u32::try_from(emitted + 1) else {
        return Err(Error::TooLarge(Limit::Symbols));
    };
    let bases = [0, counts[0], counts[0] + counts[1]];
    for (slot, symbol) in index.iter_mut().zip(&obj.symbols) {
        if *slot != 0 {
            *slot += bases[class_of(symbol) as usize];
        }
    }
    Ok(SymbolPlan {
        index,
        count,
        first_global: 1 + counts[0] + counts[1],
        need_shndx,
        uses_gnu_abi,
    })
}

/// Lays out `.shstrtab`: the writer's names, then each distinct section
/// name once, behind `.rela` when a section of that name has relocations.
fn plan_names(
    obj: &Object,
    name_has_rela: &[bool],
    need_shndx: bool,
    has_stack: bool,
) -> Result<NamePlan, Error> {
    let mut head = SHSTRTAB_HEAD.to_vec();
    let mut own_name = |wanted: bool, name: &[u8]| {
        if !wanted {
            return 0;
        }
        let offset = head.len() as u32;
        head.extend_from_slice(name);
        head.push(0);
        offset
    };
    let shndx = own_name(need_shndx, b".symtab_shndx");
    let group = own_name(!obj.groups.is_empty(), b".group");
    let stack = own_name(has_stack, b".note.GNU-stack");
    let mut offsets = Vec::with_capacity(obj.section_names.len());
    let mut size = head.len() as u64;
    for (name, &has_rela) in obj.section_names.iter().zip(name_has_rela) {
        if has_rela {
            size += RELA_PREFIX.len() as u64;
        }
        offsets.push(size as u32);
        size += name.len() as u64 + 1;
    }
    if size > u64::from(u32::MAX) {
        return Err(Error::TooLarge(Limit::SectionNameBytes));
    }
    Ok(NamePlan { head, shndx, group, stack, offsets, size })
}

impl Layout {
    fn new(obj: &Object) -> Result<Self, Error> {
        if let Some(error) = obj.deferred {
            return Err(error);
        }
        // Section header order: null, groups, sections, the stack note,
        // relocation sections, `.symtab`, `.symtab_shndx`, `.strtab`,
        // `.shstrtab`.
        let first_section = 1 + obj.groups.len() as u64;
        let has_stack = obj.stack_note != StackNote::Omit;
        let stack_index = first_section + obj.sections.len() as u64;
        let first_rela = stack_index + u64::from(has_stack);
        let sections = plan_sections(obj, first_rela)?;
        let symtab_index = first_rela + sections.rela_count;
        let symbols = plan_symbols(obj, first_section)?;
        let shnum = symtab_index + 3 + u64::from(symbols.need_shndx);
        let Ok(shnum) = u32::try_from(shnum) else {
            return Err(Error::TooLarge(Limit::Sections));
        };
        let names = plan_names(obj, &sections.name_has_rela, symbols.need_shndx, has_stack)?;

        // One word of flags per group, then each member and its relocations.
        let group_words: u64 =
            (0..obj.groups.len()).map(|group| group_len(obj, &sections.rela_index, group)).sum();

        // File order after the sections' contents: groups, relocations,
        // `.symtab`, `.symtab_shndx`, `.strtab`, `.shstrtab`, headers.
        let symbol_count = u64::from(symbols.count);
        let groups_off = align_up(sections.end, 4)?;
        let relas_off = align_up(groups_off + 4 * group_words, 8)?;
        let symtab_off = relas_off + RELA_SIZE * sections.reloc_total;
        let shndx_off = symtab_off + SYM_SIZE * symbol_count;
        let strtab_off = shndx_off + if symbols.need_shndx { 4 * symbol_count } else { 0 };
        let shstrtab_off = strtab_off + obj.strings.len() as u64;
        let shdr_off = align_up(shstrtab_off + names.size, 8)?;

        Ok(Self {
            os_abi: if sections.uses_gnu_abi || symbols.uses_gnu_abi { ELFOSABI_GNU } else { 0 },
            first_section: first_section as u32,
            stack_index: has_stack.then_some(stack_index as u32),
            symtab_index: symtab_index as u32,
            shndx_index: symbols.need_shndx.then_some(symtab_index as u32 + 1),
            strtab_index: shnum - 2,
            shstrtab_index: shnum - 1,
            shnum,
            sections,
            symbols,
            names,
            groups_off,
            relas_off,
            symtab_off,
            shndx_off,
            strtab_off,
            shstrtab_off,
            shdr_off,
            file_size: shdr_off + SHDR_SIZE * u64::from(shnum),
        })
    }

    /// The ELF section index of a section.
    fn section_index(&self, section: u32) -> u32 {
        self.first_section + section
    }

    /// `st_shndx` of a symbol, and its `.symtab_shndx` entry.
    fn shndx(&self, place: SymbolPlace) -> (u16, u32) {
        match place {
            SymbolPlace::Undefined => (0, 0),
            SymbolPlace::Absolute => (SHN_ABS, 0),
            SymbolPlace::Common => (SHN_COMMON, 0),
            SymbolPlace::Section(section) => {
                let index = self.section_index(section.0);
                if index >= SHN_LORESERVE { (SHN_XINDEX, index) } else { (index as u16, 0) }
            }
        }
    }
}

fn check_section(section: SectionId, sec: &SectionData) -> Result<(), Error> {
    if sec.kind.is_nobits() {
        if !sec.data.is_empty() {
            return Err(Error::DataInNobits { section });
        }
        if sec.reloc_count != 0 {
            return Err(Error::RelocationInNobits { section });
        }
    }
    if sec.reloc_count != 0 && sec.reloc_max > sec.size() {
        return Err(Error::RelocationOutOfBounds { section, offset: sec.reloc_max });
    }
    let entry = match sec.kind {
        SectionKind::MergeableStrings { char_size } => u64::from(char_size),
        SectionKind::MergeableConstants { entry_size } => u64::from(entry_size),
        _ => 1,
    };
    if !sec.size().is_multiple_of(entry) {
        return Err(Error::BadEntrySize { section: Some(section) });
    }
    Ok(())
}

fn check_symbol(obj: &Object, symbol: &Symbol) -> Result<(), SymbolProblem> {
    // A name is the start of a string: offset zero, or just after a NUL.
    let name = symbol.name.0 as usize;
    if name != 0 && obj.strings.get(name - 1..=name).is_none_or(|pair| pair[0] != 0) {
        return Err(SymbolProblem::UnknownName);
    }
    let in_section = match symbol.place {
        SymbolPlace::Section(section) => {
            if section.0 as usize >= obj.sections.len() {
                return Err(SymbolProblem::UnknownSection);
            }
            true
        }
        SymbolPlace::Undefined if symbol.binding == Binding::Local => {
            return Err(SymbolProblem::UndefinedLocal);
        }
        _ => false,
    };
    let local = symbol.binding == Binding::Local;
    match symbol.kind {
        SymbolKind::Section if !(local && in_section) => {
            return Err(SymbolProblem::SectionSymbolShape);
        }
        SymbolKind::File if !(local && symbol.place == SymbolPlace::Absolute) => {
            return Err(SymbolProblem::FileSymbolShape);
        }
        _ => {}
    }
    if symbol.other & 3 != 0 {
        return Err(SymbolProblem::OtherOverlapsVisibility);
    }
    Ok(())
}

/// The number of 32-bit words in a group's section.
fn group_len(obj: &Object, rela_index: &[u32], group: usize) -> u64 {
    let mut words = 1;
    let mut member = obj.groups[group].head;
    while member != NONE {
        words += 1 + u64::from(rela_index[member as usize] != 0);
        member = obj.sections[member as usize].next_in_group;
    }
    words
}

/// Where the file goes.
trait Sink {
    type Error;
    fn put(&mut self, bytes: &[u8]) -> Result<(), Self::Error>;
    /// Bytes put so far.
    fn position(&self) -> u64;

    /// Writes zeros up to file offset `offset`.
    fn pad_to(&mut self, offset: u64) -> Result<(), Self::Error> {
        static ZEROS: [u8; 4096] = [0; 4096];
        debug_assert!(offset >= self.position(), "layout: a region starts inside the previous one");
        let mut gap = offset.saturating_sub(self.position());
        while gap != 0 {
            let step = gap.min(ZEROS.len() as u64);
            self.put(&ZEROS[..step as usize])?;
            gap -= step;
        }
        Ok(())
    }
}

/// A vector sized for the whole file.
struct VecSink(Vec<u8>);

impl Sink for VecSink {
    type Error = Infallible;

    fn put(&mut self, bytes: &[u8]) -> Result<(), Infallible> {
        self.0.extend_from_slice(bytes);
        Ok(())
    }

    fn position(&self) -> u64 {
        self.0.len() as u64
    }
}

/// A writer behind a fixed buffer, so that the many small records of an
/// object do not each become a `write` call. Pieces at least as large as
/// the buffer go to the writer directly.
struct Buffered<'a, W: Write> {
    out: &'a mut W,
    buffer: Vec<u8>,
    position: u64,
}

const BUFFER_SIZE: usize = 64 * 1024;

impl<W: Write> Buffered<'_, W> {
    fn flush_buffer(&mut self) -> std::io::Result<()> {
        self.out.write_all(&self.buffer)?;
        self.buffer.clear();
        Ok(())
    }
}

impl<W: Write> Sink for Buffered<'_, W> {
    type Error = std::io::Error;

    fn put(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.position += bytes.len() as u64;
        if bytes.len() >= BUFFER_SIZE {
            self.flush_buffer()?;
            return self.out.write_all(bytes);
        }
        if self.buffer.len() + bytes.len() > BUFFER_SIZE {
            self.flush_buffer()?;
        }
        self.buffer.extend_from_slice(bytes);
        Ok(())
    }

    fn position(&self) -> u64 {
        self.position
    }
}

/// A fixed-size little-endian record under construction.
struct Record<const N: usize> {
    bytes: [u8; N],
    at: usize,
}

impl<const N: usize> Record<N> {
    const fn new() -> Self {
        Self { bytes: [0; N], at: 0 }
    }

    fn raw<const M: usize>(mut self, bytes: [u8; M]) -> Self {
        self.bytes[self.at..self.at + M].copy_from_slice(&bytes);
        self.at += M;
        self
    }

    fn u8(self, value: u8) -> Self {
        self.raw([value])
    }

    fn u16(self, value: u16) -> Self {
        self.raw(value.to_le_bytes())
    }

    fn u32(self, value: u32) -> Self {
        self.raw(value.to_le_bytes())
    }

    fn u64(self, value: u64) -> Self {
        self.raw(value.to_le_bytes())
    }

    fn done(self) -> [u8; N] {
        debug_assert!(self.at == N, "record: fields do not fill the record");
        self.bytes
    }
}

/// One section header.
#[derive(Clone, Copy, Debug, Default)]
struct Shdr {
    name: u32,
    kind: u32,
    flags: u64,
    offset: u64,
    size: u64,
    link: u32,
    info: u32,
    align: u64,
    entsize: u64,
}

impl Shdr {
    fn encode(self) -> [u8; 64] {
        Record::new()
            .u32(self.name)
            .u32(self.kind)
            .u64(self.flags)
            .u64(0) // sh_addr
            .u64(self.offset)
            .u64(self.size)
            .u32(self.link)
            .u32(self.info)
            .u64(self.align)
            .u64(self.entsize)
            .done()
    }
}

fn file_header(obj: &Object, layout: &Layout) -> [u8; 64] {
    let machine = match obj.arch {
        Arch::AArch64 => EM_AARCH64,
        Arch::X86_64 => EM_X86_64,
    };
    // ELF gABI "ELF Header": counts and indices that do not fit sixteen
    // bits are stored in section header zero.
    let escaped = |value: u32, escape: u16| {
        if value >= SHN_LORESERVE { escape } else { value as u16 }
    };
    Record::new()
        .raw(*b"\x7fELF")
        .u8(2) // ELFCLASS64
        .u8(1) // ELFDATA2LSB
        .u8(1) // EV_CURRENT
        .u8(layout.os_abi)
        .raw([0; 8]) // ABI version, padding
        .u16(1) // ET_REL
        .u16(machine)
        .u32(1) // EV_CURRENT
        .u64(0) // e_entry
        .u64(0) // e_phoff
        .u64(layout.shdr_off)
        .u32(obj.flags)
        .u16(EHDR_SIZE as u16)
        .u16(0) // e_phentsize
        .u16(0) // e_phnum
        .u16(SHDR_SIZE as u16)
        .u16(escaped(layout.shnum, 0))
        .u16(escaped(layout.shstrtab_index, SHN_XINDEX))
        .done()
}

fn symbol_record(layout: &Layout, symbol: &Symbol) -> [u8; 24] {
    let binding = match symbol.binding {
        Binding::Local => 0,
        Binding::Global => 1,
        Binding::Weak => 2,
    };
    let kind = match symbol.kind {
        SymbolKind::NoType => 0,
        SymbolKind::Object => 1,
        SymbolKind::Func => 2,
        SymbolKind::Section => 3,
        SymbolKind::File => 4,
        SymbolKind::Tls => 6,
        SymbolKind::GnuIfunc => 10,
    };
    let visibility = match symbol.visibility {
        Visibility::Default => 0,
        Visibility::Hidden => 2,
        Visibility::Protected => 3,
    };
    Record::new()
        .u32(symbol.name.0)
        .u8(binding << 4 | kind)
        .u8(symbol.other | visibility)
        .u16(layout.shndx(symbol.place).0)
        .u64(symbol.value)
        .u64(symbol.size)
        .done()
}

fn rela_record(layout: &Layout, entry: &RelocEntry) -> [u8; 24] {
    let symbol = layout.symbols.index[entry.symbol as usize];
    debug_assert!(symbol != 0, "layout: a referenced symbol was omitted");
    Record::new()
        .u64(entry.offset)
        .u64(u64::from(symbol) << 32 | u64::from(entry.r_type))
        .u64(entry.addend as u64)
        .done()
}

/// Calls `each` for every emitted symbol in `.symtab` order: file symbols,
/// the other locals, then the rest, each in id order.
fn symbols_in_order<E>(
    obj: &Object,
    layout: &Layout,
    mut each: impl FnMut(&Symbol) -> Result<(), E>,
) -> Result<(), E> {
    for class in CLASSES {
        for (symbol, &index) in obj.symbols.iter().zip(&layout.symbols.index) {
            if index != 0 && class_of(symbol) == class {
                each(symbol)?;
            }
        }
    }
    Ok(())
}

fn emit<S: Sink>(obj: &Object, layout: &Layout, out: &mut S) -> Result<(), S::Error> {
    out.put(&file_header(obj, layout))?;

    // Section contents.
    for (sec, &offset) in obj.sections.iter().zip(&layout.sections.offsets) {
        if !sec.data.is_empty() {
            out.pad_to(offset)?;
            out.put(&sec.data)?;
        }
    }

    // Group sections.
    out.pad_to(layout.groups_off)?;
    for group in &obj.groups {
        let flags = match group.kind {
            GroupKind::Comdat => GRP_COMDAT,
            GroupKind::Plain => 0,
        };
        out.put(&flags.to_le_bytes())?;
        let mut member = group.head;
        while member != NONE {
            out.put(&layout.section_index(member).to_le_bytes())?;
            let rela = layout.sections.rela_index[member as usize];
            if rela != 0 {
                out.put(&rela.to_le_bytes())?;
            }
            member = obj.sections[member as usize].next_in_group;
        }
    }

    // Relocation sections, each in increasing offset order.
    out.pad_to(layout.relas_off)?;
    let mut order: Vec<u32> = Vec::new();
    for sec in &obj.sections {
        let mut next = sec.reloc_head;
        if sec.relocs_unsorted {
            order.clear();
            for _ in 0..sec.reloc_count {
                order.push(next);
                next = obj.relocs[next as usize].next;
            }
            order.sort_by_key(|&index| obj.relocs[index as usize].offset);
            for &index in &order {
                out.put(&rela_record(layout, &obj.relocs[index as usize]))?;
            }
        } else {
            for _ in 0..sec.reloc_count {
                let entry = &obj.relocs[next as usize];
                out.put(&rela_record(layout, entry))?;
                next = entry.next;
            }
        }
        debug_assert!(next == NONE, "model: relocation chain and count disagree");
    }

    // `.symtab`, `.symtab_shndx`, `.strtab`, `.shstrtab`.
    debug_assert!(out.position() == layout.symtab_off, "layout: relocation sizes");
    out.put(&[0; 24])?;
    symbols_in_order(obj, layout, |symbol| out.put(&symbol_record(layout, symbol)))?;
    if layout.shndx_index.is_some() {
        debug_assert!(out.position() == layout.shndx_off, "layout: symbol table size");
        out.put(&[0; 4])?;
        symbols_in_order(obj, layout, |symbol| {
            out.put(&layout.shndx(symbol.place).1.to_le_bytes())
        })?;
    }
    debug_assert!(out.position() == layout.strtab_off, "layout: symbol table size");
    out.put(&obj.strings)?;
    debug_assert!(out.position() == layout.shstrtab_off, "layout: string table size");
    out.put(&layout.names.head)?;
    for (name, &has_rela) in obj.section_names.iter().zip(&layout.sections.name_has_rela) {
        if has_rela {
            out.put(RELA_PREFIX)?;
        }
        out.put(name)?;
        out.put(&[0])?;
    }
    debug_assert!(
        out.position() == layout.shstrtab_off + layout.names.size,
        "layout: section name table size"
    );

    out.pad_to(layout.shdr_off)?;
    emit_section_headers(obj, layout, out)?;
    debug_assert!(out.position() == layout.file_size, "layout: file size");
    Ok(())
}

fn emit_section_headers<S: Sink>(
    obj: &Object,
    layout: &Layout,
    out: &mut S,
) -> Result<(), S::Error> {
    // Index zero; ELF gABI "Sections": it holds the real section count and
    // string table index when those do not fit the file header.
    let escaped = |value: u32| if value >= SHN_LORESERVE { value } else { 0 };
    let null = Shdr {
        size: u64::from(escaped(layout.shnum)),
        link: escaped(layout.shstrtab_index),
        ..Shdr::default()
    };
    out.put(&null.encode())?;

    let mut offset = layout.groups_off;
    for (index, group) in obj.groups.iter().enumerate() {
        let size = 4 * group_len(obj, &layout.sections.rela_index, index);
        let header = Shdr {
            name: layout.names.group,
            kind: SHT_GROUP,
            offset,
            size,
            link: layout.symtab_index,
            info: layout.symbols.index[group.signature as usize],
            align: 4,
            entsize: 4,
            ..Shdr::default()
        };
        out.put(&header.encode())?;
        offset += size;
    }

    let group_flag = |sec: &SectionData| if sec.group == NONE { 0 } else { SHF_GROUP };
    for (sec, &offset) in obj.sections.iter().zip(&layout.sections.offsets) {
        let (kind, flags, entsize) = header_fields(sec.kind);
        // ELF gABI "Sections": `SHF_LINK_ORDER`, with the section this one
        // is ordered by in `sh_link`.
        let (link_flag, link) = match sec.linked {
            NONE => (0, 0),
            linked => (SHF_LINK_ORDER, layout.section_index(linked)),
        };
        let header = Shdr {
            name: layout.names.offsets[sec.name as usize],
            kind,
            flags: flags | group_flag(sec) | link_flag,
            offset,
            size: sec.size(),
            link,
            align: sec.align(),
            entsize,
            ..Shdr::default()
        };
        out.put(&header.encode())?;
    }

    if layout.stack_index.is_some() {
        let executable = obj.stack_note == StackNote::Executable;
        let header = Shdr {
            name: layout.names.stack,
            kind: SHT_PROGBITS,
            flags: if executable { SHF_EXECINSTR } else { 0 },
            offset: layout.groups_off,
            align: 1,
            ..Shdr::default()
        };
        out.put(&header.encode())?;
    }

    let mut offset = layout.relas_off;
    for (index, sec) in obj.sections.iter().enumerate() {
        if sec.reloc_count == 0 {
            continue;
        }
        let size = RELA_SIZE * u64::from(sec.reloc_count);
        let header = Shdr {
            name: layout.names.offsets[sec.name as usize] - RELA_PREFIX.len() as u32,
            kind: SHT_RELA,
            flags: SHF_INFO_LINK | group_flag(sec),
            offset,
            size,
            link: layout.symtab_index,
            info: layout.section_index(index as u32),
            align: 8,
            entsize: RELA_SIZE,
        };
        out.put(&header.encode())?;
        offset += size;
    }

    emit_table_headers(obj, layout, out)
}

/// The headers of `.symtab`, `.symtab_shndx`, `.strtab` and `.shstrtab`.
fn emit_table_headers<S: Sink>(obj: &Object, layout: &Layout, out: &mut S) -> Result<(), S::Error> {
    let symbol_count = u64::from(layout.symbols.count);
    let symtab = Shdr {
        name: NAME_SYMTAB,
        kind: SHT_SYMTAB,
        offset: layout.symtab_off,
        size: SYM_SIZE * symbol_count,
        link: layout.strtab_index,
        info: layout.symbols.first_global,
        align: 8,
        entsize: SYM_SIZE,
        ..Shdr::default()
    };
    out.put(&symtab.encode())?;
    if layout.shndx_index.is_some() {
        let shndx = Shdr {
            name: layout.names.shndx,
            kind: SHT_SYMTAB_SHNDX,
            offset: layout.shndx_off,
            size: 4 * symbol_count,
            link: layout.symtab_index,
            align: 4,
            entsize: 4,
            ..Shdr::default()
        };
        out.put(&shndx.encode())?;
    }
    let strtab = Shdr {
        name: NAME_STRTAB,
        kind: SHT_STRTAB,
        offset: layout.strtab_off,
        size: obj.strings.len() as u64,
        align: 1,
        ..Shdr::default()
    };
    out.put(&strtab.encode())?;
    let shstrtab = Shdr {
        name: NAME_SHSTRTAB,
        kind: SHT_STRTAB,
        offset: layout.shstrtab_off,
        size: layout.names.size,
        align: 1,
        ..Shdr::default()
    };
    out.put(&shstrtab.encode())
}

/// The size in bytes of the file [`write`] and [`to_vec`] produce.
///
/// # Errors
/// As [`to_vec`].
pub fn file_size(obj: &Object) -> Result<u64, Error> {
    Ok(Layout::new(obj)?.file_size)
}

/// Serializes an object as an ELF64 relocatable file.
///
/// # Errors
/// Any [`Error`] the object's construction deferred, or that the checks of
/// `spec/object.md` "Errors" find.
pub fn to_vec(obj: &Object) -> Result<Vec<u8>, Error> {
    let layout = Layout::new(obj)?;
    let mut bytes = Vec::new();
    usize::try_from(layout.file_size)
        .ok()
        .and_then(|size| bytes.try_reserve_exact(size).ok())
        .ok_or(Error::TooLarge(Limit::FileBytes))?;
    let mut sink = VecSink(bytes);
    let Ok(()) = emit(obj, &layout, &mut sink);
    Ok(sink.0)
}

/// Serializes an object as an ELF64 relocatable file into `out`, front to
/// back, and returns the number of bytes written. `out` is not flushed.
///
/// # Errors
/// [`WriteError::Invalid`], before anything is written, for an object
/// [`to_vec`] rejects; [`WriteError::Io`] when `out` fails.
pub fn write<W: Write>(obj: &Object, out: &mut W) -> Result<u64, WriteError> {
    let layout = Layout::new(obj)?;
    let mut sink = Buffered { out, buffer: Vec::with_capacity(BUFFER_SIZE), position: 0 };
    emit(obj, &layout, &mut sink)?;
    sink.flush_buffer()?;
    Ok(layout.file_size)
}
