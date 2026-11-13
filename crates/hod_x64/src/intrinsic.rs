//! The intrinsics no other group takes (`spec/codegen.md` §5.6.8,
//! "Intrinsics (WP-128)"): memory intrinsics, bit manipulation, `abs`,
//! min/max, funnel shifts, the saturating arithmetic and shifts,
//! `scmp`/`ucmp`, traps, the frame intrinsics, the counters, and those
//! that fold to a constant or to nothing. The dispatch (`target.rs`) has
//! already sent the variadic, stack, overflow, thread-local, coroutine
//! and floating-point intrinsics to their modules; the intrinsics that
//! generate no code at all (`llvm.lifetime.*`, `llvm.assume`,
//! `llvm.expect*`, `llvm.invariant.*`, `llvm.donothing`, `llvm.dbg.*`, ...)
//! never reach a target (`hod_ir::no_code_intrinsic`).
//!
//! **Owner: WP-128.** Tests: `tests/intrinsic.rs`. The decisions that are
//! not instruction selection are `hod_a64`'s (`intrinsic.rs`, `tail.rs`,
//! WP-041, WP-070, WP-084): what is refused, the runtime routines, the
//! memory plan, poison latitude.
//!
//! - **Integers** of `i1` to `i64` (class `Gpr`), computed in the 32-bit
//!   register for at most 32 bits and in the 64-bit one above; the bits
//!   above the width unspecified (§2.2) in operands and results. The base
//!   instruction set only (no `lzcnt`, `tzcnt`, `popcnt`, BMI), whatever
//!   the function's features say (§5.6.2):
//!   - `ctlz`: below the register's width the value moves to the top and
//!     the bit below it is set (`shl`, `bts`), so that `bsr` never sees 0
//!     and a zero counts the IR width; at 32 and 64 bits `bsr` + `cmovz`
//!     of `2R - 1`; then `xor R - 1`. `cttz`: `bts N` below the width,
//!     else `bsf` + `cmovz` of `R`. The `is_zero_poison` flag is ignored
//!     (zero counts the width), as on AArch64.
//!   - `ctpop`: zero-extended, then `llc`'s SWAR sequence at the
//!     register's width (no runtime routine).
//!   - `bswap` (`bswap`, `shr R - N`; a width that is not a multiple of
//!     16 is [`INTRINSIC`]), `bitreverse` (`bswap`, three mask-and-shift
//!     steps, `shr R - N`), `abs` (sign-extended, `neg`, `cmovs`).
//!   - `smax`/`smin`/`umax`/`umin`: both extended, `cmp`, `cmovcc`.
//!   - `fshl`/`fshr`: at 32 and 64 bits `shld`/`shrd` (the amount by `cl`,
//!     which the instruction takes modulo the width); below, the amount
//!     modulo `N` (`and` for a power of two, `div` otherwise) and two
//!     shifts by `cl`.
//!   - `{s,u}{add,sub}.sat`: at 32 and 64 bits the flags of `add`/`sub`
//!     (`cmovo` with the bound of the first operand's sign, `cmovb`);
//!     narrower, exact in 64 bits from extended operands, then clamped.
//!   - `{s,u}shl.sat`: the operand moved to the top of the 64-bit
//!     register, shifted, shifted back and compared.
//!   - `scmp`/`ucmp`: `cmp`, `setg`/`seta` and `setl`/`setb`, `sub`,
//!     `movsx`.
//! - **`i128`** (class `Mem`): the same intrinsics on the limbs of the
//!   slots (`aggr::ld`/`aggr::st`), through `rax`, `rcx`, `rdx`, `r10`,
//!   `r11` and at most three temporaries. Wider integers are [`WIDE`].
//! - **Memory**: `hod_cg::memplan` decides; inline copies are 16-byte
//!   `movups` through `xmm15` and pieces through `r11` (the tail
//!   overlapping), an inline `memmove` reads every byte (into `xmm14`,
//!   `xmm15`, `r10`, `r11`) before it writes one, an inline `memset`
//!   stores the replicated byte from `r11` in exact pieces, the loops of
//!   the `.inline` forms count in `rcx`. Calls go to `memcpy`,
//!   `memmove`, `memset` with a narrow length zero-extended. Volatile is
//!   the plain form. No `rep movs`/`rep stos`.
//! - **Traps**: `trap` is `ud2`, `debugtrap` `int3`; `ubsantrap(kind)` is
//!   `ud1 eax, [eax + kind]` with 32-bit addressing, as `llc` 19 and 22
//!   write it (WP-144): the kind in the displacement, `SIGILL`.
//! - **Frame**: `frameaddress(n)` follows `n` saved `rbp`s; `returnaddress(n)`
//!   loads `[record + 8]`. Above [`MAX_DEPTH`]: [`FRAME_DEPTH`].
//! - **Folded**: `objectsize` is `-1` (`min` false) or `0`; `is.constant`
//!   is `llc`'s `isManifestConstant` ([`is_manifest`]); `prefetch` and
//!   `clear_cache` emit nothing (x86 caches are coherent, as `llc`);
//!   `ptrmask` is an `and`; `readcyclecounter` is `rdtsc`.
//! - **Dead instructions** in a function that calls `is.constant` or
//!   `objectsize` ([`begin`], [`dropped`]): as `hod_a64::tail`.

use std::cell::RefCell;

use hod_cg::memplan::{MemOp, MemPlan, mem_plan};
use hod_cg::{CallArg, CgResult, Class, FnCx};
use hod_ir::{
    Body, CallSite, Const, ConstId, Inst, InstKind, Intrinsic, Opcode, TypeId, TypeKind, Value,
    ValueKind,
};
use hod_mc_x64 as mc;
use hod_mc_x64::{Cond, FpSize, Mem, Reg, Size};

use crate::aggr::{ld, st};
use crate::common::{
    R10, R11, RBP, XMM14, XMM15, branch_cond, deferred_first, exact_size, gp, gpr_load, mov_imm,
    put, unsupported, value_ty,
};
use crate::int::{Ext, extend_to, size_for_bits};
use crate::target::X64;

/// An intrinsic this target does not lower: target intrinsics of
/// another architecture, `clmul`, the named registers, the
/// instrumentation intrinsics, and whatever no package has planned.
pub const INTRINSIC: &str = "x64:intrinsic";
/// An integer intrinsic of an integer wider than 64 bits other than
/// `i128`.
pub const WIDE: &str = "x64:intrinsic-wide";
/// An integer intrinsic of a vector type, or a vector reduction (WP-132).
pub const VECTOR: &str = "x64:intrinsic-vector";
/// `llvm.memcpy.inline` or `llvm.memset.inline` of a length that is not a
/// constant: the `.inline` forms may not call, and a loop of unknown
/// count is not written (as `a64:mem-inline-length`).
pub const INLINE_LENGTH: &str = "x64:mem-inline-length";
/// `llvm.frameaddress` or `llvm.returnaddress` deeper than [`MAX_DEPTH`].
pub const FRAME_DEPTH: &str = "x64:frame-depth";
/// `llvm.eh.typeid.for` of something that is not a symbol.
pub const TYPEID: &str = "x64:eh-typeid";

/// `llvm.eh.sjlj.setjmp` or `longjmp` in a module with
/// `cf-protection-return` (WP-147): the jump does not unwind the shadow
/// stack (`llc` writes `rdsspq`/`incsspq` there).
pub const SJLJ_SHADOW_STACK: &str = "x64:sjlj-shadow-stack";
/// `llvm.eh.sjlj.setjmp` in a coroutine body (WP-147): its frame base
/// `rbx` is not restored at the resume address.
pub const SJLJ_COROUTINE: &str = "x64:sjlj-coroutine";

/// Every name this module refuses with.
pub const UNSUPPORTED: &[&str] = &[
    INTRINSIC,
    WIDE,
    VECTOR,
    INLINE_LENGTH,
    FRAME_DEPTH,
    TYPEID,
    SJLJ_SHADOW_STACK,
    SJLJ_COROUTINE,
];

/// The deepest frame record `frameaddress`/`returnaddress` follow.
pub const MAX_DEPTH: u64 = 256;

pub use hod_cg::memplan::{INLINE_MAX, INLINE_MOVE_MAX};

const RAX: Reg = Reg::Rax;
const RCX: Reg = Reg::Rcx;
const RDX: Reg = Reg::Rdx;

