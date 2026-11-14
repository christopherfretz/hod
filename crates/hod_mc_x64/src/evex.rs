//! EVEX-encoded instructions (AVX-512 F, VL, BW, DQ, VBMI, VBMI2) and the
//! opmask instructions (WP-208, `spec/mc-x64.md` "EVEX"; design 0007).
//!
//! A vector register is a [`Zmm`] (its number, `0..=31`) and a vector
//! length [`Vl`]: `Zmm(3)` at [`Vl::X`] is `xmm3`. An opmask register is a
//! [`K`]; [`Masking`] says which one masks the destination and whether
//! lanes that are off are zeroed (`{z}`) or keep the destination's value.
//! `k0` as a mask means "no mask".
//!
//! The families, by operand shape:
//!
//! - [`E3`]: `op dst{k}, a, b` (`a` in `EVEX.vvvv`), and [`E3Imm`] with
//!   an `imm8`;
//! - [`E2`]: `op dst{k}, src` (`vvvv` unused), and [`E2Imm`];
//! - [`EShift`]: the shifts by an immediate (`dst` in `vvvv`, a
//!   `/digit`);
//! - [`ECmp`]: an opmask destination (`vfpclassps`/`pd`);
//! - [`EStore`]: `op dst{k}, src` with the destination in `ModRM.rm`
//!   (the narrowing moves, the compresses);
//! - [`EMov`]: the moves `vmovdqu8`..`64`, `vmovups`, `vmovupd`;
//! - the opmask instructions, which are VEX-encoded: [`kmov_kk`] and the
//!   other moves, [`K3`], [`K2`], [`KShift`].
//!
//! Memory operands take the compressed displacement `disp8*N` of the
//! member's tuple type (design 0007, section 1.2); a broadcast form
//! (`_rrb`, `{1toN}`) reads one element. The constructors always write
//! EVEX, also where a VEX form of the instruction exists (LLVM's
//! assembler would choose that one unless told `{evex}`).

use crate::Fixup;
use crate::inst::{Inst, Rm, VexOp, imm, vex};
use crate::reg::{Base, Mem, Reg, Size, Xmm};

/// A vector register of the EVEX space, `0..=31`: `xmm`, `ymm` or `zmm`
/// by the instruction's [`Vl`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Zmm(u8);

impl Zmm {
    /// The register numbered `n`; `None` for `n > 31`.
    #[inline]
    pub const fn new(n: u8) -> Option<Self> {
        if n < 32 { Some(Self(n)) } else { None }
    }

    /// The register of the same number as `x`.
    #[inline]
    pub const fn of(x: Xmm) -> Self {
        Self(x.code())
    }

    /// The 5-bit register number.
    #[inline]
    pub const fn code(self) -> u8 {
        self.0
    }
}

/// The vector length of an EVEX instruction (`EVEX.L'L`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Vl {
    /// 128 bits: `xmm`.
    X,
    /// 256 bits: `ymm`.
    Y,
    /// 512 bits: `zmm`.
    Z,
}

impl Vl {
    /// Every length.
    pub const ALL: [Vl; 3] = [Vl::X, Vl::Y, Vl::Z];

    /// `L'L`.
    const fn ll(self) -> u8 {
        match self {
            Vl::X => 0,
            Vl::Y => 1,
            Vl::Z => 2,
        }
    }

    /// The length in bytes.
    pub const fn bytes(self) -> u8 {
        16 << self.ll()
    }

    const fn bit(self) -> u8 {
        1 << self.ll()
    }
}

/// An opmask register.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(u8)]
pub enum K {
    /// As a mask: no mask.
    K0 = 0,
    K1,
    K2,
    K3,
    K4,
    K5,
    K6,
    K7,
}

impl K {
    /// Every register, in number order.
    pub const ALL: [K; 8] = [K::K0, K::K1, K::K2, K::K3, K::K4, K::K5, K::K6, K::K7];

    /// The register numbered `n`; `None` for `n > 7`.
    #[inline]
    pub const fn new(n: u8) -> Option<Self> {
        if n < 8 { Some(Self::ALL[n as usize]) } else { None }
    }

    /// The 3-bit register number.
    #[inline]
    pub const fn code(self) -> u8 {
        self as u8
    }
}

/// How the destination is masked: `k` (`k0`: not at all) and, for the
/// lanes that are off, zeroing (`{z}`) or merging (they keep the
/// destination's value).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Masking {
    pub k: K,
    pub zero: bool,
}

impl Masking {
    /// No mask.
    pub const NONE: Masking = Masking { k: K::K0, zero: false };

    /// Merging-masking by `k`.
    #[inline]
    pub const fn merge(k: K) -> Self {
        Self { k, zero: false }
    }

    /// Zeroing-masking by `k` (`{z}`; `k0` is not valid).
    #[inline]
    pub const fn zero(k: K) -> Self {
        Self { k, zero: true }
    }

    /// Whether the combination exists: `{z}` needs a mask.
    #[inline]
    pub const fn valid(self) -> bool {
        !(self.zero && matches!(self.k, K::K0))
    }

    /// Whether this masks at all.
    const fn masks(self) -> bool {
        !matches!(self.k, K::K0)
    }
}

/// Embedded rounding (`{rn-sae}`..`{rz-sae}`, which also suppress
/// exceptions) or exception suppression alone (`{sae}`), on a 512-bit
/// register form.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Round {
    /// To nearest even.
    Rn,
    /// Down.
    Rd,
    /// Up.
    Ru,
    /// Toward zero.
    Rz,
    /// Suppress all exceptions, round as `MXCSR` says.
    Sae,
}

impl Round {
    /// Every mode.
    pub const ALL: [Round; 5] = [Round::Rn, Round::Rd, Round::Ru, Round::Rz, Round::Sae];

    /// `L'L` with `EVEX.b`: the rounding control, 0 for `{sae}` (as LLVM's
    /// assembler writes it).
    const fn ll(self) -> u8 {
        match self {
            Round::Rn | Round::Sae => 0,
            Round::Rd => 1,
            Round::Ru => 2,
            Round::Rz => 3,
        }
    }

    const fn is_sae(self) -> bool {
        matches!(self, Round::Sae)
    }
}

/// The tuple type: the `N` of a compressed displacement.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tuple {
    /// The whole vector (FV, FVM): `N` the length in bytes.
    Full,
    /// Half of it (HV, HVM).
    Half,
    /// A quarter (QVM).
    Quarter,
    /// An eighth (OVM).
    Eighth,
    /// 16 bytes whatever the length (M128: a shift count).
    M128,
    /// One element of this many bytes (T1S).
    Elem(u8),
}

// The flags of a member.
/// `{sae}` on a 512-bit register form.
const SAE: u8 = 1;
/// `{rn-sae}`..`{rz-sae}` on a 512-bit register form (not `{sae}`
/// alone).
const ER: u8 = 2;
/// No masking.
const NOMASK: u8 = 4;

// Lengths.
const XYZ: u8 = 7;
const YZ: u8 = 6;
const XO: u8 = 1;

