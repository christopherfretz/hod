//! Instructions on values of class `Mem` (`spec/codegen.md` §2.2): wide
//! integers, first-class aggregates and wide vectors, slot to slot —
//! binary operations, `freeze`, casts, `icmp`, `load`, `store`, `select`,
//! `extractvalue`, `insertvalue`, and the `*.with.overflow` intrinsics,
//! whose result is a `{iN, i1}`.
//!
//! **Owner: WP-040.** Tests: `tests/aggr.rs`. The dispatch in `target.rs`
//! routes every instruction whose value is of class `Mem` here, so no
//! other group's file needs a hook. Frame-to-frame copies are
//! `crate::common::copy_bytes`; `Mem`-class constants are written by
//! `crate::common::materialize_mem_const` (WP-033).
//!
//! # What a slot holds (`spec/codegen.md` §5.5.1 "Mem-class values")
//!
//! A `Mem` value is the first *store size* bytes of its slot, laid out as
//! its in-memory image (`spec/ir.md` §4.3); the bytes after them, up to
//! the alloc size, are unspecified.
//!
//! - An integer wider than 64 bits is `n = ⌈N/64⌉` little-endian 64-bit
//!   **limbs**; the alloc size is a multiple of 16, so all `n` limbs are
//!   inside the slot. The bits above `N` in the top limb are
//!   **unspecified**, as in a register (§2.2): the value may have come
//!   from registers (an argument, a call's result). A consumer that
//!   depends on them (a compare, a right shift, a division, `zext`,
//!   `sext`, a `store`, `insertvalue`, an overflow check) extends the top
//!   limb first; `add`, `sub`, `mul`, the bitwise operations and `shl`
//!   leave garbage there.
//! - A struct, an array or a vector holds its image exactly: every
//!   integer field with the bits above its width zero, the padding
//!   unspecified. `insertvalue` and the overflow intrinsics mask what
//!   they write; `load` copies what memory holds, which has them zero.
//!
//! Everything is computed through `x16`/`x17` and at most three pool
//! temporaries; frame offsets out of reach of the immediate forms are
//! rebased into the register being loaded, or into a scratch register for
//! a store (`common::Cursor`), with `add`, which keeps the flags of a carry
//! chain.

#![allow(
    clippy::many_single_char_names,
    reason = "limb arithmetic: r = a op b over limbs, x and y the scratch registers"
)]
#![allow(clippy::too_many_lines, reason = "one match per intrinsic, read top to bottom")]

use hod_cg::{CallArg, CgResult, Class, FnCx, FrameRef, RuntimeFn, SlotPlace, Target};
use hod_ir::{Const, Inst, IntPredicate, Intrinsic, MemFlags, Opcode, TypeId, Value, ValueKind};
use hod_mc_a64 as mc;
use hod_mc_a64::{Cond, Load, MemSize, Reg, RegZr, Shift, Size};

use crate::common::{
    Cursor, X16, X17, copy_bytes, deferred_first, fpr_view, frame_address, gpr_load_op,
    load_fpr_at, load_gpr_at, materialize_mem_const, mov_imm, put, slot_address, store_fpr_bytes,
    store_gpr_at, store_gpr_bytes, unsupported, value_ty, vr, xr, zero_bytes,
};
use crate::target::{A64, ATOMIC};

/// A binary operation on `Mem`-class values (`i128` and wider).
pub const BINARY: &str = "a64:mem-binary";
/// `freeze` of a `Mem`-class value.
pub const FREEZE: &str = "a64:mem-freeze";
/// A cast with a `Mem`-class side.
pub const CAST: &str = "a64:mem-cast";
/// `icmp` of `Mem`-class values.
pub const ICMP: &str = "a64:mem-icmp";
/// `load` of a `Mem`-class value.
pub const LOAD: &str = "a64:mem-load";
/// `store` of a `Mem`-class value.
pub const STORE: &str = "a64:mem-store";
/// `select` of `Mem`-class values.
pub const SELECT: &str = "a64:mem-select";
/// `extractvalue`.
pub const EXTRACT_VALUE: &str = "a64:extractvalue";
/// `insertvalue`.
pub const INSERT_VALUE: &str = "a64:insertvalue";
/// `llvm.{s,u}{add,sub,mul}.with.overflow`.
pub const OVERFLOW: &str = "a64:overflow-intrinsic";
/// `udiv`, `sdiv`, `urem`, `srem` of an integer wider than 128 bits: the
/// runtime has no function for it (`__udivti3` and its kind stop at 128).
pub const WIDE_DIVISION: &str = "a64:wide-division";
/// `llvm.{s,u}mul.with.overflow` of an integer wider than 64 bits.
pub const WIDE_MUL_OVERFLOW: &str = "a64:wide-mul-overflow";
/// An element-wise operation (arithmetic, `icmp`, a cast other than
/// `bitcast`) on a vector of class `Mem`: a vector that is not 64 or 128
/// bits of a power-of-two count, or a vector of `i1`. `load`, `store`,
/// `select`, `freeze`, `phi` and `bitcast` of them are lowered.
pub const MEM_VECTOR: &str = "a64:mem-vector";
/// A conversion between a floating-point type and an integer wider than
/// 64 bits: its runtime functions (`__floattidf`, `__fixdfti`, …) are not
/// in `hod_cg::RuntimeFn`.
pub const WIDE_FP_CAST: &str = "a64:wide-fp-cast";
/// `mul` and variable shifts of integers wider than this are refused:
/// their code grows with the square of the width (`mul`) or would need
/// a frame area of twice it (shifts). `add`, compares and the rest are
/// linear and have no limit.
pub const MAX_QUADRATIC_BITS: u32 = 4096;
/// A `mul` of an integer wider than [`MAX_QUADRATIC_BITS`].
pub const WIDE_MUL: &str = "a64:wide-mul";
/// A shift by a variable amount of an integer wider than
/// [`MAX_QUADRATIC_BITS`].
pub const WIDE_SHIFT: &str = "a64:wide-shift";

