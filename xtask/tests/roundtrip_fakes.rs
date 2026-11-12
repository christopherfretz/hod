//! `cargo xtask roundtrip` (`spec/harness.md` "roundtrip") against fake
//! tools: an `llvm-as`, an `opt`, an `llc`, a linker driver and a linker
//! that are shell scripts, so that every outcome can be provoked on any
//! Linux machine without LLVM. The modules are real LLVM text, because
//! `hod_ll` and `hod_ir` run in the process for real.
//!
//! How the fakes work. The printed text survives the fakes unchanged:
//! `llvm-as` copies it to the "bitcode", `llc` to the "object". A global
//! of the module named `@fake_<what>` makes a tool misbehave, because the
//! printer keeps global names: `fake_as_refuse`, `fake_as_crash`,
//! `fake_verify_refuse`, `fake_opt_hang`, `fake_llc_fail`,
//! `fake_link_fail`, `fake_wrong_output`. `llc` refuses to run without
//! the reference configuration's flags. A linked "program" is a script
//! that prints `hello`, or `goodbye` for `fake_wrong_output`.
#![cfg(target_os = "linux")]

mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use common::Scratch;
use hod_testutil::write_script;
use xtask::corpus::manifest::{Manifest, Row};
use xtask::irdiff::{Arch, Limits, Search};
use xtask::roundtrip::expected::Verdict;
use xtask::roundtrip::{Options, Outcome, Report, run};

const FAKE_LLVM_AS: &str = r#"PATH=/usr/bin:/bin
out= in=
while [ $# -gt 0 ]; do
  case "$1" in -o) out=$2; shift ;; -*) ;; *) in=$1 ;; esac
  shift
done
if grep -q '@fake_as_refuse' "$in"; then echo "llvm-as: error: expected type" >&2; exit 1; fi
if grep -q '@fake_as_crash' "$in"; then kill -SEGV $$; fi
cp "$in" "$out""#;

const FAKE_OPT: &str = r#"PATH=/usr/bin:/bin
for in; do :; done
if grep -q '@fake_verify_refuse' "$in"; then echo "opt: input module is broken!" >&2; exit 1; fi
if grep -q '@fake_opt_hang' "$in"; then exec sleep 60; fi
exit 0"#;

const FAKE_LLC: &str = r#"PATH=/usr/bin:/bin
if [ "$1" = --version ]; then echo "fake LLVM version 22.1.0"; exit 0; fi
case " $* " in *" -global-isel=0 "*) ;; *) echo "not the reference configuration" >&2; exit 1 ;; esac
case " $* " in *" -fast-isel=0 "*) ;; *) echo "not the reference configuration" >&2; exit 1 ;; esac
out= in=
while [ $# -gt 0 ]; do
  case "$1" in -o) out=$2; shift ;; -*) ;; *) in=$1 ;; esac
  shift
done
if grep -q '@fake_llc_fail' "$in"; then echo "llc: error: cannot select" >&2; exit 1; fi
cp "$in" "$out""#;

const FAKE_GCC: &str = r#"PATH=/usr/bin:/bin
out= in=
while [ $# -gt 0 ]; do
  case "$1" in -o) out=$2; shift ;; -*) ;; *) in=$1 ;; esac
  shift
done
if grep -q '@fake_link_fail' "$in"; then echo "ld: undefined reference to 'x'" >&2; exit 1; fi
word=hello
if grep -q '@fake_wrong_output' "$in"; then word=goodbye; fi
printf '#!/bin/sh\necho %s\n' "$word" > "$out"
chmod 755 "$out""#;

/// A fake installation: the tools of LLVM 22 on a `PATH` of their own.
struct Fakes {
    scratch: Scratch,
}

impl Fakes {
    fn new(label: &str) -> Self {
        let scratch = Scratch::new(label);
        let bin = scratch.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        write_script(&bin.join("llvm-as-22"), FAKE_LLVM_AS);
        write_script(&bin.join("opt-22"), FAKE_OPT);
        write_script(&bin.join("llc-22"), FAKE_LLC);
        write_script(&bin.join("gcc"), FAKE_GCC);
        write_script(&bin.join("ld.bfd"), "exit 0");
        std::fs::create_dir_all(scratch.join("corpus")).unwrap();
        Self { scratch }
    }

