//! Thread-local storage (`spec/codegen.md` §5.6.11, "Thread-local
//! storage (WP-134)"; design 0006 §2.8): the address of a thread-local
//! symbol by the model `hod_cg::sym_access` names, and
//! `llvm.threadlocal.address` and `llvm.thread.pointer`. Owned by WP-134.
//!
//! Each model is the sequence x86-64 Linux `llc -relocation-model=pic`
//! writes, with the same relocations, so the linkers recognize and relax
//! it (`spec/mc-x64.md` "Thread-local storage"):
//!
//! ```text
//! general dynamic (16 bytes, padded so a linker can write local or
//! initial exec over it):
//!     data16 lea rdi, [rip + x@TLSGD]              R_X86_64_TLSGD
//!     data16 data16 rex64 call __tls_get_addr@PLT  R_X86_64_PLT32
//!     ; rax = &x
//! local dynamic (12 bytes; the module's block, then the offset):
//!     lea rdi, [rip + x@TLSLD]                     R_X86_64_TLSLD
//!     call __tls_get_addr@PLT                      R_X86_64_PLT32
//!     lea rd, [rax + x@DTPOFF]                     R_X86_64_DTPOFF32
//! initial exec:
//!     mov rd, [rip + x@GOTTPOFF]                   R_X86_64_GOTTPOFF
//!     push t ; mov t, fs:[0] ; lea rd, [rd + t] ; pop t
//! local exec:
//!     mov rd, fs:[0]
//!     lea rd, [rd + x@TPOFF]                       R_X86_64_TPOFF32
//! ```
//!
//! The relocations carry no addend (as `llc`'s, and as on AArch64): an
//! offset from the symbol is added in code afterwards
//! ([`common::add_addend`](crate::common::add_addend)).
//!
//! **Two ways in.** `llvm.threadlocal.address` ([`intrinsic`]) is an
//! instruction of its own, so a dynamic model is lowered there as a call
//! (§4.4): every register value is displaced first, and the bare 16- or
//! 12-byte pair follows. A thread-local symbol may also be a plain
//! operand (a call argument, a stored value, a `select` or `phi` arm,
//! with a constant offset), and then [`address`] is reached through
//! `common::sym_address`, whose contract is that of every symbol address
//! (§3): it may run at any point of a lowering, argument placement and
//! the window between a fused compare and its `cmovcc` included, and
//! writes `rd` and nothing else, the flags kept. `__tls_get_addr` is an
//! ordinary function that clobbers every caller-saved register and the
//! flags, so the operand form of a dynamic model ([`save_and_call`])
//! keeps all of them in an area below the stack pointer (the CFA is
//! `rbp`-based in a body, so no CFI row changes, and the area is a
//! multiple of 16 so `rsp` stays aligned at the call): the flags first,
//! with `pushfq`, then the nine caller-saved general registers but `rd`
//! and the sixteen vector registers; `popfq` last restores the flags,
//! all of them (WP-144). The x87 stack must be empty there, as at any
//! call.

use hod_cg::{Access, CgResult, FnCx, SymRef, TlsAccess};
use hod_ir::{CodeModel, Const, Inst, Intrinsic, SymId, Value, ValueKind};
use hod_mc_x64 as mc;
use hod_mc_x64::{Mem, Reg, Seg, Size, Xmm};
use hod_obj::X64Reloc;

use crate::common::{CODE_MODEL, RSP, add_addend, deferred_first, gp, put, put_reloc, unsupported};
use crate::target::X64;

/// The group's refusal before WP-134; no lowering returns it any more
/// (kept for the names other files and tests refer to).
pub const TLS: &str = "x64:todo:tls";

/// The caller-saved general registers (SysV): what `__tls_get_addr` may
/// clobber, in the order the operand form saves them.
const CALLER_SAVED: [Reg; 9] =
    [Reg::Rax, Reg::Rcx, Reg::Rdx, Reg::Rsi, Reg::Rdi, Reg::R8, Reg::R9, Reg::R10, Reg::R11];

/// The save area of the operand form, from `rsp`: the 16 vector
/// registers at `16 * i`, the general registers at [`GPR_AT`] `+ 8 * i`,
/// `rflags` in the last 8 bytes (pushed by `pushfq`). A multiple of 16.
const AREA: i32 = 16 * 16 + 8 * 9 + 8;
/// Where the general registers are saved.
const GPR_AT: i32 = 16 * 16;

/// `[rsp + disp]`.
fn sp(disp: i32) -> Mem {
    Mem::base(RSP, disp)
}