/// A call of intrinsic `which` with arguments `args`.
///
/// # Errors
/// The names of this module; what placing the operands and a runtime
/// call report.
#[allow(clippy::too_many_lines, reason = "one arm per intrinsic")]
pub fn intrinsic<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    use Intrinsic as I;
    if let Some(done) = crate::x86::intrinsic(cx, inst, which, args) {
        return done;
    }
    if let Some(done) = crate::lanes::intrinsic(cx, inst, which, args) {
        return done;
    }
    match (which, args) {
        (I::Ctlz | I::Cttz | I::Ctpop | I::Bswap | I::BitReverse | I::Abs, &[x, ..]) => {
            let width = int_or_quad(cx, inst.ty())?;
            deferred_first(cx, inst, None)?;
            match width {
                Some(bits) => unary(cx, inst, which, x, bits),
                None => wide_unary(cx, inst, which, x),
            }
        }
        (
            I::Smax
            | I::Smin
            | I::Umax
            | I::Umin
            | I::SaddSat
            | I::UaddSat
            | I::SsubSat
            | I::UsubSat,
            &[a, b],
        ) => {
            let width = int_or_quad(cx, inst.ty())?;
            deferred_first(cx, inst, None)?;
            match width {
                Some(bits) => two(cx, inst, which, a, b, bits),
                None => wide_two(cx, inst, which, a, b),
            }
        }
        (I::Fshl | I::Fshr, &[a, b, c]) => {
            let width = int_or_quad(cx, inst.ty())?;
            deferred_first(cx, inst, None)?;
            match width {
                Some(bits) => funnel(cx, inst, which == I::Fshl, [a, b, c], bits),
                None => wide_funnel(cx, inst, which == I::Fshl, [a, b, c]),
            }
        }
        (I::Scmp | I::Ucmp, &[a, b]) => three_way(cx, inst, which == I::Scmp, a, b),
        (I::SshlSat | I::UshlSat, &[a, b]) => shl_sat(cx, inst, which == I::SshlSat, a, b),
        (I::Memcpy | I::MemcpyInline | I::Memmove, &[dst, src, len, _volatile]) => {
            deferred_first(cx, inst, None)?;
            copy(cx, which, dst, src, len)
        }
        (I::Memset | I::MemsetInline, &[dst, byte, len, _volatile]) => {
            deferred_first(cx, inst, None)?;
            set(cx, which == I::MemsetInline, dst, byte, len)
        }
        (I::Trap, []) => {
            put(cx, mc::ud2());
            Ok(())
        }
        (I::DebugTrap, []) => {
            put(cx, mc::int3());
            Ok(())
        }
        // The spin-wait hint, as `llc` (§5.6.19).
        (I::X86Sse2Pause, []) => {
            put(cx, mc::pause());
            Ok(())
        }
        // `ud1 eax, [eax + kind]`, as `llc` (`immarg i8`, zero-extended).
        (I::UbsanTrap, &[kind]) => {
            let Some(kind) = const_int(cx, kind) else {
                return cx.bug("llvm.ubsantrap of a kind that is not a constant");
            };
            put(cx, mc::ud1_eax(kind as u8));
            Ok(())
        }
        (I::EhSjljSetjmp, &[buf]) => sjlj_setjmp(cx, inst, buf),
        (I::EhSjljLongjmp, &[buf]) => sjlj_longjmp(cx, inst, buf),
        (I::FrameAddress | I::ReturnAddress, &[depth]) => {
            deferred_first(cx, inst, None)?;
            frame_walk(cx, inst, which == I::ReturnAddress, depth)
        }
        (I::ObjectSize, &[_, min, ..]) => {
            let bits = int_type(cx, inst.ty())?;
            deferred_first(cx, inst, None)?;
            let value = if const_int(cx, min).unwrap_or(0) & 1 == 1 { 0 } else { u64::MAX };
            let rd = cx.def_reg(inst)?;
            mov_imm(cx, gp(rd), value, bits);
            cx.finish(inst, rd)
        }
        (I::IsConstant, &[x]) => {
            let manifest = is_manifest(cx, x);
            let rd = cx.def_reg(inst)?;
            mov_imm(cx, gp(rd), u64::from(manifest), 32);
            cx.finish(inst, rd)
        }
        // Hints, and an instruction cache that x86 keeps coherent.
        (I::Prefetch | I::ClearCache, _) => deferred_first(cx, inst, None),
        (I::Ptrmask, &[ptr, mask]) => {
            if cx.class_of(ptr) != Class::Gpr || cx.class_of(mask) != Class::Gpr {
                return unsupported(VECTOR);
            }
            deferred_first(cx, inst, None)?;
            let p = cx.use_reg(ptr)?;
            let m = cx.use_reg(mask)?;
            // The mask is as wide as the pointer's index (64 bits).
            put(cx, mc::and_rr(Size::Q, gp(p), gp(m)));
            cx.finish(inst, p)
        }
        (I::EhTypeidFor, &[info]) => {
            let ValueKind::Sym(sym) = info.kind() else { return unsupported(TYPEID) };
            deferred_first(cx, inst, None)?;
            let id = cx.type_id(Some(sym));
            let rd = cx.def_reg(inst)?;
            mov_imm(cx, gp(rd), u64::from(id), 32);
            cx.finish(inst, rd)
        }
        (I::ReadCycleCounter, []) => {
            deferred_first(cx, inst, None)?;
            let rd = cx.def_reg(inst)?;
            put(cx, mc::rdtsc());
            put(cx, mc::shl_ri(Size::Q, RDX, 32));
            put(cx, mc::or_rr(Size::Q, RAX, RDX));
            put(cx, mc::mov_rr(Size::Q, gp(rd), RAX));
            cx.finish(inst, rd)
        }
        (
            I::VectorReduceAdd
            | I::VectorReduceMul
            | I::VectorReduceAnd
            | I::VectorReduceOr
            | I::VectorReduceXor
            | I::VectorReduceSmax
            | I::VectorReduceSmin
            | I::VectorReduceUmax
            | I::VectorReduceUmin
            | I::VectorReduceFmax
            | I::VectorReduceFmin
            | I::VectorReduceFadd
            | I::VectorReduceFmul
            | I::VectorExtract
            | I::VectorInsert,
            _,
        ) => unsupported(VECTOR),
        _ => unsupported(INTRINSIC),
    }
}

// ---- operands ----

/// The integer `value` is, zero-extended from its width, when it is an
/// integer constant (or a zero-like one).
pub fn const_int(cx: &FnCx<'_, X64>, value: Value) -> Option<u64> {
    let id = value.as_const()?;
    match cx.module.consts.get(id) {
        Const::Int { value, .. } => Some(value),
        Const::Null(_) | Const::Zero(_) | Const::Undef(_) | Const::Poison(_) => Some(0),
        _ => None,
    }
}

/// The width of integer type `ty` of class `Gpr`, or the refusal.
fn int_type(cx: &FnCx<'_, X64>, ty: TypeId) -> CgResult<u32> {
    match cx.types().kind(ty) {
        TypeKind::Int(bits) if (1..=64).contains(&bits) => Ok(bits),
        TypeKind::Int(_) => unsupported(WIDE),
        _ => unsupported(VECTOR),
    }
}

/// The width of integer type `ty` (`Some`, class `Gpr`), or `None` for
/// `i128` (class `Mem`); the refusal for any other type.
fn int_or_quad(cx: &FnCx<'_, X64>, ty: TypeId) -> CgResult<Option<u32>> {
    match cx.types().kind(ty) {
        TypeKind::Int(bits) if (1..=64).contains(&bits) => Ok(Some(bits)),
        TypeKind::Int(128) => Ok(None),
        TypeKind::Int(_) => unsupported(WIDE),
        _ => unsupported(VECTOR),
    }
}

/// The width of the register a value of `bits` bits is computed in.
#[inline]
fn reg_bits(bits: u32) -> u32 {
    if bits <= 32 { 32 } else { 64 }
}

/// `r ^= value`, `r &= value` and the like with a constant of the
/// register's width: an immediate when one holds it, else through `r10`
/// (which must not be `r`).
fn alu_const(
    cx: &mut FnCx<'_, X64>,
    op: fn(Size, Reg, i64) -> mc::Inst,
    op_rr: fn(Size, Reg, Reg) -> mc::Inst,
    size: Size,
    r: Reg,
    value: u64,
) {
    let imm = if size == Size::Q { value as i64 } else { i64::from(value as u32) };
    if size.fits_imm(imm) {
        put(cx, op(size, r, imm));
    } else {
        put(cx, mc::mov_ri(Size::Q, R10, imm));
        put(cx, op_rr(size, r, R10));
    }
}

// ---- bit manipulation ----