// ---- limbs and slots ----

/// The 64-bit limbs of an integer of `bits` bits.
#[inline]
fn limbs(bits: u32) -> u32 {
    bits.div_ceil(64)
}

/// The meaningful bits of the top limb (1 to 64).
#[inline]
fn top_bits(bits: u32) -> u32 {
    bits - 64 * (limbs(bits) - 1)
}

/// `slot` moved `off` bytes on.
#[inline]
fn at(slot: FrameRef, off: u32) -> FrameRef {
    match slot {
        FrameRef::Local(o) => FrameRef::Local(o + off),
        FrameRef::Incoming(o) => FrameRef::Incoming(o + off),
        FrameRef::Outgoing(o) => FrameRef::Outgoing(o + off),
    }
}

/// `ldr rt, [slot + 8 * limb]`; a far offset is rebased into `rt`.
fn ld(cx: &mut FnCx<'_, A64>, rt: Reg, slot: FrameRef, limb: u32) {
    let mut c = Cursor::frame(cx, at(slot, 8 * limb), rt);
    load_gpr_at(cx, Load::X, rt, &mut c);
}

/// `str rt, [slot + 8 * limb]`; a far offset is rebased into `tmp`.
fn st(cx: &mut FnCx<'_, A64>, rt: RegZr, tmp: Reg, slot: FrameRef, limb: u32) {
    let mut c = Cursor::frame(cx, at(slot, 8 * limb), tmp);
    store_gpr_at(cx, MemSize::X, rt, &mut c);
}

/// Extends the top limb in `reg` from `tb` bits, in place (nothing for 64).
fn extend(cx: &mut FnCx<'_, A64>, reg: Reg, tb: u32, signed: bool) {
    if tb < 64 {
        let word = if signed {
            mc::sbfx(Size::X, reg.zr(), reg.zr(), 0, tb)
        } else {
            mc::ubfx(Size::X, reg.zr(), reg.zr(), 0, tb)
        };
        put(cx, word);
    }
}

/// Loads limb `limb` of the integer of `bits` bits at `slot` into `rt`,
/// extended from the width when it is the top limb, and the extension
/// word (0, or the sign) for a limb above it.
fn ld_ext(cx: &mut FnCx<'_, A64>, rt: Reg, slot: FrameRef, bits: u32, limb: u32, signed: bool) {
    let n = limbs(bits);
    if limb + 1 < n {
        ld(cx, rt, slot, limb);
        return;
    }
    ld(cx, rt, slot, n - 1);
    extend(cx, rt, top_bits(bits), signed);
    if limb >= n {
        if signed {
            put(cx, mc::asr_imm(Size::X, rt.zr(), rt.zr(), 63));
        } else {
            put(cx, mc::mov(Size::X, rt.zr(), RegZr::ZR));
        }
    }
}

/// The store size in bytes of `ty`, for a copy (§5.5.1: the bytes after
/// it in a slot are unspecified).
fn store_size(cx: &FnCx<'_, A64>, ty: TypeId) -> CgResult<u32> {
    u32::try_from(cx.types().size_of(ty))
        .map_err(|_| hod_cg::CgError::unsupported(hod_cg::LIMIT_FRAME_SIZE))
}

/// The width of `ty` if it is an integer of class `Mem`.
fn wide_int(cx: &FnCx<'_, A64>, ty: TypeId) -> Option<u32> {
    cx.types().int_bits(ty).filter(|&bits| bits > 64)
}

/// Writes the image of the integer of `bits` bits at `src` at the
/// cursor: exactly its store size, the bits above the width zero
/// (§2.2). Writes `x16` and the cursor's `tmp`, which is not `x16`.
fn write_int_image(cx: &mut FnCx<'_, A64>, src: FrameRef, bits: u32, dst: &mut Cursor) {
    let n = limbs(bits);
    for limb in 0..n - 1 {
        ld(cx, X16, src, limb);
        store_gpr_at(cx, MemSize::X, X16.zr(), dst);
        dst.advance(8);
    }
    ld(cx, X16, src, n - 1);
    extend(cx, X16, top_bits(bits), false);
    store_gpr_bytes(cx, X16, bits.div_ceil(8) - 8 * (n - 1), dst);
    dst.advance(bits.div_ceil(8) - 8 * (n - 1));
}

/// Writes the image of the `Mem` value of type `ty` at `src` at the
/// cursor (whose `tmp` is not `x16`): an integer by
/// [`write_int_image`], anything else by copying its store size.
fn write_image(
    cx: &mut FnCx<'_, A64>,
    ty: TypeId,
    src: FrameRef,
    dst: &mut Cursor,
) -> CgResult<()> {
    if let Some(bits) = wide_int(cx, ty) {
        write_int_image(cx, src, bits, dst);
    } else {
        let bytes = store_size(cx, ty)?;
        let mut from = Cursor::frame(cx, src, X16);
        copy_bytes(cx, dst, &mut from, bytes);
    }
    Ok(())
}

/// Whether `value` is a constant whose image is all zero bytes.
fn is_zero_const(cx: &FnCx<'_, A64>, value: Value) -> bool {
    let ValueKind::Const(id) = value.kind() else { return false };
    cx.module.consts.is_zero_or_undef(id)
}

/// The cursor of a `load` or `store` through `ptr`, with `x17` as its
/// rebase register: the frame itself for a directly placed stack slot,
/// else the pointer in a register (asked for; a symbol or constant goes
/// to a temporary). Writes `x17` at most.
fn pointer(cx: &mut FnCx<'_, A64>, ptr: Value) -> CgResult<Cursor> {
    if let ValueKind::Slot(slot) = ptr.kind() {
        return Ok(match cx.slot_place(slot)? {
            SlotPlace::Direct(place) => Cursor::frame(cx, place, X17),
            SlotPlace::Indirect(_) => {
                slot_address(cx, X17, slot)?;
                Cursor::at_reg(X17, 0, X17)
            }
        });
    }
    let reg = xr(cx.use_reg(ptr)?);
    Ok(Cursor::at_reg(reg, 0, X17))
}

