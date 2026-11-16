//! Thread-local storage (`spec/codegen-x86.md` "Thread-local storage"):
//! the address of a thread-local symbol by the model `hod_cg::sym_access`
//! names, `llvm.threadlocal.address` and `llvm.thread.pointer`. Design
//! 0008's I8 (WP-299) owns this file.
//!
//! The thread pointer is `%gs:0` (the TCB holds its own address). Each
//! static model is the instruction `llc` 19 and 22 write with the same
//! relocation, in the forms the linkers recognize when they relax initial
//! exec to local exec:
//!
//! ```text
//! local exec:
//!     mov rd, gs:[0]                       65 8B ModRM 00 00 00 00
//!     lea rd, [rd + x@NTPOFF + addend]     R_386_TLS_LE
//! initial exec, static (`movl x@INDNTPOFF, %reg`: `A1` for eax, else
//! `8B` with ModRM r/m=101, the forms GNU ld, lld and mold relax):
//!     mov rd, [x@INDNTPOFF]                R_386_TLS_IE
//! initial exec, PIC (`movl x@GOTNTPOFF(%reg), %reg`, disp32):
//!     <GOT pointer in rd>                  R_386_GOTPC (`pic`)
//!     mov rd, [rd + x@GOTNTPOFF]           R_386_TLS_GOTIE
//! then, both:
//!     push t ; mov t, gs:[0] ; lea rd, [rd + t] ; pop t
//!     lea rd, [rd + addend]
//! ```
//!
//! `t` is `ecx`, or `edx` when `rd` is `ecx`: the sequence writes `rd`
//! and nothing else, the flags kept, as every symbol address (`pic`).
//! The relocations of initial exec carry no addend (a GOT entry's).
//!
//! The two dynamic models (general and local dynamic: a shared object's
//! thread-locals) call `___tls_get_addr` (three underscores: the GNU
//! `regparm` entry, its argument in `eax`, `hod_cg::RuntimeFn::
//! TlsGetAddrRegparm`) in the byte forms `llc` 19 and 22 write at `-O0`
//! and GNU ld, lld and mold relax (to initial or local exec in a
//! program); `ebx` holds the GOT pointer at the call, nothing comes
//! between the `lea` and the `call`:
//!
//! ```text
//! general dynamic:
//!     <GOT pointer in ebx>                 R_386_GOTPC (`pic`)
//!     lea eax, [ebx*1 + x@TLSGD]           8D 04 1D, R_386_TLS_GD
//!     call ___tls_get_addr@PLT             R_386_PLT32
//!     ; eax = &x
//! local dynamic:
//!     <GOT pointer in ebx>
//!     lea eax, [ebx + x@TLSLDM]            8D 83, R_386_TLS_LDM
//!     call ___tls_get_addr@PLT             R_386_PLT32
//!     lea rd, [eax + x@DTPOFF]             R_386_TLS_LDO_32
//! ```
//!
//! `llc` writes `call *___tls_get_addr@GOT(%ebx)` instead when the module
//! has `RtLibUseGOT` (`clang -fno-plt`); the `@PLT` form is valid there
//! too and hod always writes it.
//!
//! **Two ways in**, as on x86-64 (`hod_x64::tls`): `llvm.threadlocal.
//! address` is an instruction of its own and lowers a dynamic model as a
//! call (every register value displaced, `ebx` taken, the bare sequence).
//! A thread-local symbol as a plain operand reaches [`address`] through
//! `common::sym_address`, whose contract is to write `rd` and nothing
//! else, the flags kept, at any point of a lowering; `___tls_get_addr`
//! is an ordinary function that clobbers `eax`, `ecx`, `edx`, every
//! `xmm` register and the flags, and the sequence writes `ebx`. So the
//! operand form ([`save_and_call`]) keeps them in an area below `esp`
//! (`esp` is 16-aligned in a body, and stays so at the call; the CFA is
//! `ebp`-based, so no CFI row changes): `pushfd`, then `eax`, `ecx`,
//! `edx`, `ebx` but `rd`, and `xmm0`..`xmm7` (`movups`) when the function
//! has used an `xmm` register so far (none can hold a value otherwise,
//! and a CPU without SSE has none); `popfd` last. The x87 stack must be
//! empty there, as at any call.

