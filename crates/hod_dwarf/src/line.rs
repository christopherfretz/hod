//! The DWARF 5 line table: `.debug_line` and `.debug_line_str`
//! (`spec/dwarf.md` §4).

use std::collections::HashMap;

use hod_obj::elf::{SHF_MERGE, SHF_STRINGS, SHT_PROGBITS};
use hod_obj::{Arch, Object, SectionId, SectionKind, SymbolId};

use crate::Error;
use crate::leb::{write_sleb, write_uleb};
use crate::target::{
    LINE_BASE, LINE_RANGE, LineTarget, OPCODE_BASE, STANDARD_OPCODE_LENGTHS, for_arch,
};

/// The flags of a line-table row (DWARF 5 §6.2.2).
pub mod flags {
    /// The row is a recommended breakpoint location (`is_stmt`).
    pub const IS_STMT: u8 = 1;
    /// The row is where the function's prologue ends (`prologue_end`).
    pub const PROLOGUE_END: u8 = 2;
    /// The row is where the function's epilogue begins (`epilogue_begin`).
    pub const EPILOGUE_BEGIN: u8 = 4;
    /// Every flag.
    pub const ALL: u8 = IS_STMT | PROLOGUE_END | EPILOGUE_BEGIN;
}

/// One row of the line-number matrix: from `offset` on (relative to its
/// sequence's start), the code is at `line`:`column` of `file`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Row {
    /// The code offset from the sequence's `symbol + addend`.
    pub offset: u32,
    /// An index of the header's `files`.
    pub file: u32,
    /// The line, 1-based; 0 for "no source line".
    pub line: u32,
    /// The column, 1-based; 0 for none.
    pub column: u16,
    /// [`flags`].
    pub flags: u8,
}

/// A contiguous run of code with its rows, ended by `DW_LNE_end_sequence`
/// at `size`.
#[derive(Clone, Copy, Debug)]
pub struct Sequence<'a> {
    /// The position of the sequence in the table: sequences are written in
    /// increasing `order`, whatever their order in the input.
    pub order: u32,
    /// What the sequence's addresses are relative to: the section symbol
    /// of the code's section, as `llc` does it.
    pub symbol: SymbolId,
    /// The offset of the sequence's start from `symbol`.
    pub addend: i64,
    /// The length of the code in bytes; `end_sequence` is at this offset.
    pub size: u32,
    /// The rows, in code order, each below `size`.
    pub rows: &'a [Row],
}

/// A file of the line-table header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileEntry<'a> {
    /// The file's name (`DW_LNCT_path`).
    pub name: &'a [u8],
    /// An index of the header's `dirs` (`DW_LNCT_directory_index`).
    pub dir: u32,
    /// The file's MD5 (`DW_LNCT_MD5`), written only when every file has
    /// one.
    pub md5: Option<[u8; 16]>,
    /// The file's embedded source text (LLVM's `DW_LNCT_LLVM_source`),
    /// written for every file, as an empty string for a file without
    /// one, when some file has one.
    pub source: Option<&'a [u8]>,
}

/// The directory and file tables of a line-table unit. Entry 0 of each
/// is the compilation directory and the primary source file.
#[derive(Clone, Copy, Debug)]
pub struct LineHeader<'a> {
    /// The directories.
    pub dirs: &'a [&'a [u8]],
    /// The files.
    pub files: &'a [FileEntry<'a>],
}

/// The sections [`write_debug_line`] made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineSections {
    /// `.debug_line`.
    pub line: SectionId,
    /// `.debug_line_str`.
    pub line_str: SectionId,
}

// Standard and extended opcodes (DWARF 5 §7.22).
const DW_LNS_COPY: u8 = 0x01;
const DW_LNS_ADVANCE_PC: u8 = 0x02;
const DW_LNS_ADVANCE_LINE: u8 = 0x03;
const DW_LNS_SET_FILE: u8 = 0x04;
const DW_LNS_SET_COLUMN: u8 = 0x05;
const DW_LNS_NEGATE_STMT: u8 = 0x06;
const DW_LNS_CONST_ADD_PC: u8 = 0x08;
const DW_LNS_SET_PROLOGUE_END: u8 = 0x0a;
const DW_LNS_SET_EPILOGUE_BEGIN: u8 = 0x0b;
const DW_LNE_END_SEQUENCE: u8 = 0x01;
const DW_LNE_SET_ADDRESS: u8 = 0x02;
// Content types and forms (DWARF 5 §7.5.6, §7.22).
const DW_LNCT_PATH: u8 = 0x1;
const DW_LNCT_DIRECTORY_INDEX: u8 = 0x2;
const DW_LNCT_MD5: u8 = 0x5;
/// LLVM's `DW_LNCT_LLVM_source`: a file's embedded source text.
const DW_LNCT_LLVM_SOURCE: u64 = 0x2001;
const DW_FORM_LINE_STRP: u8 = 0x1f;
const DW_FORM_UDATA: u8 = 0x0f;
const DW_FORM_DATA16: u8 = 0x1e;

