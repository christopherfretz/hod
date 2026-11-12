//! Test-only support: a minimal ELF64 reader written against the ELF gABI,
//! independent of the writer, and the objects several tests share.
//!
//! [`Elf::parse`] also checks the invariants every relocatable object must
//! have (`spec/object.md` "ELF writer"), so each test that parses an object
//! checks them.

#![allow(dead_code)] // Each test binary uses its own subset.

#[cfg(unix)]
pub mod linking;

use hod_obj::{
    A64Reloc, Arch, Binding, GroupKind, Object, SectionId, SectionKind, Symbol, SymbolId,
    SymbolKind, SymbolPlace, X64Reloc,
};

pub const SHT_PROGBITS: u32 = 1;
pub const SHT_SYMTAB: u32 = 2;
pub const SHT_STRTAB: u32 = 3;
pub const SHT_RELA: u32 = 4;
pub const SHT_NOTE: u32 = 7;
pub const SHT_NOBITS: u32 = 8;
pub const SHT_INIT_ARRAY: u32 = 14;
pub const SHT_FINI_ARRAY: u32 = 15;
pub const SHT_GROUP: u32 = 17;
pub const SHT_SYMTAB_SHNDX: u32 = 18;

pub const SHF_WRITE: u64 = 0x1;
pub const SHF_ALLOC: u64 = 0x2;
pub const SHF_EXECINSTR: u64 = 0x4;
pub const SHF_MERGE: u64 = 0x10;
pub const SHF_STRINGS: u64 = 0x20;
pub const SHF_INFO_LINK: u64 = 0x40;
pub const SHF_LINK_ORDER: u64 = 0x80;
pub const SHF_GROUP: u64 = 0x200;
pub const SHF_TLS: u64 = 0x400;

pub const SHN_UNDEF: u32 = 0;
pub const SHN_ABS: u32 = 0xfff1;
pub const SHN_COMMON: u32 = 0xfff2;

pub const STB_LOCAL: u8 = 0;
pub const STB_GLOBAL: u8 = 1;
pub const STB_WEAK: u8 = 2;

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap())
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap())
}

fn c_string(table: &[u8], at: u32) -> String {
    let tail = &table[at as usize..];
    let len = tail.iter().position(|&b| b == 0).expect("unterminated string");
    String::from_utf8(tail[..len].to_vec()).expect("name is not UTF-8")
}

