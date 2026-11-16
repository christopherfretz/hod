//! The integer intrinsics of `Gpr` integers, 1 to 32 bits
//! (`spec/codegen-x86.md` §4.1): owned by WP-309.
//!
//! `bswap`, `ctpop`, `ctlz`, `cttz`, `bitreverse`, `abs`,
//! `smax`/`smin`/`umax`/`umin`, `sadd.sat`/`uadd.sat`/`ssub.sat`/`usub.sat`,
//! `sshl.sat`/`ushl.sat`, `fshl`/`fshr` with a result of at most 32 bits,
//! and `scmp`/`ucmp` of operands of at most 32 bits (a result of up to 64
//! bits). The shapes are `hod_x64`'s of the same intrinsics at 32 bits,
//! with a branch in place of every `cmov` (`llc`'s `generic` CPU has none,
//! and a branch is right on all of them, as `select`).
//!
//! # Registers
//!
//! Every operand is asked for once (`FnCx::use_reg`; an operand that
//! repeats an earlier one shares its register), so at most three pool
//! registers are pinned and `Stress::ALL`'s smallest pool suffices. The
//! work is done in the operands' registers and in `eax`, `ecx`, `edx`,
//! which hold no value; the result is left in the first operand's
//! register. Nothing asks for a temporary.
//!
//! # Narrow widths
//!
//! A value of `N < 32` bits has unspecified bits above `N` in its register
//! (§4). Every lowering here drops or extends them before they could
//! reach the result's low `N` bits: a count or a compare extends by zero
//! or sign, a reversal shifts them out to the right, a saturating shift
//! moves the value to the top of the register first.
//!
//! # Poison flags
//!
//! `is_zero_poison` (`ctlz`, `cttz`) and `is_int_min_poison` (`abs`) are
//! ignored: the defined result is computed.

use hod_cg::{CgResult, FnCx, PReg};
use hod_ir::{Inst, Intrinsic, Value};
use hod_mc_x64 as mc;
use hod_mc_x64::{Cond, Reg, Size};

use crate::alu::{Ext, extend_to};
use crate::common::{EAX, ECX, EDX, branch, branch_cond, gp, put, unsupported, value_ty};
use crate::target::X86;

/// The width of the integer type of `value` (`None` for a vector or any
/// other type).
fn int_width(cx: &FnCx<'_, X86>, value: Value) -> Option<u32> {
    cx.types().int_bits(value_ty(cx, value))
}

/// Whether the call of intrinsic `which` as `inst` with `args` is this
/// module's: one of the integer intrinsics above with an integer result
/// of 1 to 32 bits; `scmp`/`ucmp` of operands of 1 to 32 bits with a
/// result of 2 to 64 bits.
#[inline]
pub fn is_intrinsic(
    cx: &FnCx<'_, X86>,
    inst: Inst<'_>,
    which: Intrinsic<'_>,
    args: &[Value],
) -> bool {
    use Intrinsic as I;
    let result = cx.types().int_bits(inst.ty());
    match which {
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
        | I::Fshr => matches!(result, Some(1..=32)),
        I::Scmp | I::Ucmp => {
            matches!(result, Some(1..=64))
                && matches!(args.first().and_then(|&a| int_width(cx, a)), Some(1..=32))
        }
        _ => false,
    }
}

/// An immediate operand of a 32-bit instruction.
fn imm(v: u32) -> i64 {
    i64::from(v as i32)
}

/// `mov r, imm`.
fn mov_imm(cx: &mut FnCx<'_, X86>, r: Reg, v: u32) {
    put(cx, mc::mov_ri(Size::D, r, imm(v)));
}

/// The mask of the low `bits` bits (`bits` 1 to 32).
fn mask(bits: u32) -> u32 {
    if bits >= 32 { u32::MAX } else { (1 << bits) - 1 }
}

