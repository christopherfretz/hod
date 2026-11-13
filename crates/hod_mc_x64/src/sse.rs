//! Scalar SSE/SSE2 floating point: moves, arithmetic, the packed logical
//! operations used on scalars, compares and conversions
//! (`spec/mc-x64.md` "Scalar floating point").
//!
//! Every instruction here is in the `0F` map with a mandatory prefix, which
//! precedes REX: `F3` for single and `F2` for double precision scalars,
//! none and `66` for the packed single and double forms.

use crate::inst::{Inst, Op, Rm, imm, modrm};
use crate::reg::{FpCmp, FpSize, Mem, Reg, Size, Xmm};

/// `0F opcode` with mandatory prefix `prefix` (0, `66`, `F2` or `F3`).
#[inline]
pub(crate) const fn op(prefix: u8, opcode: u8) -> Op {
    let op = Op::two(opcode);
    match prefix {
        0 => op,
        0x66 => Op { p66: true, ..op },
        _ => op.rep(prefix),
    }
}

/// The scalar prefix: `F3` single, `F2` double.
#[inline]
const fn scalar(fp: FpSize) -> u8 {
    match fp {
        FpSize::S => 0xf3,
        FpSize::D => 0xf2,
    }
}

/// The packed prefix: none for single, `66` for double.
#[inline]
pub(crate) const fn packed(fp: FpSize) -> u8 {
    match fp {
        FpSize::S => 0,
        FpSize::D => 0x66,
    }
}

/// `ModRM` with two `xmm` operands.
#[inline]
pub(crate) const fn xx(op: Op, reg: Xmm, rm: Xmm) -> Inst {
    modrm(op, reg.code(), false, Rm::Reg(rm.as_gpr()), false)
}

/// `ModRM` with an `xmm` in reg and a memory operand.
#[inline]
pub(crate) const fn xm(op: Op, reg: Xmm, m: Mem) -> Inst {
    modrm(op, reg.code(), false, Rm::Mem(m), false)
}

/// Scalar `op xmm, xmm/m` (`F3`/`F2 0F opcode /r`).
#[inline]
const fn scalar_rr(opcode: u8, fp: FpSize, dst: Xmm, src: Xmm) -> Inst {
    xx(op(scalar(fp), opcode), dst, src)
}

#[inline]
const fn scalar_rm(opcode: u8, fp: FpSize, dst: Xmm, src: Mem) -> Inst {
    xm(op(scalar(fp), opcode), dst, src)
}

/// Packed `op xmm, xmm/m` (`0F opcode /r`, `66 0F opcode /r`).
#[inline]
pub(crate) const fn packed_rr(opcode: u8, fp: FpSize, dst: Xmm, src: Xmm) -> Inst {
    xx(op(packed(fp), opcode), dst, src)
}

#[inline]
pub(crate) const fn packed_rm(opcode: u8, fp: FpSize, dst: Xmm, src: Mem) -> Inst {
    xm(op(packed(fp), opcode), dst, src)
}

/// `MOVSS`/`MOVSD xmm, xmm` (`F3`/`F2 0F 10 /r`, `dst` in reg): copies
/// the low element and keeps the rest of `dst`.
#[inline]
pub const fn movs_rr(fp: FpSize, dst: Xmm, src: Xmm) -> Inst {
    scalar_rr(0x10, fp, dst, src)
}

/// `MOVSS`/`MOVSD xmm, m32/m64` (`F3`/`F2 0F 10 /r`): a load, which
/// zeroes the rest of `dst`.
#[inline]
pub const fn movs_rm(fp: FpSize, dst: Xmm, src: Mem) -> Inst {
    scalar_rm(0x10, fp, dst, src)
}

/// `MOVSS`/`MOVSD m32/m64, xmm` (`F3`/`F2 0F 11 /r`): a store.
#[inline]
pub const fn movs_mr(fp: FpSize, dst: Mem, src: Xmm) -> Inst {
    xm(op(scalar(fp), 0x11), src, dst)
}

/// `MOVAPS`/`MOVAPD xmm, xmm` (`0F 28 /r`, `66 0F 28 /r`): all 128 bits.
#[inline]
pub const fn movap_rr(fp: FpSize, dst: Xmm, src: Xmm) -> Inst {
    packed_rr(0x28, fp, dst, src)
}

/// `MOVAPS`/`MOVAPD xmm, m128` (`0F 28 /r`): `src` 16-byte aligned.
#[inline]
pub const fn movap_rm(fp: FpSize, dst: Xmm, src: Mem) -> Inst {
    packed_rm(0x28, fp, dst, src)
}

/// `MOVAPS`/`MOVAPD m128, xmm` (`0F 29 /r`): `dst` 16-byte aligned.
#[inline]
pub const fn movap_mr(fp: FpSize, dst: Mem, src: Xmm) -> Inst {
    xm(op(packed(fp), 0x29), src, dst)
}

