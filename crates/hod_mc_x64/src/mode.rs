//! The processor mode an instruction is emitted for: 64-bit (`x86_64`) or
//! 32-bit (`i386`, `i686`), `spec/mc-x64.md` "32-bit mode" (WP-285).
//!
//! The constructors encode for 64-bit mode. An instruction whose operands
//! are 8-, 16- and 32-bit registers 0-7, the 64-bit address registers
//! `rax`..`rdi` standing for `eax`..`edi`, and no RIP-relative operand has
//! the same bytes in 32-bit mode, with a 32-bit meaning wherever 64-bit
//! mode has a 64-bit default operand size (`push`, `pop`, `call`/`jmp`
//! indirect, `ret`, `leave`, `pushfq`/`popfq`). [`Mode::admit`] takes such
//! an instruction to the bytes `llvm-mc -triple=i686` writes for it, and
//! refuses by a stable [`ModeError`] what 32-bit mode cannot express. A
//! code buffer carries its `Mode` and passes every instruction through it.
//!
//! ```
//! use hod_mc_x64::{Mem, Mode, ModeError, Reg, Size, inc_r, mov_rm};
//!
//! // inc eax: `FF C0` in 64-bit mode, `40` in 32-bit mode.
//! let inc = inc_r(Size::D, Reg::Rax);
//! assert_eq!(Mode::X64.admit(inc).unwrap().bytes(), [0xff, 0xc0]);
//! assert_eq!(Mode::X86.admit(inc).unwrap().bytes(), [0x40]);
//! // mov eax, [0x10]: the moffs form, no ModRM.
//! let load = mov_rm(Size::D, Reg::Rax, Mem::abs(0x10));
//! assert_eq!(Mode::X86.admit(load).unwrap().bytes(), [0xa1, 0x10, 0, 0, 0]);
//! // r8 does not exist in 32-bit mode.
//! let r8 = inc_r(Size::D, Reg::R8);
//! assert_eq!(Mode::X86.admit(r8), Err(ModeError::ExtendedRegister));
//! ```

use crate::inst::{FLAG_ABS, FLAG_RIP, FLAG_X64, Inst};

/// The processor mode of a code buffer.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Mode {
    /// 64-bit mode: every constructor's bytes, unchanged.
    X64,
    /// 32-bit protected mode with 32-bit default operand and address
    /// sizes (`i386`..`i686` Linux).
    X86,
}

/// Why [`Mode::X86`] refuses an instruction. Each has a stable
/// [`name`](Self::name).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ModeError {
    /// An operand needs a REX prefix or the extension bits of a VEX or
    /// EVEX prefix: `r8`-`r15`, `spl`/`bpl`/`sil`/`dil`, `xmm8`-`xmm31`,
    /// `ymm8`.., `zmm8`.. (in any field: `reg`, `r/m`, base, index,
    /// `vvvv`). In 32-bit mode the bytes `40`-`4F` are `inc`/`dec`.
    ExtendedRegister,
    /// A 64-bit operand size (`REX.W`): `add rax, ..`, `movsxd`,
    /// `cdqe`, `cqo`, `movabs`, `cmpxchg16b`, `cvtsi2sd xmm, r64`, ...
    OperandSize64,
    /// A RIP-relative memory operand: `ModRM mod=00 r/m=101` is an
    /// absolute `disp32` in 32-bit mode.
    RipRelative,
    /// An instruction that exists only in 64-bit mode although it has no
    /// REX prefix: `rdfsbase`/`rdgsbase`/`wrfsbase`/`wrgsbase`, and the
    /// VEX `W1` forms with a 64-bit general register (`kmovq k, r64`,
    /// `kmovq r64, k`, the BMI operations at 64 bits).
    X64Only,
}

impl ModeError {
    /// Every refusal.
    pub const ALL: [Self; 4] =
        [Self::ExtendedRegister, Self::OperandSize64, Self::RipRelative, Self::X64Only];

