//! Integers wider than 32 bits (`spec/codegen-x86.md` §8; design 0008
//! §1.4, §5.2): design 0008's I7 (WP-298) owns this file.
//!
//! # What a slot holds
//!
//! An integer of `N` bits, `33 <= N <= 64`, is of class `Mem`: its home is
//! an 8-byte frame slot, the low word at offset 0 and the high word at 4.
//! The bits above `N` in the high word are **unspecified**; a consumer that
//! depends on them (a compare, a right shift, a division, an extension, a
//! `store`) extends the high word first, by zero or sign as its operation
//! needs. `add`, `sub`, `mul`, the bitwise operations, `shl` and `trunc`
//! leave garbage there.
//!
//! # Registers
//!
//! Every lowering here works half by half in `eax`, `ecx` and `edx`,
//! which never hold a value, and asks the pool for nothing but the
//! register of a `Gpr` operand or result. Operands are read from their
//! slots before the result's slot is written. The glue for a `Mem` value
//! (`Target::copy`, [`materialize_mem`]) writes `ecx` only.
//!
//! # Wider than 64 bits
//!
//! 65 to 128 bits: `wideint.rs` (§22, WP-346); every entry point here
//! hands such a value over first.
//!
//! # Division
//!
//! `udiv`, `sdiv`, `urem`, `srem` call libgcc's `__udivdi3`, `__divdi3`,
//! `__umoddi3`, `__moddi3` directly: the divisor's halves then the
//! dividend's are pushed (high word first), `call`, `add esp, 16`, the
//! result in `edx:eax`. The 16 bytes keep `esp` 16-aligned at the call.

use hod_cg::{CallTarget, CgResult, FnCx, FrameRef, Label, RuntimeFn, SymRef};
use hod_ir::{
    Const, Inst, InstKind, IntPredicate, Intrinsic, MemFlags, Opcode, Switch, TypeId, Value,
    ValueKind,
};
use hod_mc_x64 as mc;
use hod_mc_x64::{Cond, Mem, Reg, Size};

use crate::alu::{self, Ext};
use crate::common::{
    EAX, ECX, EDX, ESP, branch, branch_cond, frame_inst, gp, load_at, put, slot_address, store_at,
    sym_address, unsupported, value_ty,
};
use crate::target::X86;
use crate::wideint as wide;

/// An encoder of `op r32, m32`.
type RegMem = fn(Size, Reg, Mem) -> mc::Inst;

/// An `i64` passed or returned by value: what remains of WP-293's
/// refusal until I6 (`abi.rs`) takes 64-bit values.
pub const I64: &str = "x86:i64";
/// An integer wider than 64 bits (`i65` and up, `i128`).
pub const WIDE: &str = "x86:i64:wide";
/// A cast of a wide integer that is no integer or pointer cast (a
/// `bitcast` to or from a floating-point or vector type).
pub const CAST: &str = "x86:int-cast";

/// The width of `ty` if it is an integer wider than 32 bits.
#[inline]
pub fn wide_bits(cx: &FnCx<'_, X86>, ty: TypeId) -> Option<u32> {
    cx.types().int_bits(ty).filter(|&bits| bits > 32)
}

/// Whether `value` is an integer wider than 32 bits.
#[inline]
pub fn is_wide(cx: &FnCx<'_, X86>, value: Value) -> bool {
    wide_bits(cx, value_ty(cx, value)).is_some()
}

/// Whether a cast from `arg` to `inst`'s type is this module's: either
/// side a wide integer.
#[inline]
pub fn is_cast(cx: &FnCx<'_, X86>, inst: Inst<'_>, arg: Value) -> bool {
    is_wide(cx, arg) || wide_bits(cx, inst.ty()).is_some()
}

/// The width of the wide integer type `ty`: at most 64, else [`WIDE`].
fn bits_of(cx: &FnCx<'_, X86>, ty: TypeId) -> CgResult<u32> {
    match cx.types().int_bits(ty) {
        Some(bits @ 33..=64) => Ok(bits),
        Some(65..) => unsupported(WIDE),
        _ => unsupported(CAST),
    }
}

// ---- words of a slot ----

/// `mov r, [slot + 4 * word]`.
fn ld(cx: &mut FnCx<'_, X86>, r: Reg, slot: FrameRef, word: u32) {
    frame_inst(cx, slot.offset_by(4 * word), |m| mc::mov_rm(Size::D, r, m));
}

/// `mov [slot + 4 * word], r`.
fn st(cx: &mut FnCx<'_, X86>, r: Reg, slot: FrameRef, word: u32) {
    frame_inst(cx, slot.offset_by(4 * word), |m| mc::mov_mr(Size::D, m, r));
}

/// `mov dword [slot + 4 * word], imm`.
fn st_imm(cx: &mut FnCx<'_, X86>, slot: FrameRef, word: u32, imm: u32) {
    frame_inst(cx, slot.offset_by(4 * word), |m| mc::mov_mi(Size::D, m, i64::from(imm as i32)));
}

/// The instruction `make` builds over word `word` of `slot`.
fn op_m(cx: &mut FnCx<'_, X86>, slot: FrameRef, word: u32, make: impl FnOnce(Mem) -> mc::Inst) {
    frame_inst(cx, slot.offset_by(4 * word), make);
}

/// The high word of the `bits`-bit integer at `slot` into `r`, extended
/// from its `bits - 32` meaningful bits by `ext` (flags written).
fn ld_hi(cx: &mut FnCx<'_, X86>, r: Reg, slot: FrameRef, bits: u32, ext: Ext) {
    ld(cx, r, slot, 1);
    alu::extend_to(cx, r, bits - 32, ext);
}

/// Copies the two words of `src` to `dst` through `ecx`.
fn copy(cx: &mut FnCx<'_, X86>, dst: FrameRef, src: FrameRef) {
    if dst == src {
        return;
    }
    for word in 0..2 {
        ld(cx, ECX, src, word);
        st(cx, ECX, dst, word);
    }
}

/// Stores `lo` and `hi` to the two words of `r`.
fn st_pair(cx: &mut FnCx<'_, X86>, r: FrameRef, lo: Reg, hi: Reg) {
    st(cx, lo, r, 0);
    st(cx, hi, r, 1);
}

/// The 64-bit value of constant `value` (an integer, `undef`, `poison`,
/// zero), if it is one whose bits are known without a symbol.
fn const_u64(cx: &FnCx<'_, X86>, value: Value) -> Option<u64> {
    let ValueKind::Const(id) = value.kind() else { return None };
    match cx.module.consts.get(id) {
        Const::Int { value, .. } => Some(value),
        Const::Null(_) | Const::Zero(_) | Const::Undef(_) | Const::Poison(_) => Some(0),
        _ => None,
    }
}

