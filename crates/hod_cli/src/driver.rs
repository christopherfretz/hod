//! What one invocation does. Contract: `spec/cli.md` "Behaviour",
//! "Bitcode conversion", "Fallback protocol" and "Exit status".
//!
//! [`run`] takes the code generator and the standard streams as parameters,
//! so the paths hod cannot reach yet (a successful compile, a malformed
//! module) are tested with a stand-in generator.

use std::borrow::Cow;
use std::ffi::{OsStr, OsString};
use std::fmt::Write as _;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use hod::args::{self, Action, CommandLine, Input};
use hod::coverage::{FallbackStatus, Log, Outcome, Record};
use hod::env::{BITCODE_NATIVE, ENV_IN_FALLBACK, Environment};
use hod::{BitcodeReading, CompileError, Construct, InputKind, Options, OutputKind, Report};

use crate::convert;
use crate::phases::Phases;

/// Exit status: success.
pub const EXIT_OK: u8 = 0;
/// Exit status: an error (usage, I/O, malformed input, the fallback could
/// not be run).
pub const EXIT_ERROR: u8 = 1;
/// Exit status: the module is unsupported and no fallback ran.
pub const EXIT_UNSUPPORTED: u8 = 2;

/// The code generator: `hod::compile_releasing` in the binary. The input
/// is owned where nothing after the compile needs it, so that the
/// compile may free it once read (`spec/cli.md` "The facade").
pub type Compile<'a> = &'a dyn Fn(
    &mut Cow<'_, [u8]>,
    &Options,
    &hod::Settings,
    &mut Report,
) -> Result<Vec<u8>, CompileError>;

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
    census: bool,
    threads: hod::Threads,
}

impl Settings {
    /// The `llvm-dis` a bitcode input goes through, if it is not read
    /// natively (`spec/cli.md` "Bitcode conversion"): native is the
    /// default and `--bitcode-via=native`.
    fn converter(&self) -> Option<&Path> {
        self.bitcode_via.as_deref().filter(|via| via.as_os_str() != BITCODE_NATIVE)
    }

    /// What `hod::compile_with` is asked to do.
    fn compile(&self) -> hod::Settings {
        hod::Settings {
            bitcode: if self.converter().is_some() {
                BitcodeReading::Decline
            } else {
                BitcodeReading::Native
            },
            census: self.census,
            support: None,
            threads: self.threads.clone(),
        }
    }

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
    phases: &'a mut Phases,
}

/// The message of a hod started as another hod's fallback or converter.
fn fallback_loop() -> String {
    format!(
        "this hod was started by another hod, as its fallback or its bitcode converter \
         ({ENV_IN_FALLBACK} is set); those must be the real llc and the real llvm-dis"
    )
}

impl Job<'_, '_> {
    fn diagnostic(&mut self, message: &str) {
        // A closed standard error is not something to report anywhere.
        let _ = writeln!(self.io.stderr, "hod: error: {message}");
    }

    /// Ends the invocation in hod, with a diagnostic.
    fn fail(mut self, exit: u8, reason: impl Into<Cow<'static, str>>, message: &str) -> Finished {
        self.diagnostic(message);
        self.record.outcome = Outcome::Error;
        self.record.reason = Some(reason.into());
        self.record.hod_time = self.start.elapsed();
        Finished { exit, record: self.record }
    }

    /// Hands the invocation to the real `llc`: the original arguments
    /// without hod's own, and `input` on its standard input when hod has
    /// consumed it (otherwise the fallback inherits standard input).
    fn fall_back(
        mut self,
        llc: &Path,
        reason: impl Into<Cow<'static, str>>,
        input: Option<&[u8]>,
    ) -> Finished {
        self.record.reason = Some(reason.into());
        self.record.hod_time = self.start.elapsed();
        let started = Instant::now();
        match run_fallback(llc, &self.line.forwarded, input) {
            Ok(status) => {
                self.phases.end("fallback");
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
                self.record.reason = Some("fallback-unavailable".into());
                Finished { exit: EXIT_ERROR, record: self.record }
            }
        }
    }

    /// The status of a rejected command line that does not fall back.
    /// WP-021: in strict mode an option hod does not know is a decline
    /// (`llc` may know it), status 2; a usage mistake is an error.
    fn rejected_exit(&self, reason: &str) -> u8 {
        if self.settings.strict && reason == "unknown-option" {
            EXIT_UNSUPPORTED
        } else {
            EXIT_ERROR
        }
    }

