//! Location lists of variables described by value records (WP-162,
//! `spec/dwarf.md` §6.23).
//!
//! The code generator gives each range of such a variable as a
//! [`LocalRow`] with [`LocalPlace::Value`]: code offsets `start..end` of
//! the function and where the variable is there ([`ValueLoc`]), with the
//! record's expression. The rows of one instance (the variable and the
//! `inlinedAt` of the record's location) make one DIE, which the first
//! row places, as `llc` orders variables by their first record; its
//! `DW_AT_location` is a location list in `.debug_loclists`:
//! `DW_LLE_base_addressx` of the function, one `DW_LLE_offset_pair` per
//! located range (adjacent equal ones merged), `DW_LLE_end_of_list`. An
//! instance without a located range has no `DW_AT_location`
//! (`<optimized out>`).
//!
//! The attribute is a `DW_FORM_sec_offset` into `.debug_loclists` where
//! `llc` writes `DW_FORM_loclistx` and a unit `DW_AT_loclists_base`, as
//! block range lists are written (§6.7); a split unit's is made a
//! `DW_FORM_loclistx` of `.debug_loclists.dwo`'s offset table, as its
//! range lists are (`split.rs`).
//!
//! Each entry's location, from the value's place `P` and the record's
//! expression `E` (§6.23):
//! - a home `fbreg o` of a value `n` bytes wide: `E` empty, the home
//!   itself (`DW_OP_fbreg o`); `E` ending in `DW_OP_stack_value`, the value
//!   loaded (`DW_OP_fbreg o, DW_OP_deref` or `DW_OP_deref_size n`) then
//!   `E`; `E` ending in `DW_OP_deref`, a memory location: the value loaded
//!   then `E` without that last `DW_OP_deref` (`llc`'s `DwarfExpression`
//!   makes the last `DW_OP_deref` of a value's expression implicit);
//! - a constant: `DW_OP_consts` (a variable of a signed base type, the
//!   bits sign-extended) or `DW_OP_lit`/`DW_OP_constu`, then `E`, then
//!   `DW_OP_stack_value` unless `E` ends in it.
//!
//! A value narrower than 64 bits is computed on in the low bits only
//! (the loaded value is zero-extended): an `E` with an operation whose
//! low bits depend on the high ones (a division, a right shift, a
//! comparison) has no entry there, and a memory location needs an
//! address-wide value.

use std::collections::HashMap;

use hod_ir::{DiId, DiNode, DiOp, ExprId, LocId};

use crate::Error;
use crate::abbrev::{code, fam};
use crate::die::{Kind, Reloc, Sec, Target, UnitPlan, Writer};
use crate::func::{LocalData, LocalPlace, LocalRow, LocalWhat};
use crate::leb::{write_sleb, write_uleb};

/// A fragment (`DW_OP_LLVM_fragment`) in a value record's expression: a
/// variable described in pieces.
pub const DEBUG_DBG_VALUE_FRAGMENT: &str = "debug:dbg-value-fragment";

/// A value record's expression hod does not write: an LLVM operation
/// (`DW_OP_LLVM_*`, a conversion, an entry value), `DW_OP_stack_value`
/// before the end, or a memory location that does not end in
/// `DW_OP_deref`.
pub const DEBUG_DBG_VALUE_EXPR: &str = "debug:dbg-value-expr";

/// Where a variable described by value records is over one range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueLoc {
    /// The value is stored `offset` bytes from the frame base, `size`
    /// bytes wide (1, 2, 4 or 8).
    Frame {
        /// The offset from the frame base.
        offset: i64,
        /// The value's width in bytes.
        size: u8,
    },
    /// The value is this constant: `width` bits (1 to 64), zero-extended.
    Const {
        /// The bits.
        bits: u64,
        /// The width in bits.
        width: u8,
    },
    /// Nowhere the code generator can prove: no entry.
    None,
}