/// `Target::materialize_mem` for a wide integer constant: two immediate
/// stores, or a symbol's address (`ptrtoint` of an address) through `ecx`
/// with a zero high word. Flags kept.
///
/// # Errors
/// [`WIDE`]; [`crate::common::CONSTANT`] for another constant.
pub fn materialize_mem(cx: &mut FnCx<'_, X86>, dst: FrameRef, value: Value) -> CgResult<()> {
    if wide::wide_bits(cx, value_ty(cx, value)).is_some() {
        return wide::materialize_mem(cx, dst, value);
    }
    bits_of(cx, value_ty(cx, value))?;
    if let Some(v) = const_u64(cx, value) {
        st_imm(cx, dst, 0, v as u32);
        st_imm(cx, dst, 1, (v >> 32) as u32);
        return Ok(());
    }
    let ValueKind::Const(id) = value.kind() else {
        return cx.bug("materialize_mem of something that is no constant");
    };
    match cx.module.consts.get(id) {
        Const::IntOfAddr { .. } | Const::AddrOfInt { .. } => {
            crate::common::materialize_const(cx, ECX, id)?;
            st(cx, ECX, dst, 0);
            st_imm(cx, dst, 1, 0);
            Ok(())
        }
        _ => unsupported(crate::common::CONSTANT),
    }
}

// ---- binary operations ----

/// `%r = <op> %lhs, %rhs` of `i33`..`i64`.
///
/// # Errors
/// [`WIDE`]; what the operands report.
pub fn binary<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, lhs: Value, rhs: Value) -> CgResult<()> {
    if wide::wide_bits(cx, inst.ty()).is_some() {
        return wide::binary(cx, inst, lhs, rhs);
    }
    let bits = bits_of(cx, inst.ty())?;
    let op = inst.opcode();
    match op {
        Opcode::Add | Opcode::Sub | Opcode::And | Opcode::Or | Opcode::Xor => {
            let (a, b) = (cx.use_mem(lhs)?, cx.use_mem(rhs)?);
            let r = cx.def_mem(inst)?;
            ld(cx, EAX, a, 0);
            ld(cx, EDX, a, 1);
            let (low, high): (RegMem, RegMem) = match op {
                Opcode::Add => (mc::add_rm, mc::adc_rm),
                Opcode::Sub => (mc::sub_rm, mc::sbb_rm),
                Opcode::And => (mc::and_rm, mc::and_rm),
                Opcode::Or => (mc::or_rm, mc::or_rm),
                _ => (mc::xor_rm, mc::xor_rm),
            };
            op_m(cx, b, 0, |m| low(Size::D, EAX, m));
            op_m(cx, b, 1, |m| high(Size::D, EDX, m));
            st_pair(cx, r, EAX, EDX);
            Ok(())
        }
        Opcode::Mul => {
            let (a, b) = (cx.use_mem(lhs)?, cx.use_mem(rhs)?);
            let r = cx.def_mem(inst)?;
            // lo:hi = a0·b0; hi += a0·b1 + a1·b0.
            ld(cx, EAX, a, 0);
            op_m(cx, b, 0, |m| mc::mul_m(Size::D, m));
            ld(cx, ECX, a, 0);
            op_m(cx, b, 1, |m| mc::imul_rm(Size::D, ECX, m));
            put(cx, mc::add_rr(Size::D, EDX, ECX));
            ld(cx, ECX, a, 1);
            op_m(cx, b, 0, |m| mc::imul_rm(Size::D, ECX, m));
            put(cx, mc::add_rr(Size::D, EDX, ECX));
            st_pair(cx, r, EAX, EDX);
            Ok(())
        }
        Opcode::Shl | Opcode::LShr | Opcode::AShr => shift(cx, inst, op, bits, lhs, rhs),
        Opcode::UDiv | Opcode::SDiv | Opcode::URem | Opcode::SRem => {
            divide(cx, inst, op, bits, lhs, rhs)
        }
        _ => unsupported(CAST),
    }
}

/// The value `lhs` of a right shift in `eax` (low) and `edx` (high, the
/// width extended by `ext`).
fn shift_operand(cx: &mut FnCx<'_, X86>, a: FrameRef, bits: u32, ext: Ext) {
    ld_hi(cx, EDX, a, bits, ext);
    ld(cx, EAX, a, 0);
}

/// `shl`, `lshr`, `ashr`: by a constant, `shld`/`shrd` and a shift, or
/// for 32 and more a word move; by a variable amount in `cl`, the same
/// pair (which counts modulo 32) then `test cl, 32` and the word move.
/// An amount of the width or more is poison: nothing is computed.
fn shift<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    op: Opcode,
    bits: u32,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    let ext = if op == Opcode::AShr { Ext::Sign } else { Ext::Zero };
    let amount = const_u64(cx, rhs);
    if amount.is_some_and(|k| k >= u64::from(bits)) {
        return Ok(());
    }
    let a = cx.use_mem(lhs)?;
    let count = match amount {
        Some(_) => None,
        None => Some(cx.use_mem(rhs)?),
    };
    let r = cx.def_mem(inst)?;
    if op == Opcode::Shl {
        ld(cx, EAX, a, 0);
        ld(cx, EDX, a, 1);
    } else {
        shift_operand(cx, a, bits, ext);
    }
    let Some(count) = count else {
        let k = amount.unwrap_or(0) as u8;
        // The second shift of a word move by exactly 32 is by 0: none.
        let by = |cx: &mut FnCx<'_, X86>, s: fn(Size, Reg, u8) -> mc::Inst, r: Reg| {
            if k > 32 {
                put(cx, s(Size::D, r, k - 32));
            }
        };
        match (op, k) {
            (_, 0) => {}
            (Opcode::Shl, 1..32) => {
                put(cx, mc::shld_rri(Size::D, EDX, EAX, k));
                put(cx, mc::shl_ri(Size::D, EAX, k));
            }
            (Opcode::Shl, _) => {
                put(cx, mc::mov_rr(Size::D, EDX, EAX));
                by(cx, mc::shl_ri, EDX);
                put(cx, mc::mov_ri(Size::D, EAX, 0));
            }
            (_, 1..32) => {
                put(cx, mc::shrd_rri(Size::D, EAX, EDX, k));
                let s = if op == Opcode::AShr { mc::sar_ri } else { mc::shr_ri };
                put(cx, s(Size::D, EDX, k));
            }
            (Opcode::AShr, _) => {
                put(cx, mc::mov_rr(Size::D, EAX, EDX));
                by(cx, mc::sar_ri, EAX);
                put(cx, mc::sar_ri(Size::D, EDX, 31));
            }
            _ => {
                put(cx, mc::mov_rr(Size::D, EAX, EDX));
                by(cx, mc::shr_ri, EAX);
                put(cx, mc::mov_ri(Size::D, EDX, 0));
            }
        }
        st_pair(cx, r, EAX, EDX);
        return Ok(());
    };
    ld(cx, ECX, count, 0);
    let done = cx.new_label();
    if op == Opcode::Shl {
        put(cx, mc::shld_rrcl(Size::D, EDX, EAX));
        put(cx, mc::shl_rcl(Size::D, EAX));
    } else {
        put(cx, mc::shrd_rrcl(Size::D, EAX, EDX));
        let s = if op == Opcode::AShr { mc::sar_rcl } else { mc::shr_rcl };
        put(cx, s(Size::D, EDX));
    }
    put(cx, mc::test_ri(Size::B, ECX, 32));
    branch_cond(cx, Cond::E, done)?;
    match op {
        Opcode::Shl => {
            put(cx, mc::mov_rr(Size::D, EDX, EAX));
            put(cx, mc::mov_ri(Size::D, EAX, 0));
        }
        Opcode::AShr => {
            put(cx, mc::mov_rr(Size::D, EAX, EDX));
            put(cx, mc::sar_ri(Size::D, EDX, 31));
        }
        _ => {
            put(cx, mc::mov_rr(Size::D, EAX, EDX));
            put(cx, mc::mov_ri(Size::D, EDX, 0));
        }
    }
    cx.bind(done)?;
    st_pair(cx, r, EAX, EDX);
    Ok(())
}

