//! `cargo xtask roundtrip`: layer L2 of `docs/VALIDATION.md`, IR
//! ingestion. Every entry of the IR corpus is parsed by `hod_ll`, printed
//! by `hod_ir`'s printer in its LLVM major's dialect, accepted by
//! `llvm-as` and the verifier, built by the reference `llc`, linked and
//! run as `irdiff` runs the original, and must do what the entry says.
//! Contract: `spec/harness.md` "roundtrip".
//!
//! The build, link, run and comparison are `irdiff`'s engine
//! ([`crate::irdiff::Session`]), with `corpus capture`'s stand-in for hod
//! that declines every module, so that only the oracle's side runs: the
//! same way `corpus capture` admits an entry.

pub mod expected;
pub mod ingest;
pub mod stages;

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::corpus::{self, capture::DECLINE_SCRIPT, manifest::Manifest};
use crate::irdiff::{
    self, Arch, LINKER_NAMES, Limits, Linker, Mode, Search, Session, Test, Toolchain, directives,
    tools,
};

use expected::{Expectations, Verdict, verdict};

/// What became of one entry: exactly one of these.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Outcome {
    /// The printed module, built by `llc`, behaved as the entry says.
    Pass,
    /// `hod_ll` named a construct it does not support.
    ParserUnsupported,
    /// `hod_ll` refused the module, or what it made does not verify.
    ParserError,
    /// `hod_ll` panicked.
    ParserCrashed,
    /// The printer cannot write a construct in this dialect (`spec/ir.md`
    /// §9.5, §9.6).
    PrinterNotExpressible,
    /// The printer found the IR malformed.
    PrinterError,
    /// The printer panicked.
    PrinterCrashed,
    /// `llvm-as` did not accept the printed text.
    LlvmAsRefused,
    /// `opt -passes=verify` refused the printed module.
    VerifierRefused,
    /// `llc` failed on the printed module.
    LlcFailed,
    /// The object of the printed module did not link.
    LinkFailed,
    /// The program of the printed module did not do what the entry says.
    BehaviourDiffers,
    /// A tool (`llvm-as`, `opt`, `llc`, the driver or a linker) was
    /// killed by a signal.
    ToolCrashed,
    /// A tool ran into its time limit.
    ToolTimedOut,
    /// The harness could not do its work (an unreadable entry, a tool that
    /// cannot be started, an engine result it does not know).
    HarnessError,
    /// Not decided: a tool needed to decide it is missing, or the entry is
    /// for another architecture. Not compared with the expectation file.
    Skipped,
}

impl Outcome {
    pub const ALL: [Self; 16] = [
        Self::Pass,
        Self::ParserUnsupported,
        Self::ParserError,
        Self::ParserCrashed,
        Self::PrinterNotExpressible,
        Self::PrinterError,
        Self::PrinterCrashed,
        Self::LlvmAsRefused,
        Self::VerifierRefused,
        Self::LlcFailed,
        Self::LinkFailed,
        Self::BehaviourDiffers,
        Self::ToolCrashed,
        Self::ToolTimedOut,
        Self::HarnessError,
        Self::Skipped,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::ParserUnsupported => "parser-unsupported",
            Self::ParserError => "parser-error",
            Self::ParserCrashed => "parser-crashed",
            Self::PrinterNotExpressible => "printer-not-expressible",
            Self::PrinterError => "printer-error",
            Self::PrinterCrashed => "printer-crashed",
            Self::LlvmAsRefused => "llvm-as-refused",
            Self::VerifierRefused => "verifier-refused",
            Self::LlcFailed => "llc-failed",
            Self::LinkFailed => "link-failed",
            Self::BehaviourDiffers => "behaviour-differs",
            Self::ToolCrashed => "tool-crashed",
            Self::ToolTimedOut => "tool-timed-out",
            Self::HarnessError => "harness-error",
            Self::Skipped => "skipped",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|outcome| outcome.name() == name)
    }

    /// 0 for a pass, 1 for an honest "not supported" (the parser's or the
    /// printer's named construct), 2 for everything else: a bug in hod's
    /// parser or printer unless shown otherwise ("Verdicts").
    pub fn rank(self) -> u8 {
        match self {
            Self::Pass => 0,
            Self::ParserUnsupported | Self::PrinterNotExpressible => 1,
            _ => 2,
        }
    }
}