/// One located range of an instance, in the plan: code offsets of its
/// function.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Range {
    start: u32,
    end: u32,
    at: ValueLoc,
    expr: ExprId,
}

/// The rows of a function's instances (part of `FnScratch`).
#[derive(Clone, Debug, Default)]
pub(crate) struct Groups {
    /// `(variable, inlinedAt)` to the row that places the instance.
    heads: HashMap<(u32, u32), u32>,
    /// `(head row, row)`, sorted once the rows are read.
    members: Vec<(u32, u32)>,
}

impl Groups {
    pub(crate) fn clear(&mut self) {
        self.heads.clear();
        self.members.clear();
    }

    /// Adds value row `row` of instance `(node, ia)`: whether it is the
    /// instance's first, which places its DIE.
    pub(crate) fn join(&mut self, node: DiId, ia: LocId, row: u32) -> bool {
        let head = *self.heads.entry((node.raw(), ia.raw())).or_insert(row);
        self.members.push((head, row));
        head == row
    }

    /// Once every row is read.
    pub(crate) fn seal(&mut self) {
        self.members.sort_unstable();
    }
}

/// Checks a value record's expression (§6.23): the stable name of why it
/// cannot be written. The operations are those of a declared location
/// (`func::check_expr`), plus `DW_OP_stack_value` last.
pub(crate) fn check_value_expr(
    debug: &hod_ir::DebugInfo,
    expr: ExprId,
) -> Result<(), &'static str> {
    let mut last = None;
    for op in debug.ops(expr) {
        if last == Some(DiOp::StackValue) {
            return Err(DEBUG_DBG_VALUE_EXPR);
        }
        match op {
            DiOp::Fragment(..) => return Err(DEBUG_DBG_VALUE_FRAGMENT),
            DiOp::Convert(..)
            | DiOp::TagOffset(_)
            | DiOp::EntryValue(_)
            | DiOp::ImplicitPointer
            | DiOp::Arg(_)
            | DiOp::ExtractBitsSext(..)
            | DiOp::ExtractBitsZext(..)
            | DiOp::PushObjectAddress
            | DiOp::Xderef
            | DiOp::Other(_) => return Err(DEBUG_DBG_VALUE_EXPR),
            DiOp::DerefSize(n) if n > 0xff => return Err(DEBUG_DBG_VALUE_EXPR),
            _ => {}
        }
        last = Some(op);
    }
    match last {
        None | Some(DiOp::StackValue | DiOp::Deref) => Ok(()),
        Some(_) => Err(DEBUG_DBG_VALUE_EXPR),
    }
}

/// Writes one checked operation.
fn write_op(op: DiOp, out: &mut Vec<u8>) {
    out.push(op.code() as u8);
    match op {
        DiOp::Constu(v) | DiOp::PlusUconst(v) => write_uleb(out, v),
        DiOp::Consts(v) => write_sleb(out, v as i64),
        DiOp::DerefSize(n) => out.push(n as u8),
        _ => {}
    }
}

/// Whether an operation's result's low bits depend only on its operands'
/// low bits, so that it computes a narrow value right on its
/// zero-extension.
const fn low_bits_op(op: DiOp) -> bool {
    matches!(
        op,
        DiOp::PlusUconst(_)
            | DiOp::Constu(_)
            | DiOp::Consts(_)
            | DiOp::Lit0
            | DiOp::Plus
            | DiOp::Minus
            | DiOp::Mul
            | DiOp::And
            | DiOp::Or
            | DiOp::Xor
            | DiOp::Not
            | DiOp::Neg
            | DiOp::Shl
            | DiOp::Dup
            | DiOp::Swap
            | DiOp::Over
            | DiOp::Rot
            | DiOp::StackValue
    )
}

/// What an expression is: empty, a value (ends in `DW_OP_stack_value`),
/// or a memory location (ends in `DW_OP_deref`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    Empty,
    Value,
    Memory,
}

