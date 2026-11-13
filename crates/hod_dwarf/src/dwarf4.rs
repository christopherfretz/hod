//! DWARF version 4 (`spec/dwarf.md` §6.20, WP-114).
//!
//! A module whose `Dwarf Version` flag is 4 is written as `llc` 19 and 22
//! write it. The DIE tree is the one DWARF 5 has, so the DIEs are written
//! by the same code ([`Writer::unit`]) and each unit is then rewritten
//! into version 4, attribute by attribute, from the abbreviation table:
//!
//! - the unit header: version 4, `debug_abbrev_offset`, `address_size`
//!   (no `unit_type`);
//! - strings: `DW_FORM_strp`, a relocated offset into `.debug_str`;
//!   there is no `.debug_str_offsets` and no `DW_AT_str_offsets_base`;
//! - addresses: `DW_FORM_addr` and `DW_OP_addr`, relocated; there is no
//!   `.debug_addr` and no `DW_AT_addr_base`;
//! - range lists: `.debug_ranges` by `DW_FORM_sec_offset`, and no
//!   `DW_AT_rnglists_base`. As `llc` (`DwarfDebug::emitRangeList`) does:
//!   a unit whose code is one range is the base of its blocks' lists,
//!   whose entries are then offsets from it without relocations; in a
//!   unit with several ranges every entry is a relocated address;
//! - files are counted from 1, so `DW_AT_decl_file` and `DW_AT_call_file`
//!   are DWARF 5's index plus one, and the line table is version 4's
//!   ([`write_line_header`]: the directories and files inline, no
//!   `.debug_line_str`, no MD5 or source);
//! - what `llc` writes differently for version 4, found by comparing its
//!   trees of the same modules at both versions: a static data member
//!   is a `DW_TAG_member`, `DW_AT_deleted` is not written, and the
//!   languages whose default lower bound DWARF 5 introduced have none
//!   ([`default_lower_bound`]).
//!
//! The abbreviation codes are DWARF 5's: a code's version 4 entry has the
//! version 4 tag and forms, without the attributes version 4 drops. A DIE
//! of a family this module was not checked against `llc` for (one
//! appended to the table after it) is refused as [`DWARF_4_SHAPE`], as
//! is an expression that would change length across a branch. DWARF 5
//! output does not go through this module at all.

use hod_ir::Module;
use hod_obj::elf::{SHF_MERGE, SHF_STRINGS, SHT_PROGBITS};
use hod_obj::{Arch, Object, SectionId, SectionKind, SymbolId};

use crate::Error;
use crate::abbrev::{BASES, CODE_END, Children, FAMILIES, Slot, at, fam, form, option_bits, tag};
use crate::die::{Reloc, Sec, Target, UnitInput, UnitPlan, Writer, unsupported};
use crate::leb::write_uleb;
use crate::line::{LineHeader, LineProgram};
use crate::strings::DebugStrings;
use crate::target::{LINE_BASE, LINE_RANGE, LineTarget, OPCODE_BASE, STANDARD_OPCODE_LENGTHS};
use crate::{SPLIT_DWARF_4, SplitDwarf};

/// A DWARF 4 unit with a DIE shape or a location expression this writer
/// was not compared with `llc`'s version 4 output for: a family appended
/// to the table after WP-114, or an expression whose length would change
/// across a branch.
pub const DWARF_4_SHAPE: &str = "debug:dwarf-4-shape";

/// The last family of the table when WP-114 compared the trees; the
/// families after it are refused in a DWARF 4 unit until checked.
const LAST_CHECKED: usize = fam::SPLIT_COMPILE_UNIT;

const DW_FORM_STRP: u8 = 0x0e;
const DW_OP_ADDR: u8 = 0x03;
const DW_OP_ADDRX: u8 = 0xa1;
const DW_OP_SKIP: u8 = 0x2f;
const DW_OP_BRA: u8 = 0x28;
const DW_RLE_END_OF_LIST: u8 = 0x00;
const DW_RLE_BASE_ADDRESSX: u8 = 0x01;
const DW_RLE_STARTX_LENGTH: u8 = 0x03;
const DW_RLE_OFFSET_PAIR: u8 = 0x04;
/// A DWARF 5 unit header (32-bit format).
const HEADER5: usize = 12;

/// The DWARF version a unit of `module` is written in: 4 when its
/// `Dwarf Version` flag is 4, else 5 (`hod_ir::classify` refuses the
/// other values as `debug:dwarf-version`).
pub(crate) fn version_of(module: &Module) -> u16 {
    if module.props.dwarf_version == Some(4) { 4 } else { 5 }
}

