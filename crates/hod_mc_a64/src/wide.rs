//! Move wide (MOVZ/MOVN/MOVK) and the constant materializer
//! (`spec/mc-a64.md` "Moves" and "Constant materialization").

use crate::imm::encode_bitmask;
use crate::logical::mov_bitmask;
use crate::reg::{Reg, RegZr, Size};

const MOVN: u32 = 0;
const MOVZ: u32 = 2;
const MOVK: u32 = 3;

#[inline]
const fn move_wide(sz: Size, opc: u32, rd: RegZr, imm16: u16, shift: u32) -> u32 {
    sz.sf() | opc << 29 | 0x1280_0000 | ((shift / 16) & 3) << 21 | (imm16 as u32) << 5 | rd.code()
}

/// A move-wide shift: a multiple of 16 below the operand size.
#[inline]
const fn wide_shift_ok(sz: Size, shift: u32) -> bool {
    shift.is_multiple_of(16) && shift < sz.bits()
}

checked! {
    /// `MOVZ <Rd>, #imm16, LSL #shift`: MOVZ (`rd = imm16 << shift`).
    ///
    /// Precondition: `shift` is 0 or 16, or for `Size::X` also 32 or 48.
    pub fn movz / try_movz(sz: Size, rd: RegZr, imm16: u16, shift: u32)
    requires wide_shift_ok(sz, shift);
    { move_wide(sz, MOVZ, rd, imm16, shift) }

    /// `MOVN <Rd>, #imm16, LSL #shift`: MOVN (`rd = !(imm16 << shift)`).
    ///
    /// Precondition: `shift` is 0 or 16, or for `Size::X` also 32 or 48.
    pub fn movn / try_movn(sz: Size, rd: RegZr, imm16: u16, shift: u32)
    requires wide_shift_ok(sz, shift);
    { move_wide(sz, MOVN, rd, imm16, shift) }

    /// `MOVK <Rd>, #imm16, LSL #shift`: MOVK (replace 16 bits of `rd`).
    ///
    /// Precondition: `shift` is 0 or 16, or for `Size::X` also 32 or 48.
    pub fn movk / try_movk(sz: Size, rd: RegZr, imm16: u16, shift: u32)
    requires wide_shift_ok(sz, shift);
    { move_wide(sz, MOVK, rd, imm16, shift) }
}

/// The instructions that load one constant into a register: one to four
/// words, produced by [`mov_imm`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MovSeq {
    words: [u32; 4],
    len: u8,
}

impl MovSeq {
    const fn new() -> Self {
        Self { words: [0; 4], len: 0 }
    }

    const fn push(mut self, word: u32) -> Self {
        self.words[self.len as usize] = word;
        self.len += 1;
        self
    }

    /// The instruction words, in execution order.
    #[inline]
    pub const fn as_slice(&self) -> &[u32] {
        self.words.split_at(self.len as usize).0
    }

    /// The number of instructions, `1..=4`.
    #[inline]
    #[allow(clippy::len_without_is_empty, reason = "a sequence is never empty")]
    pub const fn len(&self) -> usize {
        self.len as usize
    }
}

/// The 16-bit chunk of `value` at chunk index `index`.
#[inline]
const fn chunk(value: u64, index: u32) -> u16 {
    (value >> (index * 16)) as u16
}

/// `value` with the chunk at `index` replaced by `with`.
#[inline]
const fn with_chunk(value: u64, index: u32, with: u16) -> u64 {
    value & !(0xffff << (index * 16)) | (with as u64) << (index * 16)
}

