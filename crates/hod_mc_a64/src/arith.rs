//! Integer arithmetic: add/subtract in its three operand forms, add/subtract
//! with carry, multiply, divide (`spec/mc-a64.md` "Arithmetic").

use crate::imm::is_arith_imm;
use crate::reg::{Extend, RegSp, RegZr, Shift, Size};

const ADD: u32 = 0;
const ADDS: u32 = 1;
const SUB: u32 = 2;
const SUBS: u32 = 3;
const ZR: u32 = 31;

/// Add/subtract (immediate). `op` is the `op:S` pair.
#[inline]
pub(crate) const fn addsub_imm(sz: Size, op: u32, rd: u32, rn: u32, imm: u64) -> u32 {
    let (imm12, sh) = if imm < 0x1000 { (imm as u32, 0) } else { ((imm >> 12) as u32, 1) };
    sz.sf() | op << 29 | 0x1100_0000 | sh << 22 | (imm12 & 0xfff) << 10 | rn << 5 | rd
}

/// Add/subtract (shifted register).
#[inline]
const fn addsub_shifted(
    sz: Size,
    op: u32,
    rd: u32,
    rn: u32,
    rm: u32,
    shift: Shift,
    amount: u32,
) -> u32 {
    sz.sf()
        | op << 29
        | 0x0b00_0000
        | (shift as u32) << 22
        | rm << 16
        | (amount & 0x3f) << 10
        | rn << 5
        | rd
}

/// Add/subtract (extended register).
#[inline]
const fn addsub_ext(sz: Size, op: u32, rd: u32, rn: u32, rm: u32, ext: Extend, amount: u32) -> u32 {
    sz.sf()
        | op << 29
        | 0x0b20_0000
        | rm << 16
        | (ext as u32) << 13
        | (amount & 7) << 10
        | rn << 5
        | rd
}

/// The shift operand of an add/subtract (shifted register) form: no `ROR`,
/// amount below the operand size.
#[inline]
const fn arith_shift_ok(sz: Size, shift: Shift, amount: u32) -> bool {
    !matches!(shift, Shift::Ror) && amount < sz.bits()
}