/// The fields of a member: `(map, pp, W, opcode (with a `/digit` in its
/// second byte), lengths, tuple, broadcast element bytes or 0, flags)`.
#[derive(Clone, Copy)]
struct Fields {
    map: u8,
    pp: u8,
    w: bool,
    opcode: u8,
    /// The `/digit` of [`EShift`]; 0 elsewhere.
    digit: u8,
    lengths: u8,
    tuple: Tuple,
    bcst: u8,
    flags: u8,
}

impl Fields {
    const fn allows(self, vl: Vl) -> bool {
        self.lengths & vl.bit() != 0
    }

    const fn masking_ok(self, m: Masking) -> bool {
        m.valid() && !(self.flags & NOMASK != 0 && m.masks())
    }

    /// `{er}` members take the four rounding modes, `{sae}` members
    /// `{sae}` alone.
    const fn round_ok(self, r: Round) -> bool {
        if r.is_sae() { self.flags & SAE != 0 } else { self.flags & ER != 0 }
    }

    /// `N` for a memory operand at length `vl`, broadcast or not.
    const fn n(self, vl: Vl, bcst: bool) -> i32 {
        if bcst {
            return self.bcst as i32;
        }
        let bytes = vl.bytes() as i32;
        match self.tuple {
            Full => bytes,
            Half => bytes / 2,
            Quarter => bytes / 4,
            Eighth => bytes / 8,
            M128 => 16,
            Elem(n) => n as i32,
        }
    }
}

/// Defines a family: its members with their fields and mnemonic, `ALL`,
/// `mnemonic`, `allows`, `broadcasts`, `rounds`, `masks`.
macro_rules! evex_family {
    ($(#[$meta:meta])* $name:ident {
        $($(#[$m:meta])* $member:ident = ($map:expr, $pp:expr, $w:expr, $op:expr, $len:expr,
            $tuple:expr, $bcst:expr, $flags:expr, $mn:literal),)+
    }) => {
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

            /// Whether the instruction exists at length `vl`.
            pub const fn allows(self, vl: Vl) -> bool {
                self.fields().allows(vl)
            }

            /// The bytes of the element a broadcast form reads; 0 when
            /// it has none.
            pub const fn broadcast(self) -> u8 {
                self.fields().bcst
            }

            /// Whether the 512-bit register form takes `round`.
            pub const fn rounds(self, round: Round) -> bool {
                self.fields().round_ok(round)
            }

            /// Whether the destination may be masked.
            pub const fn masks(self) -> bool {
                self.fields().flags & NOMASK == 0
            }

            const fn fields(self) -> Fields {
                match self {
                    $($name::$member => Fields {
                        map: $map, pp: $pp, w: $w,
                        opcode: (($op) as u16 & 0xff) as u8, digit: (($op) as u16 >> 8) as u8,
                        lengths: $len,
                        tuple: $tuple, bcst: $bcst, flags: $flags,
                    },)+
                }
            }
        }
    };
}

use Tuple::{Eighth, Elem, Full, Half, M128, Quarter};

// pp: 0 none, 1 `66`, 2 `F3`, 3 `F2`; map: 1 `0F`, 2 `0F38`, 3 `0F3A`.
evex_family! {
    /// `op dst{k}, a, b`: `a` in `EVEX.vvvv`, `b` a register, memory or
    /// a broadcast element.
    E3 {
        Maxps = (1, 0, false, 0x5f, XYZ, Full, 4, SAE, "vmaxps"),
        Maxpd = (1, 1, true, 0x5f, XYZ, Full, 8, SAE, "vmaxpd"),
        Minps = (1, 0, false, 0x5d, XYZ, Full, 4, SAE, "vminps"),
        Minpd = (1, 1, true, 0x5d, XYZ, Full, 8, SAE, "vminpd"),
        Packssdw = (1, 1, false, 0x6b, XYZ, Full, 4, 0, "vpackssdw"),
        Packsswb = (1, 1, false, 0x63, XYZ, Full, 0, 0, "vpacksswb"),
        Packusdw = (2, 1, false, 0x2b, XYZ, Full, 4, 0, "vpackusdw"),
        Packuswb = (1, 1, false, 0x67, XYZ, Full, 0, 0, "vpackuswb"),
        Pavgb = (1, 1, false, 0xe0, XYZ, Full, 0, 0, "vpavgb"),
        Pavgw = (1, 1, false, 0xe3, XYZ, Full, 0, 0, "vpavgw"),
        Pmulhrsw = (2, 1, false, 0x0b, XYZ, Full, 0, 0, "vpmulhrsw"),
        Pmulhuw = (1, 1, false, 0xe4, XYZ, Full, 0, 0, "vpmulhuw"),
        Pmulhw = (1, 1, false, 0xe5, XYZ, Full, 0, 0, "vpmulhw"),
        /// No masking.
        Psadbw = (1, 1, false, 0xf6, XYZ, Full, 0, NOMASK, "vpsadbw"),
        Pshufb = (2, 1, false, 0x00, XYZ, Full, 0, 0, "vpshufb"),
        Psllvd = (2, 1, false, 0x47, XYZ, Full, 4, 0, "vpsllvd"),
        Psllvq = (2, 1, true, 0x47, XYZ, Full, 8, 0, "vpsllvq"),
        Psllvw = (2, 1, true, 0x12, XYZ, Full, 0, 0, "vpsllvw"),
        Psravd = (2, 1, false, 0x46, XYZ, Full, 4, 0, "vpsravd"),
        Psravq = (2, 1, true, 0x46, XYZ, Full, 8, 0, "vpsravq"),
        Psravw = (2, 1, true, 0x11, XYZ, Full, 0, 0, "vpsravw"),
        Psrlvd = (2, 1, false, 0x45, XYZ, Full, 4, 0, "vpsrlvd"),
        Psrlvq = (2, 1, true, 0x45, XYZ, Full, 8, 0, "vpsrlvq"),
        Psrlvw = (2, 1, true, 0x10, XYZ, Full, 0, 0, "vpsrlvw"),
        /// The shifts by the count in the low 64 bits of an `xmm` or an
        /// m128 (`b` is 128 bits at every length).
        Psllw = (1, 1, false, 0xf1, XYZ, M128, 0, 0, "vpsllw"),
        Pslld = (1, 1, false, 0xf2, XYZ, M128, 0, 0, "vpslld"),
        Psllq = (1, 1, true, 0xf3, XYZ, M128, 0, 0, "vpsllq"),
        Psrlw = (1, 1, false, 0xd1, XYZ, M128, 0, 0, "vpsrlw"),
        Psrld = (1, 1, false, 0xd2, XYZ, M128, 0, 0, "vpsrld"),
        Psrlq = (1, 1, true, 0xd3, XYZ, M128, 0, 0, "vpsrlq"),
        Psraw = (1, 1, false, 0xe1, XYZ, M128, 0, 0, "vpsraw"),
        Psrad = (1, 1, false, 0xe2, XYZ, M128, 0, 0, "vpsrad"),
        Psraq = (1, 1, true, 0xe2, XYZ, M128, 0, 0, "vpsraq"),
        /// `vpermd dst, idx, src`: the index in `vvvv`.
        Permd = (2, 1, false, 0x36, YZ, Full, 4, 0, "vpermd"),
        Permps = (2, 1, false, 0x16, YZ, Full, 4, 0, "vpermps"),
        Permq = (2, 1, true, 0x36, YZ, Full, 8, 0, "vpermq"),
        Permpd = (2, 1, true, 0x16, YZ, Full, 8, 0, "vpermpd"),
        Permw = (2, 1, true, 0x8d, XYZ, Full, 0, 0, "vpermw"),
        Permb = (2, 1, false, 0x8d, XYZ, Full, 0, 0, "vpermb"),
        /// `vpermt2d dst, idx, b`: lanes of `dst` and `b` chosen by `idx`
        /// (`dst` is read).
        Permt2d = (2, 1, false, 0x7e, XYZ, Full, 4, 0, "vpermt2d"),
        Permt2q = (2, 1, true, 0x7e, XYZ, Full, 8, 0, "vpermt2q"),
        Permt2ps = (2, 1, false, 0x7f, XYZ, Full, 4, 0, "vpermt2ps"),
        Permt2pd = (2, 1, true, 0x7f, XYZ, Full, 8, 0, "vpermt2pd"),
        Permt2w = (2, 1, true, 0x7d, XYZ, Full, 0, 0, "vpermt2w"),
        Permt2b = (2, 1, false, 0x7d, XYZ, Full, 0, 0, "vpermt2b"),
        /// `vpermi2d dst, a, b`: lanes of `a` and `b` chosen by `dst`.
        Permi2d = (2, 1, false, 0x76, XYZ, Full, 4, 0, "vpermi2d"),
        Permi2q = (2, 1, true, 0x76, XYZ, Full, 8, 0, "vpermi2q"),
        Permi2ps = (2, 1, false, 0x77, XYZ, Full, 4, 0, "vpermi2ps"),
        Permi2pd = (2, 1, true, 0x77, XYZ, Full, 8, 0, "vpermi2pd"),
        Permi2w = (2, 1, true, 0x75, XYZ, Full, 0, 0, "vpermi2w"),
        Permi2b = (2, 1, false, 0x75, XYZ, Full, 0, 0, "vpermi2b"),
        Pmultishiftqb = (2, 1, true, 0x83, XYZ, Full, 8, 0, "vpmultishiftqb"),
        /// `dst = a·dst ∓ b` per element pair (`dst` is read).
        Vfmaddsub213ps = (2, 1, false, 0xa6, XYZ, Full, 4, ER, "vfmaddsub213ps"),
        Vfmaddsub213pd = (2, 1, true, 0xa6, XYZ, Full, 8, ER, "vfmaddsub213pd"),
        Paddd = (1, 1, false, 0xfe, XYZ, Full, 4, 0, "vpaddd"),
        Paddq = (1, 1, true, 0xd4, XYZ, Full, 8, 0, "vpaddq"),
        Pandd = (1, 1, false, 0xdb, XYZ, Full, 4, 0, "vpandd"),
        Pandq = (1, 1, true, 0xdb, XYZ, Full, 8, 0, "vpandq"),
        /// The scalar approximations: the low element from `b`, the
        /// others `a`'s; 128 bits only.
        Rcp14sd = (2, 1, true, 0x4d, XO, Elem(8), 0, 0, "vrcp14sd"),
        Rsqrt14sd = (2, 1, true, 0x4f, XO, Elem(8), 0, 0, "vrsqrt14sd"),
    }
}

