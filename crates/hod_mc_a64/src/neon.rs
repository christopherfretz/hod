//! The Advanced SIMD instructions behind the AArch64 target intrinsics
//! (`llvm.aarch64.neon.*`, `llvm.aarch64.crypto.*`): saturating, halving,
//! rounding and pairwise arithmetic, their scalar forms, narrowing and
//! long operations, shifts by immediate, dot products and matrix
//! multiplies, half-precision and bfloat16 arithmetic and conversions,
//! the cryptographic extensions, table lookups of one to four registers,
//! and the structure loads and stores (`spec/mc-a64.md` "NEON
//! intrinsics").
//!
//! The instructions come in families: a constructor takes an operation
//! kind (an enum whose cases are the mnemonics) and the operands the
//! family shares. A vector operand's shape is its width ([`VecBits`]) and
//! its element ([`Elem`] or, for floating point, [`FpFormat`]): unlike
//! [`crate::Arr`], this names `1D` (`V64`, `D`), which the structure loads
//! and stores take. Half-precision forms need `FEAT_FP16`; the others need
//! the extension their mnemonic belongs to (`FEAT_RDM`, `FEAT_DotProd`,
//! `FEAT_I8MM`, `FEAT_BF16`, `FEAT_FHM`, `FEAT_FCMA`, `FEAT_AES`,
//! `FEAT_SHA1`, `FEAT_SHA256`, `FEAT_SHA512`, `FEAT_SHA3`,
//! `FEAT_FRINTTS`). The crate encodes them regardless; whether the target
//! has them is the code generator's concern.

use crate::reg::{Elem, FpFormat, RegSp, VReg, VecBits};

/// The bits that turn a vector form of the three same, two-register
/// miscellaneous, three different and shift by immediate classes into its
/// scalar form: bits 30 and 28.
const SCALAR: u32 = 0x5000_0000;

