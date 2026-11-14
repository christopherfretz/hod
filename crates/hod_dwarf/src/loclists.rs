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
//!   `DW_OP_stack_value` unless `E` ends in it;
//! - a declared value's home `fbreg o` holding the variable's address
//!   (WP-221, §6.30; `E` is the declaration's, checked as a slot's):
//!   `DW_OP_fbreg o, DW_OP_deref`, then all of `E`.
//!
//! A value narrower than 64 bits is computed on in the low bits only
//! (the loaded value is zero-extended): an `E` with an operation whose
//! low bits depend on the high ones (a division, a right shift, a
//! comparison) has no entry there, and a memory location needs an
//! address-wide value.
//!
//! Fragments and conversions (§6.31): a variable with fragment records
//! gets entries composed of pieces (`compose`): per stretch of code, the
//! pieces that hold there sorted by offset, each its location then
//! `DW_OP_piece`, a gap an empty `DW_OP_piece`. `DW_OP_LLVM_convert` is
//! `DW_OP_convert` of one of the unit's base type DIEs (its first
//! children, in first use), or `llc`'s legacy extension in a split unit;
//! a constant's leading conversions are folded into it.
//!
//! Argument lists (§6.32): a `ValueLoc::Args` row is followed by its
//! values' `LocalPlace::ValueArg` rows; its entry is the expression with
//! each `DW_OP_LLVM_arg` replaced by that value's push (`arglist_width`
//! follows the stack to keep narrow values under low-bit operations).
//! Constants are sign-extended and unsigned ones written by `llc`'s
//! `emitConstu` (`write_constu`), the expression's own `DW_OP_constu`
//! too.

use std::collections::HashMap;

use hod_ir::{DiId, DiNode, DiOp, ExprId, LocId};

use crate::Error;
use crate::abbrev::{code, fam};
use crate::die::{Kind, Reloc, Sec, Target, UnitPlan, Writer};
use crate::func::{LocalData, LocalPlace, LocalRow, LocalWhat};
use crate::leb::{write_sleb, write_uleb};

/// A fragment (`DW_OP_LLVM_fragment`) in a value record's expression
/// that is not byte aligned, or of size 0 (§6.31).
pub const DEBUG_DBG_VALUE_FRAGMENT: &str = "debug:dbg-value-fragment";

/// A value record's expression hod does not write: an LLVM operation
/// (`DW_OP_LLVM_*` other than a fragment and a conversion to an integer
/// of 1 to 64 bits, an entry value), `DW_OP_stack_value` before the end,
/// a memory location that does not end in `DW_OP_deref`, or one with a
/// conversion.
pub const DEBUG_DBG_VALUE_EXPR: &str = "debug:dbg-value-expr";

/// A `#dbg_value` with a `DIArgList` (or `DW_OP_LLVM_arg`) whose
/// expression hod does not write (§6.32): one that is not a computed
/// value (does not end in `DW_OP_stack_value`), or names a value the list
/// does not have.
pub const DEBUG_DBG_VALUE_ARGLIST: &str = "debug:dbg-value-arglist";

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
    /// The variable is at the address stored `offset` bytes from the
    /// frame base, and the expression is a declaration's (WP-221,
    /// §6.30): `DW_OP_fbreg offset, DW_OP_deref`, then the expression.
    Address {
        /// The offset from the frame base.
        offset: i64,
    },
    /// The record's values are a `DIArgList` (WP-230, §6.32): the row is
    /// followed by `count` rows [`LocalPlace::ValueArg`], the places of
    /// the values `DW_OP_LLVM_arg` names.
    Args {
        /// How many values.
        count: u32,
    },
}

/// One located range of an instance, in the plan: code offsets of its
/// function, and what it holds: `count` pieces from `first` in
/// `UnitPlan::value_pieces` (one, or the variable's fragments sorted by
/// offset, §6.31).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Range {
    start: u32,
    end: u32,
    first: u32,
    count: u32,
}

/// What one record says over a range: where the value is and the
/// record's expression (with its fragment, if any).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Piece {
    at: ValueLoc,
    expr: ExprId,
    /// For [`ValueLoc::Args`]: the first of its values' places in
    /// `UnitPlan::value_args` (WP-230); 0 otherwise.
    args: u32,
}

/// A located row of a variable described in fragments, while its
/// ranges are composed (§6.31).
#[derive(Clone, Copy, Debug)]
struct PieceRow {
    start: u32,
    end: u32,
    row: u32,
    /// The fragment's bits, `(offset, size)`; `(0, u64::MAX)` for the
    /// whole variable.
    bits: (u64, u64),
    piece: Piece,
}

