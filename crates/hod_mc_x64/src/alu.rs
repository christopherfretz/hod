//! Integer arithmetic and logic: the eight two-operand ALU instructions,
//! `test`, `inc`/`dec`, `not`/`neg`, multiply and divide (`spec/mc-x64.md`
//! "Arithmetic and logic", "Multiply and divide").

use crate::inst::{Inst, Op, digit_m, digit_r, imm, rmem, rr};
use crate::reg::{Mem, Reg, Size};

/// The `/digit` (and `opcode >> 3`) of each two-operand ALU instruction.
const ADD: u8 = 0;
const OR: u8 = 1;
const ADC: u8 = 2;
const SBB: u8 = 3;
const AND: u8 = 4;
const SUB: u8 = 5;
const XOR: u8 = 6;
const CMP: u8 = 7;

/// The immediate's width in bytes for `size` (64-bit: a sign-extended
/// `imm32`).
#[inline]
pub(crate) const fn imm_len(size: Size) -> u8 {
    match size {
        Size::B => 1,
        Size::W => 2,
        Size::D | Size::Q => 4,
    }
}

/// Lockable unless the instruction is CMP, which writes no memory.
#[inline]
const fn lockable_unless_cmp(n: u8, inst: Inst) -> Inst {
    if n == CMP { inst } else { inst.lockable() }
}

/// `op r/m, reg` with a register destination (`00+8n /r`, `01+8n /r`).
#[inline]
const fn alu_rr(n: u8, size: Size, dst: Reg, src: Reg) -> Inst {
    rr(Op::one(n << 3).sized(size, 1), size, src, dst)
}

/// `op reg, m` (`02+8n /r`, `03+8n /r`).
#[inline]
const fn alu_rm(n: u8, size: Size, dst: Reg, src: Mem) -> Inst {
    rmem(Op::one(n << 3 | 2).sized(size, 1), size, dst, src)
}

/// `op m, reg` (`00+8n /r`, `01+8n /r`).
#[inline]
const fn alu_mr(n: u8, size: Size, dst: Mem, src: Reg) -> Inst {
    lockable_unless_cmp(n, rmem(Op::one(n << 3).sized(size, 1), size, src, dst))
}

/// `op reg, imm`: `83 /n ib` when the immediate is a sign-extended byte
/// (not for 8-bit operands), else the accumulator form `04+8n`/`05+8n`
/// for `al`/`ax`/`eax`/`rax`, else `80 /n ib` or `81 /n iw/id`.
#[inline]
const fn alu_ri(n: u8, size: Size, dst: Reg, value: i64) -> Inst {
    if !matches!(size, Size::B) && size.is_imm8(value) {
        imm(digit_r(Op::one(0x83).sized(size, 0), n, size, dst), value, 1)
    } else if matches!(dst, Reg::Rax) {
        imm(crate::inst::plain(Op::one(n << 3 | 4).sized(size, 1)), value, imm_len(size))
    } else {
        imm(digit_r(Op::one(0x80).sized(size, 1), n, size, dst), value, imm_len(size))
    }
}

/// `op m, imm`: `83 /n ib` when the immediate is a sign-extended byte (not
/// for 8-bit operands), else `80 /n ib` or `81 /n iw/id`.
#[inline]
const fn alu_mi(n: u8, size: Size, dst: Mem, value: i64) -> Inst {
    let inst = if !matches!(size, Size::B) && size.is_imm8(value) {
        imm(digit_m(Op::one(0x83).sized(size, 0), n, dst), value, 1)
    } else {
        imm(digit_m(Op::one(0x80).sized(size, 1), n, dst), value, imm_len(size))
    };
    lockable_unless_cmp(n, inst)
}

