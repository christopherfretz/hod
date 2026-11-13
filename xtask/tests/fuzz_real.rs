//! `cargo xtask fuzz` end to end with the real tools: csmith, clang,
//! `llc`, `llvm-extract`, the linker driver (`spec/harness.md` "Random
//! programs").
//!
//! csmith is not among the tools `HOD_REQUIRE_TOOLS=1` requires (hosted
//! CI does not install it): without it, or without clang, `llc` and gcc
//! of LLVM 22, these tests skip with a note whatever that variable says.
//! The tests of the ABI generator (`--profile abi`) need no csmith.
#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use hod_testutil::Scratch;
use xtask::fuzz::case::{Kind, Mode};
use xtask::fuzz::profile::Profile;
use xtask::fuzz::{Options, run};
use xtask::irdiff::{self, Arch, Search, directives};

/// Whether the tools are here; a note when they are not.
fn tools_found() -> bool {
    found(&["csmith", "clang-22", "llc-22", "gcc"])
}

/// Whether `tools` (a name, or `<name>-<major>`) are here.
fn found(tools: &[&str]) -> bool {
    let search = Search::from_env();
    let missing: Vec<&str> = tools
        .iter()
        .map(|&tool| {
            let here = match tool.rsplit_once('-').and_then(|(n, m)| Some((n, m.parse().ok()?))) {
                Some((name, major)) => search.versioned(name, major).is_some(),
                None => search.on_path(tool).is_some(),
            };
            (tool, here)
        })
        .filter_map(|(name, found)| (!found).then_some(name))
        .collect();
    if !missing.is_empty() || Arch::host().is_none() {
        eprintln!("note: {missing:?} not found (or no supported host); skipping the fuzz test");
        return false;
    }
    true
}

/// csmith writes `platform.info` into its current directory: every csmith
/// child runs in its seed's directory under the build directory, so none
/// is left in the crate's directory (a test's) or the workspace's.
fn no_platform_info() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    for dir in [crate_dir, crate_dir.parent().expect("the workspace")] {
        assert!(
            !dir.join("platform.info").exists(),
            "csmith wrote {}/platform.info",
            dir.display()
        );
    }
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
    no_platform_info();
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

// "Mix": a few csmith seeds split in two, each half built by hod with the
// other built by llc; and the same seeds with -g ("Debug"): no finding,
// nothing wrong on the oracle's side, and the line comparison ran.
#[test]
fn mixed_and_debug_seeds_on_the_host() {
    if !tools_found() || !found(&["llvm-extract-22", "llvm-dwarfdump-22"]) {
        return;
    }
    let scratch = Scratch::new("fuzz-modes");
    for mode in [Mode::Mix, Mode::Debug] {
        let mut options = options(&scratch, 1, 3);
        options.mode = mode;
        let summary = run(&options).expect("the run");
        let text = summary.text();
        assert_eq!(summary.seeds, 3, "{text}");
        assert!(summary.findings.is_empty() && summary.errors.is_empty(), "{text}");
        if mode == Mode::Debug && summary.agree > 0 {
            assert!(summary.lines.as_ref().is_some_and(|l| l.same > 0), "{text}");
        }
    }
    no_platform_info();
}

// Harness gap (debug comparison, not hod): `llc -O0` deletes the store of
// csmith's `(*p) = (*p);`, so hod's object alone has a row at the line,
// and `debug::permitted` has no class for it
// (xtask/src/debug/permitted.rs:165 `pair_class`; spec/dwarf.md §5.7,
// "Permitted differences"). `--debug` takes such pairs apart
// (`case::self_assignment`); this passes once the comparison classifies
// them itself and none is left to take apart.
#[test]
#[ignore = "harness gap: no permitted class for a self-assignment's store, xtask/src/debug/permitted.rs:165"]
fn self_assignments_are_a_permitted_class() {
    if !tools_found() || !found(&["llvm-dwarfdump-22"]) {
        return;
    }
    let scratch = Scratch::new("fuzz-self-assign");
    let mut options = options(&scratch, 2, 1);
    options.mode = Mode::Debug;
    let summary = run(&options).expect("the run");
    assert_eq!(summary.self_assigned, 0, "{}", summary.text());
}

