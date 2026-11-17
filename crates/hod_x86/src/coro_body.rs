//! Coroutine bodies on i386 (`spec/codegen.md` §7.6, `spec/codegen-x86.md`
//! §18): the frame base `esi`, the entry stubs, the suspend dispatch,
//! symmetric transfer and the body intrinsics. The shape of x86-64's
//! (`hod_x64/src/coro_body.rs`, `rbx` there).
//!
//! **Owner: WP-335** (written in WP-327). Tests:
//! `crates/hod/tests/i386_coro.rs`. Hooked from `frame::begin`
//! ([`enter`]), `frame::end` ([`test_mode`], [`ret_pop`]),
//! `common::frame_mem` and `common::frame_inst` (`FrameRef::Coro` is
//! `[esi + o]`, each access reported to the structural check),
//! `crate::coro` (`call::lower`'s arm for the body intrinsics and
//! `x87::barrier`'s, [`owns`], [`intrinsic`], [`is_barrier`]),
//! `asm::decide` ([`asm_reserved`], [`check_asm`]) and the `Target` items
//! `CORO_BODIES`, `coro_stubs`, `coro_dispatch`, `coro_transfer`,
//! `reg_info` and `for_function` (a coroutine's pool has no `esi`).
//!
//! ```text
//! F:          push ebp · mov ebp, esp · push ebx · push esi · push edi · sub esp, FRAME
//!             mov dword [ebp + mode], 0 · mov dword [ebp + frame], 0
//!             ...  coro.begin: mov esi, mem · call .+0 · pop ecx ·
//!                  lea edx, [ecx + F.resume - 7] · lea edx, [edx + 7] ·
//!                  mov [esi], edx · (F.destroy to [esi + 4]) ·
//!                  mov [ebp + frame], esi
//!             ...  coro.suspend k: mov eax, -1
//!   .Lk:           mov r, eax            (a resumed activation enters here)
//!             ...  epilogue: lea esp, [ebp - 12] · pop edi · pop esi · pop ebx · pop ebp ·
//!                  ret (a ramp that pops: cmp [mode], 0 · ... · jne · ret $n · ret)
//! F.resume:   push ebp · mov ebp, esp · push ebx · push esi · push edi · sub esp, FRAME ·
//!             mov esi, ecx · mov dword [ebp + mode], 1 · mov [ebp + frame], ecx ·
//!             mov eax, 0 · cmp dword [esi + index], k · je .Lk ... · ud2
//! F.destroy:  the same with mov eax, 1
//! ```
//!
//! `F.resume` and `F.destroy` are `fastcc void(ptr)`: the frame comes in
//! `ecx` (LLVM's split clones are called so, `crate::coro`). `esi` is
//! saved by every prologue, so its save, its CFI rule and its restore in
//! every exit and tail exit are the prologue's and the teardown's; a
//! coroutine takes it out of the register pool ([`REG_INFO_CORO`]).

use hod_cg::{
    CallArg, CallTarget, CfiOp, CgError, CgResult, Class, FnCx, FrameRef, Label, PReg, RegInfo,
    Target,
};
use hod_ir::{CallConv, Inst, Intrinsic, TypeId, Value};
use hod_mc_x64 as mc;
use hod_mc_x64::{Cond, Mem, Reg, Size};

use crate::common::{
    EBP, ECX, EDX, ESP, branch_cond, const_int, frame_inst, gp, mov_imm, put, unsupported,
};
use crate::frame::SAVED;
use crate::reg::{DWARF_EBP, DWARF_ESP, dwarf, gpr};
use crate::target::{REG_INFO, X86};

/// The frame base register.
pub const ESI: Reg = Reg::Rsi;

/// Inline assembly in a coroutine that names `esi`, the frame base (an
/// `S` or `{esi}` operand, an `esi` clobber).
pub const ASM_BASE: &str = "x86:coro:asm-base";

/// The register description of a coroutine: [`REG_INFO`] without `esi`,
/// the frame base, in the pool or among the holdable registers.
pub const REG_INFO_CORO: RegInfo = RegInfo {
    pool: REG_INFO.pool.without(crate::reg::ESI),
    holdable: REG_INFO.holdable.without(crate::reg::ESI),
    ..REG_INFO
};

