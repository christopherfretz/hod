//! `cargo xtask bench`: how fast hod writes objects, how much memory it
//! takes, and both against `llc -O0` on the same inputs.
//! Contract: `spec/harness.md` "bench".
//!
//! The parts: [`manifest`] lists the inputs and produces their IR,
//! [`measure`] runs one child with a stopwatch and a memory probe,
//! [`record`] is the results file, [`check`] is the gate against a
//! baseline, [`report`] is what a person reads.

pub mod check;
pub mod manifest;
pub mod measure;
pub mod record;
pub mod report;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::irdiff::process::{self, End, Limits};
use crate::irdiff::tools::{LLVM_MAJORS, Search, find_llvm, prefixed};
use crate::irdiff::{default_build_dir, workspace_root};

use check::{Check, Thresholds, baseline_path, judge_all, update_baseline};
use manifest::{Generated, Input, Manifest, PRODUCER_LIMITS, Producer, generate, version_line};
use measure::{GNU_TIME, Probe, Sample, Samples, calibrate, min_median_max, run_measured};
use record::{Machine, Record, Results, Status, Tool, valid_phase};

pub const USAGE: &str = "\
usage: cargo xtask bench [options]

Measures llc and hod on the benchmark inputs (bench/manifest): wall time
(the fastest of several serial runs), CPU time, peak memory and object
size, each also as a ratio to llc. Writes a results file under
<build directory>/bench/ and prints a summary.

actions (one at most; without one: measure and print):
  --generate           produce the inputs' IR again and list them
  --check              measure, then compare hod with this machine's baseline
  --update-baseline    measure, then record the run as this machine's
                       baseline; needs --reason
  --report             write docs/status/perf.md from a results file

options:
  --reason <text>      why the baseline changes (appended to its log)
  --runs <N>           runs per input and tool (default 3, at most 100)
  --filter <text>      only the inputs whose name contains <text> (repeatable)
  --llvm <major|dir>   the LLVM whose llc is the reference (default: 22, else 19)
  --hod <binary>       the hod to measure (default: the workspace's, built
                       with the release profile)
  --kiln <kiln++>      produce the inputs with kiln instead of clang
  --judge <tool>       what --check judges: hod (default) or hod+fallback
  --baseline <file>    the baseline to check against or to update
                       (default: bench/baselines/<machine id>.tsv)
  --results <file>     the results file to write, or for --report to read
                       (default: <build directory>/bench/results-llvm<major>.tsv)
  --out <file>         what --report writes (default: docs/status/perf.md)
  -v, --verbose        also print hod's phase times
  -h, --help           this text

exit status: 0; 1 when --check finds hod slower or larger than the baseline
allows on the baseline's machine; 3 when the run cannot be trusted (a tool
failed, a required tool is missing, the machine was too busy to judge);
2 for a usage error.
";

/// What `llc` and hod are asked to do with every input.
pub const CODEGEN_ARGS: [&str; 3] = ["-O0", "-filetype=obj", "-relocation-model=pic"];

/// The variable through which hod is asked for its phase times
/// (`spec/cli.md` "Phase report").
pub const ENV_TIME_PHASES: &str = "HOD_TIME_PHASES";

/// hod's settings; the harness configures hod by options alone.
const HOD_SETTINGS: [&str; 4] =
    ["HOD_FALLBACK_LLC", "HOD_STRICT", "HOD_COVERAGE_LOG", "HOD_IN_FALLBACK"];

/// The default number of runs per input and tool.
pub const DEFAULT_RUNS: u32 = 3;
/// The most runs that can be asked for.
pub const MAX_RUNS: u32 = 100;

/// What the command does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Measure,
    Generate,
    Check,
    UpdateBaseline,
    Report,
}

/// A run's configuration: the command line, and what tests replace.
#[derive(Clone, Debug)]
pub struct Config {
    pub action: Action,
    /// The checkout: `bench/` and `docs/status/` are found in it.
    pub workspace: PathBuf,
    /// Where `bench/` (inputs, results, the work directory) is made.
    pub build_dir: PathBuf,
    pub runs: u32,
    pub filters: Vec<String>,
    pub llvm: Option<String>,
    pub hod: Option<PathBuf>,
    pub kiln: Option<PathBuf>,
    pub judge: Tool,
    pub baseline: Option<PathBuf>,
    pub results: Option<PathBuf>,
    pub out: Option<PathBuf>,
    pub reason: Option<String>,
    pub verbose: bool,
    /// `HOD_REQUIRE_TOOLS=1`: a missing tool is a failure, not a skip.
    pub require_tools: bool,
    pub search: Search,
    /// GNU time.
    pub time: PathBuf,
    /// The limits of one measured child.
    pub limits: Limits,
    pub machine: Machine,
    pub thresholds: Thresholds,
    /// How many times a failing input is measured again by `--check`.
    pub retries: u32,
}

