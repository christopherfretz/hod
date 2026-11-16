//! The frame (`spec/codegen-x86.md` "Frame"): prologue, epilogue, CFI
//! rows, frame places, stack adjustments and the call instruction.
//! Owned by WP-293.
//!
//! ```text
//! push ebp                 def_cfa_offset 8; offset ebp, -8
//! mov ebp, esp             def_cfa_register ebp
//! push ebx; push esi; push edi
//!                          offset ebx, -12; esi, -16; edi, -20
//! sub esp, FRAME           imm32, patched in `end`
//! ```
//!
//! Every function saves the three pool registers (`ebx`, `esi`, `edi`):
//! hod does not chase the bytes a smaller save set would save. `FRAME`
//! is the local area rounded up to `≡ 12 (mod 16)`, so that `esp` is
//! 16-aligned after the prologue and at every call (the caller's call
//! left it `≡ 12`). A local `Local(o)` is `[ebp + o - 12 - FRAME]`,
//! emitted with a `disp32` that `end` biases; an incoming argument
//! `Incoming(n)` is `[ebp + 8 + n]`; an outgoing one `[esp + n]`.
//!
//! The epilogue is `lea esp, [ebp - 12]` · `pop edi` · `pop esi` · `pop
//! ebx` · `pop ebp` · `ret`. The CFI rows are recorded for the unwind
//! tables of design 0008's I10; this package writes no `.eh_frame`.

use hod_cg::{
    CallTarget, CfiOp, CgError, CgResult, FnCx, FrameDesc, FrameRef, LIMIT_FRAME_SIZE, SymRef,
};
use hod_mc_x64 as mc;
use hod_mc_x64::{Mem, Mode, Reg, Size};
use hod_obj::{I386Reloc, RelocKind};

use hod_cg::abi::Place;

use crate::abi;
use crate::common::{
    self, EBP, ESP, INCOMING_BASE, bias_local_sites, gp, put, reset_function, take_admit_error,
    unsupported,
};
use crate::reg::{DWARF_EBP, DWARF_ESP, dwarf};
use crate::target::X86;

/// The callee-saved registers, in push order.
pub const SAVED: [Reg; 3] = [Reg::Rbx, Reg::Rsi, Reg::Rdi];

/// The one `musttail` shape [`end`]'s tail exits cannot take
/// (`abi::check_call`, `spec/abi-x86.md` §8.5): stack arguments together
/// with an argument in `ecx`, which the copy into the incoming area uses.
pub const MUSTTAIL: &str = "x86:abi:musttail";

/// The largest `FRAME`.
pub const MAX_FRAME: u32 = (1 << 31) - (1 << 16);

/// What the frame remembers between `begin` and `end`.
#[derive(Clone, Copy, Default, Debug)]
pub struct FrameState {
    /// The offset of the prologue's `sub esp, imm32` immediate.
    pub frame_imm: u32,
    /// The slot that keeps an `inreg sret` pointer, returned in `eax`
    /// (`spec/abi-x86.md` §6.1).
    pub sret_slot: Option<FrameRef>,
}

/// The final `FRAME` for a local area of `size` bytes: the least value
/// `>= size` that is `≡ 12 (mod 16)`.
///
/// # Errors
/// `limit:frame-size` above [`MAX_FRAME`].
pub fn final_size(size: u32) -> CgResult<u32> {
    let frame = (u64::from(size) + 4).next_multiple_of(16) - 4;
    match u32::try_from(frame) {
        Ok(frame) if frame <= MAX_FRAME => Ok(frame),
        _ => Err(CgError::unsupported(LIMIT_FRAME_SIZE)),
    }
}

