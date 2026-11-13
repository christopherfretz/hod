//! Control flow beyond `br` (`spec/codegen.md` §6, §5.6.3): `condbr`,
//! `switch`, `indirectbr`, and the `blockaddress` constants it consumes
//! ([`block_address`]). WP-124 (X-control); tests `tests/control.rs`.
//!
//! `phi`s need nothing from this module: the driver writes every
//! successor's incoming slots before it lowers a terminator (§4.5),
//! through the scratch registers and flag-keeping glue only, so a
//! terminator reads its own operands only and needs no per-edge code.
//!
//! # `condbr`
//!
//! On a materialised `i1`: `test r8, 1` (only bit 0 is defined, §2.2)
//! and `jne`/`je`, the branch to the block emitted next omitted. On a
//! deferred integer `icmp` (§4.6): the compare's operands are fetched
//! first, then `cmp`/`test`, then the `jcc` at once, so nothing at all
//! runs between the flags' writer and their reader (the flags rule,
//! §5.6). The fused compare here is this module's own ([`fused_icmp`]);
//! WP-130 may unify it with `int`'s.
//!
//! # `switch`
//!
//! `hod_cg::switch`'s plan (§7.3): the condition zero-extended in its
//! register, the canonical cases searched by [`search_step`] (a node
//! compares with its middle case, `je` to its block, `ja` to the upper
//! half; a run of at most [`LINEAR_CASES`] is a chain of `cmp` · `je`).
//! A condition of at most 32 bits is compared with 32-bit `cmp` and an
//! immediate; a wider one with 64-bit `cmp`, the case value through
//! `r11` when it is no sign-extended 32-bit immediate. A condition of
//! class `Mem` (`i65` and wider) is a linear chain over the cases in the
//! IR's order, limb by limb ([`switch`]), as `hod_a64`'s `tail` does.
//! No jump table, so a function's code has no relocation against itself.

use std::cell::RefCell;

use hod_cg::switch::{Step, canonical_cases, case_mask, constant_target, search_step};
use hod_cg::{CgResult, Class, FnCx};
use hod_ir::{
    BlockId, Const, ConstId, Inst, InstKind, IntPredicate, Switch, SymId, Value, ValueKind,
};
use hod_mc_x64 as mc;
use hod_mc_x64::{Cond, Mem, Reg, Size};

use crate::common::{
    R10, R11, REL32, branch, branch_cond, deferred_first, frame_inst, gp, mov_imm, put,
    unsupported, value_ty,
};
use crate::target::X64;
use crate::term;

pub use hod_cg::switch::LINEAR_CASES;

/// The skeleton's refusal of the group: not refused any more; kept for
/// the coverage log's history.
pub const CONTROL: &str = "x64:todo:control";

/// A `blockaddress` of a block of another function used in code
/// (`classify` rejects it before code generation; `a64:blockaddress` on
/// AArch64).
pub const BLOCKADDRESS: &str = "x64:blockaddress";

/// A `condbr` on a deferred `fcmp`: refused until WP-127, which lowers
/// it through `float::cond_br`. The name stays for the record.
pub const FUSED_FCMP: &str = "x64:todo:condbr-fcmp";

