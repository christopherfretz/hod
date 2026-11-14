//! VEX-encoded instructions (AVX, AVX2, FMA, F16C, and the VEX forms of
//! GFNI) on `xmm` (128 bits, `VEX.L` 0) and `ymm` (256 bits, `VEX.L` 1)
//! operands: what the x86 target intrinsics past SSSE3 need (WP-202,
//! `spec/mc-x64.md` "VEX").
//!
//! A vector register is an [`Xmm`] (its number) and a [`Width`]: `ymm<n>`
//! is the 256-bit register whose low half is `xmm<n>`. The families:
//!
//! - [`V3`]: `op dst, a, b/m` (`a` in `VEX.vvvv`), and [`V3Imm`] with an
//!   `imm8`;
//! - [`V2`]: `op dst, src/m` (`VEX.vvvv` 1111), and [`V2Imm`] with an
//!   `imm8`;
//! - [`VShift`]: the shifts by an immediate (`dst` in `VEX.vvvv`, the
//!   source in `ModRM.rm`, the operation a `/digit`);
//! - [`VBlend`]: the variable blends, the mask register in `imm8[7:4]`;
//! - [`Fma`]: the `213` forms of FMA3 (`dst = a·dst ± b`);
//! - the moves `vmovdqu`, the mask extractions to a general register, the
//!   conversion `vcvtps2ph` (its destination in `ModRM.rm`), the masked
//!   loads and stores, and `vzeroupper`.
//!
//! The prefix is `C5` where it can express the instruction, `C4`
//! otherwise, as LLVM's assembler chooses (`crate::inst::vex`). A form
//! that exists in one width only refuses the other (the checked forms).

use crate::inst::{Inst, Rm, VexOp, imm, vex, vex_bare};
use crate::reg::{Mem, Reg, Xmm};

/// The vector width of a VEX instruction.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Width {
    /// 128 bits: `xmm`, `VEX.L` 0.
    X,
    /// 256 bits: `ymm`, `VEX.L` 1.
    Y,
}

impl Width {
    /// Both widths.
    pub const ALL: [Width; 2] = [Width::X, Width::Y];

    const fn l(self) -> bool {
        matches!(self, Width::Y)
    }

    /// The bit of this width in a set of allowed widths.
    const fn bit(self) -> u8 {
        match self {
            Width::X => 1,
            Width::Y => 2,
        }
    }
}

/// Both widths allowed.
const XY: u8 = 3;
/// 256 bits only.
const Y: u8 = 2;
/// 128 bits only.
const X: u8 = 1;

/// `(map, pp, W, opcode, widths)` of a member.
type Fields = (u8, u8, bool, u8, u8);

/// Defines a family: its members with their encoding fields and mnemonic,
/// `ALL`, `mnemonic`, `fields`.
macro_rules! vex_family {
    ($(#[$meta:meta])* $name:ident { $($(#[$m:meta])* $member:ident = ($map:expr, $pp:expr, $w:expr, $op:expr, $widths:expr, $mn:literal),)+ }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub enum $name { $($(#[$m])* $member,)+ }

        impl $name {
            /// Every member.
            pub const ALL: &'static [$name] = &[$($name::$member),+];

            /// The mnemonic, as LLVM prints it.
            pub const fn mnemonic(self) -> &'static str {
                match self { $($name::$member => $mn,)+ }
            }

            /// Whether the instruction exists in width `width`.
            pub const fn allows(self, width: Width) -> bool {
                self.fields().4 & width.bit() != 0
            }

            const fn fields(self) -> Fields {
                match self { $($name::$member => ($map, $pp, $w, $op, $widths),)+ }
            }

            const fn op(self, width: Width) -> VexOp {
                let (map, pp, w, opcode, _) = self.fields();
                VexOp { map, pp, w, l: width.l(), opcode }
            }
        }
    };
}

