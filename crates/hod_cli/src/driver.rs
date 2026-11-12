//! What one invocation does. Contract: `spec/cli.md` "Behaviour",
//! "Bitcode conversion", "Fallback protocol" and "Exit status".
//!
//! [`run`] takes the code generator and the standard streams as parameters,
//! so the paths hod cannot reach yet (a successful compile, a malformed
//! module) are tested with a stand-in generator.

use std::ffi::{OsStr, OsString};
use std::fmt::Write as _;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use hod::{CompileError, Construct, InputKind, Options, OutputKind};

use crate::args::{self, Action, CommandLine, Input};
use crate::convert;
use crate::coverage::{FallbackStatus, Log, Outcome, Record};

/// Exit status: success.
pub const EXIT_OK: u8 = 0;
/// Exit status: an error (usage, I/O, malformed input, the fallback could
/// not be run).
pub const EXIT_ERROR: u8 = 1;
/// Exit status: the module is unsupported and no fallback ran.
pub const EXIT_UNSUPPORTED: u8 = 2;

/// Environment variable: the real `llc` (`--fallback=`).
pub const ENV_FALLBACK: &str = "HOD_FALLBACK_LLC";
/// Environment variable: `1` is `--strict`.
pub const ENV_STRICT: &str = "HOD_STRICT";
/// Environment variable: the coverage log (`--coverage-log=`).
pub const ENV_COVERAGE_LOG: &str = "HOD_COVERAGE_LOG";
/// Environment variable: the `llvm-dis` for bitcode (`--bitcode-via=`).
pub const ENV_BITCODE_VIA: &str = "HOD_BITCODE_VIA";
/// Environment variable hod sets for the processes it starts, the fallback
/// and the bitcode converter. A hod that starts with it set stands where
/// one of them should be, and stops instead of recursing.
pub const ENV_IN_FALLBACK: &str = "HOD_IN_FALLBACK";
/// The variables that are settings. The bitcode converter is started
/// without them.
pub const SETTINGS: [&str; 4] = [ENV_FALLBACK, ENV_STRICT, ENV_COVERAGE_LOG, ENV_BITCODE_VIA];

/// hod's settings from the environment.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Environment {
    /// `HOD_FALLBACK_LLC`, if set and not empty.
    pub fallback: Option<PathBuf>,
    /// `HOD_STRICT=1`.
    pub strict: bool,
    /// `HOD_COVERAGE_LOG`, if set and not empty.
    pub coverage_log: Option<PathBuf>,
    /// `HOD_BITCODE_VIA`, if set and not empty.
    pub bitcode_via: Option<PathBuf>,
    /// `HOD_IN_FALLBACK` is set: this process was started by another hod,
    /// as its fallback or its bitcode converter.
    pub in_fallback: bool,
    /// A variable with a value hod does not accept.
    pub error: Option<String>,
}

impl Environment {
    /// Reads the settings through `var` (`std::env::var_os` in the binary).
    pub fn from_vars(var: impl Fn(&str) -> Option<OsString>) -> Self {
        let non_empty = |name: &str| var(name).filter(|value| !value.is_empty());
        let mut env = Environment {
            fallback: non_empty(ENV_FALLBACK).map(PathBuf::from),
            coverage_log: non_empty(ENV_COVERAGE_LOG).map(PathBuf::from),
            bitcode_via: non_empty(ENV_BITCODE_VIA).map(PathBuf::from),
            in_fallback: non_empty(ENV_IN_FALLBACK).is_some(),
            ..Environment::default()
        };
        match non_empty(ENV_STRICT) {
            None => {}
            Some(value) if value == "1" => env.strict = true,
            Some(value) if value == "0" => {}
            Some(value) => {
                env.error = Some(format!(
                    "invalid value '{}' for {ENV_STRICT}: expected 1 or 0",
                    value.to_string_lossy()
                ));
            }
        }
        env
    }
}

/// The code generator: `hod::compile` in the binary.
pub type Compile<'a> = &'a dyn Fn(&[u8], &Options) -> Result<Vec<u8>, CompileError>;

/// The process's standard streams.
pub struct Io<'a> {
    pub stdin: &'a mut dyn Read,
    pub stdout: &'a mut dyn Write,
    pub stderr: &'a mut dyn Write,
}

impl std::fmt::Debug for Io<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Io").finish_non_exhaustive()
    }
}

/// The effective settings: the command line over the environment.
struct Settings {
    fallback: Option<PathBuf>,
    strict: bool,
    bitcode_via: Option<PathBuf>,
}

impl Settings {
    /// The fallback to run when hod declines, unless strict.
    fn hybrid(&self) -> Option<&Path> {
        if self.strict { None } else { self.fallback.as_deref() }
    }

    /// Why no fallback runs, for the end of a diagnostic.
    fn no_fallback_note(&self) -> &'static str {
        if self.strict {
            " (strict mode: no fallback)"
        } else {
            "; no fallback is configured (--fallback=<llc> or HOD_FALLBACK_LLC)"
        }
    }
}

/// How an invocation ended, before the coverage line is written.
struct Finished {
    exit: u8,
    record: Record,
}

/// One compile invocation in progress.
struct Job<'a, 'io> {
    line: &'a CommandLine,
    settings: Settings,
    io: &'a mut Io<'io>,
    start: Instant,
    record: Record,
}