evex_family! {
    /// `op dst{k}, a, b, imm8`: `a` in `EVEX.vvvv`.
    E3Imm {
        Dbpsadbw = (3, 1, false, 0x42, XYZ, Full, 0, 0, "vdbpsadbw"),
        /// The truth table `imm8` of `dst`, `a`, `b` (`dst` is read).
        Pternlogd = (3, 1, false, 0x25, XYZ, Full, 4, 0, "vpternlogd"),
        Pternlogq = (3, 1, true, 0x25, XYZ, Full, 8, 0, "vpternlogq"),
    }
}

evex_family! {
    /// `op dst{k}, src`: `vvvv` unused. For a conversion between element
    /// sizes the length is the wider side's: `vcvtpd2dq ymm, zmm` and
    /// `vcvttps2qq zmm, ymm` are both [`Vl::Z`].
    E2 {
        Getexpps = (2, 1, false, 0x42, XYZ, Full, 4, SAE, "vgetexpps"),
        Getexppd = (2, 1, true, 0x42, XYZ, Full, 8, SAE, "vgetexppd"),
        Rcp14ps = (2, 1, false, 0x4c, XYZ, Full, 4, 0, "vrcp14ps"),
        Rcp14pd = (2, 1, true, 0x4c, XYZ, Full, 8, 0, "vrcp14pd"),
        Rsqrt14ps = (2, 1, false, 0x4e, XYZ, Full, 4, 0, "vrsqrt14ps"),
        Rsqrt14pd = (2, 1, true, 0x4e, XYZ, Full, 8, 0, "vrsqrt14pd"),
        Cvtps2dq = (1, 1, false, 0x5b, XYZ, Full, 4, ER, "vcvtps2dq"),
        Cvttps2dq = (1, 2, false, 0x5b, XYZ, Full, 4, SAE, "vcvttps2dq"),
        Cvttps2udq = (1, 0, false, 0x78, XYZ, Full, 4, SAE, "vcvttps2udq"),
        Cvtpd2qq = (1, 1, true, 0x7b, XYZ, Full, 8, ER, "vcvtpd2qq"),
        Cvttpd2qq = (1, 1, true, 0x7a, XYZ, Full, 8, SAE, "vcvttpd2qq"),
        Cvttpd2uqq = (1, 1, true, 0x78, XYZ, Full, 8, SAE, "vcvttpd2uqq"),
        /// Narrowing: the destination is half the length.
        Cvtpd2dq = (1, 3, true, 0xe6, XYZ, Full, 8, ER, "vcvtpd2dq"),
        Cvtpd2ps = (1, 1, true, 0x5a, XYZ, Full, 8, ER, "vcvtpd2ps"),
        Cvttpd2dq = (1, 1, true, 0xe6, XYZ, Full, 8, SAE, "vcvttpd2dq"),
        Cvttpd2udq = (1, 0, true, 0x78, XYZ, Full, 8, SAE, "vcvttpd2udq"),
        Cvtqq2ps = (1, 0, true, 0x5b, XYZ, Full, 8, ER, "vcvtqq2ps"),
        Cvtuqq2ps = (1, 3, true, 0x7a, XYZ, Full, 8, ER, "vcvtuqq2ps"),
        /// Widening: the source is half the length.
        Cvttps2qq = (1, 1, false, 0x7a, XYZ, Half, 4, SAE, "vcvttps2qq"),
        Cvttps2uqq = (1, 1, false, 0x78, XYZ, Half, 4, SAE, "vcvttps2uqq"),
    }
}

evex_family! {
    /// `op dst{k}, src, imm8`: `vvvv` unused.
    E2Imm {
        Rndscaleps = (3, 1, false, 0x08, XYZ, Full, 4, SAE, "vrndscaleps"),
        Rndscalepd = (3, 1, true, 0x09, XYZ, Full, 8, SAE, "vrndscalepd"),
        Permq = (3, 1, true, 0x00, YZ, Full, 8, 0, "vpermq"),
        Permpd = (3, 1, true, 0x01, YZ, Full, 8, 0, "vpermpd"),
    }
}

