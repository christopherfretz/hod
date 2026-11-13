//! WP-111: the `post-indexed-load` class of `spec/dwarf.md` §5.7 on a
//! reduction of the last unclassified `(function, file, line)` pairs of
//! WP-097 (`std::_Sp_counted_base::_M_release_last_use`,
//! `shared_ptr_base.h:187`): a member's address on a line of its own,
//! which `llc` folds into the post-indexed load of the vtable pointer. The
//! harness run on the source has no unclassified pair, and every pair one
//! side lacks (besides P1 and P3) is of that class.
#![cfg(target_os = "linux")]

mod common;

use std::path::{Path, PathBuf};

use common::Scratch;
use xtask::debug::lines::Class;
use xtask::debug::sessions::{self, Options, Outcome};
use xtask::irdiff;
use xtask::irdiff::directives::Arch;

fn build_dir() -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).parent().expect("it is <target>/tmp").to_path_buf()
}

/// The harness run on `post_index.cpp` for `target` (`None`: this
/// machine's); `None` when the tools are missing (and not required).
fn run(target: Option<Arch>) -> Option<sessions::Report> {
    let scratch = Scratch::new("debug-post-index");
    let mut options = Options::new(build_dir());
    options.hod = Some(irdiff::build_workspace_hod(&build_dir()).expect("hod builds"));
    options.results = Some(scratch.join("results.tsv"));
    options.require_tools = hod_testutil::tools_required();
    options.target = target;
    options.paths = vec![
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/debug_post_index/post_index.cpp"),
    ];
    let report = match sessions::run(&options) {
        Ok(report) => report,
        Err(e) if target.is_some() && e.contains("--target") => {
            assert!(!options.require_tools, "required tools are missing: {e}");
            eprintln!("note: skipping this tool-backed test: {e}");
            return None;
        }
        Err(e) => panic!("the run cannot be made: {e}"),
    };
    let summary = report.summary(true);
    if report.records.iter().all(|r| r.outcome == Outcome::Skip) {
        assert!(!options.require_tools, "required tools are missing:\n{summary}");
        eprintln!("note: skipping this tool-backed test:\n{summary}");
        return None;
    }
    Some(report)
}

// Post-indexed loads are AArch64's: on x86-64 the class cannot occur, and
// the x86-64 program's lines are the next test's.
#[test]
#[cfg_attr(
    target_arch = "x86_64",
    ignore = "the class is AArch64's (post-indexed loads); x86-64's lines are \
              `fused_compare_keeps_its_line_on_x86_64`'s (WP-145 finding, WP-155)"
)]
fn member_address_after_a_load_of_its_base_is_permitted() {
    let Some(report) = run(None) else { return };
    let summary = report.summary(true);
    let lines = report.lines_total();
    assert_eq!(lines.unclassified(), 0, "{summary}");
    let differing: Vec<Class> = lines
        .hod_only
        .iter()
        .chain(&lines.llc_only)
        .map(|(_, c)| *c)
        .filter(|c| !matches!(c, Class::P1 | Class::P3))
        .collect();
    assert!(
        !differing.is_empty() && differing.iter().all(|c| *c == Class::PostIndexedLoad),
        "{summary}"
    );
}

// The same program for x86-64 (natively there, remotely elsewhere): with
// clang 19's IR, `llc` gives the `cmp` of the `icmp` that the branch of
// line 18 tests its own line (19:17, then the `jne` at 18:9); hod emits
// the fused compare at the branch's line, so its table has no row at line
// 19 (`llc` for AArch64 folds the compare into the branch's line as hod
// does). The fused compare-and-branch of crates/hod_x64/src/control.rs
// (`fused_icmp`) would keep the `icmp`'s location for the compare.
#[test]
#[ignore = "hod_x64: a fused icmp's cmp has the branch's line, llc's has the icmp's \
            (post_index.cpp:19 in _ZN7Counted7releaseEv, clang 19); WP-145 finding, WP-155"]
fn fused_compare_keeps_its_line_on_x86_64() {
    let target = (Arch::host() != Some(Arch::X86_64)).then_some(Arch::X86_64);
    let Some(report) = run(target) else { return };
    let summary = report.summary(true);
    assert_eq!(report.lines_total().unclassified(), 0, "{summary}");
    assert_eq!(report.count(Outcome::Fail), 0, "{summary}");
}
