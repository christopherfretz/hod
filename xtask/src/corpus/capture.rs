//! `cargo xtask corpus capture`: runs the producers over their suites and
//! keeps what can be an `irdiff` test. Contract: `spec/harness.md`
//! "corpus", "capture".
//!
//! One group is one source with one LLVM major. A group's tests are
//! captured in parallel, each in a job directory of its own; what was
//! captured is then admitted: built by the oracle and by its witness,
//! linked with every linker and run by the code `irdiff` itself uses, so
//! that an entry of the corpus is a test whose oracle side is known to
//! pass.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use super::entry::{self, Delivered, Module, Recipe, Unfit};
use super::manifest::{self, Manifest, Row, Skip, Skips};
use super::{kiln, sha256, torture};
use crate::irdiff::directives::{Arch, Expectations, Lib};
use crate::irdiff::process::{self, End, Limits, Output};
use crate::irdiff::tools::{self, Llvm};
use crate::irdiff::{
    BuiltBy, LINKER_NAMES, Linker, MAX_JOBS, Mode, Outcome, Record, Search, Session, Test,
    Toolchain, engine,
};
use crate::shim;

/// The sources, by the names of their directories in the corpus.
pub const KILN: &str = "kiln";
pub const TORTURE: &str = "gcc-torture";
pub const SOURCES: [&str; 2] = [KILN, TORTURE];

/// Part of every entry's key: changing how entries are made changes it,
/// and the next capture makes every entry again.
const FORMAT: &str = "hod-corpus-2";

/// The table of what the captures of a corpus did, in the corpus
/// directory. Unlike the manifest it holds times.
pub const RUNS: &str = "CAPTURE.tsv";
const RUNS_HEADER: &str = "source\tllvm\ttests\tcaptured\treused\tskipped\tseconds\tjobs";

/// The reason the stand-in for hod gives when it declines a module during
/// admission.
const ADMISSION: &str = "corpus-admission";

/// The stand-in for hod during admission: it declines every module, as
/// `hod --strict` declines one it does not support (`spec/cli.md`), so
/// that only the oracle's side of each test is built, linked and run.
pub const DECLINE_SCRIPT: &str = "#!/bin/sh\n\
# Written by `cargo xtask corpus capture` (spec/harness.md \"corpus\").\n\
# Stands where hod stands while entries are admitted: declines everything.\n\
log=\n\
for arg in \"$@\"; do\n\
  case \"$arg\" in --coverage-log=*) log=${arg#--coverage-log=} ;; esac\n\
done\n\
printf 'error\\tcorpus-admission\\ttext\\t0\\t-\\t-\\t0\\t-\\n' >> \"${log:?}\"\n\
exit 2\n";

/// A capture.
#[derive(Clone, Debug)]
pub struct Options {
    /// The sources to capture, of [`SOURCES`].
    pub sources: Vec<&'static str>,
    /// The LLVM majors to capture with; empty is every one of
    /// [`tools::LLVM_MAJORS`] that is installed.
    pub llvm: Vec<u32>,
    /// The corpus directory.
    pub out: PathBuf,
    /// `kiln++`.
    pub kiln: PathBuf,
    /// kiln's repository: its tests.
    pub kiln_repo: PathBuf,
    /// A checkout of GCC's sources that holds the torture suite.
    pub torture: PathBuf,
    /// Only the first so many tests of each group, in name order.
    pub limit: Option<usize>,
    /// Tests captured at once.
    pub jobs: usize,
    /// Capture again what is already there.
    pub force: bool,
    /// Try again the tests the last capture skipped.
    pub retry_skipped: bool,
    /// Whether a missing tool of the gate is a failure.
    pub require_tools: bool,
    pub search: Search,
    /// Limits of a producer (kiln++, clang).
    pub compile_limits: Limits,
    /// Limits of `llc` and the linker driver.
    pub tool_limits: Limits,
    /// Limits of the programs.
    pub run_limits: Limits,
}

/// The default of `-j`: this is a shared machine's command.
pub const DEFAULT_JOBS: usize = 3;

impl Options {
    /// The defaults, for a corpus in `out`.
    pub fn new(out: PathBuf) -> Self {
        let cores = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
        Self {
            sources: SOURCES.to_vec(),
            llvm: Vec::new(),
            out,
            kiln: PathBuf::new(),
            kiln_repo: PathBuf::new(),
            torture: PathBuf::new(),
            limit: None,
            jobs: DEFAULT_JOBS.min(cores),
            force: false,
            retry_skipped: false,
            require_tools: false,
            search: Search::from_env(),
            // kiln's harness gives its compiler 120 s and 8 GiB.
            compile_limits: Limits { time: Duration::from_mins(2), memory_kib: 8 << 20 },
            tool_limits: Limits { time: Duration::from_mins(1), memory_kib: 4 << 20 },
            run_limits: Limits { time: Duration::from_secs(10), memory_kib: 1 << 20 },
        }
    }
}

/// What a capture did with one group.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GroupStats {
    pub source: String,
    pub llvm: u32,
    /// The tests of the group this run looked at.
    pub tests: usize,
    /// Entries written by this run.
    pub captured: usize,
    /// Entries of an earlier run that still stand.
    pub reused: usize,
    /// Tests without an entry, by reason.
    pub skipped: BTreeMap<String, usize>,
    /// Entries of an earlier run removed: their test is gone or is now
    /// skipped.
    pub removed: usize,
    pub seconds: f64,
}

/// What a capture did.
#[derive(Clone, Debug, Default)]
pub struct CaptureReport {
    pub groups: Vec<GroupStats>,
    /// What was left out and why (a source that is not there, an LLVM that
    /// is not installed).
    pub notes: Vec<String>,
    /// Tools the gate requires that are missing, when tools are required.
    pub missing_tools: Vec<String>,
    /// Entries and bytes in the corpus after the capture.
    pub entries: usize,
    pub bytes: u64,
}

impl CaptureReport {
    /// The command's exit status: 3 when a required tool is missing.
    pub fn exit_status(&self) -> u8 {
        if self.missing_tools.is_empty() { 0 } else { 3 }
    }

    /// The text the command prints.
    pub fn summary(&self, out: &Path) -> String {
        let mut text = String::new();
        for group in &self.groups {
            let skipped: usize = group.skipped.values().sum();
            let _ = writeln!(
                text,
                "corpus: {}/llvm{}: {} tests: {} captured, {} reused, {} skipped, {} removed; {:.1} s",
                group.source,
                group.llvm,
                group.tests,
                group.captured,
                group.reused,
                skipped,
                group.removed,
                group.seconds
            );
            let mut reasons: Vec<(&String, &usize)> = group.skipped.iter().collect();
            reasons.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
            for (reason, count) in reasons {
                let _ = writeln!(text, "  skipped {count:>5}  {reason}");
            }
        }
        for note in &self.notes {
            let _ = writeln!(text, "note: {note}");
        }
        for tool in &self.missing_tools {
            let _ = writeln!(text, "MISSING {tool}");
        }
        let _ = writeln!(
            text,
            "corpus: {} entries, {}.{} MiB in '{}'",
            self.entries,
            self.bytes >> 20,
            ((self.bytes % (1 << 20)) * 10) >> 20,
            out.display()
        );
        text
    }
}

/// The path of a test's entry in the corpus, with `/`:
/// `<source>/llvm<N>/<test>.ll`; a kiln test is named without its leading
/// `tests/`.
pub fn entry_path(source: &str, llvm: u32, test: &str) -> String {
    let below = if source == KILN { test.strip_prefix("tests/").unwrap_or(test) } else { test };
    format!("{source}/llvm{llvm}/{below}.ll")
}

/// The directory beside the corpus that captures work in:
/// `<corpus>.work`.
pub fn work_dir(out: &Path) -> PathBuf {
    let mut name = out.file_name().unwrap_or_default().to_os_string();
    name.push(".work");
    out.with_file_name(name)
}