    /// The stable name of the refusal.
    pub const fn name(self) -> &'static str {
        match self {
            Self::ExtendedRegister => "x86-mode:extended-register",
            Self::OperandSize64 => "x86-mode:operand-size-64",
            Self::RipRelative => "x86-mode:rip-relative",
            Self::X64Only => "x86-mode:x86-64-only",
        }
    }
}

impl core::fmt::Display for ModeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.name())
    }
}

impl std::error::Error for ModeError {}

/// The legacy prefixes the encoder writes before REX and the opcode.
const fn is_prefix(b: u8) -> bool {
    matches!(b, 0x26 | 0x2e | 0x36 | 0x3e | 0x64 | 0x65 | 0x66 | 0x67 | 0xf0 | 0xf2 | 0xf3)
}

impl Mode {
    /// `inst` as this mode encodes it, or why the mode cannot.
    ///
    /// [`Mode::X64`] returns `inst` unchanged. [`Mode::X86`], in order:
    /// refuses [`ModeError::X64Only`], [`ModeError::RipRelative`], a REX
    /// prefix ([`ModeError::OperandSize64`] when it has `REX.W`, else
    /// [`ModeError::ExtendedRegister`]) and a VEX or EVEX prefix with a
    /// register extension bit set ([`ModeError::ExtendedRegister`]); then
    /// rewrites to the forms `llvm-mc -triple=i686` writes:
    ///
    /// - a `67` prefix (32-bit addressing in 64-bit mode, [`ud1_eax`]) is
    ///   dropped: 32-bit addressing is the default, and `67` would select
    ///   16-bit addressing;
    /// - an absolute address ([`Mem::abs`]) loses its `SIB` byte:
    ///   `ModRM.r/m=101`, then `disp32`;
    /// - `mov` between `al`/`ax`/`eax` and an absolute address
    ///   (`8A`/`8B`/`88`/`89` with `ModRM` `05`) becomes the `moffs` form
    ///   `A0`/`A1`/`A2`/`A3` with the 32-bit address, after any segment
    ///   and `66` prefix;
    /// - `inc`/`dec` of a 16- or 32-bit register (`FF /0`, `FF /1`,
    ///   `mod=11`) becomes `40+r`/`48+r`.
    ///
    /// The relocatable field ([`Inst::site`]) moves with its bytes: same
    /// kind, offset reduced by the bytes removed before it, the
    /// PC-relative addend recomputed from the new length.
    ///
    /// [`ud1_eax`]: crate::ud1_eax
    /// [`Mem::abs`]: crate::Mem::abs
    pub const fn admit(self, inst: Inst) -> Result<Inst, ModeError> {
        match self {
            Self::X64 => Ok(inst),
            Self::X86 => to_x86(inst),
        }
    }
}

