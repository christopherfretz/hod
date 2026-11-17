//! Shared helpers (`spec/codegen-x86.md` "Encoding", "Frame places",
//! "Symbols"): the one way an instruction is emitted (through the
//! encoder's 32-bit mode), frame places, the glue's moves, constants,
//! symbol addresses, branches. Owned by WP-293; a later package adds a
//! helper in its own function with a doc comment.

use std::cell::{Cell, RefCell};

use hod_cg::{CgError, CgResult, FixupKind, FnCx, FrameRef, Label, PReg, SlotPlace, SymRef};
use hod_ir::{Const, ConstId, SymId, TypeId, Value, ValueKind};
use hod_mc_x64 as mc;
use hod_mc_x64::{Cond, Inst, Mem, Mode, ModeError, Reg, Size};
use hod_obj::{I386Reloc, RelocKind};

use crate::reg::{self, has_low_byte};
use crate::target::X86;

/// `eax` as the encoder names it.
pub const EAX: Reg = Reg::Rax;
/// `ecx`.
pub const ECX: Reg = Reg::Rcx;
/// `edx`.
pub const EDX: Reg = Reg::Rdx;
/// `ebp`.
pub const EBP: Reg = Reg::Rbp;
/// `esp`.
pub const ESP: Reg = Reg::Rsp;

/// A `rel32` field of a branch; the distance counts from the field's end.
pub const REL32: FixupKind = FixupKind(0);
/// A `rel8` field of a short branch.
pub const REL8: FixupKind = FixupKind(1);

/// The offset of the first incoming stack argument from `ebp`: the saved
/// `ebp` and the return address.
pub const INCOMING_BASE: i32 = 8;

/// The refusal `construct`, as a `CgResult`.
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

thread_local! {
    /// The first instruction of the function the 32-bit mode refused.
    static ADMIT_ERROR: Cell<Option<ModeError>> = const { Cell::new(None) };
    /// The `disp32` fields of the function's local accesses, biased in
    /// `end_function` (`frame::end`).
    static LOCAL_SITES: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
}

/// Emits one instruction through [`Mode::X86`]. An instruction the mode
/// refuses is a bug of the lowering: `ud2` stands for it and
/// [`take_admit_error`] reports it at the end of the function.
pub fn put(cx: &mut FnCx<'_, X86>, inst: Inst) {
    match Mode::X86.admit(inst) {
        Ok(inst) => cx.emit(inst.bytes()),
        Err(error) => {
            ADMIT_ERROR.with(|e| {
                if e.get().is_none() {
                    e.set(Some(error));
                }
            });
            cx.emit(mc::ud2().bytes());
        }
    }
}

/// Forgets the previous function's encoding error and local sites.
pub fn reset_function() {
    ADMIT_ERROR.with(|e| e.set(None));
    LOCAL_SITES.with(|s| s.borrow_mut().clear());
}

/// The encoding error of the function, if any (an internal error).
///
/// # Errors
/// `Internal` naming the mode's refusal.
pub fn take_admit_error(cx: &FnCx<'_, X86>) -> CgResult<()> {
    match ADMIT_ERROR.with(Cell::take) {
        Some(error) => cx.bug(error.name()),
        None => Ok(()),
    }
}

/// Adds `bias` to every local access's `disp32` (`frame::end`).
///
/// # Errors
/// `Internal` for a field outside the code.
pub fn bias_local_sites(cx: &mut FnCx<'_, X86>, bias: i64) -> CgResult<()> {
    let ok = LOCAL_SITES.with(|sites| {
        let code = cx.code_mut();
        sites.borrow().iter().all(|&at| {
            let at = at as usize;
            mc::Fixup::Abs32
                .extract(code, at)
                .is_some_and(|disp| mc::Fixup::Abs32.patch(code, at, disp + bias))
        })
    });
    if ok { Ok(()) } else { cx.bug("a local access whose displacement cannot be biased") }
}