family! {
    (size: Size, dst: Reg, src: Reg) => alu_rr;
    /// `ADD r/m, r` with a register destination (`00`/`01 /r`).
    pub fn add_rr = (ADD);
    /// `OR r/m, r` (`08`/`09 /r`).
    pub fn or_rr = (OR);
    /// `ADC r/m, r` (`10`/`11 /r`).
    pub fn adc_rr = (ADC);
    /// `SBB r/m, r` (`18`/`19 /r`).
    pub fn sbb_rr = (SBB);
    /// `AND r/m, r` (`20`/`21 /r`).
    pub fn and_rr = (AND);
    /// `SUB r/m, r` (`28`/`29 /r`).
    pub fn sub_rr = (SUB);
    /// `XOR r/m, r` (`30`/`31 /r`).
    pub fn xor_rr = (XOR);
    /// `CMP r/m, r` (`38`/`39 /r`): flags of `dst - src`.
    pub fn cmp_rr = (CMP);
}

family! {
    (size: Size, dst: Reg, src: Mem) => alu_rm;
    /// `ADD r, r/m` with a memory source (`02`/`03 /r`).
    pub fn add_rm = (ADD);
    /// `OR r, r/m` (`0A`/`0B /r`).
    pub fn or_rm = (OR);
    /// `ADC r, r/m` (`12`/`13 /r`).
    pub fn adc_rm = (ADC);
    /// `SBB r, r/m` (`1A`/`1B /r`).
    pub fn sbb_rm = (SBB);
    /// `AND r, r/m` (`22`/`23 /r`).
    pub fn and_rm = (AND);
    /// `SUB r, r/m` (`2A`/`2B /r`).
    pub fn sub_rm = (SUB);
    /// `XOR r, r/m` (`32`/`33 /r`).
    pub fn xor_rm = (XOR);
    /// `CMP r, r/m` (`3A`/`3B /r`): flags of `dst - [src]`.
    pub fn cmp_rm = (CMP);
}

family! {
    (size: Size, dst: Mem, src: Reg) => alu_mr;
    /// `ADD r/m, r` with a memory destination (`00`/`01 /r`). Lockable.
    pub fn add_mr = (ADD);
    /// `OR r/m, r` (`08`/`09 /r`). Lockable.
    pub fn or_mr = (OR);
    /// `ADC r/m, r` (`10`/`11 /r`). Lockable.
    pub fn adc_mr = (ADC);
    /// `SBB r/m, r` (`18`/`19 /r`). Lockable.
    pub fn sbb_mr = (SBB);
    /// `AND r/m, r` (`20`/`21 /r`). Lockable.
    pub fn and_mr = (AND);
    /// `SUB r/m, r` (`28`/`29 /r`). Lockable.
    pub fn sub_mr = (SUB);
    /// `XOR r/m, r` (`30`/`31 /r`). Lockable.
    pub fn xor_mr = (XOR);
    /// `CMP r/m, r` (`38`/`39 /r`): flags of `[dst] - src`.
    pub fn cmp_mr = (CMP);
}

family! {
    (size: Size, dst: Reg, imm: i64) requires (size.fits_imm(imm)) => alu_ri;
    /// `ADD r, imm` (`83 /0 ib`, `05`, `80`/`81 /0`).
    pub fn add_ri / try_add_ri = (ADD);
    /// `OR r, imm` (`83 /1 ib`, `0D`, `80`/`81 /1`).
    pub fn or_ri / try_or_ri = (OR);
    /// `ADC r, imm` (`83 /2 ib`, `15`, `80`/`81 /2`).
    pub fn adc_ri / try_adc_ri = (ADC);
    /// `SBB r, imm` (`83 /3 ib`, `1D`, `80`/`81 /3`).
    pub fn sbb_ri / try_sbb_ri = (SBB);
    /// `AND r, imm` (`83 /4 ib`, `25`, `80`/`81 /4`).
    pub fn and_ri / try_and_ri = (AND);
    /// `SUB r, imm` (`83 /5 ib`, `2D`, `80`/`81 /5`).
    pub fn sub_ri / try_sub_ri = (SUB);
    /// `XOR r, imm` (`83 /6 ib`, `35`, `80`/`81 /6`).
    pub fn xor_ri / try_xor_ri = (XOR);
    /// `CMP r, imm` (`83 /7 ib`, `3D`, `80`/`81 /7`).
    pub fn cmp_ri / try_cmp_ri = (CMP);
}

