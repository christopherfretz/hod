//! Stack protectors on i386 (`spec/codegen-x86.md` §19.2, WP-324): the
//! target half of `spec/codegen.md` §5.6.24, whose rules (which functions,
//! which slots move into the guarded block, its layout) are `hod_cg`'s
//! `guard`, the same on every target. Shaped as `hod_x64`'s `guard`.
//!
//! ```text
//!   | return address, saved ebp, ebx, esi, edi |
//!   | [pad]                                    |
//!   | guard word          [block + guard]      |  top of the local area
//!   | the guarded block's slots                |
//!   | the rest of the local area               |
//!   | low guard           Local(0)             |  with a dynamic area only
//!   | dynamic allocations                      |
//! ```
//!
//! The guard value is `gs:[0x14]` (glibc's i386 TCB `stack_guard`, what
//! `llc` 19 and 22 read for `i686-*-linux-gnu` in every relocation
//! model; the module flags that choose another are refused by the front
//! half, `module-flag:stack-protector-guard*`). The guard word is the
//! block's 8 bytes (`hod_cg::GUARD_BYTES`), of which the first 4 hold the
//! value. Entry code (`Target::enter_guard`, scratch `ecx`, `edx`):
//! `mov ecx, gs:[0x14]` · `lea edx, [ebp + block]` · `mov [edx + guard],
//! ecx` · (`mov [low], ecx`) · per slot of the block `lea ecx, [edx + k]`
//! · `mov [pointer], ecx`. The `lea`'s displacement is a local access
//! (`frame`), written in `end_function` before the local sites are
//! biased. Every exit sequence starts with the check `mov ecx, [guard]` ·
//! `cmp ecx, gs:[0x14]` · `jne fail` (and the same for the low guard); a
//! tail exit may hold an argument in `ecx`, so there the check is
//! bracketed by `push ecx` · `pop ecx` (`pop` keeps the flags). `fail`
//! follows the last exit: the body's CFA rule again, then the call of
//! `__stack_chk_fail` through `pic::emit_call` (`R_386_PLT32`; under PIC
//! with the GOT pointer in `ebx`, as `llc` calls it: never
//! `__stack_chk_fail_local`), which does not return.
//!
//! x87: `llc` checks the guard in a block of its own before the return
//! (`SP_return`), so a `float`/`double` result is stored at its type and
//! reloaded before the `ret`: a function `llc` protects returns its
//! result rounded, never an 80-bit copy ([`checks_at_return`], used by
//! `control::ret`); one only hod protects keeps the copy (hod's check
//! does not touch the x87 stack).

use hod_cg::abi::{Assignment, PartKind, Place, RetMemory};
use hod_cg::{CallTarget, CfiOp, CgResult, FnCx, FrameRef, Label, RuntimeFn, SymRef};
use hod_ir::{FnAttrs, SymbolKind};
use hod_mc_x64 as mc;
use hod_mc_x64::{Cond, Mem, Reg, Seg, Size};

use crate::common::{ECX, EDX, INCOMING_BASE, branch_cond, frame_inst, put};
use crate::reg::DWARF_EBP;
use crate::target::X86;

/// Where glibc keeps the guard value on i386: `gs:[0x14]`, the TCB's
/// `stack_guard` (what `llc` reads for Linux).
const GUARD_TLS_OFFSET: i32 = 0x14;

/// The scratch of [`copy_arguments`]: saved by the prologue, never an
/// argument register.
const ESI: Reg = Reg::Rsi;

/// The low guard's slot: the start of the local area, the first thing a
/// dynamic allocation's overflow reaches.
const LOW_GUARD: FrameRef = FrameRef::Local(0);

/// What the guard remembers between `begin_function`, the entry code and
/// `end_function`.
#[derive(Clone, Copy, Default, Debug)]
pub struct GuardState {
    /// The `disp32` field of the entry code's `lea edx, [ebp + block]`.
    block_field: Option<u32>,
    /// The local-area offset of the guard word, once the block is placed.
    guard: Option<u32>,
    /// The label of the call of `__stack_chk_fail`.
    fail: Option<Label>,
    /// The incoming arguments' copy ([`copy_arguments`]): the span
    /// `[lo, hi)` of the incoming area and the local slot it is at.
    args: Option<ArgsCopy>,
    /// Set while a `musttail` call writes the real incoming area.
    raw: bool,
}

