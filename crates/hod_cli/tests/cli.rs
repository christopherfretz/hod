//! The `hod` binary end to end, against `spec/cli.md`.
//!
//! The fallback and the bitcode converter here are scripts that record what
//! they were given, so these tests need no LLVM. The `real_*` tests use the
//! real `llc` and `llvm-dis` (and skip without them, see
//! `hod_testutil::tool`).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hod_testutil::{LLVM_MAJORS, Outcome, Scratch, run_with, tool, tools_required};

/// A module hod declines, as `module:asm`: module-level assembly is
/// not a construct a code-generation package turns on, so the reason does
/// not move with the support matrix.
const MODULE: &str = "\
; ModuleID = 'm.cpp'
source_filename = \"m.cpp\"
target datalayout = \"e-m:e-p270:32:32-p271:32:32-p272:64:64-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128-Fn32\"
target triple = \"aarch64-unknown-linux-gnu\"

module asm \"nop\"

define i32 @seven() {
  ret i32 7
}
";

/// The fields of a coverage-log line (spec/cli.md "Coverage log").
#[derive(Debug)]
struct Line {
    outcome: String,
    reason: String,
    input_kind: String,
    input_bytes: String,
    output: String,
    target: String,
    hod_micros: String,
    fallback_micros: String,
    conversion_micros: String,
    fallback_status: String,
    reasons: String,
}

/// Reads a coverage log, checking the shape of every line: newline
/// terminated, eleven tab-separated fields, the vocabulary of each.
fn coverage(log: &Path) -> Vec<Line> {
    let text = std::fs::read_to_string(log).unwrap_or_else(|e| panic!("{}: {e}", log.display()));
    assert!(text.ends_with('\n'), "the log ends with a newline: {text:?}");
    let number = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    text.lines()
        .map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            assert_eq!(fields.len(), 11, "eleven fields: {line:?}");
            assert!(fields.iter().all(|f| !f.is_empty()), "no empty field: {line:?}");
            assert!(["hod", "fallback", "error"].contains(&fields[0]), "outcome: {line:?}");
            assert!(["text", "bitcode", "-"].contains(&fields[2]), "input kind: {line:?}");
            assert!(fields[3] == "-" || number(fields[3]), "input bytes: {line:?}");
            assert!(number(fields[6]), "hod time: {line:?}");
            assert!(fields[7] == "-" || number(fields[7]), "fallback time: {line:?}");
            assert_eq!(fields[0] == "fallback", fields[7] != "-", "fallback time: {line:?}");
            assert_eq!(fields[0] == "hod", fields[1] == "-", "reason: {line:?}");
            assert!(fields[8] == "-" || number(fields[8]), "conversion time: {line:?}");
            assert!(fields[8] == "-" || fields[2] == "bitcode", "only bitcode converts: {line:?}");
            let status = fields[9];
            assert!(
                status == "-"
                    || number(status)
                    || status.strip_prefix("signal:").is_some_and(number),
                "fallback status: {line:?}"
            );
            assert_eq!(fields[0] == "fallback", status != "-", "fallback status: {line:?}");
            Line {
                outcome: fields[0].to_owned(),
                reason: fields[1].to_owned(),
                input_kind: fields[2].to_owned(),
                input_bytes: fields[3].to_owned(),
                output: fields[4].to_owned(),
                target: fields[5].to_owned(),
                hod_micros: fields[6].to_owned(),
                fallback_micros: fields[7].to_owned(),
                conversion_micros: fields[8].to_owned(),
                fallback_status: fields[9].to_owned(),
                reasons: fields[10].to_owned(),
            }
        })
        .collect()
}

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

fn run_hod(command: &mut Command, stdin: &[u8]) -> Outcome {
    let outcome = run_with(command, stdin, Duration::from_mins(1));
    assert!(!outcome.timed_out(), "hod timed out");
    outcome
}

fn exit_code(outcome: &Outcome) -> i32 {
    outcome.status.and_then(|s| s.code()).expect("hod exits with a status")
}

/// A stand-in for `llc`: records its arguments (one per line) in
/// `<script>.args` and its standard input in `<script>.stdin`, prints a line
/// on each output stream, and exits with `$FAKE_LLC_EXIT` (default 0).
struct FakeLlc {
    path: PathBuf,
}

impl FakeLlc {
    fn new(scratch: &Scratch, name: &str) -> Self {
        let body = "\
: > \"$0.args\"
for a in \"$@\"; do printf '%s\\n' \"$a\" >> \"$0.args\"; done
cat > \"$0.stdin\"
echo 'out from llc'
echo 'err from llc' >&2
exit \"${FAKE_LLC_EXIT:-0}\"";
        Self { path: scratch.write_script(name, body) }
    }

    fn recorded(&self, what: &str) -> PathBuf {
        let mut name = self.path.clone().into_os_string();
        name.push(what);
        PathBuf::from(name)
    }

    fn ran(&self) -> bool {
        self.recorded(".args").exists()
    }

    fn args(&self) -> Vec<String> {
        let text = std::fs::read_to_string(self.recorded(".args")).expect("the fallback ran");
        text.lines().map(str::to_owned).collect()
    }

    fn stdin(&self) -> Vec<u8> {
        std::fs::read(self.recorded(".stdin")).expect("the fallback ran")
    }
}

// spec/cli.md "Fallback protocol": the fallback gets the original arguments
// without hod's own, and the same bytes on standard input; its output
// streams and exit status are hod's.
#[test]
fn fallback_receives_original_arguments_and_identical_input() {
    let scratch = Scratch::new("cli-fallback");
    let llc = FakeLlc::new(&scratch, "llc");
    let log = scratch.join("cov.tsv");
    let out = scratch.join("a.o");
    // Larger than any pipe buffer, with bytes that are not UTF-8.
    let mut input = MODULE.as_bytes().to_vec();
    while input.len() < 3 << 20 {
        input.extend_from_slice(b"; padding \x00\xFF\xFE line\n");
    }
    let outcome = run_hod(
        hod()
            .arg("-O0")
            .arg(format!("--fallback={}", llc.path.display()))
            .args(["-relocation-model=pic", "-filetype=obj", "--strict=false"])
            .args(["-function-sections", "-mcpu=generic", "-debugger-tune=gdb", "--threads=2"])
            .arg("--coverage-log")
            .arg(&log)
            .arg("-o")
            .arg(&out)
            .arg("-"),
        &input,
    );
    assert_eq!(exit_code(&outcome), 0, "{}", outcome.stderr_text());
    assert_eq!(outcome.stdout_text(), "out from llc\n");
    assert_eq!(outcome.stderr_text(), "err from llc\n");
    assert_eq!(
        llc.args(),
        [
            "-O0",
            "-relocation-model=pic",
            "-filetype=obj",
            "-function-sections",
            "-mcpu=generic",
            "-debugger-tune=gdb",
            "-o",
            out.to_str().unwrap(),
            "-"
        ]
    );
    assert!(llc.stdin() == input, "the fallback reads the bytes hod read");

    let lines = coverage(&log);
    assert_eq!(lines.len(), 1);
    let line = &lines[0];
    assert_eq!(
        (line.outcome.as_str(), line.reason.as_str(), line.input_kind.as_str()),
        ("fallback", "module:asm", "text")
    );
    assert_eq!(line.input_bytes, input.len().to_string());
    assert_eq!(line.output, out.to_str().unwrap());
    assert_eq!(line.target, "aarch64-unknown-linux-gnu");
    assert!(line.hod_micros.parse::<u64>().is_ok() && line.fallback_micros.parse::<u64>().is_ok());
    assert_eq!((line.conversion_micros.as_str(), line.fallback_status.as_str()), ("-", "0"));
}

