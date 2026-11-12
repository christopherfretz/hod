//! Registers, operand sizes, condition codes, shift and extend kinds
//! (`spec/mc-a64.md` "Operand types").

/// A general-purpose register `X0`..`X30`, or its `W` view when the
/// instruction's [`Size`] is `W`. Never SP and never ZR.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Reg(u8);

macro_rules! named_regs {
    ($($name:ident = $n:literal),* $(,)?) => {
        $(
            #[doc = concat!("Register ", stringify!($n), ".")]
            pub const $name: Self = Self($n);
        )*
    };
}

impl Reg {
    named_regs! {
        X0 = 0, X1 = 1, X2 = 2, X3 = 3, X4 = 4, X5 = 5, X6 = 6, X7 = 7,
        X8 = 8, X9 = 9, X10 = 10, X11 = 11, X12 = 12, X13 = 13, X14 = 14, X15 = 15,
        X16 = 16, X17 = 17, X18 = 18, X19 = 19, X20 = 20, X21 = 21, X22 = 22, X23 = 23,
        X24 = 24, X25 = 25, X26 = 26, X27 = 27, X28 = 28, X29 = 29, X30 = 30,
    }

    /// The frame pointer, `X29`.
    pub const FP: Self = Self::X29;
    /// The link register, `X30`.
    pub const LR: Self = Self::X30;

    /// Register `index`, or `None` when `index > 30`.
    #[inline]
    pub const fn new(index: u8) -> Option<Self> {
        if index <= 30 { Some(Self(index)) } else { None }
    }

    /// The register number, `0..=30`.
    #[inline]
    pub const fn index(self) -> u8 {
        self.0
    }

    /// This register as an operand in a position where encoding 31 is ZR.
    #[inline]
    pub const fn zr(self) -> RegZr {
        RegZr(self.0)
    }

    /// This register as an operand in a position where encoding 31 is SP.
    #[inline]
    pub const fn sp(self) -> RegSp {
        RegSp(self.0)
    }
}

/// An operand that is a general-purpose register or the zero register
/// (`XZR`/`WZR`, encoding 31).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct RegZr(u8);

impl RegZr {
    /// The zero register.
    pub const ZR: Self = Self(31);

    /// The operand with encoding `code`: 31 is ZR. `None` when `code > 31`.
    #[inline]
    pub const fn new(code: u8) -> Option<Self> {
        if code <= 31 { Some(Self(code)) } else { None }
    }

    /// The 5-bit register field value.
    #[inline]
    pub const fn code(self) -> u32 {
        self.0 as u32
    }

    /// Whether this is the zero register.
    #[inline]
    pub const fn is_zr(self) -> bool {
        self.0 == 31
    }

    /// The general-purpose register, or `None` for ZR.
    #[inline]
    pub const fn reg(self) -> Option<Reg> {
        Reg::new(self.0)
    }
}

/// An operand that is a general-purpose register or the stack pointer
/// (`SP`/`WSP`, encoding 31).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct RegSp(u8);

impl RegSp {
    /// The stack pointer.
    pub const SP: Self = Self(31);

    /// The operand with encoding `code`: 31 is SP. `None` when `code > 31`.
    #[inline]
    pub const fn new(code: u8) -> Option<Self> {
        if code <= 31 { Some(Self(code)) } else { None }
    }

    /// The 5-bit register field value.
    #[inline]
    pub const fn code(self) -> u32 {
        self.0 as u32
    }

    /// Whether this is the stack pointer.
    #[inline]
    pub const fn is_sp(self) -> bool {
        self.0 == 31
    }

    /// The general-purpose register, or `None` for SP.
    #[inline]
    pub const fn reg(self) -> Option<Reg> {
        Reg::new(self.0)
    }
}

impl From<Reg> for RegZr {
    #[inline]
    fn from(reg: Reg) -> Self {
        reg.zr()
    }
}

impl From<Reg> for RegSp {
    #[inline]
    fn from(reg: Reg) -> Self {
        reg.sp()
    }
}

impl Reg {
    /// The 5-bit register field value.
    #[inline]
    pub(crate) const fn code(self) -> u32 {
        self.0 as u32
    }
}

