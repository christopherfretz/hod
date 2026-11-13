//! Packed SSE/SSE2: integer and floating-point arithmetic on 128-bit
//! vectors, compares, shifts, shuffles, unpacks, packs, conversions and
//! the mask extractions (`spec/mc-x64.md` "Packed SSE2").
//!
//! Every instruction is in the `0F` map. The integer forms take the
//! mandatory prefix `66`; the floating-point forms take none (single) or
//! `66` (double), selected by an [`FpSize`] operand as in `sse.rs`. A
//! 128-bit memory operand must be 16-byte aligned (none of these forms
//! accepts an unaligned one), except where a row says it reads 64 or 16
//! bits.

use crate::inst::{Inst, Rm, imm, modrm};
use crate::reg::{FpCmp, FpSize, Mem, Reg, Xmm};
use crate::sse::{op, packed, packed_rm, packed_rr, xm, xx};

/// `66 0F opcode /r` with two `xmm` operands.
#[inline]
const fn int_rr(opcode: u8, dst: Xmm, src: Xmm) -> Inst {
    xx(op(0x66, opcode), dst, src)
}

/// `66 0F opcode /r` with a 128-bit memory source.
#[inline]
const fn int_rm(opcode: u8, dst: Xmm, src: Mem) -> Inst {
    xm(op(0x66, opcode), dst, src)
}

