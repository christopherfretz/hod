//! The target features of real `-mcpu=` and `-march=` settings on AArch64
//! (WP-201, `spec/codegen.md` §5.5.2).
//!
//! The census (`tests/features/a64_census.tsv`) is what clang 19, clang 22
//! and kiln write for every `-mcpu=` CPU they accept and for
//! `-march=armv8-a` to `armv9.6-a` with common `+ext` sets. Every item of
//! every string is accepted or refused by a named reason, never
//! `attr:target-feature-unknown`. The calling convention evidence is in
//! `spec/landed/codegen/WP-201.md`: with `llc -O0 -global-isel=0
//! -fast-isel=0` 19 and 22, only `call-saved-*` and `reserve-*` (both
//! refused) change where a signature's values travel or which registers a
//! call keeps.

use std::collections::{BTreeMap, BTreeSet};

use hod_a64::features::{self, PROFILE, UNKNOWN};

const CENSUS: &str = include_str!("features/a64_census.tsv");

/// (tools, flag, target-cpu, features) of each line.
fn census() -> Vec<(&'static str, &'static str, &'static str, &'static str)> {
    CENSUS
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            assert_eq!(f.len(), 5, "{line}");
            (f[0], f[1], f[2], if f[4] == "-" { "" } else { f[4] })
        })
        .collect()
}

/// §5.5.2: every item of every census string is judged; only the R
/// profile (`-mcpu=cortex-r82`, which kiln does not accept) and the
/// strings that turn FP or Advanced SIMD off (`+nofp`, `+nosimd`) are
/// refused, each by its name.
#[test]
fn census_strings_are_judged() {
    let mut refused: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    let mut names = BTreeSet::new();
    let lines = census();
    for (tools, flag, _, string) in &lines {
        for item in string.split(',').filter(|i| !i.is_empty()) {
            names.insert(item);
            if let Err(r) = features::check_one(item) {
                assert_ne!(r.reason, UNKNOWN, "{item} of {tools} {flag}");
                refused.entry(r.reason).or_default().insert(item.to_owned());
            }
        }
        if let Err(r) = features::check(string) {
            let expected = flag.contains("nofp")
                || flag.contains("nosimd")
                || (r.reason == PROFILE && flag.starts_with("-mcpu=cortex-r82"));
            assert!(expected, "{tools} {flag}: {r:?}");
        }
    }
    let refused: Vec<_> = refused.into_iter().collect();
    assert_eq!(
        format!("{refused:?}"),
        "[(\"attr:target-feature-no-neon\", {\"-neon\"}), (\"attr:target-feature-profile\", \
         {\"+v8r\"}), (\"attr:target-feature-soft-float\", {\"-fp-armv8\"})]"
    );
    for tool in ["clang19", "clang22", "kiln"] {
        assert!(lines.iter().any(|l| l.0.split(',').any(|t| t == tool)), "{tool}");
    }
    for cpu in ["apple-m4", "neoverse-v2", "cortex-a78", "generic"] {
        assert!(lines.iter().any(|l| l.2 == cpu), "{cpu}");
    }
    eprintln!("a64 census: {} strings, {} items", lines.len(), names.len());
}