/// Pushes the two words of the `bits`-bit integer at `slot`, high word
/// first, extended by `ext` (through `ecx`).
fn push_operand(cx: &mut FnCx<'_, X86>, slot: FrameRef, bits: u32, ext: Ext) {
    if bits == 64 {
        op_m(cx, slot, 1, mc::push_m);
    } else {
        ld_hi(cx, ECX, slot, bits, ext);
        put(cx, mc::push_r(ECX));
    }
    op_m(cx, slot, 0, mc::push_m);
}

/// Calls runtime function `func` with the pushed 16 bytes of arguments
/// and pops them: the result is in `edx:eax`. The call is `pic`'s: in
/// PIC, through the PLT with the GOT pointer in `ebx` (`spec/codegen-x86.md`
/// §9.2), as `llc` calls these.
fn call_pushed(cx: &mut FnCx<'_, X86>, func: RuntimeFn) -> CgResult<()> {
    let start = cx.offset();
    crate::pic::emit_call(cx, CallTarget::Sym(SymRef::Runtime(func)), 0)?;
    cx.record_call_site(start, cx.offset() - start);
    put(cx, mc::add_ri(Size::D, ESP, 16));
    Ok(())
}

/// `udiv`, `sdiv`, `urem`, `srem` through libgcc (module comment).
fn divide<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    op: Opcode,
    bits: u32,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    let (func, ext) = match op {
        Opcode::UDiv => (RuntimeFn::UDivDi3, Ext::Zero),
        Opcode::SDiv => (RuntimeFn::DivDi3, Ext::Sign),
        Opcode::URem => (RuntimeFn::UModDi3, Ext::Zero),
        _ => (RuntimeFn::ModDi3, Ext::Sign),
    };
    let (a, b) = (cx.use_mem(lhs)?, cx.use_mem(rhs)?);
    let r = cx.def_mem(inst)?;
    push_operand(cx, b, bits, ext);
    push_operand(cx, a, bits, ext);
    call_pushed(cx, func)?;
    st_pair(cx, r, EAX, EDX);
    Ok(())
}

/// `%r = freeze %arg`: a copy.
///
/// # Errors
/// [`WIDE`]; what the operand reports.
pub fn freeze<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    if wide::wide_bits(cx, inst.ty()).is_some() {
        return wide::freeze(cx, inst, arg);
    }
    bits_of(cx, inst.ty())?;
    let a = cx.use_mem(arg)?;
    let r = cx.def_mem(inst)?;
    copy(cx, r, a);
    Ok(())
}

// ---- compares ----

/// Whether `pred` is signed.
fn signed(pred: IntPredicate) -> bool {
    matches!(pred, IntPredicate::Sgt | IntPredicate::Sge | IntPredicate::Slt | IntPredicate::Sle)
}

/// The compare of the wide integers `lhs` and `rhs` into the flags; the
/// condition that holds when `pred` does. Equality: `xor` of each half,
/// `or` of the two. Order: `cmp` of the low halves and `sbb` of the high
/// ones (extended), with the operands swapped for `>` and `<=`, so that
/// only the carry, sign and overflow flags are read.
fn compare(cx: &mut FnCx<'_, X86>, pred: IntPredicate, lhs: Value, rhs: Value) -> CgResult<Cond> {
    let bits = bits_of(cx, value_ty(cx, lhs))?;
    let (a, b) = (cx.use_mem(lhs)?, cx.use_mem(rhs)?);
    Ok(compare_slots(cx, pred, bits, a, b))
}

/// [`compare`] of the `bits`-bit integers at slots `a` and `b`.
fn compare_slots(
    cx: &mut FnCx<'_, X86>,
    pred: IntPredicate,
    bits: u32,
    a: FrameRef,
    b: FrameRef,
) -> Cond {
    let ext = if signed(pred) { Ext::Sign } else { Ext::Zero };
    if matches!(pred, IntPredicate::Eq | IntPredicate::Ne) {
        ld_hi(cx, ECX, a, bits, ext);
        ld_hi(cx, EDX, b, bits, ext);
        ld(cx, EAX, a, 0);
        op_m(cx, b, 0, |m| mc::xor_rm(Size::D, EAX, m));
        put(cx, mc::xor_rr(Size::D, ECX, EDX));
        put(cx, mc::or_rr(Size::D, EAX, ECX));
        return alu::cond_of(pred);
    }
    let (x, y, cond) = match pred {
        IntPredicate::Ult => (a, b, Cond::B),
        IntPredicate::Uge => (a, b, Cond::Ae),
        IntPredicate::Ugt => (b, a, Cond::B),
        IntPredicate::Ule => (b, a, Cond::Ae),
        IntPredicate::Slt => (a, b, Cond::L),
        IntPredicate::Sge => (a, b, Cond::Ge),
        IntPredicate::Sgt => (b, a, Cond::L),
        _ => (b, a, Cond::Ge),
    };
    ld_hi(cx, ECX, x, bits, ext);
    ld_hi(cx, EDX, y, bits, ext);
    ld(cx, EAX, x, 0);
    op_m(cx, y, 0, |m| mc::cmp_rm(Size::D, EAX, m));
    put(cx, mc::sbb_rr(Size::D, ECX, EDX));
    cond
}

/// `%r = icmp <pred> %lhs, %rhs` of wide integers: the compare, `setcc`
/// into `cl` (or the result's low byte), `movzx`.
///
/// # Errors
/// [`WIDE`]; what the operands report.
pub fn icmp<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    pred: IntPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    if wide_bits(cx, inst.ty()).is_some() {
        // A vector compare never reaches here; a wide `i1` does not exist.
        return unsupported(CAST);
    }
    if wide::wide_bits(cx, value_ty(cx, lhs)).is_some() {
        return wide::icmp(cx, inst, pred, lhs, rhs);
    }
    let rd = cx.def_reg(inst)?;
    let cond = compare(cx, pred, lhs, rhs)?;
    let r = gp(rd);
    let byte = if crate::reg::has_low_byte(r) { r } else { ECX };
    put(cx, mc::setcc_r(cond, byte));
    put(cx, mc::movzx_rr(Size::D, r, Size::B, byte));
    cx.finish(inst, rd)
}

/// `%r = select i1 %cond, %if_true, %if_false` of wide integers: the
/// condition in `eax`, `test al, 1`, and a copy of the chosen arm on
/// each side of a branch.
///
/// # Errors
/// [`WIDE`]; what the operands report.
pub fn select<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    cond: Value,
    if_true: Value,
    if_false: Value,
) -> CgResult<()> {
    if wide::wide_bits(cx, inst.ty()).is_some() {
        return wide::select(cx, inst, cond, if_true, if_false);
    }
    bits_of(cx, inst.ty())?;
    let (t, f) = (cx.use_mem(if_true)?, cx.use_mem(if_false)?);
    let r = cx.def_mem(inst)?;
    if let Some(c) = crate::common::const_int(cx, cond) {
        copy(cx, r, if c & 1 != 0 { t } else { f });
        return Ok(());
    }
    cx.put(cond, crate::reg::EAX)?;
    put(cx, mc::test_ri(Size::B, EAX, 1));
    let (other, done): (Label, Label) = (cx.new_label(), cx.new_label());
    branch_cond(cx, Cond::E, other)?;
    copy(cx, r, t);
    branch(cx, done)?;
    cx.bind(other)?;
    copy(cx, r, f);
    cx.bind(done)?;
    Ok(())
}