family! {
    (dst: Xmm, src: Xmm) => int_rr;
    /// `PADDB xmm, xmm` (`66 0F FC /r`): bytewise wrapping add.
    pub fn paddb_rr = (0xfc);
    /// `PADDW xmm, xmm` (`66 0F FD /r`).
    pub fn paddw_rr = (0xfd);
    /// `PADDD xmm, xmm` (`66 0F FE /r`).
    pub fn paddd_rr = (0xfe);
    /// `PADDQ xmm, xmm` (`66 0F D4 /r`).
    pub fn paddq_rr = (0xd4);
    /// `PSUBB xmm, xmm` (`66 0F F8 /r`).
    pub fn psubb_rr = (0xf8);
    /// `PSUBW xmm, xmm` (`66 0F F9 /r`).
    pub fn psubw_rr = (0xf9);
    /// `PSUBD xmm, xmm` (`66 0F FA /r`).
    pub fn psubd_rr = (0xfa);
    /// `PSUBQ xmm, xmm` (`66 0F FB /r`).
    pub fn psubq_rr = (0xfb);
    /// `PADDSB xmm, xmm` (`66 0F EC /r`): signed saturating.
    pub fn paddsb_rr = (0xec);
    /// `PADDSW xmm, xmm` (`66 0F ED /r`): signed saturating.
    pub fn paddsw_rr = (0xed);
    /// `PADDUSB xmm, xmm` (`66 0F DC /r`): unsigned saturating.
    pub fn paddusb_rr = (0xdc);
    /// `PADDUSW xmm, xmm` (`66 0F DD /r`): unsigned saturating.
    pub fn paddusw_rr = (0xdd);
    /// `PSUBSB xmm, xmm` (`66 0F E8 /r`): signed saturating.
    pub fn psubsb_rr = (0xe8);
    /// `PSUBSW xmm, xmm` (`66 0F E9 /r`): signed saturating.
    pub fn psubsw_rr = (0xe9);
    /// `PSUBUSB xmm, xmm` (`66 0F D8 /r`): unsigned saturating.
    pub fn psubusb_rr = (0xd8);
    /// `PSUBUSW xmm, xmm` (`66 0F D9 /r`): unsigned saturating.
    pub fn psubusw_rr = (0xd9);
    /// `PMULLW xmm, xmm` (`66 0F D5 /r`): low 16 bits of each product.
    pub fn pmullw_rr = (0xd5);
    /// `PMULHW xmm, xmm` (`66 0F E5 /r`): high 16 bits, signed.
    pub fn pmulhw_rr = (0xe5);
    /// `PMULHUW xmm, xmm` (`66 0F E4 /r`): high 16 bits, unsigned.
    pub fn pmulhuw_rr = (0xe4);
    /// `PMULUDQ xmm, xmm` (`66 0F F4 /r`): the even unsigned 32-bit
    /// elements multiplied into two 64-bit products.
    pub fn pmuludq_rr = (0xf4);
    /// `PMADDWD xmm, xmm` (`66 0F F5 /r`): signed 16-bit products, adjacent
    /// pairs added into 32 bits.
    pub fn pmaddwd_rr = (0xf5);
    /// `PAND xmm, xmm` (`66 0F DB /r`).
    pub fn pand_rr = (0xdb);
    /// `PANDN xmm, xmm` (`66 0F DF /r`): `dst = !dst & src`.
    pub fn pandn_rr = (0xdf);
    /// `POR xmm, xmm` (`66 0F EB /r`).
    pub fn por_rr = (0xeb);
    /// `PXOR xmm, xmm` (`66 0F EF /r`): with `dst == src` the zero idiom.
    pub fn pxor_rr = (0xef);
    /// `PCMPEQB xmm, xmm` (`66 0F 74 /r`): all ones where equal.
    pub fn pcmpeqb_rr = (0x74);
    /// `PCMPEQW xmm, xmm` (`66 0F 75 /r`).
    pub fn pcmpeqw_rr = (0x75);
    /// `PCMPEQD xmm, xmm` (`66 0F 76 /r`).
    pub fn pcmpeqd_rr = (0x76);
    /// `PCMPGTB xmm, xmm` (`66 0F 64 /r`): all ones where `dst > src`,
    /// signed.
    pub fn pcmpgtb_rr = (0x64);
    /// `PCMPGTW xmm, xmm` (`66 0F 65 /r`).
    pub fn pcmpgtw_rr = (0x65);
    /// `PCMPGTD xmm, xmm` (`66 0F 66 /r`).
    pub fn pcmpgtd_rr = (0x66);
    /// `PMINSW xmm, xmm` (`66 0F EA /r`): signed 16-bit minimum.
    pub fn pminsw_rr = (0xea);
    /// `PMAXSW xmm, xmm` (`66 0F EE /r`): signed 16-bit maximum.
    pub fn pmaxsw_rr = (0xee);
    /// `PMINUB xmm, xmm` (`66 0F DA /r`): unsigned 8-bit minimum.
    pub fn pminub_rr = (0xda);
    /// `PMAXUB xmm, xmm` (`66 0F DE /r`): unsigned 8-bit maximum.
    pub fn pmaxub_rr = (0xde);
    /// `PAVGB xmm, xmm` (`66 0F E0 /r`): unsigned rounded average.
    pub fn pavgb_rr = (0xe0);
    /// `PAVGW xmm, xmm` (`66 0F E3 /r`).
    pub fn pavgw_rr = (0xe3);
    /// `PSADBW xmm, xmm` (`66 0F F6 /r`): sums of absolute byte
    /// differences, one per 64-bit half.
    pub fn psadbw_rr = (0xf6);
    /// `PSLLW xmm, xmm` (`66 0F F1 /r`): every element shifted left by the
    /// low 64 bits of `src` (zero when the count exceeds the width).
    pub fn psllw_rr = (0xf1);
    /// `PSLLD xmm, xmm` (`66 0F F2 /r`).
    pub fn pslld_rr = (0xf2);
    /// `PSLLQ xmm, xmm` (`66 0F F3 /r`).
    pub fn psllq_rr = (0xf3);
    /// `PSRLW xmm, xmm` (`66 0F D1 /r`): logical right.
    pub fn psrlw_rr = (0xd1);
    /// `PSRLD xmm, xmm` (`66 0F D2 /r`).
    pub fn psrld_rr = (0xd2);
    /// `PSRLQ xmm, xmm` (`66 0F D3 /r`).
    pub fn psrlq_rr = (0xd3);
    /// `PSRAW xmm, xmm` (`66 0F E1 /r`): arithmetic right (sign fill when
    /// the count exceeds the width).
    pub fn psraw_rr = (0xe1);
    /// `PSRAD xmm, xmm` (`66 0F E2 /r`).
    pub fn psrad_rr = (0xe2);
    /// `PUNPCKLBW xmm, xmm` (`66 0F 60 /r`): interleaves the low halves.
    pub fn punpcklbw_rr = (0x60);
    /// `PUNPCKLWD xmm, xmm` (`66 0F 61 /r`).
    pub fn punpcklwd_rr = (0x61);
    /// `PUNPCKLDQ xmm, xmm` (`66 0F 62 /r`).
    pub fn punpckldq_rr = (0x62);
    /// `PUNPCKLQDQ xmm, xmm` (`66 0F 6C /r`).
    pub fn punpcklqdq_rr = (0x6c);
    /// `PUNPCKHBW xmm, xmm` (`66 0F 68 /r`): interleaves the high halves.
    pub fn punpckhbw_rr = (0x68);
    /// `PUNPCKHWD xmm, xmm` (`66 0F 69 /r`).
    pub fn punpckhwd_rr = (0x69);
    /// `PUNPCKHDQ xmm, xmm` (`66 0F 6A /r`).
    pub fn punpckhdq_rr = (0x6a);
    /// `PUNPCKHQDQ xmm, xmm` (`66 0F 6D /r`).
    pub fn punpckhqdq_rr = (0x6d);
    /// `PACKSSWB xmm, xmm` (`66 0F 63 /r`): 16 to 8 bits, signed
    /// saturation; `dst`'s elements in the low half.
    pub fn packsswb_rr = (0x63);
    /// `PACKSSDW xmm, xmm` (`66 0F 6B /r`): 32 to 16 bits, signed
    /// saturation.
    pub fn packssdw_rr = (0x6b);
    /// `PACKUSWB xmm, xmm` (`66 0F 67 /r`): signed 16 to unsigned 8 bits,
    /// saturating.
    pub fn packuswb_rr = (0x67);
}

