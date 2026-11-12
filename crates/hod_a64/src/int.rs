//! Integer instructions of class `Gpr` (`spec/codegen.md` §2.2): binary
//! operations, `freeze`, integer and pointer casts, `icmp`, `select`.
//!
//! **Owner: WP-035.** Tests: `tests/int.rs`. The dispatch in `target.rs`
//! is final. Every entry point starts with
//! [`deferred_first`](crate::common::deferred_first), naming the operand
//! it fuses.
//!
//! # Narrow values (§2.2)
//!
//! A value of `iN` lives in a `W` register for `N <= 32` and an `X`
//! register above, with the bits above `N` unspecified. Operations whose
//! low `N` result bits depend only on the low `N` operand bits (`add`,
//! `sub`, `mul`, `and`, `or`, `xor`, `shl`, `trunc`) compute in the
//! register as it is; the others extend first: `lshr`, `udiv`, `urem`
//! and unsigned compares by zero, `ashr`, `sdiv`, `srem` and signed
//! compares by sign. A compare of two registers at an odd width shifts
//! the left one up by `regbits - N` and compares it with the right one
//! shifted the same way (`cmp a, b, lsl #s`), which orders both signed
//! and unsigned and drops the unspecified bits in two instructions. A
//! register shift amount of a type narrower than five bits is masked
//! first: `lslv` and its kin read five (`W`) or six (`X`) bits of it.
//! Shift amounts of the width or more, division by zero and the
//! smallest value divided by -1 are undefined (§4.7): whatever the
//! instruction does is correct, and a constant shift amount of the
//! width or more emits nothing.
//!
//! # Compare fusion (§4.6)
//!
//! An `icmp` whose result may be deferred is deferred. Its consumer then
//! either asks for the value (the `icmp` is emitted as `cmp` + `cset`) or
//! fuses it: [`fusable`] says whether a value is such a deferred integer
//! compare, and [`flags`] emits the compare into NZCV and returns the
//! condition. This module fuses into `select` (`csel`), `zext` (`cset`)
//! and `sext` (`csetm`).

use hod_cg::{CgResult, Class, FnCx, PReg};
use hod_ir::{Const, Inst, InstKind, IntPredicate, Opcode, Value};
use hod_mc_a64 as mc;
use hod_mc_a64::{Cond, Reg, RegZr, Shift, Size};

use crate::common::{X16, X17, deferred_first, mov_imm, put, size_for_bits, unsupported, xr};
use crate::target::A64;

/// A binary operation on integers or pointers in general registers.
pub const BINARY: &str = "a64:int-binary";
/// `freeze` of a general-register value.
pub const FREEZE: &str = "a64:int-freeze";
/// A cast between general-register types.
pub const CAST: &str = "a64:int-cast";
/// `addrspacecast`: `classify` rejects every non-zero address space, and
/// within one address space LLVM has no such cast.
pub const ADDRSPACECAST: &str = "a64:addrspacecast";
/// `icmp` of general-register values.
pub const ICMP: &str = "a64:icmp";
/// `select` of general-register values.
pub const SELECT: &str = "a64:int-select";

// ---- widths and operands ----

/// How an operand's low bits are extended to its register.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ext {
    /// Left as it is: the bits above the width stay unspecified.
    None,
    /// By zero.
    Zero,
    /// By sign.
    Sign,
}

/// The width of a `Gpr`-class type: an integer's, 64 for a pointer.
#[inline]
fn width(cx: &FnCx<'_, A64>, value: Value) -> u32 {
    cx.types().int_bits(cx.body.value_type(cx.module, value)).unwrap_or(64)
}

/// The width of the register a value of `bits` bits lives in.
#[inline]
fn reg_bits(bits: u32) -> u32 {
    if bits <= 32 { 32 } else { 64 }
}

/// The mask of the low `bits` bits.
#[inline]
fn mask(bits: u32) -> u64 {
    if bits >= 64 { u64::MAX } else { (1 << bits) - 1 }
}

/// The integer `value` is, zero-extended from its width, when it is an
/// integer constant (or a zero-like one).
fn const_int(cx: &FnCx<'_, A64>, value: Value) -> Option<u64> {
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
    let wide = if ext == Ext::Sign && bits < 64 && value >> (bits - 1) & 1 == 1 {
        value | !mask(bits)
    } else {
        value
    };
    wide & mask(reg_bits(bits))
}