/// The plan's scratch for composing pieces (§6.31).
#[derive(Clone, Debug, Default)]
pub(crate) struct PieceScratch {
    rows: Vec<PieceRow>,
    bounds: Vec<u32>,
    active: Vec<PieceRow>,
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

/// `DW_ATE_signed`, `DW_ATE_unsigned`: the encodings of a
/// `DW_OP_LLVM_convert`.
const DW_ATE_SIGNED: u8 = 0x05;
const DW_ATE_UNSIGNED: u8 = 0x07;
const DW_ATE_BOOLEAN: u8 = 0x02;

/// Checks a value record's expression (§6.23, §6.31): the stable name of
/// why it cannot be written. The operations are those of a declared
/// location (`func::check_expr`), plus `DW_OP_stack_value` last, a
/// conversion to a signed or unsigned integer of 1 to 64 bits (not in a
/// memory location), and a byte-aligned fragment after all of them.
pub(crate) fn check_value_expr(
    debug: &hod_ir::DebugInfo,
    expr: ExprId,
) -> Result<(), &'static str> {
    let mut last = None;
    let mut converts = false;
    for op in debug.ops(expr) {
        if let DiOp::Fragment(offset, size) = op {
            // Last (`hod_ir` checks the order, `expr::order_error`).
            if offset % 8 != 0 || size % 8 != 0 || size == 0 {
                return Err(DEBUG_DBG_VALUE_FRAGMENT);
            }
            continue;
        }
        if last == Some(DiOp::StackValue) {
            return Err(DEBUG_DBG_VALUE_EXPR);
        }
        match op {
            DiOp::Convert(bits, encoding)
                if (1..=64).contains(&bits)
                    && (encoding == u64::from(DW_ATE_SIGNED)
                        || encoding == u64::from(DW_ATE_UNSIGNED)) =>
            {
                converts = true;
            }
            DiOp::Convert(..)
            | DiOp::Fragment(..)
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
        None | Some(DiOp::StackValue) => Ok(()),
        Some(DiOp::Deref) if !converts => Ok(()),
        Some(_) => Err(DEBUG_DBG_VALUE_EXPR),
    }
}

/// Checks the expression of a record whose values are a `DIArgList` of
/// `count` (WP-230, §6.32): the operations of [`check_value_expr`] and
/// `DW_OP_LLVM_arg` of an index under `count`, ending in
/// `DW_OP_stack_value` (before a fragment).
pub(crate) fn check_arglist_expr(
    debug: &hod_ir::DebugInfo,
    expr: ExprId,
    count: u32,
) -> Result<(), &'static str> {
    let mut last = None;
    for op in debug.ops(expr) {
        match op {
            DiOp::Fragment(offset, size) => {
                if offset % 8 != 0 || size % 8 != 0 || size == 0 {
                    return Err(DEBUG_DBG_VALUE_FRAGMENT);
                }
                continue;
            }
            _ if last == Some(DiOp::StackValue) => return Err(DEBUG_DBG_VALUE_EXPR),
            DiOp::Arg(k) if k >= u64::from(count) => return Err(DEBUG_DBG_VALUE_ARGLIST),
            DiOp::Convert(bits, encoding)
                if (1..=64).contains(&bits)
                    && (encoding == u64::from(DW_ATE_SIGNED)
                        || encoding == u64::from(DW_ATE_UNSIGNED)) => {}
            DiOp::Convert(..)
            | DiOp::TagOffset(_)
            | DiOp::EntryValue(_)
            | DiOp::ImplicitPointer
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
    if last == Some(DiOp::StackValue) { Ok(()) } else { Err(DEBUG_DBG_VALUE_ARGLIST) }
}

/// The name of the base type of a conversion (`llc`'s
/// `createBaseTypeDIEs`: the encoding's name and the bits, e.g.
/// `DW_ATE_signed_32`).
pub(crate) fn base_type_name(bits: u8, encoding: u8) -> &'static [u8] {
    let (start, len) = name_span(bits, encoding == DW_ATE_SIGNED);
    &BASE_TYPE_NAMES[start..start + len]
}

/// Where the name of `(bits, signed)` is in [`BASE_TYPE_NAMES`].
const fn name_span(bits: u8, signed: bool) -> (usize, usize) {
    let mut at = 0;
    let mut k = 0;
    while k < 128 {
        let (b, s) = ((k % 64) as u8 + 1, k < 64);
        let len = name_len(b, s);
        if b == bits && s == signed {
            return (at, len);
        }
        at += len;
        k += 1;
    }
    (0, 0)
}

const fn name_len(bits: u8, signed: bool) -> usize {
    let prefix = if signed { SIGNED.len() } else { UNSIGNED.len() };
    prefix + if bits >= 10 { 2 } else { 1 }
}

const SIGNED: &[u8] = b"DW_ATE_signed_";
const UNSIGNED: &[u8] = b"DW_ATE_unsigned_";
const NAMES_LEN: usize = {
    let mut n = 0;
    let mut b = 1;
    while b <= 64 {
        n += name_len(b, true) + name_len(b, false);
        b += 1;
    }
    n
};

/// `DW_ATE_signed_1` to `DW_ATE_signed_64`, then the unsigned ones,
/// back to back.
static BASE_TYPE_NAMES: [u8; NAMES_LEN] = {
    let mut out = [0u8; NAMES_LEN];
    let mut at = 0;
    let mut k = 0;
    while k < 128 {
        let (bits, signed) = ((k % 64) as u8 + 1, k < 64);
        let prefix = if signed { SIGNED } else { UNSIGNED };
        let mut i = 0;
        while i < prefix.len() {
            out[at] = prefix[i];
            at += 1;
            i += 1;
        }
        if bits >= 10 {
            out[at] = b'0' + bits / 10;
            at += 1;
        }
        out[at] = b'0' + bits % 10;
        at += 1;
        k += 1;
    }
    out
};

/// Writes one checked operation.
fn write_op(op: DiOp, out: &mut Vec<u8>) {
    if let DiOp::Constu(v) = op {
        return write_constu(v, out);
    }
    out.push(op.code() as u8);
    match op {
        DiOp::PlusUconst(v) => write_uleb(out, v),
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

/// What an expression is, its fragment aside: empty, a value (ends in
/// `DW_OP_stack_value`), or a memory location (ends in `DW_OP_deref`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    Empty,
    Value,
    Memory,
}

/// The operations of `expr` without its fragment.
fn body(debug: &hod_ir::DebugInfo, expr: ExprId) -> impl Iterator<Item = DiOp> + Clone + '_ {
    debug.ops(expr).filter(|op| !matches!(op, DiOp::Fragment(..)))
}

/// The fragment of `expr`, `(offset, size)` in bits (§6.31).
fn fragment(debug: &hod_ir::DebugInfo, expr: ExprId) -> Option<(u64, u64)> {
    match debug.ops(expr).last() {
        Some(DiOp::Fragment(offset, size)) => Some((offset, size)),
        _ => None,
    }
}

fn shape(debug: &hod_ir::DebugInfo, expr: ExprId) -> Shape {
    match body(debug, expr).last() {
        None => Shape::Empty,
        Some(DiOp::StackValue) => Shape::Value,
        Some(_) => Shape::Memory,
    }
}

/// A constant after the conversions that lead its expression, as
/// `llc`'s instruction selection folds them (`DIExpression::
/// constantFold`, §6.31): its bits (zero-extended), its width, and how
/// many operations were folded.
fn fold(debug: &hod_ir::DebugInfo, expr: ExprId, bits: u64, width: u8) -> (u64, u8, usize) {
    let (mut v, mut w, mut k) = (bits, width, 0);
    for op in body(debug, expr) {
        let DiOp::Convert(to, encoding) = op else { break };
        let to = to as u8; // 1 to 64: `check_value_expr`
        if to <= w {
            v &= mask(to);
        } else if encoding == u64::from(DW_ATE_SIGNED) && (v >> (w - 1)) & 1 == 1 {
            v |= mask(to) & !mask(w);
        }
        w = to;
        k += 1;
    }
    (v, w, k)
}

/// The low `bits` bits set.
const fn mask(bits: u8) -> u64 {
    if bits >= 64 { u64::MAX } else { (1 << bits) - 1 }
}

/// Whether the operations `ops` (a stack value's, after any folded
/// conversion) compute right on a value `width` bits wide pushed
/// zero-extended, and the width of what they leave: up to the first
/// conversion only operations that keep the low bits when the value is
/// narrow, and a first conversion of at most `width` bits (it reads the
/// low bits); after it the value is typed.
fn computes(ops: impl Iterator<Item = DiOp>, width: u8) -> Option<u8> {
    let mut typed = None;
    for op in ops {
        if let DiOp::Convert(to, _) = op {
            if typed.is_none() && to > u64::from(width) {
                return None;
            }
            typed = Some(to as u8);
        } else if typed.is_none() && width < 64 && !low_bits_op(op) {
            return None;
        }
    }
    Some(typed.unwrap_or(width))
}

/// What one entry of a `DIArgList` expression's stack holds (§6.32).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Held {
    /// A value this many bits wide, pushed zero-extended: only its low
    /// bits are right.
    Narrow(u8),
    /// A value of the stack's generic type, exact.
    Full,
    /// A typed value of this many bits (after `DW_OP_convert`).
    Typed(u8),
}

