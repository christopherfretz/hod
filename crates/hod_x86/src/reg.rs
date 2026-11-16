//! The registers as `hod_cg::PReg` indexes, and their DWARF numbers
//! (`spec/codegen-x86.md` "Registers"). Owned by WP-293.
//!
//! A general register's index is its hardware number (`eax` 0 ..
//! `edi` 7); `xmm<n>` is `8 + n`. The DWARF numbers are the i386 psABI's
//! (as `llvm-dwarfdump` names them for `EM_386`): `eax` 0, `ecx` 1,
//! `edx` 2, `ebx` 3, `esp` 4, `ebp` 5, `esi` 6, `edi` 7, `eip` 8,
//! `st0`..`st7` 11..18, `xmm0`..`xmm7` 21..28.

use hod_cg::PReg;
use hod_mc_x64::Reg;

/// The first `xmm` index.
const XMM_BASE: u8 = 8;

/// `eax`: results, the dividend, never a value's register.
pub const EAX: PReg = PReg::new(0);
/// `ecx`: scratch (shift counts).
pub const ECX: PReg = PReg::new(1);
/// `edx`: scratch, the callee register, the high half of products.
pub const EDX: PReg = PReg::new(2);
/// `ebx`: pool, callee-saved.
pub const EBX: PReg = PReg::new(3);
/// `esp`: the stack pointer.
pub const ESP: PReg = PReg::new(4);
/// `ebp`: the frame pointer.
pub const EBP: PReg = PReg::new(5);
/// `esi`: pool, callee-saved.
pub const ESI: PReg = PReg::new(6);
/// `edi`: pool, callee-saved.
pub const EDI: PReg = PReg::new(7);

/// `xmm<n>`, `n` < 8 (for the SSE2 package, design 0008 I12).
pub const fn xmm(n: u8) -> PReg {
    assert!(n < 8, "i386 has xmm0..xmm7");
    PReg::new(XMM_BASE + n)
}

/// Whether `reg` is a general register.
pub const fn is_gpr(reg: PReg) -> bool {
    reg.index() < XMM_BASE
}

/// The `PReg` of an encoder register.
pub const fn gpr(reg: Reg) -> PReg {
    PReg::new(reg.code())
}

/// The DWARF register number of `reg`.
pub const fn dwarf(reg: PReg) -> u16 {
    let n = reg.index();
    if n < XMM_BASE {
        n as u16
    } else if n < ST_BASE {
        21 + (n - XMM_BASE) as u16
    } else {
        11 + (n - ST_BASE) as u16
    }
}

/// The first x87 index (`st0`; design 0008 I9). No value lives in one:
/// the indexes exist for their DWARF numbers.
const ST_BASE: u8 = 16;

/// `st<n>`, `n` < 8.
pub const fn st(n: u8) -> PReg {
    assert!(n < 8, "the x87 stack has st0..st7");
    PReg::new(ST_BASE + n)
}

/// The DWARF number of `ebp`.
pub const DWARF_EBP: u16 = 5;
/// The DWARF number of `esp`.
pub const DWARF_ESP: u16 = 4;

/// The encoder's register of a general `PReg` (its 32-bit view).
///
/// # Panics
/// For a register that is no general register: an internal error of the
/// lowering, never of the input.
pub const fn reg(reg: PReg) -> Reg {
    match Reg::new(reg.index()) {
        Some(r) if is_gpr(reg) => r,
        _ => panic!("not a general register"),
    }
}

/// Whether the low byte of `reg` is addressable without a REX prefix
/// (`al`, `cl`, `dl`, `bl`): in 32-bit mode `esi`, `edi`, `ebp`, `esp`
/// have none.
pub const fn has_low_byte(reg: Reg) -> bool {
    reg.code() < 4
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dwarf_numbers_are_the_i386_psabi_ones() {
        assert_eq!(dwarf(EAX), 0);
        assert_eq!(dwarf(EBX), 3);
        assert_eq!(dwarf(EBP), DWARF_EBP);
        assert_eq!(dwarf(ESP), DWARF_ESP);
        assert_eq!(dwarf(EDI), 7);
        assert_eq!(dwarf(xmm(0)), 21);
        assert_eq!(dwarf(xmm(7)), 28);
        assert_eq!(dwarf(st(0)), 11);
        assert_eq!(dwarf(st(7)), 18);
        assert!(has_low_byte(reg(EBX)) && !has_low_byte(reg(ESI)));
    }
}