checked! {
    /// `ADD <Rd|SP>, <Rn|SP>, #imm`: ADD (immediate).
    ///
    /// Precondition: [`is_arith_imm`]`(imm)`.
    pub fn add_imm / try_add_imm(sz: Size, rd: RegSp, rn: RegSp, imm: u64)
    requires is_arith_imm(imm);
    { addsub_imm(sz, ADD, rd.code(), rn.code(), imm) }

    /// `ADDS <Rd>, <Rn|SP>, #imm`: ADDS (immediate).
    ///
    /// Precondition: [`is_arith_imm`]`(imm)`.
    pub fn adds_imm / try_adds_imm(sz: Size, rd: RegZr, rn: RegSp, imm: u64)
    requires is_arith_imm(imm);
    { addsub_imm(sz, ADDS, rd.code(), rn.code(), imm) }

    /// `SUB <Rd|SP>, <Rn|SP>, #imm`: SUB (immediate).
    ///
    /// Precondition: [`is_arith_imm`]`(imm)`.
    pub fn sub_imm / try_sub_imm(sz: Size, rd: RegSp, rn: RegSp, imm: u64)
    requires is_arith_imm(imm);
    { addsub_imm(sz, SUB, rd.code(), rn.code(), imm) }

    /// `SUBS <Rd>, <Rn|SP>, #imm`: SUBS (immediate).
    ///
    /// Precondition: [`is_arith_imm`]`(imm)`.
    pub fn subs_imm / try_subs_imm(sz: Size, rd: RegZr, rn: RegSp, imm: u64)
    requires is_arith_imm(imm);
    { addsub_imm(sz, SUBS, rd.code(), rn.code(), imm) }

    /// `CMP <Rn|SP>, #imm`: CMP (immediate), an alias of SUBS (immediate).
    ///
    /// Precondition: [`is_arith_imm`]`(imm)`.
    pub fn cmp_imm / try_cmp_imm(sz: Size, rn: RegSp, imm: u64)
    requires is_arith_imm(imm);
    { addsub_imm(sz, SUBS, ZR, rn.code(), imm) }

    /// `CMN <Rn|SP>, #imm`: CMN (immediate), an alias of ADDS (immediate).
    ///
    /// Precondition: [`is_arith_imm`]`(imm)`.
    pub fn cmn_imm / try_cmn_imm(sz: Size, rn: RegSp, imm: u64)
    requires is_arith_imm(imm);
    { addsub_imm(sz, ADDS, ZR, rn.code(), imm) }

    /// `ADD <Rd>, <Rn>, <Rm>, <shift> #amount`: ADD (shifted register).
    ///
    /// Precondition: `shift` is not `Ror` and `amount < sz.bits()`.
    pub fn add_shifted / try_add_shifted(
        sz: Size, rd: RegZr, rn: RegZr, rm: RegZr, shift: Shift, amount: u32
    )
    requires arith_shift_ok(sz, shift, amount);
    { addsub_shifted(sz, ADD, rd.code(), rn.code(), rm.code(), shift, amount) }

    /// `ADDS <Rd>, <Rn>, <Rm>, <shift> #amount`: ADDS (shifted register).
    ///
    /// Precondition: `shift` is not `Ror` and `amount < sz.bits()`.
    pub fn adds_shifted / try_adds_shifted(
        sz: Size, rd: RegZr, rn: RegZr, rm: RegZr, shift: Shift, amount: u32
    )
    requires arith_shift_ok(sz, shift, amount);
    { addsub_shifted(sz, ADDS, rd.code(), rn.code(), rm.code(), shift, amount) }

    /// `SUB <Rd>, <Rn>, <Rm>, <shift> #amount`: SUB (shifted register).
    ///
    /// Precondition: `shift` is not `Ror` and `amount < sz.bits()`.
    pub fn sub_shifted / try_sub_shifted(
        sz: Size, rd: RegZr, rn: RegZr, rm: RegZr, shift: Shift, amount: u32
    )
    requires arith_shift_ok(sz, shift, amount);
    { addsub_shifted(sz, SUB, rd.code(), rn.code(), rm.code(), shift, amount) }

    /// `SUBS <Rd>, <Rn>, <Rm>, <shift> #amount`: SUBS (shifted register).
    ///
    /// Precondition: `shift` is not `Ror` and `amount < sz.bits()`.
    pub fn subs_shifted / try_subs_shifted(
        sz: Size, rd: RegZr, rn: RegZr, rm: RegZr, shift: Shift, amount: u32
    )
    requires arith_shift_ok(sz, shift, amount);
    { addsub_shifted(sz, SUBS, rd.code(), rn.code(), rm.code(), shift, amount) }

    /// `CMP <Rn>, <Rm>, <shift> #amount`: CMP (shifted register), an alias
    /// of SUBS (shifted register).
    ///
    /// Precondition: `shift` is not `Ror` and `amount < sz.bits()`.
    pub fn cmp_shifted / try_cmp_shifted(
        sz: Size, rn: RegZr, rm: RegZr, shift: Shift, amount: u32
    )
    requires arith_shift_ok(sz, shift, amount);
    { addsub_shifted(sz, SUBS, ZR, rn.code(), rm.code(), shift, amount) }

    /// `CMN <Rn>, <Rm>, <shift> #amount`: CMN (shifted register), an alias
    /// of ADDS (shifted register).
    ///
    /// Precondition: `shift` is not `Ror` and `amount < sz.bits()`.
    pub fn cmn_shifted / try_cmn_shifted(
        sz: Size, rn: RegZr, rm: RegZr, shift: Shift, amount: u32
    )
    requires arith_shift_ok(sz, shift, amount);
    { addsub_shifted(sz, ADDS, ZR, rn.code(), rm.code(), shift, amount) }

    /// `NEG <Rd>, <Rm>, <shift> #amount`: NEG (shifted register), an alias
    /// of SUB (shifted register).
    ///
    /// Precondition: `shift` is not `Ror` and `amount < sz.bits()`.
    pub fn neg_shifted / try_neg_shifted(
        sz: Size, rd: RegZr, rm: RegZr, shift: Shift, amount: u32
    )
    requires arith_shift_ok(sz, shift, amount);
    { addsub_shifted(sz, SUB, rd.code(), ZR, rm.code(), shift, amount) }

    /// `NEGS <Rd>, <Rm>, <shift> #amount`: NEGS, an alias of SUBS (shifted
    /// register).
    ///
    /// Precondition: `shift` is not `Ror` and `amount < sz.bits()`.
    pub fn negs_shifted / try_negs_shifted(
        sz: Size, rd: RegZr, rm: RegZr, shift: Shift, amount: u32
    )
    requires arith_shift_ok(sz, shift, amount);
    { addsub_shifted(sz, SUBS, rd.code(), ZR, rm.code(), shift, amount) }

    /// `ADD <Rd|SP>, <Rn|SP>, <Rm>, <extend> #amount`: ADD (extended
    /// register). `rm` is read as `W` unless `ext` is `Uxtx`/`Sxtx`.
    ///
    /// This is the form to use when `rd` or `rn` is SP; `Extend::Uxtx` with
    /// amount 0 is a plain 64-bit add (`Extend::Uxtw` for a 32-bit one).
    ///
    /// Precondition: `amount <= 4`.
    pub fn add_ext / try_add_ext(
        sz: Size, rd: RegSp, rn: RegSp, rm: RegZr, ext: Extend, amount: u32
    )
    requires amount <= 4;
    { addsub_ext(sz, ADD, rd.code(), rn.code(), rm.code(), ext, amount) }

    /// `ADDS <Rd>, <Rn|SP>, <Rm>, <extend> #amount`: ADDS (extended
    /// register).
    ///
    /// Precondition: `amount <= 4`.
    pub fn adds_ext / try_adds_ext(
        sz: Size, rd: RegZr, rn: RegSp, rm: RegZr, ext: Extend, amount: u32
    )
    requires amount <= 4;
    { addsub_ext(sz, ADDS, rd.code(), rn.code(), rm.code(), ext, amount) }

    /// `SUB <Rd|SP>, <Rn|SP>, <Rm>, <extend> #amount`: SUB (extended
    /// register).
    ///
    /// Precondition: `amount <= 4`.
    pub fn sub_ext / try_sub_ext(
        sz: Size, rd: RegSp, rn: RegSp, rm: RegZr, ext: Extend, amount: u32
    )
    requires amount <= 4;
    { addsub_ext(sz, SUB, rd.code(), rn.code(), rm.code(), ext, amount) }

    /// `SUBS <Rd>, <Rn|SP>, <Rm>, <extend> #amount`: SUBS (extended
    /// register).
    ///
    /// Precondition: `amount <= 4`.
    pub fn subs_ext / try_subs_ext(
        sz: Size, rd: RegZr, rn: RegSp, rm: RegZr, ext: Extend, amount: u32
    )
    requires amount <= 4;
    { addsub_ext(sz, SUBS, rd.code(), rn.code(), rm.code(), ext, amount) }

    /// `CMP <Rn|SP>, <Rm>, <extend> #amount`: CMP (extended register), an
    /// alias of SUBS (extended register).
    ///
    /// Precondition: `amount <= 4`.
    pub fn cmp_ext / try_cmp_ext(sz: Size, rn: RegSp, rm: RegZr, ext: Extend, amount: u32)
    requires amount <= 4;
    { addsub_ext(sz, SUBS, ZR, rn.code(), rm.code(), ext, amount) }

    /// `CMN <Rn|SP>, <Rm>, <extend> #amount`: CMN (extended register), an
    /// alias of ADDS (extended register).
    ///
    /// Precondition: `amount <= 4`.
    pub fn cmn_ext / try_cmn_ext(sz: Size, rn: RegSp, rm: RegZr, ext: Extend, amount: u32)
    requires amount <= 4;
    { addsub_ext(sz, ADDS, ZR, rn.code(), rm.code(), ext, amount) }
}