/// Extends the low `bits` bits of `r` in place; nothing for a full
/// register.
fn extend(cx: &mut FnCx<'_, A64>, r: Reg, bits: u32, ext: Ext) {
    if bits == 32 || bits == 64 {
        return;
    }
    let sz = size_for_bits(bits);
    match ext {
        Ext::None => {}
        Ext::Zero => put(cx, mc::ubfx(sz, r.zr(), r.zr(), 0, bits)),
        Ext::Sign => put(cx, mc::sbfx(sz, r.zr(), r.zr(), 0, bits)),
    }
}

/// `value` of `bits` bits in a pinned register of the pool (which the
/// lowering may overwrite and leave its result in), extended by `ext`.
/// An integer constant is materialized already extended.
fn operand(cx: &mut FnCx<'_, A64>, value: Value, bits: u32, ext: Ext) -> CgResult<PReg> {
    if let Some(c) = const_int(cx, value) {
        let r = cx.temp(Class::Gpr)?;
        mov_imm(cx, size_for_bits(bits), xr(r), ext_const(c, bits, ext));
        return Ok(r);
    }
    let r = cx.use_reg(value)?;
    extend(cx, xr(r), bits, ext);
    Ok(r)
}

/// As [`operand`], but an integer constant goes to the scratch register
/// `scratch` and pins nothing: for an operand the result does not stay
/// in.
fn source(
    cx: &mut FnCx<'_, A64>,
    value: Value,
    bits: u32,
    ext: Ext,
    scratch: Reg,
) -> CgResult<Reg> {
    if let Some(c) = const_int(cx, value) {
        mov_imm(cx, size_for_bits(bits), scratch, ext_const(c, bits, ext));
        return Ok(scratch);
    }
    Ok(xr(operand(cx, value, bits, ext)?))
}

// ---- binary operations ----

/// `%r = <op> %lhs, %rhs`: computed into the left operand's register.
///
/// # Errors
/// [`BINARY`] for a floating-point opcode (the dispatch sends none);
/// what the operands' placement reports.
pub fn binary<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, lhs: Value, rhs: Value) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    let op = inst.opcode();
    if !op.is_int_binary() {
        return unsupported(BINARY);
    }
    let bits = width(cx, inst.result());
    let konst = const_int(cx, rhs);
    let lhs_ext = lhs_ext(op);
    // A constant shift right extends and shifts in one instruction.
    let fold_shift = matches!(op, Opcode::LShr | Opcode::AShr) && konst.is_some();
    let result = operand(cx, lhs, bits, if fold_shift { Ext::None } else { lhs_ext })?;
    let ra = xr(result);
    match op {
        Opcode::Add | Opcode::Sub => add_sub(cx, op == Opcode::Sub, ra, rhs, konst, bits)?,
        Opcode::Mul => {
            let sz = size_for_bits(bits);
            let rm = source(cx, rhs, bits, Ext::None, X16)?.zr();
            put(cx, mc::mul(sz, ra.zr(), ra.zr(), rm));
        }
        Opcode::And | Opcode::Or | Opcode::Xor => logical(cx, op, ra, rhs, konst, bits)?,
        Opcode::Shl | Opcode::LShr | Opcode::AShr => shift(cx, op, ra, rhs, konst, bits)?,
        _ => divide(cx, op, ra, rhs, lhs_ext, bits)?,
    }
    cx.finish(inst, result)
}

/// How the left operand of `op` is extended before the operation.
fn lhs_ext(op: Opcode) -> Ext {
    match op {
        Opcode::LShr | Opcode::UDiv | Opcode::URem => Ext::Zero,
        Opcode::AShr | Opcode::SDiv | Opcode::SRem => Ext::Sign,
        _ => Ext::None,
    }
}

/// How a right operand of `op` in a register is extended before the
/// operation: as the left one for a division, by zero for a shift amount
/// narrower than five bits (`lslv` and its kin read five or six bits).
fn rhs_ext(op: Opcode, bits: u32) -> Ext {
    match op {
        Opcode::Shl | Opcode::LShr | Opcode::AShr if bits < 5 => Ext::Zero,
        Opcode::UDiv | Opcode::URem | Opcode::SDiv | Opcode::SRem => lhs_ext(op),
        _ => Ext::None,
    }
}

