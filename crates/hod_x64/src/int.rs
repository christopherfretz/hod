//! Integer instructions of class `Gpr` (`spec/codegen.md` §2.2, §5.6):
//! binary operations, `freeze`, integer and pointer casts, `icmp`,
//! `select`. **Owner: WP-122** (X-int). Tests: `tests/int.rs`.
//!
//! Every entry point starts with
//! [`deferred_first`](crate::common::deferred_first), naming the operand
//! it fuses. The decisions that are not instruction selection (what is
//! extended, what is undefined, what is fused) are `hod_a64::int`'s.
//!
//! # Two operands (§5.6, design 0006 §2.2)
//!
//! `FnCx::use_reg` hands every operand over in a register the lowering
//! may overwrite (a register value dies at its one use; anything else is
//! loaded into a temporary), and two operands are never the same
//! register. So `dst = a op b` is computed in `a`'s register, `op a, b`,
//! and the result stays there: the destination is never the right
//! operand, for a non-commutative operation or any other. A constant
//! left operand of a commutative operation is swapped to the right, where
//! an immediate takes it.
//!
//! # Narrow values (§2.2)
//!
//! A value of `iN` lives in a 32-bit register for `N <= 32` and a 64-bit
//! one above, with the bits above `N` unspecified, and is computed at
//! that register width. Operations whose low `N` result bits depend only
//! on the low `N` operand bits (`add`, `sub`, `mul`, `and`, `or`, `xor`,
//! `shl`, `trunc`) compute in the register as it is; the others extend
//! first: `lshr`, `udiv`, `urem` and unsigned compares by zero; `ashr`,
//! `sdiv`, `srem` and signed compares by sign. An extension is `movzx` or
//! `movsx` for 8 and 16 bits, `mov r32, r32` or `movsxd` for 32 bits into
//! 64, `and r32, mask` for another width below 32 by zero, and a `shl`/
//! `shr` or `shl`/`sar` pair otherwise. A register shift amount of a
//! type narrower than five bits is masked: `shl r32, cl` reads five bits
//! of `cl`. Shift amounts of the width or more, division by zero and the
//! smallest value divided by -1 are undefined (§4.7): whatever the
//! instruction does (`div` traps) is correct, and a constant shift amount
//! of the width or more emits nothing.
//!
//! # Fixed registers
//!
//! `rax`, `rcx` and `rdx` never hold a value (§5.6): a division puts the
//! dividend in `rax` (`rdx` zeroed, or `cdq`/`cqo`), a register shift
//! puts the amount in `rcx`, without asking the policy. `r11` takes a
//! constant no immediate field holds.
//!
//! # Compare fusion (§4.6, §5.6 "Flags")
//!
//! An `icmp` whose result may be deferred is deferred. Its consumer then
//! either asks for the value (the `icmp` is emitted as `cmp` + `setcc` +
//! `movzx`) or fuses it: [`fusable`] says whether a value is such a
//! deferred integer compare, and [`flags`] emits the compare into the
//! flags and returns the condition. This module fuses into `select`
//! (`cmovcc`), `zext` (`setcc` + `movzx`) and `sext` (`setcc` + `movzx` +
//! `neg`). Between the compare and its reader only the glue of `common`
//! runs (`mov`, `lea`, `movzx`): it keeps the flags.
//!
//! # `select`
//!
//! `cmovcc` at the register width: the 32-bit form for every width up to
//! 32 bits (there is no 8-bit `cmov`, and the 32-bit one gives the same
//! low bits), the 64-bit form above. No `select` is a branch.

use hod_cg::{CgResult, Class, FnCx, PReg};
use hod_ir::{Const, Inst, InstKind, IntPredicate, Opcode, Value};
use hod_mc_x64 as mc;
use hod_mc_x64::{Cond, Reg, Size};

use crate::common::{R11, deferred_first, gp, mov_imm, put, unsupported};
use crate::reg::{RAX, RCX};
use crate::target::X64;

/// A binary operation the dispatch should never send here (a
/// floating-point opcode).
pub const BINARY: &str = "x64:int-binary";
/// A cast between `Gpr` types that is no integer or pointer cast (the
/// dispatch sends none).
pub const CAST: &str = "x64:int-cast";
/// `addrspacecast`: `classify` rejects every non-zero address space, and
/// within one address space LLVM has no such cast.
pub const ADDRSPACECAST: &str = "x64:addrspacecast";