/// `llc`'s `getDefaultLowerBound` at `version`, given DWARF 5's value
/// `v5` for `language`: the languages from `DW_LANG_OpenCL` (0x15) on have
/// one only from version 5 (checked with `llc` 19 and 22 for every
/// language LLVM names, with lower bounds 0 and 1).
pub(crate) const fn default_lower_bound(version: u16, language: u16, v5: i64) -> i64 {
    if version == 4 && language >= 0x15 { -1 } else { v5 }
}

impl UnitPlan<'_> {
    /// The DWARF version the unit is written in: 4 or 5 (`spec/dwarf.md`
    /// §6.20).
    pub fn version(&self) -> u16 {
        self.version
    }

    /// An empty line program for the unit's line table (its files are
    /// counted from 1 in DWARF 4).
    pub fn line_program(&self, arch: Arch) -> LineProgram {
        LineProgram::for_version(arch, self.version)
    }
}

/// Appends a version 4 line-table header (DWARF 4 §6.2.4) to `out`: the
/// fields of DWARF 5's (§4.1) without the address and segment selector
/// sizes, then `include_directories` (every directory but 0, the
/// compilation directory, each a NUL-terminated string, then an empty
/// one) and `file_names` (each file: its name, its directory index, a
/// modification time and a length of 0; then a 0). Files are numbered
/// from 1. MD5s and sources are not written: version 4 has no place for
/// them, and `llc` drops them too. The unit's length is the caller's.
pub(crate) fn write_line_header(
    out: &mut Vec<u8>,
    target: &LineTarget,
    header: &LineHeader<'_>,
) -> Result<usize, Error> {
    let start = out.len();
    out.extend_from_slice(&[0; 4]); // unit_length, patched by the caller
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&[0; 4]); // header_length, patched below
    let header_start = out.len();
    out.push(target.min_inst_length);
    out.push(1); // maximum_operations_per_instruction
    out.push(1); // default_is_stmt
    out.push(LINE_BASE as u8);
    out.push(LINE_RANGE);
    out.push(OPCODE_BASE);
    out.extend_from_slice(&STANDARD_OPCODE_LENGTHS);
    for d in header.dirs.iter().skip(1) {
        out.extend_from_slice(d);
        out.push(0);
    }
    out.push(0);
    for f in header.files {
        out.extend_from_slice(f.name);
        out.push(0);
        write_uleb(out, u64::from(f.dir));
        out.extend_from_slice(&[0, 0]); // mtime, length
    }
    out.push(0);
    let header_length = u32::try_from(out.len() - header_start).map_err(|_| Error::TooLarge)?;
    out[header_start - 4..header_start].copy_from_slice(&header_length.to_le_bytes());
    Ok(start)
}

/// Whether version 4 writes attribute `at`: not the DWARF 5 bases, and
/// not `DW_AT_deleted`, which `llc` writes from version 5 on.
const fn keeps(at: u16) -> bool {
    !matches!(at, at::STR_OFFSETS_BASE | at::ADDR_BASE | at::RNGLISTS_BASE | at::DELETED)
}

/// A slot's version 4 form.
const fn form4(f: u8) -> u8 {
    match f {
        form::STRX => DW_FORM_STRP,
        form::ADDRX => form::ADDR,
        form::RNGLISTX => form::SEC_OFFSET,
        f => f,
    }
}

/// A family's version 4 tag: `llc` writes a static data member's
/// declaration as `DW_TAG_member` before version 5.
const fn tag4(family: usize) -> u16 {
    match family {
        fam::STATIC_MEMBER | fam::STATIC_VARIABLE_ANY => tag::MEMBER,
        f => FAMILIES[f].tag,
    }
}

/// The family of abbreviation code `code` and the code's offset in its
/// block.
fn family_of(code: u32) -> Option<(usize, u32)> {
    if code == 0 || code >= CODE_END {
        return None;
    }
    let f = BASES.partition_point(|&b| b <= code) - 1;
    Some((f, code - BASES[f]))
}

/// The option mask of an offset in family `f`'s block, and whether the
/// DIE has children.
fn split_offset(f: usize, offset: u32) -> (u32, bool) {
    let family = &FAMILIES[f];
    let bits = option_bits(family);
    let children = match family.children {
        Children::No => false,
        Children::Yes => true,
        Children::Optional => offset >> bits != 0,
    };
    (offset & ((1 << bits) - 1), children)
}