/// `fs:[0]`: the thread pointer's own address, which the TCB stores at
/// its start (x86-64 psABI "Thread-Local Storage").
fn thread_pointer() -> Mem {
    Mem::abs(0).with_seg(Seg::Fs)
}

/// `mov rd, fs:[0]` (`64 48 8B 04 25 00 00 00 00`): what
/// `llvm.thread.pointer` returns, and the base of the static models.
pub fn load_thread_pointer(cx: &mut FnCx<'_, X64>, rd: Reg) {
    put(cx, mc::mov_rm(Size::Q, rd, thread_pointer()));
}

/// Emits the dynamic pair of `model` for `sym` (`rdi` set, then the call
/// of `__tls_get_addr`): `rax` is then `&sym` (general dynamic) or the
/// module's TLS block (local dynamic). Writes what a call writes; the
/// caller has displaced or saved the rest. Records the call site.
fn dynamic_pair(cx: &mut FnCx<'_, X64>, sym: SymId, model: TlsAccess) -> CgResult<()> {
    let target = SymRef::Module(sym);
    let get_addr = SymRef::Runtime(hod_cg::RuntimeFn::TlsGetAddr);
    let call_at = if model == TlsAccess::GeneralDynamic {
        put_reloc(cx, mc::tls_gd_lea(), X64Reloc::TlsGd, target, 0)?;
        let at = cx.offset();
        put_reloc(cx, mc::tls_gd_call(), X64Reloc::Plt32, get_addr, 0)?;
        at
    } else {
        put_reloc(cx, mc::lea(Size::Q, Reg::Rdi, Mem::rip(0)), X64Reloc::TlsLd, target, 0)?;
        let at = cx.offset();
        put_reloc(cx, mc::call(0), X64Reloc::Plt32, get_addr, 0)?;
        at
    };
    let len = cx.offset() - call_at;
    cx.record_call_site(call_at, len);
    Ok(())
}

/// `lea rd, [base + sym@DTPOFF]` (`R_X86_64_DTPOFF32`): the address of
/// `sym` in the module's block at `base`.
fn dtpoff(cx: &mut FnCx<'_, X64>, rd: Reg, base: Reg, sym: SymId) -> CgResult<()> {
    let Some(m) = Mem::base(base, 0).with_abs32() else {
        return cx.bug("an absolute field on a RIP-relative operand");
    };
    put_reloc(cx, mc::lea(Size::Q, rd, m), X64Reloc::DtpOff32, SymRef::Module(sym), 0)
}

/// The static models: initial exec and local exec. Writes `rd` only,
/// flags kept.
fn static_model(cx: &mut FnCx<'_, X64>, rd: Reg, sym: SymId, model: TlsAccess) -> CgResult<()> {
    let target = SymRef::Module(sym);
    if model == TlsAccess::InitialExec {
        // `mov` rather than `llc`'s `add rd, [rip + x@GOTTPOFF]` after
        // the thread pointer, which writes the flags; both are forms the
        // linkers relax.
        put_reloc(cx, mc::mov_rm(Size::Q, rd, Mem::rip(0)), X64Reloc::GotTpOff, target, 0)?;
        let t = if rd == Reg::R11 { Reg::R10 } else { Reg::R11 };
        put(cx, mc::push_r(t));
        load_thread_pointer(cx, t);
        let Some(index) = t.index() else {
            return cx.bug("r10 and r11 are index registers");
        };
        put(cx, mc::lea(Size::Q, rd, Mem::base_index(rd, index, mc::Scale::S1, 0)));
        put(cx, mc::pop_r(t));
        return Ok(());
    }
    load_thread_pointer(cx, rd);
    let Some(m) = Mem::base(rd, 0).with_abs32() else {
        return cx.bug("an absolute field on a RIP-relative operand");
    };
    put_reloc(cx, mc::lea(Size::Q, rd, m), X64Reloc::TpOff32, target, 0)
}