/// `ra = ra <op> rm` of two registers at `bits` bits, both operands
/// already extended as [`lhs_ext`] and [`rhs_ext`] say: the one
/// instruction of each operation (a remainder through `x17`).
fn reg_op(cx: &mut FnCx<'_, A64>, op: Opcode, bits: u32, ra: Reg, rm: RegZr) {
    let sz = size_for_bits(bits);
    let rd = ra.zr();
    let word = match op {
        Opcode::Add => mc::add(sz, rd, rd, rm),
        Opcode::Sub => mc::sub(sz, rd, rd, rm),
        Opcode::Mul => mc::mul(sz, rd, rd, rm),
        Opcode::And => mc::and(sz, rd, rd, rm),
        Opcode::Or => mc::orr(sz, rd, rd, rm),
        Opcode::Xor => mc::eor(sz, rd, rd, rm),
        Opcode::Shl => mc::lslv(sz, rd, rd, rm),
        Opcode::LShr => mc::lsrv(sz, rd, rd, rm),
        Opcode::AShr => mc::asrv(sz, rd, rd, rm),
        Opcode::UDiv => mc::udiv(sz, rd, rd, rm),
        Opcode::SDiv => mc::sdiv(sz, rd, rd, rm),
        _ => {
            let signed = op == Opcode::SRem;
            put(
                cx,
                if signed {
                    mc::sdiv(sz, X17.zr(), rd, rm)
                } else {
                    mc::udiv(sz, X17.zr(), rd, rm)
                },
            );
            mc::msub(sz, rd, X17.zr(), rm, rd)
        }
    };
    put(cx, word);
}

/// One lane of an element-wise integer operation (`lanes.rs`, WP-080):
/// `ra = ra <op> rb` at `bits` bits (`op` an integer binary opcode), the
/// operands extended as this module's [`binary`] extends them, then the
/// instruction [`binary`] emits for two registers. Writes `ra`, `rb` and
/// `x17`.
pub fn lane_binary(cx: &mut FnCx<'_, A64>, op: Opcode, bits: u32, ra: Reg, rb: Reg) {
    extend(cx, ra, bits, lhs_ext(op));
    extend(cx, rb, bits, rhs_ext(op, bits));
    reg_op(cx, op, bits, ra, rb.zr());
}

/// `add` or `sub` (`sub`) of `ra` and `rhs` into `ra`: an immediate when
/// the constant or its negation is one.
fn add_sub(
    cx: &mut FnCx<'_, A64>,
    sub: bool,
    ra: Reg,
    rhs: Value,
    konst: Option<u64>,
    bits: u32,
) -> CgResult<()> {
    let sz = size_for_bits(bits);
    let imm = |cx: &mut FnCx<'_, A64>, sub: bool, imm: u64| {
        put(
            cx,
            if sub {
                mc::sub_imm(sz, ra.sp(), ra.sp(), imm)
            } else {
                mc::add_imm(sz, ra.sp(), ra.sp(), imm)
            },
        );
    };
    if let Some(value) = konst {
        let neg = value.wrapping_neg() & mask(bits);
        if mc::is_arith_imm(value) {
            imm(cx, sub, value);
            return Ok(());
        }
        if mc::is_arith_imm(neg) {
            imm(cx, !sub, neg);
            return Ok(());
        }
    }
    let rm = source(cx, rhs, bits, Ext::None, X16)?.zr();
    reg_op(cx, if sub { Opcode::Sub } else { Opcode::Add }, bits, ra, rm);
    Ok(())
}