// ---- casts ----

/// `%r = <cast> %arg` with a wide integer on either side: `zext`, `sext`,
/// `trunc`, `ptrtoint`, `ptrtoaddr`, `inttoptr`, `bitcast` between
/// integers.
///
/// # Errors
/// [`WIDE`]; [`CAST`] for another cast; what the operand reports.
pub fn cast<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let from_ty = value_ty(cx, arg);
    let types = cx.types();
    let int_or_ptr =
        |ty| types.int_bits(ty).is_some() || matches!(types.kind(ty), hod_ir::TypeKind::Ptr { .. });
    if !int_or_ptr(from_ty) || !int_or_ptr(inst.ty()) {
        return unsupported(CAST);
    }
    let ext = match inst.opcode() {
        Opcode::ZExt | Opcode::PtrToInt | Opcode::PtrToAddr => Ext::Zero,
        Opcode::SExt => Ext::Sign,
        Opcode::Trunc | Opcode::IntToPtr | Opcode::BitCast => Ext::None,
        Opcode::AddrSpaceCast => return unsupported(alu::ADDRSPACECAST),
        _ => return unsupported(CAST),
    };
    if wide::wide_bits(cx, from_ty).is_some() || wide::wide_bits(cx, inst.ty()).is_some() {
        return wide::cast(cx, inst, arg, ext);
    }
    let from_wide = wide_bits(cx, from_ty).is_some();
    let to_wide = wide_bits(cx, inst.ty()).is_some();
    let from_bits = if from_wide { bits_of(cx, from_ty)? } else { crate::common::width(cx, arg) };
    if to_wide {
        let to_bits = bits_of(cx, inst.ty())?;
        if from_wide {
            // Between two wide widths: the low word as it is, the high
            // word extended from the source's width (garbage above it
            // when narrowing, which is what the slot allows).
            let a = cx.use_mem(arg)?;
            let r = cx.def_mem(inst)?;
            if from_bits < to_bits && ext != Ext::None {
                ld_hi(cx, ECX, a, from_bits, ext);
                st(cx, ECX, r, 1);
                ld(cx, ECX, a, 0);
                st(cx, ECX, r, 0);
            } else {
                copy(cx, r, a);
            }
            return Ok(());
        }
        // From a `Gpr` value: its register extended to 32 bits, then the
        // high word 0 or the sign.
        let ext = if ext == Ext::None { Ext::Zero } else { ext };
        let ra = gp(cx.use_reg(arg)?);
        let r = cx.def_mem(inst)?;
        alu::extend_to(cx, ra, from_bits, ext);
        st(cx, ra, r, 0);
        if ext == Ext::Sign {
            put(cx, mc::mov_rr(Size::D, EAX, ra));
            put(cx, mc::sar_ri(Size::D, EAX, 31));
            st(cx, EAX, r, 1);
        } else {
            st_imm(cx, r, 1, 0);
        }
        return Ok(());
    }
    // To a `Gpr` value: the low word.
    let a = cx.use_mem(arg)?;
    let rd = cx.def_reg(inst)?;
    ld(cx, gp(rd), a, 0);
    cx.finish(inst, rd)
}

// ---- memory ----

// ---- intrinsics (WP-306) ----

/// The integer intrinsics lowered here when their result is wide.
fn is_int_intrinsic(which: Intrinsic<'_>) -> bool {
    use Intrinsic as I;
    matches!(
        which,
        I::Bswap
            | I::Ctpop
            | I::Ctlz
            | I::Cttz
            | I::BitReverse
            | I::Abs
            | I::Smax
            | I::Smin
            | I::Umax
            | I::Umin
            | I::SaddSat
            | I::UaddSat
            | I::SsubSat
            | I::UsubSat
            | I::SshlSat
            | I::UshlSat
            | I::Fshl
            | I::Fshr
    )
}

/// Whether the call of intrinsic `which` as `inst` is this module's: one
/// of the integer intrinsics with a result wider than 32 bits, or
/// `scmp`/`ucmp` of operands wider than 32 bits.
#[inline]
pub fn is_intrinsic(cx: &FnCx<'_, X86>, inst: Inst<'_>, which: Intrinsic<'_>) -> bool {
    if matches!(which, Intrinsic::Scmp | Intrinsic::Ucmp) {
        let InstKind::Call(site) = inst.kind() else { return false };
        return site.args().first().is_some_and(|&a| is_wide(cx, a))
            && cx.types().int_bits(inst.ty()).is_some();
    }
    is_int_intrinsic(which) && wide_bits(cx, inst.ty()).is_some()
}

/// The mask of the low `bits` bits of a 32-bit word (`bits` 1 to 32).
fn word_mask(bits: u32) -> u32 {
    if bits >= 32 { u32::MAX } else { (1 << bits) - 1 }
}

/// `mov r, imm`.
fn mov_imm(cx: &mut FnCx<'_, X86>, r: Reg, imm: u32) {
    put(cx, mc::mov_ri(Size::D, r, i64::from(imm as i32)));
}

/// An immediate operand of a 32-bit instruction.
fn imm(v: u32) -> i64 {
    i64::from(v as i32)
}

/// `%r = call iN @llvm.<which>(...)`, `33 <= N <= 64` (`spec/codegen-x86.md`
/// §8): the bit counts, byte and bit reversal, `abs`, min/max, the
/// saturating arithmetic and shifts, the funnel shifts.
///
/// # Errors
/// [`WIDE`]; what the operands report.
pub fn intrinsic<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    use Intrinsic as I;
    if let (I::Scmp | I::Ucmp, &[a, b]) = (which, args) {
        if wide::wide_bits(cx, value_ty(cx, a)).is_some() {
            return unsupported(wide::INTRINSIC);
        }
        return three_way(cx, inst, which == I::Scmp, a, b);
    }
    if wide::wide_bits(cx, inst.ty()).is_some() {
        return unsupported(wide::INTRINSIC);
    }
    let bits = bits_of(cx, inst.ty())?;
    match (which, args) {
        (I::Bswap | I::Ctpop | I::Ctlz | I::Cttz | I::BitReverse | I::Abs, &[x, ..]) => {
            unary(cx, inst, which, bits, x)
        }
        (I::Smax | I::Smin | I::Umax | I::Umin, &[a, b]) => min_max(cx, inst, which, bits, a, b),
        (I::SaddSat | I::SsubSat, &[a, b]) => signed_sat(cx, inst, which == I::SaddSat, bits, a, b),
        (I::UaddSat | I::UsubSat, &[a, b]) => {
            unsigned_sat(cx, inst, which == I::UaddSat, bits, a, b)
        }
        (I::SshlSat | I::UshlSat, &[a, b]) => shl_sat(cx, inst, which == I::SshlSat, bits, a, b),
        (I::Fshl | I::Fshr, &[a, b, c]) => funnel(cx, inst, which == I::Fshl, bits, [a, b, c]),
        _ => cx.bug("a wide integer intrinsic with the wrong operands"),
    }
}