fn io(what: &str, path: &Path, e: &std::io::Error) -> String {
    format!("cannot {what} '{}': {e}", path.display())
}

/// Writes `content` at `path` through a temporary file beside it, with
/// mode `mode`; nothing is written when the file is already that.
fn write_file(path: &Path, content: &[u8], mode: u32) -> Result<(), String> {
    let same = fs::metadata(path).is_ok_and(|meta| meta.permissions().mode() & 0o777 == mode)
        && fs::read(path).is_ok_and(|current| current == content);
    if same {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| io("create", dir, &e))?;
    }
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".tmp-{}", std::process::id()));
    let temp = path.with_file_name(name);
    fs::write(&temp, content).map_err(|e| io("write", &temp, &e))?;
    fs::set_permissions(&temp, fs::Permissions::from_mode(mode))
        .map_err(|e| io("set the mode of", &temp, &e))?;
    fs::rename(&temp, path).map_err(|e| io("write", path, &e))
}

/// Makes `link` a symbolic link to `target`.
fn link_to(target: &Path, link: &Path) -> Result<(), String> {
    if fs::read_link(link).is_ok_and(|current| current == target) {
        return Ok(());
    }
    if fs::symlink_metadata(link).is_ok() {
        fs::remove_file(link).map_err(|e| io("replace", link, &e))?;
    }
    std::os::unix::fs::symlink(target, link).map_err(|e| io("create the link", link, &e))
}

/// The first line of what `program <args>` prints that contains `needle`,
/// or `unknown`.
fn output_line(program: &Path, args: &[&str], dir: Option<&Path>, needle: &str) -> String {
    let probe = Limits { time: Duration::from_secs(20), memory_kib: 8 << 20 };
    let configure = |command: &mut std::process::Command| {
        command.args(args).env("GIT_OPTIONAL_LOCKS", "0");
        if let Some(dir) = dir {
            command.current_dir(dir);
        }
    };
    match process::run(program, configure, b"", probe) {
        Ok(output) if output.end == End::Exit(0) => {
            let text = String::from_utf8_lossy(&output.stdout);
            text.lines()
                .map(str::trim)
                .find(|line| !line.is_empty() && line.contains(needle))
                .unwrap_or("unknown")
                .to_owned()
        }
        _ => "unknown".to_owned(),
    }
}

fn first_line(program: &Path, args: &[&str], dir: Option<&Path>) -> String {
    output_line(program, args, dir, "")
}

/// The line of `program --version` that contains `needle`, or `unknown`.
pub fn version_line(program: &Path, needle: &str) -> String {
    output_line(program, &["--version"], None, needle)
}

/// The revision of the git checkout whose top directory is `repo`: its
/// commit, with `-dirty` when tracked files differ from it; `unknown` when
/// `repo` is not the top of a checkout or there is no `git`.
pub fn revision(repo: &Path, search: &Search) -> String {
    let Some(git) = search.on_path("git") else { return "unknown".to_owned() };
    let top = first_line(&git, &["rev-parse", "--show-toplevel"], Some(repo));
    let same = match (fs::canonicalize(&top), fs::canonicalize(repo)) {
        (Ok(top), Ok(repo)) => top == repo,
        _ => false,
    };
    if !same {
        return "unknown".to_owned();
    }
    let commit = first_line(&git, &["rev-parse", "HEAD"], Some(repo));
    if commit == "unknown" {
        return commit;
    }
    let dirty = first_line(&git, &["status", "--porcelain", "--untracked-files=no"], Some(repo));
    if dirty == "unknown" { commit } else { format!("{commit}-dirty") }
}

/// `text` with the paths of this run replaced by names, cut to one line of
/// bounded length: what a table may hold.
fn tidy(text: &str, replace: &[(&Path, &str)]) -> String {
    let mut text = text.to_owned();
    for (path, name) in replace {
        text = text.replace(&*path.to_string_lossy(), name);
    }
    let line = text.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("");
    let mut end = line.len().min(300);
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    line[..end].to_owned()
}

/// "<tool> failed (exit status 1): <first line of its diagnostics>".
fn failure(tool: &str, output: &Output) -> String {
    let how = match output.end {
        End::Exit(code) => format!("exit status {code}"),
        End::Signal(signal) => format!("signal {signal}"),
        End::TimedOut => "timed out".to_owned(),
    };
    let text = String::from_utf8_lossy(&output.stderr);
    // The line that says what is wrong, if one does.
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| line.contains("error"))
        .or_else(|| text.lines().map(str::trim).find(|line| !line.is_empty()))
        .unwrap_or("");
    if line.is_empty() { format!("{tool}: {how}") } else { format!("{tool}: {how}: {line}") }
}

/// What a group is captured with.
enum Producer {
    Kiln {
        kiln: PathBuf,
        /// The directory kiln runs in: it holds links to the repository's
        /// `tests` and `resource`, so kiln sees the paths its own harness
        /// gives it and can write nothing into the repository.
        root: PathBuf,
        /// The capture directory: an LLVM directory whose `llc` records.
        llvm_dir: PathBuf,
        cache: PathBuf,
    },
    Torture {
        clang: PathBuf,
        /// The suite's directory; clang runs in it.
        execute: PathBuf,
        driver: PathBuf,
    },
}

/// One test of a group that has to be captured.
struct Item {
    /// The test as its suite names it.
    test: String,
    key: String,
    work: Work,
}

enum Work {
    Kiln(kiln::KilnTest, Box<kiln::Header>),
    Torture(torture::TortureTest),
}

/// The modules of a test that were captured and whose program ran: an
/// entry, if it is admitted.
struct Candidate {
    expect: Expectations,
    producer_args: String,
    llc_args: String,
    modules: Vec<Module>,
}

impl Candidate {
    /// The entry file.
    fn text(&self) -> Result<String, Unfit> {
        entry::compose(&self.expect, &self.producer_args, &self.llc_args, &self.modules)
    }

    /// A candidate that can be written as an entry.
    fn checked(self) -> Result<Self, Unfit> {
        self.text().map(|_| self)
    }
}

/// One `llc` run of kiln, and the entry's name for its module if the test
/// has several.
type Call = (Option<String>, kiln::BackendCall);

/// Everything the tests of one group share.
struct Group<'a> {
    options: &'a Options,
    source: &'static str,
    llvm: &'a Llvm,
    host: Arch,
    producer: Producer,
    /// The group's `llvm-dis`: what writes a module that reached `llc` as
    /// bitcode as text.
    llvm_dis: Option<PathBuf>,
    /// This run's directory under the work directory.
    run: PathBuf,
}