// pp: 0 none, 1 `66`, 2 `F3`, 3 `F2`; map: 1 `0F`, 2 `0F38`, 3 `0F3A`.
vex_family! {
    /// `op dst, a, b/m`: `a` in `VEX.vvvv`.
    V3 {
        /// `VPSHUFB`: bytes of `a` chosen within each 128-bit lane.
        Pshufb = (2, 1, false, 0x00, XY, "vpshufb"),
        Phaddw = (2, 1, false, 0x01, XY, "vphaddw"),
        Phaddd = (2, 1, false, 0x02, XY, "vphaddd"),
        Phaddsw = (2, 1, false, 0x03, XY, "vphaddsw"),
        Pmaddubsw = (2, 1, false, 0x04, XY, "vpmaddubsw"),
        Phsubw = (2, 1, false, 0x05, XY, "vphsubw"),
        Phsubd = (2, 1, false, 0x06, XY, "vphsubd"),
        Phsubsw = (2, 1, false, 0x07, XY, "vphsubsw"),
        Psignb = (2, 1, false, 0x08, XY, "vpsignb"),
        Psignw = (2, 1, false, 0x09, XY, "vpsignw"),
        Psignd = (2, 1, false, 0x0a, XY, "vpsignd"),
        Pmulhrsw = (2, 1, false, 0x0b, XY, "vpmulhrsw"),
        Permilps = (2, 1, false, 0x0c, XY, "vpermilps"),
        Permilpd = (2, 1, false, 0x0d, XY, "vpermilpd"),
        Permps = (2, 1, false, 0x16, Y, "vpermps"),
        Pmuldq = (2, 1, false, 0x28, XY, "vpmuldq"),
        Packusdw = (2, 1, false, 0x2b, XY, "vpackusdw"),
        Permd = (2, 1, false, 0x36, Y, "vpermd"),
        Psrlvd = (2, 1, false, 0x45, XY, "vpsrlvd"),
        Psrlvq = (2, 1, true, 0x45, XY, "vpsrlvq"),
        Psravd = (2, 1, false, 0x46, XY, "vpsravd"),
        Psllvd = (2, 1, false, 0x47, XY, "vpsllvd"),
        Psllvq = (2, 1, true, 0x47, XY, "vpsllvq"),
        Gf2p8mulb = (2, 1, false, 0xcf, XY, "vgf2p8mulb"),
        Packsswb = (1, 1, false, 0x63, XY, "vpacksswb"),
        Packuswb = (1, 1, false, 0x67, XY, "vpackuswb"),
        Packssdw = (1, 1, false, 0x6b, XY, "vpackssdw"),
        /// The shifts by the count in the low 64 bits of an `xmm` (`b`
        /// is always 128 bits).
        Psrlw = (1, 1, false, 0xd1, XY, "vpsrlw"),
        Psrld = (1, 1, false, 0xd2, XY, "vpsrld"),
        Psrlq = (1, 1, false, 0xd3, XY, "vpsrlq"),
        Pavgb = (1, 1, false, 0xe0, XY, "vpavgb"),
        Psraw = (1, 1, false, 0xe1, XY, "vpsraw"),
        Psrad = (1, 1, false, 0xe2, XY, "vpsrad"),
        Pavgw = (1, 1, false, 0xe3, XY, "vpavgw"),
        Pmulhuw = (1, 1, false, 0xe4, XY, "vpmulhuw"),
        Pmulhw = (1, 1, false, 0xe5, XY, "vpmulhw"),
        Psllw = (1, 1, false, 0xf1, XY, "vpsllw"),
        Pslld = (1, 1, false, 0xf2, XY, "vpslld"),
        Psllq = (1, 1, false, 0xf3, XY, "vpsllq"),
        Pmaddwd = (1, 1, false, 0xf5, XY, "vpmaddwd"),
        Psadbw = (1, 1, false, 0xf6, XY, "vpsadbw"),
        Minps = (1, 0, false, 0x5d, XY, "vminps"),
        Minpd = (1, 1, false, 0x5d, XY, "vminpd"),
        Maxps = (1, 0, false, 0x5f, XY, "vmaxps"),
        Maxpd = (1, 1, false, 0x5f, XY, "vmaxpd"),
        Haddpd = (1, 1, false, 0x7c, XY, "vhaddpd"),
        Haddps = (1, 3, false, 0x7c, XY, "vhaddps"),
        Hsubpd = (1, 1, false, 0x7d, XY, "vhsubpd"),
        Hsubps = (1, 3, false, 0x7d, XY, "vhsubps"),
        Addsubpd = (1, 1, false, 0xd0, XY, "vaddsubpd"),
        Addsubps = (1, 3, false, 0xd0, XY, "vaddsubps"),
    }
}

vex_family! {
    /// `op dst, a, b/m, imm8`: `a` in `VEX.vvvv`.
    V3Imm {
        Perm2f128 = (3, 1, false, 0x06, Y, "vperm2f128"),
        Dpps = (3, 1, false, 0x40, XY, "vdpps"),
        Dppd = (3, 1, false, 0x41, X, "vdppd"),
        Mpsadbw = (3, 1, false, 0x42, XY, "vmpsadbw"),
        /// `VPCLMULQDQ` (256 bits: the VPCLMULQDQ extension).
        Pclmulqdq = (3, 1, false, 0x44, XY, "vpclmulqdq"),
        Perm2i128 = (3, 1, false, 0x46, Y, "vperm2i128"),
        Gf2p8affineqb = (3, 1, true, 0xce, XY, "vgf2p8affineqb"),
        Gf2p8affineinvqb = (3, 1, true, 0xcf, XY, "vgf2p8affineinvqb"),
        /// `VCMPPS`/`VCMPPD`: the predicate (0 to 31) is the immediate.
        Cmpps = (1, 0, false, 0xc2, XY, "vcmpps"),
        Cmppd = (1, 1, false, 0xc2, XY, "vcmppd"),
    }
}