/// The registers of `values`, each asked for once: a value that repeats
/// an earlier one shares its register.
fn regs<const K: usize>(cx: &mut FnCx<'_, X86>, values: [Value; K]) -> CgResult<[PReg; K]> {
    let mut out = [PReg::new(0); K];
    for i in 0..K {
        out[i] = match values[..i].iter().position(|&v| v == values[i]) {
            Some(j) => out[j],
            None => cx.use_reg(values[i])?,
        };
    }
    Ok(out)
}

/// `%r = call iN @llvm.<which>(...)`, `N <= 32` (`scmp`/`ucmp`: operands of
/// at most 32 bits): see the module documentation.
///
/// # Errors
/// `x86:intrinsic` for a `bswap` of an odd number of bytes (the verifier
/// rejects it); what the operands' placement reports.
pub fn intrinsic<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    which: Intrinsic<'a>,
    args: &'a [Value],
) -> CgResult<()> {
    use Intrinsic as I;
    let Some(bits) = cx.types().int_bits(inst.ty()) else {
        return cx.bug("an integer intrinsic without an integer result");
    };
    match (which, args) {
        (I::Scmp | I::Ucmp, &[a, b]) => three_way(cx, inst, which == I::Scmp, bits, a, b),
        (I::Bswap | I::Ctpop | I::Ctlz | I::Cttz | I::BitReverse | I::Abs, &[x, ..]) => {
            if which == I::Bswap && !bits.is_multiple_of(16) {
                return unsupported(crate::call::INTRINSIC);
            }
            let [r] = regs(cx, [x])?;
            unary(cx, which, gp(r), bits)?;
            cx.finish(inst, r)
        }
        (I::Smax | I::Smin | I::Umax | I::Umin, &[a, b]) => {
            let [ra, rb] = regs(cx, [a, b])?;
            min_max(cx, which, gp(ra), gp(rb), bits)?;
            cx.finish(inst, ra)
        }
        (I::SaddSat | I::UaddSat | I::SsubSat | I::UsubSat, &[a, b]) => {
            let signed = matches!(which, I::SaddSat | I::SsubSat);
            let add = matches!(which, I::SaddSat | I::UaddSat);
            let [ra, rb] = regs(cx, [a, b])?;
            add_sat(cx, signed, add, gp(ra), gp(rb), bits)?;
            cx.finish(inst, ra)
        }
        (I::SshlSat | I::UshlSat, &[a, b]) => {
            let [ra, rb] = regs(cx, [a, b])?;
            shl_sat(cx, which == I::SshlSat, gp(ra), gp(rb), bits)?;
            cx.finish(inst, ra)
        }
        (I::Fshl | I::Fshr, &[a, b, c]) => funnel(cx, inst, which == I::Fshl, [a, b, c], bits),
        _ => cx.bug("an integer intrinsic with the wrong operands"),
    }
}

// ---- one operand ----

