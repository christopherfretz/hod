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
//! thread-locals) call `___tls_get_addr` (three underscores, the
//! `regparm` entry with its argument in `eax`), a function `hod_cg`'s
//! closed list of run-time functions does not name (it has
//! `__tls_get_addr`, the stack-argument entry, which the linkers'
//! relaxation of `leal x@TLSGD(,%ebx,1), %eax; call ___tls_get_addr@PLT`
//! does not recognize). They are refused as [`DYNAMIC`].

use hod_cg::{CgResult, FnCx, SymRef, TlsAccess};
use hod_ir::{Inst, Intrinsic, SymId, Value, ValueKind};
use hod_mc_x64 as mc;
use hod_mc_x64::{Mem, Reg, Seg, Size};
use hod_obj::I386Reloc;

use crate::common::{ECX, EDX, gp, put, unsupported};
use crate::pic::{add_addend, got_pointer, put_reloc};
use crate::target::X86;

/// The refusal of every thread-local access before WP-299; no lowering
/// returns it any more (kept for the names tests refer to).
pub const TLS: &str = "x86:tls";

/// A general- or local-dynamic access (module docs).
pub const DYNAMIC: &str = "x86:tls:dynamic";

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

/// Puts the address of thread-local `sym` plus `addend` in `rd` by
/// `model` (module docs). Writes `rd` and nothing else, the flags kept.
///
/// # Errors
/// [`DYNAMIC`]; `Internal` for an encoder inconsistency.
pub fn address(
    cx: &mut FnCx<'_, X86>,
    rd: Reg,
    sym: SymId,
    model: TlsAccess,
    addend: i64,
) -> CgResult<()> {
    let target = SymRef::Module(sym);
    match model {
        TlsAccess::GeneralDynamic | TlsAccess::LocalDynamic => unsupported(DYNAMIC),
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

/// `llvm.threadlocal.address(ptr)` (the address of the operand, which is
/// a thread-local symbol at an offset: `common::materialize_gpr` reaches
/// [`address`]) and `llvm.thread.pointer()`: the hook of `call::lower`.
///
/// # Errors
/// What [`address`] refuses; `Internal` for an operand list the verifier
/// rejects.
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
            let rd = cx.def_reg(inst)?;
            cx.put(ptr, rd)?;
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
}