/// Defines an operation-kind enum: each case is a mnemonic and the
/// instruction word with every operand field zero.
macro_rules! ops {
    (
        $(#[$meta:meta])*
        pub enum $name:ident {
            $($case:ident = ($word:expr, $mnemonic:literal),)+
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
        pub enum $name {
            $(
                #[doc = concat!("`", $mnemonic, "`.")]
                $case,
            )+
        }

        impl $name {
            /// Every case.
            pub const ALL: &[Self] = &[$(Self::$case),+];

            /// The assembly mnemonic of the vector form.
            #[inline]
            pub const fn mnemonic(self) -> &'static str {
                match self {
                    $(Self::$case => $mnemonic),+
                }
            }

            /// The word with every operand field zero.
            #[inline]
            const fn word(self) -> u32 {
                match self {
                    $(Self::$case => $word),+
                }
            }
        }
    };
}

/// The `Q` bit and the `size` field (bits 23:22) of a vector shape.
#[inline]
const fn q_size(bits: VecBits, elem: Elem) -> u32 {
    (bits as u32) << 30 | elem.log2() << 22
}

/// A vector shape that is an arrangement of the data-processing
/// instructions: anything but `1D`.
#[inline]
const fn is_arrangement(bits: VecBits, elem: Elem) -> bool {
    !matches!((bits, elem), (VecBits::V64, Elem::D))
}

#[inline]
const fn rrr(word: u32, rd: VReg, rn: VReg, rm: VReg) -> u32 {
    word | rm.code() << 16 | rn.code() << 5 | rd.code()
}

#[inline]
const fn rr(word: u32, rd: VReg, rn: VReg) -> u32 {
    word | rn.code() << 5 | rd.code()
}

// Integer three same.

ops! {
    /// Advanced SIMD three same (and three same extra), integer elements:
    /// [`neon_int3`] and [`neon_int3_scalar`].
    pub enum Int3 {
        Shadd = (0x0e20_0400, "shadd"),
        Uhadd = (0x2e20_0400, "uhadd"),
        Sqadd = (0x0e20_0c00, "sqadd"),
        Uqadd = (0x2e20_0c00, "uqadd"),
        Srhadd = (0x0e20_1400, "srhadd"),
        Urhadd = (0x2e20_1400, "urhadd"),
        Shsub = (0x0e20_2400, "shsub"),
        Uhsub = (0x2e20_2400, "uhsub"),
        Sqsub = (0x0e20_2c00, "sqsub"),
        Uqsub = (0x2e20_2c00, "uqsub"),
        Sshl = (0x0e20_4400, "sshl"),
        Ushl = (0x2e20_4400, "ushl"),
        Sqshl = (0x0e20_4c00, "sqshl"),
        Uqshl = (0x2e20_4c00, "uqshl"),
        Srshl = (0x0e20_5400, "srshl"),
        Urshl = (0x2e20_5400, "urshl"),
        Sqrshl = (0x0e20_5c00, "sqrshl"),
        Uqrshl = (0x2e20_5c00, "uqrshl"),
        Smax = (0x0e20_6400, "smax"),
        Umax = (0x2e20_6400, "umax"),
        Smin = (0x0e20_6c00, "smin"),
        Umin = (0x2e20_6c00, "umin"),
        Sabd = (0x0e20_7400, "sabd"),
        Uabd = (0x2e20_7400, "uabd"),
        Saba = (0x0e20_7c00, "saba"),
        Uaba = (0x2e20_7c00, "uaba"),
        Pmul = (0x2e20_9c00, "pmul"),
        Smaxp = (0x0e20_a400, "smaxp"),
        Umaxp = (0x2e20_a400, "umaxp"),
        Sminp = (0x0e20_ac00, "sminp"),
        Uminp = (0x2e20_ac00, "uminp"),
        Sqdmulh = (0x0e20_b400, "sqdmulh"),
        Sqrdmulh = (0x2e20_b400, "sqrdmulh"),
        Addp = (0x0e20_bc00, "addp"),
        Sqrdmlah = (0x2e00_8400, "sqrdmlah"),
        Sqrdmlsh = (0x2e00_8c00, "sqrdmlsh"),
    }
}

impl Int3 {
    /// Whether the vector form takes elements of width `elem`.
    #[inline]
    pub const fn vector_elem(self, elem: Elem) -> bool {
        match self {
            Self::Pmul => matches!(elem, Elem::B),
            Self::Sqdmulh | Self::Sqrdmulh | Self::Sqrdmlah | Self::Sqrdmlsh => {
                matches!(elem, Elem::H | Elem::S)
            }
            Self::Sqadd
            | Self::Uqadd
            | Self::Sqsub
            | Self::Uqsub
            | Self::Sshl
            | Self::Ushl
            | Self::Sqshl
            | Self::Uqshl
            | Self::Srshl
            | Self::Urshl
            | Self::Sqrshl
            | Self::Uqrshl
            | Self::Addp => true,
            _ => !matches!(elem, Elem::D),
        }
    }

    /// Whether the scalar form exists for elements of width `elem`.
    #[inline]
    pub const fn scalar_elem(self, elem: Elem) -> bool {
        match self {
            Self::Sqadd
            | Self::Uqadd
            | Self::Sqsub
            | Self::Uqsub
            | Self::Sqshl
            | Self::Uqshl
            | Self::Sqrshl
            | Self::Uqrshl => true,
            Self::Sshl | Self::Ushl | Self::Srshl | Self::Urshl => matches!(elem, Elem::D),
            Self::Sqdmulh | Self::Sqrdmulh | Self::Sqrdmlah | Self::Sqrdmlsh => {
                matches!(elem, Elem::H | Elem::S)
            }
            _ => false,
        }
    }
}

checked! {
    /// `<op> <Vd>.<T>, <Vn>.<T>, <Vm>.<T>`: an integer three same
    /// instruction, `T` being `bits` of `elem`. `SQRDMLAH` and `SQRDMLSH`
    /// accumulate into `rd`.
    ///
    /// Precondition: the shape is not `1D`, and `op.vector_elem(elem)`.
    pub fn neon_int3 / try_neon_int3(op: Int3, bits: VecBits, elem: Elem, rd: VReg, rn: VReg, rm: VReg)
    requires is_arrangement(bits, elem) && op.vector_elem(elem);
    { rrr(op.word() | q_size(bits, elem), rd, rn, rm) }

    /// `<op> <Vd>, <Vn>, <Vm>` on the `B`, `H`, `S` or `D` views: the
    /// scalar form of an integer three same instruction.
    ///
    /// Precondition: `op.scalar_elem(elem)`.
    pub fn neon_int3_scalar / try_neon_int3_scalar(op: Int3, elem: Elem, rd: VReg, rn: VReg, rm: VReg)
    requires op.scalar_elem(elem);
    { rrr(op.word() | SCALAR | elem.log2() << 22, rd, rn, rm) }
}

// Floating-point three same.

ops! {
    /// Advanced SIMD three same, floating-point elements: [`neon_fp3`] and
    /// [`neon_fp3_scalar`]. The word is the single-precision form; the
    /// half-precision form is another class.
    pub enum Fp3 {
        Fmaxnm = (0x0e20_c400, "fmaxnm"),
        Fminnm = (0x0ea0_c400, "fminnm"),
        Fmax = (0x0e20_f400, "fmax"),
        Fmin = (0x0ea0_f400, "fmin"),
        Fmulx = (0x0e20_dc00, "fmulx"),
        Frecps = (0x0e20_fc00, "frecps"),
        Frsqrts = (0x0ea0_fc00, "frsqrts"),
        Fabd = (0x2ea0_d400, "fabd"),
        Facge = (0x2e20_ec00, "facge"),
        Facgt = (0x2ea0_ec00, "facgt"),
        Faddp = (0x2e20_d400, "faddp"),
        Fmaxnmp = (0x2e20_c400, "fmaxnmp"),
        Fminnmp = (0x2ea0_c400, "fminnmp"),
        Fmaxp = (0x2e20_f400, "fmaxp"),
        Fminp = (0x2ea0_f400, "fminp"),
        Fadd = (0x0e20_d400, "fadd"),
        Fsub = (0x0ea0_d400, "fsub"),
        Fmul = (0x2e20_dc00, "fmul"),
        Fdiv = (0x2e20_fc00, "fdiv"),
    }
}

impl Fp3 {
    /// Whether the scalar form exists (as an Advanced SIMD scalar
    /// instruction).
    #[inline]
    pub const fn has_scalar(self) -> bool {
        matches!(
            self,
            Self::Fmulx | Self::Frecps | Self::Frsqrts | Self::Fabd | Self::Facge | Self::Facgt
        )
    }

    /// The word of the half-precision form, from the single-precision one:
    /// bit 23 kept, bit 22 set, bits 15:14 cleared.
    #[inline]
    const fn half(word: u32) -> u32 {
        (word & !0x0020_c000) | 0x0040_0000
    }
}

/// The word of a floating-point three same or two-register miscellaneous
/// instruction for `fmt`, from its single-precision word `word`.
#[inline]
const fn fp_word(word: u32, half: u32, fmt: FpFormat) -> u32 {
    match fmt {
        FpFormat::H => half,
        FpFormat::S => word,
        FpFormat::D => word | 1 << 22,
    }
}

/// A floating-point vector shape: anything but one double.
#[inline]
const fn is_fp_arrangement(bits: VecBits, fmt: FpFormat) -> bool {
    !matches!((bits, fmt), (VecBits::V64, FpFormat::D))
}

checked! {
    /// `<op> <Vd>.<T>, <Vn>.<T>, <Vm>.<T>`: a floating-point three same
    /// instruction, `T` being `bits` of `fmt` (`4H`, `8H`, `2S`, `4S`,
    /// `2D`).
    ///
    /// Precondition: the shape is not one double.
    pub fn neon_fp3 / try_neon_fp3(op: Fp3, bits: VecBits, fmt: FpFormat, rd: VReg, rn: VReg, rm: VReg)
    requires is_fp_arrangement(bits, fmt);
    {
        let word = fp_word(op.word(), Fp3::half(op.word()), fmt);
        rrr(word | (bits as u32) << 30, rd, rn, rm)
    }

    /// `<op> <Hd|Sd|Dd>, <Hn|Sn|Dn>, <Hm|Sm|Dm>`: the scalar form of a
    /// floating-point three same instruction.
    ///
    /// Precondition: `op.has_scalar()`.
    pub fn neon_fp3_scalar / try_neon_fp3_scalar(op: Fp3, fmt: FpFormat, rd: VReg, rn: VReg, rm: VReg)
    requires op.has_scalar();
    { rrr(fp_word(op.word(), Fp3::half(op.word()), fmt) | SCALAR, rd, rn, rm) }
}

// Integer two-register miscellaneous.

ops! {
    /// Advanced SIMD two-register miscellaneous, integer elements:
    /// [`neon_int2`] and [`neon_int2_scalar`]. For the narrowing cases
    /// (`SQXTN`, `UQXTN`, `SQXTUN`) the element is the narrow one, for the
    /// pairwise long cases the source one.
    pub enum Int2 {
        Suqadd = (0x0e20_3800, "suqadd"),
        Usqadd = (0x2e20_3800, "usqadd"),
        Cls = (0x0e20_4800, "cls"),
        Saddlp = (0x0e20_2800, "saddlp"),
        Sadalp = (0x0e20_6800, "sadalp"),
        Uadalp = (0x2e20_6800, "uadalp"),
        Sqabs = (0x0e20_7800, "sqabs"),
        Sqneg = (0x2e20_7800, "sqneg"),
        Abs = (0x0e20_b800, "abs"),
        Neg = (0x2e20_b800, "neg"),
        Sqxtn = (0x0e21_4800, "sqxtn"),
        Uqxtn = (0x2e21_4800, "uqxtn"),
        Sqxtun = (0x2e21_2800, "sqxtun"),
        Urecpe = (0x0e21_c800, "urecpe"),
        Ursqrte = (0x2e21_c800, "ursqrte"),
    }
}

impl Int2 {
    /// Whether the case narrows: its source elements are twice `elem`.
    #[inline]
    pub const fn narrows(self) -> bool {
        matches!(self, Self::Sqxtn | Self::Uqxtn | Self::Sqxtun)
    }

    /// Whether the case widens pairs: its result elements are twice
    /// `elem`.
    #[inline]
    pub const fn widens(self) -> bool {
        matches!(self, Self::Saddlp | Self::Sadalp | Self::Uadalp)
    }

    /// Whether the vector form takes `bits` of `elem`.
    #[inline]
    pub const fn vector_shape(self, bits: VecBits, elem: Elem) -> bool {
        match self {
            Self::Urecpe | Self::Ursqrte => matches!(elem, Elem::S),
            Self::Sqxtn | Self::Uqxtn | Self::Sqxtun => {
                matches!(bits, VecBits::V64) && !matches!(elem, Elem::D)
            }
            Self::Cls | Self::Saddlp | Self::Sadalp | Self::Uadalp => !matches!(elem, Elem::D),
            _ => is_arrangement(bits, elem),
        }
    }

    /// Whether the scalar form exists for `elem`.
    #[inline]
    pub const fn scalar_elem(self, elem: Elem) -> bool {
        match self {
            Self::Suqadd | Self::Usqadd | Self::Sqabs | Self::Sqneg => true,
            Self::Abs | Self::Neg => matches!(elem, Elem::D),
            Self::Sqxtn | Self::Uqxtn | Self::Sqxtun => !matches!(elem, Elem::D),
            _ => false,
        }
    }
}

checked! {
    /// `<op> <Vd>.<T>, <Vn>.<T>`: an integer two-register miscellaneous
    /// instruction on `bits` of `elem`. A narrowing case writes the lower
    /// half of `rd` (`SQXTN <Vd>.8B, <Vn>.8H`) and zeroes the rest; a
    /// pairwise long case reads `bits` of `elem` and writes half as many
    /// elements of twice the width; `SADALP`, `UADALP`, `SUQADD` and
    /// `USQADD` accumulate into `rd`.
    ///
    /// Precondition: `op.vector_shape(bits, elem)`.
    pub fn neon_int2 / try_neon_int2(op: Int2, bits: VecBits, elem: Elem, rd: VReg, rn: VReg)
    requires op.vector_shape(bits, elem);
    { rr(op.word() | q_size(bits, elem), rd, rn) }

    /// `<op> <Vd>, <Vn>` on scalar views: the scalar form of an integer
    /// two-register miscellaneous instruction. A narrowing case reads the
    /// view twice `elem` wide (`SQXTN <Sd>, <Dn>`).
    ///
    /// Precondition: `op.scalar_elem(elem)`.
    pub fn neon_int2_scalar / try_neon_int2_scalar(op: Int2, elem: Elem, rd: VReg, rn: VReg)
    requires op.scalar_elem(elem);
    { rr(op.word() | SCALAR | elem.log2() << 22, rd, rn) }
}

// Floating-point two-register miscellaneous.

ops! {
    /// Advanced SIMD two-register miscellaneous, floating-point elements:
    /// [`neon_fp2`] and [`neon_fp2_scalar`]. The word is the
    /// single-precision form; the half-precision form is another class.
    pub enum Fp2 {
        Fcvtns = (0x0e21_a800, "fcvtns"),
        Fcvtms = (0x0e21_b800, "fcvtms"),
        Fcvtas = (0x0e21_c800, "fcvtas"),
        Fcvtps = (0x0ea1_a800, "fcvtps"),
        Fcvtzs = (0x0ea1_b800, "fcvtzs"),
        Fcvtnu = (0x2e21_a800, "fcvtnu"),
        Fcvtmu = (0x2e21_b800, "fcvtmu"),
        Fcvtau = (0x2e21_c800, "fcvtau"),
        Fcvtpu = (0x2ea1_a800, "fcvtpu"),
        Fcvtzu = (0x2ea1_b800, "fcvtzu"),
        Frecpe = (0x0ea1_d800, "frecpe"),
        Frsqrte = (0x2ea1_d800, "frsqrte"),
        Frecpx = (0x0ea1_f800, "frecpx"),
        Frint32z = (0x0e21_e800, "frint32z"),
        Frint32x = (0x2e21_e800, "frint32x"),
        Frint64z = (0x0e21_f800, "frint64z"),
        Frint64x = (0x2e21_f800, "frint64x"),
        Scvtf = (0x0e21_d800, "scvtf"),
        Ucvtf = (0x2e21_d800, "ucvtf"),
        Fabs = (0x0ea0_f800, "fabs"),
        Fneg = (0x2ea0_f800, "fneg"),
        Fsqrt = (0x2ea1_f800, "fsqrt"),
        Frintn = (0x0e21_8800, "frintn"),
        Frintm = (0x0e21_9800, "frintm"),
        Frintp = (0x0ea1_8800, "frintp"),
        Frintz = (0x0ea1_9800, "frintz"),
        Frinta = (0x2e21_8800, "frinta"),
        Frintx = (0x2e21_9800, "frintx"),
        Frinti = (0x2ea1_9800, "frinti"),
    }
}

impl Fp2 {
    /// Whether the vector form takes `bits` of `fmt`.
    #[inline]
    pub const fn vector_shape(self, bits: VecBits, fmt: FpFormat) -> bool {
        match self {
            Self::Frecpx => false,
            Self::Frint32z | Self::Frint32x | Self::Frint64z | Self::Frint64x => {
                !matches!(fmt, FpFormat::H) && is_fp_arrangement(bits, fmt)
            }
            _ => is_fp_arrangement(bits, fmt),
        }
    }

    /// Whether the Advanced SIMD scalar form exists for `fmt`.
    #[inline]
    pub const fn scalar_format(self, _fmt: FpFormat) -> bool {
        matches!(
            self,
            Self::Fcvtns
                | Self::Fcvtms
                | Self::Fcvtas
                | Self::Fcvtps
                | Self::Fcvtzs
                | Self::Fcvtnu
                | Self::Fcvtmu
                | Self::Fcvtau
                | Self::Fcvtpu
                | Self::Fcvtzu
                | Self::Frecpe
                | Self::Frsqrte
                | Self::Frecpx
                | Self::Scvtf
                | Self::Ucvtf
        )
    }

    /// The word of the half-precision form: bits 22:19 set (bit 23, `a`,
    /// and the opcode stay).
    #[inline]
    const fn half(word: u32) -> u32 {
        word | 0x0078_0000
    }
}

checked! {
    /// `<op> <Vd>.<T>, <Vn>.<T>`: a floating-point two-register
    /// miscellaneous instruction on `bits` of `fmt`. The conversions to
    /// integer give integers of the same width; `SCVTF` and `UCVTF` take
    /// them.
    ///
    /// Precondition: `op.vector_shape(bits, fmt)`.
    pub fn neon_fp2 / try_neon_fp2(op: Fp2, bits: VecBits, fmt: FpFormat, rd: VReg, rn: VReg)
    requires op.vector_shape(bits, fmt);
    { rr(fp_word(op.word(), Fp2::half(op.word()), fmt) | (bits as u32) << 30, rd, rn) }

    /// `<op> <Hd|Sd|Dd>, <Hn|Sn|Dn>`: the scalar form of a floating-point
    /// two-register miscellaneous instruction.
    ///
    /// Precondition: `op.scalar_format(fmt)`.
    pub fn neon_fp2_scalar / try_neon_fp2_scalar(op: Fp2, fmt: FpFormat, rd: VReg, rn: VReg)
    requires op.scalar_format(fmt);
    { rr(fp_word(op.word(), Fp2::half(op.word()), fmt) | SCALAR, rd, rn) }
}

/// `BFCVTN <Vd>.4H, <Vn>.4S` (`V64`) / `BFCVTN2 <Vd>.8H, <Vn>.4S`
/// (`V128`): single precision to bfloat16, to nearest with ties to even,
/// into the lower half of `rd` (zeroing the upper) or the upper half
/// (keeping the lower).
#[inline]
pub const fn bfcvtn(bits: VecBits, rd: VReg, rn: VReg) -> u32 {
    rr(0x0ea1_6800 | (bits as u32) << 30, rd, rn)
}

/// `BFCVT <Hd>, <Sn>`: single precision to bfloat16.
#[inline]
pub const fn bfcvt(rd: VReg, rn: VReg) -> u32 {
    rr(0x1e63_4000, rd, rn)
}

/// `FCVTXN <Vd>.2S, <Vn>.2D`: double to single precision, rounding to odd,
/// into the lower half of `rd`.
#[inline]
pub const fn fcvtxn(rd: VReg, rn: VReg) -> u32 {
    rr(0x2e61_6800, rd, rn)
}

/// `FCVTXN <Sd>, <Dn>`: double to single precision, rounding to odd.
#[inline]
pub const fn fcvtxn_scalar(rd: VReg, rn: VReg) -> u32 {
    rr(0x7e61_6800, rd, rn)
}

// Across lanes and scalar pairwise.

checked! {
    /// `SADDLV <Hd|Sd|Dd>, <Vn>.<T>`: SADDLV, the signed sum of the
    /// elements of `bits` of `elem`, twice as wide.
    ///
    /// Precondition: at least four elements (not `2S` or `D`).
    pub fn saddlv / try_saddlv(bits: VecBits, elem: Elem, rd: VReg, rn: VReg)
    requires !matches!(elem, Elem::D) && !matches!((bits, elem), (VecBits::V64, Elem::S));
    { rr(0x0e30_3800 | q_size(bits, elem), rd, rn) }
}

ops! {
    /// Advanced SIMD across lanes, floating point: [`neon_fp_across`].
    pub enum FpAcross {
        Fmaxnmv = (0x2e30_c800, "fmaxnmv"),
        Fminnmv = (0x2eb0_c800, "fminnmv"),
        Fmaxv = (0x2e30_f800, "fmaxv"),
        Fminv = (0x2eb0_f800, "fminv"),
    }
}

checked! {
    /// `<op> <Hd|Sd>, <Vn>.<T>`: a floating-point reduction of `4H`, `8H`
    /// or `4S`.
    ///
    /// Precondition: `fmt` is `H`, or `S` with `bits` `V128`.
    pub fn neon_fp_across / try_neon_fp_across(op: FpAcross, bits: VecBits, fmt: FpFormat, rd: VReg, rn: VReg)
    requires matches!((bits, fmt), (_, FpFormat::H) | (VecBits::V128, FpFormat::S));
    {
        let word = match fmt {
            // The FP16 forms have U clear.
            FpFormat::H => op.word() & !(1 << 29),
            FpFormat::S | FpFormat::D => op.word(),
        };
        rr(word | (bits as u32) << 30, rd, rn)
    }
}

/// `ADDP <Dd>, <Vn>.2D`: ADDP (scalar), the sum of the two doublewords.
#[inline]
pub const fn addp_scalar(rd: VReg, rn: VReg) -> u32 {
    rr(0x5ef1_b800, rd, rn)
}

ops! {
    /// Advanced SIMD scalar pairwise, floating point: [`neon_fp_pair`].
    pub enum FpPair {
        Faddp = (0x7e30_d800, "faddp"),
        Fmaxnmp = (0x7e30_c800, "fmaxnmp"),
        Fminnmp = (0x7eb0_c800, "fminnmp"),
        Fmaxp = (0x7e30_f800, "fmaxp"),
        Fminp = (0x7eb0_f800, "fminp"),
    }
}

/// `<op> <Hd|Sd|Dd>, <Vn>.<2H|2S|2D>`: a floating-point operation on the
/// two elements of `rn` of format `fmt`.
#[inline]
pub const fn neon_fp_pair(op: FpPair, fmt: FpFormat, rd: VReg, rn: VReg) -> u32 {
    let word = match fmt {
        FpFormat::H => op.word() & !(1 << 29),
        FpFormat::S => op.word(),
        FpFormat::D => op.word() | 1 << 22,
    };
    rr(word, rd, rn)
}

// Three different.

ops! {
    /// Advanced SIMD three different: [`neon_long3`]. The element is the
    /// narrow one: the sources' for the long cases, the result's for the
    /// high-narrowing ones (`ADDHN`, `RADDHN`, `SUBHN`, `RSUBHN`).
    pub enum Long3 {
        Saddl = (0x0e20_0000, "saddl"),
        Uaddl = (0x2e20_0000, "uaddl"),
        Ssubl = (0x0e20_2000, "ssubl"),
        Usubl = (0x2e20_2000, "usubl"),
        Addhn = (0x0e20_4000, "addhn"),
        Raddhn = (0x2e20_4000, "raddhn"),
        Sabal = (0x0e20_5000, "sabal"),
        Uabal = (0x2e20_5000, "uabal"),
        Subhn = (0x0e20_6000, "subhn"),
        Rsubhn = (0x2e20_6000, "rsubhn"),
        Sabdl = (0x0e20_7000, "sabdl"),
        Uabdl = (0x2e20_7000, "uabdl"),
        Smlal = (0x0e20_8000, "smlal"),
        Umlal = (0x2e20_8000, "umlal"),
        Sqdmlal = (0x0e20_9000, "sqdmlal"),
        Smlsl = (0x0e20_a000, "smlsl"),
        Umlsl = (0x2e20_a000, "umlsl"),
        Sqdmlsl = (0x0e20_b000, "sqdmlsl"),
        Smull = (0x0e20_c000, "smull"),
        Umull = (0x2e20_c000, "umull"),
        Sqdmull = (0x0e20_d000, "sqdmull"),
        Pmull = (0x0e20_e000, "pmull"),
    }
}

impl Long3 {
    /// Whether the case narrows: its sources are twice `elem` wide.
    #[inline]
    pub const fn narrows(self) -> bool {
        matches!(self, Self::Addhn | Self::Raddhn | Self::Subhn | Self::Rsubhn)
    }

    /// Whether the case takes narrow elements `elem`.
    #[inline]
    pub const fn takes(self, elem: Elem) -> bool {
        match self {
            Self::Sqdmlal | Self::Sqdmlsl | Self::Sqdmull => matches!(elem, Elem::H | Elem::S),
            Self::Pmull => matches!(elem, Elem::B | Elem::D),
            _ => !matches!(elem, Elem::D),
        }
    }
}

checked! {
    /// `<op> <Vd>.<Tb>, <Vn>.<Ta>, <Vm>.<Ta>`: a three different
    /// instruction on the lower halves (no `2` suffix): a long case reads
    /// 64 bits of `elem` from each source and writes elements twice as
    /// wide (`PMULL <Vd>.1Q, <Vn>.1D, <Vm>.1D` for `D`); a narrowing case
    /// reads 128 bits of twice `elem` and writes 64 bits of `elem`,
    /// zeroing the upper half. `SABAL`, `UABAL` and the multiply-adds
    /// accumulate into `rd`.
    ///
    /// Precondition: `op.takes(elem)`.
    pub fn neon_long3 / try_neon_long3(op: Long3, elem: Elem, rd: VReg, rn: VReg, rm: VReg)
    requires op.takes(elem);
    { rrr(op.word() | elem.log2() << 22, rd, rn, rm) }

    /// `SQDMULL <Sd>, <Hn>, <Hm>` / `SQDMULL <Dd>, <Sn>, <Sm>`: SQDMULL
    /// (scalar), the saturating doubled product, twice as wide as `elem`.
    ///
    /// Precondition: `elem` is `H` or `S`.
    pub fn sqdmull_scalar / try_sqdmull_scalar(elem: Elem, rd: VReg, rn: VReg, rm: VReg)
    requires matches!(elem, Elem::H | Elem::S);
    { rrr(0x5e20_d000 | elem.log2() << 22, rd, rn, rm) }
}

// Shift by immediate.

/// How a shift by immediate encodes its amount.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ShiftKind {
    /// Right shifts and the fixed-point conversions: `1..=width`, encoded
    /// as `2 * width - amount`.
    Right,
    /// Left shifts: `0..width`, encoded as `width + amount`.
    Left,
    /// Right shifts that narrow: the element is the narrow result's,
    /// `1..=width`, encoded as `2 * width - amount`.
    Narrow,
}