/// `ctlz`, `cttz`, `ctpop`, `bswap`, `bitreverse` or `abs` of the
/// `bits`-bit value in `r`, in place (`eax` and the flags written).
fn unary(cx: &mut FnCx<'_, X86>, which: Intrinsic<'_>, r: Reg, bits: u32) -> CgResult<()> {
    use Intrinsic as I;
    match which {
        I::Ctlz => {
            if bits == 32 {
                // `bsr` of 0 sets ZF and leaves its destination undefined.
                let done = cx.new_label();
                put(cx, mc::bsr_rr(Size::D, r, r));
                branch_cond(cx, Cond::Ne, done)?;
                mov_imm(cx, r, 63);
                cx.bind(done)?;
            } else {
                // To the top, with the bit below the value set: a zero
                // value counts `bits`.
                let s = 32 - bits;
                put(cx, mc::shl_ri(Size::D, r, s as u8));
                put(cx, mc::or_ri(Size::D, r, imm(1 << (s - 1))));
                put(cx, mc::bsr_rr(Size::D, r, r));
            }
            put(cx, mc::xor_ri(Size::D, r, 31));
        }
        I::Cttz => {
            if bits == 32 {
                let done = cx.new_label();
                put(cx, mc::bsf_rr(Size::D, r, r));
                branch_cond(cx, Cond::Ne, done)?;
                mov_imm(cx, r, 32);
                cx.bind(done)?;
            } else {
                // Bit `bits` set: a zero value counts `bits`, and the
                // unspecified bits above it are never the lowest set.
                put(cx, mc::or_ri(Size::D, r, imm(1 << bits)));
                put(cx, mc::bsf_rr(Size::D, r, r));
            }
        }
        I::Ctpop => {
            extend_to(cx, r, bits, Ext::Zero);
            popcount(cx, r);
        }
        I::Bswap => {
            put(cx, mc::bswap(Size::D, r));
            if bits != 32 {
                put(cx, mc::shr_ri(Size::D, r, (32 - bits) as u8));
            }
        }
        I::BitReverse => {
            reverse_bits(cx, r);
            if bits != 32 {
                put(cx, mc::shr_ri(Size::D, r, (32 - bits) as u8));
            }
        }
        I::Abs => {
            // (x ^ s) - s with s the sign: the minimum maps to itself.
            extend_to(cx, r, bits, Ext::Sign);
            put(cx, mc::mov_rr(Size::D, EAX, r));
            put(cx, mc::sar_ri(Size::D, EAX, 31));
            put(cx, mc::xor_rr(Size::D, r, EAX));
            put(cx, mc::sub_rr(Size::D, r, EAX));
        }
        _ => return cx.bug("an integer intrinsic without a lowering"),
    }
    Ok(())
}

/// The set bits of `r` in `r`: `llc`'s SWAR sequence (`eax` and the
/// flags written).
fn popcount(cx: &mut FnCx<'_, X86>, r: Reg) {
    put(cx, mc::mov_rr(Size::D, EAX, r));
    put(cx, mc::shr_ri(Size::D, EAX, 1));
    put(cx, mc::and_ri(Size::D, EAX, imm(0x5555_5555)));
    put(cx, mc::sub_rr(Size::D, r, EAX));
    put(cx, mc::mov_rr(Size::D, EAX, r));
    put(cx, mc::and_ri(Size::D, EAX, imm(0x3333_3333)));
    put(cx, mc::shr_ri(Size::D, r, 2));
    put(cx, mc::and_ri(Size::D, r, imm(0x3333_3333)));
    put(cx, mc::add_rr(Size::D, r, EAX));
    put(cx, mc::mov_rr(Size::D, EAX, r));
    put(cx, mc::shr_ri(Size::D, EAX, 4));
    put(cx, mc::add_rr(Size::D, r, EAX));
    put(cx, mc::and_ri(Size::D, r, imm(0x0f0f_0f0f)));
    put(cx, mc::imul_rri(Size::D, r, r, imm(0x0101_0101)));
    put(cx, mc::shr_ri(Size::D, r, 24));
}

/// The bits of `r` reversed, in `r`: `bswap`, then nibbles, pairs and
/// bits swapped by masks (`eax` and the flags written).
fn reverse_bits(cx: &mut FnCx<'_, X86>, r: Reg) {
    put(cx, mc::bswap(Size::D, r));
    for (s, m) in [(4, 0x0f0f_0f0f), (2, 0x3333_3333), (1, 0x5555_5555)] {
        put(cx, mc::mov_rr(Size::D, EAX, r));
        put(cx, mc::shr_ri(Size::D, EAX, s));
        put(cx, mc::and_ri(Size::D, EAX, imm(m)));
        put(cx, mc::and_ri(Size::D, r, imm(m)));
        put(cx, mc::shl_ri(Size::D, r, s));
        put(cx, mc::or_rr(Size::D, r, EAX));
    }
}

// ---- two operands ----

