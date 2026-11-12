//! Registers, operand sizes, condition codes and memory operands
//! (`spec/mc-x64.md` "Registers and operand kinds", "Memory operands",
//! "Scalar floating point").

/// A general-purpose register, by its 4-bit number (REX extension bit and
/// the 3-bit field). Which view (`al`, `ax`, `eax`, `rax`) an instruction
/// uses is its [`Size`] operand.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(u8)]
pub enum Reg {
    Rax = 0,
    Rcx,
    Rdx,
    Rbx,
    Rsp,
    Rbp,
    Rsi,
    Rdi,
    R8,
    R9,
    R10,
    R11,
    R12,
    R13,
    R14,
    R15,
}

impl Reg {
    /// Every register, in number order.
    pub const ALL: [Self; 16] = [
        Self::Rax,
        Self::Rcx,
        Self::Rdx,
        Self::Rbx,
        Self::Rsp,
        Self::Rbp,
        Self::Rsi,
        Self::Rdi,
        Self::R8,
        Self::R9,
        Self::R10,
        Self::R11,
        Self::R12,
        Self::R13,
        Self::R14,
        Self::R15,
    ];

    /// The register numbered `n`; `None` for `n > 15`.
    #[inline]
    pub const fn new(n: u8) -> Option<Self> {
        if n < 16 { Some(Self::ALL[n as usize]) } else { None }
    }

    /// The 4-bit register number.
    #[inline]
    pub const fn code(self) -> u8 {
        self as u8
    }

    /// The low three bits, the `ModRM`/`SIB`/opcode field.
    #[inline]
    pub(crate) const fn low(self) -> u8 {
        self as u8 & 7
    }

    /// The REX extension bit.
    #[inline]
    pub(crate) const fn high(self) -> u8 {
        (self as u8 >> 3) & 1
    }

    /// As an 8-bit operand, the register is `spl`, `bpl`, `sil` or `dil`,
    /// which exist only with a REX prefix.
    #[inline]
    pub(crate) const fn needs_rex_as_byte(self) -> bool {
        matches!(self, Self::Rsp | Self::Rbp | Self::Rsi | Self::Rdi)
    }

    /// The register as a `SIB` index: `None` for `rsp`, which has no index
    /// encoding.
    #[inline]
    pub const fn index(self) -> Option<Index> {
        if matches!(self, Self::Rsp) { None } else { Some(Index(self)) }
    }
}

/// An SSE register, `xmm0`..`xmm15`, by its 4-bit number.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(u8)]
pub enum Xmm {
    Xmm0 = 0,
    Xmm1,
    Xmm2,
    Xmm3,
    Xmm4,
    Xmm5,
    Xmm6,
    Xmm7,
    Xmm8,
    Xmm9,
    Xmm10,
    Xmm11,
    Xmm12,
    Xmm13,
    Xmm14,
    Xmm15,
}

impl Xmm {
    /// Every register, in number order.
    pub const ALL: [Self; 16] = [
        Self::Xmm0,
        Self::Xmm1,
        Self::Xmm2,
        Self::Xmm3,
        Self::Xmm4,
        Self::Xmm5,
        Self::Xmm6,
        Self::Xmm7,
        Self::Xmm8,
        Self::Xmm9,
        Self::Xmm10,
        Self::Xmm11,
        Self::Xmm12,
        Self::Xmm13,
        Self::Xmm14,
        Self::Xmm15,
    ];

    /// The register numbered `n`; `None` for `n > 15`.
    #[inline]
    pub const fn new(n: u8) -> Option<Self> {
        if n < 16 { Some(Self::ALL[n as usize]) } else { None }
    }

    /// The 4-bit register number.
    #[inline]
    pub const fn code(self) -> u8 {
        self as u8
    }

    /// The general register with the same number: the same `ModRM` and
    /// REX bits, which is all the layout routine needs.
    #[inline]
    pub(crate) const fn as_gpr(self) -> Reg {
        Reg::ALL[self as usize]
    }
}

/// The precision of a scalar floating-point operation, and the element
/// type of a packed logical one.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum FpSize {
    /// Single precision, 32 bits: `..ss` (prefix `F3`), `..ps` (none).
    S,
    /// Double precision, 64 bits: `..sd` (prefix `F2`), `..pd` (`66`).
    D,
}

impl FpSize {
    /// Both precisions, single first.
    pub const ALL: [Self; 2] = [Self::S, Self::D];

    /// The width in bits.
    #[inline]
    pub const fn bits(self) -> u32 {
        match self {
            Self::S => 32,
            Self::D => 64,
        }
    }
}