const fn to_x86(inst: Inst) -> Result<Inst, ModeError> {
    let flags = inst.flags();
    if flags & FLAG_X64 != 0 {
        return Err(ModeError::X64Only);
    }
    if flags & FLAG_RIP != 0 {
        return Err(ModeError::RipRelative);
    }
    let mut inst = inst;
    let len = inst.len();
    let mut p = 0;
    while p < len && is_prefix(inst.bytes()[p]) {
        p += 1;
    }
    // A REX prefix: flagged by the layout routine, or written by hand
    // after the legacy prefixes (`crc32`, `rep movsq`); no 64-bit opcode
    // is 40-4F.
    if inst.has_rex() || (p < len && inst.bytes()[p] & 0xf0 == 0x40) {
        return Err(if p < len && inst.bytes()[p] & 8 != 0 {
            ModeError::OperandSize64
        } else {
            ModeError::ExtendedRegister
        });
    }
    // The 67 prefix: only `ud1_eax` writes it, for 32-bit addressing.
    let mut i = 0;
    while i < p {
        if inst.bytes()[i] == 0x67 {
            inst = inst.remove(i);
            p -= 1;
        } else {
            i += 1;
        }
    }
    let b = inst.bytes();
    if p >= b.len() {
        return Ok(inst);
    }
    // VEX and EVEX: the inverted extension bits must all be 1 (in 32-bit
    // mode `C4`/`C5`/`62` with a clear top bit are `LES`/`LDS`/`BOUND`).
    let (extended, opcode_at) = match b[p] {
        0xc5 => (b[p + 1] & 0xc0 != 0xc0, p + 2),
        0xc4 => (b[p + 1] & 0xe0 != 0xe0 || b[p + 2] & 0x40 == 0, p + 3),
        0x62 => (b[p + 1] & 0xf0 != 0xf0 || b[p + 2] & 0x40 == 0 || b[p + 3] & 0x08 == 0, p + 4),
        0x0f if matches!(b[p + 1], 0x38 | 0x3a) => (false, p + 2),
        0x0f => (false, p + 1),
        _ => (false, p),
    };
    // The fourth register of `vblendvps`/`vblendvpd`/`vpblendvb` (VEX
    // map 0F3A, 4A-4C) is in bits 7:4 of the last byte.
    let is4 = b[p] == 0xc4
        && b[p + 1] & 0x1f == 3
        && matches!(b[opcode_at], 0x4a..=0x4c)
        && b[b.len() - 1] & 0x80 != 0;
    if extended || is4 {
        return Err(ModeError::ExtendedRegister);
    }
    if flags & FLAG_ABS != 0 {
        // `ModRM.r/m=100`, `SIB=25` (no base, no index) to `r/m=101`.
        let modrm = opcode_at + 1;
        let (m, sib, opcode) = (b[modrm], b[modrm + 1], b[p]);
        debug_assert!(m & 0xc7 == 0x04 && sib == 0x25);
        let m = (m & 0x38) | 0x05;
        let inst = inst.set(modrm, m).remove(modrm + 1);
        // `mov al/ax/eax` to or from the address: the `moffs` forms.
        let moffs = match opcode {
            0x8a => 0xa0,
            0x8b => 0xa1,
            0x88 => 0xa2,
            0x89 => 0xa3,
            _ => 0,
        };
        return Ok(if moffs != 0 && m == 0x05 { inst.set(p, moffs).remove(p + 1) } else { inst });
    }
    // `inc`/`dec r16`/`r32`: `40+r`/`48+r`.
    if b[p] == 0xff && p + 1 < b.len() && b[p + 1] >= 0xc0 && (b[p + 1] >> 3) & 7 <= 1 {
        let short = 0x40 | (b[p + 1] & 0x0f);
        return Ok(inst.set(p, short).remove(p + 1));
    }
    // `xchg eax, eax`: 64-bit mode needs `87 C0` (`90` would not clear
    // the upper half); in 32-bit mode it is `90`.
    if p == 0 && b.len() == 2 && b[0] == 0x87 && b[1] == 0xc0 {
        return Ok(inst.set(0, 0x90).remove(1));
    }
    Ok(inst)
}

#[cfg(test)]
mod tests {
    use crate::*;

    const fn x(n: u8) -> Xmm {
        Xmm::ALL[n as usize]
    }

    fn z(n: u8) -> Zmm {
        Zmm::new(n).expect("a zmm register")
    }

    fn refused(inst: Inst) -> ModeError {
        Mode::X86.admit(inst).expect_err("32-bit mode refuses it")
    }

    fn admitted(inst: Inst) -> Vec<u8> {
        Mode::X86.admit(inst).expect("32-bit mode admits it").bytes().to_vec()
    }

    #[test]
    fn the_64_bit_mode_changes_nothing() {
        for inst in [
            inc_r(Size::D, Reg::Rax),
            mov_rm(Size::D, Reg::Rax, Mem::abs(16)),
            mov_rm(Size::Q, Reg::R8, Mem::rip(0)),
            ud1_eax(5),
            segbase_r(SegBase::RdFs, Size::D, Reg::Rax),
        ] {
            assert_eq!(Mode::X64.admit(inst), Ok(inst));
        }
    }