    fn corpus(&self) -> PathBuf {
        self.scratch.join("corpus")
    }

    /// Adds an entry for LLVM 22: the directives, then `module`.
    fn entry(&self, name: &str, stdout: &str, module: &str) {
        let arch = Arch::host().expect("a supported host").name();
        let text = format!(
            ";@ covers: fake {name}\n;@ target: {arch}\n;@ llvm: 22\n;@ link: m\n;@ stdout: {stdout}\n\
             target triple = \"{arch}-unknown-linux-gnu\"\n{module}"
        );
        let entry = format!("fake/llvm22/{name}.ll");
        let path = self.corpus().join(&entry);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        let mut manifest = Manifest::read(&self.corpus()).unwrap();
        manifest.rows.insert(
            entry.clone(),
            Row {
                entry,
                source: "fake".to_owned(),
                llvm: 22,
                test: name.to_owned(),
                revision: "unknown".to_owned(),
                producer: "fake".to_owned(),
                producer_args: "-".to_owned(),
                llc_args: "-".to_owned(),
                sha256: "0".repeat(64),
                bytes: 0,
                key: "0".repeat(64),
            },
        );
        manifest.write(&self.corpus()).unwrap();
    }

    fn options(&self) -> Options {
        let mut options = Options::new(self.scratch.join("build"));
        options.corpus = self.corpus();
        options.expected = self.scratch.join("expected.tsv");
        options.search = Search {
            path: self.scratch.join("bin").into_os_string(),
            llvm_root: self.scratch.join("no-llvm"),
        };
        options.tool_limits = Limits { time: Duration::from_secs(2), memory_kib: 4 << 20 };
        options.jobs = 3;
        options
    }
}

/// A `main` that returns 0, with a global named `marker` if there is one.
fn module(marker: Option<&str>) -> String {
    let global = marker.map_or_else(String::new, |name| format!("@{name} = global i32 7\n"));
    format!("{global}define i32 @main() {{\n  ret i32 0\n}}\n")
}

fn outcome_of(report: &Report, name: &str) -> (Outcome, String) {
    let entry = format!("fake/llvm22/{name}.ll");
    let result = report
        .results
        .iter()
        .find(|r| r.entry == entry)
        .unwrap_or_else(|| panic!("no result for {entry}: {report:?}"));
    (result.outcome, result.detail.clone())
}

/// Every outcome a fake can provoke.
fn every_outcome(fakes: &Fakes) {
    fakes.entry("pass", "hello", &module(None));
    fakes.entry("parse_error", "hello", "this is not IR\n");
    fakes.entry(
        "unsupported",
        "hello",
        "declare void @g(ptr, ...)\n\
         define void @f(ptr %p, ...) {\n  musttail call void (ptr, ...) @g(ptr %p, ...)\n  ret void\n}\n",
    );
    for marker in [
        "fake_as_refuse",
        "fake_as_crash",
        "fake_verify_refuse",
        "fake_opt_hang",
        "fake_llc_fail",
        "fake_link_fail",
        "fake_wrong_output",
    ] {
        fakes.entry(marker, "hello", &module(Some(marker)));
    }
    fakes.entry("wrong_directives", "bonjour", &module(None));
}

