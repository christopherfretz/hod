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
//! Refused as [`WIDE`]; nothing below generalizes to more words for free.
//!
//! # Division
//!
//! `udiv`, `sdiv`, `urem`, `srem` call libgcc's `__udivdi3`, `__divdi3`,
//! `__umoddi3`, `__moddi3` directly: the divisor's halves then the
//! dividend's are pushed (high word first), `call`, `add esp, 16`, the
//! result in `edx:eax`. The 16 bytes keep `esp` 16-aligned at the call.

use hod_cg::{CallTarget, CgResult, FnCx, FrameRef, Label, RuntimeFn, SymRef};
use hod_ir::{Const, Inst, IntPredicate, MemFlags, Opcode, TypeId, Value, ValueKind};
use hod_mc_x64 as mc;
use hod_mc_x64::{Cond, Mem, Reg, Size};

use crate::alu::{self, Ext};
use crate::common::{
    EAX, ECX, EDX, ESP, branch, branch_cond, frame_inst, gp, load_at, put, slot_address, store_at,
    sym_address, unsupported, value_ty,
};
use crate::target::X86;

/// An encoder of `op r32, m32`.
type RegMem = fn(Size, Reg, Mem) -> mc::Inst;

/// An `i64` passed or returned by value, or the condition of a `switch`:
/// what remains of WP-293's refusal until I6 (`abi.rs`) and the switch
/// take 64-bit values.
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
    let ext = if signed(pred) { Ext::Sign } else { Ext::Zero };
    let (a, b) = (cx.use_mem(lhs)?, cx.use_mem(rhs)?);
    if matches!(pred, IntPredicate::Eq | IntPredicate::Ne) {
        ld_hi(cx, ECX, a, bits, ext);
        ld_hi(cx, EDX, b, bits, ext);
        ld(cx, EAX, a, 0);
        op_m(cx, b, 0, |m| mc::xor_rm(Size::D, EAX, m));
        put(cx, mc::xor_rr(Size::D, ECX, EDX));
        put(cx, mc::or_rr(Size::D, EAX, ECX));
        return Ok(alu::cond_of(pred));
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
    Ok(cond)
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

/// The address of `ptr` in a register: a pool register for a value, `eax`
/// for a stack slot's or a symbol's address.
fn pointer(cx: &mut FnCx<'_, X86>, ptr: Value) -> CgResult<Reg> {
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
