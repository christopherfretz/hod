//! `cargo xtask compare-binaries`: two hod binaries, object by object
//! (WP-272). Contract: `spec/harness.md` "compare-binaries".
//!
//! Every module of the corpus directories given (`--corpus`, each module of
//! an entry on its own, in two modes: plain and `-relocation-model=pic`),
//! and every module a lane kept (`--modules`, with the arguments it was
//! really compiled with), is compiled by both binaries with the same
//! arguments and the same input on standard input, and classified:
//!
//! - `identical`: both succeed and the objects are byte-equal;
//! - `different`: both succeed and the objects differ (the sections whose
//!   bytes differ are named);
//! - `changed-outcome`: one succeeds and the other does not, or both fail
//!   with another exit status or another diagnostic;
//! - `identical-failure`: both fail with the same exit status and the same
//!   diagnostic.
//!
//! A module the harness cannot read or a child it cannot start is a
//! `harness-error`. The run passes only when something was compared and
//! nothing is `different`, `changed-outcome` or `harness-error`.

pub mod elf;
pub mod kept;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use crate::irdiff::directives::{self, Arch};
use crate::irdiff::process::{self, End, Limits};
use crate::irdiff::{self, MAX_JOBS};

pub const USAGE: &str = "\
usage: cargo xtask compare-binaries --base <hod> --head <hod>
           [--corpus <dir>]... [--modules <dir>]... [--out <absolute dir>]
           [-j <n>] [--timeout <seconds>]

  --corpus <dir>    every .ll entry under <dir> (a corpus, a part of one, or one entry):
                    each module, plain and -relocation-model=pic
  --modules <dir>   every MODULES.tsv under <dir> (kept by a lane with
                    HOD_OSS_KEEP_MODULES): each module with its recorded arguments
  --out <dir>       summary.md, results.tsv, differences.tsv (default
                    <build dir>/compare-binaries)
  -j <n>            parallel modules (default 4, at most 16; each child holds
                    a jobserver token)
  --timeout <s>     per child (default 600)
";

/// A run of the command.
#[derive(Clone, Debug)]
pub struct Options {
    pub base: Option<PathBuf>,
    pub head: Option<PathBuf>,
    pub corpora: Vec<PathBuf>,
    pub modules: Vec<PathBuf>,
    pub out: PathBuf,
    pub jobs: usize,
    pub limits: Limits,
}

impl Options {
    /// The defaults, with output under `build_dir`.
    pub fn new(build_dir: &Path) -> Self {
        let defaults = irdiff::Config::new(build_dir.to_path_buf());
        Self {
            base: None,
            head: None,
            corpora: Vec::new(),
            modules: Vec::new(),
            out: build_dir.join("compare-binaries"),
            jobs: defaults.jobs,
            limits: Limits { time: Duration::from_mins(10), memory_kib: 8 << 20 },
        }
    }
}

/// Reads the command line. `Ok(false)`: `--help`.
pub fn parse_args(args: &[String], options: &mut Options) -> Result<bool, String> {
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let (name, joined) = match arg.split_once('=') {
            Some((name, value)) if name.starts_with("--") => (name, Some(value.to_owned())),
            _ => match arg.strip_prefix("-j").filter(|n| !n.is_empty() && !arg.starts_with("--")) {
                Some(number) => ("-j", Some(number.to_owned())),
                None => (arg.as_str(), None),
            },
        };
        let mut value = || {
            joined
                .clone()
                .or_else(|| rest.next().cloned())
                .filter(|v| !v.is_empty())
                .ok_or_else(|| format!("'{name}' requires a value"))
        };
        let number = |text: String, what: &str| {
            text.parse::<u64>()
                .ok()
                .filter(|n| *n >= 1)
                .ok_or_else(|| format!("invalid {what} '{text}'"))
        };
        match name {
            "-h" | "--help" => return Ok(false),
            "--base" => options.base = Some(PathBuf::from(value()?)),
            "--head" => options.head = Some(PathBuf::from(value()?)),
            "--corpus" => options.corpora.push(PathBuf::from(value()?)),
            "--modules" => options.modules.push(PathBuf::from(value()?)),
            "--out" => {
                let out = PathBuf::from(value()?);
                if !out.is_absolute() {
                    return Err(format!("--out '{}' is not an absolute path", out.display()));
                }
                options.out = out;
            }
            "-j" | "--jobs" => {
                let jobs = number(value()?, "job count")?;
                options.jobs = usize::try_from(jobs).unwrap_or(MAX_JOBS).min(MAX_JOBS);
            }
            "--timeout" => options.limits.time = Duration::from_secs(number(value()?, "timeout")?),
            _ => return Err(format!("unknown argument '{arg}'")),
        }
    }
    if options.base.is_none() || options.head.is_none() {
        return Err("--base and --head are both required".to_owned());
    }
    if options.corpora.is_empty() && options.modules.is_empty() {
        return Err("nothing to compare: give --corpus or --modules".to_owned());
    }
    Ok(true)
}

