//! The gate's `workflows` step (WP-265; spec/harness.md "ci"):
//! `scripts/check-workflows.py` against the fixtures of
//! `tests/data/workflows/`, one fault each, and against the workspace's own
//! workflow files. No network. Without `PyYAML` (python3-yaml) each test is
//! skipped, or fails under `HOD_REQUIRE_TOOLS=1`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/workflows").join(name)
}

/// The checker's exit status and stdout over `files` (the workspace's
/// files when empty), or `None` when the parser is missing (and tools are
/// not required).
fn check(files: &[PathBuf]) -> Option<(i32, String)> {
    let mut command = Command::new("python3");
    command.arg("-I").arg(root().join(xtask::scripts::CHECK_WORKFLOWS)).args(files);
    let out = hod_testutil::run_with(&mut command, &[], Duration::from_mins(1));
    let code = out.status.and_then(|s| s.code()).expect("check-workflows.py exited");
    if code == xtask::scripts::NO_PARSER {
        let why = String::from_utf8_lossy(&out.stderr).trim().to_owned();
        assert!(!hod_testutil::tools_required(), "{why} (HOD_REQUIRE_TOOLS=1)");
        eprintln!("skipped: {why}; under HOD_REQUIRE_TOOLS=1 this fails");
        return None;
    }
    Some((code, String::from_utf8_lossy(&out.stdout).into_owned()))
}

/// Asserts that `name` fails the check with one finding containing `what`.
fn fails_with(name: &str, what: &str) {
    let Some((code, out)) = check(&[fixture(name)]) else { return };
    assert_eq!(code, 1, "{name}: {out}");
    assert_eq!(out.lines().count(), 1, "{name}: one finding: {out}");
    assert!(out.contains(what), "{name}: expected '{what}' in: {out}");
}

#[test]
fn a_valid_workflow_holds() {
    // `on` is a key (not YAML 1.1's boolean), a reusable-workflow job needs
    // no `runs-on`, `needs` is a name or a list.
    let Some((code, out)) = check(&[fixture("valid.yml")]) else { return };
    assert_eq!((code, out.as_str()), (0, ""));
}

#[test]
fn a_parse_error_fails() {
    fails_with("parse-error.yml", "does not parse");
}

#[test]
fn needs_naming_a_missing_job_fails() {
    fails_with("missing-need.yml", "job check: needs `buidl`, which is not a job");
}

#[test]
fn a_job_without_runs_on_fails() {
    fails_with("no-runs-on.yml", "job build: neither `runs-on` nor `uses`");
}

#[test]
fn a_step_without_run_or_uses_fails() {
    fails_with("step-without-run.yml", "job build: step 2 (setup) has neither `run` nor `uses`");
}

#[test]
fn a_composite_actions_step_without_run_or_uses_fails() {
    fails_with("actions/bad/action.yml", "runs: step 2 (two) has neither `run` nor `uses`");
}

#[test]
fn every_finding_of_several_files_is_reported() {
    let files: Vec<PathBuf> =
        ["valid.yml", "missing-need.yml", "no-runs-on.yml"].map(fixture).into();
    let Some((code, out)) = check(&files) else { return };
    assert_eq!(code, 1);
    assert_eq!(out.lines().count(), 2, "{out}");
}

#[test]
fn the_workspaces_workflows_hold() {
    let Some((code, out)) = check(&[]) else { return };
    assert_eq!((code, out.as_str()), (0, ""));
}