/// `.debug_abbrev` for version 4: the entries of the used codes, in code
/// order, then the terminating 0.
fn write_abbrev4(used: &[u64]) -> Vec<u8> {
    let mut out = Vec::with_capacity(256);
    for code in 1..CODE_END {
        if used[code as usize / 64] & (1 << (code % 64)) == 0 {
            continue;
        }
        let Some((f, offset)) = family_of(code) else { continue };
        let (mask, children) = split_offset(f, offset);
        write_uleb(&mut out, u64::from(code));
        write_uleb(&mut out, u64::from(tag4(f)));
        out.push(u8::from(children));
        for slot in FAMILIES[f].slots {
            let present = slot.bit == 0 || mask & (1 << (slot.bit - 1)) != 0;
            if present && keeps(slot.at) {
                write_uleb(&mut out, u64::from(slot.at));
                write_uleb(&mut out, u64::from(form4(slot.form)));
            }
        }
        out.extend_from_slice(&[0, 0]);
    }
    out.push(0);
    out
}

/// A malformed DWARF 5 unit: the writer's own bug, declined by name so
/// that the module falls back rather than being written wrong.
fn shape(unit: hod_ir::DiId) -> Error {
    unsupported(unit, DWARF_4_SHAPE)
}

fn read_uleb(b: &[u8], p: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    let mut shift = 0;
    loop {
        let byte = *b.get(*p)?;
        *p += 1;
        if shift < 64 {
            v |= u64::from(byte & 0x7f) << shift;
        }
        shift += 7;
        if byte & 0x80 == 0 {
            return Some(v);
        }
    }
}

fn skip_leb(b: &[u8], p: &mut usize) -> Option<()> {
    loop {
        let byte = *b.get(*p)?;
        *p += 1;
        if byte & 0x80 == 0 {
            return Some(());
        }
    }
}

/// The length of the operands of the operation at `b[p - 1]`, for the
/// operations whose operands need no rewriting; `None` for one that
/// names a DIE or an index (they would need it) or is unknown.
fn operands(op: u8, b: &[u8], mut p: usize) -> Option<usize> {
    let start = p;
    match op {
        0x06
        | 0x12..=0x14
        | 0x16..=0x22
        | 0x24..=0x27
        | 0x29..=0x2e
        | 0x30..=0x6f
        | 0x96
        | 0x97
        | 0x9b
        | 0x9c
        | 0x9f
        | 0xe0 => {}
        0x08 | 0x09 | 0x15 | 0x94 | 0x95 => p += 1,
        0x0a | 0x0b | 0x28 | 0x2f => p += 2,
        0x0c | 0x0d => p += 4,
        DW_OP_ADDR | 0x0e | 0x0f => p += 8,
        0x10 | 0x11 | 0x23 | 0x70..=0x93 | DW_OP_ADDRX => {
            skip_leb(b, &mut p)?;
            if matches!(op, 0x92) {
                skip_leb(b, &mut p)?; // bregx: register, offset
            }
        }
        0x9d => {
            skip_leb(b, &mut p)?; // bit_piece: size, offset
            skip_leb(b, &mut p)?;
        }
        0x9e => {
            let n = read_uleb(b, &mut p)?; // implicit_value: length, bytes
            p = p.checked_add(usize::try_from(n).ok()?)?;
        }
        _ => return None,
    }
    (p <= b.len()).then_some(p - start)
}

/// One range of a list: a symbol and the start and end offsets from it.
type Range = (SymbolId, i64, i64);

/// The version 4 output of the units, before the object is touched.
/// The DWARF 5 unit [`Out::unit`] reads: its writer, its plan's unit,
/// its one code span (when it has one) and its bytes.
struct Src<'s, 'w> {
    w: &'s Writer<'w>,
    unit: hod_ir::DiId,
    base: Option<(SymbolId, i64)>,
    bytes: &'s [u8],
}

struct Out {
    info: Vec<u8>,
    /// `.debug_info`'s relocations; `Target::Rnglists` is `.debug_ranges`.
    relocs: Vec<Reloc>,
    ranges: Vec<u8>,
    /// `.debug_ranges`'s relocations: offset, symbol, addend.
    range_relocs: Vec<(usize, SymbolId, i64)>,
    used: Vec<u64>,
    // Per unit, reused.
    /// The unit's `.debug_info` relocations of DWARF 5, by unit offset.
    old: Vec<Reloc>,
    /// `(DWARF 5 offset, DWARF 4 offset)` of each DIE, in order.
    map: Vec<(u32, u32)>,
    /// `(position in info, DWARF 5 offset)` of each reference.
    patches: Vec<(usize, u32)>,
    expr: Vec<u8>,
    /// The relocations of `expr`: position in it, target, addend.
    expr_relocs: Vec<(usize, Target, i64)>,
    list: Vec<Range>,
}

impl Out {
    fn reloc(&mut self, target: Target, addend: i64) {
        self.relocs.push(Reloc { sec: Sec::Info, offset: self.info.len(), target, addend });
    }