family! {
    (dst: Xmm, src: Mem) => int_rm;
    /// `PADDB xmm, m128`.
    pub fn paddb_rm = (0xfc);
    /// `PADDW xmm, m128`.
    pub fn paddw_rm = (0xfd);
    /// `PADDD xmm, m128`.
    pub fn paddd_rm = (0xfe);
    /// `PADDQ xmm, m128`.
    pub fn paddq_rm = (0xd4);
    /// `PSUBB xmm, m128`.
    pub fn psubb_rm = (0xf8);
    /// `PSUBW xmm, m128`.
    pub fn psubw_rm = (0xf9);
    /// `PSUBD xmm, m128`.
    pub fn psubd_rm = (0xfa);
    /// `PSUBQ xmm, m128`.
    pub fn psubq_rm = (0xfb);
    /// `PADDSB xmm, m128`.
    pub fn paddsb_rm = (0xec);
    /// `PADDSW xmm, m128`.
    pub fn paddsw_rm = (0xed);
    /// `PADDUSB xmm, m128`.
    pub fn paddusb_rm = (0xdc);
    /// `PADDUSW xmm, m128`.
    pub fn paddusw_rm = (0xdd);
    /// `PSUBSB xmm, m128`.
    pub fn psubsb_rm = (0xe8);
    /// `PSUBSW xmm, m128`.
    pub fn psubsw_rm = (0xe9);
    /// `PSUBUSB xmm, m128`.
    pub fn psubusb_rm = (0xd8);
    /// `PSUBUSW xmm, m128`.
    pub fn psubusw_rm = (0xd9);
    /// `PMULLW xmm, m128`.
    pub fn pmullw_rm = (0xd5);
    /// `PMULHW xmm, m128`.
    pub fn pmulhw_rm = (0xe5);
    /// `PMULHUW xmm, m128`.
    pub fn pmulhuw_rm = (0xe4);
    /// `PMULUDQ xmm, m128`.
    pub fn pmuludq_rm = (0xf4);
    /// `PMADDWD xmm, m128`.
    pub fn pmaddwd_rm = (0xf5);
    /// `PAND xmm, m128`.
    pub fn pand_rm = (0xdb);
    /// `PANDN xmm, m128`.
    pub fn pandn_rm = (0xdf);
    /// `POR xmm, m128`.
    pub fn por_rm = (0xeb);
    /// `PXOR xmm, m128`.
    pub fn pxor_rm = (0xef);
    /// `PCMPEQB xmm, m128`.
    pub fn pcmpeqb_rm = (0x74);
    /// `PCMPEQW xmm, m128`.
    pub fn pcmpeqw_rm = (0x75);
    /// `PCMPEQD xmm, m128`.
    pub fn pcmpeqd_rm = (0x76);
    /// `PCMPGTB xmm, m128`.
    pub fn pcmpgtb_rm = (0x64);
    /// `PCMPGTW xmm, m128`.
    pub fn pcmpgtw_rm = (0x65);
    /// `PCMPGTD xmm, m128`.
    pub fn pcmpgtd_rm = (0x66);
    /// `PMINSW xmm, m128`.
    pub fn pminsw_rm = (0xea);
    /// `PMAXSW xmm, m128`.
    pub fn pmaxsw_rm = (0xee);
    /// `PMINUB xmm, m128`.
    pub fn pminub_rm = (0xda);
    /// `PMAXUB xmm, m128`.
    pub fn pmaxub_rm = (0xde);
    /// `PAVGB xmm, m128`.
    pub fn pavgb_rm = (0xe0);
    /// `PAVGW xmm, m128`.
    pub fn pavgw_rm = (0xe3);
    /// `PSADBW xmm, m128`.
    pub fn psadbw_rm = (0xf6);
    /// `PSLLW xmm, m128`: the count is the low 64 bits of the operand.
    pub fn psllw_rm = (0xf1);
    /// `PSLLD xmm, m128`.
    pub fn pslld_rm = (0xf2);
    /// `PSLLQ xmm, m128`.
    pub fn psllq_rm = (0xf3);
    /// `PSRLW xmm, m128`.
    pub fn psrlw_rm = (0xd1);
    /// `PSRLD xmm, m128`.
    pub fn psrld_rm = (0xd2);
    /// `PSRLQ xmm, m128`.
    pub fn psrlq_rm = (0xd3);
    /// `PSRAW xmm, m128`.
    pub fn psraw_rm = (0xe1);
    /// `PSRAD xmm, m128`.
    pub fn psrad_rm = (0xe2);
    /// `PUNPCKLBW xmm, m128`.
    pub fn punpcklbw_rm = (0x60);
    /// `PUNPCKLWD xmm, m128`.
    pub fn punpcklwd_rm = (0x61);
    /// `PUNPCKLDQ xmm, m128`.
    pub fn punpckldq_rm = (0x62);
    /// `PUNPCKLQDQ xmm, m128`.
    pub fn punpcklqdq_rm = (0x6c);
    /// `PUNPCKHBW xmm, m128`.
    pub fn punpckhbw_rm = (0x68);
    /// `PUNPCKHWD xmm, m128`.
    pub fn punpckhwd_rm = (0x69);
    /// `PUNPCKHDQ xmm, m128`.
    pub fn punpckhdq_rm = (0x6a);
    /// `PUNPCKHQDQ xmm, m128`.
    pub fn punpckhqdq_rm = (0x6d);
    /// `PACKSSWB xmm, m128`.
    pub fn packsswb_rm = (0x63);
    /// `PACKSSDW xmm, m128`.
    pub fn packssdw_rm = (0x6b);
    /// `PACKUSWB xmm, m128`.
    pub fn packuswb_rm = (0x67);
}