ops! {
    /// Advanced SIMD shift by immediate: [`neon_shift`] and
    /// [`neon_shift_scalar`].
    pub enum ShiftImm {
        Sshr = (0x0f00_0400, "sshr"),
        Ssra = (0x0f00_1400, "ssra"),
        Srshr = (0x0f00_2400, "srshr"),
        Srsra = (0x0f00_3400, "srsra"),
        Ushr = (0x2f00_0400, "ushr"),
        Usra = (0x2f00_1400, "usra"),
        Urshr = (0x2f00_2400, "urshr"),
        Ursra = (0x2f00_3400, "ursra"),
        Sri = (0x2f00_4400, "sri"),
        Shl = (0x0f00_5400, "shl"),
        Sli = (0x2f00_5400, "sli"),
        Sqshl = (0x0f00_7400, "sqshl"),
        Uqshl = (0x2f00_7400, "uqshl"),
        Sqshlu = (0x2f00_6400, "sqshlu"),
        Shrn = (0x0f00_8400, "shrn"),
        Rshrn = (0x0f00_8c00, "rshrn"),
        Sqshrn = (0x0f00_9400, "sqshrn"),
        Sqrshrn = (0x0f00_9c00, "sqrshrn"),
        Sqshrun = (0x2f00_8400, "sqshrun"),
        Sqrshrun = (0x2f00_8c00, "sqrshrun"),
        Uqshrn = (0x2f00_9400, "uqshrn"),
        Uqrshrn = (0x2f00_9c00, "uqrshrn"),
        Scvtf = (0x0f00_e400, "scvtf"),
        Ucvtf = (0x2f00_e400, "ucvtf"),
        Fcvtzs = (0x0f00_fc00, "fcvtzs"),
        Fcvtzu = (0x2f00_fc00, "fcvtzu"),
    }
}