evex_family! {
    /// The shifts by an immediate `op dst{k}, src, imm8`: `dst` in
    /// `EVEX.vvvv`, the source in `ModRM.rm`, the operation a `/digit`
    /// (here the `ModRM.reg` value in the opcode field's high byte: the
    /// opcode is `op & 0xff`, the digit `op >> 8`).
    EShift {
        Psrlw = (1, 1, false, 0x71 | 2 << 8, XYZ, Full, 0, 0, "vpsrlw"),
        Psraw = (1, 1, false, 0x71 | 4 << 8, XYZ, Full, 0, 0, "vpsraw"),
        Psllw = (1, 1, false, 0x71 | 6 << 8, XYZ, Full, 0, 0, "vpsllw"),
        Psrld = (1, 1, false, 0x72 | 2 << 8, XYZ, Full, 4, 0, "vpsrld"),
        Psrad = (1, 1, false, 0x72 | 4 << 8, XYZ, Full, 4, 0, "vpsrad"),
        Pslld = (1, 1, false, 0x72 | 6 << 8, XYZ, Full, 4, 0, "vpslld"),
        Psraq = (1, 1, true, 0x72 | 4 << 8, XYZ, Full, 8, 0, "vpsraq"),
        Psrlq = (1, 1, true, 0x73 | 2 << 8, XYZ, Full, 8, 0, "vpsrlq"),
        Psllq = (1, 1, true, 0x73 | 6 << 8, XYZ, Full, 8, 0, "vpsllq"),
    }
}

evex_family! {
    /// `op k{k}, src, imm8`: an opmask destination (masked by another
    /// opmask: lanes off are 0).
    ECmp {
        Fpclassps = (3, 1, false, 0x66, XYZ, Full, 4, 0, "vfpclassps"),
        Fpclasspd = (3, 1, true, 0x66, XYZ, Full, 8, 0, "vfpclasspd"),
    }
}

evex_family! {
    /// `op dst{k}, src` with `dst` (a register or memory) in `ModRM.rm`
    /// and `src` in `ModRM.reg`. The length is the source's. A store is
    /// merging only.
    EStore {
        /// Narrowing with signed saturation: 64-bit lanes to bytes.
        Pmovsqb = (2, 2, false, 0x22, XYZ, Eighth, 0, 0, "vpmovsqb"),
        Pmovsqw = (2, 2, false, 0x24, XYZ, Quarter, 0, 0, "vpmovsqw"),
        Pmovsqd = (2, 2, false, 0x25, XYZ, Half, 0, 0, "vpmovsqd"),
        /// Narrowing with unsigned saturation.
        Pmovusdb = (2, 2, false, 0x11, XYZ, Quarter, 0, 0, "vpmovusdb"),
        Pmovusqb = (2, 2, false, 0x12, XYZ, Eighth, 0, 0, "vpmovusqb"),
        Pmovusqw = (2, 2, false, 0x14, XYZ, Quarter, 0, 0, "vpmovusqw"),
        Pmovusqd = (2, 2, false, 0x15, XYZ, Half, 0, 0, "vpmovusqd"),
        /// The lanes whose mask bit is set, packed to the low end.
        Pcompressd = (2, 1, false, 0x8b, XYZ, Elem(4), 0, 0, "vpcompressd"),
        Pcompressq = (2, 1, true, 0x8b, XYZ, Elem(8), 0, 0, "vpcompressq"),
        Compressps = (2, 1, false, 0x8a, XYZ, Elem(4), 0, 0, "vcompressps"),
        Compresspd = (2, 1, true, 0x8a, XYZ, Elem(8), 0, 0, "vcompresspd"),
        Pcompressb = (2, 1, false, 0x63, XYZ, Elem(1), 0, 0, "vpcompressb"),
        Pcompressw = (2, 1, true, 0x63, XYZ, Elem(2), 0, 0, "vpcompressw"),
    }
}

evex_family! {
    /// The unaligned moves: load `op dst{k}, src/m`, store `op m{k}, src`
    /// (the opcode plus 1 or `10`'s `11`).
    EMov {
        Movdqu8 = (1, 3, false, 0x6f, XYZ, Full, 0, 0, "vmovdqu8"),
        Movdqu16 = (1, 3, true, 0x6f, XYZ, Full, 0, 0, "vmovdqu16"),
        Movdqu32 = (1, 2, false, 0x6f, XYZ, Full, 0, 0, "vmovdqu32"),
        Movdqu64 = (1, 2, true, 0x6f, XYZ, Full, 0, 0, "vmovdqu64"),
        Movups = (1, 0, false, 0x10, XYZ, Full, 0, 0, "vmovups"),
        Movupd = (1, 1, true, 0x10, XYZ, Full, 0, 0, "vmovupd"),
    }
}

impl EMov {
    /// The store's opcode: `6F` → `7F`, `10` → `11`.
    const fn store_opcode(self) -> u8 {
        let op = self.fields().opcode;
        if op == 0x6f { 0x7f } else { 0x11 }
    }
}

/// The register-or-memory operand of an EVEX instruction.
#[derive(Clone, Copy)]
enum ERm {
    Reg(u8),
    Mem(Mem),
}

/// The fields of one instruction besides the family member's.
#[derive(Clone, Copy)]
struct Operands {
    /// `L'L`, or the rounding control when `b` is set on a register form.
    ll: u8,
    masking: Masking,
    /// `ModRM.reg`: a register number `0..=31` or a `/digit`.
    reg: u8,
    /// The second source, `0..=31`; 0 when unused (`vvvv` `1111`, `V'`
    /// 1).
    vvvv: u8,
    rm: ERm,
    /// `EVEX.b`: broadcast with memory, rounding with a register.
    b: bool,
    /// `N` of the compressed displacement.
    n: i32,
}

/// An EVEX instruction (`spec/mc-x64.md` "EVEX").
const fn evex(f: Fields, opcode: u8, o: Operands) -> Inst {
    let mut out = Inst::EMPTY;
    let r_hi = o.reg >> 3 & 1;
    let r_top = o.reg >> 4 & 1;
    let (x_hi, b_hi) = match o.rm {
        ERm::Reg(rm) => (rm >> 4 & 1, rm >> 3 & 1),
        ERm::Mem(m) => {
            if let Some(seg) = m.seg() {
                out.push(seg.prefix());
            }
            let index = match m.get_index() {
                Some((i, _)) => i.reg().high(),
                None => 0,
            };
            let base = match m.get_base() {
                Base::Reg(base) => base.high(),
                _ => 0,
            };
            (index, base)
        }
    };
    out.push(0x62);
    out.push((r_hi ^ 1) << 7 | (x_hi ^ 1) << 6 | (b_hi ^ 1) << 5 | (r_top ^ 1) << 4 | f.map);
    out.push(if f.w { 0x80 } else { 0 } | (!o.vvvv & 15) << 3 | 4 | f.pp);
    out.push(
        if o.masking.zero { 0x80 } else { 0 }
            | o.ll << 5
            | if o.b { 0x10 } else { 0 }
            | ((o.vvvv >> 4 & 1) ^ 1) << 3
            | o.masking.k.code(),
    );
    out.push(opcode);
    match o.rm {
        ERm::Reg(rm) => out.push(0xc0 | (o.reg & 7) << 3 | (rm & 7)),
        ERm::Mem(m) => mem_operand(&mut out, o.reg & 7, m, o.n),
    }
    out
}

