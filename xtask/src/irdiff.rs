//! `cargo xtask irdiff`: the IR differential harness, layer L4 of
//! `docs/VALIDATION.md`. Contract: `spec/harness.md` "irdiff" and "Test
//! directives".
//!
//! Each test is an LLVM IR module with a `main`, or several modules that
//! make one program. It is compiled by the oracle (`llc -O0` with the
//! `SelectionDAG` selector) and by hod, the objects are linked by every
//! linker of the matrix, and the programs must behave as the test's `;@`
//! directives say and as each other. With the witness, a second oracle
//! build (`llc -O2`) must behave the same.
//!
//! - [`run`] is the command as a function: find the tools, read the tests,
//!   run them, write the results file.
//! - [`Session`] is the core that other harness commands reuse on IR they
//!   produce themselves: [`Session::run_tests`] takes [`Test`]s (a path
//!   and an [`Expectations`]) and returns [`Record`]s, and depends neither
//!   on `tests/ir/` nor on directives being in the file.

pub mod directives;
pub mod engine;
pub mod process;
pub mod report;
pub mod tools;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

pub use directives::{Arch, Expectations, LINKER_NAMES, Tier};
pub use engine::{Mode, Session, Test};
pub use process::Limits;
pub use report::{BuiltBy, Outcome, Record, Report};
pub use tools::{Linker, Search, Toolchain};

/// The command's help.
pub const USAGE: &str = "\
usage: cargo xtask irdiff [options] [<file.ll or directory>...]

Builds each test with the oracle (llc -O0, SelectionDAG) and with hod, links
both objects with every linker of the matrix, runs the programs and compares
them. The default is every test under tests/ir/.

  -j <N>, --jobs <N>   tests run at once (default 4, at most 16)
  --llvm <major|dir>   the oracle: an LLVM major (22, 19) or an installation
                       directory (default: the newest of 22 and 19 found)
  --target <arch>      aarch64 or x86_64 (default: this host)
  --linker <name>      bfd, lld, mold or wild; repeatable (default: every
                       one that is found)
  --tier <quick|all>   the tests to run (default: all)
  --hod <binary>       the hod to test (default: build this workspace's)
  --hybrid             let hod fall back to the oracle, and report coverage
  --witness            also build each test with llc -O2 and require its
                       program to behave as the oracle's (else an ERROR)
  --keep               keep the work directory of the failing tests
  --results <file>     the results file (default: under the build directory)
  -v, --verbose        list every result, not only the notable ones

exit status: 0 no failure; 1 a FAIL or XPASS; 3 no FAIL but an ERROR or a
missing required tool; 2 usage
";

/// The default of `-j`.
pub const DEFAULT_JOBS: usize = 4;
/// The most `-j` gives.
pub const MAX_JOBS: usize = 16;

/// Which tests of those found are run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TierFilter {
    /// Only the tests marked `tier: quick`.
    Quick,
    /// Every test.
    All,
}

/// A run of the command.
#[derive(Clone, Debug)]
#[expect(clippy::struct_excessive_bools, reason = "one field per option of the command")]
pub struct Config {
    /// Test files and directories (searched for `*.ll`, recursively).
    /// Empty means the workspace's `tests/ir`.
    pub paths: Vec<PathBuf>,
    /// The oracle: an LLVM major or an installation directory.
    pub llvm: Option<String>,
    /// The target; `None` is this host.
    pub target: Option<Arch>,
    /// The linkers to use; empty means every one that is found.
    pub linkers: Vec<String>,
    pub tier: TierFilter,
    pub mode: Mode,
    /// The hod to test; `None` builds the workspace's.
    pub hod: Option<PathBuf>,
    /// Tests run at once.
    pub jobs: usize,
    /// Keep the work directory of failing tests.
    pub keep: bool,
    /// Confirm the oracle of every test with an `llc -O2` build.
    pub witness: bool,
    /// List every result in the summary (used by the command, not by
    /// [`run`]).
    pub verbose: bool,
    /// The build directory: the work directories and the results file go
    /// under `<build_dir>/irdiff/`, and the workspace's hod is built in it.
    pub build_dir: PathBuf,
    /// The results file; `None` is
    /// `<build_dir>/irdiff/results-<arch>-llvm<major>.tsv`, with `-hybrid`
    /// before the extension in hybrid mode.
    pub results: Option<PathBuf>,
    /// Whether a missing tool is a failure (`HOD_REQUIRE_TOOLS=1`).
    pub require_tools: bool,
    /// Limits of `llc`, `hod` and the linker driver.
    pub tool_limits: Limits,
    /// Limits of the programs under test.
    pub run_limits: Limits,
    /// Where tools are looked for.
    pub search: Search,
}