    fn run(mut self, env: &Environment, compile: Compile<'_>) -> Finished {
        let line = self.line;
        if let Some(message) = line.fatal.as_ref().or(env.error.as_ref()) {
            return self.fail(EXIT_ERROR, "usage", message);
        }
        if env.in_fallback {
            return self.fail(EXIT_ERROR, "fallback-loop", &fallback_loop());
        }
        if let Some(rejected) = &line.rejected {
            // hod cannot tell what the rest of such a command line means, so
            // it does not touch the input either.
            if let Some(llc) = self.settings.hybrid().map(Path::to_path_buf) {
                return self.fall_back(&llc, rejected.reason(), None);
            }
            let message = format!("{rejected}{}", self.settings.no_fallback_note());
            let exit = self.rejected_exit(rejected.reason());
            return self.fail(exit, rejected.reason(), &message);
        }

        let input = match read_input(&line.input, self.io.stdin) {
            Ok(input) => input,
            Err(message) => return self.fail(EXIT_ERROR, "input-unreadable", &message),
        };
        self.phases.end("read");
        self.record.input_kind = Some(InputKind::detect(&input));
        self.record.input_bytes = Some(input.len());
        self.record.target = hod::effective_triple(&input, &line.options).map(str::to_owned);
        let output = output_name(line);
        self.record.output = output.to_string_lossy().into_owned();

        // The fallback reads a file input itself; standard input was
        // consumed, so it gets the same bytes on a pipe: the module as it
        // arrived, also when hod went on with a conversion of it. Those
        // bytes are kept; any other input is the compile's to free once
        // it is read (WP-166): a conversion needs it only when the compile
        // declined the bitcode, before reading.
        let keep = matches!(line.input, Input::Stdin) && self.settings.hybrid().is_some();
        let (kept, owned) = if keep { (input, None) } else { (Vec::new(), Some(input)) };
        let piped = keep.then_some(kept.as_slice());
        let mut input = owned.map_or(Cow::Borrowed(kept.as_slice()), Cow::Owned);

        let settings = self.settings.compile();
        let mut report = Report::default();
        let mut compiled = compile(&mut input, &line.options, &settings, &mut report);
        if let Err(CompileError::Unsupported(unsupported)) = &compiled
            && unsupported.construct == Construct::BitcodeInput
            && let Some(llvm_dis) = self.settings.converter().map(Path::to_path_buf)
        {
            // Everything else about the invocation is acceptable and only
            // the encoding is in the way: go on with the module as text.
            let started = Instant::now();
            let converted = convert::run(&llvm_dis, &input);
            // The bitcode is not needed again (the fallback's copy is
            // `piped`).
            drop(input);
            self.record.conversion_time = Some(started.elapsed());
            // The attempt that declined the bitcode is in this phase too.
            self.phases.end("convert");
            match converted {
                Ok(text) => {
                    self.record.target =
                        hod::effective_triple(&text, &line.options).map(str::to_owned);
                    report = Report::default();
                    compiled =
                        compile(&mut Cow::Owned(text), &line.options, &settings, &mut report);
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
        for &(name, at) in &report.phases {
            self.phases.end_at(name, at);
        }
        self.phases.end("compile");
        if report.target.is_some() {
            self.record.target = report.target;
        }
        if settings.census {
            self.record.reasons = Some(report.reasons);
        }

        let error = match compiled {
            Ok(object) => {
                return match write_output(&output, &object, self.io.stdout) {
                    Ok(()) => {
                        self.phases.end("write");
                        self.record.outcome = Outcome::Hod;
                        self.record.hod_time = self.start.elapsed();
                        Finished { exit: EXIT_OK, record: self.record }
                    }
                    Err(message) => self.fail(EXIT_ERROR, "output-unwritable", &message),
                };
            }
            Err(error) => error,
        };
        let (reason, exit): (Cow<'static, str>, u8) = match &error {
            CompileError::Unsupported(unsupported) => {
                (unsupported.construct.name().to_owned().into(), EXIT_UNSUPPORTED)
            }
            CompileError::MalformedInput(_) => ("malformed-input".into(), EXIT_ERROR),
            CompileError::InvalidOptions(_) => ("invalid-options".into(), EXIT_ERROR),
            CompileError::Internal(_) => (hod::INTERNAL_ERROR.into(), EXIT_ERROR),
            CompileError::Panic(_) => (hod::INTERNAL_PANIC.into(), EXIT_ERROR),
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
    let mut line = args::parse(args);
    line.options.debug_compression =
        line.compress_debug_sections.or(env.compress_debug_sections).unwrap_or_default();
    let fallback = line.fallback.clone().or_else(|| env.fallback.clone());
    match line.action {
        Action::Compile => {}
        Action::Version => {
            let fallback = fallback.filter(|_| !env.in_fallback);
            return print(io, &version_text(fallback.as_deref()));
        }
        Action::Help => return print(io, HELP),
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
    let mut phases = Phases::new(start);
    let job = Job {
        line: &line,
        settings: Settings {
            fallback,
            strict: line.strict.unwrap_or(env.strict),
            bitcode_via: line.bitcode_via.clone().or_else(|| env.bitcode_via.clone()),
            census: env.census,
            threads: env.threads(line.threads),
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
            reasons: None,
        },
        phases: &mut phases,
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
    if let Some(path) = &env.time_phases
        && let Err(e) = phases.append(path)
    {
        let _ =
            writeln!(io.stderr, "hod: error: cannot write phase report '{}': {e}", path.display());
        return if exit == EXIT_OK { EXIT_ERROR } else { exit };
    }
    exit
}

/// `--help`.
pub const HELP: &str = "\
OVERVIEW: hod, a debug-build code generator with llc's command line

USAGE: hod [options] <input LLVM IR file, or - for standard input (the default)>

OPTIONS (one or two dashes; a value is joined with '=' or is the next argument):
  -o <file>                       Output file ('-' is standard output)
  -O<0|1|2|3>                     Optimization level (accepted; hod does not optimize)
  --filetype=<asm|obj>            Output kind (hod writes objects only)
  --mtriple=<triple>              Override the module's target triple
  --march=<arch>                  Architecture override
  --mcpu=<cpu>                    Target CPU
  --mattr=<+a,-b,...>             Target features
  --relocation-model=<static|pic|dynamic-no-pic|ropi|rwpi|ropi-rwpi>
  --code-model=<tiny|small|kernel|medium|large>
  --function-sections             One section per function
  --data-sections                 One section per global
  --fp-contract=<fast|on|off>     Floating-point contraction
  --debugger-tune=<gdb|lldb|dbx|sce>
  --split-dwarf-file=<name>       The .dwo name recorded in the object
  --split-dwarf-output=<file>     Where the .dwo file is written
  --x86-branches-within-32B-boundaries
  --time-trace, --time-trace-granularity=<n>, --time-trace-file=<file>
                                  Accepted; hod itself writes no trace
  --version                       Print the version
  --help                          Print this help

hod's own options (each also settable in the environment):
  --fallback=<llc>                HOD_FALLBACK_LLC   The real llc, run on the same input with the
                                                     same arguments when hod does not support
                                                     something in the module or on the command line
  --strict                        HOD_STRICT=1       No fallback: unsupported is an error
  --coverage-log=<file>           HOD_COVERAGE_LOG   Append one line per invocation to <file>
  --bitcode-via=<llvm-dis>        HOD_BITCODE_VIA    Convert a bitcode input to text with this
                                                     llvm-dis (of the LLVM that wrote the bitcode)
                                                     and compile the text
  --compress-debug-sections=<none|zlib>
                                  HOD_COMPRESS_DEBUG_SECTIONS
                                                     Write the .debug_* sections compressed
                                                     (SHF_COMPRESSED, zlib); zstd is not supported
  --threads=<n>                   HOD_THREADS        Use at most <n> threads for code generation
                                                     (without either: a make jobserver's idle
                                                     tokens, else one thread)

EXIT STATUS: 0 on success; 1 on an error; 2 when the module is unsupported and no fallback
ran; otherwise the fallback's own status. The contract is spec/cli.md.
";

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

    /// `base` with `set` applied: [`Environment`] is `#[non_exhaustive]`,
    /// so a test builds one from a value, not from a literal
    /// (`spec/cli.md` "Embedding").
    fn with(base: &Environment, set: impl FnOnce(&mut Environment)) -> Environment {
        let mut env = base.clone();
        set(&mut env);
        env
    }

    /// A module declined in the module pass (`module:code-model-tiny`) with a body
    /// the body pass refuses (`cc:ghccc`): constructs no code-generation
    /// package turns on, so the reasons do not move with the matrix.
    const MODULE: &[u8] = b"target triple = \"aarch64-unknown-linux-gnu\"\n\
        !llvm.module.flags = !{!900}\n!900 = !{i32 1, !\"Code Model\", i32 0}\ndefine ghccc void @f() nounwind {\n  ret void\n}\n";
    /// A module hod compiles: declarations only.
    const DECLARATIONS: &[u8] = b"target triple = \"aarch64-unknown-linux-gnu\"\n\
        declare i32 @g()\n";
    /// A module code generation declines after its bodies are read: a
    /// target feature hod does not honour (`spec/codegen.md` §5.5.2), a
    /// refusal no package lifts.
    const CG_DECLINED: &[u8] = b"target triple = \"aarch64-unknown-linux-gnu\"\n\
        define void @f() \"target-features\"=\"+strict-align\" {\n  ret void\n}\n";
    const OBJECT: &[u8] = b"\x7FELF-stand-in\n\0\xFF";

    /// A stand-in for `hod::compile` that succeeds.
    #[expect(clippy::unnecessary_wraps, reason = "a stand-in has the signature of the real one")]
    fn object(
        _: &mut Cow<'_, [u8]>,
        _: &Options,
        _: &hod::Settings,
        _: &mut Report,
    ) -> Result<Vec<u8>, CompileError> {
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

    // spec/cli.md "Threads": `--threads=<n>` wins over `HOD_THREADS`,
    // and either wins over a jobserver (no token is taken then).
    #[test]
    fn threads_option_environment_and_jobserver() {
        let seen = std::cell::Cell::new(None);
        let probe =
            |_: &mut Cow<'_, [u8]>, _: &Options, settings: &hod::Settings, _: &mut Report| {
                seen.set(Some((settings.threads.max, settings.threads.tokens.is_some())));
                Ok(OBJECT.to_vec())
            };
        let make = Some("-j8 --jobserver-auth=fifo:/nonexistent/fifo".to_owned());
        let threads = |args: &[&str], explicit: Option<usize>, makeflags: &Option<String>| {
            let env = with(&Environment::default(), |e| {
                e.threads = explicit;
                e.makeflags = makeflags.clone();
            });
            seen.set(None);
            let ran = run_with(args, &env, &probe, DECLARATIONS);
            assert_eq!(ran.exit, EXIT_OK, "{args:?}: {}", ran.stderr);
            seen.get().expect("compiled")
        };
        let obj = ["-filetype=obj"];
        // `max` 0 is the calling thread alone (`hod::Threads::default`).
        assert_eq!(threads(&obj, None, &None), (0, false));
        assert_eq!(threads(&obj, Some(5), &None), (5, false));
        assert_eq!(threads(&["-filetype=obj", "--threads=3"], Some(5), &None), (3, false));
        assert_eq!(threads(&["--threads", "2", "-filetype=obj"], None, &None), (2, false));
        assert_eq!(threads(&obj, None, &make), (hod::jobserver::JOBSERVER_MAX_THREADS, true));
        assert_eq!(threads(&obj, Some(5), &make), (5, false));
        assert_eq!(threads(&["-filetype=obj", "--threads=1"], None, &make), (1, false));

        // A mistake in it is an error, never a fallback; an invalid
        // HOD_THREADS is one even when the option is given.
        let llc = with(&Environment::default(), |e| {
            e.fallback = Some("/nonexistent/llc".into());
        });
        let ran = run_with(&["-filetype=obj", "--threads=0"], &llc, &probe, DECLARATIONS);
        assert_eq!(ran.exit, EXIT_ERROR);
        assert!(ran.stderr.contains("invalid value '0' for '--threads'"), "{}", ran.stderr);
        let bad = with(&llc, |e| {
            e.error = Some("invalid value 'x' for HOD_THREADS".into());
        });
        let ran = run_with(&["-filetype=obj", "--threads=2"], &bad, &probe, DECLARATIONS);
        assert_eq!(ran.exit, EXIT_ERROR);
        assert!(ran.stderr.contains("HOD_THREADS"), "{}", ran.stderr);
    }

    // spec/cli.md "Coverage log" field 11 and "Reasons" (first refusal and
    // census): with HOD_CENSUS=1 the line lists every reason; the reason
    // itself, field 2, is the same.
    #[test]
    fn census_of_every_reason() {
        let scratch = Scratch::new("driver-census");
        let log = scratch.join("cov.tsv");
        let env = with(&Environment::default(), |e| {
            e.coverage_log = Some(log.clone());
        });
        let census = with(&env, |e| {
            e.census = true;
        });
        for env in [&env, &census] {
            let ran = run_with(&["-filetype=obj"], env, &hod::compile_releasing, MODULE);
            assert_eq!(ran.exit, EXIT_UNSUPPORTED);
            let ran = run_with(&["-filetype=obj"], env, &hod::compile_releasing, DECLARATIONS);
            assert_eq!(ran.exit, EXIT_OK);
            assert!(ran.stdout.starts_with(b"\x7FELF"));
            let ran = run_with(&["-filetype=asm"], env, &hod::compile_releasing, MODULE);
            assert_eq!(ran.exit, EXIT_UNSUPPORTED);
        }
        let lines = fields(&log);
        let summary: Vec<(&str, &str)> =
            lines.iter().map(|l| (l[1].as_str(), l[10].as_str())).collect();
        assert_eq!(
            summary,
            [
                ("module:code-model-tiny", "-"),
                ("-", "-"),
                ("assembly-output", "-"),
                ("module:code-model-tiny", "module:code-model-tiny,cc:ghccc"),
                ("-", "-"),
                ("assembly-output", "assembly-output"),
            ]
        );
    }

    // spec/cli.md "Phase report": with `HOD_TIME_PHASES`, each compile
    // invocation appends its phases, in order, and `total`.
    #[test]
    fn phase_report() {
        let scratch = Scratch::new("driver-phases");
        let file = scratch.join("phases.tsv");
        let env = Environment::from_vars(|name| {
            (name == "HOD_TIME_PHASES").then(|| file.clone().into_os_string())
        });
        assert_eq!(
            env,
            with(&Environment::default(), |e| {
                e.time_phases = Some(file.clone());
            })
        );
        let phases = || {
            let text = std::fs::read_to_string(&file).unwrap();
            let _ = std::fs::remove_file(&file);
            assert!(text.ends_with('\n'), "{text:?}");
            let names: Vec<String> = text
                .lines()
                .map(|line| {
                    let (name, micros) = line.split_once('\t').expect("a tab");
                    assert!(micros.parse::<u64>().is_ok(), "{line:?}");
                    name.to_owned()
                })
                .collect();
            names.join(" ")
        };

        let ran = run_with(&["-filetype=obj", "-o", "-"], &env, &object, MODULE);
        assert_eq!((ran.exit, ran.stderr.as_str()), (EXIT_OK, ""));
        assert_eq!(phases(), "read compile write total");
        // A decline is the phases up to it.
        let ran = run_with(&["-filetype=obj", "--strict"], &env, &hod::compile_releasing, MODULE);
        assert_eq!(ran.exit, EXIT_UNSUPPORTED);
        assert_eq!(phases(), "read parse classify compile total");
        // A module hod builds reads its bodies and generates code.
        let ran =
            run_with(&["-filetype=obj", "--strict"], &env, &hod::compile_releasing, DECLARATIONS);
        assert_eq!(ran.exit, EXIT_OK);
        assert_eq!(phases(), "read parse classify bodies emission object compile write total");
        // One declined by the code generator reads its bodies too.
        let ran =
            run_with(&["-filetype=obj", "--strict"], &env, &hod::compile_releasing, CG_DECLINED);
        assert_eq!(ran.exit, EXIT_UNSUPPORTED);
        assert_eq!(phases(), "read parse classify bodies compile total");
        // A module declined before it is read has none of them.
        let ran = run_with(&["-filetype=asm", "--strict"], &env, &hod::compile_releasing, MODULE);
        assert_eq!(ran.exit, EXIT_UNSUPPORTED);
        assert_eq!(phases(), "read compile total");
        // Reports are appended; an invocation that compiles nothing has none.
        run_with(&["-filetype=obj", "-o", "-"], &env, &object, MODULE);
        run_with(&["--version"], &env, &object, b"");
        run_with(&["-filetype=obj", "-o", "-"], &env, &object, MODULE);
        assert_eq!(phases(), "read compile write total read compile write total");

        // A report that cannot be written is an error, like a coverage line.
        let nowhere = scratch.join("no-such-dir/phases.tsv");
        let env = with(&Environment::default(), |e| {
            e.time_phases = Some(nowhere.clone());
        });
        let ran = run_with(&["-filetype=obj", "-o", "-"], &env, &object, MODULE);
        assert_eq!(ran.exit, EXIT_ERROR);
        assert!(
            ran.stderr.starts_with(&format!(
                "hod: error: cannot write phase report '{}': ",
                nowhere.display()
            )),
            "{}",
            ran.stderr
        );
        let ran = run_with(&["-filetype=obj", "--strict"], &env, &hod::compile_releasing, MODULE);
        assert_eq!(ran.exit, EXIT_UNSUPPORTED, "the first failure's status stands");
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
        assert_eq!(line.len(), hod::coverage::FIELDS);
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
        assert_eq!(line[7..], ["-", "-", "-", "-"], "no fallback, no conversion: {line:?}");

        // Standard output: `-o -`, and no `-o` with standard input.
        for args in [&["-filetype=obj", "-o", "-"][..], &["-filetype=obj"]] {
            let ran = run_with(args, &env, &object, MODULE);
            assert_eq!((ran.exit, ran.stderr.as_str()), (EXIT_OK, ""), "{args:?}");
            assert_eq!(ran.stdout, OBJECT, "{args:?}");
        }

        // A file input: read from the file, and named after it without -o.
        let input = scratch.write("m.ll", MODULE);
        let seen = std::cell::RefCell::new(Vec::new());
        let record = |input: &mut Cow<'_, [u8]>, _: &Options, _: &hod::Settings, _: &mut Report| {
            seen.borrow_mut().extend_from_slice(input);
            Ok(OBJECT.to_vec())
        };
        let ran = run_with(&["-filetype=obj", input.to_str().unwrap()], &env, &record, b"unused");
        assert_eq!((ran.exit, ran.stderr.as_str()), (EXIT_OK, ""));
        assert_eq!(*seen.borrow(), MODULE);
        assert!(ran.stdout.is_empty());
        assert_eq!(std::fs::read(scratch.join("m.o")).unwrap(), OBJECT);
    }

    // spec/cli.md "The facade", WP-166: the compile owns the input (and
    // may free it once read) unless the fallback needs it after: standard
    // input with a fallback.
    #[test]
    fn the_input_is_owned_unless_the_fallback_needs_it() {
        let scratch = Scratch::new("driver-input-owned");
        let file = scratch.write("m.ll", MODULE);
        let owned = std::cell::RefCell::new(Vec::new());
        let record = |input: &mut Cow<'_, [u8]>, _: &Options, _: &hod::Settings, _: &mut Report| {
            owned.borrow_mut().push(matches!(input, Cow::Owned(_)));
            assert_eq!(&**input, MODULE);
            Ok(OBJECT.to_vec())
        };
        let fallback = with(&Environment::default(), |e| {
            e.fallback = Some(PathBuf::from("/nonexistent/llc"));
        });
        let file = file.to_str().unwrap();
        let plain = Environment::default();
        for (args, env) in [
            (&["-filetype=obj", "-o", "-", file][..], &plain),
            (&["-filetype=obj", "-o", "-", file][..], &fallback),
            (&["-filetype=obj", "-o", "-"][..], &plain),
            (&["-filetype=obj", "-o", "-", "--strict"][..], &fallback),
            (&["-filetype=obj", "-o", "-"][..], &fallback),
        ] {
            let ran = run_with(args, env, &record, MODULE);
            assert_eq!((ran.exit, ran.stderr.as_str()), (EXIT_OK, ""), "{args:?}");
        }
        assert_eq!(*owned.borrow(), [true, true, true, true, false]);
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
        let record =
            |_: &mut Cow<'_, [u8]>, options: &Options, _: &hod::Settings, _: &mut Report| {
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
    ) -> impl Fn(
        &mut Cow<'_, [u8]>,
        &Options,
        &hod::Settings,
        &mut Report,
    ) -> Result<Vec<u8>, CompileError>
    + '_ {
        |input: &mut Cow<'_, [u8]>, _: &Options, _: &hod::Settings, _: &mut Report| {
            seen.borrow_mut().push(input.to_vec());
            match InputKind::detect(input) {
                InputKind::Bitcode => Err(Unsupported::module(Construct::BitcodeInput).into()),
                InputKind::Text | InputKind::Module => Ok(OBJECT.to_vec()),
            }
        }
    }

    const BITCODE: &[u8] = b"BC\xC0\xDE\x35\x14\x00\x00\x05\x00\x00\x00";
    /// What the stand-in converters print: a module as `llvm-dis` writes it.
    const CONVERTED: &str = "; ModuleID = '<stdin>'\n\
        target triple = \"x86_64-unknown-linux-gnu\"\n\ndefine i32 @f() {\n  ret i32 7\n}\n";

    /// spec/cli.md "Phase report": a conversion is a phase of its own.
    #[test]
    fn phase_report_of_a_conversion() {
        let scratch = Scratch::new("driver-convert-phases");
        // The converter is a child of this test: it holds a jobserver slot.
        let _slot = hod_testutil::jobs::global().acquire();
        scratch.write("llvm-dis.text", CONVERTED);
        let converter = scratch.write_script("llvm-dis", "cat > /dev/null\ncat \"$0.text\"");
        let file = scratch.join("phases.tsv");
        let env = with(&Environment::default(), |e| {
            e.bitcode_via = Some(converter);
            e.time_phases = Some(file.clone());
        });
        let seen = std::cell::RefCell::new(Vec::new());
        let ran = run_with(&["-O0", "-filetype=obj", "-o", "-"], &env, &text_only(&seen), BITCODE);
        assert_eq!((ran.exit, ran.stderr.as_str()), (EXIT_OK, ""));
        let text = std::fs::read_to_string(&file).unwrap();
        let names: Vec<&str> = text.lines().map(|line| line.split('\t').next().unwrap()).collect();
        assert_eq!(names, ["read", "convert", "compile", "write", "total"]);
    }

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
        let env = with(&Environment::default(), |e| {
            e.coverage_log = Some(log.clone());
            e.bitcode_via = Some(converter.clone());
        });
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
        assert_eq!(line.len(), hod::coverage::FIELDS);
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
        let unused = with(&env, |e| {
            e.bitcode_via = Some(scratch.join("missing"));
        });
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
        let malformed = |input: &mut Cow<'_, [u8]>,
                         _: &Options,
                         _: &hod::Settings,
                         _: &mut Report| match InputKind::detect(input) {
            InputKind::Bitcode => Err(Unsupported::module(Construct::BitcodeInput).into()),
            InputKind::Text | InputKind::Module => {
                Err(CompileError::MalformedInput("line 2: expected type".into()))
            }
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
        let env = with(&Environment::default(), |e| {
            e.coverage_log = Some(log.clone());
            e.bitcode_via = Some(scratch.join("no-llvm-dis"));
        });
        // Text; assembly output of bitcode; a rejected command line.
        let ran = run_with(&["-filetype=obj"], &env, &hod::compile_releasing, MODULE);
        assert_eq!(ran.exit, EXIT_UNSUPPORTED);
        let ran = run_with(&["-filetype=asm"], &env, &hod::compile_releasing, BITCODE);
        assert_eq!(ran.exit, EXIT_UNSUPPORTED);
        let ran = run_with(&["-filetype=obj", "-bogus"], &env, &hod::compile_releasing, BITCODE);
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
                ("error", "module:code-model-tiny", "text", "-"),
                ("error", "assembly-output", "bitcode", "-"),
                ("error", "unknown-option", "-", "-"),
                ("hod", "-", "bitcode", "-"),
            ]
        );

        // Bitcode is the reason: the converter is tried, and its absence is
        // the error (status 1: hod failed, not "unsupported").
        let ran = run_with(&["-filetype=obj", "--strict"], &env, &hod::compile_releasing, BITCODE);
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
        // Without a converter, or with `native`, hod reads the bitcode
        // itself: these twelve bytes are a bitcode header and nothing else.
        for via in [None, Some(PathBuf::from(BITCODE_NATIVE))] {
            let plain = with(&env, |e| {
                e.bitcode_via = via;
            });
            let ran = run_with(&["-filetype=obj"], &plain, &hod::compile_releasing, BITCODE);
            assert_eq!(ran.exit, EXIT_ERROR, "{}", ran.stderr);
            assert!(ran.stderr.starts_with("hod: error: malformed input: bit "), "{}", ran.stderr);
        }
        let lines = fields(&log);
        for line in &lines[5..] {
            assert_eq!((line[1].as_str(), line[8].as_str()), ("malformed-input", "-"));
        }
    }

