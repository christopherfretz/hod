//! Shared by the oracle tests (`spec/dwarf.md` §8): a small ELF64 reader
//! and patcher, a reader of `llvm-dwarfdump --debug-line` output, the
//! decoding of `llc`'s line table into `hod_dwarf`'s input, and the splice
//! of `hod_dwarf`'s sections into `llc`'s object.

#![allow(dead_code, clippy::too_many_lines, clippy::format_push_string, clippy::format_collect)]

pub mod generate;

use std::path::{Path, PathBuf};

use hod_dwarf::{
    FileEntry, FunctionLines, LineHeader, Row, Sequence, SourceLoc, Transition, flags,
    function_rows, write_debug_line,
};
use hod_obj::{Arch, Object, StrId, Symbol, SymbolId};
use hod_testutil::{Scratch, run, tool};

pub const LLC_FLAGS: [&str; 5] =
    ["-O0", "-global-isel=0", "-fast-isel=0", "-relocation-model=pic", "-filetype=obj"];

pub const TRIPLES: [(Arch, &str); 2] =
    [(Arch::AArch64, "aarch64-unknown-linux-gnu"), (Arch::X86_64, "x86_64-unknown-linux-gnu")];

// ---------------------------------------------------------------- ELF ----

#[derive(Clone, Debug)]
pub struct Shdr {
    pub name: String,
    pub ty: u32,
    pub flags: u64,
    pub offset: u64,
    pub size: u64,
    pub info: u32,
    pub align: u64,
    pub entsize: u64,
}

