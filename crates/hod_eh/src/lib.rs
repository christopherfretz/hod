//! Unwind tables: `.eh_frame` and `.gcc_except_table`.
//!
//! See `docs/ARCHITECTURE.md` for this crate's role and `spec/eh.md` for its
//! contract. This crate writes the call-frame information (`spec/eh.md`
//! §1 to §8): one `.eh_frame` per object from one [`Fde`] per function,
//! through [`write_eh_frame`]. The LSDA (`.gcc_except_table`) is a later
//! work package; an FDE already carries the reference to it.
//!
//! ```
//! use hod_eh::{Fde, Op, Row, write_eh_frame};
//! use hod_obj::{Arch, Object, SectionKind, Symbol, SymbolKind};
//!
//! let mut obj = Object::new(Arch::AArch64);
//! let text = obj.add_section(b".text", SectionKind::Text, 4)?;
//! obj.append(text, &[0; 24]);
//! let name = obj.add_string(b"f")?;
//! obj.add_symbol(Symbol::defined(name, SymbolKind::Func, text, 0, 24));
//! let section = obj.section_symbol(text);
//! let rows = [
//!     Row { offset: 4, op: Op::DefCfaOffset(16) },
//!     Row { offset: 8, op: Op::DefCfa { reg: 29, offset: 16 } },
//!     Row { offset: 8, op: Op::Offset { reg: 30, cfa_offset: -8 } },
//!     Row { offset: 8, op: Op::Offset { reg: 29, cfa_offset: -16 } },
//! ];
//! let fde = Fde { order: 0, start: section, start_addend: 0, size: 24, rows: &rows,
//!                 personality: None, lsda: None };
//! let eh_frame = write_eh_frame(&mut obj, &[fde])?.expect("one FDE");
//! assert_eq!(obj.relocation_count(eh_frame), 1);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![forbid(unsafe_code)]

mod leb;
mod target;

use std::fmt;

use hod_obj::elf::SHF_ALLOC;
use hod_obj::{
    Binding, GroupKind, Object, SectionId, SectionKind, Symbol, SymbolId, SymbolKind, Visibility,
};

use leb::{Sink, sleb, uleb};
pub use leb::{uleb_len, write_sleb, write_uleb};
pub use target::{AARCH64, CfiTarget, X86_64, for_arch, pe};

/// A DWARF call-frame instruction, as data (`spec/codegen.md` §9.3,
/// `spec/eh.md` §4). Registers are DWARF register numbers; offsets are in
/// bytes, not yet divided by the data alignment factor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    /// The CFA is `reg + offset`.
    DefCfa {
        /// The register.
        reg: u16,
        /// The offset.
        offset: i32,
    },
    /// The CFA keeps its register; its offset is now this.
    DefCfaOffset(i32),
    /// The CFA keeps its offset; its register is now this.
    DefCfaRegister(u16),
    /// `reg` is saved at `CFA + cfa_offset`.
    Offset {
        /// The saved register.
        reg: u16,
        /// Where, from the CFA.
        cfa_offset: i32,
    },
    /// `reg` has its entry value again (the rule of the CIE).
    Restore {
        /// The register.
        reg: u16,
    },
}

/// A call-frame instruction and the code offset, from the function's
/// start, from which it holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Row {
    /// The code offset.
    pub offset: u32,
    /// The instruction.
    pub op: Op,
}

/// The LSDA reference of an FDE: a symbol and addend the caller names
/// (`spec/eh.md` §3.3).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Lsda {
    /// The symbol, usually the section symbol of `.gcc_except_table`.
    pub symbol: SymbolId,
    /// The addend: the LSDA's offset from the symbol.
    pub addend: i64,
}

