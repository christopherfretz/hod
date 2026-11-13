//! Target features (`spec/codegen.md` §5.6.2): which `+name`/`-name`
//! items of a `target-features` string or of `-mattr` hod's x86-64 code
//! is correct under.
//!
//! hod emits the base x86-64 instruction set (SSE2, which every x86-64
//! CPU runs) and never chooses an instruction by a feature. So a feature
//! that only makes instructions *available* changes nothing hod emits,
//! enabled or disabled, and is accepted: a module built for
//! `-march=x86-64-v3` or `-march=native` is not declined as a whole
//! (design 0006 §10.1). `target-cpu` is not a feature and is accepted
//! with any value. A feature that changes what the code must keep (no
//! SSE registers, a soft-float ABI, a mitigation hod does not apply) is
//! refused by name. Everything else is unknown and refused: a feature
//! hod has not judged is not assumed harmless.
//!
//! Each judged feature is one line of [`JUDGED`] with its reason; a group
//! package that implements what a refused feature asks for moves it.
//! The parsing is target-neutral and moves to `hod_cg` (WP-117).

/// `+soft-float`: floating point in general registers, another ABI.
pub const SOFT_FLOAT: &str = "attr:target-feature-soft-float";
/// `-sse` or `-sse2`: no SSE registers, which hod's floating point and
/// the SysV ABI use.
pub const NO_SSE: &str = "attr:target-feature-no-sse";
/// `-x87`: no x87 unit; `x86_fp80` would need a soft implementation.
pub const NO_X87: &str = "attr:target-feature-no-x87";
/// `+retpoline*`: indirect branches through thunks, which hod does not
/// emit.
pub const RETPOLINE: &str = "attr:target-feature-retpoline";
/// `+harden-sls-*`: straight-line-speculation hardening hod does not do.
pub const HARDEN_SLS: &str = "attr:target-feature-harden-sls";
/// `+lvi-*`: load-value-injection hardening hod does not do.
pub const LVI: &str = "attr:target-feature-lvi";
/// A feature hod has not judged, or an item that is not `+name` or
/// `-name`.
pub const UNKNOWN: &str = "attr:target-feature-unknown";

/// Every reason [`check`] gives.
pub const REASONS: [&str; 7] = [SOFT_FLOAT, NO_SSE, NO_X87, RETPOLINE, HARDEN_SLS, LVI, UNKNOWN];

/// A refused feature: the reason, and the item as written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refused<'a> {
    /// The stable name (one of [`REASONS`]).
    pub reason: &'static str,
    /// The item that was refused, sign included.
    pub feature: &'a str,
}

/// What a judged feature does to hod's code, enabled (`+`) and disabled
/// (`-`): `None` where it is accepted.
#[derive(Clone, Copy, Debug)]
struct Judged {
    name: &'static str,
    on: Option<&'static str>,
    off: Option<&'static str>,
}

/// Accepted either way: it only makes instructions available.
const fn available(name: &'static str) -> Judged {
    Judged { name, on: None, off: None }
}

/// The judged features (design 0006 §2.7, §10.1), one line each.
const JUDGED: &[Judged] = &[
    // The x86-64 baseline: what clang and kiln write for every function.
    // Turning one off takes away what SysV passes floats in, or `x86_fp80`.
    Judged { name: "sse", on: None, off: Some(NO_SSE) },
    Judged { name: "sse2", on: None, off: Some(NO_SSE) },
    Judged { name: "x87", on: None, off: Some(NO_X87) },
    available("mmx"),
    available("fxsr"),
    available("cmov"),
    available("cx8"),
    // Available only: hod's code uses none of these (design 0006 §10.1).
    available("sse3"),
    available("ssse3"),
    available("sse4.1"),
    available("sse4.2"),
    available("popcnt"),
    available("lzcnt"),
    available("bmi"),
    available("bmi2"),
    available("fma"),
    available("f16c"),
    available("movbe"),
    // `avx`, `avx2`: the instructions only. Whether a wide vector then
    // travels in `ymm` is the argument assignment's (WP-118,
    // `x64:wide-vector-abi`).
    available("avx"),
    available("avx2"),
    // `cmpxchg16b`: X-atomic (WP-133) uses it inline with `+cx16` and
    // libatomic without; either is correct.
    available("cx16"),
    // The other features of `-march=x86-64-v2` and `-v3`.
    available("crc32"),
    available("sahf"),
    available("xsave"),
    // Contract features: off is what hod's code already keeps.
    Judged { name: "soft-float", on: Some(SOFT_FLOAT), off: None },
];