pub const USAGE: &str = "\
usage: cargo xtask roundtrip [options]

Parses every entry of the IR corpus with hod_ll, prints it with hod_ir in
its LLVM major's dialect, and checks the text with llvm-as, the verifier,
and llc (SelectionDAG at -O0): linked and run, it must do what the entry
says. Compares each entry's outcome with the expectation file.

options:
  --corpus <dir>       the corpus (default: <build root>/corpus)
  --llvm <N>           only entries of LLVM major N; repeatable
  --filter <text>      only entries whose path contains <text>; repeatable
  --limit <N>          only the first N selected entries of each major
  --linker <name>      bfd, lld, mold or wild; repeatable (default: all found)
  -j <N>, --jobs <N>   entries at once (default: min(4, cores); at most 16)
  --expected <file>    the expectation file (default: tests/roundtrip/expected-<arch>.tsv)
  --update-expected    write what this run found into the expectation file
  --results <file>     the results file (default: <build>/roundtrip/results-<arch>.tsv)
  --keep               keep the work directories of entries that fail
  -v, --verbose        list every entry that does not pass
  -h, --help           this text

exit status: 0 every entry as expected; 1 an entry worse or better than
expected; 3 a required tool missing (HOD_REQUIRE_TOOLS=1); 2 usage, or no
corpus
";

/// A run of the command.
#[derive(Clone, Debug)]
pub struct Options {
    pub corpus: PathBuf,
    /// The majors to run; empty is every major of the corpus.
    pub llvm: Vec<u32>,
    pub filters: Vec<String>,
    pub limit: Option<usize>,
    /// The linkers; empty is every one found.
    pub linkers: Vec<String>,
    pub jobs: usize,
    pub expected: PathBuf,
    pub update: bool,
    pub results: Option<PathBuf>,
    pub keep: bool,
    pub verbose: bool,
    pub build_dir: PathBuf,
    pub require_tools: bool,
    /// For `llvm-as`, `opt`, `llc` and the linker driver.
    pub tool_limits: Limits,
    /// For the programs.
    pub run_limits: Limits,
    pub search: Search,
    /// The architecture of the programs: this host's.
    pub host: Option<Arch>,
}

impl Options {
    /// The defaults, with output under `build_dir`; limits as `irdiff`'s.
    pub fn new(build_dir: PathBuf) -> Self {
        let defaults = irdiff::Config::new(build_dir.clone());
        let host = Arch::host();
        let arch = host.map_or("unknown", Arch::name);
        Self {
            corpus: corpus::default_dir(&build_dir),
            llvm: Vec::new(),
            filters: Vec::new(),
            limit: None,
            linkers: Vec::new(),
            jobs: defaults.jobs,
            expected: irdiff::workspace_root()
                .join("tests")
                .join("roundtrip")
                .join(format!("expected-{arch}.tsv")),
            update: false,
            results: None,
            keep: false,
            verbose: false,
            build_dir,
            require_tools: false,
            tool_limits: defaults.tool_limits,
            run_limits: defaults.run_limits,
            search: defaults.search,
            host,
        }
    }

    /// The defaults for this workspace and environment
    /// (`HOD_REQUIRE_TOOLS`).
    pub fn for_workspace() -> Self {
        let mut options = Self::new(irdiff::default_build_dir());
        options.require_tools = std::env::var_os("HOD_REQUIRE_TOOLS").is_some_and(|v| v == "1");
        options
    }
}

