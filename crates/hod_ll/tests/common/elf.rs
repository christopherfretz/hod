//! A reader of the little-endian ELF64 relocatable objects `llc` writes:
//! sections with their bytes, the symbol table, the relocations and the
//! section groups. `differential.rs` checks it against `llvm-readelf`.

/// `SHT_NOBITS`.
pub const SHT_NOBITS: u32 = 8;
/// `SHF_TLS`.
pub const SHF_TLS: u64 = 0x400;
/// `SHF_GROUP`.
pub const SHF_GROUP: u64 = 0x200;
/// `SHN_UNDEF`.
pub const SHN_UNDEF: u16 = 0;
/// `SHN_COMMON`.
pub const SHN_COMMON: u16 = 0xfff2;

/// Symbol types.
pub const STT_NOTYPE: u8 = 0;
pub const STT_OBJECT: u8 = 1;
pub const STT_FUNC: u8 = 2;
pub const STT_SECTION: u8 = 3;
pub const STT_FILE: u8 = 4;
pub const STT_TLS: u8 = 6;
pub const STT_GNU_IFUNC: u8 = 10;

/// Symbol bindings.
pub const STB_LOCAL: u8 = 0;
pub const STB_GLOBAL: u8 = 1;
pub const STB_WEAK: u8 = 2;

#[derive(Clone, Debug)]
pub struct Section {
    pub name: String,
    pub kind: u32,
    pub flags: u64,
    pub size: u64,
    pub align: u64,
    pub link: u32,
    pub info: u32,
    /// Empty for `SHT_NOBITS`.
    pub data: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct Symbol {
    pub name: String,
    pub value: u64,
    pub size: u64,
    pub kind: u8,
    pub bind: u8,
    /// 0 default, 2 hidden, 3 protected.
    pub visibility: u8,
    pub shndx: u16,
}

#[derive(Clone, Copy, Debug)]
pub struct Reloc {
    /// Index of the section the relocation applies to.
    pub section: usize,
    pub offset: u64,
    pub kind: u32,
    /// Index in the symbol table.
    pub symbol: usize,
    pub addend: i64,
}

/// How a relocation computes its field.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RelocClass {
    /// Symbol + addend.
    Absolute,
    /// Symbol + addend - place.
    Relative,
    /// PLT entry of the symbol + addend - place.
    Plt,
    /// GOT entry of the symbol + addend - place.
    Got,
}

#[derive(Clone, Debug)]
pub struct Elf {
    /// `e_machine`: 62 for x86-64, 183 for AArch64.
    pub machine: u16,
    pub sections: Vec<Section>,
    pub symbols: Vec<Symbol>,
    pub relocs: Vec<Reloc>,
    /// Section groups: the signature symbol's name and the member
    /// section indices.
    pub groups: Vec<(String, Vec<usize>)>,
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap())
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap())
}

fn c_string(table: &[u8], at: usize) -> String {
    let end = table[at..].iter().position(|&b| b == 0).map_or(table.len(), |n| at + n);
    String::from_utf8_lossy(&table[at..end]).into_owned()
}

