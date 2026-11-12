//! `cargo xtask irdiff` end to end with the real tools: LLVM's `llc`, the
//! linker driver and the linker matrix (`spec/harness.md` "irdiff").
//!
//! Tools-gated like every oracle test (`docs/VALIDATION.md` "Oracles"):
//! without the tools the tests skip with a note; with `HOD_REQUIRE_TOOLS=1`
//! a missing tool is a failure.
#![cfg(target_os = "linux")]

mod common;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use common::Scratch;
use hod_testutil::write_script;
use xtask::irdiff::tools::{self, LLVM_MAJORS, REQUIRED_LINKERS};
use xtask::irdiff::{self, Arch, BuiltBy, Config, Mode, Outcome, Report, Search, TierFilter};

/// The LLVM majors of the oracle that are installed, if the machine can
/// run the harness at all: an oracle, the linker driver and a linker.
fn installed() -> Option<Vec<u32>> {
    let search = Search::from_env();
    let majors: Vec<u32> = LLVM_MAJORS
        .into_iter()
        .filter(|major| matches!(tools::find_llvm(Some(&major.to_string()), &search), Ok(Some(_))))
        .collect();
    let driver = tools::find_driver(&search).is_some();
    let linkers: Vec<&str> = REQUIRED_LINKERS
        .into_iter()
        .filter(|name| tools::find_linker(name, &search).is_some())
        .collect();
    let found = format!("LLVM {majors:?}, gcc: {driver}, linkers {linkers:?}");
    if hod_testutil::tools_required() {
        assert!(
            majors == LLVM_MAJORS && driver && linkers == REQUIRED_LINKERS,
            "required tools are missing (HOD_REQUIRE_TOOLS=1); found {found}"
        );
    }
    if majors.is_empty() || !driver || linkers.is_empty() {
        eprintln!("note: irdiff's tools not found ({found}); skipping this tool-backed test");
        return None;
    }
    Some(majors)
}

/// This checkout's build directory: the parent of the directory cargo
/// gives integration tests for their files.
fn build_dir() -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).parent().expect("it is <target>/tmp").to_path_buf()
}

/// The workspace's hod, built once.
fn workspace_hod() -> PathBuf {
    static HOD: OnceLock<PathBuf> = OnceLock::new();
    HOD.get_or_init(|| irdiff::build_workspace_hod(&build_dir()).expect("hod builds")).clone()
}

/// The command's defaults with its output in `scratch`.
fn config(scratch: &Scratch) -> Config {
    let mut config = Config::new(scratch.join("build"));
    config.require_tools = hod_testutil::tools_required();
    config
}

fn other_arch() -> Arch {
    if Arch::host() == Some(Arch::Aarch64) { Arch::X86_64 } else { Arch::Aarch64 }
}

/// The number of tests in the seed corpus.
fn corpus_size() -> usize {
    irdiff::collect(&[]).unwrap().len()
}

// spec/harness.md "Seed corpus": every test of `tests/ir/` is green with
// LLVM 22 and with LLVM 19, with every linker: the oracle's program
// behaves as the directives say, and hod either declines as the test
// expects or builds a program that behaves the same.
#[test]
fn the_seed_corpus_is_green() {
    let Some(majors) = installed() else { return };
    let scratch = Scratch::new("irdiff-corpus");
    for major in majors {
        let mut config = config(&scratch);
        config.hod = Some(workspace_hod());
        config.llvm = Some(major.to_string());
        let report = irdiff::run(&config).unwrap();
        let all = report.summary(true);
        assert_eq!(report.exit_status(), 0, "LLVM {major}:\n{all}");
        assert!(report.records.iter().all(|record| record.llvm == Some(major)), "{all}");
        // Every test is valid on both targets and both LLVMs, so on a
        // machine with the tools nothing is skipped, and every test has a
        // record per linker.
        let linkers = all.split("; linkers ").nth(1).unwrap().split(';').next().unwrap();
        let linkers = linkers.split(' ').count();
        assert!(corpus_size() >= 20 && report.tests() == corpus_size(), "{all}");
        assert_eq!(report.records.len(), corpus_size() * linkers, "{all}");
        for record in &report.records {
            assert!(
                matches!(record.outcome, Outcome::Pass | Outcome::Unsupported | Outcome::Xfail),
                "{all}"
            );
            assert!(record.covers.as_deref().is_some_and(|c| c.starts_with("spec/ir.md §")));
        }
        // The quick tier is a strict subset.
        config.tier = TierFilter::Quick;
        let quick = irdiff::run(&config).unwrap();
        assert_eq!(quick.exit_status(), 0, "{}", quick.summary(true));
        assert!(quick.tests() >= 5 && quick.tests() < report.tests(), "{}", quick.summary(true));
    }
}