/// `ctlz`, `cttz`, `ctpop`, `bswap`, `bitreverse`, `abs` of `x`, computed
/// in its register.
fn unary<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    x: Value,
    bits: u32,
) -> CgResult<()> {
    if which == Intrinsic::Bswap && !bits.is_multiple_of(16) {
        // The verifier rejects it: an odd number of bytes.
        return unsupported(INTRINSIC);
    }
    let r = cx.use_reg(x)?;
    unary_reg(cx, which, gp(r), bits)?;
    cx.finish(inst, r)
}

/// The register part of [`unary`], for one lane of an element-wise form
/// too (WP-132): `ctlz`, `cttz`, `ctpop`, `bswap`, `bitreverse` or `abs`
/// of the `bits`-bit value in `r` (not `rax`, `r10` or `r11`), in place.
/// Writes `rax`, `r10`, `r11` and the flags.
///
/// # Errors
/// [`INTRINSIC`] for `bswap` of an odd number of bytes; `Internal` for
/// another intrinsic.
pub fn unary_reg(cx: &mut FnCx<'_, X64>, which: Intrinsic<'_>, r: Reg, bits: u32) -> CgResult<()> {
    use Intrinsic as I;
    let sz = size_for_bits(bits);
    let rb = reg_bits(bits);
    match which {
        I::Ctlz => {
            if bits == rb {
                // `bsr` of 0 leaves its destination undefined and sets ZF.
                mov_imm(cx, RAX, u64::from(2 * rb - 1), 32);
                put(cx, mc::bsr_rr(sz, r, r));
                put(cx, mc::cmovcc_rr(Cond::E, sz, r, RAX));
            } else {
                // To the top, with the bit below the value set: a zero
                // value counts `bits`.
                let s = (rb - bits) as u8;
                put(cx, mc::shl_ri(sz, r, s));
                put(cx, mc::bts_ri(sz, r, s - 1));
                put(cx, mc::bsr_rr(sz, r, r));
            }
            put(cx, mc::xor_ri(sz, r, i64::from(rb - 1)));
        }
        I::Cttz => {
            if bits == rb {
                mov_imm(cx, RAX, u64::from(rb), 32);
                put(cx, mc::bsf_rr(sz, r, r));
                put(cx, mc::cmovcc_rr(Cond::E, sz, r, RAX));
            } else {
                put(cx, mc::bts_ri(sz, r, bits as u8));
                put(cx, mc::bsf_rr(sz, r, r));
            }
        }
        I::Ctpop => {
            extend_to(cx, r, bits, Ext::Zero, sz);
            popcount(cx, r, sz);
        }
        I::Bswap => {
            if !bits.is_multiple_of(16) {
                return unsupported(INTRINSIC);
            }
            put(cx, mc::bswap(sz, r));
            if bits != rb {
                put(cx, mc::shr_ri(sz, r, (rb - bits) as u8));
            }
        }
        I::BitReverse => {
            reverse_bits(cx, r, sz);
            if bits != rb {
                put(cx, mc::shr_ri(sz, r, (rb - bits) as u8));
            }
        }
        I::Abs => {
            // The smallest value is its own negation at the width.
            extend_to(cx, r, bits, Ext::Sign, sz);
            put(cx, mc::mov_rr(sz, R11, r));
            put(cx, mc::neg_r(sz, r));
            put(cx, mc::cmovcc_rr(Cond::S, sz, r, R11));
        }
        _ => return cx.bug("an integer intrinsic without a lowering"),
    }
    Ok(())
}

/// The set bits of the whole register `r` (32 or 64 bits, by `sz`), in
/// `r`: `llc`'s SWAR sequence. Writes `r10`, `r11` and the flags.
fn popcount(cx: &mut FnCx<'_, X64>, r: Reg, sz: Size) {
    let wide = sz == Size::Q;
    let pattern = |byte: u64| byte * 0x0101_0101_0101_0101;
    // r -= (r >> 1) & 0x55..
    put(cx, mc::mov_rr(sz, R11, r));
    put(cx, mc::shr_ri(sz, R11, 1));
    alu_const(cx, mc::and_ri, mc::and_rr, sz, R11, pattern(0x55));
    put(cx, mc::sub_rr(sz, r, R11));
    // r = (r & 0x33..) + ((r >> 2) & 0x33..)
    put(cx, mc::mov_rr(sz, R11, r));
    alu_const(cx, mc::and_ri, mc::and_rr, sz, R11, pattern(0x33));
    put(cx, mc::shr_ri(sz, r, 2));
    alu_const(cx, mc::and_ri, mc::and_rr, sz, r, pattern(0x33));
    put(cx, mc::add_rr(sz, r, R11));
    // r = (r + (r >> 4)) & 0x0f..
    put(cx, mc::mov_rr(sz, R11, r));
    put(cx, mc::shr_ri(sz, R11, 4));
    put(cx, mc::add_rr(sz, r, R11));
    alu_const(cx, mc::and_ri, mc::and_rr, sz, r, pattern(0x0f));
    // The sum of the bytes in the top byte.
    if wide {
        put(cx, mc::mov_ri(Size::Q, R10, pattern(1) as i64));
        put(cx, mc::imul_rr(Size::Q, r, R10));
        put(cx, mc::shr_ri(Size::Q, r, 56));
    } else {
        put(cx, mc::imul_rri(Size::D, r, r, 0x0101_0101));
        put(cx, mc::shr_ri(Size::D, r, 24));
    }
}

/// Reverses the bits of the whole register `r` (32 or 64 bits): `bswap`,
/// then nibbles, pairs and bits swapped by masks. Writes `r10`, `r11` and
/// the flags.
fn reverse_bits(cx: &mut FnCx<'_, X64>, r: Reg, sz: Size) {
    put(cx, mc::bswap(sz, r));
    for (k, byte) in [(4u8, 0x0fu64), (2, 0x33), (1, 0x55)] {
        let m = byte * 0x0101_0101_0101_0101;
        // r = ((r >> k) & m) | ((r & m) << k)
        put(cx, mc::mov_rr(sz, R11, r));
        put(cx, mc::shr_ri(sz, R11, k));
        alu_const(cx, mc::and_ri, mc::and_rr, sz, R11, m);
        alu_const(cx, mc::and_ri, mc::and_rr, sz, r, m);
        put(cx, mc::shl_ri(sz, r, k));
        put(cx, mc::or_rr(sz, r, R11));
    }
}

// ---- two operands: min/max and the saturating arithmetic ----

/// `smax`, `smin`, `umax`, `umin`, `{s,u}{add,sub}.sat` of `a` and `b`.
fn two<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    a: Value,
    b: Value,
    bits: u32,
) -> CgResult<()> {
    let ra = cx.use_reg(a)?;
    let rb = cx.use_reg(b)?;
    two_reg(cx, which, gp(ra), gp(rb), bits)?;
    cx.finish(inst, ra)
}

