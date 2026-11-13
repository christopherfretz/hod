//! Stack protectors on AArch64 (`spec/codegen.md` §5.6.24, WP-183).
//!
//! ```text
//!   | caller's frame                           |  CFA = x29 + FRAME
//!   | [pad]                                    |
//!   | guard word                               |  top of the local area
//!   | the guarded block's slots                |
//!   | the rest of the local area               |
//!   | saved lr, saved fp       [x29 + 0]       |  frame record
//!   | low guard                [x29 - 16]      |  with a dynamic area only
//!   | dynamic allocations                      |
//! ```
//!
//! The frame record is at the bottom of hod's frame (§5.5), so the
//! overflow of a dynamic allocation would reach the saved `lr` first: a
//! function with a dynamic area also has the **low guard**, 16 bytes the
//! entry code takes below the record (`sub sp, sp, #16`; the epilogue
//! then restores `sp` from `x29`).
//!
//! The guard value is the variable `__stack_chk_guard`, through the GOT
//! (`adrp`/`ldr :got_lo12:`/`ldr`), as `llc` reads it for Linux under
//! every relocation model. The entry code (`Target::enter_guard`) puts
//! the guard word's address in `x16` with `add x16, x29, #lo12` · `add
//! x16, x16, #hi12, lsl #12`, patched in `end_function` when the block's
//! place is known, stores the value there (and in the low guard), and
//! stores each slot's address (`x16` less its distance from the guard) in
//! its pointer slot. Every exit sequence starts with the check, which
//! keeps `x16` (a tail exit's callee) on the stack around it:
//! `str x16, [sp, #-16]!` · the value in `x16` · `ldr x17, [guard]` ·
//! `cmp x16, x17` · `b.ne fail` (the same for the low guard) ·
//! `ldr x16, [sp], #16`. `fail` follows the last exit: the body's rows
//! again, then `bl __stack_chk_fail`, which does not return.

use hod_cg::{CgError, CgResult, FnCx, FrameRef, LIMIT_FRAME_SIZE, Label, RuntimeFn, SymRef};
use hod_mc_a64 as mc;
use hod_mc_a64::{Load, MemSize, Reg, Size};
use hod_obj::{A64Reloc, RelocKind};

use crate::common::{Cursor, FP, SP, X16, X17, add_const, branch_cond, load_gpr_at, put};
use crate::target::A64;

/// The low guard's offset from `x29`.
const LOW_GUARD: i64 = -16;

/// What the guard remembers between the entry code and `end_function`.
#[derive(Clone, Copy, Default, Debug)]
pub struct GuardState {
    /// The offset of the entry code's two `add`s that make the guard
    /// word's address.
    patch: Option<u32>,
    /// The local-area offset of the guard word, once the block is placed.
    guard: Option<u32>,
    /// The label of the call of `__stack_chk_fail`.
    fail: Option<Label>,
}

/// Loads the guard value into `rd`: `adrp` · `ldr :got_lo12:` · `ldr`.
fn guard_value(cx: &mut FnCx<'_, A64>, rd: Reg) {
    let target = SymRef::Runtime(RuntimeFn::StackChkGuard);
    let at = cx.offset();
    put(cx, mc::adrp(rd.zr(), 0));
    cx.reloc(at, RelocKind::A64(A64Reloc::AdrGotPage), target, 0);
    put(cx, mc::ldr_off(Load::X, rd.zr(), rd.sp(), 0));
    cx.reloc(at + 4, RelocKind::A64(A64Reloc::Ld64GotLo12Nc), target, 0);
    put(cx, mc::ldr_off(Load::X, rd.zr(), rd.sp(), 0));
}

/// The two `add`s of `x29 + offset` into `x16`, `offset` below 2^24.
fn guard_address(offset: u32) -> [u32; 2] {
    [
        mc::add_imm(Size::X, X16.sp(), FP, u64::from(offset & 0xfff)),
        mc::add_imm(Size::X, X16.sp(), X16.sp(), u64::from(offset & 0xff_f000)),
    ]
}