impl Job<'_, '_> {
    fn diagnostic(&mut self, message: &str) {
        // A closed standard error is not something to report anywhere.
        let _ = writeln!(self.io.stderr, "hod: error: {message}");
    }

    /// Ends the invocation in hod, with a diagnostic.
    fn fail(mut self, exit: u8, reason: &'static str, message: &str) -> Finished {
        self.diagnostic(message);
        self.record.outcome = Outcome::Error;
        self.record.reason = Some(reason);
        self.record.hod_time = self.start.elapsed();
        Finished { exit, record: self.record }
    }

    /// Hands the invocation to the real `llc`: the original arguments
    /// without hod's own, and `input` on its standard input when hod has
    /// consumed it (otherwise the fallback inherits standard input).
    fn fall_back(mut self, llc: &Path, reason: &'static str, input: Option<&[u8]>) -> Finished {
        self.record.reason = Some(reason);
        self.record.hod_time = self.start.elapsed();
        let started = Instant::now();
        match run_fallback(llc, &self.line.forwarded, input) {
            Ok(status) => {
                self.record.outcome = Outcome::Fallback;
                self.record.fallback_time = Some(started.elapsed());
                let (exit, ended, how) = fallback_end(status);
                self.record.fallback_status = Some(ended);
                if let Some(how) = how {
                    self.diagnostic(&format!("fallback '{}' {how}", llc.display()));
                }
                Finished { exit, record: self.record }
            }
            Err(e) => {
                self.diagnostic(&format!("cannot run fallback '{}': {e}", llc.display()));
                self.record.outcome = Outcome::Error;
                self.record.reason = Some("fallback-unavailable");
                Finished { exit: EXIT_ERROR, record: self.record }
            }
        }
    }

    fn run(mut self, env: &Environment, compile: Compile<'_>) -> Finished {
        let line = self.line;
        if let Some(message) = line.fatal.as_ref().or(env.error.as_ref()) {
            return self.fail(EXIT_ERROR, "usage", message);
        }
        if env.in_fallback {
            let message = format!(
                "this hod was started by another hod, as its fallback or its bitcode converter \
                 ({ENV_IN_FALLBACK} is set); those must be the real llc and the real llvm-dis"
            );
            return self.fail(EXIT_ERROR, "fallback-loop", &message);
        }
        if let Some(rejected) = &line.rejected {
            // hod cannot tell what the rest of such a command line means, so
            // it does not touch the input either.
            if let Some(llc) = self.settings.hybrid().map(Path::to_path_buf) {
                return self.fall_back(&llc, rejected.reason(), None);
            }
            let message = format!("{rejected}{}", self.settings.no_fallback_note());
            return self.fail(EXIT_ERROR, rejected.reason(), &message);
        }

        let input = match read_input(&line.input, self.io.stdin) {
            Ok(input) => input,
            Err(message) => return self.fail(EXIT_ERROR, "input-unreadable", &message),
        };
        self.record.input_kind = Some(InputKind::detect(&input));
        self.record.input_bytes = Some(input.len());
        self.record.target = hod::effective_triple(&input, &line.options).map(str::to_owned);
        let output = output_name(line);
        self.record.output = output.to_string_lossy().into_owned();

        // The fallback reads a file input itself; standard input was
        // consumed, so it gets the same bytes on a pipe: the module as it
        // arrived, also when hod went on with a conversion of it.
        let piped = matches!(line.input, Input::Stdin).then_some(input.as_slice());

        let mut compiled = compile(&input, &line.options);
        if let Err(CompileError::Unsupported(unsupported)) = &compiled
            && unsupported.construct == Construct::BitcodeInput
            && let Some(llvm_dis) = self.settings.bitcode_via.clone()
        {
            // Everything else about the invocation is acceptable and only
            // the encoding is in the way: go on with the module as text.
            let started = Instant::now();
            let converted = convert::run(&llvm_dis, &input);
            self.record.conversion_time = Some(started.elapsed());
            match converted {
                Ok(text) => {
                    self.record.target =
                        hod::effective_triple(&text, &line.options).map(str::to_owned);
                    compiled = compile(&text, &line.options);
                }
                Err(failure) => {
                    if let Some(llc) = self.settings.hybrid().map(Path::to_path_buf) {
                        return self.fall_back(&llc, failure.reason(), piped);
                    }
                    let message = format!(
                        "{}{}",
                        failure.message(&llvm_dis),
                        self.settings.no_fallback_note()
                    );
                    return self.fail(EXIT_ERROR, failure.reason(), &message);
                }
            }
        }

        let error = match compiled {
            Ok(object) => {
                return match write_output(&output, &object, self.io.stdout) {
                    Ok(()) => {
                        self.record.outcome = Outcome::Hod;
                        self.record.hod_time = self.start.elapsed();
                        Finished { exit: EXIT_OK, record: self.record }
                    }
                    Err(message) => self.fail(EXIT_ERROR, "output-unwritable", &message),
                };
            }
            Err(error) => error,
        };
        let (reason, exit) = match &error {
            CompileError::Unsupported(unsupported) => {
                (unsupported.construct.name(), EXIT_UNSUPPORTED)
            }
            CompileError::MalformedInput(_) => ("malformed-input", EXIT_ERROR),
            CompileError::InvalidOptions(_) => ("invalid-options", EXIT_ERROR),
        };
        if let Some(llc) = self.settings.hybrid().map(Path::to_path_buf) {
            return self.fall_back(&llc, reason, piped);
        }
        let message = format!("{error}{}", self.settings.no_fallback_note());
        self.fail(exit, reason, &message)
    }
}