/// The register part of [`two`], for one lane of an element-wise form
/// too (WP-132): `x = which(x, y)` at `bits` bits for the min/max and the
/// saturating arithmetic. `x` and `y` are neither `r10` nor `r11`; both
/// are written, with `r10`, `r11` and the flags.
///
/// # Errors
/// `Internal` for another intrinsic.
pub fn two_reg(
    cx: &mut FnCx<'_, X64>,
    which: Intrinsic<'_>,
    x: Reg,
    y: Reg,
    bits: u32,
) -> CgResult<()> {
    use Intrinsic as I;
    let sz = size_for_bits(bits);
    let rb = reg_bits(bits);
    match which {
        I::Smax | I::Smin | I::Umax | I::Umin => {
            let (ext, take_y) = match which {
                I::Smax => (Ext::Sign, Cond::L),
                I::Smin => (Ext::Sign, Cond::G),
                I::Umax => (Ext::Zero, Cond::B),
                _ => (Ext::Zero, Cond::A),
            };
            extend_to(cx, x, bits, ext, sz);
            extend_to(cx, y, bits, ext, sz);
            put(cx, mc::cmp_rr(sz, x, y));
            put(cx, mc::cmovcc_rr(take_y, sz, x, y));
        }
        I::SaddSat | I::UaddSat | I::SsubSat | I::UsubSat => {
            let signed = matches!(which, I::SaddSat | I::SsubSat);
            let add = matches!(which, I::SaddSat | I::UaddSat);
            let op = if add { mc::add_rr } else { mc::sub_rr };
            if bits == rb {
                // The bound is chosen before the flags are set.
                if signed {
                    // Overflow only towards the first operand's sign.
                    put(cx, mc::mov_rr(sz, R11, x));
                    put(cx, mc::sar_ri(sz, R11, (rb - 1) as u8));
                    alu_const(cx, mc::xor_ri, mc::xor_rr, sz, R11, (1u64 << (rb - 1)) - 1);
                    put(cx, op(sz, x, y));
                    put(cx, mc::cmovcc_rr(Cond::O, sz, x, R11));
                } else {
                    mov_imm(cx, R11, if add { u64::MAX } else { 0 }, rb);
                    put(cx, op(sz, x, y));
                    put(cx, mc::cmovcc_rr(Cond::B, sz, x, R11));
                }
                return Ok(());
            }
            // Exact in 64 bits, then clamped.
            let ext = if signed { Ext::Sign } else { Ext::Zero };
            extend_to(cx, x, bits, ext, Size::Q);
            extend_to(cx, y, bits, ext, Size::Q);
            put(cx, op(Size::Q, x, y));
            if signed {
                let max = (1u64 << (bits - 1)) - 1;
                mov_imm(cx, R11, max, 64);
                put(cx, mc::cmp_rr(Size::Q, x, R11));
                put(cx, mc::cmovcc_rr(Cond::G, Size::Q, x, R11));
                mov_imm(cx, R11, !max, 64);
                put(cx, mc::cmp_rr(Size::Q, x, R11));
                put(cx, mc::cmovcc_rr(Cond::L, Size::Q, x, R11));
            } else if add {
                mov_imm(cx, R11, (1u64 << bits) - 1, 64);
                put(cx, mc::cmp_rr(Size::Q, x, R11));
                put(cx, mc::cmovcc_rr(Cond::A, Size::Q, x, R11));
            } else {
                // Both operands are below 2^63: the difference is negative
                // exactly when it saturates.
                mov_imm(cx, R11, 0, 32);
                put(cx, mc::test_rr(Size::Q, x, x));
                put(cx, mc::cmovcc_rr(Cond::S, Size::Q, x, R11));
            }
        }
        _ => return cx.bug("a two-operand intrinsic without a lowering"),
    }
    Ok(())
}

// ---- funnel shifts ----

/// `fshl` (`left`) or `fshr` of `[a, b, c]`: the concatenation `a:b`
/// shifted by `c` modulo the width, its high (`fshl`) or low half.
#[allow(clippy::many_single_char_names, reason = "the operands are LangRef's a, b, c")]
fn funnel<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    left: bool,
    [a, b, c]: [Value; 3],
    bits: u32,
) -> CgResult<()> {
    let sz = size_for_bits(bits);
    let rb = reg_bits(bits);
    // At one bit every amount is 0 modulo the width.
    if let Some(amount) = const_int(cx, c).or((bits == 1).then_some(0)) {
        let k = (amount % u64::from(bits)) as u32;
        if k == 0 {
            let r = cx.use_reg(if left { a } else { b })?;
            return cx.finish(inst, r);
        }
        let ra = cx.use_reg(a)?;
        let rv = cx.use_reg(b)?;
        let (x, y) = (gp(ra), gp(rv));
        if bits == rb {
            if left {
                put(cx, mc::shld_rri(sz, x, y, k as u8));
                return cx.finish(inst, ra);
            }
            put(cx, mc::shrd_rri(sz, y, x, k as u8));
            return cx.finish(inst, rv);
        }
        extend_to(cx, y, bits, Ext::Zero, sz);
        if left {
            // (a << k) | (b >> (N - k)).
            put(cx, mc::shl_ri(sz, x, k as u8));
            put(cx, mc::shr_ri(sz, y, (bits - k) as u8));
            put(cx, mc::or_rr(sz, x, y));
            return cx.finish(inst, ra);
        }
        // (b >> k) | (a << (N - k)).
        put(cx, mc::shr_ri(sz, y, k as u8));
        put(cx, mc::shl_ri(sz, x, (bits - k) as u8));
        put(cx, mc::or_rr(sz, y, x));
        return cx.finish(inst, rv);
    }
    let ra = cx.use_reg(a)?;
    let rv = cx.use_reg(b)?;
    let rc = cx.use_reg(c)?;
    let in_a = funnel_reg(cx, left, [gp(ra), gp(rv), gp(rc)], bits);
    cx.finish(inst, if in_a { ra } else { rv })
}

/// The register part of [`funnel`] for an amount in a register, for one
/// lane of an element-wise form too (WP-132): `fshl` (`left`) or `fshr`
/// of the `bits`-bit values in `[a, b, c]`, none of them `rax`, `rcx`,
/// `rdx`. The result is in `a` when this returns `true`, else in `b`.
/// Writes all three, `rax`, `rcx`, `rdx`, `r11` and the flags.
#[allow(clippy::many_single_char_names, reason = "the operands are LangRef's a, b, c")]
pub fn funnel_reg(cx: &mut FnCx<'_, X64>, left: bool, [a, b, c]: [Reg; 3], bits: u32) -> bool {
    let sz = size_for_bits(bits);
    if bits == reg_bits(bits) {
        // `shld`/`shrd` take the amount modulo the width; 0 leaves the
        // destination as it is.
        put(cx, mc::mov_rr(Size::D, RCX, c));
        if left {
            put(cx, mc::shld_rrcl(sz, a, b));
            return true;
        }
        put(cx, mc::shrd_rrcl(sz, b, a));
        return false;
    }
    // k = c mod N, in edx.
    extend_to(cx, b, bits, Ext::Zero, sz);
    if bits.is_power_of_two() {
        put(cx, mc::mov_rr(Size::D, RDX, c));
        put(cx, mc::and_ri(Size::D, RDX, i64::from(bits - 1)));
    } else {
        extend_to(cx, c, bits, Ext::Zero, sz);
        put(cx, mc::mov_rr(sz, RAX, c));
        mov_imm(cx, RDX, 0, 32);
        mov_imm(cx, R11, u64::from(bits), 32);
        put(cx, mc::div_r(sz, R11));
    }
    let (first, second) = if left { (a, b) } else { (b, a) };
    // first << k or b >> k; then the other by N - k (N < R: a shift by N
    // moves b out entirely, or a's bits above the width).
    put(cx, mc::mov_rr(Size::D, RCX, RDX));
    put(cx, if left { mc::shl_rcl(sz, first) } else { mc::shr_rcl(sz, first) });
    mov_imm(cx, RCX, u64::from(bits), 32);
    put(cx, mc::sub_rr(Size::D, RCX, RDX));
    put(cx, if left { mc::shr_rcl(sz, second) } else { mc::shl_rcl(sz, second) });
    put(cx, mc::or_rr(sz, first, second));
    left
}

// ---- saturating shifts ----

/// `{s,u}shl.sat` of `bits` <= 64 bits: the operand moved to the top of
/// the 64-bit register, shifted, shifted back and compared; on a
/// difference the saturated value (all ones, or the extreme of the
/// operand's sign); then moved back down. An amount of at least `bits` is
/// poison.
#[allow(clippy::many_single_char_names, reason = "the operands a, b")]
fn shl_sat<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    signed: bool,
    a: Value,
    b: Value,
) -> CgResult<()> {
    let bits = int_type(cx, inst.ty())?;
    deferred_first(cx, inst, None)?;
    let pa = cx.use_reg(a)?;
    let pb = cx.use_reg(b)?;
    let (r, k) = (gp(pa), gp(pb));
    let up = (64 - bits) as u8;
    if up != 0 {
        put(cx, mc::shl_ri(Size::Q, r, up));
        // The amount's bits above its width are unspecified.
        extend_to(cx, k, bits, Ext::Zero, Size::Q);
    }
    put(cx, mc::mov_rr(Size::D, RCX, k));
    // x = r << k in r11, back again in r10.
    put(cx, mc::mov_rr(Size::Q, R11, r));
    put(cx, mc::shl_rcl(Size::Q, R11));
    put(cx, mc::mov_rr(Size::Q, R10, R11));
    put(cx, if signed { mc::sar_rcl(Size::Q, R10) } else { mc::shr_rcl(Size::Q, R10) });
    // The saturated value in rdx, before the compare.
    if signed {
        put(cx, mc::mov_rr(Size::Q, RDX, r));
        put(cx, mc::sar_ri(Size::Q, RDX, 63));
        put(cx, mc::mov_ri(Size::Q, RAX, i64::MAX));
        put(cx, mc::xor_rr(Size::Q, RDX, RAX));
    } else {
        put(cx, mc::mov_ri(Size::Q, RDX, -1));
    }
    put(cx, mc::cmp_rr(Size::Q, R10, r));
    put(cx, mc::cmovcc_rr(Cond::Ne, Size::Q, R11, RDX));
    put(cx, mc::mov_rr(Size::Q, r, R11));
    if up != 0 {
        put(cx, if signed { mc::sar_ri(Size::Q, r, up) } else { mc::shr_ri(Size::Q, r, up) });
    }
    cx.finish(inst, pa)
}

