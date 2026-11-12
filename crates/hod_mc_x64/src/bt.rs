//! Bit tests (`spec/mc-x64.md` "Bit tests").
//!
//! Each copies the selected bit into CF; `bts`, `btr` and `btc` then set,
//! clear or complement it. With an immediate the bit index is taken modulo
//! the operand size. With a register index and a memory operand the index
//! is a signed bit offset from the operand's address, not reduced: the
//! instruction may touch memory outside the operand.

use crate::alu::not_byte;
use crate::inst::{Inst, Op, digit_m, digit_r, imm, rmem, rr};
use crate::reg::{Mem, Reg, Size};

/// `op r/m, r` (`0F opcode /r`), register destination.
#[inline]
const fn bit_rr(opcode: u8, size: Size, dst: Reg, bit: Reg) -> Inst {
    rr(Op::two(opcode).sized(size, 0), size, bit, dst)
}

/// `op m, r` (`0F opcode /r`); lockable except `bt`.
#[inline]
const fn bit_mr(opcode: u8, size: Size, dst: Mem, bit: Reg) -> Inst {
    let inst = rmem(Op::two(opcode).sized(size, 0), size, bit, dst);
    if opcode == 0xa3 { inst } else { inst.lockable() }
}

/// `op r/m, imm8` (`0F BA /digit ib`), register destination.
#[inline]
const fn bit_ri(digit: u8, size: Size, dst: Reg, bit: u8) -> Inst {
    imm(digit_r(Op::two(0xba).sized(size, 0), digit, size, dst), bit as i64, 1)
}

/// `op m, imm8` (`0F BA /digit ib`); lockable except `bt` (`/4`).
#[inline]
const fn bit_mi(digit: u8, size: Size, dst: Mem, bit: u8) -> Inst {
    let inst = imm(digit_m(Op::two(0xba).sized(size, 0), digit, dst), bit as i64, 1);
    if digit == 4 { inst } else { inst.lockable() }
}

family! {
    (size: Size, dst: Reg, bit: Reg) requires (not_byte(size)) => bit_rr;
    /// `BT r, r` (`0F A3 /r`): CF = the bit of `dst` that `bit` selects.
    pub fn bt_rr / try_bt_rr = (0xa3);
    /// `BTS r, r` (`0F AB /r`): and set it.
    pub fn bts_rr / try_bts_rr = (0xab);
    /// `BTR r, r` (`0F B3 /r`): and clear it.
    pub fn btr_rr / try_btr_rr = (0xb3);
    /// `BTC r, r` (`0F BB /r`): and complement it.
    pub fn btc_rr / try_btc_rr = (0xbb);
}

family! {
    (size: Size, dst: Mem, bit: Reg) requires (not_byte(size)) => bit_mr;
    /// `BT m, r` (`0F A3 /r`): a bit-string access. Not lockable.
    pub fn bt_mr / try_bt_mr = (0xa3);
    /// `BTS m, r` (`0F AB /r`). Lockable.
    pub fn bts_mr / try_bts_mr = (0xab);
    /// `BTR m, r` (`0F B3 /r`). Lockable.
    pub fn btr_mr / try_btr_mr = (0xb3);
    /// `BTC m, r` (`0F BB /r`). Lockable.
    pub fn btc_mr / try_btc_mr = (0xbb);
}

family! {
    (size: Size, dst: Reg, bit: u8) requires (not_byte(size)) => bit_ri;
    /// `BT r, imm8` (`0F BA /4 ib`).
    pub fn bt_ri / try_bt_ri = (4);
    /// `BTS r, imm8` (`0F BA /5 ib`).
    pub fn bts_ri / try_bts_ri = (5);
    /// `BTR r, imm8` (`0F BA /6 ib`).
    pub fn btr_ri / try_btr_ri = (6);
    /// `BTC r, imm8` (`0F BA /7 ib`).
    pub fn btc_ri / try_btc_ri = (7);
}

family! {
    (size: Size, dst: Mem, bit: u8) requires (not_byte(size)) => bit_mi;
    /// `BT m, imm8` (`0F BA /4 ib`). Not lockable.
    pub fn bt_mi / try_bt_mi = (4);
    /// `BTS m, imm8` (`0F BA /5 ib`). Lockable.
    pub fn bts_mi / try_bts_mi = (5);
    /// `BTR m, imm8` (`0F BA /6 ib`). Lockable.
    pub fn btr_mi / try_btr_mi = (6);
    /// `BTC m, imm8` (`0F BA /7 ib`). Lockable.
    pub fn btc_mi / try_btc_mi = (7);
}