/// What became of one module in one mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Class {
    Identical,
    Different,
    ChangedOutcome,
    IdenticalFailure,
    HarnessError,
}

impl Class {
    pub const ALL: [Self; 5] = [
        Self::Identical,
        Self::Different,
        Self::ChangedOutcome,
        Self::IdenticalFailure,
        Self::HarnessError,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Identical => "identical",
            Self::Different => "different",
            Self::ChangedOutcome => "changed-outcome",
            Self::IdenticalFailure => "identical-failure",
            Self::HarnessError => "harness-error",
        }
    }

    /// Whether this class fails the run.
    pub fn fails(self) -> bool {
        matches!(self, Self::Different | Self::ChangedOutcome | Self::HarnessError)
    }
}

/// Where a unit's module comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Source {
    /// The `index`-th module of a corpus entry (the whole text for an
    /// entry of one module).
    Entry { path: PathBuf, index: Option<usize> },
    /// A module file kept by a lane.
    Kept { path: PathBuf },
}

/// One module in one mode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unit {
    /// The module: the entry's path (with `#<module>` for one of several),
    /// or the kept module's file.
    pub name: String,
    pub target: String,
    pub mode: String,
    /// hod's arguments; `args[output]` is replaced by the object's path.
    pub args: Vec<String>,
    pub output: usize,
    source: Source,
}

impl Unit {
    fn input(&self) -> Result<Vec<u8>, String> {
        match &self.source {
            Source::Kept { path } => {
                fs::read(path).map_err(|e| format!("cannot read '{}': {e}", path.display()))
            }
            Source::Entry { path, index } => {
                let text = fs::read_to_string(path)
                    .map_err(|e| format!("cannot read '{}': {e}", path.display()))?;
                match index {
                    None => Ok(text.into_bytes()),
                    Some(i) => directives::modules(&text)
                        .into_iter()
                        .nth(*i)
                        .map(|m| m.text.into_bytes())
                        .ok_or_else(|| format!("'{}' has no module {i}", path.display())),
                }
            }
        }
    }
}

/// The architecture a module's `target triple` names (`unknown` without
/// one).
pub fn target_of_text(text: &str) -> String {
    text.lines()
        .find_map(|line| line.trim().strip_prefix("target triple = \""))
        .and_then(|rest| rest.split(['-', '"']).next())
        .map_or_else(|| "unknown".to_owned(), |arch| arch.replace("arm64", "aarch64"))
}

/// Every `.ll` file under `dir`, sorted (`dir` itself when it is a file).
fn ll_files(dir: &Path, found: &mut Vec<PathBuf>) -> Result<(), String> {
    if dir.is_file() {
        found.push(dir.to_path_buf());
        return Ok(());
    }
    let read = fs::read_dir(dir).map_err(|e| format!("cannot read '{}': {e}", dir.display()))?;
    let mut entries: Vec<PathBuf> = read.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            ll_files(&path, found)?;
        } else if path.extension().is_some_and(|e| e == "ll") {
            found.push(path);
        }
    }
    Ok(())
}

