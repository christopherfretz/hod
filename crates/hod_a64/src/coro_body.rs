//! Coroutine bodies on AArch64 (`spec/codegen.md` §7.6, design 0005 §3.2
//! and §3.3): the frame base `x19`, the entry stubs, the suspend dispatch
//! and the body intrinsics.
//!
//! **Owner: WP-106.** Tests: `tests/coro_body*.rs`. Hooked from
//! `frame` (the save in the prologue, its rows, its restore in every
//! exit), `common::Cursor::frame` (`FrameRef::Coro` is `[x19, #o]`),
//! `intrinsic::intrinsic` (one arm) and `Target::coro_stubs` /
//! `Target::coro_dispatch`.
//!
//! ```text
//! F:          prologue · str x19, [x29, #save] · str xzr, [x29, #mode]
//!             ...  coro.begin: mov x19, mem · adr/str the stub addresses
//!                  to [x19, #0] and [x19, #8] · str x19, [x29, #frame]
//!             ...  coro.suspend k: mov w0, #-1
//!   .Lk:           mov w<n>, w0           (a resumed activation enters here)
//!             ...  epilogue: ldr x19, [x29, #save] · ...
//! F.resume:   the prologue · str x19, [x29, #save] · mov x19, x0 ·
//!             mode := 1 · str x0, [x29, #frame] · mov w0, #0 ·
//!             ldr w16, [x19, #index] · cmp w16, #k · b.eq .Lk ... · brk
//! F.destroy:  the same with mov w0, #1
//! ```
//!
//! `x19` is callee-saved and outside the register pool (x9-x14), so
//! reserving it costs no register; every function with the coroutine
//! frame saves it below the frame record and states its rule in the CFI
//! of the prologue, of each tail exit and of each stub.

use hod_cg::{CallArg, CfiOp, CgResult, Class, FnCx, FrameRef, Label, Target};
use hod_ir::{CallConv, Inst, Intrinsic, TypeId, Value};
use hod_mc_a64 as mc;
use hod_mc_a64::{Cond, Load, MemSize, Reg, Size};

use crate::common::{
    ADR21, Cursor, FP, SP, X16, X17, branch, branch_cond, branch_zero, deferred_first, load_gpr_at,
    mov_imm, put, store_gpr_at, unsupported, xr,
};
use crate::frame::{DWARF_FP, DWARF_LR, DWARF_SP};
use crate::intrinsic::const_int;
use crate::target::A64;

/// A coroutine (or a function with the coroutine frame) whose calling
/// convention saves registers of its own (`preserve_most`,
/// `preserve_all`): its save areas are not laid out with `x19`'s.
pub const PRESERVE_CC: &str = "a64:coro-preserve-cc";

/// The frame base register.
pub const X19: Reg = Reg::X19;
/// Its DWARF number.
pub const DWARF_X19: u16 = 19;

/// What the frame remembers about the coroutine frame.
#[derive(Clone, Copy, Default, Debug)]
pub struct CoroState {
    /// The local-area offset of `x19`'s save slot.
    pub save: Option<u32>,
    /// The code offset right after the prologue's save.
    pub saved_at: u32,
    /// The labels of the two stubs, created by `coro.begin`.
    pub stubs: Option<(Label, Label)>,
}

/// From `frame::begin`, after the other register saves: `x19`'s save
/// slot and the mode and frame words, `x19` saved and the mode set to 0.
/// Under `Stress::coro_frame`, `x19` is then clobbered, so that a missing
/// restore or rule shows.
///
/// # Errors
/// [`PRESERVE_CC`]; what allocating the slots reports.
pub fn save(cx: &mut FnCx<'_, A64>) -> CgResult<()> {
    if !cx.has_coro_frame() {
        return Ok(());
    }
    let cc = match cx.module.symbol(cx.body.symbol()).kind() {
        hod_ir::SymbolKind::Function { cc, .. } => cc,
        _ => CallConv::C,
    };
    if cc == CallConv::PRESERVE_MOST || cc == CallConv::PRESERVE_ALL {
        return unsupported(PRESERVE_CC);
    }
    let FrameRef::Local(slot) = cx.alloc_slot(8, 8)? else {
        return cx.bug("a frame slot outside the local area");
    };
    let words = cx.coro_slots()?;
    put(cx, mc::str_off(MemSize::X, X19.zr(), FP, i64::from(slot)));
    cx.state.frame.coro.save = Some(slot);
    cx.state.frame.coro.saved_at = cx.offset();
    let mut at = Cursor::frame(cx, words.mode, X16);
    store_gpr_at(cx, MemSize::X, mc::RegZr::ZR, &mut at);
    // WP-108 (debug hook): the frame word is 0 until `coro.begin`, so
    // that a variable's location can tell the ramp before the heap frame
    // exists from every later point (`hod_dwarf::CoroPlace`).
    if cx.is_coroutine() {
        let mut word = Cursor::frame(cx, words.frame, X16);
        store_gpr_at(cx, MemSize::X, mc::RegZr::ZR, &mut word);
    }
    if !cx.is_coroutine() {
        put(cx, mc::mov_sp(Size::X, X19.sp(), FP));
    }
    Ok(())
}