/// `br i1 %cond, label %if_true, label %if_false`.
///
/// A condition that is a deferred integer compare (§4.6) is fused: its
/// operands are placed, `cmp`/`test` sets the flags, and `jcc` branches
/// on the predicate's condition, inverted to the false block when the
/// true block is emitted next. Otherwise `test r8, 1` and `jne`/`je`; a
/// constant condition (`undef` and `poison` are false, §4.7) or two
/// equal targets are a `br`.
///
/// # Errors
/// What `float::cond_br` reports for a deferred `fcmp`; what emitting a
/// deferred condition or the branches' fixups report.
pub fn cond_br<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    cond: Value,
    if_true: BlockId,
    if_false: BlockId,
) -> CgResult<()> {
    if if_true != if_false
        && let Some(compare) = cx.deferred(cond)
    {
        match compare.kind() {
            InstKind::ICmp { pred, lhs, rhs } if cx.class_of(lhs) == Class::Gpr => {
                deferred_first(cx, inst, Some(cond))?;
                // WP-149 (`spec/dwarf.md` §5.8, §5.9): the `cmp` is the
                // compare's code, at its location, as in `llc`'s x86-64
                // program (which keeps the compare's atom apart from the
                // branch's).
                let outer = cx.line_loc();
                cx.line_switch(cx.row_loc_of(compare));
                let c = fused_icmp(cx, pred, lhs, rhs)?;
                cx.line_switch(outer);
                return branch_on(cx, c, if_true, if_false);
            }
            InstKind::FCmp { .. } => {
                return crate::float::cond_br(cx, inst, compare, cond, if_true, if_false);
            }
            _ => {}
        }
    }
    deferred_first(cx, inst, None)?;
    if if_true == if_false {
        return term::br(cx, if_true);
    }
    if let Some(value) = const_int(cx, cond) {
        return term::br(cx, if value & 1 != 0 { if_true } else { if_false });
    }
    let r = gp(cx.use_reg(cond)?);
    put(cx, mc::test_ri(Size::B, r, 1));
    branch_on(cx, Cond::Ne, if_true, if_false)
}

/// The branches of a `condbr` once the flags are set: `jcc` to
/// `if_true` when `c` holds, else to `if_false`; a block emitted next is
/// not branched to.
fn branch_on(cx: &mut FnCx<'_, X64>, c: Cond, if_true: BlockId, if_false: BlockId) -> CgResult<()> {
    if cx.falls_through(if_true) {
        return branch_cond(cx, c.invert(), cx.block_label(if_false));
    }
    branch_cond(cx, c, cx.block_label(if_true))?;
    term::br(cx, if_false)
}

/// The integer value of a constant operand of class `Gpr`: an integer,
/// an `inttoptr` of a number, and `null`, zero, `undef`, `poison` as 0
/// (§4.7). `None` for anything else (a value, an address).
fn const_int(cx: &FnCx<'_, X64>, value: Value) -> Option<u64> {
    let ValueKind::Const(id) = value.kind() else { return None };
    match cx.module.consts.get(id) {
        Const::Int { value, .. } | Const::AddrOfInt { value } => Some(value),
        Const::Null(_) | Const::Zero(_) | Const::Undef(_) | Const::Poison(_) => Some(0),
        _ => None,
    }
}

/// The condition that holds after `cmp lhs, rhs` exactly when `lhs pred
/// rhs`.
pub fn cond_of(pred: IntPredicate) -> Cond {
    match pred {
        IntPredicate::Eq => Cond::E,
        IntPredicate::Ne => Cond::Ne,
        IntPredicate::Ugt => Cond::A,
        IntPredicate::Uge => Cond::Ae,
        IntPredicate::Ult => Cond::B,
        IntPredicate::Ule => Cond::Be,
        IntPredicate::Sgt => Cond::G,
        IntPredicate::Sge => Cond::Ge,
        IntPredicate::Slt => Cond::L,
        IntPredicate::Sle => Cond::Le,
    }
}

/// The predicate of the same compare with its operands swapped.
pub fn swapped(pred: IntPredicate) -> IntPredicate {
    match pred {
        IntPredicate::Eq | IntPredicate::Ne => pred,
        IntPredicate::Ugt => IntPredicate::Ult,
        IntPredicate::Uge => IntPredicate::Ule,
        IntPredicate::Ult => IntPredicate::Ugt,
        IntPredicate::Ule => IntPredicate::Uge,
        IntPredicate::Sgt => IntPredicate::Slt,
        IntPredicate::Sge => IntPredicate::Sle,
        IntPredicate::Slt => IntPredicate::Sgt,
        IntPredicate::Sle => IntPredicate::Sge,
    }
}

/// The operand size of a compare of `bits`-bit integers and the left
/// shift that brings their defined bits to the top of it: 8, 16, 32 and
/// 64 bits are compared at their own size; an odd width below 32 is
/// shifted to the top of 32 bits, above 32 to the top of 64. Shifting
/// both operands left keeps equality and both orders.
pub fn compare_shape(bits: u32) -> (Size, u8) {
    match bits {
        8 => (Size::B, 0),
        16 => (Size::W, 0),
        32 => (Size::D, 0),
        64 => (Size::Q, 0),
        b if b < 32 => (Size::D, (32 - b) as u8),
        b => (Size::Q, (64 - b.min(64)) as u8),
    }
}