impl Config {
    /// The defaults for a checkout and a build directory.
    pub fn new(workspace: PathBuf, build_dir: PathBuf) -> Self {
        Self {
            action: Action::Measure,
            workspace,
            build_dir,
            runs: DEFAULT_RUNS,
            filters: Vec::new(),
            llvm: None,
            hod: None,
            kiln: None,
            judge: Tool::Hod,
            baseline: None,
            results: None,
            out: None,
            reason: None,
            verbose: false,
            require_tools: false,
            search: Search::from_env(),
            time: PathBuf::from(GNU_TIME),
            limits: Limits { time: Duration::from_mins(2), memory_kib: 8 << 20 },
            machine: Machine::detect(),
            thresholds: Thresholds::default(),
            retries: 2,
        }
    }

    /// This checkout, its build directory, and `HOD_REQUIRE_TOOLS`.
    pub fn for_workspace() -> Self {
        let mut config = Self::new(workspace_root(), default_build_dir());
        config.require_tools = std::env::var_os("HOD_REQUIRE_TOOLS").is_some_and(|v| v == "1");
        config
    }

    fn bench_dir(&self) -> PathBuf {
        self.build_dir.join("bench")
    }
}

/// Parses the command's arguments into `config` (which holds the
/// defaults). `Ok(false)` means the help was asked for.
pub fn parse_args(args: &[String], config: &mut Config) -> Result<bool, String> {
    let mut rest = args.iter();
    let mut actions: Vec<&str> = Vec::new();
    while let Some(arg) = rest.next() {
        let (name, joined) = match arg.split_once('=') {
            Some((name, value)) if name.starts_with('-') => (name, Some(value.to_owned())),
            _ => (arg.as_str(), None),
        };
        let flag = |joined: &Option<String>| match joined {
            None => Ok(()),
            Some(_) => Err(format!("'{name}' takes no value")),
        };
        let mut value = || {
            joined
                .clone()
                .or_else(|| rest.next().cloned())
                .filter(|v| !v.is_empty())
                .ok_or_else(|| format!("'{name}' requires a value"))
        };
        match name {
            "-h" | "--help" => return Ok(false),
            "--generate" | "--check" | "--update-baseline" | "--report" => {
                flag(&joined)?;
                config.action = match name {
                    "--generate" => Action::Generate,
                    "--check" => Action::Check,
                    "--update-baseline" => Action::UpdateBaseline,
                    _ => Action::Report,
                };
                if !actions.contains(&name) {
                    actions.push(name);
                }
            }
            "-v" | "--verbose" => {
                flag(&joined)?;
                config.verbose = true;
            }
            "--reason" => config.reason = Some(value()?),
            "--runs" => {
                let text = value()?;
                config.runs = match text.parse::<u32>() {
                    Ok(runs) if (1..=MAX_RUNS).contains(&runs) => runs,
                    _ => return Err(format!("invalid number of runs '{text}': 1 to {MAX_RUNS}")),
                };
            }
            "--filter" => config.filters.push(value()?),
            "--llvm" => config.llvm = Some(value()?),
            "--hod" => config.hod = Some(PathBuf::from(value()?)),
            "--kiln" => config.kiln = Some(PathBuf::from(value()?)),
            "--judge" => {
                let text = value()?;
                config.judge = [Tool::Hod, Tool::HodFallback]
                    .into_iter()
                    .find(|tool| tool.name() == text)
                    .ok_or_else(|| format!("'--judge {text}': expected hod or hod+fallback"))?;
            }
            "--baseline" => config.baseline = Some(PathBuf::from(value()?)),
            "--results" => config.results = Some(PathBuf::from(value()?)),
            "--out" => config.out = Some(PathBuf::from(value()?)),
            _ if name.starts_with('-') => return Err(format!("unknown option '{name}'")),
            _ => {
                return Err(format!(
                    "unexpected argument '{arg}': the inputs are those of bench/manifest"
                ));
            }
        }
    }
    if actions.len() > 1 {
        return Err(format!("{} and {} are two actions; give one", actions[0], actions[1]));
    }
    let reason = config.reason.as_deref().map(str::trim);
    match config.action {
        Action::UpdateBaseline if reason.is_none_or(str::is_empty) => {
            return Err(
                "--update-baseline needs --reason \"<why the baseline changes>\"".to_owned()
            );
        }
        Action::UpdateBaseline if !config.filters.is_empty() || config.kiln.is_some() => {
            return Err(
                "--update-baseline takes neither --filter nor --kiln: a baseline is every input \
                        of the manifest, produced by clang"
                    .to_owned(),
            );
        }
        Action::UpdateBaseline => {}
        _ if reason.is_some() => return Err("--reason goes with --update-baseline".to_owned()),
        _ => {}
    }
    Ok(true)
}

/// An input and what producing it gave.
#[derive(Clone, Debug)]
pub struct Listed {
    pub input: Input,
    pub generated: Result<Generated, String>,
}

/// What a run of the command did.
#[derive(Clone, Debug)]
pub struct Run {
    pub action: Action,
    /// Tools that were not found. Without `HOD_REQUIRE_TOOLS=1` what needs
    /// them is skipped; with it, the run fails.
    pub missing_tools: Vec<String>,
    pub required: bool,
    /// Things worth a line that are not results.
    pub notes: Vec<String>,
    /// The inputs, when the action is `--generate`.
    pub listed: Vec<Listed>,
    pub results: Option<Results>,
    /// The results file that was written.
    pub results_path: Option<PathBuf>,
    pub check: Option<Check>,
    /// The baseline that was written.
    pub updated: Option<PathBuf>,
    /// How many inputs of a written baseline have a time the gate will
    /// not hold anything to ("The gate", "Spread").
    pub unsteady: usize,
    /// Why the baseline was not written.
    pub refused: Option<String>,
    /// The report that was written.
    pub report: Option<PathBuf>,
}

