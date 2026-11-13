//! Traps, CET markers and padding (`spec/mc-x64.md` "System and padding"),
//! the counters and `cpuid`, and the repeated string moves and stores
//! ("String instructions").

use crate::inst::{Inst, Op, plain};
use crate::reg::Size;

/// `UD2` (`0F 0B`): raises an invalid-opcode exception.
#[inline]
pub const fn ud2() -> Inst {
    plain(Op::two(0x0b))
}

/// `UD1 eax, [eax + disp]` with 32-bit addressing: `67 0F B9 00` for
/// 0, `67 0F B9 40 ib` up to 127, else `67 0F B9 80 id`. Raises `#UD`
/// like `ud2`; what `llc` 19 and 22 emit for `llvm.ubsantrap(kind)`
/// (`ud1l kind(%eax), %eax`), the kind in the displacement for a
/// handler to read.
#[inline]
pub const fn ud1_eax(disp: u8) -> Inst {
    let mut inst = Inst::EMPTY;
    inst.push(0x67);
    inst.push(0x0f);
    inst.push(0xb9);
    if disp == 0 {
        inst.push(0x00);
    } else if disp <= 127 {
        inst.push(0x40);
        inst.push(disp);
    } else {
        inst.push(0x80);
        inst.push(disp);
        inst.push(0);
        inst.push(0);
        inst.push(0);
    }
    inst
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

/// `RDTSC` (`0F 31`): the time-stamp counter into `edx:eax`.
#[inline]
pub const fn rdtsc() -> Inst {
    plain(Op::two(0x31))
}

/// `RDTSCP` (`0F 01 F9`): the time-stamp counter into `edx:eax` and
/// `IA32_TSC_AUX` into `ecx`, after the earlier instructions have
/// executed. Needs the RDTSCP CPUID feature.
#[inline]
pub const fn rdtscp() -> Inst {
    let mut inst = plain(Op::two(0x01));
    inst.push(0xf9);
    inst
}

/// `CPUID` (`0F A2`): leaf `eax`, subleaf `ecx`, into `eax`, `ebx`,
/// `ecx`, `edx`.
#[inline]
pub const fn cpuid() -> Inst {
    plain(Op::two(0xa2))
}

/// `CLD` (`FC`): clears the direction flag, so the string instructions
/// count upward.
#[inline]
pub const fn cld() -> Inst {
    plain(Op::one(0xfc))
}

/// `STD` (`FD`): sets the direction flag, so the string instructions
/// count downward (inline assembly, WP-187).
#[inline]
pub const fn std() -> Inst {
    plain(Op::one(0xfd))
}

/// `FWAIT` (`9B`, also `WAIT`): waits for pending x87 exceptions.
/// Inline assembly, WP-187.
#[inline]
pub const fn fwait() -> Inst {
    plain(Op::one(0x9b))
}

/// `HLT` (`F4`): halts until an interrupt; privileged (a `#GP` in user
/// mode). Inline assembly, WP-187.
#[inline]
pub const fn hlt() -> Inst {
    plain(Op::one(0xf4))
}

/// `SYSCALL` (`0F 05`): the system call; it writes `rcx` and `r11`.
/// Inline assembly, WP-187.
#[inline]
pub const fn syscall() -> Inst {
    plain(Op::two(0x05))
}

/// `INT imm8` (`CD ib`): the software interrupt `n`. LLVM's assembler
/// writes `int $3` as `INT3` (`CC`), so `n == 3` is [`int3`]. Inline
/// assembly, WP-187.
#[inline]
pub const fn int_i(n: u8) -> Inst {
    if n == 3 {
        return int3();
    }
    let mut inst = plain(Op::one(0xcd));
    inst.push(n);
    inst
}

/// `F3` in front of `inst`: LLVM emits `REP` as an instruction of its
/// own, so it precedes even the `66` of a 16-bit operation.
#[inline]
const fn rep(inst: Inst) -> Inst {
    let mut out = Inst::EMPTY;
    out.push(0xf3);
    let bytes = inst.bytes();
    let mut i = 0;
    while i < bytes.len() {
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// `REP MOVSB`/`MOVSW`/`MOVSD`/`MOVSQ` (`F3 [66] [REX.W] A4`/`A5`):
/// copies `rcx` elements of `size` from `[rsi]` to `[rdi]`, advancing
/// both (downward if the direction flag is set); `rcx` ends at 0.
#[inline]
pub const fn rep_movs(size: Size) -> Inst {
    rep(plain(Op::one(0xa4).sized(size, 1)))
}

/// `REP STOSB`/`STOSW`/`STOSD`/`STOSQ` (`F3 [66] [REX.W] AA`/`AB`):
/// stores `al`/`ax`/`eax`/`rax` to `rcx` elements of `size` at `[rdi]`,
/// advancing it; `rcx` ends at 0.
#[inline]
pub const fn rep_stos(size: Size) -> Inst {
    rep(plain(Op::one(0xaa).sized(size, 1)))
}