/// The largest address advance a special opcode encodes with a line
/// advance of 0: `(255 - opcode_base) / line_range`.
const MAX_SPECIAL_ADDR: u64 = (255 - OPCODE_BASE as u64) / LINE_RANGE as u64;

/// Appends the advance of `line` lines and `addr` address units and the
/// row it makes, as LLVM's `MCDwarfLineAddr::encode` does it
/// (`spec/dwarf.md` §4.5 step 3).
#[inline]
pub(crate) fn write_advance(out: &mut Vec<u8>, line: i64, addr: u64) {
    let base = i64::from(LINE_BASE);
    let range = i64::from(LINE_RANGE);
    let mut biased = line - base;
    let mut line = line;
    let mut need_copy = false;
    if !(0..range).contains(&biased) || biased + i64::from(OPCODE_BASE) > 255 {
        out.push(DW_LNS_ADVANCE_LINE);
        write_sleb(out, line);
        line = 0;
        biased = -base;
        need_copy = true;
    }
    if line == 0 && addr == 0 {
        out.push(DW_LNS_COPY);
        return;
    }
    // `biased` is in 0..line_range here.
    let op = biased as u64 + u64::from(OPCODE_BASE);
    if addr < 256 + MAX_SPECIAL_ADDR {
        let special = op + addr * u64::from(LINE_RANGE);
        if special <= 255 {
            out.push(special as u8);
            return;
        }
        if addr >= MAX_SPECIAL_ADDR {
            let special = op + (addr - MAX_SPECIAL_ADDR) * u64::from(LINE_RANGE);
            if special <= 255 {
                out.push(DW_LNS_CONST_ADD_PC);
                out.push(special as u8);
                return;
            }
        }
    }
    out.push(DW_LNS_ADVANCE_PC);
    write_uleb(out, addr);
    out.push(if need_copy { DW_LNS_COPY } else { op as u8 });
}

/// Appends the end of a sequence `addr` units after its last row (§4.5
/// step 4).
fn write_end(out: &mut Vec<u8>, addr: u64) {
    if addr == MAX_SPECIAL_ADDR {
        out.push(DW_LNS_CONST_ADD_PC);
    } else if addr != 0 {
        out.push(DW_LNS_ADVANCE_PC);
        write_uleb(out, addr);
    }
    out.extend_from_slice(&[0, 1, DW_LNE_END_SEQUENCE]);
}

/// A relocation to add once the sections exist.
#[derive(Clone, Copy)]
struct Pending {
    offset: u32,
    /// `None`: against `.debug_line_str`; `Some`: an address.
    symbol: Option<SymbolId>,
    addend: i64,
}

/// Checks a sequence's rows (§4.6).
fn check_sequence(seq: &Sequence<'_>, files: usize, min: u32) -> Result<(), Error> {
    let order = seq.order;
    if !seq.size.is_multiple_of(min) {
        return Err(Error::Unaligned { order, row: seq.rows.len() as u32 });
    }
    let mut prev = 0;
    for (i, r) in seq.rows.iter().enumerate() {
        let row = i as u32;
        if r.offset < prev {
            return Err(Error::RowOutOfOrder { order, row });
        }
        if r.offset >= seq.size {
            return Err(Error::RowBeyondEnd { order, row });
        }
        if !r.offset.is_multiple_of(min) {
            return Err(Error::Unaligned { order, row });
        }
        if r.file as usize >= files {
            return Err(Error::BadFile { order, row });
        }
        if r.flags & !flags::ALL != 0 {
            return Err(Error::BadFlags { order, row });
        }
        prev = r.offset;
    }
    Ok(())
}

/// An address field of an encoded program: its offset in the program's
/// bytes, and what it is relative to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct AddrField {
    offset: u32,
    symbol: SymbolId,
    addend: i64,
}