/// The copy of the incoming stack arguments below the guarded block.
#[derive(Clone, Copy, Debug)]
struct ArgsCopy {
    lo: u32,
    hi: u32,
    slot: u32,
}

fn guard_value() -> Mem {
    Mem::abs(GUARD_TLS_OFFSET).with_seg(Seg::Gs)
}

/// The buffer size from which `llc` protects a character array under
/// `ssp` (clang's `"stack-protector-buffer-size"`, 8 by default).
const SSP_BUFFER: u32 = 8;

/// Whether `llc` checks a guard before this function's return, so that
/// its result is rounded to its type first (module doc, x87). hod
/// protects a superset of `llc`'s functions (§5.6.24 rule 1) and its own
/// check needs no rounding; the rounding follows `llc`'s choice as far as
/// hod can see it without slot types: `sspreq`; a dynamic area; under
/// `sspstrong` an escaping slot (`llc`: an array or an address-taken
/// local); under `ssp` an escaping slot of at least [`SSP_BUFFER`] bytes
/// (`llc`: a character array that large).
pub fn checks_at_return(cx: &FnCx<'_, X86>) -> bool {
    let plan = cx.stack_guard();
    if !plan.is_on() {
        return false;
    }
    let flags = match cx.module.symbol(cx.body.symbol()).kind() {
        SymbolKind::Function { attrs, .. } => cx.module.fn_attrs(attrs).flags(),
        _ => 0,
    };
    if flags & FnAttrs::SSP_REQ != 0 || plan.low() {
        return true;
    }
    // The block's top slot is its largest; the guard follows it.
    let top = plan.pointers().iter().map(|&(_, at)| at).max();
    let Some(top) = top else { return false };
    flags & FnAttrs::SSP_STRONG != 0 || plan.guard_offset() - top >= SSP_BUFFER
}

/// `begin_function`, right after the prologue's `sub esp`: the low
/// guard's slot, which must be the local area's first.
///
/// # Errors
/// What allocating the slot reports.
pub fn reserve(cx: &mut FnCx<'_, X86>) -> CgResult<()> {
    cx.state.ssp = GuardState::default();
    if !(cx.stack_guard().is_on() && cx.stack_guard().low()) {
        return Ok(());
    }
    if cx.alloc_slot(16, 16)? != LOW_GUARD {
        return cx.bug("the low guard is not at the start of the local area");
    }
    Ok(())
}

/// Whether the bytes `[at, at + len)` of the incoming area hold an
/// argument the body reads as a value: the hidden result pointer, or a
/// part that is not a `byval` copy (`llc` reads those in place).
fn copied(asg: &Assignment, at: u32, len: u32) -> bool {
    let overlaps = |start: u32, size: u32| at < start + size.max(1) && start < at + len;
    let hidden = matches!(asg.ret_memory, Some(RetMemory { address: Place::Stack(0), .. }));
    (hidden && overlaps(0, 4))
        || asg.args.iter().any(|p| {
            p.kind != PartKind::Pointee
                && matches!(p.place, Place::Stack(start) if overlaps(start, p.size))
        })
}

/// `begin_function`, after [`reserve`]: in a protected function the
/// incoming stack arguments (the hidden result pointer, every part that
/// is not a `byval` copy) are copied into a local slot, below the guarded
/// block, before any argument register is read; every later access of
/// those bytes is to the copy ([`incoming`]). An overflow that reaches
/// the incoming area then changes nothing the body reads or writes
/// through before the check, as under `llc`, which loads each stack
/// argument at entry and keeps it in a spill slot below its protected
/// arrays. Scratch: `esi` (saved by the prologue, never an argument).
///
/// # Errors
/// What allocating the slot reports.
pub fn copy_arguments(cx: &mut FnCx<'_, X86>) -> CgResult<()> {
    if !cx.stack_guard().is_on() {
        return Ok(());
    }
    let asg = cx.fn_assignment();
    let hidden = matches!(asg.ret_memory, Some(RetMemory { address: Place::Stack(0), .. }));
    let spans =
        asg.args.iter().filter(|p| p.kind != PartKind::Pointee).filter_map(|p| match p.place {
            Place::Stack(start) => Some((start, start + p.size.max(1))),
            Place::Reg(_) => None,
        });
    let spans = spans.chain(hidden.then_some((0, 4)));
    let Some((lo, hi)) = spans.fold(None, |acc: Option<(u32, u32)>, (s, e)| {
        Some(acc.map_or((s, e), |(lo, hi)| (lo.min(s), hi.max(e))))
    }) else {
        return Ok(());
    };
    // The copy keeps each byte's offset modulo 16 (an over-aligned part
    // stays aligned) and is whole words.
    let (lo, hi) = (lo & !15, hi.next_multiple_of(4));
    let FrameRef::Local(slot) = cx.alloc_slot(u64::from(hi - lo), 16)? else {
        return cx.bug("the arguments' copy is outside the local area");
    };
    for at in (lo..hi).step_by(4) {
        if copied(cx.fn_assignment(), at, 4) {
            frame_inst(cx, FrameRef::Incoming(at), |m| mc::mov_rm(Size::D, ESI, m));
            frame_inst(cx, FrameRef::Local(slot + at - lo), |m| mc::mov_mr(Size::D, m, ESI));
        }
    }
    cx.state.ssp.args = Some(ArgsCopy { lo, hi, slot });
    Ok(())
}