/// `MOVUPS`/`MOVUPD xmm, m128` (`0F 10 /r`, `66 0F 10 /r`): unaligned.
#[inline]
pub const fn movup_rm(fp: FpSize, dst: Xmm, src: Mem) -> Inst {
    packed_rm(0x10, fp, dst, src)
}

/// `MOVUPS`/`MOVUPD m128, xmm` (`0F 11 /r`): unaligned.
#[inline]
pub const fn movup_mr(fp: FpSize, dst: Mem, src: Xmm) -> Inst {
    xm(op(packed(fp), 0x11), src, dst)
}

/// `MOVDQA xmm, xmm` (`66 0F 6F /r`).
#[inline]
pub const fn movdqa_rr(dst: Xmm, src: Xmm) -> Inst {
    xx(op(0x66, 0x6f), dst, src)
}

/// `MOVDQA xmm, m128` (`66 0F 6F /r`): `src` 16-byte aligned.
#[inline]
pub const fn movdqa_rm(dst: Xmm, src: Mem) -> Inst {
    xm(op(0x66, 0x6f), dst, src)
}

/// `MOVDQA m128, xmm` (`66 0F 7F /r`): `dst` 16-byte aligned.
#[inline]
pub const fn movdqa_mr(dst: Mem, src: Xmm) -> Inst {
    xm(op(0x66, 0x7f), src, dst)
}

/// `MOVDQU xmm, m128` (`F3 0F 6F /r`): unaligned.
#[inline]
pub const fn movdqu_rm(dst: Xmm, src: Mem) -> Inst {
    xm(op(0xf3, 0x6f), dst, src)
}

/// `MOVDQU m128, xmm` (`F3 0F 7F /r`): unaligned.
#[inline]
pub const fn movdqu_mr(dst: Mem, src: Xmm) -> Inst {
    xm(op(0xf3, 0x7f), src, dst)
}

/// `MOVUPS`/`MOVUPD xmm, xmm` (`0F 10 /r`, `66 0F 10 /r`, `dst` in reg).
#[inline]
pub const fn movup_rr(fp: FpSize, dst: Xmm, src: Xmm) -> Inst {
    packed_rr(0x10, fp, dst, src)
}

/// `MOVDQU xmm, xmm` (`F3 0F 6F /r`).
#[inline]
pub const fn movdqu_rr(dst: Xmm, src: Xmm) -> Inst {
    xx(op(0xf3, 0x6f), dst, src)
}

/// `MOVQ xmm, xmm` (`F3 0F 7E /r`): the low 64 bits, the high 64 zeroed.
#[inline]
pub const fn movq_rr(dst: Xmm, src: Xmm) -> Inst {
    xx(op(0xf3, 0x7e), dst, src)
}

checked! {
    /// `MOVD xmm, m32` (`66 0F 6E /r`) / `MOVQ xmm, m64` (`F3 0F 7E /r`):
    /// a load into the low element, the rest zeroed.
    pub fn movd_rm / try_movd_rm(size: Size, dst: Xmm, src: Mem)
    requires matches!(size, Size::D | Size::Q);
    {
        match size {
            Size::Q => xm(op(0xf3, 0x7e), dst, src),
            _ => xm(op(0x66, 0x6e), dst, src),
        }
    }

    /// `MOVD m32, xmm` (`66 0F 7E /r`) / `MOVQ m64, xmm` (`66 0F D6 /r`):
    /// a store of the low 32 or 64 bits.
    pub fn movd_mr / try_movd_mr(size: Size, dst: Mem, src: Xmm)
    requires matches!(size, Size::D | Size::Q);
    {
        match size {
            Size::Q => xm(op(0x66, 0xd6), src, dst),
            _ => xm(op(0x66, 0x7e), src, dst),
        }
    }
}

/// `66 [REX.W] 0F opcode`: `movd` for 32 bits, `movq` for 64.
#[inline]
const fn movd_op(opcode: u8, size: Size) -> Op {
    Op { p66: true, ..Op::two(opcode).sized(size, 0) }
}

checked! {
    /// `MOVD xmm, r32` / `MOVQ xmm, r64` (`66 [REX.W] 0F 6E /r`): the
    /// general register's bits into the low element, the rest zeroed.
    pub fn movd_xr / try_movd_xr(size: Size, dst: Xmm, src: Reg)
    requires matches!(size, Size::D | Size::Q);
    { modrm(movd_op(0x6e, size), dst.code(), false, Rm::Reg(src), false) }

    /// `MOVD r32, xmm` / `MOVQ r64, xmm` (`66 [REX.W] 0F 7E /r`, the
    /// `xmm` in reg): the low 32 or 64 bits.
    pub fn movd_rx / try_movd_rx(size: Size, dst: Reg, src: Xmm)
    requires matches!(size, Size::D | Size::Q);
    { modrm(movd_op(0x7e, size), src.code(), false, Rm::Reg(dst), false) }
}

