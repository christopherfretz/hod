//! Bitwise logic with an immediate or a shifted register, and the register
//! move aliases built on it (`spec/mc-a64.md` "Logical").

use crate::imm::encode_bitmask;
use crate::reg::{RegSp, RegZr, Shift, Size};

const AND: u32 = 0;
const ORR: u32 = 1;
const EOR: u32 = 2;
const ANDS: u32 = 3;
const ZR: u32 = 31;

/// Logical (immediate). `imm` must be encodable; an unencodable one gives an
/// unspecified word.
#[inline]
const fn logical_imm(sz: Size, opc: u32, rd: u32, rn: u32, imm: u64) -> u32 {
    let field = match encode_bitmask(sz, imm) {
        Some(field) => field as u32,
        None => 0,
    };
    sz.sf() | opc << 29 | 0x1200_0000 | field << 10 | rn << 5 | rd
}

/// Logical (shifted register). `n` is the "invert `Rm`" bit.
#[inline]
const fn logical_shifted(
    sz: Size,
    opc: u32,
    n: u32,
    rd: u32,
    operands: (RegZr, RegZr),
    shift: Shift,
    amount: u32,
) -> u32 {
    let (rn, rm) = operands;
    sz.sf()
        | opc << 29
        | 0x0a00_0000
        | (shift as u32) << 22
        | n << 21
        | rm.code() << 16
        | (amount & 0x3f) << 10
        | rn.code() << 5
        | rd
}