/// The predicate of `cmpss`/`cmpsd`, the instruction's `imm8` (SDM
/// Vol. 2A, "CMPSS", table 3-7). Each is false on an unordered pair except
/// `Unord`, `Neq`, `Nlt` and `Nle`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(u8)]
pub enum FpCmp {
    /// Equal (ordered).
    Eq = 0,
    /// Less than (ordered).
    Lt,
    /// Less than or equal (ordered).
    Le,
    /// Unordered: either operand is NaN.
    Unord,
    /// Not equal, or unordered.
    Neq,
    /// Not less than: greater than or equal, or unordered.
    Nlt,
    /// Not less than or equal: greater than, or unordered.
    Nle,
    /// Ordered: neither operand is NaN.
    Ord,
}

impl FpCmp {
    /// Every predicate, in `imm8` order.
    pub const ALL: [Self; 8] =
        [Self::Eq, Self::Lt, Self::Le, Self::Unord, Self::Neq, Self::Nlt, Self::Nle, Self::Ord];

    /// The predicate that holds exactly when `self` does not (also on an
    /// unordered pair): bit 2 flipped.
    #[inline]
    #[must_use]
    pub const fn invert(self) -> Self {
        Self::ALL[(self as u8 ^ 4) as usize]
    }
}

/// A register that can be a `SIB` index: any [`Reg`] except `rsp`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Index(Reg);

impl Index {
    /// The register.
    #[inline]
    pub const fn reg(self) -> Reg {
        self.0
    }
}

/// The operand size: which view of a register, or how wide a memory
/// access.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Size {
    /// 8 bits (`al`, `byte ptr`).
    B,
    /// 16 bits (`ax`, `word ptr`): operand-size prefix `0x66`.
    W,
    /// 32 bits (`eax`, `dword ptr`). Writing a 32-bit register zeroes bits
    /// 63:32.
    D,
    /// 64 bits (`rax`, `qword ptr`): `REX.W`.
    Q,
}

impl Size {
    /// Every size, narrowest first.
    pub const ALL: [Self; 4] = [Self::B, Self::W, Self::D, Self::Q];

    /// The width in bits.
    #[inline]
    pub const fn bits(self) -> u32 {
        match self {
            Self::B => 8,
            Self::W => 16,
            Self::D => 32,
            Self::Q => 64,
        }
    }

    /// The width in bytes.
    #[inline]
    pub const fn bytes(self) -> u8 {
        match self {
            Self::B => 1,
            Self::W => 2,
            Self::D => 4,
            Self::Q => 8,
        }
    }

    /// The low `bits()` bits of `v`.
    #[inline]
    pub(crate) const fn truncate(self, v: i64) -> u64 {
        match self {
            Self::B => v as u64 & 0xff,
            Self::W => v as u64 & 0xffff,
            Self::D => v as u64 & 0xffff_ffff,
            Self::Q => v as u64,
        }
    }

    /// Whether `imm` is an immediate operand of this size: a signed or an
    /// unsigned value of the size's width, except that a 64-bit
    /// instruction takes a 32-bit immediate it sign-extends
    /// (`spec/mc-x64.md` "Immediates").
    #[inline]
    pub const fn fits_imm(self, imm: i64) -> bool {
        match self {
            Self::B => imm >= -0x80 && imm <= 0xff,
            Self::W => imm >= -0x8000 && imm <= 0xffff,
            Self::D => imm >= -0x8000_0000 && imm <= 0xffff_ffff,
            Self::Q => imm >= -0x8000_0000 && imm <= 0x7fff_ffff,
        }
    }

    /// Whether `imm`, as an immediate of this size, is the sign extension
    /// of its low byte: the `imm8` forms.
    #[inline]
    pub(crate) const fn is_imm8(self, imm: i64) -> bool {
        let v = self.truncate(imm);
        let low = v as u8 as i8 as i64;
        self.truncate(low) == v
    }
}

/// A condition code, by its 4-bit `cc` field (Intel SDM Vol. 1, B.1).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(u8)]
pub enum Cond {
    /// Overflow.
    O = 0,
    /// No overflow.
    No,
    /// Below (unsigned `<`), carry.
    B,
    /// Above or equal (unsigned `>=`), no carry.
    Ae,
    /// Equal, zero.
    E,
    /// Not equal, not zero.
    Ne,
    /// Below or equal (unsigned `<=`).
    Be,
    /// Above (unsigned `>`).
    A,
    /// Sign.
    S,
    /// No sign.
    Ns,
    /// Parity even.
    P,
    /// Parity odd.
    Np,
    /// Less (signed `<`).
    L,
    /// Greater or equal (signed `>=`).
    Ge,
    /// Less or equal (signed `<=`).
    Le,
    /// Greater (signed `>`).
    G,
}

