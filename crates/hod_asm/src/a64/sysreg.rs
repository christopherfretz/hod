//! The system registers of `mrs` and `msr` (`spec/asm.md` §5.4, WP-188):
//! the names user-space and portable code reads and writes, and the
//! generic `S<op0>_<op1>_C<n>_C<m>_<op2>` form.
//!
//! [`TABLE`] was generated once from `llvm-mc-22 --triple=aarch64`: the
//! names it prints for the ID-register space (`op0=3, op1=0, CRn=0,
//! CRm=0..7`, what Linux emulates for user space) and a fixed list of
//! others ([`EXTRA`]), with the encoding and the accesses `llvm-mc`
//! accepts. `hod_asm`'s test `sysreg_table_is_llvm_mc_s` regenerates it
//! when the tool is present and compares. A name `llvm-mc` gates behind a
//! feature carries it ([`RAND`]); the names behind other features (SVE,
//! SME, MTE, `dit`, `ssbs`, ...) are not in the table: refused by name,
//! their generic form is accepted as `llvm-mc` accepts it.

/// `mrs` reads the register.
pub const READ: u8 = 1;
/// `msr` writes the register.
pub const WRITE: u8 = 2;
/// The name needs `+rand` (`rndr`, `rndrrs`).
pub const RAND: u8 = 4;

/// The names [`TABLE`] holds besides the ID-register space, as
/// `llvm-mc` is asked about them (the generator's input).
pub const EXTRA: [&str; 14] = [
    "cntfrq_el0",
    "cntvct_el0",
    "cntpct_el0",
    "tpidr_el0",
    "tpidrro_el0",
    "fpcr",
    "fpsr",
    "nzcv",
    "daif",
    "dczid_el0",
    "ctr_el0",
    "currentel",
    "rndr",
    "rndrrs",
];