impl ShiftImm {
    /// How the amount is encoded.
    #[inline]
    pub const fn kind(self) -> ShiftKind {
        match self {
            Self::Shl | Self::Sli | Self::Sqshl | Self::Uqshl | Self::Sqshlu => ShiftKind::Left,
            Self::Shrn
            | Self::Rshrn
            | Self::Sqshrn
            | Self::Sqrshrn
            | Self::Sqshrun
            | Self::Sqrshrun
            | Self::Uqshrn
            | Self::Uqrshrn => ShiftKind::Narrow,
            _ => ShiftKind::Right,
        }
    }

    /// Whether the case converts between fixed and floating point.
    #[inline]
    pub const fn converts(self) -> bool {
        matches!(self, Self::Scvtf | Self::Ucvtf | Self::Fcvtzs | Self::Fcvtzu)
    }

    /// Whether `amount` is in range for elements of width `elem`.
    #[inline]
    pub const fn amount_fits(self, elem: Elem, amount: u32) -> bool {
        match self.kind() {
            ShiftKind::Left => amount < elem.bits(),
            ShiftKind::Right | ShiftKind::Narrow => amount >= 1 && amount <= elem.bits(),
        }
    }

    /// Whether the vector form takes `bits` of `elem`.
    #[inline]
    pub const fn vector_shape(self, bits: VecBits, elem: Elem) -> bool {
        if self.converts() {
            return !matches!(elem, Elem::B) && is_arrangement(bits, elem);
        }
        match self.kind() {
            ShiftKind::Narrow => matches!(bits, VecBits::V64) && !matches!(elem, Elem::D),
            ShiftKind::Right | ShiftKind::Left => is_arrangement(bits, elem),
        }
    }