family! {
    (fp: FpSize, dst: Xmm, src: Xmm) => scalar_rr;
    /// `ADDSS`/`ADDSD xmm, xmm` (`F3`/`F2 0F 58 /r`).
    pub fn adds_rr = (0x58);
    /// `SUBSS`/`SUBSD xmm, xmm` (`F3`/`F2 0F 5C /r`).
    pub fn subs_rr = (0x5c);
    /// `MULSS`/`MULSD xmm, xmm` (`F3`/`F2 0F 59 /r`).
    pub fn muls_rr = (0x59);
    /// `DIVSS`/`DIVSD xmm, xmm` (`F3`/`F2 0F 5E /r`).
    pub fn divs_rr = (0x5e);
    /// `SQRTSS`/`SQRTSD xmm, xmm` (`F3`/`F2 0F 51 /r`).
    pub fn sqrts_rr = (0x51);
    /// `MINSS`/`MINSD xmm, xmm` (`F3`/`F2 0F 5D /r`): `dst < src ? dst :
    /// src`, so `src` when either is NaN or both are zero.
    pub fn mins_rr = (0x5d);
    /// `MAXSS`/`MAXSD xmm, xmm` (`F3`/`F2 0F 5F /r`): `dst > src ? dst :
    /// src`, so `src` when either is NaN or both are zero.
    pub fn maxs_rr = (0x5f);
}

family! {
    (fp: FpSize, dst: Xmm, src: Mem) => scalar_rm;
    /// `ADDSS`/`ADDSD xmm, m32/m64`.
    pub fn adds_rm = (0x58);
    /// `SUBSS`/`SUBSD xmm, m32/m64`.
    pub fn subs_rm = (0x5c);
    /// `MULSS`/`MULSD xmm, m32/m64`.
    pub fn muls_rm = (0x59);
    /// `DIVSS`/`DIVSD xmm, m32/m64`.
    pub fn divs_rm = (0x5e);
    /// `SQRTSS`/`SQRTSD xmm, m32/m64`.
    pub fn sqrts_rm = (0x51);
    /// `MINSS`/`MINSD xmm, m32/m64`.
    pub fn mins_rm = (0x5d);
    /// `MAXSS`/`MAXSD xmm, m32/m64`.
    pub fn maxs_rm = (0x5f);
}

family! {
    (fp: FpSize, dst: Xmm, src: Xmm) => packed_rr;
    /// `ANDPS`/`ANDPD xmm, xmm` (`[66] 0F 54 /r`): absolute value with a
    /// mask.
    pub fn andp_rr = (0x54);
    /// `ANDNPS`/`ANDNPD xmm, xmm` (`[66] 0F 55 /r`): `dst = !dst & src`.
    pub fn andnp_rr = (0x55);
    /// `ORPS`/`ORPD xmm, xmm` (`[66] 0F 56 /r`).
    pub fn orp_rr = (0x56);
    /// `XORPS`/`XORPD xmm, xmm` (`[66] 0F 57 /r`): negation with a mask;
    /// with `dst == src` the zero idiom.
    pub fn xorp_rr = (0x57);
    /// `UCOMISS`/`UCOMISD xmm, xmm` (`[66] 0F 2E /r`): compares the low
    /// elements into ZF, PF, CF (PF set when unordered); signals only on
    /// a signaling NaN.
    pub fn ucomis_rr = (0x2e);
    /// `COMISS`/`COMISD xmm, xmm` (`[66] 0F 2F /r`): as `ucomis`, but
    /// signals on any NaN.
    pub fn comis_rr = (0x2f);
}

family! {
    (fp: FpSize, dst: Xmm, src: Mem) => packed_rm;
    /// `ANDPS`/`ANDPD xmm, m128`: `src` 16-byte aligned.
    pub fn andp_rm = (0x54);
    /// `ANDNPS`/`ANDNPD xmm, m128`: `src` 16-byte aligned.
    pub fn andnp_rm = (0x55);
    /// `ORPS`/`ORPD xmm, m128`: `src` 16-byte aligned.
    pub fn orp_rm = (0x56);
    /// `XORPS`/`XORPD xmm, m128`: `src` 16-byte aligned.
    pub fn xorp_rm = (0x57);
    /// `UCOMISS`/`UCOMISD xmm, m32/m64`.
    pub fn ucomis_rm = (0x2e);
    /// `COMISS`/`COMISD xmm, m32/m64`.
    pub fn comis_rm = (0x2f);
}

