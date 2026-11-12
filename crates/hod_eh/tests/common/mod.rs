//! Shared by the oracle tests (`spec/eh.md` §7): a small ELF64 reader and
//! patcher, a reader of `llvm-dwarfdump --eh-frame` output, and the
//! round trip "llc's object → `hod_eh`'s input rows → `hod_eh`'s `.eh_frame`
//! spliced into llc's object".

#![allow(dead_code, clippy::too_many_lines, clippy::format_push_string, clippy::format_collect)]

pub mod lsda;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use hod_eh::{Fde, Lsda, Op, Row, write_eh_frame};
use hod_obj::{Arch, Object, SectionKind, StrId, Symbol, SymbolId};
use hod_testutil::{Scratch, run, tool};

pub const LLC_FLAGS: [&str; 5] =
    ["-O0", "-global-isel=0", "-fast-isel=0", "-relocation-model=pic", "-filetype=obj"];

// ---------------------------------------------------------------- ELF ----

/// A section header.
#[derive(Clone, Debug)]
pub struct Shdr {
    pub name: String,
    pub ty: u32,
    pub flags: u64,
    pub offset: u64,
    pub size: u64,
    pub link: u32,
    pub info: u32,
    pub align: u64,
}

/// A symbol table entry.
#[derive(Clone, Debug)]
pub struct Sym {
    pub name: String,
    pub info: u8,
    pub other: u8,
    pub shndx: u16,
    pub value: u64,
}

/// An `Elf64_Rela`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rela {
    pub offset: u64,
    pub sym: u32,
    pub ty: u32,
    pub addend: i64,
}

fn u16_at(d: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(d[at..at + 2].try_into().unwrap())
}
fn u32_at(d: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(d[at..at + 4].try_into().unwrap())
}
fn u64_at(d: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(d[at..at + 8].try_into().unwrap())
}
fn cstr(d: &[u8], at: usize) -> String {
    let end = d[at..].iter().position(|&b| b == 0).unwrap() + at;
    String::from_utf8_lossy(&d[at..end]).into_owned()
}

/// An ELF64 little-endian relocatable object, read.
pub struct Elf {
    pub data: Vec<u8>,
    pub sections: Vec<Shdr>,
    pub symbols: Vec<Sym>,
}

impl Elf {
    pub fn parse(data: Vec<u8>) -> Elf {
        assert_eq!(&data[..4], b"\x7fELF");
        let shoff = u64_at(&data, 0x28) as usize;
        let shentsize = usize::from(u16_at(&data, 0x3a));
        let shnum = usize::from(u16_at(&data, 0x3c));
        let shstrndx = usize::from(u16_at(&data, 0x3e));
        let raw = |i: usize| {
            let h = shoff + i * shentsize;
            (
                u32_at(&data, h),
                Shdr {
                    name: String::new(),
                    ty: u32_at(&data, h + 4),
                    flags: u64_at(&data, h + 8),
                    offset: u64_at(&data, h + 24),
                    size: u64_at(&data, h + 32),
                    link: u32_at(&data, h + 40),
                    info: u32_at(&data, h + 44),
                    align: u64_at(&data, h + 48),
                },
            )
        };
        let strtab = raw(shstrndx).1.offset as usize;
        let sections: Vec<Shdr> = (0..shnum)
            .map(|i| {
                let (name, mut s) = raw(i);
                s.name = cstr(&data, strtab + name as usize);
                s
            })
            .collect();
        let mut symbols = Vec::new();
        if let Some(symtab) = sections.iter().find(|s| s.ty == 2) {
            let names = sections[symtab.link as usize].offset as usize;
            for k in 0..(symtab.size / 24) as usize {
                let e = symtab.offset as usize + k * 24;
                symbols.push(Sym {
                    name: cstr(&data, names + u32_at(&data, e) as usize),
                    info: data[e + 4],
                    other: data[e + 5],
                    shndx: u16_at(&data, e + 6),
                    value: u64_at(&data, e + 8),
                });
            }
        }
        Elf { data, sections, symbols }
    }

    pub fn section(&self, name: &str) -> Option<usize> {
        self.sections.iter().position(|s| s.name == name)
    }

    pub fn bytes(&self, index: usize) -> &[u8] {
        let s = &self.sections[index];
        &self.data[s.offset as usize..(s.offset + s.size) as usize]
    }