impl Run {
    fn new(config: &Config) -> Self {
        Self {
            action: config.action,
            missing_tools: Vec::new(),
            required: config.require_tools,
            notes: Vec::new(),
            listed: Vec::new(),
            results: None,
            results_path: None,
            check: None,
            updated: None,
            unsteady: 0,
            refused: None,
            report: None,
        }
    }

    fn errors(&self) -> usize {
        let listed = self.listed.iter().filter(|listed| listed.generated.is_err()).count();
        let records = self.results.as_ref().map_or(0, |results| {
            results.records.iter().filter(|record| record.status == Status::Error).count()
        });
        listed + records
    }

    /// The command's exit status: 1 if the gate fails; else 3 if the run
    /// cannot be trusted; else 0.
    pub fn exit_status(&self) -> u8 {
        let untrusted = self.errors() > 0
            || (self.required && !self.missing_tools.is_empty())
            || self.refused.is_some()
            || self.check.as_ref().is_some_and(|check| check.noisy() > 0);
        if self.check.as_ref().is_some_and(|check| check.failures() > 0) {
            1
        } else if untrusted {
            3
        } else {
            0
        }
    }

    /// What the command prints.
    pub fn summary(&self, verbose: bool) -> String {
        let mut text = String::new();
        for tool in &self.missing_tools {
            let what =
                if self.required { "MISSING (HOD_REQUIRE_TOOLS=1)" } else { "not found; skipped" };
            let _ = writeln!(text, "bench: {tool}: {what}");
        }
        for note in &self.notes {
            let _ = writeln!(text, "bench: {note}");
        }
        if !self.listed.is_empty() {
            let header = ["input", "class", "IR bytes", "functions", "hash", ""];
            let rows: Vec<Vec<String>> = self
                .listed
                .iter()
                .map(|listed| {
                    let mut row =
                        vec![listed.input.id.clone(), listed.input.class.name().to_owned()];
                    match &listed.generated {
                        Ok(made) => row.extend([
                            made.ir_bytes.to_string(),
                            made.functions.to_string(),
                            made.ir_hash.clone(),
                            if made.fresh { "produced" } else { "up to date" }.to_owned(),
                        ]),
                        Err(why) => row.extend([
                            "-".to_owned(),
                            "-".to_owned(),
                            "-".to_owned(),
                            format!("ERROR: {why}"),
                        ]),
                    }
                    row
                })
                .collect();
            let header: Vec<String> = header.iter().map(|&name| name.to_owned()).collect();
            text.push_str(&check::table(&header, &rows, &[0, 1, 4, 5]));
        }
        if let Some(results) = &self.results {
            text.push_str(&report::summary(results, verbose));
        }
        if let Some(path) = &self.results_path {
            let _ = writeln!(text, "bench: results: {}", path.display());
        }
        if let Some(check) = &self.check {
            text.push_str(&check.text());
        }
        if let Some(path) = &self.updated {
            if self.unsteady > 0 {
                let _ = writeln!(
                    text,
                    "bench: baseline: {} inputs have a time whose runs disagree by more than the gate \
                     allows; it will not fail on their times (a quieter machine, or more --runs)",
                    self.unsteady
                );
            }
            let _ = writeln!(text, "bench: baseline written: {}", path.display());
        }
        if let Some(why) = &self.refused {
            let _ = writeln!(text, "bench: baseline NOT written: {why}");
        }
        if let Some(path) = &self.report {
            let _ = writeln!(text, "bench: report written: {}", path.display());
        }
        text
    }
}

/// `YYYY-MM-DDThh:mm:ssZ` for seconds since 1970-01-01T00:00:00Z.
pub fn utc(seconds: u64) -> String {
    let (days, rest) = (seconds / 86_400, seconds % 86_400);
    // Days to a civil date, counting from 0000-03-01 in 400-year eras.
    let shifted = days + 719_468;
    let (era, day_of_era) = (shifted / 146_097, shifted % 146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 { shifted_month + 3 } else { shifted_month - 9 };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

fn now_utc() -> String {
    utc(SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs()))
}

/// `LLVM version <x.y.z>` from `llc --version`.
fn llvm_version(llc: &Path) -> String {
    let limits = Limits { time: Duration::from_secs(20), memory_kib: 4 << 20 };
    let said = process::run(
        llc,
        |c| {
            c.arg("--version");
        },
        b"",
        limits,
    )
    .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
    .unwrap_or_default();
    said.split("LLVM version ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .map_or_else(|| "LLVM version ?".to_owned(), |version| format!("LLVM version {version}"))
}

/// Builds the workspace's hod with the release profile: what is measured
/// is what a user would run.
fn build_release_hod(workspace: &Path, build_dir: &Path) -> Result<PathBuf, String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| env!("CARGO").into());
    let built = Command::new(&cargo)
        .args(["build", "--quiet", "--release", "--package", "hod_cli", "--bin", "hod"])
        .current_dir(workspace)
        .env("CARGO_TARGET_DIR", build_dir)
        .status()
        .map_err(|e| format!("cannot run '{}': {e}", Path::new(&cargo).display()))?;
    let hod = build_dir.join("release").join("hod");
    if built.success() && hod.is_file() {
        Ok(hod)
    } else {
        Err("building the workspace's hod (release profile) failed".to_owned())
    }
}