/// The plain and the PIC mode of a corpus entry.
pub const CORPUS_MODES: [(&str, Option<&str>); 2] =
    [("plain", None), ("pic", Some("-relocation-model=pic"))];

/// The units of the entries under a corpus directory: each module, in
/// each of [`CORPUS_MODES`], with the entry's target and `llc-args`.
pub fn corpus_units(dir: &Path) -> Result<Vec<Unit>, String> {
    let mut files = Vec::new();
    ll_files(dir, &mut files)?;
    let mut units = Vec::new();
    for path in files {
        let text = fs::read_to_string(&path)
            .map_err(|e| format!("cannot read '{}': {e}", path.display()))?;
        let expect = directives::parse(&text).map_err(|errors| {
            let all: Vec<String> = errors.iter().map(ToString::to_string).collect();
            format!("'{}': directives: {}", path.display(), all.join("; "))
        })?;
        let modules = directives::modules(&text);
        let arch = expect.targets.first().copied().or_else(|| {
            Arch::from_name(&target_of_text(modules.first().map_or(&text, |m| &m.text)))
        });
        // WP-292: an entry that names no target (a `tests/ir` seed without
        // a triple, which irdiff builds for every target) is compiled once
        // per target hod compiles for: without `-mtriple` both binaries
        // refuse it alike, and the comparison would prove nothing.
        let arches = match arch {
            Some(arch) => vec![arch],
            None => Arch::compiled(),
        };
        let parts: Vec<(Option<usize>, String)> = if modules.is_empty() {
            vec![(None, path.display().to_string())]
        } else {
            modules
                .iter()
                .enumerate()
                .map(|(i, m)| (Some(i), format!("{}#{}", path.display(), m.name)))
                .collect()
        };
        for (arch, (mode, extra)) in
            arches.iter().flat_map(|&arch| CORPUS_MODES.map(|mode| (arch, mode)))
        {
            for (index, name) in &parts {
                let mut args: Vec<String> =
                    ["--strict", "-O0", "-filetype=obj"].map(String::from).to_vec();
                args.push(format!("-mtriple={}", arch.triple()));
                args.extend(extra.map(String::from));
                args.extend(expect.llc_args.iter().cloned());
                args.push("-o".to_owned());
                let output = args.len();
                args.push(String::new());
                args.push("-".to_owned());
                units.push(Unit {
                    name: name.clone(),
                    target: arch.name().to_owned(),
                    mode: mode.to_owned(),
                    args,
                    output,
                    source: Source::Entry { path: path.clone(), index: *index },
                });
            }
        }
    }
    Ok(units)
}

/// The units of the modules kept under `dir` (every `MODULES.tsv`).
pub fn kept_units(dir: &Path) -> Result<Vec<Unit>, String> {
    let mut manifests = Vec::new();
    kept::manifests(dir, &mut manifests)?;
    let mut units = Vec::new();
    for manifest in manifests {
        let here = manifest.parent().unwrap_or(dir);
        for row in kept::read_manifest(&manifest)? {
            let path = here.join(format!("{}.ll", row.hash));
            let (args, output) = kept::replay_args(&row.args)
                .map_err(|e| format!("'{}', module {}: {e}", manifest.display(), row.hash))?;
            let target = match args.iter().find_map(|a| a.strip_prefix("-mtriple=")) {
                Some(triple) => target_of_text(&format!("target triple = \"{triple}\"")),
                None => target_of_text(&head_of(&path)),
            };
            let model =
                args.iter().find_map(|a| a.strip_prefix("-relocation-model=")).unwrap_or("default");
            units.push(Unit {
                name: path.display().to_string(),
                target,
                mode: format!("kept:{model}"),
                args,
                output,
                source: Source::Kept { path },
            });
        }
    }
    Ok(units)
}

