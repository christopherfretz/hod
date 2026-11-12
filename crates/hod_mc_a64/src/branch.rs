//! Branches and PC-relative address generation (`spec/mc-a64.md`
//! "Branches", "Address generation").
//!
//! Every displacement is relative to the address of the instruction itself;
//! see [`Fixup`] for the units and ranges. A forward reference is encoded
//! with displacement 0 and patched later with [`Fixup::patch`].

use crate::fixup::Fixup;
use crate::reg::{Cond, RegZr, Size};

checked! {
    /// `ADR <Xd>, <label>`: ADR.
    ///
    /// Precondition: [`Fixup::Adr21`]`.fits(disp)`.
    pub fn adr / try_adr(rd: RegZr, disp: i64)
    requires Fixup::Adr21.fits(disp);
    { 0x1000_0000 | Fixup::Adr21.field(disp) | rd.code() }

    /// `ADRP <Xd>, <label>`: ADRP. `page_disp` is the distance between the
    /// target's page and this instruction's page, in bytes.
    ///
    /// Precondition: [`Fixup::Adrp21`]`.fits(page_disp)`.
    pub fn adrp / try_adrp(rd: RegZr, page_disp: i64)
    requires Fixup::Adrp21.fits(page_disp);
    { 0x9000_0000 | Fixup::Adrp21.field(page_disp) | rd.code() }

    /// `B <label>`: B.
    ///
    /// Precondition: [`Fixup::Jump26`]`.fits(disp)`.
    pub fn b / try_b(disp: i64)
    requires Fixup::Jump26.fits(disp);
    { 0x1400_0000 | Fixup::Jump26.field(disp) }

    /// `BL <label>`: BL.
    ///
    /// Precondition: [`Fixup::Jump26`]`.fits(disp)`.
    pub fn bl / try_bl(disp: i64)
    requires Fixup::Jump26.fits(disp);
    { 0x9400_0000 | Fixup::Jump26.field(disp) }

    /// `B.<cond> <label>`: B.cond.
    ///
    /// Precondition: [`Fixup::Branch19`]`.fits(disp)`.
    pub fn b_cond / try_b_cond(cond: Cond, disp: i64)
    requires Fixup::Branch19.fits(disp);
    { 0x5400_0000 | Fixup::Branch19.field(disp) | cond.code() }

    /// `CBZ <Rt>, <label>`: CBZ.
    ///
    /// Precondition: [`Fixup::Branch19`]`.fits(disp)`.
    pub fn cbz / try_cbz(sz: Size, rt: RegZr, disp: i64)
    requires Fixup::Branch19.fits(disp);
    { sz.sf() | 0x3400_0000 | Fixup::Branch19.field(disp) | rt.code() }

    /// `CBNZ <Rt>, <label>`: CBNZ.
    ///
    /// Precondition: [`Fixup::Branch19`]`.fits(disp)`.
    pub fn cbnz / try_cbnz(sz: Size, rt: RegZr, disp: i64)
    requires Fixup::Branch19.fits(disp);
    { sz.sf() | 0x3500_0000 | Fixup::Branch19.field(disp) | rt.code() }

    /// `TBZ <Rt>, #bit, <label>`: TBZ. Branches when bit `bit` of `rt` is
    /// zero.
    ///
    /// Precondition: `bit < 64` and [`Fixup::Test14`]`.fits(disp)`.
    pub fn tbz / try_tbz(rt: RegZr, bit: u32, disp: i64)
    requires bit < 64 && Fixup::Test14.fits(disp);
    { test_branch(0, rt, bit, disp) }

    /// `TBNZ <Rt>, #bit, <label>`: TBNZ. Branches when bit `bit` of `rt` is
    /// one.
    ///
    /// Precondition: `bit < 64` and [`Fixup::Test14`]`.fits(disp)`.
    pub fn tbnz / try_tbnz(rt: RegZr, bit: u32, disp: i64)
    requires bit < 64 && Fixup::Test14.fits(disp);
    { test_branch(1, rt, bit, disp) }
}

#[inline]
const fn test_branch(op: u32, rt: RegZr, bit: u32, disp: i64) -> u32 {
    (bit >> 5) << 31
        | 0x3600_0000
        | op << 24
        | (bit & 0x1f) << 19
        | Fixup::Test14.field(disp)
        | rt.code()
}

/// `BR <Xn>`: BR.
#[inline]
pub const fn br(rn: RegZr) -> u32 {
    0xd61f_0000 | rn.code() << 5
}

/// `BLR <Xn>`: BLR.
#[inline]
pub const fn blr(rn: RegZr) -> u32 {
    0xd63f_0000 | rn.code() << 5
}

/// `RET <Xn>`: RET. Assemblers default `Xn` to `X30`
/// ([`Reg::LR`](crate::reg::Reg::LR)).
#[inline]
pub const fn ret(rn: RegZr) -> u32 {
    0xd65f_0000 | rn.code() << 5
}