/// What one run needs to start a tool.
struct Context<'a> {
    llc: &'a Path,
    hod: &'a Path,
    probe: &'a Probe,
    work: &'a Path,
    limits: Limits,
}

/// One run of one tool on one input, classified.
struct Ran {
    sample: Sample,
    status: Status,
    /// The reason of a decline or a fallback.
    detail: String,
    object_bytes: Option<u64>,
    phases: Vec<(String, u64)>,
}

/// The last line of hod's coverage log: its outcome and its reason.
fn coverage_outcome(log: &Path) -> Option<(String, String)> {
    let text = std::fs::read_to_string(log).ok()?;
    let fields: Vec<&str> = text.lines().last()?.split('\t').collect();
    (fields.len() == 8).then(|| (fields[0].to_owned(), fields[1].to_owned()))
}

/// Reads hod's phase report (`spec/cli.md` "Phase report"): one line
/// `<phase><TAB><microseconds>` per phase. No file is no phases (a hod
/// that does not report them); anything else in the file is an error.
pub fn read_phases(path: &Path) -> Result<Vec<(String, u64)>, String> {
    let Ok(text) = std::fs::read_to_string(path) else { return Ok(Vec::new()) };
    text.lines()
        .map(|line| {
            line.split_once('\t')
                .filter(|(name, us)| {
                    valid_phase(name) && !us.is_empty() && us.bytes().all(|b| b.is_ascii_digit())
                })
                .and_then(|(name, us)| Some((name.to_owned(), us.parse().ok()?)))
                .ok_or_else(|| format!("wrote a malformed phase report: '{line}'"))
        })
        .collect()
}

/// Runs `tool` once on the IR file `ir` and says what it did. The error
/// is what goes into the detail of an `error` record.
fn run_once(context: &Context<'_>, id: &str, ir: &Path, tool: Tool) -> Result<Ran, String> {
    let stem = format!("{id}.{}", tool.name().replace('+', "-"));
    let file = |extension: &str| context.work.join(format!("{stem}.{extension}"));
    let (object, log, phase_file) = (file("o"), file("coverage"), file("phases"));
    for stale in [&object, &log, &phase_file] {
        let _ = std::fs::remove_file(stale);
    }
    let program = if tool == Tool::Llc { context.llc } else { context.hod };
    let sample = run_measured(
        program,
        |c| {
            if tool != Tool::Llc {
                if tool == Tool::Hod {
                    c.arg("--strict");
                } else {
                    c.arg(prefixed("--fallback=", context.llc));
                }
                c.arg(prefixed("--coverage-log=", &log));
                for name in HOD_SETTINGS {
                    c.env_remove(name);
                }
                c.env(ENV_TIME_PHASES, &phase_file);
            }
            c.args(CODEGEN_ARGS).arg("-o").arg(&object).arg(ir).current_dir(context.work);
        },
        context.probe,
        &file("usage"),
        context.limits,
    )?;
    let name = tool.name();
    let said = |why: String| {
        let line = sample.first_error_line();
        if line.is_empty() { format!("{name} {why}") } else { format!("{name} {why}: {line}") }
    };
    let status = match sample.end {
        End::TimedOut => {
            return Err(format!("{name} timed out after {} s", context.limits.time.as_secs_f64()));
        }
        End::Signal(signal) => return Err(said(format!("was terminated by signal {signal}"))),
        End::Exit(status) => status,
    };
    let object_bytes =
        std::fs::metadata(&object).ok().map(|meta| meta.len()).filter(|&bytes| bytes > 0);
    let logged = if tool == Tool::Llc { None } else { coverage_outcome(&log) };
    let outcome = logged.as_ref().map(|(outcome, reason)| (outcome.as_str(), reason.as_str()));
    let (status, detail) = match (tool, status, outcome) {
        (Tool::Llc, 0, _) | (Tool::Hod | Tool::HodFallback, 0, Some(("hod", _))) => {
            (Status::Ok, "")
        }
        (Tool::Hod, 2, Some(("error", reason))) => (Status::Declined, reason),
        (Tool::HodFallback, 0, Some(("fallback", reason))) => (Status::Fallback, reason),
        (Tool::Hod | Tool::HodFallback, 0 | 2, None) => {
            return Err(said(format!("exited with status {status} and wrote no coverage record")));
        }
        _ => return Err(said(format!("exited with status {status}"))),
    };
    if status != Status::Declined && object_bytes.is_none() {
        return Err(said("exited with status 0 and wrote no object".to_owned()));
    }
    let phases = read_phases(&phase_file).map_err(|why| format!("{name} {why}"))?;
    Ok(Ran { sample, status, detail: detail.to_owned(), object_bytes, phases })
}

