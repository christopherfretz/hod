//! The `hod` binary end to end, against `spec/cli.md`.
//!
//! The fallback here is a script that records what it was given, so these
//! tests need no LLVM. The last two use the real `llc` (and skip without
//! it, see `hod_testutil::tool`).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hod_testutil::{Outcome, Scratch, run_with, tool};

const MODULE: &str = "\
; ModuleID = 'm.cpp'
source_filename = \"m.cpp\"
target datalayout = \"e-m:e-p270:32:32-p271:32:32-p272:64:64-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128-Fn32\"
target triple = \"aarch64-unknown-linux-gnu\"

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
}

/// Reads a coverage log, checking the shape of every line: newline
/// terminated, eight tab-separated fields, the vocabulary of each.
fn coverage(log: &Path) -> Vec<Line> {
    let text = std::fs::read_to_string(log).unwrap_or_else(|e| panic!("{}: {e}", log.display()));
    assert!(text.ends_with('\n'), "the log ends with a newline: {text:?}");
    let number = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    text.lines()
        .map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            assert_eq!(fields.len(), 8, "eight fields: {line:?}");
            assert!(fields.iter().all(|f| !f.is_empty()), "no empty field: {line:?}");
            assert!(["hod", "fallback", "error"].contains(&fields[0]), "outcome: {line:?}");
            assert!(["text", "bitcode", "-"].contains(&fields[2]), "input kind: {line:?}");
            assert!(fields[3] == "-" || number(fields[3]), "input bytes: {line:?}");
            assert!(number(fields[6]), "hod time: {line:?}");
            assert!(fields[7] == "-" || number(fields[7]), "fallback time: {line:?}");
            assert_eq!(fields[0] == "fallback", fields[7] != "-", "fallback time: {line:?}");
            assert_eq!(fields[0] == "hod", fields[1] == "-", "reason: {line:?}");
            Line {
                outcome: fields[0].to_owned(),
                reason: fields[1].to_owned(),
                input_kind: fields[2].to_owned(),
                input_bytes: fields[3].to_owned(),
                output: fields[4].to_owned(),
                target: fields[5].to_owned(),
                hod_micros: fields[6].to_owned(),
                fallback_micros: fields[7].to_owned(),
            }
        })
        .collect()
}

/// `hod` with a clean `HOD_*` environment.
fn hod() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_hod"));
    for var in ["HOD_FALLBACK_LLC", "HOD_STRICT", "HOD_COVERAGE_LOG", "HOD_IN_FALLBACK"] {
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
            .args(["-function-sections", "-mcpu=generic", "-debugger-tune=gdb"])
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
        ("fallback", "codegen-not-implemented", "text")
    );
    assert_eq!(line.input_bytes, input.len().to_string());
    assert_eq!(line.output, out.to_str().unwrap());
    assert_eq!(line.target, "aarch64-unknown-linux-gnu");
    assert!(line.hod_micros.parse::<u64>().is_ok() && line.fallback_micros.parse::<u64>().is_ok());
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
    // A fallback that ran is a `fallback` line whatever its status.
    let lines = coverage(&log);
    assert_eq!(lines.len(), 4);
    assert!(lines.iter().all(|l| l.outcome == "fallback" && l.reason == "codegen-not-implemented"));

    // Killed by a signal: 128 + the signal, and hod says so.
    let killed = scratch.write_script("killed", "cat > /dev/null\nkill -TERM $$");
    let outcome = run_hod(hod().env("HOD_FALLBACK_LLC", &killed).arg("-filetype=obj"), b"");
    assert_eq!(exit_code(&outcome), 128 + 15);
    assert!(outcome.stderr_text().contains("was terminated by signal 15"), "{outcome:?}");

    // A fallback that does not read its input does not hang or break hod.
    let deaf = scratch.write_script("deaf", "exit 3");
    let big = vec![b'\n'; 4 << 20];
    let outcome = run_hod(hod().env("HOD_FALLBACK_LLC", &deaf).arg("-filetype=obj"), &big);
    assert_eq!(exit_code(&outcome), 3);

    // A fallback that cannot be started.
    let outcome = run_hod(
        hod().env("HOD_FALLBACK_LLC", scratch.join("missing")).arg("-filetype=obj"),
        MODULE.as_bytes(),
    );
    assert_eq!(exit_code(&outcome), 1);
    assert!(outcome.stderr_text().starts_with("hod: error: cannot run fallback '"), "{outcome:?}");
}

// spec/cli.md "Strict mode": unsupported is an error naming the construct,
// exit status 2, and the fallback does not run.
#[test]
fn strict_mode_fails_with_the_construct_named() {
    let scratch = Scratch::new("cli-strict");
    let llc = FakeLlc::new(&scratch, "llc");
    let log = scratch.join("cov.tsv");
    let expected = "hod: error: unsupported construct 'codegen-not-implemented' \
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
            ("error", "codegen-not-implemented", "x.o")
        );
    }

    // Without a fallback at all the same module is the same failure.
    let outcome = run_hod(hod().args(["-filetype=obj"]), MODULE.as_bytes());
    assert_eq!(exit_code(&outcome), 2);
    assert_eq!(
        outcome.stderr_text(),
        "hod: error: unsupported construct 'codegen-not-implemented'; no fallback is configured \
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
        (
            &["-filetype=obj"],
            MODULE.as_bytes(),
            "codegen-not-implemented",
            "text",
            "aarch64-unknown-linux-gnu",
        ),
        (&["-filetype=obj"], bitcode, "bitcode-input", "bitcode", "-"),
        (&["-filetype=obj"], wrapped, "bitcode-input", "bitcode", "-"),
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
            "codegen-not-implemented",
            "text",
            "-",
        ),
        (&["-filetype=obj"], b"", "codegen-not-implemented", "text", "-"),
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
            ("fallback", "codegen-not-implemented", "text")
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
    // One line from each of the two processes, the inner one first.
    let lines = coverage(&log);
    let summary: Vec<(&str, &str)> =
        lines.iter().map(|l| (l.outcome.as_str(), l.reason.as_str())).collect();
    assert_eq!(summary, [("error", "fallback-loop"), ("fallback", "codegen-not-implemented")]);
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
        ("fallback", "codegen-not-implemented", "text")
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
        ("fallback", "bitcode-input", "bitcode")
    );
    assert_eq!(lines[0].input_bytes, bitcode.len().to_string());
}
