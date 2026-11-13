//! The frame (`spec/codegen.md` §5.4, §5.6, §9.3): prologue, exit
//! sequences and their CFI rows, frame places, dynamic slots, stack
//! adjustments and the call instruction. Owned by WP-116.
//!
//! ```text
//! push rbp                 def_cfa_offset 16; offset rbp, -16
//! mov rbp, rsp             def_cfa_register rbp
//! push <the save set>      offset rbx, -24 .. (after the last push)
//! [sub rsp, FRAME]         imm8 or imm32; none for 0
//! ```
//!
//! WP-146 (§5.6.15): the save set is the members of [`SAVED`] the body
//! used ([`saved_set`]), known only once the body is emitted. So
//! [`begin`] emits the largest prologue (all five pushes, `sub rsp,
//! imm32`), and [`end`] rewrites it in place with the save set's pushes
//! and the final `sub`, and cuts the bytes left over (`FnCx::cut`, §6.4):
//! the body's offsets move with the code.
//!
//! From the second instruction on the CFA is `rbp + 16` and does not move
//! with the frame or the pushes, so the save rows are all written after
//! the last push (a register still holds the caller's value before it).
//! `FRAME` is the local area rounded up to `≡ 8n (mod 16)` for `n`
//! registers saved: the stack pointer is 16-aligned after the prologue
//! and at every call.
//!
//! The epilogue is `lea rsp, [rbp - 8n]` · the pops in reverse · `pop
//! rbp`, or `leave` for an empty save set, (`def_cfa rsp, 8`) · `ret`; a
//! tail exit is the same teardown ending in `jmp callee` or `jmp r11`,
//! after restating `def_cfa rbp, 16`. Nothing in an exit depends on
//! `FRAME`, and dynamic allocation needs nothing more.
//!
//! A local `Local(o)` is `[rbp + o - 8n - FRAME]`: emitted as `[rbp + o]`
//! with a `disp32` (`common::frame_inst`) that [`end`] biases once `n`
//! and `FRAME` are known, and writes as a `disp8` when it then fits
//! (§5.6.17). An incoming stack argument `Incoming(n)` is
//! `[rbp + 16 + n]`, so no incoming base is kept; an outgoing one
//! `Outgoing(n)` is `[rsp + n]`.

use hod_cg::{
    CallTarget, CfiOp, CgError, CgResult, FnCx, FrameDesc, FrameRef, LIMIT_FRAME_SIZE, RegSet,
};
use hod_mc_x64 as mc;
use hod_mc_x64::{Mem, Reg, Size};
use hod_obj::X64Reloc;

use crate::common::{
    INCOMING_BASE, R11, RBP, RSP, bias_local_sites, frame_inst, gp, put, put_reloc,
    reset_local_sites,
};
use crate::reg::{DWARF_RBP, DWARF_RSP, dwarf, gpr};
use crate::target::X64;

/// The callee-saved registers of the pool, in push order (§5.6): the
/// save set is those of them the body used ([`saved_set`]).
pub const SAVED: [Reg; 5] = [Reg::Rbx, Reg::R12, Reg::R13, Reg::R14, Reg::R15];

/// The bytes [`begin`] reserves after `mov rbp, rsp`: the five pushes
/// (`rbx` one byte, `r12`..`r15` two) and `sub rsp, imm32`.
const RESERVED: u32 = 1 + 4 * 2 + 7;

/// The largest `FRAME`: what `sub rsp, imm32` and every `disp32` of a
/// local still express, below 2 GiB (§5.6).
pub const MAX_X64_FRAME: u32 = (1 << 31) - (1 << 16);

/// The value the `sub rsp, imm32` is emitted with before it is patched:
/// one that forces the 32-bit immediate form.
const FRAME_PLACEHOLDER: i64 = 0x7fff_ffff;