/// Reads the command line into `options`. `Ok(false)`: `--help`.
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
        let number = |text: String, what: &str| {
            text.parse::<usize>()
                .ok()
                .filter(|n| *n >= 1)
                .ok_or_else(|| format!("invalid {what} '{text}'"))
        };
        match name {
            "-h" | "--help" => return Ok(false),
            "--corpus" => options.corpus = PathBuf::from(value()?),
            "--llvm" => {
                let text = value()?;
                let major = text
                    .parse::<u32>()
                    .ok()
                    .filter(|major| tools::LLVM_MAJORS.contains(major))
                    .ok_or_else(|| format!("invalid LLVM major '{text}': expected 22 or 19"))?;
                if !options.llvm.contains(&major) {
                    options.llvm.push(major);
                }
            }
            "--filter" => options.filters.push(value()?),
            "--limit" => options.limit = Some(number(value()?, "limit")?),
            "--linker" => {
                let linker = value()?;
                if !LINKER_NAMES.contains(&linker.as_str()) {
                    return Err(format!(
                        "unknown linker '{linker}': expected bfd, lld, mold or wild"
                    ));
                }
                options.linkers.push(linker);
            }
            "-j" | "--jobs" => options.jobs = number(value()?, "job count")?.min(irdiff::MAX_JOBS),
            "--expected" => options.expected = PathBuf::from(value()?),
            "--update-expected" => {
                flag(&joined)?;
                options.update = true;
            }
            "--results" => options.results = Some(PathBuf::from(value()?)),
            "--keep" => {
                flag(&joined)?;
                options.keep = true;
            }
            "-v" | "--verbose" => {
                flag(&joined)?;
                options.verbose = true;
            }
            _ => return Err(format!("unknown argument '{arg}'")),
        }
    }
    Ok(true)
}

/// One entry's result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryResult {
    /// The entry's path in the corpus.
    pub entry: String,
    pub llvm: u32,
    pub outcome: Outcome,
    /// The construct, the message, or what differed; empty for a pass.
    pub detail: String,
    /// What the expectation file says (`None`: not listed, must pass).
    pub expected: Option<Outcome>,
    pub verdict: Verdict,
}

/// What a run found.
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub configuration: String,
    /// In order of LLVM major (newest first), then entry.
    pub results: Vec<EntryResult>,
    pub notes: Vec<String>,
    pub missing_tools: Vec<String>,
    /// Entries the expectation file lists that a complete run did not
    /// meet: the file is stale.
    pub stale: Vec<String>,
    pub expected_file: PathBuf,
    pub updated: bool,
    pub results_file: Option<PathBuf>,
    pub kept: Option<PathBuf>,
    pub elapsed: Duration,
}

impl Report {
    /// How many entries of `major` had `outcome`.
    pub fn count(&self, major: u32, outcome: Outcome) -> usize {
        self.results.iter().filter(|r| r.llvm == major && r.outcome == outcome).count()
    }

    fn with_verdict(&self, wanted: Verdict) -> usize {
        self.results.iter().filter(|r| r.verdict == wanted).count()
    }

    /// "Exit status".
    pub fn exit_status(&self) -> u8 {
        let off = self.with_verdict(Verdict::Worse) + self.with_verdict(Verdict::Better);
        if !self.updated && (off > 0 || !self.stale.is_empty()) {
            1
        } else if !self.missing_tools.is_empty() {
            3
        } else {
            0
        }
    }

    /// The results file: one line per entry, six tab-separated fields.
    pub fn results_text(&self) -> String {
        use corpus::manifest::escape;
        let mut out = String::new();
        for r in &self.results {
            let _ = writeln!(
                out,
                "{}\t{}\t{}\t{}\t{}\t{}",
                r.outcome.name(),
                escape(&r.entry),
                r.llvm,
                r.verdict.name(),
                r.expected.unwrap_or(Outcome::Pass).name(),
                escape(&r.detail)
            );
        }
        out
    }

