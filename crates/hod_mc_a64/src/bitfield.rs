//! Shifts, bitfield moves and their aliases, EXTR, and the one-source bit
//! operations (`spec/mc-a64.md` "Shifts and bitfields", "Bit operations").

use crate::arith::dp2;
use crate::reg::{RegZr, Size};

const SBFM: u32 = 0;
const BFM: u32 = 1;
const UBFM: u32 = 2;

#[inline]
const fn bitfield(sz: Size, opc: u32, rd: RegZr, rn: RegZr, immr: u32, imms: u32) -> u32 {
    sz.sf()
        | opc << 29
        | 0x1300_0000
        | sz.bit() << 22
        | (immr & 0x3f) << 16
        | (imms & 0x3f) << 10
        | rn.code() << 5
        | rd.code()
}

/// A bitfield given as lowest bit and width lies inside the operand.
#[inline]
const fn field_ok(sz: Size, lsb: u32, width: u32) -> bool {
    lsb < sz.bits() && width >= 1 && width <= sz.bits() - lsb
}

/// `immr` for an "insert at `lsb`" alias: `-lsb` modulo the operand size.
#[inline]
const fn insert_immr(sz: Size, lsb: u32) -> u32 {
    (sz.bits() - lsb) % sz.bits()
}

checked! {
    /// `UBFM <Rd>, <Rn>, #immr, #imms`: UBFM.
    ///
    /// Precondition: `immr < sz.bits()` and `imms < sz.bits()`.
    pub fn ubfm / try_ubfm(sz: Size, rd: RegZr, rn: RegZr, immr: u32, imms: u32)
    requires immr < sz.bits() && imms < sz.bits();
    { bitfield(sz, UBFM, rd, rn, immr, imms) }

    /// `SBFM <Rd>, <Rn>, #immr, #imms`: SBFM.
    ///
    /// Precondition: `immr < sz.bits()` and `imms < sz.bits()`.
    pub fn sbfm / try_sbfm(sz: Size, rd: RegZr, rn: RegZr, immr: u32, imms: u32)
    requires immr < sz.bits() && imms < sz.bits();
    { bitfield(sz, SBFM, rd, rn, immr, imms) }

    /// `BFM <Rd>, <Rn>, #immr, #imms`: BFM.
    ///
    /// Precondition: `immr < sz.bits()` and `imms < sz.bits()`.
    pub fn bfm / try_bfm(sz: Size, rd: RegZr, rn: RegZr, immr: u32, imms: u32)
    requires immr < sz.bits() && imms < sz.bits();
    { bitfield(sz, BFM, rd, rn, immr, imms) }

    /// `LSL <Rd>, <Rn>, #amount`: LSL (immediate), an alias of UBFM.
    ///
    /// Precondition: `amount < sz.bits()`.
    pub fn lsl_imm / try_lsl_imm(sz: Size, rd: RegZr, rn: RegZr, amount: u32)
    requires amount < sz.bits();
    { bitfield(sz, UBFM, rd, rn, insert_immr(sz, amount), sz.bits() - 1 - amount) }

    /// `LSR <Rd>, <Rn>, #amount`: LSR (immediate), an alias of UBFM.
    ///
    /// Precondition: `amount < sz.bits()`.
    pub fn lsr_imm / try_lsr_imm(sz: Size, rd: RegZr, rn: RegZr, amount: u32)
    requires amount < sz.bits();
    { bitfield(sz, UBFM, rd, rn, amount, sz.bits() - 1) }

    /// `ASR <Rd>, <Rn>, #amount`: ASR (immediate), an alias of SBFM.
    ///
    /// Precondition: `amount < sz.bits()`.
    pub fn asr_imm / try_asr_imm(sz: Size, rd: RegZr, rn: RegZr, amount: u32)
    requires amount < sz.bits();
    { bitfield(sz, SBFM, rd, rn, amount, sz.bits() - 1) }

    /// `UBFX <Rd>, <Rn>, #lsb, #width`: UBFX, an alias of UBFM. Extracts
    /// `width` bits of `rn` starting at `lsb`, zero-extended.
    ///
    /// Precondition: `lsb < sz.bits()` and `1 <= width <= sz.bits() - lsb`.
    pub fn ubfx / try_ubfx(sz: Size, rd: RegZr, rn: RegZr, lsb: u32, width: u32)
    requires field_ok(sz, lsb, width);
    { bitfield(sz, UBFM, rd, rn, lsb, lsb + width - 1) }

    /// `SBFX <Rd>, <Rn>, #lsb, #width`: SBFX, an alias of SBFM. Extracts
    /// `width` bits of `rn` starting at `lsb`, sign-extended.
    ///
    /// Precondition: `lsb < sz.bits()` and `1 <= width <= sz.bits() - lsb`.
    pub fn sbfx / try_sbfx(sz: Size, rd: RegZr, rn: RegZr, lsb: u32, width: u32)
    requires field_ok(sz, lsb, width);
    { bitfield(sz, SBFM, rd, rn, lsb, lsb + width - 1) }

    /// `BFXIL <Rd>, <Rn>, #lsb, #width`: BFXIL, an alias of BFM. Copies
    /// `width` bits of `rn` starting at `lsb` into the low bits of `rd`.
    ///
    /// Precondition: `lsb < sz.bits()` and `1 <= width <= sz.bits() - lsb`.
    pub fn bfxil / try_bfxil(sz: Size, rd: RegZr, rn: RegZr, lsb: u32, width: u32)
    requires field_ok(sz, lsb, width);
    { bitfield(sz, BFM, rd, rn, lsb, lsb + width - 1) }

    /// `UBFIZ <Rd>, <Rn>, #lsb, #width`: UBFIZ, an alias of UBFM. Places
    /// the low `width` bits of `rn` at `lsb`, zeros elsewhere.
    ///
    /// Precondition: `lsb < sz.bits()` and `1 <= width <= sz.bits() - lsb`.
    pub fn ubfiz / try_ubfiz(sz: Size, rd: RegZr, rn: RegZr, lsb: u32, width: u32)
    requires field_ok(sz, lsb, width);
    { bitfield(sz, UBFM, rd, rn, insert_immr(sz, lsb), width - 1) }

    /// `SBFIZ <Rd>, <Rn>, #lsb, #width`: SBFIZ, an alias of SBFM. Places
    /// the low `width` bits of `rn` at `lsb`, sign-extended above, zeros
    /// below.
    ///
    /// Precondition: `lsb < sz.bits()` and `1 <= width <= sz.bits() - lsb`.
    pub fn sbfiz / try_sbfiz(sz: Size, rd: RegZr, rn: RegZr, lsb: u32, width: u32)
    requires field_ok(sz, lsb, width);
    { bitfield(sz, SBFM, rd, rn, insert_immr(sz, lsb), width - 1) }

    /// `BFI <Rd>, <Rn>, #lsb, #width`: BFI, an alias of BFM. Inserts the
    /// low `width` bits of `rn` into `rd` at `lsb`.
    ///
    /// Precondition: `lsb < sz.bits()` and `1 <= width <= sz.bits() - lsb`.
    pub fn bfi / try_bfi(sz: Size, rd: RegZr, rn: RegZr, lsb: u32, width: u32)
    requires field_ok(sz, lsb, width);
    { bitfield(sz, BFM, rd, rn, insert_immr(sz, lsb), width - 1) }

    /// `EXTR <Rd>, <Rn>, <Rm>, #lsb`: EXTR. Extracts an operand-sized field
    /// starting at bit `lsb` of the concatenation `rn:rm`.
    ///
    /// Precondition: `lsb < sz.bits()`.
    pub fn extr / try_extr(sz: Size, rd: RegZr, rn: RegZr, rm: RegZr, lsb: u32)
    requires lsb < sz.bits();
    {
        sz.sf()
            | 0x1380_0000
            | sz.bit() << 22
            | rm.code() << 16
            | lsb << 10
            | rn.code() << 5
            | rd.code()
    }

    /// `ROR <Rd>, <Rs>, #amount`: ROR (immediate), an alias of EXTR with
    /// both sources equal.
    ///
    /// Precondition: `amount < sz.bits()`.
    pub fn ror_imm / try_ror_imm(sz: Size, rd: RegZr, rs: RegZr, amount: u32)
    requires amount < sz.bits();
    { extr(sz, rd, rs, rs, amount) }
}

