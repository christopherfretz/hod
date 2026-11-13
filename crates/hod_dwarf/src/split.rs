//! Split DWARF (`-gsplit-dwarf`; `spec/dwarf.md` §6.15, WP-062).
//!
//! What `llc -split-dwarf-file=N [-split-dwarf-output=F]` writes for a
//! DWARF 5 unit, and therefore hod:
//!
//! - the object keeps the line table and gets a **skeleton unit**
//!   (`DW_UT_skeleton`, `DW_TAG_skeleton_unit`): `DW_AT_stmt_list`,
//!   `DW_AT_str_offsets_base`, `DW_AT_comp_dir`, `DW_AT_dwo_name`, the
//!   unit's code (`low_pc`/`high_pc`, or `low_pc 0` and a `rnglistx`
//!   range list), `DW_AT_addr_base` and `DW_AT_rnglists_base`, with its
//!   own `.debug_abbrev`, `.debug_str`, `.debug_str_offsets`, and the
//!   unit's `.debug_addr` and `.debug_rnglists`;
//! - the **split unit** (`DW_UT_split_compile`) holds every other DIE,
//!   in `.debug_info.dwo`, with `.debug_abbrev.dwo`, `.debug_str.dwo`,
//!   `.debug_str_offsets.dwo` and `.debug_rnglists.dwo` (the blocks'
//!   range lists, as `DW_FORM_rnglistx`, which the full unit writes
//!   too since WP-163). Nothing in them is relocated:
//!   addresses are `.debug_addr` indices of the skeleton's table, file
//!   numbers are the skeleton's line table's;
//! - both headers carry the **DWO id**, which ties them together;
//! - with `-split-dwarf-output` the `.dwo` sections are a file of their
//!   own ([`SplitDwarf::Separate`]: hod attaches it to the object with
//!   `Object::set_dwo`); without, they are in the object, `SHF_EXCLUDE`
//!   ([`SplitDwarf::Single`], `-gsplit-dwarf=single`).
//!
//! A unit whose only DIE is its own (a `-gline-tables-only` unit without
//! inlined code) has no split unit: `llc` writes a skeleton with the
//! unit's names instead of `DW_AT_dwo_name`, DWO id 0, and no `.dwo`
//! section.
//!
//! The DIEs are those of [`crate::write_debug_info`], written by the same
//! code ([`Writer::die`]); what differs is laid out here: the two unit
//! DIEs, the headers, and where each section's bytes and relocations go.
//! A block's range list is written as for a full unit, a
//! `DW_FORM_rnglistx` index into the offsets of `.debug_rnglists.dwo`
//! (the table the full unit's writer makes, without the unit's own list,
//! which is the skeleton's).
//!
//! The DWO id is not `llc`'s (`DIEHash::computeCUSignature`, an MD5 of
//! the unit's DIEs after LLVM's type-signature rules, which includes the
//! frame offsets of locals that hod lays out differently): it is
//! [`dwo_id`] of the `.dwo` name and the split unit's sections, computed
//! once and written into both headers.

use hod_ir::{DiCompileUnit, DiId, EmissionKind};
use hod_obj::elf::{SHF_EXCLUDE, SHF_MERGE, SHF_STRINGS, SHT_PROGBITS};
use hod_obj::{Object, SectionId, SectionKind};

use crate::Error;
use crate::abbrev::{Used, code, fam, write_abbrev};
use crate::die::{
    Kind, NONE, Reloc, Sec, Target, UnitInput, UnitPlan, Writer, unsupported, write_debug_info,
};
use crate::leb::write_uleb;
use crate::strings::DebugStrings;
use crate::target::for_arch;