/// What the frame remembers about the function between
/// `begin_function`, the lowerings and `end_function`.
#[derive(Clone, Copy, Default, Debug)]
pub struct FrameState {
    /// The offset of the pushes of the prologue, which [`end`] rewrites.
    pub saves_at: u32,
    /// The save set, as a mask over [`SAVED`]; final in [`end`].
    pub saved: u8,
    /// The function saves every member of [`SAVED`] whatever its body
    /// used (§5.6.15): a `llvm.eh.sjlj.setjmp` resumes with the
    /// registers of the code that jumped (WP-147, §5.6.16).
    pub save_all: bool,
    /// The frame description, final in [`end`] (the coroutine stubs
    /// build the same frame).
    pub desc: FrameDesc,
    /// The slots of the address returned in `rax` and of a variadic
    /// function's register save area (`crate::call::enter`, WP-125).
    pub call: crate::call::CallState,
    /// The coroutine stubs' labels (WP-137, `crate::coro_body`).
    pub coro: crate::coro_body::CoroState,
}

/// `Target::begin_function`: the prologue and its CFI rows.
///
/// # Errors
/// None in practice; `Internal` for a CFI row out of order.
pub fn begin(cx: &mut FnCx<'_, X64>) -> CgResult<()> {
    reset_local_sites();
    put(cx, mc::push_r(RBP));
    cx.cfi(CfiOp::DefCfaOffset(16));
    cx.cfi(CfiOp::Offset { reg: DWARF_RBP, cfa_offset: -16 });
    put(cx, mc::mov_rr(Size::Q, RBP, RSP));
    cx.cfi(CfiOp::DefCfaRegister(DWARF_RBP));
    // The largest prologue, rewritten and cut in `end`.
    cx.state.frame.saves_at = cx.offset();
    for &saved in &SAVED {
        put(cx, mc::push_r(saved));
    }
    put(cx, mc::sub_ri(Size::Q, RSP, FRAME_PLACEHOLDER));
    // WP-137: a coroutine's mode and frame words.
    crate::coro_body::enter(cx)?;
    // WP-125: the address returned in `rax`, the register save area.
    crate::call::enter(cx)
}

/// The final `FRAME` for a local area of `size` bytes below `saves`
/// saved registers: the least value `>= size` that is `≡ 8 x saves (mod
/// 16)`, so that the stack pointer is 16-aligned after the prologue.
///
/// # Errors
/// `Unsupported("limit:frame-size")` above [`MAX_X64_FRAME`].
pub fn final_size(size: u32, saves: u32) -> CgResult<u32> {
    let odd = u64::from(saves % 2) * 8;
    let frame = (u64::from(size) + odd).next_multiple_of(16) - odd;
    match u32::try_from(frame) {
        Ok(frame) if frame <= MAX_X64_FRAME => Ok(frame),
        _ => Err(CgError::unsupported(LIMIT_FRAME_SIZE)),
    }
}

/// `Target::end_function`: patches `FRAME` into the prologue and the
/// local accesses, then emits the epilogue and the tail exits.
///
/// # Errors
/// `limit:frame-size`; what binding the exits' labels reports.
pub fn end(cx: &mut FnCx<'_, X64>) -> CgResult<()> {
    let mask = saved_set(cx);
    cx.state.frame.saved = mask;
    let count = mask.count_ones();
    let frame = final_size(cx.frame_size(), count)?;
    let below = 8 * count + frame;
    let desc = FrameDesc { frame_size: below, frame_base: DWARF_RBP };
    cx.state.frame.desc = desc;
    prologue(cx, frame)?;
    cx.bind(cx.epilogue())?;
    // WP-125: the `sret` or return-in-memory address back in `rax`.
    crate::call::before_return(cx);
    teardown(cx);
    put(cx, mc::ret());

    for index in 0..cx.tail_exit_count() {
        let exit = cx.tail_exit(index);
        cx.bind(exit.label)?;
        // §9.3: a tail exit comes after another exit sequence, so it
        // states the body's rule again; the saved registers' rows never
        // changed.
        cx.cfi(CfiOp::DefCfa { reg: DWARF_RBP, offset: INCOMING_BASE });
        // WP-125: `al` of a variadic function, as it arrived.
        crate::call::before_tail_exit(cx);
        teardown(cx);
        cx.tail_branch_here(index, dwarf); // WP-113: `spec/dwarf.md` §6.19
        match exit.target {
            CallTarget::Sym(sym) => put_reloc(cx, mc::jmp_near(0), X64Reloc::Plt32, sym, 0)?,
            CallTarget::Reg(callee) => put(cx, mc::jmp_r(gp(callee))),
        }
        cx.tail_branch_ends_here(index); // WP-163: `spec/dwarf.md` §6.21
    }
    // Last: the exit sequences above may address locals too (WP-125).
    bias_local_sites(cx, -i64::from(below))?;
    cx.set_frame_desc(desc);
    Ok(())
}

