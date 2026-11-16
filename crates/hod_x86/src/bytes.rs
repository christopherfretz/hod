//! `llvm.memcpy`, `llvm.memmove`, `llvm.memset` (and their `.inline`
//! forms) with a 32-bit or narrower length (`spec/codegen-x86.md`
//! "Intrinsics"): `rep movsb` / `rep stosb`, inline, whatever the length.
//! Owned by WP-293. (`hod_cg`'s runtime `memcpy` takes an `i64` length,
//! which this package does not pass.)
//!
//! The string instructions need `edi`, `esi` and `ecx`, two of which may
//! hold values. So the pool registers are saved on the stack, the
//! operands pushed and popped into place (which is right whatever
//! registers the operands are in), and the pool registers restored:
//!
//! ```text
//! push esi; push edi; push dst; push src; push len
//! pop ecx; pop esi; pop edi; rep movsb; pop edi; pop esi
//! ```
//!
//! A `memmove` whose destination is above its source copies backward
//! (`std` ... `cld`); the direction flag is clear at every call and
//! return, as the psABI requires.

use hod_cg::{CgResult, FnCx};
use hod_ir::{Inst, Intrinsic, Value};
use hod_mc_x64 as mc;
use hod_mc_x64::{Cond, Mem, Reg, Size};

use crate::common::{ECX, branch, branch_cond, gp, put, unsupported};
use crate::target::X86;

/// `esi`, `edi` as the encoder names them.
const ESI: Reg = Reg::Rsi;
const EDI: Reg = Reg::Rdi;

/// The length of a call: a `Gpr` integer.
fn length(cx: &mut FnCx<'_, X86>, len: Value) -> CgResult<Reg> {
    let ty = crate::common::value_ty(cx, len);
    if !crate::abi::is_gpr(cx.types(), ty) {
        return unsupported(crate::abi::type_refusal(cx.types(), ty));
    }
    let bits = crate::common::width(cx, len);
    let r = gp(cx.use_reg(len)?);
    crate::alu::extend_to(cx, r, bits, crate::alu::Ext::Zero);
    Ok(r)
}

/// `llvm.memcpy`, `llvm.memcpy.inline`, `llvm.memmove`.
///
/// # Errors
/// The type refusal of a length wider than 32 bits; what the operands
/// report.
pub fn copy(
    cx: &mut FnCx<'_, X86>,
    which: Intrinsic<'_>,
    dst: Value,
    src: Value,
    len: Value,
) -> CgResult<()> {
    if dst == src {
        return Ok(());
    }
    let n = length(cx, len)?;
    let d = gp(cx.use_reg(dst)?);
    let s = gp(cx.use_reg(src)?);
    for r in [ESI, EDI, d, s, n] {
        put(cx, mc::push_r(r));
    }
    for r in [ECX, ESI, EDI] {
        put(cx, mc::pop_r(r));
    }
    if which == Intrinsic::Memmove {
        // Backward when `edi > esi` (unsigned): the regions may overlap
        // with the destination above.
        let forward = cx.new_label();
        let done = cx.new_label();
        put(cx, mc::cmp_rr(Size::D, EDI, ESI));
        branch_cond(cx, Cond::Be, forward)?;
        put(cx, mc::lea(Size::D, ESI, Mem::base_index(ESI, index(cx, ECX)?, mc::Scale::S1, -1)));
        put(cx, mc::lea(Size::D, EDI, Mem::base_index(EDI, index(cx, ECX)?, mc::Scale::S1, -1)));
        put(cx, mc::std());
        put(cx, mc::rep_movs(Size::B));
        put(cx, mc::cld());
        branch(cx, done)?;
        cx.bind(forward)?;
        put(cx, mc::rep_movs(Size::B));
        cx.bind(done)?;
    } else {
        put(cx, mc::rep_movs(Size::B));
    }
    put(cx, mc::pop_r(EDI));
    put(cx, mc::pop_r(ESI));
    Ok(())
}

/// The index form of `r`.
fn index(cx: &FnCx<'_, X86>, r: Reg) -> CgResult<mc::Index> {
    match r.index() {
        Some(i) => Ok(i),
        None => cx.bug("a register without an index form"),
    }
}

/// `llvm.memset`, `llvm.memset.inline`: the byte in `al`.
///
/// # Errors
/// As [`copy`].
pub fn set(cx: &mut FnCx<'_, X86>, dst: Value, byte: Value, len: Value) -> CgResult<()> {
    cx.put(byte, crate::reg::EAX)?;
    let n = length(cx, len)?;
    let d = gp(cx.use_reg(dst)?);
    for r in [EDI, d, n] {
        put(cx, mc::push_r(r));
    }
    put(cx, mc::pop_r(ECX));
    put(cx, mc::pop_r(EDI));
    put(cx, mc::rep_stos(Size::B));
    put(cx, mc::pop_r(EDI));
    Ok(())
}

/// The dispatch of the four intrinsics.
///
/// # Errors
/// As [`copy`]; `Internal` for a call with the wrong operands.
pub fn intrinsic<'a>(
    cx: &mut FnCx<'a, X86>,
    _inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    match (which, args) {
        (Intrinsic::Memcpy | Intrinsic::MemcpyInline | Intrinsic::Memmove, &[d, s, n, _]) => {
            copy(cx, which, d, s, n)
        }
        (Intrinsic::Memset | Intrinsic::MemsetInline, &[d, b, n, _]) => set(cx, d, b, n),
        _ => cx.bug("a memory intrinsic with unexpected operands"),
    }
}