// ---- binary operations ----

/// `%r = <op> %lhs, %rhs` of class `Mem`.
///
/// # Errors
/// [`MEM_VECTOR`] for a vector; [`WIDE_DIVISION`] for a division wider
/// than 128 bits; [`WIDE_MUL`] and [`WIDE_SHIFT`] beyond
/// [`MAX_QUADRATIC_BITS`]; what the
/// operands and the runtime call report.
pub fn binary<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, lhs: Value, rhs: Value) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    let Some(bits) = wide_int(cx, inst.ty()) else {
        if cx.types().is_vector(inst.ty()) {
            return crate::lanes::binary(cx, inst, lhs, rhs);
        }
        return unsupported(BINARY);
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
            multiply(cx, bits, r, a, b)
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
        _ => unsupported(BINARY),
    }
}

/// `add`, `sub` (a carry chain) and the bitwise operations, limb by limb.
fn limbwise(cx: &mut FnCx<'_, A64>, op: Opcode, bits: u32, r: FrameRef, a: FrameRef, b: FrameRef) {
    let n = limbs(bits);
    let (x, y) = (X16.zr(), X17.zr());
    for limb in 0..n {
        ld(cx, X16, a, limb);
        ld(cx, X17, b, limb);
        let (first, last) = (limb == 0, limb + 1 == n);
        let word = match op {
            Opcode::Add if first => mc::adds(Size::X, x, x, y),
            Opcode::Add if last => mc::adc(Size::X, x, x, y),
            Opcode::Add => mc::adcs(Size::X, x, x, y),
            Opcode::Sub if first => mc::subs(Size::X, x, x, y),
            Opcode::Sub if last => mc::sbc(Size::X, x, x, y),
            Opcode::Sub => mc::sbcs(Size::X, x, x, y),
            Opcode::And => mc::and(Size::X, x, x, y),
            Opcode::Or => mc::orr(Size::X, x, x, y),
            _ => mc::eor(Size::X, x, x, y),
        };
        put(cx, word);
        st(cx, x, X17, r, limb);
    }
}

/// `mul` modulo 2^(64n): for two limbs `lo = a0·b0`, `hi = umulh(a0,
/// b0) + a0·b1 + a1·b0`; wider, schoolbook into the zeroed result.
fn multiply(
    cx: &mut FnCx<'_, A64>,
    bits: u32,
    r: FrameRef,
    a: FrameRef,
    b: FrameRef,
) -> CgResult<()> {
    let n = limbs(bits);
    let (x, y) = (X16.zr(), X17.zr());
    if n == 2 {
        let t0 = xr(cx.temp(Class::Gpr)?);
        let t1 = xr(cx.temp(Class::Gpr)?);
        ld(cx, t0, a, 0);
        ld(cx, t1, b, 0);
        ld(cx, X16, a, 1);
        ld(cx, X17, b, 1);
        put(cx, mc::mul(Size::X, y, t0.zr(), y)); // a0·b1
        put(cx, mc::madd(Size::X, x, x, t1.zr(), y)); // a1·b0 + a0·b1
        put(cx, mc::umulh(y, t0.zr(), t1.zr()));
        put(cx, mc::add(Size::X, x, x, y));
        put(cx, mc::mul(Size::X, t0.zr(), t0.zr(), t1.zr()));
        st(cx, t0.zr(), X17, r, 0);
        st(cx, x, X17, r, 1);
        return Ok(());
    }
    let ai = xr(cx.temp(Class::Gpr)?);
    let hi = xr(cx.temp(Class::Gpr)?);
    let carry = xr(cx.temp(Class::Gpr)?);
    for limb in 0..n {
        st(cx, RegZr::ZR, X16, r, limb);
    }
    for i in 0..n {
        ld(cx, ai, a, i);
        put(cx, mc::mov(Size::X, carry.zr(), RegZr::ZR));
        for j in 0..n - i {
            let k = i + j;
            ld(cx, X16, b, j);
            put(cx, mc::mul(Size::X, y, ai.zr(), x));
            if k + 1 == n {
                ld(cx, X16, r, k);
                put(cx, mc::add(Size::X, x, x, y));
                put(cx, mc::add(Size::X, x, x, carry.zr()));
            } else {
                put(cx, mc::umulh(hi.zr(), ai.zr(), x));
                ld(cx, X16, r, k);
                put(cx, mc::adds(Size::X, x, x, y));
                put(cx, mc::adc(Size::X, hi.zr(), hi.zr(), RegZr::ZR));
                put(cx, mc::adds(Size::X, x, x, carry.zr()));
                put(cx, mc::adc(Size::X, carry.zr(), hi.zr(), RegZr::ZR));
            }
            st(cx, x, X17, r, k);
        }
    }
    Ok(())
}

