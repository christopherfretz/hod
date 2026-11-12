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

/// An FP/SIMD register `V0`..`V31`. Which of its views an instruction uses
/// (`B`, `H`, `S`, `D`, `Q`, or a vector arrangement) is a separate operand,
/// as [`Size`] is for [`Reg`]. Encoding 31 is `V31`: there is no zero
/// register and no stack pointer among them.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct VReg(u8);

impl VReg {
    named_regs! {
        V0 = 0, V1 = 1, V2 = 2, V3 = 3, V4 = 4, V5 = 5, V6 = 6, V7 = 7,
        V8 = 8, V9 = 9, V10 = 10, V11 = 11, V12 = 12, V13 = 13, V14 = 14, V15 = 15,
        V16 = 16, V17 = 17, V18 = 18, V19 = 19, V20 = 20, V21 = 21, V22 = 22, V23 = 23,
        V24 = 24, V25 = 25, V26 = 26, V27 = 27, V28 = 28, V29 = 29, V30 = 30, V31 = 31,
    }

    /// Register `index`, or `None` when `index > 31`.
    #[inline]
    pub const fn new(index: u8) -> Option<Self> {
        if index <= 31 { Some(Self(index)) } else { None }
    }

    /// The register number, `0..=31`.
    #[inline]
    pub const fn index(self) -> u8 {
        self.0
    }

    /// The 5-bit register field value.
    #[inline]
    pub(crate) const fn code(self) -> u32 {
        self.0 as u32
    }
}

/// The precision of a scalar floating-point operation: the `S` or `D` view
/// of a [`VReg`]. Half precision arithmetic needs `FEAT_FP16` and is not
/// encoded; see [`FpFormat`] for the conversions.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum FpSize {
    /// Single precision: `S` registers.
    S = 0,
    /// Double precision: `D` registers.
    D = 1,
}

impl FpSize {
    /// Both precisions.
    pub const ALL: [Self; 2] = [Self::S, Self::D];

    /// 32 or 64.
    #[inline]
    pub const fn bits(self) -> u32 {
        match self {
            Self::S => 32,
            Self::D => 64,
        }
    }

    /// The `ftype` field in place (bits 23:22).
    #[inline]
    pub(crate) const fn ftype(self) -> u32 {
        (self as u32) << 22
    }

    /// The `sz` bit of a vector instruction, as 0 or 1.
    #[inline]
    pub(crate) const fn bit(self) -> u32 {
        self as u32
    }
}

/// A floating-point format a conversion or a constant can name: the `H`, `S`
/// or `D` view of a [`VReg`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum FpFormat {
    /// IEEE half precision, 16 bits.
    H,
    /// IEEE single precision, 32 bits.
    S,
    /// IEEE double precision, 64 bits.
    D,
}

impl FpFormat {
    /// Every format.
    pub const ALL: [Self; 3] = [Self::H, Self::S, Self::D];

    /// 16, 32 or 64.
    #[inline]
    pub const fn bits(self) -> u32 {
        match self {
            Self::H => 16,
            Self::S => 32,
            Self::D => 64,
        }
    }

    /// The `ftype` field in place (bits 23:22).
    #[inline]
    pub(crate) const fn ftype(self) -> u32 {
        (match self {
            Self::S => 0,
            Self::D => 1,
            Self::H => 3,
        }) << 22
    }
}

/// The width of a vector operand: the low 64 bits of a [`VReg`] or all 128.
/// It is the whole operand of the instructions that work on bytes
/// (`<Vn>.8B` or `<Vn>.16B`): the logical operations, the selects, the
/// table lookups.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum VecBits {
    /// 64 bits: `8B`.
    V64 = 0,
    /// 128 bits: `16B`.
    V128 = 1,
}

impl VecBits {
    /// Both widths.
    pub const ALL: [Self; 2] = [Self::V64, Self::V128];

    /// 64 or 128.
    #[inline]
    pub const fn bits(self) -> u32 {
        64 << self as u32
    }