/// Checks one feature string: items separated by `,`, each `+name` or
/// `-name`; empty items are skipped (`-mattr` drops them, and LLVM's
/// parser does).
///
/// # Errors
/// The first item hod's code is not correct under, with its reason.
pub fn check(features: &str) -> Result<(), Refused<'_>> {
    features.split(',').filter(|item| !item.is_empty()).try_for_each(check_one)
}

/// One item of a feature string.
///
/// # Errors
/// As [`check`].
pub fn check_one(item: &str) -> Result<(), Refused<'_>> {
    let refuse = |reason| Err(Refused { reason, feature: item });
    let (on, name) = match item.split_at_checked(1) {
        Some(("+", name)) => (true, name),
        Some(("-", name)) => (false, name),
        _ => return refuse(UNKNOWN),
    };
    if let Some(judged) = JUDGED.iter().find(|judged| judged.name == name) {
        return match if on { judged.on } else { judged.off } {
            Some(reason) => refuse(reason),
            None => Ok(()),
        };
    }
    match mitigation(name) {
        // Off is what hod's code already keeps.
        Some(_) if !on => Ok(()),
        Some(reason) => refuse(reason),
        None => refuse(UNKNOWN),
    }
}

/// The reason of a family of mitigation features, if `name` is one.
fn mitigation(name: &str) -> Option<&'static str> {
    Some(match name {
        _ if name.starts_with("retpoline") => RETPOLINE,
        _ if name.starts_with("harden-sls-") => HARDEN_SLS,
        _ if name.starts_with("lvi-") => LVI,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reason(features: &str) -> Option<&'static str> {
        check(features).err().map(|refused| refused.reason)
    }

    /// §5.6.2: what kiln and clang write is accepted, `-march=x86-64-v3`
    /// included.
    #[test]
    fn what_compilers_write_is_accepted() {
        for features in [
            "",
            "+cmov,+cx8,+fxsr,+mmx,+sse,+sse2,+x87",
            "+cmov,+crc32,+cx16,+cx8,+fxsr,+mmx,+popcnt,+sahf,+sse,+sse2,+sse3,+sse4.1,+sse4.2,\
             +ssse3,+x87",
            "+avx,+avx2,+bmi,+bmi2,+cmov,+crc32,+cx16,+cx8,+f16c,+fma,+fxsr,+lzcnt,+mmx,+movbe,\
             +popcnt,+sahf,+sse,+sse2,+sse3,+sse4.1,+sse4.2,+ssse3,+x87,+xsave",
            "-avx,-avx2,-popcnt,-soft-float,-retpoline-indirect-calls,-harden-sls-ret",
            ",,+sse2,",
        ] {
            assert_eq!(check(features), Ok(()), "{features}");
        }
    }

    /// §5.6.2: what changes the contract, each by its name.
    #[test]
    fn contract_features_are_refused_by_name() {
        for (features, expected) in [
            ("+soft-float", SOFT_FLOAT),
            ("-sse", NO_SSE),
            ("+sse,-sse2", NO_SSE),
            ("-x87", NO_X87),
            ("+retpoline-indirect-calls", RETPOLINE),
            ("+retpoline-external-thunk", RETPOLINE),
            ("+harden-sls-ret", HARDEN_SLS),
            ("+lvi-cfi", LVI),
        ] {
            assert_eq!(reason(features), Some(expected), "{features}");
        }
    }

    /// §5.6.2: unknown is refused, with the item.
    #[test]
    fn unknown_features_are_refused() {
        assert_eq!(
            check("+sse2,+avx512f,+soft-float"),
            Err(Refused { reason: UNKNOWN, feature: "+avx512f" })
        );
        assert_eq!(check("sse2"), Err(Refused { reason: UNKNOWN, feature: "sse2" }));
        assert_eq!(reason("-no-such-feature"), Some(UNKNOWN));
        assert_eq!(reason("+"), Some(UNKNOWN));
        assert_eq!(reason("+SSE2"), Some(UNKNOWN));
        // An AArch64 feature is not an x86-64 one.
        assert_eq!(reason("+neon"), Some(UNKNOWN));
    }

    #[test]
    fn reasons_are_distinct_and_named_like_rows() {
        for (i, name) in REASONS.iter().enumerate() {
            assert!(name.starts_with("attr:target-feature-"), "{name}");
            assert!(!REASONS[..i].contains(name), "{name}");
        }
        for (i, judged) in JUDGED.iter().enumerate() {
            assert!(!JUDGED[..i].iter().any(|j| j.name == judged.name), "{}", judged.name);
        }
    }
}