/// The runs of one tool on one input.
#[derive(Clone, Debug, Default)]
struct ToolRuns {
    samples: Samples,
    status: Option<Status>,
    detail: String,
    object_bytes: Option<u64>,
    /// The phases of the fastest run.
    phases: Vec<(String, u64)>,
    /// What went wrong; the tool is not run again.
    failed: Option<String>,
}

impl ToolRuns {
    fn add(&mut self, ran: Result<Ran, String>) {
        let ran = match ran {
            Ok(ran) => ran,
            Err(why) => {
                self.failed = Some(why);
                return;
            }
        };
        if let Some(before) = self.status
            && (before, self.detail.as_str()) != (ran.status, ran.detail.as_str())
        {
            self.failed = Some(format!(
                "did not do the same thing twice: {} ({}), then {} ({})",
                before.name(),
                self.detail,
                ran.status.name(),
                ran.detail
            ));
            return;
        }
        let fastest = self.samples.wall_us.iter().min().copied();
        self.samples.push(&ran.sample);
        if fastest
            .is_none_or(|fastest| self.samples.wall_us.last().is_some_and(|&wall| wall < fastest))
        {
            self.phases = ran.phases;
        }
        (self.status, self.detail, self.object_bytes) =
            (Some(ran.status), ran.detail, ran.object_bytes);
    }

    fn record(&self, input: &Input, generated: &Generated, tool: Tool) -> Record {
        let identity = |status: Status| Record {
            ir_bytes: Some(generated.ir_bytes),
            functions: Some(generated.functions),
            ir_hash: Some(generated.ir_hash.clone()),
            ..Record::blank(status, &input.id, input.class, input.debug, tool)
        };
        let (Some(status), None) = (self.status, &self.failed) else {
            let detail = self.failed.clone().unwrap_or_else(|| "was not run".to_owned());
            return Record { detail, ..identity(Status::Error) };
        };
        let wall = min_median_max(&self.samples.wall_us);
        let rss = min_median_max(&self.samples.rss_kib);
        Record {
            runs: u32::try_from(self.samples.runs()).unwrap_or(u32::MAX),
            wall_us_min: wall.map(|(min, _, _)| min),
            wall_us_median: wall.map(|(_, median, _)| median),
            wall_us_max: wall.map(|(_, _, max)| max),
            cpu_us: self.samples.cpu_us.iter().min().copied(),
            rss_kib: rss.map(|(_, median, _)| median),
            rss_kib_min: rss.map(|(min, _, _)| min),
            rss_kib_max: rss.map(|(_, _, max)| max),
            object_bytes: self.object_bytes,
            phases: self.phases.clone(),
            detail: self.detail.clone(),
            ..identity(status)
        }
    }
}

/// An input and every run made on it.
struct Measured {
    input: Input,
    generated: Generated,
    tools: [ToolRuns; 3],
}

impl Measured {
    /// `rounds` more rounds. A round runs each tool once, one after the
    /// other, so that a burst of load on the machine falls on all of them
    /// and not on all the runs of one.
    fn measure(&mut self, context: &Context<'_>, rounds: u32) {
        for _ in 0..rounds {
            for (runs, tool) in self.tools.iter_mut().zip(Tool::MEASURED) {
                if runs.failed.is_none() {
                    runs.add(run_once(context, &self.input.id, &self.generated.path, tool));
                }
            }
        }
    }

    /// The records of the tools, `llc` first, with the ratios to it.
    fn records(&self) -> Vec<Record> {
        let mut records: Vec<Record> = self
            .tools
            .iter()
            .zip(Tool::MEASURED)
            .map(|(runs, tool)| runs.record(&self.input, &self.generated, tool))
            .collect();
        let llc = records[0].clone();
        for record in &mut records[1..] {
            record.set_ratios(&llc);
        }
        records
    }
}

/// The baseline to check against: the one named; else this machine's;
/// else the first, by name, of those recorded for other machines.
fn find_baseline(config: &Config) -> Result<Option<(PathBuf, Results)>, String> {
    let dir = config.workspace.join("bench").join("baselines");
    let own = config.baseline.clone().unwrap_or_else(|| baseline_path(&dir, &config.machine.id));
    if config.baseline.is_some() || own.is_file() {
        return Results::read(&own).map(|results| Some((own, results)));
    }
    let mut others: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|entries| entries.filter_map(|entry| Some(entry.ok()?.path())).collect())
        .unwrap_or_default();
    others.retain(|path| path.extension().is_some_and(|extension| extension == "tsv"));
    others.sort();
    match others.into_iter().next() {
        Some(other) => Results::read(&other).map(|results| Some((other, results))),
        None => Ok(None),
    }
}