/// `MOV <Rd|SP>, <Rn|SP>`: MOV (to/from SP), an alias of ADD (immediate)
/// with a zero immediate.
#[inline]
pub const fn mov_sp(sz: Size, rd: RegSp, rn: RegSp) -> u32 {
    addsub_imm(sz, ADD, rd.code(), rn.code(), 0)
}

/// `ADD <Rd>, <Rn>, <Rm>`: ADD (shifted register) with no shift.
#[inline]
pub const fn add(sz: Size, rd: RegZr, rn: RegZr, rm: RegZr) -> u32 {
    addsub_shifted(sz, ADD, rd.code(), rn.code(), rm.code(), Shift::Lsl, 0)
}

/// `ADDS <Rd>, <Rn>, <Rm>`: ADDS (shifted register) with no shift.
#[inline]
pub const fn adds(sz: Size, rd: RegZr, rn: RegZr, rm: RegZr) -> u32 {
    addsub_shifted(sz, ADDS, rd.code(), rn.code(), rm.code(), Shift::Lsl, 0)
}

/// `SUB <Rd>, <Rn>, <Rm>`: SUB (shifted register) with no shift.
#[inline]
pub const fn sub(sz: Size, rd: RegZr, rn: RegZr, rm: RegZr) -> u32 {
    addsub_shifted(sz, SUB, rd.code(), rn.code(), rm.code(), Shift::Lsl, 0)
}