/// `CMPSS`/`CMPSD xmm, xmm, imm8` (`F3`/`F2 0F C2 /r ib`): the low
/// element of `dst` becomes all ones where `pred` holds, else zero.
#[inline]
pub const fn cmps_rr(pred: FpCmp, fp: FpSize, dst: Xmm, src: Xmm) -> Inst {
    imm(scalar_rr(0xc2, fp, dst, src), pred as i64, 1)
}

/// `CMPSS`/`CMPSD xmm, m32/m64, imm8`.
#[inline]
pub const fn cmps_rm(pred: FpCmp, fp: FpSize, dst: Xmm, src: Mem) -> Inst {
    imm(scalar_rm(0xc2, fp, dst, src), pred as i64, 1)
}

/// `F3`/`F2 [REX.W] 0F opcode` of a conversion with a 32- or 64-bit
/// general operand.
#[inline]
const fn cvt_op(opcode: u8, fp: FpSize, size: Size) -> Op {
    op(scalar(fp), opcode).sized(size, 0)
}

checked! {
    /// `CVTSI2SS`/`CVTSI2SD xmm, r32/r64` (`F3`/`F2 [REX.W] 0F 2A /r`):
    /// the signed integer, rounded by MXCSR; the rest of `dst` kept.
    pub fn cvtsi2s_rr / try_cvtsi2s_rr(fp: FpSize, dst: Xmm, size: Size, src: Reg)
    requires matches!(size, Size::D | Size::Q);
    { modrm(cvt_op(0x2a, fp, size), dst.code(), false, Rm::Reg(src), false) }

    /// `CVTSI2SS`/`CVTSI2SD xmm, m32/m64`.
    pub fn cvtsi2s_rm / try_cvtsi2s_rm(fp: FpSize, dst: Xmm, size: Size, src: Mem)
    requires matches!(size, Size::D | Size::Q);
    { modrm(cvt_op(0x2a, fp, size), dst.code(), false, Rm::Mem(src), false) }

    /// `CVTTSS2SI`/`CVTTSD2SI r32/r64, xmm` (`F3`/`F2 [REX.W] 0F 2C /r`):
    /// toward zero; the "integer indefinite" value (the most negative) when
    /// out of range or NaN.
    pub fn cvtts2si_rr / try_cvtts2si_rr(size: Size, dst: Reg, fp: FpSize, src: Xmm)
    requires matches!(size, Size::D | Size::Q);
    { modrm(cvt_op(0x2c, fp, size), dst.code(), false, Rm::Reg(src.as_gpr()), false) }

    /// `CVTTSS2SI`/`CVTTSD2SI r32/r64, m32/m64`.
    pub fn cvtts2si_rm / try_cvtts2si_rm(size: Size, dst: Reg, fp: FpSize, src: Mem)
    requires matches!(size, Size::D | Size::Q);
    { modrm(cvt_op(0x2c, fp, size), dst.code(), false, Rm::Mem(src), false) }

    /// `CVTSS2SI`/`CVTSD2SI r32/r64, xmm` (`F3`/`F2 [REX.W] 0F 2D /r`):
    /// rounded as MXCSR says (`lrint`); the "integer indefinite" value
    /// when out of range or NaN.
    pub fn cvts2si_rr / try_cvts2si_rr(size: Size, dst: Reg, fp: FpSize, src: Xmm)
    requires matches!(size, Size::D | Size::Q);
    { modrm(cvt_op(0x2d, fp, size), dst.code(), false, Rm::Reg(src.as_gpr()), false) }

    /// `CVTSS2SI`/`CVTSD2SI r32/r64, m32/m64`.
    pub fn cvts2si_rm / try_cvts2si_rm(size: Size, dst: Reg, fp: FpSize, src: Mem)
    requires matches!(size, Size::D | Size::Q);
    { modrm(cvt_op(0x2d, fp, size), dst.code(), false, Rm::Mem(src), false) }
}

/// `CVTSS2SD xmm, xmm` (`F3 0F 5A /r`): single to double, exact.
#[inline]
pub const fn cvtss2sd_rr(dst: Xmm, src: Xmm) -> Inst {
    scalar_rr(0x5a, FpSize::S, dst, src)
}

/// `CVTSS2SD xmm, m32`.
#[inline]
pub const fn cvtss2sd_rm(dst: Xmm, src: Mem) -> Inst {
    scalar_rm(0x5a, FpSize::S, dst, src)
}

/// `CVTSD2SS xmm, xmm` (`F2 0F 5A /r`): double to single, rounded by
/// MXCSR.
#[inline]
pub const fn cvtsd2ss_rr(dst: Xmm, src: Xmm) -> Inst {
    scalar_rr(0x5a, FpSize::D, dst, src)
}

/// `CVTSD2SS xmm, m64`.
#[inline]
pub const fn cvtsd2ss_rm(dst: Xmm, src: Mem) -> Inst {
    scalar_rm(0x5a, FpSize::D, dst, src)
}