checked! {
    /// `AND <Rd|SP>, <Rn>, #imm`: AND (immediate).
    ///
    /// Precondition: [`encode_bitmask`]`(sz, imm)` is `Some`.
    pub fn and_imm / try_and_imm(sz: Size, rd: RegSp, rn: RegZr, imm: u64)
    requires encode_bitmask(sz, imm).is_some();
    { logical_imm(sz, AND, rd.code(), rn.code(), imm) }

    /// `ORR <Rd|SP>, <Rn>, #imm`: ORR (immediate).
    ///
    /// Precondition: [`encode_bitmask`]`(sz, imm)` is `Some`.
    pub fn orr_imm / try_orr_imm(sz: Size, rd: RegSp, rn: RegZr, imm: u64)
    requires encode_bitmask(sz, imm).is_some();
    { logical_imm(sz, ORR, rd.code(), rn.code(), imm) }

    /// `EOR <Rd|SP>, <Rn>, #imm`: EOR (immediate).
    ///
    /// Precondition: [`encode_bitmask`]`(sz, imm)` is `Some`.
    pub fn eor_imm / try_eor_imm(sz: Size, rd: RegSp, rn: RegZr, imm: u64)
    requires encode_bitmask(sz, imm).is_some();
    { logical_imm(sz, EOR, rd.code(), rn.code(), imm) }

    /// `ANDS <Rd>, <Rn>, #imm`: ANDS (immediate).
    ///
    /// Precondition: [`encode_bitmask`]`(sz, imm)` is `Some`.
    pub fn ands_imm / try_ands_imm(sz: Size, rd: RegZr, rn: RegZr, imm: u64)
    requires encode_bitmask(sz, imm).is_some();
    { logical_imm(sz, ANDS, rd.code(), rn.code(), imm) }

    /// `TST <Rn>, #imm`: TST (immediate), an alias of ANDS (immediate).
    ///
    /// Precondition: [`encode_bitmask`]`(sz, imm)` is `Some`.
    pub fn tst_imm / try_tst_imm(sz: Size, rn: RegZr, imm: u64)
    requires encode_bitmask(sz, imm).is_some();
    { logical_imm(sz, ANDS, ZR, rn.code(), imm) }

    /// `MOV <Rd|SP>, #imm`: MOV (bitmask immediate), an alias of ORR
    /// (immediate) with `Rn` = ZR.
    ///
    /// Precondition: [`encode_bitmask`]`(sz, imm)` is `Some`.
    pub fn mov_bitmask / try_mov_bitmask(sz: Size, rd: RegSp, imm: u64)
    requires encode_bitmask(sz, imm).is_some();
    { logical_imm(sz, ORR, rd.code(), ZR, imm) }

    /// `AND <Rd>, <Rn>, <Rm>, <shift> #amount`: AND (shifted register).
    ///
    /// Precondition: `amount < sz.bits()`.
    pub fn and_shifted / try_and_shifted(
        sz: Size, rd: RegZr, rn: RegZr, rm: RegZr, shift: Shift, amount: u32
    )
    requires amount < sz.bits();
    { logical_shifted(sz, AND, 0, rd.code(), (rn, rm), shift, amount) }

    /// `BIC <Rd>, <Rn>, <Rm>, <shift> #amount`: BIC (shifted register).
    ///
    /// Precondition: `amount < sz.bits()`.
    pub fn bic_shifted / try_bic_shifted(
        sz: Size, rd: RegZr, rn: RegZr, rm: RegZr, shift: Shift, amount: u32
    )
    requires amount < sz.bits();
    { logical_shifted(sz, AND, 1, rd.code(), (rn, rm), shift, amount) }

    /// `ORR <Rd>, <Rn>, <Rm>, <shift> #amount`: ORR (shifted register).
    ///
    /// Precondition: `amount < sz.bits()`.
    pub fn orr_shifted / try_orr_shifted(
        sz: Size, rd: RegZr, rn: RegZr, rm: RegZr, shift: Shift, amount: u32
    )
    requires amount < sz.bits();
    { logical_shifted(sz, ORR, 0, rd.code(), (rn, rm), shift, amount) }

    /// `ORN <Rd>, <Rn>, <Rm>, <shift> #amount`: ORN (shifted register).
    ///
    /// Precondition: `amount < sz.bits()`.
    pub fn orn_shifted / try_orn_shifted(
        sz: Size, rd: RegZr, rn: RegZr, rm: RegZr, shift: Shift, amount: u32
    )
    requires amount < sz.bits();
    { logical_shifted(sz, ORR, 1, rd.code(), (rn, rm), shift, amount) }

    /// `EOR <Rd>, <Rn>, <Rm>, <shift> #amount`: EOR (shifted register).
    ///
    /// Precondition: `amount < sz.bits()`.
    pub fn eor_shifted / try_eor_shifted(
        sz: Size, rd: RegZr, rn: RegZr, rm: RegZr, shift: Shift, amount: u32
    )
    requires amount < sz.bits();
    { logical_shifted(sz, EOR, 0, rd.code(), (rn, rm), shift, amount) }

    /// `EON <Rd>, <Rn>, <Rm>, <shift> #amount`: EON (shifted register).
    ///
    /// Precondition: `amount < sz.bits()`.
    pub fn eon_shifted / try_eon_shifted(
        sz: Size, rd: RegZr, rn: RegZr, rm: RegZr, shift: Shift, amount: u32
    )
    requires amount < sz.bits();
    { logical_shifted(sz, EOR, 1, rd.code(), (rn, rm), shift, amount) }

    /// `ANDS <Rd>, <Rn>, <Rm>, <shift> #amount`: ANDS (shifted register).
    ///
    /// Precondition: `amount < sz.bits()`.
    pub fn ands_shifted / try_ands_shifted(
        sz: Size, rd: RegZr, rn: RegZr, rm: RegZr, shift: Shift, amount: u32
    )
    requires amount < sz.bits();
    { logical_shifted(sz, ANDS, 0, rd.code(), (rn, rm), shift, amount) }

    /// `BICS <Rd>, <Rn>, <Rm>, <shift> #amount`: BICS (shifted register).
    ///
    /// Precondition: `amount < sz.bits()`.
    pub fn bics_shifted / try_bics_shifted(
        sz: Size, rd: RegZr, rn: RegZr, rm: RegZr, shift: Shift, amount: u32
    )
    requires amount < sz.bits();
    { logical_shifted(sz, ANDS, 1, rd.code(), (rn, rm), shift, amount) }

    /// `MVN <Rd>, <Rm>, <shift> #amount`: MVN, an alias of ORN (shifted
    /// register) with `Rn` = ZR.
    ///
    /// Precondition: `amount < sz.bits()`.
    pub fn mvn_shifted / try_mvn_shifted(
        sz: Size, rd: RegZr, rm: RegZr, shift: Shift, amount: u32
    )
    requires amount < sz.bits();
    { logical_shifted(sz, ORR, 1, rd.code(), (RegZr::ZR, rm), shift, amount) }

    /// `TST <Rn>, <Rm>, <shift> #amount`: TST (shifted register), an alias
    /// of ANDS (shifted register) with `Rd` = ZR.
    ///
    /// Precondition: `amount < sz.bits()`.
    pub fn tst_shifted / try_tst_shifted(
        sz: Size, rn: RegZr, rm: RegZr, shift: Shift, amount: u32
    )
    requires amount < sz.bits();
    { logical_shifted(sz, ANDS, 0, ZR, (rn, rm), shift, amount) }
}