/// `SUBS <Rd>, <Rn>, <Rm>`: SUBS (shifted register) with no shift.
#[inline]
pub const fn subs(sz: Size, rd: RegZr, rn: RegZr, rm: RegZr) -> u32 {
    addsub_shifted(sz, SUBS, rd.code(), rn.code(), rm.code(), Shift::Lsl, 0)
}

/// `CMP <Rn>, <Rm>`: CMP (shifted register) with no shift.
#[inline]
pub const fn cmp(sz: Size, rn: RegZr, rm: RegZr) -> u32 {
    addsub_shifted(sz, SUBS, ZR, rn.code(), rm.code(), Shift::Lsl, 0)
}

/// `CMN <Rn>, <Rm>`: CMN (shifted register) with no shift.
#[inline]
pub const fn cmn(sz: Size, rn: RegZr, rm: RegZr) -> u32 {
    addsub_shifted(sz, ADDS, ZR, rn.code(), rm.code(), Shift::Lsl, 0)
}

/// `NEG <Rd>, <Rm>`: NEG (shifted register) with no shift.
#[inline]
pub const fn neg(sz: Size, rd: RegZr, rm: RegZr) -> u32 {
    addsub_shifted(sz, SUB, rd.code(), ZR, rm.code(), Shift::Lsl, 0)
}

/// `NEGS <Rd>, <Rm>`: NEGS with no shift.
#[inline]
pub const fn negs(sz: Size, rd: RegZr, rm: RegZr) -> u32 {
    addsub_shifted(sz, SUBS, rd.code(), ZR, rm.code(), Shift::Lsl, 0)
}

/// Add/subtract (with carry).
#[inline]
const fn addsub_carry(sz: Size, op: u32, rd: RegZr, rn: RegZr, rm: RegZr) -> u32 {
    sz.sf() | op << 29 | 0x1a00_0000 | rm.code() << 16 | rn.code() << 5 | rd.code()
}

/// `ADC <Rd>, <Rn>, <Rm>`: ADC.
#[inline]
pub const fn adc(sz: Size, rd: RegZr, rn: RegZr, rm: RegZr) -> u32 {
    addsub_carry(sz, ADD, rd, rn, rm)
}

