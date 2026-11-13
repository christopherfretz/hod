//! x86-64 registers as [`PReg`] indexes and their DWARF numbers
//! (`spec/codegen.md` §5.6).
//!
//! The numbering follows the encoder's register numbers: `rax`..`r15`
//! are 0..=15 (`hod_mc_x64::Reg::code`), `xmm0`..`xmm15` are 16..=31.
//! The DWARF numbers are the System V psABI's (figure 3.36): `rax`,
//! `rdx`, `rcx`, `rbx`, `rsi`, `rdi`, `rbp`, `rsp` are 0..=7, `r8`..`r15`
//! are 8..=15, the return address (`rip`) is 16, `xmm0`..`xmm15` are
//! 17..=32, `st0`..`st7` are 33..=40.
//!
//! `st0`..`st7` (32..=39, the x87 stack from its top) are no register of
//! the allocator: they appear only as places of an argument assignment
//! (`spec/abi-x64.md` §2, §5.1), where an `x86_fp80` or a `float` or
//! `double` piece of a return value travels.

use hod_cg::PReg;
use hod_mc_x64::{Reg, Xmm};

/// The index of `xmm0`.
const XMM_BASE: u8 = 16;

/// The index of `st0`.
const X87_BASE: u8 = 32;

/// The general register `reg`.
pub const fn gpr(reg: Reg) -> PReg {
    PReg::new(reg.code())
}

/// The vector register `xmm<n>`.
pub const fn xmm(reg: Xmm) -> PReg {
    PReg::new(XMM_BASE + reg.code())
}

/// `st(<n>)`, the x87 stack slot `n` from the top.
///
/// # Panics
/// Panics if `n` is 8 or more.
pub const fn st(n: u8) -> PReg {
    assert!(n < 8, "the x87 stack is st0..st7");
    PReg::new(X87_BASE + n)
}

/// Whether `reg` is one of `st0`..`st7`.
pub const fn is_x87(reg: PReg) -> bool {
    reg.index() >= X87_BASE && reg.index() < X87_BASE + 8
}

/// The number of `reg` within its file: the encoding of a general
/// register, `n` of `xmm<n>` or of `st(<n>)`.
pub const fn number(reg: PReg) -> u8 {
    reg.index() % 16
}

/// Whether `reg` is one of `rax`..`r15`.
pub const fn is_gpr(reg: PReg) -> bool {
    reg.index() < XMM_BASE
}

/// Whether `reg` is one of `xmm0`..`xmm15`.
pub const fn is_fpr(reg: PReg) -> bool {
    reg.index() >= XMM_BASE && reg.index() < XMM_BASE + 16
}

/// The encoder's general register of `reg`.
///
/// # Panics
/// Panics for a register that is not general: a bug of the caller.
pub const fn reg(reg: PReg) -> Reg {
    match Reg::new(reg.index()) {
        Some(r) if is_gpr(reg) => r,
        _ => panic!("not a general register"),
    }
}

/// The encoder's vector register of `reg`.
///
/// # Panics
/// Panics for a register that is not a vector register.
pub const fn xreg(reg: PReg) -> Xmm {
    match Xmm::new(reg.index().wrapping_sub(XMM_BASE)) {
        Some(x) if is_fpr(reg) => x,
        _ => panic!("not a vector register"),
    }
}

/// The DWARF number of the general register numbered `code`
/// (psABI order: `rax`, `rdx`, `rcx`, `rbx`, `rsi`, `rdi`, `rbp`, `rsp`).
const GPR_DWARF: [u16; 16] = [0, 2, 1, 3, 7, 6, 4, 5, 8, 9, 10, 11, 12, 13, 14, 15];

/// The DWARF number of `reg`.
pub fn dwarf(reg: PReg) -> u16 {
    if is_fpr(reg) {
        DWARF_XMM0 + u16::from(reg.index() - XMM_BASE)
    } else if is_x87(reg) {
        DWARF_ST0 + u16::from(reg.index() - X87_BASE)
    } else {
        GPR_DWARF[usize::from(reg.index() & 15)]
    }
}

/// DWARF `rbp`: the frame pointer and the frame base.
pub const DWARF_RBP: u16 = 6;
/// DWARF `rsp`.
pub const DWARF_RSP: u16 = 7;
/// DWARF `rip`: the return-address column.
pub const DWARF_RIP: u16 = 16;
/// DWARF `xmm0`.
pub const DWARF_XMM0: u16 = 17;
/// DWARF `st0`.
pub const DWARF_ST0: u16 = 33;

macro_rules! registers {
    ($($name:ident = $make:ident($r:expr)),* $(,)?) => {
        $(
            #[doc = concat!("`", stringify!($r), "`.")]
            pub const $name: PReg = $make($r);
        )*
    };
}

registers!(
    RAX = gpr(Reg::Rax),
    RCX = gpr(Reg::Rcx),
    RDX = gpr(Reg::Rdx),
    RBX = gpr(Reg::Rbx),
    RSP = gpr(Reg::Rsp),
    RBP = gpr(Reg::Rbp),
    RSI = gpr(Reg::Rsi),
    RDI = gpr(Reg::Rdi),
    R8 = gpr(Reg::R8),
    R9 = gpr(Reg::R9),
    R10 = gpr(Reg::R10),
    R11 = gpr(Reg::R11),
    R12 = gpr(Reg::R12),
    R13 = gpr(Reg::R13),
    R14 = gpr(Reg::R14),
    R15 = gpr(Reg::R15),
    XMM0 = xmm(Xmm::Xmm0),
    XMM7 = xmm(Xmm::Xmm7),
    XMM8 = xmm(Xmm::Xmm8),
    XMM13 = xmm(Xmm::Xmm13),
    XMM14 = xmm(Xmm::Xmm14),
    XMM15 = xmm(Xmm::Xmm15),
    ST0 = st(0),
    ST1 = st(1),
);

#[cfg(test)]
mod tests {
    use super::*;

    /// §5.6: the numbering and the psABI's DWARF numbers.
    #[test]
    fn numbering_and_dwarf() {
        assert_eq!((RAX.index(), R15.index(), XMM0.index(), XMM15.index()), (0, 15, 16, 31));
        let expected = [
            (RAX, 0),
            (RDX, 1),
            (RCX, 2),
            (RBX, 3),
            (RSI, 4),
            (RDI, 5),
            (RBP, 6),
            (RSP, 7),
            (R8, 8),
            (R15, 15),
            (XMM0, 17),
            (XMM15, 32),
            (ST0, 33),
            (st(7), 40),
        ];
        for (reg, number) in expected {
            assert_eq!(dwarf(reg), number, "{reg:?}");
        }
        for r in Reg::ALL {
            assert!(is_gpr(gpr(r)) && !is_fpr(gpr(r)));
            assert_eq!(reg(gpr(r)), r);
        }
        for x in Xmm::ALL {
            assert!(is_fpr(xmm(x)) && !is_gpr(xmm(x)) && !is_x87(xmm(x)));
            assert_eq!(xreg(xmm(x)), x);
            assert_eq!(number(xmm(x)), x.code());
        }
        for n in 0..8 {
            assert!(is_x87(st(n)) && !is_gpr(st(n)) && !is_fpr(st(n)));
            assert_eq!(number(st(n)), n);
        }
    }
}
