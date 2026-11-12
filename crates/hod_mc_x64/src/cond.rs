//! Conditional set and conditional move (`spec/mc-x64.md` "Conditional").

use crate::alu::not_byte;
use crate::inst::{Inst, Op, digit_m, digit_r, rmem, rr};
use crate::reg::{Cond, Mem, Reg, Size};

/// `SETcc r8` (`0F 90+cc /0`): the byte register is 1 if `cond` holds,
/// else 0.
#[inline]
pub const fn setcc_r(cond: Cond, dst: Reg) -> Inst {
    digit_r(Op::two(0x90 | cond as u8), 0, Size::B, dst)
}

/// `SETcc m8` (`0F 90+cc /0`).
#[inline]
pub const fn setcc_m(cond: Cond, dst: Mem) -> Inst {
    digit_m(Op::two(0x90 | cond as u8), 0, dst)
}

checked! {
    /// `CMOVcc r, r/m` with a register source (`0F 40+cc /r`). A 32-bit
    /// CMOV zeroes the upper half of `dst` whether or not it moves.
    pub fn cmovcc_rr / try_cmovcc_rr(cond: Cond, size: Size, dst: Reg, src: Reg)
    requires not_byte(size);
    { rr(Op::two(0x40 | cond as u8).sized(size, 0), size, dst, src) }

    /// `CMOVcc r, r/m` with a memory source. The load happens whether or
    /// not `cond` holds.
    pub fn cmovcc_rm / try_cmovcc_rm(cond: Cond, size: Size, dst: Reg, src: Mem)
    requires not_byte(size);
    { rmem(Op::two(0x40 | cond as u8).sized(size, 0), size, dst, src) }
}