/// The population count of `r` in `r` (`t` written; flags written).
fn pop32(cx: &mut FnCx<'_, X86>, r: Reg, t: Reg) {
    put(cx, mc::mov_rr(Size::D, t, r));
    put(cx, mc::shr_ri(Size::D, t, 1));
    put(cx, mc::and_ri(Size::D, t, imm(0x5555_5555)));
    put(cx, mc::sub_rr(Size::D, r, t));
    put(cx, mc::mov_rr(Size::D, t, r));
    put(cx, mc::and_ri(Size::D, t, imm(0x3333_3333)));
    put(cx, mc::shr_ri(Size::D, r, 2));
    put(cx, mc::and_ri(Size::D, r, imm(0x3333_3333)));
    put(cx, mc::add_rr(Size::D, r, t));
    put(cx, mc::mov_rr(Size::D, t, r));
    put(cx, mc::shr_ri(Size::D, t, 4));
    put(cx, mc::add_rr(Size::D, r, t));
    put(cx, mc::and_ri(Size::D, r, imm(0x0f0f_0f0f)));
    put(cx, mc::imul_rri(Size::D, r, r, imm(0x0101_0101)));
    put(cx, mc::shr_ri(Size::D, r, 24));
}

/// The bits of `r` reversed, in `r` (`t` written; flags written).
fn rev32(cx: &mut FnCx<'_, X86>, r: Reg, t: Reg) {
    put(cx, mc::bswap(Size::D, r));
    for (s, m) in [(4, 0x0f0f_0f0f), (2, 0x3333_3333), (1, 0x5555_5555)] {
        put(cx, mc::mov_rr(Size::D, t, r));
        put(cx, mc::shr_ri(Size::D, t, s));
        put(cx, mc::and_ri(Size::D, t, imm(m)));
        put(cx, mc::and_ri(Size::D, r, imm(m)));
        put(cx, mc::shl_ri(Size::D, r, s));
        put(cx, mc::or_rr(Size::D, r, t));
    }
}

/// `edx:eax >>= k` for `k < 32` (`shrd`, `shr`); nothing for 0.
fn shr_pair(cx: &mut FnCx<'_, X86>, k: u32) {
    if k != 0 {
        put(cx, mc::shrd_rri(Size::D, EAX, EDX, k as u8));
        put(cx, mc::shr_ri(Size::D, EDX, k as u8));
    }
}

/// A count (`ctlz`, `cttz`, `ctpop`) in `ecx` or `eax` as the result: the
/// count in the low word, 0 in the high word.
fn st_count(cx: &mut FnCx<'_, X86>, r: FrameRef, count: Reg) {
    st(cx, count, r, 0);
    st_imm(cx, r, 1, 0);
}

/// `bswap`, `ctpop`, `ctlz`, `cttz`, `bitreverse`, `abs` of a wide `x`.
/// The counts branch (`bsr`/`bsf` of the word that decides); a reversal
/// of `N < 64` bits reverses all 64 and shifts right by `64 - N`, which
/// also drops the unspecified bits. `is_zero_poison` and
/// `is_int_min_poison` are ignored: the defined result is computed.
fn unary<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    bits: u32,
    x: Value,
) -> CgResult<()> {
    use Intrinsic as I;
    let a = cx.use_mem(x)?;
    let r = cx.def_mem(inst)?;
    let up = 64 - bits;
    match which {
        I::Bswap | I::BitReverse => {
            ld(cx, EAX, a, 1);
            ld(cx, EDX, a, 0);
            if which == I::Bswap {
                put(cx, mc::bswap(Size::D, EAX));
                put(cx, mc::bswap(Size::D, EDX));
            } else {
                rev32(cx, EAX, ECX);
                rev32(cx, EDX, ECX);
            }
            shr_pair(cx, up);
            st_pair(cx, r, EAX, EDX);
        }
        I::Ctpop => {
            ld(cx, EAX, a, 0);
            pop32(cx, EAX, ECX);
            ld_hi(cx, EDX, a, bits, Ext::Zero);
            pop32(cx, EDX, ECX);
            put(cx, mc::add_rr(Size::D, EAX, EDX));
            st_count(cx, r, EAX);
        }
        I::Ctlz => {
            // 64 for zero; the count of the high word if it is not zero,
            // else 32 plus the low word's; less the 64 - N bits above.
            let (low, done) = (cx.new_label(), cx.new_label());
            ld_hi(cx, EDX, a, bits, Ext::Zero);
            ld(cx, EAX, a, 0);
            mov_imm(cx, ECX, 64);
            put(cx, mc::test_rr(Size::D, EDX, EDX));
            branch_cond(cx, Cond::E, low)?;
            put(cx, mc::bsr_rr(Size::D, ECX, EDX));
            put(cx, mc::xor_ri(Size::D, ECX, 31));
            branch(cx, done)?;
            cx.bind(low)?;
            put(cx, mc::test_rr(Size::D, EAX, EAX));
            branch_cond(cx, Cond::E, done)?;
            put(cx, mc::bsr_rr(Size::D, ECX, EAX));
            put(cx, mc::xor_ri(Size::D, ECX, 31));
            put(cx, mc::add_ri(Size::D, ECX, 32));
            cx.bind(done)?;
            if up != 0 {
                put(cx, mc::sub_ri(Size::D, ECX, imm(up)));
            }
            st_count(cx, r, ECX);
        }
        I::Cttz => {
            // The low word's count, else 32 plus the high word's, whose
            // bit N - 32 is set first (N for zero; 64 for a zero i64).
            let (high, done) = (cx.new_label(), cx.new_label());
            ld(cx, EAX, a, 0);
            put(cx, mc::test_rr(Size::D, EAX, EAX));
            branch_cond(cx, Cond::E, high)?;
            put(cx, mc::bsf_rr(Size::D, ECX, EAX));
            branch(cx, done)?;
            cx.bind(high)?;
            ld(cx, EDX, a, 1);
            if bits < 64 {
                put(cx, mc::or_ri(Size::D, EDX, imm(1 << (bits - 32))));
            }
            mov_imm(cx, ECX, 64);
            put(cx, mc::test_rr(Size::D, EDX, EDX));
            branch_cond(cx, Cond::E, done)?;
            put(cx, mc::bsf_rr(Size::D, ECX, EDX));
            put(cx, mc::add_ri(Size::D, ECX, 32));
            cx.bind(done)?;
            st_count(cx, r, ECX);
        }
        I::Abs => {
            // (x ^ s) - s with s the sign: the minimum maps to itself.
            ld_hi(cx, EDX, a, bits, Ext::Sign);
            ld(cx, EAX, a, 0);
            put(cx, mc::mov_rr(Size::D, ECX, EDX));
            put(cx, mc::sar_ri(Size::D, ECX, 31));
            put(cx, mc::xor_rr(Size::D, EAX, ECX));
            put(cx, mc::xor_rr(Size::D, EDX, ECX));
            put(cx, mc::sub_rr(Size::D, EAX, ECX));
            put(cx, mc::sbb_rr(Size::D, EDX, ECX));
            st_pair(cx, r, EAX, EDX);
        }
        _ => return cx.bug("a wide unary intrinsic without a lowering"),
    }
    Ok(())
}