/// A shift by an immediate: `66 0F opcode /digit ib` on `dst`.
#[inline]
const fn shift_ri(opcode: u8, digit: u8, dst: Xmm, n: u8) -> Inst {
    imm(modrm(op(0x66, opcode), digit, false, Rm::Reg(dst.as_gpr()), false), n as i64, 1)
}

family! {
    (dst: Xmm, n: u8) => shift_ri;
    /// `PSRLW xmm, imm8` (`66 0F 71 /2 ib`).
    pub fn psrlw_ri = (0x71, 2);
    /// `PSRAW xmm, imm8` (`66 0F 71 /4 ib`).
    pub fn psraw_ri = (0x71, 4);
    /// `PSLLW xmm, imm8` (`66 0F 71 /6 ib`).
    pub fn psllw_ri = (0x71, 6);
    /// `PSRLD xmm, imm8` (`66 0F 72 /2 ib`).
    pub fn psrld_ri = (0x72, 2);
    /// `PSRAD xmm, imm8` (`66 0F 72 /4 ib`).
    pub fn psrad_ri = (0x72, 4);
    /// `PSLLD xmm, imm8` (`66 0F 72 /6 ib`).
    pub fn pslld_ri = (0x72, 6);
    /// `PSRLQ xmm, imm8` (`66 0F 73 /2 ib`).
    pub fn psrlq_ri = (0x73, 2);
    /// `PSRLDQ xmm, imm8` (`66 0F 73 /3 ib`): the whole register right by
    /// `n` bytes (zero when `n > 15`).
    pub fn psrldq_ri = (0x73, 3);
    /// `PSLLQ xmm, imm8` (`66 0F 73 /6 ib`).
    pub fn psllq_ri = (0x73, 6);
    /// `PSLLDQ xmm, imm8` (`66 0F 73 /7 ib`): the whole register left by
    /// `n` bytes.
    pub fn pslldq_ri = (0x73, 7);
}