    /// The DWARF 5 relocation at unit offset `p`.
    fn old_at(&self, p: usize) -> Option<Reloc> {
        let i = self.old.partition_point(|r| r.offset < p);
        self.old.get(i).filter(|r| r.offset == p).copied()
    }

    /// Copies DWARF 5 bytes `src[a..b]` of the unit, with the relocations
    /// in them.
    fn copy(&mut self, src: &[u8], a: usize, b: usize) {
        let base = self.info.len();
        self.info.extend_from_slice(&src[a..b]);
        let from = self.old.partition_point(|r| r.offset < a);
        let to = self.old.partition_point(|r| r.offset < b);
        for k in from..to {
            let r = self.old[k];
            self.relocs.push(Reloc { offset: base + r.offset - a, ..r });
        }
    }

    /// Appends a version 4 range list of `self.list` to `.debug_ranges`
    /// and writes its offset into `.debug_info`. `base`: the unit's one
    /// range, which the entries are offsets from; else every entry is a
    /// relocated address.
    fn ranges_list(
        &mut self,
        unit: hod_ir::DiId,
        base: Option<(SymbolId, i64)>,
    ) -> Result<(), Error> {
        let at = self.ranges.len();
        for k in 0..self.list.len() {
            let (symbol, start, end) = self.list[k];
            match base {
                Some((b, offset)) => {
                    if b != symbol || start < offset || end < start {
                        return Err(shape(unit));
                    }
                    self.ranges.extend_from_slice(&((start - offset) as u64).to_le_bytes());
                    self.ranges.extend_from_slice(&((end - offset) as u64).to_le_bytes());
                }
                None => {
                    for addend in [start, end] {
                        self.range_relocs.push((self.ranges.len(), symbol, addend));
                        self.ranges.extend_from_slice(&[0; 8]);
                    }
                }
            }
        }
        self.ranges.extend_from_slice(&[0; 16]);
        self.reloc(Target::Rnglists, at as i64);
        self.info.extend_from_slice(&[0; 4]);
        Ok(())
    }

    /// Rewrites the unit `w` just wrote at `start` (its relocations from
    /// `relocs_from` on) into version 4.
    fn unit(
        &mut self,
        w: &Writer<'_>,
        plan: &UnitPlan<'_>,
        start: usize,
        relocs_from: usize,
    ) -> Result<(), Error> {
        let unit = plan.unit;
        let bad = || shape(unit);
        self.old.clear();
        self.old.extend(
            w.relocs[relocs_from..]
                .iter()
                .filter(|r| r.sec == Sec::Info)
                .map(|r| Reloc { offset: r.offset - start, ..*r }),
        );
        self.old.sort_by_key(|r| r.offset);
        self.map.clear();
        self.patches.clear();
        let cx = Src {
            w,
            unit,
            base: (w.spans.len() == 1).then(|| (w.spans[0].symbol, w.spans[0].addend)),
            bytes: &w.info[start..],
        };

        // The header (DWARF 4 §7.5.1.1).
        let new_start = self.info.len();
        self.info.extend_from_slice(&[0; 4]);
        self.info.extend_from_slice(&4u16.to_le_bytes());
        self.reloc(Target::Abbrev, 0);
        self.info.extend_from_slice(&[0; 4]);
        self.info.push(8);

        let mut pos = HEADER5;
        while pos < cx.bytes.len() {
            let die = pos;
            let code = read_uleb(cx.bytes, &mut pos).ok_or_else(bad)?;
            if code == 0 {
                self.info.push(0);
                continue;
            }
            let new = u32::try_from(self.info.len() - new_start).map_err(|_| Error::TooLarge)?;
            self.map.push((die as u32, new));
            let code = u32::try_from(code).map_err(|_| bad())?;
            let (family, offset) = family_of(code).ok_or_else(bad)?;
            if family > LAST_CHECKED {
                return Err(bad());
            }
            self.used[code as usize / 64] |= 1 << (code % 64);
            write_uleb(&mut self.info, u64::from(code));
            let (mask, _) = split_offset(family, offset);
            for slot in FAMILIES[family].slots {
                if slot.bit == 0 || mask & (1 << (slot.bit - 1)) != 0 {
                    self.attribute(&cx, *slot, &mut pos)?;
                }
            }
        }

        // References, now that every DIE has its offset.
        for k in 0..self.patches.len() {
            let (at, old) = self.patches[k];
            let index = self.map.partition_point(|&(o, _)| o < old);
            let new = self.map.get(index).filter(|&&(o, _)| o == old).ok_or_else(bad)?.1;
            self.info[at..at + 4].copy_from_slice(&new.to_le_bytes());
        }
        let length = u32::try_from(self.info.len() - new_start - 4)
            .ok()
            .filter(|&l| l < 0xffff_fff0)
            .ok_or(Error::TooLarge)?;
        self.info[new_start..new_start + 4].copy_from_slice(&length.to_le_bytes());
        Ok(())
    }