/// The first 64 KiB of a file, as text (the triple is near the top).
fn head_of(path: &Path) -> String {
    let mut text = Vec::new();
    if let Ok(file) = fs::File::open(path) {
        let _ = file.take(64 << 10).read_to_end(&mut text);
    }
    String::from_utf8_lossy(&text).into_owned()
}

/// What one binary did with one unit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub end: End,
    /// Standard error, the object's path replaced by `<object>`.
    pub stderr: String,
    /// The object, when the run exited 0 and wrote one.
    pub object: Option<Vec<u8>>,
}

impl Run {
    fn succeeded(&self) -> bool {
        self.end == End::Exit(0) && self.object.is_some()
    }

    fn describe(&self) -> String {
        let end = match self.end {
            End::Exit(0) if self.object.is_none() => "exit 0 without an object".to_owned(),
            End::Exit(code) => format!("exit {code}"),
            End::Signal(signal) => format!("signal {signal}"),
            End::TimedOut => "timed out".to_owned(),
        };
        let line = self.stderr.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
        if line.is_empty() { end } else { format!("{end}: {}", cut(line, 160)) }
    }
}

fn cut(text: &str, at: usize) -> String {
    match text.char_indices().nth(at) {
        Some((i, _)) => format!("{}...", &text[..i]),
        None => text.to_owned(),
    }
}

/// The first line in which two texts differ (empty for a text that ended).
fn first_difference<'a>(a: &'a str, b: &'a str) -> (&'a str, &'a str) {
    let (mut la, mut lb) = (a.lines(), b.lines());
    loop {
        match (la.next(), lb.next()) {
            (None, None) => return ("", ""),
            (x, y) if x != y => return (x.unwrap_or(""), y.unwrap_or("")),
            _ => {}
        }
    }
}

/// The class of a unit from what the two binaries did, and what differs.
pub fn classify(base: &Run, head: &Run) -> (Class, String) {
    match (&base.object, &head.object) {
        (Some(a), Some(b)) if base.succeeded() && head.succeeded() => {
            if a == b {
                (Class::Identical, String::new())
            } else {
                (Class::Different, elf::difference(a, b))
            }
        }
        _ if !base.succeeded() && !head.succeeded() => {
            if base.end == head.end && base.stderr == head.stderr {
                (Class::IdenticalFailure, base.describe())
            } else {
                let detail = if base.end == head.end {
                    let (a, b) = first_difference(&base.stderr, &head.stderr);
                    format!(
                        "both {}; diagnostics differ: base '{}', head '{}'",
                        base.describe().split(':').next().unwrap_or(""),
                        cut(a, 120),
                        cut(b, 120)
                    )
                } else {
                    format!("base {}; head {}", base.describe(), head.describe())
                };
                (Class::ChangedOutcome, detail)
            }
        }
        _ => (Class::ChangedOutcome, format!("base {}; head {}", base.describe(), head.describe())),
    }
}

/// Runs `binary` on one unit: the input on standard input, the object
/// written to `object`. Every `HOD_*` variable is removed from the
/// child's environment, so both binaries run alike.
fn run_binary(
    binary: &Path,
    unit: &Unit,
    input: &[u8],
    object: &Path,
    limits: Limits,
) -> Result<Run, String> {
    let _ = fs::remove_file(object);
    let mut args = unit.args.clone();
    args[unit.output] = object.display().to_string();
    let output = process::run(
        binary,
        |command| {
            command.args(&args);
            for (name, _) in std::env::vars_os() {
                if name.to_string_lossy().starts_with("HOD_") {
                    command.env_remove(name);
                }
            }
        },
        input,
        limits,
    )?;
    let stderr =
        String::from_utf8_lossy(&output.stderr).replace(&object.display().to_string(), "<object>");
    let object = if output.end == End::Exit(0) { fs::read(object).ok() } else { None };
    let _ = fs::remove_file(args[unit.output].as_str());
    Ok(Run { end: output.end, stderr, object })
}

/// A binary as the report names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binary {
    pub path: PathBuf,
    pub version: String,
    pub sha256: String,
}

