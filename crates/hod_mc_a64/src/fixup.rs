//! PC-relative displacement fields: patching a displacement into an encoded
//! instruction and reading it back (`spec/mc-a64.md` "Fixups").

/// A PC-relative field of an instruction word.
///
/// A displacement is always the byte distance from the address of the
/// instruction that contains the field to the target, except for
/// [`Fixup::Adrp21`], where it is the distance between the two addresses'
/// 4 KiB pages: `(target & !0xfff) - (pc & !0xfff)`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Fixup {
    /// `imm26` of B and BL: a multiple of 4 in ±128 MiB.
    Jump26,
    /// `imm19` of B.cond, CBZ and CBNZ: a multiple of 4 in ±1 MiB.
    Branch19,
    /// `imm14` of TBZ and TBNZ: a multiple of 4 in ±32 KiB.
    Test14,
    /// `immhi:immlo` of ADR: any byte distance in ±1 MiB.
    Adr21,
    /// `immhi:immlo` of ADRP: a multiple of 4096 in ±4 GiB.
    Adrp21,
    /// `imm19` of LDR (literal), LDRSW (literal) and LDR (literal,
    /// SIMD&FP): a multiple of 4 in ±1 MiB.
    Literal19,
}

impl Fixup {
    /// Every fixup kind.
    pub const ALL: [Self; 6] =
        [Self::Jump26, Self::Branch19, Self::Test14, Self::Adr21, Self::Adrp21, Self::Literal19];

    /// (log2 of the unit the field counts in, field width in bits).
    #[inline]
    const fn shape(self) -> (u32, u32) {
        match self {
            Self::Jump26 => (2, 26),
            Self::Branch19 | Self::Literal19 => (2, 19),
            Self::Test14 => (2, 14),
            Self::Adr21 => (0, 21),
            Self::Adrp21 => (12, 21),
        }
    }

    /// The bits of the instruction word the field occupies.
    #[inline]
    pub const fn mask(self) -> u32 {
        match self {
            Self::Jump26 => 0x03ff_ffff,
            Self::Branch19 | Self::Literal19 => 0x00ff_ffe0,
            Self::Test14 => 0x0007_ffe0,
            Self::Adr21 | Self::Adrp21 => 0x60ff_ffe0,
        }
    }

    /// The smallest encodable displacement.
    #[inline]
    pub const fn min(self) -> i64 {
        let (scale, width) = self.shape();
        -(1i64 << (width - 1 + scale))
    }

    /// The largest encodable displacement.
    #[inline]
    pub const fn max(self) -> i64 {
        let (scale, width) = self.shape();
        ((1i64 << (width - 1)) - 1) << scale
    }

    /// Whether `disp` is encodable: in range and a multiple of the field's
    /// unit.
    #[inline]
    pub const fn fits(self, disp: i64) -> bool {
        let (scale, _) = self.shape();
        disp >= self.min() && disp <= self.max() && disp & ((1i64 << scale) - 1) == 0
    }

    /// The field bits for `disp`, in place. `disp` must fit.
    #[inline]
    pub(crate) const fn field(self, disp: i64) -> u32 {
        let (scale, width) = self.shape();
        let units = ((disp >> scale) as u32) & (u32::MAX >> (32 - width));
        match self {
            Self::Jump26 => units,
            Self::Branch19 | Self::Literal19 | Self::Test14 => units << 5,
            Self::Adr21 | Self::Adrp21 => (units & 3) << 29 | (units >> 2) << 5,
        }
    }

    /// Replaces the field in `word` with `disp`, leaving every other bit
    /// alone. `None` when `disp` is not encodable.
    #[inline]
    pub const fn patch(self, word: u32, disp: i64) -> Option<u32> {
        if self.fits(disp) { Some(word & !self.mask() | self.field(disp)) } else { None }
    }

    /// The displacement held in `word`'s field: the inverse of
    /// [`patch`](Self::patch).
    #[inline]
    pub const fn extract(self, word: u32) -> i64 {
        let (scale, width) = self.shape();
        let units = match self {
            Self::Jump26 => word & 0x03ff_ffff,
            Self::Branch19 | Self::Literal19 => (word >> 5) & 0x7_ffff,
            Self::Test14 => (word >> 5) & 0x3fff,
            Self::Adr21 | Self::Adrp21 => ((word >> 5) & 0x7_ffff) << 2 | (word >> 29) & 3,
        };
        // Sign-extend from `width` bits.
        let signed = ((units << (32 - width)) as i32) >> (32 - width);
        (signed as i64) << scale
    }
}
