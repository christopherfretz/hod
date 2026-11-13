//! Pre-opt modules with coroutines through the `hod` binary (WP-109;
//! `spec/cli.md` "Pre-opt modules", "kiln's contract"): the module a front
//! end writes before LLVM's `-O0` pipeline, on standard input, with the
//! `llc` arguments it planned.
//!
//! - Strict mode: a coroutine hod compiles is written (status 0, both LLVM
//!   dialects); one it does not is declined with status 2, the construct
//!   named, nothing written, the reason in the coverage log: the plan's
//!   refusal on AArch64, `attr:presplitcoroutine` on x86-64.
//! - Hybrid mode: the fallback runs exactly for the declined module, with
//!   the same bytes; with a real `opt` + `llc` wrapper as the fallback the
//!   declined module still becomes an object.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hod_testutil::{Outcome, Scratch, run_with, tool};

const GEN_19: &str = include_str!("../../../tests/ir/coro/gen-19.ll");
const GEN_22: &str = include_str!("../../../tests/ir/coro/gen-22.ll");
const POST_SYNC: &str =
    include_str!("../../hod_xform/tests/data/seeds/llvm22/post-sync-22.ll");

const A64: &str = "-mtriple=aarch64-unknown-linux-gnu";
const X64: &str = "-mtriple=x86_64-unknown-linux-gnu";

/// `hod` with a clean `HOD_*` environment.
fn hod() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_hod"));
    for var in [
        "HOD_FALLBACK_LLC",
        "HOD_STRICT",
        "HOD_COVERAGE_LOG",
        "HOD_BITCODE_VIA",
        "HOD_IN_FALLBACK",
        "HOD_TIME_PHASES",
        "HOD_CENSUS",
        "HOD_THREADS",
        "MAKEFLAGS",
    ] {
        command.env_remove(var);
    }
    command
}

fn run_hod(command: &mut Command, stdin: &str) -> Outcome {
    let outcome = run_with(command, stdin.as_bytes(), Duration::from_mins(1));
    assert!(!outcome.timed_out(), "hod timed out");
    outcome
}

fn status(outcome: &Outcome) -> i32 {
    outcome.status.and_then(|s| s.code()).expect("hod exits with a status")
}

/// The coverage log's lines as (outcome, reason).
fn logged(log: &Path) -> Vec<(String, String)> {
    let text = std::fs::read_to_string(log).expect("the coverage log");
    text.lines()
        .map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            (fields[0].to_owned(), fields[1].to_owned())
        })
        .collect()
}

/// kiln's command: `hod --strict <llc arguments> -o <object>`, the module
/// on standard input.
fn strict(triple: &str, out: &Path, log: &Path) -> Command {
    let mut command = hod();
    command
        .arg("--strict")
        .arg(format!("--coverage-log={}", log.display()))
        .args([triple, "-O0", "-relocation-model=pic", "-filetype=obj", "-o"])
        .arg(out);
    command
}

fn is_elf(path: &Path) -> bool {
    std::fs::read(path).is_ok_and(|bytes| bytes.starts_with(b"\x7FELF"))
}

// spec/cli.md "Pre-opt modules", "kiln's contract": strict mode builds the
// coroutine modules hod compiles and declines the others with status 2,
// writing nothing.
#[test]
fn strict_mode_builds_or_declines_by_name() {
    let scratch = Scratch::new("preopt-coro-strict");
    let log = scratch.join("cov.tsv");
    for (name, text) in [("gen-19", GEN_19), ("gen-22", GEN_22)] {
        let out = scratch.join(format!("{name}.o"));
        let outcome = run_hod(&mut strict(A64, &out, &log), text);
        assert_eq!(status(&outcome), 0, "{name}: {}", outcome.stderr_text());
        assert!(is_elf(&out), "{name}: an object");
    }

    let declined = [
        (
            A64,
            POST_SYNC,
            "hod: error: unsupported construct 'coro:frame-access-after-suspend' in function \
             '_Z1fv' (strict mode: no fallback)\n",
            "coro:frame-access-after-suspend",
        ),
        (
            X64,
            GEN_22,
            "hod: error: unsupported construct 'attr:presplitcoroutine' (strict mode: no \
             fallback)\n",
            "attr:presplitcoroutine",
        ),
    ];
    for (triple, text, stderr, _) in declined {
        let out = scratch.join("declined.o");
        let outcome = run_hod(&mut strict(triple, &out, &log), text);
        assert_eq!(status(&outcome), 2, "{}", outcome.stderr_text());
        assert_eq!(outcome.stderr_text(), stderr);
        assert!(!out.exists(), "nothing is written for a declined module");
    }
    let lines = logged(&log);
    let reasons: Vec<(&str, &str)> =
        lines.iter().map(|(outcome, reason)| (outcome.as_str(), reason.as_str())).collect();
    assert_eq!(
        reasons,
        [
            ("hod", "-"),
            ("hod", "-"),
            ("error", declined[0].3),
            ("error", declined[1].3)
        ]
    );
}