/// The amount of a constant shift operand: its low 64 bits, or
/// `u64::MAX` when a higher limb is set (poison either way).
fn const_amount(cx: &FnCx<'_, A64>, value: Value) -> Option<u64> {
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
/// made of source limbs `i ∓ k/64` and their neighbour, with `extr`.
fn shift_const(cx: &mut FnCx<'_, A64>, op: Opcode, bits: u32, r: FrameRef, a: FrameRef, k: u32) {
    let n = limbs(bits);
    let (q, s) = (k / 64, k % 64);
    let (x, y) = (X16.zr(), X17.zr());
    if op == Opcode::Shl {
        for i in 0..n {
            if i < q {
                st(cx, RegZr::ZR, X17, r, i);
                continue;
            }
            ld(cx, X16, a, i - q);
            if s != 0 {
                if i == q {
                    put(cx, mc::lsl_imm(Size::X, x, x, s));
                } else {
                    ld(cx, X17, a, i - q - 1);
                    put(cx, mc::extr(Size::X, x, x, y, 64 - s));
                }
            }
            st(cx, x, X17, r, i);
        }
        return;
    }
    let signed = op == Opcode::AShr;
    for i in 0..n {
        let src = i + q;
        ld_ext(cx, X16, a, bits, src, signed);
        if s != 0 {
            if src + 1 < n {
                ld_ext(cx, X17, a, bits, src + 1, signed);
                put(cx, mc::extr(Size::X, x, y, x, s));
            } else if signed {
                put(cx, mc::asr_imm(Size::X, x, x, s));
            } else {
                put(cx, mc::lsr_imm(Size::X, x, x, s));
            }
        }
        st(cx, x, X17, r, i);
    }
}

/// A shift by a variable amount. The source is copied into a frame area
/// of `2n` limbs next to `n` limbs of fill (zero, or the sign for
/// `ashr`), so that every result limb is two loads at a fixed distance
/// from `p`, the area's address moved by `q = min(amount / 64, n - 1)`
/// limbs: `(p[i] << s) | (p[i-1] >> 1 >> ~s)` for `shl`, `(p[i] >> s) |
/// (p[i+1] << 1 << ~s)` for the right shifts, `s` taken mod 64 by the
/// register shifts. An amount of the width or more is poison; the clamp
/// keeps every access inside the area.
fn shift_var(
    cx: &mut FnCx<'_, A64>,
    op: Opcode,
    bits: u32,
    r: FrameRef,
    a: FrameRef,
    b: FrameRef,
) -> CgResult<()> {
    let n = limbs(bits);
    let area = cx.alloc_slot(16 * u64::from(n), 16)?;
    let (x, y) = (X16.zr(), X17.zr());
    let signed = op == Opcode::AShr;
    // The source and the fill: `shl` puts the zeros below the source.
    let (src_at, fill_at) = if op == Opcode::Shl { (n, 0) } else { (0, n) };
    for limb in 0..n {
        if op == Opcode::Shl {
            ld(cx, X16, a, limb);
        } else {
            ld_ext(cx, X16, a, bits, limb, signed);
        }
        st(cx, x, X17, area, src_at + limb);
    }
    if signed {
        // `x16` holds the extended top limb.
        put(cx, mc::asr_imm(Size::X, x, x, 63));
    }
    let fill = if signed { x } else { RegZr::ZR };
    for limb in 0..n {
        st(cx, fill, X17, area, fill_at + limb);
    }
    let p = xr(cx.temp(Class::Gpr)?);
    let s = xr(cx.temp(Class::Gpr)?);
    let ns = xr(cx.temp(Class::Gpr)?);
    ld(cx, s, b, 0);
    put(cx, mc::mvn(Size::X, ns.zr(), s.zr()));
    put(cx, mc::lsr_imm(Size::X, y, s.zr(), 6));
    mov_imm(cx, Size::X, X16, u64::from(n - 1));
    put(cx, mc::cmp(Size::X, y, x));
    put(cx, mc::csel(Size::X, y, y, x, Cond::Ls));
    frame_address(cx, p, at(area, if op == Opcode::Shl { 8 * n } else { 0 }));
    let shift = if op == Opcode::Shl { mc::sub_shifted } else { mc::add_shifted };
    put(cx, shift(Size::X, p.zr(), p.zr(), y, Shift::Lsl, 3));
    for i in 0..n {
        let off = i64::from(8 * i);
        let mut c = Cursor::at_reg(p, off, X16);
        load_gpr_at(cx, Load::X, X16, &mut c);
        let mut c = Cursor::at_reg(p, if op == Opcode::Shl { off - 8 } else { off + 8 }, X17);
        load_gpr_at(cx, Load::X, X17, &mut c);
        if op == Opcode::Shl {
            put(cx, mc::lslv(Size::X, x, x, s.zr()));
            put(cx, mc::lsr_imm(Size::X, y, y, 1));
            put(cx, mc::lsrv(Size::X, y, y, ns.zr()));
        } else {
            // The sign comes from the fill above the source.
            put(cx, mc::lsrv(Size::X, x, x, s.zr()));
            put(cx, mc::lsl_imm(Size::X, y, y, 1));
            put(cx, mc::lslv(Size::X, y, y, ns.zr()));
        }
        put(cx, mc::orr(Size::X, x, x, y));
        st(cx, x, X17, r, i);
    }
    Ok(())
}

/// A division or remainder of at most 128 bits: a call of `__udivti3`,
/// `__divti3`, `__umodti3` or `__modti3`, with operands narrower than
/// 128 bits extended into slots of their own first.
fn divide<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    op: Opcode,
    bits: u32,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    let signed = matches!(op, Opcode::SDiv | Opcode::SRem);
    let func = match op {
        Opcode::UDiv => RuntimeFn::UDivTi3,
        Opcode::SDiv => RuntimeFn::DivTi3,
        Opcode::URem => RuntimeFn::UModTi3,
        _ => RuntimeFn::ModTi3,
    };
    let mut args = [CallArg::Mem(FrameRef::Local(0)); 2];
    for (arg, value) in args.iter_mut().zip([lhs, rhs]) {
        let slot = cx.use_mem(value)?;
        *arg = if bits == 128 {
            CallArg::Mem(slot)
        } else {
            let wide = cx.alloc_slot(16, 16)?;
            ld(cx, X16, slot, 0);
            st(cx, X16.zr(), X17, wide, 0);
            ld_ext(cx, X16, slot, bits, 1, signed);
            st(cx, X16.zr(), X17, wide, 1);
            CallArg::Mem(wide)
        };
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
pub fn freeze<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    let r = cx.def_mem(inst)?;
    copy_value(cx, r, arg)
}

/// Writes `value` (class `Mem`) into slot `r`: a constant directly, a
/// value by copying its store size.
fn copy_value(cx: &mut FnCx<'_, A64>, r: FrameRef, value: Value) -> CgResult<()> {
    if matches!(value.kind(), ValueKind::Const(_)) {
        return materialize_mem_const(cx, r, value);
    }
    let bytes = store_size(cx, value_ty(cx, value))?;
    let src = cx.use_mem(value)?;
    if src != r {
        let mut from = Cursor::frame(cx, src, X16);
        let mut to = Cursor::frame(cx, r, X17);
        copy_bytes(cx, &mut to, &mut from, bytes);
    }
    Ok(())
}

/// `%r = select i1 %cond, %if_true, %if_false` of class `Mem`: the
/// address of the chosen operand by `csel`, then one copy.
///
/// # Errors
/// What the operands report.
pub fn select<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    cond: Value,
    if_true: Value,
    if_false: Value,
) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    if cx.class_of(cond) != Class::Gpr {
        return unsupported(MEM_VECTOR);
    }
    let r = cx.def_mem(inst)?;
    if let ValueKind::Const(id) = cond.kind() {
        let chosen = match cx.module.consts.get(id) {
            Const::Int { value, .. } if value & 1 == 1 => if_true,
            _ => if_false,
        };
        return copy_value(cx, r, chosen);
    }
    let c = xr(cx.use_reg(cond)?);
    let bytes = store_size(cx, inst.ty())?;
    let a = cx.use_mem(if_true)?;
    let b = cx.use_mem(if_false)?;
    frame_address(cx, X16, a);
    frame_address(cx, X17, b);
    put(cx, mc::tst_imm(Size::W, c.zr(), 1));
    put(cx, mc::csel(Size::X, X16.zr(), X16.zr(), X17.zr(), Cond::Ne));
    let mut from = Cursor::at_reg(X16, 0, X16);
    let mut to = Cursor::frame(cx, r, X17);
    copy_bytes(cx, &mut to, &mut from, bytes);
    Ok(())
}

