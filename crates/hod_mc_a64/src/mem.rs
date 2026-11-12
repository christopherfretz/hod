//! Integer loads and stores in every addressing mode, and load/store pair
//! (`spec/mc-a64.md` "Loads and stores").

use crate::fixup::Fixup;
use crate::reg::{RegSp, RegZr, Size};

/// The width of a memory access.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum MemSize {
    /// 8 bits.
    B = 0,
    /// 16 bits.
    H = 1,
    /// 32 bits.
    W = 2,
    /// 64 bits.
    X = 3,
}

impl MemSize {
    /// Every access width.
    pub const ALL: [Self; 4] = [Self::B, Self::H, Self::W, Self::X];

    /// The access width in bytes: 1, 2, 4 or 8.
    #[inline]
    pub const fn bytes(self) -> i64 {
        1 << self as u32
    }

    /// The `size` field value, which is also log2 of the width in bytes.
    #[inline]
    pub const fn log2(self) -> u32 {
        self as u32
    }
}

/// What a load reads and how it fills the destination register.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Load {
    /// `LDRB Wt`: 8 bits, zero-extended.
    B,
    /// `LDRH Wt`: 16 bits, zero-extended.
    H,
    /// `LDR Wt`: 32 bits, zero-extended into the X register.
    W,
    /// `LDR Xt`: 64 bits.
    X,
    /// `LDRSB Wt`: 8 bits, sign-extended to 32.
    SbW,
    /// `LDRSH Wt`: 16 bits, sign-extended to 32.
    ShW,
    /// `LDRSB Xt`: 8 bits, sign-extended to 64.
    SbX,
    /// `LDRSH Xt`: 16 bits, sign-extended to 64.
    ShX,
    /// `LDRSW Xt`: 32 bits, sign-extended to 64.
    SwX,
}

impl Load {
    /// Every load kind.
    pub const ALL: [Self; 9] =
        [Self::B, Self::H, Self::W, Self::X, Self::SbW, Self::ShW, Self::SbX, Self::ShX, Self::SwX];

    /// The width of the memory access.
    #[inline]
    pub const fn mem_size(self) -> MemSize {
        match self {
            Self::B | Self::SbW | Self::SbX => MemSize::B,
            Self::H | Self::ShW | Self::ShX => MemSize::H,
            Self::W | Self::SwX => MemSize::W,
            Self::X => MemSize::X,
        }
    }

    /// The `opc` field: 1 zero-extends, 2 sign-extends to 64, 3 to 32.
    #[inline]
    const fn opc(self) -> u32 {
        match self {
            Self::B | Self::H | Self::W | Self::X => 1,
            Self::SbX | Self::ShX | Self::SwX => 2,
            Self::SbW | Self::ShW => 3,
        }
    }

    /// The `size` and `opc` fields in place.
    #[inline]
    const fn bits(self) -> u32 {
        self.mem_size().log2() << 30 | self.opc() << 22
    }
}

/// A load that has a PC-relative literal form.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum LitLoad {
    /// `LDR Wt, <label>`.
    W = 0,
    /// `LDR Xt, <label>`.
    X = 1,
    /// `LDRSW Xt, <label>`.
    SwX = 2,
}

impl LitLoad {
    /// Every literal load kind.
    pub const ALL: [Self; 3] = [Self::W, Self::X, Self::SwX];
}

/// How the index register of a register-offset address is extended: the
/// `option` field.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Index {
    /// The low 32 bits (`Wm`), zero-extended.
    Uxtw = 2,
    /// All 64 bits (`Xm`); written `LSL` in assembly.
    Lsl = 3,
    /// The low 32 bits (`Wm`), sign-extended.
    Sxtw = 6,
    /// All 64 bits (`Xm`); written `SXTX` in assembly.
    Sxtx = 7,
}

impl Index {
    /// Every index extension.
    pub const ALL: [Self; 4] = [Self::Uxtw, Self::Lsl, Self::Sxtw, Self::Sxtx];
}

/// The store `size` field in place (`opc` is zero).
#[inline]
const fn store_bits(ms: MemSize) -> u32 {
    ms.log2() << 30
}

/// Load/store register (unsigned immediate).
#[inline]
const fn ls_scaled(bits: u32, ms: MemSize, rt: RegZr, rn: RegSp, offset: i64) -> u32 {
    let imm12 = ((offset >> ms.log2()) as u32) & 0xfff;
    bits | 0x3900_0000 | imm12 << 10 | rn.code() << 5 | rt.code()
}

/// Load/store register (unscaled immediate, post-indexed or pre-indexed),
/// selected by `mode` in bits 11:10.
#[inline]
const fn ls_imm9(bits: u32, mode: u32, rt: RegZr, rn: RegSp, offset: i64) -> u32 {
    bits | 0x3800_0000 | ((offset as u32) & 0x1ff) << 12 | mode << 10 | rn.code() << 5 | rt.code()
}

