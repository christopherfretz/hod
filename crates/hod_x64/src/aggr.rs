//! `Mem`-class values that are neither vectors nor `x86_fp80`: integers
//! wider than 64 bits, first-class aggregates, `Mem` constants and the
//! `*.with.overflow` intrinsics (`spec/codegen.md` §5.6.5; design
//! 0006 §2.6). **Owner: WP-126.** Tests: `tests/aggr.rs`.
//!
//! The dispatch in `target.rs` routes here every binary operation,
//! `freeze`, cast, `icmp`, `load`, `store` and `select` whose `Mem`-class
//! side is not a vector's or an `x86_fp80`'s, every `extractvalue` and
//! `insertvalue`, all six overflow intrinsics (of any width: their
//! result `{iN, i1}` is an aggregate), and `Target::materialize_mem`.
//! `load`, `store`, `select`, `freeze`, `extractvalue`, `insertvalue` and
//! `bitcast` of a `Mem` vector or an `x86_fp80` are copies of their
//! image and are done here too.
//!
//! # What a slot holds (the AArch64 rule, §5.5.1 "Mem-class values")
//!
//! A `Mem` value is the first *store size* bytes of its slot, laid out as
//! its in-memory image; the bytes after them are unspecified.
//!
//! - An integer wider than 64 bits is `n = ⌈N/64⌉` little-endian 64-bit
//!   **limbs**; the alloc size is a multiple of 16, so all `n` limbs are
//!   inside the slot. The bits above `N` in the top limb are
//!   **unspecified** (the value may have come from registers). A consumer
//!   that depends on them (a compare, a right shift, a division, `zext`,
//!   `sext`, a `store`, `insertvalue`, an overflow check, a return in
//!   memory) extends the top limb first; `add`, `sub`, `mul`, the bitwise
//!   operations and `shl` leave garbage there.
//! - A struct, an array or a vector holds its image exactly: every integer
//!   field with the bits above its width zero, the padding unspecified.
//!
//! # Registers
//!
//! A lowering here computes in `rax`, `rcx`, `rdx` (which never hold a
//! value), `r10`, `r11` and `xmm15`, and asks the pool for nothing: limb
//! arithmetic loads limbs into scratch registers, operates and stores
//! back, with no load folding (design 0006 §2.4). A carry chain is
//! `add`/`adc` (`sub`/`sbb`) with only `mov` between links; an operand
//! limb that must be extended (the top one) is extended into `rcx`/`rdx`
//! *before* the chain, since the extension's shifts write the flags.
//! Glue ([`materialize_mem`]) writes `rax` and `r11` only and keeps the
//! flags.
//!
//! # A value returned in memory
//!
//! The caller passes the buffer's address in `rdi` (`spec/abi-x64.md`
//! §5.2, §8.2). The prologue saves it (`call::enter`,
//! `CallState::ret_addr`, WP-125); [`ret_memory`] (`term::ret`) writes
//! the value through it as a `store` would, and the epilogue
//! (`call::before_return`) reloads it into `rax`.

#![allow(
    clippy::many_single_char_names,
    reason = "limb arithmetic: r = a op b over limbs, x and y the scratch registers"
)]
#![allow(clippy::too_many_lines, reason = "one match per operation, read top to bottom")]

use hod_cg::sym::{FpType, fp_to_int_routine, int_divide_routine, int_to_fp_routine};
use hod_cg::{CallArg, CgError, CgResult, Class, FnCx, FrameRef, SlotPlace, Target};
use hod_ir::{
    Arch, Const, Inst, IntPredicate, Intrinsic, MemFlags, Opcode, TypeId, TypeKind, Value,
    ValueKind,
};
use hod_mc_x64 as mc;
use hod_mc_x64::{Cond, FpSize, Mem, Reg, Scale, Size};

use crate::common::{
    R10, R11, XMM15, deferred_first, exact_size, frame_address, frame_inst, gp, load_fpr, load_gpr,
    pieces, put, slot_address, store_fpr, unsupported, value_ty, xm,
};
use crate::target::X64;

/// The group's refusal of a form it does not know (an operation of class
/// `Mem` on something that is no integer).
pub const AGGR: &str = "x64:todo:aggr";
/// `udiv`, `sdiv`, `urem`, `srem` of an integer wider than 128 bits: the
/// runtime has no function for it (`__udivti3` and its kind stop at 128).
pub const WIDE_DIVISION: &str = "x64:wide-division";
/// `llvm.{s,u}mul.with.overflow` of an integer wider than half of
/// [`MAX_QUADRATIC_BITS`]: the product is formed at twice the width.
pub const WIDE_MUL_OVERFLOW: &str = "x64:wide-mul-overflow";
/// A conversion between a floating-point type and an integer wider than
/// 128 bits, or between `bfloat` and an integer of class `Mem`.
pub const WIDE_FP_CAST: &str = "x64:wide-fp-cast";
/// `mul` and variable shifts of integers wider than this are refused:
/// their code grows with the square of the width (`mul`) or needs a
/// frame area of twice it (shifts).
pub const MAX_QUADRATIC_BITS: u32 = 4096;
/// A `mul` of an integer wider than [`MAX_QUADRATIC_BITS`].
pub const WIDE_MUL: &str = "x64:wide-mul";
/// A shift by a variable amount of an integer wider than
/// [`MAX_QUADRATIC_BITS`].
pub const WIDE_SHIFT: &str = "x64:wide-shift";
/// A cast with a `Mem` side that is none of the above (`addrspacecast`).
pub const CAST: &str = "x64:mem-cast";
/// An overflow intrinsic of a vector.
pub const OVERFLOW: &str = "x64:overflow-intrinsic";

// ---- limbs, slots and destinations ----

/// The 64-bit limbs of an integer of `bits` bits.
#[inline]
pub fn limbs(bits: u32) -> u32 {
    bits.div_ceil(64)
}

/// The meaningful bits of the top limb (1 to 64).
#[inline]
pub fn top_bits(bits: u32) -> u32 {
    bits - 64 * (limbs(bits) - 1)
}

/// `slot` moved `off` bytes on.
#[inline]
fn at(slot: FrameRef, off: u32) -> FrameRef {
    slot.offset_by(off)
}

/// `mov r, [slot + 8 * limb]`.
pub fn ld(cx: &mut FnCx<'_, X64>, r: Reg, slot: FrameRef, limb: u32) {
    frame_inst(cx, at(slot, 8 * limb), |m| mc::mov_rm(Size::Q, r, m));
}

/// `mov [slot + 8 * limb], r`.
pub fn st(cx: &mut FnCx<'_, X64>, r: Reg, slot: FrameRef, limb: u32) {
    frame_inst(cx, at(slot, 8 * limb), |m| mc::mov_mr(Size::Q, m, r));
}

/// `mov qword [slot + 8 * limb], 0`.
fn st_zero(cx: &mut FnCx<'_, X64>, slot: FrameRef, limb: u32) {
    frame_inst(cx, at(slot, 8 * limb), |m| mc::mov_mi(Size::Q, m, 0));
}

/// `mov r32, 0`: zero without touching the flags.
fn zero(cx: &mut FnCx<'_, X64>, r: Reg) {
    put(cx, mc::mov_ri(Size::D, r, 0));
}

/// Extends `r` in place from its low `bits` bits (1 to 64; nothing for
/// 64): `movsxd`/`mov r32` for 32, `movsx`/`movzx` for 8 and 16, a pair
/// of shifts otherwise (which write the flags).
pub fn extend_reg(cx: &mut FnCx<'_, X64>, r: Reg, bits: u32, signed: bool) {
    match (bits, signed) {
        (64.., _) => {}
        (32, true) => put(cx, mc::movsxd_rr(r, r)),
        (32, false) => put(cx, mc::mov_rr(Size::D, r, r)),
        (8 | 16, _) => {
            let from = if bits == 8 { Size::B } else { Size::W };
            put(
                cx,
                if signed {
                    mc::movsx_rr(Size::Q, r, from, r)
                } else {
                    mc::movzx_rr(Size::D, r, from, r)
                },
            );
        }
        _ => {
            let k = (64 - bits) as u8;
            put(cx, mc::shl_ri(Size::Q, r, k));
            put(cx, if signed { mc::sar_ri(Size::Q, r, k) } else { mc::shr_ri(Size::Q, r, k) });
        }
    }
}