    /// Whether the scalar form exists for `elem`.
    #[inline]
    pub const fn scalar_elem(self, elem: Elem) -> bool {
        match self {
            Self::Sqshl | Self::Uqshl | Self::Sqshlu => true,
            Self::Scvtf | Self::Ucvtf | Self::Fcvtzs | Self::Fcvtzu => !matches!(elem, Elem::B),
            Self::Shrn | Self::Rshrn => false,
            _ => match self.kind() {
                ShiftKind::Narrow => !matches!(elem, Elem::D),
                ShiftKind::Right | ShiftKind::Left => matches!(elem, Elem::D),
            },
        }
    }

    /// The `immh:immb` field.
    #[inline]
    const fn immhb(self, elem: Elem, amount: u32) -> u32 {
        let field = match self.kind() {
            ShiftKind::Left => elem.bits() + amount,
            ShiftKind::Right | ShiftKind::Narrow => (2 * elem.bits()).wrapping_sub(amount),
        };
        (field & 0x7f) << 16
    }
}

checked! {
    /// `<op> <Vd>.<T>, <Vn>.<T>, #amount`: a shift by immediate on `bits`
    /// of `elem`. A narrowing case reads 128 bits of twice `elem` and
    /// writes the lower half of `rd`, zeroing the rest; the accumulating
    /// and inserting cases (`SSRA`, `SRI`, `SLI`, ...) keep `rd`. The
    /// conversions take `amount` fraction bits.
    ///
    /// Precondition: `op.vector_shape(bits, elem)` and
    /// `op.amount_fits(elem, amount)`.
    pub fn neon_shift / try_neon_shift(op: ShiftImm, bits: VecBits, elem: Elem, rd: VReg, rn: VReg, amount: u32)
    requires op.vector_shape(bits, elem) && op.amount_fits(elem, amount);
    { rr(op.word() | (bits as u32) << 30 | op.immhb(elem, amount), rd, rn) }

    /// `<op> <Vd>, <Vn>, #amount` on scalar views: the scalar form of a
    /// shift by immediate. A narrowing case reads the view twice `elem`
    /// wide.
    ///
    /// Precondition: `op.scalar_elem(elem)` and
    /// `op.amount_fits(elem, amount)`.
    pub fn neon_shift_scalar / try_neon_shift_scalar(op: ShiftImm, elem: Elem, rd: VReg, rn: VReg, amount: u32)
    requires op.scalar_elem(elem) && op.amount_fits(elem, amount);
    { rr(op.word() | SCALAR | op.immhb(elem, amount), rd, rn) }
}