/// Load/store register (register offset).
#[inline]
const fn ls_reg(bits: u32, rt: RegZr, rn: RegSp, rm: RegZr, index: Index, scaled: bool) -> u32 {
    bits | 0x3820_0800
        | rm.code() << 16
        | (index as u32) << 13
        | (scaled as u32) << 12
        | rn.code() << 5
        | rt.code()
}

const UNSCALED: u32 = 0;
const POST: u32 = 1;
const PRE: u32 = 3;

/// An unsigned scaled offset: a multiple of the access width from 0 to 4095
/// times the width.
#[inline]
const fn scaled_ok(ms: MemSize, offset: i64) -> bool {
    offset >= 0 && offset & (ms.bytes() - 1) == 0 && offset >> ms.log2() <= 0xfff
}

/// A signed 9-bit byte offset.
#[inline]
const fn imm9_ok(offset: i64) -> bool {
    offset >= -256 && offset <= 255
}

/// Writeback to `rn` with `rt` transferred: the two must differ unless the
/// base is SP (where encoding 31 in `rt` is ZR).
#[inline]
const fn writeback_ok(rt: RegZr, rn: RegSp) -> bool {
    rn.is_sp() || rt.code() != rn.code()
}

checked! {
    /// `LDR*  <Rt>, [<Xn|SP>, #offset]`: the unsigned-offset form of LDR
    /// (immediate), LDRB, LDRH, LDRSB, LDRSH, LDRSW.
    ///
    /// Precondition: `offset` is a multiple of the access width in
    /// `0..=4095 * width`.
    pub fn ldr_off / try_ldr_off(op: Load, rt: RegZr, rn: RegSp, offset: i64)
    requires scaled_ok(op.mem_size(), offset);
    { ls_scaled(op.bits(), op.mem_size(), rt, rn, offset) }

    /// `LDUR*  <Rt>, [<Xn|SP>, #offset]`: LDUR, LDURB, LDURH, LDURSB,
    /// LDURSH, LDURSW (unscaled offset).
    ///
    /// Precondition: `-256 <= offset <= 255`.
    pub fn ldur / try_ldur(op: Load, rt: RegZr, rn: RegSp, offset: i64)
    requires imm9_ok(offset);
    { ls_imm9(op.bits(), UNSCALED, rt, rn, offset) }

    /// `LDR*  <Rt>, [<Xn|SP>, #offset]!`: the pre-indexed form of the loads.
    ///
    /// Precondition: `-256 <= offset <= 255`, and `rt` and `rn` are
    /// different registers (always true when `rn` is SP).
    pub fn ldr_pre / try_ldr_pre(op: Load, rt: RegZr, rn: RegSp, offset: i64)
    requires imm9_ok(offset) && writeback_ok(rt, rn);
    { ls_imm9(op.bits(), PRE, rt, rn, offset) }

    /// `LDR*  <Rt>, [<Xn|SP>], #offset`: the post-indexed form of the loads.
    ///
    /// Precondition: `-256 <= offset <= 255`, and `rt` and `rn` are
    /// different registers (always true when `rn` is SP).
    pub fn ldr_post / try_ldr_post(op: Load, rt: RegZr, rn: RegSp, offset: i64)
    requires imm9_ok(offset) && writeback_ok(rt, rn);
    { ls_imm9(op.bits(), POST, rt, rn, offset) }

    /// `LDR <Wt|Xt>, <label>` / `LDRSW <Xt>, <label>`: LDR (literal),
    /// LDRSW (literal).
    ///
    /// Precondition: [`Fixup::Literal19`]`.fits(disp)`.
    pub fn ldr_lit / try_ldr_lit(op: LitLoad, rt: RegZr, disp: i64)
    requires Fixup::Literal19.fits(disp);
    { (op as u32) << 30 | 0x1800_0000 | Fixup::Literal19.field(disp) | rt.code() }

    /// `STR*  <Rt>, [<Xn|SP>, #offset]`: the unsigned-offset form of STR
    /// (immediate), STRB, STRH.
    ///
    /// Precondition: `offset` is a multiple of the access width in
    /// `0..=4095 * width`.
    pub fn str_off / try_str_off(ms: MemSize, rt: RegZr, rn: RegSp, offset: i64)
    requires scaled_ok(ms, offset);
    { ls_scaled(store_bits(ms), ms, rt, rn, offset) }

    /// `STUR*  <Rt>, [<Xn|SP>, #offset]`: STUR, STURB, STURH (unscaled
    /// offset).
    ///
    /// Precondition: `-256 <= offset <= 255`.
    pub fn stur / try_stur(ms: MemSize, rt: RegZr, rn: RegSp, offset: i64)
    requires imm9_ok(offset);
    { ls_imm9(store_bits(ms), UNSCALED, rt, rn, offset) }

    /// `STR*  <Rt>, [<Xn|SP>, #offset]!`: the pre-indexed form of the
    /// stores.
    ///
    /// Precondition: `-256 <= offset <= 255`, and `rt` and `rn` are
    /// different registers (always true when `rn` is SP).
    pub fn str_pre / try_str_pre(ms: MemSize, rt: RegZr, rn: RegSp, offset: i64)
    requires imm9_ok(offset) && writeback_ok(rt, rn);
    { ls_imm9(store_bits(ms), PRE, rt, rn, offset) }

    /// `STR*  <Rt>, [<Xn|SP>], #offset`: the post-indexed form of the
    /// stores.
    ///
    /// Precondition: `-256 <= offset <= 255`, and `rt` and `rn` are
    /// different registers (always true when `rn` is SP).
    pub fn str_post / try_str_post(ms: MemSize, rt: RegZr, rn: RegSp, offset: i64)
    requires imm9_ok(offset) && writeback_ok(rt, rn);
    { ls_imm9(store_bits(ms), POST, rt, rn, offset) }
}

