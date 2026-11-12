//! PC-relative fields: patching a displacement into emitted code and
//! reading it back (`spec/mc-x64.md` "Fixups and relocations").

/// A PC-relative field: a little-endian signed displacement from the end
/// of the instruction that contains it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Fixup {
    /// `rel8` of the short branches.
    Rel8,
    /// `rel32` of the near branches and calls, and the `disp32` of a
    /// RIP-relative memory operand.
    Rel32,
}

impl Fixup {
    /// Every fixup kind.
    pub const ALL: [Self; 2] = [Self::Rel8, Self::Rel32];

    /// The field's width in bytes.
    #[inline]
    pub const fn size(self) -> usize {
        match self {
            Self::Rel8 => 1,
            Self::Rel32 => 4,
        }
    }

    /// The smallest encodable displacement.
    #[inline]
    pub const fn min(self) -> i64 {
        match self {
            Self::Rel8 => i8::MIN as i64,
            Self::Rel32 => i32::MIN as i64,
        }
    }

    /// The largest encodable displacement.
    #[inline]
    pub const fn max(self) -> i64 {
        match self {
            Self::Rel8 => i8::MAX as i64,
            Self::Rel32 => i32::MAX as i64,
        }
    }

    /// Whether `disp` is encodable.
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

    /// The displacement held in the field at `code[at..]`, the inverse of
    /// [`patch`](Self::patch); `None` when the field does not lie inside
    /// `code`.
    #[inline]
    pub const fn extract(self, code: &[u8], at: usize) -> Option<i64> {
        if at > code.len() || code.len() - at < self.size() {
            return None;
        }
        Some(match self {
            Self::Rel8 => code[at] as i8 as i64,
            Self::Rel32 => {
                i32::from_le_bytes([code[at], code[at + 1], code[at + 2], code[at + 3]]) as i64
            }
        })
    }
}