impl Config {
    /// The defaults of the command, with output under `build_dir`.
    pub fn new(build_dir: PathBuf) -> Self {
        let cores = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
        Self {
            paths: Vec::new(),
            llvm: None,
            target: None,
            linkers: Vec::new(),
            tier: TierFilter::All,
            mode: Mode::Strict,
            hod: None,
            jobs: DEFAULT_JOBS.min(cores),
            keep: false,
            witness: false,
            verbose: false,
            build_dir,
            results: None,
            require_tools: false,
            tool_limits: Limits { time: Duration::from_mins(1), memory_kib: 4 << 20 },
            run_limits: Limits { time: Duration::from_secs(10), memory_kib: 1 << 20 },
            search: Search::from_env(),
        }
    }

    /// The defaults for this workspace and this process's environment:
    /// its build directory, and `HOD_REQUIRE_TOOLS`.
    pub fn for_workspace() -> Self {
        let mut config = Self::new(default_build_dir());
        config.require_tools = std::env::var_os("HOD_REQUIRE_TOOLS").is_some_and(|v| v == "1");
        config
    }
}

/// The workspace this harness was built from.
pub fn workspace_root() -> PathBuf {
    let xtask = Path::new(env!("CARGO_MANIFEST_DIR"));
    xtask.parent().unwrap_or(xtask).to_path_buf()
}

/// This checkout's build directory: `$CARGO_TARGET_DIR` if it is set and
/// not empty, else `<workspace>/target`.
pub fn default_build_dir() -> PathBuf {
    match std::env::var_os("CARGO_TARGET_DIR").filter(|dir| !dir.is_empty()) {
        Some(dir) => std::path::absolute(&dir).unwrap_or_else(|_| PathBuf::from(dir)),
        None => workspace_root().join("target"),
    }
}

/// Builds the workspace's `hod` (`cargo build --package hod_cli --bin
/// hod`, the development profile) into `build_dir` and returns the binary.
pub fn build_workspace_hod(build_dir: &Path) -> Result<PathBuf, String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| env!("CARGO").into());
    // A cargo step: a jobserver client of its own, stopped with xtask
    // ("Jobserver").
    // It is run once more if an artifact vanished under it ("ci").
    let mut retried = false;
    loop {
        let mut step = Command::new(&cargo);
        step.args(["build", "--quiet", "--package", "hod_cli", "--bin", "hod"])
            .current_dir(workspace_root())
            .env("CARGO_TARGET_DIR", build_dir);
        let (built, stderr) = crate::jobs::children::run_step(&mut step, true)
            .map_err(|e| format!("cannot run '{}': {e}", Path::new(&cargo).display()))?;
        if built.success() {
            break;
        }
        if !crate::ci::should_retry(&stderr, retried) {
            return Err("building the workspace's hod failed".to_owned());
        }
        eprintln!("xtask irdiff: [retry] the build of hod failed with error[E0463]; once more");
        retried = true;
    }
    let hod = build_dir.join("debug").join("hod");
    if hod.is_file() {
        Ok(hod)
    } else {
        Err(format!("the build did not produce '{}'", hod.display()))
    }
}

/// A test's name: its path relative to the workspace when it is inside,
/// else the path as given.
fn test_name(path: &Path) -> String {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let shown = absolute.strip_prefix(workspace_root()).unwrap_or(path);
    shown.to_string_lossy().into_owned()
}

/// The test files `paths` name: a file is itself, a directory is every
/// `*.ll` beneath it. Sorted by name, without duplicates.
pub fn collect(paths: &[PathBuf]) -> Result<Vec<(String, PathBuf)>, String> {
    let default = [workspace_root().join("tests").join("ir")];
    let paths = if paths.is_empty() { &default[..] } else { paths };
    let mut found = Vec::new();
    for path in paths {
        if path.is_dir() {
            let mut pending = vec![path.clone()];
            while let Some(dir) = pending.pop() {
                let failed = |e: std::io::Error| format!("cannot read '{}': {e}", dir.display());
                for entry in std::fs::read_dir(&dir).map_err(failed)? {
                    let entry = entry.map_err(failed)?.path();
                    if entry.is_dir() {
                        pending.push(entry);
                    } else if entry.extension().is_some_and(|ext| ext == "ll") {
                        found.push((test_name(&entry), entry));
                    }
                }
            }
        } else if path.is_file() {
            found.push((test_name(path), path.clone()));
        } else {
            return Err(format!("'{}' is not a file or a directory", path.display()));
        }
    }
    found.sort();
    found.dedup();
    if found.is_empty() {
        let shown: Vec<String> = paths.iter().map(|p| format!("'{}'", p.display())).collect();
        return Err(format!("no test (*.ll) in {}", shown.join(", ")));
    }
    Ok(found)
}

