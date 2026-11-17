//! Integer operations of class `Gpr` (`i1`..`i32`, `ptr`):
//! `spec/codegen-x86.md` "Integers". Owned by WP-293. Wider integers are
//! `int`'s (design 0008's I7).
//!
//! A value narrower than 32 bits leaves the bits above its width
//! unspecified in its register; an operation whose result depends on
//! them (a compare, a division, a right shift, an extension) extends its
//! operands first. `select` is a branch on every CPU (`cmov` is not in
//! `llc`'s default `generic` CPU, and a branch is right on all of them).

use hod_cg::{CgResult, Class, FnCx, PReg};
use hod_ir::{Inst, IntPredicate, Opcode, Value};
use hod_mc_x64 as mc;
use hod_mc_x64::{Cond, Reg, Size};

use crate::common::{EAX, ECX, EDX, branch_cond, const_int, gp, mov_imm, put, unsupported, width};
use crate::reg;
use crate::reg::has_low_byte as has_byte;
use crate::target::X86;

/// A cast between `Gpr` types that is no integer or pointer cast.
pub const CAST: &str = "x86:int-cast";
/// `addrspacecast`.
pub const ADDRSPACECAST: &str = "x86:addrspacecast";

/// How an operand's low bits are extended to 32.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ext {
    /// Left as it is.
    None,
    /// By zero.
    Zero,
    /// By sign.
    Sign,
}

/// The mask of the low `bits` bits.
fn mask(bits: u32) -> u32 {
    if bits >= 32 { u32::MAX } else { (1 << bits) - 1 }
}

/// `c`, of `bits` bits, extended to 32 by `ext`.
pub fn ext_const(c: u32, bits: u32, ext: Ext) -> u32 {
    if bits >= 32 {
        return c;
    }
    let low = c & mask(bits);
    match ext {
        Ext::Sign if low >> (bits - 1) & 1 != 0 => low | !mask(bits),
        Ext::Sign | Ext::Zero | Ext::None => low,
    }
}

/// Extends the low `bits` bits of `r` in place to 32 bits; nothing for
/// 32 or [`Ext::None`]. `movzx`/`movsx` for 8 and 16 bits (8 only where
/// the low byte has a name), else `and` or `shl`·`sar` (flags written).
pub fn extend_to(cx: &mut FnCx<'_, X86>, r: Reg, bits: u32, ext: Ext) {
    if bits >= 32 || ext == Ext::None {
        return;
    }
    let zero = ext == Ext::Zero;
    match bits {
        16 if zero => put(cx, mc::movzx_rr(Size::D, r, Size::W, r)),
        16 => put(cx, mc::movsx_rr(Size::D, r, Size::W, r)),
        8 if reg::has_low_byte(r) && zero => put(cx, mc::movzx_rr(Size::D, r, Size::B, r)),
        8 if reg::has_low_byte(r) => put(cx, mc::movsx_rr(Size::D, r, Size::B, r)),
        _ if zero => put(cx, mc::and_ri(Size::D, r, i64::from(mask(bits) as i32))),
        _ => {
            let s = (32 - bits) as u8;
            put(cx, mc::shl_ri(Size::D, r, s));
            put(cx, mc::sar_ri(Size::D, r, s));
        }
    }
}

/// `value` of `bits` bits in a pinned register the lowering may
/// overwrite, extended by `ext`.
fn operand(cx: &mut FnCx<'_, X86>, value: Value, bits: u32, ext: Ext) -> CgResult<PReg> {
    if let Some(c) = const_int(cx, value) {
        let r = cx.temp(Class::Gpr)?;
        mov_imm(cx, gp(r), ext_const(c, bits, ext));
        return Ok(r);
    }
    let r = cx.use_reg(value)?;
    extend_to(cx, gp(r), bits, ext);
    Ok(r)
}

/// How the left operand of `op` is extended.
fn lhs_ext(op: Opcode) -> Ext {
    match op {
        Opcode::LShr | Opcode::UDiv | Opcode::URem => Ext::Zero,
        Opcode::AShr | Opcode::SDiv | Opcode::SRem => Ext::Sign,
        _ => Ext::None,
    }
}

