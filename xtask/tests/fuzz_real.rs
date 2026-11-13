//! `cargo xtask fuzz` end to end with the real tools: csmith, clang,
//! `llc`, the linker driver (`spec/landed/harness/WP-154.md`).
//!
//! csmith is not among the tools `HOD_REQUIRE_TOOLS=1` requires (hosted
//! CI does not install it): without it, or without clang, `llc` and gcc
//! of LLVM 22, these tests skip with a note whatever that variable says.
#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use hod_testutil::Scratch;
use xtask::fuzz::{Options, case::Kind, run};
use xtask::irdiff::{self, Arch, Search, directives};

/// Whether the tools are here; a note when they are not.
fn tools_found() -> bool {
    let search = Search::from_env();
    let missing: Vec<&str> = [
        ("csmith", search.on_path("csmith").is_some()),
        ("clang-22", search.versioned("clang", 22).is_some()),
        ("llc-22", search.versioned("llc", 22).is_some()),
        ("gcc", search.on_path("gcc").is_some()),
    ]
    .into_iter()
    .filter_map(|(name, found)| (!found).then_some(name))
    .collect();
    if !missing.is_empty() || Arch::host().is_none() {
        eprintln!("note: {missing:?} not found (or no supported host); skipping the fuzz test");
        return false;
    }
    true
}

fn build_dir() -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).parent().expect("it is <target>/tmp").to_path_buf()
}

fn workspace_hod() -> PathBuf {
    static HOD: OnceLock<PathBuf> = OnceLock::new();
    HOD.get_or_init(|| irdiff::build_workspace_hod(&build_dir()).expect("hod builds")).clone()
}

fn options(scratch: &Scratch, seed: u64, count: u64) -> Options {
    let mut options = Options::new(scratch.join("build"));
    options.out = scratch.join("fuzz");
    (options.seed, options.count, options.jobs) = (seed, count, 2);
    options.hod = Some(workspace_hod());
    options
}

// "What a seed does": a few seeds on the host; hod agrees with the oracle or declines
// them, and nothing is wrong on the oracle's side.
#[test]
fn seeds_on_the_host() {
    if !tools_found() {
        return;
    }
    let scratch = Scratch::new("fuzz-host");
    let summary = run(&options(&scratch, 1, 3)).expect("the run");
    let text = summary.text();
    assert_eq!(summary.seeds, 3, "{text}");
    assert!(summary.findings.is_empty() && summary.errors.is_empty(), "{text}");
    assert!(summary.agree + summary.refused.values().sum::<usize>() >= 1, "{text}");
    let results = std::fs::read_to_string(summary.results_file.as_ref().unwrap()).unwrap();
    assert_eq!(results.lines().count(), 4, "{results}");
    // The work directory is gone; no finding was saved.
    let left: Vec<_> = std::fs::read_dir(scratch.join("fuzz")).unwrap().flatten().collect();
    assert_eq!(left.len(), 1, "{left:?}");
}

// "Findings", "Reduction": a hod that miscompiles (every `xor` is built as
// an `or`) gives a finding, saved with its commands and outputs, which
// the witness blames on hod and which is reduced to a seed test.
#[test]
fn a_wrong_hod_is_a_finding() {
    if !tools_found() {
        return;
    }
    let scratch = Scratch::new("fuzz-wrong");
    let wrong = scratch.write_script(
        "wrong-hod",
        &format!(
            "for last; do :; done\n\
             n=$# i=0\n\
             for a do i=$((i+1)); [ $i -eq 1 ] && set --; \
             if [ $i -lt $n ]; then set -- \"$@\" \"$a\"; fi; done\n\
             sed 's/ = xor / = or /' \"$last\" > \"$last.wrong.ll\"\n\
             exec '{}' \"$@\" \"$last.wrong.ll\"",
            workspace_hod().display()
        ),
    );
    let mut options = options(&scratch, 3, 1);
    options.hod = Some(wrong);
    options.reduce_budget = 40;
    let summary = run(&options).expect("the run");
    let text = summary.text();
    assert_eq!(summary.exit_status(), 1, "{text}");
    let [found] = &summary.findings[..] else { panic!("one finding: {text}") };
    assert_eq!(found.kind, Kind::Disagree, "{text}");
    assert!(found.witness.contains("hod is suspect"), "{text}");
    for file in ["prog.c", "prog.ll", "commands.sh", "oracle.stdout", "hod.stdout", "seed.ll"] {
        assert!(found.dir.join(file).is_file(), "{file} in {}", found.dir.display());
    }
    let (before, after, probes) = found.reduced.expect("reduced");
    assert!(after * 2 < before && probes <= 40, "{text}");
    // The seed test's directives parse, expect the failure on this
    // target, and name no path of the run.
    let seed = std::fs::read_to_string(found.dir.join("seed.ll")).unwrap();
    let expect = directives::parse(&seed).expect("the seed's directives");
    let host = Arch::host().unwrap();
    assert!(expect.xfail_reason(host, 22, None).is_some(), "{seed}");
    assert!(!expect.stdout.is_empty(), "{seed}");
    assert!(!seed.contains(&*scratch.join("").to_string_lossy()), "{seed}");
}