// "Hybrid mode", with the real hod and the real `llc`: whatever hod does
// not build itself the oracle builds through hod's fallback, and every
// program then behaves as the oracle's.
#[test]
fn the_seed_corpus_passes_in_hybrid_mode() {
    let Some(majors) = installed() else { return };
    let scratch = Scratch::new("irdiff-hybrid");
    let mut config = config(&scratch);
    config.hod = Some(workspace_hod());
    config.llvm = Some(majors[0].to_string());
    config.mode = Mode::Hybrid;
    let report = irdiff::run(&config).unwrap();
    let all = report.summary(true);
    assert_eq!(report.exit_status(), 0, "{all}");
    assert_eq!(report.count(Outcome::Pass), report.records.len(), "{all}");
    assert!(
        report.records.iter().all(|r| matches!(r.built_by, BuiltBy::Hod | BuiltBy::Fallback)),
        "{all}"
    );
    assert!(all.contains("irdiff: coverage: hod built "), "{all}");
}

// "Cross targets": for the architecture that is not this machine's, both
// compilers build every test's object and nothing is linked or run.
#[test]
fn the_seed_corpus_builds_for_the_other_target() {
    let Some(majors) = installed() else { return };
    let scratch = Scratch::new("irdiff-cross");
    for major in majors {
        let mut config = config(&scratch);
        config.hod = Some(workspace_hod());
        config.llvm = Some(major.to_string());
        config.target = Some(other_arch());
        let report = irdiff::run(&config).unwrap();
        let all = report.summary(true);
        assert_eq!(report.exit_status(), 0, "LLVM {major}:\n{all}");
        assert_eq!(report.records.len(), corpus_size(), "{all}");
        assert!(report.records.iter().all(|record| record.linker.is_none()), "{all}");
        assert!(report.records.iter().all(|r| r.target == other_arch().triple()), "{all}");
        assert!(report.count(Outcome::Error) + report.count(Outcome::Fail) == 0, "{all}");
    }
}

/// A stand-in for hod that compiles with the real `llc` after rewriting
/// the module with a `sed` expression: a hod that builds everything, and
/// (with an expression that changes the program) one that miscompiles.
fn rewriting_hod(scratch: &Scratch, name: &str, llc: &Path, expression: &str) -> PathBuf {
    let path = scratch.join(name);
    let body = format!(
        r#"PATH=/usr/bin:/bin
log= out= in= args=
while [ $# -gt 0 ]; do
  case "$1" in
    --coverage-log=*) log=${{1#--coverage-log=}} ;;
    --strict) ;;
    -o) out=$2; shift ;;
    -*) args="$args $1" ;;
    *) in=$1 ;;
  esac
  shift
done
sed -e '{expression}' "$in" > "$out.ll" || exit 1
'{llc}' $args -o "$out" "$out.ll" || exit 1
printf 'hod\t-\ttext\t1\t%s\t-\t5\t-\n' "$out" >> "$log""#,
        llc = llc.display()
    );
    write_script(&path, &body);
    path
}

const SUM: &str = r#";@ stdout: 40 + 2 = 42
;@ exit: 42
@.fmt = private unnamed_addr constant [14 x i8] c"%d + %d = %d\0A\00"

declare i32 @printf(ptr, ...)

define i32 @sum(i32 %a, i32 %b) {
  %r = add i32 %a, %b
  ret i32 %r
}

define i32 @main() {
entry:
  %r = call i32 @sum(i32 40, i32 2)
  call i32 (ptr, ...) @printf(ptr @.fmt, i32 40, i32 2, i32 %r)
  br label %done
spin:
  br label %spin
done:
  ret i32 %r
}
"#;

/// Runs the one test `SUM` with a hod that rewrites it.
fn run_rewritten(scratch: &Scratch, major: u32, name: &str, expression: &str) -> Report {
    let search = Search::from_env();
    let llvm = tools::find_llvm(Some(&major.to_string()), &search).unwrap().unwrap();
    let test = scratch.join("sum.ll");
    std::fs::write(&test, SUM).unwrap();
    let mut config = config(scratch);
    config.paths = vec![test];
    config.llvm = Some(major.to_string());
    config.hod = Some(rewriting_hod(scratch, name, &llvm.llc, expression));
    config.run_limits.time = Duration::from_secs(3);
    irdiff::run(&config).unwrap()
}