fn shape(debug: &hod_ir::DebugInfo, expr: ExprId) -> Shape {
    match debug.ops(expr).last() {
        None => Shape::Empty,
        Some(DiOp::StackValue) => Shape::Value,
        Some(_) => Shape::Memory,
    }
}

/// Whether a range at `at` with expression `expr` gets an entry.
fn located(debug: &hod_ir::DebugInfo, at: ValueLoc, expr: ExprId) -> bool {
    let narrow = |bits: u32| bits < 64 && !debug.ops(expr).all(low_bits_op);
    match (at, shape(debug, expr)) {
        (ValueLoc::None, _) | (ValueLoc::Const { .. }, Shape::Memory) => false,
        (ValueLoc::Frame { .. }, Shape::Empty) => true,
        (ValueLoc::Frame { size, .. }, Shape::Value) => !narrow(u32::from(size) * 8),
        (ValueLoc::Frame { size, .. }, Shape::Memory) => size == 8,
        (ValueLoc::Const { width, .. }, Shape::Empty | Shape::Value) => !narrow(u32::from(width)),
    }
}

impl UnitPlan<'_> {
    /// The location list of the instance whose DIE row `row` places
    /// (`LocalPlace::Value`): its located ranges into `value_ranges`.
    pub(crate) fn value_list(&mut self, vars: &[LocalRow], groups: &Groups, row: u32) -> LocalWhat {
        let first = self.value_ranges.len() as u32;
        let at = groups.members.partition_point(|&(h, _)| h < row);
        for &(head, r) in &groups.members[at..] {
            if head != row {
                break;
            }
            let Some(&LocalRow { expr, place: LocalPlace::Value { start, end, at }, .. }) =
                vars.get(r as usize)
            else {
                continue;
            };
            if end <= start || !located(self.debug, at, expr) {
                continue;
            }
            let n = self.value_ranges.len();
            if n > first as usize
                && let Some(last) = self.value_ranges.last_mut()
                && last.end == start
                && last.at == at
                && last.expr == expr
            {
                last.end = end;
            } else {
                self.value_ranges.push(Range { start, end, at, expr });
            }
        }
        LocalWhat::List { first, count: self.value_ranges.len() as u32 - first }
    }
}

/// The family of a variable DIE whose location is a list: `base`'s with
/// `DW_AT_location` a `DW_FORM_sec_offset` (same option bits).
pub(crate) const fn list_family(base: usize) -> usize {
    match base {
        fam::FORMAL_PARAMETER => fam::FORMAL_PARAMETER_LIST,
        fam::VARIABLE_UNNAMED => fam::VARIABLE_UNNAMED_LIST,
        fam::VARIABLE_ALIGNED => fam::VARIABLE_ALIGNED_LIST,
        _ => fam::VARIABLE_LIST,
    }
}

/// A variable's list, when `what` is one with entries.
pub(crate) const fn list_of(what: LocalWhat) -> Option<(u32, u32)> {
    match what {
        LocalWhat::List { first, count } if count > 0 => Some((first, count)),
        _ => None,
    }
}

/// `DW_LLE_*` (DWARF 5 §7.7.3).
const DW_LLE_END_OF_LIST: u8 = 0x00;
const DW_LLE_BASE_ADDRESSX: u8 = 0x01;
const DW_LLE_OFFSET_PAIR: u8 = 0x04;
const DW_OP_FBREG: u8 = 0x91;
const DW_OP_DEREF: u8 = 0x06;
const DW_OP_DEREF_SIZE: u8 = 0x94;
const DW_OP_LIT0: u8 = 0x30;
const DW_OP_CONSTU: u8 = 0x10;
const DW_OP_CONSTS: u8 = 0x11;
const DW_OP_STACK_VALUE: u8 = 0x9f;
/// The header of a unit's contribution (DWARF 5 §7.29).
pub(crate) const LISTS_HEADER: usize = 12;
/// No contribution for the current unit yet.
pub(crate) const NO_LISTS: usize = usize::MAX;