/// `shl`, `lshr`, `ashr` of `ra` (extended as the operation needs,
/// unless the amount is a constant) by `rhs` into `ra`.
fn shift(
    cx: &mut FnCx<'_, A64>,
    op: Opcode,
    ra: Reg,
    rhs: Value,
    konst: Option<u64>,
    bits: u32,
) -> CgResult<()> {
    let sz = size_for_bits(bits);
    let full = bits == reg_bits(bits);
    let rd = ra.zr();
    let word = match konst {
        // Poison (§4.7).
        Some(amount) if amount >= u64::from(bits) => return Ok(()),
        Some(amount) => {
            let amount = u32::try_from(amount).expect("below the width");
            match op {
                Opcode::Shl => mc::lsl_imm(sz, rd, rd, amount),
                Opcode::LShr if full => mc::lsr_imm(sz, rd, rd, amount),
                Opcode::LShr => mc::ubfx(sz, rd, rd, amount, bits - amount),
                _ if full => mc::asr_imm(sz, rd, rd, amount),
                _ => mc::sbfx(sz, rd, rd, amount, bits - amount),
            }
        }
        None => {
            let rm = source(cx, rhs, bits, rhs_ext(op, bits), X16)?.zr();
            reg_op(cx, op, bits, ra, rm);
            return Ok(());
        }
    };
    put(cx, word);
    Ok(())
}

/// `udiv`, `sdiv`, `urem`, `srem` of `ra` (extended by `ext`) by `rhs`
/// into `ra`; a remainder through `x17`.
fn divide(
    cx: &mut FnCx<'_, A64>,
    op: Opcode,
    ra: Reg,
    rhs: Value,
    ext: Ext,
    bits: u32,
) -> CgResult<()> {
    let rm = source(cx, rhs, bits, ext, X16)?.zr();
    reg_op(cx, op, bits, ra, rm);
    Ok(())
}

/// `and`, `or`, `xor` of `ra` with `rhs` into `ra`: a logical immediate
/// when the constant, or the constant with the bits above the width set,
/// has one; `mvn` for `xor` with all ones; nothing for the identities.
fn logical(
    cx: &mut FnCx<'_, A64>,
    op: Opcode,
    ra: Reg,
    rhs: Value,
    c: Option<u64>,
    bits: u32,
) -> CgResult<()> {
    let sz = size_for_bits(bits);
    let all = mask(bits);
    if let Some(c) = c {
        let identity = match op {
            Opcode::And => all,
            _ => 0,
        };
        if c == identity {
            return Ok(());
        }
        if op == Opcode::Xor && c == all {
            put(cx, mc::mvn(sz, ra.zr(), ra.zr()));
            return Ok(());
        }
        let regmask = mask(reg_bits(bits));
        for candidate in [c, (c | !all) & regmask] {
            let word = match op {
                Opcode::And => mc::try_and_imm(sz, ra.sp(), ra.zr(), candidate),
                Opcode::Or => mc::try_orr_imm(sz, ra.sp(), ra.zr(), candidate),
                _ => mc::try_eor_imm(sz, ra.sp(), ra.zr(), candidate),
            };
            if let Some(word) = word {
                put(cx, word);
                return Ok(());
            }
        }
    }
    let b = source(cx, rhs, bits, Ext::None, X16)?.zr();
    reg_op(cx, op, bits, ra, b);
    Ok(())
}

// ---- freeze and casts ----

/// `%r = freeze %arg`: a copy (§4.7); an `undef` or `poison` constant is
/// zero.
///
/// # Errors
/// What placing the operand reports.
pub fn freeze<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    let r = cx.use_reg(arg)?;
    cx.finish(inst, r)
}

/// `%r = <cast> %arg` with both sides of class `Gpr`: `trunc`,
/// `ptrtoint`, `ptrtoaddr` and `bitcast` are copies (the bits above the
/// result's width are unspecified); `zext` and `inttoptr` extend by
/// zero, `sext` by sign. A `zext` or `sext` of a deferred `icmp` is one
/// `cset` or `csetm`.
///
/// # Errors
/// [`ADDRSPACECAST`]; [`CAST`] for a floating-point conversion (the
/// dispatch sends none); what the operand's placement reports.
pub fn cast<'a>(cx: &mut FnCx<'a, A64>, inst: Inst<'a>, arg: Value) -> CgResult<()> {
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
        let word = if op == Opcode::ZExt {
            mc::cset(sz, xr(rd).zr(), cond)
        } else {
            mc::csetm(sz, xr(rd).zr(), cond)
        };
        put(cx, word);
        return cx.finish(inst, rd);
    }
    let from = width(cx, arg);
    let r = cx.use_reg(arg)?;
    lane_cast(cx, op, from, to, xr(r));
    cx.finish(inst, r)
}