/// The save set (§5.6.15) as a mask over [`SAVED`]: the members the body
/// used (`FnCx::regs_used`: every register the policy handed out,
/// `take_reg` included), `rbx` in a function with the coroutine
/// frame, which writes it outside the policy (WP-137), and all of them
/// in a function with a `llvm.eh.sjlj.setjmp` ([`FrameState::save_all`]).
pub fn saved_set(cx: &FnCx<'_, X64>) -> u8 {
    let mut used = cx.regs_used();
    if cx.has_coro_frame() {
        used = used.union(RegSet::of(&[gpr(Reg::Rbx)]));
    }
    if cx.state.frame.save_all {
        used = used.union(RegSet::of(&SAVED.map(gpr)));
    }
    let mut mask = 0;
    for (index, &reg) in SAVED.iter().enumerate() {
        if used.contains(gpr(reg)) {
            mask |= 1 << index;
        }
    }
    mask
}

/// The registers of the save set, in push order (final after [`end`]).
pub fn saved(cx: &FnCx<'_, X64>) -> impl DoubleEndedIterator<Item = Reg> + Clone + use<> {
    let mask = cx.state.frame.saved;
    SAVED.into_iter().enumerate().filter(move |&(i, _)| mask >> i & 1 != 0).map(|(_, r)| r)
}

/// The prologue's final form (§5.6.15), written over what [`begin`]
/// reserved: the save set's pushes, `sub rsp, FRAME` (none for 0), the
/// rest cut; the save rows after the last push.
fn prologue(cx: &mut FnCx<'_, X64>, frame: u32) -> CgResult<()> {
    let at = cx.state.frame.saves_at;
    let regs = saved(cx);
    let Some(code) = cx.code_mut().get_mut(at as usize..(at + RESERVED) as usize) else {
        return cx.bug("the prologue is not where it was emitted");
    };
    let mut len = 0;
    for reg in regs.clone() {
        len += write(&mut code[len..], mc::push_r(reg));
    }
    let pushed = len as u32;
    if frame != 0 {
        len += write(&mut code[len..], mc::sub_ri(Size::Q, RSP, i64::from(frame)));
    }
    for (index, reg) in regs.enumerate() {
        let cfa_offset = -24 - 8 * index as i32;
        cx.cfi_at(at + pushed, CfiOp::Offset { reg: dwarf(gpr(reg)), cfa_offset })?;
    }
    cx.cut(at + len as u32, RESERVED - len as u32);
    Ok(())
}

/// Writes `inst` at the start of `code`; its length.
fn write(code: &mut [u8], inst: mc::Inst) -> usize {
    let bytes = inst.bytes();
    code[..bytes.len()].copy_from_slice(bytes);
    bytes.len()
}

/// `Target::frame_base_offset`: the frame base is `rbp`, so a local is
/// at `o - 8n - FRAME` (`desc.frame_size` is `8n + FRAME`, what the
/// prologue puts below `rbp`) and an incoming argument at `16 + n`.
pub fn base_offset(desc: &FrameDesc, place: FrameRef) -> Option<i64> {
    match place {
        FrameRef::Local(o) => Some(i64::from(o) - i64::from(desc.frame_size)),
        FrameRef::Incoming(n) => Some(i64::from(INCOMING_BASE) + i64::from(n)),
        FrameRef::Outgoing(_) | FrameRef::Coro(_) => None,
    }
}

/// The teardown every exit sequence starts with: the stack pointer back
/// at the save area, the save set and `rbp` popped; `leave` when nothing
/// is saved.
fn teardown(cx: &mut FnCx<'_, X64>) {
    let count = saved(cx).count();
    if count == 0 {
        put(cx, mc::leave());
    } else {
        put(cx, mc::lea(Size::Q, RSP, Mem::base(RBP, -8 * count as i32)));
        for reg in saved(cx).rev() {
            put(cx, mc::pop_r(reg));
        }
        put(cx, mc::pop_r(RBP));
    }
    cx.cfi(CfiOp::DefCfa { reg: DWARF_RSP, offset: 8 });
}