/// Loads limb `limb` of the integer of `bits` bits at `slot` into `r`,
/// extended from the width when it is the top limb, and the extension
/// word (0, or the sign) for a limb above it. Writes the flags.
pub fn ld_ext(cx: &mut FnCx<'_, X64>, r: Reg, slot: FrameRef, bits: u32, limb: u32, signed: bool) {
    let n = limbs(bits);
    if limb + 1 < n {
        ld(cx, r, slot, limb);
        return;
    }
    ld(cx, r, slot, n - 1);
    extend_reg(cx, r, top_bits(bits), signed);
    if limb >= n {
        if signed {
            put(cx, mc::sar_ri(Size::Q, r, 63));
        } else {
            zero(cx, r);
        }
    }
}

/// The store size of `ty` in bytes, for a copy; above what a frame holds
/// it is `limit:frame-size`.
fn store_size(cx: &FnCx<'_, X64>, ty: TypeId) -> CgResult<u32> {
    match u32::try_from(cx.types().size_of(ty)) {
        Ok(bytes) if bytes <= crate::frame::MAX_X64_FRAME => Ok(bytes),
        _ => Err(CgError::unsupported(hod_cg::LIMIT_FRAME_SIZE)),
    }
}

/// The width of `ty` if it is an integer of class `Mem`.
fn wide_int(cx: &FnCx<'_, X64>, ty: TypeId) -> Option<u32> {
    cx.types().int_bits(ty).filter(|&bits| bits > 64)
}

/// Whether `value` is a constant whose image is all zero bytes.
fn is_zero_const(cx: &FnCx<'_, X64>, value: Value) -> bool {
    let ValueKind::Const(id) = value.kind() else { return false };
    cx.module.consts.is_zero_or_undef(id)
}

/// Where the bytes of a copy go or come from: a frame place, or the
/// memory a general register points to (never `r10`, `r11`).
#[derive(Clone, Copy, Debug)]
pub enum Place {
    /// A frame place.
    Frame(FrameRef),
    /// `[reg]`.
    Ptr(Reg),
}

impl Place {
    /// The place `off` bytes on.
    #[must_use]
    pub fn offset_by(self, off: u32) -> Place {
        match self {
            Place::Frame(f) => Place::Frame(f.offset_by(off)),
            Place::Ptr(_) => self,
        }
    }
}

/// Emits the instruction `make` builds over byte `off` of `place`. The
/// offsets of a copy stay below [`crate::frame::MAX_X64_FRAME`]
/// ([`store_size`]), so a pointer's displacement fits.
pub fn place_inst(
    cx: &mut FnCx<'_, X64>,
    place: Place,
    off: u32,
    make: impl FnOnce(Mem) -> mc::Inst,
) {
    match place {
        Place::Frame(f) => frame_inst(cx, f.offset_by(off), make),
        Place::Ptr(r) => put(cx, make(Mem::base(r, off as i32))),
    }
}

/// Copies `bytes` bytes from `src` to `dst`: 16 at a time through
/// `xmm15`, then pieces of 8, 4, 2, 1 through `r10`. Flags kept.
pub fn copy_place(cx: &mut FnCx<'_, X64>, dst: Place, src: Place, bytes: u32) {
    let mut done = 0;
    while bytes - done >= 16 {
        place_inst(cx, src, done, |m| mc::movup_rm(FpSize::S, XMM15, m));
        place_inst(cx, dst, done, |m| mc::movup_mr(FpSize::S, m, XMM15));
        done += 16;
    }
    for (off, size) in pieces(bytes - done) {
        let s = exact_size(size).unwrap_or(Size::Q);
        place_inst(cx, src, done + off, |m| crate::common::gpr_load(R10, m, size));
        place_inst(cx, dst, done + off, |m| mc::mov_mr(s, m, R10));
    }
}

/// Writes zero to `bytes` bytes at `dst` (immediate stores). Flags kept.
pub fn zero_place(cx: &mut FnCx<'_, X64>, dst: Place, bytes: u32) {
    for (off, size) in pieces(bytes) {
        let s = exact_size(size).unwrap_or(Size::Q);
        place_inst(cx, dst, off, |m| mc::mov_mi(s, m, 0));
    }
}

/// Stores the low `bytes` (1 to 8) bytes of `r` at byte `off` of `dst`,
/// and no byte more: one `mov` for 1, 2, 4, 8; otherwise pieces, `r`
/// shifted right between them (which writes the flags and `r`).
fn store_tail(cx: &mut FnCx<'_, X64>, r: Reg, dst: Place, off: u32, bytes: u32) {
    if let Some(s) = exact_size(bytes) {
        place_inst(cx, dst, off, |m| mc::mov_mr(s, m, r));
        return;
    }
    let mut last = 0;
    for (o, size) in pieces(bytes) {
        if o != last {
            put(cx, mc::shr_ri(Size::Q, r, ((o - last) * 8) as u8));
            last = o;
        }
        let s = exact_size(size).unwrap_or(Size::Q);
        place_inst(cx, dst, off + o, |m| mc::mov_mr(s, m, r));
    }
}

/// Writes the image of the integer of `bits` bits at `src` to `dst`:
/// exactly its store size, the bits above the width zero. Writes `r10`
/// and the flags.
fn write_int_image(cx: &mut FnCx<'_, X64>, src: FrameRef, bits: u32, dst: Place) {
    let n = limbs(bits);
    for limb in 0..n - 1 {
        ld(cx, R10, src, limb);
        place_inst(cx, dst, 8 * limb, |m| mc::mov_mr(Size::Q, m, R10));
    }
    ld(cx, R10, src, n - 1);
    extend_reg(cx, R10, top_bits(bits), false);
    store_tail(cx, R10, dst, 8 * (n - 1), bits.div_ceil(8) - 8 * (n - 1));
}

/// Writes the image of the `Mem` value of type `ty` at `src` to `dst`:
/// an integer by [`write_int_image`], anything else by copying its store
/// size.
fn write_image(cx: &mut FnCx<'_, X64>, ty: TypeId, src: FrameRef, dst: Place) -> CgResult<()> {
    if let Some(bits) = wide_int(cx, ty) {
        write_int_image(cx, src, bits, dst);
    } else {
        let bytes = store_size(cx, ty)?;
        copy_place(cx, dst, Place::Frame(src), bytes);
    }
    Ok(())
}

/// The place of the memory `ptr` points to, for a `load` or `store`: the
/// frame itself for a directly placed stack slot, else the pointer in a
/// register (`rax` for an over-aligned slot; a symbol or constant goes to
/// a temporary).
fn pointer(cx: &mut FnCx<'_, X64>, ptr: Value) -> CgResult<Place> {
    if let ValueKind::Slot(slot) = ptr.kind() {
        return Ok(match cx.slot_place(slot)? {
            SlotPlace::Direct(place) => Place::Frame(place),
            SlotPlace::Indirect(_) => {
                slot_address(cx, Reg::Rax, slot)?;
                Place::Ptr(Reg::Rax)
            }
        });
    }
    Ok(Place::Ptr(gp(cx.use_reg(ptr)?)))
}

// ---- binary operations ----