    #[test]
    fn extended_registers_are_refused() {
        use ModeError::ExtendedRegister as E;
        assert_eq!(refused(inc_r(Size::D, Reg::R8)), E);
        assert_eq!(refused(add_rr(Size::D, Reg::Rax, Reg::R15)), E);
        assert_eq!(refused(mov_rm(Size::D, Reg::Rax, Mem::base(Reg::R12, 0))), E);
        let r9 = Reg::R9.index().expect("an index");
        assert_eq!(refused(mov_rm(Size::D, Reg::Rax, Mem::index(r9, Scale::S4, 0))), E);
        // The REX-only byte registers.
        for r in [Reg::Rsp, Reg::Rbp, Reg::Rsi, Reg::Rdi] {
            assert_eq!(refused(mov_rr(Size::B, r, Reg::Rax)), E, "{r:?}");
            assert_eq!(refused(movzx_rr(Size::D, Reg::Rax, Size::B, r)), E, "{r:?}");
        }
        // REX written by hand after F2.
        assert_eq!(refused(crc32_rr(Size::D, Reg::R8, Reg::Rax)), E);
        assert_eq!(refused(adds_rr(FpSize::D, x(8), x(0))), E);
        // VEX: R, X, B and vvvv; the is4 register of the blends.
        assert_eq!(refused(v3_rrr(V3::Pshufb, Width::X, x(9), x(0), x(0))), E);
        assert_eq!(refused(v3_rrr(V3::Pshufb, Width::X, x(0), x(9), x(0))), E);
        assert_eq!(refused(v3_rrr(V3::Pshufb, Width::X, x(0), x(0), x(9))), E);
        let blend = vblend_rrrr(VBlend::Blendvps, Width::X, x(0), x(1), x(2), x(8));
        assert_eq!(refused(blend), E);
        let blend = vblend_rrrr(VBlend::Blendvps, Width::X, x(0), x(1), x(2), x(7));
        assert_eq!(admitted(blend), [0xc4, 0xe3, 0x71, 0x4a, 0xc2, 0x70]);
        // EVEX: zmm8-zmm31 in every field.
        let m = Masking::NONE;
        for (dst, src) in [(8, 0), (0, 8), (16, 0), (0, 16), (31, 31)] {
            let inst = emov_rr(EMov::Movdqu64, Vl::Z, m, z(dst), z(src));
            assert_eq!(refused(inst), E, "zmm{dst}, zmm{src}");
        }
        assert_eq!(admitted(emov_rr(EMov::Movdqu64, Vl::Z, m, z(7), z(0))).len(), 6);
    }

    #[test]
    fn a_64_bit_operand_size_is_refused() {
        use ModeError::OperandSize64 as E;
        assert_eq!(refused(add_rr(Size::Q, Reg::Rax, Reg::Rcx)), E);
        assert_eq!(refused(movsxd_rr(Reg::Rax, Reg::Rcx)), E);
        assert_eq!(refused(cdqe()), E);
        assert_eq!(refused(cqo()), E);
        assert_eq!(refused(movabs_ri(Reg::Rax, 1 << 40)), E);
        assert_eq!(refused(cmpxchg16b_m(Mem::base(Reg::Rdi, 0))), E);
        assert_eq!(refused(cvtsi2s_rr(FpSize::D, x(0), Size::Q, Reg::Rax)), E);
        assert_eq!(refused(rep_movs(Size::Q)), E);
        assert_eq!(refused(mov_rr(Size::Q, Reg::R8, Reg::Rax)), E);
    }

    #[test]
    fn rip_relative_operands_are_refused() {
        assert_eq!(refused(mov_rm(Size::D, Reg::Rax, Mem::rip(0))), ModeError::RipRelative);
        assert_eq!(refused(lea(Size::D, Reg::Rcx, Mem::rip(-8))), ModeError::RipRelative);
        let evex = emov_rm(EMov::Movdqu64, Vl::Z, Masking::NONE, z(0), Mem::rip(64));
        assert_eq!(refused(evex), ModeError::RipRelative);
    }