// ---- three-way compares ----

/// `scmp`/`ucmp` (`signed`) of `a` and `b`: -1, 0 or 1 in the result
/// type (2 to 64 bits, or `i128`), of operands of 1 to 64 bits or
/// `i128`.
#[allow(clippy::many_single_char_names, reason = "the operands a, b")]
fn three_way<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    signed: bool,
    a: Value,
    b: Value,
) -> CgResult<()> {
    let wide_result = int_or_quad(cx, inst.ty())?.is_none();
    let operand_bits = int_or_quad(cx, value_ty(cx, a))?;
    deferred_first(cx, inst, None)?;
    let (gt, lt) = if signed { (Cond::G, Cond::L) } else { (Cond::A, Cond::B) };
    let rd = if let Some(bits) = operand_bits {
        let ra = cx.use_reg(a)?;
        let rb = cx.use_reg(b)?;
        let (x, y) = (gp(ra), gp(rb));
        let sz = size_for_bits(bits);
        let ext = if signed { Ext::Sign } else { Ext::Zero };
        extend_to(cx, x, bits, ext, sz);
        extend_to(cx, y, bits, ext, sz);
        put(cx, mc::cmp_rr(sz, x, y));
        put(cx, mc::setcc_r(gt, RAX));
        put(cx, mc::setcc_r(lt, RCX));
        ra
    } else {
        // The flags of a - b (and of b - a) over both limbs order the
        // values (not ZF).
        let (sa, sb) = (cx.use_mem(a)?, cx.use_mem(b)?);
        let rd = cx.temp(Class::Gpr)?;
        ld(cx, R10, sa, 0);
        ld(cx, R11, sa, 1);
        ld(cx, RAX, sb, 0);
        ld(cx, RDX, sb, 1);
        put(cx, mc::cmp_rr(Size::Q, R10, RAX));
        put(cx, mc::sbb_rr(Size::Q, R11, RDX));
        put(cx, mc::setcc_r(lt, RCX));
        ld(cx, R11, sa, 1);
        put(cx, mc::cmp_rr(Size::Q, RAX, R10));
        put(cx, mc::sbb_rr(Size::Q, RDX, R11));
        put(cx, mc::setcc_r(lt, RAX));
        rd
    };
    let r = gp(rd);
    put(cx, mc::sub_rr(Size::B, RAX, RCX));
    put(cx, mc::movsx_rr(Size::Q, r, Size::B, RAX));
    if !wide_result {
        return cx.finish(inst, rd);
    }
    let dst = cx.def_mem(inst)?;
    st(cx, r, dst, 0);
    put(cx, mc::sar_ri(Size::Q, r, 63));
    st(cx, r, dst, 1);
    Ok(())
}

// ---- `i128` (class `Mem`) ----

/// Writes `lo` and `hi` as the two limbs of the `i128` result of `inst`.
fn wide_result<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, lo: Reg, hi: Reg) -> CgResult<()> {
    let dst = cx.def_mem(inst)?;
    st(cx, lo, dst, 0);
    st(cx, hi, dst, 1);
    Ok(())
}

/// The leading (`lead`) or trailing zeros of the 64-bit `r`, in `r`, 64
/// for zero. Writes `rax` and the flags.
fn count64(cx: &mut FnCx<'_, X64>, r: Reg, lead: bool) {
    if lead {
        mov_imm(cx, RAX, 127, 32);
        put(cx, mc::bsr_rr(Size::Q, r, r));
        put(cx, mc::cmovcc_rr(Cond::E, Size::Q, r, RAX));
        put(cx, mc::xor_ri(Size::Q, r, 63));
    } else {
        mov_imm(cx, RAX, 64, 32);
        put(cx, mc::bsf_rr(Size::Q, r, r));
        put(cx, mc::cmovcc_rr(Cond::E, Size::Q, r, RAX));
    }
}

/// `ctlz`, `cttz`, `ctpop`, `bswap`, `bitreverse`, `abs` of an `i128`:
/// the low limb in `rcx`, the high one in `rdx`.
fn wide_unary<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    x: Value,
) -> CgResult<()> {
    use Intrinsic as I;
    let src = cx.use_mem(x)?;
    let (lo, hi) = (RCX, RDX);
    ld(cx, lo, src, 0);
    ld(cx, hi, src, 1);
    match which {
        I::Ctlz | I::Cttz => {
            // The count of the limb that decides, or 64 plus the other's.
            let lead = which == I::Ctlz;
            let (first, second) = if lead { (hi, lo) } else { (lo, hi) };
            put(cx, mc::mov_rr(Size::Q, R11, first));
            count64(cx, first, lead);
            count64(cx, second, lead);
            put(cx, mc::add_ri(Size::Q, second, 64));
            put(cx, mc::test_rr(Size::Q, R11, R11));
            put(cx, mc::cmovcc_rr(Cond::E, Size::Q, first, second));
            mov_imm(cx, R11, 0, 32);
            wide_result(cx, inst, first, R11)
        }
        I::Ctpop => {
            popcount(cx, lo, Size::Q);
            popcount(cx, hi, Size::Q);
            put(cx, mc::add_rr(Size::Q, lo, hi));
            mov_imm(cx, R11, 0, 32);
            wide_result(cx, inst, lo, R11)
        }
        I::Bswap | I::BitReverse => {
            for r in [lo, hi] {
                if which == I::Bswap {
                    put(cx, mc::bswap(Size::Q, r));
                } else {
                    reverse_bits(cx, r, Size::Q);
                }
            }
            wide_result(cx, inst, hi, lo)
        }
        I::Abs => {
            // (x ^ s) - s with s the sign: INT_MIN maps to itself.
            put(cx, mc::mov_rr(Size::Q, R11, hi));
            put(cx, mc::sar_ri(Size::Q, R11, 63));
            put(cx, mc::xor_rr(Size::Q, lo, R11));
            put(cx, mc::xor_rr(Size::Q, hi, R11));
            put(cx, mc::sub_rr(Size::Q, lo, R11));
            put(cx, mc::sbb_rr(Size::Q, hi, R11));
            wide_result(cx, inst, lo, hi)
        }
        _ => cx.bug("an i128 intrinsic without a lowering"),
    }
}

/// `smax`, `smin`, `umax`, `umin` and the saturating arithmetic of two
/// `i128`s: `a` in `r10`/`r11`, `b` in `rax`/`rdx`.
fn wide_two<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    a: Value,
    b: Value,
) -> CgResult<()> {
    use Intrinsic as I;
    let (sa, sb) = (cx.use_mem(a)?, cx.use_mem(b)?);
    let (lo, hi, blo, bhi) = (R10, R11, RAX, RDX);
    ld(cx, lo, sa, 0);
    ld(cx, hi, sa, 1);
    ld(cx, blo, sb, 0);
    ld(cx, bhi, sb, 1);
    match which {
        I::Smax | I::Smin | I::Umax | I::Umin => {
            // The flags of a - b order the whole values (not ZF).
            put(cx, mc::mov_rr(Size::Q, RCX, hi));
            put(cx, mc::cmp_rr(Size::Q, lo, blo));
            put(cx, mc::sbb_rr(Size::Q, RCX, bhi));
            let take_b = match which {
                I::Smax => Cond::L,
                I::Smin => Cond::Ge,
                I::Umax => Cond::B,
                _ => Cond::Ae,
            };
            put(cx, mc::cmovcc_rr(take_b, Size::Q, lo, blo));
            put(cx, mc::cmovcc_rr(take_b, Size::Q, hi, bhi));
        }
        I::SaddSat | I::SsubSat => {
            // On overflow the bound of the first operand's sign s: low
            // limb !s, high limb s ^ i64::MAX.
            let t = gp(cx.temp(Class::Gpr)?);
            put(cx, mc::mov_rr(Size::Q, RCX, hi));
            put(cx, mc::sar_ri(Size::Q, RCX, 63));
            put(cx, mc::mov_ri(Size::Q, t, i64::MAX));
            put(cx, mc::xor_rr(Size::Q, t, RCX));
            put(cx, mc::not_r(Size::Q, RCX));
            if which == I::SaddSat {
                put(cx, mc::add_rr(Size::Q, lo, blo));
                put(cx, mc::adc_rr(Size::Q, hi, bhi));
            } else {
                put(cx, mc::sub_rr(Size::Q, lo, blo));
                put(cx, mc::sbb_rr(Size::Q, hi, bhi));
            }
            put(cx, mc::cmovcc_rr(Cond::O, Size::Q, lo, RCX));
            put(cx, mc::cmovcc_rr(Cond::O, Size::Q, hi, t));
        }
        I::UaddSat | I::UsubSat => {
            let add = which == I::UaddSat;
            mov_imm(cx, RCX, if add { u64::MAX } else { 0 }, 64);
            if add {
                put(cx, mc::add_rr(Size::Q, lo, blo));
                put(cx, mc::adc_rr(Size::Q, hi, bhi));
            } else {
                put(cx, mc::sub_rr(Size::Q, lo, blo));
                put(cx, mc::sbb_rr(Size::Q, hi, bhi));
            }
            put(cx, mc::cmovcc_rr(Cond::B, Size::Q, lo, RCX));
            put(cx, mc::cmovcc_rr(Cond::B, Size::Q, hi, RCX));
        }
        _ => return cx.bug("an i128 intrinsic without a lowering"),
    }
    wide_result(cx, inst, lo, hi)
}