    /// The `SHT_RELA` section that applies to section `index`.
    pub fn rela_of(&self, index: usize) -> Option<usize> {
        self.sections.iter().position(|s| s.ty == 4 && s.info as usize == index)
    }

    pub fn relas(&self, rela: usize) -> Vec<Rela> {
        self.bytes(rela)
            .chunks_exact(24)
            .map(|e| {
                let info = u64_at(e, 8);
                Rela {
                    offset: u64_at(e, 0),
                    sym: (info >> 32) as u32,
                    ty: info as u32,
                    addend: u64_at(e, 16) as i64,
                }
            })
            .collect()
    }

    /// The name of the function at `addend` in the section of symbol
    /// `sym` (a section symbol), or of `sym` itself.
    pub fn function_at(&self, sym: u32, addend: i64) -> String {
        let s = &self.symbols[sym as usize];
        if s.info & 0xf != 3 {
            return s.name.clone();
        }
        self.symbols
            .iter()
            .find(|f| f.info & 0xf == 2 && f.shndx == s.shndx && f.value as i64 == addend)
            .map_or_else(|| panic!("no function at {}+{addend}", s.name), |f| f.name.clone())
    }

    /// The file with the contents of each `(section, bytes)` replaced, as
    /// [`Self::replace`] does it.
    pub fn splice(&self, replacements: &[(usize, Vec<u8>)]) -> Vec<u8> {
        let mut out = self.data.clone();
        let shoff = u64_at(&out, 0x28) as usize;
        let shentsize = usize::from(u16_at(&out, 0x3a));
        for (section, bytes) in replacements {
            out.resize(out.len().next_multiple_of(8), 0);
            let at = out.len() as u64;
            out.extend_from_slice(bytes);
            let h = shoff + section * shentsize;
            out[h + 24..h + 32].copy_from_slice(&at.to_le_bytes());
            out[h + 32..h + 40].copy_from_slice(&(bytes.len() as u64).to_le_bytes());
        }
        out
    }

    /// The file with the contents of section `index` and of its relocation
    /// section `rela` replaced: the new contents are appended at the end
    /// of the file and the two section headers point at them; every other
    /// byte of the file is unchanged (`spec/eh.md` §7.2).
    pub fn replace(&self, index: usize, rela: usize, data: &[u8], relas: &[Rela]) -> Vec<u8> {
        let mut out = self.data.clone();
        let shoff = u64_at(&out, 0x28) as usize;
        let shentsize = usize::from(u16_at(&out, 0x3a));
        let put = |out: &mut Vec<u8>, section: usize, bytes: &[u8]| {
            out.resize(out.len().next_multiple_of(8), 0);
            let at = out.len() as u64;
            out.extend_from_slice(bytes);
            let h = shoff + section * shentsize;
            out[h + 24..h + 32].copy_from_slice(&at.to_le_bytes());
            out[h + 32..h + 40].copy_from_slice(&(bytes.len() as u64).to_le_bytes());
        };
        put(&mut out, index, data);
        put(&mut out, rela, &raw_relas(relas));
        out
    }
}

/// `Elf64_Rela` entries as bytes.
pub fn raw_relas(relas: &[Rela]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(relas.len() * 24);
    for r in relas {
        raw.extend_from_slice(&r.offset.to_le_bytes());
        raw.extend_from_slice(&((u64::from(r.sym) << 32) | u64::from(r.ty)).to_le_bytes());
        raw.extend_from_slice(&r.addend.to_le_bytes());
    }
    raw
}

/// The linkers, each as the `g++` flags that select it.
pub fn linkers(scratch: &Scratch) -> Vec<(&'static str, Vec<OsString>)> {
    let mut out = Vec::new();
    for (name, program) in [("bfd", "ld.bfd"), ("lld", "ld.lld"), ("mold", "ld.mold")] {
        let Some(found) = tool(program) else { continue };
        let dir = scratch.join(format!("use-{name}"));
        std::fs::create_dir_all(&dir).unwrap();
        let link = dir.join(format!("ld.{name}"));
        if !link.exists() {
            std::os::unix::fs::symlink(&found, &link).unwrap();
        }
        let mut prefix = OsString::from("-B");
        prefix.push(&dir);
        out.push((name, vec![prefix, format!("-fuse-ld={name}").into()]));
    }
    out
}

