//! Control flow beyond `br` (`spec/codegen.md` §6): `condbr`, `switch`,
//! `indirectbr`, and the `blockaddress` constants it consumes
//! ([`crate::common::block_address`]).
//!
//! **Owner: WP-037.** Tests: `tests/control.rs`. The dispatch in
//! `target.rs` is final. Branches use `crate::common::branch*`, which know
//! far mode.
//!
//! `phi`s need nothing from this module: the driver writes every
//! successor's incoming slots before it lowers a terminator (§4.5), so a
//! terminator reads its own operands only and needs no per-edge code
//! (no edge splitting, no parallel moves).
//!
//! # `switch`
//!
//! The condition is zero-extended in its register (§2.2: the bits above
//! its width are unspecified), the cases that do not go to the default
//! are put in increasing unsigned order, and the sorted list is lowered
//! as a binary search (§6.2): a node compares with its middle case
//! (`b.eq` to its block, `b.hi` to the upper half), a run of at most
//! [`LINEAR_CASES`] cases is a chain of `cmp` · `b.eq`. Code is linear in
//! the cases (at most 5 instructions per case plus one `b` per leaf),
//! a run takes about `log2(n) + LINEAR_CASES` compares, and compile time
//! is linear for cases that come sorted (a frontend's usual order) and
//! `n log n` otherwise. No jump table, so a function's code has no
//! relocation against itself (§6.2).

use std::cell::RefCell;

use hod_cg::{CgResult, Class, FnCx};
use hod_ir::{BlockId, Const, Inst, Switch, Value, ValueKind};
use hod_mc_a64 as mc;
use hod_mc_a64::{Cond, Shift, Size};

use crate::common::{
    X16, block_address, branch, branch_bit, branch_cond, deferred_first, mov_imm, put, unsupported,
    value_ty, xr,
};
use crate::target::A64;
use crate::term;

/// `br i1 %cond, label %t, label %f`: not refused any more; kept for the
/// coverage log's history.
pub const COND_BR: &str = "a64:condbr";
/// `switch`: not refused any more.
pub const SWITCH: &str = "a64:switch";
/// `indirectbr`: not refused any more.
pub const INDIRECT_BR: &str = "a64:indirectbr";

/// A `switch` on an integer wider than a register (`Mem` class:
/// `classify` rejects it, `inst:switch` being supported for `int` only).
pub const SWITCH_WIDE: &str = "a64:switch-wide";

/// A run of at most this many sorted cases is a linear chain; a longer
/// one is split at its middle case.
pub const LINEAR_CASES: usize = 4;

/// `br i1 %cond, label %if_true, label %if_false`: `tbnz` of bit 0 (§2.2:
/// only bit 0 of an `i1` is defined), the branch omitted for a target
/// emitted next; a constant condition or two equal targets are a `br`.
///
/// # Errors
/// What emitting a deferred condition or the branches' fixups report.
pub fn cond_br<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    cond: Value,
    if_true: BlockId,
    if_false: BlockId,
) -> CgResult<()> {
    // FUSION POINT (the integration package after WP-035 and WP-039):
    // when `cx.deferred(cond)` is an integer or floating-point compare,
    // set the flags with `int::flags` / `float::flags` and branch with
    // `branch_cond` on the condition they return, passing `Some(cond)` to
    // `deferred_first` so that the compare is not emitted. Until then a
    // deferred compare is asked for here and its `i1` tested below.
    deferred_first(cx, inst, None)?;
    if if_true == if_false {
        return term::br(cx, if_true);
    }
    if let ValueKind::Const(id) = cond.kind() {
        // `undef` and `poison` are zero (§4.7).
        let taken = match cx.module.consts.get(id) {
            Const::Int { value, .. } => value & 1 != 0,
            _ => false,
        };
        return term::br(cx, if taken { if_true } else { if_false });
    }
    let rt = xr(cx.use_reg(cond)?);
    if cx.falls_through(if_true) {
        return branch_bit(cx, rt, 0, false, cx.block_label(if_false));
    }
    branch_bit(cx, rt, 0, true, cx.block_label(if_true))?;
    term::br(cx, if_false)
}

thread_local! {
    /// The sorted cases of the `switch` being lowered: one buffer per
    /// thread, so a warm worker lowers a `switch` without allocating.
    static CASES: RefCell<Vec<(u64, BlockId)>> = const { RefCell::new(Vec::new()) };
}