/// A unit with a DWO id: it stands for an external split unit (a clang
/// module's skeleton, `-gmodules`), which `llc` writes as
/// `DW_AT_GNU_dwo_id`/`DW_AT_GNU_dwo_name` on an ordinary unit.
pub const DWO_ID: &str = "debug:DICompileUnit.dwoId";
/// Split DWARF for a module with several compile units (linked IR): one
/// `.dwo` would hold several split units.
pub const SPLIT_SEVERAL_UNITS: &str = "debug:split-several-units";
/// Split DWARF for a unit with `splitDebugInlining: true` (LLVM's default
/// when the field is omitted; clang and kiln write `false`) and inlined
/// code: `llc` repeats the inlined subroutines in the skeleton.
pub const SPLIT_INLINING: &str = "debug:split-dwarf-inlining";
/// Split DWARF for a `-gline-tables-only` unit with DIEs of functions
/// (inlined code): a shape no `llc` output was compared with.
pub const SPLIT_REDUCED: &str = "debug:split-reduced-unit";
/// Split DWARF for a unit without `splitDebugFilename`, or whose name is
/// not the `-split-dwarf-file` the object is asked for: hod takes
/// `DW_AT_dwo_name` from the unit (`llc` from the option; clang and kiln
/// write the same name in both).
pub const SPLIT_NAME: &str = "debug:split-dwarf-name";
/// Split DWARF with `Dwarf Version` below 5 (GNU fission:
/// `DW_AT_GNU_dwo_name`, `DW_FORM_GNU_addr_index`, `.debug_addr` without
/// a header).
pub const SPLIT_DWARF_4: &str = "debug:split-dwarf-4";

/// How the DIEs of a module are written (`spec/dwarf.md` §6.15).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SplitDwarf {
    /// One full unit per compile unit ([`crate::write_debug_info`]).
    #[default]
    Off,
    /// Skeleton and split unit, the `.dwo` sections in the object
    /// (`-split-dwarf-file` without `-split-dwarf-output`).
    Single,
    /// Skeleton and split unit, the `.dwo` sections in an object of
    /// their own, attached to the object (`Object::set_dwo`).
    Separate,
}

/// What [`write_split_debug_info`] wrote.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SplitSections {
    /// The skeleton's `.debug_info`.
    pub info: SectionId,
    /// The DWO id of both headers; 0 when there is no split unit.
    pub dwo_id: u64,
    /// `.debug_info.dwo`, in the object ([`SplitDwarf::Single`]) or in
    /// the companion ([`SplitDwarf::Separate`]); `None` without a split
    /// unit.
    pub dwo_info: Option<SectionId>,
}

const DW_UT_SKELETON: u8 = 0x04;
const DW_UT_SPLIT_COMPILE: u8 = 0x05;
const DW_RLE_END_OF_LIST: u8 = 0x00;
const DW_RLE_STARTX_LENGTH: u8 = 0x03;
const DW_FORM_SEC_OFFSET: u64 = 0x17;
const DW_FORM_IMPLICIT_CONST: u64 = 0x21;
const DW_AT_LOCATION: u64 = 0x02;
/// A DWARF 5 split or skeleton unit header: 12 bytes and the DWO id.
const HEADER: usize = 20;
/// Where the DWO id is in that header.
const ID_AT: usize = 12;
/// A range-list table's header (DWARF 5 §7.28) before its offsets.
const LISTS_HEADER: usize = 12;

/// Refuses what no split-DWARF path writes, for every unit plan
/// (`UnitPlan::new`): a unit with a DWO id.
pub(crate) fn check_unit(unit: DiId, cu: &DiCompileUnit<'_>) -> Result<(), Error> {
    if cu.dwo_id != 0 {
        return Err(unsupported(unit, DWO_ID));
    }
    Ok(())
}

/// [`crate::write_debug_info`] for [`SplitDwarf::Off`], else
/// [`write_split_debug_info`]: what the code generator calls.
///
/// # Errors
/// As the function called.
pub fn write_unit_dies(
    obj: &mut Object,
    line: SectionId,
    units: &[UnitInput<'_, '_>],
    split: SplitDwarf,
) -> Result<(), Error> {
    if let Some(result) = crate::dwarf4::write_unit_dies(obj, line, units, split) {
        return result; // WP-114: §6.20
    }
    match split {
        SplitDwarf::Off => write_debug_info(obj, line, units).map(|_| ()),
        _ => write_split_debug_info(obj, line, units, split).map(|_| ()),
    }
}