/// `Target::adjust_sp`: `lea rsp, [rsp + delta]`, flags kept, in steps
/// of at most 1 GiB.
pub fn adjust_sp(cx: &mut FnCx<'_, X64>, delta: i64) {
    const STEP: i64 = 1 << 30;
    let mut left = delta;
    while left != 0 {
        let step = left.clamp(-STEP, STEP);
        put(cx, mc::lea(Size::Q, RSP, Mem::base(RSP, step as i32)));
        left -= step;
    }
}

/// `Target::dynamic_slot`: `lea r11, [rsp - size]` · `and r11, -align` ·
/// `mov rsp, r11` · the address stored to `ptr`. The size is rounded to
/// 16 so the stack pointer stays aligned. The epilogue restores the
/// stack pointer from `rbp`, so nothing else changes.
///
/// # Errors
/// `Unsupported("limit:frame-size")` for a size or alignment beyond what
/// 32 bits express.
pub fn dynamic_slot(cx: &mut FnCx<'_, X64>, ptr: FrameRef, size: u64, align: u64) -> CgResult<()> {
    let too_big = || CgError::unsupported(LIMIT_FRAME_SIZE);
    let size = size
        .checked_next_multiple_of(16)
        .filter(|&s| s <= u64::from(MAX_X64_FRAME))
        .ok_or_else(too_big)?;
    if !align.is_power_of_two() || align > 1 << 30 {
        return Err(too_big());
    }
    put(cx, mc::lea(Size::Q, R11, Mem::base(RSP, -(size as i32))));
    put(cx, mc::and_ri(Size::Q, R11, -(align as i64)));
    put(cx, mc::mov_rr(Size::Q, RSP, R11));
    frame_inst(cx, ptr, |m| mc::mov_mr(Size::Q, m, R11));
    Ok(())
}

/// The call instruction itself (`Target::emit_call`): `mov al, n` first
/// for a variadic callee (SysV: an upper bound on the vector registers
/// used), then `call rel32` with `R_X86_64_PLT32`, or `call r11`.
///
/// # Errors
/// `Internal` only for an encoder without a relocatable field.
pub fn emit_call(
    cx: &mut FnCx<'_, X64>,
    target: CallTarget,
    variadic: bool,
    vector_regs: u8,
) -> CgResult<()> {
    if variadic {
        put(cx, mc::mov_ri(Size::B, Reg::Rax, i64::from(vector_regs)));
    }
    match target {
        CallTarget::Sym(sym) => put_reloc(cx, mc::call(0), X64Reloc::Plt32, sym, 0),
        CallTarget::Reg(callee) => {
            put(cx, mc::call_r(gp(callee)));
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §5.6.15: `FRAME ≡ 8n (mod 16)` and at least the local area.
    #[test]
    fn frame_size_keeps_the_stack_aligned() {
        for (size, frame) in [(0, 8), (8, 8), (9, 24), (16, 24), (24, 24), (25, 40)] {
            assert_eq!(final_size(size, 5).unwrap(), frame, "{size}");
        }
        for (size, frame) in [(0, 0), (1, 16), (16, 16), (17, 32)] {
            assert_eq!(final_size(size, 0).unwrap(), frame, "{size}");
            assert_eq!(final_size(size, 2).unwrap(), frame, "{size}");
        }
        for saves in 0..=5 {
            for size in 0..200 {
                let frame = final_size(size, saves).unwrap();
                assert!(frame >= size && frame < size + 16);
                assert!((16 + 8 * saves + frame).is_multiple_of(16));
            }
        }
        assert!(final_size(MAX_X64_FRAME - 8, 5).is_ok());
        assert!(final_size(MAX_X64_FRAME + 1, 5).is_err());
    }

    /// §5.6: the prologue's `sub` has a 32-bit immediate to patch.
    #[test]
    fn the_frame_size_is_a_32_bit_immediate() {
        let sub = mc::sub_ri(Size::Q, RSP, FRAME_PLACEHOLDER);
        assert_eq!(sub.bytes(), [0x48, 0x81, 0xec, 0xff, 0xff, 0xff, 0x7f]);
    }
}
