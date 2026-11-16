//! Position-independent code (`spec/codegen-x86.md` "Position-independent
//! code"): the access choice of every symbol reference, the GOT pointer,
//! `R_386_GOTPC` / `GOTOFF` / `GOT32X`, and `ebx` before `@PLT` calls.
//! Design 0008's I8 (WP-299) owns this file.
//!
//! **The access choice** ([`choice`]) follows `llc` 19 and 22
//! (`X86Subtarget::classifyGlobalReference` for 32-bit ELF), on top of
//! `hod_cg::sym_access`:
//!
//! | relocation model | `dso_local` symbol | preemptible symbol |
//! |---|---|---|
//! | static, dynamic-no-pic | `[sym]`, `R_386_32` | `[sym]`, `R_386_32` |
//! | PIC (program or shared object) | `sym@GOTOFF(got)`, `R_386_GOTOFF` | `sym@GOT(got)`, `R_386_GOT32X`, then the access |
//!
//! Functions and data are chosen alike; a call is [`emit_call`]'s.
//!
//! **The GOT pointer** is computed where it is needed, by the thunk-free
//! sequence `llc` writes at `-O0` (both majors, `__x86.get_pc_thunk.*`
//! is GCC's):
//!
//! ```text
//! call .L          e8 00 00 00 00   (no relocation: the next instruction)
//! .L: pop r
//! lea r, [r + _GLOBAL_OFFSET_TABLE_ + (field - .L)]   R_386_GOTPC, addend 3
//! ```
//!
//! `llc` writes `addl` for the last instruction; `lea` keeps the flags (a
//! symbol address may be materialized between a compare and its branch)
//! and its 32-bit field is at the same place, so the addend is the same.
//! hod keeps no GOT pointer between instructions: a frame slot or a
//! reserved register would cost one of the six allocatable registers or a
//! `hod_cg` hook, and recomputing it is 11 bytes.
//!
//! **Calls.** In PIC a call through the PLT needs `ebx` to hold the GOT
//! address (i386 psABI): [`emit_call`] displaces the value `ebx` holds,
//! if any (`FnCx::take_reg`), computes the GOT pointer in `ebx` and calls
//! `f@PLT` (`R_386_PLT32`). A `dso_local` callee is `call f`
//! (`R_386_PC32`), without `ebx`. Outside PIC a preemptible callee is
//! `call f@PLT` too, without `ebx` (as `llc`). `nonlazybind` (`-fno-plt`)
//! does not change `llc`'s 32-bit call at `-O0`, nor hod's.
//!
//! **Jump tables** do not exist (`switch` is a compare chain, `control`);
//! **constant pools** do not exist yet: I9's constants in `.rodata.cst*`
//! go through [`mem_inst`] with the pool's symbol, which gives them
//! `@GOTOFF` under PIC as `llc` does (the hook is [`mem_inst`] itself).

use hod_cg::{Access, CallTarget, CgResult, FnCx, SymRef};
use hod_ir::SymId;
use hod_mc_x64 as mc;
use hod_mc_x64::{Inst, Mem, Mode, Reg, Size};
use hod_obj::{I386Reloc, RelocKind};

use crate::common::{EAX, put, put_abs};
use crate::target::X86;

/// The refusal of a relocation model other than static before WP-299; no
/// lowering returns it any more (kept for the names tests refer to).
pub const PIC: &str = "x86:pic";

/// How code reaches a symbol that is not thread-local (module docs).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Choice {
    /// Its absolute address: `R_386_32`.
    Abs,
    /// Its offset from the GOT, added to the GOT pointer: `R_386_GOTOFF`.
    GotOff,
    /// Loaded from its GOT entry, at an offset from the GOT pointer:
    /// `R_386_GOT32X`.
    Got,
}

/// The access choice for `sym` (module docs). A thread-local symbol is
/// `tls`'s; it is never asked here.
pub fn choice(cx: &FnCx<'_, X86>, sym: SymId) -> Choice {
    if !cx.options.reloc_model.is_pic() {
        return Choice::Abs;
    }
    match hod_cg::sym_access(cx.module, cx.options, sym) {
        Access::Direct => Choice::GotOff,
        Access::Got | Access::Tls(_) => Choice::Got,
    }
}

/// The relocation models the back end compiles: all of them since
/// WP-299 (static, dynamic-no-pic as static, PIC).
///
/// # Errors
/// None.
pub fn check(_cx: &FnCx<'_, X86>) -> CgResult<()> {
    Ok(())
}