/// `SXTB <Rd>, <Wn>`: SXTB, an alias of SBFM.
#[inline]
pub const fn sxtb(sz: Size, rd: RegZr, rn: RegZr) -> u32 {
    bitfield(sz, SBFM, rd, rn, 0, 7)
}

/// `SXTH <Rd>, <Wn>`: SXTH, an alias of SBFM.
#[inline]
pub const fn sxth(sz: Size, rd: RegZr, rn: RegZr) -> u32 {
    bitfield(sz, SBFM, rd, rn, 0, 15)
}

/// `SXTW <Xd>, <Wn>`: SXTW, an alias of SBFM (64-bit only).
#[inline]
pub const fn sxtw(rd: RegZr, rn: RegZr) -> u32 {
    bitfield(Size::X, SBFM, rd, rn, 0, 31)
}

/// `UXTB <Wd>, <Wn>`: UXTB, an alias of UBFM (32-bit only; the upper half
/// of the X register is cleared as by every 32-bit operation).
#[inline]
pub const fn uxtb(rd: RegZr, rn: RegZr) -> u32 {
    bitfield(Size::W, UBFM, rd, rn, 0, 7)
}

/// `UXTH <Wd>, <Wn>`: UXTH, an alias of UBFM (32-bit only).
#[inline]
pub const fn uxth(rd: RegZr, rn: RegZr) -> u32 {
    bitfield(Size::W, UBFM, rd, rn, 0, 15)
}