/// Runs `path --version` and hashes the file. A binary that cannot run
/// or does not answer is an error: nothing is compared.
pub fn identify(path: &Path, limits: Limits) -> Result<Binary, String> {
    let bytes = fs::read(path).map_err(|e| format!("cannot read '{}': {e}", path.display()))?;
    let output = process::run(path, |c| _ = c.arg("--version"), b"", limits)
        .map_err(|e| format!("cannot run '{} --version': {e}", path.display()))?;
    let version = String::from_utf8_lossy(&output.stdout).lines().next().unwrap_or("").to_owned();
    if output.end != End::Exit(0) || version.trim().is_empty() {
        return Err(format!(
            "'{} --version' did not answer ({:?}): {}",
            path.display(),
            output.end,
            output.first_error_line()
        ));
    }
    Ok(Binary { path: path.to_path_buf(), version, sha256: crate::corpus::sha256::hex(&bytes) })
}

/// One unit's result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub name: String,
    pub target: String,
    pub mode: String,
    pub class: Class,
    pub detail: String,
}

/// What a run found.
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub base: Option<Binary>,
    pub head: Option<Binary>,
    pub inputs: Vec<String>,
    pub jobs: usize,
    pub outcomes: Vec<Outcome>,
}

impl Report {
    pub fn count(&self, class: Class) -> usize {
        self.outcomes.iter().filter(|o| o.class == class).count()
    }