// spec/cli.md "Threads": `--threads=<n>` and `HOD_THREADS` change how many
// threads build the object, never its bytes; the option wins over the
// variable, so a bad variable is still an error (it is read first).
#[test]
fn threads_option() {
    let scratch = Scratch::new("cli-threads");
    // Enough instructions for several threads (one per 4096).
    let mut module = String::from("target triple = \"aarch64-unknown-linux-gnu\"\n");
    for f in 0..24 {
        module.push_str(&format!("define i64 @f{f}(i64 %x) {{\n  %v0 = add i64 %x, {f}\n"));
        for i in 1..800 {
            module.push_str(&format!("  %v{i} = add i64 %v{}, {i}\n", i - 1));
        }
        module.push_str("  ret i64 %v799\n}\n");
    }
    let build = |command: &mut Command, name: &str| {
        let out = scratch.join(name);
        let outcome = run_hod(
            command.args(["-filetype=obj", "--strict", "-o"]).arg(&out).arg("-"),
            module.as_bytes(),
        );
        assert_eq!(exit_code(&outcome), 0, "{name}: {}", outcome.stderr_text());
        std::fs::read(&out).unwrap()
    };
    let one = build(&mut hod(), "one.o");
    assert!(one.starts_with(b"\x7FELF"));
    assert!(build(hod().arg("--threads=4"), "four.o") == one);
    assert!(build(hod().env("HOD_THREADS", "4").arg("--threads=1"), "mixed.o") == one);
    assert!(build(hod().args(["--threads", "3"]), "three.o") == one);
    let outcome = run_hod(hod().args(["-filetype=obj", "--threads=0", "-"]), module.as_bytes());
    assert_eq!(exit_code(&outcome), 1);
    assert!(outcome.stderr_text().contains("invalid value '0' for '--threads'"));
    let outcome =
        run_hod(hod().env("HOD_THREADS", "x").args(["--threads=2", "-"]), module.as_bytes());
    assert_eq!(exit_code(&outcome), 1);
    assert!(outcome.stderr_text().contains("HOD_THREADS"), "{}", outcome.stderr_text());
}

// spec/cli.md "Environment": the same settings from the environment, and
// the command line wins over it.
#[test]
fn settings_from_the_environment() {
    let scratch = Scratch::new("cli-env");
    let from_env = FakeLlc::new(&scratch, "llc-env");
    let from_flag = FakeLlc::new(&scratch, "llc-flag");
    let log_env = scratch.join("env.tsv");
    let log_flag = scratch.join("flag.tsv");

    let outcome = run_hod(
        hod()
            .env("HOD_FALLBACK_LLC", &from_env.path)
            .env("HOD_COVERAGE_LOG", &log_env)
            .env("HOD_STRICT", "0")
            .args(["-O0", "-filetype=obj", "-o", "x.o"]),
        MODULE.as_bytes(),
    );
    assert_eq!(exit_code(&outcome), 0, "{}", outcome.stderr_text());
    assert_eq!(from_env.args(), ["-O0", "-filetype=obj", "-o", "x.o"]);
    assert_eq!(from_env.stdin(), MODULE.as_bytes());
    assert_eq!(coverage(&log_env).len(), 1);

    let outcome = run_hod(
        hod()
            .env("HOD_FALLBACK_LLC", scratch.join("not-used"))
            .env("HOD_COVERAGE_LOG", scratch.join("no-such-dir/not-used.tsv"))
            .env("HOD_STRICT", "1")
            .arg(format!("--fallback={}", from_flag.path.display()))
            .arg(format!("--coverage-log={}", log_flag.display()))
            .args(["--strict=false", "-filetype=obj"]),
        MODULE.as_bytes(),
    );
    assert_eq!(exit_code(&outcome), 0, "{}", outcome.stderr_text());
    assert_eq!(from_flag.args(), ["-filetype=obj"]);
    assert_eq!(coverage(&log_flag).len(), 1);
    assert_eq!(coverage(&log_env).len(), 1, "the environment's log was not used again");

    let outcome = run_hod(hod().env("HOD_STRICT", "yes").arg("-filetype=obj"), b"");
    assert_eq!(exit_code(&outcome), 1);
    assert_eq!(
        outcome.stderr_text(),
        "hod: error: invalid value 'yes' for HOD_STRICT: expected 1 or 0\n"
    );
}

// spec/cli.md "Phase report": the time in the fallback is a phase.
#[test]
fn phase_report_with_a_fallback() {
    let scratch = Scratch::new("cli-phases");
    let llc = FakeLlc::new(&scratch, "llc");
    let report = scratch.join("phases.tsv");
    let names = || {
        let text = std::fs::read_to_string(&report).expect("a report");
        let _ = std::fs::remove_file(&report);
        let lines: Vec<(&str, &str)> = text.lines().map(|l| l.split_once('\t').unwrap()).collect();
        assert!(lines.iter().all(|(_, micros)| micros.parse::<u64>().is_ok()), "{text:?}");
        lines.iter().map(|(name, _)| *name).collect::<Vec<_>>().join(" ")
    };
    let mut fall_back = hod();
    fall_back.env("HOD_TIME_PHASES", &report).env("HOD_FALLBACK_LLC", &llc.path);
    let outcome = run_hod(fall_back.args(["-filetype=obj", "-o", "x.o"]), MODULE.as_bytes());
    assert_eq!(exit_code(&outcome), 0, "{}", outcome.stderr_text());
    assert_eq!(names(), "read parse classify compile fallback total");
    // A command line hod hands on unread has no phase before the fallback.
    let mut unread = hod();
    unread.env("HOD_TIME_PHASES", &report).env("HOD_FALLBACK_LLC", &llc.path);
    let outcome = run_hod(unread.args(["-filetype=obj", "--no-such-option"]), MODULE.as_bytes());
    assert_eq!(exit_code(&outcome), 0, "{}", outcome.stderr_text());
    assert_eq!(names(), "fallback total");
    // Without the variable nothing is written.
    let mut plain = hod();
    plain.env("HOD_FALLBACK_LLC", &llc.path);
    run_hod(plain.args(["-filetype=obj", "-o", "x.o"]), MODULE.as_bytes());
    assert!(!report.exists());
}

// spec/cli.md "Exit status": the fallback's status is hod's.
#[test]
fn fallback_exit_status_propagates() {
    let scratch = Scratch::new("cli-status");
    let llc = FakeLlc::new(&scratch, "llc");
    let log = scratch.join("cov.tsv");
    for code in [1, 2, 7, 255] {
        let outcome = run_hod(
            hod()
                .env("HOD_FALLBACK_LLC", &llc.path)
                .env("HOD_COVERAGE_LOG", &log)
                .env("FAKE_LLC_EXIT", code.to_string())
                .args(["-filetype=obj", "-o", "x.o"]),
            MODULE.as_bytes(),
        );
        assert_eq!(exit_code(&outcome), code);
        assert_eq!(outcome.stderr_text(), "err from llc\n", "hod adds nothing");
    }
    // A fallback that ran is a `fallback` line whatever its status, and
    // the status is the line's last field.
    let lines = coverage(&log);
    assert_eq!(lines.len(), 4);
    assert!(lines.iter().all(|l| l.outcome == "fallback" && l.reason == "module:asm"));
    let statuses: Vec<&str> = lines.iter().map(|l| l.fallback_status.as_str()).collect();
    assert_eq!(statuses, ["1", "2", "7", "255"]);

    // Killed by a signal: 128 + the signal, and hod says so; the log has
    // the signal, not the sum.
    let killed = scratch.write_script("killed", "cat > /dev/null\nkill -TERM $$");
    let killed_log = scratch.join("killed.tsv");
    let outcome = run_hod(
        hod()
            .env("HOD_FALLBACK_LLC", &killed)
            .env("HOD_COVERAGE_LOG", &killed_log)
            .arg("-filetype=obj"),
        b"",
    );
    assert_eq!(exit_code(&outcome), 128 + 15);
    assert!(outcome.stderr_text().contains("was terminated by signal 15"), "{outcome:?}");
    let lines = coverage(&killed_log);
    assert_eq!(lines.len(), 1);
    assert_eq!(
        (lines[0].outcome.as_str(), lines[0].fallback_status.as_str()),
        ("fallback", "signal:15")
    );

    // A fallback that does not read its input does not hang or break hod.
    let deaf = scratch.write_script("deaf", "exit 3");
    let big = vec![b'\n'; 4 << 20];
    let outcome = run_hod(hod().env("HOD_FALLBACK_LLC", &deaf).arg("-filetype=obj"), &big);
    assert_eq!(exit_code(&outcome), 3);

    // A fallback that cannot be started: no fallback ran, so no status.
    let outcome = run_hod(
        hod()
            .env("HOD_FALLBACK_LLC", scratch.join("missing"))
            .env("HOD_COVERAGE_LOG", &killed_log)
            .arg("-filetype=obj"),
        MODULE.as_bytes(),
    );
    assert_eq!(exit_code(&outcome), 1);
    assert!(outcome.stderr_text().starts_with("hod: error: cannot run fallback '"), "{outcome:?}");
    let lines = coverage(&killed_log);
    assert_eq!(
        (lines[1].outcome.as_str(), lines[1].reason.as_str(), lines[1].fallback_status.as_str()),
        ("error", "fallback-unavailable", "-")
    );
}

