//! The legacy-encoded instructions of the three-byte opcode maps that the
//! x86 target intrinsics past SSSE3 need (WP-202, `spec/mc-x64.md`
//! "SSE4 and the extensions of the `0F38`/`0F3A` maps"): SSE4.1, SSE4.2's
//! string compares, AES, PCLMULQDQ, SHA and GFNI on 128-bit `xmm`
//! operands.
//!
//! Every form is `[66] [REX] 0F 38 op /r` ([`Ext`]) or `[66] [REX] 0F 3A
//! op /r ib` ([`ExtImm`]), the `66` prefix mandatory except for SHA's.
//! The memory operand is 128 bits (no alignment required by these
//! encoders; the instruction may require it, as its row says), except
//! where a row names a narrower one (`ROUNDSS` 32 bits, `ROUNDSD` 64,
//! `INSERTPS` 32).

use crate::inst::{Inst, Op, Rm, imm, modrm3};
use crate::reg::{Mem, Xmm};

/// The two-operand forms `op xmm, xmm/m128` of map `0F38`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ext {
    /// `PACKUSDW` (`66 0F38 2B`, SSE4.1): signed 32-bit to unsigned
    /// 16-bit, saturated.
    Packusdw,
    /// `PTEST` (`66 0F38 17`, SSE4.1): `ZF` from `dst & src == 0`, `CF`
    /// from `!dst & src == 0`; no register written.
    Ptest,
    /// `PHMINPOSUW` (`66 0F38 41`, SSE4.1): the least unsigned word and
    /// its index.
    Phminposuw,
    /// `PMULDQ` (`66 0F38 28`, SSE4.1).
    Pmuldq,
    /// `AESENC` (`66 0F38 DC`).
    Aesenc,
    /// `AESENCLAST` (`66 0F38 DD`).
    Aesenclast,
    /// `AESDEC` (`66 0F38 DE`).
    Aesdec,
    /// `AESDECLAST` (`66 0F38 DF`).
    Aesdeclast,
    /// `AESIMC` (`66 0F38 DB`).
    Aesimc,
    /// `SHA1NEXTE` (`0F38 C8`).
    Sha1nexte,
    /// `SHA1MSG1` (`0F38 C9`).
    Sha1msg1,
    /// `SHA1MSG2` (`0F38 CA`).
    Sha1msg2,
    /// `SHA256RNDS2` (`0F38 CB`): its third operand is `xmm0`.
    Sha256rnds2,
    /// `SHA256MSG1` (`0F38 CC`).
    Sha256msg1,
    /// `SHA256MSG2` (`0F38 CD`).
    Sha256msg2,
    /// `GF2P8MULB` (`66 0F38 CF`, GFNI).
    Gf2p8mulb,
}

impl Ext {
    /// Every member.
    pub const ALL: [Ext; 16] = [
        Ext::Packusdw,
        Ext::Ptest,
        Ext::Phminposuw,
        Ext::Pmuldq,
        Ext::Aesenc,
        Ext::Aesenclast,
        Ext::Aesdec,
        Ext::Aesdeclast,
        Ext::Aesimc,
        Ext::Sha1nexte,
        Ext::Sha1msg1,
        Ext::Sha1msg2,
        Ext::Sha256rnds2,
        Ext::Sha256msg1,
        Ext::Sha256msg2,
        Ext::Gf2p8mulb,
    ];