/// What the function remembers about the coroutine frame.
#[derive(Clone, Copy, Default, Debug)]
pub struct CoroState {
    /// The labels of the two stubs, created at the first need.
    pub stubs: Option<(Label, Label)>,
}

/// `Target::reg_info` of a function: [`REG_INFO_CORO`] in a coroutine.
#[must_use]
pub fn reg_info(coro: bool) -> &'static RegInfo {
    if coro { &REG_INFO_CORO } else { &REG_INFO }
}

/// Whether `which` is a body intrinsic of this module.
#[must_use]
pub fn owns(which: Intrinsic<'_>) -> bool {
    use Intrinsic as I;
    matches!(
        which,
        I::CoroId
            | I::CoroAlloc
            | I::CoroSize
            | I::CoroAlign
            | I::CoroBegin
            | I::CoroSave
            | I::CoroSuspend
            | I::CoroEnd
            | I::CoroFree
            | I::CoroFrame
            | I::CoroIsInRamp
            | I::CoroAwaitSuspendVoid
            | I::CoroAwaitSuspendBool
            | I::CoroAwaitSuspendHandle
    )
}

/// The body's part of `x87::barrier`'s arm (through `crate::coro::is_barrier`,
/// `spec/codegen-x86.md` §11, §18): no 80-bit copy lives across a suspend
/// (a resumed activation's machine stack does not hold the copy's slot)
/// nor across a call `llc` makes of a body intrinsic: `coro.suspend` (a
/// function boundary in LLVM's split clones) and the `await.suspend`
/// wrappers (calls).
#[must_use]
pub fn is_barrier(which: Intrinsic<'_>) -> bool {
    use Intrinsic as I;
    matches!(
        which,
        I::CoroSuspend
            | I::CoroAwaitSuspendVoid
            | I::CoroAwaitSuspendBool
            | I::CoroAwaitSuspendHandle
    )
}

/// `asm::decide`'s registers taken from the start: `esi` in a coroutine,
/// so that no operand is given it.
#[must_use]
pub fn asm_reserved(cx: &FnCx<'_, X86>) -> u64 {
    if cx.is_coroutine() { 1 << crate::reg::ESI.index() } else { 0 }
}

/// `asm::decide`'s last check: an `asm` of a coroutine that writes `esi`
/// (`written`, a mask of register numbers) is [`ASM_BASE`].
///
/// # Errors
/// [`ASM_BASE`].
pub fn check_asm(cx: &FnCx<'_, X86>, written: u64) -> CgResult<()> {
    if cx.is_coroutine() && written & (1 << crate::reg::ESI.index()) != 0 {
        return unsupported(ASM_BASE);
    }
    Ok(())
}

/// From `frame::begin`, right after the prologue's `sub esp`: the mode
/// and frame words, both 0 (the frame word stays 0 until `coro.begin`,
/// WP-108's debug obligation). Under `Stress::coro_frame` a function that
/// is not a coroutine then has `esi` clobbered, so that a missing restore
/// or rule shows (no value is in a register of the pool at this point).
///
/// # Errors
/// What allocating the words reports.
pub fn enter(cx: &mut FnCx<'_, X86>) -> CgResult<()> {
    if !cx.has_coro_frame() {
        return Ok(());
    }
    let words = cx.coro_slots()?;
    frame_inst(cx, words.mode, |m| mc::mov_mi(Size::D, m, 0));
    if cx.is_coroutine() {
        frame_inst(cx, words.frame, |m| mc::mov_mi(Size::D, m, 0));
    } else {
        put(cx, mc::mov_rr(Size::D, ESI, EBP));
    }
    Ok(())
}

/// From `frame::end`, before the teardown of a ramp that pops `pop`
/// bytes (`sret`: `ret $4`): `cmp dword [ebp + mode], 0`, and `true`,
/// in a coroutine. The teardown keeps the flags.
///
/// # Errors
/// What allocating the words reports.
pub fn test_mode(cx: &mut FnCx<'_, X86>, pop: u32) -> CgResult<bool> {
    if pop == 0 || !cx.is_coroutine() {
        return Ok(false);
    }
    let words = cx.coro_slots()?;
    frame_inst(cx, words.mode, |m| mc::cmp_mi(Size::D, m, 0));
    Ok(true)
}

