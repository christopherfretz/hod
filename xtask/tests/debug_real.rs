//! `cargo xtask debug` (`spec/harness.md` "debug") end to end with the
//! real tools: clang, `llc`, the linkers, gdb and lldb, and the
//! workspace's hod.
//!
//! Tools-gated like every oracle test (`docs/VALIDATION.md` "Oracles"):
//! without the tools the test skips with a note; with `HOD_REQUIRE_TOOLS=1`
//! a missing tool is a failure.
#![cfg(target_os = "linux")]

mod common;

use std::path::{Path, PathBuf};

use common::Scratch;
use xtask::debug::lines::Class;
use xtask::debug::sessions::{self, Options, Outcome, Report};
use xtask::irdiff;

/// This checkout's build directory: the parent of the directory cargo
/// gives integration tests for their files.
fn build_dir() -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).parent().expect("it is <target>/tmp").to_path_buf()
}

fn options(scratch: &Scratch) -> Options {
    let mut options = Options::new(build_dir());
    options.hod = Some(irdiff::build_workspace_hod(&build_dir()).expect("hod builds"));
    options.results = Some(scratch.join("results.tsv"));
    options.require_tools = hod_testutil::tools_required();
    options
}

/// Whether the run had the tools to run any session; when not, the test
/// skips (and fails under `HOD_REQUIRE_TOOLS=1`).
fn usable(report: &Report, options: &Options) -> bool {
    if report.records.iter().any(|r| r.outcome != Outcome::Skip) {
        return true;
    }
    assert!(!options.require_tools, "required tools are missing:\n{}", report.summary(false));
    eprintln!("note: skipping this tool-backed test:\n{}", report.summary(false));
    false
}

// "Seeds", "Lines": the quick tier of tests/debug (the tests with `tier:
// quick`: stepping, the two-object COMDAT case in both orders, and a
// function's thread-local static, the harness's first finding, fixed by
// WP-094) on every front end, linker and debugger found. Nothing fails,
// nothing is in error, no `xfail` is stale. The threshold of the `(function, line)` comparison: no
// unclassified difference, and almost every pair the same.
#[test]
fn quick_tier_matches_llc() {
    let scratch = Scratch::new("debug-quick");
    let mut options = options(&scratch);
    options.quick = true;
    let report = sessions::run(&options).expect("the run can be made");
    let summary = report.summary(false);
    if !usable(&report, &options) {
        return;
    }
    for bad in [Outcome::Fail, Outcome::Xpass, Outcome::Crash, Outcome::Error] {
        assert_eq!(report.count(bad), 0, "{summary}");
    }
    assert_eq!(report.exit_status(), 0, "{summary}");
    assert!(report.count(Outcome::Pass) > 0, "{summary}");
    // An XLINK (WP-248) is a listed case of a linker exception, nothing else:
    // on hosted x86-64, tls_static.c under wild (`tls-dtpoff-debug`).
    for r in report.records.iter().filter(|r| r.outcome == Outcome::Xlink) {
        let session = xtask::debug::xlink::Session {
            linker: r.linker.as_deref().unwrap_or("-"),
            debugger: r.debugger.as_deref().unwrap_or("-"),
            target: irdiff::directives::Arch::host().map_or("", |a| a.name()),
            test: &r.test,
        };
        assert!(xtask::debug::xlink::expected(session).is_some(), "{r:?}");
    }
    let finding = |r: &&sessions::Record| r.test.ends_with("tls_static.c");
    assert!(report.records.iter().filter(finding).all(|r| r.outcome != Outcome::Fail), "{summary}");
    let lines = report.lines_total();
    assert_eq!(lines.unclassified(), 0, "{summary}");
    let differ = lines.hod_only.len() + lines.llc_only.len();
    // At least 95% of the pairs the same (all of them today).
    assert!(lines.same > 0 && differ * 20 <= lines.same + differ, "{summary}");
}

const PROGRAM: &str = "\
int twice(int x) {
  int y = x * 2;
  return y;
}

int main(void) {
  return twice(3) == 6 ? 0 : 1;
}
";

const SCRIPT: &str = "\
//@ frontend: clang-22
//@ gdb: break twice
//@ gdb: run
//@ gdb: print x
//@ gdb: continue
//@ gdb-check: Breakpoint 1, twice (x=3) at
//@ gdb-check: $1 = 3
//@ gdb-check: exited normally
";

/// A hod that builds a copy of its input whose line 2 is line 3: its
/// program stops one line further than `llc`'s.
fn mutated_hod(scratch: &Scratch, hod: &Path) -> PathBuf {
    let body = format!(
        "n=$#\nfor a; do last=$a; done\ni=0\nfor a; do i=$((i+1)); \
         if [ $i -lt $n ]; then set -- \"$@\" \"$a\"; fi; done\nshift $n\n\
         sed 's/!DILocation(line: 2,/!DILocation(line: 3,/g' \"$last\" > \"$last.mut\"\n\
         exec '{}' \"$@\" \"$last.mut\"",
        hod.display()
    );
    hod_testutil::write_script(&scratch.join("hod"), &body);
    scratch.join("hod")
}