/// The type of `value`.
#[inline]
pub fn value_ty(cx: &FnCx<'_, X86>, value: Value) -> TypeId {
    cx.body.value_type(cx.module, value)
}

/// The width of a `Gpr`-class value: an integer's, 32 for a pointer.
pub fn width(cx: &FnCx<'_, X86>, value: Value) -> u32 {
    cx.types().int_bits(value_ty(cx, value)).unwrap_or(32)
}

/// The integer constant `value` is, if it is one (its low 32 bits;
/// `undef`, `poison`, `null` and zero are 0).
pub fn const_int(cx: &FnCx<'_, X86>, value: Value) -> Option<u32> {
    let ValueKind::Const(id) = value.kind() else { return None };
    match cx.module.consts.get(id) {
        Const::Int { value, .. } => Some(value as u32),
        Const::Null(_) | Const::Zero(_) | Const::Undef(_) | Const::Poison(_) => Some(0),
        _ => None,
    }
}

// ---- frame places ----

/// The memory operand of `place`: a local `[ebp + o]` with a `disp32`
/// that `frame::end` biases, an incoming argument `[ebp + 8 + n]`, an
/// outgoing one `[esp + n]`.
fn frame_mem(place: FrameRef) -> (Mem, bool) {
    match place {
        FrameRef::Local(o) => {
            let mem = Mem::base(EBP, o as i32);
            (mem.with_abs32().unwrap_or(mem), true)
        }
        FrameRef::Incoming(n) => (Mem::base(EBP, INCOMING_BASE.wrapping_add(n as i32)), false),
        FrameRef::Outgoing(n) => (Mem::base(ESP, n as i32), false),
        // A coroutine's heap frame (WP-335, `spec/codegen-x86.md` §18):
        // `[esi + o]`.
        FrameRef::Coro(o) => (crate::coro_body::heap_mem(o), false),
    }
}

/// Emits the instruction `make` builds over the operand of `place`.
pub fn frame_inst(cx: &mut FnCx<'_, X86>, place: FrameRef, make: impl FnOnce(Mem) -> Inst) {
    let place = crate::ssp::incoming(cx, place); // WP-324: the arguments' copy
    let (mem, local) = frame_mem(place);
    crate::coro_body::access(cx, place);
    let inst = make(mem);
    let at = cx.offset();
    put(cx, inst);
    if local
        && let Ok(admitted) = Mode::X86.admit(inst)
        && let Some(site) = admitted.site()
    {
        LOCAL_SITES.with(|s| s.borrow_mut().push(at + site.offset as u32));
    }
}

/// `lea rd, place`.
pub fn frame_address(cx: &mut FnCx<'_, X86>, rd: Reg, of: FrameRef) {
    frame_inst(cx, of, |m| mc::lea(Size::D, rd, m));
}

/// The address of body slot `slot` in `rd`.
///
/// # Errors
/// `Internal` for a slot the body does not have.
pub fn slot_address(cx: &mut FnCx<'_, X86>, rd: Reg, slot: hod_ir::SlotId) -> CgResult<()> {
    match cx.slot_place(slot)? {
        SlotPlace::Direct(place) => frame_address(cx, rd, place),
        SlotPlace::Indirect(pointer) => frame_inst(cx, pointer, |m| mc::mov_rm(Size::D, rd, m)),
    }
    Ok(())
}

/// The scratch register (`ecx`, `edx`) that is not `reg`.
pub fn other_scratch(reg: Reg) -> Reg {
    if reg == ECX { EDX } else { ECX }
}

/// The load of `bytes` (1 to 4) bytes at `m` into `rd`; for 3 bytes also
/// writes `aux` (and the flags).
pub fn load_at(cx: &mut FnCx<'_, X86>, rd: Reg, bytes: u32, aux: Reg, m: impl Fn(i32) -> Mem) {
    match bytes {
        0 | 1 => put(cx, mc::movzx_rm(Size::D, rd, Size::B, m(0))),
        2 => put(cx, mc::movzx_rm(Size::D, rd, Size::W, m(0))),
        3 => {
            put(cx, mc::movzx_rm(Size::D, rd, Size::W, m(0)));
            put(cx, mc::movzx_rm(Size::D, aux, Size::B, m(2)));
            put(cx, mc::shl_ri(Size::D, aux, 16));
            put(cx, mc::or_rr(Size::D, rd, aux));
        }
        _ => put(cx, mc::mov_rm(Size::D, rd, m(0))),
    }
}

