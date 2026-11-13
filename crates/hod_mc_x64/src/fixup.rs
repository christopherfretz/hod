//! Relocatable fields: patching a value into emitted code and reading it
//! back (`spec/mc-x64.md` "Fixups and relocations").

/// A field of an instruction that a fixup or a relocation fills: a
/// little-endian value, signed except for [`Fixup::Abs32U`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Fixup {
    /// `rel8` of the short branches: a displacement from the end of the
    /// instruction.
    Rel8,
    /// `rel32` of the near branches and calls, and the `disp32` of a
    /// RIP-relative memory operand: a displacement from the end of the
    /// instruction.
    Rel32,
    /// A signed absolute 32-bit field, not relative to the instruction:
    /// the `disp32` of a memory operand marked
    /// [`with_abs32`](crate::Mem::with_abs32) (`R_X86_64_TPOFF32`,
    /// `R_X86_64_DTPOFF32`, `R_X86_64_32S`), and the sign-extended `imm32`
    /// of [`mov_ri32s`](crate::mov_ri32s), [`mov_mi32s`](crate::mov_mi32s)
    /// and [`push_i32`](crate::push_i32) (`R_X86_64_32S`).
    Abs32,
    /// An unsigned absolute 32-bit field: the zero-extended `imm32` of
    /// [`mov_ri32`](crate::mov_ri32) (`R_X86_64_32`).
    Abs32U,
    /// An absolute 64-bit field: the `imm64` of
    /// [`movabs_ri`](crate::movabs_ri) (`R_X86_64_64`).
    Abs64,
}

impl Fixup {
    /// Every fixup kind.
    pub const ALL: [Self; 5] = [Self::Rel8, Self::Rel32, Self::Abs32, Self::Abs32U, Self::Abs64];

    /// Whether the field holds a distance from the end of the
    /// instruction (`Rel8`, `Rel32`) rather than an absolute value.
    #[inline]
    pub const fn is_pc_relative(self) -> bool {
        matches!(self, Self::Rel8 | Self::Rel32)
    }

    /// The field's width in bytes.
    #[inline]
    pub const fn size(self) -> usize {
        match self {
            Self::Rel8 => 1,
            Self::Rel32 | Self::Abs32 | Self::Abs32U => 4,
            Self::Abs64 => 8,
        }
    }

    /// The smallest encodable value.
    #[inline]
    pub const fn min(self) -> i64 {
        match self {
            Self::Rel8 => i8::MIN as i64,
            Self::Rel32 | Self::Abs32 => i32::MIN as i64,
            Self::Abs32U => 0,
            Self::Abs64 => i64::MIN,
        }
    }

    /// The largest encodable value.
    #[inline]
    pub const fn max(self) -> i64 {
        match self {
            Self::Rel8 => i8::MAX as i64,
            Self::Rel32 | Self::Abs32 => i32::MAX as i64,
            Self::Abs32U => u32::MAX as i64,
            Self::Abs64 => i64::MAX,
        }
    }

    /// Whether `disp` is encodable in the field.
    #[inline]
    pub const fn fits(self, disp: i64) -> bool {
        disp >= self.min() && disp <= self.max()
    }

    /// Writes `disp` into the field at `code[at..]`. Returns `false`, and
    /// leaves `code` unchanged, when `disp` does not fit or the field does
    /// not lie inside `code`.
    #[inline]
    pub const fn patch(self, code: &mut [u8], at: usize, disp: i64) -> bool {
        let n = self.size();
        if !self.fits(disp) || at > code.len() || code.len() - at < n {
            return false;
        }
        let mut i = 0;
        while i < n {
            code[at + i] = (disp >> (8 * i)) as u8;
            i += 1;
        }
        true
    }

    /// The value held in the field at `code[at..]`, the inverse of
    /// [`patch`](Self::patch); `None` when the field does not lie inside
    /// `code`.
    #[inline]
    pub const fn extract(self, code: &[u8], at: usize) -> Option<i64> {
        if at > code.len() || code.len() - at < self.size() {
            return None;
        }
        let mut v: u64 = 0;
        let mut i = self.size();
        while i > 0 {
            i -= 1;
            v = v << 8 | code[at + i] as u64;
        }
        Some(match self {
            Self::Rel8 => v as u8 as i8 as i64,
            Self::Rel32 | Self::Abs32 => v as u32 as i32 as i64,
            Self::Abs32U | Self::Abs64 => v as i64,
        })
    }
}