impl Elf {
    /// Reads an object file.
    ///
    /// # Panics
    /// If `bytes` is not a little-endian ELF64 relocatable object.
    pub fn parse(bytes: &[u8]) -> Elf {
        assert_eq!(&bytes[..6], b"\x7fELF\x02\x01", "not a little-endian ELF64 file");
        let machine = u16_at(bytes, 18);
        let shoff = u64_at(bytes, 0x28) as usize;
        let shentsize = u16_at(bytes, 0x3a) as usize;
        let shnum = u16_at(bytes, 0x3c) as usize;
        let shstrndx = u16_at(bytes, 0x3e) as usize;
        let header = |index: usize| &bytes[shoff + index * shentsize..][..shentsize];
        let contents = |index: usize| {
            let h = header(index);
            if u32_at(h, 4) == SHT_NOBITS {
                return &bytes[..0];
            }
            &bytes[u64_at(h, 0x18) as usize..][..u64_at(h, 0x20) as usize]
        };
        let names = contents(shstrndx);
        let sections: Vec<Section> = (0..shnum)
            .map(|index| {
                let h = header(index);
                Section {
                    name: c_string(names, u32_at(h, 0) as usize),
                    kind: u32_at(h, 4),
                    flags: u64_at(h, 8),
                    size: u64_at(h, 0x20),
                    align: u64_at(h, 0x30),
                    link: u32_at(h, 0x28),
                    info: u32_at(h, 0x2c),
                    data: contents(index).to_vec(),
                }
            })
            .collect();

        let mut symbols = Vec::new();
        if let Some(symtab) = sections.iter().find(|s| s.kind == 2) {
            let strings = &sections[symtab.link as usize].data;
            for entry in symtab.data.chunks_exact(24) {
                let shndx = u16_at(entry, 6);
                let mut name = c_string(strings, u32_at(entry, 0) as usize);
                if entry[4] & 15 == STT_SECTION && name.is_empty() {
                    name.clone_from(&sections[shndx as usize].name);
                }
                symbols.push(Symbol {
                    name,
                    value: u64_at(entry, 8),
                    size: u64_at(entry, 16),
                    kind: entry[4] & 15,
                    bind: entry[4] >> 4,
                    visibility: entry[5] & 3,
                    shndx,
                });
            }
        }

        let mut relocs = Vec::new();
        let mut groups = Vec::new();
        for section in &sections {
            if section.kind == 4 {
                for entry in section.data.chunks_exact(24) {
                    let info = u64_at(entry, 8);
                    relocs.push(Reloc {
                        section: section.info as usize,
                        offset: u64_at(entry, 0),
                        kind: info as u32,
                        symbol: (info >> 32) as usize,
                        addend: u64_at(entry, 16) as i64,
                    });
                }
            } else if section.kind == 17 {
                let members =
                    section.data.chunks_exact(4).skip(1).map(|m| u32_at(m, 0) as usize).collect();
                groups.push((symbols[section.info as usize].name.clone(), members));
            }
        }
        Elf { machine, sections, symbols, relocs, groups }
    }

    /// The symbol called `name` that is not a section symbol.
    pub fn symbol(&self, name: &str) -> Option<(usize, &Symbol)> {
        self.symbols
            .iter()
            .enumerate()
            .find(|(_, symbol)| symbol.name == name && symbol.kind != STT_SECTION)
    }

    /// The field size in bytes and the class of relocation type `kind`.
    pub fn reloc_class(&self, kind: u32) -> Option<(u8, RelocClass)> {
        use RelocClass::{Absolute, Got, Plt, Relative};
        Some(match (self.machine, kind) {
            // R_X86_64_64; R_AARCH64_ABS64.
            (62, 1) | (183, 257) => (8, Absolute),
            // R_X86_64_32, _32S; R_AARCH64_ABS32.
            (62, 10 | 11) | (183, 258) => (4, Absolute),
            // R_X86_64_16; R_AARCH64_ABS16.
            (62, 12) | (183, 259) => (2, Absolute),
            // R_X86_64_8.
            (62, 14) => (1, Absolute),
            // R_X86_64_PC64; R_AARCH64_PREL64.
            (62, 24) | (183, 260) => (8, Relative),
            // R_X86_64_PC32; R_AARCH64_PREL32.
            (62, 2) | (183, 261) => (4, Relative),
            // R_X86_64_PC16; R_AARCH64_PREL16.
            (62, 13) | (183, 262) => (2, Relative),
            // R_X86_64_PC8.
            (62, 15) => (1, Relative),
            // R_X86_64_PLT32; R_AARCH64_PLT32.
            (62, 4) | (183, 314) => (4, Plt),
            // R_X86_64_GOTPCREL; R_AARCH64_GOTPCREL32.
            (62, 9) | (183, 315) => (4, Got),
            _ => return None,
        })
    }

    /// The group that section `index` is a member of.
    pub fn group_of(&self, index: usize) -> Option<&str> {
        self.groups
            .iter()
            .find(|(_, members)| members.contains(&index))
            .map(|(name, _)| name.as_str())
    }
}
