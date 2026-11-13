//! What every lowering of this target shares: register conversions, frame
//! places, the moves between registers and memory, constant and symbol
//! materialization, and branches (`spec/codegen.md` §5.6, §6.2, §7.4,
//! §7.5).
//!
//! Owned by WP-116. The instruction-group modules build on these helpers
//! and do not re-implement them; a helper a group needs and this module
//! lacks is added here, in its own function, with a doc comment.
//!
//! # Conventions
//!
//! - `r10`, `r11`, `xmm14` and `xmm15` are the scratch registers
//!   ([`R10`], [`R11`], [`XMM14`], [`XMM15`]). The policy passes `r10` to
//!   glue; a helper that needs a register of its own takes one *other
//!   than* the registers it was given ([`other_scratch`]).
//! - `rax`, `rcx` and `rdx` never hold a value (§5.6): a lowering may
//!   write them without asking the policy (division, shifts by `cl`,
//!   the variadic `al`).
//! - **Flags** (§5.6.3): every helper here except [`extend`] and the
//!   prologue is flag-preserving (`mov`, `movzx`, `lea`, `push`, `pop`),
//!   so it may run between a compare and the `jcc`, `setcc` or `cmovcc`
//!   that reads it. Zero is `mov r32, 0`, never `xor`.
//! - **Frame places** go through [`frame_inst`]: a local is addressed
//!   from `rbp` by a `disp32` that `end_function` biases by the final
//!   frame size (`frame::end`), so every access to a local must be made
//!   by that function.

use std::cell::RefCell;

use hod_cg::{
    Access, CgError, CgResult, FixupKind, FnCx, FrameRef, Label, PReg, SlotPlace, SymRef,
};
use hod_ir::{CodeModel, Const, ConstId, Inst as IrInst, SymId, TypeId, Value, ValueKind};
use hod_mc_x64 as mc;
use hod_mc_x64::{Cond, FpSize, Inst, Mem, Reg, Size, Xmm};
use hod_obj::{RelocKind, X64Reloc};

use crate::reg;
use crate::target::X64;

/// The context of a lowering of this target.
pub type Cx<'c, 'a> = &'c mut FnCx<'a, X64>;

/// The first general scratch register: what the policy passes to glue.
pub const R10: Reg = Reg::R10;
/// The second general scratch register, and the indirect callee's
/// register (`RegInfo::callee`).
pub const R11: Reg = Reg::R11;
/// The first vector scratch register.
pub const XMM14: Xmm = Xmm::Xmm14;
/// The second vector scratch register.
pub const XMM15: Xmm = Xmm::Xmm15;
/// The frame pointer.
pub const RBP: Reg = Reg::Rbp;
/// The stack pointer.
pub const RSP: Reg = Reg::Rsp;

/// A `rel32` field: a jump, a call, a RIP-relative `disp32` that ends its
/// instruction. The fixup's offset is the field's; the label's distance
/// is counted from the field's end (§6.2: `disp = target - (at + 4)`).
pub const REL32: FixupKind = FixupKind(0);
/// A `rel8` field of a short jump, counted the same way from the end of
/// the one-byte field. Only a branch to a label already bound uses it
/// ([`branch`]), so its fixup is always patched at once.
pub const REL8: FixupKind = FixupKind(1);

/// The bytes the prologue pushes below the frame pointer: the save set
/// `rbx`, `r12`..`r15` (§5.6).
pub const SAVE_BYTES: i32 = 40;
/// The offset of the first incoming stack argument from `rbp`: the
/// saved `rbp` and the return address.
pub const INCOMING_BASE: i32 = 16;

/// The field's width in bytes of a fixup kind of this target.
pub fn fixup_field(kind: FixupKind) -> Option<mc::Fixup> {
    match kind {
        REL32 => Some(mc::Fixup::Rel32),
        REL8 => Some(mc::Fixup::Rel8),
        _ => None,
    }
}

/// The refusal `construct` (`Unsupported`), as a `CgResult`.
///
/// # Errors
/// Always.
#[cold]
pub fn unsupported<R>(construct: &'static str) -> CgResult<R> {
    Err(CgError::unsupported(construct))
}

