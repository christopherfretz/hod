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
//! So each is an ignored test here that states what must become true: the
//! harness run on the source has no unclassified pair and no failing
//! session. Every one fails today; run them with
//! `cargo test -p xtask --test debug_m3 -- --ignored`.
#![cfg(target_os = "linux")]

mod common;

use std::path::{Path, PathBuf};

use common::Scratch;
use xtask::debug::sessions::{self, Options, Outcome};
use xtask::irdiff;

fn build_dir() -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).parent().expect("it is <target>/tmp").to_path_buf()
}

/// Runs the harness on one source of `data/debug_lines` and requires the
/// two programs' `(function, line)` sets and sessions to agree.
fn agrees_with_llc(name: &str) {
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
        return;
    }
    assert_eq!(report.lines_total().unclassified(), 0, "{summary}");
    assert_eq!(report.count(Outcome::Fail), 0, "{summary}");
}

// Class `unreachable`: hod's trap for `unreachable` has a row (223 pairs,
// 37 modules). crates/hod_cg (the `unreachable` terminator's code keeps
// the instruction's location).
#[test]
#[ignore = "finding (WP-099): hod's trap for `unreachable` has a line row llc's program lacks"]
fn unreachable_has_no_row() {
    agrees_with_llc("unreachable_line.c");
}

// Class `gep-placement`, hod only: a zero-offset `getelementptr` on a line
// of its own has code in hod's program (213 pairs, 23 modules).
#[test]
#[ignore = "finding (WP-099): hod has code for a getelementptr llc folds into its user"]
fn folded_gep_has_no_row() {
    agrees_with_llc("gep_member.cpp");
}

// Class `gep-placement`, llc only: hod folds a `getelementptr` llc
// computes at its own line (33 pairs, 7 modules).
#[test]
#[ignore = "finding (WP-099): hod folds a getelementptr llc computes at its own line"]
fn gep_with_offset_has_a_row() {
    agrees_with_llc("gep_offset.c");
}

// Class `is-constant`: `llc` deletes the blocks under a folded
// `llvm.is.constant`, hod keeps them (40 pairs, 13 modules).
#[test]
#[ignore = "finding (WP-099): hod keeps the blocks llc deletes under llvm.is.constant"]
fn is_constant_blocks_have_no_rows() {
    agrees_with_llc("is_constant.c");
}

// Class `compare-at-user`: hod's compare is at its user's location
// (10 pairs, 10 modules).
#[test]
#[ignore = "finding (WP-099): hod places a compare at its user's line"]
fn compare_has_its_own_row() {
    agrees_with_llc("compare_user.cpp");
}

// Class `load-placement`, llc only: a dead load `llc` merges with a later
// load keeps its line in `llc`'s program (40 pairs, 13 modules).
#[test]
#[ignore = "finding (WP-099): llc's merged dead load has a row hod's program lacks"]
fn dead_load_line() {
    agrees_with_llc("dead_load.cpp");
}

// Class `load-placement`, hod only: hod's load of a call argument on a
// line of its own has a row (20 pairs, 10 modules).
#[test]
#[ignore = "finding (WP-099): hod's argument load has a row at its own line"]
fn argument_load_line() {
    agrees_with_llc("arg_load.c");
}