impl Writer<'_> {
    /// `DW_AT_location` of a variable whose ranges are `first..first +
    /// count` of the plan's (§6.23): a `DW_FORM_sec_offset` to its list,
    /// appended to the unit's `.debug_loclists` contribution.
    pub(crate) fn value_location(
        &mut self,
        plan: &UnitPlan<'_>,
        l: &LocalData,
        node: DiId,
        (first, count): (u32, u32),
    ) {
        if self.loclists_unit == NO_LISTS {
            self.loclists_unit = self.loclists.len();
            self.loclists.extend_from_slice(&[0; 4]);
            self.loclists.extend_from_slice(&5u16.to_le_bytes());
            self.loclists.extend_from_slice(&[8, 0]);
            self.loclists.extend_from_slice(&0u32.to_le_bytes()); // offset_entry_count
        }
        let at = self.loclists.len();
        let signed = signed_type(plan, node);
        let base = self.fn_addr.get(l.func as usize).copied().unwrap_or(0);
        self.loclists.push(DW_LLE_BASE_ADDRESSX);
        write_uleb(&mut self.loclists, u64::from(base));
        let ranges = &plan.value_ranges[first as usize..(first + count) as usize];
        for r in ranges {
            self.loclists.push(DW_LLE_OFFSET_PAIR);
            write_uleb(&mut self.loclists, u64::from(r.start));
            write_uleb(&mut self.loclists, u64::from(r.end));
            self.expr.clear();
            entry_ops(plan.debug, r, signed, &mut self.expr);
            write_uleb(&mut self.loclists, self.expr.len() as u64);
            self.loclists.extend_from_slice(&self.expr);
        }
        self.loclists.push(DW_LLE_END_OF_LIST);
        self.relocs.push(Reloc {
            sec: Sec::Info,
            offset: self.info.len(),
            target: Target::Loclists,
            addend: at as i64,
        });
        self.info.extend_from_slice(&[0; 4]);
    }

    /// The unit's contribution's `unit_length`, once its DIEs are
    /// written; the next unit starts its own.
    pub(crate) fn finish_loclists(&mut self) -> Result<(), Error> {
        if self.loclists_unit != NO_LISTS {
            let start = self.loclists_unit;
            let length =
                u32::try_from(self.loclists.len() - start - 4).map_err(|_| Error::TooLarge)?;
            self.loclists[start..start + 4].copy_from_slice(&length.to_le_bytes());
        }
        self.loclists_unit = NO_LISTS;
        Ok(())
    }

    /// A concrete variable of an abstract one whose location is the list
    /// `list`, or which has none (`DW_AT_abstract_origin` alone).
    pub(crate) fn list_of_abstract(
        &mut self,
        plan: &UnitPlan<'_>,
        l: &LocalData,
        node: DiId,
        kind: Kind,
        origin: u32,
    ) {
        let family = if kind == Kind::LocalParam {
            fam::FORMAL_PARAMETER_OF_ABSTRACT_LIST
        } else {
            fam::VARIABLE_OF_ABSTRACT_LIST
        };
        let list = list_of(l.what);
        self.abbrev(code(family, u32::from(list.is_some()), false));
        if let Some(list) = list {
            self.value_location(plan, l, node, list);
        }
        self.ref4(origin);
    }
}

/// Whether the variable's type is a signed base type (`llc`'s
/// `DwarfDebug::emitDebugLocValue` reads the type itself, not through a
/// typedef).
fn signed_type(plan: &UnitPlan<'_>, node: DiId) -> bool {
    let DiNode::LocalVariable(v) = plan.debug.node(node) else { return false };
    let Some(ty) = v.ty else { return false };
    matches!(plan.debug.node(ty), DiNode::BasicType(b) if matches!(b.encoding, 0x05 | 0x06))
}