    /// The modules compared (a module in two modes is one).
    pub fn modules(&self) -> usize {
        let mut names: Vec<&str> = self.outcomes.iter().map(|o| o.name.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        names.len()
    }

    /// 0: something was compared and nothing failed; 1 otherwise.
    pub fn exit_status(&self) -> u8 {
        u8::from(self.outcomes.is_empty() || self.outcomes.iter().any(|o| o.class.fails()))
    }

    /// The Markdown summary.
    pub fn summary(&self) -> String {
        let mut text = String::from("# compare-binaries\n\n");
        for (role, binary) in [("base", &self.base), ("head", &self.head)] {
            if let Some(b) = binary {
                let _ = writeln!(
                    text,
                    "- {role}: `{}`: `{}`, sha256 `{}`",
                    b.path.display(),
                    b.version,
                    b.sha256
                );
            }
        }
        for input in &self.inputs {
            let _ = writeln!(text, "- input: `{input}`");
        }
        let _ = writeln!(text, "- jobs: {}\n", self.jobs);
        let mut header = "| target | mode | compared |".to_owned();
        let mut rule = "|---|---|---|".to_owned();
        for class in Class::ALL {
            let _ = write!(header, " {} |", class.name());
            rule.push_str("---|");
        }
        let _ = writeln!(text, "{header}\n{rule}");
        let mut rows: BTreeMap<(&str, &str), [usize; 5]> = BTreeMap::new();
        for o in &self.outcomes {
            rows.entry((&o.target, &o.mode)).or_default()[o.class as usize] += 1;
        }
        let mut total = [0usize; 5];
        for ((target, mode), counts) in &rows {
            let all: usize = counts.iter().sum();
            let cells: Vec<String> = counts.iter().map(ToString::to_string).collect();
            let _ = writeln!(text, "| {target} | {mode} | {all} | {} |", cells.join(" | "));
            for (t, c) in total.iter_mut().zip(counts) {
                *t += c;
            }
        }
        let cells: Vec<String> = total.iter().map(ToString::to_string).collect();
        let _ = writeln!(text, "| total | | {} | {} |\n", self.outcomes.len(), cells.join(" | "));
        let _ = writeln!(text, "{}", self.totals_line());
        text
    }

    /// The last line: totals and the verdict.
    pub fn totals_line(&self) -> String {
        let verdict = if self.outcomes.is_empty() {
            "FAIL: nothing was compared".to_owned()
        } else if self.exit_status() == 0 {
            "PASS".to_owned()
        } else {
            "FAIL".to_owned()
        };
        let counts: Vec<String> =
            Class::ALL.iter().map(|c| format!("{} {}", c.name(), self.count(*c))).collect();
        format!(
            "compare-binaries: {} modules, {} comparisons: {}; {verdict}",
            self.modules(),
            self.outcomes.len(),
            counts.join(", ")
        )
    }

    /// `results.tsv`: every comparison.
    pub fn results_text(&self) -> String {
        let mut text = String::from("class\ttarget\tmode\tmodule\n");
        for o in &self.outcomes {
            let _ = writeln!(text, "{}\t{}\t{}\t{}", o.class.name(), o.target, o.mode, o.name);
        }
        text
    }

    /// `differences.tsv`: every comparison that fails the run, with what
    /// differs.
    pub fn differences_text(&self) -> String {
        let mut text = String::from("class\ttarget\tmode\tmodule\tdetail\n");
        for o in self.outcomes.iter().filter(|o| o.class.fails()) {
            let detail = o.detail.replace(['\t', '\n'], " ");
            let _ = writeln!(
                text,
                "{}\t{}\t{}\t{}\t{detail}",
                o.class.name(),
                o.target,
                o.mode,
                o.name
            );
        }
        text
    }
}

/// Runs the comparison and writes the report files under `options.out`.
/// `Err` is a run that compared nothing for a reason of its own (exit 2).
pub fn run(options: &Options) -> Result<Report, String> {
    let (Some(base), Some(head)) = (&options.base, &options.head) else {
        return Err("--base and --head are both required".to_owned());
    };
    let id_limits = Limits { time: Duration::from_mins(1), memory_kib: options.limits.memory_kib };
    let mut report = Report {
        base: Some(identify(base, id_limits)?),
        head: Some(identify(head, id_limits)?),
        jobs: options.jobs.clamp(1, MAX_JOBS),
        ..Report::default()
    };
    let mut units = Vec::new();
    for dir in &options.corpora {
        units.extend(corpus_units(dir)?);
        report.inputs.push(format!("corpus {}", dir.display()));
    }
    for dir in &options.modules {
        units.extend(kept_units(dir)?);
        report.inputs.push(format!("kept modules {}", dir.display()));
    }
    let work = options.out.join("work");
    fs::create_dir_all(&work).map_err(|e| format!("cannot create '{}': {e}", work.display()))?;
    let next = AtomicUsize::new(0);
    let slots: Mutex<Vec<Option<Outcome>>> = Mutex::new(vec![None; units.len()]);
    std::thread::scope(|scope| {
        for _ in 0..report.jobs.min(units.len()) {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(unit) = units.get(index) else { break };
                    let (class, detail) = compare_unit(base, head, unit, &work, index, options);
                    let outcome = Outcome {
                        name: unit.name.clone(),
                        target: unit.target.clone(),
                        mode: unit.mode.clone(),
                        class,
                        detail,
                    };
                    slots.lock().unwrap_or_else(PoisonError::into_inner)[index] = Some(outcome);
                }
            });
        }
    });
    let _ = fs::remove_dir_all(&work);
    report.outcomes =
        slots.into_inner().unwrap_or_else(PoisonError::into_inner).into_iter().flatten().collect();
    for (name, text) in [
        ("summary.md", report.summary()),
        ("results.tsv", report.results_text()),
        ("differences.tsv", report.differences_text()),
    ] {
        let path = options.out.join(name);
        fs::write(&path, text).map_err(|e| format!("cannot write '{}': {e}", path.display()))?;
    }
    Ok(report)
}

fn compare_unit(
    base: &Path,
    head: &Path,
    unit: &Unit,
    work: &Path,
    index: usize,
    options: &Options,
) -> (Class, String) {
    let input = match unit.input() {
        Ok(input) => input,
        Err(e) => return (Class::HarnessError, e),
    };
    let ran = |binary: &Path, role: &str| {
        let object = work.join(format!("{index}-{role}.o"));
        run_binary(binary, unit, &input, &object, options.limits)
            .map_err(|e| format!("{role}: {e}"))
    };
    match (ran(base, "base"), ran(head, "head")) {
        (Ok(a), Ok(b)) => classify(&a, &b),
        (Err(e), _) | (_, Err(e)) => (Class::HarnessError, e),
    }
}

