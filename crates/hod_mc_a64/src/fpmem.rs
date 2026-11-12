//! FP/SIMD loads and stores in every addressing mode, and load/store pair
//! (`spec/mc-a64.md` "FP/SIMD loads and stores"). These are the `V` = 1
//! halves of the encoding classes in [`crate::mem`], which holds the shared
//! field packing.

use crate::fixup::Fixup;
use crate::mem::{
    Index, PAIR_OFFSET, PAIR_POST, PAIR_PRE, POST, PRE, UNSCALED, imm9_ok, ls_imm9, ls_reg,
    ls_scaled, pair, pair_offset_ok, scaled_ok,
};
use crate::reg::{RegSp, RegZr, VReg};

/// The width of an FP/SIMD load or store: which scalar view of the register
/// is transferred. A load zeroes the rest of the register.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum VMemSize {
    /// 8 bits: `B` registers.
    B = 0,
    /// 16 bits: `H` registers.
    H = 1,
    /// 32 bits: `S` registers.
    S = 2,
    /// 64 bits: `D` registers.
    D = 3,
    /// 128 bits: `Q` registers.
    Q = 4,
}

impl VMemSize {
    /// Every access width.
    pub const ALL: [Self; 5] = [Self::B, Self::H, Self::S, Self::D, Self::Q];

    /// The access width in bytes: 1, 2, 4, 8 or 16.
    #[inline]
    pub const fn bytes(self) -> i64 {
        1 << self as u32
    }

    /// log2 of the width in bytes.
    #[inline]
    pub const fn log2(self) -> u32 {
        self as u32
    }

    /// The `size` field, the `V` bit and the `opc` field in place. The
    /// 128-bit access is `size` = 0 with the high bit of `opc` set.
    #[inline]
    const fn bits(self, load: u32) -> u32 {
        let (size, opc) = match self {
            Self::Q => (0, 2 | load),
            _ => (self as u32, load),
        };
        size << 30 | V | opc << 22
    }
}

/// The width of each register of an FP/SIMD pair, and of an FP/SIMD
/// literal load: the forms that exist only for `S`, `D` and `Q`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum VPairSize {
    /// 32 bits: `S` registers.
    S = 0,
    /// 64 bits: `D` registers.
    D = 1,
    /// 128 bits: `Q` registers.
    Q = 2,
}

impl VPairSize {
    /// Every pair width.
    pub const ALL: [Self; 3] = [Self::S, Self::D, Self::Q];

    /// The width of one register in bytes: 4, 8 or 16.
    #[inline]
    pub const fn bytes(self) -> i64 {
        4 << self as u32
    }

    /// log2 of the width of one register in bytes.
    #[inline]
    const fn scale(self) -> u32 {
        self as u32 + 2
    }
}

/// The `V` bit: an FP/SIMD register is transferred.
const V: u32 = 1 << 26;
const LOAD: u32 = 1;
const STORE: u32 = 0;

#[inline]
const fn off(load: u32, sz: VMemSize, rt: VReg, rn: RegSp, offset: i64) -> u32 {
    ls_scaled(sz.bits(load), sz.log2(), rt.code(), rn, offset)
}

#[inline]
const fn imm9(mode: u32, load: u32, sz: VMemSize, rt: VReg, rn: RegSp, offset: i64) -> u32 {
    ls_imm9(sz.bits(load), mode, rt.code(), rn, offset)
}

#[inline]
const fn reg(
    load: u32,
    sz: VMemSize,
    rt: VReg,
    rn: RegSp,
    rm: RegZr,
    index: Index,
    scaled: bool,
) -> u32 {
    ls_reg(sz.bits(load), rt.code(), rn, rm, index, scaled)
}

#[inline]
const fn vpair(
    mode: u32,
    load: u32,
    sz: VPairSize,
    rt: VReg,
    rt2: VReg,
    rn: RegSp,
    offset: i64,
) -> u32 {
    V | pair(sz as u32, mode, load, (rt.code(), rt2.code()), rn, sz.scale(), offset)
}

family! {
    (sz: VMemSize, rt: VReg, rn: RegSp, offset: i64)
    requires (scaled_ok(sz.log2(), offset)) => off;

    /// `LDR <Bt|Ht|St|Dt|Qt>, [<Xn|SP>, #offset]`: LDR (immediate,
    /// SIMD&FP), unsigned offset.
    ///
    /// Precondition: `offset` is a multiple of the access width in
    /// `0..=4095 * width`.
    pub fn ldr_fp_off / try_ldr_fp_off = (LOAD);
    /// `STR <Bt|Ht|St|Dt|Qt>, [<Xn|SP>, #offset]`: STR (immediate,
    /// SIMD&FP), unsigned offset.
    ///
    /// Precondition: `offset` is a multiple of the access width in
    /// `0..=4095 * width`.
    pub fn str_fp_off / try_str_fp_off = (STORE);
}