impl Group<'_> {
    fn tidy(&self, text: &str, job: &Path) -> String {
        tidy(
            text,
            &[(job, "<job>"), (&self.run, "<run>"), (&work_dir(&self.options.out), "<work>")],
        )
    }

    /// Captures one test in the job directory `job`.
    fn capture(&self, item: &Item, job: &Path) -> Result<Candidate, Unfit> {
        let broken = |what: String| Unfit::new("capture-broken", what);
        for dir in [job.join("tmp"), job.join("run")] {
            fs::create_dir_all(&dir).map_err(|e| broken(io("create", &dir, &e)))?;
        }
        match (&self.producer, &item.work) {
            (Producer::Kiln { .. }, Work::Kiln(test, header)) => {
                self.capture_kiln(test, header, job)
            }
            (Producer::Torture { clang, execute, driver }, Work::Torture(test)) => {
                self.capture_torture(test, job, clang, execute, driver)
            }
            _ => Err(broken("the test is not of the group's source".to_owned())),
        }
    }

    /// Runs kiln++ in its root with `args`; what it gives its backend is
    /// saved under `calls`.
    fn run_kiln(
        &self,
        args: &[std::ffi::OsString],
        calls: &Path,
        job: &Path,
    ) -> Result<Output, Unfit> {
        let Producer::Kiln { kiln, root, cache, .. } = &self.producer else {
            return Err(Unfit::new("capture-broken", "the group's producer is not kiln"));
        };
        let configure = |command: &mut std::process::Command| {
            command
                .args(args)
                .current_dir(root)
                .env("TMPDIR", job.join("tmp"))
                .env("XDG_CACHE_HOME", cache)
                .env("HOD_CORPUS_CAPTURE", calls)
                .env("LC_ALL", "C")
                .env_remove("KILN_LLVM_DIR");
        };
        process::run(kiln, configure, b"", self.options.compile_limits)
            .map_err(|what| Unfit::new("capture-broken", what))
    }

    /// kiln's arguments for one command: `--llvm-dir=`, `args` with one
    /// more before its last (the source): debug information names the
    /// directory kiln ran in, which is this machine's; it is written as
    /// `.` instead. Then `-o <output>`.
    fn kiln_args(&self, args: &[String], output: &Path) -> Vec<std::ffi::OsString> {
        let Producer::Kiln { root, llvm_dir, .. } = &self.producer else { return Vec::new() };
        let (source, before) = args.split_last().expect("the source is an argument");
        let mut all = vec![tools::prefixed("--llvm-dir=", llvm_dir)];
        all.extend(before.iter().map(Into::into));
        let mut map = tools::prefixed("-fdebug-prefix-map=", root);
        map.push("=.");
        all.push(map);
        all.push(source.into());
        all.push("-o".into());
        all.push(output.into());
        all
    }

    /// What kiln gave `llc` for a test, in the order of the link, and the
    /// arguments it was run with as the tables show them.
    fn kiln_calls(
        &self,
        plan: &kiln::Plan,
        job: &Path,
        program: &Path,
    ) -> Result<(Vec<Call>, String), Unfit> {
        let broken = |what: String| Unfit::new("capture-broken", what);
        let shown = |args: &[String]| {
            let mut shown = args.to_vec();
            shown.insert(shown.len() - 1, "-fdebug-prefix-map=<cwd>=.".to_owned());
            shown.join(" ")
        };
        let no_module = |plan: &kiln::Plan| {
            if plan.compile_args.iter().any(|arg| arg.starts_with("-flto")) {
                Unfit::new("opt-first-lto", "link-time optimization: kiln++ did not run llc")
            } else {
                Unfit::new("no-module", "kiln++ did not run llc")
            }
        };
        let Some(steps) = plan.two_step(&job.join("bmi").to_string_lossy()) else {
            // One command.
            let built = self.run_kiln(&self.kiln_args(&plan.compile_args, program), job, job)?;
            let calls = kiln::backend_calls(job).map_err(broken)?;
            if !(built.end == End::Exit(0) && program.is_file()) {
                // kiln stops at the first tool that fails: with a module
                // already given to llc, it was llc or the link.
                let reason = if calls.is_empty() { "compile-fails" } else { "link-fails" };
                return Err(Unfit::new(reason, self.tidy(&failure("kiln++", &built), job)));
            }
            let count = calls.len();
            let calls = match count {
                0 => return Err(no_module(plan)),
                1 => calls.into_iter().map(|call| (None, call)).collect(),
                // Several files on one command line.
                _ => kiln::in_link_order(calls)
                    .ok_or_else(|| {
                        let detail = format!(
                            "kiln++ ran llc {count} times and its objects do not say in which order they are linked"
                        );
                        Unfit::new("multi-file", detail)
                    })?
                    .into_iter()
                    .map(|(name, call)| (Some(name), call))
                    .collect(),
            };
            return Ok((calls, shown(&plan.compile_args)));
        };
        // Separate steps, as kiln's own harness builds the test.
        let steps = steps?;
        for dir in ["bmi", "obj"] {
            let dir = job.join(dir);
            fs::create_dir_all(&dir).map_err(|e| broken(io("create", &dir, &e)))?;
        }
        let object = |name: &str| job.join("obj").join(format!("{name}.o"));
        let mut made: Vec<(String, kiln::BackendCall)> = Vec::new();
        for (index, compile) in steps.compiles.iter().enumerate() {
            let calls_dir = job.join(format!("step-{index}"));
            fs::create_dir_all(&calls_dir).map_err(|e| broken(io("create", &calls_dir, &e)))?;
            let output = object(&compile.name);
            let built = self.run_kiln(&self.kiln_args(&compile.args, &output), &calls_dir, job)?;
            let mut calls = kiln::backend_calls(&calls_dir).map_err(broken)?;
            if !(built.end == End::Exit(0) && output.is_file()) {
                let reason = if calls.is_empty() { "compile-fails" } else { "llc-fails" };
                let detail = format!("module '{}': {}", compile.name, failure("kiln++", &built));
                return Err(Unfit::new(reason, self.tidy(&detail, job)));
            }
            match (calls.pop(), calls.len()) {
                (Some(call), 0) => made.push((compile.name.clone(), call)),
                (None, _) => return Err(no_module(plan)),
                (Some(_), more) => {
                    let detail = format!(
                        "kiln++ ran llc {} times for the one file of module '{}'",
                        more + 1,
                        compile.name
                    );
                    return Err(Unfit::new("multi-file", detail));
                }
            }
        }
        let mut link: Vec<std::ffi::OsString> = Vec::new();
        if let Producer::Kiln { llvm_dir, .. } = &self.producer {
            link.push(tools::prefixed("--llvm-dir=", llvm_dir));
        }
        link.extend(steps.link_args.iter().map(Into::into));
        link.extend(steps.link_order.iter().map(|name| object(name).into_os_string()));
        link.push("-o".into());
        link.push(program.into());
        let linked = self.run_kiln(&link, &job.join("obj"), job)?;
        if !(linked.end == End::Exit(0) && program.is_file()) {
            return Err(Unfit::new("link-fails", self.tidy(&failure("kiln++", &linked), job)));
        }
        // In the order of the link; a test of one file is one module.
        let alone = steps.link_order.len() == 1;
        let mut calls = Vec::new();
        for name in &steps.link_order {
            let at = made.iter().position(|(made, _)| made == name).expect("every step was run");
            let (name, call) = made.swap_remove(at);
            calls.push(((!alone).then_some(name), call));
        }
        // The same steps, with a name in place of this job's directory.
        let named = plan.two_step("<job>/bmi").expect("the plan has steps")?;
        let mut commands: Vec<String> = named
            .compiles
            .iter()
            .map(|compile| format!("{} -o {}.o", shown(&compile.args), compile.name))
            .collect();
        let objects: Vec<String> =
            named.link_order.iter().map(|name| format!("{name}.o")).collect();
        commands.push(format!("{} {} -o test.out", named.link_args.join(" "), objects.join(" ")));
        Ok((calls, commands.join(" && ")))
    }

    /// The module of one `llc` run as an entry holds it: bitcode is
    /// written as text by the group's `llvm-dis`.
    fn module_of_call(
        &self,
        name: Option<String>,
        module: Vec<u8>,
        test: &kiln::KilnTest,
        plan: &kiln::Plan,
        job: &Path,
    ) -> Result<Module, Unfit> {
        let broken = |what: String| Unfit::new("capture-broken", what);
        if !kiln::is_bitcode(&module) {
            return Ok(Module { name, text: module, delivered: Delivered::Text });
        }
        let Some(llvm_dis) = &self.llvm_dis else {
            // Without the tool the test is skipped for what made kiln run
            // `opt`, as a capture without this step skipped it.
            let source = kiln::source_with_includes(&test.path);
            let reason = kiln::opt_first_reason(&plan.compile_args, &source);
            let detail = "llc was given bitcode (opt ran first) and no llvm-dis is installed to \
                          write it as text";
            return Err(Unfit::new(reason, detail));
        };
        let listing = job.join("module.ll");
        let configure = |command: &mut std::process::Command| {
            command.arg("-o").arg(&listing).arg("-").env("LC_ALL", "C");
        };
        let output =
            process::run(llvm_dis, configure, &module, self.options.tool_limits).map_err(broken)?;
        if output.end != End::Exit(0) {
            let detail = self.tidy(&failure("llvm-dis", &output), job);
            return Err(Unfit::new("llvm-dis-fails", detail));
        }
        let bytes = fs::read(&listing).map_err(|e| broken(io("read", &listing, &e)))?;
        Ok(Module { name, text: bytes, delivered: Delivered::Bitcode })
    }

    fn capture_kiln(
        &self,
        test: &kiln::KilnTest,
        header: &kiln::Header,
        job: &Path,
    ) -> Result<Candidate, Unfit> {
        let broken = |what: String| Unfit::new("capture-broken", what);
        let plan = kiln::plan(test, header, self.host, self.llvm.major)?;
        let program = job.join("test.out");
        let (calls, producer_args) = self.kiln_calls(&plan, job, &program)?;

        // What llc was given besides the modules: the same for each.
        let mut llc_args: Option<Vec<String>> = None;
        for (_, call) in &calls {
            let Some(args) = kiln::llc_args(&call.args) else {
                let detail =
                    format!("llc's command line is not '... -o <object> -': {:?}", call.args);
                return Err(Unfit::new("llc-args-unexpected", self.tidy(&detail, job)));
            };
            match &llc_args {
                Some(first) if *first != args => {
                    let detail = format!(
                        "the test's modules were given different llc arguments: '{}' and '{}'",
                        first.join(" "),
                        args.join(" ")
                    );
                    return Err(Unfit::new("multi-file", detail));
                }
                _ => llc_args = Some(args),
            }
        }
        let llc_args = llc_args.expect("a test has a module");
        let pie = !llc_args.iter().any(|arg| arg == "-relocation-model=static");
        let mut modules = Vec::new();
        for (name, call) in calls {
            modules.push(self.module_of_call(name, call.stdin, test, &plan, job)?);
        }

        let ran = self.run_program(&program, &plan.run_args, &plan.stdin, job).map_err(broken)?;
        let recipe = Recipe {
            covers: format!("{KILN} {}", test.name),
            arch: self.host,
            llvm: self.llvm.major,
            libs: vec![Lib::Cxx, Lib::M],
            pie,
            args: plan.run_args.clone(),
            stdin: plan.stdin.clone(),
            llc_args: kiln::repeated_llc_args(&llc_args),
        };
        if !matches!(ran.end, End::Exit(_)) {
            // Not an exit: the reason and detail of `expectations`.
            entry::expectations(&recipe, &ran)?;
        }
        if ran.end != End::Exit(i32::from(plan.want_exit)) {
            let detail = format!(
                "the program ended with {}, the test expects exit status {}",
                ended(ran.end),
                plan.want_exit
            );
            return Err(Unfit::new("expectation-mismatch", detail));
        }
        if plan.want_stdout.as_ref().is_some_and(|want| want.as_bytes() != ran.stdout) {
            let detail = "the program's standard output is not the test's 'stdout:' lines";
            return Err(Unfit::new("expectation-mismatch", detail));
        }
        let expect = entry::expectations(&recipe, &ran)?;
        Candidate { expect, producer_args, llc_args: llc_args.join(" "), modules }.checked()
    }

    fn capture_torture(
        &self,
        test: &torture::TortureTest,
        job: &Path,
        clang: &Path,
        execute: &Path,
        driver: &Path,
    ) -> Result<Candidate, Unfit> {
        let broken = |what: String| Unfit::new("capture-broken", what);
        if let Some(unfit) = &test.unfit {
            return Err(unfit.clone());
        }
        let source = fs::read(&test.path).map_err(|e| broken(io("read", &test.path, &e)))?;
        let x_file = fs::read_to_string(test.path.with_extension("x")).ok();
        let flags = torture::flags(
            &test.name,
            &String::from_utf8_lossy(&source),
            x_file.as_deref(),
            self.host,
        )?;
        let tool_limits = self.options.tool_limits;
        let step = |tool: &Path,
                    name: &str,
                    reason: &'static str,
                    args: &[&std::ffi::OsStr],
                    made: &Path| {
            let configure = |command: &mut std::process::Command| {
                command.args(args).current_dir(execute).env("LC_ALL", "C");
            };
            let limits =
                if reason == "clang-rejects" { self.options.compile_limits } else { tool_limits };
            let output = process::run(tool, configure, b"", limits).map_err(broken)?;
            if output.end == End::Exit(0) && made.is_file() {
                Ok(())
            } else {
                Err(Unfit::new(reason, self.tidy(&failure(name, &output), job)))
            }
        };
        // The test, and the files it is built with: each a module.
        let mut files: Vec<(Option<&str>, &str)> = vec![(None, &test.name)];
        if !test.with.is_empty() {
            files[0].0 = Some(torture::TEST_MODULE);
            files.extend(test.with.iter().map(|(name, file)| (Some(*name), file.as_str())));
        }
        // The program the expectations are read from is the oracle's.
        let llc_args = kiln::harness_llc_args(true);
        let (mut commands, mut modules, mut objects) = (Vec::new(), Vec::new(), Vec::new());
        for (index, (name, file)) in files.iter().enumerate() {
            let module = job.join(format!("module-{index}.ll"));
            let object = job.join(format!("module-{index}.o"));
            let clang_args = torture::clang_args(file, &flags);
            let mut args: Vec<&std::ffi::OsStr> = clang_args.iter().map(AsRef::as_ref).collect();
            args.insert(args.len() - 1, "-o".as_ref());
            args.insert(args.len() - 1, module.as_ref());
            step(clang, "clang", "clang-rejects", &args, &module)?;
            let mut args: Vec<&std::ffi::OsStr> = llc_args.iter().map(AsRef::as_ref).collect();
            args.extend(engine::REFERENCE_SELECTOR.iter().map(AsRef::<std::ffi::OsStr>::as_ref));
            args.extend(["-o".as_ref(), object.as_os_str(), module.as_os_str()]);
            step(&self.llvm.llc, "llc", "llc-fails", &args, &object)?;
            let bytes = fs::read(&module).map_err(|e| broken(io("read", &module, &e)))?;
            modules.push(Module {
                name: name.map(str::to_owned),
                text: bytes,
                delivered: Delivered::Text,
            });
            commands.push(clang_args.join(" "));
            objects.push(object);
        }
        let program = job.join("program");
        let mut args: Vec<&std::ffi::OsStr> =
            vec!["-pie".as_ref(), "-o".as_ref(), program.as_ref()];
        args.extend(objects.iter().map(AsRef::<std::ffi::OsStr>::as_ref));
        args.push("-lm".as_ref());
        step(driver, "link", "link-fails", &args, &program)?;

        let ran = self.run_program(&program, &[], b"", job).map_err(broken)?;
        let recipe = Recipe {
            covers: format!("{TORTURE} {}", test.name),
            arch: self.host,
            llvm: self.llvm.major,
            libs: vec![Lib::M],
            pie: true,
            args: Vec::new(),
            stdin: Vec::new(),
            llc_args: Vec::new(),
        };
        let expect = entry::expectations(&recipe, &ran)?;
        if ran.end != End::Exit(0) {
            // The suite's tests report failure by `abort` or a status.
            return Err(Unfit::new("program-fails", ended(ran.end)));
        }
        let (producer_args, llc_args) = (commands.join(" && "), llc_args.join(" "));
        Candidate { expect, producer_args, llc_args, modules }.checked()
    }

    fn run_program(
        &self,
        program: &Path,
        args: &[String],
        stdin: &[u8],
        job: &Path,
    ) -> Result<Output, String> {
        let configure = |command: &mut std::process::Command| {
            command.args(args).current_dir(job.join("run"));
        };
        process::run(program, configure, stdin, self.options.run_limits)
    }
}