/// The program of one line-table unit (`spec/dwarf.md` §4.5), encoded
/// one sequence at a time (§7.1).
///
/// A sequence's bytes depend only on its own rows (each sequence starts
/// from the state machine's initial state), and its one address is a
/// relocation recorded relative to the program's start. So a worker can
/// encode the sequences of its functions into a program of its own and
/// the merge can [`append`](LineProgram::append) the programs in module
/// order: the bytes are those of encoding every sequence into one
/// program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineProgram {
    target: &'static LineTarget,
    bytes: Vec<u8>,
    addrs: Vec<AddrField>,
}

impl LineProgram {
    /// An empty program for `arch`.
    pub fn new(arch: Arch) -> Self {
        Self { target: for_arch(arch), bytes: Vec::new(), addrs: Vec::new() }
    }

    /// An empty program with room for about `rows` rows in `seqs`
    /// sequences.
    pub fn with_capacity(arch: Arch, rows: usize, seqs: usize) -> Self {
        Self {
            target: for_arch(arch),
            bytes: Vec::with_capacity(rows * 5 + seqs * 16),
            addrs: Vec::with_capacity(seqs),
        }
    }

    /// The architecture.
    pub fn arch(&self) -> Arch {
        self.target.arch
    }

    /// Empties the program, keeping its allocations.
    pub fn clear(&mut self) {
        self.bytes.clear();
        self.addrs.clear();
    }

    /// The encoded bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The number of sequences written (a sequence without rows writes
    /// nothing and is not counted).
    pub fn sequence_count(&self) -> usize {
        self.addrs.len()
    }

    /// Appends `seq`'s program, for a header of `files` files. Its
    /// `order` is used only to name it in an error: sequences are in the
    /// order they are pushed.
    ///
    /// # Errors
    /// `BadFile`, `RowOutOfOrder`, `RowBeyondEnd`, `Unaligned`,
    /// `BadFlags` (§4.6), `TooLarge`; the program is then unchanged.
    pub fn push(&mut self, files: usize, seq: &Sequence<'_>) -> Result<(), Error> {
        check_sequence(seq, files, u32::from(self.target.min_inst_length))?;
        let len = self.bytes.len();
        let result = self.encode(seq);
        if result.is_err() {
            self.bytes.truncate(len);
        }
        result
    }

    /// Appends `other`'s sequences after this program's.
    ///
    /// # Errors
    /// `TooLarge`; `ArchMismatch` if the two are for different
    /// architectures. The program is then unchanged.
    pub fn append(&mut self, other: &LineProgram) -> Result<(), Error> {
        if other.target.arch != self.target.arch {
            return Err(Error::ArchMismatch);
        }
        let base = u32::try_from(self.bytes.len()).map_err(|_| Error::TooLarge)?;
        if u32::try_from(self.bytes.len() + other.bytes.len()).is_err() {
            return Err(Error::TooLarge);
        }
        self.bytes.extend_from_slice(&other.bytes);
        self.addrs.extend(other.addrs.iter().map(|a| AddrField { offset: base + a.offset, ..*a }));
        Ok(())
    }

    /// Appends one checked sequence (§4.5).
    fn encode(&mut self, seq: &Sequence<'_>) -> Result<(), Error> {
        let Some(first) = seq.rows.first() else { return Ok(()) };
        let target = self.target;
        let out = &mut self.bytes;
        let min = u32::from(target.min_inst_length);
        let (mut file, mut line, mut column, mut stmt) = (1u32, 1u32, 0u16, true);
        let mut at = first.offset;
        let mut addr = None;
        for (i, r) in seq.rows.iter().enumerate() {
            if r.file != file {
                file = r.file;
                out.push(DW_LNS_SET_FILE);
                write_uleb(out, u64::from(file));
            }
            if r.column != column {
                column = r.column;
                out.push(DW_LNS_SET_COLUMN);
                write_uleb(out, u64::from(column));
            }
            let is_stmt = r.flags & flags::IS_STMT != 0;
            if is_stmt != stmt {
                stmt = is_stmt;
                out.push(DW_LNS_NEGATE_STMT);
            }
            if r.flags & flags::PROLOGUE_END != 0 {
                out.push(DW_LNS_SET_PROLOGUE_END);
            }
            if r.flags & flags::EPILOGUE_BEGIN != 0 {
                out.push(DW_LNS_SET_EPILOGUE_BEGIN);
            }
            let delta = i64::from(r.line) - i64::from(line);
            if i == 0 {
                out.extend_from_slice(&[0, 1 + target.address_size, DW_LNE_SET_ADDRESS]);
                addr = Some(out.len());
                out.extend_from_slice(&[0; 8][..usize::from(target.address_size)]);
                write_advance(out, delta, 0);
            } else {
                write_advance(out, delta, u64::from((r.offset - at) / min));
            }
            line = r.line;
            at = r.offset;
        }
        write_end(out, u64::from((seq.size - at) / min));
        if u32::try_from(out.len()).is_err() {
            return Err(Error::TooLarge);
        }
        if let Some(offset) = addr {
            let addend = seq.addend + i64::from(first.offset);
            self.addrs.push(AddrField { offset: offset as u32, symbol: seq.symbol, addend });
        }
        Ok(())
    }
}