// ---- icmp ----

/// `%r = icmp <pred> %lhs, %rhs` of `Mem`-class operands: `eq`/`ne` by
/// or-ing the limbs' differences, the orderings by a `subs`/`sbcs`
/// chain whose flags are those of the whole numbers. The top limbs are
/// shifted left by `64 - tb` first, so that the bits above the width
/// drop out and the sign is bit 63; the borrow into them is still right
/// (`spec/codegen.md` §5.5.1).
///
/// # Errors
/// [`MEM_VECTOR`] for vector operands; what the operands report.
pub fn icmp<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    pred: IntPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    let Some(bits) = wide_int(cx, value_ty(cx, lhs)) else {
        return unsupported(MEM_VECTOR);
    };
    let a = cx.use_mem(lhs)?;
    let b = cx.use_mem(rhs)?;
    let rd = cx.def_reg(inst)?;
    let cond = compare(cx, pred, bits, a, b, xr(rd));
    put(cx, mc::cset(Size::W, xr(rd).zr(), cond));
    cx.finish(inst, rd)
}

/// Sets the flags for `pred` on the integers of `bits` bits at `a` and
/// `b` and returns the condition that holds when it does; `acc` is a
/// register the `eq`/`ne` form may write.
fn compare(
    cx: &mut FnCx<'_, A64>,
    pred: IntPredicate,
    bits: u32,
    a: FrameRef,
    b: FrameRef,
    acc: Reg,
) -> Cond {
    use IntPredicate as P;
    let n = limbs(bits);
    let tb = top_bits(bits);
    let (x, y) = (X16.zr(), X17.zr());
    if matches!(pred, P::Eq | P::Ne) {
        for limb in 0..n {
            ld(cx, X16, a, limb);
            ld(cx, X17, b, limb);
            put(cx, mc::eor(Size::X, x, x, y));
            if limb + 1 == n && tb < 64 {
                put(cx, mc::lsl_imm(Size::X, x, x, 64 - tb));
            }
            if limb == 0 {
                put(cx, mc::mov(Size::X, acc.zr(), x));
            } else {
                put(cx, mc::orr(Size::X, acc.zr(), acc.zr(), x));
            }
        }
        put(cx, mc::cmp_imm(Size::X, acc.sp(), 0));
        return if pred == P::Eq { Cond::Eq } else { Cond::Ne };
    }
    let (first, second, cond) = match pred {
        P::Ult => (a, b, Cond::Lo),
        P::Uge => (a, b, Cond::Hs),
        P::Ugt => (b, a, Cond::Lo),
        P::Ule => (b, a, Cond::Hs),
        P::Slt => (a, b, Cond::Lt),
        P::Sge => (a, b, Cond::Ge),
        P::Sgt => (b, a, Cond::Lt),
        _ => (b, a, Cond::Ge),
    };
    for limb in 0..n {
        ld(cx, X16, first, limb);
        ld(cx, X17, second, limb);
        if limb + 1 == n && tb < 64 {
            put(cx, mc::lsl_imm(Size::X, x, x, 64 - tb));
            put(cx, mc::lsl_imm(Size::X, y, y, 64 - tb));
        }
        if limb == 0 {
            put(cx, mc::cmp(Size::X, x, y));
        } else {
            put(cx, mc::sbcs(Size::X, RegZr::ZR, x, y));
        }
    }
    cond
}

// ---- casts ----