/// `%r = <op> %lhs, %rhs` into the left operand's register (a division
/// through `eax`/`edx`, a register shift through `cl`).
///
/// # Errors
/// [`CAST`] for a floating-point opcode (the dispatch sends none); what
/// the operands' placement reports.
pub fn binary<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, lhs: Value, rhs: Value) -> CgResult<()> {
    let op = inst.opcode();
    if !op.is_int_binary() {
        return unsupported(CAST);
    }
    let bits = width(cx, inst.result());
    let ext = lhs_ext(op);
    if matches!(op, Opcode::UDiv | Opcode::SDiv | Opcode::URem | Opcode::SRem) {
        return divide(cx, inst, op, lhs, rhs, bits);
    }
    let shift = matches!(op, Opcode::Shl | Opcode::LShr | Opcode::AShr);
    let result = operand(cx, lhs, bits, ext)?;
    let ra = gp(result);
    match (op, const_int(cx, rhs)) {
        (_, Some(k)) if shift => {
            if k < bits {
                let k = k as u8;
                put(
                    cx,
                    match op {
                        Opcode::Shl => mc::shl_ri(Size::D, ra, k),
                        Opcode::LShr => mc::shr_ri(Size::D, ra, k),
                        _ => mc::sar_ri(Size::D, ra, k),
                    },
                );
            }
        }
        _ if shift => {
            cx.put(rhs, reg::ECX)?;
            put(
                cx,
                match op {
                    Opcode::Shl => mc::shl_rcl(Size::D, ra),
                    Opcode::LShr => mc::shr_rcl(Size::D, ra),
                    _ => mc::sar_rcl(Size::D, ra),
                },
            );
        }
        (_, Some(c)) => {
            let imm = i64::from(c as i32);
            put(
                cx,
                match op {
                    Opcode::Add => mc::add_ri(Size::D, ra, imm),
                    Opcode::Sub => mc::sub_ri(Size::D, ra, imm),
                    Opcode::Mul => mc::imul_rri(Size::D, ra, ra, imm),
                    Opcode::And => mc::and_ri(Size::D, ra, imm),
                    Opcode::Or => mc::or_ri(Size::D, ra, imm),
                    _ => mc::xor_ri(Size::D, ra, imm),
                },
            );
        }
        _ => {
            let rb = if rhs == lhs { ra } else { gp(cx.use_reg(rhs)?) };
            put(
                cx,
                match op {
                    Opcode::Add => mc::add_rr(Size::D, ra, rb),
                    Opcode::Sub => mc::sub_rr(Size::D, ra, rb),
                    Opcode::Mul => mc::imul_rr(Size::D, ra, rb),
                    Opcode::And => mc::and_rr(Size::D, ra, rb),
                    Opcode::Or => mc::or_rr(Size::D, ra, rb),
                    _ => mc::xor_rr(Size::D, ra, rb),
                },
            );
        }
    }
    cx.finish(inst, result)
}

/// `udiv`, `sdiv`, `urem`, `srem`: both operands extended to 32 bits
/// (the dividend in `eax`), `cdq` or `edx` zeroed, `idiv`/`div`, the
/// quotient (`eax`) or the remainder (`edx`) moved to the divisor's
/// register.
fn divide<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    op: Opcode,
    lhs: Value,
    rhs: Value,
    bits: u32,
) -> CgResult<()> {
    let ext = lhs_ext(op);
    let rd = operand(cx, rhs, bits, ext)?;
    cx.put(lhs, reg::EAX)?;
    extend_to(cx, EAX, bits, ext);
    if ext == Ext::Sign {
        put(cx, mc::cdq());
        put(cx, mc::idiv_r(Size::D, gp(rd)));
    } else {
        put(cx, mc::mov_ri(Size::D, EDX, 0));
        put(cx, mc::div_r(Size::D, gp(rd)));
    }
    let from = if matches!(op, Opcode::URem | Opcode::SRem) { EDX } else { EAX };
    put(cx, mc::mov_rr(Size::D, gp(rd), from));
    cx.finish(inst, rd)
}

/// `%r = freeze %arg`: a copy.
///
/// # Errors
/// What placing the operand reports.
pub fn freeze<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let r = cx.use_reg(arg)?;
    cx.finish(inst, r)
}

/// `%r = <cast> %arg` with both sides `Gpr`: `trunc`, `ptrtoint`,
/// `ptrtoaddr`, `bitcast` copy; `zext` and `inttoptr` extend by zero,
/// `sext` by sign.
///
/// # Errors
/// [`ADDRSPACECAST`], [`CAST`]; what the operand's placement reports.
pub fn cast<'a>(cx: &mut FnCx<'a, X86>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    let ext = match inst.opcode() {
        Opcode::ZExt | Opcode::IntToPtr => Ext::Zero,
        Opcode::SExt => Ext::Sign,
        Opcode::Trunc | Opcode::PtrToInt | Opcode::PtrToAddr | Opcode::BitCast => Ext::None,
        Opcode::AddrSpaceCast => return unsupported(ADDRSPACECAST),
        _ => return unsupported(CAST),
    };
    let from = width(cx, arg);
    let r = cx.use_reg(arg)?;
    if from < width(cx, inst.result()) {
        extend_to(cx, gp(r), from, ext);
    }
    cx.finish(inst, r)
}

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