/// Finds the reference `llc` and the producer and makes every input's IR
/// (into `run.listed`). `Ok(None)`: a tool is missing, and `run` says
/// which.
fn produce_inputs(
    config: &Config,
    run: &mut Run,
) -> Result<Option<(crate::irdiff::tools::Llvm, Producer)>, String> {
    let bench = config.workspace.join("bench");
    let manifest = Manifest::read(&bench.join("manifest"))?;
    let mut inputs = manifest.inputs();
    if !config.filters.is_empty() {
        inputs
            .retain(|input| config.filters.iter().any(|filter| input.id.contains(filter.as_str())));
        if inputs.is_empty() {
            return Err(format!("no input's name contains '{}'", config.filters.join("' or '")));
        }
    }

    let Some(llvm) = find_llvm(config.llvm.as_deref(), &config.search)? else {
        let wanted = config.llvm.clone().unwrap_or_else(|| "22 or 19".to_owned());
        run.missing_tools.push(format!("llc (LLVM {wanted})"));
        return Ok(None);
    };
    if config.action == Action::UpdateBaseline && llvm.major != LLVM_MAJORS[0] {
        return Err(format!(
            "a baseline is recorded with LLVM {}, not {}",
            LLVM_MAJORS[0], llvm.major
        ));
    }
    let producer = match &config.kiln {
        Some(kiln) => Producer::kiln(kiln)?,
        None => match Producer::clang(&llvm, &config.search) {
            Ok(producer) => producer,
            Err(why) => {
                run.missing_tools.push(format!("clang ({why})"));
                return Ok(None);
            }
        },
    };

    // The inputs' IR.
    let inputs_dir = config.bench_dir().join("inputs").join(&producer.tag);
    let force = config.action == Action::Generate;
    for input in inputs {
        let generated =
            generate(&input, &producer, &bench.join("src"), &inputs_dir, force, PRODUCER_LIMITS);
        run.listed.push(Listed { input, generated });
    }
    if config.action == Action::Generate {
        run.notes.push(format!("inputs by {} in {}", producer.version, inputs_dir.display()));
    }
    Ok(Some((llvm, producer)))
}

/// Runs the command.
///
/// An error is a run that could not be made: the manifest or a named file
/// cannot be read, the results cannot be written.
pub fn run(config: &Config) -> Result<Run, String> {
    let mut run = Run::new(config);
    if config.action == Action::Report {
        return write_report(config, run);
    }
    let Some((llvm, producer)) = produce_inputs(config, &mut run)? else { return Ok(run) };
    if config.action == Action::Generate {
        return Ok(run);
    }
    let listed = std::mem::take(&mut run.listed);

    // The tools and the method.
    let (hod, hod_build) = match &config.hod {
        Some(hod) if hod.is_file() => {
            (std::path::absolute(hod).unwrap_or_else(|_| hod.clone()), "given with --hod")
        }
        Some(hod) => return Err(format!("'{}' is not a file", hod.display())),
        None => {
            (build_release_hod(&config.workspace, &config.build_dir)?, "workspace, release profile")
        }
    };
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_nanos());
    let work = config.bench_dir().join(format!("run-{}-{stamp}", std::process::id()));
    std::fs::create_dir_all(&work)
        .map_err(|e| format!("cannot create '{}': {e}", work.display()))?;
    let probe = Probe::detect(&config.time, &work);
    if probe.time.is_none() {
        run.missing_tools
            .push(format!("GNU time ({}): no memory or CPU figures", config.time.display()));
    }
    let mut results = header(config, &llvm.llc, &hod, hod_build, &producer, &probe, &work);

    // The measurement: input by input, serially.
    let context =
        Context { llc: &llvm.llc, hod: &hod, probe: &probe, work: &work, limits: config.limits };
    let mut session = Session::default();
    for Listed { input, generated } in listed {
        match generated {
            Ok(generated) => {
                let mut one = Measured { input, generated, tools: Default::default() };
                one.measure(&context, config.runs);
                session.measured.push(one);
            }
            Err(why) => session.failed_inputs.push(Record {
                detail: why,
                ..Record::blank(Status::Error, &input.id, input.class, input.debug, Tool::Generate)
            }),
        }
    }
    results.records = session.records();
    let checked = if config.action == Action::Check {
        gate(config, &context, &mut session, &mut results)
    } else {
        Ok(None)
    };
    let _ = std::fs::remove_dir_all(&work);
    match checked? {
        Some(check) => run.check = Some(check),
        None if config.action == Action::Check => run.notes.push(
            "check: no baseline is recorded (bench/baselines); nothing to compare with".to_owned(),
        ),
        None => {}
    }

    let default_name = if producer.tag == "kiln" {
        format!("results-kiln-llvm{}.tsv", llvm.major)
    } else {
        format!("results-llvm{}.tsv", llvm.major)
    };
    let path = config.results.clone().unwrap_or_else(|| config.bench_dir().join(default_name));
    results.write(&path)?;
    run.results_path = Some(path);

    if config.action == Action::UpdateBaseline {
        let dir = config.workspace.join("bench").join("baselines");
        let path =
            config.baseline.clone().unwrap_or_else(|| baseline_path(&dir, &config.machine.id));
        match update_baseline(&path, &results, config.reason.as_deref().unwrap_or("")) {
            Ok(()) => {
                run.unsteady = check::unsteady(&results, &config.thresholds);
                run.updated = Some(path);
            }
            Err(why) => run.refused = Some(why),
        }
    }
    run.results = Some(results);
    Ok(run)
}