/// A test that was read, or the reason it cannot be run.
enum Planned {
    Run(Box<Test>),
    Broken { name: String, detail: String },
}

/// Reads the tests `config` selects, in name order.
fn plan(config: &Config) -> Result<Vec<Planned>, String> {
    let mut planned = Vec::new();
    for (name, path) in collect(&config.paths)? {
        let read = std::fs::read_to_string(&path).map_err(|e| format!("cannot read the test: {e}"));
        match read.and_then(|text| {
            directives::parse(&text).map_err(|errors| {
                let all: Vec<String> = errors.iter().map(ToString::to_string).collect();
                format!("directives: {}", all.join("; "))
            })
        }) {
            Ok(expect) if config.tier == TierFilter::Quick && expect.tier != Tier::Quick => {}
            Ok(expect) => planned.push(Planned::Run(Box::new(Test { name, ir: path, expect }))),
            // A test that cannot be read is reported whatever the tier.
            Err(detail) => planned.push(Planned::Broken { name, detail }),
        }
    }
    if planned.is_empty() {
        return Err("no test is selected (no test found is in the quick tier)".to_owned());
    }
    Ok(planned)
}

/// Records that a tool is missing: a failure when it is required and
/// tools are required, else a note.
fn missing(config: &Config, report: &mut Report, what: String, required: bool) {
    if required && config.require_tools {
        report.missing_tools.push(what);
    } else {
        report.notes.push(what);
    }
}

/// Finds everything but the oracle: the hod under test, the linker driver
/// and the linkers.
fn toolchain(
    config: &Config,
    llvm: &tools::Llvm,
    report: &mut Report,
) -> Result<Toolchain, String> {
    let hod = match &config.hod {
        Some(hod) if hod.is_file() => std::path::absolute(hod).unwrap_or_else(|_| hod.clone()),
        Some(hod) => return Err(format!("'{}' is not a file", hod.display())),
        None => build_workspace_hod(&config.build_dir)?,
    };
    let driver = tools::find_driver(&config.search);
    if driver.is_none() {
        let what = "linker driver 'gcc' not found; nothing is linked or run";
        missing(config, report, what.to_owned(), true);
    }
    let mut linkers = Vec::new();
    for name in LINKER_NAMES {
        let asked = config.linkers.iter().any(|wanted| wanted == name);
        if !(asked || config.linkers.is_empty()) {
            continue;
        }
        match tools::find_linker(name, &config.search) {
            Some(path) => linkers.push(Linker { name: name.to_owned(), path }),
            None if asked => return Err(format!("linker '{name}' was asked for and is not found")),
            None => {
                let required = tools::REQUIRED_LINKERS.contains(&name);
                missing(
                    config,
                    report,
                    format!("linker '{name}' not found; not in this run"),
                    required,
                );
            }
        }
    }
    Ok(Toolchain { llc: llvm.llc.clone(), llvm_major: llvm.major, hod, driver, linkers })
}