/// `AND <Rd>, <Rn>, <Rm>`: AND (shifted register) with no shift.
#[inline]
pub const fn and(sz: Size, rd: RegZr, rn: RegZr, rm: RegZr) -> u32 {
    logical_shifted(sz, AND, 0, rd.code(), (rn, rm), Shift::Lsl, 0)
}

/// `BIC <Rd>, <Rn>, <Rm>`: BIC (shifted register) with no shift.
#[inline]
pub const fn bic(sz: Size, rd: RegZr, rn: RegZr, rm: RegZr) -> u32 {
    logical_shifted(sz, AND, 1, rd.code(), (rn, rm), Shift::Lsl, 0)
}

/// `ORR <Rd>, <Rn>, <Rm>`: ORR (shifted register) with no shift.
#[inline]
pub const fn orr(sz: Size, rd: RegZr, rn: RegZr, rm: RegZr) -> u32 {
    logical_shifted(sz, ORR, 0, rd.code(), (rn, rm), Shift::Lsl, 0)
}

/// `ORN <Rd>, <Rn>, <Rm>`: ORN (shifted register) with no shift.
#[inline]
pub const fn orn(sz: Size, rd: RegZr, rn: RegZr, rm: RegZr) -> u32 {
    logical_shifted(sz, ORR, 1, rd.code(), (rn, rm), Shift::Lsl, 0)
}

/// `EOR <Rd>, <Rn>, <Rm>`: EOR (shifted register) with no shift.
#[inline]
pub const fn eor(sz: Size, rd: RegZr, rn: RegZr, rm: RegZr) -> u32 {
    logical_shifted(sz, EOR, 0, rd.code(), (rn, rm), Shift::Lsl, 0)
}

/// `EON <Rd>, <Rn>, <Rm>`: EON (shifted register) with no shift.
#[inline]
pub const fn eon(sz: Size, rd: RegZr, rn: RegZr, rm: RegZr) -> u32 {
    logical_shifted(sz, EOR, 1, rd.code(), (rn, rm), Shift::Lsl, 0)
}

/// `ANDS <Rd>, <Rn>, <Rm>`: ANDS (shifted register) with no shift.
#[inline]
pub const fn ands(sz: Size, rd: RegZr, rn: RegZr, rm: RegZr) -> u32 {
    logical_shifted(sz, ANDS, 0, rd.code(), (rn, rm), Shift::Lsl, 0)
}

/// `BICS <Rd>, <Rn>, <Rm>`: BICS (shifted register) with no shift.
#[inline]
pub const fn bics(sz: Size, rd: RegZr, rn: RegZr, rm: RegZr) -> u32 {
    logical_shifted(sz, ANDS, 1, rd.code(), (rn, rm), Shift::Lsl, 0)
}

/// `MOV <Rd>, <Rm>`: MOV (register), an alias of ORR (shifted register)
/// with `Rn` = ZR. Neither operand can be SP: use
/// [`mov_sp`](crate::arith::mov_sp) for that.
#[inline]
pub const fn mov(sz: Size, rd: RegZr, rm: RegZr) -> u32 {
    logical_shifted(sz, ORR, 0, rd.code(), (RegZr::ZR, rm), Shift::Lsl, 0)
}

/// `MVN <Rd>, <Rm>`: MVN with no shift.
#[inline]
pub const fn mvn(sz: Size, rd: RegZr, rm: RegZr) -> u32 {
    logical_shifted(sz, ORR, 1, rd.code(), (RegZr::ZR, rm), Shift::Lsl, 0)
}

/// `TST <Rn>, <Rm>`: TST (shifted register) with no shift.
#[inline]
pub const fn tst(sz: Size, rn: RegZr, rm: RegZr) -> u32 {
    logical_shifted(sz, ANDS, 0, ZR, (rn, rm), Shift::Lsl, 0)
}
