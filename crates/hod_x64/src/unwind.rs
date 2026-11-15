//! The frame as an unwinder sees it (`spec/codegen.md` §5.6.38,
//! WP-215): `llvm.eh.dwarf.cfa`, `llvm.addressofreturnaddress`,
//! `llvm.eh.unwind.init` and `llvm.eh.return.i64`.
//!
//! The canonical frame address is `rbp + 16` from the second instruction
//! of the prologue to the epilogue (§5.6: the frame is never realigned,
//! an over-aligned slot and an `alloca` live below `rsp`), so
//! `llvm.eh.dwarf.cfa(k)` is `rbp + 16 + sext(k)` wherever it stands, as
//! `llc` 19 and 22 compute it. The return address is at `rbp + 8`.
//!
//! `llvm.eh.unwind.init` makes the function save all five callee-saved
//! registers (`frame::FrameState::save_all`), as `llc` does: an unwinder
//! finds their values in the frame through its rows.
//!
//! `llvm.eh.return.i64(offset, handler)` leaves the function for
//! `handler` with the stack pointer at `CFA + offset`, as `llc` does:
//!
//! ```text
//! body:   lea rcx, [rbp + offset + 8] · mov [rcx], handler · jmp .Leh
//! .Leh:   def_cfa rbp, 16 · mov rax, [slot] · mov rdx, [slot + 8] ·
//!         the teardown of every exit · mov rsp, rcx · ret
//! ```
//!
//! The function also keeps `rax` and `rdx` (the registers the landing
//! pad receives its data in) in a slot written right after the prologue,
//! with their rows: an unwinder writes the handler's data there, and the
//! `eh.return` exit loads them. The ordinary epilogue leaves `rax` and
//! `rdx` alone, so a value the function returns survives (`llc` 19 and 22
//! pop both over it: the record of WP-215 has the evidence).

use hod_cg::{CfiOp, CgResult, FnCx, FrameRef, Label};
use hod_ir::{Inst, Intrinsic, Value};
use hod_mc_x64 as mc;
use hod_mc_x64::{Mem, Reg, Scale, Size};

use crate::common::{
    INCOMING_BASE, RBP, add_addend, branch, deferred_first, frame_inst, gp, put, unsupported,
};
use crate::intrinsic::const_int;
use crate::target::X64;

/// `llvm.eh.return.i64` in a module with `cf-protection-return`: the
/// `ret` to the handler does not match the shadow stack (`llc` writes
/// the plain `ret`).
pub const EH_RETURN_SHADOW_STACK: &str = "x64:eh-return-shadow-stack";
/// `llvm.eh.return.i64` in a `preserve_mostcc`, `preserve_allcc` or
/// (WP-246) `win64cc` function or a coroutine body: their exits restore
/// other registers (`rax` and `rdx` among them) or another frame base.
pub const EH_RETURN_CONVENTION: &str = "x64:eh-return-convention";

/// Every name this module refuses with.
pub const UNSUPPORTED: &[&str] = &[EH_RETURN_SHADOW_STACK, EH_RETURN_CONVENTION];

/// The DWARF numbers of `rax` and `rdx` (SysV psABI figure 3.36).
const DWARF_RAX: u16 = 0;
const DWARF_RDX: u16 = 1;

/// What a function with `llvm.eh.return.i64` keeps between
/// `begin_function`, the lowering and `end_function`.
#[derive(Clone, Copy, Default, Debug)]
pub struct UnwindState {
    /// The slot of `rax` (and `rdx` 8 bytes above), if the function has
    /// an `llvm.eh.return.i64`.
    slot: Option<FrameRef>,
    /// The code offset right after their stores.
    rows_at: u32,
    /// The exit the `llvm.eh.return.i64` calls branch to.
    exit: Option<Label>,
}

/// Whether the function being compiled calls `llvm.eh.return.i64`.
fn has_eh_return(cx: &FnCx<'_, X64>) -> bool {
    cx.body.calls().any(|site| site.intrinsic() == Some(Intrinsic::EhReturnI64))
}

/// From `frame::begin`, after the local area's fixed slots: a function
/// with `llvm.eh.return.i64` stores `rax` and `rdx` in a slot of their
/// own.
///
/// # Errors
/// The refusals of this module; what `alloc_slot` reports.
pub fn enter(cx: &mut FnCx<'_, X64>) -> CgResult<()> {
    cx.state.frame.unwind = UnwindState::default();
    if !has_eh_return(cx) {
        return Ok(());
    }
    let bits = cx.module.props.branch_protection.bits();
    if bits & hod_ir::BranchProtection::CET_SHSTK != 0 {
        return unsupported(EH_RETURN_SHADOW_STACK);
    }
    if crate::frame::is_preserving(cx) || crate::frame::is_win64(cx) || cx.has_coro_frame() {
        return unsupported(EH_RETURN_CONVENTION);
    }
    let slot = cx.alloc_slot(16, 8)?;
    frame_inst(cx, slot, |m| mc::mov_mr(Size::Q, m, Reg::Rax));
    frame_inst(cx, slot.offset_by(8), |m| mc::mov_mr(Size::Q, m, Reg::Rdx));
    cx.state.frame.unwind.slot = Some(slot);
    cx.state.frame.unwind.rows_at = cx.offset();
    Ok(())
}