/// `%r = <op> %lhs, %rhs` of a wide integer.
///
/// # Errors
/// [`WIDE_DIVISION`] for a division wider than 128 bits; [`WIDE_MUL`]
/// and [`WIDE_SHIFT`] beyond [`MAX_QUADRATIC_BITS`]; [`AGGR`] for
/// something that is no integer; what the operands and the runtime call
/// report.
pub fn binary<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, lhs: Value, rhs: Value) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    let Some(bits) = wide_int(cx, inst.ty()) else {
        return unsupported(AGGR);
    };
    let op = inst.opcode();
    match op {
        Opcode::Add | Opcode::Sub | Opcode::And | Opcode::Or | Opcode::Xor => {
            let (a, b) = (cx.use_mem(lhs)?, cx.use_mem(rhs)?);
            let r = cx.def_mem(inst)?;
            limbwise(cx, op, bits, r, a, b);
            Ok(())
        }
        Opcode::Mul => {
            if bits > MAX_QUADRATIC_BITS {
                return unsupported(WIDE_MUL);
            }
            let (a, b) = (cx.use_mem(lhs)?, cx.use_mem(rhs)?);
            let r = cx.def_mem(inst)?;
            multiply(cx, bits, r, a, b);
            Ok(())
        }
        Opcode::Shl | Opcode::LShr | Opcode::AShr => {
            if let Some(amount) = const_amount(cx, rhs) {
                if amount >= u64::from(bits) {
                    // Poison (§4.7): nothing to compute.
                    return Ok(());
                }
                let a = cx.use_mem(lhs)?;
                let r = cx.def_mem(inst)?;
                shift_const(cx, op, bits, r, a, amount as u32);
                Ok(())
            } else {
                if bits > MAX_QUADRATIC_BITS {
                    return unsupported(WIDE_SHIFT);
                }
                let (a, b) = (cx.use_mem(lhs)?, cx.use_mem(rhs)?);
                let r = cx.def_mem(inst)?;
                shift_var(cx, op, bits, r, a, b)
            }
        }
        Opcode::UDiv | Opcode::SDiv | Opcode::URem | Opcode::SRem => {
            if bits > 128 {
                return unsupported(WIDE_DIVISION);
            }
            divide(cx, inst, op, bits, lhs, rhs)
        }
        _ => unsupported(AGGR),
    }
}

/// `add`, `sub` (a carry chain, nothing but `mov` between its links) and
/// the bitwise operations, limb by limb.
fn limbwise(cx: &mut FnCx<'_, X64>, op: Opcode, bits: u32, r: FrameRef, a: FrameRef, b: FrameRef) {
    for limb in 0..limbs(bits) {
        ld(cx, R10, a, limb);
        ld(cx, R11, b, limb);
        let first = limb == 0;
        let word = match op {
            Opcode::Add if first => mc::add_rr(Size::Q, R10, R11),
            Opcode::Add => mc::adc_rr(Size::Q, R10, R11),
            Opcode::Sub if first => mc::sub_rr(Size::Q, R10, R11),
            Opcode::Sub => mc::sbb_rr(Size::Q, R10, R11),
            Opcode::And => mc::and_rr(Size::Q, R10, R11),
            Opcode::Or => mc::or_rr(Size::Q, R10, R11),
            _ => mc::xor_rr(Size::Q, R10, R11),
        };
        put(cx, word);
        st(cx, R10, r, limb);
    }
}

/// `mul` modulo 2^(64n), `r` distinct from `a` and `b`: for two limbs
/// `lo:hi = a0·b0` by `mul`, `hi += a0·b1 + a1·b0` by `imul`; wider,
/// schoolbook into the zeroed result, each partial product by `mul`
/// (`rdx:rax`) with its carry in `rcx`. Writes `rax`, `rcx`, `rdx`,
/// `r10`, `r11`.
pub fn multiply(cx: &mut FnCx<'_, X64>, bits: u32, r: FrameRef, a: FrameRef, b: FrameRef) {
    let n = limbs(bits);
    let (rax, rcx, rdx) = (Reg::Rax, Reg::Rcx, Reg::Rdx);
    if n == 2 {
        ld(cx, rax, a, 0);
        ld(cx, R10, b, 0);
        put(cx, mc::mul_r(Size::Q, R10));
        ld(cx, R11, a, 0);
        ld(cx, R10, b, 1);
        put(cx, mc::imul_rr(Size::Q, R11, R10));
        put(cx, mc::add_rr(Size::Q, rdx, R11));
        ld(cx, R11, a, 1);
        ld(cx, R10, b, 0);
        put(cx, mc::imul_rr(Size::Q, R11, R10));
        put(cx, mc::add_rr(Size::Q, rdx, R11));
        st(cx, rax, r, 0);
        st(cx, rdx, r, 1);
        return;
    }
    for limb in 0..n {
        st_zero(cx, r, limb);
    }
    for i in 0..n {
        ld(cx, R11, a, i);
        zero(cx, rcx);
        for j in 0..n - i {
            let k = i + j;
            ld(cx, rax, b, j);
            if k + 1 == n {
                put(cx, mc::imul_rr(Size::Q, rax, R11));
                ld(cx, R10, r, k);
                put(cx, mc::add_rr(Size::Q, R10, rax));
                put(cx, mc::add_rr(Size::Q, R10, rcx));
            } else {
                put(cx, mc::mul_r(Size::Q, R11));
                ld(cx, R10, r, k);
                put(cx, mc::add_rr(Size::Q, R10, rax));
                put(cx, mc::adc_ri(Size::Q, rdx, 0));
                put(cx, mc::add_rr(Size::Q, R10, rcx));
                put(cx, mc::adc_ri(Size::Q, rdx, 0));
                put(cx, mc::mov_rr(Size::Q, rcx, rdx));
            }
            st(cx, R10, r, k);
        }
    }
}

/// The amount of a constant shift operand: its low 64 bits, or
/// `u64::MAX` when a higher limb is set (poison either way).
fn const_amount(cx: &FnCx<'_, X64>, value: Value) -> Option<u64> {
    let ValueKind::Const(id) = value.kind() else { return None };
    match cx.module.consts.get(id) {
        Const::Int { value, .. } => Some(value),
        Const::WideInt { words, .. } => {
            let low = words.first().copied().unwrap_or(0);
            Some(if words.iter().skip(1).any(|&w| w != 0) { u64::MAX } else { low })
        }
        Const::Zero(_) | Const::Undef(_) | Const::Poison(_) => Some(0),
        _ => None,
    }
}

/// A shift by a constant `k` below the width: limb `i` of the result is
/// made of source limbs `i ∓ k/64` and their neighbour, by `shld`/`shrd`.
fn shift_const(cx: &mut FnCx<'_, X64>, op: Opcode, bits: u32, r: FrameRef, a: FrameRef, k: u32) {
    let n = limbs(bits);
    let (q, s) = (k / 64, (k % 64) as u8);
    if op == Opcode::Shl {
        for i in 0..n {
            if i < q {
                st_zero(cx, r, i);
                continue;
            }
            ld(cx, R10, a, i - q);
            if s != 0 {
                if i == q {
                    put(cx, mc::shl_ri(Size::Q, R10, s));
                } else {
                    ld(cx, R11, a, i - q - 1);
                    put(cx, mc::shld_rri(Size::Q, R10, R11, s));
                }
            }
            st(cx, R10, r, i);
        }
        return;
    }
    let signed = op == Opcode::AShr;
    for i in 0..n {
        let src = i + q;
        ld_ext(cx, R10, a, bits, src, signed);
        if s != 0 {
            if src + 1 < n {
                ld_ext(cx, R11, a, bits, src + 1, signed);
                put(cx, mc::shrd_rri(Size::Q, R10, R11, s));
            } else if signed {
                put(cx, mc::sar_ri(Size::Q, R10, s));
            } else {
                put(cx, mc::shr_ri(Size::Q, R10, s));
            }
        }
        st(cx, R10, r, i);
    }
}