// Three same extra: dot products, matrix multiplies, widening multiply-adds.

ops! {
    /// The accumulating products with fixed element types: [`neon_dot`].
    pub enum Dot {
        Sdot = (0x0e80_9400, "sdot"),
        Udot = (0x2e80_9400, "udot"),
        Usdot = (0x0e80_9c00, "usdot"),
        Smmla = (0x4e80_a400, "smmla"),
        Ummla = (0x6e80_a400, "ummla"),
        Usmmla = (0x4e80_ac00, "usmmla"),
        Bfdot = (0x2e40_fc00, "bfdot"),
        Bfmmla = (0x6e40_ec00, "bfmmla"),
        Bfmlalb = (0x2ec0_fc00, "bfmlalb"),
        Bfmlalt = (0x6ec0_fc00, "bfmlalt"),
        Fmlal = (0x0e20_ec00, "fmlal"),
        Fmlal2 = (0x2e20_cc00, "fmlal2"),
        Fmlsl = (0x0ea0_ec00, "fmlsl"),
        Fmlsl2 = (0x2ea0_cc00, "fmlsl2"),
    }
}

impl Dot {
    /// Whether the case only has the 128-bit form.
    #[inline]
    pub const fn only_128(self) -> bool {
        matches!(
            self,
            Self::Smmla | Self::Ummla | Self::Usmmla | Self::Bfmmla | Self::Bfmlalb | Self::Bfmlalt
        )
    }
}