/// `%r = <cast> %arg` with a side of class `Mem`.
///
/// # Errors
/// [`WIDE_FP_CAST`] for a conversion to or from floating point;
/// [`MEM_VECTOR`] for an element-wise cast of vectors; [`CAST`] for
/// `addrspacecast`; what the operand reports.
pub fn cast<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    let from = value_ty(cx, arg);
    let to = inst.ty();
    if cx.types().is_vector(to) && !matches!(inst.opcode(), Opcode::BitCast | Opcode::AddrSpaceCast)
    {
        return crate::lanes::cast(cx, inst, arg);
    }
    match inst.opcode() {
        Opcode::BitCast => bitcast(cx, inst, arg, from, to),
        Opcode::ZExt | Opcode::SExt | Opcode::Trunc => {
            let (Some(fb), Some(tb)) = (cx.types().int_bits(from), cx.types().int_bits(to)) else {
                return unsupported(MEM_VECTOR);
            };
            if inst.opcode() == Opcode::Trunc {
                truncate(cx, inst, arg, tb)
            } else {
                widen(cx, inst, arg, fb, tb, inst.opcode() == Opcode::SExt)
            }
        }
        Opcode::PtrToInt | Opcode::PtrToAddr => {
            if cx.types().is_vector(from) {
                return unsupported(MEM_VECTOR);
            }
            // A pointer is 64 bits: zero-extended.
            let tb = cx.types().int_bits(to).unwrap_or(64);
            widen(cx, inst, arg, 64, tb, false)
        }
        Opcode::IntToPtr => {
            if cx.types().is_vector(to) {
                return unsupported(MEM_VECTOR);
            }
            truncate(cx, inst, arg, 64)
        }
        Opcode::FPToUI
        | Opcode::FPToSI
        | Opcode::UIToFP
        | Opcode::SIToFP
        | Opcode::FPTrunc
        | Opcode::FPExt => {
            if cx.types().is_vector(from) || cx.types().is_vector(to) {
                unsupported(MEM_VECTOR)
            } else {
                unsupported(WIDE_FP_CAST)
            }
        }
        _ => unsupported(CAST),
    }
}

/// `zext`/`sext` (and `ptrtoint`) to an integer of class `Mem`: the
/// source's limbs, its top one extended, then the fill.
fn widen<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    arg: Value,
    from_bits: u32,
    to_bits: u32,
    signed: bool,
) -> CgResult<()> {
    let x = X16.zr();
    let (r, m) = if cx.class_of(arg) == Class::Gpr {
        let reg = xr(cx.use_reg(arg)?);
        let r = cx.def_mem(inst)?;
        put(cx, mc::mov(Size::X, x, reg.zr()));
        extend(cx, X16, from_bits, signed);
        st(cx, x, X17, r, 0);
        (r, 1)
    } else {
        let a = cx.use_mem(arg)?;
        let r = cx.def_mem(inst)?;
        let m = limbs(from_bits);
        for limb in 0..m {
            ld_ext(cx, X16, a, from_bits, limb, signed);
            st(cx, x, X17, r, limb);
        }
        (r, m)
    };
    let n = limbs(to_bits);
    if m < n {
        // `x16` holds the extended top limb of the source.
        let fill = if signed {
            put(cx, mc::asr_imm(Size::X, x, x, 63));
            x
        } else {
            RegZr::ZR
        };
        for limb in m..n {
            st(cx, fill, X17, r, limb);
        }
    }
    Ok(())
}

/// `trunc` (and `inttoptr`) from an integer of class `Mem`: its low
/// limbs; the bits above the new width stay unspecified.
fn truncate<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, arg: Value, to_bits: u32) -> CgResult<()> {
    let a = cx.use_mem(arg)?;
    if to_bits <= 64 {
        let rd = cx.def_reg(inst)?;
        ld(cx, xr(rd), a, 0);
        return cx.finish(inst, rd);
    }
    let r = cx.def_mem(inst)?;
    for limb in 0..limbs(to_bits) {
        ld(cx, X16, a, limb);
        st(cx, X16.zr(), X17, r, limb);
    }
    Ok(())
}

/// `bitcast` with a side of class `Mem`: the same image (`LangRef`: the
/// value stored as one type and loaded as the other), moved between a
/// register and a slot or between slots. An integer is masked to its
/// width on its way into a vector's image.
fn bitcast<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    arg: Value,
    from: TypeId,
    to: TypeId,
) -> CgResult<()> {
    let bytes = store_size(cx, from)?;
    match (cx.class_of(arg), A64.class_of(cx.types(), to)) {
        (Class::Mem, Class::Mem) => {
            let a = cx.use_mem(arg)?;
            let r = cx.def_mem(inst)?;
            let mut dst = Cursor::frame(cx, r, X17);
            write_image(cx, from, a, &mut dst)
        }
        (Class::Gpr, Class::Mem) => {
            let reg = xr(cx.use_reg(arg)?);
            let r = cx.def_mem(inst)?;
            let bits = cx.types().int_bits(from).unwrap_or(64);
            put(cx, mc::mov(Size::X, X16.zr(), reg.zr()));
            extend(cx, X16, bits, false);
            let mut dst = Cursor::frame(cx, r, X17);
            store_gpr_bytes(cx, X16, bytes, &mut dst);
            Ok(())
        }
        (Class::Fpr, Class::Mem) => {
            let reg = vr(cx.use_reg(arg)?);
            let r = cx.def_mem(inst)?;
            let mut dst = Cursor::frame(cx, r, X17);
            store_fpr_bytes(cx, reg, bytes, &mut dst, crate::common::V31);
            Ok(())
        }
        (Class::Mem, Class::Gpr) => {
            let a = cx.use_mem(arg)?;
            let rd = cx.def_reg(inst)?;
            let mut src = Cursor::frame(cx, a, xr(rd));
            load_gpr_at(cx, gpr_load_op(bytes), xr(rd), &mut src);
            cx.finish(inst, rd)
        }
        (Class::Mem, Class::Fpr) => {
            let a = cx.use_mem(arg)?;
            let rd = cx.def_reg(inst)?;
            let mut src = Cursor::frame(cx, a, X16);
            load_fpr_at(cx, fpr_view(bytes), vr(rd), &mut src);
            cx.finish(inst, rd)
        }
        _ => cx.bug("a bitcast without a Mem side reached aggr"),
    }
}

// ---- memory ----