// ---- widths and operands ----

/// How an operand's low bits are extended to its register.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ext {
    /// Left as it is: the bits above the width stay unspecified.
    None,
    /// By zero.
    Zero,
    /// By sign.
    Sign,
}

/// The width of a `Gpr`-class value: an integer's, 64 for a pointer.
#[inline]
fn width(cx: &FnCx<'_, X64>, value: Value) -> u32 {
    cx.types().int_bits(cx.body.value_type(cx.module, value)).unwrap_or(64)
}

/// The width of the register a value of `bits` bits is computed in.
#[inline]
fn reg_bits(bits: u32) -> u32 {
    if bits <= 32 { 32 } else { 64 }
}

/// The operand size a value of `bits` bits is computed at: 32 or 64
/// bits.
#[inline]
pub fn size_for_bits(bits: u32) -> Size {
    if bits <= 32 { Size::D } else { Size::Q }
}

/// The mask of the low `bits` bits.
#[inline]
fn mask(bits: u32) -> u64 {
    if bits >= 64 { u64::MAX } else { (1 << bits) - 1 }
}

/// `value` sign-extended from its low `bits` bits.
#[inline]
fn sign_extend(value: u64, bits: u32) -> u64 {
    if bits >= 64 || value >> (bits - 1) & 1 == 0 {
        value & mask(bits)
    } else {
        value | !mask(bits)
    }
}

/// The integer `value` is, zero-extended from its width, when it is an
/// integer constant (or a zero-like one).
fn const_int(cx: &FnCx<'_, X64>, value: Value) -> Option<u64> {
    let id = value.as_const()?;
    match cx.module.consts.get(id) {
        Const::Int { value, .. } => Some(value),
        Const::Null(_) | Const::Zero(_) | Const::Undef(_) | Const::Poison(_) => Some(0),
        _ => None,
    }
}

/// `value` (zero-extended from `bits`) as its register holds it after
/// extension `ext`: sign-extended for [`Ext::Sign`], then cut to the
/// register's width.
fn ext_const(value: u64, bits: u32, ext: Ext) -> u64 {
    let wide = if ext == Ext::Sign { sign_extend(value, bits) } else { value };
    wide & mask(reg_bits(bits))
}

/// The immediate field of an instruction of `size` that holds `value` (a
/// register's bits), if one does: any 32-bit value at 32 bits, a
/// sign-extended 32-bit one at 64.
#[inline]
fn imm32(value: u64, size: Size) -> Option<i64> {
    match size {
        Size::Q => i32::try_from(value as i64).ok().map(i64::from),
        _ => Some(i64::from(value as u32 as i32)),
    }
}

/// Extends the low `bits` bits of `r` in place to the whole register of
/// `size` (32 or 64 bits); nothing when `bits` fills it or for
/// [`Ext::None`]. `movzx`/`movsx` for 8 and 16 bits, `mov r32, r32` /
/// `movsxd` for 32 into 64, `and r32, mask` by zero below 32 bits, a
/// `shl`/`shr` or `shl`/`sar` pair otherwise. The last two write the
/// flags: never between a compare and its reader.
///
/// For a later group (lanes, conversions) that needs an integer
/// extended.
pub fn extend_to(cx: &mut FnCx<'_, X64>, r: Reg, bits: u32, ext: Ext, size: Size) {
    let full = if size == Size::Q { 64 } else { 32 };
    if bits >= full || ext == Ext::None {
        return;
    }
    let zero = ext == Ext::Zero;
    let inst = match bits {
        8 | 16 => {
            let from = if bits == 8 { Size::B } else { Size::W };
            if zero {
                // The 32-bit `movzx` zeroes bits 32..64 too.
                mc::movzx_rr(Size::D, r, from, r)
            } else {
                mc::movsx_rr(size, r, from, r)
            }
        }
        32 if zero => mc::mov_rr(Size::D, r, r),
        32 => mc::movsxd_rr(r, r),
        _ if zero && bits < 32 => mc::and_ri(Size::D, r, mask(bits) as i64),
        _ => {
            let s = (full - bits) as u8;
            put(cx, mc::shl_ri(size, r, s));
            if zero { mc::shr_ri(size, r, s) } else { mc::sar_ri(size, r, s) }
        }
    };
    put(cx, inst);
}

