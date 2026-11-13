//! The cargo steps of `cargo xtask ci` (`spec/harness.md` "ci"). Ported
//! from kiln's `xtask ci`, which met the same build directory.

use std::process::Command;

use crate::jobs::children::run_step;

/// Should a cargo step that failed with `stderr` be run again
/// (`spec/harness.md` "ci")? Once, and only for the two errors rustc gives
/// when a dependency's artifact is not in the build directory although
/// cargo had just written it, which the workspace's file system does to a
/// build under load (its directory listings lag behind fresh files):
///
/// - ``error[E0463]: can't find crate for `X` ``: neither the rlib nor the
///   rmeta of a dependency found by `-L dependency=` was listed;
/// - ``error: crate `X` required to be available in rlib format, but was not
///   found in this form``: its rmeta was listed (written earlier, at the end
///   of analysis) but its rlib (renamed into place at the end of codegen)
///   not yet, and a binary is being linked.
///
/// A rerun finds the artifacts, or rebuilds them. Every other failure is
/// real.
pub fn should_retry(stderr: &str, retried: bool) -> bool {
    !retried && stderr.lines().any(|line| vanished_artifact(line).is_some())
}

/// The rustc error that `line` is, if it is one of those [`should_retry`]
/// retries: its short name for the note.
fn vanished_artifact(line: &str) -> Option<&'static str> {
    let line = line.trim();
    if line.starts_with("error[E0463]") {
        return Some("error[E0463] (can't find crate)");
    }
    let rlib = line.strip_prefix("error: crate `")?.strip_suffix(
        "` required to be available in rlib format, but was not found in this form",
    )?;
    (!rlib.is_empty() && !rlib.contains(char::is_whitespace))
        .then_some("crate required in rlib format")
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
        let why = stderr.lines().find_map(vanished_artifact).unwrap_or("a missing artifact");
        eprintln!(
            "ci: [retry] cargo {shown} failed with {why}: an artifact was not visible to rustc \
             mid-build; running it once more"
        );
        retried = true;
    }
}

/// One `irdiff` run of the gate (`spec/harness.md` "irdiff", "In the
/// gate").
#[cfg(unix)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IrdiffRun {
    /// The oracle's LLVM major; `None` is the default (the newest found).
    pub llvm: Option<u32>,
    /// `None` is the host's.
    pub target: Option<crate::irdiff::Arch>,
    pub tier: crate::irdiff::TierFilter,
    pub mode: crate::irdiff::Mode,
    pub witness: bool,
    /// The tests, relative to the workspace; empty is all of `tests/ir`.
    pub paths: &'static [&'static str],
}

/// The seeds the quick gate runs under the emulator, for the target that
/// is not the host ("In the gate"): a fixed handful that crosses the
/// process interface (arguments, input, exit status), calls into the C
/// library, floating point, and the C++ runtime with an exception, which
/// together take about a minute at load 20.
#[cfg(unix)]
pub const QUICK_CROSS_SEEDS: [&str; 4] = [
    "tests/ir/main_args.ll",
    "tests/ir/exit_float.ll",
    "tests/ir/two_modules.ll",
    "tests/ir/eh_invoke.ll",
];

/// The `irdiff` runs of the gate, in order, on a host of architecture
/// `host` ("In the gate"):
///
/// - quick: the quick tier with the default oracle; then, for the other
///   architecture, [`QUICK_CROSS_SEEDS`] under the emulator;
/// - full: every test with the witness, once per oracle (LLVM 22, 19);
///   every test in hybrid mode with the default oracle; and every test for
///   the other architecture under the emulator, once per oracle, so that a
///   difference between LLVM 19's and 22's programs of that target is a
///   failure of one of the two runs.
///
/// A host that is not a Linux machine of a known architecture runs no
/// cross run: there is no "other" target to emulate.
#[cfg(unix)]
pub fn irdiff_runs(quick: bool, host: Option<crate::irdiff::Arch>) -> Vec<IrdiffRun> {
    use crate::irdiff::tools::LLVM_MAJORS;
    use crate::irdiff::{Arch, Mode, TierFilter};
    let base = IrdiffRun {
        llvm: None,
        target: None,
        tier: TierFilter::All,
        mode: Mode::Strict,
        witness: false,
        paths: &[],
    };
    let other = host.map(|host| if host == Arch::Aarch64 { Arch::X86_64 } else { Arch::Aarch64 });
    let mut runs = Vec::new();
    if quick {
        runs.push(IrdiffRun { tier: TierFilter::Quick, ..base.clone() });
        if let Some(other) = other {
            runs.push(IrdiffRun { target: Some(other), paths: &QUICK_CROSS_SEEDS, ..base });
        }
        return runs;
    }
    for major in LLVM_MAJORS {
        runs.push(IrdiffRun { llvm: Some(major), witness: true, ..base.clone() });
    }
    runs.push(IrdiffRun { mode: Mode::Hybrid, ..base.clone() });
    if let Some(other) = other {
        for major in LLVM_MAJORS {
            runs.push(IrdiffRun { llvm: Some(major), target: Some(other), ..base.clone() });
        }
    }
    runs
}