/// `smax`, `smin`, `umax`, `umin`: the compare of the two, and a copy of
/// the chosen operand on each side of a branch (as `select`).
fn min_max<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    bits: u32,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    let pred = match which {
        Intrinsic::Smax => IntPredicate::Sge,
        Intrinsic::Smin => IntPredicate::Sle,
        Intrinsic::Umax => IntPredicate::Uge,
        _ => IntPredicate::Ule,
    };
    let (a, b) = (cx.use_mem(lhs)?, cx.use_mem(rhs)?);
    let r = cx.def_mem(inst)?;
    let cond = compare_slots(cx, pred, bits, a, b);
    let (take_a, done) = (cx.new_label(), cx.new_label());
    branch_cond(cx, cond, take_a)?;
    copy(cx, r, b);
    branch(cx, done)?;
    cx.bind(take_a)?;
    copy(cx, r, a);
    cx.bind(done)?;
    Ok(())
}

/// `%r = call iM @llvm.scmp/ucmp(iN %a, iN %b)`, `33 <= N <= 64`, `M`
/// of 2 to 64 bits: `a > b` and then `a < b` by [`compare_slots`], each
/// `setcc`, the first kept on the stack while the second is computed;
/// the result their difference (1, 0 or -1, sign-extended to the high
/// word of a wide result). No branch (as `llc`).
///
/// # Errors
/// [`WIDE`] for operands or a result wider than 64 bits; what the
/// operands report.
fn three_way<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    signed: bool,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    let bits = bits_of(cx, value_ty(cx, lhs))?;
    let result = cx.types().int_bits(inst.ty()).unwrap_or(0);
    if result > 64 {
        return unsupported(WIDE);
    }
    let (gt, lt) = if signed {
        (IntPredicate::Sgt, IntPredicate::Slt)
    } else {
        (IntPredicate::Ugt, IntPredicate::Ult)
    };
    let (a, b) = (cx.use_mem(lhs)?, cx.use_mem(rhs)?);
    let cond = compare_slots(cx, gt, bits, a, b);
    put(cx, mc::setcc_r(cond, EAX));
    put(cx, mc::movzx_rr(Size::D, EAX, Size::B, EAX));
    put(cx, mc::push_r(EAX));
    let cond = compare_slots(cx, lt, bits, a, b);
    put(cx, mc::setcc_r(cond, ECX));
    put(cx, mc::movzx_rr(Size::D, ECX, Size::B, ECX));
    put(cx, mc::pop_r(EAX));
    put(cx, mc::sub_rr(Size::D, EAX, ECX));
    if result <= 32 {
        let rd = cx.def_reg(inst)?;
        put(cx, mc::mov_rr(Size::D, gp(rd), EAX));
        return cx.finish(inst, rd);
    }
    let r = cx.def_mem(inst)?;
    st(cx, EAX, r, 0);
    put(cx, mc::sar_ri(Size::D, EAX, 31));
    st(cx, EAX, r, 1);
    Ok(())
}

/// The signed bound of `N` bits on the side of the sign mask in `edx`
/// (0: the maximum, -1: the minimum), into `edx:eax` (the high word
/// sign-extended).
fn signed_bound(cx: &mut FnCx<'_, X86>, bits: u32) {
    put(cx, mc::mov_rr(Size::D, EAX, EDX));
    put(cx, mc::not_r(Size::D, EAX));
    put(cx, mc::xor_ri(Size::D, EDX, imm(word_mask(bits - 33))));
}

/// `sadd.sat`, `ssub.sat`: of 64 bits, `add`/`adc` (`sub`/`sbb`) and on
/// overflow the bound of the true result's sign (the opposite of the
/// wrapped sign); of fewer, the operands sign-extended (no 64-bit
/// overflow is possible) and the bound when the sum does not fit `N` bits
/// (its high word differs from itself sign-extended from `N - 32` bits).
fn signed_sat<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    add: bool,
    bits: u32,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    let (a, b) = (cx.use_mem(lhs)?, cx.use_mem(rhs)?);
    let r = cx.def_mem(inst)?;
    ld_hi(cx, ECX, b, bits, Ext::Sign);
    ld_hi(cx, EDX, a, bits, Ext::Sign);
    ld(cx, EAX, a, 0);
    if add {
        op_m(cx, b, 0, |m| mc::add_rm(Size::D, EAX, m));
        put(cx, mc::adc_rr(Size::D, EDX, ECX));
    } else {
        op_m(cx, b, 0, |m| mc::sub_rm(Size::D, EAX, m));
        put(cx, mc::sbb_rr(Size::D, EDX, ECX));
    }
    let done = cx.new_label();
    if bits == 64 {
        branch_cond(cx, Cond::No, done)?;
        put(cx, mc::sar_ri(Size::D, EDX, 31));
        put(cx, mc::not_r(Size::D, EDX));
    } else {
        put(cx, mc::mov_rr(Size::D, ECX, EDX));
        alu::extend_to(cx, ECX, bits - 32, Ext::Sign);
        put(cx, mc::cmp_rr(Size::D, ECX, EDX));
        branch_cond(cx, Cond::E, done)?;
        put(cx, mc::sar_ri(Size::D, EDX, 31));
    }
    signed_bound(cx, bits);
    cx.bind(done)?;
    st_pair(cx, r, EAX, EDX);
    Ok(())
}

/// `uadd.sat`, `usub.sat`: the operands zero-extended, `add`/`adc`
/// (`sub`/`sbb`); the bound (all ones, or 0) on a carry out of 64 bits, or
/// for an add of fewer bits when bit `N` of the sum is set.
fn unsigned_sat<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    add: bool,
    bits: u32,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    let (a, b) = (cx.use_mem(lhs)?, cx.use_mem(rhs)?);
    let r = cx.def_mem(inst)?;
    ld_hi(cx, ECX, b, bits, Ext::Zero);
    ld_hi(cx, EDX, a, bits, Ext::Zero);
    ld(cx, EAX, a, 0);
    if add {
        op_m(cx, b, 0, |m| mc::add_rm(Size::D, EAX, m));
        put(cx, mc::adc_rr(Size::D, EDX, ECX));
    } else {
        op_m(cx, b, 0, |m| mc::sub_rm(Size::D, EAX, m));
        put(cx, mc::sbb_rr(Size::D, EDX, ECX));
    }
    let done = cx.new_label();
    if add && bits < 64 {
        put(cx, mc::test_ri(Size::D, EDX, imm(1 << (bits - 32))));
        branch_cond(cx, Cond::E, done)?;
    } else {
        branch_cond(cx, Cond::Ae, done)?;
    }
    let bound = if add { u32::MAX } else { 0 };
    mov_imm(cx, EAX, bound);
    mov_imm(cx, EDX, bound);
    cx.bind(done)?;
    st_pair(cx, r, EAX, EDX);
    Ok(())
}