    /// Rewrites one attribute of `cx.bytes` at `*pos` (advanced past it).
    fn attribute(&mut self, cx: &Src<'_, '_>, slot: Slot, pos: &mut usize) -> Result<(), Error> {
        let (w, src) = (cx.w, cx.bytes);
        let bad = || shape(cx.unit);
        let addr = |index: u64| w.addrs.get(usize::try_from(index).ok()?).copied();
        let keep = keeps(slot.at);
        let from = *pos;
        match slot.form {
            form::STRX => {
                let index = read_uleb(src, pos).ok_or_else(bad)?;
                if keep {
                    let offsets = w.strings.unit_offsets();
                    let off = usize::try_from(index).ok().and_then(|k| offsets.get(k));
                    self.reloc(Target::Str, i64::from(*off.ok_or_else(bad)?));
                    self.info.extend_from_slice(&[0; 4]);
                }
            }
            form::ADDRX => {
                let (symbol, addend) = read_uleb(src, pos).and_then(addr).ok_or_else(bad)?;
                self.reloc(Target::Symbol(symbol), addend);
                self.info.extend_from_slice(&[0; 8]);
            }
            form::RNGLISTX => {
                // The unit's own list (§6.15's families never reach here:
                // split DWARF 4 is refused).
                read_uleb(src, pos).ok_or_else(bad)?;
                self.list.clear();
                self.list
                    .extend(w.spans.iter().map(|s| (s.symbol, s.addend, s.addend + s.size as i64)));
                self.ranges_list(cx.unit, None)?;
            }
            form::SEC_OFFSET => {
                *pos += 4;
                let old = self.old_at(from).ok_or_else(bad)?;
                match old.target {
                    _ if !keep => {}
                    Target::Line => {
                        self.reloc(Target::Line, old.addend);
                        self.info.extend_from_slice(&[0; 4]);
                    }
                    Target::Rnglists => {
                        self.v5_list(w, cx.unit, old.addend)?;
                        self.ranges_list(cx.unit, cx.base)?;
                    }
                    _ => return Err(bad()),
                }
            }
            form::REF4 => {
                let old = src.get(*pos..*pos + 4).ok_or_else(bad)?;
                let old = u32::from_le_bytes([old[0], old[1], old[2], old[3]]);
                *pos += 4;
                self.patches.push((self.info.len(), old));
                self.info.extend_from_slice(&[0; 4]);
            }
            form::EXPRLOC => {
                let len = read_uleb(src, pos).ok_or_else(bad)?;
                let end = usize::try_from(len).ok().and_then(|l| pos.checked_add(l));
                let end = end.filter(|&e| e <= src.len()).ok_or_else(bad)?;
                self.exprloc(src, *pos, end, &addr).ok_or_else(bad)?;
                *pos = end;
            }
            form::UDATA => {
                let value = read_uleb(src, pos).ok_or_else(bad)?;
                if matches!(slot.at, at::DECL_FILE | at::CALL_FILE) {
                    write_uleb(&mut self.info, value + 1);
                } else {
                    self.copy(src, from, *pos);
                }
            }
            form::SDATA => {
                skip_leb(src, pos).ok_or_else(bad)?;
                self.copy(src, from, *pos);
            }
            form::FLAG_PRESENT => {}
            form::DATA1 | form::DATA2 | form::DATA4 | form::ADDR => {
                *pos += match slot.form {
                    form::DATA1 => 1,
                    form::DATA2 => 2,
                    form::DATA4 => 4,
                    _ => 8,
                };
                if *pos > src.len() {
                    return Err(bad());
                }
                if keep {
                    self.copy(src, from, *pos);
                }
            }
            _ => return Err(bad()),
        }
        Ok(())
    }