/// Materializes a constant: the shortest sequence that sets `rd` to `value`
/// among sequences made of one MOVZ, MOVN or MOV (bitmask immediate, that
/// is ORR from the zero register) followed by MOVKs on `rd`.
///
/// For [`Size::W`] the constant is the low 32 bits of `value`; the upper
/// bits are ignored (and the sequence clears the upper half of the X
/// register, as every 32-bit operation does).
///
/// Among sequences of the minimal length the choice is fixed: MOVZ-first
/// before MOVN-first before ORR-first.
///
/// The destination is a [`Reg`] because the forms disagree about encoding
/// 31: it is ZR for MOVZ/MOVN/MOVK and SP for ORR (immediate).
pub const fn mov_imm(sz: Size, rd: Reg, value: u64) -> MovSeq {
    let (value, chunks) = match sz {
        Size::W => (value & 0xffff_ffff, 2),
        Size::X => (value, 4),
    };
    let mut nonzero = 0;
    let mut nonones = 0;
    let mut i = 0;
    while i < chunks {
        nonzero += (chunk(value, i) != 0) as u32;
        nonones += (chunk(value, i) != 0xffff) as u32;
        i += 1;
    }
    let use_movn = nonones < nonzero;
    let wide_len = if use_movn { nonones } else { nonzero };
    // An ORR-first sequence of length k keeps 5 - k of the four chunks from
    // the bitmask immediate, so it can only win when the wide form needs
    // more than k instructions.
    if wide_len > 1 {
        if encode_bitmask(sz, value).is_some() {
            return MovSeq::new().push(mov_bitmask(sz, rd.sp(), value));
        }
        if wide_len > 2
            && let Some(seq) = orr_then_movk(rd, value, wide_len > 3)
        {
            return seq;
        }
    }
    let skip: u16 = if use_movn { 0xffff } else { 0 };
    let rd = rd.zr();
    let mut seq = MovSeq::new();
    let mut i = 0;
    while i < chunks {
        let c = chunk(value, i);
        if c != skip {
            seq = seq.push(if seq.len != 0 {
                movk(sz, rd, c, i * 16)
            } else if use_movn {
                movn(sz, rd, !c, i * 16)
            } else {
                movz(sz, rd, c, i * 16)
            });
        }
        i += 1;
    }
    if seq.len == 0 {
        // All chunks equal the background: zero, or all ones.
        seq = seq.push(if use_movn { movn(sz, rd, 0, 0) } else { movz(sz, rd, 0, 0) });
    }
    seq
}

/// A 64-bit ORR-first sequence with one MOVK, or with two when `two` is set
/// and no one-MOVK sequence exists.
///
/// The search is complete (`spec/mc-a64.md` "Constant materialization"): if
/// any bitmask immediate agrees with `value` outside one (two) chunks, then
/// one does whose replaced chunks are each `0x0000`, `0xffff` or a chunk of
/// `value`.
const fn orr_then_movk(rd: Reg, value: u64, two: bool) -> Option<MovSeq> {
    let fills = [0, 0xffff, chunk(value, 0), chunk(value, 1), chunk(value, 2), chunk(value, 3)];
    let mut i = 0;
    while i < 4 {
        let mut f = 0;
        while f < fills.len() {
            let base = with_chunk(value, i, fills[f]);
            if encode_bitmask(Size::X, base).is_some() {
                return Some(MovSeq::new().push(mov_bitmask(Size::X, rd.sp(), base)).push(movk(
                    Size::X,
                    rd.zr(),
                    chunk(value, i),
                    i * 16,
                )));
            }
            f += 1;
        }
        i += 1;
    }
    if !two {
        return None;
    }
    let mut i = 0;
    while i < 4 {
        let mut j = i + 1;
        while j < 4 {
            let mut f = 0;
            while f < fills.len() * fills.len() {
                let base = with_chunk(
                    with_chunk(value, i, fills[f / fills.len()]),
                    j,
                    fills[f % fills.len()],
                );
                if encode_bitmask(Size::X, base).is_some() {
                    return Some(
                        MovSeq::new()
                            .push(mov_bitmask(Size::X, rd.sp(), base))
                            .push(movk(Size::X, rd.zr(), chunk(value, i), i * 16))
                            .push(movk(Size::X, rd.zr(), chunk(value, j), j * 16)),
                    );
                }
                f += 1;
            }
            j += 1;
        }
        i += 1;
    }
    None
}
