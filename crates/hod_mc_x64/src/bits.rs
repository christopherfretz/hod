//! Bit scans, counts and byte swap (`spec/mc-x64.md` "Bit counting").

use crate::alu::not_byte;
use crate::inst::{Inst, Op, plus_r, rmem, rr};
use crate::reg::{Mem, Reg, Size};

/// `op r, r/m`, register source.
#[inline]
const fn count_rr(rep: u8, opcode: u8, size: Size, dst: Reg, src: Reg) -> Inst {
    rr(Op::two(opcode).rep(rep).sized(size, 0), size, dst, src)
}

/// `op r, r/m`, memory source.
#[inline]
const fn count_rm(rep: u8, opcode: u8, size: Size, dst: Reg, src: Mem) -> Inst {
    rmem(Op::two(opcode).rep(rep).sized(size, 0), size, dst, src)
}

family! {
    (size: Size, dst: Reg, src: Reg) requires (not_byte(size)) => count_rr;
    /// `BSF r, r/m` (`0F BC /r`): index of the lowest set bit; `dst`
    /// unchanged and ZF set when `src` is 0.
    pub fn bsf_rr / try_bsf_rr = (0, 0xbc);
    /// `BSR r, r/m` (`0F BD /r`): index of the highest set bit.
    pub fn bsr_rr / try_bsr_rr = (0, 0xbd);
    /// `TZCNT r, r/m` (`F3 0F BC /r`, BMI1): trailing zeros; the width for 0.
    pub fn tzcnt_rr / try_tzcnt_rr = (0xf3, 0xbc);
    /// `LZCNT r, r/m` (`F3 0F BD /r`, LZCNT): leading zeros; the width for 0.
    pub fn lzcnt_rr / try_lzcnt_rr = (0xf3, 0xbd);
    /// `POPCNT r, r/m` (`F3 0F B8 /r`, POPCNT): set bits.
    pub fn popcnt_rr / try_popcnt_rr = (0xf3, 0xb8);
}

family! {
    (size: Size, dst: Reg, src: Mem) requires (not_byte(size)) => count_rm;
    /// `BSF r, m`.
    pub fn bsf_rm / try_bsf_rm = (0, 0xbc);
    /// `BSR r, m`.
    pub fn bsr_rm / try_bsr_rm = (0, 0xbd);
    /// `TZCNT r, m`.
    pub fn tzcnt_rm / try_tzcnt_rm = (0xf3, 0xbc);
    /// `LZCNT r, m`.
    pub fn lzcnt_rm / try_lzcnt_rm = (0xf3, 0xbd);
    /// `POPCNT r, m`.
    pub fn popcnt_rm / try_popcnt_rm = (0xf3, 0xb8);
}

checked! {
    /// `BSWAP r32`/`r64` (`0F C8+r`). The 16-bit form is undefined.
    pub fn bswap / try_bswap(size: Size, r: Reg)
    requires matches!(size, Size::D | Size::Q);
    { plus_r(Op::two(0xc8).sized(size, 0), r, false) }
}