use hod_cg::{CallTarget, CgResult, FnCx, SymRef, TlsAccess};
use hod_ir::{Inst, Intrinsic, SymId, Value, ValueKind};
use hod_mc_x64 as mc;
use hod_mc_x64::{FpSize, Mem, Reg, Seg, Size, Xmm};
use hod_obj::I386Reloc;

use crate::common::{EAX, ECX, EDX, ESP, gp, put};
use crate::pic::{add_addend, got_pointer, put_reloc};
use crate::target::X86;

/// `ebx`, the GOT pointer at the call.
const EBX: Reg = Reg::Rbx;

/// The general registers the operand form saves (but `rd`), in order:
/// what `___tls_get_addr` may clobber and `ebx`, which the sequence
/// writes.
const SAVED: [Reg; 4] = [EAX, ECX, EDX, EBX];

/// `[esp + disp]`.
fn sp(disp: i32) -> Mem {
    Mem::base(ESP, disp)
}

/// `gs:[0]`: the thread pointer's own address.
fn thread_pointer() -> Mem {
    Mem::abs(0).with_seg(Seg::Gs)
}

/// `mov rd, gs:[0]`: what `llvm.thread.pointer` returns.
pub fn load_thread_pointer(cx: &mut FnCx<'_, X86>, rd: Reg) {
    put(cx, mc::mov_rm(Size::D, rd, thread_pointer()));
}

/// `[r + disp32]`, the displacement a relocatable field.
fn field(r: Reg) -> Mem {
    let m = Mem::base(r, 0);
    m.with_abs32().unwrap_or(m)
}

/// `lea eax, [ebx*1 + disp32]`: general dynamic's first instruction (`8D
/// 04 1D`, the SIB form with no base the linkers recognize).
fn gd_lea() -> Option<mc::Inst> {
    let m = Mem::index(EBX.index()?, mc::Scale::S1, 0).with_abs32()?;
    Some(mc::lea(Size::D, EAX, m))
}

/// The dynamic sequence of `model` for `sym` (module docs), the GOT
/// pointer included: `eax` is then `&sym` (general dynamic) or the
/// module's block (local dynamic). Writes `ebx` and what a call writes;
/// the caller has displaced or saved the rest. Records the call site.
///
/// # Errors
/// `Internal` for an encoder inconsistency.
fn dynamic_pair(cx: &mut FnCx<'_, X86>, sym: SymId, model: TlsAccess) -> CgResult<()> {
    let target = SymRef::Module(sym);
    got_pointer(cx, EBX)?;
    if model == TlsAccess::GeneralDynamic {
        let Some(lea) = gd_lea() else { return cx.bug("ebx is an index register") };
        put_reloc(cx, lea, I386Reloc::TlsGd, target, 0)?;
    } else {
        put_reloc(cx, mc::lea(Size::D, EAX, field(EBX)), I386Reloc::TlsLdm, target, 0)?;
    }
    let at = cx.offset();
    let get_addr = SymRef::Runtime(hod_cg::RuntimeFn::TlsGetAddrRegparm);
    // `ebx` holds the GOT pointer already (the sequence's own).
    crate::frame::emit_call(cx, CallTarget::Sym(get_addr), 0)?;
    let len = cx.offset() - at;
    cx.record_call_site(at, len);
    Ok(())
}

/// `rd` = `&sym` from `eax` after [`dynamic_pair`]: `mov rd, eax`, or
/// `lea rd, [eax + sym@DTPOFF]` (`R_386_TLS_LDO_32`) for local dynamic.
fn dynamic_result(cx: &mut FnCx<'_, X86>, rd: Reg, sym: SymId, model: TlsAccess) -> CgResult<()> {
    if model == TlsAccess::LocalDynamic {
        let lea = mc::lea(Size::D, rd, field(EAX));
        put_reloc(cx, lea, I386Reloc::TlsLdo32, SymRef::Module(sym), 0)
    } else {
        if rd != EAX {
            put(cx, mc::mov_rr(Size::D, rd, EAX));
        }
        Ok(())
    }
}

/// The save area of the operand form, from `esp` (a multiple of 16, the
/// flags in its last four bytes, pushed by `pushfd`): `xmm0`..`xmm7` at
/// `16 * i` when saved, then the general registers at `4 * i`.
fn area(with_xmm: bool) -> (i32, i32) {
    if with_xmm { (160, 128) } else { (32, 0) }
}