/// `fshl`/`fshr` (`left`) of three `i128`s: with `k = c mod 128`, bit 6
/// of `k` picks three consecutive limbs of `a:b` by `cmov`, and each
/// result limb is `shld`/`shrd` of two of them by `cl` (which the
/// instructions take modulo 64; 0 leaves the destination). Three
/// temporaries.
#[allow(clippy::many_single_char_names, reason = "the operands are LangRef's a, b, c")]
fn wide_funnel<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    left: bool,
    [a, b, c]: [Value; 3],
) -> CgResult<()> {
    let (sa, sb, sc) = (cx.use_mem(a)?, cx.use_mem(b)?, cx.use_mem(c)?);
    let p = gp(cx.temp(Class::Gpr)?);
    let q = gp(cx.temp(Class::Gpr)?);
    let r = gp(cx.temp(Class::Gpr)?);
    ld(cx, RCX, sc, 0);
    // Limbs from the high end of the window (fshl) or the low end (fshr):
    // k < 64 and k >= 64.
    let (small, large) = if left {
        ([(sa, 1), (sa, 0), (sb, 1)], [(sa, 0), (sb, 1), (sb, 0)])
    } else {
        ([(sb, 0), (sb, 1), (sa, 0)], [(sb, 1), (sa, 0), (sa, 1)])
    };
    for (dst, (slot, limb)) in [p, q, r].into_iter().zip(small) {
        ld(cx, dst, slot, limb);
    }
    for (dst, (slot, limb)) in [R10, R11, RAX].into_iter().zip(large) {
        ld(cx, dst, slot, limb);
    }
    put(cx, mc::test_ri(Size::D, RCX, 64));
    put(cx, mc::cmovcc_rr(Cond::Ne, Size::Q, p, R10));
    put(cx, mc::cmovcc_rr(Cond::Ne, Size::Q, q, R11));
    put(cx, mc::cmovcc_rr(Cond::Ne, Size::Q, r, RAX));
    if left {
        // hi = p:q << k, lo = q:r << k.
        put(cx, mc::shld_rrcl(Size::Q, p, q));
        put(cx, mc::shld_rrcl(Size::Q, q, r));
        return wide_result(cx, inst, q, p);
    }
    // lo = q:p >> k, hi = r:q >> k.
    put(cx, mc::shrd_rrcl(Size::Q, p, q));
    put(cx, mc::shrd_rrcl(Size::Q, q, r));
    wide_result(cx, inst, p, q)
}

// ---- memory ----

/// The length of a memory intrinsic as a runtime argument: `size_t`,
/// zero-extended from a narrower type into a temporary.
fn length_arg(cx: &mut FnCx<'_, X64>, len: Value) -> CgResult<CallArg> {
    let bits = cx.types().int_bits(value_ty(cx, len)).unwrap_or(64);
    if bits >= 64 || const_int(cx, len).is_some() {
        // A constant is materialized zero-extended.
        return Ok(CallArg::Value(len));
    }
    let r = cx.use_reg(len)?;
    let t = cx.temp(Class::Gpr)?;
    put(cx, mc::mov_rr(Size::Q, gp(t), gp(r)));
    extend_to(cx, gp(t), bits, Ext::Zero, Size::Q);
    cx.release(len);
    cx.free_temp(r);
    Ok(CallArg::Reg(t))
}

/// `[base + off]`.
#[inline]
fn at(base: Reg, off: u64) -> Mem {
    Mem::base(base, off as i32)
}

/// A byte in every byte of a 64-bit word.
const ONES: u64 = 0x0101_0101_0101_0101;

/// The largest of 16, 8, 4, 2, 1 that is at most `bytes`.
fn access_size(bytes: u64) -> u64 {
    match bytes {
        16.. => 16,
        8.. => 8,
        4.. => 4,
        2.. => 2,
        _ => 1,
    }
}

/// Copies `size` bytes (1, 2, 4, 8 or 16) from `[s + from]` to `[d + to]`
/// through `xmm15` or `r11`.
fn move_piece(cx: &mut FnCx<'_, X64>, d: Reg, s: Reg, off: u64, size: u64) {
    if size == 16 {
        put(cx, mc::movup_rm(FpSize::S, XMM15, at(s, off)));
        put(cx, mc::movup_mr(FpSize::S, at(d, off), XMM15));
        return;
    }
    let width = exact_size(size as u32).unwrap_or(Size::Q);
    put(cx, gpr_load(R11, at(s, off), size as u32));
    put(cx, mc::mov_mr(width, at(d, off), R11));
}

/// `memcpy`, `memcpy.inline`, `memmove` of `len` bytes from `src` to
/// `dst`.
fn copy<'a>(
    cx: &mut FnCx<'a, X64>,
    which: Intrinsic<'a>,
    dst: Value,
    src: Value,
    len: Value,
) -> CgResult<()> {
    let op = match which {
        Intrinsic::Memmove => MemOp::Move,
        Intrinsic::MemcpyInline => MemOp::CopyInline,
        _ => MemOp::Copy,
    };
    let constant = const_int(cx, len);
    let bytes = match mem_plan(op, constant) {
        MemPlan::Nothing => return Ok(()),
        MemPlan::VariableInline => return unsupported(INLINE_LENGTH),
        MemPlan::Call(func) => {
            let len = length_arg(cx, len)?;
            cx.call_runtime(func, &[CallArg::Value(dst), CallArg::Value(src), len], None)?;
            return Ok(());
        }
        MemPlan::Inline { bytes } | MemPlan::Loop { bytes } => bytes,
    };
    let d = gp(cx.use_reg(dst)?);
    let s = gp(cx.use_reg(src)?);
    if op == MemOp::Move {
        move_bytes(cx, d, s, bytes);
    } else if bytes <= INLINE_MAX {
        copy_bytes(cx, d, s, bytes);
    } else {
        copy_loop(cx, d, s, bytes)?;
    }
    Ok(())
}

/// Copies `bytes` (1 to [`INLINE_MAX`]) bytes from `[s]` to `[d]`: 16 at
/// a time, then the rest by one 16-byte access ending at the end of the
/// range (when there were 16), or by two accesses of the largest size
/// that fits, at its start and its end. Every access is inside the
/// range.
fn copy_bytes(cx: &mut FnCx<'_, X64>, d: Reg, s: Reg, bytes: u64) {
    let mut off = 0;
    while bytes - off >= 16 {
        move_piece(cx, d, s, off, 16);
        off += 16;
    }
    let rest = bytes - off;
    if rest == 0 {
        return;
    }
    if off != 0 {
        move_piece(cx, d, s, bytes - 16, 16);
        return;
    }
    let size = access_size(rest);
    move_piece(cx, d, s, 0, size);
    if rest != size {
        move_piece(cx, d, s, rest - size, size);
    }
}