    /// The mnemonic, as LLVM prints it.
    pub const fn mnemonic(self) -> &'static str {
        match self {
            Ext::Packusdw => "packusdw",
            Ext::Ptest => "ptest",
            Ext::Phminposuw => "phminposuw",
            Ext::Pmuldq => "pmuldq",
            Ext::Aesenc => "aesenc",
            Ext::Aesenclast => "aesenclast",
            Ext::Aesdec => "aesdec",
            Ext::Aesdeclast => "aesdeclast",
            Ext::Aesimc => "aesimc",
            Ext::Sha1nexte => "sha1nexte",
            Ext::Sha1msg1 => "sha1msg1",
            Ext::Sha1msg2 => "sha1msg2",
            Ext::Sha256rnds2 => "sha256rnds2",
            Ext::Sha256msg1 => "sha256msg1",
            Ext::Sha256msg2 => "sha256msg2",
            Ext::Gf2p8mulb => "gf2p8mulb",
        }
    }

    /// Whether the `66` prefix is part of the opcode (all but SHA's).
    const fn p66(self) -> bool {
        !matches!(
            self,
            Ext::Sha1nexte
                | Ext::Sha1msg1
                | Ext::Sha1msg2
                | Ext::Sha256rnds2
                | Ext::Sha256msg1
                | Ext::Sha256msg2
        )
    }

    const fn opcode(self) -> u8 {
        match self {
            Ext::Packusdw => 0x2b,
            Ext::Ptest => 0x17,
            Ext::Phminposuw => 0x41,
            Ext::Pmuldq => 0x28,
            Ext::Aesenc => 0xdc,
            Ext::Aesenclast => 0xdd,
            Ext::Aesdec => 0xde,
            Ext::Aesdeclast => 0xdf,
            Ext::Aesimc => 0xdb,
            Ext::Sha1nexte => 0xc8,
            Ext::Sha1msg1 => 0xc9,
            Ext::Sha1msg2 => 0xca,
            Ext::Sha256rnds2 => 0xcb,
            Ext::Sha256msg1 => 0xcc,
            Ext::Sha256msg2 => 0xcd,
            Ext::Gf2p8mulb => 0xcf,
        }
    }
}

/// The forms `op xmm, xmm/m, imm8` of map `0F3A` (and SHA1RNDS4's).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExtImm {
    /// `ROUNDPS` (`66 0F3A 08`).
    Roundps,
    /// `ROUNDPD` (`66 0F3A 09`).
    Roundpd,
    /// `ROUNDSS` (`66 0F3A 0A`): the low `f32`, the rest of `dst` kept.
    Roundss,
    /// `ROUNDSD` (`66 0F3A 0B`).
    Roundsd,
    /// `INSERTPS` (`66 0F3A 21`).
    Insertps,
    /// `DPPS` (`66 0F3A 40`).
    Dpps,
    /// `DPPD` (`66 0F3A 41`).
    Dppd,
    /// `MPSADBW` (`66 0F3A 42`).
    Mpsadbw,
    /// `PCLMULQDQ` (`66 0F3A 44`).
    Pclmulqdq,
    /// `PCMPESTRM` (`66 0F3A 60`): lengths in `eax`, `edx`; mask to
    /// `xmm0`.
    Pcmpestrm,
    /// `PCMPESTRI` (`66 0F3A 61`): lengths in `eax`, `edx`; index to
    /// `ecx`.
    Pcmpestri,
    /// `PCMPISTRM` (`66 0F3A 62`): mask to `xmm0`.
    Pcmpistrm,
    /// `PCMPISTRI` (`66 0F3A 63`): index to `ecx`.
    Pcmpistri,
    /// `AESKEYGENASSIST` (`66 0F3A DF`).
    Aeskeygenassist,
    /// `SHA1RNDS4` (`0F3A CC`).
    Sha1rnds4,
    /// `GF2P8AFFINEQB` (`66 0F3A CE`, GFNI).
    Gf2p8affineqb,
    /// `GF2P8AFFINEINVQB` (`66 0F3A CF`, GFNI).
    Gf2p8affineinvqb,
}

impl ExtImm {
    /// Every member.
    pub const ALL: [ExtImm; 17] = [
        ExtImm::Roundps,
        ExtImm::Roundpd,
        ExtImm::Roundss,
        ExtImm::Roundsd,
        ExtImm::Insertps,
        ExtImm::Dpps,
        ExtImm::Dppd,
        ExtImm::Mpsadbw,
        ExtImm::Pclmulqdq,
        ExtImm::Pcmpestrm,
        ExtImm::Pcmpestri,
        ExtImm::Pcmpistrm,
        ExtImm::Pcmpistri,
        ExtImm::Aeskeygenassist,
        ExtImm::Sha1rnds4,
        ExtImm::Gf2p8affineqb,
        ExtImm::Gf2p8affineinvqb,
    ];