/// `edx:eax <<= cl` (`shl`) or `>>= cl` (`lshr`, `ashr` by `signed`), for
/// `cl < 64`: the pair instruction and the shift (which count modulo 32),
/// then `test cl, 32` and the word move.
fn shift_pair_cl(cx: &mut FnCx<'_, X86>, left: bool, signed: bool) -> CgResult<()> {
    let done = cx.new_label();
    if left {
        put(cx, mc::shld_rrcl(Size::D, EDX, EAX));
        put(cx, mc::shl_rcl(Size::D, EAX));
    } else {
        put(cx, mc::shrd_rrcl(Size::D, EAX, EDX));
        put(cx, if signed { mc::sar_rcl(Size::D, EDX) } else { mc::shr_rcl(Size::D, EDX) });
    }
    put(cx, mc::test_ri(Size::B, ECX, 32));
    branch_cond(cx, Cond::E, done)?;
    if left {
        put(cx, mc::mov_rr(Size::D, EDX, EAX));
        mov_imm(cx, EAX, 0);
    } else {
        put(cx, mc::mov_rr(Size::D, EAX, EDX));
        if signed {
            put(cx, mc::sar_ri(Size::D, EDX, 31));
        } else {
            mov_imm(cx, EDX, 0);
        }
    }
    cx.bind(done)
}

/// `sshl.sat`, `ushl.sat` by `k < N` (more is poison): the value moved to
/// the top of 64 bits (`x << (64 - N)`, which drops the unspecified bits),
/// shifted left by `k` and back right; when that differs from the moved
/// value, the bound (of the value's sign, or all ones), else the shifted
/// value moved back down. The moved value lives in a frame slot of its
/// own; the shifted one in the result's (every operand is read first).
fn shl_sat<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    signed: bool,
    bits: u32,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    let up = 64 - bits;
    let (a, k) = (cx.use_mem(lhs)?, cx.use_mem(rhs)?);
    let top = cx.alloc_slot(8, 4)?;
    let r = cx.def_mem(inst)?;
    ld(cx, ECX, k, 0);
    ld(cx, EAX, a, 0);
    ld(cx, EDX, a, 1);
    if up != 0 {
        put(cx, mc::shld_rri(Size::D, EDX, EAX, up as u8));
        put(cx, mc::shl_ri(Size::D, EAX, up as u8));
    }
    st_pair(cx, top, EAX, EDX);
    shift_pair_cl(cx, true, signed)?;
    st_pair(cx, r, EAX, EDX);
    shift_pair_cl(cx, false, signed)?;
    op_m(cx, top, 0, |m| mc::xor_rm(Size::D, EAX, m));
    op_m(cx, top, 1, |m| mc::xor_rm(Size::D, EDX, m));
    put(cx, mc::or_rr(Size::D, EAX, EDX));
    let (bound, done) = (cx.new_label(), cx.new_label());
    branch_cond(cx, Cond::Ne, bound)?;
    ld(cx, EAX, r, 0);
    ld(cx, EDX, r, 1);
    shr_pair(cx, up);
    st_pair(cx, r, EAX, EDX);
    branch(cx, done)?;
    cx.bind(bound)?;
    if signed {
        ld(cx, EDX, top, 1);
        put(cx, mc::sar_ri(Size::D, EDX, 31));
        signed_bound(cx, bits);
    } else {
        mov_imm(cx, EAX, u32::MAX);
        mov_imm(cx, EDX, u32::MAX);
    }
    st_pair(cx, r, EAX, EDX);
    cx.bind(done)
}

/// A word of a funnel shift's window: word `.1` of slot `.0`.
type Word = (FrameRef, u32);

/// One window of a funnel shift `[x, y, z]` by the count (`cl`, or the
/// immediate `k < 32`): `first = op(x, y)` into word 2 of `scratch`,
/// `second = op(y, z)` in `edx`, `op` being `shld` (`left`) or `shrd`.
fn window(
    cx: &mut FnCx<'_, X86>,
    left: bool,
    k: Option<u32>,
    [x, y, z]: [Word; 3],
    scratch: FrameRef,
) {
    let op = |cx: &mut FnCx<'_, X86>, dst: Reg, src: Reg| match (left, k) {
        (true, None) => put(cx, mc::shld_rrcl(Size::D, dst, src)),
        (false, None) => put(cx, mc::shrd_rrcl(Size::D, dst, src)),
        (_, Some(0)) => {}
        (true, Some(k)) => put(cx, mc::shld_rri(Size::D, dst, src, k as u8)),
        (false, Some(k)) => put(cx, mc::shrd_rri(Size::D, dst, src, k as u8)),
    };
    ld(cx, EAX, x.0, x.1);
    ld(cx, EDX, y.0, y.1);
    op(cx, EAX, EDX);
    st(cx, EAX, scratch, 2);
    ld(cx, EAX, z.0, z.1);
    op(cx, EDX, EAX);
}

/// `fshl`, `fshr` (`left`) of `N` bits by `c mod N`: `b` moved to the top
/// of 64 bits (`B = b << (64 - N)`, kept in a frame slot of its own), then
/// the 64-bit funnel shift of `a:B` by `k < N`, `fshr`'s result moved back
/// down by `64 - N`. The 64-bit shift picks three words of `a:B` by bit 5
/// of `k` (at compile time for a constant amount, else `test cl, 32` and
/// a branch) and shifts them by `k mod 32` with `shld`/`shrd`. A variable
/// amount of fewer than 64 bits is reduced modulo `N` by two `div`s
/// (`edx:eax / N` of the high word, then of the low word).
#[allow(clippy::many_single_char_names, reason = "the operands are LangRef's a, b, c")]
fn funnel<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    left: bool,
    bits: u32,
    [a, b, c]: [Value; 3],
) -> CgResult<()> {
    let up = 64 - bits;
    let all = u64::MAX >> up;
    let amount = const_u64(cx, c).map(|k| ((k & all) % u64::from(bits)) as u32);
    let (sa, sb) = (cx.use_mem(a)?, cx.use_mem(b)?);
    let sc = match amount {
        Some(_) => None,
        None => Some(cx.use_mem(c)?),
    };
    let s = cx.alloc_slot(12, 4)?;
    let r = cx.def_mem(inst)?;
    ld(cx, EAX, sb, 0);
    ld(cx, EDX, sb, 1);
    if up != 0 {
        put(cx, mc::shld_rri(Size::D, EDX, EAX, up as u8));
        put(cx, mc::shl_ri(Size::D, EAX, up as u8));
    }
    st_pair(cx, s, EAX, EDX);
    let (a0, a1, b0, b1) = ((sa, 0), (sa, 1), (s, 0), (s, 1));
    // [x, y, z] for k < 32 and for k >= 32.
    let (small, large) =
        if left { ([a1, a0, b1], [a0, b1, b0]) } else { ([b0, b1, a0], [b1, a0, a1]) };
    match (amount, sc) {
        (Some(k), _) => window(cx, left, Some(k % 32), if k < 32 { small } else { large }, s),
        (None, Some(sc)) => {
            if bits == 64 {
                ld(cx, ECX, sc, 0);
            } else {
                ld_hi(cx, EAX, sc, bits, Ext::Zero);
                put(cx, mc::xor_rr(Size::D, EDX, EDX));
                mov_imm(cx, ECX, bits);
                put(cx, mc::div_r(Size::D, ECX));
                ld(cx, EAX, sc, 0);
                put(cx, mc::div_r(Size::D, ECX));
                put(cx, mc::mov_rr(Size::D, ECX, EDX));
            }
            let (big, join) = (cx.new_label(), cx.new_label());
            put(cx, mc::test_ri(Size::B, ECX, 32));
            branch_cond(cx, Cond::Ne, big)?;
            window(cx, left, None, small, s);
            branch(cx, join)?;
            cx.bind(big)?;
            window(cx, left, None, large, s);
            cx.bind(join)?;
        }
        (None, None) => return cx.bug("a funnel shift without an amount"),
    }
    if left {
        // hi = first, lo = second.
        ld(cx, EAX, s, 2);
        st_pair(cx, r, EDX, EAX);
    } else {
        // lo = first, hi = second.
        ld(cx, EAX, s, 2);
        shr_pair(cx, up);
        st_pair(cx, r, EAX, EDX);
    }
    Ok(())
}