/// The register part of [`cast`] (and one lane of an element-wise cast,
/// `lanes.rs`, WP-080): `r`, a value of `from` bits, as the `op` cast of
/// it to `to` bits, in place: extended by zero (`zext`, `inttoptr`) or
/// sign (`sext`) when it widens, else unchanged.
pub fn lane_cast(cx: &mut FnCx<'_, A64>, op: Opcode, from: u32, to: u32, r: Reg) {
    if from < to {
        let (sz, rr) = (size_for_bits(to), r.zr());
        match op {
            Opcode::ZExt | Opcode::IntToPtr => put(cx, mc::ubfx(sz, rr, rr, 0, from)),
            Opcode::SExt => put(cx, mc::sbfx(sz, rr, rr, 0, from)),
            _ => {}
        }
    }
}

// ---- compares ----

/// The condition that holds after `cmp lhs, rhs` when `pred` does.
pub fn cond_of(pred: IntPredicate) -> Cond {
    match pred {
        IntPredicate::Eq => Cond::Eq,
        IntPredicate::Ne => Cond::Ne,
        IntPredicate::Ugt => Cond::Hi,
        IntPredicate::Uge => Cond::Hs,
        IntPredicate::Ult => Cond::Lo,
        IntPredicate::Ule => Cond::Ls,
        IntPredicate::Sgt => Cond::Gt,
        IntPredicate::Sge => Cond::Ge,
        IntPredicate::Slt => Cond::Lt,
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

/// Emits the compare of `lhs` with `rhs` into NZCV and returns the
/// condition that holds when `pred` does. Pins at most two pool
/// registers; writes `x16` for a constant no `cmp`/`cmn` immediate
/// takes.
fn compare(cx: &mut FnCx<'_, A64>, pred: IntPredicate, lhs: Value, rhs: Value) -> CgResult<Cond> {
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
        let a = xr(operand(cx, lhs, bits, ext)?);
        let e = ext_const(c, bits, ext);
        let neg = e.wrapping_neg() & mask(rbits);
        let word = if mc::is_arith_imm(e) {
            mc::cmp_imm(sz, a.sp(), e)
        } else if mc::is_arith_imm(neg) {
            // `cmn a, #k` sets the flags `cmp a, #-k` would: -k is never
            // the smallest value, and is not 0.
            mc::cmn_imm(sz, a.sp(), neg)
        } else {
            mov_imm(cx, sz, X16, e);
            mc::cmp(sz, a.zr(), X16.zr())
        };
        put(cx, word);
    } else {
        let a = xr(operand(cx, lhs, bits, Ext::None)?);
        let b = xr(operand(cx, rhs, bits, Ext::None)?);
        if bits == rbits {
            put(cx, mc::cmp(sz, a.zr(), b.zr()));
        } else {
            let s = rbits - bits;
            put(cx, mc::lsl_imm(sz, a.zr(), a.zr(), s));
            put(cx, mc::cmp_shifted(sz, a.zr(), b.zr(), Shift::Lsl, s));
        }
    }
    Ok(cond_of(pred))
}

/// The deferred integer compare that defines `value`, if it is one:
/// what [`flags`] accepts. `None` for anything else, an `icmp` that has
/// been emitted included.
pub fn fusable<'a>(cx: &FnCx<'a, A64>, value: Value) -> Option<Inst<'a>> {
    let compare = cx.deferred(value)?;
    match compare.kind() {
        InstKind::ICmp { lhs, .. } if cx.class_of(lhs) == Class::Gpr => Some(compare),
        _ => None,
    }
}