/// `frame::end`'s return of a function that pops `pop` bytes: `ret $pop`;
/// after [`test_mode`] (`resumed`), a stub-entered activation, called
/// `fastcc` with no stack argument, returns with `ret`: `jne` over the
/// `ret $pop`.
///
/// # Errors
/// What the fixups report.
pub fn ret_pop(cx: &mut FnCx<'_, X86>, pop: u16, resumed: bool) -> CgResult<()> {
    if !resumed {
        put(cx, mc::ret_imm(pop));
        return Ok(());
    }
    let plain = cx.new_label();
    branch_cond(cx, Cond::Ne, plain)?;
    put(cx, mc::ret_imm(pop));
    cx.bind(plain)?;
    put(cx, mc::ret());
    Ok(())
}

/// `Target::coro_stubs`: `F.resume` and `F.destroy`, after the epilogue
/// and the tail exits (whose local accesses `frame::end` has already
/// biased: the stubs address their locals with final displacements).
///
/// # Errors
/// What the fixups report; `Internal` for a function without the words.
pub fn stubs(cx: &mut FnCx<'_, X86>) -> CgResult<()> {
    let frame = crate::frame::final_size(cx.frame_size())?;
    let below = 12 + i64::from(frame);
    let words = cx.coro_slots()?;
    let layout = cx.coro_layout()?;
    let local = |place: FrameRef| -> CgResult<Mem> {
        match place {
            FrameRef::Local(o) => Ok(Mem::base(EBP, (i64::from(o) - below) as i32)),
            _ => Err(CgError::Internal("a coroutine word outside the local area".into())),
        }
    };
    let (mode, word) = (local(words.mode)?, local(words.frame)?);
    let (resume, destroy) = stub_labels(cx);
    let points = cx.coro_reentries();
    let mut at = [0u32; 2];
    for (which, label) in [resume, destroy].into_iter().enumerate() {
        at[which] = cx.offset();
        cx.bind(label)?;
        // The entry state: the CFA is `esp + 4`, nothing saved.
        cx.cfi(CfiOp::DefCfa { reg: DWARF_ESP, offset: 4 });
        cx.cfi(CfiOp::Restore { reg: DWARF_EBP });
        for saved in SAVED {
            cx.cfi(CfiOp::Restore { reg: dwarf(gpr(saved)) });
        }
        // WP-324, §19.3: called through a pointer, as `llc`'s
        // `F.resume` and `F.destroy` begin.
        crate::cet::landing(cx);
        // The prologue of `frame::begin`, with its rows.
        put(cx, mc::push_r(EBP));
        cx.cfi(CfiOp::DefCfaOffset(8));
        cx.cfi(CfiOp::Offset { reg: DWARF_EBP, cfa_offset: -8 });
        put(cx, mc::mov_rr(Size::D, EBP, ESP));
        cx.cfi(CfiOp::DefCfaRegister(DWARF_EBP));
        for saved in SAVED {
            put(cx, mc::push_r(saved));
        }
        for (index, saved) in SAVED.into_iter().enumerate() {
            let cfa_offset = -12 - 4 * index as i32;
            cx.cfi(CfiOp::Offset { reg: dwarf(gpr(saved)), cfa_offset });
        }
        put(cx, mc::sub_ri(Size::D, ESP, i64::from(frame)));
        // The frame base, the mode, the frame word, the suspend result.
        put(cx, mc::mov_rr(Size::D, ESI, ECX));
        put(cx, mc::mov_mi(Size::D, mode, 1));
        put(cx, mc::mov_mr(Size::D, word, ECX));
        // WP-324, §19.2: this activation's guard and guarded block.
        crate::ssp::stub_enter(cx, local)?;
        // WP-108 (debug hook): the stub's `prologue_end`.
        cx.coro_stub_ready();
        mov_imm(cx, Reg::Rax, which as u32);
        let index = Mem::base(ESI, layout.index as i32);
        for &(point, label) in &points {
            put(cx, mc::cmp_mi(Size::D, index, i64::from(point)));
            branch_cond(cx, Cond::E, label)?;
        }
        // An index no suspend stored: a frame that is not this
        // coroutine's.
        put(cx, mc::ud2());
    }
    let end = cx.offset();
    cx.set_coro_entries(at[0], at[1], end);
    Ok(())
}