/// Emits `inst` with a relocation of `kind` against `target` + `addend`
/// at its relocatable field.
///
/// # Errors
/// `Internal` for an instruction without a relocatable field.
pub fn put_reloc(
    cx: &mut FnCx<'_, X86>,
    inst: Inst,
    kind: I386Reloc,
    target: SymRef,
    addend: i64,
) -> CgResult<()> {
    let Some(site) = Mode::X86.admit(inst).ok().and_then(|i| i.site()) else {
        return cx.bug("a relocated instruction without a relocatable field");
    };
    let at = cx.offset();
    put(cx, inst);
    cx.reloc(at + site.offset as u32, RelocKind::I386(kind), target, addend);
    Ok(())
}

/// `[r + disp32]` with the displacement a relocatable field.
fn field(r: Reg) -> Mem {
    let m = Mem::base(r, 0);
    m.with_abs32().unwrap_or(m)
}

/// The GOT's address in `r` (module docs): `call .L; .L: pop r; lea r,
/// [r + _GLOBAL_OFFSET_TABLE_ + 3]`. Writes `r` and the word below `esp`;
/// the flags kept.
///
/// # Errors
/// `Internal` only for an encoder without a relocatable field.
pub fn got_pointer(cx: &mut FnCx<'_, X86>, r: Reg) -> CgResult<()> {
    // From `.L` (the `pop`, one byte) to the `lea`'s field (opcode, ModRM).
    const FIELD_FROM_LABEL: i64 = 3;
    put(cx, mc::call(0));
    put(cx, mc::pop_r(r));
    let got = SymRef::Runtime(hod_cg::RuntimeFn::GlobalOffsetTable);
    put_reloc(cx, mc::lea(Size::D, r, field(r)), I386Reloc::GotPc, got, FIELD_FROM_LABEL)
}

/// The address of `sym` + `addend` in `rd` (the body of
/// `common::sym_address`): `lea rd, [sym + addend]`, or the GOT pointer in
/// `rd` then `lea rd, [rd + sym@GOTOFF + addend]` or `mov rd, [rd +
/// sym@GOT]` and `lea rd, [rd + addend]`; a thread-local symbol by
/// `tls::address`. Writes `rd` and nothing else (the word below `esp`
/// aside); the flags kept.
///
/// # Errors
/// What `tls::address` refuses; `Internal` for an encoder inconsistency.
pub fn address(cx: &mut FnCx<'_, X86>, rd: Reg, sym: SymId, addend: i64) -> CgResult<()> {
    if let Access::Tls(model) = hod_cg::sym_access(cx.module, cx.options, sym) {
        return crate::tls::address(cx, rd, sym, model, addend);
    }
    let target = SymRef::Module(sym);
    match choice(cx, sym) {
        Choice::Abs => put_abs(cx, mc::lea(Size::D, rd, field_abs()), target, addend),
        Choice::GotOff => {
            got_pointer(cx, rd)?;
            put_reloc(cx, mc::lea(Size::D, rd, field(rd)), I386Reloc::GotOff, target, addend)
        }
        Choice::Got => {
            got_pointer(cx, rd)?;
            put_reloc(cx, mc::mov_rm(Size::D, rd, field(rd)), I386Reloc::Got32X, target, 0)?;
            add_addend(cx, rd, addend);
            Ok(())
        }
    }
}

/// `[disp32]`, the displacement a relocatable field.
fn field_abs() -> Mem {
    Mem::abs(0).with_abs32().unwrap_or(Mem::abs(0))
}

/// `lea rd, [rd + addend]` unless `addend` is 0 (modulo 2^32); the flags
/// kept.
pub fn add_addend(cx: &mut FnCx<'_, X86>, rd: Reg, addend: i64) {
    let addend = addend as i32;
    if addend != 0 {
        put(cx, mc::lea(Size::D, rd, Mem::base(rd, addend)));
    }
}

/// Whether `inst`, built over a memory operand based on `eax`, also names
/// `eax` as a register it reads (the `ModRM.reg` field of a store or an
/// arithmetic operation): [`mem_inst`]'s contract forbids it.
fn reads_eax(inst: Inst) -> bool {
    let Ok(admitted) = Mode::X86.admit(inst) else { return false };
    let bytes = admitted.bytes();
    let mut at = 0;
    while bytes.get(at).is_some_and(|b| matches!(b, 0x66 | 0x65 | 0x64 | 0xf0)) {
        at += 1;
    }
    let (Some(&op), Some(&modrm)) = (bytes.get(at), bytes.get(at + 1)) else { return false };
    // Loads (`mov`, `lea`, `movzx`/`movsx` behind `0F`) write their
    // register after the address is formed; groups (`/digit`) name none.
    let writes_only = matches!(op, 0x8a | 0x8b | 0x8d | 0x0f);
    let group = matches!(
        op,
        0x80..=0x83 | 0xc0 | 0xc1 | 0xc6 | 0xc7 | 0xd0..=0xd3 | 0xf6 | 0xf7 | 0xfe | 0xff
    );
    let reg_field = (modrm >> 3) & 7;
    !writes_only && !group && reg_field == 0
}