/// The rule for `x19`, from `at` in `end` or from the current offset.
///
/// # Errors
/// What [`FnCx::cfi_at`] reports.
pub fn rows(cx: &mut FnCx<'_, A64>, at: Option<u32>, frame: u32) -> CgResult<()> {
    let Some(slot) = cx.state.frame.coro.save else { return Ok(()) };
    let op = CfiOp::Offset { reg: DWARF_X19, cfa_offset: slot as i32 - frame as i32 };
    if let Some(at) = at {
        return cx.cfi_at(at, op);
    }
    cx.cfi(op);
    Ok(())
}

/// From `frame::teardown`, before the frame record is reloaded: `x19`
/// back.
pub fn restore(cx: &mut FnCx<'_, A64>) {
    let Some(slot) = cx.state.frame.coro.save else { return };
    put(cx, mc::ldr_off(Load::X, X19.zr(), FP, i64::from(slot)));
    cx.cfi(CfiOp::Restore { reg: DWARF_X19 });
}

/// `Target::coro_stubs`: `F.resume` and `F.destroy`.
///
/// # Errors
/// What the fixups report; `Internal` for a function without the frame.
pub fn stubs(cx: &mut FnCx<'_, A64>) -> CgResult<()> {
    let frame = crate::frame::final_size(cx)?;
    let Some(save) = cx.state.frame.coro.save else {
        return cx.bug("coroutine stubs without the coroutine frame");
    };
    let words = cx.coro_slots()?;
    let layout = cx.coro_layout()?;
    let (resume, destroy) = stub_labels(cx);
    let points = cx.coro_reentries();
    let start = cx.offset();
    let mut at = [0u32; 2];
    for (which, label) in [resume, destroy].into_iter().enumerate() {
        at[which] = cx.offset();
        cx.bind(label)?;
        // The entry state: nothing saved, the CFA is the stack pointer.
        cx.cfi(CfiOp::DefCfa { reg: DWARF_SP, offset: 0 });
        cx.cfi(CfiOp::Restore { reg: DWARF_FP });
        cx.cfi(CfiOp::Restore { reg: DWARF_LR });
        cx.cfi(CfiOp::Restore { reg: DWARF_X19 });
        crate::protect::stub_entry(cx); // WP-185
        // The prologue, as `frame::begin` emitted and `frame::end`
        // patched it, with the same rows.
        crate::probe::put_allocation(cx, frame); // WP-186: the two `sub`s
        put(cx, mc::stp_off(Size::X, Reg::X29.zr(), Reg::X30.zr(), SP, 0));
        let f = frame as i32;
        cx.cfi(CfiOp::Offset { reg: DWARF_FP, cfa_offset: -f });
        cx.cfi(CfiOp::Offset { reg: DWARF_LR, cfa_offset: -f + 8 });
        put(cx, mc::mov_sp(Size::X, FP, SP));
        cx.cfi(CfiOp::DefCfaRegister(DWARF_FP));
        put(cx, mc::str_off(MemSize::X, X19.zr(), FP, i64::from(save)));
        rows(cx, None, frame)?;
        // The frame base, the mode, the frame word, the suspend result.
        put(cx, mc::mov_sp(Size::X, X19.sp(), Reg::X0.sp()));
        mov_imm(cx, Size::X, X16, 1);
        let mut mode = Cursor::frame(cx, words.mode, X17);
        store_gpr_at(cx, MemSize::X, X16.zr(), &mut mode);
        let mut word = Cursor::frame(cx, words.frame, X17);
        store_gpr_at(cx, MemSize::X, Reg::X0.zr(), &mut word);
        // WP-183, §5.6.24: a protected coroutine's guard, again.
        crate::guard::stub_enter(cx)?;
        // WP-108 (debug hook): the stub's `prologue_end`.
        cx.coro_stub_ready();
        mov_imm(cx, Size::W, Reg::X0, which as u64);
        let mut index = Cursor::at_reg(X19, i64::from(layout.index), X17);
        load_gpr_at(cx, Load::W, X16, &mut index);
        for &(point, label) in &points {
            compare(cx, X16, point);
            branch_cond(cx, Cond::Eq, label)?;
        }
        // An index no suspend stored: a frame that is not this
        // coroutine's.
        put(cx, mc::brk(1));
    }
    let end = cx.offset();
    debug_assert!(start == at[0], "the resume stub is first");
    cx.set_coro_entries(at[0], at[1], end);
    Ok(())
}