/// A dynamic model as an operand (module docs): writes `rd` and nothing
/// else, the flags kept.
///
/// # Errors
/// `Internal` for an encoder inconsistency.
fn save_and_call(cx: &mut FnCx<'_, X86>, rd: Reg, sym: SymId, model: TlsAccess) -> CgResult<()> {
    let with_xmm = !cx.regs_used().intersection(crate::target::REG_INFO.fpr).is_empty();
    let (size, gpr_at) = area(with_xmm);
    let xmms = if with_xmm { &Xmm::ALL[..8] } else { &[] };
    put(cx, mc::pushfq());
    put(cx, mc::lea(Size::D, ESP, sp(4 - size)));
    let saved = SAVED.iter().enumerate().filter(|(_, r)| **r != rd);
    for (i, r) in saved.clone() {
        put(cx, mc::mov_mr(Size::D, sp(gpr_at + 4 * i as i32), *r));
    }
    for (i, x) in xmms.iter().enumerate() {
        put(cx, mc::movup_mr(FpSize::S, sp(16 * i as i32), *x));
    }
    dynamic_pair(cx, sym, model)?;
    dynamic_result(cx, rd, sym, model)?;
    for (i, x) in xmms.iter().enumerate() {
        put(cx, mc::movup_rm(FpSize::S, *x, sp(16 * i as i32)));
    }
    for (i, r) in saved {
        put(cx, mc::mov_rm(Size::D, *r, sp(gpr_at + 4 * i as i32)));
    }
    put(cx, mc::lea(Size::D, ESP, sp(size - 4)));
    put(cx, mc::popfq());
    Ok(())
}

/// Puts the address of thread-local `sym` plus `addend` in `rd` by
/// `model` (module docs). Writes `rd` and nothing else, the flags kept
/// (a dynamic model saves what its call clobbers).
///
/// # Errors
/// `Internal` for an encoder inconsistency.
pub fn address(
    cx: &mut FnCx<'_, X86>,
    rd: Reg,
    sym: SymId,
    model: TlsAccess,
    addend: i64,
) -> CgResult<()> {
    let target = SymRef::Module(sym);
    match model {
        TlsAccess::GeneralDynamic | TlsAccess::LocalDynamic => {
            save_and_call(cx, rd, sym, model)?;
            add_addend(cx, rd, addend);
            Ok(())
        }
        TlsAccess::LocalExec => {
            load_thread_pointer(cx, rd);
            put_reloc(cx, mc::lea(Size::D, rd, field(rd)), I386Reloc::TlsLe, target, addend)
        }
        TlsAccess::InitialExec => {
            if cx.options.reloc_model.is_pic() {
                got_pointer(cx, rd)?;
                let load = mc::mov_rm(Size::D, rd, field(rd));
                put_reloc(cx, load, I386Reloc::TlsGotIe, target, 0)?;
            } else {
                let abs = Mem::abs(0).with_abs32().unwrap_or(Mem::abs(0));
                put_reloc(cx, mc::mov_rm(Size::D, rd, abs), I386Reloc::TlsIe, target, 0)?;
            }
            let t = if rd == ECX { EDX } else { ECX };
            let Some(index) = t.index() else {
                return cx.bug("ecx and edx are index registers");
            };
            put(cx, mc::push_r(t));
            load_thread_pointer(cx, t);
            put(cx, mc::lea(Size::D, rd, Mem::base_index(rd, index, mc::Scale::S1, 0)));
            put(cx, mc::pop_r(t));
            add_addend(cx, rd, addend);
            Ok(())
        }
    }
}

/// The thread-local symbol of a dynamic model and the offset `ptr` names,
/// if it is one.
pub(crate) fn dynamic_symbol(
    cx: &FnCx<'_, X86>,
    ptr: Value,
) -> Option<(SymId, i64, TlsAccess)> {
    let (sym, addend) = match ptr.kind() {
        ValueKind::Sym(sym) => (sym, 0),
        ValueKind::Const(id) => match cx.module.consts.get(id) {
            hod_ir::Const::Addr { sym, addend } => (sym, addend),
            _ => return None,
        },
        _ => return None,
    };
    match hod_cg::sym_access(cx.module, cx.options, sym) {
        hod_cg::Access::Tls(model @ (TlsAccess::GeneralDynamic | TlsAccess::LocalDynamic)) => {
            Some((sym, addend, model))
        }
        _ => None,
    }
}

