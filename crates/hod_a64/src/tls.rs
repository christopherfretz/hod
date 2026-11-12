//! Thread-local storage (`spec/codegen.md` §5.5.1, "Thread-local storage
//! (WP-073)"): the address of a thread-local symbol by the model
//! `hod_cg::sym_access` names, and `llvm.threadlocal.address` and
//! `llvm.thread.pointer`. Owned by WP-073.
//!
//! Each model is the sequence AArch64 Linux `llc` writes, with the same
//! relocations, so the linkers recognize and relax it:
//!
//! ```text
//! general and local dynamic (TLS descriptors; LLVM lowers local dynamic so):
//!     adrp x0, :tlsdesc:sym                 R_AARCH64_TLSDESC_ADR_PAGE21
//!     ldr  x1, [x0, :tlsdesc_lo12:sym]      R_AARCH64_TLSDESC_LD64_LO12
//!     add  x0, x0, :tlsdesc_lo12:sym        R_AARCH64_TLSDESC_ADD_LO12
//!     blr  x1                               R_AARCH64_TLSDESC_CALL
//!     mrs  x1, tpidr_el0 ; add rd, x1, x0
//! initial exec:
//!     adrp rd, :gottprel:sym                R_AARCH64_TLSIE_ADR_GOTTPREL_PAGE21
//!     ldr  rd, [rd, :gottprel_lo12:sym]     R_AARCH64_TLSIE_LD64_GOTTPREL_LO12_NC
//!     mrs  t, tpidr_el0 ; add rd, t, rd
//! local exec (`llc`'s default TLS size, 24 bits):
//!     mrs  rd, tpidr_el0
//!     add  rd, rd, #:tprel_hi12:sym, lsl #12   R_AARCH64_TLSLE_ADD_TPREL_HI12
//!     add  rd, rd, #:tprel_lo12_nc:sym         R_AARCH64_TLSLE_ADD_TPREL_LO12_NC
//! ```
//!
//! The relocations carry no addend (as `llc`'s): an offset from the
//! symbol is added in code afterwards ([`common::add_addend`]).
//!
//! **A thread-local address is an operand like any other symbol address**
//! (§3): it may be formed at any point of a lowering, argument placement
//! included, and writes `rd` and nothing else. The descriptor call
//! clobbers `x0`, `x1` (the sequence's own), the flags and `x30` (`llc`'s
//! `TLSDESC_CALLSEQ` defines `NZCV`, `LR`, `X0`, `X1`; the resolver
//! preserves every other register, `CSR_AArch64_TLS_ELF`). So the
//! sequence saves `x0`, `x1` and `NZCV` below the stack pointer and
//! restores all but `rd`; `x30` holds no value in a body (the prologue
//! saved it, §5.5). Initial exec borrows `x0` (or `x1`) the same way for
//! the thread pointer. The stack pointer moves for the length of the
//! sequence only; the CFA is `x29`-based in a body, so no CFI row
//! changes.

use hod_cg::{CgResult, FnCx, SymRef, TlsAccess};
use hod_ir::{Inst, Intrinsic, SymId, Value, ValueKind};
use hod_mc_a64 as mc;
use hod_mc_a64::{Load, MemSize, Reg, Size, SysReg};
use hod_obj::{A64Reloc, RelocKind};

use crate::common::{SP, add_addend, deferred_first, put, put_all, xr};
use crate::target::A64;

/// The bytes the descriptor sequence keeps below the stack pointer:
/// `x0`, `x1`, then `NZCV` (16-byte aligned).
const SAVE_AREA: i64 = 32;

/// `add rd, rn, #0, lsl #12`: the instruction a `:tprel_hi12:`
/// relocation fills (`llvm-mc` writes the field 0 with the shift set).
fn add_hi12(rd: Reg, rn: Reg) -> u32 {
    mc::add_imm(Size::X, rd.sp(), rn.sp(), 0x1000) & !(0xfff << 10)
}

/// `x0`, unless that is `rd`: then `x1`.
fn other_than(rd: Reg) -> Reg {
    if rd == Reg::X0 { Reg::X1 } else { Reg::X0 }
}