/// The instruction `make` builds over the memory operand `[sym + addend]`
/// (the body of `common::sym_inst`): `[sym + addend]` (`R_386_32`)
/// outside PIC; in PIC the GOT pointer in `eax`, then
/// `[eax + sym@GOTOFF + addend]`, or `mov eax, [eax + sym@GOT]` and
/// `[eax + addend]`. A thread-local symbol's address is put in `eax` by
/// `tls::address`.
///
/// Contract: in PIC this writes `eax` (and the word below `esp`), so
/// `eax` holds nothing live and `make` names it as no register it reads;
/// a store or an operation of `eax` over a symbol is `Internal`.
///
/// # Errors
/// What `tls::address` refuses; `Internal` as above.
pub fn mem_inst(
    cx: &mut FnCx<'_, X86>,
    sym: SymId,
    addend: i64,
    make: impl FnOnce(Mem) -> Inst,
) -> CgResult<()> {
    let target = SymRef::Module(sym);
    if let Access::Tls(model) = hod_cg::sym_access(cx.module, cx.options, sym) {
        crate::tls::address(cx, EAX, sym, model, addend)?;
        return through_eax(cx, make(Mem::base(EAX, 0)));
    }
    match choice(cx, sym) {
        Choice::Abs => put_abs(cx, make(field_abs()), target, addend),
        Choice::GotOff => {
            let inst = make(field(EAX));
            if reads_eax(inst) {
                return cx.bug("an access over a symbol that reads eax (PIC)");
            }
            got_pointer(cx, EAX)?;
            put_reloc(cx, inst, I386Reloc::GotOff, target, addend)
        }
        Choice::Got => {
            let inst = make(Mem::base(EAX, addend as i32));
            if reads_eax(inst) {
                return cx.bug("an access over a symbol that reads eax (PIC)");
            }
            got_pointer(cx, EAX)?;
            put_reloc(cx, mc::mov_rm(Size::D, EAX, field(EAX)), I386Reloc::Got32X, target, 0)?;
            put(cx, inst);
            Ok(())
        }
    }
}

/// Emits `inst`, an access over `[eax]`, unless it reads `eax`.
fn through_eax(cx: &mut FnCx<'_, X86>, inst: Inst) -> CgResult<()> {
    if reads_eax(inst) {
        return cx.bug("an access over a symbol that reads eax (TLS)");
    }
    put(cx, inst);
    Ok(())
}

/// `Target::emit_call` (module docs): in PIC, a call through the PLT with
/// the GOT pointer in `ebx` (the value `ebx` held displaced first); every
/// other call is `frame::emit_call`'s.
///
/// # Errors
/// `Internal` for an encoder inconsistency or a pinned `ebx`.
pub fn emit_call(cx: &mut FnCx<'_, X86>, target: CallTarget) -> CgResult<()> {
    let plt = match target {
        CallTarget::Sym(SymRef::Module(id)) => {
            hod_cg::call_access(cx.module, id) == hod_cg::CallAccess::Plt
        }
        CallTarget::Sym(_) => true,
        CallTarget::Reg(_) => false,
    };
    if cx.options.reloc_model.is_pic() && plt {
        cx.take_reg(crate::reg::EBX)?;
        got_pointer(cx, Reg::Rbx)?;
        crate::frame::emit_call(cx, target)?;
        cx.free_temp(crate::reg::EBX);
        return Ok(());
    }
    crate::frame::emit_call(cx, target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_got_pointer_field_is_three_bytes_past_the_label() {
        for r in [Reg::Rax, Reg::Rbx, Reg::Rcx, Reg::Rsi] {
            let lea = Mode::X86.admit(mc::lea(Size::D, r, field(r))).unwrap();
            assert_eq!(lea.site().map(|s| s.offset), Some(2), "{r:?}");
            assert_eq!(Mode::X86.admit(mc::pop_r(r)).unwrap().bytes().len(), 1);
        }
        assert_eq!(Mode::X86.admit(mc::call(0)).unwrap().bytes(), [0xe8, 0, 0, 0, 0]);
    }

    #[test]
    fn stores_of_eax_over_eax_are_caught() {
        let m = field(EAX);
        assert!(reads_eax(mc::mov_mr(Size::D, m, EAX)));
        assert!(!reads_eax(mc::mov_mr(Size::D, m, Reg::Rcx)));
        assert!(!reads_eax(mc::mov_rm(Size::D, EAX, m)));
        assert!(!reads_eax(mc::lea(Size::D, EAX, m)));
        assert!(!reads_eax(mc::movzx_rm(Size::D, EAX, Size::B, m)));
        assert!(reads_eax(mc::mov_mr(Size::W, m, EAX)));
    }
}