/// `ModRM`, `SIB` and displacement of an EVEX memory operand: as
/// `inst.rs`'s routine, but a displacement that is a multiple of `n`
/// whose quotient fits in a byte is written as that quotient, and any
/// other one that is not 0 (or 0 with an `rbp`/`r13` base) as `disp32`.
const fn mem_operand(out: &mut Inst, reg: u8, m: Mem, n: i32) {
    let disp = m.disp();
    let reg = reg << 3;
    match (m.get_base(), m.get_index()) {
        (Base::Rip, _) => {
            out.push(reg | 0b101);
            out.mark_site(Fixup::Rel32);
            out.push_imm(disp as u32 as u64, 4);
        }
        (Base::None, index) => {
            out.push(reg | 0b100);
            let sib = match index {
                Some((i, s)) => (s as u8) << 6 | i.reg().low() << 3 | 0b101,
                None => 0b00_100_101,
            };
            out.push(sib);
            if m.is_abs32() {
                out.mark_site(Fixup::Abs32);
            }
            out.push_imm(disp as u32 as u64, 4);
        }
        (Base::Reg(base), index) => {
            let short = disp % n == 0 && disp / n >= -128 && disp / n <= 127;
            let md = if m.is_abs32() {
                0b10
            } else if disp == 0 && base.low() != 0b101 {
                0b00
            } else if short {
                0b01
            } else {
                0b10
            };
            match index {
                Some((i, s)) => {
                    out.push(md << 6 | reg | 0b100);
                    out.push((s as u8) << 6 | i.reg().low() << 3 | base.low());
                }
                None if base.low() == 0b100 => {
                    out.push(md << 6 | reg | 0b100);
                    out.push(0b00_100_100);
                }
                None => out.push(md << 6 | reg | base.low()),
            }
            match md {
                0b01 => out.push((disp / n) as u8),
                0b10 => {
                    if m.is_abs32() {
                        out.mark_site(Fixup::Abs32);
                    }
                    out.push_imm(disp as u32 as u64, 4);
                }
                _ => {}
            }
        }
    }
}

/// A register form at length `vl`.
const fn rr(vl: Vl, masking: Masking, reg: u8, vvvv: u8, rm: u8) -> Operands {
    Operands { ll: vl.ll(), masking, reg, vvvv, rm: ERm::Reg(rm), b: false, n: 1 }
}

/// A 512-bit register form with `round`.
const fn rr_round(masking: Masking, reg: u8, vvvv: u8, rm: u8, round: Round) -> Operands {
    Operands { ll: round.ll(), masking, reg, vvvv, rm: ERm::Reg(rm), b: true, n: 1 }
}

/// A memory form at length `vl`, broadcast or not.
const fn rm(f: Fields, vl: Vl, masking: Masking, reg: u8, vvvv: u8, m: Mem, b: bool) -> Operands {
    Operands { ll: vl.ll(), masking, reg, vvvv, rm: ERm::Mem(m), b, n: f.n(vl, b) }
}