#[derive(Clone, Debug)]
pub struct Section {
    pub name: String,
    pub sh_name: u32,
    pub kind: u32,
    pub flags: u64,
    pub addr: u64,
    pub offset: u64,
    pub size: u64,
    pub link: u32,
    pub info: u32,
    pub align: u64,
    pub entsize: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sym {
    pub name: String,
    pub bind: u8,
    pub kind: u8,
    pub other: u8,
    /// The section index, through `.symtab_shndx` when `st_shndx` is
    /// `SHN_XINDEX`; the reserved values stay as they are.
    pub shndx: u32,
    pub value: u64,
    pub size: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rela {
    pub offset: u64,
    pub sym: u32,
    pub kind: u32,
    pub addend: i64,
}

#[derive(Debug)]
pub struct Elf {
    pub bytes: Vec<u8>,
    pub os_abi: u8,
    pub machine: u16,
    pub flags: u32,
    /// `e_shnum` and `e_shstrndx` as stored in the file header.
    pub raw_shnum: u16,
    pub raw_shstrndx: u16,
    pub shstrndx: u32,
    pub sections: Vec<Section>,
    pub symtab: usize,
    pub symbols: Vec<Sym>,
}

impl Elf {
    /// Parses a relocatable object and checks its structural invariants.
    ///
    /// # Panics
    /// Panics, saying what is wrong, when the file is not a well-formed
    /// ELF64 little-endian relocatable object.
    pub fn parse(bytes: &[u8]) -> Self {
        assert!(bytes.len() >= 64, "shorter than a file header");
        assert_eq!(&bytes[..4], b"\x7fELF");
        assert_eq!(bytes[4], 2, "ELFCLASS64");
        assert_eq!(bytes[5], 1, "ELFDATA2LSB");
        assert_eq!(bytes[6], 1, "EI_VERSION");
        assert_eq!(&bytes[8..16], &[0; 8], "ABI version and padding");
        assert_eq!(u16_at(bytes, 16), 1, "ET_REL");
        assert_eq!(u32_at(bytes, 20), 1, "e_version");
        assert_eq!(u64_at(bytes, 24), 0, "e_entry");
        assert_eq!(u64_at(bytes, 32), 0, "e_phoff");
        assert_eq!(u16_at(bytes, 52), 64, "e_ehsize");
        assert_eq!(u16_at(bytes, 54), 0, "e_phentsize");
        assert_eq!(u16_at(bytes, 56), 0, "e_phnum");
        assert_eq!(u16_at(bytes, 58), 64, "e_shentsize");
        let shoff = u64_at(bytes, 40) as usize;
        assert_eq!(shoff % 8, 0, "section header table alignment");
        let raw_shnum = u16_at(bytes, 60);
        let raw_shstrndx = u16_at(bytes, 62);

        // ELF gABI "ELF Header": the escapes for large section counts.
        let header = |index: usize| &bytes[shoff + 64 * index..shoff + 64 * (index + 1)];
        let zero = header(0);
        let shnum = if raw_shnum == 0 { u64_at(zero, 32) as usize } else { raw_shnum as usize };
        let shstrndx = if raw_shstrndx == 0xffff { u32_at(zero, 40) } else { raw_shstrndx.into() };
        assert!(shnum >= 0xff00 || u64_at(zero, 32) == 0, "sh_size of header 0 without escape");
        assert!(shstrndx >= 0xff00 || u32_at(zero, 40) == 0, "sh_link of header 0 without escape");
        assert_eq!(raw_shnum == 0, shnum >= 0xff00, "e_shnum escape");
        assert_eq!(raw_shstrndx == 0xffff, shstrndx >= 0xff00, "e_shstrndx escape");
        assert_eq!(shoff + 64 * shnum, bytes.len(), "the section header table ends the file");
        for field in [0, 4, 8, 16, 24, 44, 48, 56] {
            let width = if field == 0 || field == 4 || field == 44 { 4 } else { 8 };
            assert!(zero[field..field + width].iter().all(|&b| b == 0), "header 0 field {field}");
        }

        let mut sections: Vec<Section> = (0..shnum)
            .map(|index| {
                let h = header(index);
                Section {
                    name: String::new(),
                    sh_name: u32_at(h, 0),
                    kind: u32_at(h, 4),
                    flags: u64_at(h, 8),
                    addr: u64_at(h, 16),
                    offset: u64_at(h, 24),
                    size: u64_at(h, 32),
                    link: u32_at(h, 40),
                    info: u32_at(h, 44),
                    align: u64_at(h, 48),
                    entsize: u64_at(h, 56),
                }
            })
            .collect();

        // Contents lie between the file header and the section header
        // table, aligned, and do not overlap.
        let mut extents = Vec::new();
        for (index, sec) in sections.iter().enumerate().skip(1) {
            assert_eq!(sec.addr, 0, "sh_addr of section {index}");
            assert!(sec.align.is_power_of_two(), "sh_addralign of section {index}");
            if sec.kind != SHT_NOBITS {
                assert_eq!(sec.offset % sec.align, 0, "file alignment of section {index}");
            }
            assert!(sec.offset >= 64 && sec.offset <= shoff as u64, "offset of section {index}");
            if sec.kind != SHT_NOBITS && sec.size != 0 {
                assert!(sec.offset + sec.size <= shoff as u64, "end of section {index}");
                extents.push((sec.offset, sec.offset + sec.size, index));
            }
        }
        extents.sort_unstable();
        for pair in extents.windows(2) {
            assert!(pair[0].1 <= pair[1].0, "sections {} and {} overlap", pair[0].2, pair[1].2);
        }

        let strings = &sections[shstrndx as usize];
        assert_eq!(strings.kind, SHT_STRTAB, "e_shstrndx is a string table");
        let table = &bytes[strings.offset as usize..(strings.offset + strings.size) as usize];
        assert_eq!(table.first(), Some(&0), "string table starts with NUL");
        assert_eq!(table.last(), Some(&0), "string table ends with NUL");
        let names: Vec<String> = sections.iter().map(|s| c_string(table, s.sh_name)).collect();
        for (sec, name) in sections.iter_mut().zip(names) {
            sec.name = name;
        }
        assert_eq!(sections[shstrndx as usize].name, ".shstrtab");

        let mut elf = Self {
            bytes: bytes.to_vec(),
            os_abi: bytes[7],
            machine: u16_at(bytes, 18),
            flags: u32_at(bytes, 48),
            raw_shnum,
            raw_shstrndx,
            shstrndx,
            sections,
            symtab: 0,
            symbols: Vec::new(),
        };
        elf.read_symbols();
        elf.check_relocations();
        elf.check_groups();
        elf.check_links();
        elf
    }

    pub fn data(&self, index: usize) -> &[u8] {
        let sec = &self.sections[index];
        if sec.kind == SHT_NOBITS {
            return &[];
        }
        &self.bytes[sec.offset as usize..(sec.offset + sec.size) as usize]
    }

    /// Indices of the sections with this name, in header order.
    pub fn named(&self, name: &str) -> Vec<usize> {
        (0..self.sections.len()).filter(|&i| self.sections[i].name == name).collect()
    }

    /// The index of the only section with this name.
    pub fn only(&self, name: &str) -> usize {
        let found = self.named(name);
        assert_eq!(found.len(), 1, "sections named {name}: {found:?}");
        found[0]
    }

    /// The symbol table index of the only symbol with this name.
    pub fn symbol(&self, name: &str) -> usize {
        let found: Vec<_> =
            (0..self.symbols.len()).filter(|&i| self.symbols[i].name == name).collect();
        assert_eq!(found.len(), 1, "symbols named {name}: {found:?}");
        found[0]
    }

    fn read_symbols(&mut self) {
        let tables: Vec<_> =
            (0..self.sections.len()).filter(|&i| self.sections[i].kind == SHT_SYMTAB).collect();
        assert_eq!(tables.len(), 1, "exactly one SHT_SYMTAB");
        self.symtab = tables[0];
        let symtab = &self.sections[self.symtab];
        assert_eq!(symtab.name, ".symtab");
        assert_eq!((symtab.entsize, symtab.align, symtab.flags), (24, 8, 0));
        assert_eq!(symtab.size % 24, 0);
        let count = (symtab.size / 24) as usize;
        let strtab = &self.sections[symtab.link as usize];
        assert_eq!((strtab.kind, strtab.name.as_str()), (SHT_STRTAB, ".strtab"));
        let names = self.data(symtab.link as usize);
        assert_eq!((names.first(), names.last()), (Some(&0), Some(&0)));

        // ELF gABI "Symbol Table": extended section indices.
        let extended: Vec<_> = (0..self.sections.len())
            .filter(|&i| self.sections[i].kind == SHT_SYMTAB_SHNDX)
            .collect();
        assert!(extended.len() <= 1);
        let extended = extended.first().map(|&index| {
            let sec = &self.sections[index];
            assert_eq!(sec.name, ".symtab_shndx");
            assert_eq!(sec.link as usize, self.symtab, ".symtab_shndx sh_link");
            assert_eq!((sec.entsize, sec.align), (4, 4));
            assert_eq!(sec.size, 4 * count as u64, "one entry per symbol");
            self.data(index)
        });

        let data = self.data(self.symtab);
        let mut symbols = Vec::with_capacity(count);
        let mut needs_extended = false;
        for index in 0..count {
            let entry = &data[24 * index..24 * (index + 1)];
            let raw = u16_at(entry, 6);
            let word = extended.map_or(0, |table| u32_at(table, 4 * index));
            let shndx = if raw == 0xffff {
                needs_extended = true;
                assert!(extended.is_some(), "SHN_XINDEX without .symtab_shndx");
                assert!(word >= 0xff00, "SHN_XINDEX for an index that fits");
                word
            } else {
                assert_eq!(word, 0, ".symtab_shndx entry of a symbol that does not use it");
                u32::from(raw)
            };
            if !matches!(shndx, SHN_UNDEF | SHN_ABS | SHN_COMMON) {
                assert!((shndx as usize) < self.sections.len(), "symbol {index} section");
                assert!(raw == 0xffff || raw < 0xff00, "symbol {index} reserved index {raw:#x}");
            }
            symbols.push(Sym {
                name: c_string(names, u32_at(entry, 0)),
                bind: entry[4] >> 4,
                kind: entry[4] & 0xf,
                other: entry[5],
                shndx,
                value: u64_at(entry, 8),
                size: u64_at(entry, 16),
            });
        }
        assert_eq!(extended.is_some(), needs_extended, ".symtab_shndx only when needed");
        assert!(count >= 1, "the null symbol");
        let null =
            Sym { name: String::new(), bind: 0, kind: 0, other: 0, shndx: 0, value: 0, size: 0 };
        assert_eq!(symbols[0], null);

        // Locals first; sh_info is the index of the first non-local.
        let first_global = symtab.info as usize;
        assert!(first_global >= 1 && first_global <= count, "sh_info of .symtab");
        for (index, symbol) in symbols.iter().enumerate() {
            assert_eq!(symbol.bind == STB_LOCAL, index < first_global, "symbol {index} binding");
            if index != 0 && symbol.shndx == SHN_UNDEF {
                assert_ne!(symbol.bind, STB_LOCAL, "undefined local symbol {index}");
            }
        }
        // A file symbol precedes the other locals.
        let files = symbols.iter().skip(1).take_while(|s| s.kind == 4).count();
        assert!(symbols.iter().skip(1 + files).all(|s| s.kind != 4), "file symbols come first");
        self.symbols = symbols;
    }

    /// The relocations that apply to section `target`.
    pub fn relas_of(&self, target: usize) -> Vec<Rela> {
        let found: Vec<_> = (0..self.sections.len())
            .filter(|&i| {
                self.sections[i].kind == SHT_RELA && self.sections[i].info == target as u32
            })
            .collect();
        assert!(found.len() <= 1, "several relocation sections for section {target}");
        found.first().map(|&index| self.relas_in(index)).unwrap_or_default()
    }

    /// The entries of relocation section `index`.
    pub fn relas_in(&self, index: usize) -> Vec<Rela> {
        self.data(index)
            .chunks_exact(24)
            .map(|entry| {
                let info = u64_at(entry, 8);
                Rela {
                    offset: u64_at(entry, 0),
                    sym: (info >> 32) as u32,
                    kind: info as u32,
                    addend: u64_at(entry, 16) as i64,
                }
            })
            .collect()
    }

    fn check_relocations(&self) {
        for (index, sec) in self.sections.iter().enumerate() {
            if sec.kind != SHT_RELA {
                continue;
            }
            assert_eq!((sec.entsize, sec.align), (24, 8), "relocation section {index}");
            assert_eq!(sec.size % 24, 0);
            assert_ne!(sec.size, 0, "empty relocation section {index}");
            assert_eq!(sec.link as usize, self.symtab, "sh_link of relocation section {index}");
            assert_eq!(sec.flags & !SHF_GROUP, SHF_INFO_LINK, "flags of relocation section");
            let target = &self.sections[sec.info as usize];
            assert_eq!(sec.name, format!(".rela{}", target.name));
            assert_ne!(target.kind, SHT_NOBITS);
            assert_eq!(sec.flags & SHF_GROUP, target.flags & SHF_GROUP, "group of {}", sec.name);
            let relas = self.relas_in(index);
            for rela in &relas {
                assert!((rela.sym as usize) < self.symbols.len(), "relocation symbol index");
                assert_ne!(rela.sym, 0, "relocation against the null symbol");
                assert!(rela.offset <= target.size, "relocation offset in {}", sec.name);
            }
            assert!(relas.is_sorted_by_key(|r| r.offset), "{} is sorted by offset", sec.name);
        }
    }

    /// ELF gABI "Sections": `sh_link` and `sh_info` of the sections that
    /// are not tables. A section has a link exactly when it has
    /// `SHF_LINK_ORDER`, and the link names another section with contents
    /// of its own; `sh_info` is unused.
    fn check_links(&self) {
        let is_table = |kind| {
            matches!(kind, 0 | SHT_SYMTAB | SHT_STRTAB | SHT_RELA | SHT_GROUP | SHT_SYMTAB_SHNDX)
        };
        for (index, sec) in self.sections.iter().enumerate() {
            if is_table(sec.kind) {
                assert_eq!(sec.flags & SHF_LINK_ORDER, 0, "SHF_LINK_ORDER on table {index}");
                continue;
            }
            assert_eq!(sec.info, 0, "sh_info of section {index}");
            assert_eq!(sec.flags & SHF_INFO_LINK, 0, "SHF_INFO_LINK on section {index}");
            assert_eq!(sec.flags & SHF_LINK_ORDER != 0, sec.link != 0, "sh_link of {index}");
            if sec.link != 0 {
                let target = self.sections.get(sec.link as usize);
                let target = target.unwrap_or_else(|| panic!("sh_link of section {index}"));
                assert!(!is_table(target.kind), "section {index} is linked to a table");
                assert_ne!(sec.link as usize, index, "section {index} is linked to itself");
            }
        }
    }

    /// The flag word and the member section indices of group section
    /// `index`.
    pub fn group(&self, index: usize) -> (u32, Vec<u32>) {
        let words: Vec<u32> = self.data(index).chunks_exact(4).map(|w| u32_at(w, 0)).collect();
        (words[0], words[1..].to_vec())
    }

    fn check_groups(&self) {
        let mut owner = vec![0usize; self.sections.len()];
        for (index, sec) in self.sections.iter().enumerate() {
            if sec.kind != SHT_GROUP {
                continue;
            }
            assert_eq!(sec.name, ".group");
            assert_eq!((sec.entsize, sec.align, sec.flags), (4, 4, 0), "group section {index}");
            assert!(sec.size >= 4 && sec.size % 4 == 0);
            assert_eq!(sec.link as usize, self.symtab, "sh_link of group {index}");
            assert!((sec.info as usize) < self.symbols.len() && sec.info != 0, "signature");
            let (flags, members) = self.group(index);
            assert!(flags <= 1, "group flags");
            for member in members {
                // ELF gABI "Section Groups": the group's header precedes
                // its members'.
                assert!(member as usize > index, "member {member} before its group {index}");
                let section = &self.sections[member as usize];
                assert_ne!(section.flags & SHF_GROUP, 0, "member {member} lacks SHF_GROUP");
                assert_eq!(owner[member as usize], 0, "section {member} is in two groups");
                owner[member as usize] = index;
            }
        }
        for (index, sec) in self.sections.iter().enumerate() {
            assert_eq!(sec.flags & SHF_GROUP != 0, owner[index] != 0, "SHF_GROUP of {index}");
            if sec.kind == SHT_RELA && owner[index] != 0 {
                assert_eq!(owner[index], owner[sec.info as usize], "group of {}", sec.name);
            }
        }
    }
}

/// AArch64 instruction words as section bytes.
pub fn words(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

pub const MOV_W0_42: u32 = 0x5280_0540;
pub const MOV_W0_0: u32 = 0x5280_0000;
pub const RET: u32 = 0xd65f_03c0;
/// `stp x29, x30, [sp, #-16]!`
pub const PUSH_FRAME: u32 = 0xa9bf_7bfd;
/// `mov x29, sp`
pub const SET_FP: u32 = 0x9100_03fd;
/// `ldp x29, x30, [sp], #16`
pub const POP_FRAME: u32 = 0xa8c1_7bfd;
/// `adrp x0, <ADR_PREL_PG_HI21>`
pub const ADRP_X0: u32 = 0x9000_0000;
/// `add x0, x0, <ADD_ABS_LO12_NC>`
pub const ADD_X0_LO12: u32 = 0x9100_0000;
/// `ldr x0, [x0, <LDST64_ABS_LO12_NC>]`
pub const LDR_X0_LO12: u32 = 0xf940_0000;
/// `ldr w0, [x0, <LDST32_ABS_LO12_NC>]`
pub const LDR_W0_LO12: u32 = 0xb940_0000;
/// `bl <CALL26>`
pub const BL: u32 = 0x9400_0000;
/// `b <JUMP26>`
pub const B: u32 = 0x1400_0000;

/// `bti c`: a landing pad; a no-op where the feature is absent.
pub const BTI_C: u32 = 0xd503_245f;
/// `mrs x0, tpidr_el0`: the thread pointer.
pub const MRS_X0_TP: u32 = 0xd53b_d040;
/// `mrs x1, tpidr_el0`
pub const MRS_X1_TP: u32 = 0xd53b_d041;
/// `add x0, x1, x0`
pub const ADD_X0_X1_X0: u32 = 0x8b00_0020;

/// Adds a global function symbol covering `code`, appended to `section`.
pub fn function(obj: &mut Object, section: SectionId, name: &str, code: &[u32]) -> SymbolId {
    function_bytes(obj, section, name, &words(code))
}

/// Adds a global function symbol covering `code`, appended to `section`.
pub fn function_bytes(obj: &mut Object, section: SectionId, name: &str, code: &[u8]) -> SymbolId {
    let at = obj.append(section, code);
    let name = obj.add_string(name.as_bytes()).unwrap();
    obj.add_symbol(Symbol::defined(name, SymbolKind::Func, section, at, code.len() as u64))
}

/// Adds a symbol of the given kind and binding at `value` in `section`.
pub fn define(
    obj: &mut Object,
    name: &str,
    kind: SymbolKind,
    binding: Binding,
    section: SectionId,
    value: u64,
    size: u64,
) -> SymbolId {
    let name = obj.add_string(name.as_bytes()).unwrap();
    obj.add_symbol(Symbol::defined(name, kind, section, value, size).with_binding(binding))
}

/// The sections and the `puts` symbol a [`say`] function needs.
#[derive(Clone, Copy, Debug)]
pub struct Say {
    pub text: SectionId,
    pub rodata: SectionId,
    pub puts: SymbolId,
}

impl Say {
    pub fn new(obj: &mut Object) -> Self {
        let text = obj.add_section(b".text", SectionKind::Text, 16).unwrap();
        let rodata = obj.add_section(b".rodata", SectionKind::ReadOnlyData, 1).unwrap();
        Self { text, rodata, puts: undefined(obj, "puts") }
    }
}

/// Adds `int name(void) { puts(message); return 0; }` for the object's
/// architecture: the message through a local symbol in `.rodata`, `puts`
/// through the PLT.
pub fn say(obj: &mut Object, at: Say, name: &str, message: &str) -> SymbolId {
    let offset = obj.append(at.rodata, message.as_bytes());
    obj.append(at.rodata, &[0]);
    let size = message.len() as u64 + 1;
    let string = define(obj, ".L.str", SymbolKind::Object, Binding::Local, at.rodata, offset, size);
    match obj.arch() {
        Arch::AArch64 => {
            let code = [PUSH_FRAME, SET_FP, ADRP_X0, ADD_X0_LO12, BL, MOV_W0_0, POP_FRAME, RET];
            let function = function(obj, at.text, name, &code);
            let start = obj.symbol(function).value;
            obj.add_relocation(at.text, start + 8, A64Reloc::AdrPrelPgHi21, string, 0);
            obj.add_relocation(at.text, start + 12, A64Reloc::AddAbsLo12Nc, string, 0);
            obj.add_relocation(at.text, start + 16, A64Reloc::Call26, at.puts, 0);
            function
        }
        Arch::X86_64 => {
            #[rustfmt::skip]
            let code = [
                0x55,                         // push %rbp
                0x48, 0x89, 0xe5,             // mov %rsp, %rbp
                0x48, 0x8d, 0x3d, 0, 0, 0, 0, // lea .L.str(%rip), %rdi
                0xe8, 0, 0, 0, 0,             // call puts
                0x31, 0xc0,                   // xor %eax, %eax
                0x5d,                         // pop %rbp
                0xc3,                         // ret
            ];
            let function = function_bytes(obj, at.text, name, &code);
            let start = obj.symbol(function).value;
            obj.add_relocation(at.text, start + 7, X64Reloc::Pc32, string, -4);
            obj.add_relocation(at.text, start + 12, X64Reloc::Plt32, at.puts, -4);
            function
        }
    }
}

/// Adds a global undefined symbol.
pub fn undefined(obj: &mut Object, name: &str) -> SymbolId {
    let name = obj.add_string(name.as_bytes()).unwrap();
    obj.add_symbol(Symbol::undefined(name))
}

/// `int main(void) { return 42; }`
pub fn return_42() -> Object {
    let mut obj = Object::new(Arch::AArch64);
    let text = obj.add_section(b".text", SectionKind::Text, 4).unwrap();
    function(&mut obj, text, "main", &[MOV_W0_42, RET]);
    obj
}

/// `int main(void) { puts(message); return 0; }` with the message in
/// `.rodata`, addressed through a local symbol.
pub fn hello(message: &str) -> Object {
    let mut obj = Object::new(Arch::AArch64);
    obj.add_file_symbol(b"hello.c").unwrap();
    let text = obj.add_section(b".text", SectionKind::Text, 4).unwrap();
    let rodata = obj.add_section(b".rodata", SectionKind::ReadOnlyData, 1).unwrap();
    let at = obj.append(rodata, message.as_bytes());
    obj.append(rodata, &[0]);
    let name = obj.add_string(b".L.str").unwrap();
    let string = obj.add_symbol(
        Symbol::defined(name, SymbolKind::Object, rodata, at, message.len() as u64 + 1)
            .with_binding(Binding::Local),
    );
    let puts = undefined(&mut obj, "puts");
    function(
        &mut obj,
        text,
        "main",
        &[PUSH_FRAME, SET_FP, ADRP_X0, ADD_X0_LO12, BL, MOV_W0_0, POP_FRAME, RET],
    );
    obj.add_relocation(text, 8, A64Reloc::AdrPrelPgHi21, string, 0);
    obj.add_relocation(text, 12, A64Reloc::AddAbsLo12Nc, string, 0);
    obj.add_relocation(text, 16, A64Reloc::Call26, puts, 0);
    obj
}

/// `int main(void) { puts("x86"); return 42; }` for x86-64, and a pointer
/// to `main` in `.data`.
pub fn x86_64_program() -> Object {
    let mut obj = Object::new(Arch::X86_64);
    let text = obj.add_section(b".text", SectionKind::Text, 16).unwrap();
    let rodata = obj.add_section(b".rodata", SectionKind::ReadOnlyData, 1).unwrap();
    let data = obj.add_section(b".data", SectionKind::Data, 8).unwrap();
    obj.append(rodata, b"x86\0");
    let name = obj.add_string(b".L.str").unwrap();
    let string = obj.add_symbol(
        Symbol::defined(name, SymbolKind::Object, rodata, 0, 4).with_binding(Binding::Local),
    );
    let puts = undefined(&mut obj, "puts");
    #[rustfmt::skip]
    let code = [
        0x55,                                     // push %rbp
        0x48, 0x89, 0xe5,                         // mov %rsp, %rbp
        0x48, 0x8d, 0x3d, 0, 0, 0, 0,             // lea .L.str(%rip), %rdi
        0xe8, 0, 0, 0, 0,                         // call puts
        0xb8, 42, 0, 0, 0,                        // mov $42, %eax
        0x5d,                                     // pop %rbp
        0xc3,                                     // ret
    ];
    obj.append(text, &code);
    let name = obj.add_string(b"main").unwrap();
    let main = obj.add_symbol(Symbol::defined(name, SymbolKind::Func, text, 0, code.len() as u64));
    obj.add_relocation(text, 7, X64Reloc::Pc32, string, -4);
    obj.add_relocation(text, 12, X64Reloc::Plt32, puts, -4);
    obj.append_zeros(data, 8);
    obj.add_relocation(data, 0, X64Reloc::Abs64, main, 0);
    obj
}

/// Adds a COMDAT group `answer`: a function `answer` in `.text.answer`
/// that returns the 32-bit value stored in `.rodata.answer`, which is in
/// the group too. The function's symbol has the given binding.
pub fn add_answer_comdat(obj: &mut Object, binding: Binding, value: u32) -> SymbolId {
    let text = obj.add_section(b".text.answer", SectionKind::Text, 4).unwrap();
    let rodata = obj.add_section(b".rodata.answer", SectionKind::ReadOnlyData, 4).unwrap();
    obj.append(rodata, &value.to_le_bytes());
    let name = obj.add_string(b"answer_value").unwrap();
    let data = obj.add_symbol(
        Symbol::defined(name, SymbolKind::Object, rodata, 0, 4).with_binding(Binding::Weak),
    );
    let answer = function(obj, text, "answer", &[ADRP_X0, LDR_W0_LO12, RET]);
    obj.symbol_mut(answer).binding = binding;
    obj.add_relocation(text, 0, A64Reloc::AdrPrelPgHi21, data, 0);
    obj.add_relocation(text, 4, A64Reloc::Ldst32AbsLo12Nc, data, 0);
    let group = obj.add_group(answer, GroupKind::Comdat);
    obj.add_group_member(group, text).unwrap();
    obj.add_group_member(group, rodata).unwrap();
    answer
}

/// An object with `count` one-instruction functions, each in its own
/// `.text.f<i>` section, and `main` in the last section, which tail-calls
/// the last of them; that one returns 42 and the others return 1. With
/// `count` above 0xff00 the section count, the section name table index
/// and the section indices of the last symbols all need the ELF escapes.
pub fn many_sections(count: u32) -> Object {
    let mut obj = Object::new(Arch::AArch64);
    obj.reserve(count as usize + 1, count as usize + 1, 1, 12 * count as usize);
    let mut last = None;
    for index in 0..count {
        let name = format!(".text.f{index}");
        let section = obj.add_section(name.as_bytes(), SectionKind::Text, 4).unwrap();
        let result = if index + 1 == count { MOV_W0_42 } else { 0x5280_0020 };
        last = Some(function(&mut obj, section, &name[6..], &[result, RET]));
    }
    let text = obj.add_section(b".text.main", SectionKind::Text, 4).unwrap();
    function(&mut obj, text, "main", &[B]);
    obj.add_relocation(text, 0, A64Reloc::Jump26, last.unwrap(), 0);
    obj
}

/// The place of a symbol, for assertions on the model.
pub fn place(obj: &Object, symbol: SymbolId) -> SymbolPlace {
    obj.symbol(symbol).place
}