/// Everything measured in a run.
#[derive(Default)]
struct Session {
    /// The inputs that could not be produced, as `error` records.
    failed_inputs: Vec<Record>,
    measured: Vec<Measured>,
}

impl Session {
    fn records(&self) -> Vec<Record> {
        let measured = self.measured.iter().flat_map(Measured::records);
        self.failed_inputs.iter().cloned().chain(measured).collect()
    }
}

/// The header of a run's results: where, when, with what, and what the
/// method itself costs (measured now, in `work`).
fn header(
    config: &Config,
    llc: &Path,
    hod: &Path,
    hod_build: &str,
    producer: &Producer,
    probe: &Probe,
    work: &Path,
) -> Results {
    let calibration = calibrate(probe, work);
    let mut results = Results::default();
    results.set("date", now_utc());
    results.set("machine", config.machine.id.clone());
    results.set("cpu", config.machine.cpu.clone());
    results.set("cores", config.machine.cores.to_string());
    results.set("arch", config.machine.arch.clone());
    results.set("llc_version", llvm_version(llc));
    results.set("hod_version", version_line(hod).unwrap_or_else(|_| "hod version ?".to_owned()));
    results.set("hod_build", hod_build);
    results.set("producer", producer.version.clone());
    results.set("runs", config.runs.to_string());
    results.set("usage", probe.describe());
    if let Some(floor) = calibration.floor_us {
        results.set("floor_us", floor.to_string());
    }
    if let Some(error) = calibration.rss_error_kib {
        results.set("rss_error_kib", format!("{error:+}"));
    }
    if let Ok(load) = std::fs::read_to_string("/proc/loadavg") {
        results.set("load", load.split_whitespace().take(3).collect::<Vec<_>>().join(" "));
    }
    results
}

/// `--check`: judges the run against the baseline, measuring again what
/// fails. `Ok(None)`: there is no baseline at all.
fn gate(
    config: &Config,
    context: &Context<'_>,
    session: &mut Session,
    results: &mut Results,
) -> Result<Option<Check>, String> {
    let Some((path, baseline)) = find_baseline(config)? else { return Ok(None) };
    let mut again: BTreeMap<String, u32> = BTreeMap::new();
    let mut judged = loop {
        let judged = judge_all(&baseline, results, config.judge, &config.thresholds);
        let retry: Vec<&str> = judged
            .iter()
            .filter(|one| {
                let done = again.get(&one.input).copied().unwrap_or(0);
                one.verdict.remeasure() && done < config.retries
            })
            .map(|one| one.input.as_str())
            .collect();
        if retry.is_empty() {
            break judged;
        }
        // A failure must survive being measured again: the new runs join
        // the old ones, and the fastest of all of them is judged.
        for one in session.measured.iter_mut().filter(|one| retry.contains(&one.input.id.as_str()))
        {
            one.measure(context, config.runs);
            *again.entry(one.input.id.clone()).or_default() += 1;
        }
        results.records = session.records();
    };
    for one in &mut judged {
        one.remeasured = again.get(&one.input).copied().unwrap_or(0);
    }
    Ok(Some(Check {
        recorded: baseline.get("date").unwrap_or("?").to_owned(),
        advisory: check::comparable(&baseline, results).err(),
        baseline: path,
        judged,
        thresholds: config.thresholds,
    }))
}

/// `--report`: `docs/status/perf.md` from a results file.
fn write_report(config: &Config, mut run: Run) -> Result<Run, String> {
    let source = match (&config.results, config.llvm.as_deref()) {
        (Some(path), _) => path.clone(),
        (None, None) => config.bench_dir().join(format!("results-llvm{}.tsv", LLVM_MAJORS[0])),
        (None, Some(major)) if major.bytes().all(|b| b.is_ascii_digit()) => {
            config.bench_dir().join(format!("results-llvm{major}.tsv"))
        }
        (None, Some(_)) => {
            return Err("--report with an LLVM directory needs --results <file>".to_owned());
        }
    };
    let results = Results::read(&source)?;
    let out = config.out.clone().unwrap_or_else(|| config.workspace.join("docs/status/perf.md"));
    record::write_replacing(&out, &report::perf_md(&results))?;
    run.notes.push(format!("report from {}", source.display()));
    run.report = Some(out);
    Ok(run)
}