// ---- switch, ptradd index (WP-306) ----

/// `switch` on an `i33`..`i64`: the low word in `eax`, the high word
/// zero-extended in `edx`; per case `cmp eax, lo`·`jne next`·`cmp edx,
/// hi`·`je case`; then the default.
///
/// # Errors
/// [`WIDE`]; what the operand and the branches report.
pub fn switch<'a>(cx: &mut FnCx<'a, X86>, switch: &Switch<'a>) -> CgResult<()> {
    if wide::wide_bits(cx, value_ty(cx, switch.cond)).is_some() {
        return unsupported(wide::SWITCH);
    }
    let bits = bits_of(cx, value_ty(cx, switch.cond))?;
    let all = u64::MAX >> (64 - bits);
    let case_value = |cx: &FnCx<'a, X86>, id| match cx.module.consts.get(id) {
        Const::Int { value, .. } => value & all,
        _ => 0,
    };
    if let Some(c) = const_u64(cx, switch.cond) {
        let target = switch
            .cases()
            .find(|&(case, _)| case_value(cx, case) == c & all)
            .map_or(switch.default, |(_, block)| block);
        return crate::control::folded_br(cx, target);
    }
    let a = cx.use_mem(switch.cond)?;
    ld_hi(cx, EDX, a, bits, Ext::Zero);
    ld(cx, EAX, a, 0);
    for (case, block) in switch.cases() {
        let value = case_value(cx, case);
        let next = cx.new_label();
        put(cx, mc::cmp_ri(Size::D, EAX, imm(value as u32)));
        branch_cond(cx, Cond::Ne, next)?;
        put(cx, mc::cmp_ri(Size::D, EDX, imm((value >> 32) as u32)));
        let label = cx.block_label(block);
        branch_cond(cx, Cond::E, label)?;
        cx.bind(next)?;
    }
    crate::control::br(cx, switch.default)
}

/// Whether `index` is a `ptradd` index of this module (`i33`..`i64`).
///
/// # Errors
/// [`WIDE`] for a wider index.
pub fn index_ok(cx: &FnCx<'_, X86>, index: Value) -> CgResult<bool> {
    if !is_wide(cx, index) {
        return Ok(false);
    }
    match cx.types().int_bits(value_ty(cx, index)) {
        // The low limb, as for 33 to 64 bits.
        Some(65..=128) => Ok(true),
        Some(129..) => unsupported(wide::WIDTH),
        _ => bits_of(cx, value_ty(cx, index)).map(|_| true),
    }
}

/// A `ptradd` index into `r`: a wide one's low word (the index truncated
/// to the pointer's 32 bits, as `llc`), any other as `FnCx::put`.
///
/// # Errors
/// What the operand reports.
pub fn put_index(cx: &mut FnCx<'_, X86>, index: Value, r: hod_cg::PReg) -> CgResult<()> {
    if !is_wide(cx, index) {
        return cx.put(index, r);
    }
    let slot = cx.use_mem(index)?;
    ld(cx, gp(r), slot, 0);
    Ok(())
}

// ---- memory ----

/// The address of `ptr` in a register: a pool register for a value, `eax`
/// for a stack slot's or a symbol's address.
pub(crate) fn pointer(cx: &mut FnCx<'_, X86>, ptr: Value) -> CgResult<Reg> {
    match ptr.kind() {
        ValueKind::Slot(slot) => {
            slot_address(cx, EAX, slot)?;
            Ok(EAX)
        }
        ValueKind::Sym(sym)
            if !matches!(
                hod_cg::sym_access(cx.module, cx.options, sym),
                hod_cg::Access::Tls(_)
            ) =>
        {
            sym_address(cx, EAX, sym, 0)?;
            Ok(EAX)
        }
        _ => Ok(gp(cx.use_reg(ptr)?)),
    }
}

/// The store size in bytes of the wide integer type `ty` (5 to 8).
fn store_bytes(cx: &FnCx<'_, X86>, ty: TypeId) -> CgResult<u32> {
    let bits = bits_of(cx, ty)?;
    Ok(bits.div_ceil(8))
}

/// Rejects an ordered access.
fn check(mem: MemFlags) -> CgResult<()> {
    if mem.ordering().is_some() { unsupported(crate::mem::ATOMIC) } else { Ok(()) }
}

/// `%r = load iN, ptr %ptr`: the low word, then the rest of the store
/// size (1 to 4 bytes), each through `ecx` into the slot.
///
/// # Errors
/// [`WIDE`], `x86:atomic`; what the operands report.
pub fn load<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, ptr: Value, mem: MemFlags) -> CgResult<()> {
    check(mem)?;
    if wide::wide_bits(cx, inst.ty()).is_some() {
        return wide::load(cx, inst, ptr);
    }
    let bytes = store_bytes(cx, inst.ty())?;
    let p = pointer(cx, ptr)?;
    let r = cx.def_mem(inst)?;
    load_at(cx, ECX, 4, EDX, |k| Mem::base(p, k));
    st(cx, ECX, r, 0);
    load_at(cx, ECX, bytes - 4, EDX, |k| Mem::base(p, 4 + k));
    st(cx, ECX, r, 1);
    Ok(())
}

/// `store iN %value, ptr %ptr`: the low word, then the high word
/// zero-extended from the width, exactly the store size.
///
/// # Errors
/// [`WIDE`], `x86:atomic`; what the operands report.
pub fn store<'a>(
    cx: &mut FnCx<'a, X86>,
    _inst: Inst<'a>,
    value: Value,
    ptr: Value,
    mem: MemFlags,
) -> CgResult<()> {
    check(mem)?;
    let ty = value_ty(cx, value);
    if wide::wide_bits(cx, ty).is_some() {
        return wide::store(cx, value, ptr);
    }
    let bits = bits_of(cx, ty)?;
    let bytes = store_bytes(cx, ty)?;
    let v = cx.use_mem(value)?;
    let p = pointer(cx, ptr)?;
    ld(cx, ECX, v, 0);
    store_at(cx, ECX, 4, EDX, |k| Mem::base(p, k));
    ld_hi(cx, ECX, v, bits, Ext::Zero);
    store_at(cx, ECX, bytes - 4, EDX, |k| Mem::base(p, 4 + k));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_stable() {
        assert_eq!(I64, "x86:i64");
        assert_eq!(WIDE, "x86:i64:wide");
        assert_eq!(CAST, alu::CAST);
    }
}