    /// Reads the DWARF 5 range list at `offset` of `w`'s
    /// `.debug_rnglists` into `self.list`.
    fn v5_list(&mut self, w: &Writer<'_>, unit: hod_ir::DiId, offset: i64) -> Result<(), Error> {
        let bad = || shape(unit);
        let b = &w.rnglists;
        let addr = |index: u64| w.addrs.get(usize::try_from(index).ok()?).copied();
        let mut p = usize::try_from(offset).map_err(|_| bad())?;
        let mut base = None;
        self.list.clear();
        loop {
            let op = *b.get(p).ok_or_else(bad)?;
            p += 1;
            match op {
                DW_RLE_END_OF_LIST => return Ok(()),
                DW_RLE_BASE_ADDRESSX => {
                    base = Some(read_uleb(b, &mut p).and_then(addr).ok_or_else(bad)?);
                }
                DW_RLE_OFFSET_PAIR => {
                    let (symbol, at) = base.ok_or_else(bad)?;
                    let from = read_uleb(b, &mut p).ok_or_else(bad)?;
                    let to = read_uleb(b, &mut p).ok_or_else(bad)?;
                    self.list.push((symbol, at + from as i64, at + to as i64));
                }
                DW_RLE_STARTX_LENGTH => {
                    let (symbol, at) = read_uleb(b, &mut p).and_then(addr).ok_or_else(bad)?;
                    let length = read_uleb(b, &mut p).ok_or_else(bad)?;
                    self.list.push((symbol, at, at + length as i64));
                }
                _ => return Err(bad()),
            }
        }
    }

    /// Writes the expression `src[p..end]` with `DW_OP_addrx` as
    /// `DW_OP_addr` and its relocated address. `None` for an operation
    /// [`operands`] does not know, or a branch in an expression whose
    /// length changes.
    fn exprloc(
        &mut self,
        src: &[u8],
        mut p: usize,
        end: usize,
        addr: &dyn Fn(u64) -> Option<(SymbolId, i64)>,
    ) -> Option<()> {
        self.expr.clear();
        self.expr_relocs.clear();
        let (mut branch, mut changed) = (false, false);
        while p < end {
            let op = src[p];
            let n = operands(op, &src[..end], p + 1)?;
            if op == DW_OP_ADDRX {
                let mut q = p + 1;
                let (symbol, addend) = read_uleb(src, &mut q).and_then(addr)?;
                self.expr.push(DW_OP_ADDR);
                self.expr_relocs.push((self.expr.len(), Target::Symbol(symbol), addend));
                self.expr.extend_from_slice(&[0; 8]);
                changed = true;
            } else {
                let at = self.expr.len();
                self.expr.extend_from_slice(&src[p..p + 1 + n]);
                let from = self.old.partition_point(|r| r.offset < p);
                let to = self.old.partition_point(|r| r.offset < p + 1 + n);
                for k in from..to {
                    let r = self.old[k];
                    self.expr_relocs.push((at + r.offset - p, r.target, r.addend));
                }
            }
            branch |= matches!(op, DW_OP_BRA | DW_OP_SKIP);
            p += 1 + n;
        }
        if branch && changed {
            return None;
        }
        write_uleb(&mut self.info, self.expr.len() as u64);
        let base = self.info.len();
        self.info.extend_from_slice(&self.expr);
        for &(at, target, addend) in &self.expr_relocs {
            self.relocs.push(Reloc { sec: Sec::Info, offset: base + at, target, addend });
        }
        Some(())
    }
}

/// The sections [`write_debug_info4`] made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dwarf4Sections {
    /// `.debug_abbrev`.
    pub abbrev: SectionId,
    /// `.debug_info`.
    pub info: SectionId,
    /// `.debug_str`.
    pub str: SectionId,
    /// `.debug_ranges`, when a unit's code or a block's is in several
    /// ranges.
    pub ranges: Option<SectionId>,
}

fn not_dwarf4(u: &UnitInput<'_, '_>) -> Error {
    unsupported(u.plan.unit, "a DWARF 5 unit given to the DWARF 4 writer")
}

/// The DWARF 5 writers' guard: `units` are DWARF 5's (a DWARF 4 plan is
/// written by [`write_debug_info4`], or [`crate::write_unit_dies`]).
///
/// # Errors
/// `NoUnit` for no units; `Unsupported` for a DWARF 4 plan.
pub(crate) fn check_dwarf5(units: &[UnitInput<'_, '_>]) -> Result<(), Error> {
    if units.is_empty() {
        return Err(Error::NoUnit);
    }
    match units.iter().find(|u| u.plan.version == 4) {
        Some(u) => Err(unsupported(u.plan.unit, "a DWARF 4 unit given to the DWARF 5 writer")),
        None => Ok(()),
    }
}

/// What [`crate::write_unit_dies`] does for DWARF 4 units: they are
/// written by [`write_debug_info4`]; split DWARF 4 (GNU fission) is
/// refused as [`SPLIT_DWARF_4`]. `None` for DWARF 5 units.
pub(crate) fn write_unit_dies(
    obj: &mut Object,
    line: SectionId,
    units: &[UnitInput<'_, '_>],
    split: SplitDwarf,
) -> Option<Result<(), Error>> {
    let u = units.iter().find(|u| u.plan.version == 4)?;
    if split != SplitDwarf::Off {
        return Some(Err(unsupported(u.plan.unit, SPLIT_DWARF_4)));
    }
    Some(write_debug_info4(obj, line, units).map(|_| ()))
}