/// `cargo xtask bench <args>`: runs the command and returns its exit
/// status.
pub fn main(args: &[String]) -> std::process::ExitCode {
    let mut config = Config::for_workspace();
    match parse_args(args, &mut config) {
        Ok(true) => {}
        Ok(false) => {
            print!("{USAGE}");
            return std::process::ExitCode::SUCCESS;
        }
        Err(message) => {
            eprint!("xtask bench: {message}\n{USAGE}");
            return std::process::ExitCode::from(2);
        }
    }
    match run(&config) {
        Ok(run) => {
            print!("{}", run.summary(config.verbose));
            std::process::ExitCode::from(run.exit_status())
        }
        Err(message) => {
            eprintln!("xtask bench: {message}");
            std::process::ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(args: &[&str]) -> Result<Config, String> {
        let mut config = Config::new(PathBuf::from("/w"), PathBuf::from("/b"));
        let args: Vec<String> = args.iter().map(|&arg| arg.to_owned()).collect();
        parse_args(&args, &mut config).map(|_| config)
    }

    // spec/harness.md "bench", "Options".
    #[test]
    fn options() {
        let defaults = parsed(&[]).unwrap();
        assert_eq!(
            (defaults.action, defaults.runs, defaults.judge),
            (Action::Measure, 3, Tool::Hod)
        );
        assert!(defaults.filters.is_empty() && defaults.llvm.is_none() && !defaults.verbose);

        let all = parsed(&[
            "--check",
            "--runs",
            "5",
            "--filter=small",
            "--filter",
            "tables",
            "--llvm=19",
            "--hod",
            "/x/hod",
            "--kiln=/k/kiln++",
            "--judge",
            "hod+fallback",
            "--baseline",
            "b.tsv",
            "--results=r.tsv",
            "--out",
            "p.md",
            "-v",
        ])
        .unwrap();
        assert_eq!((all.action, all.runs, all.judge), (Action::Check, 5, Tool::HodFallback));
        assert_eq!(all.filters, ["small", "tables"]);
        assert_eq!(
            (all.llvm.as_deref(), all.hod, all.kiln),
            (Some("19"), Some("/x/hod".into()), Some("/k/kiln++".into()))
        );
        assert_eq!(
            (all.baseline, all.results, all.out),
            (Some("b.tsv".into()), Some("r.tsv".into()), Some("p.md".into()))
        );
        assert!(all.verbose);

        assert_eq!(parsed(&["--generate"]).unwrap().action, Action::Generate);
        assert_eq!(parsed(&["--report"]).unwrap().action, Action::Report);
        let update =
            parsed(&["--update-baseline", "--reason", "hod builds integer functions"]).unwrap();
        assert_eq!(update.action, Action::UpdateBaseline);
        assert_eq!(update.reason.as_deref(), Some("hod builds integer functions"));
        let mut config = Config::new(PathBuf::from("/w"), PathBuf::from("/b"));
        assert_eq!(parse_args(&["--help".to_owned()], &mut config), Ok(false));

        for (args, message) in [
            (&["--check", "--report"][..], "--check and --report are two actions; give one"),
            (
                &["--update-baseline"],
                "--update-baseline needs --reason \"<why the baseline changes>\"",
            ),
            (
                &["--update-baseline", "--reason", "  "],
                "--update-baseline needs --reason \"<why the baseline changes>\"",
            ),
            (&["--reason", "why"], "--reason goes with --update-baseline"),
            (
                &["--update-baseline", "--reason", "x", "--filter", "small"],
                "--update-baseline takes neither --filter nor --kiln: a baseline is every input of the manifest, produced by clang",
            ),
            (&["--runs", "0"], "invalid number of runs '0': 1 to 100"),
            (&["--runs", "many"], "invalid number of runs 'many': 1 to 100"),
            (&["--runs"], "'--runs' requires a value"),
            (&["--check=yes"], "'--check' takes no value"),
            (&["--judge", "llc"], "'--judge llc': expected hod or hod+fallback"),
            (&["--fast"], "unknown option '--fast'"),
            (
                &["small.ll"],
                "unexpected argument 'small.ll': the inputs are those of bench/manifest",
            ),
        ] {
            assert_eq!(parsed(args).unwrap_err(), message, "{args:?}");
        }
    }

    // "Results file", the header's `date`.
    #[test]
    fn utc_dates() {
        assert_eq!(utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc(951_782_399), "2000-02-28T23:59:59Z");
        assert_eq!(utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(utc(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(utc(4_102_444_800), "2100-01-01T00:00:00Z");
        assert_eq!(utc(4_107_542_399), "2100-02-28T23:59:59Z");
        assert_eq!(utc(4_107_542_400), "2100-03-01T00:00:00Z");
        assert!(now_utc().as_str() > "2026-01-01T00:00:00Z");
    }

    // "Phases": hod's report is `<phase><TAB><microseconds>` lines.
    #[test]
    fn phase_reports() {
        let scratch = hod_testutil::Scratch::new("bench-phases");
        assert_eq!(read_phases(&scratch.join("absent")), Ok(Vec::new()));
        let file = scratch.write("phases", "read\t410\ncompile\t3\ndebug-info\t0\ntotal\t520\n");
        assert_eq!(
            read_phases(&file).unwrap(),
            [
                ("read".to_owned(), 410),
                ("compile".to_owned(), 3),
                ("debug-info".to_owned(), 0),
                ("total".to_owned(), 520)
            ]
        );
        assert_eq!(read_phases(&scratch.write("empty", "")), Ok(Vec::new()));
        for bad in ["read 410\n", "Read\t410\n", "read\t4.1\n", "read\t\n", "read\t1\n\n"] {
            let error = read_phases(&scratch.write("bad", bad)).unwrap_err();
            assert!(error.starts_with("wrote a malformed phase report: '"), "{bad:?}: {error}");
        }
    }
}