/// Where an access of `place` goes: an incoming argument's bytes that
/// [`copy_arguments`] copied are read and written in the copy.
pub fn incoming(cx: &FnCx<'_, X86>, place: FrameRef) -> FrameRef {
    let (FrameRef::Incoming(at), Some(copy)) = (place, cx.state.ssp.args) else { return place };
    if cx.state.ssp.raw || at < copy.lo || at >= copy.hi || !copied(cx.fn_assignment(), at, 1) {
        return place;
    }
    FrameRef::Local(copy.slot + at - copy.lo)
}

/// `f` with the real incoming area: a `musttail` call writes its stack
/// arguments there, over the caller's.
pub fn raw_incoming(cx: &mut FnCx<'_, X86>, f: impl FnOnce(&mut FnCx<'_, X86>)) {
    let was = cx.state.ssp.raw;
    cx.state.ssp.raw = true;
    f(cx);
    cx.state.ssp.raw = was;
}

/// `Target::enter_guard`: the guard words and the block's pointers.
///
/// # Errors
/// `Internal` only for a plan without a guard.
pub fn enter(cx: &mut FnCx<'_, X86>) -> CgResult<()> {
    if !cx.stack_guard().is_on() {
        return cx.bug("guard entry code for a function without a guard");
    }
    put(cx, mc::mov_rm(Size::D, ECX, guard_value()));
    frame_inst(cx, FrameRef::Local(0), |m| mc::lea(Size::D, EDX, m));
    // `lea r, [ebp + disp32]` ends with its displacement.
    cx.state.ssp.block_field = Some(cx.offset() - 4);
    let guard = cx.stack_guard().guard_offset();
    put(cx, mc::mov_mr(Size::D, Mem::base(EDX, guard as i32), ECX));
    if cx.stack_guard().low() {
        frame_inst(cx, LOW_GUARD, |m| mc::mov_mr(Size::D, m, ECX));
    }
    for index in 0..cx.stack_guard().pointers().len() {
        let (pointer, offset) = cx.stack_guard().pointers()[index];
        put(cx, mc::lea(Size::D, ECX, Mem::base(EDX, offset as i32)));
        frame_inst(cx, pointer, |m| mc::mov_mr(Size::D, m, ECX));
    }
    Ok(())
}

/// A coroutine stub's entry code (`spec/codegen-x86.md` §19.2, §18.1;
/// the 64-bit targets' `guard::stub_enter`): a resumed activation has a
/// machine frame of its own, so its guard words and the guarded block's
/// pointers are written again, as [`enter`] wrote them in the ramp. The
/// stub runs after `end_function` placed the block: `local` gives a
/// local slot's final operand. Scratch `ecx` (the stub has copied the
/// frame pointer it brought into `esi` and the frame word).
///
/// # Errors
/// What `local` reports.
pub fn stub_enter(
    cx: &mut FnCx<'_, X86>,
    local: impl Fn(FrameRef) -> CgResult<Mem>,
) -> CgResult<()> {
    let Some(guard) = cx.state.ssp.guard else { return Ok(()) };
    let block = guard - cx.stack_guard().guard_offset();
    put(cx, mc::mov_rm(Size::D, ECX, guard_value()));
    put(cx, mc::mov_mr(Size::D, local(FrameRef::Local(guard))?, ECX));
    if cx.stack_guard().low() {
        put(cx, mc::mov_mr(Size::D, local(LOW_GUARD)?, ECX));
    }
    for index in 0..cx.stack_guard().pointers().len() {
        let (pointer, offset) = cx.stack_guard().pointers()[index];
        put(cx, mc::lea(Size::D, ECX, local(FrameRef::Local(block + offset))?));
        put(cx, mc::mov_mr(Size::D, local(pointer)?, ECX));
    }
    Ok(())
}