/// `LDR*  <Rt>, [<Xn|SP>, <Rm>, <extend> #amount]`: the register-offset
/// form of the loads. The index is shifted left by log2 of the access width
/// when `scaled`, and not at all otherwise.
#[inline]
pub const fn ldr_reg(op: Load, rt: RegZr, rn: RegSp, rm: RegZr, index: Index, scaled: bool) -> u32 {
    ls_reg(op.bits(), rt, rn, rm, index, scaled)
}

/// `STR*  <Rt>, [<Xn|SP>, <Rm>, <extend> #amount]`: the register-offset
/// form of the stores. The index is shifted left by log2 of the access
/// width when `scaled`, and not at all otherwise.
#[inline]
pub const fn str_reg(
    ms: MemSize,
    rt: RegZr,
    rn: RegSp,
    rm: RegZr,
    index: Index,
    scaled: bool,
) -> u32 {
    ls_reg(store_bits(ms), rt, rn, rm, index, scaled)
}

const PAIR_POST: u32 = 1;
const PAIR_OFFSET: u32 = 2;
const PAIR_PRE: u32 = 3;
const PAIR_W: u32 = 0;
const PAIR_SW: u32 = 1;
const PAIR_X: u32 = 2;

#[inline]
const fn pair_opc(sz: Size) -> u32 {
    match sz {
        Size::W => PAIR_W,
        Size::X => PAIR_X,
    }
}

/// Load/store pair. `scale` is log2 of the element width in bytes.
#[inline]
const fn pair(
    opc: u32,
    mode: u32,
    load: u32,
    regs: (RegZr, RegZr),
    rn: RegSp,
    scale: u32,
    offset: i64,
) -> u32 {
    let (rt, rt2) = regs;
    let imm7 = ((offset >> scale) as u32) & 0x7f;
    opc << 30
        | 0x2800_0000
        | mode << 23
        | load << 22
        | imm7 << 15
        | rt2.code() << 10
        | rn.code() << 5
        | rt.code()
}

/// A pair offset: a multiple of the element width in `-64..=63` widths.
#[inline]
const fn pair_offset_ok(scale: u32, offset: i64) -> bool {
    offset & ((1i64 << scale) - 1) == 0 && offset >> scale >= -64 && offset >> scale <= 63
}

#[inline]
const fn pair_scale(sz: Size) -> u32 {
    match sz {
        Size::W => 2,
        Size::X => 3,
    }
}

/// Writeback with two transferred registers.
#[inline]
const fn pair_writeback_ok(rt: RegZr, rt2: RegZr, rn: RegSp) -> bool {
    writeback_ok(rt, rn) && writeback_ok(rt2, rn)
}