/// The two stubs' labels, created at the first need.
fn stub_labels(cx: &mut FnCx<'_, X86>) -> (Label, Label) {
    if let Some(labels) = cx.state.coro.stubs {
        return labels;
    }
    let labels = (cx.new_label(), cx.new_label());
    cx.state.coro.stubs = Some(labels);
    labels
}

/// `Target::coro_dispatch`: 0 to `resume`, 1 to `destroy` (the `i8`
/// result's low byte, through `cl` when `cond` has none).
///
/// # Errors
/// What the fixups report.
pub fn dispatch(cx: &mut FnCx<'_, X86>, cond: PReg, resume: Label, destroy: Label) -> CgResult<()> {
    let mut r = gp(cond);
    if !crate::reg::has_low_byte(r) {
        put(cx, mc::mov_rr(Size::D, ECX, r));
        r = ECX;
    }
    put(cx, mc::cmp_ri(Size::B, r, 0));
    branch_cond(cx, Cond::E, resume)?;
    put(cx, mc::cmp_ri(Size::B, r, 1));
    branch_cond(cx, Cond::E, destroy)
}

/// A body intrinsic of a coroutine.
///
/// # Errors
/// What the call sequence reports; `Internal` for an argument list the
/// verifier rejects or an intrinsic outside a coroutine.
pub fn intrinsic<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    use Intrinsic as I;
    if !cx.is_coroutine() {
        return cx.bug("a coroutine body intrinsic outside a coroutine");
    }
    match (which, args) {
        (I::CoroId, _) => Ok(()),
        (I::CoroAlloc, _) => constant(cx, inst, 1),
        (I::CoroSize, _) => {
            let size = cx.coro_layout()?.size;
            constant(cx, inst, size)
        }
        (I::CoroAlign, _) => {
            let align = cx.coro_layout()?.align;
            constant(cx, inst, align)
        }
        (I::CoroBegin, &[_, mem]) => begin(cx, mem),
        (I::CoroSave, _) => save_point(cx, inst),
        (I::CoroSuspend, _) => suspend(cx, inst),
        (I::CoroEnd, &[_, unwind, ..]) => end(cx, inst, unwind),
        (I::CoroFree | I::CoroFrame, _) => {
            let rd = cx.def_reg(inst)?;
            put(cx, mc::mov_rr(Size::D, gp(rd), ESI));
            cx.finish(inst, rd)
        }
        (I::CoroIsInRamp, _) => {
            let rd = cx.def_reg(inst)?;
            load_mode(cx, gp(rd))?;
            put(cx, mc::xor_ri(Size::D, gp(rd), 1));
            cx.finish(inst, rd)
        }
        // The handle form's transfer follows the wrapper's call, from the
        // call sequence (`transfer`).
        (
            I::CoroAwaitSuspendVoid | I::CoroAwaitSuspendBool | I::CoroAwaitSuspendHandle,
            &[awaiter, frame, wrapper],
        ) => {
            let callee = cx.temp(Class::Gpr)?;
            let t = cx.target;
            t.materialize(cx, callee, wrapper)?;
            let args = [CallArg::Value(awaiter), CallArg::Value(frame)];
            cx.call_through(inst, callee, &args, &[TypeId::PTR, TypeId::PTR], CallConv::C)
        }
        _ => cx.bug("a coroutine body intrinsic with unexpected arguments"),
    }
}

/// A constant result: `i1`, `i32`, or an `i64` (`coro.size.i64`,
/// `coro.align.i64` of a module written for a 64-bit target), whose two
/// halves are stored to its home (`spec/codegen-x86.md` §8).
fn constant<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, value: u32) -> CgResult<()> {
    if cx.class_of(inst.result()) == Class::Mem {
        let home = cx.def_mem(inst)?;
        frame_inst(cx, home, |m| mc::mov_mi(Size::D, m, i64::from(value)));
        frame_inst(cx, home.offset_by(4), |m| mc::mov_mi(Size::D, m, 0));
        return Ok(());
    }
    let rd = cx.def_reg(inst)?;
    mov_imm(cx, gp(rd), value);
    cx.finish(inst, rd)
}