#[derive(Clone, Debug)]
pub struct Sym {
    pub name: String,
    pub info: u8,
    pub shndx: u16,
    pub value: u64,
    pub size: u64,
}

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
                u32_at(&data, h + 40),
                Shdr {
                    name: String::new(),
                    ty: u32_at(&data, h + 4),
                    flags: u64_at(&data, h + 8),
                    offset: u64_at(&data, h + 24),
                    size: u64_at(&data, h + 32),
                    info: u32_at(&data, h + 44),
                    align: u64_at(&data, h + 48),
                    entsize: u64_at(&data, h + 56),
                },
            )
        };
        let strtab = raw(shstrndx).2.offset as usize;
        let mut links = Vec::new();
        let sections: Vec<Shdr> = (0..shnum)
            .map(|i| {
                let (name, link, mut s) = raw(i);
                s.name = cstr(&data, strtab + name as usize);
                links.push(link);
                s
            })
            .collect();
        let mut symbols = Vec::new();
        if let Some(symtab) = sections.iter().position(|s| s.ty == 2) {
            let names = sections[links[symtab] as usize].offset as usize;
            let s = &sections[symtab];
            for k in 0..(s.size / 24) as usize {
                let e = s.offset as usize + k * 24;
                symbols.push(Sym {
                    name: cstr(&data, names + u32_at(&data, e) as usize),
                    info: data[e + 4],
                    shndx: u16_at(&data, e + 6),
                    value: u64_at(&data, e + 8),
                    size: u64_at(&data, e + 16),
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

    /// The index of the section symbol of section `shndx`.
    pub fn section_symbol(&self, shndx: usize) -> u32 {
        self.symbols
            .iter()
            .position(|s| s.info & 0xf == 3 && usize::from(s.shndx) == shndx)
            .expect("a section symbol") as u32
    }

    /// The file with the contents of some sections replaced: the new
    /// contents are appended at the end and the section headers point at
    /// them; every other byte is unchanged.
    pub fn replace(&self, parts: &[(usize, &[u8])]) -> Vec<u8> {
        let mut out = self.data.clone();
        let shoff = u64_at(&out, 0x28) as usize;
        let shentsize = usize::from(u16_at(&out, 0x3a));
        for &(section, bytes) in parts {
            out.resize(out.len().next_multiple_of(8), 0);
            let at = out.len() as u64;
            out.extend_from_slice(bytes);
            let h = shoff + section * shentsize;
            out[h + 24..h + 32].copy_from_slice(&at.to_le_bytes());
            out[h + 32..h + 40].copy_from_slice(&(bytes.len() as u64).to_le_bytes());
        }
        out
    }
}

pub fn rela_bytes(relas: &[Rela]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(relas.len() * 24);
    for r in relas {
        raw.extend_from_slice(&r.offset.to_le_bytes());
        raw.extend_from_slice(&((u64::from(r.sym) << 32) | u64::from(r.ty)).to_le_bytes());
        raw.extend_from_slice(&r.addend.to_le_bytes());
    }
    raw
}

// ------------------------------------------------------ llvm-dwarfdump ----

/// A decoded row as `llvm-dwarfdump --debug-line` prints it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DumpRow {
    pub address: u64,
    pub line: u32,
    pub column: u16,
    pub file: u32,
    pub flags: u8,
    pub end: bool,
}

#[derive(Clone, Debug, Default)]
pub struct DumpFile {
    pub name: String,
    pub dir: u32,
    pub md5: Option<[u8; 16]>,
}

/// One line-table unit as `llvm-dwarfdump --debug-line` prints it.
#[derive(Clone, Debug, Default)]
pub struct LineDump {
    pub dirs: Vec<String>,
    pub files: Vec<DumpFile>,
    pub rows: Vec<DumpRow>,
}

fn quoted(s: &str) -> String {
    let s = s.trim();
    s.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(s).to_string()
}

pub fn parse_dump(text: &str) -> LineDump {
    let mut d = LineDump::default();
    let mut in_rows = false;
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("include_directories[") {
            d.dirs.push(quoted(rest.split_once('=').unwrap().1));
        } else if t.starts_with("file_names[") {
            d.files.push(DumpFile::default());
        } else if let Some(rest) = t.strip_prefix("name:") {
            d.files.last_mut().unwrap().name = quoted(rest);
        } else if let Some(rest) = t.strip_prefix("dir_index:") {
            d.files.last_mut().unwrap().dir = rest.trim().parse().unwrap();
        } else if let Some(rest) = t.strip_prefix("md5_checksum:") {
            let hex = rest.trim();
            let mut sum = [0u8; 16];
            for (i, b) in sum.iter_mut().enumerate() {
                *b = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap();
            }
            d.files.last_mut().unwrap().md5 = Some(sum);
        } else if t.starts_with("Address") {
            in_rows = true;
        } else if in_rows && t.starts_with("0x") {
            let w: Vec<&str> = t.split_whitespace().collect();
            let mut flags = 0;
            let mut end = false;
            for f in &w[7..] {
                match *f {
                    "is_stmt" => flags |= flags::IS_STMT,
                    "prologue_end" => flags |= flags::PROLOGUE_END,
                    "epilogue_begin" => flags |= flags::EPILOGUE_BEGIN,
                    "end_sequence" => end = true,
                    other => panic!("unexpected flag {other}"),
                }
            }
            d.rows.push(DumpRow {
                address: u64::from_str_radix(&w[0][2..], 16).unwrap(),
                line: w[1].parse().unwrap(),
                column: w[2].parse().unwrap(),
                file: w[3].parse().unwrap(),
                flags,
                end,
            });
        } else if in_rows && t.is_empty() {
            // the end of the unit's rows
        }
    }
    d
}

/// `llvm-dwarfdump --debug-line` of an object, or `None` when the tool is
/// missing.
pub fn dump_lines(dwarfdump: &Path, scratch: &Scratch, name: &str, object: &[u8]) -> LineDump {
    let path = scratch.write(name, object);
    let out = run(dwarfdump, [std::ffi::OsString::from("--debug-line"), path.into()]);
    assert!(out.success(), "llvm-dwarfdump {name}: {}", out.stderr_text());
    parse_dump(&out.stdout_text())
}

/// `llvm-dwarfdump --verify --debug-line` must be clean.
pub fn verify(dwarfdump: &Path, scratch: &Scratch, name: &str, object: &[u8]) {
    let path = scratch.write(name, object);
    let out = run(dwarfdump, ["--verify".into(), "--debug-line".into(), path.into_os_string()]);
    assert!(
        out.success() && out.stdout_text().contains("No errors."),
        "llvm-dwarfdump --verify {name}:\n{}{}",
        out.stdout_text(),
        out.stderr_text()
    );
}

pub fn dwarfdumps() -> Vec<(u32, PathBuf)> {
    [19, 22]
        .into_iter()
        .filter_map(|m| tool(&format!("llvm-dwarfdump-{m}")).map(|p| (m, p)))
        .collect()
}

// ----------------------------------------------- llc's table, decoded ----

/// One sequence of `llc`'s table: the code section it is in (by index),
/// its start in that section, its size and rows (offsets from its start).
#[derive(Clone, Debug)]
pub struct LlcSequence {
    pub section: usize,
    pub symbol: u32,
    pub start: i64,
    pub size: u32,
    pub rows: Vec<Row>,
}

/// `llc`'s line table, decoded into `hod_dwarf`'s input.
pub struct LlcTable {
    pub elf: Elf,
    pub line: usize,
    pub line_rela: usize,
    pub line_str: usize,
    pub dump: LineDump,
    pub dirs: Vec<Vec<u8>>,
    pub files: Vec<(Vec<u8>, u32, Option<[u8; 16]>)>,
    pub seqs: Vec<LlcSequence>,
}

pub fn decode(dwarfdump: &Path, scratch: &Scratch, label: &str, object: &[u8]) -> LlcTable {
    let dump = dump_lines(dwarfdump, scratch, &format!("{label}.orig.o"), object);
    let elf = Elf::parse(object.to_vec());
    let line = elf.section(".debug_line").expect(".debug_line");
    let line_rela = elf.rela_of(line).expect(".rela.debug_line");
    let line_str = elf.section(".debug_line_str").expect(".debug_line_str");
    let relas = elf.relas(line_rela);
    // The address relocations, in order: one per sequence.
    let addresses: Vec<Rela> = relas.iter().filter(|r| matches!(r.ty, 257 | 1)).copied().collect();
    let mut seqs = Vec::new();
    let mut current: Vec<DumpRow> = Vec::new();
    for r in &dump.rows {
        if r.end {
            let base = current[0].address;
            let rel = addresses[seqs.len()];
            assert_eq!(rel.addend as u64, base, "{label}: set_address of sequence {}", seqs.len());
            seqs.push(LlcSequence {
                section: usize::from(elf.symbols[rel.sym as usize].shndx),
                symbol: rel.sym,
                start: rel.addend,
                size: (r.address - base) as u32,
                rows: current
                    .iter()
                    .map(|c| Row {
                        offset: (c.address - base) as u32,
                        file: c.file,
                        line: c.line,
                        column: c.column,
                        flags: c.flags,
                    })
                    .collect(),
            });
            current.clear();
        } else {
            current.push(*r);
        }
    }
    assert!(current.is_empty(), "{label}: rows after the last end_sequence");
    assert_eq!(seqs.len(), addresses.len(), "{label}: one address relocation per sequence");
    LlcTable {
        dirs: dump.dirs.iter().map(|d| d.as_bytes().to_vec()).collect(),
        files: dump.files.iter().map(|f| (f.name.as_bytes().to_vec(), f.dir, f.md5)).collect(),
        elf,
        line,
        line_rela,
        line_str,
        dump,
        seqs,
    }
}

/// What `write_debug_line` produced, in `llc`'s symbol numbering.
pub struct Written {
    pub line: Vec<u8>,
    pub line_str: Vec<u8>,
    pub relas: Vec<Rela>,
    pub line_kind: hod_obj::SectionKind,
    pub str_kind: hod_obj::SectionKind,
    pub aligns: (u64, u64),
}

/// Writes a line table for `seqs` (each `(llc symbol, addend, size,
/// rows)`) with `llc`'s header, into an object whose symbols stand for
/// `llc`'s, and maps the relocations back to `llc`'s symbol numbers.
pub fn write(
    table: &LlcTable,
    arch: Arch,
    seqs: &[(u32, i64, u32, Vec<Row>)],
    reverse: bool,
) -> Written {
    let mut obj = Object::new(arch);
    let n = table.elf.symbols.len() as u32;
    for _ in 1..n {
        obj.add_symbol(Symbol::undefined(StrId::EMPTY));
    }
    let dirs: Vec<&[u8]> = table.dirs.iter().map(Vec::as_slice).collect();
    let files: Vec<FileEntry<'_>> = table
        .files
        .iter()
        .map(|(name, dir, md5)| FileEntry { name, dir: *dir, md5: *md5 })
        .collect();
    let mut input: Vec<Sequence<'_>> = seqs
        .iter()
        .enumerate()
        .map(|(i, (sym, addend, size, rows))| Sequence {
            order: i as u32,
            symbol: SymbolId::from_index(sym - 1),
            addend: *addend,
            size: *size,
            rows,
        })
        .collect();
    if reverse {
        input.reverse();
    }
    let header = LineHeader { dirs: &dirs, files: &files };
    let out = write_debug_line(&mut obj, &header, &input).expect("hod_dwarf");
    let str_sym = table.elf.section_symbol(table.line_str);
    let relas = obj
        .relocations(out.line)
        .map(|r| {
            let index = r.symbol.index() + 1;
            Rela {
                offset: r.offset,
                sym: if index >= n { str_sym } else { index },
                ty: r.kind.elf_type(),
                addend: r.addend,
            }
        })
        .collect();
    Written {
        line: obj.data(out.line).to_vec(),
        line_str: obj.data(out.line_str).to_vec(),
        relas,
        line_kind: obj.section_kind(out.line),
        str_kind: obj.section_kind(out.line_str),
        aligns: (obj.section_align(out.line), obj.section_align(out.line_str)),
    }
}

/// `llc`'s object with hod's line table in place of its own.
pub fn splice(table: &LlcTable, w: &Written) -> Vec<u8> {
    let rela = rela_bytes(&w.relas);
    table.elf.replace(&[
        (table.line, &w.line),
        (table.line_rela, &rela),
        (table.line_str, &w.line_str),
    ])
}

// -------------------------------------------------- per-function view ----

/// A function of `llc`'s object and its rows, taken from the sequence
/// that covers it.
#[derive(Clone, Debug)]
pub struct LlcFunction {
    pub name: String,
    pub section: usize,
    pub symbol: u32,
    pub start: i64,
    pub size: u32,
    pub rows: Vec<Row>,
}

pub fn functions(table: &LlcTable) -> Vec<LlcFunction> {
    let mut out = Vec::new();
    for s in &table.elf.symbols {
        if s.info & 0xf != 2 || s.size == 0 {
            continue;
        }
        let section = usize::from(s.shndx);
        let lo = s.value as i64;
        let hi = lo + s.size as i64;
        let Some(seq) = table
            .seqs
            .iter()
            .find(|q| q.section == section && q.start <= lo && lo < q.start + i64::from(q.size))
        else {
            continue; // no line information
        };
        let rows = seq
            .rows
            .iter()
            .filter(|r| {
                let at = seq.start + i64::from(r.offset);
                lo <= at && at < hi
            })
            .map(|r| Row { offset: (seq.start + i64::from(r.offset) - lo) as u32, ..*r })
            .collect();
        out.push(LlcFunction {
            name: s.name.clone(),
            section,
            symbol: seq.symbol,
            start: lo,
            size: s.size as u32,
            rows,
        });
    }
    out.sort_by_key(|f| (f.section, f.start));
    out
}

/// `llc`'s rows of one function turned back into what `hod_cg` records
/// (`spec/dwarf.md` §8.2): the start row dropped and its line taken as
/// the scope, the `epilogue_begin` row as the epilogue.
pub fn transitions_of(f: &LlcFunction) -> (FunctionLines<'static>, Vec<Transition>) {
    let mut rows: &[Row] = &f.rows;
    let mut scope = SourceLoc { file: rows[0].file, line: rows[0].line, column: 0 };
    if rows[0].offset == 0 && rows[0].flags & flags::PROLOGUE_END == 0 && rows[0].column == 0 {
        scope = SourceLoc { file: rows[0].file, line: rows[0].line, column: 0 };
        rows = &rows[1..];
    }
    let epilogue = rows.last().is_some_and(|r| r.flags & flags::EPILOGUE_BEGIN != 0);
    let ts = rows
        .iter()
        .map(|r| Transition {
            offset: r.offset,
            loc: Some(SourceLoc { file: r.file, line: r.line, column: r.column }),
        })
        .collect();
    (FunctionLines { scope, size: f.size, transitions: &[], epilogue }, ts)
}

/// The rows `hod_dwarf`'s policy gives a function of `llc`'s object.
pub fn policy_rows(f: &LlcFunction) -> Vec<Row> {
    let (lines, ts) = transitions_of(f);
    let mut out = Vec::new();
    function_rows(&FunctionLines { transitions: &ts, ..lines }, &mut out).expect("policy");
    out
}

/// `llc`'s rows with the differences `spec/dwarf.md` §5 names applied:
/// a change of file at the same line is a statement, and a row that
/// repeats the previous one's position without a new flag is merged.
/// Also returns how many rows each difference changed: (statements
/// added, rows merged).
pub fn with_named_differences(rows: &[Row]) -> (Vec<Row>, usize, usize) {
    let mut out: Vec<Row> = Vec::new();
    let (mut stmts, mut merged) = (0, 0);
    let mut last: Option<(u32, u32)> = None;
    for r in rows {
        let mut r = *r;
        if r.line != 0 {
            if last.is_some_and(|(file, line)| line == r.line && file != r.file)
                && r.flags & flags::IS_STMT == 0
            {
                r.flags |= flags::IS_STMT;
                stmts += 1;
            }
            last = Some((r.file, r.line));
        }
        if let Some(p) = out.last()
            && (p.file, p.line, p.column) == (r.file, r.line, r.column)
            && r.flags & !p.flags == 0
        {
            merged += 1;
            continue;
        }
        out.push(r);
    }
    (out, stmts, merged)
}

// -------------------------------------------------------------- llc ----

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn llc(major: u32) -> Option<PathBuf> {
    tool(&format!("llc-{major}"))
}

pub fn compile(llc: &Path, scratch: &Scratch, name: &str, ir: &str, triple: &str) -> Vec<u8> {
    let input = scratch.write(&format!("{name}.ll"), ir);
    let output = scratch.join(format!("{name}.o"));
    let mut args: Vec<std::ffi::OsString> = LLC_FLAGS.iter().map(Into::into).collect();
    args.push(format!("-mtriple={triple}").into());
    args.push(input.into());
    args.push("-o".into());
    args.push(output.clone().into());
    let out = run(llc, &args);
    assert!(out.success(), "llc {name}: {}", out.stderr_text());
    std::fs::read(output).unwrap()
}