    /// The `Q` bit in place (bit 30).
    #[inline]
    pub(crate) const fn q(self) -> u32 {
        (self as u32) << 30
    }
}

/// The width of a vector element or of a scalar view of a [`VReg`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Elem {
    /// 8 bits.
    B = 0,
    /// 16 bits.
    H = 1,
    /// 32 bits.
    S = 2,
    /// 64 bits.
    D = 3,
}

impl Elem {
    /// Every element width.
    pub const ALL: [Self; 4] = [Self::B, Self::H, Self::S, Self::D];

    /// 8, 16, 32 or 64.
    #[inline]
    pub const fn bits(self) -> u32 {
        8 << self as u32
    }

    /// The number of elements of this width in a 128-bit register: 16, 8,
    /// 4 or 2. Element indices are below it.
    #[inline]
    pub const fn lanes(self) -> u32 {
        16 >> self as u32
    }

    /// log2 of the width in bytes, which is the `size` field value.
    #[inline]
    pub(crate) const fn log2(self) -> u32 {
        self as u32
    }
}

/// A vector arrangement: the element width and the number of elements, as
/// in `<Vn>.4S`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Arr {
    /// `8B`: 8 bytes in 64 bits.
    B8,
    /// `16B`: 16 bytes.
    B16,
    /// `4H`: 4 halfwords in 64 bits.
    H4,
    /// `8H`: 8 halfwords.
    H8,
    /// `2S`: 2 words in 64 bits.
    S2,
    /// `4S`: 4 words.
    S4,
    /// `2D`: 2 doublewords.
    D2,
}

impl Arr {
    /// Every arrangement.
    pub const ALL: [Self; 7] =
        [Self::B8, Self::B16, Self::H4, Self::H8, Self::S2, Self::S4, Self::D2];

    /// The element width.
    #[inline]
    pub const fn elem(self) -> Elem {
        match self {
            Self::B8 | Self::B16 => Elem::B,
            Self::H4 | Self::H8 => Elem::H,
            Self::S2 | Self::S4 => Elem::S,
            Self::D2 => Elem::D,
        }
    }

    /// The width of the whole vector.
    #[inline]
    pub const fn vec_bits(self) -> VecBits {
        match self {
            Self::B8 | Self::H4 | Self::S2 => VecBits::V64,
            Self::B16 | Self::H8 | Self::S4 | Self::D2 => VecBits::V128,
        }
    }

    /// The number of elements.
    #[inline]
    pub const fn lanes(self) -> u32 {
        self.vec_bits().bits() / self.elem().bits()
    }

    /// The `Q` bit and the `size` field (bits 23:22) in place.
    #[inline]
    pub(crate) const fn q_size(self) -> u32 {
        self.vec_bits().q() | self.elem().log2() << 22
    }
}

/// The arrangement of a vector floating-point operation. Half precision
/// elements need `FEAT_FP16` and are not encoded.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum FpArr {
    /// `2S`: 2 single-precision elements in 64 bits.
    S2,
    /// `4S`: 4 single-precision elements.
    S4,
    /// `2D`: 2 double-precision elements.
    D2,
}

impl FpArr {
    /// Every floating-point arrangement.
    pub const ALL: [Self; 3] = [Self::S2, Self::S4, Self::D2];

    /// The same arrangement as an [`Arr`].
    #[inline]
    pub const fn arr(self) -> Arr {
        match self {
            Self::S2 => Arr::S2,
            Self::S4 => Arr::S4,
            Self::D2 => Arr::D2,
        }
    }

    /// The element precision.
    #[inline]
    pub const fn fp_size(self) -> FpSize {
        match self {
            Self::S2 | Self::S4 => FpSize::S,
            Self::D2 => FpSize::D,
        }
    }

    /// The `Q` bit and the `sz` bit (bit 22) in place.
    #[inline]
    pub(crate) const fn q_sz(self) -> u32 {
        self.arr().vec_bits().q() | self.fp_size().bit() << 22
    }
}