/// The store of the low `bytes` (1 to 4) bytes of `rs` at `m`; a byte
/// of `esi`/`edi` goes through `aux` (as for the third byte).
pub fn store_at(cx: &mut FnCx<'_, X86>, rs: Reg, bytes: u32, aux: Reg, m: impl Fn(i32) -> Mem) {
    match bytes {
        0 | 1 if has_low_byte(rs) => put(cx, mc::mov_mr(Size::B, m(0), rs)),
        0 | 1 => {
            put(cx, mc::mov_rr(Size::D, aux, rs));
            put(cx, mc::mov_mr(Size::B, m(0), aux));
        }
        2 => put(cx, mc::mov_mr(Size::W, m(0), rs)),
        3 => {
            put(cx, mc::mov_mr(Size::W, m(0), rs));
            put(cx, mc::mov_rr(Size::D, aux, rs));
            put(cx, mc::shr_ri(Size::D, aux, 16));
            put(cx, mc::mov_mr(Size::B, m(2), aux));
        }
        _ => put(cx, mc::mov_mr(Size::D, m(0), rs)),
    }
}

/// A frame place `k` bytes on, as a displacement closure's operand.
fn frame_inst_at(cx: &mut FnCx<'_, X86>, place: FrameRef, k: u32, make: impl FnOnce(Mem) -> Inst) {
    frame_inst(cx, place.offset_by(k), make);
}

/// Glue `load`: `bytes` (1 to 4) of `src` into `rd`.
pub fn load_gpr(cx: &mut FnCx<'_, X86>, rd: Reg, src: FrameRef, bytes: u32) {
    if bytes == 3 {
        let aux = other_scratch(rd);
        frame_inst(cx, src, |m| mc::movzx_rm(Size::D, rd, Size::W, m));
        frame_inst_at(cx, src, 2, |m| mc::movzx_rm(Size::D, aux, Size::B, m));
        put(cx, mc::shl_ri(Size::D, aux, 16));
        put(cx, mc::or_rr(Size::D, rd, aux));
        return;
    }
    frame_inst(cx, src, |m| match bytes {
        0 | 1 => mc::movzx_rm(Size::D, rd, Size::B, m),
        2 => mc::movzx_rm(Size::D, rd, Size::W, m),
        _ => mc::mov_rm(Size::D, rd, m),
    });
}

/// Glue `store`: the low `bytes` (1 to 4) of `rs` to `dst`.
pub fn store_gpr(cx: &mut FnCx<'_, X86>, rs: Reg, dst: FrameRef, bytes: u32) {
    let aux = other_scratch(rs);
    match bytes {
        0 | 1 if !has_low_byte(rs) => {
            put(cx, mc::mov_rr(Size::D, aux, rs));
            frame_inst(cx, dst, |m| mc::mov_mr(Size::B, m, aux));
        }
        0 | 1 => frame_inst(cx, dst, |m| mc::mov_mr(Size::B, m, rs)),
        2 => frame_inst(cx, dst, |m| mc::mov_mr(Size::W, m, rs)),
        3 => {
            frame_inst(cx, dst, |m| mc::mov_mr(Size::W, m, rs));
            put(cx, mc::mov_rr(Size::D, aux, rs));
            put(cx, mc::shr_ri(Size::D, aux, 16));
            frame_inst_at(cx, dst, 2, |m| mc::mov_mr(Size::B, m, aux));
        }
        _ => frame_inst(cx, dst, |m| mc::mov_mr(Size::D, m, rs)),
    }
}