/// The two stubs' labels, created at the first need.
fn stub_labels(cx: &mut FnCx<'_, A64>) -> (Label, Label) {
    if let Some(labels) = cx.state.frame.coro.stubs {
        return labels;
    }
    let labels = (cx.new_label(), cx.new_label());
    cx.state.frame.coro.stubs = Some(labels);
    labels
}

/// `cmp w<rt>, #value` (through `x17` above the immediate's range).
fn compare(cx: &mut FnCx<'_, A64>, rt: Reg, value: u32) {
    if let Some(word) = mc::try_cmp_imm(Size::W, rt.sp(), u64::from(value)) {
        put(cx, word);
    } else {
        mov_imm(cx, Size::W, X17, u64::from(value));
        put(cx, mc::cmp_shifted(Size::W, rt.zr(), X17.zr(), mc::Shift::Lsl, 0));
    }
}

/// `Target::coro_dispatch`: 0 to `resume`, 1 to `destroy`.
///
/// # Errors
/// What the fixups report.
pub fn dispatch(
    cx: &mut FnCx<'_, A64>,
    cond: hod_cg::PReg,
    resume: Label,
    destroy: Label,
) -> CgResult<()> {
    let rt = xr(cond);
    compare(cx, rt, 0);
    branch_cond(cx, Cond::Eq, resume)?;
    compare(cx, rt, 1);
    branch_cond(cx, Cond::Eq, destroy)
}

/// A body intrinsic of a coroutine.
///
/// # Errors
/// What the call sequence reports; `Internal` for an argument list the
/// verifier rejects or an intrinsic outside a coroutine.
pub fn intrinsic<'a>(
    cx: &mut FnCx<'a, A64>,
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
            put(cx, mc::mov_sp(Size::X, xr(rd).sp(), X19.sp()));
            cx.finish(inst, rd)
        }
        (I::CoroIsInRamp, _) => {
            let rd = cx.def_reg(inst)?;
            load_mode(cx, xr(rd))?;
            put(cx, mc::eor_imm(Size::W, xr(rd).sp(), xr(rd).zr(), 1));
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
fn constant<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, value: u64) -> CgResult<()> {
    let rd = cx.def_reg(inst)?;
    mov_imm(cx, Size::X, xr(rd), value);
    cx.finish(inst, rd)
}

/// `Target::coro_transfer` (WP-107, design 0005 §3.3), right after the
/// wrapper of `llvm.coro.await.suspend.handle` returned the handle in
/// `x0`:
///
/// ```text
///     ldr x16, [x0]            the handle's resume function
///     ldr w17, [x29, #mode]
///     cbnz w17, exit           a resumed activation: the tail exit,
///                              which restores x19, x29, x30, releases
///                              the frame and ends in `br x16`
///     blr x16                  the ramp: a call (its own call-site row,
///                              no landing pad), then the suspend's −1
///                              path
/// ```
///
/// Neither path addresses the coroutine's frame: `[x0]` is the other
/// coroutine's.
///
/// # Errors
/// What the fixups report.
pub fn transfer(cx: &mut FnCx<'_, A64>, exit: Label) -> CgResult<()> {
    put(cx, mc::ldr_off(Load::X, X16.zr(), Reg::X0.sp(), 0));
    load_mode(cx, X17)?;
    branch_zero(cx, Size::W, X17, true, exit)?;
    let start = cx.offset();
    put(cx, mc::blr(X16.zr()));
    cx.record_call_site(start, 4);
    Ok(())
}

/// The mode word into `rd`: 0 in the ramp, 1 in a resumed activation.
fn load_mode(cx: &mut FnCx<'_, A64>, rd: Reg) -> CgResult<()> {
    let words = cx.coro_slots()?;
    let mut at = Cursor::frame(cx, words.mode, X17);
    load_gpr_at(cx, Load::W, rd, &mut at);
    Ok(())
}

