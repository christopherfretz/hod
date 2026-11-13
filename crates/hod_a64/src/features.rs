//! Target features (`spec/codegen.md` §5.5.2): which `+name`/`-name`
//! items of a `target-features` string or of `-mattr` hod's code is
//! correct under.
//!
//! hod emits ARMv8.0 code with FP and Advanced SIMD and never chooses an
//! instruction by a feature. So a feature that only makes instructions
//! *available* changes nothing hod emits, enabled or disabled, and is
//! accepted. A feature that changes the contract the code must keep (no
//! FP registers, no unaligned accesses, a register taken from the
//! allocator or the calling convention, no data in code, a mitigation
//! hod does not apply) is refused by name. Everything else is unknown and
//! refused: a feature hod has not judged is not assumed harmless. The
//! reading of the string is `hod_cg::features` (WP-117); the table is
//! this module's.
//!
//! Branch protection is not a feature: `+bti` and `+pauth` only make the
//! instructions available. Whether a function must start with `bti` or
//! sign its return address is the `branch-target-enforcement` and
//! `sign-return-address` attributes and module flags, which `classify`
//! refuses (`docs/status/support.md`).

/// `-fp-armv8`: no floating-point registers, the soft-float ABI.
pub const SOFT_FLOAT: &str = "attr:target-feature-soft-float";
/// `-neon`: no Advanced SIMD, which hod's floating-point code uses.
pub const NO_NEON: &str = "attr:target-feature-no-neon";
/// `+strict-align`: every access must be aligned.
pub const STRICT_ALIGN: &str = "attr:target-feature-strict-align";
/// `+reserve-<reg>`: a register taken from the allocator.
pub const RESERVE: &str = "attr:target-feature-reserve";
/// `+call-saved-<reg>`: a calling convention with more callee-saved
/// registers.
pub const CALL_SAVED: &str = "attr:target-feature-call-saved";
/// `+tagged-globals`: global addresses carry a memory tag.
pub const TAGGED_GLOBALS: &str = "attr:target-feature-tagged-globals";
/// `+execute-only`: no data in code sections; hod writes data islands.
pub const EXECUTE_ONLY: &str = "attr:target-feature-execute-only";
/// `+harden-sls-*`: straight-line-speculation hardening hod does not do.
pub const HARDEN_SLS: &str = "attr:target-feature-harden-sls";
/// `+fix-cortex-*`: an erratum workaround hod does not apply.
pub const ERRATUM: &str = "attr:target-feature-erratum";
/// `+v8r`: the R profile, a different memory system.
pub const PROFILE: &str = "attr:target-feature-profile";
use hod_cg::features::FeatureTable;
pub use hod_cg::features::{Refused, UNKNOWN};

/// Every reason [`check`] gives.
pub const REASONS: [&str; 11] = [
    SOFT_FLOAT,
    NO_NEON,
    STRICT_ALIGN,
    RESERVE,
    CALL_SAVED,
    TAGGED_GLOBALS,
    EXECUTE_ONLY,
    HARDEN_SLS,
    ERRATUM,
    PROFILE,
    UNKNOWN,
];

/// Features that only make instructions or system registers available
/// (LLVM's `AArch64Features.td`, LLVM 19 and 22, and the architecture
/// versions), plus `fmv`, which changes no code hod emits (no function
/// multi-versioning), and `outline-atomics`, which picks the atomic
/// sequence (`atomic.rs`, WP-074; either one is correct).
const AVAILABILITY: &[&str] = &[
    "v8a",
    "v8.1a",
    "v8.2a",
    "v8.3a",
    "v8.4a",
    "v8.5a",
    "v8.6a",
    "v8.7a",
    "v8.8a",
    "v8.9a",
    "v9a",
    "v9.1a",
    "v9.2a",
    "v9.3a",
    "v9.4a",
    "v9.5a",
    "v9.6a",
    "fp-armv8",
    "neon",
    "fmv",
    "outline-atomics",
    "aes",
    "sha2",
    "sha3",
    "sm4",
    "crypto",
    "crc",
    "lse",
    "lse2",
    "lse128",
    "rdm",
    "ras",
    "rasv2",
    "rcpc",
    "rcpc-immo",
    "rcpc3",
    "fullfp16",
    "fp16fml",
    "dotprod",
    "jsconv",
    "complxnum",
    "ccidx",
    "pauth",
    "pauth-lr",
    "bti",
    "dit",
    "flagm",
    "altnzcv",
    "fptoint",
    "i8mm",
    "bf16",
    "f32mm",
    "f64mm",
    "sb",
    "ssbs",
    "predres",
    "specrestrict",
    "mte",
    "rand",
    "tme",
    "ls64",
    "hbc",
    "mops",
    "cssc",
    "wfxt",
    "xs",
    "ecv",
    "fgt",
    "amvs",
    "pmuv3",
    "perfmon",
    "spe",
    "spe-eef",
    "statistical-profiling",
    "sve",
    "sve2",
    "sve-aes",
    "sve2-aes",
    "sve2-sm4",
    "sve2-sha3",
    "sve-bitperm",
    "sve2-bitperm",
    "sve2p1",
    "sve2p2",
    "sve-b16b16",
    "sme",
    "sme2",
    "sme2p1",
    "sme2p2",
    "sme-f64f64",
    "sme-i16i64",
    "sme-f16f16",
    "sme-b16b16",
    "sme-fa64",
    "el2vmsa",
    "el3",
    "vh",
    "lor",
    "pan",
    "pan-rwv",
    "uaops",
    "ccpp",
    "ccdp",
    "sel2",
    "tracev8.4",
    "trbe",
    "nv",
    "am",
    "mpam",
    "rme",
    "brbe",
    "ete",
    "chk",
    "gcs",
    "the",
    "d128",
    "ite",
    "cpa",
    "faminmax",
    "fp8",
    "fp8fma",
    "fp8dot2",
    "fp8dot4",
    "lut",
    "fpac",
    "mec",
    "clrbhrb",
    "fprcvt",
    "cmpbr",
    "lsfe",
    "lsui",
    "occmo",
    "pcdphint",
    "pops",
    "ssve-aes",
    "ssve-bitperm",
    "ssve-fp8fma",
    "ssve-fp8dot2",
    "ssve-fp8dot4",
    "sme-lutv2",
    "sme-f8f16",
    "sme-f8f32",
    "sve-sha3",
    "sve-sm4",
    "sve-bfscale",
    "sve-f16f32mm",
    "f8f16mm",
    "f8f32mm",
    "tlb-rmi",
    "tlbiw",
    "flagm2",
    "frint3264",
    "sme-mop4",
    "sme-tmop",
];