/// The DWO id hod writes: a 64-bit hash of `parts` (the `.dwo` name,
/// then the split unit's `.debug_info.dwo` with a zero id,
/// `.debug_abbrev.dwo`, `.debug_str_offsets.dwo`, `.debug_str.dwo` and
/// `.debug_rnglists.dwo`, then `.debug_loclists.dwo` when there is one,
/// WP-162), never 0 (`spec/dwarf.md` §6.15).
///
/// Each part is hashed as its length, then its bytes in 8-byte
/// little-endian words (the last one zero-padded); each word goes
/// through `h = rotl((h ^ w) * K, 31)`, which is a bijection of `h ^ w`,
/// so two inputs that differ in one word never collide; the result is
/// `MurmurHash3`'s 64-bit finalizer of `h`.
pub fn dwo_id(parts: &[&[u8]]) -> u64 {
    const K: u64 = 0x9e37_79b9_7f4a_7c15;
    let step = |h: u64, w: u64| (h ^ w).wrapping_mul(K).rotate_left(31);
    let mut h: u64 = 0x6873_706c_6974_0001; // "hsplit", 1
    for part in parts {
        h = step(h, part.len() as u64);
        let mut words = part.chunks_exact(8);
        for word in &mut words {
            let mut b = [0u8; 8];
            b.copy_from_slice(word);
            h = step(h, u64::from_le_bytes(b));
        }
        let rest = words.remainder();
        if !rest.is_empty() {
            let mut b = [0u8; 8];
            b[..rest.len()].copy_from_slice(rest);
            h = step(h, u64::from_le_bytes(b));
        }
    }
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= h >> 33;
    h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    h ^= h >> 33;
    if h == 0 { 1 } else { h }
}

/// What the split unit's layout tells the skeleton.
struct Layout {
    /// The unit's code is one span (`low_pc`/`high_pc`).
    pc: bool,
    /// The unit's code is several spans (a range list).
    ranges: bool,
    /// The unit has a `.debug_addr` contribution.
    has_addr: bool,
    /// The `.debug_addr` index of the first span.
    span_base: usize,
}

impl<'a> Writer<'a> {
    fn for_split(dies: usize) -> Self {
        Writer {
            info: Vec::with_capacity(32 + dies * 10),
            str_offsets: Vec::with_capacity(8 + dies * 4),
            addr: Vec::new(),
            rnglists: Vec::new(),
            strings: DebugStrings::new(),
            used: Used::new(),
            relocs: Vec::with_capacity(8 + dies),
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
            block_starts: Vec::new(),
            loclists: Vec::new(),
            loclists_unit: crate::loclists::NO_LISTS,
            split: true,
            tls_addrs: Vec::new(),
        }
    }

