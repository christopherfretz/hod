//! The frame (`spec/codegen.md` §5.4, §5.5, §9.3): prologue, exit
//! sequences and their CFI rows, the incoming base, dynamic slots and
//! stack adjustments. Owned by WP-033.
//!
//! ```text
//! [mov x16, sp]                  only with an incoming base
//! sub sp, sp, #lo12              patched in end_function
//! sub sp, sp, #hi12, lsl #12     patched in end_function
//! stp x29, x30, [sp]
//! mov x29, sp
//! [str x16, [x29, #base]]        the incoming base
//! [str x8, [x29, #ret]]          the address of an in-memory result
//! ```
//!
//! The epilogue is `[mov sp, x29]` (dynamic allocation only) ·
//! `ldp x29, x30, [sp]` · `add sp, sp, #FRAME` (one instruction, or
//! `#hi12, lsl #12` then `#lo12`) · `ret`; a tail exit is the same
//! teardown ending in `b callee` or `br x16`.

use hod_cg::abi::{PartKind, Place};
use hod_cg::{CallTarget, CfiOp, CgError, CgResult, FnCx, FrameDesc, FrameRef, LIMIT_FRAME_SIZE};
use hod_mc_a64 as mc;
use hod_mc_a64::{Load, MemSize, Reg, Size};
use hod_obj::{A64Reloc, RelocKind};

use crate::abi::INDIRECT_RESULT;
use crate::common::{
    Cursor, FP, SP, X16, X17, add_const, frame_address, mov_imm, put, store_gpr_at, xr,
};
use crate::target::A64;

/// The largest `FRAME` the fixed-size prologue expresses: two 12-bit
/// immediates, rounded down to the stack alignment (§5.5).
pub const MAX_A64_FRAME: u32 = (1 << 24) - 16;

/// The DWARF numbers of the frame pointer, the link register and the
/// stack pointer.
pub const DWARF_FP: u16 = 29;
/// The link register `x30`.
pub const DWARF_LR: u16 = 30;
/// The stack pointer.
pub const DWARF_SP: u16 = 31;

/// The bytes of the prologue from its first `sub` to its `mov x29, sp`.
pub const PROLOGUE_BYTES: u32 = 16;

/// What the frame remembers about the function between
/// `begin_function`, the lowerings and `end_function`.
#[derive(Clone, Copy, Default, Debug)]
pub struct FrameState {
    /// The offset of the prologue's first `sub`.
    pub prologue: u32,
    /// The local-area offset of the incoming base, if the function has
    /// one (§5.5).
    pub incoming_base: Option<u32>,
    /// The local-area offset of the slot holding the address of the
    /// buffer an in-memory result is written to (`x8` at entry).
    pub ret_memory: Option<u32>,
    /// Whether the stack pointer moves below the frame (a dynamic slot,
    /// an `alloca`): the epilogue then restores it from the frame
    /// pointer. Set by every lowering that lowers it for longer than one
    /// instruction's sequence.
    pub dynamic: bool,
    /// The coroutine frame's `x19` save (WP-106, `crate::coro_body`).
    pub coro: crate::coro_body::CoroState,
}

/// Whether the function addresses its incoming stack area (§5.5): it has
/// a stack-passed or `byval` argument, or is variadic.
fn needs_incoming_base(cx: &FnCx<'_, A64>) -> bool {
    let asg = cx.fn_assignment();
    let stack = asg
        .args
        .iter()
        .any(|part| matches!(part.place, Place::Stack(_)) || part.kind == PartKind::Pointee);
    stack || is_variadic(cx)
}

/// Whether the function being compiled is variadic.
pub fn is_variadic(cx: &FnCx<'_, A64>) -> bool {
    matches!(cx.types().kind(cx.body.fn_type()), hod_ir::TypeKind::Function { variadic: true, .. })
}

/// `Target::begin_function`.
pub fn begin(cx: &mut FnCx<'_, A64>) -> CgResult<()> {
    if cx.alloc_slot(16, 16)? != FrameRef::Local(0) {
        return cx.bug("the frame record is not at the start of the local area");
    }
    let base = needs_incoming_base(cx);
    if base {
        put(cx, mc::mov_sp(Size::X, X16.sp(), SP));
    }
    cx.state.frame.prologue = cx.offset();
    // The immediates are patched in `end`; until then both are 0.
    put(cx, mc::sub_imm(Size::X, SP, SP, 0));
    put(cx, mc::sub_imm(Size::X, SP, SP, 0));
    put(cx, mc::stp_off(Size::X, Reg::X29.zr(), Reg::X30.zr(), SP, 0));
    put(cx, mc::mov_sp(Size::X, FP, SP));
    crate::tail2::save_preserved(cx)?;
    crate::tail3::save_all(cx)?;
    crate::coro_body::save(cx)?;
    if base {
        let FrameRef::Local(slot) = cx.alloc_slot(8, 8)? else {
            return cx.bug("a frame slot outside the local area");
        };
        cx.state.frame.incoming_base = Some(slot);
        put(cx, mc::str_off(MemSize::X, X16.zr(), FP, i64::from(slot)));
    }
    if cx.fn_assignment().ret_memory.is_some() {
        let FrameRef::Local(slot) = cx.alloc_slot(8, 8)? else {
            return cx.bug("a frame slot outside the local area");
        };
        cx.state.frame.ret_memory = Some(slot);
        let mut at = Cursor::frame(cx, FrameRef::Local(slot), X16);
        store_gpr_at(cx, MemSize::X, xr(INDIRECT_RESULT).zr(), &mut at);
    }
    if is_variadic(cx) {
        crate::call::reserve_variadic(cx)?;
    }
    Ok(())
}