/// Runs one invocation of `hod` with the arguments after the program name;
/// returns the exit status.
pub fn run(args: &[OsString], env: &Environment, compile: Compile<'_>, io: &mut Io<'_>) -> u8 {
    let start = Instant::now();
    let line = args::parse(args);
    let fallback = line.fallback.clone().or_else(|| env.fallback.clone());
    match line.action {
        Action::Compile => {}
        Action::Version => {
            let fallback = fallback.filter(|_| !env.in_fallback);
            return print(io, &version_text(fallback.as_deref()));
        }
        Action::Help => return print(io, args::HELP),
    }
    let log_path = line.coverage_log.clone().or_else(|| env.coverage_log.clone());
    let mut log = match log_path.as_deref().map(Log::open).transpose() {
        Ok(log) => log,
        Err(e) => {
            let path = log_path.unwrap_or_default();
            let _ = writeln!(
                io.stderr,
                "hod: error: cannot open coverage log '{}': {e}",
                path.display()
            );
            return EXIT_ERROR;
        }
    };
    let job = Job {
        line: &line,
        settings: Settings {
            fallback,
            strict: line.strict.unwrap_or(env.strict),
            bitcode_via: line.bitcode_via.clone().or_else(|| env.bitcode_via.clone()),
        },
        io,
        start,
        record: Record {
            outcome: Outcome::Error,
            reason: None,
            input_kind: None,
            input_bytes: None,
            output: line.output.as_deref().map_or("-".into(), |o| o.to_string_lossy().into_owned()),
            target: line.options.triple.clone(),
            hod_time: Duration::ZERO,
            fallback_time: None,
            conversion_time: None,
            fallback_status: None,
        },
    };
    let Finished { exit, record } = job.run(env, compile);
    if let Some(log) = &mut log
        && let Err(e) = log.append(&record)
    {
        let path = log_path.unwrap_or_default();
        let _ =
            writeln!(io.stderr, "hod: error: cannot write coverage log '{}': {e}", path.display());
        return if exit == EXIT_OK { EXIT_ERROR } else { exit };
    }
    exit
}

fn print(io: &mut Io<'_>, text: &str) -> u8 {
    match io.stdout.write_all(text.as_bytes()).and_then(|()| io.stdout.flush()) {
        Ok(()) => EXIT_OK,
        Err(_) => EXIT_ERROR,
    }
}

/// `--version`: the first line is hod's; with a fallback, its path and its
/// own `--version` output follow, so that a script looking for the LLVM
/// version in `llc --version` still finds it.
fn version_text(fallback: Option<&Path>) -> String {
    let mut text = format!("hod version {}\n", env!("CARGO_PKG_VERSION"));
    if let Some(llc) = fallback {
        let _ = writeln!(text, "fallback: {}", llc.display());
        let output = Command::new(llc)
            .arg("--version")
            .env(ENV_IN_FALLBACK, "1")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output();
        if let Ok(output) = output {
            text.push_str(&String::from_utf8_lossy(&output.stdout));
            if !text.ends_with('\n') {
                text.push('\n');
            }
        }
    }
    text
}

fn read_input(input: &Input, stdin: &mut dyn Read) -> Result<Vec<u8>, String> {
    match input {
        Input::Stdin => {
            let mut bytes = Vec::new();
            match stdin.read_to_end(&mut bytes) {
                Ok(_) => Ok(bytes),
                Err(e) => Err(format!("cannot read standard input: {e}")),
            }
        }
        Input::Path(path) => {
            std::fs::read(path).map_err(|e| format!("cannot read '{}': {e}", path.display()))
        }
    }
}

/// The output's name, by `llc`'s rule: `-o` if given; else standard output
/// (`-`) for standard input; else the input's name with a final `.ll` or
/// `.bc` removed and `.o` (object) or `.s` (assembly) added.
fn output_name(line: &CommandLine) -> OsString {
    if let Some(output) = &line.output {
        return output.clone();
    }
    let Input::Path(path) = &line.input else { return OsString::from("-") };
    let extension = path.extension();
    let stem = if extension == Some(OsStr::new("ll")) || extension == Some(OsStr::new("bc")) {
        path.with_extension("")
    } else {
        path.clone()
    };
    let mut name = stem.into_os_string();
    name.push(match line.options.output_kind {
        OutputKind::Object => ".o",
        OutputKind::Assembly => ".s",
    });
    name
}

fn write_output(name: &OsStr, bytes: &[u8], stdout: &mut dyn Write) -> Result<(), String> {
    if name == "-" {
        stdout
            .write_all(bytes)
            .and_then(|()| stdout.flush())
            .map_err(|e| format!("cannot write standard output: {e}"))
    } else {
        std::fs::write(name, bytes)
            .map_err(|e| format!("cannot write '{}': {e}", Path::new(name).display()))
    }
}