    /// The summary ("Summary").
    pub fn summary(&self, verbose: bool) -> String {
        let mut out = format!("roundtrip: {}\n", self.configuration);
        for note in &self.notes {
            let _ = writeln!(out, "roundtrip: note: {note}");
        }
        for tool in &self.missing_tools {
            let _ = writeln!(out, "roundtrip: error: {tool} (HOD_REQUIRE_TOOLS=1)");
        }
        for r in &self.results {
            let shown = match r.verdict {
                Verdict::Worse | Verdict::Better => true,
                Verdict::Expected | Verdict::Unchecked => verbose && r.outcome != Outcome::Pass,
            };
            if shown {
                let expected = r.expected.unwrap_or(Outcome::Pass).name();
                let _ = writeln!(
                    out,
                    "{:<9} {}: {} (expected {expected}): {}",
                    r.verdict.name(),
                    r.entry,
                    r.outcome.name(),
                    stages::clip(&r.detail, 300)
                );
            }
        }
        for entry in &self.stale {
            let _ = writeln!(
                out,
                "STALE     {entry}: listed in the expectation file, not in the corpus"
            );
        }
        let mut majors: Vec<u32> = self.results.iter().map(|r| r.llvm).collect();
        majors.sort_unstable_by(|a, b| b.cmp(a));
        majors.dedup();
        for major in majors {
            let total = self.results.iter().filter(|r| r.llvm == major).count();
            let counts: Vec<String> = Outcome::ALL
                .into_iter()
                .filter_map(|outcome| match self.count(major, outcome) {
                    0 => None,
                    n => Some(format!("{n} {}", outcome.name())),
                })
                .collect();
            let _ =
                writeln!(out, "roundtrip: LLVM {major}: {total} entries: {}", counts.join(", "));
        }
        let _ = writeln!(
            out,
            "roundtrip: {} entries, {} as expected, {} worse, {} better, {} unchecked, {} stale \
             (against {}) in {:.1}s",
            self.results.len(),
            self.with_verdict(Verdict::Expected),
            self.with_verdict(Verdict::Worse),
            self.with_verdict(Verdict::Better),
            self.with_verdict(Verdict::Unchecked),
            self.stale.len(),
            self.expected_file.display(),
            self.elapsed.as_secs_f64()
        );
        if self.updated {
            let _ = writeln!(out, "roundtrip: updated {}", self.expected_file.display());
        }
        if let Some(kept) = &self.kept {
            let _ =
                writeln!(out, "roundtrip: kept the work of failing entries in {}", kept.display());
        }
        if let Some(results) = &self.results_file {
            let _ = writeln!(out, "roundtrip: results: {}", results.display());
        }
        out
    }
}

/// Everything that is the same for the entries of one LLVM major.
struct Group {
    major: u32,
    llvm_as: Option<PathBuf>,
    opt: Option<PathBuf>,
    /// The engine, when every tool it needs is there.
    session: Option<Session>,
    /// What is missing, when something is: the entries' tool steps are
    /// `skipped`.
    missing: Option<String>,
    work: PathBuf,
}

/// Records that a tool is missing: a failure when it is required and
/// tools are required, else a note.
fn missing(options: &Options, report: &mut Report, what: String, required: bool) {
    if required && options.require_tools {
        report.missing_tools.push(what);
    } else {
        report.notes.push(what);
    }
}

fn write_script(path: &Path, text: &str) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::write(path, text).map_err(|e| format!("cannot write '{}': {e}", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("cannot write '{}': {e}", path.display()))
}

/// Finds the tools of each major and prepares its engine.
fn groups(
    options: &Options,
    majors: &[u32],
    host: Arch,
    run: &Path,
    report: &mut Report,
) -> Result<Vec<Group>, String> {
    let search = &options.search;
    let driver = tools::find_driver(search);
    if driver.is_none() {
        missing(options, report, "linker driver 'gcc' not found".to_owned(), true);
    }
    let mut linkers = Vec::new();
    for name in LINKER_NAMES {
        let asked = options.linkers.iter().any(|wanted| wanted == name);
        if !(asked || options.linkers.is_empty()) {
            continue;
        }
        match tools::find_linker(name, search) {
            Some(path) => linkers.push(Linker { name: name.to_owned(), path }),
            None if asked => return Err(format!("linker '{name}' was asked for and is not found")),
            None => {
                let required = tools::REQUIRED_LINKERS.contains(&name);
                missing(
                    options,
                    report,
                    format!("linker '{name}' not found; not in this run"),
                    required,
                );
            }
        }
    }
    let decline = run.join("decline-hod");
    write_script(&decline, DECLINE_SCRIPT)?;

    let mut groups = Vec::new();
    for &major in majors {
        let mut absent = Vec::new();
        let llc = match tools::find_llvm(Some(&major.to_string()), search) {
            Ok(Some(llvm)) if llvm.major == major => Some(llvm.llc),
            Ok(_) => None,
            Err(e) => {
                report.notes.push(format!("LLVM {major}: {e}"));
                None
            }
        };
        let llvm_as = search.versioned("llvm-as", major);
        let opt = search.versioned("opt", major);
        for (name, found) in
            [("llc", llc.is_some()), ("llvm-as", llvm_as.is_some()), ("opt", opt.is_some())]
        {
            if !found {
                absent.push(format!("{name} {major}"));
            }
        }
        if driver.is_none() {
            absent.push("gcc".to_owned());
        }
        if linkers.is_empty() {
            absent.push("a linker".to_owned());
        }
        let work = run.join(format!("llvm{major}"));
        fs::create_dir_all(&work)
            .map_err(|e| format!("cannot create '{}': {e}", work.display()))?;
        let session = match (&llc, &driver) {
            (Some(llc), Some(driver)) if absent.is_empty() => {
                let wrapper = run.join(format!("llc-{major}"));
                write_script(&wrapper, &stages::reference_llc_script(llc)?)?;
                let session = Session {
                    toolchain: Toolchain {
                        llc: wrapper,
                        llvm_major: major,
                        hod: decline.clone(),
                        driver: Some(driver.clone()),
                        linkers: linkers.clone(),
                    },
                    arch: host,
                    host: Some(host),
                    mode: Mode::Strict,
                    tool_limits: options.tool_limits,
                    run_limits: options.run_limits,
                    work: work.join("engine"),
                    keep: false,
                };
                session.prepare()?;
                Some(session)
            }
            _ => None,
        };
        let llvm_absent: Vec<&str> = absent
            .iter()
            .filter(|a| a.ends_with(&format!(" {major}")))
            .map(String::as_str)
            .collect();
        if !llvm_absent.is_empty() {
            let what = format!(
                "LLVM {major}: {} not found; its entries are parsed and printed only",
                llvm_absent.join(", ")
            );
            missing(options, report, what, true);
        }
        let missing_text =
            (!absent.is_empty()).then(|| format!("not found: {}", absent.join(", ")));
        groups.push(Group { major, llvm_as, opt, session, missing: missing_text, work });
    }
    Ok(groups)
}