/// One line-table unit for [`write_debug_line_units`]: its header and
/// its encoded program.
#[derive(Clone, Copy, Debug)]
pub struct LineUnit<'a> {
    /// The directories and files.
    pub header: LineHeader<'a>,
    /// The sequences.
    pub program: &'a LineProgram,
}

/// What [`write_debug_line_units`] made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineUnits {
    /// The sections.
    pub sections: LineSections,
    /// The offset of each unit in `.debug_line`: its `DW_AT_stmt_list`.
    pub offsets: Vec<u32>,
}

/// The line strings: each distinct name once, in order of first use
/// (§4.3).
struct LineStrings<'a> {
    data: Vec<u8>,
    offsets: HashMap<&'a [u8], u32>,
}

impl<'a> LineStrings<'a> {
    fn new(count: usize, bytes: usize) -> Self {
        Self { data: Vec::with_capacity(bytes + count), offsets: HashMap::with_capacity(count) }
    }

    fn add(&mut self, s: &'a [u8]) -> Result<u32, Error> {
        if let Some(&at) = self.offsets.get(s) {
            return Ok(at);
        }
        let at = u32::try_from(self.data.len()).map_err(|_| Error::TooLarge)?;
        self.data.extend_from_slice(s);
        self.data.push(0);
        self.offsets.insert(s, at);
        Ok(at)
    }
}

/// Checks a header (§4.6).
fn check_header(header: &LineHeader<'_>) -> Result<(), Error> {
    if header.dirs.is_empty() || header.files.is_empty() {
        return Err(Error::NoPrimary);
    }
    for (i, f) in header.files.iter().enumerate() {
        if f.dir as usize >= header.dirs.len() {
            return Err(Error::BadDirectory { file: i as u32 });
        }
        if f.name.contains(&0) || f.source.is_some_and(|s| s.contains(&0)) {
            return Err(Error::NulInName);
        }
    }
    if header.dirs.iter().any(|d| d.contains(&0)) {
        return Err(Error::NulInName);
    }
    Ok(())
}

/// Checks the whole input of [`write_debug_line`] (§4.6) and returns
/// the sequences' indices in `order`.
fn check(header: &LineHeader<'_>, seqs: &[Sequence<'_>], min: u32) -> Result<Vec<u32>, Error> {
    check_header(header)?;
    for seq in seqs {
        check_sequence(seq, header.files.len(), min)?;
    }
    let mut sorted: Vec<u32> = (0..seqs.len() as u32).collect();
    if !seqs.windows(2).all(|w| w[0].order < w[1].order) {
        sorted.sort_unstable_by_key(|&i| seqs[i as usize].order);
        if let Some(w) =
            sorted.windows(2).find(|w| seqs[w[0] as usize].order == seqs[w[1] as usize].order)
        {
            return Err(Error::DuplicateOrder { order: seqs[w[0] as usize].order });
        }
    }
    Ok(sorted)
}