vex_family! {
    /// `op dst, src/m`: `VEX.vvvv` unused.
    V2 {
        Ptest = (2, 1, false, 0x17, XY, "vptest"),
        Testps = (2, 1, false, 0x0e, XY, "vtestps"),
        Testpd = (2, 1, false, 0x0f, XY, "vtestpd"),
        /// `VCVTPH2PS`: the source is 64 or 128 bits (`xmm`).
        Cvtph2ps = (2, 1, false, 0x13, XY, "vcvtph2ps"),
        Rsqrtps = (1, 0, false, 0x52, XY, "vrsqrtps"),
        Rcpps = (1, 0, false, 0x53, XY, "vrcpps"),
        /// `VCVTPD2PS`, `VCVTPD2DQ`, `VCVTTPD2DQ`: the destination is
        /// an `xmm` at both widths.
        Cvtpd2ps = (1, 1, false, 0x5a, XY, "vcvtpd2ps"),
        Cvtps2dq = (1, 1, false, 0x5b, XY, "vcvtps2dq"),
        Cvttps2dq = (1, 2, false, 0x5b, XY, "vcvttps2dq"),
        Cvttpd2dq = (1, 1, false, 0xe6, XY, "vcvttpd2dq"),
        Cvtpd2dq = (1, 3, false, 0xe6, XY, "vcvtpd2dq"),
        /// `VMOVDQU` load.
        Movdqu = (1, 2, false, 0x6f, XY, "vmovdqu"),
    }
}

vex_family! {
    /// `op dst, src/m, imm8`: `VEX.vvvv` unused.
    V2Imm {
        Permq = (3, 1, true, 0x00, Y, "vpermq"),
        Permpd = (3, 1, true, 0x01, Y, "vpermpd"),
        Roundps = (3, 1, false, 0x08, XY, "vroundps"),
        Roundpd = (3, 1, false, 0x09, XY, "vroundpd"),
    }
}

vex_family! {
    /// The variable blends `op dst, a, b/m, mask`: `a` in `VEX.vvvv`, the
    /// mask register in `imm8[7:4]`; each element from `b` where the
    /// mask element's sign bit is set, else from `a`.
    VBlend {
        Blendvps = (3, 1, false, 0x4a, XY, "vblendvps"),
        Blendvpd = (3, 1, false, 0x4b, XY, "vblendvpd"),
        Pblendvb = (3, 1, false, 0x4c, XY, "vpblendvb"),
    }
}

vex_family! {
    /// The FMA3 `213` forms `op dst, a, b/m`: `dst = a·dst ± b` per
    /// element (`a` in `VEX.vvvv`), rounded once.
    Fma {
        Vfmaddsub213ps = (2, 1, false, 0xa6, XY, "vfmaddsub213ps"),
        Vfmaddsub213pd = (2, 1, true, 0xa6, XY, "vfmaddsub213pd"),
        Vfmsubadd213ps = (2, 1, false, 0xa7, XY, "vfmsubadd213ps"),
        Vfmsubadd213pd = (2, 1, true, 0xa7, XY, "vfmsubadd213pd"),
    }
}

vex_family! {
    /// The masked moves `op dst, mask, m` (load) and `op m, mask, src`
    /// (store): elements whose mask sign bit is clear are not accessed.
    VMask {
        Maskmovps = (2, 1, false, 0x2c, XY, "vmaskmovps"),
        Maskmovpd = (2, 1, false, 0x2d, XY, "vmaskmovpd"),
        Pmaskmovd = (2, 1, false, 0x8c, XY, "vpmaskmovd"),
        Pmaskmovq = (2, 1, true, 0x8c, XY, "vpmaskmovq"),
    }
}

/// The shifts by an immediate: `op dst, src, imm8` with `dst` in
/// `VEX.vvvv` and `src` in `ModRM.rm` (`VEX.66.0F 71`/`72`/`73 /digit
/// ib`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VShift {
    Psrlw,
    Psraw,
    Psllw,
    Psrld,
    Psrad,
    Pslld,
    Psrlq,
    Psllq,
    /// `VPSRLDQ`: bytes, within each 128-bit lane.
    Psrldq,
    /// `VPSLLDQ`: bytes, within each 128-bit lane.
    Pslldq,
}

