//! The line-table findings of the M3 measurement (WP-099,
//! `docs/status/debug.md` "Line tables"): each source of
//! `xtask/tests/data/debug_lines/` is the smallest program that shows one
//! class of `(function, line)` pairs only one of hod's and `llc`'s programs
//! has, outside `spec/dwarf.md` §5.7's permitted classes. A debugger
//! breaking at that line stops elsewhere (or not at all) in one of the two
//! programs, so the sessions differ too.
//!
//! `tests/debug/` cannot hold them: an unclassified pair fails `cargo xtask
//! debug` whatever its `xfail` says (`spec/harness.md` "debug", "Lines").
//! So each is a test here that states what is true since WP-097: the
//! harness run on the source has no unclassified pair and no failing
//! session (hod's line table has what `llc`'s has: `spec/dwarf.md` §5.8),
//! or its pairs are of a class `spec/dwarf.md` §5.7 permits, recognised
//! from the IR (the sessions then differ where the class says, and are
//! not judged).
#![cfg(target_os = "linux")]

mod common;

use std::path::{Path, PathBuf};

use common::Scratch;
use xtask::debug::lines::Class;
use xtask::debug::sessions::{self, Options, Outcome, Report};
use xtask::irdiff;

fn build_dir() -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).parent().expect("it is <target>/tmp").to_path_buf()
}

/// Runs the harness on one source of `data/debug_lines`; `None` when the
/// tools are missing (and not required).
fn run(name: &str) -> Option<(Report, String)> {
    let scratch = Scratch::new("debug-m3");
    let mut options = Options::new(build_dir());
    options.hod = Some(irdiff::build_workspace_hod(&build_dir()).expect("hod builds"));
    options.results = Some(scratch.join("results.tsv"));
    options.require_tools = hod_testutil::tools_required();
    options.paths =
        vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/debug_lines").join(name)];
    let report = sessions::run(&options).expect("the run can be made");
    let summary = report.summary(true);
    if report.records.iter().all(|r| r.outcome == Outcome::Skip) {
        assert!(!options.require_tools, "required tools are missing:\n{summary}");
        eprintln!("note: skipping this tool-backed test:\n{summary}");
        return None;
    }
    Some((report, summary))
}

/// The two programs' `(function, line)` sets and sessions agree.
fn agrees_with_llc(name: &str) {
    let Some((report, summary)) = run(name) else { return };
    let lines = report.lines_total();
    assert_eq!(lines.unclassified(), 0, "{summary}");
    assert!(
        lines
            .hod_only
            .iter()
            .chain(&lines.llc_only)
            .all(|(_, c)| matches!(c, Class::P1 | Class::P3)),
        "{summary}"
    );
    assert_eq!(report.count(Outcome::Fail), 0, "{summary}");
    assert!(report.count(Outcome::Pass) > 0, "{summary}");
}

/// The pairs one side lacks are all of `class` (and there are some).
fn classified_as(name: &str, class: Class) {
    let Some((report, summary)) = run(name) else { return };
    let lines = report.lines_total();
    assert_eq!(lines.unclassified(), 0, "{summary}");
    let differing: Vec<Class> = lines
        .hod_only
        .iter()
        .chain(&lines.llc_only)
        .map(|(_, c)| *c)
        .filter(|c| !matches!(c, Class::P1 | Class::P3))
        .collect();
    assert!(!differing.is_empty() && differing.iter().all(|c| *c == class), "{summary}");
}

// Class `unreachable`: hod's trap for `unreachable` has a row (223 pairs,
// 37 modules). crates/hod_cg (the `unreachable` terminator's code keeps
// the instruction's location).
#[test]
fn unreachable_has_no_row() {
    agrees_with_llc("unreachable_line.c");
}

// Class `gep-placement`, hod only: a zero-offset `getelementptr` on a line
// of its own has code in hod's program (213 pairs, 23 modules).
#[test]
fn folded_gep_has_no_row() {
    agrees_with_llc("gep_member.cpp");
}

// Class `gep-placement`, llc only: hod folds a `getelementptr` llc
// computes at its own line (33 pairs, 7 modules).
#[test]
fn gep_with_offset_has_a_row() {
    agrees_with_llc("gep_offset.c");
}

// Class `is-constant`: `llc` deletes the blocks under a folded
// `llvm.is.constant`, hod keeps them (40 pairs, 13 modules).
#[test]
fn is_constant_blocks_are_permitted() {
    classified_as("is_constant.c", Class::IsConstant);
}

// Class `compare-at-user`: hod's compare is at its user's location
// (10 pairs, 10 modules).
#[test]
fn compare_has_its_own_row() {
    agrees_with_llc("compare_user.cpp");
}

// Class `load-placement`, llc only: a dead load `llc` merges with a later
// load keeps its line in `llc`'s program (40 pairs, 13 modules).
#[test]
fn dead_load_line_is_permitted() {
    classified_as("dead_load.cpp", Class::LoadPlacement);
}

// Class `load-placement`, hod only: hod's load of a call argument on a
// line of its own has a row (20 pairs, 10 modules).
#[test]
#[cfg_attr(
    target_arch = "x86_64",
    ignore = "x86-64 runner: the difference it classifies is AArch64's; on x86-64 hod's and llc's lines are the same; WP-145 finding"
)]
fn argument_load_line_is_permitted() {
    classified_as("arg_load.c", Class::LoadPlacement);
}