/// Operand size of a data-processing instruction: the `sf` bit.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Size {
    /// 32-bit: `W` registers.
    W = 0,
    /// 64-bit: `X` registers.
    X = 1,
}

impl Size {
    /// Both sizes.
    pub const ALL: [Self; 2] = [Self::W, Self::X];

    /// 32 or 64.
    #[inline]
    pub const fn bits(self) -> u32 {
        match self {
            Self::W => 32,
            Self::X => 64,
        }
    }

    /// The `sf` bit in place (bit 31).
    #[inline]
    pub(crate) const fn sf(self) -> u32 {
        (self as u32) << 31
    }

    /// The `sf` bit as 0 or 1.
    #[inline]
    pub(crate) const fn bit(self) -> u32 {
        self as u32
    }
}

/// A condition code, with the Arm ARM's encoding.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Cond {
    /// Equal (`Z`).
    Eq = 0,
    /// Not equal.
    Ne = 1,
    /// Unsigned higher or same, carry set (`HS`, `CS`).
    Hs = 2,
    /// Unsigned lower, carry clear (`LO`, `CC`).
    Lo = 3,
    /// Negative.
    Mi = 4,
    /// Positive or zero.
    Pl = 5,
    /// Overflow.
    Vs = 6,
    /// No overflow.
    Vc = 7,
    /// Unsigned higher.
    Hi = 8,
    /// Unsigned lower or same.
    Ls = 9,
    /// Signed greater than or equal.
    Ge = 10,
    /// Signed less than.
    Lt = 11,
    /// Signed greater than.
    Gt = 12,
    /// Signed less than or equal.
    Le = 13,
    /// Always.
    Al = 14,
    /// Always (the second encoding of "always").
    Nv = 15,
}

impl Cond {
    /// Every condition, indexed by encoding.
    pub const ALL: [Self; 16] = [
        Self::Eq,
        Self::Ne,
        Self::Hs,
        Self::Lo,
        Self::Mi,
        Self::Pl,
        Self::Vs,
        Self::Vc,
        Self::Hi,
        Self::Ls,
        Self::Ge,
        Self::Lt,
        Self::Gt,
        Self::Le,
        Self::Al,
        Self::Nv,
    ];

    /// The 4-bit encoding.
    #[inline]
    pub const fn code(self) -> u32 {
        self as u32
    }

    /// The condition with the lowest encoding bit flipped: the logical
    /// negation for every condition except `Al`/`Nv`, which map to each
    /// other and both still mean "always".
    #[inline]
    #[must_use]
    pub const fn invert(self) -> Self {
        Self::ALL[(self as usize) ^ 1]
    }

    /// Whether this is `Al` or `Nv`.
    #[inline]
    pub const fn is_always(self) -> bool {
        matches!(self, Self::Al | Self::Nv)
    }
}

/// Shift applied to the last register operand of a shifted-register form.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Shift {
    /// Logical shift left.
    Lsl = 0,
    /// Logical shift right.
    Lsr = 1,
    /// Arithmetic shift right.
    Asr = 2,
    /// Rotate right. Logical instructions only.
    Ror = 3,
}

impl Shift {
    /// Every shift kind.
    pub const ALL: [Self; 4] = [Self::Lsl, Self::Lsr, Self::Asr, Self::Ror];
}

/// Extension applied to the last register operand of an extended-register
/// form: the `option` field.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Extend {
    /// Zero-extend the low byte.
    Uxtb = 0,
    /// Zero-extend the low 16 bits.
    Uxth = 1,
    /// Zero-extend the low 32 bits.
    Uxtw = 2,
    /// Use all 64 bits (`LSL` in assembly when a register is SP).
    Uxtx = 3,
    /// Sign-extend the low byte.
    Sxtb = 4,
    /// Sign-extend the low 16 bits.
    Sxth = 5,
    /// Sign-extend the low 32 bits.
    Sxtw = 6,
    /// Use all 64 bits.
    Sxtx = 7,
}

impl Extend {
    /// Every extend kind, indexed by encoding.
    pub const ALL: [Self; 8] = [
        Self::Uxtb,
        Self::Uxth,
        Self::Uxtw,
        Self::Uxtx,
        Self::Sxtb,
        Self::Sxth,
        Self::Sxtw,
        Self::Sxtx,
    ];
}
