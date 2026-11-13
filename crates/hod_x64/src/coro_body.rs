//! Coroutine bodies on x86-64 (`spec/codegen.md` §7.6 "x86-64", design
//! 0005 §3.2, §3.3 and package C7): the frame base `rbx`, the entry stubs,
//! the suspend dispatch, symmetric transfer and the body intrinsics.
//!
//! **Owner: WP-137.** Tests: `tests/coro*.rs`. Hooked from `frame::begin`
//! (the mode and frame words), `common::frame_mem` and
//! `common::frame_inst_shifted` (`FrameRef::Coro` is `[rbx + o]`, each
//! access reported to the structural check), `target::lower_call` (the
//! body intrinsics) and the `Target` items `CORO_BODIES`, `coro_stubs`,
//! `coro_dispatch`, `coro_transfer`, `reg_info` (a coroutine's pool has
//! no `rbx`).
//!
//! ```text
//! F:          push rbp · mov rbp, rsp · push rbx .. r15 · sub rsp, FRAME
//!             mov qword [rbp + mode], 0 · mov qword [rbp + frame], 0
//!             ...  coro.begin: mov rbx, mem · lea r11, [rip + F.resume] ·
//!                  mov [rbx], r11 · (F.destroy to [rbx + 8]) ·
//!                  mov [rbp + frame], rbx
//!             ...  coro.suspend k: mov eax, -1
//!   .Lk:           mov r<n>d, eax        (a resumed activation enters here)
//!             ...  epilogue: lea rsp, [rbp - 8n] · pops of the save set · pop rbp
//! F.resume:   push rbp · mov rbp, rsp · the save set's pushes · sub rsp, FRAME ·
//!             mov rbx, rdi · mov qword [rbp + mode], 1 ·
//!             mov [rbp + frame], rdi · mov eax, 0 ·
//!             cmp dword [rbx + index], k · je .Lk ... · ud2
//! F.destroy:  the same with mov eax, 1
//! ```
//!
//! `rbx` is in the save set every function pushes and pops (§5.6), so its
//! save, its CFI rule and its restore in every exit and tail exit are the
//! prologue's and the teardown's; a coroutine takes it out of the
//! register pool ([`REG_INFO_CORO`]), which is all reserving it costs.
//! The stubs build the same machine frame as the prologue with the same
//! rows: every activation has CFA = `rbp + 16`, the save set under it,
//! and the return address of whoever called the stub.

use hod_cg::{
    CallArg, CfiOp, CgResult, Class, FnCx, FrameRef, Label, PReg, RegInfo, RegSet, Target,
};
use hod_ir::{CallConv, Inst, Intrinsic, TypeId, Value};
use hod_mc_x64 as mc;
use hod_mc_x64::{Cond, Mem, Reg, Size};

use crate::common::{
    R11, RBP, REL32, RSP, branch_cond, deferred_first, frame_inst, gp, mov_imm, put,
};
use crate::frame::SAVED;
use crate::intrinsic::const_int;
use crate::reg::{DWARF_RBP, DWARF_RSP, dwarf, gpr};
use crate::target::{REG_INFO, X64};

/// The frame base register.
pub const RBX: Reg = Reg::Rbx;

/// The register description of a coroutine (and of a function compiled
/// with the coroutine frame): [`REG_INFO`] without `rbx`, the frame base,
/// in the pool or among the holdable registers.
pub const REG_INFO_CORO: RegInfo = RegInfo {
    pool: without_rbx(REG_INFO.pool),
    holdable: without_rbx(REG_INFO.holdable),
    ..REG_INFO
};

const fn without_rbx(set: RegSet) -> RegSet {
    set.without(crate::reg::RBX)
}

/// What the frame remembers about the coroutine frame.
#[derive(Clone, Copy, Default, Debug)]
pub struct CoroState {
    /// The labels of the two stubs, created by `coro.begin`.
    pub stubs: Option<(Label, Label)>,
}