/// `Target::coro_transfer` (WP-107, design 0005 §3.3), right after the
/// wrapper of `llvm.coro.await.suspend.handle` returned the handle in
/// `eax`:
///
/// ```text
///     mov edx, [eax]               the handle's resume function
///     mov ecx, eax                 its argument (`fastcc`)
///     cmp dword [ebp + mode], 0
///     jne exit                     a resumed activation: the tail exit,
///                                  which pops the saved registers (esi
///                                  with them) and ebp and ends in `jmp edx`
///     call edx                     the ramp: a call (its own call-site
///                                  row, no landing pad), then the
///                                  suspend's −1 path
/// ```
///
/// Neither path addresses the coroutine's frame: `[eax]` is the other
/// coroutine's. `esp` is where it was at the wrapper's call: 16-aligned.
///
/// # Errors
/// What the fixups report.
pub fn transfer(cx: &mut FnCx<'_, X86>, exit: Label) -> CgResult<()> {
    put(cx, mc::mov_rm(Size::D, EDX, Mem::base(Reg::Rax, 0)));
    put(cx, mc::mov_rr(Size::D, ECX, Reg::Rax));
    let words = cx.coro_slots()?;
    frame_inst(cx, words.mode, |m| mc::cmp_mi(Size::D, m, 0));
    branch_cond(cx, Cond::Ne, exit)?;
    let start = cx.offset();
    crate::pic::emit_call(cx, CallTarget::Reg(crate::reg::EDX), 0)?;
    cx.record_call_site(start, cx.offset() - start);
    Ok(())
}

/// The mode word into `rd`: 0 in the ramp, 1 in a resumed activation.
fn load_mode(cx: &mut FnCx<'_, X86>, rd: Reg) -> CgResult<()> {
    let words = cx.coro_slots()?;
    frame_inst(cx, words.mode, |m| mc::mov_rm(Size::D, rd, m));
    Ok(())
}

/// `coro.begin(id, mem)`: `esi := mem`, the header, the frame word, the
/// copies. The stubs' addresses are formed without a relocation, in
/// every model: `call .+0` · `pop ecx` (`ecx` := the `pop`'s address),
/// then per stub `lea edx, [ecx + stub - lea end]` (a `REL32` label
/// fixup counts from the end of its instruction) and `lea edx, [edx +
/// lea end - pop]`. The result is the frame word, set by `coro_setup`.
fn begin(cx: &mut FnCx<'_, X86>, mem: Value) -> CgResult<()> {
    let p = gp(cx.use_reg(mem)?);
    put(cx, mc::mov_rr(Size::D, ESI, p));
    put(cx, mc::call(0));
    let pop = cx.offset();
    put(cx, mc::pop_r(ECX));
    let (resume, destroy) = stub_labels(cx);
    for (label, offset) in [(resume, crate::coro::RESUME), (destroy, crate::coro::DESTROY)] {
        let m = Mem::base(ECX, 0);
        let lea = mc::lea(Size::D, EDX, m.with_abs32().unwrap_or(m));
        let Some(site) = mc::Mode::X86.admit(lea).ok().and_then(|i| i.site()) else {
            return cx.bug("a lea without a displacement field");
        };
        let at = cx.offset();
        put(cx, lea);
        cx.fixup(at + site.offset as u32, crate::common::REL32, label)?;
        let back = (cx.offset() - pop) as i32;
        put(cx, mc::lea(Size::D, EDX, Mem::base(EDX, back)));
        put(cx, mc::mov_mr(Size::D, Mem::base(ESI, offset as i32), EDX));
    }
    let words = cx.coro_slots()?;
    frame_inst(cx, words.frame, |m| mc::mov_mr(Size::D, m, ESI));
    cx.coro_began()
}

/// `coro.save`: the index, and for the final suspend a null resume
/// pointer (design 0005 §3.3: `done()` is true from here on).
fn save_point<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>) -> CgResult<()> {
    let (index, is_final) = cx.coro_save(inst)?;
    let layout = cx.coro_layout()?;
    frame_inst(cx, FrameRef::Coro(layout.index), |m| mc::mov_mi(Size::D, m, i64::from(index)));
    if is_final {
        frame_inst(cx, FrameRef::Coro(crate::coro::RESUME), |m| mc::mov_mi(Size::D, m, 0));
    }
    cx.coro_saved(inst);
    Ok(())
}