/// How a program ended, for a skip's detail.
fn ended(end: End) -> String {
    match end {
        End::Exit(code) => format!("exit status {code}"),
        End::Signal(signal) => format!("signal {signal}"),
        End::TimedOut => "a timeout".to_owned(),
    }
}

/// Runs `work` on every index below `count`, `jobs` at a time; the results
/// are in index order.
fn in_parallel<T: Send>(count: usize, jobs: usize, work: impl Fn(usize) -> T + Sync) -> Vec<T> {
    let next = AtomicUsize::new(0);
    let slots: Mutex<Vec<Option<T>>> = Mutex::new((0..count).map(|_| None).collect());
    std::thread::scope(|scope| {
        for _ in 0..jobs.clamp(1, count.max(1)) {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= count {
                        break;
                    }
                    let result = work(index);
                    slots.lock().unwrap_or_else(PoisonError::into_inner)[index] = Some(result);
                }
            });
        }
    });
    let slots = slots.into_inner().unwrap_or_else(PoisonError::into_inner);
    slots.into_iter().map(|slot| slot.expect("every index was worked on")).collect()
}

/// The key of a test: a hash of everything its entry is captured from that
/// this command can see.
fn key_of(parts: &[&str]) -> String {
    let mut text = String::from(FORMAT);
    for part in parts {
        text.push('\0');
        text.push_str(part);
    }
    sha256::hex(text.as_bytes())
}