checked! {
    /// `LDP <Rt>, <Rt2>, [<Xn|SP>, #offset]`: LDP, signed-offset form.
    ///
    /// Precondition: `offset` is a multiple of the register width in
    /// `-64..=63` widths; `rt` and `rt2` have different encodings.
    pub fn ldp_off / try_ldp_off(sz: Size, rt: RegZr, rt2: RegZr, rn: RegSp, offset: i64)
    requires pair_offset_ok(pair_scale(sz), offset) && rt.code() != rt2.code();
    { pair(pair_opc(sz), PAIR_OFFSET, 1, (rt, rt2), rn, pair_scale(sz), offset) }

    /// `LDP <Rt>, <Rt2>, [<Xn|SP>, #offset]!`: LDP, pre-indexed form.
    ///
    /// Precondition: as [`ldp_off`], and neither `rt` nor `rt2` is the same
    /// register as `rn` (always true when `rn` is SP).
    pub fn ldp_pre / try_ldp_pre(sz: Size, rt: RegZr, rt2: RegZr, rn: RegSp, offset: i64)
    requires pair_offset_ok(pair_scale(sz), offset)
        && rt.code() != rt2.code()
        && pair_writeback_ok(rt, rt2, rn);
    { pair(pair_opc(sz), PAIR_PRE, 1, (rt, rt2), rn, pair_scale(sz), offset) }

    /// `LDP <Rt>, <Rt2>, [<Xn|SP>], #offset`: LDP, post-indexed form.
    ///
    /// Precondition: as [`ldp_pre`].
    pub fn ldp_post / try_ldp_post(sz: Size, rt: RegZr, rt2: RegZr, rn: RegSp, offset: i64)
    requires pair_offset_ok(pair_scale(sz), offset)
        && rt.code() != rt2.code()
        && pair_writeback_ok(rt, rt2, rn);
    { pair(pair_opc(sz), PAIR_POST, 1, (rt, rt2), rn, pair_scale(sz), offset) }

    /// `LDPSW <Xt>, <Xt2>, [<Xn|SP>, #offset]`: LDPSW, signed-offset form.
    ///
    /// Precondition: `offset` is a multiple of 4 in `-256..=252`; `rt` and
    /// `rt2` have different encodings.
    pub fn ldpsw_off / try_ldpsw_off(rt: RegZr, rt2: RegZr, rn: RegSp, offset: i64)
    requires pair_offset_ok(2, offset) && rt.code() != rt2.code();
    { pair(PAIR_SW, PAIR_OFFSET, 1, (rt, rt2), rn, 2, offset) }

    /// `LDPSW <Xt>, <Xt2>, [<Xn|SP>, #offset]!`: LDPSW, pre-indexed form.
    ///
    /// Precondition: as [`ldpsw_off`], and neither `rt` nor `rt2` is the
    /// same register as `rn` (always true when `rn` is SP).
    pub fn ldpsw_pre / try_ldpsw_pre(rt: RegZr, rt2: RegZr, rn: RegSp, offset: i64)
    requires pair_offset_ok(2, offset)
        && rt.code() != rt2.code()
        && pair_writeback_ok(rt, rt2, rn);
    { pair(PAIR_SW, PAIR_PRE, 1, (rt, rt2), rn, 2, offset) }

    /// `LDPSW <Xt>, <Xt2>, [<Xn|SP>], #offset`: LDPSW, post-indexed form.
    ///
    /// Precondition: as [`ldpsw_pre`].
    pub fn ldpsw_post / try_ldpsw_post(rt: RegZr, rt2: RegZr, rn: RegSp, offset: i64)
    requires pair_offset_ok(2, offset)
        && rt.code() != rt2.code()
        && pair_writeback_ok(rt, rt2, rn);
    { pair(PAIR_SW, PAIR_POST, 1, (rt, rt2), rn, 2, offset) }

    /// `STP <Rt>, <Rt2>, [<Xn|SP>, #offset]`: STP, signed-offset form.
    ///
    /// Precondition: `offset` is a multiple of the register width in
    /// `-64..=63` widths.
    pub fn stp_off / try_stp_off(sz: Size, rt: RegZr, rt2: RegZr, rn: RegSp, offset: i64)
    requires pair_offset_ok(pair_scale(sz), offset);
    { pair(pair_opc(sz), PAIR_OFFSET, 0, (rt, rt2), rn, pair_scale(sz), offset) }

    /// `STP <Rt>, <Rt2>, [<Xn|SP>, #offset]!`: STP, pre-indexed form.
    ///
    /// Precondition: as [`stp_off`], and neither `rt` nor `rt2` is the same
    /// register as `rn` (always true when `rn` is SP).
    pub fn stp_pre / try_stp_pre(sz: Size, rt: RegZr, rt2: RegZr, rn: RegSp, offset: i64)
    requires pair_offset_ok(pair_scale(sz), offset) && pair_writeback_ok(rt, rt2, rn);
    { pair(pair_opc(sz), PAIR_PRE, 0, (rt, rt2), rn, pair_scale(sz), offset) }

    /// `STP <Rt>, <Rt2>, [<Xn|SP>], #offset`: STP, post-indexed form.
    ///
    /// Precondition: as [`stp_pre`].
    pub fn stp_post / try_stp_post(sz: Size, rt: RegZr, rt2: RegZr, rn: RegSp, offset: i64)
    requires pair_offset_ok(pair_scale(sz), offset) && pair_writeback_ok(rt, rt2, rn);
    { pair(pair_opc(sz), PAIR_POST, 0, (rt, rt2), rn, pair_scale(sz), offset) }
}