/// The mask of the low bits of an operand of `size`.
fn size_mask(size: Size) -> u64 {
    match size {
        Size::B => 0xff,
        Size::W => 0xffff,
        Size::D => 0xffff_ffff,
        Size::Q => u64::MAX,
    }
}

/// Sets the flags as `cmp r, value` at `size` (`value` within the
/// size): `test r, r` for 0, `cmp r, imm` when the immediate form takes
/// it (sign-extended 32 bits at 64), else the value through `r11` (`r`
/// is never `r11`: it is a register of the policy).
pub fn compare_imm(cx: &mut FnCx<'_, X64>, size: Size, r: Reg, value: u64) {
    if value == 0 {
        put(cx, mc::test_rr(size, r, r));
    } else if let Some(inst) = mc::try_cmp_ri(size, r, value as i64) {
        put(cx, inst);
    } else {
        mov_imm(cx, R11, value, 64);
        put(cx, mc::cmp_rr(size, r, R11));
    }
}

/// Fuses the deferred integer compare `lhs pred rhs` (class `Gpr`, a
/// pointer included) into the flags and returns the condition that
/// holds exactly when the `icmp` gives `true`. Call it after
/// [`deferred_first`] with the compare as the fused operand.
///
/// The operands are placed first (a constant on the left is swapped to
/// the right), shifted in place to the top of their compare size for an
/// odd width ([`compare_shape`]), then `cmp`/`test`: nothing is emitted
/// after it, so the caller's `jcc` follows the flags' writer directly.
/// Writes the operands' registers (which `use_reg` gives this
/// instruction) and `r11`. A candidate to share with `int` (WP-130).
///
/// # Errors
/// What placing the operands reports.
pub fn fused_icmp(
    cx: &mut FnCx<'_, X64>,
    pred: IntPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<Cond> {
    for operand in [lhs, rhs] {
        if cx.deferred(operand).is_some() {
            cx.use_reg(operand)?;
        }
    }
    let bits = cx.types().int_bits(value_ty(cx, lhs)).unwrap_or(64);
    let (mut pred, mut lhs, mut rhs) = (pred, lhs, rhs);
    if const_int(cx, lhs).is_some() && const_int(cx, rhs).is_none() {
        (pred, lhs, rhs) = (swapped(pred), rhs, lhs);
    }
    let (size, shift) = compare_shape(bits);
    let a = gp(cx.use_reg(lhs)?);
    if shift > 0 {
        put(cx, mc::shl_ri(size, a, shift));
    }
    if let Some(value) = const_int(cx, rhs) {
        compare_imm(cx, size, a, (value << shift) & size_mask(size));
    } else {
        let b = if rhs == lhs { a } else { gp(cx.use_reg(rhs)?) };
        if shift > 0 && b != a {
            put(cx, mc::shl_ri(size, b, shift));
        }
        put(cx, mc::cmp_rr(size, a, b));
    }
    Ok(cond_of(pred))
}

thread_local! {
    /// The sorted cases of the `switch` being lowered: one buffer per
    /// thread, so a warm worker lowers a `switch` without allocating.
    static CASES: RefCell<Vec<(u64, BlockId)>> = const { RefCell::new(Vec::new()) };
}

/// `switch <ty> %cond, label %default [cases]`: the search of
/// `hod_cg::switch` over the zero-extended condition (module
/// documentation); a constant condition is a `br` to its target
/// (`constant_target`); a condition of class `Mem` is compared limb by
/// limb.
///
/// # Errors
/// What emitting a deferred condition or the branches' fixups report.
pub fn switch<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, switch: &Switch<'a>) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    if cx.class_of(switch.cond) != Class::Gpr {
        return switch_wide(cx, switch);
    }
    let bits = cx.types().int_bits(value_ty(cx, switch.cond)).unwrap_or(64);
    let mask = case_mask(bits);
    let case_value = |cx: &FnCx<'a, X64>, id| match cx.module.consts.get(id) {
        Const::Int { value, .. } => value & mask,
        _ => 0,
    };
    if let Some(value) = const_int(cx, switch.cond) {
        let cases = switch.cases().map(|(case, block)| (case_value(cx, case), block));
        return term::br(cx, constant_target(cases, value & mask, switch.default));
    }
    let mut cases = CASES.with(|cell| std::mem::take(&mut *cell.borrow_mut()));
    canonical_cases(
        &mut cases,
        switch.cases().map(|(case, block)| (case_value(cx, case), block)),
        switch.default,
    );
    let result = if cases.is_empty() {
        term::br(cx, switch.default)
    } else {
        lower_search(cx, switch.cond, bits, &cases, switch.default)
    };
    CASES.with(|cell| *cell.borrow_mut() = cases);
    result
}