/// The encoder's general register of a general `PReg`.
#[inline]
pub fn gp(reg: PReg) -> Reg {
    reg::reg(reg)
}

/// The encoder's vector register of a vector `PReg`.
#[inline]
pub fn xm(reg: PReg) -> Xmm {
    reg::xreg(reg)
}

/// The general scratch register that is not `reg`: `r11`, or `r10` when
/// `reg` is `r11`.
#[inline]
pub fn other_scratch(reg: Reg) -> Reg {
    if reg == R11 { R10 } else { R11 }
}

/// Emits one instruction.
#[inline]
pub fn put(cx: &mut FnCx<'_, X64>, inst: Inst) {
    cx.emit(inst.bytes());
}

/// The type of `value` in the function being compiled.
#[inline]
pub fn value_ty(cx: &FnCx<'_, X64>, value: Value) -> TypeId {
    cx.body.value_type(cx.module, value)
}

/// Emits every deferred operand of `inst` but `fused`, before the
/// lowering of `inst` places anything (§4.6).
///
/// # Errors
/// What emitting a deferred operand reports.
pub fn deferred_first<'a>(cx: Cx<'_, 'a>, inst: IrInst<'a>, fused: Option<Value>) -> CgResult<()> {
    for &operand in inst.operands() {
        if Some(operand) != fused && cx.deferred(operand).is_some() {
            cx.use_reg(operand)?;
        }
    }
    Ok(())
}

// ---- frame places ----

thread_local! {
    /// The offsets of the `disp32` fields of the accesses to locals in the
    /// function being compiled: `frame::end` adds the bias of the final
    /// frame to each. One buffer per thread, reused across functions.
    static LOCAL_SITES: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
}

/// Forgets the local accesses of the previous function (`begin_function`).
pub fn reset_local_sites() {
    LOCAL_SITES.with(|sites| sites.borrow_mut().clear());
}

/// Adds `bias` to the `disp32` of every local access of the function
/// (`end_function`).
///
/// # Errors
/// `Internal` for a field outside the code or a displacement the bias
/// takes out of 32 bits (the frame limit keeps it in).
pub fn bias_local_sites(cx: &mut FnCx<'_, X64>, bias: i64) -> CgResult<()> {
    let code = cx.code_mut();
    let ok = LOCAL_SITES.with(|sites| {
        sites.borrow().iter().all(|&at| {
            let at = at as usize;
            mc::Fixup::Abs32
                .extract(code, at)
                .is_some_and(|disp| mc::Fixup::Abs32.patch(code, at, disp + bias))
        })
    });
    if ok { Ok(()) } else { cx.bug("a local access whose displacement cannot be biased") }
}

/// The memory operand of frame place `place`, with the stack pointer
/// `sp_shift` bytes below where the place was defined (an `Outgoing`
/// place moves with it), and whether it is a local (its `disp32` must be
/// recorded). A local is `[rbp + o]` with an absolute `disp32` that
/// `end_function` biases to `o - 40 - FRAME`; an incoming argument is
/// `[rbp + 16 + n]`; an outgoing one `[rsp + n]`.
fn frame_mem(place: FrameRef, sp_shift: i32) -> (Mem, bool) {
    match place {
        FrameRef::Local(o) => {
            let mem = Mem::base(RBP, o as i32);
            (mem.with_abs32().unwrap_or(mem), true)
        }
        FrameRef::Incoming(n) => (Mem::base(RBP, INCOMING_BASE.wrapping_add(n as i32)), false),
        FrameRef::Outgoing(n) => (Mem::base(RSP, (n as i32).wrapping_add(sp_shift)), false),
    }
}

/// Emits the instruction `make` builds over the memory operand of
/// `place` (§5.6): the one way code addresses the frame.
pub fn frame_inst(cx: &mut FnCx<'_, X64>, place: FrameRef, make: impl FnOnce(Mem) -> Inst) {
    frame_inst_shifted(cx, place, 0, make);
}