/// A dynamic model as an operand: saves every caller-saved register but
/// `rd`, the vector registers and the flags below `rsp`, calls, puts the
/// address in `rd`, restores the rest (module docs).
fn save_and_call(cx: &mut FnCx<'_, X64>, rd: Reg, sym: SymId, model: TlsAccess) -> CgResult<()> {
    put(cx, mc::pushfq());
    put(cx, mc::lea(Size::Q, RSP, sp(8 - AREA)));
    let saved = |r: &Reg| *r != rd;
    for (i, r) in CALLER_SAVED.iter().enumerate().filter(|(_, r)| saved(r)) {
        put(cx, mc::mov_mr(Size::Q, sp(GPR_AT + 8 * i as i32), *r));
    }
    for (i, x) in Xmm::ALL.into_iter().enumerate() {
        put(cx, mc::movdqu_mr(sp(16 * i as i32), x));
    }
    dynamic_pair(cx, sym, model)?;
    if model == TlsAccess::LocalDynamic {
        dtpoff(cx, rd, Reg::Rax, sym)?;
    } else if rd != Reg::Rax {
        put(cx, mc::mov_rr(Size::Q, rd, Reg::Rax));
    }
    for (i, x) in Xmm::ALL.into_iter().enumerate() {
        put(cx, mc::movdqu_rm(x, sp(16 * i as i32)));
    }
    for (i, r) in CALLER_SAVED.iter().enumerate().filter(|(_, r)| saved(r)) {
        put(cx, mc::mov_rm(Size::Q, *r, sp(GPR_AT + 8 * i as i32)));
    }
    put(cx, mc::lea(Size::Q, RSP, sp(AREA - 8)));
    put(cx, mc::popfq());
    Ok(())
}

/// Puts the address of thread-local `sym` plus `addend` in `rd` by the
/// model `model` (module docs): the hook of `common::sym_address`. Writes
/// `rd` and nothing else, the flags kept (for an addend beyond 32 bits
/// also the general scratch register other than `rd`, as
/// `common::sym_address`); a dynamic model records its call as a call
/// site (§7.1).
///
/// # Errors
/// `Internal` only for an encoder inconsistency.
pub fn address(
    cx: &mut FnCx<'_, X64>,
    rd: Reg,
    sym: SymId,
    model: TlsAccess,
    addend: i64,
) -> CgResult<()> {
    match model {
        TlsAccess::GeneralDynamic | TlsAccess::LocalDynamic => save_and_call(cx, rd, sym, model)?,
        TlsAccess::InitialExec | TlsAccess::LocalExec => static_model(cx, rd, sym, model)?,
    }
    add_addend(cx, rd, addend);
    Ok(())
}

/// The thread-local symbol and offset `ptr` names, if it is one.
fn tls_symbol(cx: &FnCx<'_, X64>, ptr: Value) -> Option<(SymId, i64)> {
    let (sym, addend) = match ptr.kind() {
        ValueKind::Sym(sym) => (sym, 0),
        ValueKind::Const(id) => match cx.module.consts.get(id) {
            Const::Addr { sym, addend } => (sym, addend),
            _ => return None,
        },
        _ => return None,
    };
    Some((sym, addend))
}

/// `llvm.threadlocal.address(sym)` and `llvm.thread.pointer()`: the hook
/// of the dispatch (`target.rs`). A dynamic model is lowered as a call
/// (§4.4): every register value is displaced, then the bare pair; the
/// static models and anything else are the operand's own sequence.
///
/// # Errors
/// [`CODE_MODEL`] for a code model other than small; `Internal` for an
/// operand list the verifier rejects.
pub fn intrinsic<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    match (which, args) {
        (Intrinsic::ThreadLocalAddress, &[ptr]) => {
            if !matches!(ptr.kind(), ValueKind::Sym(_) | ValueKind::Const(_)) {
                return cx.bug("llvm.threadlocal.address of something that is no symbol");
            }
            let dynamic = tls_symbol(cx, ptr).and_then(|(sym, addend)| {
                match hod_cg::sym_access(cx.module, cx.options, sym) {
                    Access::Tls(model @ (TlsAccess::GeneralDynamic | TlsAccess::LocalDynamic)) => {
                        Some((sym, addend, model))
                    }
                    _ => None,
                }
            });
            let Some((sym, addend, model)) = dynamic else {
                let rd = cx.def_reg(inst)?;
                cx.put(ptr, rd)?;
                return cx.finish(inst, rd);
            };
            if cx.options.code_model(cx.module) != CodeModel::Small {
                return unsupported(CODE_MODEL);
            }
            cx.displace_all()?;
            let rd = cx.def_reg(inst)?;
            dynamic_pair(cx, sym, model)?;
            let r = gp(rd);
            if model == TlsAccess::LocalDynamic {
                dtpoff(cx, r, Reg::Rax, sym)?;
            } else {
                put(cx, mc::mov_rr(Size::Q, r, Reg::Rax));
            }
            add_addend(cx, r, addend);
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
