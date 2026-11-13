//! Branches, calls, returns and the stack (`spec/mc-x64.md` "Branches and
//! calls", "Stack").
//!
//! A displacement is the field's value: the distance from the end of the
//! branch to its target.

use crate::fixup::Fixup;
use crate::inst::{Inst, Op, abs_imm, digit_m, digit_r, imm, plain, plus_r, rel};
use crate::reg::{Cond, Mem, Reg, Size};

/// `JMP rel8` (`EB cb`).
#[inline]
pub const fn jmp_short(disp: i8) -> Inst {
    rel(plain(Op::one(0xeb)), Fixup::Rel8, disp as i64)
}

/// `JMP rel32` (`E9 cd`).
#[inline]
pub const fn jmp_near(disp: i32) -> Inst {
    rel(plain(Op::one(0xe9)), Fixup::Rel32, disp as i64)
}

/// `Jcc rel8` (`70+cc cb`).
#[inline]
pub const fn jcc_short(cond: Cond, disp: i8) -> Inst {
    rel(plain(Op::one(0x70 | cond as u8)), Fixup::Rel8, disp as i64)
}

/// `Jcc rel32` (`0F 80+cc cd`).
#[inline]
pub const fn jcc_near(cond: Cond, disp: i32) -> Inst {
    rel(plain(Op::two(0x80 | cond as u8)), Fixup::Rel32, disp as i64)
}

/// `CALL rel32` (`E8 cd`).
#[inline]
pub const fn call(disp: i32) -> Inst {
    rel(plain(Op::one(0xe8)), Fixup::Rel32, disp as i64)
}

/// `JMP r64` (`FF /4`).
#[inline]
pub const fn jmp_r(target: Reg) -> Inst {
    digit_r(Op::one(0xff), 4, Size::Q, target)
}

/// `JMP m64` (`FF /4`).
#[inline]
pub const fn jmp_m(target: Mem) -> Inst {
    digit_m(Op::one(0xff), 4, target)
}

/// `CALL r64` (`FF /2`).
#[inline]
pub const fn call_r(target: Reg) -> Inst {
    digit_r(Op::one(0xff), 2, Size::Q, target)
}

/// `CALL m64` (`FF /2`).
#[inline]
pub const fn call_m(target: Mem) -> Inst {
    digit_m(Op::one(0xff), 2, target)
}

/// `RET` (`C3`).
#[inline]
pub const fn ret() -> Inst {
    plain(Op::one(0xc3))
}

/// `RET imm16` (`C2 iw`): returns, then pops `n` more bytes.
#[inline]
pub const fn ret_imm(n: u16) -> Inst {
    imm(plain(Op::one(0xc2)), n as i64, 2)
}

/// `PUSH r64` (`50+r`).
#[inline]
pub const fn push_r(r: Reg) -> Inst {
    plus_r(Op::one(0x50), r, false)
}

/// `POP r64` (`58+r`).
#[inline]
pub const fn pop_r(r: Reg) -> Inst {
    plus_r(Op::one(0x58), r, false)
}

/// `PUSHFQ` (`9C`): pushes `rflags` as 8 bytes.
#[inline]
pub const fn pushfq() -> Inst {
    plain(Op::one(0x9c))
}

/// `POPFQ` (`9D`): pops 8 bytes into `rflags` (the bits user code may
/// write).
#[inline]
pub const fn popfq() -> Inst {
    plain(Op::one(0x9d))
}

/// `PUSH m64` (`FF /6`).
#[inline]
pub const fn push_m(m: Mem) -> Inst {
    digit_m(Op::one(0xff), 6, m)
}

/// `POP m64` (`8F /0`).
#[inline]
pub const fn pop_m(m: Mem) -> Inst {
    digit_m(Op::one(0x8f), 0, m)
}

/// `PUSH imm`: pushes the sign-extended value as 8 bytes; `6A ib` for a
/// sign-extended byte, else `68 id`.
#[inline]
pub const fn push_i(value: i32) -> Inst {
    if value >= -128 && value <= 127 {
        imm(plain(Op::one(0x6a)), value as i64, 1)
    } else {
        imm(plain(Op::one(0x68)), value as i64, 4)
    }
}

/// `PUSH imm32` (`68 id`), always the 4-byte immediate, which is the
/// instruction's site, [`Fixup::Abs32`] (`R_X86_64_32S`): pushes the
/// sign-extended value as 8 bytes.
#[inline]
pub const fn push_i32(value: i32) -> Inst {
    abs_imm(plain(Op::one(0x68)), Fixup::Abs32, value as i64)
}

/// `LEAVE` (`C9`): `mov rsp, rbp; pop rbp`.
#[inline]
pub const fn leave() -> Inst {
    plain(Op::one(0xc9))
}
