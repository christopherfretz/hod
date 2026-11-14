//! Scalar floating point: arithmetic, compares, selects, conversions, the
//! FMOV forms, and the floating-point constant materializer
//! (`spec/mc-a64.md` "Scalar floating point" and "Floating-point constant
//! materialization").

use crate::imm::encode_fp_imm8;
use crate::reg::{Cond, FpFormat, FpSize, Reg, RegZr, Size, VReg, VecBits};
use crate::simd::movi_mask;
use crate::wide::mov_imm;

/// Floating-point data-processing (1 source). `opcode` is bits 20:15.
#[inline]
const fn fp_data1(opcode: u32, sz: FpSize, rd: VReg, rn: VReg) -> u32 {
    0x1e20_4000 | sz.ftype() | opcode << 15 | rn.code() << 5 | rd.code()
}

/// Floating-point data-processing (2 source). `opcode` is bits 15:12.
#[inline]
const fn fp_data2(opcode: u32, sz: FpSize, rd: VReg, rn: VReg, rm: VReg) -> u32 {
    0x1e20_0800 | sz.ftype() | rm.code() << 16 | opcode << 12 | rn.code() << 5 | rd.code()
}

/// Floating-point data-processing (3 source). `o1` is bit 21, `o0` bit 15.
#[inline]
const fn fp_data3(o1: u32, o0: u32, sz: FpSize, rd: VReg, rn: VReg, rm: VReg, ra: VReg) -> u32 {
    0x1f00_0000
        | sz.ftype()
        | o1 << 21
        | rm.code() << 16
        | o0 << 15
        | ra.code() << 10
        | rn.code() << 5
        | rd.code()
}

/// Floating-point compare. `opcode2` is bits 4:0; `rm` is the register
/// field, zero in the compare-with-zero forms.
#[inline]
const fn fp_compare(opcode2: u32, sz: FpSize, rn: VReg, rm: u32) -> u32 {
    0x1e20_2000 | sz.ftype() | rm << 16 | rn.code() << 5 | opcode2
}

/// Floating-point conditional compare. `op` is bit 4.
#[inline]
const fn fp_cond_compare(op: u32, sz: FpSize, rn: VReg, rm: VReg, nzcv: u32, cond: Cond) -> u32 {
    0x1e20_0400
        | sz.ftype()
        | rm.code() << 16
        | cond.code() << 12
        | rn.code() << 5
        | op << 4
        | (nzcv & 0xf)
}

/// Conversion between floating-point and integer. `rmode` is bits 20:19,
/// `opcode` bits 18:16; `ftype` is in place.
#[inline]
const fn fp_int(rmode: u32, opcode: u32, sz: Size, ftype: u32, rd: u32, rn: u32) -> u32 {
    0x1e20_0000 | sz.sf() | ftype | rmode << 19 | opcode << 16 | rn << 5 | rd
}

/// A conversion from floating point to an integer register.
#[inline]
const fn fp_to_int(rmode: u32, opcode: u32, sz: Size, rd: RegZr, fsz: FpSize, rn: VReg) -> u32 {
    fp_int(rmode, opcode, sz, fsz.ftype(), rd.code(), rn.code())
}

/// A conversion from an integer register to floating point.
#[inline]
const fn int_to_fp(opcode: u32, fsz: FpSize, rd: VReg, sz: Size, rn: RegZr) -> u32 {
    fp_int(0, opcode, sz, fsz.ftype(), rd.code(), rn.code())
}

/// The half-precision form (`FEAT_FP16`, `ftype` `11`) of a word that a
/// constructor of this module built for single precision ([`FpSize::S`]):
/// the data-processing forms (`FMAX <Hd>, <Hn>, <Hm>`, ...), the compares
/// and the conversions to and from a general register (`FCVTAS <Wd|Xd>,
/// <Hn>`, ...). Not for the FMOV (general) forms, whose `ftype` follows
/// the general register.
#[inline]
pub const fn fp_half(single: u32) -> u32 {
    single | 0x00c0_0000
}

