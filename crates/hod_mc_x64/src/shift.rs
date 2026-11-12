//! Shifts and rotates by an immediate and by `cl`, and the double shifts
//! `shld`/`shrd` (`spec/mc-x64.md` "Shifts").

use crate::alu::not_byte;
use crate::inst::{Inst, Op, Rm, digit_m, digit_r, imm, modrm};
use crate::reg::{Mem, Reg, Size};

/// By `n`: `D0`/`D1 /d` when `n == 1`, else `C0`/`C1 /d ib`.
#[inline]
const fn shift_ri(digit: u8, size: Size, r: Reg, n: u8) -> Inst {
    if n == 1 {
        digit_r(Op::one(0xd0).sized(size, 1), digit, size, r)
    } else {
        imm(digit_r(Op::one(0xc0).sized(size, 1), digit, size, r), n as i64, 1)
    }
}

#[inline]
const fn shift_mi(digit: u8, size: Size, m: Mem, n: u8) -> Inst {
    if n == 1 {
        digit_m(Op::one(0xd0).sized(size, 1), digit, m)
    } else {
        imm(digit_m(Op::one(0xc0).sized(size, 1), digit, m), n as i64, 1)
    }
}

/// By `cl`: `D2`/`D3 /d`.
#[inline]
const fn shift_rcl(digit: u8, size: Size, r: Reg) -> Inst {
    digit_r(Op::one(0xd2).sized(size, 1), digit, size, r)
}

#[inline]
const fn shift_mcl(digit: u8, size: Size, m: Mem) -> Inst {
    digit_m(Op::one(0xd2).sized(size, 1), digit, m)
}

family! {
    (size: Size, r: Reg, n: u8) => shift_ri;
    /// `ROL r, imm8` (`/0`). The processor masks the count to 5 bits (6 for
    /// 64-bit operands).
    pub fn rol_ri = (0);
    /// `ROR r, imm8` (`/1`).
    pub fn ror_ri = (1);
    /// `RCL r, imm8` (`/2`).
    pub fn rcl_ri = (2);
    /// `RCR r, imm8` (`/3`).
    pub fn rcr_ri = (3);
    /// `SHL r, imm8` (`/4`; `SAL` is the same instruction).
    pub fn shl_ri = (4);
    /// `SHR r, imm8` (`/5`).
    pub fn shr_ri = (5);
    /// `SAR r, imm8` (`/7`).
    pub fn sar_ri = (7);
}

family! {
    (size: Size, m: Mem, n: u8) => shift_mi;
    /// `ROL m, imm8`.
    pub fn rol_mi = (0);
    /// `ROR m, imm8`.
    pub fn ror_mi = (1);
    /// `RCL m, imm8`.
    pub fn rcl_mi = (2);
    /// `RCR m, imm8`.
    pub fn rcr_mi = (3);
    /// `SHL m, imm8`.
    pub fn shl_mi = (4);
    /// `SHR m, imm8`.
    pub fn shr_mi = (5);
    /// `SAR m, imm8`.
    pub fn sar_mi = (7);
}

family! {
    (size: Size, r: Reg) => shift_rcl;
    /// `ROL r, cl`.
    pub fn rol_rcl = (0);
    /// `ROR r, cl`.
    pub fn ror_rcl = (1);
    /// `RCL r, cl`.
    pub fn rcl_rcl = (2);
    /// `RCR r, cl`.
    pub fn rcr_rcl = (3);
    /// `SHL r, cl`.
    pub fn shl_rcl = (4);
    /// `SHR r, cl`.
    pub fn shr_rcl = (5);
    /// `SAR r, cl`.
    pub fn sar_rcl = (7);
}

family! {
    (size: Size, m: Mem) => shift_mcl;
    /// `ROL m, cl`.
    pub fn rol_mcl = (0);
    /// `ROR m, cl`.
    pub fn ror_mcl = (1);
    /// `RCL m, cl`.
    pub fn rcl_mcl = (2);
    /// `RCR m, cl`.
    pub fn rcr_mcl = (3);
    /// `SHL m, cl`.
    pub fn shl_mcl = (4);
    /// `SHR m, cl`.
    pub fn shr_mcl = (5);
    /// `SAR m, cl`.
    pub fn sar_mcl = (7);
}

/// `SHLD`/`SHRD r/m, r, imm8` (`0F A4`/`AC /r ib`) or `.., cl` (`0F
/// A5`/`AD /r`): `opcode` is the immediate form.
#[inline]
const fn double(opcode: u8, size: Size, dst: Rm, src: Reg, n: Option<u8>) -> Inst {
    match n {
        Some(n) => {
            imm(modrm(Op::two(opcode).sized(size, 0), src.code(), false, dst, false), n as i64, 1)
        }
        None => modrm(Op::two(opcode | 1).sized(size, 0), src.code(), false, dst, false),
    }
}

checked! {
    /// `SHLD r, r, imm8`: shifts `dst` left, filling from the top of `src`.
    pub fn shld_rri / try_shld_rri(size: Size, dst: Reg, src: Reg, n: u8)
    requires not_byte(size);
    { double(0xa4, size, Rm::Reg(dst), src, Some(n)) }

    /// `SHLD r, r, cl`.
    pub fn shld_rrcl / try_shld_rrcl(size: Size, dst: Reg, src: Reg)
    requires not_byte(size);
    { double(0xa4, size, Rm::Reg(dst), src, None) }

    /// `SHLD m, r, imm8`.
    pub fn shld_mri / try_shld_mri(size: Size, dst: Mem, src: Reg, n: u8)
    requires not_byte(size);
    { double(0xa4, size, Rm::Mem(dst), src, Some(n)) }

    /// `SHLD m, r, cl`.
    pub fn shld_mrcl / try_shld_mrcl(size: Size, dst: Mem, src: Reg)
    requires not_byte(size);
    { double(0xa4, size, Rm::Mem(dst), src, None) }

    /// `SHRD r, r, imm8`: shifts `dst` right, filling from the bottom of
    /// `src`.
    pub fn shrd_rri / try_shrd_rri(size: Size, dst: Reg, src: Reg, n: u8)
    requires not_byte(size);
    { double(0xac, size, Rm::Reg(dst), src, Some(n)) }

    /// `SHRD r, r, cl`.
    pub fn shrd_rrcl / try_shrd_rrcl(size: Size, dst: Reg, src: Reg)
    requires not_byte(size);
    { double(0xac, size, Rm::Reg(dst), src, None) }

    /// `SHRD m, r, imm8`.
    pub fn shrd_mri / try_shrd_mri(size: Size, dst: Mem, src: Reg, n: u8)
    requires not_byte(size);
    { double(0xac, size, Rm::Mem(dst), src, Some(n)) }

    /// `SHRD m, r, cl`.
    pub fn shrd_mrcl / try_shrd_mrcl(size: Size, dst: Mem, src: Reg)
    requires not_byte(size);
    { double(0xac, size, Rm::Mem(dst), src, None) }
}