/// [`frame_inst`] while the stack pointer is `sp_shift` bytes below where
/// it was when `place` was made.
pub fn frame_inst_shifted(
    cx: &mut FnCx<'_, X64>,
    place: FrameRef,
    sp_shift: i32,
    make: impl FnOnce(Mem) -> Inst,
) {
    let (mem, local) = frame_mem(place, sp_shift);
    let inst = make(mem);
    let at = cx.offset();
    put(cx, inst);
    if local && let Some(site) = inst.site() {
        let field = at + site.offset as u32;
        LOCAL_SITES.with(|sites| sites.borrow_mut().push(field));
    }
}

/// Puts the address of frame place `of` in `rd`: `lea`, flags kept.
pub fn frame_address(cx: &mut FnCx<'_, X64>, rd: Reg, of: FrameRef) {
    frame_inst(cx, of, |m| mc::lea(Size::Q, rd, m));
}

/// Puts the address of body slot `slot` in `rd`, or loads it from its
/// pointer slot for an over-aligned one (§5.1).
///
/// # Errors
/// `Internal` for a slot the body does not have.
pub fn slot_address(cx: &mut FnCx<'_, X64>, rd: Reg, slot: hod_ir::SlotId) -> CgResult<()> {
    match cx.slot_place(slot)? {
        SlotPlace::Direct(place) => frame_address(cx, rd, place),
        SlotPlace::Indirect(pointer) => frame_inst(cx, pointer, |m| mc::mov_rm(Size::Q, rd, m)),
    }
    Ok(())
}

// ---- moves between registers and the frame ----

/// The general load of `bytes` bytes (1 to 8) into `rd`: `movzx` for 1
/// and 2, the 32-bit `mov` up to 4 (zeroing bits 32..64), the 64-bit
/// `mov` above. Reads at least `bytes` bytes, at most the next power of
/// two.
pub fn load_gpr(cx: &mut FnCx<'_, X64>, rd: Reg, src: FrameRef, bytes: u32) {
    frame_inst(cx, src, |m| gpr_load(rd, m, bytes));
}

/// The general load instruction of [`load_gpr`] from `m`.
pub fn gpr_load(rd: Reg, m: Mem, bytes: u32) -> Inst {
    match bytes {
        0 | 1 => mc::movzx_rm(Size::D, rd, Size::B, m),
        2 => mc::movzx_rm(Size::D, rd, Size::W, m),
        3 | 4 => mc::mov_rm(Size::D, rd, m),
        _ => mc::mov_rm(Size::Q, rd, m),
    }
}

/// The vector load of at least `bytes` bytes into the low lanes of `rd`:
/// `movss` up to 4, `movsd` up to 8, `movups` above (16).
pub fn load_fpr(cx: &mut FnCx<'_, X64>, rd: Xmm, src: FrameRef, bytes: u32) {
    frame_inst(cx, src, |m| fpr_load(rd, m, bytes));
}

/// The vector load instruction of [`load_fpr`] from `m`.
pub fn fpr_load(rd: Xmm, m: Mem, bytes: u32) -> Inst {
    match bytes {
        0..=4 => mc::movs_rm(FpSize::S, rd, m),
        5..=8 => mc::movs_rm(FpSize::D, rd, m),
        _ => mc::movup_rm(FpSize::S, rd, m),
    }
}

/// The size of a single general access of exactly `bytes` bytes.
pub const fn exact_size(bytes: u32) -> Option<Size> {
    match bytes {
        1 => Some(Size::B),
        2 => Some(Size::W),
        4 => Some(Size::D),
        8 => Some(Size::Q),
        _ => None,
    }
}

/// Stores the low `bytes` bytes of general register `rs` to `dst`, and
/// no byte more: one `mov` for 1, 2, 4 and 8 bytes; otherwise through
/// [`store_bounced`].
pub fn store_gpr(cx: &mut FnCx<'_, X64>, rs: Reg, dst: FrameRef, bytes: u32) {
    match exact_size(bytes) {
        Some(size) => frame_inst(cx, dst, |m| mc::mov_mr(size, m, rs)),
        None => store_bounced(cx, Bounced::Gpr(rs), dst, bytes),
    }
}