/// `value` of `bits` bits in a pinned register (which the lowering may
/// overwrite and leave its result in), extended by `ext`. An integer
/// constant is materialized already extended.
fn operand(cx: &mut FnCx<'_, X64>, value: Value, bits: u32, ext: Ext) -> CgResult<PReg> {
    if let Some(c) = const_int(cx, value) {
        let r = cx.temp(Class::Gpr)?;
        mov_imm(cx, gp(r), ext_const(c, bits, ext), reg_bits(bits));
        return Ok(r);
    }
    let r = cx.use_reg(value)?;
    extend_to(cx, gp(r), bits, ext, size_for_bits(bits));
    Ok(r)
}

/// As [`operand`], but an integer constant goes to `scratch` and pins
/// nothing: for an operand the result does not stay in. The pinned
/// register, if any, comes back too.
fn source(
    cx: &mut FnCx<'_, X64>,
    value: Value,
    bits: u32,
    ext: Ext,
    scratch: Reg,
) -> CgResult<(Reg, Option<PReg>)> {
    if let Some(c) = const_int(cx, value) {
        mov_imm(cx, scratch, ext_const(c, bits, ext), reg_bits(bits));
        return Ok((scratch, None));
    }
    let r = operand(cx, value, bits, ext)?;
    Ok((gp(r), Some(r)))
}

// ---- binary operations ----

/// Whether `op` commutes.
#[inline]
fn commutes(op: Opcode) -> bool {
    matches!(op, Opcode::Add | Opcode::Mul | Opcode::And | Opcode::Or | Opcode::Xor)
}

/// How the left operand of `op` is extended before the operation.
fn lhs_ext(op: Opcode) -> Ext {
    match op {
        Opcode::LShr | Opcode::UDiv | Opcode::URem => Ext::Zero,
        Opcode::AShr | Opcode::SDiv | Opcode::SRem => Ext::Sign,
        _ => Ext::None,
    }
}

/// `%r = <op> %lhs, %rhs`: computed into the left operand's register
/// (a division through `rax`/`rdx`, a register shift through `cl`).
///
/// # Errors
/// [`BINARY`] for a floating-point opcode (the dispatch sends none);
/// what the operands' placement reports.
pub fn binary<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, lhs: Value, rhs: Value) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    let op = inst.opcode();
    if !op.is_int_binary() {
        return unsupported(BINARY);
    }
    let bits = width(cx, inst.result());
    let (lhs, rhs) = if commutes(op) && const_int(cx, lhs).is_some() && const_int(cx, rhs).is_none()
    {
        (rhs, lhs)
    } else {
        (lhs, rhs)
    };
    let konst = const_int(cx, rhs);
    match op {
        Opcode::UDiv | Opcode::SDiv | Opcode::URem | Opcode::SRem => {
            return divide(cx, inst, op, lhs, rhs, bits);
        }
        Opcode::Shl | Opcode::LShr | Opcode::AShr if konst.is_none() => {
            return shift_by_cl(cx, inst, op, lhs, rhs, bits);
        }
        _ => {}
    }
    let fold_shift = matches!(op, Opcode::LShr | Opcode::AShr);
    let result = operand(cx, lhs, bits, if fold_shift { Ext::None } else { lhs_ext(op) })?;
    let ra = gp(result);
    let sz = size_for_bits(bits);
    match (op, konst) {
        (Opcode::Shl | Opcode::LShr | Opcode::AShr, Some(amount)) => {
            shift_by_imm(cx, op, ra, amount, bits);
        }
        (_, Some(c)) => alu_imm(cx, op, ra, c, bits),
        (_, None) => {
            let (rb, _) = source(cx, rhs, bits, Ext::None, R11)?;
            put(cx, alu_rr(op, sz, ra, rb));
        }
    }
    cx.finish(inst, result)
}

/// `ra = ra <op> rb` for `add`, `sub`, `mul`, `and`, `or`, `xor`.
fn alu_rr(op: Opcode, sz: Size, ra: Reg, rb: Reg) -> mc::Inst {
    match op {
        Opcode::Add => mc::add_rr(sz, ra, rb),
        Opcode::Sub => mc::sub_rr(sz, ra, rb),
        Opcode::Mul => mc::imul_rr(sz, ra, rb),
        Opcode::And => mc::and_rr(sz, ra, rb),
        Opcode::Or => mc::or_rr(sz, ra, rb),
        _ => mc::xor_rr(sz, ra, rb),
    }
}