// "Outcomes", end to end: a real object that is right passes with every
// linker, and a real miscompile is a FAIL with every linker, whether it
// shows in the output, in the exit status, at link time or as a hang.
#[test]
fn a_real_miscompile_is_detected() {
    let Some(majors) = installed() else { return };
    let scratch = Scratch::new("irdiff-miscompile");
    let major = majors[0];

    let right = run_rewritten(&scratch, major, "hod-right", "s/^$//");
    let all = right.summary(true);
    assert_eq!(right.exit_status(), 0, "{all}");
    assert!(!right.records.is_empty(), "{all}");
    assert!(
        right.records.iter().all(|r| r.outcome == Outcome::Pass && r.linker.is_some()),
        "{all}"
    );
    assert!(right.records.iter().all(|r| r.built_by == BuiltBy::Hod), "{all}");

    for (name, expression, detail) in [
        (
            "hod-wrong-result",
            "s/add i32 %a, %b/sub i32 %a, %b/",
            "the program of hod's object ended with exit status 38, llc's with exit status 42",
        ),
        (
            "hod-wrong-output",
            "s/i32 2, i32 %r)/i32 2, i32 7)/",
            "the program of hod's object wrote stdout that differs from llc's at line 1: \
             expected \"40 + 2 = 42\\n\", got \"40 + 2 = 7\\n\"",
        ),
        (
            "hod-hangs",
            "s/br label %done/br label %spin/",
            "the program of hod's object timed out after 3 s",
        ),
        ("hod-no-main", "s/@main()/@not_main()/", "hod's object: link with "),
    ] {
        let wrong = run_rewritten(&scratch, major, name, expression);
        let all = wrong.summary(true);
        assert_eq!(wrong.exit_status(), 1, "{name}:\n{all}");
        assert_eq!(wrong.records.len(), right.records.len(), "{name}:\n{all}");
        for record in &wrong.records {
            assert_eq!(record.outcome, Outcome::Fail, "{name}:\n{all}");
            assert!(record.detail.starts_with(detail), "{name}:\n{all}");
        }
    }
}

const ALLOCATES: &str = r#";@ stdout: refused
@.refused = private unnamed_addr constant [8 x i8] c"refused\00"
@.granted = private unnamed_addr constant [8 x i8] c"granted\00"

declare ptr @malloc(i64)
declare i32 @puts(ptr)

define i32 @main() {
  %p = call ptr @malloc(i64 2147483648)
  %none = icmp eq ptr %p, null
  %text = select i1 %none, ptr @.refused, ptr @.granted
  call i32 @puts(ptr %text)
  ret i32 0
}
"#;

// "Child processes", on a real program: the default memory limit of a
// test program (1 GiB of address space) refuses a 2 GiB allocation, and
// the same program is granted it when the limit is raised. A program that
// never ends is an ERROR of the test when it is the oracle's.
#[test]
fn limits_hold_for_real_programs() {
    let Some(majors) = installed() else { return };
    let scratch = Scratch::new("irdiff-real-limits");
    let search = Search::from_env();
    let llvm = tools::find_llvm(Some(&majors[0].to_string()), &search).unwrap().unwrap();
    let test = scratch.join("allocates.ll");
    std::fs::write(&test, ALLOCATES).unwrap();
    let mut config = config(&scratch);
    config.paths = vec![test];
    config.llvm = Some(llvm.major.to_string());
    config.hod = Some(rewriting_hod(&scratch, "hod", &llvm.llc, "s/^$//"));
    let limited = irdiff::run(&config).unwrap();
    assert_eq!(limited.exit_status(), 0, "{}", limited.summary(true));
    assert_eq!(limited.count(Outcome::Pass), limited.records.len());

    config.run_limits.memory_kib = 16 << 20;
    let roomy = irdiff::run(&config).unwrap();
    let all = roomy.summary(true);
    assert_eq!(roomy.exit_status(), 3, "{all}");
    assert!(all.contains("expected \"refused\\n\", got \"granted\\n\""), "{all}");

    let spins = scratch.join("spins.ll");
    std::fs::write(&spins, "define i32 @main() {\n  br label %l\nl:\n  br label %l\n}\n").unwrap();
    config.paths = vec![spins];
    config.run_limits.time = Duration::from_secs(2);
    let report = irdiff::run(&config).unwrap();
    let all = report.summary(true);
    assert_eq!(report.count(Outcome::Error), report.records.len(), "{all}");
    assert!(all.contains("oracle: the program of llc's object timed out after 2 s"), "{all}");
    assert_eq!(report.exit_status(), 3);
}