// ------------------------------------------------------ llvm-dwarfdump ----

/// One CIE or FDE as `llvm-dwarfdump --eh-frame` prints it.
#[derive(Clone, Debug, Default)]
pub struct Entry {
    pub offset: u64,
    pub length: u64,
    pub is_cie: bool,
    /// For an FDE, the offset of its CIE.
    pub cie: u64,
    pub pc_begin: u64,
    pub pc_end: u64,
    /// `Augmentation:`, `Code alignment factor:`, ... of a CIE, without
    /// the lines whose values depend on where the entry is.
    pub header: Vec<String>,
    /// The `DW_CFA_*` lines.
    pub insns: Vec<String>,
    /// The unwind table rows (`0x4: CFA=WSP+32`, `CFA=WSP`).
    pub rows: Vec<String>,
}

pub fn dwarfdump(scratch: &Scratch, name: &str, object: &[u8]) -> Option<Vec<Entry>> {
    let dump = tool("llvm-dwarfdump")?;
    let path = scratch.write(name, object);
    let out = run(&dump, [std::ffi::OsStr::new("--eh-frame"), path.as_os_str()]);
    assert!(out.success(), "llvm-dwarfdump {name}: {}", out.stderr_text());
    Some(parse_dump(&out.stdout_text()))
}

pub fn parse_dump(text: &str) -> Vec<Entry> {
    let mut entries: Vec<Entry> = Vec::new();
    let mut in_eh = false;
    for line in text.lines() {
        if line.starts_with(".eh_frame contents:") {
            in_eh = true;
            continue;
        }
        if !in_eh || line.trim().is_empty() {
            continue;
        }
        if !line.starts_with(' ') {
            let f: Vec<&str> = line.split_whitespace().collect();
            let hex = |s: &str| u64::from_str_radix(s, 16).unwrap();
            let mut e = Entry { offset: hex(f[0]), length: hex(f[1]), ..Entry::default() };
            if f[3] == "CIE" {
                e.is_cie = true;
            } else if f[3] == "FDE" {
                e.cie = hex(f[4].trim_start_matches("cie="));
                let (a, b) = f[5].trim_start_matches("pc=").split_once("...").unwrap();
                (e.pc_begin, e.pc_end) = (hex(a), hex(b));
            } else {
                // The terminator `ZERO terminator`.
                continue;
            }
            entries.push(e);
            continue;
        }
        let e = entries.last_mut().expect("an entry header first");
        let t = line.trim();
        if t.starts_with("DW_CFA_") {
            e.insns.push(t.to_string());
        } else if t.starts_with("0x") || t.starts_with("CFA=") {
            e.rows.push(t.to_string());
        } else if !(t.starts_with("Format:")
            || t.starts_with("Personality Address:")
            || t.starts_with("LSDA Address:")
            || t.starts_with("Augmentation data:"))
        {
            e.header.push(t.to_string());
        } else if t.starts_with("Augmentation data:") {
            // The encodings, without the pointer bytes (which are the
            // unrelocated field and so depend on the layout).
            let data = t.strip_prefix("Augmentation data:").unwrap_or_default();
            let bytes: Vec<&str> = data.split_whitespace().collect();
            let (first, last) = (bytes.first().copied(), bytes.last().copied());
            e.header
                .push(format!("Augmentation encodings: {first:?} .. {last:?} ({})", bytes.len()));
        }
    }
    entries
}

/// The DWARF number of a register as `llvm-dwarfdump` names it.
pub fn reg_number(arch: Arch, name: &str) -> u16 {
    let num = |prefix: &str| name.strip_prefix(prefix).and_then(|n| n.parse::<u16>().ok());
    match arch {
        Arch::AArch64 => {
            if name == "WSP" || name == "SP" {
                return 31;
            }
            if let Some(n) = num("W").or_else(|| num("X")) {
                return n;
            }
            for p in ["B", "H", "S", "D", "Q", "V"] {
                if let Some(n) = num(p) {
                    return 64 + n;
                }
            }
            panic!("register {name}")
        }
        Arch::X86_64 => {
            const NAMES: [&str; 17] = [
                "RAX", "RDX", "RCX", "RBX", "RSI", "RDI", "RBP", "RSP", "R8", "R9", "R10", "R11",
                "R12", "R13", "R14", "R15", "RIP",
            ];
            if let Some(n) = NAMES.iter().position(|&r| r == name) {
                return n as u16;
            }
            if let Some(n) = num("XMM") {
                return 17 + n;
            }
            panic!("register {name}")
        }
    }
}