/// The tools every group needs.
struct Common {
    host: Arch,
    driver: PathBuf,
    linkers: Vec<Linker>,
    decline: PathBuf,
    run: PathBuf,
}

/// Why a candidate is not admitted.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Refused {
    detail: String,
    /// The oracle's own side passed and its witness did not confirm it.
    unconfirmed: bool,
    /// The oracle's objects did not link: what a link that tolerates
    /// warnings may get past.
    link: bool,
}

impl Refused {
    /// The skip reason.
    fn reason(&self) -> &'static str {
        if self.unconfirmed { "oracle-unconfirmed" } else { "not-reproducible" }
    }
}

/// Whether the oracle's side of a test passed with every linker, and its
/// witness agreed: the records are those of a run with the witness in
/// which hod declined.
fn admitted(records: &[Record], linkers: usize) -> Result<(), Refused> {
    let passed = |record: &Record| {
        record.outcome == Outcome::Fail
            && record.built_by == BuiltBy::Declined
            && record.reason.as_deref() == Some(ADMISSION)
            && record.linker.is_some()
    };
    match records.iter().find(|record| !passed(record)) {
        Some(record) => {
            // Output that differs from run to run (an address, a time) is
            // not quoted: the table is the same after every capture.
            const DIFFERS: [&str; 2] = ["differs from the directives", "differs from llc -O0's"];
            let cut = DIFFERS
                .iter()
                .find_map(|differs| record.detail.find(differs).map(|at| at + differs.len()));
            let detail = &record.detail[..cut.unwrap_or(record.detail.len())];
            let error = record.outcome == Outcome::Error;
            Err(Refused {
                detail: format!(
                    "{}{detail}",
                    record
                        .linker
                        .as_ref()
                        .map_or_else(String::new, |linker| format!("with {linker}: ")),
                ),
                unconfirmed: error && record.detail.starts_with(engine::WITNESS),
                link: error && record.detail.starts_with("oracle: link with "),
            })
        }
        None if records.len() != linkers => Err(Refused {
            detail: format!("{} results for {linkers} linkers", records.len()),
            unconfirmed: false,
            link: false,
        }),
        None => Ok(()),
    }
}

/// The detail of a candidate that was not admitted, as a table holds it.
/// `irdiff` cuts its own details to a length, so one that names a path of
/// the run ends at a place that depends on the name of the run's
/// directory: the detail is cut before the first path under the work
/// directory (the run's parent) instead.
fn admission_detail(detail: &str, run: &Path) -> String {
    let work = run.parent().unwrap_or(run).to_string_lossy();
    let stable = detail.find(&*work).map_or(detail, |at| &detail[..at]);
    tidy(stable.trim_end_matches([' ', ':']), &[])
}

/// Removes the entry file `entry` of the corpus `out`, and the directories
/// it leaves empty.
fn remove_entry(out: &Path, entry: &str) {
    let path = out.join(entry);
    let _ = fs::remove_file(&path);
    let mut dir = path.parent();
    while let Some(current) = dir.filter(|dir| dir.starts_with(out) && *dir != out) {
        if fs::remove_dir(current).is_err() {
            break;
        }
        dir = current.parent();
    }
}