/// `Target::begin_function`: the prologue, `FRAME` a placeholder.
///
/// # Errors
/// None today.
pub fn begin(cx: &mut FnCx<'_, X86>) -> CgResult<()> {
    reset_function();
    put(cx, mc::push_r(EBP));
    cx.cfi(CfiOp::DefCfaOffset(8));
    cx.cfi(CfiOp::Offset { reg: DWARF_EBP, cfa_offset: -8 });
    put(cx, mc::mov_rr(Size::D, EBP, ESP));
    cx.cfi(CfiOp::DefCfaRegister(DWARF_EBP));
    for reg in SAVED {
        put(cx, mc::push_r(reg));
    }
    for (index, reg) in SAVED.into_iter().enumerate() {
        let reg = dwarf(crate::reg::gpr(reg));
        cx.cfi(CfiOp::Offset { reg, cfa_offset: -12 - 4 * index as i32 });
    }
    let sub = mc::sub_ri(Size::D, ESP, 0x7fff_ffff);
    // `81 EC imm32`: the immediate is the last four bytes.
    cx.state.frame.frame_imm = cx.offset() + sub.bytes().len() as u32 - 4;
    put(cx, sub);
    // `spec/abi-x86.md` §5.2: a function that writes its result through
    // a hidden pointer is design 0008's I11 (`aggr`); §6.1: an `inreg
    // sret` pointer is kept for the return, before any argument
    // register is read.
    let asg = cx.fn_assignment();
    if asg.ret_memory.is_some() {
        return unsupported(crate::target::AGGREGATE);
    }
    let inreg_sret = asg.sret_returned_in.is_some()
        && asg.args.first().is_some_and(|p| p.arg == 0 && p.place == Place::Reg(crate::reg::EAX));
    cx.state.frame.sret_slot = None;
    if inreg_sret {
        let slot = cx.alloc_slot(4, 4)?;
        common::store_gpr(cx, Reg::Rax, slot, 4);
        cx.state.frame.sret_slot = Some(slot);
    }
    Ok(())
}

/// `Target::end_function`: patches `FRAME` into the prologue and the
/// local accesses, then emits the epilogue.
///
/// # Errors
/// `limit:frame-size`; an encoding error of the function.
pub fn end(cx: &mut FnCx<'_, X86>) -> CgResult<()> {
    let frame = final_size(cx.frame_size())?;
    let below = 12 + frame;
    let at = cx.state.frame.frame_imm as usize;
    if !mc::Fixup::Abs32.patch(cx.code_mut(), at, i64::from(frame)) {
        return cx.bug("the prologue is not where it was emitted");
    }
    cx.bind(cx.epilogue())?;
    // §5.2, §6.1: the `sret` pointer is returned in `eax`; a stack one
    // is popped by `ret $4`.
    let pop = abi::callee_pop(cx.fn_assignment());
    if let Some(slot) = cx.state.frame.sret_slot {
        common::load_gpr(cx, Reg::Rax, slot, 4);
    } else if pop != 0 {
        common::load_gpr(cx, Reg::Rax, FrameRef::Incoming(0), 4);
    }
    teardown(cx);
    if pop == 0 {
        put(cx, mc::ret());
    } else {
        put(cx, mc::ret_imm(pop as u16));
    }
    // `spec/abi-x86.md` §8.5: a `musttail` exit. The call sequence wrote
    // the stack arguments over the incoming ones and the register
    // arguments in place; the callee has this function's signature, so
    // it pops what this function would have popped.
    for index in 0..cx.tail_exit_count() {
        let exit = cx.tail_exit(index);
        cx.bind(exit.label)?;
        // The exit comes after another exit sequence: the body's rule
        // again; the saved registers' rows never changed.
        cx.cfi(CfiOp::DefCfa { reg: DWARF_EBP, offset: INCOMING_BASE });
        teardown(cx);
        cx.tail_branch_here(index, dwarf);
        match exit.target {
            CallTarget::Sym(sym) => {
                let inst = mc::jmp_near(0);
                let Some(site) = Mode::X86.admit(inst).ok().and_then(|i| i.site()) else {
                    return cx.bug("a jump without a relocatable field");
                };
                let at = cx.offset();
                put(cx, inst);
                let kind = RelocKind::I386(call_reloc(cx, sym));
                cx.reloc(at + site.offset as u32, kind, sym, site.addend);
            }
            CallTarget::Reg(callee) => put(cx, mc::jmp_r(gp(callee))),
        }
        cx.tail_branch_ends_here(index);
    }
    bias_local_sites(cx, -i64::from(below))?;
    cx.set_frame_desc(FrameDesc { frame_size: below, frame_base: DWARF_EBP });
    take_admit_error(cx)
}

/// The frame's teardown before a `ret` or a tail jump: `lea esp, [ebp -
/// 12]`, the saved registers and `ebp` popped. Writes no argument or
/// result register.
fn teardown(cx: &mut FnCx<'_, X86>) {
    put(cx, mc::lea(Size::D, ESP, Mem::base(EBP, -12)));
    for reg in SAVED.into_iter().rev() {
        put(cx, mc::pop_r(reg));
    }
    put(cx, mc::pop_r(EBP));
    cx.cfi(CfiOp::DefCfa { reg: DWARF_ESP, offset: 4 });
}