family! {
    (size: Size, dst: Mem, imm: i64) requires (size.fits_imm(imm)) => alu_mi;
    /// `ADD m, imm` (`83 /0 ib`, `80`/`81 /0`). Lockable.
    pub fn add_mi / try_add_mi = (ADD);
    /// `OR m, imm`. Lockable.
    pub fn or_mi / try_or_mi = (OR);
    /// `ADC m, imm`. Lockable.
    pub fn adc_mi / try_adc_mi = (ADC);
    /// `SBB m, imm`. Lockable.
    pub fn sbb_mi / try_sbb_mi = (SBB);
    /// `AND m, imm`. Lockable.
    pub fn and_mi / try_and_mi = (AND);
    /// `SUB m, imm`. Lockable.
    pub fn sub_mi / try_sub_mi = (SUB);
    /// `XOR m, imm`. Lockable.
    pub fn xor_mi / try_xor_mi = (XOR);
    /// `CMP m, imm`.
    pub fn cmp_mi / try_cmp_mi = (CMP);
}

/// `TEST r/m, r` with a register first operand (`84`/`85 /r`): flags of
/// `a & b`.
#[inline]
pub const fn test_rr(size: Size, a: Reg, b: Reg) -> Inst {
    rr(Op::one(0x84).sized(size, 1), size, b, a)
}

/// `TEST r/m, r` with a memory first operand (`84`/`85 /r`).
#[inline]
pub const fn test_mr(size: Size, a: Mem, b: Reg) -> Inst {
    rmem(Op::one(0x84).sized(size, 1), size, b, a)
}

checked! {
    /// `TEST r, imm`: `A8`/`A9` for the accumulator, else `F6`/`F7 /0`.
    /// There is no sign-extended byte form.
    pub fn test_ri / try_test_ri(size: Size, a: Reg, value: i64)
    requires size.fits_imm(value);
    {
        let n = imm_len(size);
        if matches!(a, Reg::Rax) {
            imm(crate::inst::plain(Op::one(0xa8).sized(size, 1)), value, n)
        } else {
            imm(digit_r(Op::one(0xf6).sized(size, 1), 0, size, a), value, n)
        }
    }

    /// `TEST m, imm` (`F6`/`F7 /0`).
    pub fn test_mi / try_test_mi(size: Size, a: Mem, value: i64)
    requires size.fits_imm(value);
    { imm(digit_m(Op::one(0xf6).sized(size, 1), 0, a), value, imm_len(size)) }
}

/// A `/digit` instruction of the `F6`/`F7` or `FE`/`FF` group on a
/// register.
#[inline]
const fn group_r(opcode: u8, digit: u8, size: Size, r: Reg) -> Inst {
    digit_r(Op::one(opcode).sized(size, 1), digit, size, r)
}

/// The same on memory.
#[inline]
const fn group_m(opcode: u8, digit: u8, size: Size, m: Mem) -> Inst {
    digit_m(Op::one(opcode).sized(size, 1), digit, m)
}

/// The same on memory, lockable.
#[inline]
const fn group_m_lockable(opcode: u8, digit: u8, size: Size, m: Mem) -> Inst {
    group_m(opcode, digit, size, m).lockable()
}