/// `cargo xtask irdiff` arguments that make the same run, for the log.
#[cfg(unix)]
pub fn irdiff_command(run: &IrdiffRun) -> String {
    let mut words = vec!["cargo xtask irdiff".to_owned()];
    if let Some(major) = run.llvm {
        words.push(format!("--llvm {major}"));
    }
    if let Some(target) = run.target {
        words.push(format!("--target {}", target.name()));
    }
    if run.tier == crate::irdiff::TierFilter::Quick {
        words.push("--tier quick".to_owned());
    }
    if run.mode == crate::irdiff::Mode::Hybrid {
        words.push("--hybrid".to_owned());
    }
    if run.witness {
        words.push("--witness".to_owned());
    }
    words.extend(run.paths.iter().map(|path| (*path).to_owned()));
    words.join(" ")
}

#[cfg(test)]
mod tests {
    use super::should_retry;

    // spec/harness.md "irdiff", "In the gate": the runs of each tier. The
    // full gate runs the other architecture with BOTH oracles: one run
    // with LLVM 22 alone would let a program LLVM 19 builds differently
    // for that target go unseen.
    #[cfg(unix)]
    #[test]
    fn the_gates_irdiff_runs() {
        use super::{QUICK_CROSS_SEEDS, irdiff_command, irdiff_runs};
        use crate::irdiff::Arch;
        let commands = |quick, host| -> Vec<String> {
            irdiff_runs(quick, host).iter().map(irdiff_command).collect()
        };
        let seeds = QUICK_CROSS_SEEDS.join(" ");
        assert_eq!(
            commands(true, Some(Arch::Aarch64)),
            [
                "cargo xtask irdiff --tier quick".to_owned(),
                format!("cargo xtask irdiff --target x86_64 {seeds}")
            ]
        );
        assert_eq!(
            commands(false, Some(Arch::Aarch64)),
            [
                "cargo xtask irdiff --llvm 22 --witness",
                "cargo xtask irdiff --llvm 19 --witness",
                "cargo xtask irdiff --hybrid",
                "cargo xtask irdiff --llvm 22 --target x86_64",
                "cargo xtask irdiff --llvm 19 --target x86_64",
            ]
        );
        let on_x86 = commands(false, Some(Arch::X86_64));
        assert_eq!(on_x86[3], "cargo xtask irdiff --llvm 22 --target aarch64");
        assert_eq!(commands(true, None), ["cargo xtask irdiff --tier quick"]);
        assert_eq!(commands(false, None).len(), 3);
        // The quick cross seeds exist, and none is in another LLVM's
        // dialect or for one target only.
        for seed in QUICK_CROSS_SEEDS {
            let text = std::fs::read_to_string(crate::irdiff::workspace_root().join(seed));
            let expect = crate::irdiff::directives::parse(&text.unwrap()).unwrap();
            assert!(expect.dialect.is_none() && Arch::ALL.iter().all(|a| expect.runs_on(*a)));
        }
    }

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

    // spec/harness.md "ci": the rlib form of the same cause, exactly as
    // rustc prints it, is retried once; its relatives are not.
    #[test]
    fn retries_a_crate_seen_only_as_rmeta_once() {
        let rlib = "   Compiling hod v0.0.0\n\
                    error: crate `hod_cg` required to be available in rlib format, but was not \
                    found in this form\n\n\
                    error: could not compile `hod` (test \"cli\") due to 1 previous error\n";
        assert!(should_retry(rlib, false));
        assert!(!should_retry(rlib, true), "a second failure is real");
        let unit = "error: crate `hod_cg` required to be available in dylib format, but was not \
                    found in this form\n";
        assert!(!should_retry(unit, false), "hod links no dylib: that is a real error");
        assert!(!should_retry(
            "error: crate `` required to be available in rlib format, but was \
                               not found in this form\n",
            false
        ));
        assert!(!should_retry(
            "note: error: crate `a` required to be available in rlib format, \
                               but was not found in this form\n",
            false
        ));
        assert!(!should_retry("error: crate `a` required to be available in rlib format\n", false));
        assert!(!should_retry("error: linking with `cc` failed: exit status: 1\n", false));
    }
}