/// Copies `bytes` (1 to [`INLINE_MOVE_MAX`]) bytes from `[s]` to `[d]`,
/// all of them read before any is written: two accesses of the largest
/// size that fits, at the start and at the end of the range (they
/// overlap when `bytes` is not that size).
fn move_bytes(cx: &mut FnCx<'_, X64>, d: Reg, s: Reg, bytes: u64) {
    let size = access_size(bytes);
    let last = bytes - size;
    if size == 16 {
        put(cx, mc::movup_rm(FpSize::S, XMM14, at(s, 0)));
        put(cx, mc::movup_rm(FpSize::S, XMM15, at(s, last)));
        put(cx, mc::movup_mr(FpSize::S, at(d, 0), XMM14));
        put(cx, mc::movup_mr(FpSize::S, at(d, last), XMM15));
        return;
    }
    let width = exact_size(size as u32).unwrap_or(Size::Q);
    put(cx, gpr_load(R10, at(s, 0), size as u32));
    if last != 0 {
        put(cx, gpr_load(R11, at(s, last), size as u32));
    }
    put(cx, mc::mov_mr(width, at(d, 0), R10));
    if last != 0 {
        put(cx, mc::mov_mr(width, at(d, last), R11));
    }
}

/// Copies `bytes` (more than 16) bytes from `[s]` to `[d]` by a loop of
/// 16-byte accesses that moves both registers, counted in `rcx`, then the
/// last 16 bytes of the range again if `bytes` is not a multiple of 16.
fn copy_loop(cx: &mut FnCx<'_, X64>, d: Reg, s: Reg, bytes: u64) -> CgResult<()> {
    mov_imm(cx, RCX, bytes / 16, 64);
    let top = cx.new_label();
    cx.bind(top)?;
    put(cx, mc::movup_rm(FpSize::S, XMM15, at(s, 0)));
    put(cx, mc::movup_mr(FpSize::S, at(d, 0), XMM15));
    put(cx, mc::lea(Size::Q, s, at(s, 16)));
    put(cx, mc::lea(Size::Q, d, at(d, 16)));
    put(cx, mc::sub_ri(Size::Q, RCX, 1));
    branch_cond(cx, Cond::Ne, top)?;
    let rest = (bytes % 16) as i32;
    if rest != 0 {
        put(cx, mc::movup_rm(FpSize::S, XMM15, Mem::base(s, rest - 16)));
        put(cx, mc::movup_mr(FpSize::S, Mem::base(d, rest - 16), XMM15));
    }
    Ok(())
}

/// `memset` or `memset.inline` (`inline`) of `len` bytes at `dst` to
/// `byte`.
fn set(cx: &mut FnCx<'_, X64>, inline: bool, dst: Value, byte: Value, len: Value) -> CgResult<()> {
    let op = if inline { MemOp::SetInline } else { MemOp::Set };
    let constant = const_int(cx, len);
    let bytes = match mem_plan(op, constant) {
        MemPlan::Nothing => return Ok(()),
        MemPlan::VariableInline => return unsupported(INLINE_LENGTH),
        MemPlan::Call(func) => {
            let len = length_arg(cx, len)?;
            cx.call_runtime(func, &[CallArg::Value(dst), CallArg::Value(byte), len], None)?;
            return Ok(());
        }
        MemPlan::Inline { bytes } | MemPlan::Loop { bytes } => bytes,
    };
    let d = gp(cx.use_reg(dst)?);
    // The byte in every byte of r11.
    if let Some(value) = const_int(cx, byte) {
        mov_imm(cx, R11, (value & 0xff) * ONES, 64);
    } else {
        let b = gp(cx.use_reg(byte)?);
        put(cx, mc::movzx_rr(Size::D, R11, Size::B, b));
        put(cx, mc::mov_ri(Size::Q, R10, ONES as i64));
        put(cx, mc::imul_rr(Size::Q, R11, R10));
    }
    if bytes <= INLINE_MAX {
        let mut off = 0;
        for size in [8u64, 4, 2, 1] {
            while bytes - off >= size {
                let width = exact_size(size as u32).unwrap_or(Size::Q);
                put(cx, mc::mov_mr(width, at(d, off), R11));
                off += size;
            }
        }
        return Ok(());
    }
    mov_imm(cx, RCX, bytes / 8, 64);
    let top = cx.new_label();
    cx.bind(top)?;
    put(cx, mc::mov_mr(Size::Q, at(d, 0), R11));
    put(cx, mc::lea(Size::Q, d, at(d, 8)));
    put(cx, mc::sub_ri(Size::Q, RCX, 1));
    branch_cond(cx, Cond::Ne, top)?;
    let rest = (bytes % 8) as i32;
    if rest != 0 {
        put(cx, mc::mov_mr(Size::Q, Mem::base(d, rest - 8), R11));
    }
    Ok(())
}

// ---- the frame ----

/// `frameaddress(depth)` or `returnaddress(depth)` (`ret`): `rbp`, then
/// `depth` saved frame pointers loaded from the chain of frame records,
/// then for `ret` the return address above the last record
/// (`[x + 8]`), as `llc` walks it. Every hod function has its record.
fn frame_walk<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, ret: bool, depth: Value) -> CgResult<()> {
    let depth = const_int(cx, depth).unwrap_or(0) & 0xffff_ffff;
    if depth > MAX_DEPTH {
        return unsupported(FRAME_DEPTH);
    }
    let rd = cx.def_reg(inst)?;
    let r = gp(rd);
    let mut base = RBP;
    for _ in 0..depth {
        put(cx, mc::mov_rm(Size::Q, r, Mem::base(base, 0)));
        base = r;
    }
    if ret {
        put(cx, mc::mov_rm(Size::Q, r, Mem::base(base, 8)));
    } else if depth == 0 {
        put(cx, mc::mov_rr(Size::Q, r, RBP));
    }
    cx.finish(inst, rd)
}

// ---- `__builtin_setjmp` and `__builtin_longjmp` (WP-147) ----

/// Refuses both intrinsics under a shadow stack.
fn sjlj_allowed(cx: &FnCx<'_, X64>) -> CgResult<()> {
    let bits = cx.module.props.branch_protection.bits();
    if bits & hod_ir::BranchProtection::CET_SHSTK != 0 {
        return unsupported(SJLJ_SHADOW_STACK);
    }
    Ok(())
}

/// `%r = call i32 @llvm.eh.sjlj.setjmp(ptr %buf)`, `llc`'s layout of the
/// five-word buffer (the front end has stored the frame address in word
/// 0 and the stack pointer in word 2): the resume address to word 1, 0
/// as the result; a `longjmp` (hod's or `llc`'s) restores `rbp` and
/// `rsp` and jumps there, where the result is 1. Every value is in
/// memory across it (as across a call: `displace_all`, the entry
/// block's arguments included), and from it on no spill slot released
/// is reused (`FnCx::never_reuse_spill_slots`, as for a `returns_twice`
/// call), so the second return reads what the first one did. The function saves all five
/// callee-saved registers (`FrameState::save_all`, §5.6.15): the resume
/// point is entered with the registers of the code that jumped, and the
/// epilogue restores the caller's from `rbp`.
///
/// ```text
///   lea r11, [rip + .Lresume] · mov [buf + 8], r11 · mov eax, 0 · jmp .Ldone
/// .Lresume: mov eax, 1
/// .Ldone:   mov rd, eax
/// ```
fn sjlj_setjmp<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, buf: Value) -> CgResult<()> {
    sjlj_allowed(cx)?;
    if cx.target.coro {
        return unsupported(SJLJ_COROUTINE);
    }
    cx.state.frame.save_all = true;
    deferred_first(cx, inst, None)?;
    let b = gp(cx.use_reg(buf)?);
    // The code after the call runs again at the second return: every
    // value is in memory across it and no slot it reads is reused.
    cx.never_reuse_spill_slots();
    cx.displace_all()?;
    let (resume, done) = (cx.new_label(), cx.new_label());
    let lea = mc::lea(Size::Q, R11, Mem::rip(0));
    let Some(site) = lea.site() else {
        return cx.bug("a RIP-relative lea without a displacement field");
    };
    let at = cx.offset();
    put(cx, lea);
    cx.fixup(at + site.offset as u32, crate::common::REL32, resume)?;
    put(cx, mc::mov_mr(Size::Q, Mem::base(b, 8), R11));
    put(cx, mc::mov_ri(Size::D, RAX, 0));
    crate::common::branch(cx, done)?;
    cx.bind(resume)?;
    put(cx, mc::mov_ri(Size::D, RAX, 1));
    cx.bind(done)?;
    let rd = cx.def_reg(inst)?;
    put(cx, mc::mov_rr(Size::D, gp(rd), RAX));
    cx.finish(inst, rd)
}