/// `ADCS <Rd>, <Rn>, <Rm>`: ADCS.
#[inline]
pub const fn adcs(sz: Size, rd: RegZr, rn: RegZr, rm: RegZr) -> u32 {
    addsub_carry(sz, ADDS, rd, rn, rm)
}

/// `SBC <Rd>, <Rn>, <Rm>`: SBC.
#[inline]
pub const fn sbc(sz: Size, rd: RegZr, rn: RegZr, rm: RegZr) -> u32 {
    addsub_carry(sz, SUB, rd, rn, rm)
}

/// `SBCS <Rd>, <Rn>, <Rm>`: SBCS.
#[inline]
pub const fn sbcs(sz: Size, rd: RegZr, rn: RegZr, rm: RegZr) -> u32 {
    addsub_carry(sz, SUBS, rd, rn, rm)
}

/// Data-processing (3 source): `base` carries `sf`, `op31` and `o0`.
#[inline]
const fn dp3(base: u32, rd: RegZr, rn: RegZr, rm: RegZr, ra: RegZr) -> u32 {
    base | rm.code() << 16 | ra.code() << 10 | rn.code() << 5 | rd.code()
}

/// `MADD <Rd>, <Rn>, <Rm>, <Ra>`: MADD (`rd = ra + rn * rm`).
#[inline]
pub const fn madd(sz: Size, rd: RegZr, rn: RegZr, rm: RegZr, ra: RegZr) -> u32 {
    dp3(sz.sf() | 0x1b00_0000, rd, rn, rm, ra)
}

/// `MSUB <Rd>, <Rn>, <Rm>, <Ra>`: MSUB (`rd = ra - rn * rm`).
#[inline]
pub const fn msub(sz: Size, rd: RegZr, rn: RegZr, rm: RegZr, ra: RegZr) -> u32 {
    dp3(sz.sf() | 0x1b00_8000, rd, rn, rm, ra)
}

/// `MUL <Rd>, <Rn>, <Rm>`: MUL, an alias of MADD with `Ra` = ZR.
#[inline]
pub const fn mul(sz: Size, rd: RegZr, rn: RegZr, rm: RegZr) -> u32 {
    madd(sz, rd, rn, rm, RegZr::ZR)
}

/// `MNEG <Rd>, <Rn>, <Rm>`: MNEG, an alias of MSUB with `Ra` = ZR.
#[inline]
pub const fn mneg(sz: Size, rd: RegZr, rn: RegZr, rm: RegZr) -> u32 {
    msub(sz, rd, rn, rm, RegZr::ZR)
}

/// `SMADDL <Xd>, <Wn>, <Wm>, <Xa>`: SMADDL.
#[inline]
pub const fn smaddl(rd: RegZr, rn: RegZr, rm: RegZr, ra: RegZr) -> u32 {
    dp3(0x9b20_0000, rd, rn, rm, ra)
}

/// `SMSUBL <Xd>, <Wn>, <Wm>, <Xa>`: SMSUBL.
#[inline]
pub const fn smsubl(rd: RegZr, rn: RegZr, rm: RegZr, ra: RegZr) -> u32 {
    dp3(0x9b20_8000, rd, rn, rm, ra)
}

/// `UMADDL <Xd>, <Wn>, <Wm>, <Xa>`: UMADDL.
#[inline]
pub const fn umaddl(rd: RegZr, rn: RegZr, rm: RegZr, ra: RegZr) -> u32 {
    dp3(0x9ba0_0000, rd, rn, rm, ra)
}

/// `UMSUBL <Xd>, <Wn>, <Wm>, <Xa>`: UMSUBL.
#[inline]
pub const fn umsubl(rd: RegZr, rn: RegZr, rm: RegZr, ra: RegZr) -> u32 {
    dp3(0x9ba0_8000, rd, rn, rm, ra)
}