/// The final frame size: the local area rounded to the stack alignment.
///
/// # Errors
/// `Unsupported("limit:frame-size")` above [`MAX_A64_FRAME`].
pub fn final_size(cx: &FnCx<'_, A64>) -> CgResult<u32> {
    let frame = cx.frame_size().next_multiple_of(16);
    if frame > MAX_A64_FRAME {
        return Err(CgError::unsupported(LIMIT_FRAME_SIZE));
    }
    Ok(frame)
}

/// `Target::end_function`.
pub fn end(cx: &mut FnCx<'_, A64>) -> CgResult<()> {
    let frame = final_size(cx)?;
    let (lo, hi) = (frame & 0xfff, frame & 0xff_f000);
    let at = cx.state.frame.prologue;
    let patch =
        [mc::sub_imm(Size::X, SP, SP, u64::from(lo)), mc::sub_imm(Size::X, SP, SP, u64::from(hi))];
    for (index, word) in patch.into_iter().enumerate() {
        let start = at as usize + index * 4;
        cx.code_mut()[start..start + 4].copy_from_slice(&word.to_le_bytes());
    }
    prologue_rows(cx, at, frame)?;
    crate::tail2::preserved_rows(cx, Some(at + PROLOGUE_BYTES + crate::tail2::SAVE_BYTES), frame)?;
    crate::tail3::all_rows(cx, Some(at + PROLOGUE_BYTES), frame)?;
    let saved_at = cx.state.frame.coro.saved_at;
    crate::coro_body::rows(cx, Some(saved_at), frame)?;

    cx.bind(cx.epilogue())?;
    teardown(cx, frame);
    put(cx, mc::ret(Reg::LR.zr()));

    for index in 0..cx.tail_exit_count() {
        let exit = cx.tail_exit(index);
        cx.bind(exit.label)?;
        // §9.3: a tail exit comes after another exit sequence, so it
        // states the body's rules again.
        body_rows(cx, frame);
        teardown(cx, frame);
        match exit.target {
            CallTarget::Sym(sym) => {
                let at = cx.offset();
                put(cx, mc::b(0));
                cx.reloc(at, RelocKind::A64(A64Reloc::Jump26), sym, 0);
            }
            CallTarget::Reg(callee) => put(cx, mc::br(xr(callee).zr())),
        }
    }
    cx.set_frame_desc(FrameDesc { frame_size: frame, frame_base: DWARF_FP });
    Ok(())
}

/// `Target::frame_base_offset`: the frame base is `x29`, which the
/// prologue sets to the entry stack pointer less `FRAME`, so a local is
/// at its own offset and an incoming argument `FRAME` further up.
pub fn base_offset(desc: &FrameDesc, place: FrameRef) -> Option<i64> {
    match place {
        FrameRef::Local(at) => Some(i64::from(at)),
        FrameRef::Incoming(at) => Some(i64::from(desc.frame_size) + i64::from(at)),
        FrameRef::Outgoing(_) | FrameRef::Coro(_) => None,
    }
}

/// The CFI rows of the prologue at `at` (§5.5, §9.3): the CFA moves with
/// each `sub`, the frame record is saved by the `stp`, and from the
/// `mov x29, sp` on the CFA is `x29 + FRAME`.
fn prologue_rows(cx: &mut FnCx<'_, A64>, at: u32, frame: u32) -> CgResult<()> {
    let (lo, hi) = (frame & 0xfff, frame & 0xff_f000);
    if lo != 0 {
        cx.cfi_at(at + 4, CfiOp::DefCfaOffset(lo as i32))?;
    }
    if hi != 0 {
        cx.cfi_at(at + 8, CfiOp::DefCfaOffset(frame as i32))?;
    }
    let f = frame as i32;
    cx.cfi_at(at + 12, CfiOp::Offset { reg: DWARF_FP, cfa_offset: -f })?;
    cx.cfi_at(at + 12, CfiOp::Offset { reg: DWARF_LR, cfa_offset: -f + 8 })?;
    cx.cfi_at(at + 16, CfiOp::DefCfaRegister(DWARF_FP))
}