checked! {
    /// `op dst{k}, a, b` of the [`E3`] family.
    pub fn e3_rrr / try_e3_rrr(op: E3, vl: Vl, masking: Masking, dst: Zmm, a: Zmm, b: Zmm)
    requires op.allows(vl) && op.fields().masking_ok(masking);
    { evex(op.fields(), op.fields().opcode, rr(vl, masking, dst.0, a.0, b.0)) }

    /// `op dst{k}, a, m` of the [`E3`] family.
    pub fn e3_rrm / try_e3_rrm(op: E3, vl: Vl, masking: Masking, dst: Zmm, a: Zmm, b: Mem)
    requires op.allows(vl) && op.fields().masking_ok(masking);
    { let f = op.fields(); evex(f, f.opcode, rm(f, vl, masking, dst.0, a.0, b, false)) }

    /// `op dst{k}, a, m{1toN}` of the [`E3`] family: one element of `m`
    /// repeated.
    pub fn e3_rrb / try_e3_rrb(op: E3, vl: Vl, masking: Masking, dst: Zmm, a: Zmm, b: Mem)
    requires op.allows(vl) && op.fields().masking_ok(masking) && op.broadcast() != 0;
    { let f = op.fields(); evex(f, f.opcode, rm(f, vl, masking, dst.0, a.0, b, true)) }

    /// `op dst{k}, a, b, {round}` of the [`E3`] family, 512 bits.
    pub fn e3_rrr_round / try_e3_rrr_round(op: E3, masking: Masking, dst: Zmm, a: Zmm, b: Zmm, round: Round)
    requires op.allows(Vl::Z) && op.fields().masking_ok(masking) && op.rounds(round);
    { evex(op.fields(), op.fields().opcode, rr_round(masking, dst.0, a.0, b.0, round)) }

    /// `op dst{k}, a, b, imm8` of the [`E3Imm`] family.
    pub fn e3_rrri / try_e3_rrri(op: E3Imm, vl: Vl, masking: Masking, dst: Zmm, a: Zmm, b: Zmm, imm8: u8)
    requires op.allows(vl) && op.fields().masking_ok(masking);
    { imm(evex(op.fields(), op.fields().opcode, rr(vl, masking, dst.0, a.0, b.0)), imm8 as i64, 1) }

    /// `op dst{k}, a, m, imm8` of the [`E3Imm`] family.
    pub fn e3_rrmi / try_e3_rrmi(op: E3Imm, vl: Vl, masking: Masking, dst: Zmm, a: Zmm, b: Mem, imm8: u8)
    requires op.allows(vl) && op.fields().masking_ok(masking);
    {
        let f = op.fields();
        imm(evex(f, f.opcode, rm(f, vl, masking, dst.0, a.0, b, false)), imm8 as i64, 1)
    }

    /// `op dst{k}, a, m{1toN}, imm8` of the [`E3Imm`] family.
    pub fn e3_rrbi / try_e3_rrbi(op: E3Imm, vl: Vl, masking: Masking, dst: Zmm, a: Zmm, b: Mem, imm8: u8)
    requires op.allows(vl) && op.fields().masking_ok(masking) && op.broadcast() != 0;
    {
        let f = op.fields();
        imm(evex(f, f.opcode, rm(f, vl, masking, dst.0, a.0, b, true)), imm8 as i64, 1)
    }

    /// `op dst{k}, src` of the [`E2`] family.
    pub fn e2_rr / try_e2_rr(op: E2, vl: Vl, masking: Masking, dst: Zmm, src: Zmm)
    requires op.allows(vl) && op.fields().masking_ok(masking);
    { evex(op.fields(), op.fields().opcode, rr(vl, masking, dst.0, 0, src.0)) }

    /// `op dst{k}, m` of the [`E2`] family.
    pub fn e2_rm / try_e2_rm(op: E2, vl: Vl, masking: Masking, dst: Zmm, src: Mem)
    requires op.allows(vl) && op.fields().masking_ok(masking);
    { let f = op.fields(); evex(f, f.opcode, rm(f, vl, masking, dst.0, 0, src, false)) }

    /// `op dst{k}, m{1toN}` of the [`E2`] family.
    pub fn e2_rb / try_e2_rb(op: E2, vl: Vl, masking: Masking, dst: Zmm, src: Mem)
    requires op.allows(vl) && op.fields().masking_ok(masking) && op.broadcast() != 0;
    { let f = op.fields(); evex(f, f.opcode, rm(f, vl, masking, dst.0, 0, src, true)) }

    /// `op dst{k}, src, {round}` of the [`E2`] family, 512 bits.
    pub fn e2_rr_round / try_e2_rr_round(op: E2, masking: Masking, dst: Zmm, src: Zmm, round: Round)
    requires op.allows(Vl::Z) && op.fields().masking_ok(masking) && op.rounds(round);
    { evex(op.fields(), op.fields().opcode, rr_round(masking, dst.0, 0, src.0, round)) }

    /// `op dst{k}, src, imm8` of the [`E2Imm`] family.
    pub fn e2_rri / try_e2_rri(op: E2Imm, vl: Vl, masking: Masking, dst: Zmm, src: Zmm, imm8: u8)
    requires op.allows(vl) && op.fields().masking_ok(masking);
    { imm(evex(op.fields(), op.fields().opcode, rr(vl, masking, dst.0, 0, src.0)), imm8 as i64, 1) }

    /// `op dst{k}, m, imm8` of the [`E2Imm`] family.
    pub fn e2_rmi / try_e2_rmi(op: E2Imm, vl: Vl, masking: Masking, dst: Zmm, src: Mem, imm8: u8)
    requires op.allows(vl) && op.fields().masking_ok(masking);
    {
        let f = op.fields();
        imm(evex(f, f.opcode, rm(f, vl, masking, dst.0, 0, src, false)), imm8 as i64, 1)
    }

    /// `op dst{k}, m{1toN}, imm8` of the [`E2Imm`] family.
    pub fn e2_rbi / try_e2_rbi(op: E2Imm, vl: Vl, masking: Masking, dst: Zmm, src: Mem, imm8: u8)
    requires op.allows(vl) && op.fields().masking_ok(masking) && op.broadcast() != 0;
    {
        let f = op.fields();
        imm(evex(f, f.opcode, rm(f, vl, masking, dst.0, 0, src, true)), imm8 as i64, 1)
    }

    /// `op dst{k}, src, {sae}, imm8` of the [`E2Imm`] family, 512 bits.
    pub fn e2_rri_round / try_e2_rri_round(op: E2Imm, masking: Masking, dst: Zmm, src: Zmm, round: Round, imm8: u8)
    requires op.allows(Vl::Z) && op.fields().masking_ok(masking) && op.rounds(round);
    {
        imm(evex(op.fields(), op.fields().opcode, rr_round(masking, dst.0, 0, src.0, round)), imm8 as i64, 1)
    }

    /// `op dst{k}, src` of the [`EStore`] family, a register destination.
    pub fn estore_rr / try_estore_rr(op: EStore, vl: Vl, masking: Masking, dst: Zmm, src: Zmm)
    requires op.allows(vl) && op.fields().masking_ok(masking);
    { evex(op.fields(), op.fields().opcode, rr(vl, masking, src.0, 0, dst.0)) }

    /// The load form `op dst{k}, src` of the [`EMov`] family.
    pub fn emov_rr / try_emov_rr(op: EMov, vl: Vl, masking: Masking, dst: Zmm, src: Zmm)
    requires masking.valid();
    { evex(op.fields(), op.fields().opcode, rr(vl, masking, dst.0, 0, src.0)) }

    /// `op dst{k}, m` of the [`EMov`] family: a load.
    pub fn emov_rm / try_emov_rm(op: EMov, vl: Vl, masking: Masking, dst: Zmm, src: Mem)
    requires masking.valid();
    { let f = op.fields(); evex(f, f.opcode, rm(f, vl, masking, dst.0, 0, src, false)) }

    /// `op dst{k}, src, imm8` of the [`EShift`] family: `dst` in
    /// `EVEX.vvvv`.
    pub fn eshift_rri / try_eshift_rri(op: EShift, vl: Vl, masking: Masking, dst: Zmm, src: Zmm, imm8: u8)
    requires op.allows(vl) && masking.valid();
    {
        let (opcode, digit) = op.code();
        imm(evex(op.fields(), opcode, rr(vl, masking, digit, dst.0, src.0)), imm8 as i64, 1)
    }

    /// `op dst{k}, m, imm8` of the [`EShift`] family.
    pub fn eshift_rmi / try_eshift_rmi(op: EShift, vl: Vl, masking: Masking, dst: Zmm, src: Mem, imm8: u8)
    requires op.allows(vl) && masking.valid();
    {
        let (opcode, digit) = op.code();
        let f = op.fields();
        imm(evex(f, opcode, rm(f, vl, masking, digit, dst.0, src, false)), imm8 as i64, 1)
    }

    /// `op dst{k}, m{1toN}, imm8` of the [`EShift`] family.
    pub fn eshift_rbi / try_eshift_rbi(op: EShift, vl: Vl, masking: Masking, dst: Zmm, src: Mem, imm8: u8)
    requires op.allows(vl) && masking.valid() && op.broadcast() != 0;
    {
        let (opcode, digit) = op.code();
        let f = op.fields();
        imm(evex(f, opcode, rm(f, vl, masking, digit, dst.0, src, true)), imm8 as i64, 1)
    }

    /// `VCVTPS2PH dst{k}, src, imm8` (`EVEX.66.0F3A.W0 1D /r ib`): `dst`
    /// (half the length, in `ModRM.rm`) the `half` conversions of `src`.
    pub fn evcvtps2ph_rri / try_evcvtps2ph_rri(vl: Vl, masking: Masking, dst: Zmm, src: Zmm, imm8: u8)
    requires masking.valid();
    { imm(evex(CVTPS2PH, 0x1d, rr(vl, masking, src.0, 0, dst.0)), imm8 as i64, 1) }
}

/// `VCVTPS2PH`'s fields: half-vector memory.
const CVTPS2PH: Fields = Fields {
    map: 3,
    pp: 1,
    w: false,
    opcode: 0x1d,
    digit: 0,
    lengths: XYZ,
    tuple: Half,
    bcst: 0,
    flags: 0,
};

impl EShift {
    /// `(opcode, digit)`.
    const fn code(self) -> (u8, u8) {
        let f = self.fields();
        (f.opcode, f.digit)
    }
}

/// `op k{k}, src, imm8` of the [`ECmp`] family (every member has every
/// length).
#[inline]
pub const fn ecmp_kri(op: ECmp, vl: Vl, mask: K, dst: K, src: Zmm, imm8: u8) -> Inst {
    let o = rr(vl, Masking::merge(mask), dst.code(), 0, src.0);
    imm(evex(op.fields(), op.fields().opcode, o), imm8 as i64, 1)
}

/// `op k{k}, m, imm8` of the [`ECmp`] family.
#[inline]
pub const fn ecmp_kmi(op: ECmp, vl: Vl, mask: K, dst: K, src: Mem, imm8: u8) -> Inst {
    let f = op.fields();
    imm(
        evex(f, f.opcode, rm(f, vl, Masking::merge(mask), dst.code(), 0, src, false)),
        imm8 as i64,
        1,
    )
}