// spec/cli.md "Strict mode": unsupported is an error naming the construct,
// exit status 2, and the fallback does not run.
#[test]
fn strict_mode_fails_with_the_construct_named() {
    let scratch = Scratch::new("cli-strict");
    let llc = FakeLlc::new(&scratch, "llc");
    let log = scratch.join("cov.tsv");
    let expected = "hod: error: unsupported construct 'module:asm' \
                    (strict mode: no fallback)\n";
    let by_flag = run_hod(
        hod().env("HOD_FALLBACK_LLC", &llc.path).env("HOD_COVERAGE_LOG", &log).args([
            "--strict",
            "-O0",
            "-filetype=obj",
            "-o",
            "x.o",
        ]),
        MODULE.as_bytes(),
    );
    let by_env = run_hod(
        hod()
            .env("HOD_FALLBACK_LLC", &llc.path)
            .env("HOD_COVERAGE_LOG", &log)
            .env("HOD_STRICT", "1")
            .args(["-O0", "-filetype=obj", "-o", "x.o"]),
        MODULE.as_bytes(),
    );
    for outcome in [&by_flag, &by_env] {
        assert_eq!(exit_code(outcome), 2);
        assert_eq!(outcome.stderr_text(), expected);
        assert!(outcome.stdout.is_empty());
    }
    assert!(!llc.ran(), "strict mode never runs the fallback");
    let lines = coverage(&log);
    assert_eq!(lines.len(), 2);
    for line in &lines {
        assert_eq!(
            (line.outcome.as_str(), line.reason.as_str(), line.output.as_str()),
            ("error", "module:asm", "x.o")
        );
    }

    // Without a fallback at all the same module is the same failure.
    let outcome = run_hod(hod().args(["-filetype=obj"]), MODULE.as_bytes());
    assert_eq!(exit_code(&outcome), 2);
    assert_eq!(
        outcome.stderr_text(),
        "hod: error: unsupported construct 'module:asm'; no fallback is configured \
         (--fallback=<llc> or HOD_FALLBACK_LLC)\n"
    );
}

// spec/cli.md "Reasons": what hod declines, and why, in the order the
// checks run.
#[test]
fn reasons_reach_the_log() {
    // Arguments, input; then the log's reason, input kind and target.
    type Case<'a> = (&'a [&'a str], &'a [u8], &'a str, &'a str, &'a str);
    let scratch = Scratch::new("cli-reasons");
    let llc = FakeLlc::new(&scratch, "llc");
    let log = scratch.join("cov.tsv");
    let bitcode = b"BC\xC0\xDE\x35\x14\x00\x00\x05\x00\x00\x00";
    let wrapped = b"\xDE\xC0\x17\x0B\x00\x00\x00\x00\x14\x00\x00\x00";
    let cases: &[Case] = &[
        (&["-filetype=obj"], MODULE.as_bytes(), "module:asm", "text", "aarch64-unknown-linux-gnu"),
        (
            &["-filetype=obj"],
            b"target triple = \"x86_64-unknown-linux-gnu\"\ndeclare void @g()\n",
            "codegen-not-implemented",
            "text",
            "x86_64-unknown-linux-gnu",
        ),
        // Read natively: a bitcode header and nothing else.
        (&["-filetype=obj"], bitcode, "malformed-input", "bitcode", "-"),
        (&["-filetype=obj"], wrapped, "malformed-input", "bitcode", "-"),
        (&[], MODULE.as_bytes(), "assembly-output", "text", "aarch64-unknown-linux-gnu"),
        (&["-filetype=asm"], bitcode, "assembly-output", "bitcode", "-"),
        (
            &["-filetype=obj", "-mtriple=riscv64-unknown-linux-gnu"],
            MODULE.as_bytes(),
            "unsupported-target",
            "text",
            "riscv64-unknown-linux-gnu",
        ),
        (
            &["-filetype=obj", "-march=aarch64"],
            MODULE.as_bytes(),
            "unsupported-target",
            "text",
            "aarch64-unknown-linux-gnu",
        ),
        (
            &["-filetype=obj"],
            b"define void @f() {\n  ret void\n}\n",
            "unsupported-target",
            "text",
            "-",
        ),
        (&["-filetype=obj"], b"", "unsupported-target", "text", "-"),
    ];
    for (args, input, ..) in cases {
        let outcome = run_hod(
            hod().env("HOD_FALLBACK_LLC", &llc.path).env("HOD_COVERAGE_LOG", &log).args(*args),
            input,
        );
        assert_eq!(exit_code(&outcome), 0, "{args:?}: {}", outcome.stderr_text());
        assert_eq!(llc.args(), *args);
        assert!(llc.stdin() == *input, "{args:?}");
    }
    let lines = coverage(&log);
    assert_eq!(lines.len(), cases.len());
    for (line, (args, input, reason, kind, target)) in lines.iter().zip(cases) {
        assert_eq!(line.outcome, "fallback", "{args:?}");
        assert_eq!(
            (&line.reason, &line.input_kind, &line.target),
            (&(*reason).to_owned(), &(*kind).to_owned(), &(*target).to_owned()),
            "{args:?}"
        );
        assert_eq!(line.input_bytes, input.len().to_string(), "{args:?}");
        assert_eq!(line.output, "-", "{args:?}");
    }
}

// spec/cli.md "Options": an option hod does not know is an error naming it;
// with a fallback the invocation is handed over untouched, standard input
// included.
#[test]
fn unknown_options() {
    let scratch = Scratch::new("cli-unknown");
    let llc = FakeLlc::new(&scratch, "llc");
    let log = scratch.join("cov.tsv");
    let args = ["-O0", "-filetype=obj", "-frobnicate=3", "-stats", "-o", "x.o", "-"];

    let outcome = run_hod(hod().env("HOD_COVERAGE_LOG", &log).args(args), MODULE.as_bytes());
    assert_eq!(exit_code(&outcome), 1);
    assert_eq!(
        outcome.stderr_text(),
        "hod: error: unknown option '-frobnicate=3'; no fallback is configured \
         (--fallback=<llc> or HOD_FALLBACK_LLC)\n"
    );

    let outcome = run_hod(
        hod().env("HOD_COVERAGE_LOG", &log).env("HOD_FALLBACK_LLC", &llc.path).args(args),
        MODULE.as_bytes(),
    );
    assert_eq!(exit_code(&outcome), 0, "{}", outcome.stderr_text());
    assert_eq!(outcome.stderr_text(), "err from llc\n");
    assert_eq!(llc.args(), args);
    assert_eq!(llc.stdin(), MODULE.as_bytes(), "the fallback inherits standard input");

    // A value hod does not know for an option it does is the same thing.
    let outcome = run_hod(
        hod().env("HOD_COVERAGE_LOG", &log).env("HOD_FALLBACK_LLC", &llc.path).args([
            "-filetype=null",
            "-o",
            "/dev/null",
        ]),
        MODULE.as_bytes(),
    );
    assert_eq!(exit_code(&outcome), 0, "{}", outcome.stderr_text());
    assert_eq!(llc.args(), ["-filetype=null", "-o", "/dev/null"]);

    // Strict mode: an error even with a fallback.
    let outcome = run_hod(
        hod()
            .env("HOD_COVERAGE_LOG", &log)
            .env("HOD_FALLBACK_LLC", &llc.path)
            .env("HOD_STRICT", "1")
            .args(args),
        MODULE.as_bytes(),
    );
    assert_eq!(exit_code(&outcome), 1);
    assert_eq!(
        outcome.stderr_text(),
        "hod: error: unknown option '-frobnicate=3' (strict mode: no fallback)\n"
    );

    let lines = coverage(&log);
    let summary: Vec<(&str, &str, &str, &str, &str)> = lines
        .iter()
        .map(|l| {
            (
                l.outcome.as_str(),
                l.reason.as_str(),
                l.input_kind.as_str(),
                l.input_bytes.as_str(),
                l.output.as_str(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            ("error", "unknown-option", "-", "-", "x.o"),
            ("fallback", "unknown-option", "-", "-", "x.o"),
            ("fallback", "unknown-option", "-", "-", "/dev/null"),
            ("error", "unknown-option", "-", "-", "x.o"),
        ]
    );
}

// spec/cli.md "Fallback protocol": a file input is named to the fallback as
// it was to hod, and standard input is left alone.
#[test]
fn file_input() {
    let scratch = Scratch::new("cli-file");
    let llc = FakeLlc::new(&scratch, "llc");
    let log = scratch.join("cov.tsv");
    let input = scratch.write("m.ll", MODULE);
    let outcome = run_hod(
        hod()
            .env("HOD_FALLBACK_LLC", &llc.path)
            .env("HOD_COVERAGE_LOG", &log)
            .arg(&input)
            .args(["-O0", "-filetype=obj"]),
        b"not the module",
    );
    assert_eq!(exit_code(&outcome), 0, "{}", outcome.stderr_text());
    assert_eq!(llc.args(), [input.to_str().unwrap(), "-O0", "-filetype=obj"]);
    assert_eq!(llc.stdin(), b"not the module");
    let lines = coverage(&log);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].input_bytes, MODULE.len().to_string());
    // No -o: llc's rule names the output after the input.
    assert_eq!(lines[0].output, scratch.join("m.o").to_str().unwrap());

    let missing = scratch.join("missing.ll");
    let outcome =
        run_hod(hod().env("HOD_FALLBACK_LLC", &llc.path).arg(&missing).arg("-filetype=obj"), b"");
    assert_eq!(exit_code(&outcome), 1);
    assert!(outcome.stderr_text().starts_with("hod: error: cannot read '"), "{outcome:?}");
}

// spec/cli.md "Coverage log": one whole line per invocation, also when many
// compilers append at once.
#[test]
fn coverage_log_survives_concurrent_writers() {
    const WRITERS: usize = 16;
    const ROUNDS: usize = 6;
    let scratch = Scratch::new("cli-concurrent");
    let llc = scratch.write_script("llc", "cat > /dev/null");
    let log = scratch.join("cov.tsv");
    // Long lines make an interleaved write visible.
    let padding = "p".repeat(900);
    std::thread::scope(|scope| {
        for writer in 0..WRITERS {
            let (llc, log, padding) = (&llc, &log, &padding);
            scope.spawn(move || {
                for round in 0..ROUNDS {
                    let outcome = run_hod(
                        hod()
                            .env("HOD_FALLBACK_LLC", llc)
                            .env("HOD_COVERAGE_LOG", log)
                            .args(["-O0", "-filetype=obj", "-o"])
                            .arg(format!("{padding}-{writer}-{round}.o")),
                        MODULE.as_bytes(),
                    );
                    assert_eq!(exit_code(&outcome), 0, "{}", outcome.stderr_text());
                }
            });
        }
    });
    let lines = coverage(&log);
    assert_eq!(lines.len(), WRITERS * ROUNDS);
    let mut outputs: Vec<&str> = lines.iter().map(|l| l.output.as_str()).collect();
    outputs.sort_unstable();
    let mut expected: Vec<String> = (0..WRITERS)
        .flat_map(|w| (0..ROUNDS).map(move |r| (w, r)))
        .map(|(w, r)| format!("{padding}-{w}-{r}.o"))
        .collect();
    expected.sort_unstable();
    assert_eq!(outputs, expected, "every invocation has exactly one whole line");
    for line in &lines {
        assert_eq!(
            (line.outcome.as_str(), line.reason.as_str(), line.input_kind.as_str()),
            ("fallback", "module:asm", "text")
        );
        assert_eq!(line.input_bytes, MODULE.len().to_string());
    }
}

// spec/cli.md "Options": --version and --help.
#[test]
fn version_and_help() {
    let scratch = Scratch::new("cli-version");
    let log = scratch.join("cov.tsv");
    let outcome = run_hod(hod().env("HOD_COVERAGE_LOG", &log).arg("--version"), b"");
    assert_eq!(exit_code(&outcome), 0);
    assert_eq!(outcome.stdout_text(), format!("hod version {}\n", env!("CARGO_PKG_VERSION")));

    // With a fallback, its own version output follows.
    let llc = scratch.write_script("llc", "echo \"LLVM version 22.1.8 ($1)\"");
    let outcome = run_hod(
        hod().env("HOD_COVERAGE_LOG", &log).env("HOD_FALLBACK_LLC", &llc).arg("-version"),
        b"",
    );
    assert_eq!(exit_code(&outcome), 0);
    let text = outcome.stdout_text();
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines[0].starts_with("hod version "), "{text}");
    assert_eq!(lines[1], format!("fallback: {}", llc.display()));
    assert_eq!(lines[2], "LLVM version 22.1.8 (--version)");

    let outcome = run_hod(hod().env("HOD_COVERAGE_LOG", &log).arg("--help"), b"");
    assert_eq!(exit_code(&outcome), 0);
    let help = outcome.stdout_text();
    for needle in
        ["USAGE: hod", "--fallback=<llc>", "--strict", "--coverage-log=<file>", "-o <file>"]
    {
        assert!(help.contains(needle), "{needle}: {help}");
    }
    assert!(!log.exists(), "--version and --help are not compiles: no coverage line");
}