/// The condition zero-extended in its register (which `use_reg` gives
/// this instruction), then the search.
fn lower_search(
    cx: &mut FnCx<'_, X64>,
    cond: Value,
    bits: u32,
    cases: &[(u64, BlockId)],
    default: BlockId,
) -> CgResult<()> {
    let r = gp(cx.use_reg(cond)?);
    let size = if bits <= 32 { Size::D } else { Size::Q };
    match bits {
        8 => put(cx, mc::movzx_rr(Size::D, r, Size::B, r)),
        16 => put(cx, mc::movzx_rr(Size::D, r, Size::W, r)),
        32 | 64 => {}
        b if b < 32 => put(cx, mc::and_ri(Size::D, r, case_mask(b) as i64)),
        b => {
            let s = (64 - b) as u8;
            put(cx, mc::shl_ri(Size::Q, r, s));
            put(cx, mc::shr_ri(Size::Q, r, s));
        }
    }
    search(cx, r, size, cases, default, true)
}

/// The search over `cases` (sorted, distinct, none to `default`) of the
/// zero-extended `r`; `last` when nothing is emitted after it, so that
/// its final branch may fall through.
fn search(
    cx: &mut FnCx<'_, X64>,
    r: Reg,
    size: Size,
    cases: &[(u64, BlockId)],
    default: BlockId,
    last: bool,
) -> CgResult<()> {
    let Step::Node { lower, mid: (value, block), upper } = search_step(cases) else {
        let Some((&(final_value, final_block), init)) = cases.split_last() else {
            return cx.bug("a search over no case");
        };
        for &(value, block) in init {
            compare_imm(cx, size, r, value);
            branch_cond(cx, Cond::E, cx.block_label(block))?;
        }
        compare_imm(cx, size, r, final_value);
        if last && cx.falls_through(final_block) {
            return branch_cond(cx, Cond::Ne, cx.block_label(default));
        }
        branch_cond(cx, Cond::E, cx.block_label(final_block))?;
        return if last { term::br(cx, default) } else { branch(cx, cx.block_label(default)) };
    };
    compare_imm(cx, size, r, value);
    branch_cond(cx, Cond::E, cx.block_label(block))?;
    let upper_label = cx.new_label();
    branch_cond(cx, Cond::A, upper_label)?;
    search(cx, r, size, lower, default, false)?;
    cx.bind(upper_label)?;
    search(cx, r, size, upper, default, last)
}

/// Limb `k` of the integer constant `id`; `None` if it is no integer.
fn const_limb(cx: &FnCx<'_, X64>, id: ConstId, k: u32) -> Option<u64> {
    match cx.module.consts.get(id) {
        Const::Int { value, .. } => Some(if k == 0 { value } else { 0 }),
        Const::WideInt { words, .. } => Some(words.get(k as usize).copied().unwrap_or(0)),
        Const::Zero(_) | Const::Undef(_) | Const::Poison(_) => Some(0),
        _ => None,
    }
}