/// The relocation of a call or tail jump to `sym`: `R_386_PLT32` for a
/// callee that may be in another module, `R_386_PC32` for one in this
/// output (as `llc` writes them in the static model).
fn call_reloc(cx: &FnCx<'_, X86>, sym: SymRef) -> I386Reloc {
    match sym {
        SymRef::Module(id) => match hod_cg::call_access(cx.module, id) {
            hod_cg::CallAccess::Direct => I386Reloc::Pc32,
            hod_cg::CallAccess::Plt => I386Reloc::Plt32,
        },
        _ => I386Reloc::Plt32,
    }
}

/// `Target::frame_base_offset`: from `ebp`.
pub fn base_offset(desc: &FrameDesc, place: FrameRef) -> Option<i64> {
    match place {
        FrameRef::Local(o) => Some(i64::from(o) - i64::from(desc.frame_size)),
        FrameRef::Incoming(n) => Some(i64::from(INCOMING_BASE) + i64::from(n)),
        FrameRef::Outgoing(_) | FrameRef::Coro(_) => None,
    }
}

/// `Target::adjust_sp`: `lea esp, [esp + delta]`, flags kept.
pub fn adjust_sp(cx: &mut FnCx<'_, X86>, delta: i64) {
    const STEP: i64 = 1 << 30;
    let mut left = delta;
    while left != 0 {
        let step = left.clamp(-STEP, STEP);
        put(cx, mc::lea(Size::D, ESP, Mem::base(ESP, step as i32)));
        left -= step;
    }
}

/// An `alloca` the frame does not hold (a dynamic size, an alignment
/// above 16) and `llvm.stacksave`/`stackrestore`: design 0008's I11.
pub const DYNAMIC_ALLOCA: &str = "x86:dynamic-alloca";

/// `Target::dynamic_slot`: refused ([`DYNAMIC_ALLOCA`]).
///
/// # Errors
/// Always.
pub fn dynamic_slot(
    _cx: &mut FnCx<'_, X86>,
    _ptr: FrameRef,
    _size: u64,
    _align: u64,
) -> CgResult<()> {
    unsupported(DYNAMIC_ALLOCA)
}

/// The call instruction (`Target::emit_call`): `call rel32` with
/// `R_386_PLT32` for a callee that may be in another module and
/// `R_386_PC32` for one in this output (as `llc` writes them in the
/// static model), or `call edx`. `callee_pop` is
/// `CallEmit::callee_pop`, the bytes the callee removed
/// (`abi::callee_pop`): `esp` is moved back by them, so that the call
/// sequence frees the whole area it reserved.
///
/// # Errors
/// `Internal` only for an encoder without a relocatable field.
pub fn emit_call(cx: &mut FnCx<'_, X86>, target: CallTarget, callee_pop: u32) -> CgResult<()> {
    call_instruction(cx, target)?;
    // `spec/abi-x86.md` §8.1: the callee popped its hidden pointer.
    if callee_pop != 0 {
        adjust_sp(cx, -i64::from(callee_pop));
    }
    Ok(())
}

/// The call instruction alone.
fn call_instruction(cx: &mut FnCx<'_, X86>, target: CallTarget) -> CgResult<()> {
    match target {
        CallTarget::Sym(sym) => {
            let kind = call_reloc(cx, sym);
            let inst = mc::call(0);
            let Some(site) = Mode::X86.admit(inst).ok().and_then(|i| i.site()) else {
                return cx.bug("a call without a relocatable field");
            };
            let at = cx.offset();
            put(cx, inst);
            cx.reloc(at + site.offset as u32, RelocKind::I386(kind), sym, site.addend);
            Ok(())
        }
        CallTarget::Reg(callee) => {
            put(cx, mc::call_r(gp(callee)));
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `esp` is 16-aligned after the prologue: `4 + 4 + 12 + FRAME`.
    #[test]
    fn frame_size_keeps_the_stack_aligned() {
        for size in 0..100 {
            let frame = final_size(size).unwrap();
            assert!(frame >= size && frame < size + 16, "{size}");
            assert!((20 + frame).is_multiple_of(16), "{size}");
        }
        assert!(final_size(MAX_FRAME + 1).is_err());
    }

    #[test]
    fn the_prologue_immediate_is_the_last_four_bytes() {
        let sub = Mode::X86.admit(mc::sub_ri(Size::D, ESP, 0x7fff_ffff)).unwrap();
        assert_eq!(sub.bytes(), [0x81, 0xec, 0xff, 0xff, 0xff, 0x7f]);
    }
}