/// A named system register: its lower-case name, its
/// `op0:op1:CRn:CRm:op2` field (16 bits), and its [`READ`], [`WRITE`]
/// and feature bits. Sorted by name.
pub type Entry = (&'static str, u16, u8);

/// Every named system register, sorted by name (generated: see the
/// module doc).
pub const TABLE: &[Entry] = &[
    ("cntfrq_el0", 0xdf00, READ | WRITE),
    ("cntpct_el0", 0xdf01, READ),
    ("cntvct_el0", 0xdf02, READ),
    ("ctr_el0", 0xd801, READ),
    ("currentel", 0xc212, READ),
    ("daif", 0xda11, READ | WRITE),
    ("dczid_el0", 0xd807, READ),
    ("fpcr", 0xda20, READ | WRITE),
    ("fpsr", 0xda21, READ | WRITE),
    ("id_aa64afr0_el1", 0xc02c, READ),
    ("id_aa64afr1_el1", 0xc02d, READ),
    ("id_aa64dfr0_el1", 0xc028, READ),
    ("id_aa64dfr1_el1", 0xc029, READ),
    ("id_aa64dfr2_el1", 0xc02a, READ),
    ("id_aa64fpfr0_el1", 0xc027, READ),
    ("id_aa64isar0_el1", 0xc030, READ),
    ("id_aa64isar1_el1", 0xc031, READ),
    ("id_aa64isar2_el1", 0xc032, READ),
    ("id_aa64isar3_el1", 0xc033, READ),
    ("id_aa64mmfr0_el1", 0xc038, READ),
    ("id_aa64mmfr1_el1", 0xc039, READ),
    ("id_aa64mmfr2_el1", 0xc03a, READ),
    ("id_aa64mmfr3_el1", 0xc03b, READ),
    ("id_aa64mmfr4_el1", 0xc03c, READ),
    ("id_aa64pfr0_el1", 0xc020, READ),
    ("id_aa64pfr1_el1", 0xc021, READ),
    ("id_aa64pfr2_el1", 0xc022, READ),
    ("id_afr0_el1", 0xc00b, READ),
    ("id_dfr0_el1", 0xc00a, READ),
    ("id_dfr1_el1", 0xc01d, READ),
    ("id_isar0_el1", 0xc010, READ),
    ("id_isar1_el1", 0xc011, READ),
    ("id_isar2_el1", 0xc012, READ),
    ("id_isar3_el1", 0xc013, READ),
    ("id_isar4_el1", 0xc014, READ),
    ("id_isar5_el1", 0xc015, READ),
    ("id_mmfr0_el1", 0xc00c, READ),
    ("id_mmfr1_el1", 0xc00d, READ),
    ("id_mmfr2_el1", 0xc00e, READ),
    ("id_mmfr3_el1", 0xc00f, READ),
    ("id_mmfr4_el1", 0xc016, READ),
    ("id_mmfr5_el1", 0xc01e, READ),
    ("id_pfr0_el1", 0xc008, READ),
    ("id_pfr1_el1", 0xc009, READ),
    ("midr_el1", 0xc000, READ),
    ("mpidr_el1", 0xc005, READ),
    ("mvfr0_el1", 0xc018, READ),
    ("mvfr1_el1", 0xc019, READ),
    ("mvfr2_el1", 0xc01a, READ),
    ("nzcv", 0xda10, READ | WRITE),
    ("revidr_el1", 0xc006, READ),
    ("rndr", 0xd920, READ | RAND),
    ("rndrrs", 0xd921, READ | RAND),
    ("tpidr_el0", 0xde82, READ | WRITE),
    ("tpidrro_el0", 0xde83, READ | WRITE),
];

/// The `op0:op1:CRn:CRm:op2` field of `S<op0>_<op1>_C<crn>_C<crm>_<op2>`.
#[must_use]
pub const fn field(op0: u16, op1: u16, crn: u16, crm: u16, op2: u16) -> u16 {
    op0 << 14 | op1 << 11 | crn << 7 | crm << 3 | op2
}

/// One decimal number of at most `max`, without a leading zero (as
/// `llvm-mc` reads the generic form).
fn number(text: &str, max: u16) -> Option<u16> {
    if text.is_empty() || text.len() > 2 || (text.len() > 1 && text.starts_with('0')) {
        return None;
    }
    let n = text
        .bytes()
        .try_fold(0u16, |n, b| b.is_ascii_digit().then(|| n * 10 + u16::from(b - b'0')))?;
    (n <= max).then_some(n)
}

/// The field of the generic form `S<op0>_<op1>_C<n>_C<m>_<op2>` (any
/// case), if `name` is one.
#[must_use]
pub fn generic(name: &str) -> Option<u16> {
    let rest = name.strip_prefix(['s', 'S'])?;
    let mut parts = rest.split('_');
    let op0 = number(parts.next()?, 3)?;
    let op1 = number(parts.next()?, 7)?;
    let crn = number(parts.next()?.strip_prefix(['c', 'C'])?, 15)?;
    let crm = number(parts.next()?.strip_prefix(['c', 'C'])?, 15)?;
    let op2 = number(parts.next()?, 7)?;
    if parts.next().is_some() {
        return None;
    }
    Some(field(op0, op1, crn, crm, op2))
}

/// The table entry of `name` (any case).
#[must_use]
pub fn named(name: &str) -> Option<Entry> {
    if name.len() > 24 {
        return None;
    }
    let mut buf = [0u8; 24];
    let lower = &mut buf[..name.len()];
    lower.copy_from_slice(name.as_bytes());
    lower.make_ascii_lowercase();
    let lower = std::str::from_utf8(lower).ok()?;
    TABLE.binary_search_by(|entry| entry.0.cmp(lower)).ok().map(|at| TABLE[at])
}

/// `MRS <Xt>, <field>`.
#[must_use]
pub const fn mrs(rt: u32, field: u16) -> u32 {
    0xd520_0000 | (field as u32) << 5 | rt
}

/// `MSR <field>, <Xt>`.
#[must_use]
pub const fn msr(field: u16, rt: u32) -> u32 {
    0xd500_0000 | (field as u32) << 5 | rt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_sorted_and_unique() {
        assert!(TABLE.windows(2).all(|w| w[0].0 < w[1].0));
        for name in EXTRA {
            assert!(named(name).is_some(), "{name}");
        }
    }

    #[test]
    fn generic_form() {
        assert_eq!(generic("S3_3_C4_C2_0"), Some(0xda10));
        assert_eq!(generic("s3_7_c15_c15_7"), Some(0xffff));
        assert_eq!(generic("s0_0_c0_c0_0"), Some(0));
        for bad in [
            "s4_0_c0_c0_0",
            "s3_0_c0_c0_8",
            "s3_0_c00_c0_0",
            "s03_0_c0_c0_0",
            "s3_0_c16_c0_0",
            "s3_0_c0_c0_0x",
            "s3_0_c0_c0",
            "s3_0_c0_c0_0_1",
            "s3_0_0_c0_0",
        ] {
            assert_eq!(generic(bad), None, "{bad}");
        }
        assert_eq!(named("MIDR_EL1").map(|e| e.1), Some(0xc000));
        assert_eq!(named("midr_el2"), None);
    }
}