/// A shift by a variable amount. The source is copied into a frame area
/// of `2n` limbs next to `n` limbs of fill (zero, or the sign for
/// `ashr`), so that every result limb is two loads at a fixed distance
/// from `p` (in `rax`), the area's address moved by `q = min(amount / 64,
/// n - 1)` limbs, combined by `shld`/`shrd` by `cl` (which takes the
/// amount mod 64, and leaves the first operand alone for 0). An amount
/// of the width or more is poison; the clamp keeps every access inside
/// the area.
fn shift_var(
    cx: &mut FnCx<'_, X64>,
    op: Opcode,
    bits: u32,
    r: FrameRef,
    a: FrameRef,
    b: FrameRef,
) -> CgResult<()> {
    let n = limbs(bits);
    let area = cx.alloc_slot(16 * u64::from(n), 16)?;
    let (rax, rcx, rdx) = (Reg::Rax, Reg::Rcx, Reg::Rdx);
    let shl = op == Opcode::Shl;
    let signed = op == Opcode::AShr;
    // The source and the fill: `shl` puts the zeros below the source.
    let (src_at, fill_at) = if shl { (n, 0) } else { (0, n) };
    for limb in 0..n {
        if shl {
            ld(cx, R10, a, limb);
        } else {
            ld_ext(cx, R10, a, bits, limb, signed);
        }
        st(cx, R10, area, src_at + limb);
    }
    if signed {
        // `r10` holds the extended top limb.
        put(cx, mc::sar_ri(Size::Q, R10, 63));
        for limb in 0..n {
            st(cx, R10, area, fill_at + limb);
        }
    } else {
        for limb in 0..n {
            st_zero(cx, area, fill_at + limb);
        }
    }
    ld(cx, rcx, b, 0);
    put(cx, mc::mov_rr(Size::Q, rdx, rcx));
    put(cx, mc::shr_ri(Size::Q, rdx, 6));
    put(cx, mc::mov_ri(Size::D, R10, i64::from(n - 1)));
    put(cx, mc::cmp_rr(Size::Q, rdx, R10));
    put(cx, mc::cmovcc_rr(Cond::A, Size::Q, rdx, R10));
    frame_address(cx, rax, at(area, if shl { 8 * n } else { 0 }));
    if shl {
        put(cx, mc::neg_r(Size::Q, rdx));
    }
    if let Some(index) = rdx.index() {
        put(cx, mc::lea(Size::Q, rax, Mem::base_index(rax, index, Scale::S8, 0)));
    }
    for i in 0..n {
        let off = (8 * i) as i32;
        put(cx, mc::mov_rm(Size::Q, R10, Mem::base(rax, off)));
        put(cx, mc::mov_rm(Size::Q, R11, Mem::base(rax, if shl { off - 8 } else { off + 8 })));
        put(
            cx,
            if shl { mc::shld_rrcl(Size::Q, R10, R11) } else { mc::shrd_rrcl(Size::Q, R10, R11) },
        );
        st(cx, R10, r, i);
    }
    Ok(())
}

/// Copies the integer of `bits` (65 to 128) bits at `slot` into a new
/// 16-byte slot, extended to 128 bits; `slot` itself at 128.
fn widen_to_128(
    cx: &mut FnCx<'_, X64>,
    slot: FrameRef,
    bits: u32,
    signed: bool,
) -> CgResult<FrameRef> {
    if bits == 128 {
        return Ok(slot);
    }
    let wide = cx.alloc_slot(16, 16)?;
    ld(cx, R10, slot, 0);
    st(cx, R10, wide, 0);
    ld_ext(cx, R10, slot, bits, 1, signed);
    st(cx, R10, wide, 1);
    Ok(wide)
}

/// A division or remainder of at most 128 bits: a call of `__udivti3`,
/// `__divti3`, `__umodti3` or `__modti3` (`int_divide_routine`), with
/// operands narrower than 128 bits extended into slots of their own
/// first.
fn divide<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    op: Opcode,
    bits: u32,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    let signed = matches!(op, Opcode::SDiv | Opcode::SRem);
    let Some(func) = int_divide_routine(op) else {
        return cx.bug("a division call of another opcode");
    };
    let mut args = [CallArg::Mem(FrameRef::Local(0)); 2];
    for (arg, value) in args.iter_mut().zip([lhs, rhs]) {
        let slot = cx.use_mem(value)?;
        *arg = CallArg::Mem(widen_to_128(cx, slot, bits, signed)?);
    }
    let r = cx.def_mem(inst)?;
    cx.call_runtime(func, &args, Some(r))?;
    Ok(())
}

// ---- freeze, select ----

/// `%r = freeze %arg` of class `Mem`: a copy (§4.7); `undef` and
/// `poison` are zero.
///
/// # Errors
/// What the operand reports.
pub fn freeze<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    let r = cx.def_mem(inst)?;
    copy_value(cx, r, arg)
}

/// Writes `value` (class `Mem`) into slot `r`: a constant directly, a
/// value by copying its store size.
fn copy_value(cx: &mut FnCx<'_, X64>, r: FrameRef, value: Value) -> CgResult<()> {
    if matches!(value.kind(), ValueKind::Const(_)) {
        return materialize_mem(cx, r, value);
    }
    let bytes = store_size(cx, value_ty(cx, value))?;
    let src = cx.use_mem(value)?;
    if src != r {
        copy_place(cx, Place::Frame(r), Place::Frame(src), bytes);
    }
    Ok(())
}

/// `%r = select i1 %cond, %if_true, %if_false` of class `Mem`: the
/// address of the chosen operand by `cmovz`, then one copy.
///
/// # Errors
/// `x64:todo:lanes` for a vector condition; what the operands report.
pub fn select<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    cond: Value,
    if_true: Value,
    if_false: Value,
) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    if cx.types().is_vector(value_ty(cx, cond)) {
        return unsupported(crate::lanes::LANES);
    }
    let r = cx.def_mem(inst)?;
    if let ValueKind::Const(id) = cond.kind() {
        let chosen = match cx.module.consts.get(id) {
            Const::Int { value, .. } if value & 1 == 1 => if_true,
            _ => if_false,
        };
        return copy_value(cx, r, chosen);
    }
    let c = gp(cx.use_reg(cond)?);
    let bytes = store_size(cx, inst.ty())?;
    let a = cx.use_mem(if_true)?;
    let b = cx.use_mem(if_false)?;
    frame_address(cx, Reg::Rax, a);
    frame_address(cx, Reg::Rcx, b);
    put(cx, mc::test_ri(Size::B, c, 1));
    put(cx, mc::cmovcc_rr(Cond::E, Size::Q, Reg::Rax, Reg::Rcx));
    copy_place(cx, Place::Frame(r), Place::Ptr(Reg::Rax), bytes);
    Ok(())
}

// ---- icmp ----

/// `%r = icmp <pred> %lhs, %rhs` of wide integers: `eq`/`ne` by or-ing
/// the limbs' differences, the orderings by a `cmp`/`sbb` chain whose
/// flags are those of the whole numbers. The top limbs are shifted left
/// by `64 - tb` first (before the chain), so that the bits above the
/// width drop out and the sign is bit 63; the borrow into them is still
/// right.
///
/// # Errors
/// `x64:todo:lanes` for vector operands; what the operands report.
pub fn icmp<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    pred: IntPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    let Some(bits) = wide_int(cx, value_ty(cx, lhs)) else {
        return unsupported(crate::lanes::LANES);
    };
    let a = cx.use_mem(lhs)?;
    let b = cx.use_mem(rhs)?;
    let rd = cx.def_reg(inst)?;
    let cond = compare(cx, pred, bits, a, b);
    let d = gp(rd);
    put(cx, mc::setcc_r(cond, d));
    put(cx, mc::movzx_rr(Size::D, d, Size::B, d));
    cx.finish(inst, rd)
}