/// An empty [`Writer`] for the DWARF 5 bytes of units of `dies` DIEs in
/// all.
fn scratch_writer<'a>(dies: usize) -> Writer<'a> {
    Writer {
        info: Vec::with_capacity(16 + dies * 10),
        str_offsets: Vec::new(),
        addr: Vec::new(),
        rnglists: Vec::new(),
        strings: DebugStrings::new(),
        used: crate::abbrev::Used::new(),
        relocs: Vec::new(),
        offsets: Vec::new(),
        patches: Vec::new(),
        stack: Vec::new(),
        addr_of: Vec::new(),
        addrs: Vec::new(),
        spans: Vec::new(),
        unit_start: 0,
        cur: Vec::new(),
        expr: Vec::new(),
        fn_addr: Vec::new(),
        block_lists: Vec::new(),
        block_lists_base: 0,
    }
}

/// Writes the DIEs of DWARF 4 `units` into new `.debug_abbrev`,
/// `.debug_info` and `.debug_str` sections of `obj`, and `.debug_ranges`
/// when a list is needed, with their relocations (`spec/dwarf.md`
/// §6.20). As [`crate::write_debug_info`] otherwise: `line` is the
/// `.debug_line` section the units' `DW_AT_stmt_list` point into, units
/// are written in the order given, and on an error `obj` is unchanged.
///
/// # Errors
/// [`crate::write_debug_info`]'s; `Unsupported` for a DWARF 5 unit, and
/// as [`DWARF_4_SHAPE`] for what this writer was not checked for.
pub fn write_debug_info4(
    obj: &mut Object,
    line: SectionId,
    units: &[UnitInput<'_, '_>],
) -> Result<Dwarf4Sections, Error> {
    if units.is_empty() {
        return Err(Error::NoUnit);
    }
    if let Some(u) = units.iter().find(|u| u.plan.version != 4) {
        return Err(not_dwarf4(u));
    }
    if line.index() >= obj.section_count() {
        return Err(Error::Object(hod_obj::Error::TooLarge(hod_obj::Limit::Sections)));
    }
    if let Some(u) = units.iter().find(|u| !u.plan.sealed) {
        return Err(unsupported(u.plan.unit, "a unit plan that is not sealed"));
    }
    let symbols = obj.symbol_count();
    let dies: usize = units.iter().map(|u| u.plan.dies.len()).sum();
    let mut w = scratch_writer(dies);
    let mut out = Out {
        info: Vec::with_capacity(16 + dies * 12),
        relocs: Vec::with_capacity(8 + dies * 2),
        ranges: Vec::new(),
        range_relocs: Vec::new(),
        used: vec![0; (CODE_END as usize).div_ceil(64)],
        old: Vec::new(),
        map: Vec::new(),
        patches: Vec::new(),
        expr: Vec::new(),
        expr_relocs: Vec::new(),
        list: Vec::new(),
    };
    for u in units {
        // The unit as DWARF 5, then rewritten; the DWARF 5 bytes are
        // dropped (only `.debug_str` is kept, whose offsets they name).
        w.info.clear();
        w.relocs.clear();
        w.str_offsets.clear();
        w.addr.clear();
        w.rnglists.clear();
        w.unit(u, symbols)?;
        out.unit(&w, u.plan, 0, 0)?;
    }
    let abbrev = write_abbrev4(&out.used);
    if u32::try_from(out.info.len()).is_err() || u32::try_from(out.ranges.len()).is_err() {
        return Err(Error::TooLarge);
    }
    if obj.section_count() >= u32::MAX - 5 {
        return Err(Error::Object(hod_obj::Error::TooLarge(hod_obj::Limit::Sections)));
    }

    // Only now touch the object, in `llc`'s order of the sections.
    let target = crate::target::for_arch(obj.arch());
    let abbrev_sec = obj.add_section(b".debug_abbrev", SectionKind::Debug, 1)?;
    let info = obj.add_section(b".debug_info", SectionKind::Debug, 1)?;
    let ranges = if out.ranges.is_empty() {
        None
    } else {
        Some(obj.add_section(b".debug_ranges", SectionKind::Debug, 1)?)
    };
    let str_kind = SectionKind::Elf {
        sh_type: SHT_PROGBITS,
        sh_flags: SHF_MERGE | SHF_STRINGS,
        sh_entsize: 1,
    };
    let str_sec = obj.add_section(b".debug_str", str_kind, 1)?;
    obj.append_owned(abbrev_sec, abbrev);
    obj.append_owned(info, out.info);
    if let Some(r) = ranges {
        obj.append_owned(r, out.ranges);
    }
    obj.append_owned(str_sec, w.strings.into_data());
    let sym_abbrev = obj.section_symbol(abbrev_sec);
    let sym_str = obj.section_symbol(str_sec);
    let sym_line = obj.section_symbol(line);
    let sym_ranges = ranges.map(|r| obj.section_symbol(r));
    obj.reserve(0, 0, out.relocs.len() + out.range_relocs.len(), 0);
    for r in &out.relocs {
        let (symbol, wide) = match r.target {
            // `StrOffsets` and `Addr` are not made by `Out`: DWARF 5's
            // sections are not written.
            Target::Abbrev | Target::StrOffsets | Target::Addr => (sym_abbrev, false),
            Target::Str => (sym_str, false),
            Target::Line => (sym_line, false),
            Target::Rnglists => (sym_ranges.unwrap_or(sym_abbrev), false),
            Target::Symbol(s) | Target::Tls(s) => (s, true),
        };
        let kind = crate::x86_64::reloc_kind(r.target, wide, target);
        obj.add_relocation(info, r.offset as u64, kind, symbol, r.addend);
    }
    if let Some(r) = ranges {
        for &(offset, symbol, addend) in &out.range_relocs {
            obj.add_relocation(r, offset as u64, target.address_reloc, symbol, addend);
        }
    }
    Ok(Dwarf4Sections { abbrev: abbrev_sec, info, str: str_sec, ranges })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_4_forms_and_attributes() {
        assert_eq!(form4(form::STRX), DW_FORM_STRP);
        assert_eq!(form4(form::ADDRX), form::ADDR);
        assert_eq!(form4(form::RNGLISTX), form::SEC_OFFSET);
        assert_eq!(form4(form::REF4), form::REF4);
        assert!(!keeps(at::STR_OFFSETS_BASE) && !keeps(at::ADDR_BASE));
        assert!(!keeps(at::RNGLISTS_BASE) && !keeps(at::DELETED));
        assert!(keeps(at::NAME) && keeps(at::RANGES));
        assert_eq!(tag4(fam::STATIC_MEMBER), tag::MEMBER);
        assert_eq!(tag4(fam::STATIC_VARIABLE_ANY), tag::MEMBER);
        assert_eq!(tag4(fam::GLOBAL_VARIABLE), tag::VARIABLE);
    }

    #[test]
    fn every_code_has_its_family() {
        for (f, &base) in BASES.iter().enumerate() {
            assert_eq!(family_of(base), Some((f, 0)));
            let last = base + u32::from(FAMILIES[f].block) - 1;
            assert_eq!(family_of(last), Some((f, last - base)));
        }
        assert_eq!(family_of(0), None);
        assert_eq!(family_of(CODE_END), None);
    }

    #[test]
    fn lower_bounds_of_version_4() {
        // `DW_LANG_C_plus_plus_14` (0x21): 0 from version 5 on, none
        // before; `DW_LANG_C99` (0x0c) 0 in both; `DW_LANG_Fortran77`
        // (0x07) 1 in both.
        assert_eq!(default_lower_bound(4, 0x21, 0), -1);
        assert_eq!(default_lower_bound(5, 0x21, 0), 0);
        assert_eq!(default_lower_bound(4, 0x0c, 0), 0);
        assert_eq!(default_lower_bound(4, 0x07, 1), 1);
        assert_eq!(default_lower_bound(4, 0x14, 0), 0);
        assert_eq!(default_lower_bound(4, 0x15, 0), -1);
    }

    #[test]
    fn operand_lengths() {
        // fbreg -8, plus_uconst 300, addr, const8u, piece 4.
        assert_eq!(operands(0x91, &[0x91, 0x78], 1), Some(1));
        assert_eq!(operands(0x23, &[0x23, 0xac, 0x02], 1), Some(2));
        assert_eq!(operands(DW_OP_ADDR, &[3; 9], 1), Some(8));
        assert_eq!(operands(0x0e, &[0; 9], 1), Some(8));
        assert_eq!(operands(0x93, &[0x93, 4], 1), Some(1));
        assert_eq!(operands(0x9f, &[0x9f], 1), Some(0));
        // Truncated, or naming a DIE (`DW_OP_convert`).
        assert_eq!(operands(0x0e, &[0; 4], 1), None);
        assert_eq!(operands(0xa8, &[0xa8, 1], 1), None);
    }
}
