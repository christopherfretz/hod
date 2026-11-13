//! Debug information on x86-64 (WP-136) end to end with the real tools:
//! `cargo xtask debug --target x86_64` (gdb-multiarch and lldb through
//! qemu-x86_64's gdb stub) and `cargo xtask debug trees` (the DIE trees
//! and the relocations of `.debug_info` against `llc`'s). Contract:
//! `spec/harness.md` "debug", "Other targets" and "Trees";
//! `spec/dwarf.md` §6.17.
//!
//! Tools-gated like every oracle test: without the tools (the emulator,
//! the x86-64 system root, clang 22) the test skips with a note; with
//! `HOD_REQUIRE_TOOLS=1` a missing tool is a failure.
#![cfg(target_os = "linux")]

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use common::Scratch;
use xtask::debug::sessions::{self, Options, Outcome};
use xtask::debug::trees;
use xtask::irdiff;
use xtask::irdiff::directives::Arch;

fn build_dir() -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).parent().expect("it is <target>/tmp").to_path_buf()
}

fn debug_test(name: &str) -> PathBuf {
    irdiff::workspace_root().join("tests").join("debug").join(name)
}

fn skip(why: &str) {
    assert!(!hod_testutil::tools_required(), "required tools are missing: {why}");
    eprintln!("note: skipping this tool-backed test: {why}");
}

/// The x86-64 places (sret, byval, memory arguments, a variable-length
/// array, an over-aligned local) and stepping, by clang 22, under every
/// linker and debugger found: hod's sessions print what `llc`'s print.
/// A wrong frame base register or a wrong frame offset shows here as a
/// wrong value.
#[test]
fn x86_64_sessions_match_llc() {
    if Arch::host() == Some(Arch::X86_64) {
        // Not a missing tool: the native sessions are the other tests'.
        eprintln!("note: this test is of the remote sessions; the host is x86-64");
        return;
    }
    let scratch = Scratch::new("debug-x64");
    let mut options = Options::new(build_dir());
    options.hod = Some(irdiff::build_workspace_hod(&build_dir()).expect("hod builds"));
    options.results = Some(scratch.join("results.tsv"));
    options.require_tools = hod_testutil::tools_required();
    options.target = Some(Arch::X86_64);
    options.frontends = vec!["clang-22".to_owned()];
    options.paths = vec![debug_test("x64_places.c"), debug_test("step.c")];
    let report = match sessions::run(&options) {
        Ok(report) => report,
        Err(e) if e.contains("--target x86_64") => return skip(&e),
        Err(e) => panic!("the run cannot be made: {e}"),
    };
    let summary = report.summary(false);
    if report.records.iter().all(|r| r.outcome == Outcome::Skip) {
        return skip(&summary);
    }
    for bad in [Outcome::Fail, Outcome::Xpass, Outcome::Crash, Outcome::Error] {
        assert_eq!(report.count(bad), 0, "{summary}");
    }
    assert!(report.count(Outcome::Pass) >= 2, "{summary}");
    assert_eq!(report.lines_total().unclassified(), 0, "{summary}");
}

/// clang 22's IR of `name` for x86-64 into `dir`, or `None` without it.
fn x86_ir(dir: &Path, name: &str) -> Option<PathBuf> {
    let clang = hod_testutil::find_tool("clang")?;
    let out = dir.join(Path::new(name).with_extension("ll"));
    let mut command = Command::new(clang);
    command.args(["--target=x86_64-unknown-linux-gnu", "-g", "-O0", "-S", "-emit-llvm", "-o"]);
    command.arg(&out).arg(debug_test(name));
    let done = hod_testutil::run_with(&mut command, b"", hod_testutil::DEFAULT_TIMEOUT);
    assert!(done.success(), "clang: {}", done.stderr_text());
    Some(out)
}

/// hod's DIE trees equal `llc`'s, attribute for attribute (the frame
/// base `DW_OP_reg6` included, and each location's operations), and so
/// do the relocations of `.debug_info` (a thread-local's
/// `R_X86_64_DTPOFF64`).
#[test]
fn x86_64_trees_match_llc() {
    let scratch = Scratch::new("debug-x64-trees");
    let mut paths = Vec::new();
    for name in ["x64_places.c", "tls.c", "tls_static.c", "step.c", "globals.c"] {
        match x86_ir(&scratch.join(""), name) {
            Some(ir) => paths.push(ir),
            None => return skip("no clang"),
        }
    }
    let options = trees::Options {
        paths,
        jobs: 2,
        hod: Some(irdiff::build_workspace_hod(&build_dir()).expect("hod builds")),
        build_dir: build_dir(),
        llc: Some(22),
        verbose: true,
    };
    let outcomes = match trees::run(&options) {
        Ok(outcomes) => outcomes,
        Err(e) => return skip(&e),
    };
    let summary = trees::summary(&outcomes, true);
    assert_eq!(outcomes.len(), 5, "{summary}");
    for (module, outcome) in &outcomes {
        let name = module.file_name().unwrap_or_default().to_string_lossy();
        match outcome {
            trees::Outcome::Same(_) if name != "x64_places.ll" => {}
            // `spec/dwarf.md` §6.7, §6.17: `llc`'s location list of the
            // sret pointer and of the array, its byval argument area, its
            // `rsp`-relative places in the realigned frame.
            trees::Outcome::Permitted(classes) if name == "x64_places.ll" => {
                let mut classes = classes.clone();
                classes.sort_unstable();
                assert_eq!(classes, ["byval", "realigned frame", "sret"], "{summary}");
            }
            _ => panic!("{name}: {summary}"),
        }
    }
}
