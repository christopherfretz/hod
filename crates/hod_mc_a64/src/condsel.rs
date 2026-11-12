//! Conditional select and conditional compare (`spec/mc-a64.md`
//! "Conditional").

use crate::reg::{Cond, Reg, RegZr, Size};

/// Conditional select. `op` is bit 30, `o2` is bit 10.
#[inline]
const fn cond_select(
    sz: Size,
    op: u32,
    o2: u32,
    rd: RegZr,
    rn: RegZr,
    rm: RegZr,
    cond: Cond,
) -> u32 {
    sz.sf()
        | op << 30
        | 0x1a80_0000
        | rm.code() << 16
        | cond.code() << 12
        | o2 << 10
        | rn.code() << 5
        | rd.code()
}

/// `CSEL <Rd>, <Rn>, <Rm>, <cond>`: CSEL (`rd = cond ? rn : rm`).
#[inline]
pub const fn csel(sz: Size, rd: RegZr, rn: RegZr, rm: RegZr, cond: Cond) -> u32 {
    cond_select(sz, 0, 0, rd, rn, rm, cond)
}

/// `CSINC <Rd>, <Rn>, <Rm>, <cond>`: CSINC (`rd = cond ? rn : rm + 1`).
#[inline]
pub const fn csinc(sz: Size, rd: RegZr, rn: RegZr, rm: RegZr, cond: Cond) -> u32 {
    cond_select(sz, 0, 1, rd, rn, rm, cond)
}

/// `CSINV <Rd>, <Rn>, <Rm>, <cond>`: CSINV (`rd = cond ? rn : !rm`).
#[inline]
pub const fn csinv(sz: Size, rd: RegZr, rn: RegZr, rm: RegZr, cond: Cond) -> u32 {
    cond_select(sz, 1, 0, rd, rn, rm, cond)
}

/// `CSNEG <Rd>, <Rn>, <Rm>, <cond>`: CSNEG (`rd = cond ? rn : -rm`).
#[inline]
pub const fn csneg(sz: Size, rd: RegZr, rn: RegZr, rm: RegZr, cond: Cond) -> u32 {
    cond_select(sz, 1, 1, rd, rn, rm, cond)
}

/// Conditional compare. `op` is bit 30 (0 = CCMN, 1 = CCMP); `imm` is bit
/// 11; `operand` is `Rm` or the 5-bit immediate.
#[inline]
const fn cond_compare(
    sz: Size,
    op: u32,
    imm: u32,
    rn: RegZr,
    operand: u32,
    nzcv: u32,
    cond: Cond,
) -> u32 {
    sz.sf()
        | op << 30
        | 0x3a40_0000
        | (operand & 0x1f) << 16
        | cond.code() << 12
        | imm << 11
        | rn.code() << 5
        | (nzcv & 0xf)
}

checked! {
    /// `CSET <Rd>, <cond>`: CSET (`rd = cond ? 1 : 0`), an alias of CSINC.
    ///
    /// Precondition: `cond` is not `Al` or `Nv`.
    pub fn cset / try_cset(sz: Size, rd: RegZr, cond: Cond)
    requires !cond.is_always();
    { cond_select(sz, 0, 1, rd, RegZr::ZR, RegZr::ZR, cond.invert()) }

    /// `CSETM <Rd>, <cond>`: CSETM (`rd = cond ? -1 : 0`), an alias of
    /// CSINV.
    ///
    /// Precondition: `cond` is not `Al` or `Nv`.
    pub fn csetm / try_csetm(sz: Size, rd: RegZr, cond: Cond)
    requires !cond.is_always();
    { cond_select(sz, 1, 0, rd, RegZr::ZR, RegZr::ZR, cond.invert()) }

    /// `CINC <Rd>, <Rn>, <cond>`: CINC (`rd = cond ? rn + 1 : rn`), an
    /// alias of CSINC. `rn` cannot be ZR: that encoding is CSET.
    ///
    /// Precondition: `cond` is not `Al` or `Nv`.
    pub fn cinc / try_cinc(sz: Size, rd: RegZr, rn: Reg, cond: Cond)
    requires !cond.is_always();
    { cond_select(sz, 0, 1, rd, rn.zr(), rn.zr(), cond.invert()) }

    /// `CINV <Rd>, <Rn>, <cond>`: CINV (`rd = cond ? !rn : rn`), an alias
    /// of CSINV. `rn` cannot be ZR: that encoding is CSETM.
    ///
    /// Precondition: `cond` is not `Al` or `Nv`.
    pub fn cinv / try_cinv(sz: Size, rd: RegZr, rn: Reg, cond: Cond)
    requires !cond.is_always();
    { cond_select(sz, 1, 0, rd, rn.zr(), rn.zr(), cond.invert()) }

    /// `CNEG <Rd>, <Rn>, <cond>`: CNEG (`rd = cond ? -rn : rn`), an alias
    /// of CSNEG.
    ///
    /// Precondition: `cond` is not `Al` or `Nv`.
    pub fn cneg / try_cneg(sz: Size, rd: RegZr, rn: RegZr, cond: Cond)
    requires !cond.is_always();
    { cond_select(sz, 1, 1, rd, rn, rn, cond.invert()) }

    /// `CCMP <Rn>, <Rm>, #nzcv, <cond>`: CCMP (register). Flags are those
    /// of `CMP rn, rm` if `cond` holds, else `nzcv`.
    ///
    /// Precondition: `nzcv < 16`.
    pub fn ccmp / try_ccmp(sz: Size, rn: RegZr, rm: RegZr, nzcv: u32, cond: Cond)
    requires nzcv < 16;
    { cond_compare(sz, 1, 0, rn, rm.code(), nzcv, cond) }

    /// `CCMN <Rn>, <Rm>, #nzcv, <cond>`: CCMN (register).
    ///
    /// Precondition: `nzcv < 16`.
    pub fn ccmn / try_ccmn(sz: Size, rn: RegZr, rm: RegZr, nzcv: u32, cond: Cond)
    requires nzcv < 16;
    { cond_compare(sz, 0, 0, rn, rm.code(), nzcv, cond) }

    /// `CCMP <Rn>, #imm, #nzcv, <cond>`: CCMP (immediate).
    ///
    /// Precondition: `imm < 32` and `nzcv < 16`.
    pub fn ccmp_imm / try_ccmp_imm(sz: Size, rn: RegZr, imm: u32, nzcv: u32, cond: Cond)
    requires imm < 32 && nzcv < 16;
    { cond_compare(sz, 1, 1, rn, imm, nzcv, cond) }

    /// `CCMN <Rn>, #imm, #nzcv, <cond>`: CCMN (immediate).
    ///
    /// Precondition: `imm < 32` and `nzcv < 16`.
    pub fn ccmn_imm / try_ccmn_imm(sz: Size, rn: RegZr, imm: u32, nzcv: u32, cond: Cond)
    requires imm < 32 && nzcv < 16;
    { cond_compare(sz, 0, 1, rn, imm, nzcv, cond) }
}