family! {
    (size: Size, r: Reg) => group_r;
    /// `INC r` (`FE`/`FF /0`).
    pub fn inc_r = (0xfe, 0);
    /// `DEC r` (`FE`/`FF /1`).
    pub fn dec_r = (0xfe, 1);
    /// `NOT r` (`F6`/`F7 /2`).
    pub fn not_r = (0xf6, 2);
    /// `NEG r` (`F6`/`F7 /3`).
    pub fn neg_r = (0xf6, 3);
    /// `MUL r` (`F6`/`F7 /4`): unsigned `rdx:rax = rax * r` (by size).
    pub fn mul_r = (0xf6, 4);
    /// `IMUL r` (`F6`/`F7 /5`): signed `rdx:rax = rax * r` (by size).
    pub fn imul_r = (0xf6, 5);
    /// `DIV r` (`F6`/`F7 /6`): unsigned `rdx:rax / r` (by size).
    pub fn div_r = (0xf6, 6);
    /// `IDIV r` (`F6`/`F7 /7`): signed `rdx:rax / r` (by size).
    pub fn idiv_r = (0xf6, 7);
}

family! {
    (size: Size, m: Mem) => group_m_lockable;
    /// `INC m` (`FE`/`FF /0`). Lockable.
    pub fn inc_m = (0xfe, 0);
    /// `DEC m` (`FE`/`FF /1`). Lockable.
    pub fn dec_m = (0xfe, 1);
    /// `NOT m` (`F6`/`F7 /2`). Lockable.
    pub fn not_m = (0xf6, 2);
    /// `NEG m` (`F6`/`F7 /3`). Lockable.
    pub fn neg_m = (0xf6, 3);
}

family! {
    (size: Size, m: Mem) => group_m;
    /// `MUL m` (`F6`/`F7 /4`).
    pub fn mul_m = (0xf6, 4);
    /// `IMUL m` (`F6`/`F7 /5`).
    pub fn imul_m = (0xf6, 5);
    /// `DIV m` (`F6`/`F7 /6`).
    pub fn div_m = (0xf6, 6);
    /// `IDIV m` (`F6`/`F7 /7`).
    pub fn idiv_m = (0xf6, 7);
}

/// Whether `size` is 16, 32 or 64 bits: the sizes of the instructions with
/// no byte form.
#[inline]
pub(crate) const fn not_byte(size: Size) -> bool {
    !matches!(size, Size::B)
}

/// `IMUL r, r/m, imm`: `6B /r ib` for a sign-extended byte, else
/// `69 /r iw/id`.
#[inline]
const fn imul3(size: Size, dst: Reg, src: crate::inst::Rm, value: i64) -> Inst {
    let byte = size.is_imm8(value);
    let op = Op::one(if byte { 0x6b } else { 0x69 }).sized(size, 0);
    let inst = crate::inst::modrm(op, dst.code(), false, src, false);
    imm(inst, value, if byte { 1 } else { imm_len(size) })
}

checked! {
    /// `IMUL r, r/m` with a register source (`0F AF /r`): the low half of
    /// `dst * src`.
    pub fn imul_rr / try_imul_rr(size: Size, dst: Reg, src: Reg)
    requires not_byte(size);
    { rr(Op::two(0xaf).sized(size, 0), size, dst, src) }

    /// `IMUL r, r/m` with a memory source (`0F AF /r`).
    pub fn imul_rm / try_imul_rm(size: Size, dst: Reg, src: Mem)
    requires not_byte(size);
    { rmem(Op::two(0xaf).sized(size, 0), size, dst, src) }

    /// `IMUL r, r/m, imm` with a register source (`6B /r ib`, `69 /r`).
    pub fn imul_rri / try_imul_rri(size: Size, dst: Reg, src: Reg, value: i64)
    requires not_byte(size) && size.fits_imm(value);
    { imul3(size, dst, crate::inst::Rm::Reg(src), value) }

    /// `IMUL r, r/m, imm` with a memory source (`6B /r ib`, `69 /r`).
    pub fn imul_rmi / try_imul_rmi(size: Size, dst: Reg, src: Mem, value: i64)
    requires not_byte(size) && size.fits_imm(value);
    { imul3(size, dst, crate::inst::Rm::Mem(src), value) }
}