    // spec/cli.md "Exit status": unsupported without a fallback is 2 and
    // names the construct (and the function, when there is one).
    #[test]
    fn unsupported_without_fallback() {
        let scratch = Scratch::new("driver-unsupported");
        let log = scratch.join("cov.tsv");
        let env = with(&Environment::default(), |e| {
            e.coverage_log = Some(log.clone());
        });
        let ran =
            run_with(&["-filetype=obj", "-o", "-"], &env, &hod::compile_releasing, CG_DECLINED);
        assert_eq!(ran.exit, EXIT_UNSUPPORTED);
        assert!(ran.stdout.is_empty());
        assert_eq!(
            ran.stderr,
            "hod: error: unsupported construct 'attr:target-feature-strict-align' in function \
             'f': +strict-align; no fallback is configured (--fallback=<llc> or \
             HOD_FALLBACK_LLC)\n"
        );

        let in_function =
            |_: &mut Cow<'_, [u8]>, _: &Options, _: &hod::Settings, _: &mut Report| {
                Err(Unsupported::in_function(
                    Construct::Named("x64:todo:abi".into()),
                    Some("_Z1fv".into()),
                )
                .into())
            };
        let ran = run_with(&["-filetype=obj", "--strict"], &env, &in_function, MODULE);
        assert_eq!(ran.exit, EXIT_UNSUPPORTED);
        assert_eq!(
            ran.stderr,
            "hod: error: unsupported construct 'x64:todo:abi' in function '_Z1fv' \
             (strict mode: no fallback)\n"
        );