fn signed(s: &str) -> i32 {
    s.trim_start_matches('+').parse().unwrap()
}

/// Rows equivalent to an FDE's instructions. `DW_CFA_remember_state` and
/// `DW_CFA_restore_state`, which hod's rows do not have, become rows that
/// state the remembered rules again (as `spec/codegen.md` §9.3 asks of a
/// target); the second result says whether there were any.
pub fn rows_of(arch: Arch, e: &Entry) -> (Vec<Row>, bool) {
    #[derive(Clone)]
    struct State {
        cfa: (u16, i32),
        saved: BTreeMap<u16, i32>,
    }
    let initial = match arch {
        Arch::AArch64 => State { cfa: (31, 0), saved: BTreeMap::new() },
        Arch::X86_64 => State { cfa: (7, 8), saved: BTreeMap::from([(16, -8)]) },
    };
    let mut state = initial.clone();
    let mut stack = Vec::new();
    let mut rows = Vec::new();
    let mut at = 0u32;
    let mut used_state = false;
    for insn in &e.insns {
        let (op, args) = insn.split_once(':').unwrap();
        let args: Vec<&str> = args.split_whitespace().collect();
        let reg = |i: usize| reg_number(arch, args[i]);
        let mut push = |op: Op| rows.push(Row { offset: at, op });
        match op {
            "DW_CFA_advance_loc"
            | "DW_CFA_advance_loc1"
            | "DW_CFA_advance_loc2"
            | "DW_CFA_advance_loc4" => {
                let to = u64::from_str_radix(args[2].trim_start_matches("0x"), 16).unwrap();
                at = (to - e.pc_begin) as u32;
            }
            "DW_CFA_def_cfa" | "DW_CFA_def_cfa_sf" => {
                state.cfa = (reg(0), signed(args[1]));
                push(Op::DefCfa { reg: state.cfa.0, offset: state.cfa.1 });
            }
            "DW_CFA_def_cfa_offset" | "DW_CFA_def_cfa_offset_sf" => {
                state.cfa.1 = signed(args[0]);
                push(Op::DefCfaOffset(state.cfa.1));
            }
            "DW_CFA_def_cfa_register" => {
                state.cfa.0 = reg(0);
                push(Op::DefCfaRegister(state.cfa.0));
            }
            "DW_CFA_offset" | "DW_CFA_offset_extended" | "DW_CFA_offset_extended_sf" => {
                let (r, o) = (reg(0), signed(args[1]));
                state.saved.insert(r, o);
                push(Op::Offset { reg: r, cfa_offset: o });
            }
            "DW_CFA_restore" | "DW_CFA_restore_extended" => {
                let r = reg(0);
                match initial.saved.get(&r) {
                    Some(&o) => state.saved.insert(r, o),
                    None => state.saved.remove(&r),
                };
                push(Op::Restore { reg: r });
            }
            "DW_CFA_remember_state" => {
                used_state = true;
                stack.push(state.clone());
            }
            "DW_CFA_restore_state" => {
                used_state = true;
                let old = stack.pop().expect("remembered state");
                push(Op::DefCfa { reg: old.cfa.0, offset: old.cfa.1 });
                for (&r, &o) in &old.saved {
                    if state.saved.get(&r) != Some(&o) {
                        push(Op::Offset { reg: r, cfa_offset: o });
                    }
                }
                for &r in state.saved.keys() {
                    if !old.saved.contains_key(&r) {
                        push(Op::Restore { reg: r });
                    }
                }
                state = old;
            }
            "DW_CFA_nop" => {}
            other => panic!("unexpected CFI instruction {other}"),
        }
    }
    (rows, used_state)
}

// ------------------------------------------------------- the round trip --

/// The result of re-encoding llc's `.eh_frame` with `hod_eh`.
pub struct RoundTrip {
    /// llc's object with hod's `.eh_frame` and `.rela.eh_frame` in place of
    /// its own.
    pub patched: Vec<u8>,
    /// Whether llc's FDEs used `DW_CFA_remember_state`, which hod
    /// restates, so that byte identity is not expected.
    pub used_state: bool,
    /// Whether hod's section contents and relocations equal llc's byte for
    /// byte.
    pub identical: bool,
    pub fdes: usize,
    pub cies: usize,
    /// The names of the instructions llc used.
    pub ops: std::collections::BTreeSet<String>,
}