/// From `frame::end`, once `below` (the bytes the prologue puts below
/// `rbp`) is known: the rows of `rax` and `rdx`, from after their stores.
///
/// # Errors
/// What `cfi_at` reports.
pub fn rows(cx: &mut FnCx<'_, X64>, below: u32) -> CgResult<()> {
    let Some(FrameRef::Local(o)) = cx.state.frame.unwind.slot else { return Ok(()) };
    let at = cx.state.frame.unwind.rows_at;
    let cfa_offset = (i64::from(o) - i64::from(below) - i64::from(INCOMING_BASE)) as i32;
    cx.cfi_at(at, CfiOp::Offset { reg: DWARF_RAX, cfa_offset })?;
    cx.cfi_at(at, CfiOp::Offset { reg: DWARF_RDX, cfa_offset: cfa_offset + 8 })
}

/// The label of the `llvm.eh.return.i64` exit, if a call branched to it.
pub fn exit_label(cx: &FnCx<'_, X64>) -> Option<Label> {
    cx.state.frame.unwind.exit
}

/// The start of the `llvm.eh.return.i64` exit, after its label: `rax`
/// and `rdx` from their slot (what an unwinder wrote there).
pub fn reload(cx: &mut FnCx<'_, X64>) -> CgResult<()> {
    let Some(slot) = cx.state.frame.unwind.slot else {
        return cx.bug("an eh.return exit without the slot of rax and rdx");
    };
    frame_inst(cx, slot, |m| mc::mov_rm(Size::Q, Reg::Rax, m));
    frame_inst(cx, slot.offset_by(8), |m| mc::mov_rm(Size::Q, Reg::Rdx, m));
    Ok(())
}

/// A call of `which`, one of this module's intrinsics, with `args`.
///
/// # Errors
/// The refusals of this module; what the register policy reports.
pub fn intrinsic<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &[Value],
) -> CgResult<()> {
    use Intrinsic as I;
    deferred_first(cx, inst, None)?;
    match (which, args) {
        (I::EhDwarfCfa, &[offset]) => dwarf_cfa(cx, inst, offset),
        (I::AddressOfReturnAddress, []) => {
            let rd = cx.def_reg(inst)?;
            put(cx, mc::lea(Size::Q, gp(rd), Mem::base(RBP, 8)));
            cx.finish(inst, rd)
        }
        (I::EhUnwindInit, []) => {
            cx.state.frame.save_all = true;
            Ok(())
        }
        (I::EhReturnI64, &[offset, handler]) => eh_return(cx, offset, handler),
        _ => cx.bug("a frame intrinsic with unexpected operands"),
    }
}

/// `llvm.eh.dwarf.cfa(offset)`: `rbp + 16 + sext(offset)`.
fn dwarf_cfa<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, offset: Value) -> CgResult<()> {
    if let Some(k) = const_int(cx, offset) {
        let k = i64::from(k as u32 as i32);
        let rd = cx.def_reg(inst)?;
        put(cx, mc::lea(Size::Q, gp(rd), Mem::base(RBP, INCOMING_BASE)));
        add_addend(cx, gp(rd), k);
        return cx.finish(inst, rd);
    }
    let o = gp(cx.use_reg(offset)?);
    let rd = cx.def_reg(inst)?;
    let r = gp(rd);
    put(cx, mc::movsxd_rr(r, o));
    let Some(index) = r.index() else { return cx.bug("rsp holds a value") };
    put(cx, mc::lea(Size::Q, r, Mem::base_index(RBP, index, Scale::S1, INCOMING_BASE)));
    cx.finish(inst, rd)
}

/// `llvm.eh.return.i64(offset, handler)`: the handler stored where the
/// return address of a frame whose CFA is `CFA + offset` would be, its
/// address in `rcx`, then the exit. `rcx` never holds a value (§5.6), so
/// neither operand is in it, and the exit writes no `rcx`.
fn eh_return(cx: &mut FnCx<'_, X64>, offset: Value, handler: Value) -> CgResult<()> {
    let o = gp(cx.use_reg(offset)?);
    let h = gp(cx.use_reg(handler)?);
    let Some(index) = o.index() else { return cx.bug("rsp holds a value") };
    put(cx, mc::lea(Size::Q, Reg::Rcx, Mem::base_index(RBP, index, Scale::S1, 8)));
    put(cx, mc::mov_mr(Size::Q, Mem::base(Reg::Rcx, 0), h));
    let exit = if let Some(exit) = cx.state.frame.unwind.exit {
        exit
    } else {
        let exit = cx.new_label();
        cx.state.frame.unwind.exit = Some(exit);
        exit
    };
    branch(cx, exit)
}