    /// The split unit of `u` into `info` (when the unit has DIEs besides
    /// its own), with the unit's `.debug_str_offsets.dwo` entries (still
    /// as relocations), `.debug_addr` and the blocks' range lists after a
    /// range-list header: `Writer::unit` with the split unit's header and
    /// unit DIE.
    fn split_unit(
        &mut self,
        u: &UnitInput<'_, 'a>,
        symbols: u32,
        dwo_name: &'a [u8],
    ) -> Result<Layout, Error> {
        let plan = u.plan;
        let n = plan.dies.len();
        self.addr_of.clear();
        self.addr_of.resize(n, NONE);
        self.cur.clear();
        self.cur.extend(u.globals.iter().map(|g| (g.symbol, g.addend)));
        for (i, g) in u.globals.iter().enumerate() {
            if g.symbol.index() >= symbols {
                return Err(Error::BadSymbol { symbol: g.symbol.index() });
            }
            let die =
                plan.die_of_expr(g.expr).ok_or(Error::GlobalNotInUnit { expr: g.expr.raw() })?;
            if self.addr_of[die as usize] != NONE {
                return Err(Error::DuplicateGlobal { expr: g.expr.raw() });
            }
            self.addr_of[die as usize] = i as u32;
        }
        self.spans(u.code, symbols)?;
        let globals = u
            .globals
            .iter()
            .filter(|g| {
                plan.die_of_expr(g.expr).is_some_and(|d| {
                    plan.global_takes_address(d) || crate::x86_64::split_tls_entry(plan, d) // WP-189
                })
            })
            .count()
            + plan.template_addrs();
        self.clear_addrs();
        self.offsets.clear();
        self.offsets.resize(n, NONE);
        self.patches.clear();
        self.strings.begin_unit();
        self.unit_start = 0;

        let ranges = self.spans.len() > 1;
        // The blocks' lists are the `.dwo` table's; the unit's own list
        // is the skeleton's.
        let span_base = self.begin_functions(plan, globals);
        let layout = Layout {
            pc: self.spans.len() == 1,
            ranges,
            has_addr: span_base > 0 || !self.spans.is_empty(),
            span_base,
        };
        if n > 1 {
            // The header (DWARF 5 §7.5.1.2): the abbreviations at 0 of
            // `.debug_abbrev.dwo`, the DWO id written last.
            self.info.extend_from_slice(&[0; 4]);
            self.info.extend_from_slice(&5u16.to_le_bytes());
            self.info.push(DW_UT_SPLIT_COMPILE);
            self.info.push(8);
            self.info.extend_from_slice(&[0; 12]);
            debug_assert_eq!(self.info.len(), HEADER);
            self.offsets[0] = HEADER as u32;
            let cu = &plan.cu;
            self.abbrev(code(fam::SPLIT_COMPILE_UNIT, 0, true));
            self.strx(cu.producer)?;
            self.info.extend_from_slice(&cu.language.to_le_bytes());
            self.strx(plan.name)?;
            self.strx(dwo_name)?;

            // The children, depth first, as `Writer::unit`.
            self.stack.clear();
            self.stack.push((0, plan.children_of(0).start));
            while let Some(top) = self.stack.last_mut() {
                let (die, cursor) = *top;
                if cursor == plan.children_of(die).end {
                    self.stack.pop();
                    self.info.push(0);
                    continue;
                }
                top.1 += 1;
                let child = plan.children[cursor];
                let has_children = !plan.children_of(child).is_empty();
                self.die(plan, child, has_children)?;
                if has_children {
                    self.stack.push((child, plan.children_of(child).start));
                }
            }
            for &(at, die) in &self.patches {
                let offset = self.offsets[die as usize];
                debug_assert_ne!(offset, NONE, "every DIE of the plan is written");
                self.info[at..at + 4].copy_from_slice(&offset.to_le_bytes());
            }
            let length = u32::try_from(self.info.len() - 4)
                .ok()
                .filter(|&l| l < 0xffff_fff0)
                .ok_or(Error::TooLarge)?;
            self.info[..4].copy_from_slice(&length.to_le_bytes());
        }
        debug_assert_eq!(self.addrs.len(), span_base);
        self.contributions(layout.has_addr, false, span_base)?;
        Ok(layout)
    }
}

fn read_uleb(bytes: &[u8], at: &mut usize) -> u64 {
    let mut value = 0u64;
    let mut shift = 0;
    while let Some(&b) = bytes.get(*at) {
        *at += 1;
        if shift < 64 {
            value |= u64::from(b & 0x7f) << shift;
        }
        shift += 7;
        if b & 0x80 == 0 {
            break;
        }
    }
    value
}

/// `.debug_abbrev.dwo`: the table of `used`, with every
/// `DW_AT_location`/`DW_FORM_sec_offset` pair (a location list, WP-162)
/// turned into `DW_FORM_loclistx` (both forms are one ULEB byte, so
/// nothing moves).
fn dwo_abbrev(used: &Used) -> Vec<u8> {
    let mut table = write_abbrev(used);
    let mut at = 0;
    loop {
        if read_uleb(&table, &mut at) == 0 {
            break; // the terminating 0
        }
        read_uleb(&table, &mut at); // the tag
        at += 1; // DW_CHILDREN_*
        loop {
            let name = read_uleb(&table, &mut at);
            let form_at = at;
            let form = read_uleb(&table, &mut at);
            if name == 0 && form == 0 {
                break;
            }
            if form == DW_FORM_IMPLICIT_CONST {
                while table.get(at).is_some_and(|&b| b & 0x80 != 0) {
                    at += 1;
                }
                at += 1;
            }
            if name == DW_AT_LOCATION && form == DW_FORM_SEC_OFFSET {
                table[form_at] = crate::abbrev::form::LOCLISTX; // WP-162
            }
        }
        if at >= table.len() {
            break;
        }
    }
    table
}

