//! Relocatable fields: patching a value into emitted code and reading it
//! back (`spec/mc-x64.md` "Fixups and relocations").

/// A field of an instruction that a fixup or a relocation fills: a
/// little-endian signed value.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Fixup {
    /// `rel8` of the short branches: a displacement from the end of the
    /// instruction.
    Rel8,
    /// `rel32` of the near branches and calls, and the `disp32` of a
    /// RIP-relative memory operand: a displacement from the end of the
    /// instruction.
    Rel32,
    /// The `disp32` of a memory operand marked
    /// [`with_abs32`](crate::Mem::with_abs32): an absolute value, not
    /// relative to the instruction (`R_X86_64_TPOFF32`,
    /// `R_X86_64_DTPOFF32`, `R_X86_64_32S`).
    Abs32,
}

impl Fixup {
    /// Every fixup kind.
    pub const ALL: [Self; 3] = [Self::Rel8, Self::Rel32, Self::Abs32];

    /// Whether the field holds a distance from the end of the
    /// instruction (`Rel8`, `Rel32`) rather than an absolute value.
    #[inline]
    pub const fn is_pc_relative(self) -> bool {
        !matches!(self, Self::Abs32)
    }

    /// The field's width in bytes.
    #[inline]
    pub const fn size(self) -> usize {
        match self {
            Self::Rel8 => 1,
            Self::Rel32 | Self::Abs32 => 4,
        }
    }

    /// The smallest encodable value.
    #[inline]
    pub const fn min(self) -> i64 {
        match self {
            Self::Rel8 => i8::MIN as i64,
            Self::Rel32 | Self::Abs32 => i32::MIN as i64,
        }
    }

    /// The largest encodable value.
    #[inline]
    pub const fn max(self) -> i64 {
        match self {
            Self::Rel8 => i8::MAX as i64,
            Self::Rel32 | Self::Abs32 => i32::MAX as i64,
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
        Some(match self {
            Self::Rel8 => code[at] as i8 as i64,
            Self::Rel32 | Self::Abs32 => {
                i32::from_le_bytes([code[at], code[at + 1], code[at + 2], code[at + 3]]) as i64
            }
        })
    }
}