/// `coro.suspend`: −1 now; 0 or 1 when a stub enters at the label.
fn suspend<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>) -> CgResult<()> {
    let label = cx.coro_reentry(inst)?;
    put(cx, mc::mov_ri(Size::D, Reg::Rax, -1));
    cx.bind(label)?;
    let rd = cx.def_reg(inst)?;
    put(cx, mc::mov_rr(Size::D, gp(rd), Reg::Rax));
    cx.finish(inst, rd)
}

/// `coro.end(frame, unwind, token)`. Not unwinding: a resumed activation
/// returns (the epilogue); the ramp goes on. Unwinding: a resumed
/// activation marks the coroutine done at its final suspend. LLVM 19's
/// `i1` result is the mode: true when not in the ramp.
fn end<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, unwind: Value) -> CgResult<()> {
    let Some(unwind) = const_int(cx, unwind) else {
        return cx.bug("coro.end with an unwind operand that is not a constant");
    };
    if inst.ty() != TypeId::VOID {
        let rd = cx.def_reg(inst)?;
        load_mode(cx, gp(rd))?;
        cx.finish(inst, rd)?;
    }
    let words = cx.coro_slots()?;
    frame_inst(cx, words.mode, |m| mc::cmp_mi(Size::D, m, 0));
    if unwind & 1 == 0 {
        return branch_cond(cx, Cond::Ne, cx.epilogue());
    }
    let layout = cx.coro_layout()?;
    let Some(final_index) = layout.final_index else {
        return cx.bug("an unwinding coro.end in a coroutine without a final suspend");
    };
    let skip = cx.new_label();
    branch_cond(cx, Cond::E, skip)?;
    frame_inst(cx, FrameRef::Coro(crate::coro::RESUME), |m| mc::mov_mi(Size::D, m, 0));
    let index = FrameRef::Coro(layout.index);
    frame_inst(cx, index, |m| mc::mov_mi(Size::D, m, i64::from(final_index)));
    cx.bind(skip)
}

/// The memory operand of heap-frame offset `at`: `[esi + at]`
/// (`common::frame_mem`'s `FrameRef::Coro` arm).
#[must_use]
pub fn heap_mem(at: u32) -> Mem {
    Mem::base(ESI, at as i32)
}

/// Reports a heap access being formed to the structural check
/// (`common::frame_inst`, for every place).
#[inline]
pub fn access(cx: &mut FnCx<'_, X86>, place: FrameRef) {
    if matches!(place, FrameRef::Coro(_)) {
        cx.coro_access();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_coroutine_pool_has_no_esi() {
        REG_INFO_CORO.check().unwrap();
        assert!(!REG_INFO_CORO.pool.contains(crate::reg::ESI));
        assert!(!REG_INFO_CORO.holdable.contains(crate::reg::ESI));
        assert!(REG_INFO_CORO.pool.contains(crate::reg::EBX));
        assert!(REG_INFO_CORO.pool.contains(crate::reg::EDI));
        assert_eq!(REG_INFO_CORO.pool.intersection(REG_INFO_CORO.gpr).len(), 2);
    }

    /// [`begin`]'s `lea` keeps a 32-bit displacement field last, which
    /// the `REL32` label fixup counts from the end of.
    #[test]
    fn the_stub_address_lea_ends_in_its_field() {
        let m = Mem::base(ECX, 0).with_abs32().unwrap();
        let lea = mc::Mode::X86.admit(mc::lea(Size::D, EDX, m)).unwrap();
        assert_eq!(lea.bytes().len(), 6);
        assert_eq!(lea.site().map(|s| s.offset), Some(2));
    }

    /// `spec/codegen-x86.md` §18.
    #[test]
    fn suspends_and_calls_are_x87_barriers() {
        use Intrinsic as I;
        for which in [I::CoroSuspend, I::CoroAwaitSuspendHandle, I::CoroResume, I::CoroDestroy] {
            assert!(crate::coro::is_barrier(which));
        }
        for which in [I::CoroSave, I::CoroBegin, I::CoroEnd, I::CoroSize, I::CoroFree] {
            assert!(!crate::coro::is_barrier(which));
        }
    }
}
