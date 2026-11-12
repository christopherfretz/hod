//! Traps, CET markers and padding (`spec/mc-x64.md` "System and padding").

use crate::inst::{Inst, Op, plain};

/// `UD2` (`0F 0B`): raises an invalid-opcode exception.
#[inline]
pub const fn ud2() -> Inst {
    plain(Op::two(0x0b))
}

/// `INT3` (`CC`): a breakpoint.
#[inline]
pub const fn int3() -> Inst {
    plain(Op::one(0xcc))
}

/// `ENDBR64` (`F3 0F 1E FA`): an indirect-branch target under CET IBT; a
/// `nop` elsewhere.
#[inline]
pub const fn endbr64() -> Inst {
    let mut inst = plain(Op::two(0x1e).rep(0xf3));
    inst.push(0xfa);
    inst
}

/// The recommended `NOP` of each length: Intel SDM Vol. 2B, "NOP", table
/// 4-12 for 1 to 9 bytes; above that, `66 2E` and then further `66`
/// prefixes on the 9-byte form, as LLVM pads.
const NOP9: [u8; 9] = [0x66, 0x0f, 0x1f, 0x84, 0, 0, 0, 0, 0];

/// Whether `len` is a length [`nop`] produces.
#[inline]
pub const fn nop_len_ok(len: usize) -> bool {
    len >= 1 && len <= crate::MAX_LEN
}

checked! {
    /// One `NOP` instruction of exactly `len` bytes, `1..=15`.
    pub fn nop / try_nop(len: usize)
    requires nop_len_ok(len);
    {
        let mut inst = Inst::EMPTY;
        let body: &[u8] = match len {
            1 => &[0x90],
            2 => &[0x66, 0x90],
            3 => &[0x0f, 0x1f, 0x00],
            4 => &[0x0f, 0x1f, 0x40, 0x00],
            5 => &[0x0f, 0x1f, 0x44, 0x00, 0x00],
            6 => &[0x66, 0x0f, 0x1f, 0x44, 0x00, 0x00],
            7 => &[0x0f, 0x1f, 0x80, 0, 0, 0, 0],
            8 => &[0x0f, 0x1f, 0x84, 0, 0, 0, 0, 0],
            _ => &NOP9,
        };
        let mut extra = if len > 9 { len - 10 } else { 0 };
        while extra > 0 {
            inst.push(0x66);
            extra -= 1;
        }
        if len > 9 {
            inst.push(0x66);
            inst.push(0x2e);
            // The 9-byte form without its own 0x66, which was pushed above.
            let mut i = 1;
            while i < NOP9.len() {
                inst.push(NOP9[i]);
                i += 1;
            }
        } else {
            let mut i = 0;
            while i < body.len() {
                inst.push(body[i]);
                i += 1;
            }
        }
        inst
    }
}