/// `switch` on an integer of class `Mem` (`deferred_first` is done), as
/// `hod_a64`'s `tail::switch_wide`: for each case not to the default, in
/// the IR's order (of two equal cases the first wins, as in the
/// interpreter), the limbs loaded into `r10` and compared (`jne` to the
/// next case, `je` to its block after the top limb, whose bits above
/// the width are shifted out first); then the default. A constant
/// condition is a `br`.
///
/// # Errors
/// What the condition and the branches' fixups report.
fn switch_wide<'a>(cx: &mut FnCx<'a, X64>, switch: &Switch<'a>) -> CgResult<()> {
    let Some(bits) = cx.types().int_bits(value_ty(cx, switch.cond)) else {
        return cx.bug("a switch on something that is no integer");
    };
    let n = bits.div_ceil(64);
    let top = bits - 64 * (n - 1);
    let top_shift = (64 - top) as u8;
    let top_mask = if top >= 64 { u64::MAX } else { (1u64 << top) - 1 };
    let limb = |cx: &FnCx<'a, X64>, id, k| {
        const_limb(cx, id, k).map(|w| if k + 1 == n { w & top_mask } else { w })
    };
    if let ValueKind::Const(id) = switch.cond.kind()
        && limb(cx, id, 0).is_some()
    {
        let same = |cx: &FnCx<'a, X64>, case| (0..n).all(|k| limb(cx, case, k) == limb(cx, id, k));
        let target =
            switch.cases().find(|&(case, _)| same(cx, case)).map_or(switch.default, |(_, b)| b);
        return term::br(cx, target);
    }
    let c = cx.use_mem(switch.cond)?;
    for (case, block) in switch.cases() {
        if block == switch.default {
            continue;
        }
        let next = cx.new_label();
        for k in 0..n {
            let Some(mut word) = limb(cx, case, k) else {
                return cx.bug("a switch case that is no integer");
            };
            frame_inst(cx, c.offset_by(8 * k), |m| mc::mov_rm(Size::Q, R10, m));
            if k + 1 == n && top_shift > 0 {
                put(cx, mc::shl_ri(Size::Q, R10, top_shift));
                word <<= top_shift;
            }
            compare_imm(cx, Size::Q, R10, word);
            if k + 1 < n {
                branch_cond(cx, Cond::Ne, next)?;
            } else {
                branch_cond(cx, Cond::E, cx.block_label(block))?;
            }
        }
        cx.bind(next)?;
    }
    term::br(cx, switch.default)
}

/// `indirectbr ptr %addr, [labels]`: `jmp r`; a `blockaddress` constant
/// of this function is a direct `jmp` to its block.
///
/// # Errors
/// [`BLOCKADDRESS`] for a block of another function; what placing the
/// address reports.
pub fn indirect_br<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, addr: Value) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    if let ValueKind::Const(id) = addr.kind()
        && let Const::BlockAddr { func, block } = cx.module.consts.get(id)
    {
        if func != cx.body.symbol() || block.index() >= cx.body.block_count() {
            return unsupported(BLOCKADDRESS);
        }
        return term::br(cx, block);
    }
    let r = gp(cx.use_reg(addr)?);
    put(cx, mc::jmp_r(r));
    Ok(())
}

/// Puts the address of block `block` of function `func` in `rd`: `lea
/// rd, [rip + label]`, its `disp32` a [`REL32`] fixup to the block's
/// label (the field ends the instruction, so its end is `rip`); flags
/// kept. An unreachable block's label is bound to the function's shared
/// trap (§6.1). `common::materialize_gpr_const` routes `blockaddress`
/// constants here.
///
/// # Errors
/// [`BLOCKADDRESS`] for a block of another function (`classify` rejects
/// it first); what the fixup reports.
pub fn block_address(cx: &mut FnCx<'_, X64>, rd: Reg, func: SymId, block: BlockId) -> CgResult<()> {
    if func != cx.body.symbol() || block.index() >= cx.body.block_count() {
        return unsupported(BLOCKADDRESS);
    }
    let inst = mc::lea(Size::Q, rd, Mem::rip(0));
    let Some(site) = inst.site() else {
        return cx.bug("a RIP-relative lea without a displacement field");
    };
    let at = cx.offset();
    put(cx, inst);
    cx.fixup(at + site.offset as u32, REL32, cx.block_label(block))
}