/// `x = which(x, y)` for the min/max: both extended, `cmp`, and `y` copied
/// over `x` unless `x` is kept (`y` and the flags written; `x == y` is
/// allowed).
fn min_max(
    cx: &mut FnCx<'_, X86>,
    which: Intrinsic<'_>,
    x: Reg,
    y: Reg,
    bits: u32,
) -> CgResult<()> {
    use Intrinsic as I;
    let (ext, keep_x) = match which {
        I::Smax => (Ext::Sign, Cond::Ge),
        I::Smin => (Ext::Sign, Cond::Le),
        I::Umax => (Ext::Zero, Cond::Ae),
        _ => (Ext::Zero, Cond::Be),
    };
    if x == y {
        return Ok(());
    }
    extend_to(cx, x, bits, ext);
    extend_to(cx, y, bits, ext);
    put(cx, mc::cmp_rr(Size::D, x, y));
    let done = cx.new_label();
    branch_cond(cx, keep_x, done)?;
    put(cx, mc::mov_rr(Size::D, x, y));
    cx.bind(done)
}

/// `x = {s,u}{add,sub}.sat(x, y)` at `bits` bits (`y`, `eax` and the flags
/// written; `x == y` is allowed). At 32 bits the operation's own carry or
/// overflow decides, the signed bound chosen from `x` before it; below,
/// the operands are extended, the operation is exact in 32 bits, and the
/// sum or difference is clamped to the width's bounds.
fn add_sat(
    cx: &mut FnCx<'_, X86>,
    signed: bool,
    add: bool,
    x: Reg,
    y: Reg,
    bits: u32,
) -> CgResult<()> {
    let op = if add { mc::add_rr } else { mc::sub_rr };
    let done = cx.new_label();
    if bits == 32 {
        if signed {
            // Overflow only towards the first operand's sign.
            put(cx, mc::mov_rr(Size::D, EAX, x));
            put(cx, mc::sar_ri(Size::D, EAX, 31));
            put(cx, mc::xor_ri(Size::D, EAX, imm(0x7fff_ffff)));
            put(cx, op(Size::D, x, y));
            branch_cond(cx, Cond::No, done)?;
            put(cx, mc::mov_rr(Size::D, x, EAX));
        } else {
            put(cx, op(Size::D, x, y));
            branch_cond(cx, Cond::Ae, done)?;
            mov_imm(cx, x, if add { u32::MAX } else { 0 });
        }
        return cx.bind(done);
    }
    let ext = if signed { Ext::Sign } else { Ext::Zero };
    extend_to(cx, x, bits, ext);
    if y != x {
        extend_to(cx, y, bits, ext);
    }
    put(cx, op(Size::D, x, y));
    if signed {
        let max = (1u32 << (bits - 1)) - 1;
        let low = cx.new_label();
        put(cx, mc::cmp_ri(Size::D, x, imm(max)));
        branch_cond(cx, Cond::Le, low)?;
        mov_imm(cx, x, max);
        branch(cx, done)?;
        cx.bind(low)?;
        put(cx, mc::cmp_ri(Size::D, x, imm(!max)));
        branch_cond(cx, Cond::Ge, done)?;
        mov_imm(cx, x, !max);
    } else if add {
        put(cx, mc::cmp_ri(Size::D, x, imm(mask(bits))));
        branch_cond(cx, Cond::Be, done)?;
        mov_imm(cx, x, mask(bits));
    } else {
        // Both operands are below 2^31: the difference is negative
        // exactly when it saturates.
        put(cx, mc::test_rr(Size::D, x, x));
        branch_cond(cx, Cond::Ns, done)?;
        mov_imm(cx, x, 0);
    }
    cx.bind(done)
}