/// Two operands' result kind: narrow if either is (the narrower), else
/// typed if either is, else exact.
fn join(a: Held, b: Held) -> Held {
    match (a, b) {
        (Held::Narrow(x), Held::Narrow(y)) => Held::Narrow(x.min(y)),
        (Held::Narrow(x), _) | (_, Held::Narrow(x)) => Held::Narrow(x),
        (Held::Typed(x), _) | (_, Held::Typed(x)) => Held::Typed(x),
        _ => Held::Full,
    }
}

/// The places of a `DIArgList` row's values: the rows after row `r`
/// (WP-230, §6.32).
fn arg_rows(vars: &[LocalRow], r: u32, at: ValueLoc) -> &[LocalRow] {
    let ValueLoc::Args { count } = at else { return &[] };
    let first = r as usize + 1;
    vars.get(first..first + count as usize).unwrap_or_default()
}

/// The place of one `DIArgList` value's row.
fn arg_loc(row: &LocalRow) -> ValueLoc {
    match row.place {
        LocalPlace::ValueArg(at) => at,
        _ => ValueLoc::None,
    }
}

/// Whether a `DIArgList` expression (checked, ending in
/// `DW_OP_stack_value`) computes right on its values pushed as hod
/// pushes them, and the width of its result (WP-230, §6.32): every value
/// is a home or a constant; a value under 64 bits is pushed
/// zero-extended, so it only meets operations whose low bits depend on
/// their operands' low bits (as §6.23's `computes`), and a conversion of
/// it reads no more than its bits. The stack is followed entry by entry.
fn arglist_width(ops: impl Iterator<Item = DiOp>, args: &[LocalRow]) -> Option<u8> {
    let mut stack = [Held::Full; 32];
    let mut n = 0usize;
    for op in ops {
        let (pops, push) = match op {
            DiOp::Arg(k) => {
                let held = match arg_loc(args.get(k as usize)?) {
                    ValueLoc::Frame { size, .. } if size < 8 => Held::Narrow(size * 8),
                    ValueLoc::Const { width, .. } if width < 64 => Held::Narrow(width),
                    ValueLoc::Frame { .. } | ValueLoc::Const { .. } => Held::Full,
                    _ => return None,
                };
                (0, held)
            }
            DiOp::Constu(_) | DiOp::Consts(_) | DiOp::Lit0 => (0, Held::Full),
            DiOp::StackValue => continue,
            DiOp::Dup | DiOp::Over => {
                let back = if op == DiOp::Dup { 1 } else { 2 };
                (0, *stack.get(n.checked_sub(back)?)?)
            }
            DiOp::Swap | DiOp::Rot => {
                let k = if op == DiOp::Swap { 2 } else { 3 };
                let top = stack.get_mut(n.checked_sub(k)?..n)?;
                top.rotate_right(1);
                continue;
            }
            DiOp::Convert(to, _) => {
                let x = *stack.get(n.checked_sub(1)?)?;
                if matches!(x, Held::Narrow(w) if to > u64::from(w)) {
                    return None;
                }
                (1, Held::Typed(to as u8))
            }
            DiOp::PlusUconst(_) | DiOp::Not | DiOp::Neg => (1, *stack.get(n.checked_sub(1)?)?),
            DiOp::Abs | DiOp::Deref | DiOp::DerefSize(_) => {
                let x = *stack.get(n.checked_sub(1)?)?;
                if matches!(x, Held::Narrow(_)) {
                    return None;
                }
                let held = match op {
                    DiOp::Abs => x,
                    DiOp::DerefSize(k) if k < 8 => Held::Narrow(k as u8 * 8),
                    _ => Held::Full,
                };
                (1, held)
            }
            _ => {
                // Binary.
                let b = *stack.get(n.checked_sub(1)?)?;
                let a = *stack.get(n.checked_sub(2)?)?;
                let narrow = matches!(a, Held::Narrow(_)) || matches!(b, Held::Narrow(_));
                if narrow && !low_bits_op(op) {
                    return None;
                }
                (2, join(a, b))
            }
        };
        n -= pops;
        *stack.get_mut(n)? = push;
        n += 1;
    }
    match *stack.get(n.checked_sub(1)?)? {
        Held::Narrow(w) | Held::Typed(w) => Some(w),
        Held::Full => Some(64),
    }
}