/// `ra = ra <op> c` for `add`, `sub`, `mul`, `and`, `or`, `xor` with a
/// constant (zero-extended from `bits`): nothing for an identity, `not`
/// for `xor` with all ones, the immediate form when the constant
/// sign-extended from its width fits one (only the low `bits` bits of
/// the result count), else through `r11`.
fn alu_imm(cx: &mut FnCx<'_, X64>, op: Opcode, ra: Reg, c: u64, bits: u32) {
    let sz = size_for_bits(bits);
    let all = mask(bits);
    let identity = match op {
        Opcode::And => all,
        Opcode::Mul => 1,
        _ => 0,
    };
    if c & all == identity {
        return;
    }
    if op == Opcode::Xor && c & all == all {
        put(cx, mc::not_r(sz, ra));
        return;
    }
    let wide = sign_extend(c, bits) & mask(reg_bits(bits));
    if let Some(imm) = imm32(wide, sz) {
        let inst = match op {
            Opcode::Add => mc::add_ri(sz, ra, imm),
            Opcode::Sub => mc::sub_ri(sz, ra, imm),
            Opcode::Mul => mc::imul_rri(sz, ra, ra, imm),
            Opcode::And => mc::and_ri(sz, ra, imm),
            Opcode::Or => mc::or_ri(sz, ra, imm),
            _ => mc::xor_ri(sz, ra, imm),
        };
        put(cx, inst);
    } else {
        mov_imm(cx, R11, wide, 64);
        put(cx, alu_rr(op, sz, ra, R11));
    }
}

/// `shl`, `lshr`, `ashr` of `ra` (not yet extended) by the constant
/// `amount`: nothing for 0 or for the width or more (poison, §4.7); a
/// right shift of a value narrower than its register is `movzx`/`movsx`
/// then the shift for 8 and 16 bits, else `shl` by the unused bits and
/// `shr`/`sar` by them plus the amount.
fn shift_by_imm(cx: &mut FnCx<'_, X64>, op: Opcode, ra: Reg, amount: u64, bits: u32) {
    if amount == 0 || amount >= u64::from(bits) {
        return;
    }
    let sz = size_for_bits(bits);
    let k = amount as u8;
    let s = (reg_bits(bits) - bits) as u8;
    let right = |cx: &mut FnCx<'_, X64>, n: u8| {
        put(cx, if op == Opcode::LShr { mc::shr_ri(sz, ra, n) } else { mc::sar_ri(sz, ra, n) });
    };
    match op {
        Opcode::Shl => put(cx, mc::shl_ri(sz, ra, k)),
        _ if s == 0 => right(cx, k),
        _ if bits == 8 || bits == 16 => {
            extend_to(cx, ra, bits, lhs_ext(op), sz);
            right(cx, k);
        }
        _ => {
            put(cx, mc::shl_ri(sz, ra, s));
            right(cx, s + k);
        }
    }
}

/// `shl`, `lshr`, `ashr` by a register amount: the amount in `rcx`
/// (masked to its width when that is below five bits), the left operand
/// extended as the operation needs, then `shl`/`shr`/`sar r, cl`.
fn shift_by_cl<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    op: Opcode,
    lhs: Value,
    rhs: Value,
    bits: u32,
) -> CgResult<()> {
    let result = operand(cx, lhs, bits, lhs_ext(op))?;
    let ra = gp(result);
    let rcx = gp(RCX);
    cx.put(rhs, RCX)?;
    if bits < 5 {
        put(cx, mc::and_ri(Size::D, rcx, mask(bits) as i64));
    }
    let sz = size_for_bits(bits);
    put(
        cx,
        match op {
            Opcode::Shl => mc::shl_rcl(sz, ra),
            Opcode::LShr => mc::shr_rcl(sz, ra),
            _ => mc::sar_rcl(sz, ra),
        },
    );
    cx.finish(inst, result)
}