/// `prefix 0F 70 /r ib` with two `xmm` operands.
#[inline]
const fn pshuf_rri(prefix: u8, dst: Xmm, src: Xmm, order: u8) -> Inst {
    imm(xx(op(prefix, 0x70), dst, src), order as i64, 1)
}

/// `prefix 0F 70 /r ib` with a 128-bit memory source.
#[inline]
const fn pshuf_rmi(prefix: u8, dst: Xmm, src: Mem, order: u8) -> Inst {
    imm(xm(op(prefix, 0x70), dst, src), order as i64, 1)
}

family! {
    (dst: Xmm, src: Xmm, order: u8) => pshuf_rri;
    /// `PSHUFD xmm, xmm, imm8` (`66 0F 70 /r ib`): element `i` of `dst`
    /// is the 32-bit element `(order >> 2i) & 3` of `src`.
    pub fn pshufd_rri = (0x66);
    /// `PSHUFLW xmm, xmm, imm8` (`F2 0F 70 /r ib`): the same on the four
    /// low 16-bit elements; the high 64 bits copied.
    pub fn pshuflw_rri = (0xf2);
    /// `PSHUFHW xmm, xmm, imm8` (`F3 0F 70 /r ib`): the same on the four
    /// high 16-bit elements; the low 64 bits copied.
    pub fn pshufhw_rri = (0xf3);
}

family! {
    (dst: Xmm, src: Mem, order: u8) => pshuf_rmi;
    /// `PSHUFD xmm, m128, imm8`.
    pub fn pshufd_rmi = (0x66);
    /// `PSHUFLW xmm, m128, imm8`.
    pub fn pshuflw_rmi = (0xf2);
    /// `PSHUFHW xmm, m128, imm8`.
    pub fn pshufhw_rmi = (0xf3);
}

family! {
    (fp: FpSize, dst: Xmm, src: Xmm) => packed_rr;
    /// `ADDPS`/`ADDPD xmm, xmm` (`[66] 0F 58 /r`).
    pub fn addp_rr = (0x58);
    /// `SUBPS`/`SUBPD xmm, xmm` (`[66] 0F 5C /r`).
    pub fn subp_rr = (0x5c);
    /// `MULPS`/`MULPD xmm, xmm` (`[66] 0F 59 /r`).
    pub fn mulp_rr = (0x59);
    /// `DIVPS`/`DIVPD xmm, xmm` (`[66] 0F 5E /r`).
    pub fn divp_rr = (0x5e);
    /// `SQRTPS`/`SQRTPD xmm, xmm` (`[66] 0F 51 /r`).
    pub fn sqrtp_rr = (0x51);
    /// `MINPS`/`MINPD xmm, xmm` (`[66] 0F 5D /r`): per element as `mins`.
    pub fn minp_rr = (0x5d);
    /// `MAXPS`/`MAXPD xmm, xmm` (`[66] 0F 5F /r`): per element as `maxs`.
    pub fn maxp_rr = (0x5f);
    /// `UNPCKLPS`/`UNPCKLPD xmm, xmm` (`[66] 0F 14 /r`): interleaves the
    /// low halves.
    pub fn unpcklp_rr = (0x14);
    /// `UNPCKHPS`/`UNPCKHPD xmm, xmm` (`[66] 0F 15 /r`): interleaves the
    /// high halves.
    pub fn unpckhp_rr = (0x15);
}

family! {
    (fp: FpSize, dst: Xmm, src: Mem) => packed_rm;
    /// `ADDPS`/`ADDPD xmm, m128`.
    pub fn addp_rm = (0x58);
    /// `SUBPS`/`SUBPD xmm, m128`.
    pub fn subp_rm = (0x5c);
    /// `MULPS`/`MULPD xmm, m128`.
    pub fn mulp_rm = (0x59);
    /// `DIVPS`/`DIVPD xmm, m128`.
    pub fn divp_rm = (0x5e);
    /// `SQRTPS`/`SQRTPD xmm, m128`.
    pub fn sqrtp_rm = (0x51);
    /// `MINPS`/`MINPD xmm, m128`.
    pub fn minp_rm = (0x5d);
    /// `MAXPS`/`MAXPD xmm, m128`.
    pub fn maxp_rm = (0x5f);
    /// `UNPCKLPS`/`UNPCKLPD xmm, m128`.
    pub fn unpcklp_rm = (0x14);
    /// `UNPCKHPS`/`UNPCKHPD xmm, m128`.
    pub fn unpckhp_rm = (0x15);
}