        let lines = fields(&log);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0][..3], ["error", "attr:target-feature-strict-align", "text"]);
        assert_eq!(lines[1][..3], ["error", "x64:todo:abi", "text"]);
        for line in &lines {
            assert_eq!(line[7], "-");
        }
    }

    // spec/cli.md "Exit status": the other compile errors are 1.
    #[test]
    fn malformed_input_and_invalid_options() {
        let scratch = Scratch::new("driver-malformed");
        let log = scratch.join("cov.tsv");
        let env = with(&Environment::default(), |e| {
            e.coverage_log = Some(log.clone());
        });
        let malformed = |_: &mut Cow<'_, [u8]>, _: &Options, _: &hod::Settings, _: &mut Report| {
            Err(CompileError::MalformedInput("line 2: expected '}'".into()))
        };
        let ran = run_with(&["-filetype=obj"], &env, &malformed, b"define");
        assert_eq!(ran.exit, EXIT_ERROR);
        assert!(ran.stderr.starts_with("hod: error: malformed input: line 2: expected '}'"));
        let invalid = |_: &mut Cow<'_, [u8]>, _: &Options, _: &hod::Settings, _: &mut Report| {
            Err(CompileError::InvalidOptions("a without b".into()))
        };
        let ran = run_with(&["-filetype=obj"], &env, &invalid, b"");
        assert_eq!(ran.exit, EXIT_ERROR);
        assert!(ran.stderr.starts_with("hod: error: invalid options: a without b"));
        let reasons: Vec<String> = fields(&log).into_iter().map(|l| l[1].clone()).collect();
        assert_eq!(reasons, ["malformed-input", "invalid-options"]);
    }

    // spec/cli.md "Reasons", "Exit status": an internal error of code
    // generation is not a decline: status 1, reason `internal-error`, and
    // in hybrid mode the fallback builds the module.
    #[test]
    fn internal_errors_are_not_declines() {
        let scratch = Scratch::new("driver-internal");
        let log = scratch.join("cov.tsv");
        let env = with(&Environment::default(), |e| {
            e.coverage_log = Some(log.clone());
        });
        let internal = |_: &mut Cow<'_, [u8]>, _: &Options, _: &hod::Settings, _: &mut Report| {
            Err(CompileError::Internal("function 'f': a lost value".into()))
        };
        let ran = run_with(&["-filetype=obj", "--strict"], &env, &internal, MODULE);
        assert_eq!(ran.exit, EXIT_ERROR);
        assert_eq!(
            ran.stderr,
            "hod: error: internal error: function 'f': a lost value (strict mode: no fallback)\n"
        );
        let ran = run_with(&["-filetype=obj", "--fallback=/bin/true"], &env, &internal, MODULE);
        assert_eq!((ran.exit, ran.stderr.as_str()), (EXIT_OK, ""));
        let lines = fields(&log);
        let summary: Vec<(&str, &str)> =
            lines.iter().map(|l| (l[0].as_str(), l[1].as_str())).collect();
        assert_eq!(summary, [("error", "internal-error"), ("fallback", "internal-error")]);
        // A contained panic (WP-093, "Embedding") likewise: `internal-panic`.
        let panicked = |_: &mut Cow<'_, [u8]>, _: &Options, _: &hod::Settings, _: &mut Report| {
            Err(CompileError::Panic("index out of bounds".into()))
        };
        let ran = run_with(&["-filetype=obj", "--strict"], &env, &panicked, MODULE);
        assert_eq!(ran.exit, EXIT_ERROR);
        assert_eq!(
            ran.stderr,
            "hod: error: internal error: hod panicked: index out of bounds (strict mode: no fallback)\n"
        );
        let ran = run_with(&["-filetype=obj", "--fallback=/bin/true"], &env, &panicked, MODULE);
        assert_eq!((ran.exit, ran.stderr.as_str()), (EXIT_OK, ""));
        let lines = fields(&log);
        let reasons: Vec<&str> = lines[2..].iter().map(|l| l[1].as_str()).collect();
        assert_eq!(reasons, ["internal-panic", "internal-panic"]);
    }

    // spec/cli.md "Options": without a fallback an unknown option is an
    // error naming it, and the input is not read.
    #[test]
    fn unknown_option_without_fallback() {
        let scratch = Scratch::new("driver-unknown");
        let log = scratch.join("cov.tsv");
        let env = with(&Environment::default(), |e| {
            e.coverage_log = Some(log.clone());
        });
        let never = |_: &mut Cow<'_, [u8]>,
                     _: &Options,
                     _: &hod::Settings,
                     _: &mut Report|
         -> Result<Vec<u8>, CompileError> {
            panic!("the code generator must not run")
        };
        let ran = run_with(&["-filetype=obj", "-bogus", "-o", "a.o"], &env, &never, MODULE);
        assert_eq!(ran.exit, EXIT_ERROR);
        assert!(ran.stderr.starts_with("hod: error: unknown option '-bogus'"), "{}", ran.stderr);
        assert_eq!(ran.stderr.lines().count(), 1);
        // Strict mode ignores a configured fallback.
        let strict = with(&env, |e| {
            e.fallback = Some("/nonexistent/llc".into());
            e.strict = true;
        });
        let ran = run_with(&["-filetype=null"], &strict, &never, MODULE);
        // WP-021: in strict mode an unknown option is a decline, status 2;
        // a usage mistake stays an error.
        assert_eq!(ran.exit, EXIT_UNSUPPORTED);
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
        // WP-021: strict mode changes the status of `unknown-option` only.
        let ran = run_with(&["-filetype=obj", "a.ll", "b.ll"], &strict, &never, MODULE);
        assert_eq!(ran.exit, EXIT_ERROR);
        let ran = run_with(&["-filetype=obj", "-bogus"], &strict, &never, MODULE);
        assert_eq!(ran.exit, EXIT_UNSUPPORTED);
    }

    // spec/cli.md "hod's own options" and "Environment": mistakes in them
    // are errors even when a fallback is configured.
    #[test]
    fn mistakes_in_hod_settings() {
        let scratch = Scratch::new("driver-usage");
        let log = scratch.join("cov.tsv");
        let env = with(&Environment::default(), |e| {
            e.fallback = Some("/nonexistent/llc".into());
            e.coverage_log = Some(log.clone());
        });
        let ran = run_with(&["-filetype=obj", "--strict=maybe"], &env, &object, MODULE);
        assert_eq!(ran.exit, EXIT_ERROR);
        assert!(ran.stderr.contains("invalid value 'maybe' for '--strict'"), "{}", ran.stderr);
        let bad_env = with(&env, |e| {
            e.error = Some("invalid value 'yes' for HOD_STRICT".into());
        });
        let ran = run_with(&["-filetype=obj"], &bad_env, &object, MODULE);
        assert_eq!(ran.exit, EXIT_ERROR);
        assert_eq!(ran.stderr, "hod: error: invalid value 'yes' for HOD_STRICT\n");
        let reasons: Vec<String> = fields(&log).into_iter().map(|l| l[1].clone()).collect();
        assert_eq!(reasons, ["usage", "usage"]);
    }

    // spec/cli.md "Fallback protocol": a hod started as a fallback stops.
    #[test]
    fn fallback_loop_is_refused() {
        let env = with(&Environment::default(), |e| {
            e.fallback = Some("/nonexistent/llc".into());
            e.in_fallback = true;
        });
        let ran = run_with(&["-filetype=obj"], &env, &hod::compile_releasing, MODULE);
        assert_eq!(ran.exit, EXIT_ERROR);
        assert!(ran.stderr.contains("HOD_IN_FALLBACK"), "{}", ran.stderr);
        assert!(ran.stderr.contains("fallback or its bitcode converter"), "{}", ran.stderr);
    }

    // spec/cli.md "Exit status": I/O failures are errors with one line.
    #[test]
    fn io_errors() {
        let scratch = Scratch::new("driver-io");
        let log = scratch.join("cov.tsv");
        let env = with(&Environment::default(), |e| {
            e.coverage_log = Some(log.clone());
        });
        let missing = scratch.join("missing.ll");
        let ran = run_with(&["-filetype=obj", missing.to_str().unwrap()], &env, &object, b"");
        assert_eq!(ran.exit, EXIT_ERROR);
        assert!(ran.stderr.starts_with("hod: error: cannot read '"), "{}", ran.stderr);
        let unwritable = scratch.join("no-such-dir/a.o");
        let ran =
            run_with(&["-filetype=obj", "-o", unwritable.to_str().unwrap()], &env, &object, MODULE);
        assert_eq!(ran.exit, EXIT_ERROR);
        assert!(ran.stderr.starts_with("hod: error: cannot write '"), "{}", ran.stderr);
        let unrunnable = with(&env, |e| {
            e.fallback = Some(scratch.join("no-llc"));
        });
        let ran = run_with(&["-filetype=obj"], &unrunnable, &hod::compile_releasing, MODULE);
        assert_eq!(ran.exit, EXIT_ERROR);
        assert!(ran.stderr.starts_with("hod: error: cannot run fallback '"), "{}", ran.stderr);
        let reasons: Vec<String> = fields(&log).into_iter().map(|l| l[1].clone()).collect();
        assert_eq!(reasons, ["input-unreadable", "output-unwritable", "fallback-unavailable"]);

        // A coverage log that cannot be opened stops the invocation before
        // anything is compiled or written.
        let bad_log = with(&Environment::default(), |e| {
            e.coverage_log = Some(scratch.join("no-such-dir/cov.tsv"));
        });
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
        let env = with(&Environment::default(), |e| {
            e.coverage_log = Some(log.clone());
        });
        let ran = run_with(&["--version"], &env, &object, b"");
        assert_eq!(ran.exit, EXIT_OK);
        let text = String::from_utf8(ran.stdout).unwrap();
        assert_eq!(text, format!("hod version {}\n", env!("CARGO_PKG_VERSION")));
        let ran = run_with(&["-O0", "--help"], &env, &object, b"");
        assert_eq!(ran.exit, EXIT_OK);
        assert!(String::from_utf8(ran.stdout).unwrap().starts_with("OVERVIEW: hod"));
        assert!(!log.exists());
        assert!(HELP.contains("--fallback=<llc>") && HELP.contains("HOD_COVERAGE_LOG"));
        assert!(HELP.contains("--bitcode-via=<llvm-dis>") && HELP.contains("HOD_BITCODE_VIA"));
        assert!(HELP.contains("--compress-debug-sections=<none|zlib>"));
        assert!(HELP.contains("--threads=<n>") && HELP.contains("HOD_THREADS"));
    }
}
