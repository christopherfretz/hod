//! The cargo steps of `cargo xtask ci` (`spec/harness.md` "ci"). Ported
//! from kiln's `xtask ci`, which met the same build directory.

use std::process::Command;

use crate::jobs::children::run_step;

/// Should a cargo step that failed with `stderr` be run again
/// (`spec/harness.md` "ci")? Once, and only for rustc's `error[E0463]`
/// (can't find crate): a dependency's artifact was not there when a
/// dependent was compiled, which the workspace's file system does to a
/// build under load although cargo had just written it; a rerun finds or
/// rebuilds it. Every other failure is real.
pub fn should_retry(stderr: &str, retried: bool) -> bool {
    !retried && stderr.lines().any(|line| line.trim_start().starts_with("error[E0463]"))
}

/// Runs `cargo <args>`, echoing its standard error while reading it, and
/// runs it once more when it failed as [`should_retry`] describes. Returns
/// whether the step passed.
///
/// The step is a jobserver client of its own and is stopped with `xtask`
/// ([`run_step`], `spec/harness.md` "Jobserver").
pub fn cargo_step(args: &[&str]) -> bool {
    let shown = args.join(" ");
    let mut retried = false;
    loop {
        let mut cargo = Command::new(env!("CARGO"));
        let (status, stderr) = match run_step(cargo.args(args), true) {
            Ok(result) => result,
            Err(e) => {
                eprintln!("ci: cannot run cargo {shown}: {e}");
                return false;
            }
        };
        if status.success() {
            return true;
        }
        if !should_retry(&stderr, retried) {
            eprintln!("ci: cargo {shown}: {status}");
            return false;
        }
        eprintln!(
            "ci: [retry] cargo {shown} failed with error[E0463] (can't find crate: an artifact \
             vanished mid-build); running it once more"
        );
        retried = true;
    }
}

#[cfg(test)]
mod tests {
    use super::should_retry;

    // spec/harness.md "ci": one rerun, only for a missing crate, and only
    // when rustc said so at the start of a line.
    #[test]
    fn retries_only_missing_crates_once() {
        let missing = "   Compiling hod_ir v0.0.0\n\
                       error[E0463]: can't find crate for `hod_ir`\n \
                       --> crates/hod_interp/tests/common/mod.rs:7:5\n";
        assert!(should_retry(missing, false));
        assert!(!should_retry(missing, true), "a second failure is real");
        assert!(should_retry("warning: x\n  error[E0463]: can't find crate for `std`\n", false));
        assert!(!should_retry("error[E0308]: mismatched types\n", false));
        assert!(!should_retry("test result: FAILED. 1 passed; 1 failed\n", false));
        assert!(!should_retry("note: the test checks that error[E0463] is retried\n", false));
        assert!(!should_retry("", false));
    }
}