family! {
    (sz: VMemSize, rt: VReg, rn: RegSp, offset: i64)
    requires (imm9_ok(offset)) => imm9;

    /// `LDUR <Bt|Ht|St|Dt|Qt>, [<Xn|SP>, #offset]`: LDUR (SIMD&FP).
    ///
    /// Precondition: `-256 <= offset <= 255`.
    pub fn ldur_fp / try_ldur_fp = (UNSCALED, LOAD);
    /// `LDR <Bt|Ht|St|Dt|Qt>, [<Xn|SP>, #offset]!`: LDR (immediate,
    /// SIMD&FP), pre-indexed.
    ///
    /// Precondition: `-256 <= offset <= 255`.
    pub fn ldr_fp_pre / try_ldr_fp_pre = (PRE, LOAD);
    /// `LDR <Bt|Ht|St|Dt|Qt>, [<Xn|SP>], #offset`: LDR (immediate,
    /// SIMD&FP), post-indexed.
    ///
    /// Precondition: `-256 <= offset <= 255`.
    pub fn ldr_fp_post / try_ldr_fp_post = (POST, LOAD);
    /// `STUR <Bt|Ht|St|Dt|Qt>, [<Xn|SP>, #offset]`: STUR (SIMD&FP).
    ///
    /// Precondition: `-256 <= offset <= 255`.
    pub fn stur_fp / try_stur_fp = (UNSCALED, STORE);
    /// `STR <Bt|Ht|St|Dt|Qt>, [<Xn|SP>, #offset]!`: STR (immediate,
    /// SIMD&FP), pre-indexed.
    ///
    /// Precondition: `-256 <= offset <= 255`.
    pub fn str_fp_pre / try_str_fp_pre = (PRE, STORE);
    /// `STR <Bt|Ht|St|Dt|Qt>, [<Xn|SP>], #offset`: STR (immediate,
    /// SIMD&FP), post-indexed.
    ///
    /// Precondition: `-256 <= offset <= 255`.
    pub fn str_fp_post / try_str_fp_post = (POST, STORE);
}

family! {
    (sz: VMemSize, rt: VReg, rn: RegSp, rm: RegZr, index: Index, scaled: bool) => reg;

    /// `LDR <Bt|Ht|St|Dt|Qt>, [<Xn|SP>, <Rm>, <extend> #amount]`: LDR
    /// (register, SIMD&FP). The index is shifted left by log2 of the access
    /// width when `scaled`, and not at all otherwise.
    pub fn ldr_fp_reg = (LOAD);
    /// `STR <Bt|Ht|St|Dt|Qt>, [<Xn|SP>, <Rm>, <extend> #amount]`: STR
    /// (register, SIMD&FP). The index is shifted left by log2 of the access
    /// width when `scaled`, and not at all otherwise.
    pub fn str_fp_reg = (STORE);
}

checked! {
    /// `LDR <St|Dt|Qt>, <label>`: LDR (literal, SIMD&FP).
    ///
    /// Precondition: [`Fixup::Literal19`]`.fits(disp)`.
    pub fn ldr_fp_lit / try_ldr_fp_lit(sz: VPairSize, rt: VReg, disp: i64)
    requires Fixup::Literal19.fits(disp);
    { (sz as u32) << 30 | 0x1c00_0000 | Fixup::Literal19.field(disp) | rt.code() }
}

family! {
    (sz: VPairSize, rt: VReg, rt2: VReg, rn: RegSp, offset: i64)
    requires (pair_offset_ok(sz.scale(), offset) && rt.code() != rt2.code()) => vpair;

    /// `LDP <St|Dt|Qt>, <St2|Dt2|Qt2>, [<Xn|SP>, #offset]`: LDP (SIMD&FP),
    /// signed offset.
    ///
    /// Precondition: `offset` is a multiple of the register width in
    /// `-64..=63` widths; `rt` and `rt2` are different registers.
    pub fn ldp_fp_off / try_ldp_fp_off = (PAIR_OFFSET, LOAD);
    /// `LDP <St|Dt|Qt>, <St2|Dt2|Qt2>, [<Xn|SP>, #offset]!`: LDP
    /// (SIMD&FP), pre-indexed.
    ///
    /// Precondition: as [`ldp_fp_off`].
    pub fn ldp_fp_pre / try_ldp_fp_pre = (PAIR_PRE, LOAD);
    /// `LDP <St|Dt|Qt>, <St2|Dt2|Qt2>, [<Xn|SP>], #offset`: LDP (SIMD&FP),
    /// post-indexed.
    ///
    /// Precondition: as [`ldp_fp_off`].
    pub fn ldp_fp_post / try_ldp_fp_post = (PAIR_POST, LOAD);
}

family! {
    (sz: VPairSize, rt: VReg, rt2: VReg, rn: RegSp, offset: i64)
    requires (pair_offset_ok(sz.scale(), offset)) => vpair;

    /// `STP <St|Dt|Qt>, <St2|Dt2|Qt2>, [<Xn|SP>, #offset]`: STP (SIMD&FP),
    /// signed offset.
    ///
    /// Precondition: `offset` is a multiple of the register width in
    /// `-64..=63` widths.
    pub fn stp_fp_off / try_stp_fp_off = (PAIR_OFFSET, STORE);
    /// `STP <St|Dt|Qt>, <St2|Dt2|Qt2>, [<Xn|SP>, #offset]!`: STP
    /// (SIMD&FP), pre-indexed.
    ///
    /// Precondition: as [`stp_fp_off`].
    pub fn stp_fp_pre / try_stp_fp_pre = (PAIR_PRE, STORE);
    /// `STP <St|Dt|Qt>, <St2|Dt2|Qt2>, [<Xn|SP>], #offset`: STP (SIMD&FP),
    /// post-indexed.
    ///
    /// Precondition: as [`stp_fp_off`].
    pub fn stp_fp_post / try_stp_fp_post = (PAIR_POST, STORE);
}
