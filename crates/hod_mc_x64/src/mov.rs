//! Moves, zero and sign extensions, the accumulator extensions and `lea`
//! (`spec/mc-x64.md` "Moves and extensions").

use crate::alu::{imm_len, not_byte};
use crate::inst::{Inst, Op, Rm, digit_m, digit_r, imm, modrm, plain, plus_r, rmem, rr};
use crate::reg::{Mem, Reg, Size};

/// `MOV r/m, r` with a register destination (`88`/`89 /r`).
#[inline]
pub const fn mov_rr(size: Size, dst: Reg, src: Reg) -> Inst {
    rr(Op::one(0x88).sized(size, 1), size, src, dst)
}

/// `MOV r, r/m` with a memory source: a load (`8A`/`8B /r`).
#[inline]
pub const fn mov_rm(size: Size, dst: Reg, src: Mem) -> Inst {
    rmem(Op::one(0x8a).sized(size, 1), size, dst, src)
}

/// `MOV r/m, r` with a memory destination: a store (`88`/`89 /r`).
#[inline]
pub const fn mov_mr(size: Size, dst: Mem, src: Reg) -> Inst {
    rmem(Op::one(0x88).sized(size, 1), size, src, dst)
}

/// Whether `imm` is a `mov_ri` immediate: any 64-bit value for
/// [`Size::Q`], else as [`Size::fits_imm`].
#[inline]
pub const fn mov_imm_fits(size: Size, imm: i64) -> bool {
    matches!(size, Size::Q) || size.fits_imm(imm)
}

checked! {
    /// `MOV r, imm`: `B0+r ib`, `B8+r iw/id` for 8, 16 and 32 bits; for 64
    /// bits `C7 /0 id` (sign-extended) when the value is a 32-bit signed
    /// value, else `REX.W B8+r io` (`movabs`).
    pub fn mov_ri / try_mov_ri(size: Size, dst: Reg, value: i64)
    requires mov_imm_fits(size, value);
    {
        match size {
            Size::Q if value >= i32::MIN as i64 && value <= i32::MAX as i64 => {
                imm(digit_r(Op::one(0xc7).sized(size, 0), 0, size, dst), value, 4)
            }
            Size::Q => imm(plus_r(Op::one(0xb8).sized(size, 0), dst, false), value, 8),
            Size::B => imm(plus_r(Op::one(0xb0), dst, true), value, 1),
            _ => imm(plus_r(Op::one(0xb8).sized(size, 0), dst, false), value, imm_len(size)),
        }
    }

    /// `MOV m, imm` (`C6`/`C7 /0`): for 64 bits a sign-extended `imm32`.
    pub fn mov_mi / try_mov_mi(size: Size, dst: Mem, value: i64)
    requires size.fits_imm(value);
    { imm(digit_m(Op::one(0xc6).sized(size, 1), 0, dst), value, imm_len(size)) }
}

/// Whether `src` is a source size of `movzx`/`movsx` into `dst`: 8 or 16
/// bits, narrower than the destination.
#[inline]
pub const fn extends(dst: Size, src: Size) -> bool {
    matches!((dst, src), (Size::W | Size::D | Size::Q, Size::B) | (Size::D | Size::Q, Size::W))
}

/// `movzx`/`movsx`: `0F B6`/`B7` or `0F BE`/`BF`, by source size.
#[inline]
const fn extend(opcode: u8, dst_size: Size, dst: Reg, src_size: Size, src: Rm) -> Inst {
    let op = Op::two(if matches!(src_size, Size::B) { opcode } else { opcode | 1 });
    modrm(op.sized(dst_size, 0), dst.code(), false, src, matches!(src_size, Size::B))
}

checked! {
    /// `MOVZX r, r/m` with a register source (`0F B6`/`B7 /r`).
    pub fn movzx_rr / try_movzx_rr(dst_size: Size, dst: Reg, src_size: Size, src: Reg)
    requires extends(dst_size, src_size);
    { extend(0xb6, dst_size, dst, src_size, Rm::Reg(src)) }

    /// `MOVZX r, r/m` with a memory source.
    pub fn movzx_rm / try_movzx_rm(dst_size: Size, dst: Reg, src_size: Size, src: Mem)
    requires extends(dst_size, src_size);
    { extend(0xb6, dst_size, dst, src_size, Rm::Mem(src)) }

    /// `MOVSX r, r/m` with a register source (`0F BE`/`BF /r`).
    pub fn movsx_rr / try_movsx_rr(dst_size: Size, dst: Reg, src_size: Size, src: Reg)
    requires extends(dst_size, src_size);
    { extend(0xbe, dst_size, dst, src_size, Rm::Reg(src)) }

    /// `MOVSX r, r/m` with a memory source.
    pub fn movsx_rm / try_movsx_rm(dst_size: Size, dst: Reg, src_size: Size, src: Mem)
    requires extends(dst_size, src_size);
    { extend(0xbe, dst_size, dst, src_size, Rm::Mem(src)) }
}

/// `MOVSXD r64, r/m32` with a register source (`REX.W 63 /r`).
#[inline]
pub const fn movsxd_rr(dst: Reg, src: Reg) -> Inst {
    rr(Op::one(0x63).sized(Size::Q, 0), Size::Q, dst, src)
}

/// `MOVSXD r64, r/m32` with a memory source.
#[inline]
pub const fn movsxd_rm(dst: Reg, src: Mem) -> Inst {
    rmem(Op::one(0x63).sized(Size::Q, 0), Size::Q, dst, src)
}

/// `CBW`: `ax = sext(al)` (`66 98`).
#[inline]
pub const fn cbw() -> Inst {
    plain(Op::one(0x98).sized(Size::W, 0))
}

/// `CWDE`: `eax = sext(ax)` (`98`).
#[inline]
pub const fn cwde() -> Inst {
    plain(Op::one(0x98))
}

/// `CDQE`: `rax = sext(eax)` (`REX.W 98`).
#[inline]
pub const fn cdqe() -> Inst {
    plain(Op::one(0x98).sized(Size::Q, 0))
}

/// `CWD`: `dx:ax = sext(ax)` (`66 99`).
#[inline]
pub const fn cwd() -> Inst {
    plain(Op::one(0x99).sized(Size::W, 0))
}

/// `CDQ`: `edx:eax = sext(eax)` (`99`).
#[inline]
pub const fn cdq() -> Inst {
    plain(Op::one(0x99))
}

/// `CQO`: `rdx:rax = sext(rax)` (`REX.W 99`).
#[inline]
pub const fn cqo() -> Inst {
    plain(Op::one(0x99).sized(Size::Q, 0))
}

checked! {
    /// `LEA r, m` (`8D /r`): the address, truncated to `size`.
    pub fn lea / try_lea(size: Size, dst: Reg, src: Mem)
    requires not_byte(size);
    { rmem(Op::one(0x8d).sized(size, 0), size, dst, src) }
}