/// Captures one group into the corpus and the two tables.
#[expect(
    clippy::too_many_lines,
    reason = "one case after another; splitting it would not make it clearer"
)]
fn capture_group(
    group: &Group<'_>,
    common: &Common,
    found: Vec<(String, PathBuf, Work)>,
    revision: &str,
    producer: &str,
    manifest: &mut Manifest,
    skips: &mut Skips,
) -> Result<GroupStats, String> {
    let started = Instant::now();
    let options = group.options;
    let (source, major) = (group.source, group.llvm.major);
    let label = format!("{source}/llvm{major}");
    let mut stats = GroupStats { source: source.to_owned(), llvm: major, ..GroupStats::default() };
    let llvm_version = version_line(&group.llvm.llc, "LLVM version");

    // What has to be done: a test whose entry or skip was made from the
    // same inputs is left alone.
    let limited: Vec<(String, PathBuf, Work)> =
        found.into_iter().take(options.limit.unwrap_or(usize::MAX)).collect();
    let seen: BTreeSet<&str> = limited.iter().map(|(test, _, _)| test.as_str()).collect();
    if options.limit.is_none() {
        // Entries and skips of tests that are no longer in the suite.
        let stale: Vec<String> = manifest
            .rows
            .values()
            .filter(|row| row.source == source && row.llvm == major)
            .filter(|row| !seen.contains(row.test.as_str()))
            .map(|row| row.entry.clone())
            .collect();
        for entry in stale {
            remove_entry(&options.out, &entry);
            manifest.rows.remove(&entry);
            stats.removed += 1;
        }
        skips.rows.retain(|(of, llvm, test), _| {
            !(of == source && *llvm == major) || seen.contains(test.as_str())
        });
    }
    stats.tests = limited.len();
    let mut todo = Vec::new();
    for (test, path, work) in limited {
        let content = fs::read(&path).map_err(|e| io("read", &path, &e))?;
        let key = key_of(&[
            source,
            &major.to_string(),
            &llvm_version,
            producer,
            revision,
            &test,
            &sha256::hex(&content),
        ]);
        let slot = (source.to_owned(), major, test.clone());
        if !options.force {
            let entry = entry_path(source, major, &test);
            let stands = manifest.rows.get(&entry).is_some_and(|row| {
                row.key == key
                    && fs::read(options.out.join(&entry))
                        .is_ok_and(|bytes| sha256::hex(&bytes) == row.sha256)
            });
            if stands {
                stats.reused += 1;
                continue;
            }
            if let Some(skip) = skips.rows.get(&slot).filter(|skip| skip.key == key)
                && !options.retry_skipped
                && !manifest.rows.contains_key(&entry)
            {
                *stats.skipped.entry(skip.reason.clone()).or_default() += 1;
                continue;
            }
        }
        todo.push(Item { test, key, work });
    }

    // Capture.
    let done = AtomicUsize::new(0);
    let jobs = options.jobs.clamp(1, MAX_JOBS);
    let total = todo.len();
    let captured = in_parallel(total, jobs, |index| {
        let job = group.run.join(format!("job-{source}-llvm{major}-{index}"));
        let result = group.capture(&todo[index], &job);
        let _ = fs::remove_dir_all(&job);
        let count = done.fetch_add(1, Ordering::Relaxed) + 1;
        if count.is_multiple_of(200) && count < total {
            eprintln!("corpus: {label}: {count} of {total} tests captured");
        }
        result
    });

    // Admit: the oracle's side of every candidate, by irdiff's own code.
    let stage = group.run.join("stage");
    let session = Session {
        toolchain: Toolchain {
            llc: group.llvm.llc.clone(),
            llvm_major: major,
            hod: common.decline.clone(),
            driver: Some(common.driver.clone()),
            linkers: common.linkers.clone(),
        },
        arch: common.host,
        host: Some(common.host),
        mode: Mode::Strict,
        tool_limits: options.tool_limits,
        run_limits: options.run_limits,
        work: group.run.join(format!("admit-{source}-llvm{major}")),
        keep: false,
        // An entry is a program the oracle is sure of ("Admission").
        witness: true,
    };
    let mut captured = captured;
    let admit = |captured: &[Result<Candidate, Unfit>],
                 which: &[usize]|
     -> Result<BTreeMap<usize, Result<(), Refused>>, String> {
        let mut tests = Vec::new();
        for index in which {
            let Ok(candidate) = &captured[*index] else { continue };
            let Ok(text) = candidate.text() else { continue };
            let entry = entry_path(source, major, &todo[*index].test);
            let path = stage.join(&entry);
            write_file(&path, text.as_bytes(), 0o644)?;
            tests.push((*index, Test { name: entry, ir: path, expect: candidate.expect.clone() }));
        }
        let run: Vec<Test> = tests.iter().map(|(_, test)| test.clone()).collect();
        let records = session.run_tests(&run, jobs)?;
        let _ = fs::remove_dir_all(&session.work);
        Ok(tests
            .iter()
            .zip(&records)
            .map(|((index, _), records)| (*index, admitted(records, common.linkers.len())))
            .collect())
    };
    let all: Vec<usize> = (0..captured.len()).filter(|index| captured[*index].is_ok()).collect();
    if !all.is_empty() {
        eprintln!("corpus: {label}: admitting {} of {total} captured tests", all.len());
    }
    let mut verdicts = admit(&captured, &all)?;
    // A candidate whose objects do not link is tried once more with a
    // link that tolerates warnings; if that is what it takes, the entry
    // says so (`link: allow-warnings`).
    let warned: Vec<usize> = verdicts
        .iter()
        .filter(|(_, verdict)| verdict.as_ref().is_err_and(|refused| refused.link))
        .map(|(index, _)| *index)
        .collect();
    if !warned.is_empty() {
        for index in &warned {
            if let Ok(candidate) = &mut captured[*index] {
                candidate.expect.allow_warnings = true;
            }
        }
        for (index, verdict) in admit(&captured, &warned)? {
            match (verdict, &mut captured[index]) {
                (Ok(()), _) => _ = verdicts.insert(index, Ok(())),
                // The first verdict stands.
                (Err(_), Ok(candidate)) => candidate.expect.allow_warnings = false,
                (Err(_), Err(_)) => {}
            }
        }
    }
    let _ = fs::remove_dir_all(&stage);

    // Commit.
    for (index, (item, result)) in todo.iter().zip(captured).enumerate() {
        let entry = entry_path(source, major, &item.test);
        let slot = (source.to_owned(), major, item.test.clone());
        let result = result.and_then(|candidate| match verdicts.remove(&index) {
            Some(Ok(())) => candidate.text().map(|text| (candidate, text)),
            Some(Err(refused)) => {
                Err(Unfit::new(refused.reason(), admission_detail(&refused.detail, &group.run)))
            }
            None => Err(Unfit::new("capture-broken", "the candidate was not admitted")),
        });
        match result {
            Ok((candidate, text)) => {
                write_file(&options.out.join(&entry), text.as_bytes(), 0o644)?;
                let row = Row {
                    entry: entry.clone(),
                    source: source.to_owned(),
                    llvm: major,
                    test: item.test.clone(),
                    revision: revision.to_owned(),
                    producer: producer.to_owned(),
                    producer_args: candidate.producer_args,
                    llc_args: candidate.llc_args,
                    delivered: entry::delivered(&candidate.modules),
                    sha256: sha256::hex(text.as_bytes()),
                    bytes: text.len() as u64,
                    key: item.key.clone(),
                };
                manifest.rows.insert(entry, row);
                skips.rows.remove(&slot);
                stats.captured += 1;
            }
            Err(unfit) => {
                if manifest.rows.remove(&entry).is_some() {
                    remove_entry(&options.out, &entry);
                    stats.removed += 1;
                }
                *stats.skipped.entry(unfit.reason.to_owned()).or_default() += 1;
                let skip = Skip {
                    source: source.to_owned(),
                    llvm: major,
                    test: item.test.clone(),
                    reason: unfit.reason.to_owned(),
                    detail: unfit.detail,
                    key: item.key.clone(),
                };
                skips.rows.insert(slot, skip);
            }
        }
    }
    manifest.write(&options.out)?;
    skips.write(&options.out)?;
    stats.seconds = started.elapsed().as_secs_f64();
    // Only a capture of the whole group from nothing is timed: how long
    // an incremental one takes says nothing about the corpus.
    if total > 0 && stats.reused == 0 && options.limit.is_none() {
        record_run(&options.out, &stats, jobs)?;
    }
    Ok(stats)
}

/// One row of [`RUNS`]: what the last capture that made a whole group
/// anew (nothing reused, no `--limit`) did, and how long it took.
#[derive(Clone, Debug, PartialEq)]
pub struct RunRow {
    pub source: String,
    pub llvm: u32,
    pub tests: usize,
    pub captured: usize,
    pub reused: usize,
    pub skipped: usize,
    pub seconds: f64,
    pub jobs: usize,
}

/// Reads `<dir>/CAPTURE.tsv`; a missing or damaged file is no rows.
pub fn read_runs(dir: &Path) -> Vec<RunRow> {
    let text = fs::read_to_string(dir.join(RUNS)).unwrap_or_default();
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            let [source, llvm, tests, captured, reused, skipped, seconds, jobs] = fields[..] else {
                return None;
            };
            Some(RunRow {
                source: source.to_owned(),
                llvm: llvm.parse().ok()?,
                tests: tests.parse().ok()?,
                captured: captured.parse().ok()?,
                reused: reused.parse().ok()?,
                skipped: skipped.parse().ok()?,
                seconds: seconds.parse().ok()?,
                jobs: jobs.parse().ok()?,
            })
        })
        .collect()
}

fn record_run(out: &Path, stats: &GroupStats, jobs: usize) -> Result<(), String> {
    let mut rows = read_runs(out);
    rows.retain(|row| !(row.source == stats.source && row.llvm == stats.llvm));
    rows.push(RunRow {
        source: stats.source.clone(),
        llvm: stats.llvm,
        tests: stats.tests,
        captured: stats.captured,
        reused: stats.reused,
        skipped: stats.skipped.values().sum(),
        seconds: stats.seconds,
        jobs,
    });
    rows.sort_by(|a, b| (&a.source, a.llvm).cmp(&(&b.source, b.llvm)));
    let mut text = format!("{RUNS_HEADER}\n");
    for row in rows {
        let _ = writeln!(
            text,
            "{}\t{}\t{}\t{}\t{}\t{}\t{:.1}\t{}",
            row.source,
            row.llvm,
            row.tests,
            row.captured,
            row.reused,
            row.skipped,
            row.seconds,
            row.jobs
        );
    }
    write_file(&out.join(RUNS), text.as_bytes(), 0o644)
}