/// Whether a range at `at` with expression `expr` gets an entry (or,
/// for a fragment, a piece): §6.23, and for a fragment of `size` bits a
/// value at least that wide (the piece is its low bits). `args`: a
/// `DIArgList`'s value rows (§6.32).
fn located(debug: &hod_ir::DebugInfo, at: ValueLoc, expr: ExprId, args: &[LocalRow]) -> bool {
    let fits = |width: u8| fragment(debug, expr).is_none_or(|(_, size)| size <= u64::from(width));
    match (at, shape(debug, expr)) {
        (ValueLoc::Args { .. }, Shape::Value) => {
            arglist_width(body(debug, expr), args).is_some_and(|w| w > 0 && fits(w))
        }
        (ValueLoc::Args { .. } | ValueLoc::None, _) | (ValueLoc::Const { .. }, Shape::Memory) => {
            false
        }
        (ValueLoc::Address { .. }, _) => true,
        (ValueLoc::Frame { size, .. }, Shape::Empty) => fits(size * 8),
        (ValueLoc::Frame { size, .. }, Shape::Value) => {
            computes(body(debug, expr), size * 8).is_some_and(fits)
        }
        (ValueLoc::Frame { size, .. }, Shape::Memory) => size == 8,
        (ValueLoc::Const { bits, width }, Shape::Empty | Shape::Value) => {
            let (_, width, k) = fold(debug, expr, bits, width);
            computes(body(debug, expr).skip(k), width).is_some_and(fits)
        }
    }
}