// spec/cli.md "Fallback protocol": hod as its own fallback stops at once
// instead of starting processes without end.
#[test]
fn hod_is_not_its_own_fallback() {
    let scratch = Scratch::new("cli-loop");
    let log = scratch.join("cov.tsv");
    let outcome = run_hod(
        hod()
            .env("HOD_FALLBACK_LLC", env!("CARGO_BIN_EXE_hod"))
            .env("HOD_COVERAGE_LOG", &log)
            .args(["-filetype=obj"]),
        MODULE.as_bytes(),
    );
    assert_eq!(exit_code(&outcome), 1);
    let stderr = outcome.stderr_text();
    assert!(stderr.contains("HOD_IN_FALLBACK") && stderr.contains("real llc"), "{stderr}");
    assert_eq!(stderr.lines().count(), 1, "the outer hod adds nothing: {stderr}");
    // One line from each of the two processes, the inner one first.
    let lines = coverage(&log);
    let summary: Vec<(&str, &str)> =
        lines.iter().map(|l| (l.outcome.as_str(), l.reason.as_str())).collect();
    assert_eq!(summary, [("error", "fallback-loop"), ("fallback", "module:asm")]);
    assert_eq!(lines[1].fallback_status, "1");
}

/// What the stand-in converters print for a module: text as `llvm-dis`
/// writes it, with a triple that is not the one of `MODULE`.
const CONVERTED: &str = "\
; ModuleID = '<stdin>'
source_filename = \"m.cpp\"
target triple = \"x86_64-unknown-linux-gnu\"

module asm \"nop\"

define i32 @seven() {
  ret i32 7
}
";

/// Bytes that start with the bitcode magic: `size` of them, not UTF-8,
/// more than a pipe holds when `size` says so.
fn bitcode_like(size: usize) -> Vec<u8> {
    let mut bytes = b"BC\xC0\xDE\x35\x14\x00\x00".to_vec();
    while bytes.len() < size {
        bytes.extend_from_slice(b"\x00\xFF\xFE record \n");
    }
    bytes
}

/// A stand-in for `llvm-dis`: records its arguments (one per line) in
/// `<script>.args` and those of hod's variables it was started with (the
/// four settings and `HOD_IN_FALLBACK`; the harness has `HOD_*` variables
/// of its own, which are not hod's) in `<script>.env`, then does
/// `behaviour`, which deals with standard input itself. `<script>.text`
/// holds `CONVERTED` for it to print.
struct FakeDis {
    path: PathBuf,
}

impl FakeDis {
    /// Reads all of its input into `<script>.stdin` and prints the module.
    const GOOD: &str = "cat > \"$0.stdin\"\ncat \"$0.text\"";

    fn new(scratch: &Scratch, name: &str, behaviour: &str) -> Self {
        let body = format!(
            ": > \"$0.args\"\n\
             for a in \"$@\"; do printf '%s\\n' \"$a\" >> \"$0.args\"; done\n\
             env | grep -E \
             '^HOD_(FALLBACK_LLC|STRICT|COVERAGE_LOG|BITCODE_VIA|IN_FALLBACK)=' \
             | sort > \"$0.env\"\n\
             {behaviour}"
        );
        let path = scratch.write_script(name, &body);
        scratch.write(&format!("{name}.text"), CONVERTED);
        Self { path }
    }

    fn recorded(&self, what: &str) -> PathBuf {
        let mut name = self.path.clone().into_os_string();
        name.push(what);
        PathBuf::from(name)
    }

    fn ran(&self) -> bool {
        self.recorded(".args").exists()
    }

    fn args(&self) -> Vec<String> {
        let text = std::fs::read_to_string(self.recorded(".args")).expect("the converter ran");
        text.lines().map(str::to_owned).collect()
    }

    fn env(&self) -> String {
        std::fs::read_to_string(self.recorded(".env")).expect("the converter ran")
    }