/// `op k{k}, m{1toN}, imm8` of the [`ECmp`] family (every member
/// broadcasts).
#[inline]
pub const fn ecmp_kbi(op: ECmp, vl: Vl, mask: K, dst: K, src: Mem, imm8: u8) -> Inst {
    let f = op.fields();
    imm(
        evex(f, f.opcode, rm(f, vl, Masking::merge(mask), dst.code(), 0, src, true)),
        imm8 as i64,
        1,
    )
}

/// `op m{k}, src` of the [`EStore`] family (merging only; every member
/// has every length).
#[inline]
pub const fn estore_mr(op: EStore, vl: Vl, mask: K, dst: Mem, src: Zmm) -> Inst {
    let f = op.fields();
    evex(f, f.opcode, rm(f, vl, Masking::merge(mask), src.0, 0, dst, false))
}

/// The store `op m{k}, src` of the [`EMov`] family (merging only).
#[inline]
pub const fn emov_mr(op: EMov, vl: Vl, mask: K, dst: Mem, src: Zmm) -> Inst {
    let f = op.fields();
    evex(f, op.store_opcode(), rm(f, vl, Masking::merge(mask), src.0, 0, dst, false))
}

/// `VCVTPS2PH m{k}, src, imm8`: the store form (merging only).
#[inline]
pub const fn evcvtps2ph_mri(vl: Vl, mask: K, dst: Mem, src: Zmm, imm8: u8) -> Inst {
    let o = rm(CVTPS2PH, vl, Masking::merge(mask), src.0, 0, dst, false);
    imm(evex(CVTPS2PH, 0x1d, o), imm8 as i64, 1)
}

// The opmask instructions (VEX-encoded, `L` 0 but where said).

/// `(pp, W)` of a `kmov` between opmask registers or with memory (`90`,
/// `91`), and of the two-operand forms: `b` `66` W0, `w` none W0, `d`
/// `66` W1, `q` none W1.
const fn k_pw(size: Size) -> (u8, bool) {
    match size {
        Size::B => (1, false),
        Size::W => (0, false),
        Size::D => (1, true),
        Size::Q => (0, true),
    }
}

/// `(pp, W)` of a `kmov` with a general register (`92`, `93`): `b` `66`
/// W0, `w` none W0, `d` `F2` W0, `q` `F2` W1.
const fn kr_pw(size: Size) -> (u8, bool) {
    match size {
        Size::B => (1, false),
        Size::W => (0, false),
        Size::D => (3, false),
        Size::Q => (3, true),
    }
}

const fn kop(pw: (u8, bool), map: u8, l: bool, opcode: u8) -> VexOp {
    VexOp { map, pp: pw.0, w: pw.1, l, opcode }
}

const fn kreg(k: K) -> Rm {
    Rm::Reg(Reg::ALL[k as usize])
}

/// `KMOV{B,W,D,Q} dst, src` between opmask registers (`0F 90 /r`).
#[inline]
pub const fn kmov_kk(size: Size, dst: K, src: K) -> Inst {
    vex(kop(k_pw(size), 1, false, 0x90), dst.code(), 0, kreg(src))
}

/// `KMOV{B,W,D,Q} dst, m` (`0F 90 /r`): a load of the mask's bytes.
#[inline]
pub const fn kmov_km(size: Size, dst: K, src: Mem) -> Inst {
    vex(kop(k_pw(size), 1, false, 0x90), dst.code(), 0, Rm::Mem(src))
}

/// `KMOV{B,W,D,Q} m, src` (`0F 91 /r`).
#[inline]
pub const fn kmov_mk(size: Size, dst: Mem, src: K) -> Inst {
    vex(kop(k_pw(size), 1, false, 0x91), src.code(), 0, Rm::Mem(dst))
}

/// `KMOV{B,W,D,Q} k, r32/r64` (`0F 92 /r`): `Q` reads a 64-bit register,
/// the others a 32-bit one.
#[inline]
pub const fn kmov_kr(size: Size, dst: K, src: Reg) -> Inst {
    vex(kop(kr_pw(size), 1, false, 0x92), dst.code(), 0, Rm::Reg(src))
}

/// `KMOV{B,W,D,Q} r32/r64, k` (`0F 93 /r`): zero-extended.
#[inline]
pub const fn kmov_rk(size: Size, dst: Reg, src: K) -> Inst {
    vex(kop(kr_pw(size), 1, false, 0x93), dst.code(), 0, kreg(src))
}

/// The three-operand opmask operations `op dst, a, b` (`VEX.L1`, `a` in
/// `vvvv`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum K3 {
    And,
    Andn,
    Or,
    Xnor,
    Xor,
    Add,
    /// `KUNPCKBW`/`WD`/`DQ` at sizes `W`/`D`/`Q`: the low halves of `b`
    /// (low) and `a` (high) joined.
    Unpck,
}

impl K3 {
    /// Every member.
    pub const ALL: [K3; 7] = [K3::And, K3::Andn, K3::Or, K3::Xnor, K3::Xor, K3::Add, K3::Unpck];

    /// The mnemonic at `size`, as LLVM prints it; `None` where the
    /// operation does not exist (`kunpck` of bytes).
    pub const fn mnemonic(self, size: Size) -> Option<&'static str> {
        let s = match (self, size) {
            (K3::Unpck, Size::B) => return None,
            (K3::Unpck, Size::W) => "kunpckbw",
            (K3::Unpck, Size::D) => "kunpckwd",
            (K3::Unpck, Size::Q) => "kunpckdq",
            (K3::And, Size::B) => "kandb",
            (K3::And, Size::W) => "kandw",
            (K3::And, Size::D) => "kandd",
            (K3::And, Size::Q) => "kandq",
            (K3::Andn, Size::B) => "kandnb",
            (K3::Andn, Size::W) => "kandnw",
            (K3::Andn, Size::D) => "kandnd",
            (K3::Andn, Size::Q) => "kandnq",
            (K3::Or, Size::B) => "korb",
            (K3::Or, Size::W) => "korw",
            (K3::Or, Size::D) => "kord",
            (K3::Or, Size::Q) => "korq",
            (K3::Xnor, Size::B) => "kxnorb",
            (K3::Xnor, Size::W) => "kxnorw",
            (K3::Xnor, Size::D) => "kxnord",
            (K3::Xnor, Size::Q) => "kxnorq",
            (K3::Xor, Size::B) => "kxorb",
            (K3::Xor, Size::W) => "kxorw",
            (K3::Xor, Size::D) => "kxord",
            (K3::Xor, Size::Q) => "kxorq",
            (K3::Add, Size::B) => "kaddb",
            (K3::Add, Size::W) => "kaddw",
            (K3::Add, Size::D) => "kaddd",
            (K3::Add, Size::Q) => "kaddq",
        };
        Some(s)
    }

    const fn opcode(self) -> u8 {
        match self {
            K3::And => 0x41,
            K3::Andn => 0x42,
            K3::Or => 0x45,
            K3::Xnor => 0x46,
            K3::Xor => 0x47,
            K3::Add => 0x4a,
            K3::Unpck => 0x4b,
        }
    }

    /// `(pp, W)`: `kunpckbw` `66` W0, `wd` none W0, `dq` none W1.
    const fn pw(self, size: Size) -> (u8, bool) {
        match (self, size) {
            (K3::Unpck, Size::W) => (1, false),
            (K3::Unpck, Size::D) => (0, false),
            (K3::Unpck, _) => (0, true),
            _ => k_pw(size),
        }
    }
}