/// `call void @llvm.eh.sjlj.longjmp(ptr %buf)`: `rbp` from word 0, the
/// resume address from word 1, `rsp` from word 2, `jmp`. Every word is
/// read before `rbp` or `rsp` is written, so neither the buffer's
/// address nor its words depend on the frame being left (`llc` 19 and
/// 22 can reload `rbp` before reading the buffer through it).
fn sjlj_longjmp<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, buf: Value) -> CgResult<()> {
    sjlj_allowed(cx)?;
    deferred_first(cx, inst, None)?;
    let b = gp(cx.use_reg(buf)?);
    put(cx, mc::mov_rm(Size::Q, R11, Mem::base(b, 8)));
    put(cx, mc::mov_rm(Size::Q, RCX, Mem::base(b, 16)));
    put(cx, mc::mov_rm(Size::Q, RBP, Mem::base(b, 0)));
    put(cx, mc::mov_rr(Size::Q, Reg::Rsp, RCX));
    put(cx, mc::jmp_r(R11));
    Ok(())
}

// ---- `is.constant` and the instructions its folding removes ----

/// Whether `llvm.is.constant` of `value` is true, as `llc`'s
/// `isManifestConstant` decides it: a constant that is data (a number,
/// `null`, a zero, `undef`, `poison`, `none`, a string of bytes), or an
/// aggregate, splat or constant expression all of whose operands are.
/// The address of a symbol or block, and so anything built on one, is
/// not. (`hod_a64::tail::is_manifest`.)
pub fn is_manifest(cx: &FnCx<'_, X64>, value: Value) -> bool {
    let Some(id) = value.as_const() else { return false };
    let mut pending: Vec<ConstId> = Vec::new();
    let mut next = Some(id);
    while let Some(id) = next {
        match cx.module.consts.get(id) {
            Const::Int { .. }
            | Const::WideInt { .. }
            | Const::Float { .. }
            | Const::WideFloat { .. }
            | Const::Null(_)
            | Const::Zero(_)
            | Const::Undef(_)
            | Const::Poison(_)
            | Const::NoneToken
            | Const::Bytes { .. }
            // `inttoptr` of an integer: an expression of data.
            | Const::AddrOfInt { .. } => {}
            Const::Aggregate { elems: args, .. } | Const::Expr { args, .. } => {
                pending.extend_from_slice(args);
            }
            Const::Splat { elem, .. } => pending.push(elem),
            _ => return false,
        }
        next = pending.pop();
    }
    true
}

/// The high bit of a use count: a use outside the block, or by a `phi`.
const EXPORTED: u32 = 1 << 31;

thread_local! {
    /// Per instruction of the function: its uses by instructions other
    /// than folded calls, and [`EXPORTED`]. One buffer per thread.
    static USES: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
    /// Two bitsets over the instructions: the folded calls, then the
    /// instructions not emitted. Empty when the function drops nothing.
    static BITS: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
}

/// Whether the call is one `llc`'s `LowerConstantIntrinsics` replaces by
/// a constant before instruction selection.
fn folds(site: &CallSite<'_>) -> bool {
    matches!(site.intrinsic(), Some(Intrinsic::IsConstant | Intrinsic::ObjectSize))
}

/// Whether `inst` has no effect but its result: the driver's rule for a
/// result nothing uses (`hod_cg`'s `driver.rs`, `has_no_effect`), which is
/// also what `SelectionDAG` deletes when nothing uses its node.
fn removable(inst: Inst<'_>) -> bool {
    let op = inst.opcode();
    if op.is_binary() || op.is_cast() {
        return true;
    }
    match op {
        Opcode::FNeg
        | Opcode::ICmp
        | Opcode::FCmp
        | Opcode::PtrAdd
        | Opcode::Select
        | Opcode::Freeze
        | Opcode::ExtractValue
        | Opcode::InsertValue
        | Opcode::ExtractElement
        | Opcode::InsertElement
        | Opcode::ShuffleVector => true,
        Opcode::Load => matches!(
            inst.kind(),
            InstKind::Load { mem, .. } if !mem.is_volatile() && mem.ordering().is_none()
        ),
        _ => false,
    }
}

/// `Target::begin_function`'s part (`spec/codegen.md` §5.5.1 "WP-084",
/// §5.6.8): in a function that calls `llvm.is.constant` or
/// `llvm.objectsize`, decides which instructions are not emitted, as
/// `llc` does (`hod_a64::tail::begin`). Its IR pass replaces those calls
/// by constants and nothing else; `SelectionDAG` then deletes a node
/// nothing uses and that has no effect, which frees its operands' nodes
/// in turn; a value used in another block (or by a `phi`) is exported and
/// stays. A function without those calls costs a look at its call sites.
/// [`dropped`] answers for each instruction afterwards.
pub fn begin(cx: &FnCx<'_, X64>) {
    begin_body(cx.body);
}

/// [`begin`] for `body`.
pub fn begin_body(body: &Body) {
    let drops = body.calls().any(|site| folds(&site)) && decide(body);
    if !drops {
        BITS.with(|bits| bits.borrow_mut().clear());
    }
}

/// Fills [`USES`] and [`BITS`] for `body`; whether any instruction is
/// not emitted.
fn decide(body: &Body) -> bool {
    let n = body.inst_count();
    let words = n.div_ceil(64);
    USES.with(|uses| {
        BITS.with(|bits| {
            let (mut uses, mut bits) = (uses.borrow_mut(), bits.borrow_mut());
            uses.clear();
            uses.resize(n, 0);
            bits.clear();
            bits.resize(2 * words, 0);
            let set =
                |bits: &mut [u64], base: usize, i: usize| bits[base + i / 64] |= 1 << (i % 64);
            let get =
                |bits: &[u64], base: usize, i: usize| bits[base + i / 64] >> (i % 64) & 1 != 0;
            for site in body.calls() {
                if folds(&site) {
                    set(&mut bits, 0, site.inst().index());
                }
            }
            for block in body.blocks() {
                let range = block.inst_range();
                for inst in block.insts() {
                    if get(&bits, 0, inst.id().index()) {
                        continue;
                    }
                    let phi = inst.opcode() == Opcode::Phi;
                    for &op in inst.operands() {
                        if let ValueKind::Inst(x) = op.kind() {
                            let u = &mut uses[x.index()];
                            *u = (*u & EXPORTED) | ((*u & !EXPORTED) + 1).min(!EXPORTED);
                            if phi || !range.contains(&x.raw()) {
                                *u |= EXPORTED;
                            }
                        }
                    }
                }
            }
            let mut any = false;
            for block in body.blocks() {
                for inst in block.insts().rev() {
                    let i = inst.id().index();
                    if uses[i] != 0
                        || inst.ty() == TypeId::VOID
                        || get(&bits, 0, i)
                        || !removable(inst)
                    {
                        continue;
                    }
                    set(&mut bits, words, i);
                    any = true;
                    for &op in inst.operands() {
                        if let ValueKind::Inst(x) = op.kind() {
                            let u = &mut uses[x.index()];
                            if *u & !EXPORTED != 0 {
                                *u -= 1;
                            }
                        }
                    }
                }
            }
            any
        })
    })
}

/// Whether `inst` of the function [`begin`] last looked at is not
/// emitted: `Target::lower` returns at once for it.
pub fn dropped(inst: Inst<'_>) -> bool {
    let i = inst.id().index();
    BITS.with(|bits| {
        let bits = bits.borrow();
        let words = bits.len() / 2;
        words != 0 && bits.get(words + i / 64).is_some_and(|w| w >> (i % 64) & 1 != 0)
    })
}

/// Whether a register is one of those a lowering here may write freely.
#[cfg(test)]
fn is_fixed(reg: hod_cg::PReg) -> bool {
    matches!(gp(reg), Reg::Rax | Reg::Rcx | Reg::Rdx | Reg::R10 | Reg::R11)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_widths() {
        assert_eq!(reg_bits(1), 32);
        assert_eq!(reg_bits(32), 32);
        assert_eq!(reg_bits(33), 64);
        assert_eq!(reg_bits(64), 64);
    }

    #[test]
    fn access_sizes() {
        let sizes: Vec<u64> = (1..=17).map(access_size).collect();
        assert_eq!(sizes, [1, 2, 2, 4, 4, 4, 4, 8, 8, 8, 8, 8, 8, 8, 8, 16, 16]);
    }

    /// The registers this module writes freely never hold a value.
    #[test]
    fn fixed_registers_hold_nothing() {
        for reg in [crate::reg::RAX, crate::reg::RCX, crate::reg::RDX] {
            assert!(is_fixed(reg));
            assert!(!crate::REG_INFO.holdable.contains(reg));
        }
    }
}
