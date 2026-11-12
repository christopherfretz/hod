//! AArch64 registers as [`PReg`] indexes: `spec/abi-a64.md` §2.
//!
//! The numbering is the one `hod_cg::PReg` documents for this target:
//! `x0`..`x30` are 0..=30, `sp` is 31, `v0`..`v31` are 32..=63.

use hod_cg::PReg;

/// The index of `v0`.
const VECTOR_BASE: u8 = 32;

/// The general register `x<n>`.
///
/// # Panics
/// Panics if `n` is above 30.
pub const fn x(n: u8) -> PReg {
    assert!(n <= 30, "the general registers are x0..x30");
    PReg::new(n)
}

/// The vector register `v<n>`.
///
/// # Panics
/// Panics if `n` is above 31.
pub const fn v(n: u8) -> PReg {
    assert!(n <= 31, "the vector registers are v0..v31");
    PReg::new(VECTOR_BASE + n)
}

/// Whether `reg` is one of `x0`..`x30`.
pub const fn is_gpr(reg: PReg) -> bool {
    reg.index() <= 30
}

/// Whether `reg` is one of `v0`..`v31`.
pub const fn is_fpr(reg: PReg) -> bool {
    reg.index() >= VECTOR_BASE
}

/// The number `n` of `x<n>` or `v<n>`; 31 for `sp`.
pub const fn number(reg: PReg) -> u8 {
    reg.index() % VECTOR_BASE
}

macro_rules! registers {
    ($make:ident: $($name:ident = $n:literal),* $(,)?) => {
        $(
            #[doc = concat!("`", stringify!($make), stringify!($n), "`.")]
            pub const $name: PReg = $make($n);
        )*
    };
}

registers!(x:
    X0 = 0, X1 = 1, X2 = 2, X3 = 3, X4 = 4, X5 = 5, X6 = 6, X7 = 7,
    X8 = 8, X9 = 9, X10 = 10, X11 = 11, X12 = 12, X13 = 13, X14 = 14, X15 = 15,
    X16 = 16, X17 = 17, X18 = 18, X19 = 19, X20 = 20, X21 = 21, X22 = 22, X23 = 23,
    X24 = 24, X25 = 25, X26 = 26, X27 = 27, X28 = 28, X29 = 29, X30 = 30,
);

registers!(v:
    V0 = 0, V1 = 1, V2 = 2, V3 = 3, V4 = 4, V5 = 5, V6 = 6, V7 = 7,
    V8 = 8, V9 = 9, V10 = 10, V11 = 11, V12 = 12, V13 = 13, V14 = 14, V15 = 15,
    V16 = 16, V17 = 17, V18 = 18, V19 = 19, V20 = 20, V21 = 21, V22 = 22, V23 = 23,
    V24 = 24, V25 = 25, V26 = 26, V27 = 27, V28 = 28, V29 = 29, V30 = 30, V31 = 31,
);

/// The frame pointer, `x29`.
pub const FP: PReg = X29;
/// The link register, `x30`.
pub const LR: PReg = X30;
/// The stack pointer.
pub const SP: PReg = PReg::new(31);

#[cfg(test)]
mod tests {
    use super::*;

    /// `spec/abi-a64.md` §2: the numbering.
    #[test]
    fn numbering() {
        assert_eq!(X0.index(), 0);
        assert_eq!(X30.index(), 30);
        assert_eq!(SP.index(), 31);
        assert_eq!(V0.index(), 32);
        assert_eq!(V31.index(), 63);
        assert_eq!((FP, LR), (X29, X30));
        for n in 0..=30 {
            assert!(is_gpr(x(n)) && !is_fpr(x(n)));
            assert_eq!(number(x(n)), n);
        }
        for n in 0..=31 {
            assert!(is_fpr(v(n)) && !is_gpr(v(n)));
            assert_eq!(number(v(n)), n);
        }
        assert!(!is_gpr(SP) && !is_fpr(SP));
        assert_eq!(number(SP), 31);
    }
}