/// Appends one unit's header (§4.1) to `out`, with its string fields as
/// pending relocations.
fn write_header<'a>(
    out: &mut Vec<u8>,
    relocs: &mut Vec<Pending>,
    strings: &mut LineStrings<'a>,
    target: &LineTarget,
    header: &LineHeader<'a>,
) -> Result<usize, Error> {
    let start = out.len();
    out.extend_from_slice(&[0; 4]); // unit_length, patched by the caller
    out.extend_from_slice(&5u16.to_le_bytes());
    out.push(target.address_size);
    out.push(0); // segment_selector_size
    out.extend_from_slice(&[0; 4]); // header_length, patched below
    let header_start = out.len();
    out.push(target.min_inst_length);
    out.push(1); // maximum_operations_per_instruction
    out.push(1); // default_is_stmt
    out.push(LINE_BASE as u8);
    out.push(LINE_RANGE);
    out.push(OPCODE_BASE);
    out.extend_from_slice(&STANDARD_OPCODE_LENGTHS);
    let mut strp = |out: &mut Vec<u8>, s: &'a [u8]| -> Result<(), Error> {
        let at = strings.add(s)?;
        let offset = u32::try_from(out.len()).map_err(|_| Error::TooLarge)?;
        relocs.push(Pending { offset, symbol: None, addend: i64::from(at) });
        out.extend_from_slice(&[0; 4]);
        Ok(())
    };
    out.extend_from_slice(&[1, DW_LNCT_PATH, DW_FORM_LINE_STRP]);
    write_uleb(out, header.dirs.len() as u64);
    for d in header.dirs {
        strp(out, d)?;
    }
    let md5 = header.files.iter().all(|f| f.md5.is_some());
    let source = header.files.iter().any(|f| f.source.is_some());
    out.push(2 + u8::from(md5) + u8::from(source));
    out.extend_from_slice(&[
        DW_LNCT_PATH,
        DW_FORM_LINE_STRP,
        DW_LNCT_DIRECTORY_INDEX,
        DW_FORM_UDATA,
    ]);
    if md5 {
        out.extend_from_slice(&[DW_LNCT_MD5, DW_FORM_DATA16]);
    }
    if source {
        write_uleb(out, DW_LNCT_LLVM_SOURCE);
        out.push(DW_FORM_LINE_STRP);
    }
    write_uleb(out, header.files.len() as u64);
    for f in header.files {
        strp(out, f.name)?;
        write_uleb(out, u64::from(f.dir));
        if let Some(sum) = f.md5.filter(|_| md5) {
            out.extend_from_slice(&sum);
        }
        if source {
            strp(out, f.source.unwrap_or(b""))?;
        }
    }
    let header_length = u32::try_from(out.len() - header_start).map_err(|_| Error::TooLarge)?;
    out[header_start - 4..header_start].copy_from_slice(&header_length.to_le_bytes());
    Ok(start)
}