/// Puts the address of thread-local `sym` plus `addend` in `rd` by the
/// model `model` (module docs). Writes `rd` and nothing else; records
/// the descriptor call as a call site (§7.1).
///
/// # Errors
/// None today; `CgResult` for symmetry with `common::sym_address`.
pub fn address(
    cx: &mut FnCx<'_, A64>,
    rd: Reg,
    sym: SymId,
    model: TlsAccess,
    addend: i64,
) -> CgResult<()> {
    let target = SymRef::Module(sym);
    let a64 = |kind| RelocKind::A64(kind);
    match model {
        TlsAccess::GeneralDynamic | TlsAccess::LocalDynamic => {
            let (x0, x1) = (Reg::X0, Reg::X1);
            put_all(
                cx,
                &[
                    mc::stp_pre(Size::X, x0.zr(), x1.zr(), SP, -SAVE_AREA),
                    mc::mrs(x1.zr(), SysReg::Nzcv),
                    mc::str_off(MemSize::X, x1.zr(), SP, 16),
                ],
            );
            let at = cx.offset();
            put_all(
                cx,
                &[
                    mc::adrp(x0.zr(), 0),
                    mc::ldr_off(Load::X, x1.zr(), x0.sp(), 0),
                    mc::add_imm(Size::X, x0.sp(), x0.sp(), 0),
                    mc::blr(x1.zr()),
                ],
            );
            cx.reloc(at, a64(A64Reloc::TlsdescAdrPage21), target, 0);
            cx.reloc(at + 4, a64(A64Reloc::TlsdescLd64Lo12), target, 0);
            cx.reloc(at + 8, a64(A64Reloc::TlsdescAddLo12), target, 0);
            cx.reloc(at + 12, a64(A64Reloc::TlsdescCall), target, 0);
            cx.record_call_site(at + 12, 4);
            put_all(
                cx,
                &[
                    mc::mrs(x1.zr(), SysReg::TpidrEl0),
                    mc::add_shifted(Size::X, rd.zr(), x1.zr(), x0.zr(), mc::Shift::Lsl, 0),
                ],
            );
            let flags = if rd == x1 { x0 } else { x1 };
            put_all(
                cx,
                &[mc::ldr_off(Load::X, flags.zr(), SP, 16), mc::msr(SysReg::Nzcv, flags.zr())],
            );
            match rd {
                Reg::X0 => put(cx, mc::ldr_off(Load::X, x1.zr(), SP, 8)),
                Reg::X1 => put(cx, mc::ldr_off(Load::X, x0.zr(), SP, 0)),
                _ => put(cx, mc::ldp_off(Size::X, x0.zr(), x1.zr(), SP, 0)),
            }
            put(cx, mc::add_imm(Size::X, SP, SP, SAVE_AREA as u64));
        }
        TlsAccess::InitialExec => {
            let at = cx.offset();
            put_all(cx, &[mc::adrp(rd.zr(), 0), mc::ldr_off(Load::X, rd.zr(), rd.sp(), 0)]);
            cx.reloc(at, a64(A64Reloc::TlsieAdrGottprelPage21), target, 0);
            cx.reloc(at + 4, a64(A64Reloc::TlsieLd64GottprelLo12Nc), target, 0);
            let t = other_than(rd);
            put_all(
                cx,
                &[
                    mc::str_pre(MemSize::X, t.zr(), SP, -16),
                    mc::mrs(t.zr(), SysReg::TpidrEl0),
                    mc::add_shifted(Size::X, rd.zr(), t.zr(), rd.zr(), mc::Shift::Lsl, 0),
                    mc::ldr_post(Load::X, t.zr(), SP, 16),
                ],
            );
        }
        TlsAccess::LocalExec => {
            put(cx, mc::mrs(rd.zr(), SysReg::TpidrEl0));
            let at = cx.offset();
            put_all(cx, &[add_hi12(rd, rd), mc::add_imm(Size::X, rd.sp(), rd.sp(), 0)]);
            cx.reloc(at, a64(A64Reloc::TlsleAddTprelHi12), target, 0);
            cx.reloc(at + 4, a64(A64Reloc::TlsleAddTprelLo12Nc), target, 0);
        }
    }
    add_addend(cx, rd, addend);
    Ok(())
}

/// `llvm.threadlocal.address(sym)` and `llvm.thread.pointer()`: the hook
/// of the dispatch (`target.rs`).
///
/// # Errors
/// `Internal` for an operand list the verifier rejects.
pub fn intrinsic<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    match (which, args) {
        (Intrinsic::ThreadLocalAddress, &[ptr]) => {
            let rd = cx.def_reg(inst)?;
            match ptr.kind() {
                ValueKind::Sym(_) | ValueKind::Const(_) => {
                    cx.put(ptr, rd)?;
                }
                _ => return cx.bug("llvm.threadlocal.address of something that is no symbol"),
            }
            cx.finish(inst, rd)
        }
        (Intrinsic::ThreadPointer, &[]) => {
            let rd = cx.def_reg(inst)?;
            put(cx, mc::mrs(xr(rd).zr(), SysReg::TpidrEl0));
            cx.finish(inst, rd)
        }
        _ => cx.bug("a thread-local intrinsic with the wrong operands"),
    }
}