/// `x = {s,u}shl.sat(x, k)` at `bits` bits: the value moved to the top of
/// the register, shifted by `k` and back; when that changes it, the
/// saturated value (all ones, or the extreme of its sign); then moved
/// back down. An amount of `bits` or more is poison. Writes `k`, `eax`,
/// `ecx`, `edx` and the flags; `x == k` is allowed.
fn shl_sat(cx: &mut FnCx<'_, X86>, signed: bool, x: Reg, k: Reg, bits: u32) -> CgResult<()> {
    let up = (32 - bits) as u8;
    put(cx, mc::mov_rr(Size::D, ECX, k));
    extend_to(cx, ECX, bits, Ext::Zero);
    if up != 0 {
        put(cx, mc::shl_ri(Size::D, x, up));
    }
    put(cx, mc::mov_rr(Size::D, EAX, x));
    put(cx, mc::shl_rcl(Size::D, EAX));
    put(cx, mc::mov_rr(Size::D, EDX, EAX));
    put(cx, if signed { mc::sar_rcl(Size::D, EDX) } else { mc::shr_rcl(Size::D, EDX) });
    put(cx, mc::cmp_rr(Size::D, EDX, x));
    let done = cx.new_label();
    branch_cond(cx, Cond::E, done)?;
    if signed {
        put(cx, mc::mov_rr(Size::D, EAX, x));
        put(cx, mc::sar_ri(Size::D, EAX, 31));
        put(cx, mc::xor_ri(Size::D, EAX, imm(0x7fff_ffff)));
    } else {
        mov_imm(cx, EAX, u32::MAX);
    }
    cx.bind(done)?;
    put(cx, mc::mov_rr(Size::D, x, EAX));
    if up != 0 {
        put(cx, if signed { mc::sar_ri(Size::D, x, up) } else { mc::shr_ri(Size::D, x, up) });
    }
    Ok(())
}

// ---- funnel shifts ----

/// `fshl` (`left`) or `fshr` of `[a, b, c]` at `bits` bits: the
/// concatenation `a:b` shifted by `c` modulo the width, its high (`fshl`)
/// or low half. At 32 bits `shld`/`shrd` (which take the amount modulo
/// 32); below, `b` zero-extended in `eax`, the two halves shifted
/// separately and or-ed, the variable amount reduced modulo `bits` by an
/// `and` (a power of two) or a `div`.
#[allow(clippy::many_single_char_names, reason = "the operands are LangRef's a, b, c")]
fn funnel<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    left: bool,
    [a, b, c]: [Value; 3],
    bits: u32,
) -> CgResult<()> {
    // At one bit every amount is 0 modulo the width.
    let amount = crate::common::const_int(cx, c).or((bits == 1).then_some(0));
    if let Some(amount) = amount {
        let k = (amount & mask(bits)) % bits;
        if k == 0 {
            let [r] = regs(cx, [if left { a } else { b }])?;
            return cx.finish(inst, r);
        }
        let [ra, rb] = regs(cx, [a, b])?;
        let (x, y) = (gp(ra), gp(rb));
        if bits == 32 {
            if left {
                put(cx, mc::shld_rri(Size::D, x, y, k as u8));
                return cx.finish(inst, ra);
            }
            put(cx, mc::shrd_rri(Size::D, y, x, k as u8));
            return cx.finish(inst, rb);
        }
        put(cx, mc::mov_rr(Size::D, EAX, y));
        extend_to(cx, EAX, bits, Ext::Zero);
        let (kx, ky) = if left { (k, bits - k) } else { (bits - k, k) };
        // fshl: (a << k) | (b >> (N - k)); fshr: (a << (N - k)) | (b >> k).
        put(cx, mc::shl_ri(Size::D, x, kx as u8));
        put(cx, mc::shr_ri(Size::D, EAX, ky as u8));
        put(cx, mc::or_rr(Size::D, x, EAX));
        return cx.finish(inst, ra);
    }
    let [ra, rb, rc] = regs(cx, [a, b, c])?;
    let (x, y, z) = (gp(ra), gp(rb), gp(rc));
    if bits == 32 {
        put(cx, mc::mov_rr(Size::D, ECX, z));
        if left {
            put(cx, mc::shld_rrcl(Size::D, x, y));
            return cx.finish(inst, ra);
        }
        put(cx, mc::shrd_rrcl(Size::D, y, x));
        return cx.finish(inst, rb);
    }
    // k = c mod N, in edx.
    put(cx, mc::mov_rr(Size::D, EDX, z));
    if bits.is_power_of_two() {
        put(cx, mc::and_ri(Size::D, EDX, imm(bits - 1)));
    } else {
        put(cx, mc::mov_rr(Size::D, EAX, EDX));
        extend_to(cx, EAX, bits, Ext::Zero);
        mov_imm(cx, EDX, 0);
        mov_imm(cx, ECX, bits);
        put(cx, mc::div_r(Size::D, ECX));
    }
    // b zero-extended in eax; a shifted by k (fshl) or N - k (fshr), b by
    // the other: a shift by N moves b out entirely, or a's bits above the
    // width.
    put(cx, mc::mov_rr(Size::D, EAX, y));
    extend_to(cx, EAX, bits, Ext::Zero);
    put(cx, mc::mov_rr(Size::D, ECX, EDX));
    put(cx, if left { mc::shl_rcl(Size::D, x) } else { mc::shr_rcl(Size::D, EAX) });
    mov_imm(cx, ECX, bits);
    put(cx, mc::sub_rr(Size::D, ECX, EDX));
    put(cx, if left { mc::shr_rcl(Size::D, EAX) } else { mc::shl_rcl(Size::D, x) });
    put(cx, mc::or_rr(Size::D, x, EAX));
    cx.finish(inst, ra)
}