/// Sets the flags for `pred` on the integers of `bits` bits at `a` and
/// `b` and returns the condition that holds when it does. Writes `rax`,
/// `rcx`, `rdx`, `r10`, `r11`.
fn compare(
    cx: &mut FnCx<'_, X64>,
    pred: IntPredicate,
    bits: u32,
    a: FrameRef,
    b: FrameRef,
) -> Cond {
    use IntPredicate as P;
    let n = limbs(bits);
    let tb = top_bits(bits);
    let (rax, rcx, rdx) = (Reg::Rax, Reg::Rcx, Reg::Rdx);
    if matches!(pred, P::Eq | P::Ne) {
        for limb in 0..n {
            ld(cx, R10, a, limb);
            ld(cx, R11, b, limb);
            put(cx, mc::xor_rr(Size::Q, R10, R11));
            if limb + 1 == n && tb < 64 {
                put(cx, mc::shl_ri(Size::Q, R10, (64 - tb) as u8));
            }
            if limb == 0 {
                put(cx, mc::mov_rr(Size::Q, rax, R10));
            } else {
                put(cx, mc::or_rr(Size::Q, rax, R10));
            }
        }
        put(cx, mc::test_rr(Size::Q, rax, rax));
        return if pred == P::Eq { Cond::E } else { Cond::Ne };
    }
    let (first, second, cond) = match pred {
        P::Ult => (a, b, Cond::B),
        P::Uge => (a, b, Cond::Ae),
        P::Ugt => (b, a, Cond::B),
        P::Ule => (b, a, Cond::Ae),
        P::Slt => (a, b, Cond::L),
        P::Sge => (a, b, Cond::Ge),
        P::Sgt => (b, a, Cond::L),
        _ => (b, a, Cond::Ge),
    };
    // The top limbs, shifted, before the chain.
    ld(cx, rcx, first, n - 1);
    ld(cx, rdx, second, n - 1);
    if tb < 64 {
        put(cx, mc::shl_ri(Size::Q, rcx, (64 - tb) as u8));
        put(cx, mc::shl_ri(Size::Q, rdx, (64 - tb) as u8));
    }
    for limb in 0..n - 1 {
        ld(cx, R10, first, limb);
        ld(cx, R11, second, limb);
        put(
            cx,
            if limb == 0 { mc::cmp_rr(Size::Q, R10, R11) } else { mc::sbb_rr(Size::Q, R10, R11) },
        );
    }
    put(cx, mc::sbb_rr(Size::Q, rcx, rdx));
    cond
}

// ---- casts ----

/// `%r = <cast> %arg` with a side of class `Mem` that is not a vector's
/// element-wise cast nor an `x86_fp80`'s (those go to `lanes` and `x87`).
///
/// # Errors
/// [`WIDE_FP_CAST`] for a conversion between floating point and an
/// integer wider than 128 bits or `bfloat`; [`CAST`] for
/// `addrspacecast` and what no rule covers; what the operand and the
/// runtime call report.
pub fn cast<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    let from = value_ty(cx, arg);
    let to = inst.ty();
    let vector = cx.types().is_vector(from) || cx.types().is_vector(to);
    match inst.opcode() {
        Opcode::BitCast => bitcast(cx, inst, arg, from, to),
        _ if vector => unsupported(crate::lanes::LANES),
        Opcode::ZExt | Opcode::SExt | Opcode::Trunc => {
            let (Some(fb), Some(tb)) = (cx.types().int_bits(from), cx.types().int_bits(to)) else {
                return unsupported(CAST);
            };
            if inst.opcode() == Opcode::Trunc {
                truncate(cx, inst, arg, tb)
            } else {
                widen(cx, inst, arg, fb, tb, inst.opcode() == Opcode::SExt)
            }
        }
        Opcode::PtrToInt | Opcode::PtrToAddr => {
            // A pointer is 64 bits: zero-extended.
            let tb = cx.types().int_bits(to).unwrap_or(64);
            widen(cx, inst, arg, 64, tb, false)
        }
        Opcode::IntToPtr => truncate(cx, inst, arg, 64),
        Opcode::FPToUI | Opcode::FPToSI | Opcode::UIToFP | Opcode::SIToFP => {
            fp_int_cast(cx, inst, arg)
        }
        _ => unsupported(CAST),
    }
}

/// `zext`/`sext` (and `ptrtoint`) to a wide integer: the source's limbs,
/// its top one extended, then the fill.
fn widen<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    arg: Value,
    from_bits: u32,
    to_bits: u32,
    signed: bool,
) -> CgResult<()> {
    let (r, m) = if cx.class_of(arg) == Class::Gpr {
        let reg = gp(cx.use_reg(arg)?);
        let r = cx.def_mem(inst)?;
        put(cx, mc::mov_rr(Size::Q, R10, reg));
        extend_reg(cx, R10, from_bits, signed);
        st(cx, R10, r, 0);
        (r, 1)
    } else {
        let a = cx.use_mem(arg)?;
        let r = cx.def_mem(inst)?;
        let m = limbs(from_bits);
        for limb in 0..m {
            ld_ext(cx, R10, a, from_bits, limb, signed);
            st(cx, R10, r, limb);
        }
        (r, m)
    };
    let n = limbs(to_bits);
    if m < n {
        // `r10` holds the extended top limb of the source.
        if signed {
            put(cx, mc::sar_ri(Size::Q, R10, 63));
            for limb in m..n {
                st(cx, R10, r, limb);
            }
        } else {
            for limb in m..n {
                st_zero(cx, r, limb);
            }
        }
    }
    Ok(())
}

/// `trunc` (and `inttoptr`) from a wide integer: its low limbs; the bits
/// above the new width stay unspecified.
fn truncate<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, arg: Value, to_bits: u32) -> CgResult<()> {
    let a = cx.use_mem(arg)?;
    if to_bits <= 64 {
        let rd = cx.def_reg(inst)?;
        ld(cx, gp(rd), a, 0);
        return cx.finish(inst, rd);
    }
    let r = cx.def_mem(inst)?;
    for limb in 0..limbs(to_bits) {
        ld(cx, R10, a, limb);
        st(cx, R10, r, limb);
    }
    Ok(())
}

/// `bitcast` with a side of class `Mem`: the same image, moved between a
/// register and a slot or between slots. An integer is masked to its
/// width on its way into another type's image.
fn bitcast<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    arg: Value,
    from: TypeId,
    to: TypeId,
) -> CgResult<()> {
    let bytes = store_size(cx, from)?;
    match (cx.class_of(arg), X64.class_of(cx.types(), to)) {
        (Class::Mem, Class::Mem) => {
            let a = cx.use_mem(arg)?;
            let r = cx.def_mem(inst)?;
            write_image(cx, from, a, Place::Frame(r))
        }
        (Class::Gpr, Class::Mem) => {
            let reg = gp(cx.use_reg(arg)?);
            let r = cx.def_mem(inst)?;
            let bits = cx.types().int_bits(from).unwrap_or(64);
            put(cx, mc::mov_rr(Size::Q, R10, reg));
            extend_reg(cx, R10, bits, false);
            store_tail(cx, R10, Place::Frame(r), 0, bytes);
            Ok(())
        }
        (Class::Fpr, Class::Mem) => {
            let reg = xm(cx.use_reg(arg)?);
            let r = cx.def_mem(inst)?;
            store_fpr(cx, reg, r, bytes);
            Ok(())
        }
        (Class::Mem, Class::Gpr) => {
            let a = cx.use_mem(arg)?;
            let rd = cx.def_reg(inst)?;
            load_gpr(cx, gp(rd), a, bytes);
            cx.finish(inst, rd)
        }
        (Class::Mem, Class::Fpr) => {
            let a = cx.use_mem(arg)?;
            let rd = cx.def_reg(inst)?;
            load_fpr(cx, xm(rd), a, bytes);
            cx.finish(inst, rd)
        }
        _ => cx.bug("a bitcast without a Mem side reached aggr"),
    }
}