/// One function's frame description, the input of [`write_eh_frame`]
/// (`spec/eh.md` §3).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Fde<'a> {
    /// The function's position in the module. FDEs are written in this
    /// order within their CIE, whatever order they are passed in.
    pub order: u32,
    /// The symbol the initial location is relative to: the section symbol
    /// of the function's section, as `llc` does it.
    pub start: SymbolId,
    /// The function's offset from `start`.
    pub start_addend: i64,
    /// The size of the function's code.
    pub size: u32,
    /// The call-frame rows, in code order.
    pub rows: &'a [Row],
    /// The personality's indirection slot (`DW.ref.<personality>`, see
    /// [`personality_ref`]), for a function that uses one.
    pub personality: Option<SymbolId>,
    /// The function's LSDA; only with a personality.
    pub lsda: Option<Lsda>,
}

/// Why `.eh_frame` could not be written. `order` names the [`Fde`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Error {
    /// A row's offset is below the previous row's.
    RowOutOfOrder {
        /// The FDE.
        order: u32,
        /// The row's index in [`Fde::rows`].
        row: usize,
    },
    /// A row's offset is beyond the function's size.
    RowBeyondEnd {
        /// The FDE.
        order: u32,
        /// The row's index in [`Fde::rows`].
        row: usize,
    },
    /// A row's offset is not a multiple of the target's data alignment
    /// factor where the encoding needs one (a saved register's place, a
    /// negative CFA offset), or a code offset not one of the code
    /// alignment factor.
    Unfactored {
        /// The FDE.
        order: u32,
        /// The row's index in [`Fde::rows`].
        row: usize,
    },
    /// An FDE has an LSDA but no personality.
    LsdaWithoutPersonality {
        /// The FDE.
        order: u32,
    },
    /// Two FDEs have the same `order`.
    DuplicateOrder {
        /// The order.
        order: u32,
    },
    /// The section would exceed what 32-bit CIE pointers reach.
    TooLarge,
    /// The object model refused the section.
    Object(hod_obj::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::RowOutOfOrder { order, row } => {
                write!(f, "FDE {order}: CFI row {row} is before the previous row")
            }
            Self::RowBeyondEnd { order, row } => {
                write!(f, "FDE {order}: CFI row {row} is beyond the end of the function")
            }
            Self::Unfactored { order, row } => {
                write!(f, "FDE {order}: CFI row {row} is not a multiple of the alignment factor")
            }
            Self::LsdaWithoutPersonality { order } => {
                write!(f, "FDE {order} has an LSDA but no personality")
            }
            Self::DuplicateOrder { order } => write!(f, "two FDEs have order {order}"),
            Self::TooLarge => f.write_str(".eh_frame would exceed 4 GiB"),
            Self::Object(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<hod_obj::Error> for Error {
    fn from(e: hod_obj::Error) -> Self {
        Self::Object(e)
    }
}

// DWARF 5 §7.24 call-frame instruction encodings.
const DW_CFA_ADVANCE_LOC: u8 = 0x40;
const DW_CFA_OFFSET: u8 = 0x80;
const DW_CFA_RESTORE: u8 = 0xc0;
const DW_CFA_ADVANCE_LOC1: u8 = 0x02;
const DW_CFA_ADVANCE_LOC2: u8 = 0x03;
const DW_CFA_ADVANCE_LOC4: u8 = 0x04;
const DW_CFA_OFFSET_EXTENDED: u8 = 0x05;
const DW_CFA_RESTORE_EXTENDED: u8 = 0x06;
const DW_CFA_DEF_CFA: u8 = 0x0c;
const DW_CFA_DEF_CFA_REGISTER: u8 = 0x0d;
const DW_CFA_DEF_CFA_OFFSET: u8 = 0x0e;
const DW_CFA_OFFSET_EXTENDED_SF: u8 = 0x11;
const DW_CFA_DEF_CFA_SF: u8 = 0x12;
const DW_CFA_DEF_CFA_OFFSET_SF: u8 = 0x13;

/// The longest encoding of one row: `DW_CFA_advance_loc4` and an
/// instruction with two LEB128 operands (a 16-bit register, a 32-bit
/// factored offset).
const MAX_ROW: usize = 5 + 1 + 3 + 5;

/// A small stack buffer an FDE is encoded into before it is appended.
struct Staging {
    buf: [u8; 128],
    len: usize,
}

impl Staging {
    const fn new() -> Self {
        Staging { buf: [0; 128], len: 0 }
    }
    /// Appends what it holds to `out` when another row might not fit.
    #[inline]
    fn make_room(&mut self, out: &mut Vec<u8>, need: usize) {
        if self.len + need > self.buf.len() {
            self.flush(out);
        }
    }
    #[inline]
    fn flush(&mut self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.buf[..self.len]);
        self.len = 0;
    }
}

impl Sink for Staging {
    #[inline]
    fn byte(&mut self, b: u8) {
        self.buf[self.len] = b;
        self.len += 1;
    }
    #[inline]
    fn bytes(&mut self, b: &[u8]) {
        self.buf[self.len..self.len + b.len()].copy_from_slice(b);
        self.len += b.len();
    }
}

/// Appends the advance of the code location by `delta` (already divided
/// by the code alignment factor), with the shortest of
/// `DW_CFA_advance_loc`, `_loc1`, `_loc2`, `_loc4` that holds it, nothing
/// for no change (`spec/eh.md` §4.1).
pub fn write_advance(out: &mut Vec<u8>, delta: u32) {
    advance(out, delta);
}

#[inline]
fn advance<S: Sink>(out: &mut S, delta: u32) {
    if delta == 0 {
        // Nothing.
    } else if delta < 0x40 {
        out.byte(DW_CFA_ADVANCE_LOC | delta as u8);
    } else if delta <= 0xff {
        out.bytes(&[DW_CFA_ADVANCE_LOC1, delta as u8]);
    } else if delta <= 0xffff {
        out.byte(DW_CFA_ADVANCE_LOC2);
        out.bytes(&(delta as u16).to_le_bytes());
    } else {
        out.byte(DW_CFA_ADVANCE_LOC4);
        out.bytes(&delta.to_le_bytes());
    }
}

/// `value / factor` when it divides exactly. The factors of every target
/// are powers of two (±1, ±4, ±8), which take a shift, not a division.
#[inline]
fn factored(value: i32, factor: i32) -> Option<i64> {
    let magnitude = factor.unsigned_abs();
    let quotient = if magnitude.is_power_of_two() {
        if value as u32 & (magnitude - 1) != 0 {
            return None;
        }
        i64::from(value >> magnitude.trailing_zeros())
    } else {
        let (value, m) = (i64::from(value), i64::from(magnitude));
        if value % m != 0 {
            return None;
        }
        value / m
    };
    Some(if factor < 0 { -quotient } else { quotient })
}

/// Appends one call-frame instruction, as `llc` chooses its form
/// (`spec/eh.md` §4.2). `None` when an offset does not factor.
pub fn write_op(out: &mut Vec<u8>, target: &CfiTarget, op: Op) -> Option<()> {
    instruction(out, target, op)
}

#[inline]
fn instruction<S: Sink>(out: &mut S, target: &CfiTarget, op: Op) -> Option<()> {
    match op {
        Op::DefCfa { reg, offset } => {
            if offset >= 0 {
                out.byte(DW_CFA_DEF_CFA);
                uleb(out, u64::from(reg));
                uleb(out, offset as u64);
            } else {
                let f = factored(offset, target.data_align)?;
                out.byte(DW_CFA_DEF_CFA_SF);
                uleb(out, u64::from(reg));
                sleb(out, f);
            }
        }
        Op::DefCfaOffset(offset) => {
            if offset >= 0 {
                out.byte(DW_CFA_DEF_CFA_OFFSET);
                uleb(out, offset as u64);
            } else {
                let f = factored(offset, target.data_align)?;
                out.byte(DW_CFA_DEF_CFA_OFFSET_SF);
                sleb(out, f);
            }
        }
        Op::DefCfaRegister(reg) => {
            out.byte(DW_CFA_DEF_CFA_REGISTER);
            uleb(out, u64::from(reg));
        }
        Op::Offset { reg, cfa_offset } => {
            let f = factored(cfa_offset, target.data_align)?;
            if f < 0 {
                out.byte(DW_CFA_OFFSET_EXTENDED_SF);
                uleb(out, u64::from(reg));
                sleb(out, f);
            } else if reg < 64 {
                out.byte(DW_CFA_OFFSET | reg as u8);
                uleb(out, f as u64);
            } else {
                out.byte(DW_CFA_OFFSET_EXTENDED);
                uleb(out, u64::from(reg));
                uleb(out, f as u64);
            }
        }
        Op::Restore { reg } => {
            if reg < 64 {
                out.byte(DW_CFA_RESTORE | reg as u8);
            } else {
                out.byte(DW_CFA_RESTORE_EXTENDED);
                uleb(out, u64::from(reg));
            }
        }
    }
    Some(())
}

/// Appends an FDE's instructions: each row's advance, then its
/// instruction (`spec/eh.md` §4), staged in `stage`.
#[inline]
fn write_rows(
    out: &mut Vec<u8>,
    stage: &mut Staging,
    target: &CfiTarget,
    fde: &Fde<'_>,
) -> Result<(), Error> {
    let mut at = 0u32;
    let code_align = target.code_align;
    for (row, r) in fde.rows.iter().enumerate() {
        let order = fde.order;
        if r.offset < at {
            return Err(Error::RowOutOfOrder { order, row });
        }
        if r.offset > fde.size {
            return Err(Error::RowBeyondEnd { order, row });
        }
        let mut delta = r.offset - at;
        if code_align != 1 {
            if !delta.is_multiple_of(code_align) {
                return Err(Error::Unfactored { order, row });
            }
            delta /= code_align;
        }
        stage.make_room(out, MAX_ROW);
        advance(stage, delta);
        at = r.offset;
        instruction(stage, target, r.op).ok_or(Error::Unfactored { order, row })?;
    }
    Ok(())
}

/// Pads `out` with `DW_CFA_nop` to a multiple of `align`.
fn pad(out: &mut Vec<u8>, align: usize) {
    let len = out.len().next_multiple_of(align);
    out.resize(len, 0);
}

/// Stores the length of the entry that starts at `start` and ends at the
/// end of `out`.
fn patch_length(out: &mut [u8], start: usize) {
    let len = (out.len() - start - 4) as u32;
    out[start..start + 4].copy_from_slice(&len.to_le_bytes());
}

/// A relocation of the section being built: `pointer` selects the target's
/// pointer relocation over its initial-location one.
struct PendingReloc {
    offset: u32,
    pointer: bool,
    symbol: SymbolId,
    addend: i64,
}

/// The FDEs in the order they are written (`spec/eh.md` §5): grouped by
/// CIE, the CIE without a personality first, then one per personality in
/// the order of the first function (by `order`) that uses it; within a
/// CIE by `order`. Returns the order and, per CIE, its personality and the
/// number of FDEs.
/// The FDEs' indices in output order, and per CIE its personality and its
/// number of FDEs.
type Layout = (Vec<u32>, Vec<(Option<SymbolId>, u32)>);

fn layout(fdes: &[Fde<'_>]) -> Result<Layout, Error> {
    // Personalities with the lowest order that uses each; few per object.
    let mut personalities: Vec<(SymbolId, u32)> = Vec::new();
    for fde in fdes {
        if let Some(p) = fde.personality {
            match personalities.iter_mut().find(|(q, _)| *q == p) {
                Some((_, first)) => *first = (*first).min(fde.order),
                None => personalities.push((p, fde.order)),
            }
        } else if fde.lsda.is_some() {
            return Err(Error::LsdaWithoutPersonality { order: fde.order });
        }
    }
    personalities.sort_unstable_by_key(|&(_, first)| first);
    if let Some(w) = personalities.windows(2).find(|w| w[0].1 == w[1].1) {
        return Err(Error::DuplicateOrder { order: w[0].1 });
    }
    let rank = |fde: &Fde<'_>| match fde.personality {
        None => 0,
        Some(p) => 1 + personalities.iter().position(|&(q, _)| q == p).unwrap_or(0),
    };
    // A counting sort by CIE, stable, then by `order` within each CIE
    // (already sorted when the input is in module order).
    let mut counts = vec![0u32; personalities.len() + 1];
    for fde in fdes {
        counts[rank(fde)] += 1;
    }
    let mut cies: Vec<(Option<SymbolId>, u32)> = Vec::with_capacity(counts.len());
    let mut starts = Vec::with_capacity(counts.len());
    let mut next = 0u32;
    for (i, &count) in counts.iter().enumerate() {
        starts.push(next);
        next += count;
        cies.push((i.checked_sub(1).map(|i| personalities[i].0), count));
    }
    let mut sorted = vec![0u32; fdes.len()];
    for (i, fde) in fdes.iter().enumerate() {
        let slot = &mut starts[rank(fde)];
        sorted[*slot as usize] = i as u32;
        *slot += 1;
    }
    let mut begin = 0usize;
    for &(_, count) in &cies {
        let group = &mut sorted[begin..begin + count as usize];
        if !group.is_sorted_by_key(|&i| fdes[i as usize].order) {
            group.sort_unstable_by_key(|&i| fdes[i as usize].order);
        }
        if let Some(w) =
            group.windows(2).find(|w| fdes[w[0] as usize].order == fdes[w[1] as usize].order)
        {
            return Err(Error::DuplicateOrder { order: fdes[w[0] as usize].order });
        }
        begin += count as usize;
    }
    cies.retain(|&(_, count)| count > 0);
    Ok((sorted, cies))
}

/// Appends a CIE (`spec/eh.md` §2).
fn write_cie(
    out: &mut Vec<u8>,
    relocs: &mut Vec<PendingReloc>,
    target: &CfiTarget,
    personality: Option<SymbolId>,
) {
    let start = out.len();
    out.extend_from_slice(&[0; 4]); // length
    out.extend_from_slice(&[0; 4]); // CIE id
    out.push(1); // version
    out.extend_from_slice(if personality.is_some() { b"zPLR\0" } else { b"zR\0" });
    write_uleb(out, u64::from(target.code_align));
    write_sleb(out, i64::from(target.data_align));
    out.push(target.return_address);
    if let Some(symbol) = personality {
        let size = usize::from(target.pointer_size);
        write_uleb(out, 3 + size as u64);
        out.push(target.personality_encoding);
        relocs.push(PendingReloc { offset: out.len() as u32, pointer: true, symbol, addend: 0 });
        out.resize(out.len() + size, 0);
        out.push(target.lsda_encoding);
        out.push(target.fde_encoding);
    } else {
        write_uleb(out, 1);
        out.push(target.fde_encoding);
    }
    for &op in target.initial {
        // The table's initial instructions always factor.
        let _ = write_op(out, target, op);
    }
    pad(out, 4);
    patch_length(out, start);
}

/// Writes `.eh_frame` for `fdes` into `obj`, with its relocations, and
/// returns the section; `None`, and no section, when there are no FDEs
/// (`spec/eh.md`).
///
/// The output depends only on the set of FDEs, not on the order of the
/// slice (§5). On an error `obj` is unchanged.
pub fn write_eh_frame(obj: &mut Object, fdes: &[Fde<'_>]) -> Result<Option<SectionId>, Error> {
    if fdes.is_empty() {
        return Ok(None);
    }
    let target = for_arch(obj.arch());
    let (sorted, cies) = layout(fdes)?;
    let rows: usize = fdes.iter().map(|f| f.rows.len()).sum();
    let with_lsda = fdes.iter().filter(|f| f.lsda.is_some()).count();
    let mut out = Vec::with_capacity(cies.len() * 40 + fdes.len() * 28 + rows * 5);
    let mut relocs = Vec::with_capacity(cies.len() + fdes.len() + with_lsda);
    let mut next = sorted.iter();
    let mut last_start = 0;
    for &(personality, count) in &cies {
        let cie = out.len();
        write_cie(&mut out, &mut relocs, target, personality);
        for _ in 0..count {
            let Some(&i) = next.next() else { unreachable!("layout counts every FDE") };
            let fde = &fdes[i as usize];
            let start = out.len();
            let pointer = u32::try_from(start + 4 - cie).map_err(|_| Error::TooLarge)?;
            relocs.push(PendingReloc {
                offset: (start + 8) as u32,
                pointer: false,
                symbol: fde.start,
                addend: fde.start_addend,
            });
            let mut stage = Staging::new();
            stage.bytes(&[0; 4]); // length, patched below
            stage.bytes(&pointer.to_le_bytes());
            stage.bytes(&[0; 4]); // initial location, relocated
            stage.bytes(&fde.size.to_le_bytes());
            if personality.is_some() {
                let size = usize::from(target.pointer_size);
                stage.byte(size as u8); // augmentation length (ULEB128)
                if let Some(lsda) = fde.lsda {
                    relocs.push(PendingReloc {
                        offset: (start + stage.len) as u32,
                        pointer: true,
                        symbol: lsda.symbol,
                        addend: lsda.addend,
                    });
                }
                stage.bytes(&[0; 8][..size]);
            } else {
                stage.byte(0);
            }
            write_rows(&mut out, &mut stage, target, fde)?;
            // DW_CFA_nop to a multiple of 4.
            stage.make_room(&mut out, 3);
            let end = out.len() + stage.len;
            stage.bytes(&[0; 3][..end.next_multiple_of(4) - end]);
            stage.flush(&mut out);
            last_start = start;
            patch_length(&mut out, start);
        }
    }
    // The last FDE is padded to the section's alignment (as `llc` does),
    // which keeps the section's size a multiple of it.
    pad(&mut out, usize::from(target.section_align));
    patch_length(&mut out, last_start);
    if u32::try_from(out.len()).is_err() {
        return Err(Error::TooLarge);
    }
    let kind =
        SectionKind::Elf { sh_type: target.section_type, sh_flags: SHF_ALLOC, sh_entsize: 0 };
    let section = obj.add_section(b".eh_frame", kind, u64::from(target.section_align))?;
    obj.append_owned(section, out);
    obj.reserve(0, 0, relocs.len(), 0);
    for r in relocs {
        let kind = if r.pointer { target.pointer_reloc } else { target.pc_reloc };
        obj.add_relocation(section, u64::from(r.offset), kind, r.symbol, r.addend);
    }
    Ok(Some(section))
}

/// Adds the indirection slot `llc` makes for a personality routine
/// (`spec/eh.md` §2.2): an 8-byte word holding the routine's address, in
/// its own writable section `.data.DW.ref.<name>` of a COMDAT group whose
/// signature is the slot's symbol `DW.ref.<name>`, a weak hidden object.
/// Every object that uses the personality has one, and the linker keeps
/// one. Returns the slot's symbol, the [`Fde::personality`] of the
/// functions that use the routine.
pub fn personality_ref(obj: &mut Object, personality: SymbolId) -> Result<SymbolId, Error> {
    let target = for_arch(obj.arch());
    let routine = obj.string(obj.symbol(personality).name).to_vec();
    let mut name = Vec::with_capacity(routine.len() + 7);
    name.extend_from_slice(b"DW.ref.");
    name.extend_from_slice(&routine);
    let mut section_name = Vec::with_capacity(name.len() + 6);
    section_name.extend_from_slice(b".data.");
    section_name.extend_from_slice(&name);
    let section = obj.add_section(&section_name, SectionKind::Data, 8)?;
    let name = obj.add_string(&name)?;
    let slot = obj.add_symbol(
        Symbol::defined(name, SymbolKind::Object, section, 0, 8)
            .with_binding(Binding::Weak)
            .with_visibility(Visibility::Hidden),
    );
    let group = obj.add_group(slot, GroupKind::Comdat);
    obj.add_group_member(group, section)?;
    obj.append_zeros(section, 8);
    obj.add_relocation(section, 0, target.abs_reloc, personality, 0);
    Ok(slot)
}