/// `coro.begin(id, mem)`: `x19 := mem`, the header, the frame word, the
/// copies. The result is the frame word, set by `coro_setup`.
fn begin(cx: &mut FnCx<'_, A64>, mem: Value) -> CgResult<()> {
    let p = cx.use_reg(mem)?;
    put(cx, mc::mov_sp(Size::X, X19.sp(), xr(p).sp()));
    let (resume, destroy) = stub_labels(cx);
    for (label, offset) in [(resume, crate::coro::RESUME), (destroy, crate::coro::DESTROY)] {
        let at = cx.offset();
        put(cx, mc::adr(X16.zr(), 0));
        cx.fixup(at, ADR21, label)?;
        put(cx, mc::str_off(MemSize::X, X16.zr(), X19.sp(), i64::from(offset)));
    }
    let words = cx.coro_slots()?;
    let mut at = Cursor::frame(cx, words.frame, X17);
    store_gpr_at(cx, MemSize::X, X19.zr(), &mut at);
    cx.coro_began()
}

/// `coro.save`: the index, and for the final suspend a null resume
/// pointer (design 0005 §3.3: `done()` is true from here on).
fn save_point<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>) -> CgResult<()> {
    let (index, is_final) = cx.coro_save(inst)?;
    let layout = cx.coro_layout()?;
    mov_imm(cx, Size::W, X16, u64::from(index));
    let mut at = Cursor::frame(cx, FrameRef::Coro(layout.index), X17);
    store_gpr_at(cx, MemSize::W, X16.zr(), &mut at);
    if is_final {
        let mut at = Cursor::frame(cx, FrameRef::Coro(crate::coro::RESUME), X17);
        store_gpr_at(cx, MemSize::X, mc::RegZr::ZR, &mut at);
    }
    cx.coro_saved(inst);
    Ok(())
}

/// `coro.suspend`: −1 now; 0 or 1 when a stub enters at the label.
fn suspend<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>) -> CgResult<()> {
    let label = cx.coro_reentry(inst)?;
    put(cx, mc::movn(Size::W, Reg::X0.zr(), 0, 0));
    cx.bind(label)?;
    let rd = cx.def_reg(inst)?;
    put(cx, mc::orr_shifted(Size::W, xr(rd).zr(), mc::RegZr::ZR, Reg::X0.zr(), mc::Shift::Lsl, 0));
    cx.finish(inst, rd)
}

/// `coro.end(frame, unwind, token)`. Not unwinding: a resumed activation
/// returns (the epilogue); the ramp goes on. Unwinding: a resumed
/// activation marks the coroutine done at its final suspend. LLVM 19's
/// `i1` result is the mode: true when not in the ramp.
fn end<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, unwind: Value) -> CgResult<()> {
    let Some(unwind) = const_int(cx, unwind) else {
        return cx.bug("coro.end with an unwind operand that is not a constant");
    };
    if inst.ty() != TypeId::VOID {
        let rd = cx.def_reg(inst)?;
        load_mode(cx, xr(rd))?;
        cx.finish(inst, rd)?;
    }
    load_mode(cx, X16)?;
    if unwind & 1 == 0 {
        let skip = cx.new_label();
        compare(cx, X16, 0);
        branch_cond(cx, Cond::Eq, skip)?;
        branch(cx, cx.epilogue())?;
        return cx.bind(skip);
    }
    let layout = cx.coro_layout()?;
    let Some(final_index) = layout.final_index else {
        return cx.bug("an unwinding coro.end in a coroutine without a final suspend");
    };
    let skip = cx.new_label();
    compare(cx, X16, 0);
    branch_cond(cx, Cond::Eq, skip)?;
    let mut at = Cursor::frame(cx, FrameRef::Coro(crate::coro::RESUME), X17);
    store_gpr_at(cx, MemSize::X, mc::RegZr::ZR, &mut at);
    mov_imm(cx, Size::W, X16, u64::from(final_index));
    let mut at = Cursor::frame(cx, FrameRef::Coro(layout.index), X17);
    store_gpr_at(cx, MemSize::W, X16.zr(), &mut at);
    cx.bind(skip)
}

/// A heap access: `[x19, #o]`, reported to the structural check.
pub fn heap_cursor(cx: &mut FnCx<'_, A64>, at: u32, tmp: Reg) -> Cursor {
    cx.coro_access();
    Cursor::at_reg(X19, i64::from(at), tmp)
}