/// The compare of `lhs` with `rhs` into the flags, both extended to 32
/// bits by sign for a signed predicate and by zero otherwise; the
/// condition that holds when `pred` does. Pins at most two pool
/// registers.
///
/// # Errors
/// What placing the operands reports.
pub fn compare(
    cx: &mut FnCx<'_, X86>,
    pred: IntPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<Cond> {
    Ok(compare_in(cx, pred, lhs, rhs)?.0)
}

/// [`compare`], also giving the register the left operand was placed
/// in: the operand is dead after the compare, and a result may take it.
fn compare_in(
    cx: &mut FnCx<'_, X86>,
    pred: IntPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<(Cond, PReg)> {
    let bits = width(cx, lhs);
    let signed = matches!(
        pred,
        IntPredicate::Sgt | IntPredicate::Sge | IntPredicate::Slt | IntPredicate::Sle
    );
    let ext = if signed { Ext::Sign } else { Ext::Zero };
    let ra = operand(cx, lhs, bits, ext)?;
    let a = gp(ra);
    if let Some(c) = const_int(cx, rhs) {
        put(cx, mc::cmp_ri(Size::D, a, i64::from(ext_const(c, bits, ext) as i32)));
    } else {
        let b = if rhs == lhs { a } else { gp(operand(cx, rhs, bits, ext)?) };
        put(cx, mc::cmp_rr(Size::D, a, b));
    }
    Ok((cond_of(pred), ra))
}

/// `%r = icmp <pred> %lhs, %rhs`: `cmp`, `setcc` (into `cl` when the
/// result's register has no low byte), `movzx`. The result takes the
/// left operand's register (dead once compared: `FnCx::use_reg`), so the
/// lowering pins two pool registers, not three (the two-register rule,
/// `spec/codegen-x86.md` §2).
///
/// # Errors
/// What placing the operands reports.
pub fn icmp<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    pred: IntPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    let (cond, rd) = compare_in(cx, pred, lhs, rhs)?;
    let r = gp(rd);
    let byte = if has_byte(r) { r } else { ECX };
    put(cx, mc::setcc_r(cond, byte));
    put(cx, mc::movzx_rr(Size::D, r, Size::B, byte));
    cx.finish(inst, rd)
}

/// `%r = select i1 %cond, %if_true, %if_false`: the condition in `eax`,
/// the false arm's register, `test al, 1`, `je` over `mov` of the true
/// arm.
///
/// # Errors
/// What placing the operands reports.
pub fn select<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    cond: Value,
    if_true: Value,
    if_false: Value,
) -> CgResult<()> {
    if if_true == if_false {
        let r = cx.use_reg(if_true)?;
        return cx.finish(inst, r);
    }
    if let Some(c) = const_int(cx, cond) {
        let r = cx.use_reg(if c & 1 != 0 { if_true } else { if_false })?;
        return cx.finish(inst, r);
    }
    cx.put(cond, reg::EAX)?;
    let rd = cx.use_reg(if_false)?;
    let rt = cx.use_reg(if_true)?;
    put(cx, mc::test_ri(Size::B, EAX, 1));
    let skip = cx.new_label();
    branch_cond(cx, Cond::E, skip)?;
    put(cx, mc::mov_rr(Size::D, gp(rd), gp(rt)));
    cx.bind(skip)?;
    cx.finish(inst, rd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_extend_to_32_bits() {
        assert_eq!(ext_const(0x40, 7, Ext::Sign), 0xffff_ffc0);
        assert_eq!(ext_const(0x40, 7, Ext::Zero), 0x40);
        assert_eq!(ext_const(1, 1, Ext::Sign), u32::MAX);
        assert_eq!(ext_const(0xff, 8, Ext::Zero), 0xff);
        assert_eq!(ext_const(0x8000_0000, 32, Ext::Sign), 0x8000_0000);
    }

    #[test]
    fn every_predicate_has_a_condition() {
        for &pred in IntPredicate::ALL {
            let _ = cond_of(pred);
        }
    }
}
