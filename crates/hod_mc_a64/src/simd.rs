//! The Advanced SIMD subset a code generator needs for vectors as values:
//! moves, element access, immediates, elementwise integer and
//! floating-point operations, compares and selects, shifts, permutes,
//! widening and narrowing, and reductions (`spec/mc-a64.md` "SIMD").
//!
//! A constructor is named by its mnemonic. Where the crate already has a
//! scalar instruction of that name (`add`, `fadd`, `neg`, `rev16`, ...) the
//! vector one ends in `_vec`.

use crate::imm::encode_fp_imm8;
use crate::reg::{Arr, Elem, FpArr, FpSize, RegZr, Size, VReg, VecBits};

/// Advanced SIMD three same. `q_size` holds `Q` and bits 23:22 in place.
#[inline]
const fn three_same(u: u32, opcode: u32, q_size: u32, rd: VReg, rn: VReg, rm: VReg) -> u32 {
    0x0e20_0400 | u << 29 | q_size | rm.code() << 16 | opcode << 11 | rn.code() << 5 | rd.code()
}

/// Three same, integer elements.
#[inline]
const fn int3(u: u32, opcode: u32, arr: Arr, rd: VReg, rn: VReg, rm: VReg) -> u32 {
    three_same(u, opcode, arr.q_size(), rd, rn, rm)
}

/// Three same, bitwise: the `size` field selects the operation.
#[inline]
const fn bits3(u: u32, size: u32, bits: VecBits, rd: VReg, rn: VReg, rm: VReg) -> u32 {
    three_same(u, 0b0_0011, bits.q() | size << 22, rd, rn, rm)
}

/// Three same, floating-point elements: bit 23 (`a`) is part of the opcode.
#[inline]
const fn fp3(u: u32, a: u32, opcode: u32, arr: FpArr, rd: VReg, rn: VReg, rm: VReg) -> u32 {
    three_same(u, opcode, arr.q_sz() | a << 23, rd, rn, rm)
}

/// Advanced SIMD two-register miscellaneous.
#[inline]
const fn two_misc(u: u32, opcode: u32, q_size: u32, rd: VReg, rn: VReg) -> u32 {
    0x0e20_0800 | u << 29 | q_size | opcode << 12 | rn.code() << 5 | rd.code()
}

/// Two-register miscellaneous, integer elements.
#[inline]
const fn int2(u: u32, opcode: u32, arr: Arr, rd: VReg, rn: VReg) -> u32 {
    two_misc(u, opcode, arr.q_size(), rd, rn)
}

/// Two-register miscellaneous on bytes: `size` is part of the opcode.
#[inline]
const fn bytes2(u: u32, size: u32, opcode: u32, bits: VecBits, rd: VReg, rn: VReg) -> u32 {
    two_misc(u, opcode, bits.q() | size << 22, rd, rn)
}

/// Two-register miscellaneous, floating-point elements.
#[inline]
const fn fp2(u: u32, a: u32, opcode: u32, arr: FpArr, rd: VReg, rn: VReg) -> u32 {
    two_misc(u, opcode, arr.q_sz() | a << 23, rd, rn)
}

/// Advanced SIMD across lanes.
#[inline]
const fn across(u: u32, opcode: u32, arr: Arr, rd: VReg, rn: VReg) -> u32 {
    0x0e30_0800 | u << 29 | arr.q_size() | opcode << 12 | rn.code() << 5 | rd.code()
}

/// Advanced SIMD permute.
#[inline]
const fn permute(opcode: u32, arr: Arr, rd: VReg, rn: VReg, rm: VReg) -> u32 {
    0x0e00_0800 | arr.q_size() | rm.code() << 16 | opcode << 12 | rn.code() << 5 | rd.code()
}

/// Advanced SIMD copy. `q` and `op` are bits 30 and 29 as 0 or 1.
#[inline]
const fn copy(q: u32, op: u32, imm4: u32, imm5: u32, rd: u32, rn: u32) -> u32 {
    0x0e00_0400 | q << 30 | op << 29 | imm5 << 16 | imm4 << 11 | rn << 5 | rd
}

/// The `imm5` field naming element `index` of width `elem`: the index
/// above a one in the bit position of the width.
#[inline]
const fn lane(elem: Elem, index: u32) -> u32 {
    ((index << 1 | 1) << elem.log2()) & 0x1f
}

/// Advanced SIMD modified immediate. `q` is in place; `op` is bit 29.
#[inline]
const fn modified_imm(q: u32, op: u32, cmode: u32, imm8: u32, rd: VReg) -> u32 {
    0x0f00_0400 | q | op << 29 | (imm8 >> 5) << 16 | cmode << 12 | (imm8 & 0x1f) << 5 | rd.code()
}

/// Advanced SIMD shift by immediate. `immhb` is `immh:immb`, bits 22:16.
#[inline]
const fn shift_imm(u: u32, opcode: u32, q: u32, immhb: u32, rd: VReg, rn: VReg) -> u32 {
    0x0f00_0400 | q | u << 29 | (immhb & 0x7f) << 16 | opcode << 11 | rn.code() << 5 | rd.code()
}