/// `CMPPS`/`CMPPD xmm, xmm, imm8` (`[66] 0F C2 /r ib`): every element of
/// `dst` all ones where `pred` holds, else zero.
#[inline]
pub const fn cmpp_rr(pred: FpCmp, fp: FpSize, dst: Xmm, src: Xmm) -> Inst {
    imm(packed_rr(0xc2, fp, dst, src), pred as i64, 1)
}

/// `CMPPS`/`CMPPD xmm, m128, imm8`.
#[inline]
pub const fn cmpp_rm(pred: FpCmp, fp: FpSize, dst: Xmm, src: Mem) -> Inst {
    imm(packed_rm(0xc2, fp, dst, src), pred as i64, 1)
}

/// `SHUFPS`/`SHUFPD xmm, xmm, imm8` (`[66] 0F C6 /r ib`): the low half
/// of the result from `dst`, the high half from `src`, elements picked by
/// `order` (2 bits per element for `S`, 1 for `D`).
#[inline]
pub const fn shufp_rri(fp: FpSize, dst: Xmm, src: Xmm, order: u8) -> Inst {
    imm(packed_rr(0xc6, fp, dst, src), order as i64, 1)
}

/// `SHUFPS`/`SHUFPD xmm, m128, imm8`.
#[inline]
pub const fn shufp_rmi(fp: FpSize, dst: Xmm, src: Mem, order: u8) -> Inst {
    imm(packed_rm(0xc6, fp, dst, src), order as i64, 1)
}

/// `MOVLHPS xmm, xmm` (`0F 16 /r`): the low 64 bits of `src` into the
/// high 64 bits of `dst`.
#[inline]
pub const fn movlhps_rr(dst: Xmm, src: Xmm) -> Inst {
    xx(op(0, 0x16), dst, src)
}

/// `MOVHLPS xmm, xmm` (`0F 12 /r`): the high 64 bits of `src` into the
/// low 64 bits of `dst`.
#[inline]
pub const fn movhlps_rr(dst: Xmm, src: Xmm) -> Inst {
    xx(op(0, 0x12), dst, src)
}

family! {
    (fp: FpSize, dst: Xmm, src: Mem) => packed_rm;
    /// `MOVLPS`/`MOVLPD xmm, m64` (`[66] 0F 12 /r`): a load into the low
    /// 64 bits; the high 64 kept.
    pub fn movlp_rm = (0x12);
    /// `MOVHPS`/`MOVHPD xmm, m64` (`[66] 0F 16 /r`): a load into the high
    /// 64 bits; the low 64 kept.
    pub fn movhp_rm = (0x16);
}

/// `MOVLPS`/`MOVLPD m64, xmm` (`[66] 0F 13 /r`): the low 64 bits stored.
#[inline]
pub const fn movlp_mr(fp: FpSize, dst: Mem, src: Xmm) -> Inst {
    xm(op(packed(fp), 0x13), src, dst)
}

/// `MOVHPS`/`MOVHPD m64, xmm` (`[66] 0F 17 /r`): the high 64 bits stored.
#[inline]
pub const fn movhp_mr(fp: FpSize, dst: Mem, src: Xmm) -> Inst {
    xm(op(packed(fp), 0x17), src, dst)
}

/// A conversion `prefix 0F opcode /r` with two `xmm` operands.
#[inline]
const fn cvt_rr(prefix: u8, opcode: u8, dst: Xmm, src: Xmm) -> Inst {
    xx(op(prefix, opcode), dst, src)
}

/// The same with a memory source.
#[inline]
const fn cvt_rm(prefix: u8, opcode: u8, dst: Xmm, src: Mem) -> Inst {
    xm(op(prefix, opcode), dst, src)
}