impl Cond {
    /// Every condition, in field order.
    pub const ALL: [Self; 16] = [
        Self::O,
        Self::No,
        Self::B,
        Self::Ae,
        Self::E,
        Self::Ne,
        Self::Be,
        Self::A,
        Self::S,
        Self::Ns,
        Self::P,
        Self::Np,
        Self::L,
        Self::Ge,
        Self::Le,
        Self::G,
    ];

    /// The condition that holds exactly when `self` does not.
    #[inline]
    #[must_use]
    pub const fn invert(self) -> Self {
        Self::ALL[(self as u8 ^ 1) as usize]
    }
}

/// A `SIB` scale factor.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(u8)]
pub enum Scale {
    S1 = 0,
    S2,
    S4,
    S8,
}

impl Scale {
    /// Every scale, smallest first.
    pub const ALL: [Self; 4] = [Self::S1, Self::S2, Self::S4, Self::S8];

    /// The factor: 1, 2, 4 or 8.
    #[inline]
    pub const fn factor(self) -> u8 {
        1 << self as u8
    }
}

/// A segment override of a memory operand. Only `fs` and `gs` have a
/// non-zero base in 64-bit mode.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Seg {
    Fs,
    Gs,
}

impl Seg {
    /// The prefix byte.
    #[inline]
    pub(crate) const fn prefix(self) -> u8 {
        match self {
            Self::Fs => 0x64,
            Self::Gs => 0x65,
        }
    }
}

/// The base of a memory operand.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Base {
    /// No base register: an absolute address, or index and scale alone.
    None,
    /// A general-purpose register.
    Reg(Reg),
    /// The address of the next instruction (`[rip + disp32]`).
    Rip,
}

/// A memory operand: `seg:[base + index * scale + disp]`.
///
/// Every value of this type has an encoding: `rsp` is not an [`Index`],
/// and a RIP-relative operand has no index by construction
/// (`spec/mc-x64.md` "Memory operands").
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Mem {
    base: Base,
    index: Option<(Index, Scale)>,
    disp: i32,
    seg: Option<Seg>,
    /// The displacement is an absolute 32-bit field: always `disp32`, and
    /// the instruction's site.
    abs32: bool,
}

impl Mem {
    /// `[base + disp]`.
    #[inline]
    pub const fn base(base: Reg, disp: i32) -> Self {
        Self { base: Base::Reg(base), index: None, disp, seg: None, abs32: false }
    }

    /// `[base + index * scale + disp]`.
    #[inline]
    pub const fn base_index(base: Reg, index: Index, scale: Scale, disp: i32) -> Self {
        Self { base: Base::Reg(base), index: Some((index, scale)), disp, seg: None, abs32: false }
    }

    /// `[index * scale + disp]`: no base register, a 32-bit displacement.
    #[inline]
    pub const fn index(index: Index, scale: Scale, disp: i32) -> Self {
        Self { base: Base::None, index: Some((index, scale)), disp, seg: None, abs32: false }
    }

    /// `[disp]`: the sign-extended 32-bit absolute address (`SIB` with no
    /// base and no index; not RIP-relative).
    #[inline]
    pub const fn abs(disp: i32) -> Self {
        Self { base: Base::None, index: None, disp, seg: None, abs32: false }
    }

    /// `[rip + disp]`: `disp` from the address of the next instruction.
    #[inline]
    pub const fn rip(disp: i32) -> Self {
        Self { base: Base::Rip, index: None, disp, seg: None, abs32: false }
    }

    /// The same operand with a segment override.
    #[inline]
    #[must_use]
    pub const fn with_seg(self, seg: Seg) -> Self {
        Self { seg: Some(seg), ..self }
    }

    /// The same operand with its displacement an absolute 32-bit field
    /// ([`Fixup::Abs32`](crate::Fixup::Abs32)): encoded as `disp32` even
    /// where a shorter form exists, and recorded as the instruction's
    /// site, for a relocation such as `R_X86_64_TPOFF32` to fill. `None`
    /// for a RIP-relative operand, whose displacement is already the
    /// instruction's PC-relative field.
    #[inline]
    #[must_use]
    pub const fn with_abs32(self) -> Option<Self> {
        if matches!(self.base, Base::Rip) { None } else { Some(Self { abs32: true, ..self }) }
    }

    /// Whether the displacement is an absolute 32-bit field
    /// ([`with_abs32`](Self::with_abs32)).
    #[inline]
    pub const fn is_abs32(self) -> bool {
        self.abs32
    }

    #[inline]
    pub const fn get_base(self) -> Base {
        self.base
    }

    #[inline]
    pub const fn get_index(self) -> Option<(Index, Scale)> {
        self.index
    }

    #[inline]
    pub const fn disp(self) -> i32 {
        self.disp
    }

    #[inline]
    pub const fn seg(self) -> Option<Seg> {
        self.seg
    }
}