/// Writes one DWARF 5 line-table unit into new `.debug_line` and
/// `.debug_line_str` sections of `obj`, with their relocations
/// (`spec/dwarf.md` §4).
///
/// Sequences are written in increasing [`Sequence::order`]; the output
/// does not depend on the order of the slice. On an error `obj` is
/// unchanged.
///
/// # Errors
/// Every [`Error`] of `spec/dwarf.md` §4.6 except the policy's.
pub fn write_debug_line(
    obj: &mut Object,
    header: &LineHeader<'_>,
    seqs: &[Sequence<'_>],
) -> Result<LineSections, Error> {
    let target = for_arch(obj.arch());
    let min = u32::from(target.min_inst_length);

    // Check everything before writing anything.
    let sorted = check(header, seqs, min)?;
    let rows: usize = seqs.iter().map(|s| s.rows.len()).sum();
    let mut program = LineProgram::with_capacity(obj.arch(), rows, seqs.len());
    for &i in &sorted {
        program.encode(&seqs[i as usize])?;
    }
    let unit = LineUnit { header: *header, program: &program };
    Ok(write_debug_line_units(obj, &[unit])?.sections)
}

/// Writes line-table units, one after another, into new `.debug_line`
/// and `.debug_line_str` sections of `obj` (`spec/dwarf.md` §1.2, §4):
/// each unit's header, then its encoded program, with the relocations of
/// both. The line strings are pooled across the units. On an error `obj`
/// is unchanged.
///
/// # Errors
/// The header errors of §4.6 (`NoPrimary`, `BadDirectory`,
/// `NulInName`), `TooLarge`, `ArchMismatch` for a program of another
/// architecture than `obj`'s, and `Object`.
pub fn write_debug_line_units(
    obj: &mut Object,
    units: &[LineUnit<'_>],
) -> Result<LineUnits, Error> {
    let target = for_arch(obj.arch());
    for u in units {
        check_header(&u.header)?;
        if u.program.target.arch != target.arch {
            return Err(Error::ArchMismatch);
        }
    }
    if obj.section_count() >= u32::MAX - 3 {
        return Err(Error::Object(hod_obj::Error::TooLarge(hod_obj::Limit::Sections)));
    }

    // The strings.
    let names: usize = units.iter().map(|u| u.header.dirs.len() + 2 * u.header.files.len()).sum();
    let name_bytes: usize = units
        .iter()
        .map(|u| {
            u.header.dirs.iter().map(|d| d.len()).sum::<usize>()
                + u.header.files.iter().map(|f| f.name.len()).sum::<usize>()
        })
        .sum();
    let mut strings = LineStrings::new(names, name_bytes);

    let bytes: usize = units.iter().map(|u| u.program.bytes.len()).sum();
    let addrs: usize = units.iter().map(|u| u.program.addrs.len()).sum();
    let mut out = Vec::with_capacity(units.len() * 64 + names * 22 + bytes);
    let mut relocs = Vec::with_capacity(names + addrs);
    let mut offsets = Vec::with_capacity(units.len());
    for u in units {
        let start = write_header(&mut out, &mut relocs, &mut strings, target, &u.header)?;
        let base = u32::try_from(out.len()).map_err(|_| Error::TooLarge)?;
        out.extend_from_slice(&u.program.bytes);
        relocs.extend(u.program.addrs.iter().map(|a| Pending {
            offset: base.wrapping_add(a.offset),
            symbol: Some(a.symbol),
            addend: a.addend,
        }));
        let unit_length = u32::try_from(out.len() - start - 4)
            .ok()
            .filter(|&n| n < 0xffff_fff0)
            .ok_or(Error::TooLarge)?;
        out[start..start + 4].copy_from_slice(&unit_length.to_le_bytes());
        offsets.push(start as u32);
    }
    if u32::try_from(out.len()).is_err() || u32::try_from(strings.data.len()).is_err() {
        return Err(Error::TooLarge);
    }

    // Only now touch the object.
    let line = obj.add_section(b".debug_line", SectionKind::Debug, 1)?;
    let str_kind = SectionKind::Elf {
        sh_type: SHT_PROGBITS,
        sh_flags: SHF_MERGE | SHF_STRINGS,
        sh_entsize: 1,
    };
    let line_str = obj.add_section(b".debug_line_str", str_kind, 1)?;
    obj.append_owned(line, out);
    obj.append_owned(line_str, strings.data);
    let str_symbol = obj.section_symbol(line_str);
    obj.reserve(0, 0, relocs.len(), 0);
    for r in relocs {
        let (kind, symbol) = match r.symbol {
            None => (target.offset_reloc, str_symbol),
            Some(s) => (target.address_reloc, s),
        };
        obj.add_relocation(line, u64::from(r.offset), kind, symbol, r.addend);
    }
    Ok(LineUnits { sections: LineSections { line, line_str }, offsets })
}

/// Builds a line-table header from DWARF file descriptions with `llc`'s
/// rules (`spec/dwarf.md` §4.4).
#[derive(Clone, Debug)]
pub struct FileTable<'a> {
    dirs: Vec<&'a [u8]>,
    files: Vec<FileEntry<'a>>,
    dir_index: HashMap<&'a [u8], u32>,
    file_index: HashMap<(u32, &'a [u8]), u32>,
}

impl<'a> FileTable<'a> {
    /// A table whose directory 0 is `comp_dir` and file 0 `primary`.
    pub fn new(comp_dir: &'a [u8], primary: &'a [u8], md5: Option<[u8; 16]>) -> Self {
        Self::new_with_source(comp_dir, primary, md5, None)
    }

    /// [`FileTable::new`], with the primary file's embedded source.
    pub fn new_with_source(
        comp_dir: &'a [u8],
        primary: &'a [u8],
        md5: Option<[u8; 16]>,
        source: Option<&'a [u8]>,
    ) -> Self {
        let mut file_index = HashMap::new();
        file_index.insert((0, primary), 0);
        Self {
            dirs: vec![comp_dir],
            files: vec![FileEntry { name: primary, dir: 0, md5, source }],
            dir_index: HashMap::new(),
            file_index,
        }
    }

    /// The index of the file `filename` in `directory`, added if it is new.
    pub fn add(&mut self, directory: &'a [u8], filename: &'a [u8], md5: Option<[u8; 16]>) -> u32 {
        self.add_with_source(directory, filename, md5, None)
    }

    /// [`FileTable::add`], with the file's embedded source. A file added
    /// again keeps what it was first added with.
    pub fn add_with_source(
        &mut self,
        directory: &'a [u8],
        filename: &'a [u8],
        md5: Option<[u8; 16]>,
        source: Option<&'a [u8]>,
    ) -> u32 {
        let comp_dir = self.dirs[0];
        if directory == comp_dir && self.file_index.contains_key(&(0, filename)) {
            return self.file_index[&(0, filename)];
        }
        let (dir, name) = if directory.is_empty() || directory == comp_dir {
            match filename.iter().rposition(|&b| b == b'/') {
                Some(0) => (&filename[..1], &filename[1..]),
                Some(slash) if slash + 1 < filename.len() => {
                    (&filename[..slash], &filename[slash + 1..])
                }
                _ => (&[][..], filename),
            }
        } else {
            (directory, filename)
        };
        let dir = if dir.is_empty() {
            0
        } else {
            let next = self.dirs.len() as u32;
            let at = *self.dir_index.entry(dir).or_insert(next);
            if at == next {
                self.dirs.push(dir);
            }
            at
        };
        let next = self.files.len() as u32;
        let at = *self.file_index.entry((dir, name)).or_insert(next);
        if at == next {
            self.files.push(FileEntry { name, dir, md5, source });
        }
        at
    }

    /// The directories.
    pub fn dirs(&self) -> &[&'a [u8]] {
        &self.dirs
    }

    /// The files.
    pub fn files(&self) -> &[FileEntry<'a>] {
        &self.files
    }

    /// The header for [`write_debug_line`].
    pub fn header(&self) -> LineHeader<'_> {
        LineHeader { dirs: &self.dirs, files: &self.files }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adv(line: i64, addr: u64) -> Vec<u8> {
        let mut out = Vec::new();
        write_advance(&mut out, line, addr);
        out
    }

    fn end(addr: u64) -> Vec<u8> {
        let mut out = Vec::new();
        write_end(&mut out, addr);
        out
    }

    /// `spec/dwarf.md` §4.5 step 3, each case of LLVM's encoder.
    #[test]
    fn advances() {
        // Special opcodes, at the edges of the line range.
        assert_eq!(adv(2, 0), [0x14]);
        assert_eq!(adv(1, 4), [0x4b]);
        assert_eq!(adv(-5, 0), [0x0d]);
        assert_eq!(adv(8, 0), [0x1a]);
        assert_eq!(adv(-5, 17), [0x0d + 17 * 14]);
        // `DW_LNS_copy` for no advance at all.
        assert_eq!(adv(0, 0), [0x01]);
        // `DW_LNS_const_add_pc` and a special opcode.
        assert_eq!(adv(0, 17), [0x08, 0x12]);
        assert_eq!(adv(1, 20), [0x08, 0x3d]);
        assert_eq!(adv(0, 33), [0x08, 0x12 + 16 * 14]);
        assert_eq!(adv(0, 34), [0x02, 34, 0x12]);
        // `DW_LNS_advance_pc` and a special opcode.
        assert_eq!(adv(0, 35), [0x02, 35, 0x12]);
        assert_eq!(adv(0, 300), [0x02, 0xac, 0x02, 0x12]);
        // `DW_LNS_advance_line` outside the range, then a copy or an
        // address-only special opcode or `advance_pc` and a copy.
        assert_eq!(adv(9, 0), [0x03, 9, 0x01]);
        assert_eq!(adv(-6, 0), [0x03, 0x7a, 0x01]);
        assert_eq!(adv(10, 0), [0x03, 10, 0x01]);
        assert_eq!(adv(20, 5), [0x03, 20, 0x12 + 5 * 14]);
        assert_eq!(adv(-1000, 17), [0x03, 0x98, 0x78, 0x08, 0x12]);
        assert_eq!(adv(20, 300), [0x03, 20, 0x02, 0xac, 0x02, 0x01]);
        assert_eq!(adv(1 << 40, 1 << 33), {
            let mut v = vec![0x03];
            write_sleb(&mut v, 1 << 40);
            v.push(0x02);
            write_uleb(&mut v, 1 << 33);
            v.push(0x01);
            v
        });
    }

    /// §4.5 step 4.
    #[test]
    fn ends() {
        assert_eq!(end(0), [0, 1, 1]);
        assert_eq!(end(5), [0x02, 5, 0, 1, 1]);
        assert_eq!(end(17), [0x08, 0, 1, 1]);
        assert_eq!(end(18), [0x02, 18, 0, 1, 1]);
    }
}
