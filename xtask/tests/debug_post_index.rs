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

fn build_dir() -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).parent().expect("it is <target>/tmp").to_path_buf()
}

#[test]
#[cfg_attr(
    target_arch = "x86_64",
    ignore = "x86-64 runner: on x86-64 llc has an unclassified line (post_index.cpp:19 in _ZN7Counted7releaseEv, clang 19); WP-145 finding"
)]
fn member_address_after_a_load_of_its_base_is_permitted() {
    let scratch = Scratch::new("debug-post-index");
    let mut options = Options::new(build_dir());
    options.hod = Some(irdiff::build_workspace_hod(&build_dir()).expect("hod builds"));
    options.results = Some(scratch.join("results.tsv"));
    options.require_tools = hod_testutil::tools_required();
    options.paths = vec![
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/debug_post_index/post_index.cpp"),
    ];
    let report = sessions::run(&options).expect("the run can be made");
    let summary = report.summary(true);
    if report.records.iter().all(|r| r.outcome == Outcome::Skip) {
        assert!(!options.require_tools, "required tools are missing:\n{summary}");
        eprintln!("note: skipping this tool-backed test:\n{summary}");
        return;
    }
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