impl VShift {
    /// Every member.
    pub const ALL: [VShift; 10] = [
        VShift::Psrlw,
        VShift::Psraw,
        VShift::Psllw,
        VShift::Psrld,
        VShift::Psrad,
        VShift::Pslld,
        VShift::Psrlq,
        VShift::Psllq,
        VShift::Psrldq,
        VShift::Pslldq,
    ];

    /// The mnemonic, as LLVM prints it.
    pub const fn mnemonic(self) -> &'static str {
        match self {
            VShift::Psrlw => "vpsrlw",
            VShift::Psraw => "vpsraw",
            VShift::Psllw => "vpsllw",
            VShift::Psrld => "vpsrld",
            VShift::Psrad => "vpsrad",
            VShift::Pslld => "vpslld",
            VShift::Psrlq => "vpsrlq",
            VShift::Psllq => "vpsllq",
            VShift::Psrldq => "vpsrldq",
            VShift::Pslldq => "vpslldq",
        }
    }

    /// `(opcode, digit)`.
    const fn code(self) -> (u8, u8) {
        match self {
            VShift::Psrlw => (0x71, 2),
            VShift::Psraw => (0x71, 4),
            VShift::Psllw => (0x71, 6),
            VShift::Psrld => (0x72, 2),
            VShift::Psrad => (0x72, 4),
            VShift::Pslld => (0x72, 6),
            VShift::Psrlq => (0x73, 2),
            VShift::Psllq => (0x73, 6),
            VShift::Psrldq => (0x73, 3),
            VShift::Pslldq => (0x73, 7),
        }
    }
}

const fn reg(x: Xmm) -> Rm {
    Rm::Reg(x.as_gpr())
}

checked! {
    /// `op dst, a, b` of the [`V3`] family.
    pub fn v3_rrr / try_v3_rrr(op: V3, width: Width, dst: Xmm, a: Xmm, b: Xmm)
    requires op.allows(width);
    { vex(op.op(width), dst.code(), a.code(), reg(b)) }

    /// `op dst, a, m` of the [`V3`] family.
    pub fn v3_rrm / try_v3_rrm(op: V3, width: Width, dst: Xmm, a: Xmm, b: Mem)
    requires op.allows(width);
    { vex(op.op(width), dst.code(), a.code(), Rm::Mem(b)) }

    /// `op dst, a, b, imm8` of the [`V3Imm`] family.
    pub fn v3_rrri / try_v3_rrri(op: V3Imm, width: Width, dst: Xmm, a: Xmm, b: Xmm, imm8: u8)
    requires op.allows(width);
    { imm(vex(op.op(width), dst.code(), a.code(), reg(b)), imm8 as i64, 1) }

    /// `op dst, a, m, imm8` of the [`V3Imm`] family.
    pub fn v3_rrmi / try_v3_rrmi(op: V3Imm, width: Width, dst: Xmm, a: Xmm, b: Mem, imm8: u8)
    requires op.allows(width);
    { imm(vex(op.op(width), dst.code(), a.code(), Rm::Mem(b)), imm8 as i64, 1) }

    /// `op dst, src, imm8` of the [`V2Imm`] family.
    pub fn v2_rri / try_v2_rri(op: V2Imm, width: Width, dst: Xmm, src: Xmm, imm8: u8)
    requires op.allows(width);
    { imm(vex(op.op(width), dst.code(), 0, reg(src)), imm8 as i64, 1) }

    /// `op dst, m, imm8` of the [`V2Imm`] family.
    pub fn v2_rmi / try_v2_rmi(op: V2Imm, width: Width, dst: Xmm, src: Mem, imm8: u8)
    requires op.allows(width);
    { imm(vex(op.op(width), dst.code(), 0, Rm::Mem(src)), imm8 as i64, 1) }
}

/// `op dst, src` of the [`V2`] family (every member has both widths).
#[inline]
pub const fn v2_rr(op: V2, width: Width, dst: Xmm, src: Xmm) -> Inst {
    vex(op.op(width), dst.code(), 0, reg(src))
}

/// `op dst, m` of the [`V2`] family.
#[inline]
pub const fn v2_rm(op: V2, width: Width, dst: Xmm, src: Mem) -> Inst {
    vex(op.op(width), dst.code(), 0, Rm::Mem(src))
}