/// `fptosi`, `fptoui`, `sitofp`, `uitofp` between a floating-point type
/// and an integer of 65 to 128 bits: the calls `llc` makes
/// (`fp_to_int_routine`, `int_to_fp_routine`: `__fix[uns]{hf,sf,df,tf}ti`,
/// `__float[un]ti{hf,sf,df,tf}`). An operand narrower than 128 bits is
/// extended into a 16-byte slot first.
fn fp_int_cast<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let op = inst.opcode();
    let to_int = matches!(op, Opcode::FPToSI | Opcode::FPToUI);
    let signed = matches!(op, Opcode::FPToSI | Opcode::SIToFP);
    let (int_ty, fp_ty) =
        if to_int { (inst.ty(), value_ty(cx, arg)) } else { (value_ty(cx, arg), inst.ty()) };
    let bits = match cx.types().int_bits(int_ty) {
        Some(bits) if (65..=128).contains(&bits) => bits,
        _ => return unsupported(WIDE_FP_CAST),
    };
    let fp = match cx.types().kind(fp_ty) {
        TypeKind::BFloat => return unsupported(WIDE_FP_CAST),
        kind => FpType::of(kind),
    };
    let Some(fp) = fp else { return unsupported(WIDE_FP_CAST) };
    if to_int {
        let Some(func) = fp_to_int_routine(fp, signed, bits, Arch::X86_64) else {
            return unsupported(WIDE_FP_CAST);
        };
        // The bits of a narrower result above its width are unspecified:
        // the 128-bit result is its home as it is.
        let r = cx.def_mem(inst)?;
        cx.call_runtime(func, &[CallArg::Value(arg)], Some(r))?;
        return Ok(());
    }
    let Some(func) = int_to_fp_routine(signed, bits, fp, Arch::X86_64) else {
        return unsupported(WIDE_FP_CAST);
    };
    let slot = cx.use_mem(arg)?;
    let src = widen_to_128(cx, slot, bits, signed)?;
    let Some(reg) = cx.call_runtime(func, &[CallArg::Mem(src)], None)? else {
        return cx.bug("a runtime function returned nothing");
    };
    cx.finish(inst, reg)
}

// ---- memory ----

/// `%r = load <ty>, ptr %ptr` of class `Mem`: exactly the store size,
/// copied into the result's slot.
///
/// # Errors
/// `x64:todo:atomic` for an atomic load; what the operand reports.
pub fn load<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, ptr: Value, mem: MemFlags) -> CgResult<()> {
    if mem.ordering().is_some() {
        return unsupported(crate::atomic::ATOMIC);
    }
    deferred_first(cx, inst, None)?;
    let bytes = store_size(cx, inst.ty())?;
    let from = pointer(cx, ptr)?;
    let r = cx.def_mem(inst)?;
    copy_place(cx, Place::Frame(r), from, bytes);
    Ok(())
}

/// `store <ty> %value, ptr %ptr` of class `Mem`: exactly the store size,
/// an integer with the bits above its width zero.
///
/// # Errors
/// `x64:todo:atomic` for an atomic store; what the operands report.
pub fn store<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    value: Value,
    ptr: Value,
    mem: MemFlags,
) -> CgResult<()> {
    if mem.ordering().is_some() {
        return unsupported(crate::atomic::ATOMIC);
    }
    deferred_first(cx, inst, None)?;
    let ty = value_ty(cx, value);
    if is_zero_const(cx, value) {
        let bytes = store_size(cx, ty)?;
        let to = pointer(cx, ptr)?;
        zero_place(cx, to, bytes);
        return Ok(());
    }
    // A constant is written to a slot (through `rax` and `r11`) before
    // the pointer may be put in `rax`.
    let src = cx.use_mem(value)?;
    let to = pointer(cx, ptr)?;
    write_image(cx, ty, src, to)
}

// ---- aggregates ----

/// The byte offset of a field, as a frame offset.
fn field_offset(offset: u64) -> CgResult<u32> {
    match u32::try_from(offset) {
        Ok(off) if off <= crate::frame::MAX_X64_FRAME => Ok(off),
        _ => Err(CgError::unsupported(hod_cg::LIMIT_FRAME_SIZE)),
    }
}

/// `%r = extractvalue %agg, <offset>` (the field at byte `offset`).
///
/// # Errors
/// What the operand reports.
pub fn extract_value<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    agg: Value,
    offset: u64,
) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    let offset = field_offset(offset)?;
    let a = at(cx.use_mem(agg)?, offset);
    let ty = inst.ty();
    let bytes = store_size(cx, ty)?;
    match X64.class_of(cx.types(), ty) {
        Class::Gpr => {
            let rd = cx.def_reg(inst)?;
            load_gpr(cx, gp(rd), a, bytes);
            cx.finish(inst, rd)
        }
        Class::Fpr => {
            let rd = cx.def_reg(inst)?;
            load_fpr(cx, xm(rd), a, bytes);
            cx.finish(inst, rd)
        }
        Class::Mem => {
            let r = cx.def_mem(inst)?;
            copy_place(cx, Place::Frame(r), Place::Frame(a), bytes);
            Ok(())
        }
    }
}

/// `%r = insertvalue %agg, %value, <offset>`: the aggregate copied (a
/// constant one written in place), then the field written with exactly
/// its store size, an integer masked to its width.
///
/// # Errors
/// What the operands report.
pub fn insert_value<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    agg: Value,
    value: Value,
    offset: u64,
) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    let offset = field_offset(offset)?;
    let ty = value_ty(cx, value);
    let bytes = store_size(cx, ty)?;
    match cx.class_of(value) {
        Class::Gpr => {
            let src = if is_zero_const(cx, value) { None } else { Some(gp(cx.use_reg(value)?)) };
            let r = cx.def_mem(inst)?;
            copy_value(cx, r, agg)?;
            let to = Place::Frame(at(r, offset));
            match src {
                None => zero_place(cx, to, bytes),
                Some(reg) => {
                    let bits = cx.types().int_bits(ty).unwrap_or(64);
                    put(cx, mc::mov_rr(Size::Q, R10, reg));
                    extend_reg(cx, R10, bits, false);
                    store_tail(cx, R10, to, 0, bytes);
                }
            }
        }
        Class::Fpr => {
            let reg = xm(cx.use_reg(value)?);
            let r = cx.def_mem(inst)?;
            copy_value(cx, r, agg)?;
            store_fpr(cx, reg, at(r, offset), bytes);
        }
        Class::Mem => {
            let r = cx.def_mem(inst)?;
            copy_value(cx, r, agg)?;
            let to = Place::Frame(at(r, offset));
            if is_zero_const(cx, value) {
                zero_place(cx, to, bytes);
            } else {
                let src = cx.use_mem(value)?;
                write_image(cx, ty, src, to)?;
            }
        }
    }
    Ok(())
}

// ---- overflow intrinsics ----