/// The location of one entry (module doc).
fn entry_ops(debug: &hod_ir::DebugInfo, r: &Range, signed: bool, out: &mut Vec<u8>) {
    let shape = shape(debug, r.expr);
    match r.at {
        ValueLoc::Frame { offset, size } => {
            out.push(DW_OP_FBREG);
            write_sleb(out, offset);
            if shape == Shape::Empty {
                return;
            }
            if size == 8 {
                out.push(DW_OP_DEREF);
            } else {
                out.push(DW_OP_DEREF_SIZE);
                out.push(size);
            }
        }
        ValueLoc::Const { bits, width } => {
            if signed {
                let shift = 64 - u32::from(width);
                out.push(DW_OP_CONSTS);
                write_sleb(out, ((bits << shift) as i64) >> shift);
            } else if bits < 32 {
                out.push(DW_OP_LIT0 + bits as u8);
            } else {
                out.push(DW_OP_CONSTU);
                write_uleb(out, bits);
            }
        }
        ValueLoc::None => return,
    }
    let n = debug.ops(r.expr).count();
    for (k, op) in debug.ops(r.expr).enumerate() {
        if shape == Shape::Memory && k + 1 == n {
            break; // the last `DW_OP_deref` is the memory location's own
        }
        write_op(op, out);
    }
    if matches!(r.at, ValueLoc::Const { .. }) && shape == Shape::Empty {
        out.push(DW_OP_STACK_VALUE);
    }
}

/// `.debug_loclists.dwo` (§6.15, §6.23): the lists the writer put after
/// the contribution's header in `w.loclists`, reached through an offset
/// table (one entry per list, in DIE order), and each `DW_AT_location`
/// of `lists` (its offset in `.debug_info.dwo`, the list's offset in
/// `w.loclists`) made the list's `DW_FORM_loclistx` index.
pub(crate) fn dwo_loclists(w: &mut Writer<'_>, lists: &[(usize, i64)]) -> Result<Vec<u8>, Error> {
    if lists.is_empty() {
        return Ok(Vec::new());
    }
    let body = w.loclists.get(LISTS_HEADER..).unwrap_or(&[]);
    let count = u32::try_from(lists.len()).map_err(|_| Error::TooLarge)?;
    let mut out = Vec::with_capacity(LISTS_HEADER + 4 * lists.len() + body.len());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&5u16.to_le_bytes());
    out.extend_from_slice(&[8, 0]);
    out.extend_from_slice(&count.to_le_bytes());
    for (k, &(at, list)) in lists.iter().enumerate() {
        let within = usize::try_from(list)
            .ok()
            .and_then(|l| l.checked_sub(LISTS_HEADER))
            .filter(|&l| l < body.len())
            .ok_or(Error::TooLarge)?;
        let offset = u32::try_from(4 * lists.len() + within).map_err(|_| Error::TooLarge)?;
        out.extend_from_slice(&offset.to_le_bytes());
        w.info[at..at + 4].copy_from_slice(&padded_uleb4(k as u32)?);
    }
    out.extend_from_slice(body);
    let length = u32::try_from(out.len() - 4).map_err(|_| Error::TooLarge)?;
    out[..4].copy_from_slice(&length.to_le_bytes());
    Ok(out)
}

/// `n` as a ULEB128 of exactly four bytes: `DW_FORM_loclistx` in a
/// `DW_FORM_sec_offset`'s place.
fn padded_uleb4(n: u32) -> Result<[u8; 4], Error> {
    if n >= 1 << 28 {
        return Err(Error::TooLarge);
    }
    Ok([
        (n & 0x7f) as u8 | 0x80,
        ((n >> 7) & 0x7f) as u8 | 0x80,
        ((n >> 14) & 0x7f) as u8 | 0x80,
        (n >> 21) as u8,
    ])
}