    /// The mnemonic, as LLVM prints it.
    pub const fn mnemonic(self) -> &'static str {
        match self {
            ExtImm::Roundps => "roundps",
            ExtImm::Roundpd => "roundpd",
            ExtImm::Roundss => "roundss",
            ExtImm::Roundsd => "roundsd",
            ExtImm::Insertps => "insertps",
            ExtImm::Dpps => "dpps",
            ExtImm::Dppd => "dppd",
            ExtImm::Mpsadbw => "mpsadbw",
            ExtImm::Pclmulqdq => "pclmulqdq",
            ExtImm::Pcmpestrm => "pcmpestrm",
            ExtImm::Pcmpestri => "pcmpestri",
            ExtImm::Pcmpistrm => "pcmpistrm",
            ExtImm::Pcmpistri => "pcmpistri",
            ExtImm::Aeskeygenassist => "aeskeygenassist",
            ExtImm::Sha1rnds4 => "sha1rnds4",
            ExtImm::Gf2p8affineqb => "gf2p8affineqb",
            ExtImm::Gf2p8affineinvqb => "gf2p8affineinvqb",
        }
    }

    /// The size of the memory operand in bytes (16 but for the scalar
    /// forms).
    pub const fn mem_bytes(self) -> u8 {
        match self {
            ExtImm::Roundss | ExtImm::Insertps => 4,
            ExtImm::Roundsd => 8,
            _ => 16,
        }
    }

    const fn opcode(self) -> u8 {
        match self {
            ExtImm::Roundps => 0x08,
            ExtImm::Roundpd => 0x09,
            ExtImm::Roundss => 0x0a,
            ExtImm::Roundsd => 0x0b,
            ExtImm::Insertps => 0x21,
            ExtImm::Dpps => 0x40,
            ExtImm::Dppd => 0x41,
            ExtImm::Mpsadbw => 0x42,
            ExtImm::Pclmulqdq => 0x44,
            ExtImm::Pcmpestrm => 0x60,
            ExtImm::Pcmpestri => 0x61,
            ExtImm::Pcmpistrm => 0x62,
            ExtImm::Pcmpistri => 0x63,
            ExtImm::Aeskeygenassist => 0xdf,
            ExtImm::Sha1rnds4 => 0xcc,
            ExtImm::Gf2p8affineqb => 0xce,
            ExtImm::Gf2p8affineinvqb => 0xcf,
        }
    }
}

/// `0F escape`, with the `66` prefix when `p66`.
const fn escape(p66: bool, escape: u8) -> Op {
    Op { p66, ..Op::two(escape) }
}

/// `op dst, src` of the [`Ext`] family (`[66] 0F 38 op /r`).
#[inline]
pub const fn ext_rr(op: Ext, dst: Xmm, src: Xmm) -> Inst {
    modrm3(escape(op.p66(), 0x38), op.opcode(), dst.code(), Rm::Reg(src.as_gpr()))
}

/// `op dst, m128` of the [`Ext`] family.
#[inline]
pub const fn ext_rm(op: Ext, dst: Xmm, src: Mem) -> Inst {
    modrm3(escape(op.p66(), 0x38), op.opcode(), dst.code(), Rm::Mem(src))
}

/// `op dst, src, imm8` of the [`ExtImm`] family (`[66] 0F 3A op /r ib`).
#[inline]
pub const fn ext_rri(op: ExtImm, dst: Xmm, src: Xmm, imm8: u8) -> Inst {
    let p66 = !matches!(op, ExtImm::Sha1rnds4);
    let inst = modrm3(escape(p66, 0x3a), op.opcode(), dst.code(), Rm::Reg(src.as_gpr()));
    imm(inst, imm8 as i64, 1)
}

/// `op dst, m, imm8` of the [`ExtImm`] family.
#[inline]
pub const fn ext_rmi(op: ExtImm, dst: Xmm, src: Mem, imm8: u8) -> Inst {
    let p66 = !matches!(op, ExtImm::Sha1rnds4);
    let inst = modrm3(escape(p66, 0x3a), op.opcode(), dst.code(), Rm::Mem(src));
    imm(inst, imm8 as i64, 1)
}