/// `llvm.{s,u}{add,sub,mul}.with.overflow`: the result `{iN, i1}` is
/// written into its slot, the integer masked to its width (an aggregate
/// holds its image) and the flag as a byte.
///
/// # Errors
/// [`WIDE_MUL_OVERFLOW`] for a multiplication wider than half of
/// [`MAX_QUADRATIC_BITS`]; [`OVERFLOW`] for a vector; what the operands
/// report.
pub fn overflow_intrinsic<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    let &[lhs, rhs] = args else {
        return cx.bug("an overflow intrinsic without two operands");
    };
    let Some(bits) = cx.types().int_bits(value_ty(cx, lhs)) else {
        return unsupported(OVERFLOW);
    };
    let flag_at = field_offset(cx.types().field_offset(inst.ty(), 1))?;
    let signed = matches!(
        which,
        Intrinsic::SaddWithOverflow | Intrinsic::SsubWithOverflow | Intrinsic::SmulWithOverflow
    );
    if bits > 64 {
        if matches!(which, Intrinsic::SmulWithOverflow | Intrinsic::UmulWithOverflow) {
            return mul_overflow_wide(cx, inst, signed, bits, [lhs, rhs], flag_at);
        }
        return overflow_wide(cx, inst, which, bits, lhs, rhs, flag_at);
    }
    let ra = gp(cx.use_reg(lhs)?);
    let rb = gp(cx.use_reg(rhs)?);
    let r = cx.def_mem(inst)?;
    let (rax, rcx, rdx) = (Reg::Rax, Reg::Rcx, Reg::Rdx);
    put(cx, mc::mov_rr(Size::Q, R10, ra));
    put(cx, mc::mov_rr(Size::Q, R11, rb));
    extend_reg(cx, R10, bits, signed);
    extend_reg(cx, R11, bits, signed);
    // The result's low `bits` bits end in `r10`, the flag in `cl`.
    match which {
        Intrinsic::UaddWithOverflow if bits == 64 => {
            put(cx, mc::add_rr(Size::Q, R10, R11));
            put(cx, mc::setcc_r(Cond::B, rcx));
        }
        Intrinsic::UaddWithOverflow => {
            // The sum of two numbers below 2^bits is below 2^64.
            put(cx, mc::add_rr(Size::Q, R10, R11));
            above_width(cx, R10, bits);
        }
        Intrinsic::UsubWithOverflow => {
            put(cx, mc::sub_rr(Size::Q, R10, R11));
            put(cx, mc::setcc_r(Cond::B, rcx));
        }
        Intrinsic::SaddWithOverflow | Intrinsic::SsubWithOverflow => {
            let word = if which == Intrinsic::SaddWithOverflow {
                mc::add_rr(Size::Q, R10, R11)
            } else {
                mc::sub_rr(Size::Q, R10, R11)
            };
            put(cx, word);
            if bits == 64 {
                put(cx, mc::setcc_r(Cond::O, rcx));
            } else {
                not_own_extension(cx, R10, bits, rcx);
            }
        }
        Intrinsic::UmulWithOverflow if bits <= 32 => {
            put(cx, mc::imul_rr(Size::Q, R10, R11));
            above_width(cx, R10, bits);
        }
        Intrinsic::UmulWithOverflow => {
            put(cx, mc::mov_rr(Size::Q, rax, R10));
            put(cx, mc::mul_r(Size::Q, R11));
            put(cx, mc::mov_rr(Size::Q, R10, rax));
            if bits < 64 {
                put(cx, mc::shr_ri(Size::Q, rax, bits as u8));
                put(cx, mc::or_rr(Size::Q, rdx, rax));
            } else {
                put(cx, mc::test_rr(Size::Q, rdx, rdx));
            }
            put(cx, mc::setcc_r(Cond::Ne, rcx));
        }
        Intrinsic::SmulWithOverflow if bits <= 32 => {
            put(cx, mc::imul_rr(Size::Q, R10, R11));
            not_own_extension(cx, R10, bits, rcx);
        }
        Intrinsic::SmulWithOverflow => {
            put(cx, mc::mov_rr(Size::Q, rax, R10));
            put(cx, mc::imul_r(Size::Q, R11));
            put(cx, mc::mov_rr(Size::Q, R10, rax));
            if bits == 64 {
                put(cx, mc::setcc_r(Cond::O, rcx));
            } else {
                // The 128-bit product `rdx:rax` fits iff its high half
                // is the sign of its low half and the low half is the low
                // `bits` bits sign-extended.
                put(cx, mc::sar_ri(Size::Q, rax, 63));
                put(cx, mc::cmp_rr(Size::Q, rdx, rax));
                put(cx, mc::setcc_r(Cond::Ne, rdx));
                not_own_extension(cx, R10, bits, rcx);
                put(cx, mc::or_rr(Size::B, rcx, rdx));
            }
        }
        _ => return cx.bug("an intrinsic that is not an overflow intrinsic reached aggr"),
    }
    extend_reg(cx, R10, bits, false);
    store_tail(cx, R10, Place::Frame(r), 0, bits.div_ceil(8));
    frame_inst(cx, at(r, flag_at), |m| mc::mov_mr(Size::B, m, rcx));
    Ok(())
}

/// `cl = (r >> bits) != 0`, `r` unchanged (`bits` < 64).
fn above_width(cx: &mut FnCx<'_, X64>, r: Reg, bits: u32) {
    put(cx, mc::mov_rr(Size::Q, Reg::Rax, r));
    put(cx, mc::shr_ri(Size::Q, Reg::Rax, bits as u8));
    put(cx, mc::setcc_r(Cond::Ne, Reg::Rcx));
}

/// `flag = r != sext(r from bits)` as a byte, `r` unchanged (`bits` <
/// 64); writes `rax`.
fn not_own_extension(cx: &mut FnCx<'_, X64>, r: Reg, bits: u32, flag: Reg) {
    put(cx, mc::mov_rr(Size::Q, Reg::Rax, r));
    extend_reg(cx, Reg::Rax, bits, true);
    put(cx, mc::cmp_rr(Size::Q, Reg::Rax, r));
    put(cx, mc::setcc_r(Cond::Ne, flag));
}

/// `llvm.{s,u}{add,sub}.with.overflow` of a wide integer: a carry chain
/// over the operands with their top limbs extended (into `rcx`/`rdx`
/// before the chain); the unsigned flag is the carry out (or the bits
/// above the width), the signed one `OF` (or the top limb not being its
/// own extension).
fn overflow_wide<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    bits: u32,
    lhs: Value,
    rhs: Value,
    flag_at: u32,
) -> CgResult<()> {
    let a = cx.use_mem(lhs)?;
    let b = cx.use_mem(rhs)?;
    let r = cx.def_mem(inst)?;
    let n = limbs(bits);
    let tb = top_bits(bits);
    let signed = matches!(which, Intrinsic::SaddWithOverflow | Intrinsic::SsubWithOverflow);
    let add = matches!(which, Intrinsic::SaddWithOverflow | Intrinsic::UaddWithOverflow);
    let (rcx, rdx) = (Reg::Rcx, Reg::Rdx);
    ld_ext(cx, rcx, a, bits, n - 1, signed);
    ld_ext(cx, rdx, b, bits, n - 1, signed);
    for limb in 0..n - 1 {
        ld(cx, R10, a, limb);
        ld(cx, R11, b, limb);
        let word = match (add, limb == 0) {
            (true, true) => mc::add_rr(Size::Q, R10, R11),
            (true, false) => mc::adc_rr(Size::Q, R10, R11),
            (false, true) => mc::sub_rr(Size::Q, R10, R11),
            (false, false) => mc::sbb_rr(Size::Q, R10, R11),
        };
        put(cx, word);
        st(cx, R10, r, limb);
    }
    put(cx, if add { mc::adc_rr(Size::Q, rcx, rdx) } else { mc::sbb_rr(Size::Q, rcx, rdx) });
    // `rcx` is the top limb of the result, the flags are the chain's;
    // the flag goes to `dl`.
    match (signed, add, tb == 64) {
        (false, true, true) | (false, false, _) => put(cx, mc::setcc_r(Cond::B, rdx)),
        (false, true, false) => {
            put(cx, mc::mov_rr(Size::Q, Reg::Rax, rcx));
            put(cx, mc::shr_ri(Size::Q, Reg::Rax, tb as u8));
            put(cx, mc::setcc_r(Cond::Ne, rdx));
        }
        (true, _, true) => put(cx, mc::setcc_r(Cond::O, rdx)),
        (true, _, false) => not_own_extension(cx, rcx, tb, rdx),
    }
    extend_reg(cx, rcx, tb, false);
    st(cx, rcx, r, n - 1);
    frame_inst(cx, at(r, flag_at), |m| mc::mov_mr(Size::B, m, rdx));
    Ok(())
}