/// `udiv`, `sdiv`, `urem`, `srem`: the dividend, extended, in `rax`;
/// `rdx` zeroed (`mov edx, 0`) or the sign of `rax` (`cdq`/`cqo`); the
/// divisor, extended, in a register (`r11` for a constant); `div`/`idiv`
/// at the register width; the quotient (`rax`) or the remainder (`rdx`)
/// moved to the divisor's register, or to a temporary.
fn divide<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    op: Opcode,
    lhs: Value,
    rhs: Value,
    bits: u32,
) -> CgResult<()> {
    let ext = lhs_ext(op);
    let sz = size_for_bits(bits);
    let (rax, rdx) = (gp(RAX), Reg::Rdx);
    if let Some(c) = const_int(cx, lhs) {
        mov_imm(cx, rax, ext_const(c, bits, ext), reg_bits(bits));
    } else {
        cx.put(lhs, RAX)?;
        extend_to(cx, rax, bits, ext, sz);
    }
    let (rb, pinned) = source(cx, rhs, bits, ext, R11)?;
    let signed = ext == Ext::Sign;
    if signed {
        put(cx, if sz == Size::Q { mc::cqo() } else { mc::cdq() });
        put(cx, mc::idiv_r(sz, rb));
    } else {
        put(cx, mc::mov_ri(Size::D, rdx, 0));
        put(cx, mc::div_r(sz, rb));
    }
    let rd = match pinned {
        Some(r) => r,
        None => cx.def_reg(inst)?,
    };
    let from = if matches!(op, Opcode::URem | Opcode::SRem) { rdx } else { rax };
    put(cx, mc::mov_rr(sz, gp(rd), from));
    cx.finish(inst, rd)
}

// ---- freeze and casts ----

/// `%r = freeze %arg`: a copy (§4.7); an `undef` or `poison` constant is
/// zero.
///
/// # Errors
/// What placing the operand reports.
pub fn freeze<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    let r = cx.use_reg(arg)?;
    cx.finish(inst, r)
}

/// `%r = <cast> %arg` with both sides of class `Gpr`: `trunc`,
/// `ptrtoint`, `ptrtoaddr` and `bitcast` are copies (the bits above the
/// result's width are unspecified); `zext` and `inttoptr` extend by
/// zero, `sext` by sign, to the result's register width. A `zext` or
/// `sext` of a deferred `icmp` is `setcc` + `movzx` (+ `neg`).
///
/// # Errors
/// [`ADDRSPACECAST`]; [`CAST`] for a floating-point conversion (the
/// dispatch sends none); what the operand's placement reports.
pub fn cast<'a>(cx: &mut FnCx<'a, X64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let op = inst.opcode();
    match op {
        Opcode::Trunc
        | Opcode::ZExt
        | Opcode::SExt
        | Opcode::PtrToInt
        | Opcode::PtrToAddr
        | Opcode::IntToPtr
        | Opcode::BitCast => {}
        Opcode::AddrSpaceCast => return unsupported(ADDRSPACECAST),
        _ => return unsupported(CAST),
    }
    let fused = if matches!(op, Opcode::ZExt | Opcode::SExt) { fusable(cx, arg) } else { None };
    deferred_first(cx, inst, fused.map(|_| arg))?;
    let to = width(cx, inst.result());
    let sz = size_for_bits(to);
    if let Some(compare) = fused {
        let cond = flags(cx, compare)?;
        let rd = cx.def_reg(inst)?;
        let r = gp(rd);
        put(cx, mc::setcc_r(cond, r));
        put(cx, mc::movzx_rr(Size::D, r, Size::B, r));
        if op == Opcode::SExt {
            put(cx, mc::neg_r(sz, r));
        }
        return cx.finish(inst, rd);
    }
    let from = width(cx, arg);
    let r = cx.use_reg(arg)?;
    if from < to {
        match op {
            Opcode::ZExt | Opcode::IntToPtr => extend_to(cx, gp(r), from, Ext::Zero, sz),
            Opcode::SExt => extend_to(cx, gp(r), from, Ext::Sign, sz),
            _ => {}
        }
    }
    cx.finish(inst, r)
}

// ---- compares ----

/// The condition that holds after `cmp lhs, rhs` when `pred` does.
pub fn cond_of(pred: IntPredicate) -> Cond {
    match pred {
        IntPredicate::Eq => Cond::E,
        IntPredicate::Ne => Cond::Ne,
        IntPredicate::Ugt => Cond::A,
        IntPredicate::Uge => Cond::Ae,
        IntPredicate::Ult => Cond::B,
        IntPredicate::Ule => Cond::Be,
        IntPredicate::Sgt => Cond::G,
        IntPredicate::Sge => Cond::Ge,
        IntPredicate::Slt => Cond::L,
        IntPredicate::Sle => Cond::Le,
    }
}