/// TBL: `len` is the number of table registers less one.
#[inline]
const fn table(len: u32, bits: VecBits, rd: VReg, rn: VReg, rm: VReg) -> u32 {
    0x0e00_0000 | bits.q() | rm.code() << 16 | len << 13 | rn.code() << 5 | rd.code()
}

#[inline]
const fn shl_fields(arr: Arr, rd: VReg, rn: VReg, amount: u32) -> u32 {
    shift_imm(0, 0b0_1010, arr.vec_bits().q(), arr.elem().bits() + amount, rd, rn)
}

#[inline]
const fn shr_fields(u: u32, arr: Arr, rd: VReg, rn: VReg, amount: u32) -> u32 {
    shift_imm(u, 0b0_0000, arr.vec_bits().q(), (2 * arr.elem().bits()).wrapping_sub(amount), rd, rn)
}

#[inline]
const fn shll_fields(u: u32, from: Elem, rd: VReg, rn: VReg, amount: u32) -> u32 {
    shift_imm(u, 0b1_0100, 0, from.bits() + amount, rd, rn)
}

#[inline]
const fn xtl_fields(u: u32, from: Elem, rd: VReg, rn: VReg) -> u32 {
    shll_fields(u, from, rd, rn, 0)
}

#[inline]
const fn has_elem_d(arr: Arr) -> bool {
    matches!(arr, Arr::D2)
}

#[inline]
const fn is_elem_d(elem: Elem) -> bool {
    matches!(elem, Elem::D)
}

/// The arrangements the across-lanes reductions take: at least four
/// elements.
#[inline]
const fn reducible(arr: Arr) -> bool {
    matches!(arr, Arr::B8 | Arr::B16 | Arr::H4 | Arr::H8 | Arr::S4)
}

/// The `cmode` field of a shifted MOVI or MVNI for the arrangement, or
/// `None` when the element width does not have that shift.
#[inline]
const fn shifted_cmode(arr: Arr, shift: u32) -> Option<u32> {
    if !shift.is_multiple_of(8) {
        return None;
    }
    match arr.elem() {
        Elem::B if shift == 0 => Some(0b1110),
        Elem::H if shift < 16 => Some(0b1000 | (shift / 8) << 1),
        Elem::S if shift < 32 => Some((shift / 8) << 1),
        _ => None,
    }
}

/// The `imm8` field of a 64-bit MOVI: one bit per byte of `mask`, each of
/// which must be `0x00` or `0xff`.
#[inline]
const fn byte_mask_imm8(mask: u64) -> Option<u32> {
    let mut imm8 = 0;
    let mut i = 0;
    while i < 8 {
        match (mask >> (i * 8)) & 0xff {
            0 => {}
            0xff => imm8 |= 1 << i,
            _ => return None,
        }
        i += 1;
    }
    Some(imm8)
}

#[inline]
const fn or_zero(field: Option<u32>) -> u32 {
    match field {
        Some(field) => field,
        None => 0,
    }
}

// Moves and element access.

/// `MOV <Vd>.<8B|16B>, <Vn>.<8B|16B>`: MOV (vector), that is ORR (vector)
/// with both sources `rn`. The 64-bit form zeroes the upper half of `Vd`.
#[inline]
pub const fn mov_vec(bits: VecBits, rd: VReg, rn: VReg) -> u32 {
    orr_vec(bits, rd, rn, rn)
}

/// `DUP <Vd>.<T>, <Wn|Xn>`: DUP (general), a general-purpose register to
/// every element. The register is `Xn` for [`Arr::D2`] and `Wn` otherwise.
#[inline]
pub const fn dup_gpr(arr: Arr, rd: VReg, rn: RegZr) -> u32 {
    copy(arr.vec_bits() as u32, 0, 0b0001, lane(arr.elem(), 0), rd.code(), rn.code())
}