/// The `S` view goes with `W` and the `D` view with `X` in the FMOV
/// (general) forms.
#[inline]
const fn fmov_ftype(sz: Size) -> u32 {
    match sz {
        Size::W => FpSize::S.ftype(),
        Size::X => FpSize::D.ftype(),
    }
}

family! {
    (sz: FpSize, rd: VReg, rn: VReg) => fp_data1;

    /// `FMOV <Sd|Dd>, <Sn|Dn>`: FMOV (register). The rest of `Vd` is
    /// zeroed, as for every scalar write.
    pub fn fmov = (0b00_0000);
    /// `FABS <Sd|Dd>, <Sn|Dn>`: FABS (scalar).
    pub fn fabs = (0b00_0001);
    /// `FNEG <Sd|Dd>, <Sn|Dn>`: FNEG (scalar).
    pub fn fneg = (0b00_0010);
    /// `FSQRT <Sd|Dd>, <Sn|Dn>`: FSQRT (scalar).
    pub fn fsqrt = (0b00_0011);
    /// `FRINTN <Sd|Dd>, <Sn|Dn>`: FRINTN (scalar), to nearest with ties
    /// to even.
    pub fn frintn = (0b00_1000);
    /// `FRINTP <Sd|Dd>, <Sn|Dn>`: FRINTP (scalar), toward plus infinity.
    pub fn frintp = (0b00_1001);
    /// `FRINTM <Sd|Dd>, <Sn|Dn>`: FRINTM (scalar), toward minus infinity.
    pub fn frintm = (0b00_1010);
    /// `FRINTZ <Sd|Dd>, <Sn|Dn>`: FRINTZ (scalar), toward zero.
    pub fn frintz = (0b00_1011);
    /// `FRINTA <Sd|Dd>, <Sn|Dn>`: FRINTA (scalar), to nearest with ties
    /// away from zero.
    pub fn frinta = (0b00_1100);
    /// `FRINTX <Sd|Dd>, <Sn|Dn>`: FRINTX (scalar), in the current rounding
    /// mode, signalling inexact.
    pub fn frintx = (0b00_1110);
    /// `FRINTI <Sd|Dd>, <Sn|Dn>`: FRINTI (scalar), in the current rounding
    /// mode.
    pub fn frinti = (0b00_1111);
}

family! {
    (sz: FpSize, rd: VReg, rn: VReg, rm: VReg) => fp_data2;

    /// `FMUL <Sd|Dd>, <Sn|Dn>, <Sm|Dm>`: FMUL (scalar).
    pub fn fmul = (0b0000);
    /// `FDIV <Sd|Dd>, <Sn|Dn>, <Sm|Dm>`: FDIV (scalar), `rn / rm`.
    pub fn fdiv = (0b0001);
    /// `FADD <Sd|Dd>, <Sn|Dn>, <Sm|Dm>`: FADD (scalar).
    pub fn fadd = (0b0010);
    /// `FSUB <Sd|Dd>, <Sn|Dn>, <Sm|Dm>`: FSUB (scalar), `rn - rm`.
    pub fn fsub = (0b0011);
    /// `FMAX <Sd|Dd>, <Sn|Dn>, <Sm|Dm>`: FMAX (scalar); a NaN operand
    /// gives NaN.
    pub fn fmax = (0b0100);
    /// `FMIN <Sd|Dd>, <Sn|Dn>, <Sm|Dm>`: FMIN (scalar); a NaN operand
    /// gives NaN.
    pub fn fmin = (0b0101);
    /// `FMAXNM <Sd|Dd>, <Sn|Dn>, <Sm|Dm>`: FMAXNM (scalar); a quiet NaN
    /// operand gives the other operand.
    pub fn fmaxnm = (0b0110);
    /// `FMINNM <Sd|Dd>, <Sn|Dn>, <Sm|Dm>`: FMINNM (scalar); a quiet NaN
    /// operand gives the other operand.
    pub fn fminnm = (0b0111);
    /// `FNMUL <Sd|Dd>, <Sn|Dn>, <Sm|Dm>`: FNMUL (scalar), `-(rn * rm)`.
    pub fn fnmul = (0b1000);
}