/// Whether two pieces' bits overlap (`(0, u64::MAX)`: the whole
/// variable).
fn overlap(a: (u64, u64), b: (u64, u64)) -> bool {
    a.0 < b.0.saturating_add(b.1) && b.0 < a.0.saturating_add(a.1)
}

impl UnitPlan<'_> {
    /// The location list of the instance whose DIE row `row` places
    /// (`LocalPlace::Value`): its located ranges into `value_ranges`.
    pub(crate) fn value_list(&mut self, vars: &[LocalRow], groups: &Groups, row: u32) -> LocalWhat {
        let debug = self.debug;
        let first = self.value_ranges.len() as u32;
        let at = groups.members.partition_point(|&(h, _)| h < row);
        let members = groups.members[at..].iter().take_while(|&&(h, _)| h == row);
        let pieces = members.clone().any(|&(_, r)| {
            vars.get(r as usize).is_some_and(|v| {
                matches!(v.place, LocalPlace::Value { .. }) && fragment(debug, v.expr).is_some()
            })
        });
        if pieces {
            self.compose(vars, members.map(|&(_, r)| r), first);
        } else {
            for &(_, r) in members {
                let Some(&LocalRow { expr, place: LocalPlace::Value { start, end, at }, .. }) =
                    vars.get(r as usize)
                else {
                    continue;
                };
                if end <= start || !located(debug, at, expr, arg_rows(vars, r, at)) {
                    continue;
                }
                let piece = self.piece(vars, r, at, expr);
                if self.value_ranges.len() > first as usize
                    && let Some(last) = self.value_ranges.last_mut()
                    && last.end == start
                    && self.value_pieces.get(last.first as usize) == Some(&piece)
                {
                    last.end = end;
                } else {
                    self.push_range(start, end, &[piece]);
                }
            }
        }
        LocalWhat::List { first, count: self.value_ranges.len() as u32 - first }
    }

    /// The ranges of a variable described in fragments (§6.31): over
    /// each stretch of code between the bounds of its rows, the pieces
    /// that hold there, sorted by offset (`llc`'s `DebugLocEntry`); a
    /// later row replaces the earlier ones it overlaps. Adjacent equal
    /// entries are merged; a stretch without a piece has no entry.
    fn compose(&mut self, vars: &[LocalRow], rows: impl Iterator<Item = u32>, first: u32) {
        let debug = self.debug;
        let mut s = std::mem::take(&mut self.piece_scratch);
        s.rows.clear();
        s.bounds.clear();
        s.active.clear();
        for r in rows {
            let Some(&LocalRow { expr, place: LocalPlace::Value { start, end, at }, .. }) =
                vars.get(r as usize)
            else {
                continue;
            };
            if end <= start || !located(debug, at, expr, arg_rows(vars, r, at)) {
                continue;
            }
            let bits = fragment(debug, expr).unwrap_or((0, u64::MAX));
            let piece = self.piece(vars, r, at, expr);
            s.rows.push(PieceRow { start, end, row: r, bits, piece });
            s.bounds.extend([start, end]);
        }
        s.rows.sort_by_key(|p| (p.start, p.row));
        s.bounds.sort_unstable();
        s.bounds.dedup();
        let mut next = 0;
        let mut pieces = Vec::new();
        for w in 1..s.bounds.len() {
            let (a, b) = (s.bounds[w - 1], s.bounds[w]);
            s.active.retain(|p| p.end > a);
            while let Some(&p) = s.rows.get(next) {
                if p.start > a {
                    break;
                }
                next += 1;
                s.active.retain(|q| !overlap(q.bits, p.bits));
                if p.end > a {
                    s.active.push(p);
                }
            }
            if s.active.is_empty() {
                continue;
            }
            s.active.sort_by_key(|p| p.bits.0);
            pieces.clear();
            pieces.extend(s.active.iter().map(|p| p.piece));
            if self.value_ranges.len() > first as usize
                && let Some(last) = self.value_ranges.last_mut()
                && last.end == a
                && self
                    .value_pieces
                    .get(last.first as usize..(last.first + last.count) as usize)
                    .is_some_and(|held| held == pieces.as_slice())
            {
                last.end = b;
            } else {
                self.push_range(a, b, &pieces);
            }
        }
        self.piece_scratch = s;
    }

    /// The piece of row `r` (located): a `DIArgList`'s value places join
    /// `value_args`, or reuse the last ones when they are the same
    /// (WP-230), so that equal entries compare equal.
    fn piece(&mut self, vars: &[LocalRow], r: u32, at: ValueLoc, expr: ExprId) -> Piece {
        let rows = arg_rows(vars, r, at);
        if rows.is_empty() {
            return Piece { at, expr, args: 0 };
        }
        let n = rows.len();
        let len = self.value_args.len();
        let same = len >= n
            && self.value_args[len - n..].iter().zip(rows).all(|(&a, row)| a == arg_loc(row));
        if !same {
            self.value_args.extend(rows.iter().map(arg_loc));
        }
        Piece { at, expr, args: (self.value_args.len() - n) as u32 }
    }

    /// A new range holding `pieces`; the base types their conversions
    /// name join the unit's, in first use (§6.31).
    fn push_range(&mut self, start: u32, end: u32, pieces: &[Piece]) {
        let first = self.value_pieces.len() as u32;
        self.value_pieces.extend_from_slice(pieces);
        self.value_ranges.push(Range { start, end, first, count: pieces.len() as u32 });
        for p in pieces {
            let skip = match p.at {
                ValueLoc::Const { bits, width } => fold(self.debug, p.expr, bits, width).2,
                _ => 0,
            };
            for op in body(self.debug, p.expr).skip(skip) {
                if let DiOp::Convert(bits, encoding) = op {
                    let t = (bits as u8, encoding as u8);
                    if !self.base_types.contains(&t) {
                        self.base_types.push(t);
                    }
                }
            }
        }
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
const DW_OP_PIECE: u8 = 0x93;
const DW_OP_CONVERT: u8 = 0xa8;
const DW_OP_DUP: u8 = 0x12;
const DW_OP_SHR: u8 = 0x25;
const DW_OP_NOT: u8 = 0x20;
const DW_OP_MUL: u8 = 0x1e;
const DW_OP_SHL: u8 = 0x24;
const DW_OP_OR: u8 = 0x21;
const DW_OP_MINUS: u8 = 0x1c;
const DW_OP_AND: u8 = 0x1a;
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
        let ty = type_encoding(plan, node);
        let cv =
            Converts { legacy: self.split, types: &plan.base_types, dies: &self.base_type_dies };
        let base = self.fn_addr.get(l.func as usize).copied().unwrap_or(0);
        self.loclists.push(DW_LLE_BASE_ADDRESSX);
        write_uleb(&mut self.loclists, u64::from(base));
        let ranges = &plan.value_ranges[first as usize..(first + count) as usize];
        for r in ranges {
            self.loclists.push(DW_LLE_OFFSET_PAIR);
            write_uleb(&mut self.loclists, u64::from(r.start));
            write_uleb(&mut self.loclists, u64::from(r.end));
            self.expr.clear();
            let pieces = &plan.value_pieces[r.first as usize..(r.first + r.count) as usize];
            let mut done = 0; // the bits the pieces so far cover
            for p in pieces {
                let bits = fragment(plan.debug, p.expr);
                if let Some((offset, _)) = bits
                    && offset > done
                {
                    self.expr.push(DW_OP_PIECE);
                    write_uleb(&mut self.expr, (offset - done) / 8);
                }
                entry_ops(plan.debug, p, ty, &cv, &plan.value_args, &mut self.expr);
                if let Some((offset, size)) = bits {
                    self.expr.push(DW_OP_PIECE);
                    write_uleb(&mut self.expr, size / 8);
                    done = offset + size;
                }
            }
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

/// The encoding of the variable's type when it is a base type (`llc`'s
/// `DwarfDebug::emitDebugLocValue` reads the type itself, not through a
/// typedef).
fn type_encoding(plan: &UnitPlan<'_>, node: DiId) -> Option<u8> {
    let DiNode::LocalVariable(v) = plan.debug.node(node) else { return None };
    match plan.debug.node(v.ty?) {
        DiNode::BasicType(b) => Some(b.encoding as u8),
        _ => None,
    }
}

/// How a unit writes `DW_OP_LLVM_convert` (§6.31): `DW_OP_convert` and
/// the base type DIE at `dies[k]` for `types[k]`, or, in a split unit,
/// `llc`'s legacy operations (`DwarfDebug::useOpConvert` is false for
/// gdb with split DWARF).
struct Converts<'w> {
    legacy: bool,
    types: &'w [(u8, u8)],
    dies: &'w [u32],
}

/// Pushes the value in a home `offset` bytes from the frame base, `size`
/// bytes wide (zero-extended).
fn push_home(offset: i64, size: u8, out: &mut Vec<u8>) {
    out.push(DW_OP_FBREG);
    write_sleb(out, offset);
    if size == 8 {
        out.push(DW_OP_DEREF);
    } else {
        out.push(DW_OP_DEREF_SIZE);
        out.push(size);
    }
}

/// Pushes a constant `width` bits wide for a variable whose type has
/// encoding `ty` (§6.23, §6.31, §6.32). `llc` keeps a constant as its
/// sign extension to 64 bits (`getSExtValue`), and writes it signed for
/// a signed type and unsigned (`emitConstu`) for any other: an `i32 -1`
/// of an `unsigned long` is all ones (WP-230 measured 19 and 22).
fn push_const(mut bits: u64, width: u8, ty: Option<u8>, out: &mut Vec<u8>) {
    // LLVM 22: a boolean's constant is 0 or 1; an `i1` is zero-extended
    // (19 sign-extended it).
    if ty == Some(DW_ATE_BOOLEAN) && bits != 0 {
        bits = 1;
    }
    let shift = if width > 1 { 64 - u32::from(width) } else { 0 };
    let value = ((bits << shift) as i64) >> shift;
    if matches!(ty, Some(0x05 | 0x06)) {
        out.push(DW_OP_CONSTS);
        write_sleb(out, value);
    } else {
        write_constu(value as u64, out);
    }
}

/// `llc`'s `DwarfExpression::emitConstu`, which writes every unsigned
/// constant of a location, the expression's `DW_OP_constu` included:
/// `DW_OP_litN` under 32, `DW_OP_lit0, DW_OP_not` for all ones,
/// `DW_OP_constu` otherwise.
fn write_constu(bits: u64, out: &mut Vec<u8>) {
    if bits < 32 {
        out.push(DW_OP_LIT0 + bits as u8);
    } else if bits == u64::MAX {
        out.extend_from_slice(&[DW_OP_LIT0, DW_OP_NOT]);
    } else {
        out.push(DW_OP_CONSTU);
        write_uleb(out, bits);
    }
}

/// The location of one piece (module doc), without its `DW_OP_piece`.
/// `args`: the plan's `value_args` (§6.32).
fn entry_ops(
    debug: &hod_ir::DebugInfo,
    p: &Piece,
    ty: Option<u8>,
    cv: &Converts<'_>,
    args: &[ValueLoc],
    out: &mut Vec<u8>,
) {
    let shape = shape(debug, p.expr);
    let mut skip = 0;
    match p.at {
        ValueLoc::Frame { offset, size } => {
            if shape == Shape::Empty {
                out.push(DW_OP_FBREG);
                write_sleb(out, offset);
                return;
            }
            push_home(offset, size, out);
        }
        ValueLoc::Const { bits, width } => {
            let (bits, width, k) = fold(debug, p.expr, bits, width);
            skip = k;
            push_const(bits, width, ty, out);
        }
        // WP-230 (§6.32): the values are pushed where `DW_OP_LLVM_arg`
        // names them, in the loop below.
        ValueLoc::Args { .. } => {}
        ValueLoc::None => return,
        ValueLoc::Address { offset } => {
            // WP-221: a declaration's expression, whole (`func::write_ops`).
            out.push(DW_OP_FBREG);
            write_sleb(out, offset);
            out.push(DW_OP_DEREF);
            for op in debug.ops(p.expr) {
                write_op(op, out);
            }
            return;
        }
    }
    let n = body(debug, p.expr).count();
    let mut prev: Option<u8> = None; // the legacy form's pending conversion
    for (k, op) in body(debug, p.expr).enumerate().skip(skip) {
        if shape == Shape::Memory && k + 1 == n {
            break; // the last `DW_OP_deref` is the memory location's own
        }
        match op {
            // WP-230 (§6.32): a `DIArgList` value, as a single record's
            // value is pushed (`located` checked every one has a place).
            DiOp::Arg(i) => match args.get(p.args as usize + i as usize) {
                Some(&ValueLoc::Frame { offset, size }) => push_home(offset, size, out),
                Some(&ValueLoc::Const { bits, width }) => push_const(bits, width, ty, out),
                _ => unreachable!("a located `DIArgList` value has a place (located)"),
            },
            DiOp::Convert(bits, encoding) if cv.legacy => {
                // `DwarfExpression::addExpression`: a widening pair is an
                // extension, anything else waits for the next.
                let (bits, encoding) = (bits as u8, encoding as u8);
                match prev {
                    Some(from) if from < bits => {
                        if encoding == DW_ATE_SIGNED {
                            legacy_sext(from, out);
                        } else {
                            legacy_zext(from, out);
                        }
                        prev = None;
                    }
                    _ => prev = Some(bits),
                }
            }
            DiOp::Convert(bits, encoding) => {
                let t = (bits as u8, encoding as u8);
                let die = cv.types.iter().position(|&u| u == t).and_then(|k| cv.dies.get(k));
                let Some(&die) = die else {
                    unreachable!("a located conversion's base type is the unit's (push_range)")
                };
                out.push(DW_OP_CONVERT);
                // `llc`'s `ULEB128PadSize`: four bytes.
                out.extend_from_slice(&[
                    (die & 0x7f) as u8 | 0x80,
                    ((die >> 7) & 0x7f) as u8 | 0x80,
                    ((die >> 14) & 0x7f) as u8 | 0x80,
                    (die >> 21) as u8,
                ]);
            }
            _ => write_op(op, out),
        }
    }
    if matches!(p.at, ValueLoc::Const { .. }) && shape == Shape::Empty {
        out.push(DW_OP_STACK_VALUE);
    }
}

/// `llc`'s `emitLegacySExt`: `(((X >> (from - 1)) * ~0) << from) | X`.
fn legacy_sext(from: u8, out: &mut Vec<u8>) {
    out.extend_from_slice(&[DW_OP_DUP, DW_OP_CONSTU]);
    write_uleb(out, u64::from(from) - 1);
    out.extend_from_slice(&[DW_OP_SHR, DW_OP_LIT0, DW_OP_NOT, DW_OP_MUL, DW_OP_CONSTU]);
    write_uleb(out, u64::from(from));
    out.extend_from_slice(&[DW_OP_SHL, DW_OP_OR]);
}

/// `llc`'s `emitLegacyZExt`: `X & ((1 << from) - 1)`.
fn legacy_zext(from: u8, out: &mut Vec<u8>) {
    if from / 7 < 5 {
        out.push(DW_OP_CONSTU);
        write_uleb(out, mask(from));
    } else {
        out.extend_from_slice(&[DW_OP_LIT0 + 1, DW_OP_CONSTU]);
        write_uleb(out, u64::from(from));
        out.extend_from_slice(&[DW_OP_SHL, DW_OP_LIT0 + 1, DW_OP_MINUS]);
    }
    out.push(DW_OP_AND);
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