/// The predicate that holds of `(b, a)` when `pred` holds of `(a, b)`.
fn swapped(pred: IntPredicate) -> IntPredicate {
    match pred {
        IntPredicate::Eq | IntPredicate::Ne => pred,
        IntPredicate::Ugt => IntPredicate::Ult,
        IntPredicate::Uge => IntPredicate::Ule,
        IntPredicate::Ult => IntPredicate::Ugt,
        IntPredicate::Ule => IntPredicate::Uge,
        IntPredicate::Sgt => IntPredicate::Slt,
        IntPredicate::Sge => IntPredicate::Sle,
        IntPredicate::Slt => IntPredicate::Sgt,
        IntPredicate::Sle => IntPredicate::Sge,
    }
}

/// Emits the compare of `lhs` with `rhs` into the flags and returns the
/// condition that holds when `pred` does. With a constant (a constant
/// left operand swaps the predicate), the other operand is extended as
/// the predicate orders (by sign or zero) and compared with `cmp r,
/// imm32`, or with `r11`. Two registers of a width narrower than theirs
/// are both extended (8 and 16 bits) or both shifted left by the unused
/// bits, which orders signed and unsigned alike. Pins at most two pool
/// registers.
fn emit_compare(
    cx: &mut FnCx<'_, X64>,
    pred: IntPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<Cond> {
    let (pred, lhs, rhs) = if const_int(cx, lhs).is_some() && const_int(cx, rhs).is_none() {
        (swapped(pred), rhs, lhs)
    } else {
        (pred, lhs, rhs)
    };
    let bits = width(cx, lhs);
    let sz = size_for_bits(bits);
    let rbits = reg_bits(bits);
    let signed = matches!(
        pred,
        IntPredicate::Sgt | IntPredicate::Sge | IntPredicate::Slt | IntPredicate::Sle
    );
    let ext = if signed { Ext::Sign } else { Ext::Zero };
    if let Some(c) = const_int(cx, rhs) {
        let a = gp(operand(cx, lhs, bits, ext)?);
        let e = ext_const(c, bits, ext);
        if let Some(imm) = imm32(e, sz) {
            put(cx, mc::cmp_ri(sz, a, imm));
        } else {
            mov_imm(cx, R11, e, 64);
            put(cx, mc::cmp_rr(sz, a, R11));
        }
    } else {
        let a = gp(operand(cx, lhs, bits, Ext::None)?);
        let b = gp(operand(cx, rhs, bits, Ext::None)?);
        if bits == 8 || bits == 16 {
            extend_to(cx, a, bits, ext, sz);
            extend_to(cx, b, bits, ext, sz);
        } else if bits != rbits {
            let s = (rbits - bits) as u8;
            put(cx, mc::shl_ri(sz, a, s));
            put(cx, mc::shl_ri(sz, b, s));
        }
        put(cx, mc::cmp_rr(sz, a, b));
    }
    Ok(cond_of(pred))
}

/// The deferred integer compare that defines `value`, if it is one:
/// what [`flags`] accepts. `None` for anything else, an `icmp` that has
/// been emitted included.
pub fn fusable<'a>(cx: &FnCx<'a, X64>, value: Value) -> Option<Inst<'a>> {
    let compare = cx.deferred(value)?;
    match compare.kind() {
        InstKind::ICmp { lhs, .. } if cx.class_of(lhs) == Class::Gpr => Some(compare),
        _ => None,
    }
}