family! {
    (sz: FpSize, rd: VReg, rn: VReg, rm: VReg, ra: VReg) => fp_data3;

    /// `FMADD <Sd|Dd>, <Sn|Dn>, <Sm|Dm>, <Sa|Da>`: FMADD,
    /// `ra + rn * rm` with one rounding.
    pub fn fmadd = (0, 0);
    /// `FMSUB <Sd|Dd>, <Sn|Dn>, <Sm|Dm>, <Sa|Da>`: FMSUB,
    /// `ra - rn * rm`.
    pub fn fmsub = (0, 1);
    /// `FNMADD <Sd|Dd>, <Sn|Dn>, <Sm|Dm>, <Sa|Da>`: FNMADD,
    /// `-ra - rn * rm`.
    pub fn fnmadd = (1, 0);
    /// `FNMSUB <Sd|Dd>, <Sn|Dn>, <Sm|Dm>, <Sa|Da>`: FNMSUB,
    /// `-ra + rn * rm`.
    pub fn fnmsub = (1, 1);
}

/// `FCMP <Sn|Dn>, <Sm|Dm>`: FCMP. Sets NZCV; an unordered result is
/// `0011`. Signals only for a signalling NaN.
#[inline]
pub const fn fcmp(sz: FpSize, rn: VReg, rm: VReg) -> u32 {
    fp_compare(0b0_0000, sz, rn, rm.code())
}

/// `FCMPE <Sn|Dn>, <Sm|Dm>`: FCMPE, which also signals for a quiet NaN.
#[inline]
pub const fn fcmpe(sz: FpSize, rn: VReg, rm: VReg) -> u32 {
    fp_compare(0b1_0000, sz, rn, rm.code())
}

/// `FCMP <Sn|Dn>, #0.0`: FCMP with zero.
#[inline]
pub const fn fcmp_zero(sz: FpSize, rn: VReg) -> u32 {
    fp_compare(0b0_1000, sz, rn, 0)
}

/// `FCMPE <Sn|Dn>, #0.0`: FCMPE with zero.
#[inline]
pub const fn fcmpe_zero(sz: FpSize, rn: VReg) -> u32 {
    fp_compare(0b1_1000, sz, rn, 0)
}

/// `FCSEL <Sd|Dd>, <Sn|Dn>, <Sm|Dm>, <cond>`: FCSEL
/// (`rd = cond ? rn : rm`).
#[inline]
pub const fn fcsel(sz: FpSize, rd: VReg, rn: VReg, rm: VReg, cond: Cond) -> u32 {
    0x1e20_0c00 | sz.ftype() | rm.code() << 16 | cond.code() << 12 | rn.code() << 5 | rd.code()
}

checked! {
    /// `FCCMP <Sn|Dn>, <Sm|Dm>, #nzcv, <cond>`: FCCMP. Flags are those of
    /// `FCMP rn, rm` if `cond` holds, else `nzcv`.
    ///
    /// Precondition: `nzcv < 16`.
    pub fn fccmp / try_fccmp(sz: FpSize, rn: VReg, rm: VReg, nzcv: u32, cond: Cond)
    requires nzcv < 16;
    { fp_cond_compare(0, sz, rn, rm, nzcv, cond) }

    /// `FCCMPE <Sn|Dn>, <Sm|Dm>, #nzcv, <cond>`: FCCMPE.
    ///
    /// Precondition: `nzcv < 16`.
    pub fn fccmpe / try_fccmpe(sz: FpSize, rn: VReg, rm: VReg, nzcv: u32, cond: Cond)
    requires nzcv < 16;
    { fp_cond_compare(1, sz, rn, rm, nzcv, cond) }

    /// `FCVT <Hd|Sd|Dd>, <Hn|Sn|Dn>`: FCVT, a conversion between two
    /// floating-point formats in the current rounding mode.
    ///
    /// Precondition: `to` and `from` differ.
    pub fn fcvt / try_fcvt(to: FpFormat, rd: VReg, from: FpFormat, rn: VReg)
    requires !matches!(
        (to, from),
        (FpFormat::H, FpFormat::H) | (FpFormat::S, FpFormat::S) | (FpFormat::D, FpFormat::D)
    );
    {
        let opcode = match to {
            FpFormat::S => 0b00_0100,
            FpFormat::D => 0b00_0101,
            FpFormat::H => 0b00_0111,
        };
        0x1e20_4000 | from.ftype() | opcode << 15 | rn.code() << 5 | rd.code()
    }

    /// `FMOV <Sd|Dd>, #imm`: FMOV (scalar, immediate). `bits` is the
    /// constant's bit pattern at precision `sz`.
    ///
    /// Precondition: [`encode_fp_imm8`]`(sz, bits).is_some()`.
    pub fn fmov_imm / try_fmov_imm(sz: FpSize, rd: VReg, bits: u64)
    requires encode_fp_imm8(sz, bits).is_some();
    {
        let imm8 = match encode_fp_imm8(sz, bits) {
            Some(imm8) => imm8 as u32,
            None => 0,
        };
        0x1e20_1000 | sz.ftype() | imm8 << 13 | rd.code()
    }
}