/// `%r = load <ty>, ptr %ptr` of class `Mem`: exactly the store size,
/// copied into the result's slot.
///
/// # Errors
/// `a64:atomic` for an atomic load; what the operand reports.
pub fn load<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, ptr: Value, mem: MemFlags) -> CgResult<()> {
    if mem.ordering().is_some() {
        return unsupported(ATOMIC);
    }
    deferred_first(cx, inst, None)?;
    let bytes = store_size(cx, inst.ty())?;
    let mut from = pointer(cx, ptr)?;
    let r = cx.def_mem(inst)?;
    let mut to = Cursor::frame(cx, r, X16);
    copy_bytes(cx, &mut to, &mut from, bytes);
    Ok(())
}

/// `store <ty> %value, ptr %ptr` of class `Mem`: exactly the store size,
/// an integer with the bits above its width zero (§2.2).
///
/// # Errors
/// `a64:atomic` for an atomic store; what the operands report.
pub fn store<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    value: Value,
    ptr: Value,
    mem: MemFlags,
) -> CgResult<()> {
    if mem.ordering().is_some() {
        return unsupported(ATOMIC);
    }
    deferred_first(cx, inst, None)?;
    let ty = value_ty(cx, value);
    if is_zero_const(cx, value) {
        let bytes = store_size(cx, ty)?;
        let mut to = pointer(cx, ptr)?;
        zero_bytes(cx, &mut to, bytes);
        return Ok(());
    }
    // A constant is written to a slot through `x16`/`x17` before the
    // pointer's cursor holds `x17`.
    let src = cx.use_mem(value)?;
    let mut to = pointer(cx, ptr)?;
    write_image(cx, ty, src, &mut to)
}

// ---- aggregates ----