/// `llvm.{s,u}mul.with.overflow` of `bits` > 64 bits: the product of the
/// operands extended to `2 * bits` is exact; the unsigned flag is any bit
/// of it at or above `bits`, the signed one any such bit that differs
/// from bit `bits - 1`.
fn mul_overflow_wide<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    signed: bool,
    bits: u32,
    [lhs, rhs]: [Value; 2],
    flag_at: u32,
) -> CgResult<()> {
    if bits > MAX_QUADRATIC_BITS / 2 {
        return unsupported(WIDE_MUL_OVERFLOW);
    }
    let wide = 2 * bits;
    let n = limbs(wide);
    let a = cx.use_mem(lhs)?;
    let b = cx.use_mem(rhs)?;
    let r = cx.def_mem(inst)?;
    let ea = cx.alloc_slot(u64::from(8 * n), 16)?;
    let eb = cx.alloc_slot(u64::from(8 * n), 16)?;
    let p = cx.alloc_slot(u64::from(8 * n), 16)?;
    for limb in 0..n {
        ld_ext(cx, R10, a, bits, limb, signed);
        st(cx, R10, ea, limb);
        ld_ext(cx, R10, b, bits, limb, signed);
        st(cx, R10, eb, limb);
    }
    multiply(cx, wide, p, ea, eb);
    let rcx = Reg::Rcx;
    let (lb, off) = (bits / 64, (bits % 64) as u8);
    ld(cx, R10, p, lb);
    if signed {
        // `rcx` = bit `bits - 1` everywhere.
        ld(cx, rcx, p, (bits - 1) / 64);
        put(cx, mc::shl_ri(Size::Q, rcx, (63 - (bits - 1) % 64) as u8));
        put(cx, mc::sar_ri(Size::Q, rcx, 63));
        if off != 0 {
            put(cx, mc::sar_ri(Size::Q, R10, off));
        }
        put(cx, mc::xor_rr(Size::Q, R10, rcx));
        for k in lb + 1..n {
            ld(cx, R11, p, k);
            put(cx, mc::xor_rr(Size::Q, R11, rcx));
            put(cx, mc::or_rr(Size::Q, R10, R11));
        }
    } else {
        if off != 0 {
            put(cx, mc::shr_ri(Size::Q, R10, off));
        }
        for k in lb + 1..n {
            ld(cx, R11, p, k);
            put(cx, mc::or_rr(Size::Q, R10, R11));
        }
    }
    put(cx, mc::test_rr(Size::Q, R10, R10));
    put(cx, mc::setcc_r(Cond::Ne, rcx));
    let m = limbs(bits);
    for limb in 0..m {
        ld(cx, R10, p, limb);
        if limb + 1 == m {
            extend_reg(cx, R10, top_bits(bits), false);
        }
        st(cx, R10, r, limb);
    }
    frame_inst(cx, at(r, flag_at), |m| mc::mov_mr(Size::B, m, rcx));
    Ok(())
}

// ---- constants ----

thread_local! {
    /// The image and relocations of the `Mem`-class constant being
    /// written: one buffer per thread, so a warm worker writes aggregates
    /// without allocating.
    static IMAGE: std::cell::RefCell<(Vec<u8>, Vec<hod_ir::Fixup>)> =
        const { std::cell::RefCell::new((Vec::new(), Vec::new())) };
}

/// Glue (`Target::materialize_mem`): writes the in-memory image of the
/// `Mem`-class constant `value` to `dst`: zero-like constants as
/// immediate stores of zero, wide integers word by word, anything else
/// from its flattened image (`hod_ir::ConstPool::flatten`), whose
/// relocated fields `constaddr::write_image` computes in code. Writes
/// `rax` and `r11` only and keeps the flags (`r10` may hold a value of
/// the glue that called it).
///
/// # Errors
/// `x64:todo:constant` for a constant without an image; what
/// `constaddr::write_image` reports.
pub fn materialize_mem(cx: &mut FnCx<'_, X64>, dst: FrameRef, value: Value) -> CgResult<()> {
    let ValueKind::Const(id) = value.kind() else {
        return cx.bug("materialize_mem of something that is no constant");
    };
    let ty = cx.module.consts.ty(id);
    let bytes = store_size(cx, ty)?;
    match cx.module.consts.get(id) {
        Const::Null(_) | Const::Zero(_) | Const::Undef(_) | Const::Poison(_) => {
            zero_place(cx, Place::Frame(dst), bytes);
        }
        Const::Int { value, .. } => write_words(cx, dst, &[value], bytes),
        Const::WideInt { words, .. } => write_words(cx, dst, words, bytes),
        _ => {
            let (mut image, mut fixups) =
                IMAGE.with(|cell| std::mem::take(&mut *cell.borrow_mut()));
            image.clear();
            fixups.clear();
            let flattened = cx.module.consts.flatten(cx.types(), id, &mut image, &mut fixups);
            let result = match flattened {
                Ok(()) => crate::constaddr::write_image(cx, dst, ty, &image, &fixups),
                Err(_) => unsupported(crate::common::CONSTANT),
            };
            IMAGE.with(|cell| *cell.borrow_mut() = (image, fixups));
            result?;
        }
    }
    Ok(())
}

/// Writes `bytes` bytes of the little-endian words `words` (zero past
/// their end) at `dst`.
fn write_words(cx: &mut FnCx<'_, X64>, dst: FrameRef, words: &[u64], bytes: u32) {
    let mut done = 0;
    let mut index = 0;
    while done < bytes {
        let word = words.get(index).copied().unwrap_or(0);
        let take = (bytes - done).min(8);
        store_word(cx, dst.offset_by(done), word, take);
        done += take;
        index += 1;
    }
}

/// Stores the low `take` (1 to 8) bytes of `word` at `dst`: immediate
/// stores, through `r11` for an 8-byte word that is no sign-extended
/// 32-bit value. Flags kept.
pub fn store_word(cx: &mut FnCx<'_, X64>, dst: FrameRef, word: u64, take: u32) {
    if take == 8 {
        if let Ok(small) = i32::try_from(word as i64) {
            frame_inst(cx, dst, |m| mc::mov_mi(Size::Q, m, i64::from(small)));
        } else {
            put(cx, mc::mov_ri(Size::Q, R11, word as i64));
            frame_inst(cx, dst, |m| mc::mov_mr(Size::Q, m, R11));
        }
        return;
    }
    for (off, size) in pieces(take) {
        let part = (word >> (off * 8)) & ((1u64 << (size * 8)) - 1);
        let s = exact_size(size).unwrap_or(Size::D);
        // A 32-bit immediate is the field's bits as they are.
        let imm = if size == 4 { i64::from(part as u32 as i32) } else { part as i64 };
        frame_inst(cx, dst.offset_by(off), |m| mc::mov_mi(s, m, imm));
    }
}

// ---- a value returned in memory ----

/// `ret` of a function that returns in memory (`term::ret`): the value
/// written to the buffer as a `store` of it would write it (an integer
/// with the bits above its width zero), through the address
/// `call::enter` saved (`CallState::ret_addr`, loaded into `rax` here);
/// the epilogue (`call::before_return`) puts it back in `rax` for the
/// caller (`spec/abi-x64.md` §5.2).
///
/// # Errors
/// `x64:todo:abi` for a value of class `Gpr` or `Fpr` (no assignment
/// returns one in memory); what the value reports.
pub fn ret_memory(cx: &mut FnCx<'_, X64>, value: Value) -> CgResult<()> {
    if cx.class_of(value) != Class::Mem {
        return unsupported(crate::abi::ABI);
    }
    let ty = value_ty(cx, value);
    let src = cx.use_mem(value)?;
    let Some(slot) = cx.state.frame.call.ret_addr else {
        return cx.bug("a return in memory without the buffer's address");
    };
    frame_inst(cx, slot, |m| mc::mov_rm(Size::Q, Reg::Rax, m));
    write_image(cx, ty, src, Place::Ptr(Reg::Rax))
}