// "Outcomes": what the harness must catch. A hod whose line table is wrong
// FAILs; the same test with the real hod PASSes; a check `llc`'s session
// does not meet is an ERROR of the test; an `xfail` on a passing test is
// an XPASS. One front end, one linker, gdb.
#[test]
fn catches_differences_and_broken_tests() {
    let scratch = Scratch::new("debug-outcomes");
    let plain = scratch.join("plain.c");
    std::fs::write(&plain, format!("{PROGRAM}{SCRIPT}")).unwrap();
    let wrong = scratch.join("wrong_check.c");
    std::fs::write(&wrong, format!("{PROGRAM}{SCRIPT}//@ gdb-check: $2 = 7\n")).unwrap();
    let stale = scratch.join("stale_xfail.c");
    std::fs::write(&stale, format!("{PROGRAM}{SCRIPT}//@ xfail: no longer fails\n")).unwrap();

    let mut options = options(&scratch);
    (options.linkers, options.debuggers) = (vec!["bfd".into()], vec!["gdb".into()]);
    options.lines = false;
    options.paths = vec![plain.clone(), wrong.clone(), stale.clone()];
    let report = sessions::run(&options).expect("the run can be made");
    let summary = report.summary(true);
    if !usable(&report, &options) {
        return;
    }
    let outcome = |report: &Report, name: &str| {
        let hod: Vec<Outcome> = report
            .records
            .iter()
            .filter(|r| r.test.ends_with(name) && r.variant == "hod")
            .map(|r| r.outcome)
            .collect();
        assert_eq!(hod.len(), 1, "{name}: {hod:?}");
        hod[0]
    };
    assert_eq!(outcome(&report, "plain.c"), Outcome::Pass, "{summary}");
    assert_eq!(outcome(&report, "wrong_check.c"), Outcome::Error, "{summary}");
    assert_eq!(outcome(&report, "stale_xfail.c"), Outcome::Xpass, "{summary}");
    assert_eq!(report.exit_status(), 1, "{summary}");

    options.hod = Some(mutated_hod(&scratch, options.hod.as_deref().expect("set")));
    options.paths = vec![plain];
    let report = sessions::run(&options).expect("the run can be made");
    let summary = report.summary(true);
    assert_eq!(outcome(&report, "plain.c"), Outcome::Fail, "{summary}");
    assert!(summary.contains("hod's 'Breakpoint 1 at 0x?: file plain.c, line 3.'"), "{summary}");
    assert_eq!(report.exit_status(), 1, "{summary}");
}

// "Lines", `selector-split` (WP-250): in tests/debug/declare_fragment.c,
// `sort`'s `j--` (line 18, an `add nsw i32 %j, -1` of clang 22's -O1 IR)
// has a row in hod's program, none in the oracle's (SelectionDAG folds
// it into the `subs` of line 17's loop test), and one under FastISel and
// under GlobalISel of the same llc. The pair is set apart with that
// evidence, listed and counted; nothing is unclassified.
#[test]
fn selector_split_sets_apart_with_evidence() {
    let scratch = Scratch::new("debug-selector-split");
    let mut options = options(&scratch);
    (options.linkers, options.debuggers) = (vec!["lld".into()], vec!["gdb".into()]);
    options.frontends = vec!["clang-19".into(), "clang-22".into()];
    options.paths = vec![irdiff::workspace_root().join("tests/debug/declare_fragment.c")];
    let report = sessions::run(&options).expect("the run can be made");
    let summary = report.summary(true);
    if !usable(&report, &options) {
        return;
    }
    assert_eq!(report.exit_status(), 0, "{summary}");
    assert_eq!(report.lines_total().unclassified(), 0, "{summary}");
    for result in &report.lines {
        let classed = result.report.hod_only.iter().filter(|(_, c)| *c == Class::SelectorSplit);
        let pairs: Vec<_> = classed.map(|(p, _)| p.clone()).collect();
        let evidence: Vec<_> = result.selector_splits.iter().map(|s| s.pair.clone()).collect();
        assert_eq!(pairs, evidence, "{summary}");
        assert!(result.report.llc_only.iter().all(|(_, c)| *c != Class::SelectorSplit));
    }
    let clang22 = report.lines.iter().find(|r| r.frontend == "clang-22").expect("{summary}");
    let split = clang22.selector_splits.iter().find(|s| s.pair.0 == "sort" && s.pair.2 == 18);
    let split = split.unwrap_or_else(|| panic!("no selector-split at sort:18\n{summary}"));
    assert!(split.hod > 0 && split.fast_isel > 0 && split.global_isel > 0, "{summary}");
    assert!(
        summary.contains(
            "selector-split: tests/debug/declare_fragment.c [clang-22] sort at \
             declare_fragment.c:18: rows hod "
        ),
        "{summary}"
    );
}