/// Runs the command: finds the tools, reads the tests, runs them and
/// writes the results file. An error is a request that cannot be carried
/// out at all (the command's exit status 2); everything else is in the
/// report.
pub fn run(config: &Config) -> Result<Report, String> {
    let started = Instant::now();
    let host = Arch::host();
    let arch = config.target.or(host).ok_or(
        "this host is not a Linux machine of a supported architecture; pass --target <arch>",
    )?;
    for name in &config.linkers {
        if !LINKER_NAMES.contains(&name.as_str()) {
            return Err(format!("unknown linker '{name}': expected bfd, lld, mold or wild"));
        }
    }
    let planned = plan(config)?;
    let llvm = tools::find_llvm(config.llvm.as_deref(), &config.search)?;
    let blank = |name: &str, outcome: Outcome, detail: &str| Record {
        outcome,
        test: name.to_owned(),
        target: arch.triple().to_owned(),
        llvm: llvm.as_ref().map(|llvm| llvm.major),
        linker: None,
        built_by: BuiltBy::Nobody,
        reason: None,
        detail: detail.to_owned(),
        covers: None,
    };
    let llvm_name =
        llvm.as_ref().map_or_else(|| "nollvm".to_owned(), |llvm| format!("llvm{}", llvm.major));
    let results = config.results.clone().unwrap_or_else(|| {
        let mode = if config.mode == Mode::Hybrid { "-hybrid" } else { "" };
        let file = format!("results-{}-{llvm_name}{mode}.tsv", arch.name());
        config.build_dir.join("irdiff").join(file)
    });
    let mut report = Report::default();

    if let Some(llvm) = &llvm {
        let toolchain = toolchain(config, llvm, &mut report)?;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let run = format!("run-{}-{stamp}", std::process::id());
        let session = Session {
            toolchain,
            arch,
            host,
            mode: config.mode,
            tool_limits: config.tool_limits,
            run_limits: config.run_limits,
            work: config.build_dir.join("irdiff").join(run),
            keep: config.keep,
            witness: config.witness,
        };
        let jobs = config.jobs.clamp(1, MAX_JOBS);
        report.configuration = describe(&session, jobs);
        if let Some(why) = session.cannot_run().filter(|_| host != Some(arch)) {
            report.notes.push(format!("{why}; objects are built, programs are not"));
        }
        let tests: Vec<Test> = planned
            .iter()
            .filter_map(|test| match test {
                Planned::Run(test) => Some(Test::clone(test)),
                Planned::Broken { .. } => None,
            })
            .collect();
        let mut ran = match session.run_tests(&tests, jobs) {
            Ok(ran) => ran.into_iter(),
            Err(e) => {
                let _ = std::fs::remove_dir_all(&session.work);
                return Err(e);
            }
        };
        for test in &planned {
            match test {
                Planned::Run(_) => report.records.extend(ran.next().unwrap_or_default()),
                Planned::Broken { name, detail } => {
                    report.records.push(blank(name, Outcome::Error, detail));
                }
            }
        }
        if config.keep && report.records.iter().any(|record| record.outcome.is_failure()) {
            report.kept = Some(session.work.clone());
        } else {
            let _ = std::fs::remove_dir_all(&session.work);
        }
    } else {
        let wanted = config.llvm.as_deref().map_or("22 or 19", |major| major);
        let why = format!("LLVM {wanted} (llc) not found");
        missing(config, &mut report, format!("{why}; every test is skipped"), true);
        report.records = planned
            .iter()
            .map(|test| match test {
                Planned::Run(test) => Record {
                    covers: test.expect.covers.clone(),
                    ..blank(&test.name, Outcome::Skip, &why)
                },
                Planned::Broken { name, detail } => blank(name, Outcome::Error, detail),
            })
            .collect();
        report.configuration = format!("no oracle; target {}", arch.triple());
    }
    report.elapsed = started.elapsed();
    report.write_results(&results)?;
    report.results_file = Some(results);
    Ok(report)
}

/// The line of the summary that says what ran. It names the jobserver
/// pool when there is one: `-j` is then an upper bound ("Jobserver").
fn describe(session: &Session, jobs: usize) -> String {
    let tools = &session.toolchain;
    let linkers: Vec<&str> = tools.linkers.iter().map(|linker| linker.name.as_str()).collect();
    let pool = crate::jobs::global()
        .describe()
        .map_or_else(String::new, |pool| format!(" (jobserver {pool})"));
    format!(
        "LLVM {} ({}); hod {}; target {}; linkers {}; {}{}; -j {jobs}{pool}",
        tools.llvm_major,
        tools.llc.display(),
        tools.hod.display(),
        session.arch.triple(),
        if linkers.is_empty() { "none".to_owned() } else { linkers.join(" ") },
        match session.mode {
            Mode::Strict => "strict",
            Mode::Hybrid => "hybrid",
        },
        if session.witness { "; witness" } else { "" },
    )
}