    fn stdin(&self) -> Vec<u8> {
        std::fs::read(self.recorded(".stdin")).expect("the converter read its input")
    }
}

// spec/cli.md "Bitcode conversion": with a converter, bitcode goes through
// it on pipes (more than a pipe holds, in both directions at once) and the
// invocation goes on as for text. The converter gets none of hod's
// settings. "Fallback protocol": the fallback still gets the original
// arguments and the original bitcode.
#[test]
fn bitcode_is_converted_and_goes_on_as_text() {
    let scratch = Scratch::new("cli-convert");
    let llc = FakeLlc::new(&scratch, "llc");
    // Prints the module, then every byte it reads as a comment line while
    // it is still reading: about four times the input, while hod is still
    // writing it.
    let dis = FakeDis::new(
        &scratch,
        "llvm-dis",
        "cat \"$0.text\"\ntee \"$0.stdin\" | od -An -v -tx1 | sed 's/^/;/'",
    );
    let log = scratch.join("cov.tsv");
    let out = scratch.join("a.o");
    let input = bitcode_like(1 << 20);
    let outcome = run_hod(
        hod()
            .env("HOD_FALLBACK_LLC", &llc.path)
            .env("HOD_COVERAGE_LOG", &log)
            .env("HOD_STRICT", "0")
            .env("HOD_BITCODE_VIA", &dis.path)
            .args(["-O0", "-relocation-model=pic", "-filetype=obj", "-o"])
            .arg(&out)
            .arg("-"),
        &input,
    );
    assert_eq!(exit_code(&outcome), 0, "{}", outcome.stderr_text());
    assert_eq!(outcome.stdout_text(), "out from llc\n");
    assert_eq!(outcome.stderr_text(), "err from llc\n", "hod adds nothing");

    assert_eq!(dis.args(), ["-o", "-", "-"]);
    assert!(dis.stdin() == input, "the converter reads the bytes hod read");
    assert_eq!(dis.env(), "HOD_IN_FALLBACK=1\n", "none of hod's settings reach the converter");
    assert_eq!(
        llc.args(),
        ["-O0", "-relocation-model=pic", "-filetype=obj", "-o", out.to_str().unwrap(), "-"]
    );
    assert!(llc.stdin() == input, "the fallback reads the original bitcode, not the text");

    let lines = coverage(&log);
    assert_eq!(lines.len(), 1);
    let line = &lines[0];
    assert_eq!(
        (line.outcome.as_str(), line.reason.as_str(), line.input_kind.as_str()),
        ("fallback", "module:asm", "bitcode"),
        "the text reached the same decline as a text input"
    );
    assert_eq!(line.input_bytes, input.len().to_string(), "the bitcode's size");
    assert_eq!(line.output, out.to_str().unwrap());
    assert_eq!(line.target, "x86_64-unknown-linux-gnu", "the converted module's triple");
    let micros = |field: &str| field.parse::<u64>().unwrap_or_else(|_| panic!("{line:?}"));
    assert!(micros(&line.conversion_micros) > 0, "{line:?}");
    assert!(micros(&line.hod_micros) >= micros(&line.conversion_micros), "{line:?}");
    assert_eq!(line.fallback_status, "0");

    // The module's triple is now known, so the target check applies to
    // converted bitcode as it does to text ("Reasons").
    let dis_wasm = FakeDis::new(&scratch, "llvm-dis-wasm", FakeDis::GOOD);
    let wasm = CONVERTED.replace("x86_64-unknown-linux-gnu", "wasm32-unknown-unknown");
    scratch.write("llvm-dis-wasm.text", wasm);
    let outcome = run_hod(
        hod()
            .env("HOD_FALLBACK_LLC", &llc.path)
            .env("HOD_COVERAGE_LOG", &log)
            .arg(format!("--bitcode-via={}", dis_wasm.path.display()))
            .arg("-filetype=obj"),
        &input,
    );
    assert_eq!(exit_code(&outcome), 0, "{}", outcome.stderr_text());
    assert_eq!(llc.args(), ["-filetype=obj"], "--bitcode-via is hod's own: not forwarded");
    let lines = coverage(&log);
    assert_eq!(
        (lines[1].reason.as_str(), lines[1].input_kind.as_str(), lines[1].target.as_str()),
        ("unsupported-target", "bitcode", "wasm32-unknown-unknown")
    );
    assert_ne!(lines[1].conversion_micros, "-");
}