/// Runs one entry through every step.
fn run_entry(
    options: &Options,
    group: &Group,
    host: Arch,
    index: usize,
    entry: &str,
) -> (Outcome, String) {
    let path = options.corpus.join(entry);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) => return (Outcome::HarnessError, format!("cannot read '{}': {e}", path.display())),
    };
    let expect = match directives::parse(&text) {
        Ok(expect) => expect,
        Err(errors) => {
            let all: Vec<String> = errors.iter().map(ToString::to_string).collect();
            return (Outcome::HarnessError, format!("directives: {}", all.join("; ")));
        }
    };
    if !expect.runs_on(host) || !expect.accepts_llvm(group.major) {
        return (Outcome::Skipped, "the entry is for another target or LLVM".to_owned());
    }
    let printed = match ingest::ingest(text.as_bytes(), group.major) {
        Ok(printed) => printed,
        Err(failed) => return failed,
    };
    let (Some(session), Some(llvm_as), Some(opt)) = (&group.session, &group.llvm_as, &group.opt)
    else {
        let why = group.missing.clone().unwrap_or_default();
        return (Outcome::Skipped, format!("parsed and printed; {why}"));
    };
    let dir = group.work.join(format!("{index:05}"));
    let result = check_printed(
        session,
        llvm_as,
        opt,
        options.tool_limits,
        &dir,
        index,
        entry,
        &printed,
        expect,
    );
    let keep = options.keep && result.0.rank() == 2;
    if !keep {
        let _ = fs::remove_dir_all(&dir);
    }
    result
}

/// Steps 3 to 5 for the printed text of an entry, in `dir`.
#[allow(clippy::too_many_arguments)]
fn check_printed(
    session: &Session,
    llvm_as: &Path,
    opt: &Path,
    limits: Limits,
    dir: &Path,
    index: usize,
    entry: &str,
    printed: &str,
    expect: directives::Expectations,
) -> (Outcome, String) {
    let ll = dir.join("printed.ll");
    let bc = dir.join("printed.bc");
    if let Err(e) = fs::create_dir_all(dir).and_then(|()| fs::write(&ll, printed)) {
        return (Outcome::HarnessError, format!("cannot write '{}': {e}", ll.display()));
    }
    let as_args: [&OsStr; 4] =
        ["-disable-verify".as_ref(), "-o".as_ref(), bc.as_os_str(), ll.as_os_str()];
    if let Err(failed) =
        stages::tool_step(llvm_as, "llvm-as", &as_args, limits, Outcome::LlvmAsRefused)
    {
        return failed;
    }
    let opt_args: [&OsStr; 3] =
        ["-passes=verify".as_ref(), "-disable-output".as_ref(), bc.as_os_str()];
    if let Err(failed) = stages::tool_step(opt, "opt", &opt_args, limits, Outcome::VerifierRefused)
    {
        return failed;
    }
    let test = Test { name: entry.to_owned(), ir: ll, expect };
    let records = session.run_test(index, &test);
    match stages::from_records(&records, session.toolchain.linkers.len()) {
        Ok(()) => (Outcome::Pass, String::new()),
        Err(failed) => failed,
    }
}

