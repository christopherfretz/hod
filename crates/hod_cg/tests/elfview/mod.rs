//! A small ELF64 little-endian reader for comparing objects with `llc`'s:
//! an object becomes a normalized, order-independent description of its
//! sections, symbols and relocations (`spec/landed/codegen/WP-032.md`
//! lists what is normalized away and why).

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fmt::Write as _;

const SHT_SYMTAB: u32 = 2;
const SHT_RELA: u32 = 4;
const SHT_NOBITS: u32 = 8;
const SHT_GROUP: u32 = 17;
const SHF_GROUP: u64 = 0x200;
/// `hod_obj` marks relocation sections `SHF_INFO_LINK`; `llc` does not.
const SHF_INFO_LINK: u64 = 0x40;

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes(b[o..o + 2].try_into().unwrap())
}
fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn u64_at(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}
fn cstr(b: &[u8], o: usize) -> String {
    let end = b[o..].iter().position(|&c| c == 0).unwrap() + o;
    String::from_utf8_lossy(&b[o..end]).into_owned()
}

/// One section header.
#[derive(Clone, Debug)]
pub struct Section {
    pub name: String,
    pub sh_type: u32,
    pub flags: u64,
    pub offset: u64,
    pub size: u64,
    pub link: u32,
    pub info: u32,
    pub align: u64,
    pub entsize: u64,
}

/// One symbol.
#[derive(Clone, Debug)]
pub struct Sym {
    pub name: String,
    pub info: u8,
    pub other: u8,
    pub shndx: u16,
    pub value: u64,
    pub size: u64,
}

/// A parsed object.
pub struct Elf {
    pub bytes: Vec<u8>,
    pub sections: Vec<Section>,
    pub symbols: Vec<Sym>,
}

impl Elf {
    pub fn parse(bytes: Vec<u8>) -> Elf {
        assert_eq!(&bytes[..4], b"\x7fELF", "not an ELF file");
        let shoff = u64_at(&bytes, 0x28) as usize;
        let shnum = u16_at(&bytes, 0x3c) as usize;
        let shstrndx = u16_at(&bytes, 0x3e) as usize;
        let header = |i: usize| shoff + i * 64;
        let strtab_off = u64_at(&bytes, header(shstrndx) + 0x18) as usize;
        let mut sections = Vec::new();
        for i in 0..shnum {
            let h = header(i);
            sections.push(Section {
                name: cstr(&bytes, strtab_off + u32_at(&bytes, h) as usize),
                sh_type: u32_at(&bytes, h + 4),
                flags: u64_at(&bytes, h + 8),
                offset: u64_at(&bytes, h + 0x18),
                size: u64_at(&bytes, h + 0x20),
                link: u32_at(&bytes, h + 0x28),
                info: u32_at(&bytes, h + 0x2c),
                align: u64_at(&bytes, h + 0x30),
                entsize: u64_at(&bytes, h + 0x38),
            });
        }
        let mut symbols = Vec::new();
        if let Some(symtab) = sections.iter().find(|s| s.sh_type == SHT_SYMTAB) {
            let strings = sections[symtab.link as usize].offset as usize;
            for i in 0..(symtab.size / 24) as usize {
                let o = symtab.offset as usize + i * 24;
                symbols.push(Sym {
                    name: cstr(&bytes, strings + u32_at(&bytes, o) as usize),
                    info: bytes[o + 4],
                    other: bytes[o + 5],
                    shndx: u16_at(&bytes, o + 6),
                    value: u64_at(&bytes, o + 8),
                    size: u64_at(&bytes, o + 16),
                });
            }
        }
        Elf { bytes, sections, symbols }
    }

    pub fn data(&self, index: usize) -> &[u8] {
        let s = &self.sections[index];
        if s.sh_type == SHT_NOBITS {
            return &[];
        }
        &self.bytes[s.offset as usize..(s.offset + s.size) as usize]
    }

    /// The group signature of each section in a group.
    fn groups(&self) -> BTreeMap<usize, String> {
        let mut groups = BTreeMap::new();
        for s in &self.sections {
            if s.sh_type != SHT_GROUP {
                continue;
            }
            let signature = self.symbols[s.info as usize].name.clone();
            let words = (s.size / 4) as usize;
            for w in 1..words {
                let member = u32_at(&self.bytes, s.offset as usize + w * 4) as usize;
                groups.insert(member, signature.clone());
            }
        }
        groups
    }

    /// The key that names a section independently of its index: its
    /// name, group signature and flags (sections of one name differ in
    /// one of the other two).
    fn key(&self, index: usize, groups: &BTreeMap<usize, String>) -> String {
        let s = &self.sections[index];
        let group = groups.get(&index).map_or(String::new(), |g| format!(" [{g}]"));
        format!("{}{group} f{:x}", s.name, s.flags & !(SHF_GROUP | SHF_INFO_LINK))
    }

    /// The normalized description: one line per section (with its
    /// contents), group, symbol and relocation, sorted. `ignore_data`
    /// names sections whose contents are not compared.
    pub fn describe(&self, ignore_data: &dyn Fn(&str) -> bool) -> Vec<String> {
        self.describe_without(ignore_data, &|_| false)
    }