family! {
    (dst: Xmm, src: Xmm) => cvt_rr;
    /// `CVTDQ2PS xmm, xmm` (`0F 5B /r`): four `i32` to `f32`, rounded as
    /// MXCSR says.
    pub fn cvtdq2ps_rr = (0, 0x5b);
    /// `CVTPS2DQ xmm, xmm` (`66 0F 5B /r`): four `f32` to `i32`, rounded
    /// as MXCSR says.
    pub fn cvtps2dq_rr = (0x66, 0x5b);
    /// `CVTTPS2DQ xmm, xmm` (`F3 0F 5B /r`): four `f32` to `i32`, toward
    /// zero.
    pub fn cvttps2dq_rr = (0xf3, 0x5b);
    /// `CVTDQ2PD xmm, xmm` (`F3 0F E6 /r`): the two low `i32` to `f64`.
    pub fn cvtdq2pd_rr = (0xf3, 0xe6);
    /// `CVTPD2DQ xmm, xmm` (`F2 0F E6 /r`): two `f64` to `i32` in the low
    /// half, rounded as MXCSR says; the high half zeroed.
    pub fn cvtpd2dq_rr = (0xf2, 0xe6);
    /// `CVTTPD2DQ xmm, xmm` (`66 0F E6 /r`): the same toward zero.
    pub fn cvttpd2dq_rr = (0x66, 0xe6);
    /// `CVTPS2PD xmm, xmm` (`0F 5A /r`): the two low `f32` to `f64`.
    pub fn cvtps2pd_rr = (0, 0x5a);
    /// `CVTPD2PS xmm, xmm` (`66 0F 5A /r`): two `f64` to `f32` in the low
    /// half; the high half zeroed.
    pub fn cvtpd2ps_rr = (0x66, 0x5a);
}

family! {
    (dst: Xmm, src: Mem) => cvt_rm;
    /// `CVTDQ2PS xmm, m128`.
    pub fn cvtdq2ps_rm = (0, 0x5b);
    /// `CVTPS2DQ xmm, m128`.
    pub fn cvtps2dq_rm = (0x66, 0x5b);
    /// `CVTTPS2DQ xmm, m128`.
    pub fn cvttps2dq_rm = (0xf3, 0x5b);
    /// `CVTDQ2PD xmm, m64`: reads 64 bits.
    pub fn cvtdq2pd_rm = (0xf3, 0xe6);
    /// `CVTPD2DQ xmm, m128`.
    pub fn cvtpd2dq_rm = (0xf2, 0xe6);
    /// `CVTTPD2DQ xmm, m128`.
    pub fn cvttpd2dq_rm = (0x66, 0xe6);
    /// `CVTPS2PD xmm, m64`: reads 64 bits.
    pub fn cvtps2pd_rm = (0, 0x5a);
    /// `CVTPD2PS xmm, m128`.
    pub fn cvtpd2ps_rm = (0x66, 0x5a);
}

/// `PMOVMSKB r32, xmm` (`66 0F D7 /r`, the general register in reg): the
/// sign bit of each byte, zero-extended into `dst`.
#[inline]
pub const fn pmovmskb_rr(dst: Reg, src: Xmm) -> Inst {
    modrm(op(0x66, 0xd7), dst.code(), false, Rm::Reg(src.as_gpr()), false)
}

/// `MOVMSKPS`/`MOVMSKPD r32, xmm` (`[66] 0F 50 /r`): the sign bit of each
/// element, zero-extended into `dst`.
#[inline]
pub const fn movmskp_rr(fp: FpSize, dst: Reg, src: Xmm) -> Inst {
    modrm(op(packed(fp), 0x50), dst.code(), false, Rm::Reg(src.as_gpr()), false)
}

/// `PEXTRW r32, xmm, imm8` (`66 0F C5 /r ib`): the 16-bit element
/// `lane & 7`, zero-extended into `dst`.
#[inline]
pub const fn pextrw_rri(dst: Reg, src: Xmm, lane: u8) -> Inst {
    let inst = modrm(op(0x66, 0xc5), dst.code(), false, Rm::Reg(src.as_gpr()), false);
    imm(inst, lane as i64, 1)
}

/// `PINSRW xmm, r32, imm8` (`66 0F C4 /r ib`): the low 16 bits of `src`
/// into element `lane & 7` of `dst`.
#[inline]
pub const fn pinsrw_rri(dst: Xmm, src: Reg, lane: u8) -> Inst {
    let inst = modrm(op(0x66, 0xc4), dst.code(), false, Rm::Reg(src), false);
    imm(inst, lane as i64, 1)
}

/// `PINSRW xmm, m16, imm8` (`66 0F C4 /r ib`): reads 16 bits.
#[inline]
pub const fn pinsrw_rmi(dst: Xmm, src: Mem, lane: u8) -> Inst {
    imm(xm(op(0x66, 0xc4), dst, src), lane as i64, 1)
}