/// Stores the low `bytes` bytes of vector register `rs` to `dst`, and no
/// byte more: `movss`, `movsd`, `movups` for 4, 8, 16; otherwise through
/// [`store_bounced`].
pub fn store_fpr(cx: &mut FnCx<'_, X64>, rs: Xmm, dst: FrameRef, bytes: u32) {
    match bytes {
        4 => frame_inst(cx, dst, |m| mc::movs_mr(FpSize::S, m, rs)),
        8 => frame_inst(cx, dst, |m| mc::movs_mr(FpSize::D, m, rs)),
        16 => frame_inst(cx, dst, |m| mc::movup_mr(FpSize::S, m, rs)),
        _ => store_bounced(cx, Bounced::Fpr(rs), dst, bytes),
    }
}

/// A register whose bytes [`store_bounced`] stores.
#[derive(Clone, Copy, Debug)]
pub enum Bounced {
    /// A general register.
    Gpr(Reg),
    /// A vector register.
    Fpr(Xmm),
}

/// Stores the low `bytes` bytes (at most 16) of a register whose size is
/// no single store: the register goes below the stack pointer (`lea rsp,
/// [rsp - 16]` first, so nothing is written below it), the bytes are
/// copied from there to `dst` in pieces through a general scratch
/// register other than the source, and the stack pointer comes back.
/// Flags are kept.
pub fn store_bounced(cx: &mut FnCx<'_, X64>, from: Bounced, dst: FrameRef, bytes: u32) {
    let t = match from {
        Bounced::Gpr(r) => other_scratch(r),
        Bounced::Fpr(_) => R11,
    };
    put(cx, mc::lea(Size::Q, RSP, Mem::base(RSP, -16)));
    match from {
        Bounced::Gpr(r) => put(cx, mc::mov_mr(Size::Q, Mem::base(RSP, 0), r)),
        Bounced::Fpr(x) => put(cx, mc::movup_mr(FpSize::S, Mem::base(RSP, 0), x)),
    }
    let mut done = 0;
    for (at, size) in pieces(bytes.min(16)) {
        put(cx, gpr_load(t, Mem::base(RSP, at as i32), size));
        let store_size = exact_size(size).unwrap_or(Size::Q);
        frame_inst_shifted(cx, dst.offset_by(at), 16, |m| mc::mov_mr(store_size, m, t));
        done += size;
    }
    debug_assert_eq!(done, bytes.min(16));
    put(cx, mc::lea(Size::Q, RSP, Mem::base(RSP, 16)));
}

/// `bytes` as pieces of 8, 4, 2 and 1 bytes, in address order: `(offset,
/// size)`.
pub fn pieces(bytes: u32) -> impl Iterator<Item = (u32, u32)> {
    let mut at = 0;
    std::iter::from_fn(move || {
        let left = bytes - at;
        let size = match left {
            0 => return None,
            8.. => 8,
            4.. => 4,
            2.. => 2,
            _ => 1,
        };
        let piece = (at, size);
        at += size;
        Some(piece)
    })
}

/// Copies `bytes` bytes between frame places: 16 at a time through
/// `xmm15`, the rest in pieces of 8, 4, 2, 1 through `r11`. Flags are
/// kept.
pub fn copy_frame(cx: &mut FnCx<'_, X64>, dst: FrameRef, src: FrameRef, bytes: u32) {
    let mut at = 0;
    while bytes - at >= 16 {
        frame_inst(cx, src.offset_by(at), |m| mc::movup_rm(FpSize::S, XMM15, m));
        frame_inst(cx, dst.offset_by(at), |m| mc::movup_mr(FpSize::S, m, XMM15));
        at += 16;
    }
    for (off, size) in pieces(bytes - at) {
        let size_of = exact_size(size).unwrap_or(Size::Q);
        frame_inst(cx, src.offset_by(at + off), |m| gpr_load(R11, m, size));
        frame_inst(cx, dst.offset_by(at + off), |m| mc::mov_mr(size_of, m, R11));
    }
}

/// The largest displacement a copy from a pointer uses before it moves
/// the pointer on (and back afterwards).
const REBASE: u32 = 1 << 30;