/// Writes the DIEs of `units` as split DWARF (`spec/dwarf.md` §6.15):
/// the skeleton unit and its sections into `obj`, the split unit's
/// sections into `obj` ([`SplitDwarf::Single`]) or into an object
/// attached to it with `Object::set_dwo` ([`SplitDwarf::Separate`]; an
/// empty one when there is no split unit, since the `.dwo` file is
/// written either way). `line` is the `.debug_line` section of the units'
/// line tables. On an error `obj` is unchanged.
///
/// # Errors
/// As [`crate::write_debug_info`]; `Unsupported` with [`SPLIT_SEVERAL_UNITS`],
/// [`SPLIT_NAME`], [`SPLIT_REDUCED`], [`SPLIT_INLINING`]; `Unsupported`
/// without a `debug:` name for [`SplitDwarf::Off`] or a DIE that would
/// need a relocation in the `.dwo`.
#[expect(clippy::too_many_lines, reason = "checks, then every section in order")]
pub fn write_split_debug_info(
    obj: &mut Object,
    line: SectionId,
    units: &[UnitInput<'_, '_>],
    split: SplitDwarf,
) -> Result<SplitSections, Error> {
    let Some(u) = units.first() else { return Err(Error::NoUnit) };
    let plan = u.plan;
    if split == SplitDwarf::Off {
        return Err(unsupported(plan.unit, "split DWARF asked for with SplitDwarf::Off"));
    }
    if let Some(second) = units.get(1) {
        return Err(unsupported(second.plan.unit, SPLIT_SEVERAL_UNITS));
    }
    if line.index() >= obj.section_count() {
        return Err(Error::Object(hod_obj::Error::TooLarge(hod_obj::Limit::Sections)));
    }
    if !plan.sealed {
        return Err(unsupported(plan.unit, "a unit plan that is not sealed"));
    }
    check(plan)?;
    let dwo_name = plan.cu.split_debug_filename;
    let has_split = plan.dies.len() > 1;
    let symbols = obj.symbol_count();
    let mut w = Writer::for_split(plan.dies.len());
    let layout = w.split_unit(u, symbols, dwo_name)?;

    // The `.dwo` sections: no relocation survives.
    let mut locs: Vec<(usize, i64)> = Vec::new();
    for r in &w.relocs {
        match (r.sec, r.target) {
            (Sec::Info, Target::Loclists) => locs.push((r.offset, r.addend)), // WP-162
            (Sec::StrOffsets, Target::Str) => {
                let offset = u32::try_from(r.addend).map_err(|_| Error::TooLarge)?;
                w.str_offsets[r.offset..r.offset + 4].copy_from_slice(&offset.to_le_bytes());
            }
            (Sec::Addr, Target::Symbol(_) | Target::Tls(_)) => {} // WP-189: §6.24
            _ => return Err(unsupported(plan.unit, "a split unit attribute with a relocation")),
        }
    }
    // `.debug_rnglists.dwo`: the blocks' lists through their offsets
    // (`Writer::contributions`), empty without one.
    let rnglists = std::mem::take(&mut w.rnglists);
    let loclists = crate::loclists::dwo_loclists(&mut w, &locs)?; // WP-162
    let abbrev = dwo_abbrev(&w.used);
    let strings = std::mem::take(&mut w.strings).into_data();
    let id = if has_split {
        let mut parts: Vec<&[u8]> =
            vec![dwo_name, &w.info, &abbrev, &w.str_offsets, &strings, &rnglists];
        if !loclists.is_empty() {
            parts.push(&loclists); // WP-162
        }
        let id = dwo_id(&parts);
        w.info[ID_AT..ID_AT + 8].copy_from_slice(&id.to_le_bytes());
        id
    } else {
        0
    };

    // The skeleton.
    let skeleton = skeleton(plan, &w, &layout, u.line_offset, id)?;
    for len in [w.info.len(), w.str_offsets.len(), w.addr.len(), skeleton.info.len()] {
        if u32::try_from(len).is_err() {
            return Err(Error::TooLarge);
        }
    }
    if obj.section_count() >= u32::MAX - 13 {
        return Err(Error::Object(hod_obj::Error::TooLarge(hod_obj::Limit::Sections)));
    }

    // Only now touch the object.
    let target = for_arch(obj.arch());
    let abbrev_sec = obj.add_section(b".debug_abbrev", SectionKind::Debug, 1)?;
    let info = obj.add_section(b".debug_info", SectionKind::Debug, 1)?;
    let str_offsets = obj.add_section(b".debug_str_offsets", SectionKind::Debug, 1)?;
    let str_kind = SectionKind::Elf {
        sh_type: SHT_PROGBITS,
        sh_flags: SHF_MERGE | SHF_STRINGS,
        sh_entsize: 1,
    };
    let str_sec = obj.add_section(b".debug_str", str_kind, 1)?;
    let addr = if layout.has_addr {
        Some(obj.add_section(b".debug_addr", SectionKind::Debug, 1)?)
    } else {
        None
    };
    let lists_sec = if layout.ranges {
        Some(obj.add_section(b".debug_rnglists", SectionKind::Debug, 1)?)
    } else {
        None
    };
    obj.append_owned(abbrev_sec, skeleton.abbrev);
    obj.append_owned(info, skeleton.info);
    obj.append_owned(str_offsets, skeleton.str_offsets);
    obj.append_owned(str_sec, skeleton.strings);
    if let Some(a) = addr {
        obj.append_owned(a, std::mem::take(&mut w.addr));
    }
    if let Some(r) = lists_sec {
        obj.append_owned(r, skeleton.rnglists);
    }
    let sym_abbrev = obj.section_symbol(abbrev_sec);
    let sym_str = obj.section_symbol(str_sec);
    let sym_str_offsets = obj.section_symbol(str_offsets);
    let sym_line = obj.section_symbol(line);
    let sym_addr = addr.map(|a| obj.section_symbol(a));
    let sym_lists = lists_sec.map(|r| obj.section_symbol(r));
    obj.reserve(0, 0, skeleton.relocs.len() + w.addrs.len(), 0);
    for r in skeleton.relocs.iter().chain(w.relocs.iter().filter(|r| r.sec == Sec::Addr)) {
        let (symbol, wide) = match r.target {
            // `Loclists`: only in the split unit (WP-162).
            Target::Abbrev | Target::Loclists => (sym_abbrev, false),
            Target::Str => (sym_str, false),
            Target::StrOffsets => (sym_str_offsets, false),
            Target::Line => (sym_line, false),
            Target::Addr => (sym_addr.unwrap_or(sym_abbrev), false),
            Target::Rnglists => (sym_lists.unwrap_or(sym_abbrev), false),
            // `Tls`: a thread-local offset in `.debug_addr` (WP-189).
            Target::Symbol(s) | Target::Tls(s) => (s, true),
        };
        let section = match r.sec {
            Sec::Info => info,
            Sec::StrOffsets => str_offsets,
            Sec::Addr => addr.unwrap_or(info),
        };
        let kind = crate::x86_64::reloc_kind(r.target, wide, target); // WP-189
        obj.add_relocation(section, r.offset as u64, kind, symbol, r.addend);
    }

    // The `.dwo` sections, in `llc`'s order.
    let mut companion = Object::new(obj.arch());
    let dwo = if split == SplitDwarf::Single { &mut *obj } else { &mut companion };
    let mut dwo_info = None;
    if has_split {
        let kind = SectionKind::Elf { sh_type: SHT_PROGBITS, sh_flags: SHF_EXCLUDE, sh_entsize: 0 };
        let str_kind = SectionKind::Elf {
            sh_type: SHT_PROGBITS,
            sh_flags: SHF_MERGE | SHF_STRINGS | SHF_EXCLUDE,
            sh_entsize: 1,
        };
        let s = dwo.add_section(b".debug_str_offsets.dwo", kind, 1)?;
        dwo.append_owned(s, std::mem::take(&mut w.str_offsets));
        let s = dwo.add_section(b".debug_str.dwo", str_kind, 1)?;
        dwo.append_owned(s, strings);
        let s = dwo.add_section(b".debug_info.dwo", kind, 1)?;
        dwo.append_owned(s, std::mem::take(&mut w.info));
        dwo_info = Some(s);
        let s = dwo.add_section(b".debug_abbrev.dwo", kind, 1)?;
        dwo.append_owned(s, abbrev);
        if !rnglists.is_empty() {
            let s = dwo.add_section(b".debug_rnglists.dwo", kind, 1)?;
            dwo.append_owned(s, rnglists);
        }
        if !loclists.is_empty() {
            let s = dwo.add_section(b".debug_loclists.dwo", kind, 1)?; // WP-162
            dwo.append_owned(s, loclists);
        }
    }
    if split == SplitDwarf::Separate {
        obj.set_dwo(companion);
    }
    Ok(SplitSections { info, dwo_id: id, dwo_info })
}