checked! {
    /// `DUP <Vd>.<T>, <Vn>.<Ts>[index]`: DUP (element), vector form: one
    /// element of `rn` to every element of `rd`.
    ///
    /// Precondition: `index < arr.elem().lanes()`.
    pub fn dup_elem / try_dup_elem(arr: Arr, rd: VReg, rn: VReg, index: u32)
    requires index < arr.elem().lanes();
    {
        copy(arr.vec_bits() as u32, 0, 0b0000, lane(arr.elem(), index), rd.code(), rn.code())
    }

    /// `MOV <Bd|Hd|Sd|Dd>, <Vn>.<Ts>[index]`: DUP (element), scalar form:
    /// one element of `rn` to the scalar view of `rd`, zeroing the rest.
    ///
    /// Precondition: `index < elem.lanes()`.
    pub fn dup_scalar / try_dup_scalar(elem: Elem, rd: VReg, rn: VReg, index: u32)
    requires index < elem.lanes();
    { 0x5e00_0400 | lane(elem, index) << 16 | rn.code() << 5 | rd.code() }

    /// `MOV <Vd>.<Ts>[rd_index], <Vn>.<Ts>[rn_index]`: INS (element). The
    /// other elements of `rd` are kept.
    ///
    /// Precondition: both indices are below `elem.lanes()`.
    pub fn ins_elem / try_ins_elem(elem: Elem, rd: VReg, rd_index: u32, rn: VReg, rn_index: u32)
    requires rd_index < elem.lanes() && rn_index < elem.lanes();
    { copy(1, 1, rn_index << elem.log2(), lane(elem, rd_index), rd.code(), rn.code()) }

    /// `MOV <Vd>.<Ts>[index], <Wn|Xn>`: INS (general). The register is
    /// `Xn` for [`Elem::D`] and `Wn` otherwise; the other elements of `rd`
    /// are kept.
    ///
    /// Precondition: `index < elem.lanes()`.
    pub fn ins_gpr / try_ins_gpr(elem: Elem, rd: VReg, index: u32, rn: RegZr)
    requires index < elem.lanes();
    { copy(1, 0, 0b0011, lane(elem, index), rd.code(), rn.code()) }

    /// `UMOV <Wd>, <Vn>.<B|H|S>[index]` / `UMOV <Xd>, <Vn>.D[index]`:
    /// UMOV, an element zero-extended to a general-purpose register (`MOV`
    /// in assembly for `S` and `D`).
    ///
    /// Precondition: `index < elem.lanes()`.
    pub fn umov / try_umov(elem: Elem, rd: RegZr, rn: VReg, index: u32)
    requires index < elem.lanes();
    { copy(is_elem_d(elem) as u32, 0, 0b0111, lane(elem, index), rd.code(), rn.code()) }

    /// `SMOV <Wd|Xd>, <Vn>.<Ts>[index]`: SMOV, an element sign-extended to
    /// a general-purpose register.
    ///
    /// Precondition: the element is narrower than the register
    /// (`elem.bits() < sz.bits()`), and `index < elem.lanes()`.
    pub fn smov / try_smov(sz: Size, elem: Elem, rd: RegZr, rn: VReg, index: u32)
    requires elem.bits() < sz.bits() && index < elem.lanes();
    { copy(sz.bit(), 0, 0b0101, lane(elem, index), rd.code(), rn.code()) }
}

// Immediates.

checked! {
    /// `MOVI <Vd>.<T>, #imm8, LSL #shift`: MOVI, the 8-, 16- and 32-bit
    /// shifted forms: every element is `imm8 << shift`.
    ///
    /// Precondition: `shift` is 0 for byte elements; 0 or 8 for 16-bit
    /// elements; 0, 8, 16 or 24 for 32-bit elements. [`Arr::D2`] has no
    /// such form (see [`movi_mask`]).
    pub fn movi / try_movi(arr: Arr, rd: VReg, imm8: u8, shift: u32)
    requires shifted_cmode(arr, shift).is_some();
    {
        let cmode = or_zero(shifted_cmode(arr, shift));
        modified_imm(arr.vec_bits().q(), 0, cmode, imm8 as u32, rd)
    }

    /// `MVNI <Vd>.<T>, #imm8, LSL #shift`: MVNI, the 16- and 32-bit
    /// shifted forms: every element is `!(imm8 << shift)`.
    ///
    /// Precondition: the elements are 16 or 32 bits wide, and `shift` is 0
    /// or 8, or for 32-bit elements also 16 or 24.
    pub fn mvni / try_mvni(arr: Arr, rd: VReg, imm8: u8, shift: u32)
    requires !matches!(arr.elem(), Elem::B) && shifted_cmode(arr, shift).is_some();
    {
        let cmode = or_zero(shifted_cmode(arr, shift));
        modified_imm(arr.vec_bits().q(), 1, cmode, imm8 as u32, rd)
    }

    /// `MOVI <Dd>, #mask` / `MOVI <Vd>.2D, #mask`: MOVI, the 64-bit forms.
    /// [`VecBits::V64`] writes `mask` to the `D` view and zeroes the upper
    /// half; [`VecBits::V128`] writes it to both 64-bit elements. A zero
    /// `mask` is the idiom for clearing a register.
    ///
    /// Precondition: every byte of `mask` is `0x00` or `0xff`.
    pub fn movi_mask / try_movi_mask(bits: VecBits, rd: VReg, mask: u64)
    requires byte_mask_imm8(mask).is_some();
    { modified_imm(bits.q(), 1, 0b1110, or_zero(byte_mask_imm8(mask)), rd) }

    /// `FMOV <Vd>.<2S|4S|2D>, #imm`: FMOV (vector, immediate): every
    /// element is the floating-point constant with bit pattern `bits`.
    ///
    /// Precondition: [`encode_fp_imm8`]`(arr.fp_size(), bits).is_some()`.
    pub fn fmov_imm_vec / try_fmov_imm_vec(arr: FpArr, rd: VReg, bits: u64)
    requires encode_fp_imm8(arr.fp_size(), bits).is_some();
    {
        let imm8 = match encode_fp_imm8(arr.fp_size(), bits) {
            Some(imm8) => imm8 as u32,
            None => 0,
        };
        modified_imm(arr.arr().vec_bits().q(), arr.fp_size().bit(), 0b1111, imm8, rd)
    }
}