// spec/harness.md "roundtrip", "Outcomes": each step's failure is its own
// outcome, and a tool's crash or timeout is one too.
#[test]
fn every_outcome_is_named() {
    let fakes = Fakes::new("roundtrip-outcomes");
    every_outcome(&fakes);
    let report = run(&fakes.options()).unwrap();
    let cases = [
        ("pass", Outcome::Pass, ""),
        ("parse_error", Outcome::ParserError, "module: "),
        ("unsupported", Outcome::ParserUnsupported, "call:forwarded-varargs"),
        (
            "fake_as_refuse",
            Outcome::LlvmAsRefused,
            "llvm-as failed (exit status 1): llvm-as: error",
        ),
        ("fake_as_crash", Outcome::ToolCrashed, "llvm-as was terminated by signal 11"),
        ("fake_verify_refuse", Outcome::VerifierRefused, "opt failed (exit status 1): opt: input"),
        ("fake_opt_hang", Outcome::ToolTimedOut, "opt timed out after 2 s"),
        ("fake_llc_fail", Outcome::LlcFailed, "llc failed (exit status 1): llc: error"),
        ("fake_link_fail", Outcome::LinkFailed, "with bfd: link with bfd failed"),
        ("fake_wrong_output", Outcome::BehaviourDiffers, "with bfd: the program of llc's object"),
        ("wrong_directives", Outcome::BehaviourDiffers, "with bfd: the program of llc's object"),
    ];
    for (name, outcome, detail) in cases {
        let (got, text) = outcome_of(&report, name);
        assert_eq!(got, outcome, "{name}: {text}");
        assert!(text.starts_with(detail), "{name}: {text}");
    }
    assert_eq!(report.results.len(), cases.len());
    // No expectation file: every failure is worse than expected.
    assert_eq!(report.results.iter().filter(|r| r.verdict == Verdict::Worse).count(), 10);
    assert_eq!(report.exit_status(), 1);
    let summary = report.summary(false);
    assert!(
        summary.contains("roundtrip: LLVM 22: 11 entries: 1 pass, 1 parser-unsupported"),
        "{summary}"
    );
    assert!(
        summary.contains("WORSE     fake/llvm22/fake_llc_fail.ll: llc-failed (expected pass)"),
        "{summary}"
    );
    let results = std::fs::read_to_string(report.results_file.as_ref().unwrap()).unwrap();
    assert_eq!(results.lines().count(), 11);
    assert!(
        results.contains("llc-failed\tfake/llvm22/fake_llc_fail.ll\t22\tWORSE\tpass\t"),
        "{results}"
    );
}

// "Expectation file", "Verdicts", "Exit status": the file makes the
// command a gate, in both directions.
#[test]
fn the_expectation_file_is_a_gate() {
    let fakes = Fakes::new("roundtrip-gate");
    every_outcome(&fakes);
    let mut options = fakes.options();
    options.update = true;
    let report = run(&options).unwrap();
    assert!(report.updated);
    assert_eq!(report.exit_status(), 0);
    let file = std::fs::read_to_string(&options.expected).unwrap();
    assert_eq!(file.lines().filter(|line| !line.starts_with('#')).count(), 10, "{file}");
    assert!(
        file.contains("fake/llvm22/unsupported.ll\tparser-unsupported\tcall:forwarded-varargs\n")
    );

    options.update = false;
    let report = run(&options).unwrap();
    assert!(report.results.iter().all(|r| r.verdict == Verdict::Expected), "{report:?}");
    assert_eq!(report.exit_status(), 0);

    // An expected failure that now passes: the file is stale.
    let edited = file
        .replace("fake/llvm22/fake_llc_fail.ll\tllc-failed", "fake/llvm22/pass.ll\tllc-failed")
        + "fake/llvm22/fake_llc_fail.ll\tllc-failed\t-\n";
    std::fs::write(&options.expected, &edited).unwrap();
    let report = run(&options).unwrap();
    let pass = report.results.iter().find(|r| r.entry == "fake/llvm22/pass.ll").unwrap();
    assert_eq!(pass.verdict, Verdict::Better);
    assert_eq!(report.exit_status(), 1);
    assert!(
        report.summary(false).contains("BETTER    fake/llvm22/pass.ll: pass (expected llc-failed)")
    );

    // Another failure than the expected one is worse, even of the same
    // rank; an honest "unsupported" where a bug was is better.
    std::fs::write(
        &options.expected,
        file.replace(
            "fake_wrong_output.ll\tbehaviour-differs",
            "fake_wrong_output.ll\tlink-failed",
        )
        .replace("fake_llc_fail.ll\tllc-failed", "fake_llc_fail.ll\tparser-unsupported"),
    )
    .unwrap();
    let report = run(&options).unwrap();
    let verdict = |name: &str| {
        report.results.iter().find(|r| r.entry == format!("fake/llvm22/{name}.ll")).unwrap().verdict
    };
    assert_eq!(verdict("fake_wrong_output"), Verdict::Worse);
    assert_eq!(verdict("fake_llc_fail"), Verdict::Worse);
    assert_eq!(report.exit_status(), 1);

    // A complete run reports listed entries the corpus does not have.
    std::fs::write(&options.expected, file.clone() + "fake/llvm22/gone.ll\tllc-failed\t-\n")
        .unwrap();
    let report = run(&options).unwrap();
    assert_eq!(report.stale, ["fake/llvm22/gone.ll"]);
    assert_eq!(report.exit_status(), 1);
    // A partial one cannot tell.
    options.limit = Some(2);
    let report = run(&options).unwrap();
    assert!(report.stale.is_empty());
    assert_eq!(report.results.len(), 2);
    assert_eq!(report.exit_status(), 0);
}