/// A converter that fails: its name, what it does, the reason of the
/// failure, and the diagnostic after the converter's path.
type FailingConverter = (&'static str, &'static str, &'static str, String);

/// Every way a converter that starts can fail.
fn failing_converters() -> Vec<FailingConverter> {
    let not_a_module = "' wrote something that is not one whole module of LLVM text";
    vec![
        (
            "fail",
            "cat > /dev/null\necho >&2\necho 'llvm-dis: error: Invalid record' >&2\n\
             echo 'more' >&2\nexit 1",
            "bitcode-converter-failed",
            "' failed (exit status 1): llvm-dis: error: Invalid record".into(),
        ),
        (
            "fail-after-output",
            "cat > /dev/null\ncat \"$0.text\"\nexit 4",
            "bitcode-converter-failed",
            "' failed (exit status 4)".into(),
        ),
        ("fail-deaf", "exit 5", "bitcode-converter-failed", "' failed (exit status 5)".into()),
        (
            // A megabyte on standard error before reading anything: all of
            // it is read, 200 characters of its first line are quoted.
            "fail-noisy",
            "head -c 1048576 /dev/zero | tr '\\0' 'e' >&2\necho >&2\ncat > /dev/null\nexit 1",
            "bitcode-converter-failed",
            format!("' failed (exit status 1): {}...", "e".repeat(200)),
        ),
        (
            "killed",
            "kill -KILL $$",
            "bitcode-converter-killed",
            "' was terminated by signal 9".into(),
        ),
        ("nothing", "cat > /dev/null", "bitcode-converter-output", "' wrote nothing".into()),
        ("deaf", "exit 0", "bitcode-converter-output", "' wrote nothing".into()),
        (
            "garbage",
            "cat > /dev/null\necho 'not a module'",
            "bitcode-converter-output",
            not_a_module.into(),
        ),
        ("bitcode", "cat", "bitcode-converter-output", not_a_module.into()),
        (
            "cut-short",
            "cat > /dev/null\nprintf '%s' '; ModuleID = x'",
            "bitcode-converter-output",
            not_a_module.into(),
        ),
        (
            "warned",
            "cat > /dev/null\ncat \"$0.text\"\necho 'warning: ignoring invalid debug info' >&2",
            "bitcode-converter-stderr",
            "' wrote to standard error: warning: ignoring invalid debug info".into(),
        ),
    ]
}

/// Checks the log of a run of failed conversions of `input`: per reason,
/// in order, one line of a fallback and then `errors` lines of errors.
fn assert_failed_conversions(log: &Path, reasons: &[&str], errors: usize, input: &[u8]) {
    let lines = coverage(log);
    assert_eq!(lines.len(), reasons.len() * (1 + errors));
    for (index, line) in lines.iter().enumerate() {
        let reason = reasons[index / (1 + errors)];
        let outcome = if index.is_multiple_of(1 + errors) { "fallback" } else { "error" };
        assert_eq!((line.outcome.as_str(), line.reason.as_str()), (outcome, reason), "{index}");
        assert_eq!(line.input_kind, "bitcode", "{index}");
        assert_eq!(line.input_bytes, input.len().to_string(), "{index}");
        assert_eq!(line.target, "-", "{index}: a module that was not converted has no triple");
        assert_ne!(line.conversion_micros, "-", "{index}: an attempt has a time");
        assert_eq!(line.fallback_status, if outcome == "fallback" { "0" } else { "-" }, "{index}");
    }
}

// spec/cli.md "Bitcode conversion" and "Reasons": every way a conversion
// fails has its reason; in hybrid mode the fallback then runs exactly as
// for any other decline (original arguments, original bitcode, hod silent);
// without a fallback it is an error, exit status 1.
#[test]
fn conversion_failures() {
    let scratch = Scratch::new("cli-convert-fail");
    let log = scratch.join("cov.tsv");
    let strict_llc = FakeLlc::new(&scratch, "llc-strict");
    // More than a pipe holds: a converter that does not read it all must
    // not block hod, nor kill it with SIGPIPE.
    let input = bitcode_like(1 << 20);
    let cases = failing_converters();
    for (name, behaviour, _, said) in &cases {
        let dis = FakeDis::new(&scratch, name, behaviour);
        let llc = FakeLlc::new(&scratch, &format!("llc-{name}"));
        let args = ["-O0", "-filetype=obj", "-o", "x.o", "-"];
        let via = format!("--bitcode-via={}", dis.path.display());

        // Hybrid: the fallback, as for any other decline.
        let outcome = run_hod(
            hod()
                .env("HOD_COVERAGE_LOG", &log)
                .env("HOD_FALLBACK_LLC", &llc.path)
                .arg(&via)
                .args(args),
            &input,
        );
        assert_eq!(exit_code(&outcome), 0, "{name}: {}", outcome.stderr_text());
        assert_eq!(outcome.stdout_text(), "out from llc\n", "{name}");
        assert_eq!(outcome.stderr_text(), "err from llc\n", "{name}: hod adds nothing");
        assert!(dis.ran(), "{name}");
        assert_eq!(llc.args(), args, "{name}");
        assert!(llc.stdin() == input, "{name}: the fallback reads the original bitcode");

        // Strict, and no fallback at all: an error that names the converter.
        let expected = format!("hod: error: bitcode converter '{}{said}", dis.path.display());
        let strict = run_hod(
            hod()
                .env("HOD_COVERAGE_LOG", &log)
                .env("HOD_FALLBACK_LLC", &strict_llc.path)
                .env("HOD_STRICT", "1")
                .arg(&via)
                .args(args),
            &input,
        );
        assert_eq!(exit_code(&strict), 1, "{name}");
        assert_eq!(strict.stderr_text(), format!("{expected} (strict mode: no fallback)\n"));
        assert!(strict.stdout.is_empty(), "{name}");
        let alone = run_hod(hod().env("HOD_COVERAGE_LOG", &log).arg(&via).args(args), &input);
        assert_eq!(exit_code(&alone), 1, "{name}");
        assert_eq!(
            alone.stderr_text(),
            format!(
                "{expected}; no fallback is configured (--fallback=<llc> or HOD_FALLBACK_LLC)\n"
            )
        );
    }
    assert!(!strict_llc.ran(), "strict mode never runs the fallback");
    let reasons: Vec<&str> = cases.iter().map(|case| case.2).collect();
    assert_failed_conversions(&log, &reasons, 2, &input);
}

// spec/cli.md "Bitcode conversion" and "Reasons": a converter that cannot
// be started, because it is missing or is not executable.
#[test]
fn converter_that_cannot_be_started() {
    let scratch = Scratch::new("cli-convert-missing");
    let log = scratch.join("cov.tsv");
    let input = bitcode_like(1 << 20);
    let missing = scratch.join("no-such-llvm-dis");
    let plain_file = scratch.write("not-executable", "text");
    let llc = FakeLlc::new(&scratch, "llc");
    for converter in [&missing, &plain_file] {
        let via = format!("--bitcode-via={}", converter.display());
        let outcome = run_hod(
            hod()
                .env("HOD_COVERAGE_LOG", &log)
                .env("HOD_FALLBACK_LLC", &llc.path)
                .arg(&via)
                .args(["-filetype=obj"]),
            &input,
        );
        assert_eq!(exit_code(&outcome), 0, "{}", outcome.stderr_text());
        assert_eq!(outcome.stderr_text(), "err from llc\n");
        assert_eq!(llc.args(), ["-filetype=obj"]);
        assert!(llc.stdin() == input);
        let strict = run_hod(
            hod().env("HOD_COVERAGE_LOG", &log).arg(&via).args(["-filetype=obj", "--strict"]),
            &input,
        );
        assert_eq!(exit_code(&strict), 1);
        let stderr = strict.stderr_text();
        let start = format!("hod: error: cannot run bitcode converter '{}': ", converter.display());
        assert!(stderr.starts_with(&start), "{stderr}");
        assert!(stderr.ends_with(" (strict mode: no fallback)\n"), "{stderr}");
    }
    assert_failed_conversions(&log, &["bitcode-converter-unavailable"; 2], 1, &input);
}

// spec/cli.md "Bitcode conversion": a bitcode file is converted the same
// way (its bytes on the converter's standard input); "Fallback protocol":
// the fallback is given the file's name and hod's own standard input.
#[test]
fn bitcode_file_input() {
    let scratch = Scratch::new("cli-convert-file");
    let llc = FakeLlc::new(&scratch, "llc");
    let dis = FakeDis::new(&scratch, "llvm-dis", FakeDis::GOOD);
    let log = scratch.join("cov.tsv");
    let bitcode = bitcode_like(4096);
    let input = scratch.write("m.bc", &bitcode);
    let outcome = run_hod(
        hod()
            .env("HOD_FALLBACK_LLC", &llc.path)
            .env("HOD_COVERAGE_LOG", &log)
            .env("HOD_BITCODE_VIA", &dis.path)
            .arg(&input)
            .args(["-O0", "-filetype=obj"]),
        b"not the module",
    );
    assert_eq!(exit_code(&outcome), 0, "{}", outcome.stderr_text());
    assert_eq!(dis.args(), ["-o", "-", "-"]);
    assert_eq!(dis.stdin(), bitcode);
    assert_eq!(llc.args(), [input.to_str().unwrap(), "-O0", "-filetype=obj"]);
    assert_eq!(llc.stdin(), b"not the module");
    let lines = coverage(&log);
    assert_eq!(lines.len(), 1);
    let line = &lines[0];
    assert_eq!(
        (line.reason.as_str(), line.input_kind.as_str(), line.target.as_str()),
        ("module:asm", "bitcode", "x86_64-unknown-linux-gnu")
    );
    assert_eq!(line.input_bytes, bitcode.len().to_string());
    assert_eq!(line.output, scratch.join("m.o").to_str().unwrap());
    assert_ne!(line.conversion_micros, "-");
}

// spec/cli.md "hod's own options" and "Environment" for the converter, and
// "Bitcode conversion": it runs only for a module whose encoding is all
// that stands in the way.
#[test]
fn converter_settings() {
    let scratch = Scratch::new("cli-convert-settings");
    let llc = FakeLlc::new(&scratch, "llc");
    let from_env = FakeDis::new(&scratch, "dis-env", FakeDis::GOOD);
    let from_flag = FakeDis::new(&scratch, "dis-flag", FakeDis::GOOD);
    let never = FakeDis::new(&scratch, "dis-never", FakeDis::GOOD);
    let log = scratch.join("cov.tsv");
    let bitcode = bitcode_like(64);
    let base = || {
        let mut command = hod();
        command.env("HOD_FALLBACK_LLC", &llc.path).env("HOD_COVERAGE_LOG", &log);
        command
    };

    // The environment; then the command line over the environment.
    let outcome =
        run_hod(base().env("HOD_BITCODE_VIA", &from_env.path).arg("-filetype=obj"), &bitcode);
    assert_eq!(exit_code(&outcome), 0, "{}", outcome.stderr_text());
    assert!(from_env.ran());
    std::fs::remove_file(from_env.recorded(".args")).unwrap();
    let outcome = run_hod(
        base()
            .env("HOD_BITCODE_VIA", &from_env.path)
            .arg("--bitcode-via")
            .arg(&from_flag.path)
            .arg("-filetype=obj"),
        &bitcode,
    );
    assert_eq!(exit_code(&outcome), 0, "{}", outcome.stderr_text());
    assert!(from_flag.ran() && !from_env.ran());
    assert_eq!(llc.args(), ["-filetype=obj"]);

    // Set but empty is unset: bitcode is read natively (and these bytes
    // are no module). `native` is the same, also over the environment's
    // converter.
    let outcome = run_hod(base().env("HOD_BITCODE_VIA", "").arg("-filetype=obj"), &bitcode);
    assert_eq!(exit_code(&outcome), 0, "{}", outcome.stderr_text());
    let outcome = run_hod(base().env("HOD_BITCODE_VIA", "native").arg("-filetype=obj"), &bitcode);
    assert_eq!(exit_code(&outcome), 0, "{}", outcome.stderr_text());
    let outcome = run_hod(
        base()
            .env("HOD_BITCODE_VIA", &from_env.path)
            .args(["--bitcode-via=native", "-filetype=obj"]),
        &bitcode,
    );
    assert_eq!(exit_code(&outcome), 0, "{}", outcome.stderr_text());
    assert!(!from_env.ran());

    // Not run: a text module; assembly output; a rejected command line.
    let via = format!("--bitcode-via={}", never.path.display());
    for (args, input) in [
        (&["-filetype=obj"][..], MODULE.as_bytes()),
        (&["-filetype=asm"], &bitcode),
        (&["-filetype=obj", "-frobnicate"], &bitcode),
    ] {
        let outcome = run_hod(base().arg(&via).args(args), input);
        assert_eq!(exit_code(&outcome), 0, "{args:?}: {}", outcome.stderr_text());
        assert_eq!(llc.args(), args, "{args:?}");
    }
    assert!(!never.ran(), "nothing to convert, nothing run");

    // A mistake in the option is an error whatever else is configured.
    let outcome = run_hod(base().args(["-filetype=obj", "--bitcode-via="]), &bitcode);
    assert_eq!(exit_code(&outcome), 1);
    assert_eq!(
        outcome.stderr_text(),
        "hod: error: option '--bitcode-via' requires a non-empty value\n"
    );

    let lines = coverage(&log);
    let summary: Vec<(&str, &str, &str, bool)> = lines
        .iter()
        .map(|l| {
            (
                l.outcome.as_str(),
                l.reason.as_str(),
                l.input_kind.as_str(),
                l.conversion_micros != "-",
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            ("fallback", "module:asm", "bitcode", true),
            ("fallback", "module:asm", "bitcode", true),
            ("fallback", "malformed-input", "bitcode", false),
            ("fallback", "malformed-input", "bitcode", false),
            ("fallback", "malformed-input", "bitcode", false),
            ("fallback", "module:asm", "text", false),
            ("fallback", "assembly-output", "bitcode", false),
            ("fallback", "unknown-option", "-", false),
            ("error", "usage", "-", false),
        ]
    );
}

// spec/cli.md "Bitcode conversion": hod where the converter should be,
// named directly or through an `llc` entry of a shim directory (which sets
// the settings again), stops at once instead of starting converters without
// end; the invocation falls back like any failed conversion.
#[test]
fn hod_is_not_its_own_converter() {
    let scratch = Scratch::new("cli-convert-loop");
    let llc = FakeLlc::new(&scratch, "llc");
    let log = scratch.join("cov.tsv");
    let hod_path = env!("CARGO_BIN_EXE_hod");
    let entry = scratch.write_script(
        "shim-llc",
        &format!(
            "HOD_FALLBACK_LLC='{}' HOD_BITCODE_VIA=\"$0\" exec '{hod_path}' \"$@\"",
            llc.path.display()
        ),
    );
    let bitcode = bitcode_like(64);
    for converter in [Path::new(hod_path), &entry] {
        let outcome = run_hod(
            hod()
                .env("HOD_FALLBACK_LLC", &llc.path)
                .env("HOD_COVERAGE_LOG", &log)
                .env("HOD_BITCODE_VIA", converter)
                .args(["-filetype=obj"]),
            &bitcode,
        );
        assert_eq!(exit_code(&outcome), 0, "{}", outcome.stderr_text());
        assert_eq!(outcome.stderr_text(), "err from llc\n");
        assert_eq!(llc.stdin(), bitcode);

        let strict = run_hod(
            hod().env("HOD_BITCODE_VIA", converter).args(["-filetype=obj", "--strict"]),
            &bitcode,
        );
        assert_eq!(exit_code(&strict), 1);
        let stderr = strict.stderr_text();
        assert!(
            stderr.starts_with(&format!(
                "hod: error: bitcode converter '{}' failed (exit status 1): hod: error: this hod \
                 was started by another hod",
                converter.display()
            )),
            "{stderr}"
        );
        assert_eq!(stderr.lines().count(), 1, "{stderr}");
    }
    // One line per invocation that had a log: the inner hod has none of the
    // outer one's settings, so it writes no line of its own.
    let lines = coverage(&log);
    let summary: Vec<(&str, &str)> =
        lines.iter().map(|l| (l.outcome.as_str(), l.reason.as_str())).collect();
    assert_eq!(summary, [("fallback", "bitcode-converter-failed"); 2]);
}

/// `llvm-readelf -h` of `object`, asserting that it is a relocatable ELF.
fn assert_relocatable_elf(readelf: &Path, object: &Path) {
    let outcome = hod_testutil::run(readelf, [Path::new("-h"), object]);
    assert!(outcome.success(), "{}", outcome.stderr_text());
    let header = outcome.stdout_text();
    assert!(header.contains("ELF64"), "{header}");
    assert!(header.contains("REL (Relocatable file)"), "{header}");
    assert!(header.contains("AArch64"), "{header}");
}

// spec/cli.md "Fallback protocol", with the real llc: text IR on standard
// input becomes a valid object.
#[test]
fn real_llc_fallback_text() {
    let (Some(llc), Some(readelf)) = (tool("llc"), tool("llvm-readelf")) else { return };
    let scratch = Scratch::new("cli-llc-text");
    let log = scratch.join("cov.tsv");
    let out = scratch.join("seven.o");
    let outcome = run_hod(
        hod()
            .arg(format!("--fallback={}", llc.display()))
            .arg(format!("--coverage-log={}", log.display()))
            .args(["-O0", "-relocation-model=pic", "-filetype=obj", "-function-sections", "-o"])
            .arg(&out)
            .arg("-"),
        MODULE.as_bytes(),
    );
    assert_eq!(exit_code(&outcome), 0, "{}", outcome.stderr_text());
    assert_eq!(outcome.stderr_text(), "");
    assert_relocatable_elf(&readelf, &out);
    let lines = coverage(&log);
    assert_eq!(lines.len(), 1);
    assert_eq!(
        (lines[0].outcome.as_str(), lines[0].reason.as_str(), lines[0].input_kind.as_str()),
        ("fallback", "module:asm", "text")
    );

    // `-o -`: the object arrives on hod's standard output.
    let outcome = run_hod(
        hod().arg(format!("--fallback={}", llc.display())).args([
            "-O0",
            "-filetype=obj",
            "-o",
            "-",
            "-",
        ]),
        MODULE.as_bytes(),
    );
    assert_eq!(exit_code(&outcome), 0, "{}", outcome.stderr_text());
    assert!(outcome.stdout.starts_with(b"\x7FELF"));
    let piped = scratch.write("piped.o", &outcome.stdout);
    assert_relocatable_elf(&readelf, &piped);

    // llc's own diagnostics and failure status pass through.
    let outcome = run_hod(
        hod().arg(format!("--fallback={}", llc.display())).args([
            "-O0",
            "-filetype=obj",
            "-o",
            "-",
        ]),
        b"define i32 @broken( {\n",
    );
    assert_eq!(exit_code(&outcome), 1);
    assert!(outcome.stderr_text().contains("error:"), "{}", outcome.stderr_text());
    assert!(!outcome.stderr_text().contains("hod:"), "{}", outcome.stderr_text());
}

// spec/cli.md "Input" and "Fallback protocol", with the real llc: bitcode
// on standard input is recognized and becomes a valid object.
#[test]
fn real_llc_fallback_bitcode() {
    let (Some(llc), Some(llvm_as), Some(readelf)) =
        (tool("llc"), tool("llvm-as"), tool("llvm-readelf"))
    else {
        return;
    };
    let scratch = Scratch::new("cli-llc-bitcode");
    let assembled = run_with(
        Command::new(&llvm_as).args(["-o", "-", "-"]),
        MODULE.as_bytes(),
        Duration::from_mins(1),
    );
    assert!(assembled.success(), "{}", assembled.stderr_text());
    let bitcode = assembled.stdout;
    assert!(bitcode.starts_with(b"BC\xC0\xDE"), "llvm-as writes raw bitcode");

    let log = scratch.join("cov.tsv");
    let out = scratch.join("seven.o");
    let outcome = run_hod(
        hod()
            .env("HOD_FALLBACK_LLC", &llc)
            .env("HOD_COVERAGE_LOG", &log)
            .args(["-O0", "-relocation-model=pic", "-filetype=obj", "-o"])
            .arg(&out)
            .arg("-"),
        &bitcode,
    );
    assert_eq!(exit_code(&outcome), 0, "{}", outcome.stderr_text());
    assert_relocatable_elf(&readelf, &out);
    let lines = coverage(&log);
    assert_eq!(lines.len(), 1);
    assert_eq!(
        (lines[0].outcome.as_str(), lines[0].reason.as_str(), lines[0].input_kind.as_str()),
        ("fallback", "module:asm", "bitcode"),
        "read natively, declined for what it contains"
    );
    assert_eq!(lines[0].input_bytes, bitcode.len().to_string());
    assert_eq!(lines[0].target, "aarch64-unknown-linux-gnu", "the module's own, as read");
    assert_eq!(lines[0].conversion_micros, "-");
}

/// The `bin` directory of every supported LLVM that is installed with the
/// tools these tests use, newest first. Under `HOD_REQUIRE_TOOLS=1` at
/// least one must be.
fn llvm_installations() -> Vec<(u32, PathBuf)> {
    let found: Vec<(u32, PathBuf)> = LLVM_MAJORS
        .iter()
        .map(|&major| (major, PathBuf::from(format!("/usr/lib/llvm-{major}/bin"))))
        .filter(|(_, bin)| {
            ["llvm-as", "llvm-dis", "llc", "llvm-readelf"].iter().all(|t| bin.join(t).is_file())
        })
        .collect();
    if found.is_empty() {
        assert!(
            !tools_required(),
            "no LLVM installation in /usr/lib/llvm-<N> (HOD_REQUIRE_TOOLS=1)"
        );
        eprintln!("note: no LLVM installation in /usr/lib/llvm-<N>; skipping");
    }
    found
}

/// `module` as bitcode, by `bin`'s `llvm-as`.
fn assemble(bin: &Path, module: &str) -> Vec<u8> {
    let assembled = run_with(
        Command::new(bin.join("llvm-as")).args(["-o", "-", "-"]),
        module.as_bytes(),
        Duration::from_mins(1),
    );
    assert!(assembled.success(), "{}", assembled.stderr_text());
    assert!(assembled.stdout.starts_with(b"BC\xC0\xDE"), "llvm-as writes raw bitcode");
    assembled.stdout
}

// spec/cli.md "Bitcode conversion", with the real llvm-dis and llc of every
// installed LLVM: bitcode is converted by the llvm-dis of the LLVM that
// wrote it, the module's triple becomes known, hod reaches the same decline
// as for the text, and the fallback builds a valid object from the bitcode.
#[test]
fn real_llvm_dis_converts_bitcode() {
    for (major, bin) in llvm_installations() {
        let scratch = Scratch::new(&format!("cli-dis-{major}"));
        let bitcode = assemble(&bin, MODULE);
        let log = scratch.join("cov.tsv");
        let out = scratch.join("seven.o");
        let via = format!("--bitcode-via={}", bin.join("llvm-dis").display());
        let outcome = run_hod(
            hod()
                .env("HOD_FALLBACK_LLC", bin.join("llc"))
                .env("HOD_COVERAGE_LOG", &log)
                .arg(&via)
                .args(["-O0", "-relocation-model=pic", "-filetype=obj", "-o"])
                .arg(&out)
                .arg("-"),
            &bitcode,
        );
        assert_eq!(exit_code(&outcome), 0, "LLVM {major}: {}", outcome.stderr_text());
        assert_eq!(outcome.stderr_text(), "", "LLVM {major}");
        assert_relocatable_elf(&bin.join("llvm-readelf"), &out);

        // Strict: the decline is the text path's, with status 2.
        let strict = run_hod(
            hod().env("HOD_COVERAGE_LOG", &log).arg(&via).args(["-filetype=obj", "--strict"]),
            &bitcode,
        );
        assert_eq!(exit_code(&strict), 2, "LLVM {major}");
        assert_eq!(
            strict.stderr_text(),
            "hod: error: unsupported construct 'module:asm' \
             (strict mode: no fallback)\n"
        );
        // The wrapper form of bitcode is bitcode too; llvm-dis reads it.
        let mut wrapped = 0x0B17_C0DE_u32.to_le_bytes().to_vec();
        for word in [0_u32, 20, u32::try_from(bitcode.len()).unwrap(), 0] {
            wrapped.extend_from_slice(&word.to_le_bytes());
        }
        wrapped.extend_from_slice(&bitcode);
        let alone = run_hod(
            hod().env("HOD_COVERAGE_LOG", &log).arg(&via).args(["-filetype=obj"]),
            &wrapped,
        );
        assert_eq!(exit_code(&alone), 2, "LLVM {major}: {}", alone.stderr_text());

        let lines = coverage(&log);
        assert_eq!(lines.len(), 3);
        for (line, (outcome, bytes)) in lines.iter().zip([
            ("fallback", bitcode.len()),
            ("error", bitcode.len()),
            ("error", wrapped.len()),
        ]) {
            assert_eq!(
                (line.outcome.as_str(), line.reason.as_str(), line.input_kind.as_str()),
                (outcome, "module:asm", "bitcode"),
                "LLVM {major}"
            );
            assert_eq!(line.input_bytes, bytes.to_string(), "LLVM {major}");
            assert_eq!(line.target, "aarch64-unknown-linux-gnu", "LLVM {major}: {line:?}");
            assert_ne!(line.conversion_micros, "-", "LLVM {major}");
        }
        assert_eq!(lines[0].fallback_status, "0");
        assert!(lines.iter().all(|line| line.reasons == "-"), "no census asked for");

        // The native reader and the converter give the same reason and
        // the same census (field 11), for plain and wrapped bitcode.
        let census = scratch.join("census.tsv");
        for input in [&bitcode, &wrapped] {
            for route in [via.as_str(), "--bitcode-via=native"] {
                let ran = run_hod(
                    hod()
                        .env("HOD_COVERAGE_LOG", &census)
                        .env("HOD_CENSUS", "1")
                        .args([route, "-filetype=obj"]),
                    input,
                );
                assert_eq!(exit_code(&ran), 2, "LLVM {major} {route}: {}", ran.stderr_text());
            }
        }
        let lines = coverage(&census);
        assert_eq!(lines.len(), 4);
        for line in &lines {
            assert_eq!(line.reason, "module:asm", "LLVM {major}: {line:?}");
            assert_eq!(line.reasons, lines[0].reasons, "LLVM {major}: {line:?}");
            assert_eq!(line.target, "aarch64-unknown-linux-gnu", "LLVM {major}: {line:?}");
        }
        // The census begins with the refusal; what follows it moves with
        // the support matrix.
        assert_eq!(lines[0].reasons.split(',').next(), Some("module:asm"), "{lines:?}");
        assert_eq!(
            lines.iter().map(|l| l.conversion_micros != "-").collect::<Vec<_>>(),
            [true, false, true, false]
        );
    }
}

// spec/cli.md "Bitcode conversion": the converter must be the llvm-dis of
// the LLVM that wrote the bitcode. An older one fails on what it does not
// know, and the invocation falls back: the newer llc still builds the
// object. Needs both LLVM 22 and LLVM 19.
#[test]
fn real_llvm_dis_of_another_major() {
    let installations = llvm_installations();
    let [(new_major, new), .., (old_major, old)] = &installations[..] else {
        eprintln!("note: fewer than two LLVM installations; skipping the mismatch test");
        return;
    };
    // `captures` is an attribute LLVM 19 does not have.
    assert!(*new_major >= 21 && *old_major < 21, "{installations:?}");
    let module = "target triple = \"aarch64-unknown-linux-gnu\"\n\
                  define ptr @keep(ptr captures(none) %p) {\n  ret ptr null\n}\n";
    let bitcode = assemble(new, module);
    let scratch = Scratch::new("cli-dis-mismatch");
    let log = scratch.join("cov.tsv");
    let out = scratch.join("keep.o");
    let via = format!("--bitcode-via={}", old.join("llvm-dis").display());
    let outcome = run_hod(
        hod()
            .env("HOD_FALLBACK_LLC", new.join("llc"))
            .env("HOD_COVERAGE_LOG", &log)
            .arg(&via)
            .args(["-O0", "-filetype=obj", "-o"])
            .arg(&out)
            .arg("-"),
        &bitcode,
    );
    assert_eq!(exit_code(&outcome), 0, "{}", outcome.stderr_text());
    assert_eq!(outcome.stderr_text(), "");
    assert_relocatable_elf(&new.join("llvm-readelf"), &out);
    let strict = run_hod(hod().arg(&via).args(["-filetype=obj", "--strict"]), &bitcode);
    assert_eq!(exit_code(&strict), 1);
    let stderr = strict.stderr_text();
    assert!(stderr.contains("failed (exit status 1): ") && stderr.contains("llvm-dis"), "{stderr}");
    let lines = coverage(&log);
    assert_eq!(lines.len(), 1);
    assert_eq!(
        (lines[0].outcome.as_str(), lines[0].reason.as_str(), lines[0].target.as_str()),
        ("fallback", "bitcode-converter-failed", "-")
    );
    assert_eq!(lines[0].fallback_status, "0");
}