/// Fuses the deferred integer compare `compare` (what [`fusable`]
/// returned for an operand of the instruction being lowered): emits the
/// compare into NZCV and returns the condition to branch or select on,
/// which holds exactly when the `icmp` would give `true`.
///
/// Contract, for the consumer's lowering (`select` here; `condbr` by the
/// integration package):
///
/// - Call it after [`deferred_first`] with the compare's value as the
///   fused operand, and at most once per compare. The `icmp` itself is
///   then never emitted: releasing the consumer's operands releases the
///   compare's (§4.2).
/// - It accepts every predicate on every width of class `Gpr`, pointers
///   included, with any operand a register value, a home value or a
///   constant (folded into `cmp`/`cmn` when an immediate takes it).
/// - It pins at most two pool registers (the compare's operands) until
///   the consumer has been emitted, and writes those registers (in
///   place extension or shift), NZCV and `x16`. Nothing else.
/// - Between it and the instruction that reads the flags, emit nothing
///   that writes NZCV. Placing the consumer's other operands with
///   `use_reg`/`put` is safe (loads, moves, constant and address
///   materialization never write it), as is a `branch_cond`.
///
/// # Errors
/// `Internal` when `compare` is not a deferred `icmp` of `Gpr` operands;
/// what placing the operands reports.
pub fn flags<'a>(cx: &mut FnCx<'a, A64>, compare: Inst<'a>) -> CgResult<Cond> {
    // `spec/dwarf.md` §5.8: the compare's code is at the compare's
    // location, inside its consumer's.
    let outer = cx.line_loc();
    cx.line_switch(cx.row_loc_of(compare));
    let flags = compare_flags(cx, compare);
    cx.line_switch(outer);
    flags
}

/// [`flags`] without the line switch.
fn compare_flags<'a>(cx: &mut FnCx<'a, A64>, compare: Inst<'a>) -> CgResult<Cond> {
    let InstKind::ICmp { pred, lhs, rhs } = compare.kind() else {
        return cx.bug("flags of an instruction that is no icmp");
    };
    if fusable(cx, compare.result()).is_none() {
        return cx.bug("flags of a compare that is not a deferred integer icmp");
    }
    compare_operands(cx, pred, lhs, rhs)
}

/// [`compare`], after the compare's own deferred operands (there are
/// none when it was deferred after its own `deferred_first`).
fn compare_operands(
    cx: &mut FnCx<'_, A64>,
    pred: IntPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<Cond> {
    for operand in [lhs, rhs] {
        if cx.deferred(operand).is_some() {
            cx.use_reg(operand)?;
        }
    }
    compare(cx, pred, lhs, rhs)
}

/// `%r = icmp <pred> %lhs, %rhs`: deferred when it may be (§4.6), else
/// `cmp` + `cset`.
///
/// # Errors
/// What placing the operands reports.
pub fn icmp<'a>(
    cx: &mut FnCx<'a, A64>,
    inst: Inst<'a>,
    pred: IntPredicate,
    lhs: Value,
    rhs: Value,
) -> CgResult<()> {
    deferred_first(cx, inst, None)?;
    if cx.may_defer(inst) {
        return cx.defer(inst);
    }
    let cond = compare(cx, pred, lhs, rhs)?;
    let rd = cx.def_reg(inst)?;
    put(cx, mc::cset(Size::W, xr(rd).zr(), cond));
    cx.finish(inst, rd)
}

// ---- select ----

/// An arm of a `select`: `None` for the constant zero (`xzr`), else its
/// pinned register.
fn arm(cx: &mut FnCx<'_, A64>, value: Value) -> CgResult<Option<PReg>> {
    if const_int(cx, value) == Some(0) {
        return Ok(None);
    }
    cx.use_reg(value).map(Some)
}

/// `%r = select i1 %cond, %if_true, %if_false` of class `Gpr`: `csel` on
/// the fused compare's condition, or on bit 0 of the condition (`tst`).
///
/// # Errors
/// What placing the operands reports.
pub fn select<'a>(
    cx: &mut FnCx<'a, A64>,
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
        let rc = xr(cx.use_reg(cond)?);
        put(cx, mc::tst_imm(Size::W, rc.zr(), 1));
        Cond::Ne
    };
    let sz = size_for_bits(width(cx, inst.result()));
    let t = arm(cx, if_true)?;
    let f = arm(cx, if_false)?;
    let rd = match (t, f) {
        (Some(r), _) | (None, Some(r)) => r,
        (None, None) => cx.def_reg(inst)?,
    };
    let zr = |r: Option<PReg>| r.map_or(RegZr::ZR, |r| xr(r).zr());
    put(cx, mc::csel(sz, xr(rd).zr(), zr(t), zr(f), cc));
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
    fn swapping_twice_is_the_identity_and_conditions_match() {
        for &pred in IntPredicate::ALL {
            assert_eq!(swapped(swapped(pred)), pred);
            let _ = cond_of(pred);
        }
    }
}