/// hod's AArch64 table: `fp-armv8` and `neon` required, the
/// [`AVAILABILITY`] features, the [`contract`] ones.
const TABLE: FeatureTable = FeatureTable {
    required: &[("fp-armv8", SOFT_FLOAT), ("neon", NO_NEON)],
    available: AVAILABILITY,
    contract,
};

/// Checks one feature string against hod's AArch64 table
/// ([`FeatureTable::check`]).
///
/// # Errors
/// The first item hod's code is not correct under, with its reason (one
/// of [`REASONS`]).
pub fn check(features: &str) -> Result<(), Refused<'_>> {
    TABLE.check(features)
}

/// One item of a feature string.
///
/// # Errors
/// As [`check`].
pub fn check_one(item: &str) -> Result<(), Refused<'_>> {
    TABLE.check_one(item)
}

/// The reason of a feature that changes the contract of the code, if
/// `name` is one.
fn contract(name: &str) -> Option<&'static str> {
    Some(match name {
        "strict-align" => STRICT_ALIGN,
        "tagged-globals" => TAGGED_GLOBALS,
        "execute-only" => EXECUTE_ONLY,
        "v8r" => PROFILE,
        _ if name.starts_with("reserve-") => RESERVE,
        _ if name.starts_with("call-saved-") => CALL_SAVED,
        _ if name.starts_with("harden-sls-") => HARDEN_SLS,
        _ if name.starts_with("fix-cortex-") => ERRATUM,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reason(features: &str) -> Option<&'static str> {
        check(features).err().map(|refused| refused.reason)
    }

    // spec/codegen.md §5.5.2: what kiln and clang write is accepted.
    #[test]
    fn what_compilers_write_is_accepted() {
        for features in [
            "",
            "+fp-armv8,+neon,+outline-atomics,+v8a,-fmv",
            "+aes,+crc,+fp-armv8,+fullfp16,+lse,+neon,+outline-atomics,+ras,+rdm,+v8.1a,+v8.2a,+v8a,-fmv",
            "+bf16,+bti,+ccidx,+complxnum,+crc,+dit,+dotprod,+flagm,+fp-armv8,+i8mm,+jsconv,+lse,\
             +neon,+outline-atomics,+pauth,+predres,+ras,+rcpc,+rdm,+sb,+ssbs,+v8.1a,+v8.2a,\
             +v8.3a,+v8.4a,+v8.5a,+v8.6a,+v8a,-fmv",
            "-outline-atomics,-lse,+sve,+sme",
            "-strict-align,-reserve-x18,-execute-only",
            ",,+neon,",
        ] {
            assert_eq!(check(features), Ok(()), "{features}");
        }
    }

    // spec/codegen.md §5.5.2: the contract features, each by its name.
    #[test]
    fn contract_features_are_refused_by_name() {
        for (features, expected) in [
            ("+neon,-fp-armv8", SOFT_FLOAT),
            ("-neon", NO_NEON),
            ("+strict-align", STRICT_ALIGN),
            ("+reserve-x18", RESERVE),
            ("+reserve-x1", RESERVE),
            ("+reserve-lr-for-ra", RESERVE),
            ("+call-saved-x10", CALL_SAVED),
            ("+tagged-globals", TAGGED_GLOBALS),
            ("+execute-only", EXECUTE_ONLY),
            ("+harden-sls-retbr", HARDEN_SLS),
            ("+fix-cortex-a53-835769", ERRATUM),
            ("+v8r", PROFILE),
        ] {
            assert_eq!(reason(features), Some(expected), "{features}");
        }
    }

    // spec/codegen.md §5.5.2: unknown is refused, with the item.
    #[test]
    fn unknown_features_are_refused() {
        assert_eq!(
            check("+neon,+no-such-feature,+strict-align"),
            Err(Refused { reason: UNKNOWN, feature: "+no-such-feature" })
        );
        assert_eq!(check("neon"), Err(Refused { reason: UNKNOWN, feature: "neon" }));
        assert_eq!(reason("-no-such-feature"), Some(UNKNOWN));
        assert_eq!(reason("+"), Some(UNKNOWN));
        assert_eq!(reason("+NEON"), Some(UNKNOWN));
        // The first refusal of the string is the answer.
        assert_eq!(reason("+strict-align,+bogus"), Some(STRICT_ALIGN));
    }

    #[test]
    fn reasons_are_distinct_and_named_like_rows() {
        for (i, name) in REASONS.iter().enumerate() {
            assert!(name.starts_with("attr:target-feature-"), "{name}");
            assert!(!REASONS[..i].contains(name), "{name}");
        }
    }
}