// Elementwise integer operations.

family! {
    (arr: Arr, rd: VReg, rn: VReg, rm: VReg) => int3;

    /// `ADD <Vd>.<T>, <Vn>.<T>, <Vm>.<T>`: ADD (vector).
    pub fn add_vec = (0, 0b1_0000);
    /// `SUB <Vd>.<T>, <Vn>.<T>, <Vm>.<T>`: SUB (vector), `rn - rm`.
    pub fn sub_vec = (1, 0b1_0000);
    /// `CMEQ <Vd>.<T>, <Vn>.<T>, <Vm>.<T>`: CMEQ (register). Like every
    /// compare here, an element of the result is all ones where the
    /// comparison holds and zero where it does not.
    pub fn cmeq = (1, 0b1_0001);
    /// `CMGT <Vd>.<T>, <Vn>.<T>, <Vm>.<T>`: CMGT (register), signed
    /// `rn > rm`.
    pub fn cmgt = (0, 0b0_0110);
    /// `CMGE <Vd>.<T>, <Vn>.<T>, <Vm>.<T>`: CMGE (register), signed
    /// `rn >= rm`.
    pub fn cmge = (0, 0b0_0111);
    /// `CMHI <Vd>.<T>, <Vn>.<T>, <Vm>.<T>`: CMHI (register), unsigned
    /// `rn > rm`.
    pub fn cmhi = (1, 0b0_0110);
    /// `CMHS <Vd>.<T>, <Vn>.<T>, <Vm>.<T>`: CMHS (register), unsigned
    /// `rn >= rm`.
    pub fn cmhs = (1, 0b0_0111);
    /// `CMTST <Vd>.<T>, <Vn>.<T>, <Vm>.<T>`: CMTST, `rn & rm != 0`.
    pub fn cmtst = (0, 0b1_0001);
    /// `SSHL <Vd>.<T>, <Vn>.<T>, <Vm>.<T>`: SSHL, each element of `rn`
    /// shifted left by the signed low byte of the element of `rm`; a
    /// negative amount is an arithmetic shift right.
    pub fn sshl = (0, 0b0_1000);
    /// `USHL <Vd>.<T>, <Vn>.<T>, <Vm>.<T>`: USHL, as [`sshl`] with a
    /// logical shift right for a negative amount.
    pub fn ushl = (1, 0b0_1000);
}

family! {
    (arr: Arr, rd: VReg, rn: VReg, rm: VReg) requires (!has_elem_d(arr)) => int3;

    /// `MUL <Vd>.<T>, <Vn>.<T>, <Vm>.<T>`: MUL (vector).
    ///
    /// Precondition: `arr` is not [`Arr::D2`].
    pub fn mul_vec / try_mul_vec = (0, 0b1_0011);
    /// `SMAX <Vd>.<T>, <Vn>.<T>, <Vm>.<T>`: SMAX.
    ///
    /// Precondition: `arr` is not [`Arr::D2`].
    pub fn smax / try_smax = (0, 0b0_1100);
    /// `SMIN <Vd>.<T>, <Vn>.<T>, <Vm>.<T>`: SMIN.
    ///
    /// Precondition: `arr` is not [`Arr::D2`].
    pub fn smin / try_smin = (0, 0b0_1101);
    /// `UMAX <Vd>.<T>, <Vn>.<T>, <Vm>.<T>`: UMAX.
    ///
    /// Precondition: `arr` is not [`Arr::D2`].
    pub fn umax / try_umax = (1, 0b0_1100);
    /// `UMIN <Vd>.<T>, <Vn>.<T>, <Vm>.<T>`: UMIN.
    ///
    /// Precondition: `arr` is not [`Arr::D2`].
    pub fn umin / try_umin = (1, 0b0_1101);
}

family! {
    (arr: Arr, rd: VReg, rn: VReg) => int2;

    /// `ABS <Vd>.<T>, <Vn>.<T>`: ABS (vector).
    pub fn abs = (0, 0b0_1011);
    /// `NEG <Vd>.<T>, <Vn>.<T>`: NEG (vector).
    pub fn neg_vec = (1, 0b0_1011);
    /// `CMEQ <Vd>.<T>, <Vn>.<T>, #0`: CMEQ (zero).
    pub fn cmeq_zero = (0, 0b0_1001);
    /// `CMGT <Vd>.<T>, <Vn>.<T>, #0`: CMGT (zero), signed `rn > 0`.
    pub fn cmgt_zero = (0, 0b0_1000);
    /// `CMGE <Vd>.<T>, <Vn>.<T>, #0`: CMGE (zero), signed `rn >= 0`.
    pub fn cmge_zero = (1, 0b0_1000);
    /// `CMLT <Vd>.<T>, <Vn>.<T>, #0`: CMLT (zero), signed `rn < 0`.
    pub fn cmlt_zero = (0, 0b0_1010);
    /// `CMLE <Vd>.<T>, <Vn>.<T>, #0`: CMLE (zero), signed `rn <= 0`.
    pub fn cmle_zero = (1, 0b0_1001);
}

