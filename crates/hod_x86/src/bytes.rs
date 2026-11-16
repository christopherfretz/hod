//! `llvm.memcpy`, `llvm.memmove`, `llvm.memset` (and their `.inline`
//! forms) with a length of 1 to 64 bits (`spec/codegen-x86.md` §5 and
//! "Memory intrinsics"): `rep movsb` / `rep stosb`, inline, whatever the
//! length. Owned by WP-293; WP-315 took lengths of 33 to 64 bits (their
//! low word, as `llc` passes `size_t`) and [`calls_libc`].
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
//! A wide length is not pushed: its low word is loaded into `ecx` from
//! its slot (or as an immediate) once the pointers are on the stack.
//!
//! A `memmove` whose destination is above its source copies backward
//! (`std` ... `cld`); the direction flag is clear at every call and
//! return, as the psABI requires.
//!
//! The result is the one libc's routine gives, so hod never calls it;
//! where `llc` does ([`calls_libc`]) is an x87 barrier (§11).

use hod_cg::{CgResult, FnCx};
use hod_ir::{CallSite, Const, Inst, Intrinsic, Value, ValueKind};
use hod_mc_x64 as mc;
use hod_mc_x64::{Cond, Mem, Reg, Size};

use crate::common::{ECX, branch, branch_cond, frame_inst, gp, put, unsupported};
use crate::target::X86;

/// `esi`, `edi` as the encoder names them.
const ESI: Reg = Reg::Rsi;
const EDI: Reg = Reg::Rdi;

/// How the length of a call reaches `ecx`.
#[derive(Clone, Copy)]
enum Len {
    /// A `Gpr` integer already in `ecx` (zero-extended): it takes no pool
    /// register (`spec/codegen-x86.md` §2, the two-register rule).
    Ecx,
    /// The low 32 bits of a wide constant.
    Imm(u32),
    /// The low word of a wide integer's slot.
    Slot(hod_cg::FrameRef),
}

/// The length of a call: a `Gpr` integer put in `ecx` now (after the
/// pointers are placed: placing a pointer may call and write `ecx`), or
/// an integer of 33 to 64 bits truncated to its low word (as `llc`,
/// whose `size_t` is 32 bits).
fn length(cx: &mut FnCx<'_, X86>, len: Value) -> CgResult<Len> {
    let ty = crate::common::value_ty(cx, len);
    if crate::int::wide_bits(cx, ty).is_some_and(|bits| bits <= 64) {
        if let ValueKind::Const(id) = len.kind() {
            return Ok(Len::Imm(match cx.module.consts.get(id) {
                Const::Int { value, .. } => value as u32,
                _ => 0,
            }));
        }
        return Ok(Len::Slot(cx.use_mem(len)?));
    }
    if !crate::abi::is_gpr(cx.types(), ty) {
        return unsupported(crate::abi::type_refusal(cx.types(), ty));
    }
    let bits = crate::common::width(cx, len);
    cx.put(len, crate::reg::ECX)?;
    crate::alu::extend_to(cx, ECX, bits, crate::alu::Ext::Zero);
    Ok(Len::Ecx)
}

/// Pushes `regs`; then loads a constant or wide length into `ecx`.
fn push_operands(cx: &mut FnCx<'_, X86>, regs: &[Reg], n: Len) {
    for &r in regs {
        put(cx, mc::push_r(r));
    }
    match n {
        Len::Ecx => {}
        Len::Imm(v) => crate::common::mov_imm(cx, ECX, v),
        // The slot is `ebp`-relative: the pushes do not move it.
        Len::Slot(slot) => frame_inst(cx, slot, |m| mc::mov_rm(Size::D, ECX, m)),
    }
}

/// `llvm.memcpy`, `llvm.memcpy.inline`, `llvm.memmove`.
///
/// # Errors
/// The type refusal of a length wider than 64 bits; what the operands
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
    let d = gp(cx.use_reg(dst)?);
    let s = gp(cx.use_reg(src)?);
    let n = length(cx, len)?;
    push_operands(cx, &[ESI, EDI, d, s], n);
    for r in [ESI, EDI] {
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
    let d = gp(cx.use_reg(dst)?);
    let n = length(cx, len)?;
    cx.put(byte, crate::reg::EAX)?;
    push_operands(cx, &[EDI, d], n);
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

/// The three memory operations, for [`calls_libc`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MemOp {
    /// `llvm.memcpy`.
    Copy,
    /// `llvm.memmove`.
    Move,
    /// `llvm.memset`.
    Set,
}