/// Copies `bytes` bytes from the memory at `ptr + offset` to `dst`, with
/// `ptr` unchanged at the end: through `xmm15` and the general scratch
/// register other than `ptr`. A displacement beyond 1 GiB moves `ptr` on
/// with `lea` and back at the end. Flags are kept.
pub fn copy_from_ptr(cx: &mut FnCx<'_, X64>, dst: FrameRef, ptr: Reg, offset: u32, bytes: u32) {
    let t = other_scratch(ptr);
    let mut moved: u64 = 0;
    let mut at: u32 = 0;
    let mut disp = |cx: &mut FnCx<'_, X64>, from: u32| -> i32 {
        let mut wanted = u64::from(offset) + u64::from(from) - moved;
        while wanted >= u64::from(REBASE) {
            put(cx, mc::lea(Size::Q, ptr, Mem::base(ptr, REBASE as i32)));
            moved += u64::from(REBASE);
            wanted -= u64::from(REBASE);
        }
        wanted as i32
    };
    while bytes - at >= 16 {
        let d = disp(cx, at);
        put(cx, mc::movup_rm(FpSize::S, XMM15, Mem::base(ptr, d)));
        frame_inst(cx, dst.offset_by(at), |m| mc::movup_mr(FpSize::S, m, XMM15));
        at += 16;
    }
    for (off, size) in pieces(bytes - at) {
        let d = disp(cx, at + off);
        put(cx, gpr_load(t, Mem::base(ptr, d), size));
        let size_of = exact_size(size).unwrap_or(Size::Q);
        frame_inst(cx, dst.offset_by(at + off), |m| mc::mov_mr(size_of, m, t));
    }
    while moved > 0 {
        put(cx, mc::lea(Size::Q, ptr, Mem::base(ptr, -(REBASE as i32))));
        moved -= u64::from(REBASE);
    }
}

// ---- immediates ----

/// Puts `value` in `rd`, its low `bits` bits significant (§5.6.4): `mov
/// r32, imm32` for up to 32 bits or a value that zero-extends from 32;
/// `mov r64, simm32` for one that sign-extends from 32; `movabs`
/// otherwise. Never `xor`: flags are kept.
pub fn mov_imm(cx: &mut FnCx<'_, X64>, rd: Reg, value: u64, bits: u32) {
    let value = if bits < 64 { value & ((1u64 << bits) - 1) } else { value };
    if bits <= 32 || u32::try_from(value).is_ok() {
        put(cx, mc::mov_ri(Size::D, rd, (value & 0xffff_ffff) as i64));
    } else {
        // `mov_ri` picks `C7 /0` for a sign-extended 32-bit value.
        put(cx, mc::mov_ri(Size::Q, rd, value as i64));
    }
}

/// `rd += value` (64-bit, wrapping), flags kept, with `rd` the only
/// register written besides the general scratch register other than
/// `rd` for a value beyond 32 bits: `lea rd, [rd + value]`, or `movabs`
/// into that register and `lea rd, [rd + t]`.
pub fn add_addend(cx: &mut FnCx<'_, X64>, rd: Reg, value: i64) {
    if value == 0 {
        return;
    }
    if let Ok(small) = i32::try_from(value) {
        put(cx, mc::lea(Size::Q, rd, Mem::base(rd, small)));
        return;
    }
    let t = other_scratch(rd);
    put(cx, mc::mov_ri(Size::Q, t, value));
    // `r10` and `r11` are both index registers (only `rsp` is not).
    if let Some(index) = t.index() {
        put(cx, mc::lea(Size::Q, rd, Mem::base_index(rd, index, mc::Scale::S1, 0)));
    }
}

// ---- symbols and constants ----

/// A code model other than small (§5.6).
pub const CODE_MODEL: &str = "x64:code-model";
/// A constant of class `Gpr` this target does not materialize yet.
pub const CONSTANT: &str = "x64:todo:constant";