/// `%r = extractvalue %agg, <offset>` (the field at byte `offset`).
///
/// # Errors
/// What the operand reports.
pub fn extract_value<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    agg: Value,
    offset: u64,
) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    let Ok(offset) = u32::try_from(offset) else {
        return unsupported(hod_cg::LIMIT_FRAME_SIZE);
    };
    let a = at(cx.use_mem(agg)?, offset);
    let ty = inst.ty();
    let bytes = store_size(cx, ty)?;
    match A64.class_of(cx.types(), ty) {
        Class::Gpr => {
            let rd = cx.def_reg(inst)?;
            let mut src = Cursor::frame(cx, a, xr(rd));
            load_gpr_at(cx, gpr_load_op(bytes), xr(rd), &mut src);
            cx.finish(inst, rd)
        }
        Class::Fpr => {
            let rd = cx.def_reg(inst)?;
            let mut src = Cursor::frame(cx, a, X16);
            load_fpr_at(cx, fpr_view(bytes), vr(rd), &mut src);
            cx.finish(inst, rd)
        }
        Class::Mem => {
            let r = cx.def_mem(inst)?;
            let mut from = Cursor::frame(cx, a, X16);
            let mut to = Cursor::frame(cx, r, X17);
            copy_bytes(cx, &mut to, &mut from, bytes);
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
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    agg: Value,
    value: Value,
    offset: u64,
) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    let Ok(offset) = u32::try_from(offset) else {
        return unsupported(hod_cg::LIMIT_FRAME_SIZE);
    };
    let ty = value_ty(cx, value);
    let bytes = store_size(cx, ty)?;
    match cx.class_of(value) {
        Class::Gpr => {
            let src = if is_zero_const(cx, value) { None } else { Some(xr(cx.use_reg(value)?)) };
            let r = cx.def_mem(inst)?;
            copy_value(cx, r, agg)?;
            let mut to = Cursor::frame(cx, at(r, offset), X17);
            match src {
                None => zero_bytes(cx, &mut to, bytes),
                Some(reg) => {
                    let bits = cx.types().int_bits(ty).unwrap_or(64);
                    put(cx, mc::mov(Size::X, X16.zr(), reg.zr()));
                    extend(cx, X16, bits, false);
                    store_gpr_bytes(cx, X16, bytes, &mut to);
                }
            }
        }
        Class::Fpr => {
            let reg = vr(cx.use_reg(value)?);
            let r = cx.def_mem(inst)?;
            copy_value(cx, r, agg)?;
            let mut to = Cursor::frame(cx, at(r, offset), X17);
            store_fpr_bytes(cx, reg, bytes, &mut to, crate::common::V31);
        }
        Class::Mem => {
            let r = cx.def_mem(inst)?;
            copy_value(cx, r, agg)?;
            if is_zero_const(cx, value) {
                let mut to = Cursor::frame(cx, at(r, offset), X17);
                zero_bytes(cx, &mut to, bytes);
            } else {
                let src = cx.use_mem(value)?;
                let mut to = Cursor::frame(cx, at(r, offset), X17);
                write_image(cx, ty, src, &mut to)?;
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
/// [`WIDE_MUL_OVERFLOW`] for a multiplication wider than 64 bits;
/// [`OVERFLOW`] for a vector; what the operands report.
pub fn overflow_intrinsic<'a>(
    cx: &mut FnCx<'a, A64>,
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
    let flag_at = u32::try_from(cx.types().field_offset(inst.ty(), 1))
        .map_err(|_| hod_cg::CgError::unsupported(hod_cg::LIMIT_FRAME_SIZE))?;
    if bits > 64 {
        if matches!(which, Intrinsic::SmulWithOverflow | Intrinsic::UmulWithOverflow) {
            return unsupported(WIDE_MUL_OVERFLOW);
        }
        return overflow_wide(cx, inst, which, bits, lhs, rhs, flag_at);
    }
    let ra = xr(cx.use_reg(lhs)?);
    let rb = xr(cx.use_reg(rhs)?);
    let (a, b) = (ra.zr(), rb.zr());
    let (x, y) = (X16.zr(), X17.zr());
    let signed = matches!(
        which,
        Intrinsic::SaddWithOverflow | Intrinsic::SsubWithOverflow | Intrinsic::SmulWithOverflow
    );
    extend(cx, ra, bits, signed);
    extend(cx, rb, bits, signed);
    let above = !((1u64 << (bits % 64)) - 1); // the bits above the width (bits < 64)
    let cond = match which {
        Intrinsic::UaddWithOverflow if bits == 64 => {
            put(cx, mc::adds(Size::X, a, a, b));
            Cond::Hs
        }
        Intrinsic::UaddWithOverflow => {
            put(cx, mc::add(Size::X, a, a, b));
            put(cx, mc::tst_imm(Size::X, a, above));
            Cond::Ne
        }
        Intrinsic::UsubWithOverflow => {
            put(cx, mc::subs(Size::X, a, a, b));
            Cond::Lo
        }
        Intrinsic::SaddWithOverflow | Intrinsic::SsubWithOverflow if bits == 64 => {
            put(
                cx,
                if which == Intrinsic::SaddWithOverflow {
                    mc::adds(Size::X, a, a, b)
                } else {
                    mc::subs(Size::X, a, a, b)
                },
            );
            Cond::Vs
        }
        Intrinsic::SaddWithOverflow | Intrinsic::SsubWithOverflow => {
            put(
                cx,
                if which == Intrinsic::SaddWithOverflow {
                    mc::add(Size::X, a, a, b)
                } else {
                    mc::sub(Size::X, a, a, b)
                },
            );
            put(cx, mc::sbfx(Size::X, x, a, 0, bits));
            put(cx, mc::cmp(Size::X, x, a));
            Cond::Ne
        }
        Intrinsic::UmulWithOverflow if bits <= 32 => {
            put(cx, mc::mul(Size::X, a, a, b));
            put(cx, mc::tst_imm(Size::X, a, above));
            Cond::Ne
        }
        Intrinsic::UmulWithOverflow => {
            put(cx, mc::umulh(x, a, b));
            put(cx, mc::mul(Size::X, a, a, b));
            if bits < 64 {
                put(cx, mc::lsr_imm(Size::X, y, a, bits));
                put(cx, mc::orr(Size::X, x, x, y));
            }
            put(cx, mc::cmp_imm(Size::X, X16.sp(), 0));
            Cond::Ne
        }
        Intrinsic::SmulWithOverflow if bits <= 32 => {
            put(cx, mc::mul(Size::X, a, a, b));
            put(cx, mc::sbfx(Size::X, x, a, 0, bits));
            put(cx, mc::cmp(Size::X, x, a));
            Cond::Ne
        }
        Intrinsic::SmulWithOverflow => {
            // The 128-bit product `x16:a` fits iff its high half is the
            // sign of its low half and the low half is the low `bits`
            // bits sign-extended.
            put(cx, mc::smulh(x, a, b));
            put(cx, mc::mul(Size::X, a, a, b));
            if bits < 64 {
                put(cx, mc::sbfx(Size::X, y, a, 0, bits));
                put(cx, mc::cmp(Size::X, y, a));
                put(cx, mc::asr_imm(Size::X, y, a, 63));
                put(cx, mc::ccmp(Size::X, x, y, 0, Cond::Eq));
            } else {
                put(cx, mc::asr_imm(Size::X, y, a, 63));
                put(cx, mc::cmp(Size::X, x, y));
            }
            Cond::Ne
        }
        _ => return cx.bug("an intrinsic that is not an overflow intrinsic reached aggr"),
    };
    put(cx, mc::cset(Size::W, b, cond));
    let r = cx.def_mem(inst)?;
    extend(cx, ra, bits, false);
    let mut to = Cursor::frame(cx, r, X17);
    store_gpr_bytes(cx, ra, bits.div_ceil(8), &mut to);
    let mut to = Cursor::frame(cx, at(r, flag_at), X17);
    store_gpr_at(cx, MemSize::B, b, &mut to);
    Ok(())
}

/// `llvm.{s,u}{add,sub}.with.overflow` of an integer of class `Mem`: a
/// carry chain over the operands with their top limbs extended; the
/// unsigned flag is the carry out (or the bits above the width), the
/// signed one `V` (or the top limb not being its own extension).
fn overflow_wide<'a>(
    cx: &mut FnCx<'a, A64>,
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
    let flag = xr(cx.temp(Class::Gpr)?);
    let n = limbs(bits);
    let tb = top_bits(bits);
    let signed = matches!(which, Intrinsic::SaddWithOverflow | Intrinsic::SsubWithOverflow);
    let add = matches!(which, Intrinsic::SaddWithOverflow | Intrinsic::UaddWithOverflow);
    let (x, y) = (X16.zr(), X17.zr());
    for limb in 0..n {
        ld_ext(cx, X16, a, bits, limb, signed);
        ld_ext(cx, X17, b, bits, limb, signed);
        let word = match (add, limb == 0) {
            (true, true) => mc::adds(Size::X, x, x, y),
            (true, false) => mc::adcs(Size::X, x, x, y),
            (false, true) => mc::subs(Size::X, x, x, y),
            (false, false) => mc::sbcs(Size::X, x, x, y),
        };
        put(cx, word);
        if limb + 1 < n {
            st(cx, x, X17, r, limb);
        }
    }
    // `x16` is the top limb of the result, the flags are the chain's.
    let cond = match (signed, add, tb == 64) {
        (false, true, true) => Cond::Hs,
        (false, true, false) => {
            put(cx, mc::tst_imm(Size::X, x, !((1u64 << tb) - 1)));
            Cond::Ne
        }
        (false, false, _) => Cond::Lo,
        (true, _, true) => Cond::Vs,
        (true, _, false) => {
            put(cx, mc::sbfx(Size::X, y, x, 0, tb));
            put(cx, mc::cmp(Size::X, y, x));
            Cond::Ne
        }
    };
    put(cx, mc::cset(Size::W, flag.zr(), cond));
    extend(cx, X16, tb, false);
    st(cx, x, X17, r, n - 1);
    let mut to = Cursor::frame(cx, at(r, flag_at), X17);
    store_gpr_at(cx, MemSize::B, flag.zr(), &mut to);
    Ok(())
}