/// `op dst, a, b, mask` of the [`VBlend`] family.
#[inline]
pub const fn vblend_rrrr(op: VBlend, width: Width, dst: Xmm, a: Xmm, b: Xmm, mask: Xmm) -> Inst {
    imm(vex(op.op(width), dst.code(), a.code(), reg(b)), (mask.code() << 4) as i64, 1)
}

/// `op dst, a, m, mask` of the [`VBlend`] family.
#[inline]
pub const fn vblend_rrmr(op: VBlend, width: Width, dst: Xmm, a: Xmm, b: Mem, mask: Xmm) -> Inst {
    imm(vex(op.op(width), dst.code(), a.code(), Rm::Mem(b)), (mask.code() << 4) as i64, 1)
}

/// `op dst, a, b` of the [`Fma`] family.
#[inline]
pub const fn fma_rrr(op: Fma, width: Width, dst: Xmm, a: Xmm, b: Xmm) -> Inst {
    vex(op.op(width), dst.code(), a.code(), reg(b))
}

/// `op dst, a, m` of the [`Fma`] family.
#[inline]
pub const fn fma_rrm(op: Fma, width: Width, dst: Xmm, a: Xmm, b: Mem) -> Inst {
    vex(op.op(width), dst.code(), a.code(), Rm::Mem(b))
}

/// The masked load `op dst, mask, m` of the [`VMask`] family.
#[inline]
pub const fn vmask_load(op: VMask, width: Width, dst: Xmm, mask: Xmm, src: Mem) -> Inst {
    vex(op.op(width), dst.code(), mask.code(), Rm::Mem(src))
}

/// The masked store `op m, mask, src` of the [`VMask`] family (the
/// load's opcode plus 2).
#[inline]
pub const fn vmask_store(op: VMask, width: Width, dst: Mem, mask: Xmm, src: Xmm) -> Inst {
    let load = op.op(width);
    vex(VexOp { opcode: load.opcode + 2, ..load }, src.code(), mask.code(), Rm::Mem(dst))
}

/// `op dst, src, imm8` of the [`VShift`] family.
#[inline]
pub const fn vshift_rri(op: VShift, width: Width, dst: Xmm, src: Xmm, imm8: u8) -> Inst {
    let (opcode, digit) = op.code();
    let op = VexOp { map: 1, pp: 1, w: false, l: width.l(), opcode };
    imm(vex(op, digit, dst.code(), reg(src)), imm8 as i64, 1)
}

/// `VMOVDQU m, src` (`VEX.F3.0F 7F /r`): the store.
#[inline]
pub const fn vmovdqu_mr(width: Width, dst: Mem, src: Xmm) -> Inst {
    vex(VexOp { map: 1, pp: 2, w: false, l: width.l(), opcode: 0x7f }, src.code(), 0, Rm::Mem(dst))
}

/// `VPMOVMSKB r32, src` (`VEX.66.0F D7 /r`): the sign bit of each byte.
#[inline]
pub const fn vpmovmskb_rr(width: Width, dst: Reg, src: Xmm) -> Inst {
    vex(VexOp { map: 1, pp: 1, w: false, l: width.l(), opcode: 0xd7 }, dst.code(), 0, reg(src))
}

/// `VMOVMSKPS`/`VMOVMSKPD r32, src` (`VEX.0F 50 /r`, `VEX.66.0F 50
/// /r`): the sign bit of each element; `double` selects the `pd` form.
#[inline]
pub const fn vmovmskp_rr(double: bool, width: Width, dst: Reg, src: Xmm) -> Inst {
    let pp = if double { 1 } else { 0 };
    vex(VexOp { map: 1, pp, w: false, l: width.l(), opcode: 0x50 }, dst.code(), 0, reg(src))
}

/// `VCVTPS2PH dst, src, imm8` (`VEX.66.0F3A 1D /r ib`, F16C): `dst` (an
/// `xmm`, in `ModRM.rm`) the `half` conversions of `src` (`xmm` for
/// [`Width::X`], `ymm` for [`Width::Y`]), rounded as `imm8` says.
#[inline]
pub const fn vcvtps2ph_rri(width: Width, dst: Xmm, src: Xmm, imm8: u8) -> Inst {
    let op = VexOp { map: 3, pp: 1, w: false, l: width.l(), opcode: 0x1d };
    imm(vex(op, src.code(), 0, reg(dst)), imm8 as i64, 1)
}

/// `VZEROUPPER` (`VEX.128.0F 77`): bits 128 to 255 of every `ymm`
/// cleared.
#[inline]
pub const fn vzeroupper() -> Inst {
    vex_bare(VexOp { map: 1, pp: 0, w: false, l: false, opcode: 0x77 })
}