/// `LSLV <Rd>, <Rn>, <Rm>`: LSLV (`LSL` by a register amount).
#[inline]
pub const fn lslv(sz: Size, rd: RegZr, rn: RegZr, rm: RegZr) -> u32 {
    dp2(sz, 0b00_1000, rd, rn, rm)
}

/// `LSRV <Rd>, <Rn>, <Rm>`: LSRV (`LSR` by a register amount).
#[inline]
pub const fn lsrv(sz: Size, rd: RegZr, rn: RegZr, rm: RegZr) -> u32 {
    dp2(sz, 0b00_1001, rd, rn, rm)
}

/// `ASRV <Rd>, <Rn>, <Rm>`: ASRV (`ASR` by a register amount).
#[inline]
pub const fn asrv(sz: Size, rd: RegZr, rn: RegZr, rm: RegZr) -> u32 {
    dp2(sz, 0b00_1010, rd, rn, rm)
}

/// `RORV <Rd>, <Rn>, <Rm>`: RORV (`ROR` by a register amount).
#[inline]
pub const fn rorv(sz: Size, rd: RegZr, rn: RegZr, rm: RegZr) -> u32 {
    dp2(sz, 0b00_1011, rd, rn, rm)
}

/// Data-processing (1 source).
#[inline]
const fn dp1(sz: Size, opcode: u32, rd: RegZr, rn: RegZr) -> u32 {
    sz.sf() | 0x5ac0_0000 | opcode << 10 | rn.code() << 5 | rd.code()
}

/// `RBIT <Rd>, <Rn>`: RBIT (reverse the bits).
#[inline]
pub const fn rbit(sz: Size, rd: RegZr, rn: RegZr) -> u32 {
    dp1(sz, 0, rd, rn)
}

/// `REV16 <Rd>, <Rn>`: REV16 (reverse the bytes of each 16-bit half-word).
#[inline]
pub const fn rev16(sz: Size, rd: RegZr, rn: RegZr) -> u32 {
    dp1(sz, 1, rd, rn)
}

/// `REV32 <Xd>, <Xn>`: REV32 (reverse the bytes of each 32-bit word;
/// 64-bit only).
#[inline]
pub const fn rev32(rd: RegZr, rn: RegZr) -> u32 {
    dp1(Size::X, 2, rd, rn)
}

/// `REV <Rd>, <Rn>`: REV (reverse the bytes of the whole register).
#[inline]
pub const fn rev(sz: Size, rd: RegZr, rn: RegZr) -> u32 {
    dp1(sz, 2 + sz.bit(), rd, rn)
}

/// `CLZ <Rd>, <Rn>`: CLZ (count leading zeros).
#[inline]
pub const fn clz(sz: Size, rd: RegZr, rn: RegZr) -> u32 {
    dp1(sz, 4, rd, rn)
}

/// `CLS <Rd>, <Rn>`: CLS (count leading sign bits).
#[inline]
pub const fn cls(sz: Size, rd: RegZr, rn: RegZr) -> u32 {
    dp1(sz, 5, rd, rn)
}