family! {
    (arr: Arr, rd: VReg, rn: VReg) requires (!has_elem_d(arr)) => int2;

    /// `CLZ <Vd>.<T>, <Vn>.<T>`: CLZ (vector).
    ///
    /// Precondition: `arr` is not [`Arr::D2`].
    pub fn clz_vec / try_clz_vec = (1, 0b0_0100);
    /// `REV64 <Vd>.<T>, <Vn>.<T>`: REV64, the elements reversed within
    /// each 64 bits.
    ///
    /// Precondition: `arr` is not [`Arr::D2`].
    pub fn rev64 / try_rev64 = (0, 0b0_0000);
    /// `UADDLP <Vd>.<Ta>, <Vn>.<T>`: UADDLP, the sums of adjacent pairs of
    /// unsigned elements, in elements twice as wide. `arr` is the source
    /// arrangement.
    ///
    /// Precondition: `arr` is not [`Arr::D2`].
    pub fn uaddlp / try_uaddlp = (1, 0b0_0010);
}

checked! {
    /// `REV32 <Vd>.<T>, <Vn>.<T>`: REV32 (vector), the elements reversed
    /// within each 32 bits.
    ///
    /// Precondition: the elements are 8 or 16 bits wide.
    pub fn rev32_vec / try_rev32_vec(arr: Arr, rd: VReg, rn: VReg)
    requires matches!(arr.elem(), Elem::B | Elem::H);
    { int2(1, 0b0_0000, arr, rd, rn) }
}

family! {
    (bits: VecBits, rd: VReg, rn: VReg) => bytes2;

    /// `REV16 <Vd>.<8B|16B>, <Vn>.<8B|16B>`: REV16 (vector), the bytes
    /// swapped within each 16 bits.
    pub fn rev16_vec = (0, 0b00, 0b0_0001);
    /// `CNT <Vd>.<8B|16B>, <Vn>.<8B|16B>`: CNT, the number of set bits in
    /// each byte.
    pub fn cnt = (0, 0b00, 0b0_0101);
    /// `NOT <Vd>.<8B|16B>, <Vn>.<8B|16B>`: NOT (`MVN` in assembly).
    pub fn not = (1, 0b00, 0b0_0101);
    /// `RBIT <Vd>.<8B|16B>, <Vn>.<8B|16B>`: RBIT (vector), the bits
    /// reversed within each byte.
    pub fn rbit_vec = (1, 0b01, 0b0_0101);
}

// Bitwise operations and selects.

family! {
    (bits: VecBits, rd: VReg, rn: VReg, rm: VReg) => bits3;

    /// `AND <Vd>.<8B|16B>, <Vn>.<8B|16B>, <Vm>.<8B|16B>`: AND (vector).
    pub fn and_vec = (0, 0b00);
    /// `BIC <Vd>.<8B|16B>, <Vn>.<8B|16B>, <Vm>.<8B|16B>`: BIC (vector,
    /// register), `rn & !rm`.
    pub fn bic_vec = (0, 0b01);
    /// `ORR <Vd>.<8B|16B>, <Vn>.<8B|16B>, <Vm>.<8B|16B>`: ORR (vector,
    /// register).
    pub fn orr_vec = (0, 0b10);
    /// `ORN <Vd>.<8B|16B>, <Vn>.<8B|16B>, <Vm>.<8B|16B>`: ORN (vector),
    /// `rn | !rm`.
    pub fn orn_vec = (0, 0b11);
    /// `EOR <Vd>.<8B|16B>, <Vn>.<8B|16B>, <Vm>.<8B|16B>`: EOR (vector).
    pub fn eor_vec = (1, 0b00);
    /// `BSL <Vd>.<8B|16B>, <Vn>.<8B|16B>, <Vm>.<8B|16B>`: BSL, a bitwise
    /// select with the mask in `rd`: `rd = (rd & rn) | (!rd & rm)`.
    pub fn bsl = (1, 0b01);
    /// `BIT <Vd>.<8B|16B>, <Vn>.<8B|16B>, <Vm>.<8B|16B>`: BIT, insert
    /// where the mask `rm` is set: `rd = (rm & rn) | (!rm & rd)`.
    pub fn bit = (1, 0b10);
    /// `BIF <Vd>.<8B|16B>, <Vn>.<8B|16B>, <Vm>.<8B|16B>`: BIF, insert
    /// where the mask `rm` is clear: `rd = (!rm & rn) | (rm & rd)`.
    pub fn bif = (1, 0b11);
}

// Shifts by an immediate, widening and narrowing.

checked! {
    /// `SHL <Vd>.<T>, <Vn>.<T>, #amount`: SHL.
    ///
    /// Precondition: `amount < arr.elem().bits()`.
    pub fn shl / try_shl(arr: Arr, rd: VReg, rn: VReg, amount: u32)
    requires amount < arr.elem().bits();
    { shl_fields(arr, rd, rn, amount) }
}