/// The largest addend, in magnitude and exclusive, folded into a direct
/// symbol reference (`R_X86_64_PC32`): `llc`'s 16 MiB of
/// `isOffsetSuitableForCodeModel` for the small code model, on both
/// sides, so the field cannot overflow for an address within the object
/// or near it.
pub const MAX_FOLDED_OFFSET: i64 = 16 << 20;

/// Emits `inst`, whose site is a `disp32` or `rel32`, with relocation
/// `kind` against `target` there: the site's own addend (the distance
/// from the field to the end of the instruction) plus `addend`.
pub fn put_reloc(
    cx: &mut FnCx<'_, X64>,
    inst: Inst,
    kind: X64Reloc,
    target: SymRef,
    addend: i64,
) -> CgResult<()> {
    let Some(site) = inst.site() else {
        return cx.bug("a relocated instruction without a relocatable field");
    };
    let at = cx.offset();
    put(cx, inst);
    cx.reloc(at + site.offset as u32, RelocKind::X64(kind), target, site.addend + addend);
    Ok(())
}

/// Puts the address of `sym` + `addend` in `rd` (§7.4, §5.6.5), flags
/// kept: `lea rd, [rip + sym]` (`R_X86_64_PC32`) for a direct symbol,
/// the addend folded when it is within [`MAX_FOLDED_OFFSET`];
/// `mov rd, [rip + sym@GOTPCREL]` (`R_X86_64_REX_GOTPCRELX`, which
/// linkers may relax) otherwise, then the addend added by
/// [`add_addend`]. Writes `rd`, and for an addend beyond 32 bits the
/// general scratch register other than `rd`.
///
/// # Errors
/// [`CODE_MODEL`] for a code model other than small; `x64:todo:tls` for
/// a thread-local symbol (WP-134).
pub fn sym_address(cx: &mut FnCx<'_, X64>, rd: Reg, sym: SymId, addend: i64) -> CgResult<()> {
    if cx.options.code_model(cx.module) != CodeModel::Small {
        return unsupported(CODE_MODEL);
    }
    let target = SymRef::Module(sym);
    match hod_cg::sym_access(cx.module, cx.options, sym) {
        Access::Direct => {
            let folded = if addend.abs() < MAX_FOLDED_OFFSET { addend } else { 0 };
            put_reloc(cx, mc::lea(Size::Q, rd, Mem::rip(0)), X64Reloc::Pc32, target, folded)?;
            add_addend(cx, rd, addend - folded);
        }
        Access::Got => {
            let load = mc::mov_rm(Size::Q, rd, Mem::rip(0));
            let kind = if load.has_rex() { X64Reloc::RexGotPcrelX } else { X64Reloc::GotPcrelX };
            put_reloc(cx, load, kind, target, 0)?;
            add_addend(cx, rd, addend);
        }
        Access::Tls(_) => return unsupported(crate::tls::TLS),
    }
    Ok(())
}

/// Puts the general-class constant `id` in `rd` (§7.5): integers and
/// `inttoptr` of a number by [`mov_imm`], zero-like constants as `mov
/// r32, 0`, addresses by [`sym_address`].
///
/// # Errors
/// What [`sym_address`] reports; `x64:todo:control` for `blockaddress`,
/// `x64:todo:constaddr` for a difference or expression of addresses,
/// [`CONSTANT`] for anything else.
pub fn materialize_gpr_const(cx: &mut FnCx<'_, X64>, rd: Reg, id: ConstId) -> CgResult<()> {
    match cx.module.consts.get(id) {
        Const::Int { ty, value } => {
            let bits = cx.types().int_bits(ty).unwrap_or(64);
            mov_imm(cx, rd, value, bits);
        }
        Const::AddrOfInt { value } => mov_imm(cx, rd, value, 64),
        Const::Null(_) | Const::Zero(_) | Const::Undef(_) | Const::Poison(_) => {
            mov_imm(cx, rd, 0, 32);
        }
        Const::Addr { sym, addend } => sym_address(cx, rd, sym, addend)?,
        Const::IntOfAddr { addr, .. } => return materialize_gpr_const(cx, rd, addr),
        Const::DsoLocalEquivalent(sym) | Const::NoCfi(sym) => sym_address(cx, rd, sym, 0)?,
        Const::BlockAddr { .. } => return unsupported(crate::control::CONTROL),
        Const::SymDiff { .. } | Const::Expr { .. } => crate::constaddr::gpr(cx, rd, id)?,
        _ => return unsupported(CONSTANT),
    }
    Ok(())
}