/// The rules that hold in the body, from the current offset.
fn body_rows(cx: &mut FnCx<'_, A64>, frame: u32) {
    let f = frame as i32;
    cx.cfi(CfiOp::DefCfa { reg: DWARF_FP, offset: f });
    cx.cfi(CfiOp::Offset { reg: DWARF_FP, cfa_offset: -f });
    cx.cfi(CfiOp::Offset { reg: DWARF_LR, cfa_offset: -f + 8 });
    // WP-095: a hook that cannot fail when the prologue's rows did not.
    let _ = crate::tail2::preserved_rows(cx, None, frame);
    let _ = crate::tail3::all_rows(cx, None, frame);
    let _ = crate::coro_body::rows(cx, None, frame);
}

/// The teardown every exit sequence starts with, with its rows: the
/// stack pointer back at the frame record, the record reloaded, the frame
/// released.
fn teardown(cx: &mut FnCx<'_, A64>, frame: u32) {
    if cx.state.frame.dynamic {
        put(cx, mc::mov_sp(Size::X, SP, FP));
    }
    cx.cfi(CfiOp::DefCfa { reg: DWARF_SP, offset: frame as i32 });
    crate::tail2::restore_preserved(cx);
    crate::tail3::restore_all(cx);
    crate::coro_body::restore(cx);
    put(cx, mc::ldp_off(Size::X, Reg::X29.zr(), Reg::X30.zr(), SP, 0));
    cx.cfi(CfiOp::Restore { reg: DWARF_FP });
    cx.cfi(CfiOp::Restore { reg: DWARF_LR });
    let (lo, hi) = (frame & 0xfff, frame & 0xff_f000);
    if lo == 0 || hi == 0 {
        put(cx, mc::add_imm(Size::X, SP, SP, u64::from(frame)));
    } else {
        put(cx, mc::add_imm(Size::X, SP, SP, u64::from(hi)));
        cx.cfi(CfiOp::DefCfaOffset(lo as i32));
        put(cx, mc::add_imm(Size::X, SP, SP, u64::from(lo)));
    }
    cx.cfi(CfiOp::DefCfaOffset(0));
}

/// `Target::adjust_sp`: `delta` added to the stack pointer, writing no
/// other register.
pub fn adjust_sp(cx: &mut FnCx<'_, A64>, delta: i64) {
    if delta != 0 {
        add_const(cx, SP, SP, delta);
    }
}

/// `Target::dynamic_slot`: `sub x16, sp, #size` · `and x16, x16,
/// #-align` · `mov sp, x16` · the address stored to `ptr`. The size is
/// rounded to 16 so the stack pointer stays aligned.
///
/// # Errors
/// `Unsupported("limit:frame-size")` for a size or alignment beyond 4 GiB.
pub fn dynamic_slot(cx: &mut FnCx<'_, A64>, ptr: FrameRef, size: u64, align: u64) -> CgResult<()> {
    let too_big = || CgError::unsupported(LIMIT_FRAME_SIZE);
    let size = size.checked_next_multiple_of(16).filter(|&s| s < 1 << 32).ok_or_else(too_big)?;
    if !align.is_power_of_two() || align >= 1 << 32 {
        return Err(too_big());
    }
    if size < 1 << 24 {
        add_const(cx, X16.sp(), SP, -(size as i64));
    } else {
        mov_imm(cx, Size::X, X17, size);
        put(cx, mc::sub_ext(Size::X, X16.sp(), SP, X17.zr(), mc::Extend::Uxtx, 0));
    }
    put(cx, mc::and_imm(Size::X, X16.sp(), X16.zr(), !(align - 1)));
    put(cx, mc::mov_sp(Size::X, SP, X16.sp()));
    let mut at = Cursor::frame(cx, ptr, X17);
    store_gpr_at(cx, MemSize::X, X16.zr(), &mut at);
    cx.state.frame.dynamic = true;
    Ok(())
}

/// Loads the address of the in-memory result's buffer into `rd`.
///
/// # Errors
/// `Internal` when the function does not return in memory.
pub fn ret_memory_address(cx: &mut FnCx<'_, A64>, rd: Reg) -> CgResult<()> {
    let Some(slot) = cx.state.frame.ret_memory else {
        return cx.bug("the result buffer of a function that does not return in memory");
    };
    put(cx, mc::ldr_off(Load::X, rd.zr(), FP, i64::from(slot)));
    Ok(())
}

/// Puts the address of frame place `of` in `rd` (`Target::frame_addr`).
pub fn frame_addr(cx: &mut FnCx<'_, A64>, rd: Reg, of: FrameRef) {
    frame_address(cx, rd, of);
}

/// The call instruction itself (`Target::emit_call`): `bl` with its
/// `R_AARCH64_CALL26`, or `blr`.
pub fn emit_call(cx: &mut FnCx<'_, A64>, target: CallTarget) {
    match target {
        CallTarget::Sym(sym) => {
            let at = cx.offset();
            put(cx, mc::bl(0));
            cx.reloc(at, RelocKind::A64(A64Reloc::Call26), sym, 0);
        }
        CallTarget::Reg(callee) => put(cx, mc::blr(xr(callee).zr())),
    }
}