family! {
    (arr: Arr, rd: VReg, rn: VReg, amount: u32)
    requires (amount >= 1 && amount <= arr.elem().bits()) => shr_fields;

    /// `SSHR <Vd>.<T>, <Vn>.<T>, #amount`: SSHR, arithmetic shift right.
    ///
    /// Precondition: `1 <= amount <= arr.elem().bits()`.
    pub fn sshr / try_sshr = (0);
    /// `USHR <Vd>.<T>, <Vn>.<T>, #amount`: USHR, logical shift right.
    ///
    /// Precondition: `1 <= amount <= arr.elem().bits()`.
    pub fn ushr / try_ushr = (1);
}

family! {
    (from: Elem, rd: VReg, rn: VReg, amount: u32)
    requires (!is_elem_d(from) && amount < from.bits()) => shll_fields;

    /// `SSHLL <Vd>.<Ta>, <Vn>.<Tb>, #amount`: SSHLL, the lower half of
    /// `rn` (elements of width `from`) sign-extended to elements twice as
    /// wide and shifted left.
    ///
    /// Precondition: `from` is not [`Elem::D`], and `amount < from.bits()`.
    pub fn sshll / try_sshll = (0);
    /// `USHLL <Vd>.<Ta>, <Vn>.<Tb>, #amount`: USHLL, as [`sshll`] with
    /// zero extension.
    ///
    /// Precondition: `from` is not [`Elem::D`], and `amount < from.bits()`.
    pub fn ushll / try_ushll = (1);
}

family! {
    (from: Elem, rd: VReg, rn: VReg) requires (!is_elem_d(from)) => xtl_fields;

    /// `SXTL <Vd>.<Ta>, <Vn>.<Tb>`: SXTL, that is SSHLL by zero: the lower
    /// half of `rn` sign-extended to elements twice as wide.
    ///
    /// Precondition: `from` is not [`Elem::D`].
    pub fn sxtl / try_sxtl = (0);
    /// `UXTL <Vd>.<Ta>, <Vn>.<Tb>`: UXTL, that is USHLL by zero: the lower
    /// half of `rn` zero-extended to elements twice as wide.
    ///
    /// Precondition: `from` is not [`Elem::D`].
    pub fn uxtl / try_uxtl = (1);
}

checked! {
    /// `XTN <Vd>.<Tb>, <Vn>.<Ta>`: XTN, each element of `rn` truncated to
    /// half its width, `to`; the result fills the lower half of `rd` and
    /// the upper half is zeroed.
    ///
    /// Precondition: `to` is not [`Elem::D`].
    pub fn xtn / try_xtn(to: Elem, rd: VReg, rn: VReg)
    requires !is_elem_d(to);
    { two_misc(0, 0b1_0010, to.log2() << 22, rd, rn) }
}

// Permutes and table lookups.

family! {
    (arr: Arr, rd: VReg, rn: VReg, rm: VReg) => permute;

    /// `UZP1 <Vd>.<T>, <Vn>.<T>, <Vm>.<T>`: UZP1, the even-numbered
    /// elements of `rn:rm`.
    pub fn uzp1 = (0b001);
    /// `TRN1 <Vd>.<T>, <Vn>.<T>, <Vm>.<T>`: TRN1, the even-numbered
    /// elements of `rn` and `rm`, interleaved.
    pub fn trn1 = (0b010);
    /// `ZIP1 <Vd>.<T>, <Vn>.<T>, <Vm>.<T>`: ZIP1, the lower halves of `rn`
    /// and `rm`, interleaved.
    pub fn zip1 = (0b011);
    /// `UZP2 <Vd>.<T>, <Vn>.<T>, <Vm>.<T>`: UZP2, the odd-numbered
    /// elements of `rn:rm`.
    pub fn uzp2 = (0b101);
    /// `TRN2 <Vd>.<T>, <Vn>.<T>, <Vm>.<T>`: TRN2, the odd-numbered
    /// elements of `rn` and `rm`, interleaved.
    pub fn trn2 = (0b110);
    /// `ZIP2 <Vd>.<T>, <Vn>.<T>, <Vm>.<T>`: ZIP2, the upper halves of `rn`
    /// and `rm`, interleaved.
    pub fn zip2 = (0b111);
}

checked! {
    /// `EXT <Vd>.<8B|16B>, <Vn>.<8B|16B>, <Vm>.<8B|16B>, #index`: EXT, the
    /// bytes of `rm:rn` starting at byte `index` of `rn`.
    ///
    /// Precondition: `index` is below the number of bytes in the vector
    /// (8 or 16).
    pub fn ext / try_ext(bits: VecBits, rd: VReg, rn: VReg, rm: VReg, index: u32)
    requires index < bits.bits() / 8;
    { 0x2e00_0000 | bits.q() | rm.code() << 16 | index << 11 | rn.code() << 5 | rd.code() }
}