family! {
    (fsz: FpSize, rd: VReg, sz: Size, rn: RegZr) => int_to_fp;

    /// `SCVTF <Sd|Dd>, <Wn|Xn>`: SCVTF (scalar, integer), signed integer
    /// to floating point in the current rounding mode.
    pub fn scvtf = (0b010);
    /// `UCVTF <Sd|Dd>, <Wn|Xn>`: UCVTF (scalar, integer), unsigned integer
    /// to floating point in the current rounding mode.
    pub fn ucvtf = (0b011);
}

family! {
    (sz: Size, rd: RegZr, fsz: FpSize, rn: VReg) => fp_to_int;

    /// `FCVTNS <Wd|Xd>, <Sn|Dn>`: FCVTNS (scalar), to signed integer,
    /// nearest with ties to even. Like every conversion to integer here it
    /// saturates, and gives zero for a NaN.
    pub fn fcvtns = (0b00, 0b000);
    /// `FCVTNU <Wd|Xd>, <Sn|Dn>`: FCVTNU (scalar), to unsigned integer,
    /// nearest with ties to even.
    pub fn fcvtnu = (0b00, 0b001);
    /// `FCVTAS <Wd|Xd>, <Sn|Dn>`: FCVTAS (scalar), to signed integer,
    /// nearest with ties away from zero.
    pub fn fcvtas = (0b00, 0b100);
    /// `FCVTAU <Wd|Xd>, <Sn|Dn>`: FCVTAU (scalar), to unsigned integer,
    /// nearest with ties away from zero.
    pub fn fcvtau = (0b00, 0b101);
    /// `FCVTPS <Wd|Xd>, <Sn|Dn>`: FCVTPS (scalar), to signed integer,
    /// toward plus infinity.
    pub fn fcvtps = (0b01, 0b000);
    /// `FCVTPU <Wd|Xd>, <Sn|Dn>`: FCVTPU (scalar), to unsigned integer,
    /// toward plus infinity.
    pub fn fcvtpu = (0b01, 0b001);
    /// `FCVTMS <Wd|Xd>, <Sn|Dn>`: FCVTMS (scalar), to signed integer,
    /// toward minus infinity.
    pub fn fcvtms = (0b10, 0b000);
    /// `FCVTMU <Wd|Xd>, <Sn|Dn>`: FCVTMU (scalar), to unsigned integer,
    /// toward minus infinity.
    pub fn fcvtmu = (0b10, 0b001);
    /// `FCVTZS <Wd|Xd>, <Sn|Dn>`: FCVTZS (scalar, integer), to signed
    /// integer, toward zero.
    pub fn fcvtzs = (0b11, 0b000);
    /// `FCVTZU <Wd|Xd>, <Sn|Dn>`: FCVTZU (scalar, integer), to unsigned
    /// integer, toward zero.
    pub fn fcvtzu = (0b11, 0b001);
}

/// `FMOV <Wd>, <Sn>` / `FMOV <Xd>, <Dn>`: FMOV (general), the bits of the
/// `S` view to a `W` register or of the `D` view to an `X` register.
#[inline]
pub const fn fmov_fp_to_gpr(sz: Size, rd: RegZr, rn: VReg) -> u32 {
    fp_int(0b00, 0b110, sz, fmov_ftype(sz), rd.code(), rn.code())
}