/// One relocation of `.eh_frame`, by what it means: the entry's index,
/// which field, the function or symbol it names, the addend and the type.
type Meaning = (usize, &'static str, String, i64, u32);

fn meanings(elf: &Elf, entries: &[Entry], relas: &[Rela]) -> Vec<Meaning> {
    let mut out = Vec::new();
    for r in relas {
        let (i, e) = entries
            .iter()
            .enumerate()
            .find(|(_, e)| r.offset >= e.offset && r.offset < e.offset + e.length + 4)
            .expect("relocation in an entry");
        let field = match (e.is_cie, r.offset - e.offset) {
            (true, _) => "personality",
            (false, 8) => "pc",
            (false, _) => "lsda",
        };
        let target = if field == "pc" {
            elf.function_at(r.sym, r.addend)
        } else {
            elf.symbols[r.sym as usize].name.clone()
        };
        out.push((i, field, target, r.addend, r.ty));
    }
    out
}

/// Re-encodes the `.eh_frame` of `object` (compiled by llc from `ir`) with
/// `hod_eh`, splices it in, and checks what `spec/eh.md` §7.1 asks:
/// the section's type, flags and alignment, the relocations by meaning,
/// the CIEs and FDEs decoded by `llvm-dwarfdump` (augmentation, factors,
/// rules at every code offset), and byte identity when llc used no
/// remembered state. `None` when `llvm-dwarfdump` is missing.
pub fn round_trip(
    scratch: &Scratch,
    label: &str,
    arch: Arch,
    ir: &str,
    object: &[u8],
    reverse: bool,
    lsda_of: Option<&dyn Fn(&str) -> Lsda>,
) -> Option<RoundTrip> {
    let original = dwarfdump(scratch, &format!("{label}.orig.o"), object)?;
    let elf = Elf::parse(object.to_vec());
    let eh = elf.section(".eh_frame").expect(".eh_frame");
    let rela = elf.rela_of(eh).expect(".rela.eh_frame");
    let relas = elf.relas(rela);
    let reloc_at = |offset: u64| relas.iter().find(|r| r.offset == offset).copied();
    // The functions' module order: their `define`s in the IR.
    let order_of = |name: &str| {
        let (plain, quoted) = (format!("@{name}("), format!("@\"{name}\"("));
        ir.lines()
            .filter(|l| l.starts_with("define "))
            .position(|l| l.contains(&plain) || l.contains(&quoted))
            .unwrap_or_else(|| panic!("a definition of {name} in the IR")) as u32
    };

    // Placeholder symbols that stand for llc's symbol table entries 1..n.
    let mut obj = Object::new(arch);
    for _ in 1..elf.symbols.len() {
        obj.add_symbol(Symbol::undefined(StrId::EMPTY));
    }
    let sym = |index: u32| SymbolId::from_index(index - 1);

    let mut rows_store: Vec<Vec<Row>> = Vec::new();
    let mut inputs = Vec::new();
    let mut used_state = false;
    for e in original.iter().filter(|e| !e.is_cie) {
        let cie = original.iter().find(|c| c.is_cie && c.offset == e.cie).unwrap();
        let pc = reloc_at(e.offset + 8).expect("pc relocation");
        let personality = relas
            .iter()
            .find(|r| r.offset > cie.offset && r.offset < cie.offset + cie.length + 4)
            .map(|r| sym(r.sym));
        let mut lsda =
            reloc_at(e.offset + 17).map(|r| Lsda { symbol: sym(r.sym), addend: r.addend });
        if let (Some(lsda_of), Some(l)) = (lsda_of, lsda.as_mut()) {
            *l = lsda_of(&elf.function_at(pc.sym, pc.addend));
        }
        let (rows, state) = rows_of(arch, e);
        used_state |= state;
        rows_store.push(rows);
        inputs.push((
            order_of(&elf.function_at(pc.sym, pc.addend)),
            sym(pc.sym),
            pc.addend,
            (e.pc_end - e.pc_begin) as u32,
            personality,
            lsda,
        ));
    }
    let mut fdes: Vec<Fde<'_>> = inputs
        .iter()
        .zip(&rows_store)
        .map(|(&(order, start, start_addend, size, personality, lsda), rows)| Fde {
            order,
            start,
            start_addend,
            size,
            rows,
            personality,
            lsda,
        })
        .collect();
    if reverse {
        fdes.reverse();
    }
    let section = write_eh_frame(&mut obj, &fdes).expect("hod_eh").expect("a section");

    // The section header (§6).
    let llc = &elf.sections[eh];
    assert_eq!(
        obj.section_kind(section),
        SectionKind::Elf { sh_type: llc.ty, sh_flags: llc.flags, sh_entsize: 0 },
        "{label}: section type and flags"
    );
    assert_eq!(obj.section_align(section), llc.align, "{label}: section alignment");

    let data = obj.data(section).to_vec();
    let new_relas: Vec<Rela> = obj
        .relocations(section)
        .map(|r| Rela {
            offset: r.offset,
            sym: r.symbol.index() + 1,
            ty: r.kind.elf_type(),
            addend: r.addend,
        })
        .collect();
    let identical = data == elf.bytes(eh) && new_relas == relas;
    if !used_state {
        assert_eq!(hex(&data), hex(elf.bytes(eh)), "{label}: section bytes");
        assert_eq!(new_relas, relas, "{label}: relocations");
    }
    let patched = elf.replace(eh, rela, &data, &new_relas);
    let decoded = dwarfdump(scratch, &format!("{label}.hod.o"), &patched)?;

    // Decoded equality: the same entries in the same order, each CIE with
    // the same header and rules, each FDE with the same CIE (by index),
    // range and rules at every offset.
    assert_eq!(decoded.len(), original.len(), "{label}: entry count");
    let cie_index = |entries: &[Entry], at: u64| entries.iter().position(|c| c.offset == at);
    for (i, (a, b)) in original.iter().zip(&decoded).enumerate() {
        assert_eq!(a.is_cie, b.is_cie, "{label}: entry {i} kind");
        assert_eq!(a.header, b.header, "{label}: entry {i} header");
        assert_eq!((a.pc_begin, a.pc_end), (b.pc_begin, b.pc_end), "{label}: entry {i} range");
        assert_eq!(a.rows, b.rows, "{label}: entry {i} rules");
        if !a.is_cie {
            assert_eq!(
                cie_index(&original, a.cie),
                cie_index(&decoded, b.cie),
                "{label}: entry {i} CIE"
            );
        }
    }
    let patched_elf = Elf::parse(patched.clone());
    assert_eq!(
        meanings(&elf, &original, &relas),
        meanings(&patched_elf, &decoded, &new_relas),
        "{label}: relocations by meaning"
    );
    Some(RoundTrip {
        patched,
        used_state,
        identical,
        fdes: fdes.len(),
        cies: original.iter().filter(|e| e.is_cie).count(),
        ops: original
            .iter()
            .flat_map(|e| e.insns.iter().map(|i| i.split(':').next().unwrap().to_string()))
            .collect(),
    })
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `llc-<major>`, or a skip.
pub fn llc(major: u32) -> Option<PathBuf> {
    tool(&format!("llc-{major}"))
}

/// Compiles LLVM text with `llc` at the oracle's flags.
pub fn compile(llc: &Path, scratch: &Scratch, name: &str, ir: &str, triple: &str) -> Vec<u8> {
    compile_with(llc, scratch, name, ir, triple, &[])
}

/// [`compile`] with more `llc` flags.
pub fn compile_with(
    llc: &Path,
    scratch: &Scratch,
    name: &str,
    ir: &str,
    triple: &str,
    extra: &[&str],
) -> Vec<u8> {
    let input = scratch.write(&format!("{name}.ll"), ir);
    let output = scratch.join(format!("{name}.o"));
    let mut args: Vec<OsString> = LLC_FLAGS.iter().map(Into::into).collect();
    args.push(format!("-mtriple={triple}").into());
    args.extend(extra.iter().map(Into::into));
    args.push(input.into());
    args.push("-o".into());
    args.push(output.clone().into());
    let out = run(llc, &args);
    assert!(out.success(), "llc {name}: {}", out.stderr_text());
    std::fs::read(output).unwrap()
}