checked! {
    /// `<op> <Vd>.<2S|4S>, <Vn>.<Tb>, <Vm>.<Tb>`: a product accumulated
    /// into the single-precision or 32-bit elements of `rd`, `bits` wide.
    /// The sources are bytes (`SDOT`, `UDOT`, `USDOT`, the `MMLA`s), bfloat16
    /// (`BF`) or half precision (`FMLAL`, `FMLSL`, which read the lower
    /// (`2H`/`4H`) halves of `rn` and `rm`, or the upper ones for the `2`
    /// forms).
    ///
    /// Precondition: `bits` is `V128` for the matrix multiplies and
    /// `BFMLALB`/`BFMLALT`.
    pub fn neon_dot / try_neon_dot(op: Dot, bits: VecBits, rd: VReg, rn: VReg, rm: VReg)
    requires matches!(bits, VecBits::V128) || !op.only_128();
    {
        // The 128-bit-only cases have their `Q` bit in the word: for
        // `BFMLALB` and `BFMLALT` it is the bottom/top choice.
        let q = if op.only_128() { 0 } else { (bits as u32) << 30 };
        rrr(op.word() | q, rd, rn, rm)
    }

    /// `FCMLA <Vd>.<T>, <Vn>.<T>, <Vm>.<T>, #rot`: FCMLA (vector), the
    /// complex multiply-add with the rotation `rot * 90` degrees.
    ///
    /// Precondition: the shape is not one double, and `rot < 4`.
    pub fn fcmla / try_fcmla(bits: VecBits, fmt: FpFormat, rd: VReg, rn: VReg, rm: VReg, rot: u32)
    requires is_fp_arrangement(bits, fmt) && rot < 4;
    {
        let size = match fmt {
            FpFormat::H => 1,
            FpFormat::S => 2,
            FpFormat::D => 3,
        };
        rrr(0x2e00_c400 | (bits as u32) << 30 | size << 22 | (rot & 3) << 11, rd, rn, rm)
    }

    /// `FCADD <Vd>.<T>, <Vn>.<T>, <Vm>.<T>, #rot`: FCADD, the complex add
    /// with `rm` rotated by 90 (`rot270` false) or 270 degrees.
    ///
    /// Precondition: the shape is not one double.
    pub fn fcadd / try_fcadd(bits: VecBits, fmt: FpFormat, rd: VReg, rn: VReg, rm: VReg, rot270: bool)
    requires is_fp_arrangement(bits, fmt);
    {
        let size = match fmt {
            FpFormat::H => 1,
            FpFormat::S => 2,
            FpFormat::D => 3,
        };
        rrr(0x2e00_e400 | (bits as u32) << 30 | size << 22 | (rot270 as u32) << 12, rd, rn, rm)
    }
}

// Cryptography.

ops! {
    /// The AES instructions, on `16B`: [`aes`].
    pub enum Aes {
        Aese = (0x4e28_4800, "aese"),
        Aesd = (0x4e28_5800, "aesd"),
        Aesmc = (0x4e28_6800, "aesmc"),
        Aesimc = (0x4e28_7800, "aesimc"),
    }
}

/// `<op> <Vd>.16B, <Vn>.16B`: an AES round step. `AESE` and `AESD` combine
/// `rd` with `rn`.
#[inline]
pub const fn aes(op: Aes, rd: VReg, rn: VReg) -> u32 {
    rr(op.word(), rd, rn)
}

ops! {
    /// The SHA instructions with three registers: [`sha3reg`].
    pub enum Sha3Reg {
        Sha1c = (0x5e00_0000, "sha1c"),
        Sha1p = (0x5e00_1000, "sha1p"),
        Sha1m = (0x5e00_2000, "sha1m"),
        Sha1su0 = (0x5e00_3000, "sha1su0"),
        Sha256h = (0x5e00_4000, "sha256h"),
        Sha256h2 = (0x5e00_5000, "sha256h2"),
        Sha256su1 = (0x5e00_6000, "sha256su1"),
        Sha512h = (0xce60_8000, "sha512h"),
        Sha512h2 = (0xce60_8400, "sha512h2"),
        Sha512su1 = (0xce60_8800, "sha512su1"),
        Rax1 = (0xce60_8c00, "rax1"),
    }
}

/// `<op> <Qd|Vd.4S|Vd.2D>, <Sn|Qn|Vn.4S|Vn.2D>, <Vm>.<4S|2D>`: a SHA hash
/// or schedule update (or `RAX1`); all but `RAX1` combine `rd` with the
/// sources.
#[inline]
pub const fn sha3reg(op: Sha3Reg, rd: VReg, rn: VReg, rm: VReg) -> u32 {
    rrr(op.word(), rd, rn, rm)
}

ops! {
    /// The SHA instructions with two registers: [`sha2reg`].
    pub enum Sha2Reg {
        Sha1h = (0x5e28_0800, "sha1h"),
        Sha1su1 = (0x5e28_1800, "sha1su1"),
        Sha256su0 = (0x5e28_2800, "sha256su0"),
        Sha512su0 = (0xcec0_8000, "sha512su0"),
    }
}

/// `<op> <Sd|Vd.4S|Vd.2D>, <Sn|Vn.4S|Vn.2D>`: `SHA1H` or a schedule update,
/// which combines `rd` with `rn`.
#[inline]
pub const fn sha2reg(op: Sha2Reg, rd: VReg, rn: VReg) -> u32 {
    rr(op.word(), rd, rn)
}

/// `EOR3 <Vd>.16B, <Vn>.16B, <Vm>.16B, <Va>.16B`: EOR3, `rn ^ rm ^ ra`.
#[inline]
pub const fn eor3(rd: VReg, rn: VReg, rm: VReg, ra: VReg) -> u32 {
    rrr(0xce00_0000 | ra.code() << 10, rd, rn, rm)
}