/// The most stores `llc` writes for an inline `memcpy` / `memmove`
/// (`MaxStoresPerMemcpy`, `MaxStoresPerMemmove` at `-O0`).
const MAX_COPY_STORES: u64 = 8;
/// The most stores of an inline `memset`.
const MAX_SET_STORES: u64 = 16;
/// The longest `memcpy` / `memset` `llc` writes as `rep movs` / `rep
/// stos` (`getMaxInlineSizeThreshold`) when the alignment is 4 or more.
const MAX_REP_BYTES: u64 = 128;

/// Whether `llc` 19 and 22 (`-O0`, a CPU without SSE) lower the memory
/// operation `op` of `len` bytes (`None`: not a constant; a wide constant
/// with all its bits) at alignment `align` (the least of the pointers'
/// `align` attributes, 1 without), `volatile` or not, as a call of libc's
/// routine (measured over lengths 0 to 140, 200, 255 to 257 and 1000,
/// alignments 1 to 16, both flags, a constant or variable byte; a CPU
/// with `cmov` and none give the same):
///
/// - stores of 4, 2 and 1 bytes first: `ceil(len / 4)` of them when the
///   last may overlap (a `memcpy` or `memset` that is not volatile),
///   else `len / 4` and one or two for the rest; at most 8 for a copy, 16
///   for a `memset`: no call;
/// - then `memcpy` and `memset` at an alignment of 4 or more and of at
///   most 128 bytes: `rep movs` / `rep stos`, no call;
/// - anything else (every `memmove` beyond the stores, a variable
///   length): a call.
///
/// The `.inline` forms are never a call.
#[must_use]
pub fn calls_libc(op: MemOp, len: Option<u64>, align: u64, volatile: bool) -> bool {
    let Some(n) = len else { return true };
    let overlap = op != MemOp::Move && !volatile;
    let stores = if overlap { n.div_ceil(4) } else { n / 4 + [0, 1, 1, 2][(n % 4) as usize] };
    let limit = if op == MemOp::Set { MAX_SET_STORES } else { MAX_COPY_STORES };
    if stores <= limit {
        return false;
    }
    op == MemOp::Move || align < 4 || n > MAX_REP_BYTES
}

/// Whether the memory intrinsic call `site` (`llvm.memcpy`, `memmove`,
/// `memset`; not the `.inline` forms) is a call of libc's routine in
/// `llc`'s code ([`calls_libc`]); `false` for any other call.
#[must_use]
pub fn site_calls_libc(cx: &FnCx<'_, X86>, site: &CallSite<'_>) -> bool {
    let (op, pointers) = match site.intrinsic() {
        Some(Intrinsic::Memcpy) => (MemOp::Copy, 2),
        Some(Intrinsic::Memmove) => (MemOp::Move, 2),
        Some(Intrinsic::Memset) => (MemOp::Set, 1),
        _ => return false,
    };
    let &[.., len, volatile] = site.args() else { return false };
    let constant = |v: Value| match v.kind() {
        ValueKind::Const(id) => match cx.module.consts.get(id) {
            Const::Int { value, .. } => Some(value),
            Const::Null(_) | Const::Zero(_) | Const::Undef(_) | Const::Poison(_) => Some(0),
            _ => None,
        },
        _ => None,
    };
    let align = (0..pointers).map(|i| site.arg_attrs(i).align().unwrap_or(1)).min().unwrap_or(1);
    let volatile = constant(volatile).is_some_and(|v| v & 1 == 1);
    calls_libc(op, constant(len), align, volatile)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Points of `llc`'s rule (`calls_libc`'s measurement).
    #[test]
    fn libc_calls_follow_llc() {
        use MemOp::{Copy, Move, Set};
        assert!(!calls_libc(Copy, Some(0), 1, true));
        assert!(!calls_libc(Copy, Some(32), 1, false));
        assert!(calls_libc(Copy, Some(33), 1, false));
        assert!(calls_libc(Copy, Some(31), 1, true));
        assert!(!calls_libc(Copy, Some(32), 1, true));
        assert!(!calls_libc(Copy, Some(128), 4, false));
        assert!(calls_libc(Copy, Some(129), 16, false));
        assert!(calls_libc(Move, Some(31), 16, false));
        assert!(!calls_libc(Move, Some(30), 1, true));
        assert!(calls_libc(Move, Some(64), 16, false));
        assert!(!calls_libc(Set, Some(64), 1, false));
        assert!(calls_libc(Set, Some(63), 1, true));
        assert!(calls_libc(Set, Some(65), 2, false));
        assert!(!calls_libc(Set, Some(65), 4, false));
        assert!(calls_libc(Set, Some(1 << 32 | 1), 4, false));
        assert!(calls_libc(Set, None, 16, false));
    }
}