/// Parses the command's arguments into `config` (which holds the
/// defaults). `Ok(false)` means the help was asked for.
pub fn parse_args(args: &[String], config: &mut Config) -> Result<bool, String> {
    let mut rest = args.iter();
    let mut options = true;
    while let Some(arg) = rest.next() {
        if !options || !arg.starts_with('-') || arg == "-" {
            config.paths.push(PathBuf::from(arg));
            continue;
        }
        let (name, joined) = match arg.split_once('=') {
            Some((name, value)) => (name, Some(value.to_owned())),
            None => {
                match arg.strip_prefix("-j").filter(|n| !n.is_empty() && !arg.starts_with("--")) {
                    Some(number) => ("-j", Some(number.to_owned())),
                    None => (arg.as_str(), None),
                }
            }
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
            "--" => options = false,
            "-h" | "--help" => return Ok(false),
            "--hybrid" => {
                flag(&joined)?;
                config.mode = Mode::Hybrid;
            }
            "--keep" => {
                flag(&joined)?;
                config.keep = true;
            }
            "--witness" => {
                flag(&joined)?;
                config.witness = true;
            }
            "-v" | "--verbose" => {
                flag(&joined)?;
                config.verbose = true;
            }
            "-j" | "--jobs" => {
                let text = value()?;
                config.jobs = match text.parse::<usize>() {
                    Ok(jobs) if jobs >= 1 => jobs.min(MAX_JOBS),
                    _ => return Err(format!("invalid job count '{text}'")),
                };
            }
            "--llvm" => config.llvm = Some(value()?),
            "--target" => {
                let text = value()?;
                config.target = Some(Arch::from_name(&text).ok_or_else(|| {
                    format!("unknown target '{text}': expected aarch64 or x86_64")
                })?);
            }
            "--linker" => {
                let text = value()?;
                if !LINKER_NAMES.contains(&text.as_str()) {
                    return Err(format!(
                        "unknown linker '{text}': expected bfd, lld, mold or wild"
                    ));
                }
                if !config.linkers.contains(&text) {
                    config.linkers.push(text);
                }
            }
            "--tier" => {
                let text = value()?;
                config.tier = match text.as_str() {
                    "quick" => TierFilter::Quick,
                    "all" => TierFilter::All,
                    _ => return Err(format!("unknown tier '{text}': expected quick or all")),
                };
            }
            "--hod" => config.hod = Some(PathBuf::from(value()?)),
            "--results" => config.results = Some(PathBuf::from(value()?)),
            _ => return Err(format!("unknown option '{arg}'")),
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hod_testutil::Scratch;

    fn parsed(args: &[&str]) -> Result<Config, String> {
        let args: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
        let mut config = Config::new(PathBuf::from("/build"));
        parse_args(&args, &mut config).map(|run| {
            assert!(run, "{args:?} asked for the help");
            config
        })
    }

    // spec/harness.md "irdiff", "Options": the defaults.
    #[test]
    fn defaults() {
        let config = parsed(&[]).unwrap();
        assert!(config.paths.is_empty() && config.linkers.is_empty());
        assert_eq!(
            (config.llvm, config.target, config.hod, config.results),
            (None, None, None, None)
        );
        assert_eq!(config.tier, TierFilter::All);
        assert_eq!(config.mode, Mode::Strict);
        assert!(!config.keep && !config.verbose && !config.require_tools && !config.witness);
        assert!((1..=DEFAULT_JOBS).contains(&config.jobs));
        assert_eq!(
            config.tool_limits,
            Limits { time: Duration::from_mins(1), memory_kib: 4 << 20 }
        );
        assert_eq!(
            config.run_limits,
            Limits { time: Duration::from_secs(10), memory_kib: 1 << 20 }
        );
        assert_eq!(config.build_dir, Path::new("/build"));
    }

    // "Options": every option, in both spellings.
    #[test]
    fn options() {
        let config = parsed(&[
            "-j",
            "2",
            "--llvm",
            "19",
            "--target=x86_64",
            "--linker",
            "mold",
            "--linker=bfd",
            "--linker",
            "mold",
            "--tier",
            "quick",
            "--hod",
            "/h/hod",
            "--hybrid",
            "--keep",
            "--witness",
            "--results=/r.tsv",
            "-v",
            "tests/ir/a.ll",
            "some dir",
        ])
        .unwrap();
        assert_eq!(config.jobs, 2);
        assert_eq!(config.llvm.as_deref(), Some("19"));
        assert_eq!(config.target, Some(Arch::X86_64));
        assert_eq!(config.linkers, ["mold", "bfd"]);
        assert_eq!(config.tier, TierFilter::Quick);
        assert_eq!(config.hod.as_deref(), Some(Path::new("/h/hod")));
        assert_eq!(config.mode, Mode::Hybrid);
        assert!(config.keep && config.verbose && config.witness);
        assert_eq!(config.results.as_deref(), Some(Path::new("/r.tsv")));
        assert_eq!(config.paths, [PathBuf::from("tests/ir/a.ll"), PathBuf::from("some dir")]);

        let config =
            parsed(&["-j8", "--tier=all", "--llvm=/usr/lib/llvm-22", "--verbose"]).unwrap();
        assert_eq!((config.jobs, config.tier), (8, TierFilter::All));
        assert_eq!(config.llvm.as_deref(), Some("/usr/lib/llvm-22"));
        assert!(config.verbose);
        assert_eq!(parsed(&["--jobs=3"]).unwrap().jobs, 3);
        assert_eq!(parsed(&["-j=5"]).unwrap().jobs, 5);
        // The cap.
        assert_eq!(parsed(&["-j", "1000"]).unwrap().jobs, MAX_JOBS);
        // After `--` everything is a path.
        let config = parsed(&["--keep", "--", "--hybrid", "-x.ll"]).unwrap();
        assert_eq!(config.paths, [PathBuf::from("--hybrid"), PathBuf::from("-x.ll")]);
        assert_eq!(config.mode, Mode::Strict);

        for help in ["-h", "--help"] {
            let mut config = Config::new(PathBuf::from("/build"));
            assert_eq!(parse_args(&[help.to_owned()], &mut config), Ok(false));
        }
    }

    // "Options": usage errors.
    #[test]
    fn usage_errors() {
        for (args, needle) in [
            (&["--frobnicate"][..], "unknown option '--frobnicate'"),
            (&["-x"], "unknown option '-x'"),
            (&["-j"], "'-j' requires a value"),
            (&["-j", "0"], "invalid job count '0'"),
            (&["-j", "many"], "invalid job count 'many'"),
            (&["-jx"], "invalid job count 'x'"),
            (&["--llvm"], "'--llvm' requires a value"),
            (&["--llvm="], "'--llvm' requires a value"),
            (&["--target", "arm"], "unknown target 'arm'"),
            (&["--linker", "gold"], "unknown linker 'gold'"),
            (&["--tier", "full"], "unknown tier 'full'"),
            (&["--hybrid=yes"], "'--hybrid' takes no value"),
            (&["--keep=1"], "'--keep' takes no value"),
            (&["--witness=1"], "'--witness' takes no value"),
            (&["--hod"], "'--hod' requires a value"),
        ] {
            let error = parsed(args).unwrap_err();
            assert!(error.contains(needle), "{args:?}: {error}");
        }
    }

    // "Tests": which files a path names, and their names.
    #[test]
    fn collecting_tests() {
        let scratch = Scratch::new("irdiff-collect");
        for file in ["b.ll", "a.ll", "sub/deeper/c.ll", "notes.txt", "sub/d.bc"] {
            let path = scratch.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "").unwrap();
        }
        let names = |paths: &[PathBuf]| -> Vec<String> {
            let prefix = format!("{}/", scratch.path().display());
            collect(paths)
                .unwrap()
                .into_iter()
                .map(|(name, _)| name.strip_prefix(&prefix).unwrap_or(&name).to_owned())
                .collect()
        };
        let dir = scratch.path().to_path_buf();
        assert_eq!(names(std::slice::from_ref(&dir)), ["a.ll", "b.ll", "sub/deeper/c.ll"]);
        // A file is taken as it is, whatever its name; duplicates are one.
        assert_eq!(
            names(&[scratch.join("notes.txt"), dir.clone(), scratch.join("a.ll")]),
            ["a.ll", "b.ll", "notes.txt", "sub/deeper/c.ll"]
        );
        let missing = collect(&[scratch.join("nope")]).unwrap_err();
        assert!(missing.contains("is not a file or a directory"), "{missing}");
        let empty = collect(&[scratch.join("sub/deeper/c.ll").parent().unwrap().join("..")]);
        assert_eq!(empty.unwrap().len(), 1);
        std::fs::create_dir(scratch.join("empty")).unwrap();
        let none = collect(&[scratch.join("empty")]).unwrap_err();
        assert!(none.contains("no test (*.ll) in"), "{none}");

        // The default is the workspace's corpus, named relative to it.
        let corpus = collect(&[]).unwrap();
        assert!(corpus.len() >= 20, "the seed corpus: {}", corpus.len());
        assert!(corpus.iter().all(|(name, path)| name.starts_with("tests/ir/") && path.is_file()));
    }
}