/// The LLVM installation directory an `llc` belongs to: the parent of the
/// `bin` directory its resolved path is in.
fn installation(llc: &Path) -> Option<PathBuf> {
    let resolved = fs::canonicalize(llc).ok()?;
    Some(resolved.parent()?.parent()?.to_path_buf())
}

/// The kiln tests that are run (`expect: run`), with their headers.
fn kiln_tests(repo: &Path) -> Result<Vec<(String, PathBuf, Work)>, String> {
    let mut found = Vec::new();
    for test in kiln::discover(repo)? {
        let source = fs::read(&test.path).map_err(|e| io("read", &test.path, &e))?;
        let header = kiln::parse_header(&String::from_utf8_lossy(&source));
        if header.expect.as_deref() == Some("run") {
            found.push((test.name.clone(), test.path.clone(), Work::Kiln(test, Box::new(header))));
        }
    }
    Ok(found)
}

/// Runs the command. An error is a request that cannot be carried out at
/// all; everything else is in the report.
#[expect(
    clippy::too_many_lines,
    reason = "one case after another; splitting it would not make it clearer"
)]
pub fn capture(options: &Options) -> Result<CaptureReport, String> {
    let mut report = CaptureReport::default();
    let host = Arch::host().ok_or(
        "this host is not a Linux machine of a supported architecture; a capture runs programs",
    )?;
    let out = &options.out;
    fs::create_dir_all(out).map_err(|e| io("create", out, &e))?;
    let mut manifest = Manifest::read(out)?;
    let mut skips = Skips::read(out)?;
    let totals = |report: &mut CaptureReport, manifest: &Manifest| {
        report.entries = manifest.rows.len();
        report.bytes = manifest.rows.values().map(|row| row.bytes).sum();
    };
    let missing = |report: &mut CaptureReport, what: String, required: bool| {
        if required && options.require_tools {
            report.missing_tools.push(what);
        } else {
            report.notes.push(what);
        }
    };

    // The oracles.
    let wanted: Vec<u32> =
        if options.llvm.is_empty() { tools::LLVM_MAJORS.to_vec() } else { options.llvm.clone() };
    let mut llvms = Vec::new();
    for major in wanted {
        match tools::find_llvm(Some(&major.to_string()), &options.search)? {
            Some(llvm) => llvms.push(llvm),
            None => missing(
                &mut report,
                format!("LLVM {major} (llc) not found; nothing is captured with it"),
                true,
            ),
        }
    }
    // The link: what admission needs, and so what every entry needs.
    let driver = tools::find_driver(&options.search);
    let mut linkers = Vec::new();
    for name in LINKER_NAMES {
        match tools::find_linker(name, &options.search) {
            Some(path) => linkers.push(Linker { name: name.to_owned(), path }),
            None => missing(
                &mut report,
                format!("linker '{name}' not found; entries are not admitted with it"),
                tools::REQUIRED_LINKERS.contains(&name),
            ),
        }
    }
    let Some(driver) = driver.filter(|_| !linkers.is_empty()) else {
        missing(
            &mut report,
            "no linker driver (gcc) or no linker found; nothing is captured".to_owned(),
            true,
        );
        totals(&mut report, &manifest);
        return Ok(report);
    };

    let work = work_dir(out);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let run = work.join(format!("run-{}-{stamp}", std::process::id()));
    fs::create_dir_all(&run).map_err(|e| io("create", &run, &e))?;
    let decline = work.join("decline-hod");
    write_file(&decline, DECLINE_SCRIPT.as_bytes(), 0o755)?;
    let common = Common { host, driver, linkers, decline, run: run.clone() };

    let result = (|| -> Result<(), String> {
        for source in SOURCES {
            if !options.sources.contains(&source) {
                continue;
            }
            // Whether the source is there at all.
            let (suite, revision) = if source == KILN {
                if !options.kiln.is_file() {
                    report.notes.push(format!(
                        "no kiln++ at '{}' (--kiln, $HOD_KILN); the kiln source is skipped",
                        options.kiln.display()
                    ));
                    continue;
                }
                if !options.kiln_repo.join("tests").is_dir() {
                    report.notes.push(format!(
                        "no kiln repository at '{}' (--kiln-repo, $HOD_KILN_REPO); the kiln source is skipped",
                        options.kiln_repo.display()
                    ));
                    continue;
                }
                let repo = fs::canonicalize(&options.kiln_repo)
                    .map_err(|e| io("resolve", &options.kiln_repo, &e))?;
                let revision = revision(&repo, &options.search);
                (repo, revision)
            } else {
                let execute = options.torture.join(torture::EXECUTE_DIR);
                if !execute.is_dir() {
                    report.notes.push(format!(
                        "no GCC torture suite at '{}' (--torture, $HOD_GCC_TESTS; `cargo xtask corpus fetch` gets it); the gcc-torture source is skipped",
                        options.torture.display()
                    ));
                    continue;
                }
                let revision = revision(&options.torture, &options.search);
                (fs::canonicalize(&execute).map_err(|e| io("resolve", &execute, &e))?, revision)
            };
            for llvm in &llvms {
                let major = llvm.major;
                let (producer, version, found) = if source == KILN {
                    let Some(installed) = installation(&llvm.llc) else {
                        report.notes.push(format!(
                            "'{}' is not in an LLVM installation; kiln is not captured with LLVM {major}",
                            llvm.llc.display()
                        ));
                        continue;
                    };
                    let script = work.join("capture-llc");
                    write_file(&script, kiln::CAPTURE_SCRIPT.as_bytes(), 0o755)?;
                    let request = shim::Request {
                        llvm: installed,
                        hod: script,
                        out: work.join(format!("llvm-{major}")),
                        kiln: None,
                    };
                    let llvm_dir = match shim::build(&request) {
                        Ok(shim) => shim.dir,
                        Err(why) => {
                            report
                                .notes
                                .push(format!("kiln is not captured with LLVM {major}: {why}"));
                            continue;
                        }
                    };
                    let root = work.join("kiln-root");
                    let cache = work.join("cache");
                    for dir in [&root, &cache] {
                        fs::create_dir_all(dir).map_err(|e| io("create", dir, &e))?;
                    }
                    for name in ["tests", "resource"] {
                        if suite.join(name).exists() {
                            link_to(&suite.join(name), &root.join(name))?;
                        }
                    }
                    // By the name it was given: `kiln++` is a link to
                    // `kiln`, and the name is what makes it a C++ driver.
                    let kiln = std::path::absolute(&options.kiln)
                        .map_err(|e| io("resolve", &options.kiln, &e))?;
                    let version = version_line(&kiln, "");
                    (Producer::Kiln { kiln, root, llvm_dir, cache }, version, kiln_tests(&suite)?)
                } else {
                    let Some(clang) = options.search.versioned("clang", major) else {
                        missing(
                            &mut report,
                            format!("clang {major} not found; gcc-torture is not captured with it"),
                            true,
                        );
                        continue;
                    };
                    let version = version_line(&clang, "");
                    let found = torture::discover(&suite)?
                        .into_iter()
                        .map(|test| (test.name.clone(), test.path.clone(), Work::Torture(test)))
                        .collect();
                    let producer = Producer::Torture {
                        clang,
                        execute: suite.clone(),
                        driver: common.driver.clone(),
                    };
                    (producer, version, found)
                };
                // What writes a module kiln gave llc as bitcode as text.
                let llvm_dis = options.search.versioned("llvm-dis", major);
                if source == KILN && llvm_dis.is_none() {
                    missing(
                        &mut report,
                        format!(
                            "llvm-dis {major} not found; kiln's modules that reach llc as bitcode are not captured with it"
                        ),
                        true,
                    );
                }
                let group = Group {
                    options,
                    source,
                    llvm,
                    host,
                    producer,
                    llvm_dis,
                    run: common.run.clone(),
                };
                let stats = capture_group(
                    &group,
                    &common,
                    found,
                    &revision,
                    &version,
                    &mut manifest,
                    &mut skips,
                )?;
                report.groups.push(stats);
            }
        }
        Ok(())
    })();
    let _ = fs::remove_dir_all(&run);
    result?;
    // The tables exist after any capture, even one that found nothing.
    manifest.write(out)?;
    skips.write(out)?;
    totals(&mut report, &manifest);
    Ok(report)
}

