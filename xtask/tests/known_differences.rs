//! Differences from the oracle found by work packages in code they do
//! not own, each an `irdiff` test under `tests/data/known/` that passes
//! once the owner fixes it. Ignored until then, with the reason; run them
//! with `cargo test -p xtask --test known_differences -- --ignored`.
#![cfg(target_os = "linux")]

mod common;

use std::path::{Path, PathBuf};

use common::Scratch;
use xtask::irdiff::{self, Config};

fn run(name: &str) {
    let scratch = Scratch::new("known-differences");
    let mut config = Config::new(scratch.join("build"));
    config.paths = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/known").join(name)];
    let build = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let build = build.parent().expect("it is <target>/tmp");
    config.hod = Some(irdiff::build_workspace_hod(build).expect("hod builds"));
    let report = irdiff::run(&config).unwrap();
    assert_eq!(report.exit_status(), 0, "{}", report.summary(true));
}

// spec/ir.md §5.5 (WP-086): llc folds `llvm.is.constant` of a literal
// aggregate to true, hod to false.
#[test]
#[ignore = "hod_a64 folds is.constant of an aggregate literal to false (crates/hod_a64/src/intrinsic.rs, the IsConstant arm)"]
fn is_constant_of_an_aggregate_literal() {
    run("is_constant_aggregate.ll");
}

// spec/ir.md §5.5 (WP-086): llc deletes the dead operand of a folded
// `llvm.is.constant` (GCC's bcp-1.c), hod still runs its load.
#[test]
#[ignore = "hod_a64 still emits the dead operand of a folded is.constant (crates/hod_a64/src/intrinsic.rs, the IsConstant arm)"]
fn is_constant_drops_its_dead_operand() {
    run("is_constant_dead_operand.ll");
}
