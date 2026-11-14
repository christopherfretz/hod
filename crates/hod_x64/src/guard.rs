//! Stack protectors on x86-64 (`spec/codegen.md` §5.6.24, WP-183).
//!
//! ```text
//!   | return address, saved rbp, the save set  |
//!   | [pad]                                    |
//!   | guard word          [block + guard]      |  top of the local area
//!   | the guarded block's slots                |
//!   | the rest of the local area               |
//!   | low guard           Local(0)             |  with a dynamic area only
//!   | dynamic allocations                      |
//! ```
//!
//! The guard value is `fs:[0x28]` (glibc's TCB `stack_guard`, what `llc`
//! reads). The entry code (`Target::enter_guard`) is
//! `mov r10, fs:[0x28]` · `lea r11, [rbp + block]` · `mov [r11 + guard],
//! r10` · (`mov [low], r10`) · per slot of the block `lea r10, [r11 + k]`
//! · `mov [pointer], r10`. The block's place is known in `end_function`
//! only: the `lea`'s displacement is written then, before every local
//! access is biased (§5.6). Every exit sequence starts with the check
//! `mov r10, [guard]` · `cmp r10, fs:[0x28]` · `jne fail` (and the same
//! for the low guard): `r10` is free at every exit (`r11` may hold a tail
//! exit's callee). `fail` follows the last exit: the body's CFA rule
//! again, then `call __stack_chk_fail`, which does not return.

use hod_cg::{CfiOp, CgResult, FnCx, FrameRef, Label, RuntimeFn, SymRef};
use hod_mc_x64 as mc;
use hod_mc_x64::{Cond, Mem, Seg, Size};
use hod_obj::X64Reloc;

use crate::common::{INCOMING_BASE, R10, R11, branch_cond, frame_inst, put, put_reloc};
use crate::reg::DWARF_RBP;
use crate::target::X64;

/// Where glibc keeps the guard value: `fs:[0x28]`, the TCB's
/// `stack_guard` (x86-64 psABI; what `llc` reads for Linux).
const GUARD_TLS_OFFSET: i32 = 0x28;

/// The low guard's slot: the start of the local area (§5.1), the first
/// thing a dynamic allocation's overflow reaches.
const LOW_GUARD: FrameRef = FrameRef::Local(0);

/// What the guard remembers between `begin_function`, the entry code and
/// `end_function`.
#[derive(Clone, Copy, Default, Debug)]
pub struct GuardState {
    /// The `disp32` field of the entry code's `lea r11, [rbp + block]`.
    block_field: Option<u32>,
    /// The local-area offset of the guard word, once the block is placed.
    guard: Option<u32>,
    /// The label of the call of `__stack_chk_fail`.
    fail: Option<Label>,
}

fn guard_value() -> Mem {
    Mem::abs(GUARD_TLS_OFFSET).with_seg(Seg::Fs)
}

/// `begin_function`, right after the prologue: the low guard's slot,
/// which must be the local area's first.
///
/// # Errors
/// What allocating the slot reports.
pub fn reserve(cx: &mut FnCx<'_, X64>) -> CgResult<()> {
    cx.state.guard = GuardState::default();
    if !(cx.stack_guard().is_on() && cx.stack_guard().low()) {
        return Ok(());
    }
    if cx.alloc_slot(16, 16)? != LOW_GUARD {
        return cx.bug("the low guard is not at the start of the local area");
    }
    Ok(())
}

/// `Target::enter_guard`: the guard words and the block's pointers.
///
/// # Errors
/// `Internal` only for a plan without a guard.
pub fn enter(cx: &mut FnCx<'_, X64>) -> CgResult<()> {
    if !cx.stack_guard().is_on() {
        return cx.bug("guard entry code for a function without a guard");
    }
    put(cx, mc::mov_rm(Size::Q, R10, guard_value()));
    frame_inst(cx, FrameRef::Local(0), |m| mc::lea(Size::Q, R11, m));
    // `lea r, [rbp + disp32]` ends with its displacement.
    cx.state.guard.block_field = Some(cx.offset() - 4);
    let guard = cx.stack_guard().guard_offset();
    put(cx, mc::mov_mr(Size::Q, Mem::base(R11, guard as i32), R10));
    if cx.stack_guard().low() {
        frame_inst(cx, LOW_GUARD, |m| mc::mov_mr(Size::Q, m, R10));
    }
    for index in 0..cx.stack_guard().pointers().len() {
        let (pointer, offset) = cx.stack_guard().pointers()[index];
        put(cx, mc::lea(Size::Q, R10, Mem::base(R11, offset as i32)));
        frame_inst(cx, pointer, |m| mc::mov_mr(Size::Q, m, R10));
    }
    Ok(())
}

/// `end_function`, before the local area's size is read: the block as
/// the local area's last allocation, so that it ends at its top, and its
/// offset written into the entry code.
///
/// # Errors
/// `limit:frame-size`.
pub fn place(cx: &mut FnCx<'_, X64>) -> CgResult<()> {
    let Some(field) = cx.state.guard.block_field else { return Ok(()) };
    let FrameRef::Local(block) = cx.alloc_slot(u64::from(cx.stack_guard().block_size()), 16)?
    else {
        return cx.bug("the guarded block is outside the local area");
    };
    if !mc::Fixup::Abs32.patch(cx.code_mut(), field as usize, i64::from(block)) {
        return cx.bug("the guarded block's offset does not fit its field");
    }
    cx.state.guard.guard = Some(block + cx.stack_guard().guard_offset());
    cx.state.guard.fail = Some(cx.new_label());
    Ok(())
}

/// The check every exit sequence starts with (`frame::teardown`).
///
/// # Errors
/// What branching to the failure call reports.
pub fn check(cx: &mut FnCx<'_, X64>) -> CgResult<()> {
    let (Some(guard), Some(fail)) = (cx.state.guard.guard, cx.state.guard.fail) else {
        return Ok(());
    };
    let words = [Some(FrameRef::Local(guard)), cx.stack_guard().low().then_some(LOW_GUARD)];
    for word in words.into_iter().flatten() {
        frame_inst(cx, word, |m| mc::mov_rm(Size::Q, R10, m));
        put(cx, mc::cmp_rm(Size::Q, R10, guard_value()));
        branch_cond(cx, Cond::Ne, fail)?;
    }
    Ok(())
}

/// After the last exit sequence: the failure call, under the body's CFA
/// rule (the exits before it changed it; the save rows never change).
///
/// # Errors
/// What binding the label or the call's relocation reports.
pub fn fail(cx: &mut FnCx<'_, X64>) -> CgResult<()> {
    let Some(fail) = cx.state.guard.fail else { return Ok(()) };
    cx.bind(fail)?;
    cx.cfi(CfiOp::DefCfa { reg: DWARF_RBP, offset: INCOMING_BASE });
    let target = SymRef::Runtime(RuntimeFn::StackChkFail);
    put_reloc(cx, mc::call(0), X64Reloc::Plt32, target, 0)
}