/// The names of both tables, for callers that list a corpus's own files.
pub const TABLES: [&str; 3] = [manifest::MANIFEST, manifest::SKIPPED, RUNS];

#[cfg(test)]
mod tests {
    use super::*;

    // spec/harness.md "corpus", "Layout".
    #[test]
    fn entry_paths() {
        assert_eq!(
            entry_path(KILN, 22, "tests/conformance/expr.const/a-run.cpp"),
            "kiln/llvm22/conformance/expr.const/a-run.cpp.ll"
        );
        assert_eq!(
            entry_path(TORTURE, 19, "ieee/fp-cmp-1.c"),
            "gcc-torture/llvm19/ieee/fp-cmp-1.c.ll"
        );
        assert_eq!(work_dir(Path::new("/b/corpus")), Path::new("/b/corpus.work"));
    }

    // "Incremental": the key changes with every input and with nothing
    // else, and no two lists of inputs run together.
    #[test]
    fn keys() {
        let key = key_of(&["kiln", "22", "v", "p", "r", "t", "h"]);
        assert_eq!(key.len(), 64);
        assert_eq!(key, key_of(&["kiln", "22", "v", "p", "r", "t", "h"]));
        assert_ne!(key, key_of(&["kiln", "22", "v", "p", "r", "t", "i"]));
        assert_ne!(key_of(&["ab", "c"]), key_of(&["a", "bc"]));
    }

    // "Admission": only a test whose oracle side passed with every linker
    // is an entry.
    #[test]
    fn admission_verdicts() {
        let record =
            |outcome, built_by, reason: Option<&str>, linker: Option<&str>, detail: &str| Record {
                outcome,
                test: "t".to_owned(),
                target: "aarch64-unknown-linux-gnu".to_owned(),
                llvm: Some(22),
                linker: linker.map(str::to_owned),
                built_by,
                reason: reason.map(str::to_owned),
                detail: detail.to_owned(),
                covers: None,
            };
        let declined = |linker| {
            record(Outcome::Fail, BuiltBy::Declined, Some(ADMISSION), Some(linker), "hod declined")
        };
        let admitted = |records: &[Record], linkers| {
            admitted(records, linkers).map_err(|refused| refused.detail)
        };
        assert_eq!(admitted(&[declined("bfd"), declined("lld")], 2), Ok(()));
        assert_eq!(admitted(&[declined("bfd")], 2), Err("1 results for 2 linkers".to_owned()));
        assert!(admitted(&[], 0).is_ok());
        let broken = record(
            Outcome::Error,
            BuiltBy::Declined,
            Some(ADMISSION),
            Some("mold"),
            "oracle: the program of llc's object ended with signal 11",
        );
        assert_eq!(
            admitted(&[declined("bfd"), broken], 2),
            Err("with mold: oracle: the program of llc's object ended with signal 11".to_owned())
        );
        let varies = record(
            Outcome::Error,
            BuiltBy::Declined,
            Some(ADMISSION),
            Some("bfd"),
            "oracle: the program of llc's object wrote stdout that differs from the directives \
             at line 1: expected \"0xffff1\\n\", got \"0xffff2\\n\"",
        );
        assert_eq!(
            admitted(&[varies], 1),
            Err("with bfd: oracle: the program of llc's object wrote stdout that differs from \
                 the directives"
                .to_owned())
        );
        // A detail that `irdiff` cut in the middle of a path of the run.
        let run = Path::new("/w/c.work/run-123-4");
        for cut in [
            "/w/c.work/run-123-4/admit/linkers/bfd/ld: /w/c.work/run-123-4/a...",
            "/w/c.work/run-12...",
        ] {
            let detail = format!("with bfd: oracle: link failed (exit status 1): {cut}");
            assert_eq!(
                admission_detail(&detail, run),
                "with bfd: oracle: link failed (exit status 1)"
            );
        }
        assert_eq!(
            admission_detail("with mold: oracle: mold: error: x", run),
            "with mold: oracle: mold: error: x"
        );
        let no_object = record(Outcome::Error, BuiltBy::Nobody, None, None, "oracle: llc failed");
        assert_eq!(admitted(&[no_object], 1), Err("oracle: llc failed".to_owned()));
        // "Skip reasons": an oracle its witness does not confirm, and a
        // link that a warning may be all that is wrong with.
        let error = |linker, detail| {
            record(Outcome::Error, BuiltBy::Declined, Some(ADMISSION), Some(linker), detail)
        };
        let unconfirmed = super::admitted(
            &[error(
                "bfd",
                "witness: the program of llc -O2's object wrote stdout that differs from \
                 llc -O0's at line 2: expected \"1\\n\", got \"2\\n\"",
            )],
            1,
        )
        .unwrap_err();
        assert_eq!(
            unconfirmed.detail,
            "with bfd: witness: the program of llc -O2's object wrote stdout that differs from \
             llc -O0's"
        );
        assert_eq!((unconfirmed.reason(), unconfirmed.link), ("oracle-unconfirmed", false));
        let link = super::admitted(
            &[
                error("bfd", "oracle: link with bfd failed (exit status 1): warning"),
                declined("lld"),
            ],
            2,
        )
        .unwrap_err();
        assert_eq!((link.reason(), link.link, link.unconfirmed), ("not-reproducible", true, false));
        let run = super::admitted(&[error("bfd", "oracle: the program of llc's object x")], 1);
        let run = run.unwrap_err();
        assert_eq!((run.reason(), run.link), ("not-reproducible", false));
        // A record the stand-in did not cause is not an admission.
        let other = record(Outcome::Fail, BuiltBy::Declined, Some("other"), Some("bfd"), "x");
        assert!(admitted(&[other], 1).is_err());
        let passed = record(Outcome::Pass, BuiltBy::Hod, None, Some("bfd"), "");
        assert!(admitted(&[passed], 1).is_err());
        // The stand-in's log line is one hod could have written.
        assert!(DECLINE_SCRIPT.contains("printf 'error\\tcorpus-admission\\t"));
        assert_eq!(DECLINE_SCRIPT.matches("\\t").count(), 7);
    }

    // "Skip reasons": details hold no path of the run and are one line.
    #[test]
    fn details_are_tidied() {
        let job = Path::new("/w/run-1-2/job-kiln-llvm22-7");
        let run = Path::new("/w/run-1-2");
        let text = "\n  /w/run-1-2/job-kiln-llvm22-7/tmp/a.o: error in /w/run-1-2/stage\nmore\n";
        assert_eq!(
            tidy(text, &[(job, "<job>"), (run, "<run>")]),
            "<job>/tmp/a.o: error in <run>/stage"
        );
        assert_eq!(tidy(&"é".repeat(400), &[]).len(), 300);
        assert_eq!(tidy("", &[]), "");
        let output = Output {
            end: End::Exit(1),
            stdout: Vec::new(),
            stderr: b"In file included from a.c:1:\na.c:3:1: error: bad\n".to_vec(),
            truncated: false,
        };
        assert_eq!(failure("clang", &output), "clang: exit status 1: a.c:3:1: error: bad");
        let silent = Output { end: End::TimedOut, stderr: Vec::new(), ..output };
        assert_eq!(failure("llc", &silent), "llc: timed out");
    }
}