/// The pieces of a copy of `bytes` bytes: (offset, size) of 4, 2, 1.
fn pieces(bytes: u32) -> impl Iterator<Item = (u32, u32)> {
    let mut at = 0;
    std::iter::from_fn(move || {
        let left = bytes - at;
        let size = match left {
            0 => return None,
            1 => 1,
            2 | 3 => 2,
            _ => 4,
        };
        at += size;
        Some((at - size, size))
    })
}

/// Glue `copy`: `bytes` from `src` to `dst` through `ecx`.
pub fn copy_frame(cx: &mut FnCx<'_, X86>, dst: FrameRef, src: FrameRef, bytes: u32) {
    for (at, size) in pieces(bytes) {
        load_gpr(cx, ECX, src.offset_by(at), size);
        store_gpr(cx, ECX, dst.offset_by(at), size);
    }
}

/// Glue `copy_from_ptr`: `bytes` at `[ptr + offset]` to `dst`, through
/// the scratch register that is not `ptr`.
pub fn copy_from_ptr(cx: &mut FnCx<'_, X86>, dst: FrameRef, ptr: Reg, offset: u32, bytes: u32) {
    let t = other_scratch(ptr);
    for (at, size) in pieces(bytes) {
        let disp = offset.wrapping_add(at) as i32;
        load_at(cx, t, size, t, |k| Mem::base(ptr, disp + k));
        store_gpr(cx, t, dst.offset_by(at), size);
    }
}

// ---- constants and symbols ----

/// `mov rd, imm32`.
pub fn mov_imm(cx: &mut FnCx<'_, X86>, rd: Reg, value: u32) {
    put(cx, mc::mov_ri(Size::D, rd, i64::from(value)));
}

/// Emits `inst`, whose site is an absolute `disp32` or immediate, with
/// `R_386_32` against `target` + `addend` there.
///
/// # Errors
/// `Internal` for an instruction without a relocatable field.
pub fn put_abs(cx: &mut FnCx<'_, X86>, inst: Inst, target: SymRef, addend: i64) -> CgResult<()> {
    let Some(site) = Mode::X86.admit(inst).ok().and_then(|i| i.site()) else {
        return cx.bug("an absolute reference without a relocatable field");
    };
    let at = cx.offset();
    put(cx, inst);
    cx.reloc(at + site.offset as u32, RelocKind::I386(I386Reloc::Abs32), target, addend);
    Ok(())
}

/// The memory operand `[sym + addend]` for `make` by the access choice
/// (`crate::pic::mem_inst`): `R_386_32` outside PIC; in PIC through the
/// GOT pointer in `eax`, so `make` must not read `eax`; a thread-local
/// symbol is `tls`'s.
///
/// # Errors
/// What [`crate::tls`] and [`crate::pic`] refuse.
pub fn sym_inst(
    cx: &mut FnCx<'_, X86>,
    sym: SymId,
    addend: i64,
    make: impl FnOnce(Mem) -> Inst,
) -> CgResult<()> {
    crate::pic::mem_inst(cx, sym, addend, make)
}

/// The address of `sym` + `addend` in `rd` by the access choice
/// (`crate::pic::address`); writes `rd` only, the flags kept.
///
/// # Errors
/// As [`sym_inst`].
pub fn sym_address(cx: &mut FnCx<'_, X86>, rd: Reg, sym: SymId, addend: i64) -> CgResult<()> {
    crate::pic::address(cx, rd, sym, addend)
}

/// A constant that is no integer and is no address (an aggregate, a
/// floating-point constant reaching a general register).
pub const CONSTANT: &str = "x86:constant";