/// `Target::enter_guard`: the guard words and the block's pointers.
///
/// # Errors
/// `limit:frame-size` for a pointer slot beyond a scaled `str` (they are
/// the first slots after `begin_function`'s, so never in practice).
pub fn enter(cx: &mut FnCx<'_, A64>) -> CgResult<()> {
    cx.state.guard = GuardState::default();
    if !cx.stack_guard().is_on() {
        return cx.bug("guard entry code for a function without a guard");
    }
    guard_value(cx, X17);
    cx.state.guard.patch = Some(cx.offset());
    for word in guard_address(0) {
        put(cx, word);
    }
    put(cx, mc::str_off(MemSize::X, X17.zr(), X16.sp(), 0));
    if cx.stack_guard().low() {
        put(cx, mc::sub_imm(Size::X, SP, SP, 16));
        put(cx, mc::str_off(MemSize::X, X17.zr(), SP, 0));
        cx.state.frame.dynamic = true;
    }
    let guard = i64::from(cx.stack_guard().guard_offset());
    for index in 0..cx.stack_guard().pointers().len() {
        let (pointer, offset) = cx.stack_guard().pointers()[index];
        let FrameRef::Local(at) = pointer else {
            return cx.bug("a guarded slot's pointer outside the local area");
        };
        if at % 8 != 0 || at >= 8 << 12 {
            return Err(CgError::unsupported(LIMIT_FRAME_SIZE));
        }
        add_const(cx, X17.sp(), X16.sp(), i64::from(offset) - guard);
        put(cx, mc::str_off(MemSize::X, X17.zr(), FP, i64::from(at)));
    }
    Ok(())
}

/// `end_function`, before the frame size is read: the block as the local
/// area's last allocation, so that it ends at the CFA, and the guard
/// word's offset written into the entry code.
///
/// # Errors
/// `limit:frame-size`.
pub fn place(cx: &mut FnCx<'_, A64>) -> CgResult<()> {
    let Some(at) = cx.state.guard.patch else { return Ok(()) };
    let FrameRef::Local(block) = cx.alloc_slot(u64::from(cx.stack_guard().block_size()), 16)?
    else {
        return cx.bug("the guarded block is outside the local area");
    };
    let guard = block + cx.stack_guard().guard_offset();
    if guard >= 1 << 24 {
        return Err(CgError::unsupported(LIMIT_FRAME_SIZE));
    }
    for (index, word) in guard_address(guard).into_iter().enumerate() {
        let start = at as usize + index * 4;
        cx.code_mut()[start..start + 4].copy_from_slice(&word.to_le_bytes());
    }
    cx.state.guard.guard = Some(guard);
    cx.state.guard.fail = Some(cx.new_label());
    Ok(())
}

/// The check every exit sequence starts with (`frame::teardown`), before
/// the stack pointer is restored.
///
/// # Errors
/// What branching to the failure call reports.
pub fn check(cx: &mut FnCx<'_, A64>) -> CgResult<()> {
    let (Some(guard), Some(fail)) = (cx.state.guard.guard, cx.state.guard.fail) else {
        return Ok(());
    };
    put(cx, mc::str_pre(MemSize::X, X16.zr(), SP, -16));
    guard_value(cx, X16);
    let mut word = Cursor::frame(cx, FrameRef::Local(guard), X17);
    load_gpr_at(cx, Load::X, X17, &mut word);
    put(cx, mc::cmp(Size::X, X16.zr(), X17.zr()));
    branch_cond(cx, mc::Cond::Ne, fail)?;
    if cx.stack_guard().low() {
        put(cx, mc::ldur(Load::X, X17.zr(), FP, LOW_GUARD));
        put(cx, mc::cmp(Size::X, X16.zr(), X17.zr()));
        branch_cond(cx, mc::Cond::Ne, fail)?;
    }
    put(cx, mc::ldr_post(Load::X, X16.zr(), SP, 16));
    Ok(())
}

/// The label of the failure call, if the function has a guard: the frame
/// binds it after the last exit and restates the body's rows.
pub fn fail_label(cx: &FnCx<'_, A64>) -> Option<Label> {
    cx.state.guard.fail
}

/// The failure call, at its label (`fail_label`).
pub fn fail(cx: &mut FnCx<'_, A64>) {
    let at = cx.offset();
    put(cx, mc::bl(0));
    let target = SymRef::Runtime(RuntimeFn::StackChkFail);
    cx.reloc(at, RelocKind::A64(A64Reloc::Call26), target, 0);
}