/// `switch <ty> %cond, label %default [cases]`: a binary search over the
/// cases in unsigned order (module documentation).
///
/// # Errors
/// What emitting a deferred condition or the branches' fixups report.
pub fn switch<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, switch: &Switch<'a>) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    if cx.class_of(switch.cond) != Class::Gpr {
        return unsupported(SWITCH_WIDE);
    }
    let bits = cx.types().int_bits(value_ty(cx, switch.cond)).unwrap_or(64);
    let mask = if bits >= 64 { u64::MAX } else { (1u64 << bits) - 1 };
    let case_value = |cx: &FnCx<'a, A64>, id| match cx.module.consts.get(id) {
        Const::Int { value, .. } => value & mask,
        _ => 0,
    };
    if let ValueKind::Const(id) = switch.cond.kind() {
        let value = case_value(cx, id);
        let target = switch
            .cases()
            .find(|&(case, _)| case_value(cx, case) == value)
            .map_or(switch.default, |(_, block)| block);
        return term::br(cx, target);
    }
    let mut cases = CASES.with(|cell| std::mem::take(&mut *cell.borrow_mut()));
    cases.clear();
    cases.extend(
        switch
            .cases()
            .filter(|&(_, block)| block != switch.default)
            .map(|(case, block)| (case_value(cx, case), block)),
    );
    if !cases.is_sorted_by_key(|&(value, _)| value) {
        // Stable, so that of two equal cases (invalid IR) the first wins,
        // as in the interpreter.
        cases.sort_by_key(|&(value, _)| value);
    }
    cases.dedup_by_key(|&mut (value, _)| value);
    let result = if cases.is_empty() {
        term::br(cx, switch.default)
    } else {
        let rt = xr(cx.use_reg(switch.cond)?);
        if bits < 64 {
            // The register is this instruction's to overwrite (`use_reg`).
            put(cx, mc::ubfx(Size::X, rt.zr(), rt.zr(), 0, bits));
        }
        search(cx, rt, &cases, switch.default, true)
    };
    CASES.with(|cell| *cell.borrow_mut() = cases);
    result
}

/// The search over `cases` (sorted, distinct, none to `default`) of the
/// zero-extended `rt`; `last` when nothing is emitted after it, so that
/// its final branch may fall through.
fn search(
    cx: &mut FnCx<'_, A64>,
    rt: mc::Reg,
    cases: &[(u64, BlockId)],
    default: BlockId,
    last: bool,
) -> CgResult<()> {
    if cases.len() <= LINEAR_CASES {
        let (&(final_value, final_block), init) = cases.split_last().expect("a case");
        for &(value, block) in init {
            compare(cx, rt, value);
            branch_cond(cx, Cond::Eq, cx.block_label(block))?;
        }
        compare(cx, rt, final_value);
        if last && cx.falls_through(final_block) {
            return branch_cond(cx, Cond::Ne, cx.block_label(default));
        }
        branch_cond(cx, Cond::Eq, cx.block_label(final_block))?;
        return if last { term::br(cx, default) } else { branch(cx, cx.block_label(default)) };
    }
    let mid = cases.len() / 2;
    let (value, block) = cases[mid];
    compare(cx, rt, value);
    branch_cond(cx, Cond::Eq, cx.block_label(block))?;
    let upper = cx.new_label();
    branch_cond(cx, Cond::Hi, upper)?;
    search(cx, rt, &cases[..mid], default, false)?;
    cx.bind(upper)?;
    search(cx, rt, &cases[mid + 1..], default, last)
}

/// Sets the flags as `cmp rt, #value`: by the immediate form, by `cmn`
/// of the negation (the same `Z` and `C`), or through `x16`.
fn compare(cx: &mut FnCx<'_, A64>, rt: mc::Reg, value: u64) {
    if let Some(word) = mc::try_cmp_imm(Size::X, rt.sp(), value) {
        put(cx, word);
    } else if let Some(word) = mc::try_cmn_imm(Size::X, rt.sp(), value.wrapping_neg()) {
        put(cx, word);
    } else {
        mov_imm(cx, Size::X, X16, value);
        put(cx, mc::cmp_shifted(Size::X, rt.zr(), X16.zr(), Shift::Lsl, 0));
    }
}

/// `indirectbr ptr %addr, [labels]`: `br` to the address; a
/// `blockaddress` constant is formed in `x16` first.
///
/// # Errors
/// What forming the address reports.
pub fn indirect_br<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, addr: Value) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    let rn = match addr.kind() {
        ValueKind::Const(id) => match cx.module.consts.get(id) {
            Const::BlockAddr { func, block } => {
                block_address(cx, X16, func, block)?;
                X16
            }
            _ => xr(cx.use_reg(addr)?),
        },
        _ => xr(cx.use_reg(addr)?),
    };
    put(cx, mc::br(rn.zr()));
    Ok(())
}