// "Options": filters, the limit, and the majors.
#[test]
fn selection() {
    let fakes = Fakes::new("roundtrip-select");
    every_outcome(&fakes);
    let mut options = fakes.options();
    options.filters = vec!["fake_as".to_owned(), "pass".to_owned()];
    let report = run(&options).unwrap();
    let names: Vec<&str> = report.results.iter().map(|r| r.entry.as_str()).collect();
    assert_eq!(
        names,
        ["fake/llvm22/fake_as_crash.ll", "fake/llvm22/fake_as_refuse.ll", "fake/llvm22/pass.ll"]
    );
    options.limit = Some(1);
    assert_eq!(run(&options).unwrap().results.len(), 1);
    options.llvm = vec![19];
    assert_eq!(run(&options).unwrap_err(), "no corpus entry is selected");
    options.corpus = fakes.scratch.join("nowhere");
    assert!(run(&options).unwrap_err().starts_with("no corpus at "));
}

// "Tools": without LLVM's tools an entry is parsed and printed, and the
// rest is `skipped`: a note, or under HOD_REQUIRE_TOOLS=1 exit status 3.
#[test]
fn missing_tools() {
    let fakes = Fakes::new("roundtrip-missing");
    fakes.entry("pass", "hello", &module(None));
    fakes.entry("parse_error", "hello", "this is not IR\n");
    std::fs::remove_file(fakes.scratch.join("bin").join("opt-22")).unwrap();
    let mut options = fakes.options();
    let report = run(&options).unwrap();
    assert_eq!(outcome_of(&report, "pass").0, Outcome::Skipped);
    assert!(outcome_of(&report, "pass").1.contains("opt 22"), "{report:?}");
    assert_eq!(outcome_of(&report, "parse_error").0, Outcome::ParserError);
    assert_eq!(report.exit_status(), 1, "the parse error is worse than expected");
    assert!(report.notes.iter().any(|n| n.contains("LLVM 22: opt 22 not found")), "{report:?}");
    options.filters = vec!["pass".to_owned()];
    assert_eq!(run(&options).unwrap().exit_status(), 0);
    options.require_tools = true;
    let report = run(&options).unwrap();
    assert_eq!(report.exit_status(), 3);
    assert!(report.summary(false).contains("roundtrip: error: LLVM 22: opt 22 not found"));
}

// "Work directory": nothing is left behind but what `--keep` keeps.
#[test]
fn work_directory_and_keep() {
    let fakes = Fakes::new("roundtrip-keep");
    fakes.entry("pass", "hello", &module(None));
    fakes.entry("fake_llc_fail", "hello", &module(Some("fake_llc_fail")));
    let mut options = fakes.options();
    let work = |options: &Options| -> Vec<PathBuf> {
        let dir = options.build_dir.join("roundtrip");
        let mut found: Vec<PathBuf> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.is_dir())
            .collect();
        found.sort();
        found
    };
    run(&options).unwrap();
    assert!(work(&options).is_empty());
    options.keep = true;
    let report = run(&options).unwrap();
    let kept = report.kept.clone().expect("a kept directory");
    assert_eq!(work(&options), std::slice::from_ref(&kept));
    let printed: Vec<PathBuf> = std::fs::read_dir(kept.join("llvm22"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.join("printed.ll").is_file())
        .collect();
    assert_eq!(printed.len(), 1, "only the failing entry's printed text is kept");
    let text = std::fs::read_to_string(printed[0].join("printed.ll")).unwrap();
    assert!(text.contains("@fake_llc_fail"), "{text}");
    assert!(Path::new(&kept).starts_with(&options.build_dir));
}