/// `BCAX <Vd>.16B, <Vn>.16B, <Vm>.16B, <Va>.16B`: BCAX, `rn ^ (rm & !ra)`.
#[inline]
pub const fn bcax(rd: VReg, rn: VReg, rm: VReg, ra: VReg) -> u32 {
    rrr(0xce20_0000 | ra.code() << 10, rd, rn, rm)
}

checked! {
    /// `XAR <Vd>.2D, <Vn>.2D, <Vm>.2D, #amount`: XAR, `rn ^ rm` rotated
    /// right by `amount`.
    ///
    /// Precondition: `amount < 64`.
    pub fn xar / try_xar(rd: VReg, rn: VReg, rm: VReg, amount: u32)
    requires amount < 64;
    { rrr(0xce80_0000 | (amount & 0x3f) << 10, rd, rn, rm) }

    // Table lookups.

    /// `TBL <Vd>.<8B|16B>, {<Vn>.16B, ...}, <Vm>.<8B|16B>` (`extend`
    /// false) / `TBX` (`extend` true): a lookup in the `len` consecutive
    /// registers from `rn` (wrapping after `V31`). `TBL` gives zero for an
    /// index out of range, `TBX` keeps the element of `rd`.
    ///
    /// Precondition: `1 <= len <= 4`.
    pub fn tbl_n / try_tbl_n(extend: bool, len: u32, bits: VecBits, rd: VReg, rn: VReg, rm: VReg)
    requires len >= 1 && len <= 4;
    {
        let word = 0x0e00_0000 | (bits as u32) << 30 | ((len.wrapping_sub(1)) & 3) << 13;
        rrr(word | (extend as u32) << 12, rd, rn, rm)
    }
}

// Structure loads and stores.

ops! {
    /// The multiple-structure loads and stores: [`ldst_multi`]. `Ld1x2`
    /// is `LD1` of two registers, and so on.
    pub enum Multi {
        Ld1 = (0x0c40_7000, "ld1"),
        Ld1x2 = (0x0c40_a000, "ld1"),
        Ld1x3 = (0x0c40_6000, "ld1"),
        Ld1x4 = (0x0c40_2000, "ld1"),
        Ld2 = (0x0c40_8000, "ld2"),
        Ld3 = (0x0c40_4000, "ld3"),
        Ld4 = (0x0c40_0000, "ld4"),
        St1 = (0x0c00_7000, "st1"),
        St1x2 = (0x0c00_a000, "st1"),
        St1x3 = (0x0c00_6000, "st1"),
        St1x4 = (0x0c00_2000, "st1"),
        St2 = (0x0c00_8000, "st2"),
        St3 = (0x0c00_4000, "st3"),
        St4 = (0x0c00_0000, "st4"),
    }
}

impl Multi {
    /// The number of registers.
    #[inline]
    pub const fn registers(self) -> u32 {
        match self {
            Self::Ld1 | Self::St1 => 1,
            Self::Ld1x2 | Self::St1x2 | Self::Ld2 | Self::St2 => 2,
            Self::Ld1x3 | Self::St1x3 | Self::Ld3 | Self::St3 => 3,
            Self::Ld1x4 | Self::St1x4 | Self::Ld4 | Self::St4 => 4,
        }
    }

    /// Whether the case interleaves (`LD2`..`LD4`, `ST2`..`ST4`), which
    /// has no `1D` form.
    #[inline]
    pub const fn interleaves(self) -> bool {
        matches!(self, Self::Ld2 | Self::Ld3 | Self::Ld4 | Self::St2 | Self::St3 | Self::St4)
    }
}

checked! {
    /// `<op> {<Vt>.<T>, ...}, [<Xn|SP>]`: a multiple-structure load or
    /// store of `op.registers()` consecutive registers from `rt` (wrapping
    /// after `V31`), each `bits` of `elem`. `LD1` and `ST1` move the
    /// registers in order; the others (de)interleave elements.
    ///
    /// Precondition: an interleaving case is not `1D`.
    pub fn ldst_multi / try_ldst_multi(op: Multi, bits: VecBits, elem: Elem, rt: VReg, rn: RegSp)
    requires !op.interleaves() || is_arrangement(bits, elem);
    { op.word() | (bits as u32) << 30 | elem.log2() << 10 | rn.code() << 5 | rt.code() }

    /// `LD<n> {<Vt>.<Ts>, ...}[index], [<Xn|SP>]` (`load`) /
    /// `ST<n> ...`: a single-structure load or store of element `index` of
    /// the `n` consecutive registers from `rt`; a load keeps the other
    /// elements.
    ///
    /// Precondition: `1 <= n <= 4` and `index < elem.lanes()`.
    pub fn ldst_lane / try_ldst_lane(load: bool, n: u32, elem: Elem, index: u32, rt: VReg, rn: RegSp)
    requires n >= 1 && n <= 4 && index < elem.lanes();
    {
        let m = n.wrapping_sub(1) & 3;
        // Q:S:size holds the index, scaled by the element size.
        let (opcode, qss) = match elem {
            Elem::B => (0b000, index & 0xf),
            Elem::H => (0b010, (index & 7) << 1),
            Elem::S => (0b100, (index & 3) << 2),
            Elem::D => (0b100, (index & 1) << 3 | 1),
        };
        0x0d00_0000
            | (qss >> 3) << 30
            | (load as u32) << 22
            | (m & 1) << 21
            | (opcode | m >> 1) << 13
            | (qss >> 2 & 1) << 12
            | (qss & 3) << 10
            | rn.code() << 5
            | rt.code()
    }

    /// `LD<n>R {<Vt>.<T>, ...}, [<Xn|SP>]`: a single structure loaded and
    /// replicated to every element of the `n` consecutive registers from
    /// `rt`, each `bits` of `elem`.
    ///
    /// Precondition: `1 <= n <= 4`.
    pub fn ld_rep / try_ld_rep(n: u32, bits: VecBits, elem: Elem, rt: VReg, rn: RegSp)
    requires n >= 1 && n <= 4;
    {
        let m = n.wrapping_sub(1) & 3;
        0x0d40_c000
            | (bits as u32) << 30
            | (m & 1) << 21
            | (m >> 1) << 13
            | elem.log2() << 10
            | rn.code() << 5
            | rt.code()
    }
}