/// Runs the fallback to completion. Its standard output and error are this
/// process's own; `input`, if any, is written to its standard input.
fn run_fallback(
    llc: &Path,
    args: &[OsString],
    input: Option<&[u8]>,
) -> std::io::Result<ExitStatus> {
    let mut child = Command::new(llc)
        .args(args)
        .env(ENV_IN_FALLBACK, "1")
        .stdin(if input.is_some() { Stdio::piped() } else { Stdio::inherit() })
        .spawn()?;
    if let (Some(mut pipe), Some(bytes)) = (child.stdin.take(), input) {
        // A fallback that exits without reading everything closes the pipe;
        // its exit status says what went wrong.
        let _ = pipe.write_all(bytes);
    }
    child.wait()
}

/// How the fallback ended: the exit status hod passes on, what the
/// coverage log records, and a note when hod has something to add. The
/// status is the fallback's exit code or, for a fallback killed by a
/// signal, 128 plus the signal number.
fn fallback_end(status: ExitStatus) -> (u8, FallbackStatus, Option<String>) {
    if let Some(code) = status.code() {
        let exit = u8::try_from(code & 0xFF).unwrap_or(EXIT_ERROR);
        return (exit, FallbackStatus::Exited(code), None);
    }
    #[cfg(unix)]
    if let Some(signal) = std::os::unix::process::ExitStatusExt::signal(&status) {
        let exit = u8::try_from(128 + signal).unwrap_or(u8::MAX);
        let how = format!("was terminated by signal {signal}");
        return (exit, FallbackStatus::Signaled(signal), Some(how));
    }
    (EXIT_ERROR, FallbackStatus::Unknown, Some("ended without an exit status".to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hod::{Construct, Unsupported};
    use hod_testutil::Scratch;

    const MODULE: &[u8] = b"target triple = \"aarch64-unknown-linux-gnu\"\n\
        define i32 @f() {\n  ret i32 7\n}\n";
    const OBJECT: &[u8] = b"\x7FELF-stand-in\n\0\xFF";

    /// A stand-in for `hod::compile` that succeeds.
    #[expect(clippy::unnecessary_wraps, reason = "a stand-in has the signature of the real one")]
    fn object(_: &[u8], _: &Options) -> Result<Vec<u8>, CompileError> {
        Ok(OBJECT.to_vec())
    }

    struct Ran {
        exit: u8,
        stdout: Vec<u8>,
        stderr: String,
    }

    fn run_with(args: &[&str], env: &Environment, compile: Compile<'_>, stdin: &[u8]) -> Ran {
        let args: Vec<OsString> = args.iter().map(OsString::from).collect();
        let (mut input, mut stdout, mut stderr) = (stdin, Vec::new(), Vec::new());
        let mut io = Io { stdin: &mut input, stdout: &mut stdout, stderr: &mut stderr };
        let exit = run(&args, env, compile, &mut io);
        Ran { exit, stdout, stderr: String::from_utf8(stderr).unwrap() }
    }

    fn fields(log: &Path) -> Vec<Vec<String>> {
        let text = std::fs::read_to_string(log).unwrap();
        assert!(text.is_empty() || text.ends_with('\n'), "{text:?}");
        text.lines().map(|l| l.split('\t').map(str::to_owned).collect()).collect()
    }

    // spec/cli.md "Environment".
    #[test]
    fn environment() {
        let vars = |pairs: &'static [(&'static str, &'static str)]| {
            Environment::from_vars(move |name| {
                pairs.iter().find(|(k, _)| *k == name).map(|(_, v)| OsString::from(v))
            })
        };
        assert_eq!(vars(&[]), Environment::default());
        let all = vars(&[
            ("HOD_FALLBACK_LLC", "/usr/lib/llvm-22/bin/llc"),
            ("HOD_STRICT", "1"),
            ("HOD_COVERAGE_LOG", "cov.tsv"),
            ("HOD_BITCODE_VIA", "/usr/lib/llvm-22/bin/llvm-dis"),
            ("HOD_IN_FALLBACK", "1"),
        ]);
        assert_eq!(all.fallback, Some("/usr/lib/llvm-22/bin/llc".into()));
        assert_eq!(all.coverage_log, Some("cov.tsv".into()));
        assert_eq!(all.bitcode_via, Some("/usr/lib/llvm-22/bin/llvm-dis".into()));
        assert!(all.strict && all.in_fallback && all.error.is_none());
        // Empty is unset; HOD_STRICT=0 is off.
        let empty = vars(&[
            ("HOD_FALLBACK_LLC", ""),
            ("HOD_STRICT", ""),
            ("HOD_COVERAGE_LOG", ""),
            ("HOD_BITCODE_VIA", ""),
            ("HOD_IN_FALLBACK", ""),
        ]);
        assert_eq!(empty, Environment::default());
        assert_eq!(vars(&[("HOD_STRICT", "0")]), Environment::default());
        let bad = vars(&[("HOD_STRICT", "yes")]);
        assert!(!bad.strict);
        assert!(bad.error.unwrap().contains("invalid value 'yes' for HOD_STRICT"));
    }

    // spec/cli.md "Behaviour": a successful compile writes the object to
    // `-o`, or to standard output, or to the name derived from the input.
    #[test]
    fn success_writes_the_object() {
        let scratch = Scratch::new("driver-ok");
        let env = Environment::default();
        let out = scratch.join("a.o");
        let log = scratch.join("cov.tsv");
        let ran = run_with(
            &[
                "-O0",
                "-filetype=obj",
                "-o",
                out.to_str().unwrap(),
                &format!("--coverage-log={}", log.display()),
                "-",
            ],
            &env,
            &object,
            MODULE,
        );
        assert_eq!((ran.exit, ran.stderr.as_str()), (EXIT_OK, ""));
        assert!(ran.stdout.is_empty());
        assert_eq!(std::fs::read(&out).unwrap(), OBJECT);
        let lines = fields(&log);
        assert_eq!(lines.len(), 1);
        let line = &lines[0];
        assert_eq!(line.len(), crate::coverage::FIELDS);
        assert_eq!(
            line[..6],
            [
                "hod",
                "-",
                "text",
                &MODULE.len().to_string(),
                out.to_str().unwrap(),
                "aarch64-unknown-linux-gnu"
            ]
        );
        assert!(line[6].parse::<u64>().is_ok(), "{line:?}");
        assert_eq!(line[7..], ["-", "-", "-"], "no fallback, no conversion: {line:?}");

        // Standard output: `-o -`, and no `-o` with standard input.
        for args in [&["-filetype=obj", "-o", "-"][..], &["-filetype=obj"]] {
            let ran = run_with(args, &env, &object, MODULE);
            assert_eq!((ran.exit, ran.stderr.as_str()), (EXIT_OK, ""), "{args:?}");
            assert_eq!(ran.stdout, OBJECT, "{args:?}");
        }

        // A file input: read from the file, and named after it without -o.
        let input = scratch.write("m.ll", MODULE);
        let seen = std::cell::RefCell::new(Vec::new());
        let record = |input: &[u8], _: &Options| {
            seen.borrow_mut().extend_from_slice(input);
            Ok(OBJECT.to_vec())
        };
        let ran = run_with(&["-filetype=obj", input.to_str().unwrap()], &env, &record, b"unused");
        assert_eq!((ran.exit, ran.stderr.as_str()), (EXIT_OK, ""));
        assert_eq!(*seen.borrow(), MODULE);
        assert!(ran.stdout.is_empty());
        assert_eq!(std::fs::read(scratch.join("m.o")).unwrap(), OBJECT);
    }

    #[test]
    fn output_names_follow_llc() {
        let name = |args: &[&str]| {
            let args: Vec<OsString> = args.iter().map(OsString::from).collect();
            output_name(&args::parse(&args)).into_string().unwrap()
        };
        assert_eq!(name(&[]), "-");
        assert_eq!(name(&["-"]), "-");
        assert_eq!(name(&["-o", "x.o", "dir/m.ll"]), "x.o");
        assert_eq!(name(&["dir/m.ll"]), "dir/m.s");
        assert_eq!(name(&["dir/m.ll", "-filetype=obj"]), "dir/m.o");
        assert_eq!(name(&["m.bc", "-filetype=obj"]), "m.o");
        assert_eq!(name(&["m.x.ll", "-filetype=obj"]), "m.x.o");
        assert_eq!(name(&["m.txt", "-filetype=obj"]), "m.txt.o");
        assert_eq!(name(&["m", "-filetype=asm"]), "m.s");
    }

    // spec/cli.md "Behaviour": the options reach the code generator.
    #[test]
    fn options_reach_the_code_generator() {
        let seen = std::cell::RefCell::new(None);
        let record = |_: &[u8], options: &Options| {
            *seen.borrow_mut() = Some(options.clone());
            Ok(Vec::new())
        };
        let ran = run_with(
            &["-O0", "-relocation-model=pic", "-filetype=obj", "-function-sections", "-o", "-"],
            &Environment::default(),
            &record,
            MODULE,
        );
        assert_eq!(ran.exit, EXIT_OK);
        let options = seen.borrow().clone().unwrap();
        assert_eq!(options.opt_level, hod::OptLevel::O0);
        assert_eq!(options.relocation_model, Some(hod::RelocModel::Pic));
        assert_eq!(options.output_kind, OutputKind::Object);
        assert!(options.function_sections && !options.data_sections);
    }

    /// A stand-in for a hod that compiles text and has no bitcode reader;
    /// it keeps what it was given.
    fn text_only(
        seen: &std::cell::RefCell<Vec<Vec<u8>>>,
    ) -> impl Fn(&[u8], &Options) -> Result<Vec<u8>, CompileError> + '_ {
        |input: &[u8], _: &Options| {
            seen.borrow_mut().push(input.to_vec());
            match InputKind::detect(input) {
                InputKind::Bitcode => Err(Unsupported::module(Construct::BitcodeInput).into()),
                InputKind::Text => Ok(OBJECT.to_vec()),
            }
        }
    }

    const BITCODE: &[u8] = b"BC\xC0\xDE\x35\x14\x00\x00\x05\x00\x00\x00";
    /// What the stand-in converters print: a module as `llvm-dis` writes it.
    const CONVERTED: &str = "; ModuleID = '<stdin>'\n\
        target triple = \"x86_64-unknown-linux-gnu\"\n\ndefine i32 @f() {\n  ret i32 7\n}\n";

    // spec/cli.md "Bitcode conversion" and "Behaviour" step 8: with a
    // converter, a bitcode module is compiled from its text, and the log
    // line says so: the kind and size are the bitcode's, the target is the
    // text's, field 9 is the conversion's time.
    #[test]
    fn converted_bitcode_is_compiled() {
        let scratch = Scratch::new("driver-convert");
        // The converter is a child of this test: it holds a jobserver slot.
        let _slot = hod_testutil::jobs::global().acquire();
        scratch.write("llvm-dis.text", CONVERTED);
        let converter = scratch.write_script(
            "llvm-dis",
            "cat > \"$0.stdin\"\nprintf '%s\\n' \"$@\" > \"$0.args\"\ncat \"$0.text\"",
        );
        let log = scratch.join("cov.tsv");
        let out = scratch.join("a.o");
        let env = Environment {
            coverage_log: Some(log.clone()),
            bitcode_via: Some(converter.clone()),
            ..Environment::default()
        };
        let seen = std::cell::RefCell::new(Vec::new());
        let ran = run_with(
            &["-O0", "-filetype=obj", "-o", out.to_str().unwrap()],
            &env,
            &text_only(&seen),
            BITCODE,
        );
        assert_eq!((ran.exit, ran.stderr.as_str()), (EXIT_OK, ""));
        assert_eq!(std::fs::read(&out).unwrap(), OBJECT);
        assert_eq!(*seen.borrow(), [BITCODE.to_vec(), CONVERTED.as_bytes().to_vec()]);
        assert_eq!(std::fs::read(scratch.join("llvm-dis.stdin")).unwrap(), BITCODE);
        assert_eq!(std::fs::read_to_string(scratch.join("llvm-dis.args")).unwrap(), "-o\n-\n-\n");
        let lines = fields(&log);
        assert_eq!(lines.len(), 1);
        let line = &lines[0];
        assert_eq!(line.len(), crate::coverage::FIELDS);
        assert_eq!(
            line[..6],
            [
                "hod",
                "-",
                "bitcode",
                &BITCODE.len().to_string(),
                out.to_str().unwrap(),
                "x86_64-unknown-linux-gnu"
            ]
        );
        let micros = |field: &str| field.parse::<u64>().unwrap_or_else(|_| panic!("{line:?}"));
        assert!(micros(&line[6]) >= micros(&line[8]), "hod's time includes the conversion");
        assert_eq!((line[7].as_str(), line[9].as_str()), ("-", "-"));

        // The command line's converter wins over the environment's, and
        // -mtriple over the converted module's own triple.
        let unused = Environment { bitcode_via: Some(scratch.join("missing")), ..env.clone() };
        let ran = run_with(
            &[
                "-filetype=obj",
                "-mtriple=aarch64-unknown-linux-gnu",
                &format!("--bitcode-via={}", converter.display()),
                "-o",
                "-",
            ],
            &unused,
            &text_only(&seen),
            BITCODE,
        );
        assert_eq!((ran.exit, ran.stderr.as_str()), (EXIT_OK, ""));
        assert_eq!(ran.stdout, OBJECT);
        assert_eq!(fields(&log)[1][5], "aarch64-unknown-linux-gnu");

        // What the text reader says of converted text is the reason.
        let malformed = |input: &[u8], _: &Options| match InputKind::detect(input) {
            InputKind::Bitcode => Err(Unsupported::module(Construct::BitcodeInput).into()),
            InputKind::Text => Err(CompileError::MalformedInput("line 2: expected type".into())),
        };
        let ran = run_with(&["-filetype=obj"], &env, &malformed, BITCODE);
        assert_eq!(ran.exit, EXIT_ERROR);
        assert!(ran.stderr.starts_with("hod: error: malformed input: line 2"), "{}", ran.stderr);
        let line = &fields(&log)[2];
        assert_eq!(line[..3], ["error", "malformed-input", "bitcode"]);
        assert!(line[8].parse::<u64>().is_ok(), "{line:?}");
    }

    // spec/cli.md "Bitcode conversion": the converter runs only when the
    // encoding is all that stands in the way. Here it does not exist, so
    // running it would be an error.
    #[test]
    fn conversion_only_when_bitcode_is_the_reason() {
        let scratch = Scratch::new("driver-no-convert");
        let log = scratch.join("cov.tsv");
        let env = Environment {
            coverage_log: Some(log.clone()),
            bitcode_via: Some(scratch.join("no-llvm-dis")),
            ..Environment::default()
        };
        // Text; assembly output of bitcode; a rejected command line.
        let ran = run_with(&["-filetype=obj"], &env, &hod::compile, MODULE);
        assert_eq!(ran.exit, EXIT_UNSUPPORTED);
        let ran = run_with(&["-filetype=asm"], &env, &hod::compile, BITCODE);
        assert_eq!(ran.exit, EXIT_UNSUPPORTED);
        let ran = run_with(&["-filetype=obj", "-bogus"], &env, &hod::compile, BITCODE);
        assert_eq!(ran.exit, EXIT_ERROR);
        // A generator that reads bitcode itself never asks for a conversion.
        let ran = run_with(&["-filetype=obj", "-o", "-"], &env, &object, BITCODE);
        assert_eq!(ran.exit, EXIT_OK);
        let lines = fields(&log);
        let summary: Vec<(&str, &str, &str, &str)> = lines
            .iter()
            .map(|l| (l[0].as_str(), l[1].as_str(), l[2].as_str(), l[8].as_str()))
            .collect();
        assert_eq!(
            summary,
            [
                ("error", "codegen-not-implemented", "text", "-"),
                ("error", "assembly-output", "bitcode", "-"),
                ("error", "unknown-option", "-", "-"),
                ("hod", "-", "bitcode", "-"),
            ]
        );

        // Bitcode is the reason: the converter is tried, and its absence is
        // the error (status 1: hod failed, not "unsupported").
        let ran = run_with(&["-filetype=obj", "--strict"], &env, &hod::compile, BITCODE);
        assert_eq!(ran.exit, EXIT_ERROR);
        assert!(
            ran.stderr.starts_with("hod: error: cannot run bitcode converter '")
                && ran.stderr.ends_with(" (strict mode: no fallback)\n"),
            "{}",
            ran.stderr
        );
        let line = &fields(&log)[4];
        assert_eq!(line[..4], ["error", "bitcode-converter-unavailable", "bitcode", "12"]);
        assert!(line[8].parse::<u64>().is_ok(), "an attempt has a time: {line:?}");
        // Without a converter, bitcode is declined as before.
        let plain = Environment { bitcode_via: None, ..env };
        let ran = run_with(&["-filetype=obj"], &plain, &hod::compile, BITCODE);
        assert_eq!(ran.exit, EXIT_UNSUPPORTED);
        let line = &fields(&log)[5];
        assert_eq!((line[1].as_str(), line[8].as_str()), ("bitcode-input", "-"));
    }

    // spec/cli.md "Exit status": unsupported without a fallback is 2 and
    // names the construct (and the function, when there is one).
    #[test]
    fn unsupported_without_fallback() {
        let scratch = Scratch::new("driver-unsupported");
        let log = scratch.join("cov.tsv");
        let env = Environment { coverage_log: Some(log.clone()), ..Environment::default() };
        let ran = run_with(&["-filetype=obj", "-o", "-"], &env, &hod::compile, MODULE);
        assert_eq!(ran.exit, EXIT_UNSUPPORTED);
        assert!(ran.stdout.is_empty());
        assert_eq!(
            ran.stderr,
            "hod: error: unsupported construct 'codegen-not-implemented'; no fallback is \
             configured (--fallback=<llc> or HOD_FALLBACK_LLC)\n"
        );

        let in_function = |_: &[u8], _: &Options| {
            Err(Unsupported {
                construct: Construct::CodegenNotImplemented,
                function: Some("_Z1fv".into()),
            }
            .into())
        };
        let ran = run_with(&["-filetype=obj", "--strict"], &env, &in_function, MODULE);
        assert_eq!(ran.exit, EXIT_UNSUPPORTED);
        assert_eq!(
            ran.stderr,
            "hod: error: unsupported construct 'codegen-not-implemented' in function '_Z1fv' \
             (strict mode: no fallback)\n"
        );

        let lines = fields(&log);
        assert_eq!(lines.len(), 2);
        for line in &lines {
            assert_eq!(line[..3], ["error", "codegen-not-implemented", "text"]);
            assert_eq!(line[7], "-");
        }
    }

    // spec/cli.md "Exit status": the other compile errors are 1.
    #[test]
    fn malformed_input_and_invalid_options() {
        let scratch = Scratch::new("driver-malformed");
        let log = scratch.join("cov.tsv");
        let env = Environment { coverage_log: Some(log.clone()), ..Environment::default() };
        let malformed = |_: &[u8], _: &Options| {
            Err(CompileError::MalformedInput("line 2: expected '}'".into()))
        };
        let ran = run_with(&["-filetype=obj"], &env, &malformed, b"define");
        assert_eq!(ran.exit, EXIT_ERROR);
        assert!(ran.stderr.starts_with("hod: error: malformed input: line 2: expected '}'"));
        let invalid =
            |_: &[u8], _: &Options| Err(CompileError::InvalidOptions("a without b".into()));
        let ran = run_with(&["-filetype=obj"], &env, &invalid, b"");
        assert_eq!(ran.exit, EXIT_ERROR);
        assert!(ran.stderr.starts_with("hod: error: invalid options: a without b"));
        let reasons: Vec<String> = fields(&log).into_iter().map(|l| l[1].clone()).collect();
        assert_eq!(reasons, ["malformed-input", "invalid-options"]);
    }

    // spec/cli.md "Options": without a fallback an unknown option is an
    // error naming it, and the input is not read.
    #[test]
    fn unknown_option_without_fallback() {
        let scratch = Scratch::new("driver-unknown");
        let log = scratch.join("cov.tsv");
        let env = Environment { coverage_log: Some(log.clone()), ..Environment::default() };
        let never = |_: &[u8], _: &Options| -> Result<Vec<u8>, CompileError> {
            panic!("the code generator must not run")
        };
        let ran = run_with(&["-filetype=obj", "-bogus", "-o", "a.o"], &env, &never, MODULE);
        assert_eq!(ran.exit, EXIT_ERROR);
        assert!(ran.stderr.starts_with("hod: error: unknown option '-bogus'"), "{}", ran.stderr);
        assert_eq!(ran.stderr.lines().count(), 1);
        // Strict mode ignores a configured fallback.
        let strict =
            Environment { fallback: Some("/nonexistent/llc".into()), strict: true, ..env.clone() };
        let ran = run_with(&["-filetype=null"], &strict, &never, MODULE);
        assert_eq!(ran.exit, EXIT_ERROR);
        assert_eq!(
            ran.stderr,
            "hod: error: unknown value 'null' for option '-filetype' (strict mode: no fallback)\n"
        );
        let ran = run_with(&["a.ll", "b.ll"], &env, &never, MODULE);
        assert_eq!(ran.exit, EXIT_ERROR);
        let lines = fields(&log);
        let summary: Vec<(&str, &str, &str, &str, &str)> = lines
            .iter()
            .map(|l| (l[0].as_str(), l[1].as_str(), l[2].as_str(), l[3].as_str(), l[4].as_str()))
            .collect();
        assert_eq!(
            summary,
            [
                ("error", "unknown-option", "-", "-", "a.o"),
                ("error", "unknown-option", "-", "-", "-"),
                ("error", "usage", "-", "-", "-"),
            ]
        );
    }

    // spec/cli.md "hod's own options" and "Environment": mistakes in them
    // are errors even when a fallback is configured.
    #[test]
    fn mistakes_in_hod_settings() {
        let scratch = Scratch::new("driver-usage");
        let log = scratch.join("cov.tsv");
        let env = Environment {
            fallback: Some("/nonexistent/llc".into()),
            coverage_log: Some(log.clone()),
            ..Environment::default()
        };
        let ran = run_with(&["-filetype=obj", "--strict=maybe"], &env, &object, MODULE);
        assert_eq!(ran.exit, EXIT_ERROR);
        assert!(ran.stderr.contains("invalid value 'maybe' for '--strict'"), "{}", ran.stderr);
        let bad_env =
            Environment { error: Some("invalid value 'yes' for HOD_STRICT".into()), ..env };
        let ran = run_with(&["-filetype=obj"], &bad_env, &object, MODULE);
        assert_eq!(ran.exit, EXIT_ERROR);
        assert_eq!(ran.stderr, "hod: error: invalid value 'yes' for HOD_STRICT\n");
        let reasons: Vec<String> = fields(&log).into_iter().map(|l| l[1].clone()).collect();
        assert_eq!(reasons, ["usage", "usage"]);
    }

    // spec/cli.md "Fallback protocol": a hod started as a fallback stops.
    #[test]
    fn fallback_loop_is_refused() {
        let env = Environment {
            fallback: Some("/nonexistent/llc".into()),
            in_fallback: true,
            ..Environment::default()
        };
        let ran = run_with(&["-filetype=obj"], &env, &hod::compile, MODULE);
        assert_eq!(ran.exit, EXIT_ERROR);
        assert!(ran.stderr.contains("HOD_IN_FALLBACK"), "{}", ran.stderr);
        assert!(ran.stderr.contains("fallback or its bitcode converter"), "{}", ran.stderr);
    }

    // spec/cli.md "Exit status": I/O failures are errors with one line.
    #[test]
    fn io_errors() {
        let scratch = Scratch::new("driver-io");
        let log = scratch.join("cov.tsv");
        let env = Environment { coverage_log: Some(log.clone()), ..Environment::default() };
        let missing = scratch.join("missing.ll");
        let ran = run_with(&["-filetype=obj", missing.to_str().unwrap()], &env, &object, b"");
        assert_eq!(ran.exit, EXIT_ERROR);
        assert!(ran.stderr.starts_with("hod: error: cannot read '"), "{}", ran.stderr);
        let unwritable = scratch.join("no-such-dir/a.o");
        let ran =
            run_with(&["-filetype=obj", "-o", unwritable.to_str().unwrap()], &env, &object, MODULE);
        assert_eq!(ran.exit, EXIT_ERROR);
        assert!(ran.stderr.starts_with("hod: error: cannot write '"), "{}", ran.stderr);
        let unrunnable = Environment { fallback: Some(scratch.join("no-llc")), ..env.clone() };
        let ran = run_with(&["-filetype=obj"], &unrunnable, &hod::compile, MODULE);
        assert_eq!(ran.exit, EXIT_ERROR);
        assert!(ran.stderr.starts_with("hod: error: cannot run fallback '"), "{}", ran.stderr);
        let reasons: Vec<String> = fields(&log).into_iter().map(|l| l[1].clone()).collect();
        assert_eq!(reasons, ["input-unreadable", "output-unwritable", "fallback-unavailable"]);

        // A coverage log that cannot be opened stops the invocation before
        // anything is compiled or written.
        let bad_log = Environment {
            coverage_log: Some(scratch.join("no-such-dir/cov.tsv")),
            ..Environment::default()
        };
        let out = scratch.join("b.o");
        let ran =
            run_with(&["-filetype=obj", "-o", out.to_str().unwrap()], &bad_log, &object, MODULE);
        assert_eq!(ran.exit, EXIT_ERROR);
        assert!(ran.stderr.starts_with("hod: error: cannot open coverage log '"), "{}", ran.stderr);
        assert!(!out.exists());
    }

    // spec/cli.md "Options": --version and --help write no coverage line.
    #[test]
    fn version_and_help() {
        let scratch = Scratch::new("driver-version");
        let log = scratch.join("cov.tsv");
        let env = Environment { coverage_log: Some(log.clone()), ..Environment::default() };
        let ran = run_with(&["--version"], &env, &object, b"");
        assert_eq!(ran.exit, EXIT_OK);
        let text = String::from_utf8(ran.stdout).unwrap();
        assert_eq!(text, format!("hod version {}\n", env!("CARGO_PKG_VERSION")));
        let ran = run_with(&["-O0", "--help"], &env, &object, b"");
        assert_eq!(ran.exit, EXIT_OK);
        assert!(String::from_utf8(ran.stdout).unwrap().starts_with("OVERVIEW: hod"));
        assert!(!log.exists());
    }
}