// ---- three-way compares ----

/// `scmp`/`ucmp` (`signed`) of `a` and `b` (1 to 32 bits): -1, 0 or 1 in a
/// result of `bits` bits: both extended, `cmp`, `setg`/`seta` into `al`
/// and `setl`/`setb` into `cl`, their difference sign-extended. A result
/// of more than 32 bits (class `Mem`) is stored as two words.
#[allow(clippy::many_single_char_names, reason = "the operands a, b")]
fn three_way<'a>(
    cx: &mut FnCx<'a, X86>,
    inst: Inst<'a>,
    signed: bool,
    bits: u32,
    a: Value,
    b: Value,
) -> CgResult<()> {
    let Some(width) = int_width(cx, a) else {
        return cx.bug("a three-way compare of no integer");
    };
    let (gt, lt) = if signed { (Cond::G, Cond::L) } else { (Cond::A, Cond::B) };
    let ext = if signed { Ext::Sign } else { Ext::Zero };
    let [ra, rb] = regs(cx, [a, b])?;
    let (x, y) = (gp(ra), gp(rb));
    extend_to(cx, x, width, ext);
    if y != x {
        extend_to(cx, y, width, ext);
    }
    put(cx, mc::cmp_rr(Size::D, x, y));
    put(cx, mc::setcc_r(gt, EAX));
    put(cx, mc::setcc_r(lt, ECX));
    put(cx, mc::sub_rr(Size::B, EAX, ECX));
    put(cx, mc::movsx_rr(Size::D, x, Size::B, EAX));
    if bits <= 32 {
        return cx.finish(inst, ra);
    }
    let dst = cx.def_mem(inst)?;
    crate::common::store_gpr(cx, x, dst, 4);
    put(cx, mc::sar_ri(Size::D, x, 31));
    crate::common::store_gpr(cx, x, dst.offset_by(4), 4);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_and_immediates() {
        assert_eq!(mask(1), 1);
        assert_eq!(mask(24), 0x00ff_ffff);
        assert_eq!(mask(32), u32::MAX);
        assert_eq!(imm(0xffff_ffff), -1);
        assert_eq!(imm(0x7fff_ffff), i64::from(i32::MAX));
    }
}