/// From `frame::begin`, right after the prologue's `sub rsp`: the mode
/// and frame words, both 0 (the frame word stays 0 until `coro.begin`,
/// WP-108's debug obligation). Under `Stress::coro_frame` a function that
/// is not a coroutine then has `rbx` clobbered, so that a missing restore
/// or rule shows (at this point no value is in a register of the pool).
///
/// # Errors
/// What allocating the words reports.
pub fn enter(cx: &mut FnCx<'_, X64>) -> CgResult<()> {
    if !cx.has_coro_frame() {
        return Ok(());
    }
    let words = cx.coro_slots()?;
    frame_inst(cx, words.mode, |m| mc::mov_mi(Size::Q, m, 0));
    if cx.is_coroutine() {
        frame_inst(cx, words.frame, |m| mc::mov_mi(Size::Q, m, 0));
    } else {
        put(cx, mc::mov_rr(Size::Q, RBX, RBP));
    }
    Ok(())
}

/// `Target::reg_info` of a function: [`REG_INFO_CORO`] when it has the
/// coroutine frame.
pub fn reg_info(coro: bool) -> &'static RegInfo {
    if coro { &REG_INFO_CORO } else { &REG_INFO }
}

/// `Target::coro_stubs`: `F.resume` and `F.destroy`, after the epilogue
/// and the tail exits (whose local accesses `frame::end` has already
/// biased: the stubs address their locals with final displacements).
///
/// # Errors
/// What the fixups report; `Internal` for a function without the words.
pub fn stubs(cx: &mut FnCx<'_, X64>) -> CgResult<()> {
    // WP-146: the body's frame, built the same way (`frame::end`).
    let desc = cx.state.frame.desc;
    let frame = desc.frame_size - 8 * crate::frame::saved(cx).count() as u32;
    let words = cx.coro_slots()?;
    let layout = cx.coro_layout()?;
    let local = |place: FrameRef| -> CgResult<Mem> {
        match crate::frame::base_offset(&desc, place).map(i32::try_from) {
            Some(Ok(disp)) => Ok(Mem::base(RBP, disp)),
            _ => Err(hod_cg::CgError::Internal("a coroutine word outside the local area".into())),
        }
    };
    let (mode, word) = (local(words.mode)?, local(words.frame)?);
    let (resume, destroy) = stub_labels(cx);
    let points = cx.coro_reentries();
    let mut at = [0u32; 2];
    for (which, label) in [resume, destroy].into_iter().enumerate() {
        at[which] = cx.offset();
        cx.bind(label)?;
        // The entry state: the CFA is `rsp + 8`, nothing saved.
        cx.cfi(CfiOp::DefCfa { reg: DWARF_RSP, offset: 8 });
        cx.cfi(CfiOp::Restore { reg: DWARF_RBP });
        for &saved in &SAVED {
            cx.cfi(CfiOp::Restore { reg: dwarf(gpr(saved)) });
        }
        // The prologue of `frame::begin`, with its rows.
        put(cx, mc::push_r(RBP));
        cx.cfi(CfiOp::DefCfaOffset(16));
        cx.cfi(CfiOp::Offset { reg: DWARF_RBP, cfa_offset: -16 });
        put(cx, mc::mov_rr(Size::Q, RBP, RSP));
        cx.cfi(CfiOp::DefCfaRegister(DWARF_RBP));
        for (index, saved) in crate::frame::saved(cx).enumerate() {
            put(cx, mc::push_r(saved));
            let cfa_offset = -24 - 8 * index as i32;
            cx.cfi(CfiOp::Offset { reg: dwarf(gpr(saved)), cfa_offset });
        }
        put(cx, mc::sub_ri(Size::Q, RSP, i64::from(frame)));
        // The frame base, the mode, the frame word, the suspend result.
        put(cx, mc::mov_rr(Size::Q, RBX, Reg::Rdi));
        put(cx, mc::mov_mi(Size::Q, mode, 1));
        put(cx, mc::mov_mr(Size::Q, word, Reg::Rdi));
        // WP-108 (debug hook): the stub's `prologue_end`.
        cx.coro_stub_ready();
        put(cx, mc::mov_ri(Size::D, Reg::Rax, which as i64));
        let index = Mem::base(RBX, layout.index as i32);
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
fn stub_labels(cx: &mut FnCx<'_, X64>) -> (Label, Label) {
    if let Some(labels) = cx.state.frame.coro.stubs {
        return labels;
    }
    let labels = (cx.new_label(), cx.new_label());
    cx.state.frame.coro.stubs = Some(labels);
    labels
}

/// `Target::coro_dispatch`: 0 to `resume`, 1 to `destroy` (the `i8`
/// result's low byte).
///
/// # Errors
/// What the fixups report.
pub fn dispatch(cx: &mut FnCx<'_, X64>, cond: PReg, resume: Label, destroy: Label) -> CgResult<()> {
    let r = gp(cond);
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
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    use Intrinsic as I;
    deferred_first(cx, inst, None)?;
    if !cx.is_coroutine() {
        return cx.bug("a coroutine body intrinsic outside a coroutine");
    }
    match (which, args) {
        (I::CoroId, _) => Ok(()),
        (I::CoroAlloc, _) => constant(cx, inst, 1),
        (I::CoroSize, _) => {
            let size = cx.coro_layout()?.size;
            constant(cx, inst, u64::from(size))
        }
        (I::CoroAlign, _) => {
            let align = cx.coro_layout()?.align;
            constant(cx, inst, u64::from(align))
        }
        (I::CoroBegin, &[_, mem]) => begin(cx, mem),
        (I::CoroSave, _) => save_point(cx, inst),
        (I::CoroSuspend, _) => suspend(cx, inst),
        (I::CoroEnd, &[_, unwind, ..]) => end(cx, inst, unwind),
        (I::CoroFree | I::CoroFrame, _) => {
            let rd = cx.def_reg(inst)?;
            put(cx, mc::mov_rr(Size::Q, gp(rd), RBX));
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

/// A constant result.
fn constant<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, value: u64) -> CgResult<()> {
    let rd = cx.def_reg(inst)?;
    let bits = cx.types().int_bits(inst.ty()).unwrap_or(64);
    mov_imm(cx, gp(rd), value, bits);
    cx.finish(inst, rd)
}

/// `Target::coro_transfer` (WP-107, design 0005 §3.3), right after the
/// wrapper of `llvm.coro.await.suspend.handle` returned the handle in
/// `rax`:
///
/// ```text
///     mov r11, [rax]               the handle's resume function
///     mov rdi, rax                 its argument
///     cmp qword [rbp + mode], 0
///     jne exit                     a resumed activation: the tail exit,
///                                  which pops the save set (rbx with it)
///                                  and rbp and ends in `jmp r11`
///     call r11                     the ramp: a call (its own call-site
///                                  row, no landing pad), then the
///                                  suspend's −1 path
/// ```
///
/// Neither path addresses the coroutine's frame: `[rax]` is the other
/// coroutine's. `rsp` is where it was at the wrapper's call: 16-aligned.
///
/// # Errors
/// What the fixups report.
pub fn transfer(cx: &mut FnCx<'_, X64>, exit: Label) -> CgResult<()> {
    put(cx, mc::mov_rm(Size::Q, R11, Mem::base(Reg::Rax, 0)));
    put(cx, mc::mov_rr(Size::Q, Reg::Rdi, Reg::Rax));
    let words = cx.coro_slots()?;
    frame_inst(cx, words.mode, |m| mc::cmp_mi(Size::Q, m, 0));
    branch_cond(cx, Cond::Ne, exit)?;
    let start = cx.offset();
    put(cx, mc::call_r(R11));
    cx.record_call_site(start, cx.offset() - start);
    Ok(())
}

/// The mode word into `rd`: 0 in the ramp, 1 in a resumed activation.
fn load_mode(cx: &mut FnCx<'_, X64>, rd: Reg) -> CgResult<()> {
    let words = cx.coro_slots()?;
    frame_inst(cx, words.mode, |m| mc::mov_rm(Size::D, rd, m));
    Ok(())
}

/// `coro.begin(id, mem)`: `rbx := mem`, the header, the frame word, the
/// copies. The result is the frame word, set by `coro_setup`.
fn begin(cx: &mut FnCx<'_, X64>, mem: Value) -> CgResult<()> {
    let p = gp(cx.use_reg(mem)?);
    put(cx, mc::mov_rr(Size::Q, RBX, p));
    let (resume, destroy) = stub_labels(cx);
    for (label, offset) in [(resume, crate::coro::RESUME), (destroy, crate::coro::DESTROY)] {
        let lea = mc::lea(Size::Q, R11, Mem::rip(0));
        let Some(site) = lea.site() else {
            return cx.bug("a RIP-relative lea without a displacement field");
        };
        let at = cx.offset();
        put(cx, lea);
        cx.fixup(at + site.offset as u32, REL32, label)?;
        put(cx, mc::mov_mr(Size::Q, Mem::base(RBX, offset as i32), R11));
    }
    let words = cx.coro_slots()?;
    frame_inst(cx, words.frame, |m| mc::mov_mr(Size::Q, m, RBX));
    cx.coro_began()
}

/// `coro.save`: the index, and for the final suspend a null resume
/// pointer (design 0005 §3.3: `done()` is true from here on).
fn save_point<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>) -> CgResult<()> {
    let (index, is_final) = cx.coro_save(inst)?;
    let layout = cx.coro_layout()?;
    frame_inst(cx, FrameRef::Coro(layout.index), |m| mc::mov_mi(Size::D, m, i64::from(index)));
    if is_final {
        frame_inst(cx, FrameRef::Coro(crate::coro::RESUME), |m| mc::mov_mi(Size::Q, m, 0));
    }
    cx.coro_saved(inst);
    Ok(())
}

/// `coro.suspend`: −1 now; 0 or 1 when a stub enters at the label.
fn suspend<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>) -> CgResult<()> {
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
fn end<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, unwind: Value) -> CgResult<()> {
    let Some(unwind) = const_int(cx, unwind) else {
        return cx.bug("coro.end with an unwind operand that is not a constant");
    };
    if inst.ty() != TypeId::VOID {
        let rd = cx.def_reg(inst)?;
        load_mode(cx, gp(rd))?;
        cx.finish(inst, rd)?;
    }
    let words = cx.coro_slots()?;
    frame_inst(cx, words.mode, |m| mc::cmp_mi(Size::Q, m, 0));
    if unwind & 1 == 0 {
        return branch_cond(cx, Cond::Ne, cx.epilogue());
    }
    let layout = cx.coro_layout()?;
    let Some(final_index) = layout.final_index else {
        return cx.bug("an unwinding coro.end in a coroutine without a final suspend");
    };
    let skip = cx.new_label();
    branch_cond(cx, Cond::E, skip)?;
    frame_inst(cx, FrameRef::Coro(crate::coro::RESUME), |m| mc::mov_mi(Size::Q, m, 0));
    let index = FrameRef::Coro(layout.index);
    frame_inst(cx, index, |m| mc::mov_mi(Size::D, m, i64::from(final_index)));
    cx.bind(skip)
}

/// The memory operand of heap-frame offset `at`: `[rbx + at]`
/// (`common::frame_mem`'s `FrameRef::Coro` arm).
pub fn heap_mem(at: u32) -> Mem {
    Mem::base(RBX, at as i32)
}

/// Reports a heap access being formed to the structural check
/// (`common::frame_inst_shifted`, for every place).
#[inline]
pub fn access(cx: &mut FnCx<'_, X64>, place: FrameRef) {
    if matches!(place, FrameRef::Coro(_)) {
        cx.coro_access();
    }
}