/// `llvm.threadlocal.address(ptr)` and `llvm.thread.pointer()`: the hook
/// of `call::lower`. A dynamic model is lowered as a call (module docs):
/// every register value displaced, `ebx` taken, the bare sequence; any
/// other operand (a static model at an offset) is put in the result's
/// register (`common::materialize_gpr` reaches [`address`]).
///
/// # Errors
/// `Internal` for an operand list the verifier rejects.
pub fn intrinsic<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    match (which, args) {
        (Intrinsic::ThreadLocalAddress, &[ptr]) => {
            if !matches!(ptr.kind(), ValueKind::Sym(_) | ValueKind::Const(_)) {
                return cx.bug("llvm.threadlocal.address of something that is no symbol");
            }
            let Some((sym, addend, model)) = dynamic_symbol(cx, ptr) else {
                let rd = cx.def_reg(inst)?;
                cx.put(ptr, rd)?;
                return cx.finish(inst, rd);
            };
            cx.displace_all()?;
            cx.take_reg(crate::reg::EBX)?;
            let rd = cx.def_reg(inst)?;
            dynamic_pair(cx, sym, model)?;
            let r = gp(rd);
            dynamic_result(cx, r, sym, model)?;
            add_addend(cx, r, addend);
            cx.free_temp(crate::reg::EBX);
            cx.finish(inst, rd)
        }
        (Intrinsic::ThreadPointer, &[]) => {
            let rd = cx.def_reg(inst)?;
            load_thread_pointer(cx, gp(rd));
            cx.finish(inst, rd)
        }
        _ => cx.bug("a thread-local intrinsic with the wrong operands"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hod_mc_x64::Mode;

    /// The forms the linkers relax (module docs), against `llvm-mc`.
    #[test]
    fn static_forms_are_llvm_mc_s() {
        let bytes = |i| Mode::X86.admit(i).unwrap().bytes().to_vec();
        let abs = Mem::abs(0).with_abs32().unwrap();
        assert_eq!(bytes(mc::mov_rm(Size::D, Reg::Rax, abs)), [0xa1, 0, 0, 0, 0]);
        assert_eq!(bytes(mc::mov_rm(Size::D, ECX, abs)), [0x8b, 0x0d, 0, 0, 0, 0]);
        assert_eq!(
            bytes(mc::mov_rm(Size::D, Reg::Rax, thread_pointer())),
            [0x65, 0xa1, 0, 0, 0, 0]
        );
        assert_eq!(bytes(mc::mov_rm(Size::D, ECX, field(Reg::Rbx))), [0x8b, 0x8b, 0, 0, 0, 0]);
    }

    /// The dynamic forms, as `llvm-mc` encodes `llc`'s `leal
    /// x@TLSGD(,%ebx), %eax`, `leal x@TLSLDM(%ebx), %eax`, `leal
    /// x@DTPOFF(%eax), %ecx`, and the flags' save and restore.
    #[test]
    fn dynamic_forms_are_llvm_mc_s() {
        let admit = |i| Mode::X86.admit(i).unwrap();
        let gd = admit(gd_lea().unwrap());
        assert_eq!(gd.bytes(), [0x8d, 0x04, 0x1d, 0, 0, 0, 0]);
        assert_eq!(gd.site().map(|s| s.offset), Some(3));
        let ld = admit(mc::lea(Size::D, EAX, field(EBX)));
        assert_eq!(ld.bytes(), [0x8d, 0x83, 0, 0, 0, 0]);
        assert_eq!(ld.site().map(|s| s.offset), Some(2));
        assert_eq!(admit(mc::lea(Size::D, ECX, field(EAX))).bytes(), [0x8d, 0x88, 0, 0, 0, 0]);
        assert_eq!(admit(mc::pushfq()).bytes(), [0x9c]);
        assert_eq!(admit(mc::popfq()).bytes(), [0x9d]);
        assert_eq!(
            admit(mc::movup_mr(FpSize::S, sp(16), Xmm::ALL[7])).bytes(),
            [0x0f, 0x11, 0x7c, 0x24, 0x10]
        );
        assert_eq!(
            Xmm::ALL[..8].iter().map(|x| x.code()).collect::<Vec<_>>(),
            [0, 1, 2, 3, 4, 5, 6, 7]
        );
    }

    #[test]
    fn the_save_areas_keep_esp_aligned() {
        for with_xmm in [false, true] {
            let (size, gpr_at) = area(with_xmm);
            assert_eq!(size % 16, 0);
            assert!(gpr_at + 16 <= size - 4);
        }
    }
}