/// `FMOV <Sd>, <Wn>` / `FMOV <Dd>, <Xn>`: FMOV (general), the bits of a
/// `W` register to the `S` view or of an `X` register to the `D` view. The
/// rest of `Vd` is zeroed.
#[inline]
pub const fn fmov_gpr_to_fp(sz: Size, rd: VReg, rn: RegZr) -> u32 {
    fp_int(0b00, 0b111, sz, fmov_ftype(sz), rd.code(), rn.code())
}

/// `FMOV <Xd>, <Vn>.D[1]`: FMOV (general), the upper 64 bits of `Vn` to
/// an `X` register.
#[inline]
pub const fn fmov_hi_to_gpr(rd: RegZr, rn: VReg) -> u32 {
    fp_int(0b01, 0b110, Size::X, 2 << 22, rd.code(), rn.code())
}

/// `FMOV <Vd>.D[1], <Xn>`: FMOV (general), an `X` register to the upper
/// 64 bits of `Vd`. The lower 64 bits are kept.
#[inline]
pub const fn fmov_gpr_to_hi(rd: VReg, rn: RegZr) -> u32 {
    fp_int(0b01, 0b111, Size::X, 2 << 22, rd.code(), rn.code())
}

/// The instructions that load one floating-point constant: one to five
/// words, produced by [`fmov_const`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FmovSeq {
    words: [u32; 5],
    len: u8,
}

impl FmovSeq {
    const fn new() -> Self {
        Self { words: [0; 5], len: 0 }
    }

    const fn push(mut self, word: u32) -> Self {
        self.words[self.len as usize] = word;
        self.len += 1;
        self
    }

    /// The instruction words, in execution order.
    #[inline]
    pub const fn as_slice(&self) -> &[u32] {
        self.words.split_at(self.len as usize).0
    }

    /// The number of instructions, `1..=5`.
    #[inline]
    #[allow(clippy::len_without_is_empty, reason = "a sequence is never empty")]
    pub const fn len(&self) -> usize {
        self.len as usize
    }
}

/// Materializes a floating-point constant: instructions that leave the bit
/// pattern `bits` in the `fmt` view of `rd` and zero the rest of the
/// register.
///
/// The constant is the low `fmt.bits()` bits of `bits`; the bits above are
/// ignored. The result is the first of these that applies:
///
/// 1. all zero: `MOVI <Dd>, #0`;
/// 2. an FMOV immediate ([`encode_fp_imm8`]; `S` and `D` only):
///    `FMOV <Sd|Dd>, #imm`;
/// 3. otherwise [`mov_imm`] of the pattern into `scratch` (at 64 bits for
///    `D`, at 32 for `S` and `H`), then `FMOV <Dd>, <Xscratch>` or
///    `FMOV <Sd>, <Wscratch>`.
///
/// `scratch` is written only in the third case.
pub const fn fmov_const(fmt: FpFormat, rd: VReg, scratch: Reg, bits: u64) -> FmovSeq {
    let (bits, fp, gpr) = match fmt {
        FpFormat::H => (bits & 0xffff, None, Size::W),
        FpFormat::S => (bits & 0xffff_ffff, Some(FpSize::S), Size::W),
        FpFormat::D => (bits, Some(FpSize::D), Size::X),
    };
    if bits == 0 {
        return FmovSeq::new().push(movi_mask(VecBits::V64, rd, 0));
    }
    if let Some(sz) = fp
        && encode_fp_imm8(sz, bits).is_some()
    {
        return FmovSeq::new().push(fmov_imm(sz, rd, bits));
    }
    let mov = mov_imm(gpr, scratch, bits);
    let words = mov.as_slice();
    let mut seq = FmovSeq::new();
    let mut i = 0;
    while i < words.len() {
        seq = seq.push(words[i]);
        i += 1;
    }
    seq.push(fmov_gpr_to_fp(gpr, rd, scratch.zr()))
}