/// The two-operand opmask operations `op a, b` (`VEX.L0`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum K2 {
    /// `KNOT dst, src`.
    Not,
    /// `KORTEST a, b`: `ZF` when `a | b` is 0, `CF` when it is all ones.
    Ortest,
    /// `KTEST a, b`: `ZF` when `a & b` is 0, `CF` when `~a & b` is.
    Test,
}

impl K2 {
    /// Every member.
    pub const ALL: [K2; 3] = [K2::Not, K2::Ortest, K2::Test];

    /// The mnemonic's stem; the size letter follows.
    pub const fn stem(self) -> &'static str {
        match self {
            K2::Not => "knot",
            K2::Ortest => "kortest",
            K2::Test => "ktest",
        }
    }

    const fn opcode(self) -> u8 {
        match self {
            K2::Not => 0x44,
            K2::Ortest => 0x98,
            K2::Test => 0x99,
        }
    }
}

/// The opmask shifts `op dst, src, imm8` (`VEX.L0.66.0F3A`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KShift {
    Left,
    Right,
}

impl KShift {
    /// Both members.
    pub const ALL: [KShift; 2] = [KShift::Left, KShift::Right];

    /// The mnemonic's stem; the size letter follows.
    pub const fn stem(self) -> &'static str {
        match self {
            KShift::Left => "kshiftl",
            KShift::Right => "kshiftr",
        }
    }
}

checked! {
    /// `op dst, a, b` of the [`K3`] family (`kunpck` has no byte form).
    pub fn k3_kkk / try_k3_kkk(op: K3, size: Size, dst: K, a: K, b: K)
    requires op.mnemonic(size).is_some();
    { vex(kop(op.pw(size), 1, true, op.opcode()), dst.code(), a.code(), kreg(b)) }
}

/// `op a, b` of the [`K2`] family.
#[inline]
pub const fn k2_kk(op: K2, size: Size, a: K, b: K) -> Inst {
    vex(kop(k_pw(size), 1, false, op.opcode()), a.code(), 0, kreg(b))
}

/// `KSHIFTL`/`KSHIFTR{B,W,D,Q} dst, src, imm8`: `30` (right) and `32`
/// (left) with W0 for `b`, W1 for `w`; `31` and `33` for `d` (W0) and
/// `q` (W1).
#[inline]
pub const fn kshift_kki(op: KShift, size: Size, dst: K, src: K, imm8: u8) -> Inst {
    let base = match op {
        KShift::Right => 0x30,
        KShift::Left => 0x32,
    };
    let (opcode, w) = match size {
        Size::B => (base, false),
        Size::W => (base, true),
        Size::D => (base + 1, false),
        Size::Q => (base + 1, true),
    };
    let o = VexOp { map: 3, pp: 1, w, l: false, opcode };
    imm(vex(o, dst.code(), 0, kreg(src)), imm8 as i64, 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn z(n: u8) -> Zmm {
        Zmm::new(n).unwrap()
    }

    // Bytes measured with `llvm-mc` 19 and 22 (design 0007, section 1.1).
    #[test]
    fn the_measured_examples() {
        let m = |d| Mem::base(Reg::Rax, d);
        assert_eq!(
            e3_rrm(E3::Paddd, Vl::Z, Masking::NONE, z(0), z(1), m(64)).bytes(),
            [0x62, 0xf1, 0x75, 0x48, 0xfe, 0x40, 0x01]
        );
        assert_eq!(
            e3_rrm(E3::Paddd, Vl::Z, Masking::NONE, z(0), z(1), m(32)).bytes(),
            [0x62, 0xf1, 0x75, 0x48, 0xfe, 0x80, 0x20, 0, 0, 0]
        );
        assert_eq!(
            e3_rrb(E3::Paddd, Vl::Z, Masking::zero(K::K1), z(0), z(1), m(4)).bytes(),
            [0x62, 0xf1, 0x75, 0xd9, 0xfe, 0x40, 0x01]
        );
        assert_eq!(
            e3_rrr(E3::Paddd, Vl::Z, Masking::NONE, z(31), z(30), z(29)).bytes(),
            [0x62, 0x01, 0x0d, 0x40, 0xfe, 0xfd]
        );
        assert_eq!(
            emov_rm(EMov::Movdqu64, Vl::Z, Masking::NONE, z(16), Mem::base(Reg::Rsp, 128)).bytes(),
            [0x62, 0xe1, 0xfe, 0x48, 0x6f, 0x44, 0x24, 0x02]
        );
        assert_eq!(
            estore_mr(EStore::Pmovusqb, Vl::Z, K::K0, m(8), z(1)).bytes(),
            [0x62, 0xf2, 0x7e, 0x48, 0x12, 0x48, 0x01]
        );
        assert_eq!(
            e2_rr_round(E2::Cvttpd2qq, Masking::NONE, z(0), z(1), Round::Sae).bytes(),
            [0x62, 0xf1, 0xfd, 0x18, 0x7a, 0xc1]
        );
        assert_eq!(
            e2_rr_round(E2::Cvtpd2qq, Masking::NONE, z(0), z(1), Round::Rz).bytes(),
            [0x62, 0xf1, 0xfd, 0x78, 0x7b, 0xc1]
        );
        assert_eq!(kmov_kr(Size::W, K::K1, Reg::Rax).bytes(), [0xc5, 0xf8, 0x92, 0xc8]);
        assert_eq!(kmov_kr(Size::Q, K::K1, Reg::Rax).bytes(), [0xc4, 0xe1, 0xfb, 0x92, 0xc8]);
        assert_eq!(kmov_kk(Size::B, K::K1, K::K2).bytes(), [0xc5, 0xf9, 0x90, 0xca]);
        assert_eq!(k2_kk(K2::Ortest, Size::W, K::K1, K::K2).bytes(), [0xc5, 0xf8, 0x98, 0xca]);
    }

    #[test]
    fn the_preconditions() {
        assert!(try_e3_rrr(E3::Paddd, Vl::Z, Masking::zero(K::K0), z(0), z(1), z(2)).is_none());
        assert!(try_e3_rrr(E3::Psadbw, Vl::Z, Masking::merge(K::K1), z(0), z(1), z(2)).is_none());
        assert!(try_e3_rrr(E3::Permd, Vl::X, Masking::NONE, z(0), z(1), z(2)).is_none());
        assert!(try_e3_rrb(E3::Pshufb, Vl::Z, Masking::NONE, z(0), z(1), Mem::abs(0)).is_none());
        let r = |rd| try_e3_rrr_round(E3::Maxps, Masking::NONE, z(0), z(1), z(2), rd);
        assert!(r(Round::Rn).is_none());
        assert!(r(Round::Sae).is_some());
        assert!(try_k3_kkk(K3::Unpck, Size::B, K::K1, K::K2, K::K3).is_none());
        assert!(Zmm::new(32).is_none());
        assert!(K::new(8).is_none());
    }
}