/// Fuses the deferred integer compare `compare` (what [`fusable`]
/// returned for an operand of the instruction being lowered): emits the
/// compare into the flags and returns the condition to branch, set or
/// move on, which holds exactly when the `icmp` would give `true`.
///
/// Contract, for the consumer's lowering (`select`, `zext`, `sext` here;
/// `condbr` by WP-124):
///
/// - Call it after [`deferred_first`] with the compare's value as the
///   fused operand, and at most once per compare. The `icmp` itself is
///   then never emitted: releasing the consumer's operands releases the
///   compare's (§4.2).
/// - It accepts every predicate on every width of class `Gpr`, pointers
///   included, with any operand a register value, a home value or a
///   constant (an immediate when `cmp r, imm32` holds it).
/// - It pins at most two pool registers (the compare's operands) until
///   the consumer has been emitted, and writes those registers (in-place
///   extension or shift), the flags and `r11`. Nothing else: not `rax`,
///   `rcx`, `rdx`.
/// - Between it and the instruction that reads the flags, emit nothing
///   that writes them (§5.6 "Flags"). Placing the consumer's other
///   operands with `use_reg`/`put`, a `def_reg`, and `common`'s glue and
///   `branch_cond` keep them.
///
/// # Errors
/// `Internal` when `compare` is not a deferred `icmp` of `Gpr` operands;
/// what placing the operands reports.
pub fn flags<'a>(cx: &mut FnCx<'a, X64>, compare: Inst<'a>) -> CgResult<Cond> {
    let InstKind::ICmp { pred, lhs, rhs } = compare.kind() else {
        return cx.bug("flags of an instruction that is no icmp");
    };
    if fusable(cx, compare.result()).is_none() {
        return cx.bug("flags of a compare that is not a deferred integer icmp");
    }
    for operand in [lhs, rhs] {
        if cx.deferred(operand).is_some() {
            cx.use_reg(operand)?;
        }
    }
    emit_compare(cx, pred, lhs, rhs)
}

/// `%r = icmp <pred> %lhs, %rhs`: deferred when it may be (§4.6), else
/// `cmp` + `setcc` + `movzx`.
///
/// # Errors
/// What placing the operands reports.
pub fn icmp<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    pred: IntPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    if cx.may_defer(inst) {
        return cx.defer(inst);
    }
    let cond = emit_compare(cx, pred, lhs, rhs)?;
    let rd = cx.def_reg(inst)?;
    let r = gp(rd);
    put(cx, mc::setcc_r(cond, r));
    put(cx, mc::movzx_rr(Size::D, r, Size::B, r));
    cx.finish(inst, rd)
}

// ---- select ----

/// `%r = select i1 %cond, %if_true, %if_false` of class `Gpr`: the false
/// arm's register, then `cmovcc` from the true arm's on the fused
/// compare's condition, or on bit 0 of the condition (`test r8, 1`).
///
/// # Errors
/// What placing the operands reports.
pub fn select<'a>(
    cx: &mut FnCx<'a, X64>,
    inst: Inst<'a>,
    cond: Value,
    if_true: Value,
    if_false: Value,
) -> CgResult<()> {
    let fused = fusable(cx, cond);
    deferred_first(cx, inst, fused.map(|_| cond))?;
    let cc = if let Some(compare) = fused {
        flags(cx, compare)?
    } else {
        let rc = gp(cx.use_reg(cond)?);
        put(cx, mc::test_ri(Size::B, rc, 1));
        Cond::Ne
    };
    let sz = size_for_bits(width(cx, inst.result()));
    let rd = cx.use_reg(if_false)?;
    let rt = cx.use_reg(if_true)?;
    put(cx, mc::cmovcc_rr(cc, sz, gp(rd), gp(rt)));
    cx.finish(inst, rd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_extend_to_their_register() {
        assert_eq!(ext_const(0x40, 7, Ext::Sign), 0xffff_ffc0);
        assert_eq!(ext_const(0x40, 7, Ext::Zero), 0x40);
        assert_eq!(ext_const(1, 1, Ext::Sign), 0xffff_ffff);
        assert_eq!(ext_const(1 << 32, 33, Ext::Sign), 0xffff_ffff_0000_0000);
        assert_eq!(ext_const(u64::MAX, 64, Ext::Sign), u64::MAX);
        assert_eq!(ext_const(0x8000_0000, 32, Ext::Sign), 0x8000_0000);
    }

    #[test]
    fn immediates_hold_what_the_field_sign_extends() {
        assert_eq!(imm32(0xffff_ffff, Size::D), Some(-1));
        assert_eq!(imm32(0x8000_0000, Size::D), Some(i64::from(i32::MIN)));
        assert_eq!(imm32(u64::MAX, Size::Q), Some(-1));
        assert_eq!(imm32(0x8000_0000, Size::Q), None);
        assert_eq!(imm32(0xffff_ffff_8000_0000, Size::Q), Some(i64::from(i32::MIN)));
    }

    #[test]
    fn swapping_twice_is_the_identity() {
        for &pred in IntPredicate::ALL {
            assert_eq!(swapped(swapped(pred)), pred);
            let _ = cond_of(pred);
        }
    }
}