/// Puts an operand without a location (§3) in general register `rd`: a
/// constant, a symbol's address, a stack slot's address.
///
/// # Errors
/// As [`materialize_gpr_const`]; `Internal` for something else.
pub fn materialize_gpr(cx: &mut FnCx<'_, X64>, rd: Reg, value: Value) -> CgResult<()> {
    match value.kind() {
        ValueKind::Const(id) => materialize_gpr_const(cx, rd, id),
        ValueKind::Sym(sym) => sym_address(cx, rd, sym, 0),
        ValueKind::Slot(slot) => slot_address(cx, rd, slot),
        _ => cx.bug("materialize of something that is no constant"),
    }
}

// ---- argument extension ----

/// Brings the integer `bits` wide in `rd` to what a producer owes
/// (§5.6, SysV): `zeroext`/`signext` below 32 bits extend to 32
/// (`movzx`/`movsx`, flags kept); a plain `i1` is masked to bit 0 (`and
/// r32, 1`, which writes the flags: only where none are live, before a
/// call or at a return).
pub fn extend(cx: &mut FnCx<'_, X64>, rd: Reg, bits: u32, signed: Option<bool>) {
    match (bits, signed) {
        (8 | 16, Some(signed)) => {
            let from = if bits == 8 { Size::B } else { Size::W };
            let inst = if signed {
                mc::movsx_rr(Size::D, rd, from, rd)
            } else {
                mc::movzx_rr(Size::D, rd, from, rd)
            };
            put(cx, inst);
        }
        (1, Some(true)) => {
            // `i1 signext`: 0 or -1 in 32 bits. Bit 0 to all bits.
            put(cx, mc::and_ri(Size::D, rd, 1));
            put(cx, mc::neg_r(Size::D, rd));
        }
        (1, _) => put(cx, mc::and_ri(Size::D, rd, 1)),
        _ => {}
    }
}

// ---- branches ----

/// `jmp label` (§6.2, §5.6.4): to a label already bound (backward) the
/// shortest form, `jmp rel8` when it reaches; otherwise `jmp rel32`,
/// patched when the label is bound. There is no far mode: `rel32`
/// reaches every function (§6.3 never fires).
///
/// # Errors
/// What [`FnCx::fixup`] reports.
pub fn branch(cx: &mut FnCx<'_, X64>, label: Label) -> CgResult<()> {
    let at = cx.offset();
    if let Some(target) = cx.label_offset(label) {
        let short = i64::from(target) - i64::from(at + 2);
        if let Ok(disp) = i8::try_from(short) {
            put(cx, mc::jmp_short(disp));
            return Ok(());
        }
    }
    put(cx, mc::jmp_near(0));
    cx.fixup(at + 1, REL32, label)
}

/// `jcc label`: as [`branch`], `jcc rel8` backward when it reaches,
/// `jcc rel32` otherwise.
///
/// # Errors
/// What [`FnCx::fixup`] reports.
pub fn branch_cond(cx: &mut FnCx<'_, X64>, cond: Cond, label: Label) -> CgResult<()> {
    let at = cx.offset();
    if let Some(target) = cx.label_offset(label) {
        let short = i64::from(target) - i64::from(at + 2);
        if let Ok(disp) = i8::try_from(short) {
            put(cx, mc::jcc_short(cond, disp));
            return Ok(());
        }
    }
    put(cx, mc::jcc_near(cond, 0));
    cx.fixup(at + 2, REL32, label)
}

/// `Target::patch_branch`: `at` is the field's offset; the field holds
/// the distance from its own end (`disp - width`).
pub fn patch_branch(code: &mut [u8], at: u32, kind: FixupKind, disp: i64) -> bool {
    let Some(field) = fixup_field(kind) else { return false };
    field.patch(code, at as usize, disp - field.size() as i64)
}