    #[test]
    fn x86_64_only_instructions_are_refused() {
        use ModeError::X64Only as E;
        for op in SegBase::ALL {
            assert_eq!(refused(segbase_r(op, Size::D, Reg::Rax)), E, "{op:?}");
        }
        assert_eq!(refused(kmov_kr(Size::Q, K::K1, Reg::Rax)), E);
        assert_eq!(refused(kmov_rk(Size::Q, Reg::Rax, K::K1)), E);
        assert_eq!(refused(bmi_rrr(Bmi::Pdep, Size::Q, Reg::Rax, Reg::Rcx, Reg::Rdx)), E);
        // Their 32-bit forms exist.
        assert_eq!(admitted(kmov_kr(Size::D, K::K1, Reg::Rax)), [0xc5, 0xfb, 0x92, 0xc8]);
        assert!(Mode::X86.admit(bmi_rrr(Bmi::Pdep, Size::D, Reg::Rax, Reg::Rcx, Reg::Rdx)).is_ok());
    }

    #[test]
    fn the_names_are_stable() {
        let names: Vec<&str> = ModeError::ALL.iter().map(|e| e.name()).collect();
        assert_eq!(
            names,
            [
                "x86-mode:extended-register",
                "x86-mode:operand-size-64",
                "x86-mode:rip-relative",
                "x86-mode:x86-64-only"
            ]
        );
        assert_eq!(ModeError::RipRelative.to_string(), "x86-mode:rip-relative");
    }

    #[test]
    fn the_short_forms() {
        assert_eq!(admitted(inc_r(Size::D, Reg::Rdi)), [0x47]);
        assert_eq!(admitted(dec_r(Size::W, Reg::Rcx)), [0x66, 0x49]);
        assert_eq!(admitted(inc_r(Size::B, Reg::Rax)), [0xfe, 0xc0]);
        assert_eq!(admitted(xchg_rr(Size::D, Reg::Rax, Reg::Rax)), [0x90]);
        assert_eq!(admitted(ud1_eax(0)), [0x0f, 0xb9, 0x00]);
        let abs = Mem::abs(0x10);
        assert_eq!(admitted(mov_rm(Size::D, Reg::Rcx, abs)), [0x8b, 0x0d, 0x10, 0, 0, 0]);
        assert_eq!(admitted(mov_rm(Size::B, Reg::Rax, abs)), [0xa0, 0x10, 0, 0, 0]);
        assert_eq!(admitted(mov_mr(Size::W, abs, Reg::Rax)), [0x66, 0xa3, 0x10, 0, 0, 0]);
        let gs = abs.with_seg(Seg::Gs);
        assert_eq!(admitted(mov_rm(Size::D, Reg::Rax, gs)), [0x65, 0xa1, 0x10, 0, 0, 0]);
        // An index without a base keeps its SIB byte.
        let index = Mem::index(Reg::Rcx.index().expect("ecx"), Scale::S4, 0x10);
        assert_eq!(admitted(mov_rm(Size::D, Reg::Rax, index)), [0x8b, 0x04, 0x8d, 0x10, 0, 0, 0]);
    }

    #[test]
    fn sites_move_with_their_bytes() {
        let field = Mem::abs(0).with_abs32().expect("absolute").with_seg(Seg::Gs);
        let tls = Mode::X86.admit(mov_rm(Size::D, Reg::Rax, field)).expect("admitted");
        assert_eq!(tls.bytes(), [0x65, 0xa1, 0, 0, 0, 0]);
        let site = tls.site().expect("a site");
        assert_eq!((site.offset, site.fixup, site.addend), (2, Fixup::Abs32, 0));
        let store = Mode::X86.admit(mov_mi(Size::D, Mem::abs(0x10), 0)).expect("admitted");
        assert_eq!(store.bytes(), [0xc7, 0x05, 0x10, 0, 0, 0, 0, 0, 0, 0]);
        let locked = Mode::X86.admit(lock(add_mi(Size::D, Mem::abs(0x10), 1))).expect("admitted");
        assert_eq!(locked.bytes(), [0xf0, 0x83, 0x05, 0x10, 0, 0, 0, 0x01]);
        let call = Mode::X86.admit(call(0)).expect("admitted");
        assert_eq!(call.site().map(|s| (s.offset, s.addend)), Some((1, -4)));
    }
}