/// The selected entries: `(major, entry)`, newest major first, entries in
/// order.
fn select(options: &Options, manifest: &Manifest) -> Vec<(u32, String)> {
    let mut by_major: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    for row in manifest.rows.values() {
        let wanted = options.llvm.is_empty() || options.llvm.contains(&row.llvm);
        let filtered = options.filters.is_empty()
            || options.filters.iter().any(|f| row.entry.contains(f.as_str()));
        if wanted && filtered {
            by_major.entry(row.llvm).or_default().push(row.entry.clone());
        }
    }
    let mut selected = Vec::new();
    for (major, mut entries) in by_major.into_iter().rev() {
        entries.sort();
        entries.truncate(options.limit.unwrap_or(usize::MAX));
        selected.extend(entries.into_iter().map(|entry| (major, entry)));
    }
    selected
}

/// Runs the command. `Err` is exit status 2: nothing was run.
pub fn run(options: &Options) -> Result<Report, String> {
    let started = Instant::now();
    let host =
        options.host.ok_or("this host is not a Linux machine of a supported architecture")?;
    if !options.corpus.join(corpus::manifest::MANIFEST).is_file() {
        return Err(format!(
            "no corpus at '{}' (cargo xtask corpus capture)",
            options.corpus.display()
        ));
    }
    let manifest = Manifest::read(&options.corpus)?;
    let selected = select(options, &manifest);
    if selected.is_empty() {
        return Err("no corpus entry is selected".to_owned());
    }
    let mut file = Expectations::read(&options.expected)?;
    let mut majors: Vec<u32> = selected.iter().map(|(major, _)| *major).collect();
    majors.dedup();

    let mut report = Report { expected_file: options.expected.clone(), ..Report::default() };
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let run =
        options.build_dir.join("roundtrip").join(format!("run-{}-{stamp}", std::process::id()));
    fs::create_dir_all(&run).map_err(|e| format!("cannot create '{}': {e}", run.display()))?;
    let groups = match groups(options, &majors, host, &run, &mut report) {
        Ok(groups) => groups,
        Err(e) => {
            let _ = fs::remove_dir_all(&run);
            return Err(e);
        }
    };
    let jobs = options.jobs.clamp(1, irdiff::MAX_JOBS);
    report.configuration = describe(options, &groups, jobs);

    let next = AtomicUsize::new(0);
    let slots = Mutex::new(vec![None; selected.len()]);
    std::thread::scope(|scope| {
        for _ in 0..jobs.min(selected.len()) {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some((major, entry)) = selected.get(index) else { break };
                    let group =
                        groups.iter().find(|g| g.major == *major).expect("a group per major");
                    let result = run_entry(options, group, host, index, entry);
                    slots.lock().unwrap_or_else(PoisonError::into_inner)[index] = Some(result);
                }
            });
        }
    });
    let slots = slots.into_inner().unwrap_or_else(PoisonError::into_inner);
    for ((major, entry), slot) in selected.iter().zip(slots) {
        let (outcome, detail) = slot.unwrap_or((Outcome::HarnessError, "not run".to_owned()));
        let expected = file.outcome(entry);
        report.results.push(EntryResult {
            entry: entry.clone(),
            llvm: *major,
            outcome,
            detail,
            expected,
            verdict: verdict(outcome, expected),
        });
    }
    // A complete run also checks that every listed entry still exists.
    let complete = options.filters.is_empty() && options.limit.is_none() && options.llvm.is_empty();
    if complete {
        report.stale =
            file.entries.keys().filter(|e| !manifest.rows.contains_key(*e)).cloned().collect();
    }
    if options.update {
        for r in &report.results {
            file.update(&r.entry, r.outcome, &r.detail);
        }
        for entry in &report.stale {
            file.entries.remove(entry);
        }
        file.write(&options.expected)?;
        report.updated = true;
    }
    let kept = options.keep
        && report.results.iter().any(|r| r.outcome.rank() == 2 && r.outcome != Outcome::Skipped);
    if kept {
        report.kept = Some(run.clone());
    } else {
        let _ = fs::remove_dir_all(&run);
    }
    report.elapsed = started.elapsed();
    let results = options.results.clone().unwrap_or_else(|| {
        options.build_dir.join("roundtrip").join(format!("results-{}.tsv", host.name()))
    });
    if let Some(dir) = results.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("cannot create '{}': {e}", dir.display()))?;
    }
    fs::write(&results, report.results_text())
        .map_err(|e| format!("cannot write '{}': {e}", results.display()))?;
    report.results_file = Some(results);
    Ok(report)
}