    /// As [`Elf::describe`], leaving out the sections `skip` names and
    /// their relocations (and group members).
    pub fn describe_without(
        &self,
        ignore_data: &dyn Fn(&str) -> bool,
        skip: &dyn Fn(&str) -> bool,
    ) -> Vec<String> {
        let skipped = |i: usize| {
            let s = &self.sections[i];
            skip(&s.name) || (s.sh_type == SHT_RELA && skip(&self.sections[s.info as usize].name))
        };
        let groups = self.groups();
        let mut lines = Vec::new();
        for (i, s) in self.sections.iter().enumerate() {
            if matches!(s.sh_type, 0 | SHT_SYMTAB | SHT_RELA | 3) || skipped(i) {
                continue;
            }
            if s.sh_type == SHT_GROUP {
                let flag = u32_at(&self.bytes, s.offset as usize);
                let words = (s.size / 4) as usize;
                let mut members: Vec<String> = (1..words)
                    .map(|w| u32_at(&self.bytes, s.offset as usize + w * 4) as usize)
                    .filter(|&m| !skipped(m))
                    .map(|m| self.key(m, &groups))
                    .collect();
                members.sort();
                lines.push(format!(
                    "group {} flag {flag} members {members:?}",
                    self.symbols[s.info as usize].name
                ));
                continue;
            }
            let mut line = format!(
                "section {} type {:#x} align {} entsize {} size {}",
                self.key(i, &groups),
                s.sh_type,
                s.align,
                s.entsize,
                s.size
            );
            if !ignore_data(&s.name) && s.sh_type != SHT_NOBITS {
                line.push_str(" data ");
                for b in self.data(i) {
                    let _ = write!(line, "{b:02x}");
                }
            }
            lines.push(line);
        }
        let signatures: Vec<&str> = self
            .sections
            .iter()
            .filter(|s| s.sh_type == SHT_GROUP)
            .map(|s| self.symbols[s.info as usize].name.as_str())
            .collect();
        for sym in self.symbols.iter().skip(1) {
            let kind = sym.info & 0xf;
            if kind == 3 || kind == 4 || is_mapping(&sym.name) {
                continue;
            }
            let section = match sym.shndx {
                0 => "UND".to_string(),
                0xfff1 => "ABS".to_string(),
                0xfff2 => "COM".to_string(),
                // A COMDAT signature symbol `llc` makes is in `.group`;
                // `hod` puts it in the first member.
                _ if sym.info >> 4 == 0 && signatures.contains(&sym.name.as_str()) => {
                    "(signature)".to_string()
                }
                n => self.key(n as usize, &groups),
            };
            let value = if sym.shndx == 0 { 0 } else { sym.value };
            lines.push(format!(
                "symbol {} {section} value {value} size {} type {kind} bind {} vis {}",
                sym.name,
                sym.size,
                sym.info >> 4,
                sym.other & 3
            ));
        }
        for (i, s) in self.sections.iter().enumerate() {
            if s.sh_type != SHT_RELA || skipped(i) {
                continue;
            }
            let target = self.key(s.info as usize, &groups);
            for r in 0..(s.size / 24) as usize {
                let o = s.offset as usize + r * 24;
                let offset = u64_at(&self.bytes, o);
                let info = u64_at(&self.bytes, o + 8);
                let addend = u64_at(&self.bytes, o + 16) as i64;
                let sym = &self.symbols[(info >> 32) as usize];
                let name = if sym.info & 0xf == 3 {
                    format!("[{}]", self.key(sym.shndx as usize, &groups))
                } else {
                    sym.name.clone()
                };
                lines.push(format!(
                    "reloc {target} +{offset} type {} {name} {addend:+}",
                    info & 0xffff_ffff
                ));
            }
        }
        lines.sort();
        lines
    }
}

impl Elf {
    /// The relocations of the section named `name`: type, symbol name
    /// (for a section symbol, `[section]`), addend.
    pub fn relocs_of(&self, name: &str) -> Vec<(u32, String, i64)> {
        let mut out = Vec::new();
        for s in &self.sections {
            if s.sh_type != SHT_RELA || self.sections[s.info as usize].name != name {
                continue;
            }
            for r in 0..(s.size / 24) as usize {
                let o = s.offset as usize + r * 24;
                let info = u64_at(&self.bytes, o + 8);
                let sym = &self.symbols[(info >> 32) as usize];
                let symbol = if sym.info & 0xf == 3 {
                    format!("[{}]", self.sections[sym.shndx as usize].name)
                } else {
                    sym.name.clone()
                };
                out.push(((info & 0xffff_ffff) as u32, symbol, u64_at(&self.bytes, o + 16) as i64));
            }
        }
        out
    }
}

/// An AArch64 mapping symbol: `$x`, `$d`, or one with a suffix.
pub fn is_mapping(name: &str) -> bool {
    name == "$x" || name == "$d" || name.starts_with("$x.") || name.starts_with("$d.")
}

/// The lines in one description and not the other.
pub fn diff(ours: &[String], theirs: &[String]) -> (Vec<String>, Vec<String>) {
    let mut only_ours = Vec::new();
    let mut only_theirs = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < ours.len() || j < theirs.len() {
        match (ours.get(i), theirs.get(j)) {
            (Some(a), Some(b)) if a == b => {
                i += 1;
                j += 1;
            }
            (Some(a), b) if b.is_none_or(|b| a < b) => {
                only_ours.push(a.clone());
                i += 1;
            }
            (_, Some(b)) => {
                only_theirs.push(b.clone());
                j += 1;
            }
            (_, None) => break,
        }
    }
    (only_ours, only_theirs)
}