// Harness gap (debug comparison, not hod): at csmith's `if ((x, -6L))`,
// a branch on a constant to the next block, hod emits nothing (it folds
// the branch, spec/codegen.md §6.2) and `llc -O0` a test and a jump, so
// llc's object alone has a row at the line; `debug::permitted` has no
// class for it (xtask/src/debug/permitted.rs:165 `pair_class`). `--debug`
// takes such pairs apart (`case::constant_branch`); this passes once the
// comparison classifies them itself. Seed 231 on x86-64 has one.
#[test]
#[ignore = "harness gap: no permitted class for a folded constant branch, xtask/src/debug/permitted.rs:165"]
fn constant_branches_are_a_permitted_class() {
    if !found(&["csmith", "clang-22", "llc-22", "x86_64-linux-gnu-gcc", "llvm-dwarfdump-22"]) {
        return;
    }
    let scratch = Scratch::new("fuzz-constant-branch");
    let mut options = options(&scratch, 231, 1);
    options.mode = Mode::Debug;
    options.target = Some(Arch::X86_64);
    let summary = run(&options).expect("the run");
    assert_eq!(summary.constant_branches, 0, "{}", summary.text());
}

/// A hod that swaps the first two parameters of every `callee_*` of the
/// ABI generator's programs in its definitions, declarations and calls
/// alike (`fuzz/swap-params.awk`): a module agrees with itself, an object
/// of llc's does not.
fn swapping_hod(scratch: &Scratch) -> PathBuf {
    let awk = scratch.write("swap-params.awk", include_str!("fuzz/swap-params.awk"));
    scratch.write_script(
        "swapping-hod",
        &format!(
            "for last; do :; done\n\
             n=$# i=0\n\
             for a do i=$((i+1)); [ $i -eq 1 ] && set --; \
             if [ $i -lt $n ]; then set -- \"$@\" \"$a\"; fi; done\n\
             awk -f '{}' \"$last\" > \"$last.wrong.ll\" || exit 1\n\
             exec '{}' \"$@\" \"$last.wrong.ll\"",
            awk.display(),
            workspace_hod().display()
        ),
    )
}

// "Mix", "Reduction": the planted ABI bug is invisible to the whole-module
// comparison and a finding of --mix, blamed on hod by both witnesses,
// reduced with the split kept, and saved as a seed of two modules.
#[test]
fn a_wrong_calling_convention_is_a_mix_finding() {
    if !found(&["clang-22", "llc-22", "llvm-extract-22", "gcc", "awk"]) || Arch::host().is_none() {
        return;
    }
    let scratch = Scratch::new("fuzz-mix-wrong");
    let mut options = options(&scratch, 2, 1);
    options.profile = Profile::by_name("abi").unwrap();
    options.hod = Some(swapping_hod(&scratch));
    options.reduce_budget = 40;
    let whole = run(&options).expect("the whole run");
    assert!(whole.findings.is_empty() && whole.agree == 1, "{}", whole.text());
    options.mode = Mode::Mix;
    let summary = run(&options).expect("the mixed run");
    let text = summary.text();
    assert_eq!(summary.exit_status(), 1, "{text}");
    let [found] = &summary.findings[..] else { panic!("one finding: {text}") };
    assert_eq!(found.kind, Kind::Disagree, "{text}");
    assert!(
        found.detail.contains("hod A + llc B") || found.detail.contains("llc A + hod B"),
        "{text}"
    );
    assert!(found.witness.contains("hod is suspect"), "{text}");
    assert!(found.witness.contains("disagree between objects"), "{text}");
    let (before, after, probes) = found.reduced.expect("reduced");
    assert!(after * 4 < before && probes <= 40, "{text}");
    let seed = std::fs::read_to_string(found.dir.join("seed.ll")).unwrap();
    let expect = directives::parse(&seed).expect("the seed's directives");
    assert!(expect.xfail_reason(Arch::host().unwrap(), 22, None).is_some(), "{seed}");
    assert_eq!(seed.matches("\n;@ module: ").count(), 2, "{seed}");
    assert!(!seed.contains(&*scratch.join("").to_string_lossy()), "{seed}");
}

// "Profiles", "Census": the ABI generator's programs reach what csmith's
// do not, and the census counts it.
#[test]
fn the_census_of_the_abi_profile() {
    if !found(&["clang-22"]) || Arch::host().is_none() {
        return;
    }
    let scratch = Scratch::new("fuzz-census");
    let mut options = options(&scratch, 1, 8);
    options.profile = Profile::by_name("abi").unwrap();
    options.mode = Mode::Census;
    let summary = run(&options).expect("the census");
    let census = summary.census.as_ref().expect("a census");
    assert_eq!(census.programs, 8, "{}", summary.text());
    for feature in ["define:varargs", "type:i128", "intrinsic:llvm.va_start", "attr:sret"] {
        assert!(census.features.contains_key(feature), "{feature}: {}", summary.text());
    }
}