/// The summary's first line: what ran.
fn describe(options: &Options, groups: &[Group], jobs: usize) -> String {
    let majors: Vec<String> = groups.iter().map(|g| g.major.to_string()).collect();
    let linkers: Vec<String> = groups
        .iter()
        .find_map(|g| g.session.as_ref())
        .map(|s| s.toolchain.linkers.iter().map(|l| l.name.clone()).collect())
        .unwrap_or_default();
    let pool = crate::jobs::global()
        .describe()
        .map_or_else(String::new, |pool| format!(" (jobserver {pool})"));
    format!(
        "corpus {}; LLVM {}; llc -O0 {}; linkers {}; jobs {jobs}{pool}",
        options.corpus.display(),
        majors.join(" "),
        stages::REFERENCE_FLAGS.join(" "),
        if linkers.is_empty() { "none".to_owned() } else { linkers.join(" ") },
    )
}

/// `cargo xtask roundtrip`: the exit status.
pub fn command(args: &[String]) -> u8 {
    let mut options = Options::for_workspace();
    match parse_args(args, &mut options) {
        Ok(true) => {}
        Ok(false) => {
            print!("{USAGE}");
            return 0;
        }
        Err(message) => {
            eprint!("xtask roundtrip: {message}\n{USAGE}");
            return 2;
        }
    }
    match run(&options) {
        Ok(report) => {
            print!("{}", report.summary(options.verbose));
            report.exit_status()
        }
        Err(message) => {
            eprintln!("xtask roundtrip: {message}");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec/harness.md "roundtrip", "Outcomes": names are stable and
    // distinct.
    #[test]
    fn outcome_names() {
        for outcome in Outcome::ALL {
            assert_eq!(Outcome::from_name(outcome.name()), Some(outcome));
        }
        assert_eq!(Outcome::from_name("PASS"), None);
        assert_eq!(Outcome::Pass.rank(), 0);
        assert_eq!(Outcome::PrinterNotExpressible.rank(), 1);
        assert_eq!(Outcome::BehaviourDiffers.rank(), 2);
    }

    // "Options".
    #[test]
    fn options() {
        let mut options = Options::new(PathBuf::from("/b"));
        assert_eq!(options.corpus, PathBuf::from("/b/corpus"));
        let args: Vec<String> = [
            "--llvm",
            "22",
            "--llvm=19",
            "--filter",
            "torture",
            "--limit=3",
            "-j2",
            "--linker",
            "bfd",
            "--expected",
            "/e.tsv",
            "--update-expected",
            "--results=/r.tsv",
            "--keep",
            "-v",
            "--corpus",
            "/c",
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
        assert_eq!(parse_args(&args, &mut options), Ok(true));
        assert_eq!(options.llvm, [22, 19]);
        assert_eq!(options.filters, ["torture"]);
        assert_eq!(options.limit, Some(3));
        assert_eq!(options.jobs, 2);
        assert_eq!(options.linkers, ["bfd"]);
        assert_eq!(options.expected, PathBuf::from("/e.tsv"));
        assert!(options.update && options.keep && options.verbose);
        assert_eq!(options.results, Some(PathBuf::from("/r.tsv")));
        assert_eq!(options.corpus, PathBuf::from("/c"));
        let bad = |args: &[&str]| {
            let args: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
            parse_args(&args, &mut Options::new(PathBuf::from("/b")))
        };
        assert_eq!(bad(&["--help"]), Ok(false));
        assert!(bad(&["--llvm", "18"]).is_err());
        assert!(bad(&["--limit", "0"]).is_err());
        assert!(bad(&["--linker", "gold"]).is_err());
        assert!(bad(&["--keep=1"]).is_err());
        assert!(bad(&["--filter"]).is_err());
        assert!(bad(&["stray"]).is_err());
    }
}