/// `cargo xtask compare-binaries`: the exit status.
pub fn command(args: &[String]) -> u8 {
    let mut options = Options::new(&irdiff::default_build_dir());
    match parse_args(args, &mut options) {
        Ok(true) => {}
        Ok(false) => {
            print!("{USAGE}");
            return 0;
        }
        Err(message) => {
            eprint!("xtask compare-binaries: {message}\n{USAGE}");
            return 2;
        }
    }
    match run(&options) {
        Ok(report) => {
            print!("{}", report.summary());
            println!("report: {}", options.out.join("summary.md").display());
            if report.exit_status() != 0 {
                println!("differences: {}", options.out.join("differences.tsv").display());
            }
            report.exit_status()
        }
        Err(message) => {
            eprintln!("xtask compare-binaries: {message}");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_of(end: End, stderr: &str, object: Option<&[u8]>) -> Run {
        Run { end, stderr: stderr.to_owned(), object: object.map(<[u8]>::to_vec) }
    }

    #[test]
    fn classes() {
        let ok = run_of(End::Exit(0), "", Some(b"abc"));
        let other = run_of(End::Exit(0), "", Some(b"abd"));
        let refused = run_of(End::Exit(2), "hod: error: unsupported construct 'x'\n", None);
        let refused_y = run_of(End::Exit(2), "hod: error: unsupported construct 'y'\n", None);
        let failed = run_of(End::Exit(1), "hod: error: unsupported construct 'x'\n", None);
        let no_object = run_of(End::Exit(0), "", None);
        assert_eq!(classify(&ok, &ok).0, Class::Identical);
        assert_eq!(classify(&ok, &other).0, Class::Different);
        assert_eq!(classify(&ok, &refused).0, Class::ChangedOutcome);
        assert_eq!(classify(&refused, &ok).0, Class::ChangedOutcome);
        assert_eq!(classify(&refused, &refused).0, Class::IdenticalFailure);
        let (class, detail) = classify(&refused, &refused_y);
        assert_eq!(class, Class::ChangedOutcome);
        assert!(detail.contains("'y'"), "{detail}");
        assert_eq!(classify(&refused, &failed).0, Class::ChangedOutcome);
        assert_eq!(classify(&ok, &no_object).0, Class::ChangedOutcome);
        assert!(Class::Different.fails() && !Class::IdenticalFailure.fails());
    }

    #[test]
    fn triples() {
        assert_eq!(target_of_text("target triple = \"x86_64-unknown-linux-gnu\"\n"), "x86_64");
        assert_eq!(target_of_text("; x\ntarget triple = \"aarch64-unknown-linux-gnu\""), "aarch64");
        assert_eq!(target_of_text("define void @f() {}"), "unknown");
    }

    #[test]
    fn arguments() {
        let strings = |a: &[&str]| a.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        let mut o = Options::new(Path::new("/b"));
        assert!(parse_args(&strings(&["--base", "a", "--head", "b"]), &mut o).is_err());
        let mut o = Options::new(Path::new("/b"));
        assert!(parse_args(&strings(&["--base", "a", "--corpus", "c"]), &mut o).is_err());
        let mut o = Options::new(Path::new("/b"));
        let relative = strings(&["--base", "a", "--head", "b", "--corpus", "c", "--out", "rel"]);
        assert!(parse_args(&relative, &mut o).is_err());
        let mut o = Options::new(Path::new("/b"));
        let good = strings(&["--base=a", "--head", "b", "--modules", "m", "-j99", "--out", "/o"]);
        assert_eq!(parse_args(&good, &mut o), Ok(true));
        assert_eq!((o.jobs, o.out.as_path()), (MAX_JOBS, Path::new("/o")));
        assert_eq!(o.out, PathBuf::from("/o"));
    }
}