/// Puts constant `id` in `rd`.
///
/// # Errors
/// [`CONSTANT`]; what [`sym_address`] refuses.
pub fn materialize_const(cx: &mut FnCx<'_, X86>, rd: Reg, id: ConstId) -> CgResult<()> {
    match cx.module.consts.get(id) {
        Const::Int { value, .. } | Const::AddrOfInt { value } => mov_imm(cx, rd, value as u32),
        Const::Null(_) | Const::Zero(_) | Const::Undef(_) | Const::Poison(_) => {
            mov_imm(cx, rd, 0);
        }
        Const::Addr { sym, addend } => sym_address(cx, rd, sym, addend)?,
        Const::IntOfAddr { addr, .. } => return materialize_const(cx, rd, addr),
        Const::DsoLocalEquivalent(sym) | Const::NoCfi(sym) => sym_address(cx, rd, sym, 0)?,
        Const::BlockAddr { func, block } => crate::ibr::block_address(cx, rd, func, block)?,
        _ => return unsupported(CONSTANT),
    }
    Ok(())
}

/// Puts an operand without a location in `rd`: a constant, a symbol's
/// address, a stack slot's address.
///
/// # Errors
/// As [`materialize_const`]; `Internal` for something else.
pub fn materialize_gpr(cx: &mut FnCx<'_, X86>, rd: Reg, value: Value) -> CgResult<()> {
    match value.kind() {
        ValueKind::Const(id) => materialize_const(cx, rd, id),
        ValueKind::Sym(sym) => sym_address(cx, rd, sym, 0),
        ValueKind::Slot(slot) => slot_address(cx, rd, slot),
        _ => cx.bug("materialize of something that is no constant"),
    }
}

// ---- branches ----

/// `jmp label`: `rel8` backward when it reaches, else `rel32` (relaxed
/// once the function is complete).
///
/// # Errors
/// What [`FnCx::fixup`] reports.
pub fn branch(cx: &mut FnCx<'_, X86>, label: Label) -> CgResult<()> {
    let at = cx.offset();
    if let Some(target) = cx.label_offset(label)
        && let Ok(disp) = i8::try_from(i64::from(target) - i64::from(at + 2))
    {
        put(cx, mc::jmp_short(disp));
        return cx.fixup(at + 1, REL8, label);
    }
    put(cx, mc::jmp_near(0));
    cx.fixup(at + 1, REL32, label)?;
    cx.relaxable(at, 0xeb, REL8);
    Ok(())
}

/// `jcc label`, as [`branch`].
///
/// # Errors
/// What [`FnCx::fixup`] reports.
pub fn branch_cond(cx: &mut FnCx<'_, X86>, cond: Cond, label: Label) -> CgResult<()> {
    let at = cx.offset();
    if let Some(target) = cx.label_offset(label)
        && let Ok(disp) = i8::try_from(i64::from(target) - i64::from(at + 2))
    {
        put(cx, mc::jcc_short(cond, disp));
        return cx.fixup(at + 1, REL8, label);
    }
    put(cx, mc::jcc_near(cond, 0));
    cx.fixup(at + 2, REL32, label)?;
    cx.relaxable(at, 0x70 | cond as u8, REL8);
    Ok(())
}

/// `Target::patch_branch`.
pub fn patch_branch(code: &mut [u8], at: u32, kind: FixupKind, disp: i64) -> bool {
    let field = match kind {
        REL32 => mc::Fixup::Rel32,
        REL8 => mc::Fixup::Rel8,
        _ => return false,
    };
    field.patch(code, at as usize, disp - field.size() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_are_cut_in_words_halves_and_bytes() {
        assert_eq!(pieces(7).collect::<Vec<_>>(), [(0, 4), (4, 2), (6, 1)]);
        assert_eq!(pieces(3).collect::<Vec<_>>(), [(0, 2), (2, 1)]);
        assert_eq!(pieces(0).count(), 0);
    }

    #[test]
    fn a_local_access_keeps_its_disp32_in_32_bit_mode() {
        let m = Mem::base(EBP, 0).with_abs32().unwrap();
        let inst = Mode::X86.admit(mc::mov_rm(Size::D, Reg::Rbx, m)).unwrap();
        assert_eq!(inst.bytes(), [0x8b, 0x9d, 0, 0, 0, 0]);
        assert_eq!(inst.site().map(|s| s.offset), Some(2));
    }
}