/// A stand-in fallback: records that it ran and its standard input.
fn recording_fallback(scratch: &Scratch) -> PathBuf {
    scratch.write_script("fallback", "cat > \"$0.stdin\"\nexit 0")
}

// spec/cli.md "Pre-opt modules": in hybrid mode the fallback runs for the
// declined module only, with the same bytes.
#[test]
fn hybrid_mode_falls_back_for_the_declined_module_only() {
    let scratch = Scratch::new("preopt-coro-hybrid");
    let log = scratch.join("cov.tsv");
    let fallback = recording_fallback(&scratch);
    let recorded = scratch.join("fallback.stdin");
    let hybrid = |triple: &str, out: &Path| {
        let mut command = hod();
        command
            .arg(format!("--fallback={}", fallback.display()))
            .arg(format!("--coverage-log={}", log.display()))
            .args([triple, "-O0", "-relocation-model=pic", "-filetype=obj", "-o"])
            .arg(out);
        command
    };
    let out = scratch.join("gen.o");
    let outcome = run_hod(&mut hybrid(A64, &out), GEN_22);
    assert_eq!(status(&outcome), 0, "{}", outcome.stderr_text());
    assert!(is_elf(&out) && !recorded.exists(), "hod built it; no fallback");

    for (triple, text) in [(A64, POST_SYNC), (X64, GEN_22)] {
        let outcome = run_hod(&mut hybrid(triple, &scratch.join("declined.o")), text);
        assert_eq!(status(&outcome), 0, "{}", outcome.stderr_text());
        assert_eq!(std::fs::read_to_string(&recorded).unwrap(), text, "the same bytes");
        std::fs::remove_file(&recorded).unwrap();
    }
    let outcomes: Vec<String> = logged(&log).into_iter().map(|(outcome, _)| outcome).collect();
    assert_eq!(outcomes, ["hod", "fallback", "fallback"]);
}

// spec/cli.md "kiln's contract": the fallback of a pre-opt module is
// `opt -passes='default<O0>'` then `llc`; through it the declined module
// is an object.
#[test]
fn a_declined_module_becomes_an_object_through_opt_and_llc() {
    let (Some(opt), Some(llc)) = (tool("opt-22"), tool("llc-22")) else { return };
    let scratch = Scratch::new("preopt-coro-opt-llc");
    let body = format!(
        "t=\"$0.pre.ll\"\n'{}' -passes='default<O0>' -S -o \"$t\" - || exit $?\n\
         '{}' -global-isel=0 -fast-isel=0 \"$@\" \"$t\"\nstatus=$?\nrm -f \"$t\"\nexit $status",
        opt.display(),
        llc.display()
    );
    let wrapper = scratch.write_script("opt-llc", &body);
    let out = scratch.join("post-sync.o");
    let outcome = run_hod(
        hod()
            .arg(format!("--fallback={}", wrapper.display()))
            .args([A64, "-O0", "-relocation-model=pic", "-filetype=obj", "-o"])
            .arg(&out),
        POST_SYNC,
    );
    assert_eq!(status(&outcome), 0, "{}", outcome.stderr_text());
    assert!(is_elf(&out), "opt + llc wrote the object");
}