/// `SMULL <Xd>, <Wn>, <Wm>`: SMULL, an alias of SMADDL with `Xa` = XZR.
#[inline]
pub const fn smull(rd: RegZr, rn: RegZr, rm: RegZr) -> u32 {
    smaddl(rd, rn, rm, RegZr::ZR)
}

/// `UMULL <Xd>, <Wn>, <Wm>`: UMULL, an alias of UMADDL with `Xa` = XZR.
#[inline]
pub const fn umull(rd: RegZr, rn: RegZr, rm: RegZr) -> u32 {
    umaddl(rd, rn, rm, RegZr::ZR)
}

/// `SMULH <Xd>, <Xn>, <Xm>`: SMULH (high 64 bits of the signed product).
#[inline]
pub const fn smulh(rd: RegZr, rn: RegZr, rm: RegZr) -> u32 {
    dp3(0x9b40_0000, rd, rn, rm, RegZr::ZR)
}

/// `UMULH <Xd>, <Xn>, <Xm>`: UMULH (high 64 bits of the unsigned product).
#[inline]
pub const fn umulh(rd: RegZr, rn: RegZr, rm: RegZr) -> u32 {
    dp3(0x9bc0_0000, rd, rn, rm, RegZr::ZR)
}

/// Data-processing (2 source).
#[inline]
pub(crate) const fn dp2(sz: Size, opcode: u32, rd: RegZr, rn: RegZr, rm: RegZr) -> u32 {
    sz.sf() | 0x1ac0_0000 | rm.code() << 16 | opcode << 10 | rn.code() << 5 | rd.code()
}

/// `UDIV <Rd>, <Rn>, <Rm>`: UDIV.
#[inline]
pub const fn udiv(sz: Size, rd: RegZr, rn: RegZr, rm: RegZr) -> u32 {
    dp2(sz, 0b00_0010, rd, rn, rm)
}

/// `SDIV <Rd>, <Rn>, <Rm>`: SDIV.
#[inline]
pub const fn sdiv(sz: Size, rd: RegZr, rn: RegZr, rm: RegZr) -> u32 {
    dp2(sz, 0b00_0011, rd, rn, rm)
}

/// The eight CRC32 instructions (FEAT_CRC32): the polynomial (`C`: the
/// Castagnoli one) and the width of the data operand.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Crc32 {
    /// `CRC32B`: a byte.
    B,
    /// `CRC32H`: a halfword.
    H,
    /// `CRC32W`: a word.
    W,
    /// `CRC32X`: a doubleword (the data operand is an `X` register).
    X,
    /// `CRC32CB`.
    Cb,
    /// `CRC32CH`.
    Ch,
    /// `CRC32CW`.
    Cw,
    /// `CRC32CX`.
    Cx,
}

impl Crc32 {
    /// Every form.
    pub const ALL: [Self; 8] =
        [Self::B, Self::H, Self::W, Self::X, Self::Cb, Self::Ch, Self::Cw, Self::Cx];
}

/// `CRC32{B,H,W,X} <Wd>, <Wn>, <Wm>|<Xm>` and the `CRC32C` forms: CRC32,
/// CRC32C (WP-084). `rd` and `rn` are `W` registers; `rm` is an `X`
/// register for the `X` forms, else a `W` register.
#[inline]
pub const fn crc32(kind: Crc32, rd: RegZr, rn: RegZr, rm: RegZr) -> u32 {
    let (c, sz) = match kind {
        Crc32::B => (0, 0),
        Crc32::H => (0, 1),
        Crc32::W => (0, 2),
        Crc32::X => (0, 3),
        Crc32::Cb => (1, 0),
        Crc32::Ch => (1, 1),
        Crc32::Cw => (1, 2),
        Crc32::Cx => (1, 3),
    };
    let size = if sz == 3 { Size::X } else { Size::W };
    dp2(size, 0b01_0000 | c << 2 | sz, rd, rn, rm)
}