/// `end_function`, before the local area's size is read: the block as
/// the local area's last allocation, so that it ends at its top, and its
/// offset written into the entry code.
///
/// # Errors
/// `limit:frame-size`.
pub fn place(cx: &mut FnCx<'_, X86>) -> CgResult<()> {
    let Some(field) = cx.state.ssp.block_field else { return Ok(()) };
    let FrameRef::Local(block) = cx.alloc_slot(u64::from(cx.stack_guard().block_size()), 16)?
    else {
        return cx.bug("the guarded block is outside the local area");
    };
    if !mc::Fixup::Abs32.patch(cx.code_mut(), field as usize, i64::from(block)) {
        return cx.bug("the guarded block's offset does not fit its field");
    }
    cx.state.ssp.guard = Some(block + cx.stack_guard().guard_offset());
    cx.state.ssp.fail = Some(cx.new_label());
    Ok(())
}

/// The check an exit sequence starts with (`frame::end`), before
/// anything is popped. `keep_ecx`: a tail exit, whose `ecx` may hold an
/// argument.
///
/// # Errors
/// What branching to the failure call reports.
pub fn check(cx: &mut FnCx<'_, X86>, keep_ecx: bool) -> CgResult<()> {
    let (Some(guard), Some(fail)) = (cx.state.ssp.guard, cx.state.ssp.fail) else {
        return Ok(());
    };
    let words = [Some(FrameRef::Local(guard)), cx.stack_guard().low().then_some(LOW_GUARD)];
    for word in words.into_iter().flatten() {
        if keep_ecx {
            put(cx, mc::push_r(ECX));
        }
        frame_inst(cx, word, |m| mc::mov_rm(Size::D, ECX, m));
        put(cx, mc::cmp_rm(Size::D, ECX, guard_value()));
        if keep_ecx {
            put(cx, mc::pop_r(ECX));
        }
        branch_cond(cx, Cond::Ne, fail)?;
    }
    Ok(())
}

/// After the last exit sequence: the failure call, under the body's CFA
/// rule (the exits before it changed it; the save rows never change).
///
/// # Errors
/// What binding the label or the call reports.
pub fn fail(cx: &mut FnCx<'_, X86>) -> CgResult<()> {
    let Some(fail) = cx.state.ssp.fail else { return Ok(()) };
    cx.bind(fail)?;
    cx.cfi(CfiOp::DefCfa { reg: DWARF_EBP, offset: INCOMING_BASE });
    let target = CallTarget::Sym(SymRef::Runtime(RuntimeFn::StackChkFail));
    crate::pic::emit_call(cx, target, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hod_mc_x64::Mode;

    /// `llc`'s bytes for `movl %gs:20, %ecx` and `cmpl %gs:20, %ecx`.
    #[test]
    fn the_guard_value_is_gs_20() {
        let mov = Mode::X86.admit(mc::mov_rm(Size::D, ECX, guard_value())).unwrap();
        assert_eq!(mov.bytes(), [0x65, 0x8b, 0x0d, 0x14, 0, 0, 0]);
        let cmp = Mode::X86.admit(mc::cmp_rm(Size::D, ECX, guard_value())).unwrap();
        assert_eq!(cmp.bytes(), [0x65, 0x3b, 0x0d, 0x14, 0, 0, 0]);
    }

    /// The entry code's `lea edx, [ebp + disp32]` ends with its field.
    #[test]
    fn the_block_lea_ends_with_its_displacement() {
        let m = Mem::base(crate::common::EBP, 0).with_abs32().unwrap();
        let lea = Mode::X86.admit(mc::lea(Size::D, EDX, m)).unwrap();
        let site = lea.site().unwrap();
        assert_eq!(site.offset as usize + 4, lea.bytes().len());
    }
}