/// What [`write_split_debug_info`] refuses besides the full writer.
fn check(plan: &UnitPlan<'_>) -> Result<(), Error> {
    if plan.version == 4 {
        return Err(unsupported(plan.unit, SPLIT_DWARF_4)); // WP-114: §6.20
    }
    let cu = &plan.cu;
    // clang writes no name for a unit that is only a line table, whose
    // skeleton names no `.dwo`.
    if cu.split_debug_filename.is_empty() && plan.dies.len() > 1 {
        return Err(unsupported(plan.unit, SPLIT_NAME));
    }
    if cu.emission_kind != EmissionKind::FullDebug && plan.dies.len() > 1 {
        return Err(unsupported(plan.unit, SPLIT_REDUCED));
    }
    if cu.split_debug_inlining && plan.dies.iter().any(|d| d.kind == Kind::Inlined) {
        return Err(unsupported(plan.unit, SPLIT_INLINING));
    }
    Ok(())
}

/// The skeleton's sections before the object is touched.
struct Skeleton {
    info: Vec<u8>,
    abbrev: Vec<u8>,
    str_offsets: Vec<u8>,
    strings: Vec<u8>,
    rnglists: Vec<u8>,
    relocs: Vec<Reloc>,
}

/// The skeleton unit of `plan` (`id` 0: the unit alone, with its names).
fn skeleton<'a>(
    plan: &UnitPlan<'a>,
    w: &Writer<'a>,
    layout: &Layout,
    line_offset: u32,
    id: u64,
) -> Result<Skeleton, Error> {
    let mut strings = DebugStrings::new();
    let mut used = Used::new();
    let mut relocs = Vec::new();
    let mut info = Vec::with_capacity(64);
    let mut sec_offset = |info: &mut Vec<u8>, target: Target, addend: i64| {
        relocs.push(Reloc { sec: Sec::Info, offset: info.len(), target, addend });
        info.extend_from_slice(&[0; 4]);
    };
    info.extend_from_slice(&[0; 4]);
    info.extend_from_slice(&5u16.to_le_bytes());
    info.push(DW_UT_SKELETON);
    info.push(8);
    sec_offset(&mut info, Target::Abbrev, 0);
    info.extend_from_slice(&id.to_le_bytes());
    debug_assert_eq!(info.len(), HEADER);

    let alone = id == 0;
    let family = match (alone, layout.ranges) {
        (false, false) => fam::SKELETON_UNIT,
        (false, true) => fam::SKELETON_UNIT_RANGES,
        (true, false) => fam::SKELETON_UNIT_ALONE,
        (true, true) => fam::SKELETON_UNIT_ALONE_RANGES,
    };
    let mask =
        if layout.ranges { 0 } else { u32::from(layout.pc) | u32::from(layout.has_addr) << 1 };
    let unit_code = code(family, mask, false);
    used.mark(unit_code);
    write_uleb(&mut info, u64::from(unit_code));
    sec_offset(&mut info, Target::Line, i64::from(line_offset));
    sec_offset(&mut info, Target::StrOffsets, 8);
    write_uleb(&mut info, u64::from(strings.index(plan.comp_dir)?));
    if alone {
        write_uleb(&mut info, u64::from(strings.index(plan.cu.producer)?));
        info.extend_from_slice(&plan.cu.language.to_le_bytes());
        write_uleb(&mut info, u64::from(strings.index(plan.name)?));
    } else {
        write_uleb(&mut info, u64::from(strings.index(plan.cu.split_debug_filename)?));
    }
    if layout.pc {
        let size = u32::try_from(w.spans[0].size).map_err(|_| Error::TooLarge)?;
        write_uleb(&mut info, layout.span_base as u64);
        info.extend_from_slice(&size.to_le_bytes());
    }
    if layout.ranges {
        info.extend_from_slice(&[0; 8]); // DW_AT_low_pc 0, no relocation
        write_uleb(&mut info, 0);
    }
    if layout.has_addr {
        sec_offset(&mut info, Target::Addr, 8);
    }
    if layout.ranges {
        sec_offset(&mut info, Target::Rnglists, LISTS_HEADER as i64);
    }
    let length = u32::try_from(info.len() - 4).map_err(|_| Error::TooLarge)?;
    info[..4].copy_from_slice(&length.to_le_bytes());

    // `.debug_str_offsets`, relocated against `.debug_str`.
    let offs = strings.unit_offsets();
    let mut str_offsets = Vec::with_capacity(8 + 4 * offs.len());
    str_offsets.extend_from_slice(&(4 + 4 * offs.len() as u32).to_le_bytes());
    str_offsets.extend_from_slice(&5u16.to_le_bytes());
    str_offsets.extend_from_slice(&[0; 2]);
    for &off in offs {
        relocs.push(Reloc {
            sec: Sec::StrOffsets,
            offset: str_offsets.len(),
            target: Target::Str,
            addend: i64::from(off),
        });
        str_offsets.extend_from_slice(&[0; 4]);
    }

    // `.debug_rnglists`: the unit's list, one `DW_RLE_startx_length` per
    // span, as a full unit's.
    let mut rnglists = Vec::new();
    if layout.ranges {
        rnglists.extend_from_slice(&[0; 4]);
        rnglists.extend_from_slice(&5u16.to_le_bytes());
        rnglists.extend_from_slice(&[8, 0]);
        rnglists.extend_from_slice(&1u32.to_le_bytes());
        rnglists.extend_from_slice(&4u32.to_le_bytes());
        for (k, s) in w.spans.iter().enumerate() {
            rnglists.push(DW_RLE_STARTX_LENGTH);
            write_uleb(&mut rnglists, (layout.span_base + k) as u64);
            write_uleb(&mut rnglists, s.size);
        }
        rnglists.push(DW_RLE_END_OF_LIST);
        let length = u32::try_from(rnglists.len() - 4).map_err(|_| Error::TooLarge)?;
        rnglists[..4].copy_from_slice(&length.to_le_bytes());
    }
    Ok(Skeleton {
        info,
        abbrev: write_abbrev(&used),
        str_offsets,
        strings: strings.into_data(),
        rnglists,
        relocs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dwo_ids_differ_and_are_never_zero() {
        let a = dwo_id(&[b"a.dwo", b"\x01\x02"]);
        assert_ne!(a, dwo_id(&[b"b.dwo", b"\x01\x02"]));
        assert_ne!(a, dwo_id(&[b"a.dwo", b"\x01\x03"]));
        // The parts' lengths are hashed: moving a byte between parts
        // changes the id.
        assert_ne!(a, dwo_id(&[b"a.dwo\x01", b"\x02"]));
        assert_eq!(a, dwo_id(&[b"a.dwo", b"\x01\x02"]));
        assert_ne!(dwo_id(&[]), 0);
    }

    /// WP-163: every `DW_AT_ranges` of the table is `DW_FORM_rnglistx`,
    /// so neither a `.dwo` nor a full unit has a relocation per range
    /// list (§6.4, §6.15).
    #[test]
    fn every_range_list_is_rnglistx() {
        let mut ranges = 0;
        for f in crate::abbrev::FAMILIES {
            for slot in f.slots {
                if slot.at == crate::abbrev::at::RANGES {
                    assert_eq!(slot.form, crate::abbrev::form::RNGLISTX, "{}", f.name);
                    ranges += 1;
                }
            }
        }
        assert!(ranges >= 4, "the unit, blocks and inlined subroutines have ranges");
    }
}