family! {
    (bits: VecBits, rd: VReg, rn: VReg, rm: VReg) => table;

    /// `TBL <Vd>.<8B|16B>, { <Vn>.16B }, <Vm>.<8B|16B>`: TBL with a
    /// one-register table: each byte of `rm` indexes the 16 bytes of `rn`;
    /// an index out of range gives zero.
    pub fn tbl1 = (0);
    /// `TBL <Vd>.<8B|16B>, { <Vn>.16B, <Vn+1>.16B }, <Vm>.<8B|16B>`: TBL
    /// with a two-register table: `rn` and the register after it (`V0`
    /// after `V31`).
    pub fn tbl2 = (1);
}

// Reductions.

family! {
    (arr: Arr, rd: VReg, rn: VReg) requires (reducible(arr)) => across;

    /// `ADDV <Bd|Hd|Sd>, <Vn>.<T>`: ADDV, the sum of the elements, in the
    /// scalar view of `rd` of the element width.
    ///
    /// Precondition: `arr` is not [`Arr::S2`] or [`Arr::D2`].
    pub fn addv / try_addv = (0, 0b1_1011);
    /// `UADDLV <Hd|Sd|Dd>, <Vn>.<T>`: UADDLV, the sum of the unsigned
    /// elements, in the scalar view of `rd` twice the element width.
    ///
    /// Precondition: `arr` is not [`Arr::S2`] or [`Arr::D2`].
    pub fn uaddlv / try_uaddlv = (1, 0b0_0011);
    /// `SMAXV <Bd|Hd|Sd>, <Vn>.<T>`: SMAXV, the signed maximum element.
    ///
    /// Precondition: `arr` is not [`Arr::S2`] or [`Arr::D2`].
    pub fn smaxv / try_smaxv = (0, 0b0_1010);
    /// `SMINV <Bd|Hd|Sd>, <Vn>.<T>`: SMINV, the signed minimum element.
    ///
    /// Precondition: `arr` is not [`Arr::S2`] or [`Arr::D2`].
    pub fn sminv / try_sminv = (0, 0b1_1010);
    /// `UMAXV <Bd|Hd|Sd>, <Vn>.<T>`: UMAXV, the unsigned maximum element.
    ///
    /// Precondition: `arr` is not [`Arr::S2`] or [`Arr::D2`].
    pub fn umaxv / try_umaxv = (1, 0b0_1010);
    /// `UMINV <Bd|Hd|Sd>, <Vn>.<T>`: UMINV, the unsigned minimum element.
    ///
    /// Precondition: `arr` is not [`Arr::S2`] or [`Arr::D2`].
    pub fn uminv / try_uminv = (1, 0b1_1010);
}

// Elementwise floating point.

family! {
    (arr: FpArr, rd: VReg, rn: VReg, rm: VReg) => fp3;

    /// `FADD <Vd>.<T>, <Vn>.<T>, <Vm>.<T>`: FADD (vector).
    pub fn fadd_vec = (0, 0, 0b1_1010);
    /// `FSUB <Vd>.<T>, <Vn>.<T>, <Vm>.<T>`: FSUB (vector), `rn - rm`.
    pub fn fsub_vec = (0, 1, 0b1_1010);
    /// `FMUL <Vd>.<T>, <Vn>.<T>, <Vm>.<T>`: FMUL (vector).
    pub fn fmul_vec = (1, 0, 0b1_1011);
    /// `FDIV <Vd>.<T>, <Vn>.<T>, <Vm>.<T>`: FDIV (vector), `rn / rm`.
    pub fn fdiv_vec = (1, 0, 0b1_1111);
    /// `FMAX <Vd>.<T>, <Vn>.<T>, <Vm>.<T>`: FMAX (vector).
    pub fn fmax_vec = (0, 0, 0b1_1110);
    /// `FMIN <Vd>.<T>, <Vn>.<T>, <Vm>.<T>`: FMIN (vector).
    pub fn fmin_vec = (0, 1, 0b1_1110);
    /// `FMAXNM <Vd>.<T>, <Vn>.<T>, <Vm>.<T>`: FMAXNM (vector).
    pub fn fmaxnm_vec = (0, 0, 0b1_1000);
    /// `FMINNM <Vd>.<T>, <Vn>.<T>, <Vm>.<T>`: FMINNM (vector).
    pub fn fminnm_vec = (0, 1, 0b1_1000);
    /// `FMLA <Vd>.<T>, <Vn>.<T>, <Vm>.<T>`: FMLA (vector),
    /// `rd + rn * rm` with one rounding.
    pub fn fmla = (0, 0, 0b1_1001);
    /// `FMLS <Vd>.<T>, <Vn>.<T>, <Vm>.<T>`: FMLS (vector),
    /// `rd - rn * rm` with one rounding.
    pub fn fmls = (0, 1, 0b1_1001);
    /// `FCMEQ <Vd>.<T>, <Vn>.<T>, <Vm>.<T>`: FCMEQ (register). False for
    /// an unordered pair, as are the two below.
    pub fn fcmeq = (0, 0, 0b1_1100);
    /// `FCMGE <Vd>.<T>, <Vn>.<T>, <Vm>.<T>`: FCMGE (register), `rn >= rm`.
    pub fn fcmge = (1, 0, 0b1_1100);
    /// `FCMGT <Vd>.<T>, <Vn>.<T>, <Vm>.<T>`: FCMGT (register), `rn > rm`.
    pub fn fcmgt = (1, 1, 0b1_1100);
}

family! {
    (arr: FpArr, rd: VReg, rn: VReg) => fp2;

    /// `FABS <Vd>.<T>, <Vn>.<T>`: FABS (vector).
    pub fn fabs_vec = (0, 1, 0b0_1111);
    /// `FNEG <Vd>.<T>, <Vn>.<T>`: FNEG (vector).
    pub fn fneg_vec = (1, 1, 0b0_1111);
    /// `FSQRT <Vd>.<T>, <Vn>.<T>`: FSQRT (vector).
    pub fn fsqrt_vec = (1, 1, 0b1_1111);
    /// `SCVTF <Vd>.<T>, <Vn>.<T>`: SCVTF (vector, integer), signed integer
    /// elements to floating point of the same width.
    pub fn scvtf_vec = (0, 0, 0b1_1101);
    /// `UCVTF <Vd>.<T>, <Vn>.<T>`: UCVTF (vector, integer), unsigned
    /// integer elements to floating point of the same width.
    pub fn ucvtf_vec = (1, 0, 0b1_1101);
    /// `FCVTZS <Vd>.<T>, <Vn>.<T>`: FCVTZS (vector, integer), to signed
    /// integer elements, toward zero, saturating.
    pub fn fcvtzs_vec = (0, 1, 0b1_1011);
    /// `FCVTZU <Vd>.<T>, <Vn>.<T>`: FCVTZU (vector, integer), to unsigned
    /// integer elements, toward zero, saturating.
    pub fn fcvtzu_vec = (1, 1, 0b1_1011);
    /// `FCMEQ <Vd>.<T>, <Vn>.<T>, #0.0`: FCMEQ (zero).
    pub fn fcmeq_zero = (0, 1, 0b0_1101);
    /// `FCMGT <Vd>.<T>, <Vn>.<T>, #0.0`: FCMGT (zero), `rn > 0`.
    pub fn fcmgt_zero = (0, 1, 0b0_1100);
    /// `FCMGE <Vd>.<T>, <Vn>.<T>, #0.0`: FCMGE (zero), `rn >= 0`.
    pub fn fcmge_zero = (1, 1, 0b0_1100);
    /// `FCMLT <Vd>.<T>, <Vn>.<T>, #0.0`: FCMLT (zero), `rn < 0`.
    pub fn fcmlt_zero = (0, 1, 0b0_1110);
    /// `FCMLE <Vd>.<T>, <Vn>.<T>, #0.0`: FCMLE (zero), `rn <= 0`.
    pub fn fcmle_zero = (1, 1, 0b0_1101);
    /// `FRINTN <Vd>.<T>, <Vn>.<T>`: FRINTN (vector), to nearest with ties
    /// to even.
    pub fn frintn_vec = (0, 0, 0b1_1000);
    /// `FRINTM <Vd>.<T>, <Vn>.<T>`: FRINTM (vector), toward minus
    /// infinity.
    pub fn frintm_vec = (0, 0, 0b1_1001);
    /// `FRINTP <Vd>.<T>, <Vn>.<T>`: FRINTP (vector), toward plus infinity.
    pub fn frintp_vec = (0, 1, 0b1_1000);
    /// `FRINTZ <Vd>.<T>, <Vn>.<T>`: FRINTZ (vector), toward zero.
    pub fn frintz_vec = (0, 1, 0b1_1001);
    /// `FRINTA <Vd>.<T>, <Vn>.<T>`: FRINTA (vector), to nearest with ties
    /// away from zero.
    pub fn frinta_vec = (1, 0, 0b1_1000);
    /// `FRINTX <Vd>.<T>, <Vn>.<T>`: FRINTX (vector), in the current
    /// rounding mode, signalling inexact.
    pub fn frintx_vec = (1, 0, 0b1_1001);
    /// `FRINTI <Vd>.<T>, <Vn>.<T>`: FRINTI (vector), in the current
    /// rounding mode.
    pub fn frinti_vec = (1, 1, 0b1_1001);
}

/// `FCVTL <Vd>.4S, <Vn>.4H` / `FCVTL <Vd>.2D, <Vn>.2S`: FCVTL, the lower
/// half of `rn` converted to the next wider format, `wide`.
#[inline]
pub const fn fcvtl(wide: FpSize, rd: VReg, rn: VReg) -> u32 {
    two_misc(0, 0b1_0111, wide.bit() << 22, rd, rn)
}

/// `FCVTN <Vd>.4H, <Vn>.4S` / `FCVTN <Vd>.2S, <Vn>.2D`: FCVTN, each
/// element of `rn`, of format `wide`, converted to the next narrower format
/// in the current rounding mode; the result fills the lower half of `rd`
/// and the upper half is zeroed.
#[inline]
pub const fn fcvtn(wide: FpSize, rd: VReg, rn: VReg) -> u32 {
    two_misc(0, 0b1_0110, wide.bit() << 22, rd, rn)
}
