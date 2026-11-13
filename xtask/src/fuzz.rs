//! `cargo xtask fuzz`: random differential testing, layer L10 of
//! `docs/VALIDATION.md`. Contract: `spec/harness.md` "Random programs".
//!
//! Each seed is a random program (csmith's, or the ABI generator's),
//! compiled by clang at `-O0` to LLVM text for the target, built by the
//! oracle (`llc -O0` with the reference selector) and by `hod --strict`,
//! linked, run (a cross target's under its emulator) and compared: exit
//! status and standard output, which ends in csmith's checksum of the
//! program's globals. `--mix` builds half of the module with each
//! compiler, `--debug` adds `-g` and compares the line tables, `--census`
//! counts what the IR contains.
//!
//! - [`case`]: one seed, from the generator to the verdict;
//! - [`split`]: `--mix`, the module in two halves;
//! - [`profile`]: the option sets of the generator, and the census;
//! - [`abi`]: the ABI generator (`--profile abi`);
//! - [`reduce`]: a finding cut down to a small module, and the witness.

pub mod abi;
pub mod case;
pub mod profile;
pub mod reduce;
pub mod split;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::debug::lines::LinesReport;
use crate::irdiff::process::{End, Limits};
use crate::irdiff::tools::{self, Emulator, Search};
use crate::irdiff::{Arch, LINKER_NAMES, build_workspace_hod, default_build_dir};
use case::{Bench, Files, Kind, Mode, Outcome, Side, Tools, Verdict};
use profile::{Census, Generator, PROFILES, Profile};

/// The command's help.
pub const USAGE: &str = "\
usage: cargo xtask fuzz [options]

Generates random C programs (csmith, or hod's ABI generator), compiles
each with clang -O0 -emit-llvm, builds the module with the oracle (llc -O0,
SelectionDAG) and with hod --strict, links and runs both and compares their
exit status and output. A disagreement or a failure of hod is a finding:
it is saved with its commands, reduced, and makes the exit status 1.

  --mix               split each module in two with llvm-extract; hod builds
                      one half and llc the other, both ways round
  --debug             compile with -g: llvm-dwarfdump --verify on hod's
                      object, and its (function, line) pairs against llc's
  --census            count the IR's opcodes, types and attributes; build
                      nothing
  --profile <name>    the generator's option set (default: default):
PROFILES
  --seed <N>          the first seed (default 1)
  --count <M>         seeds N to N+M-1 (default 100)
  --target <arch>     aarch64 or x86_64 (default: this host)
  --llvm <major>      22 or 19: clang and llc of that LLVM (default 22)
  -j <J>, --jobs <J>  seeds at once (default 4, at most 16)
  --linker <name>     bfd, lld, mold or wild (default: the driver's ld)
  --timeout <secs>    each program's time limit, emulated or not (default 5)
  --reduce-budget <n> probes a reduction may run (default 300; 0: none)
  --hod <binary>      the hod to test (default: build this workspace's)
  --out <dir>         findings and the results file (default: <build>/fuzz)
  -v, --verbose       print every seed's verdict

exit status: 0 no finding; 1 a finding; 3 no finding but an error of the
oracle's side or a missing tool; 2 usage
";

/// The help, with the profiles.
pub fn usage() -> String {
    let profiles: String = PROFILES
        .iter()
        .map(|p| format!("                        {:<11} {}\n", p.name, p.about))
        .collect();
    USAGE.replace("PROFILES\n", &profiles)
}

/// The most `-j` gives.
pub const MAX_JOBS: usize = 16;

/// A run of the command.
#[derive(Clone, Debug)]
pub struct Options {
    pub seed: u64,
    pub count: u64,
    pub target: Option<Arch>,
    pub llvm: u32,
    pub jobs: usize,
    pub linker: Option<String>,
    pub timeout: Duration,
    pub reduce_budget: usize,
    pub hod: Option<PathBuf>,
    /// Where findings, the results file and the work directory go.
    pub out: PathBuf,
    /// The build directory the workspace's hod is built in.
    pub build_dir: PathBuf,
    pub verbose: bool,
    pub search: Search,
    pub mode: Mode,
    pub profile: &'static Profile,
}

impl Options {
    /// The defaults, with output under `build_dir`.
    pub fn new(build_dir: PathBuf) -> Self {
        Self {
            seed: 1,
            count: 100,
            target: None,
            llvm: tools::LLVM_MAJORS[0],
            jobs: 4,
            linker: None,
            timeout: Duration::from_secs(5),
            reduce_budget: 300,
            hod: None,
            out: build_dir.join("fuzz"),
            build_dir,
            verbose: false,
            search: Search::from_env(),
            mode: Mode::Whole,
            profile: Profile::default_profile(),
        }
    }
}

/// Parses the arguments into `options`. `Ok(false)` is the help.
pub fn parse_args(args: &[String], options: &mut Options) -> Result<bool, String> {
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let (name, joined) = match arg.split_once('=') {
            Some((name, value)) => (name, Some(value.to_owned())),
            None => {
                match arg.strip_prefix("-j").filter(|n| !n.is_empty() && !arg.starts_with("--")) {
                    Some(number) => ("-j", Some(number.to_owned())),
                    None => (arg.as_str(), None),
                }
            }
        };
        let mut value = || {
            joined
                .clone()
                .or_else(|| rest.next().cloned())
                .filter(|v| !v.is_empty())
                .ok_or_else(|| format!("'{name}' requires a value"))
        };
        let number = |text: String| {
            text.parse::<u64>().map_err(|_| format!("invalid number '{text}' for '{name}'"))
        };
        match name {
            "-h" | "--help" => return Ok(false),
            "-v" | "--verbose" if joined.is_none() => options.verbose = true,
            "--seed" => options.seed = number(value()?)?,
            "--count" => options.count = number(value()?)?,
            "-j" | "--jobs" => {
                let jobs = number(value()?)?;
                if jobs == 0 {
                    return Err("invalid job count '0'".to_owned());
                }
                options.jobs = usize::try_from(jobs).unwrap_or(MAX_JOBS).min(MAX_JOBS);
            }
            "--target" => {
                let text = value()?;
                options.target = Some(Arch::from_name(&text).ok_or_else(|| {
                    format!("unknown target '{text}': expected aarch64 or x86_64")
                })?);
            }
            "--llvm" => {
                let text = value()?;
                options.llvm = match text.parse::<u32>() {
                    Ok(major) if tools::LLVM_MAJORS.contains(&major) => major,
                    _ => return Err(format!("unknown LLVM '{text}': expected 22 or 19")),
                };
            }
            "--linker" => {
                let text = value()?;
                if !LINKER_NAMES.contains(&text.as_str()) {
                    return Err(format!(
                        "unknown linker '{text}': expected bfd, lld, mold or wild"
                    ));
                }
                options.linker = Some(text);
            }
            "--timeout" => options.timeout = Duration::from_secs(number(value()?)?.max(1)),
            "--reduce-budget" => {
                options.reduce_budget = usize::try_from(number(value()?)?).unwrap_or(usize::MAX);
            }
            "--mix" | "--debug" | "--census" if joined.is_none() => {
                let mode = match name {
                    "--mix" => Mode::Mix,
                    "--debug" => Mode::Debug,
                    _ => Mode::Census,
                };
                if options.mode != Mode::Whole && options.mode != mode {
                    return Err(format!(
                        "'--{}' and '{name}' cannot be combined",
                        options.mode.name()
                    ));
                }
                options.mode = mode;
            }
            "--profile" => {
                let text = value()?;
                options.profile = Profile::by_name(&text).ok_or_else(|| {
                    let names: Vec<&str> = PROFILES.iter().map(|p| p.name).collect();
                    format!("unknown profile '{text}': expected one of {}", names.join(", "))
                })?;
            }
            "--hod" => options.hod = Some(PathBuf::from(value()?)),
            "--out" => options.out = PathBuf::from(value()?),
            _ => return Err(format!("unknown option '{arg}'")),
        }
    }
    Ok(true)
}

/// The tools of a run, or what is missing.
fn find_tools(options: &Options, arch: Arch, work: &Path) -> Result<Tools, String> {
    let search = &options.search;
    let major = options.llvm;
    let (csmith, csmith_include) = if matches!(options.profile.generator, Generator::Abi) {
        (None, None)
    } else {
        let csmith = search.on_path("csmith").ok_or("csmith not found (Debian package csmith)")?;
        // csmith.h: /usr/include/csmith for the Debian package, else
        // <prefix>/include/csmith-<version> or <prefix>/include beside it.
        let prefix = csmith.parent().and_then(Path::parent).unwrap_or(Path::new("/usr"));
        let include = ["include/csmith", "include"]
            .iter()
            .map(|dir| prefix.join(dir))
            .find(|dir| dir.join("csmith.h").is_file())
            .ok_or_else(|| format!("csmith.h not found under '{}'", prefix.display()))?;
        (Some(csmith), Some(include))
    };
    let extract = match options.mode {
        Mode::Mix => Some(
            search
                .versioned("llvm-extract", major)
                .ok_or(format!("llvm-extract-{major} not found"))?,
        ),
        _ => None,
    };
    let (dwarfdump, lines) = match options.mode {
        Mode::Debug => (
            Some(
                search
                    .versioned("llvm-dwarfdump", major)
                    .ok_or(format!("llvm-dwarfdump-{major} not found"))?,
            ),
            Some(crate::debug::sessions::find_line_tools().ok_or(
                "the line comparison's tools (llvm-dwarfdump, llvm-nm, llvm-objdump) not found",
            )?),
        ),
        _ => (None, None),
    };
    let clang = search.versioned("clang", major).ok_or(format!("clang-{major} not found"))?;
    let llc = search.versioned("llc", major).ok_or(format!("llc-{major} not found"))?;
    let llvm_major = tools::llc_major(&llc)?;
    if llvm_major != major {
        return Err(format!("'{}' is LLVM {llvm_major}, not {major}", llc.display()));
    }
    let target = tools::target_of(arch);
    let driver = tools::find_driver_for(arch, search)
        .ok_or_else(|| format!("linker driver '{}' not found", target.c_driver()))?;
    let (linker_dir, linker_name) = match &options.linker {
        None => (None, "ld".to_owned()),
        Some(name) => {
            let path = tools::find_linker_for(name, arch, search)
                .ok_or_else(|| format!("linker '{name}' not found"))?;
            let dir = work.join("linkers").join(name);
            fs::create_dir_all(&dir)
                .map_err(|e| format!("cannot create '{}': {e}", dir.display()))?;
            std::os::unix::fs::symlink(&path, dir.join("ld"))
                .map_err(|e| format!("cannot link the linker into '{}': {e}", dir.display()))?;
            (Some(dir), name.clone())
        }
    };
    let emulator = if Arch::host() == Some(arch) {
        None
    } else {
        let program = tools::find_emulator(arch, search)
            .ok_or_else(|| format!("emulator '{}' not found", target.emulator()))?;
        let sysroot = target.sysroot();
        if !sysroot.is_dir() {
            return Err(format!("system root '{}' not found", sysroot.display()));
        }
        Some(Emulator { args: target.emulator_args(&sysroot), program })
    };
    let hod = match &options.hod {
        Some(hod) if hod.is_file() => std::path::absolute(hod).unwrap_or_else(|_| hod.clone()),
        Some(hod) => return Err(format!("'{}' is not a file", hod.display())),
        // The census builds nothing.
        None if options.mode == Mode::Census => PathBuf::from("hod"),
        None => build_workspace_hod(&options.build_dir)?,
    };
    Ok(Tools {
        csmith,
        csmith_include,
        clang,
        llc,
        llvm_major,
        hod,
        driver,
        linker_dir,
        linker_name,
        emulator,
        extract,
        dwarfdump,
        lines,
    })
}

/// A finding as it was saved.
#[derive(Clone, Debug)]
pub struct Saved {
    pub seed: u64,
    pub kind: Kind,
    pub detail: String,
    /// The directory with its files.
    pub dir: PathBuf,
    /// What the witness said on the whole program.
    pub witness: String,
    /// Lines before and after the reduction, and its probes.
    pub reduced: Option<(usize, usize, usize)>,
}

/// What a run found.
#[derive(Debug, Default)]
pub struct Summary {
    pub configuration: String,
    pub seeds: usize,
    pub agree: usize,
    pub refused: BTreeMap<String, usize>,
    pub skipped: BTreeMap<String, usize>,
    pub errors: Vec<(u64, String)>,
    pub findings: Vec<Saved>,
    /// How long the oracle's programs ran.
    pub oracle_times: Vec<Duration>,
    pub elapsed: Duration,
    pub results_file: Option<PathBuf>,
    pub mode: Option<Mode>,
    /// `--census`: the features of every module.
    pub census: Option<Census>,
    /// `--debug`: the line comparisons of every seed, added.
    pub lines: Option<LinesReport>,
}

impl Summary {
    /// The exit status ("Exit status").
    pub fn exit_status(&self) -> u8 {
        if !self.findings.is_empty() {
            1
        } else if self.errors.is_empty() {
            0
        } else {
            3
        }
    }

    fn add(&mut self, outcome: &Outcome) {
        self.seeds += 1;
        match &outcome.verdict {
            Verdict::Agree => self.agree += 1,
            Verdict::Refused(reason) => *self.refused.entry(reason.clone()).or_default() += 1,
            Verdict::Skipped(why) => *self.skipped.entry((*why).to_owned()).or_default() += 1,
            Verdict::Error(detail) => self.errors.push((outcome.seed, detail.clone())),
            Verdict::Finding(_) => {}
        }
        self.oracle_times.extend(outcome.oracle_time);
        if let Some(features) = &outcome.census {
            self.census.get_or_insert_default().add(features);
        }
        if let Some(report) = &outcome.lines {
            self.lines.get_or_insert_default().add(report);
        }
    }

    /// The table a run prints.
    pub fn text(&self) -> String {
        let mut text = String::new();
        let minutes = self.elapsed.as_secs_f64() / 60.0;
        #[expect(clippy::cast_precision_loss, reason = "a rate, for people")]
        let rate = if minutes > 0.0 { self.seeds as f64 / minutes } else { 0.0 };
        let _ = writeln!(text, "fuzz: {}", self.configuration);
        let _ = writeln!(
            text,
            "fuzz: {} seeds in {:.0} s ({rate:.1} programs/min)",
            self.seeds,
            self.elapsed.as_secs_f64()
        );
        let row = |text: &mut String, name: &str, count: usize| {
            let _ = writeln!(text, "  {name:<44} {count:>6}");
        };
        row(
            &mut text,
            if self.mode == Some(Mode::Census) { "scanned" } else { "agree" },
            self.agree,
        );
        row(&mut text, "refused", self.refused.values().sum());
        let mut reasons: Vec<(&String, &usize)> = self.refused.iter().collect();
        reasons.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        for (reason, count) in reasons {
            row(&mut text, &format!("  {reason}"), *count);
        }
        if self.mode == Some(Mode::Debug) {
            let debug: usize =
                self.refused.iter().filter(|(r, _)| r.starts_with("debug:")).map(|(_, n)| n).sum();
            row(&mut text, "  of which debug:", debug);
        }
        row(&mut text, "skipped", self.skipped.values().sum());
        for (why, count) in &self.skipped {
            row(&mut text, &format!("  {why}"), *count);
        }
        row(&mut text, "error (the oracle's side)", self.errors.len());
        for (seed, detail) in &self.errors {
            let _ = writeln!(text, "    seed {seed}: {detail}");
        }
        row(&mut text, "FINDINGS", self.findings.len());
        for found in &self.findings {
            let _ = writeln!(
                text,
                "    seed {}: {}: {}\n      witness: {}\n      saved: {}",
                found.seed,
                found.kind.name(),
                found.detail,
                found.witness,
                found.dir.display()
            );
            if let Some((before, after, probes)) = found.reduced {
                let _ =
                    writeln!(text, "      reduced: {before} -> {after} lines ({probes} probes)");
            }
        }
        if !self.oracle_times.is_empty() {
            let mut times = self.oracle_times.clone();
            times.sort();
            let at = |q: usize| times[(times.len() - 1) * q / 100].as_secs_f64();
            let _ = writeln!(
                text,
                "  the oracle's programs ran p50 {:.2} s, p90 {:.2} s, max {:.2} s",
                at(50),
                at(90),
                at(100)
            );
        }
        if let Some(report) = &self.lines {
            let _ = writeln!(text, "  lines: {}", report.summary());
            let _ = writeln!(text, "  columns: {}", report.columns.summary());
        }
        if let Some(file) = &self.results_file {
            let _ = writeln!(text, "  results: {}", file.display());
        }
        if let Some(census) = &self.census {
            text.push_str(&census.text());
        }
        text
    }
}

/// A seed's place in the results: its outcome and the finding saved.
type Slot = Option<(Outcome, Option<Saved>)>;

/// Runs the command.
pub fn run(options: &Options) -> Result<Summary, String> {
    let started = Instant::now();
    let arch = options
        .target
        .or(Arch::host())
        .ok_or("this host is not a Linux machine of a supported architecture; pass --target")?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let work = options.out.join(format!("run-{}-{stamp}", std::process::id()));
    fs::create_dir_all(&work).map_err(|e| format!("cannot create '{}': {e}", work.display()))?;
    let tools = match find_tools(options, arch, &work) {
        Ok(tools) => tools,
        Err(e) => {
            let _ = fs::remove_dir_all(&work);
            return Err(e);
        }
    };
    let bench = Bench {
        arch,
        tool_limits: Limits { time: Duration::from_mins(1), memory_kib: 4 << 20 },
        run_limits: Limits { time: options.timeout, memory_kib: 1 << 20 },
        tools,
        mode: options.mode,
        profile: options.profile,
    };
    let jobs = options.jobs.clamp(1, MAX_JOBS);
    let mut summary = Summary {
        configuration: describe(options, &bench, jobs),
        mode: Some(options.mode),
        ..Summary::default()
    };
    let seeds: Vec<u64> = (0..options.count).map(|i| options.seed.saturating_add(i)).collect();
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let slots: Mutex<Vec<Slot>> = Mutex::new(vec![None; seeds.len()]);
    let findings_dir = options.out.join("findings");
    std::thread::scope(|scope| {
        for _ in 0..jobs.min(seeds.len().max(1)) {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(&seed) = seeds.get(index) else { break };
                    let files = Files { dir: work.join(format!("seed-{seed}")) };
                    let outcome = match fs::create_dir_all(&files.dir) {
                        Ok(()) => bench.run_seed(seed, &files),
                        Err(e) => Outcome {
                            verdict: Verdict::Error(format!(
                                "harness: cannot create the work directory: {e}"
                            )),
                            ..Outcome::new(seed, options.profile.flavour(seed))
                        },
                    };
                    let saved = matches!(outcome.verdict, Verdict::Finding(_))
                        .then(|| save(&bench, options, &outcome, &files, &findings_dir));
                    let _ = fs::remove_dir_all(&files.dir);
                    let finished = done.fetch_add(1, Ordering::Relaxed) + 1;
                    if options.verbose || saved.is_some() {
                        eprintln!(
                            "fuzz: seed {seed} ({}): {} {}",
                            outcome.flavour.name,
                            outcome.verdict.name(),
                            outcome.verdict.detail()
                        );
                    }
                    if finished.is_multiple_of(100) {
                        eprintln!("fuzz: {finished}/{} seeds", seeds.len());
                    }
                    slots.lock().unwrap_or_else(PoisonError::into_inner)[index] =
                        Some((outcome, saved));
                }
            });
        }
    });
    let _ = fs::remove_dir_all(&work);
    let mut results = String::from("# seed\tflavour\tverdict\tdetail\toracle_ms\n");
    for (outcome, saved) in
        slots.into_inner().unwrap_or_else(PoisonError::into_inner).into_iter().flatten()
    {
        summary.add(&outcome);
        let ms = outcome.oracle_time.map_or_else(|| "-".to_owned(), |t| t.as_millis().to_string());
        let detail = outcome.verdict.detail().replace(['\t', '\n'], " ");
        let _ = writeln!(
            results,
            "{}\t{}\t{}\t{detail}\t{ms}",
            outcome.seed,
            outcome.flavour.name,
            outcome.verdict.name()
        );
        summary.findings.extend(saved);
    }
    let file = options.out.join(format!("results-{}.tsv", run_name(arch, options.llvm, options)));
    fs::write(&file, results).map_err(|e| format!("cannot write '{}': {e}", file.display()))?;
    summary.results_file = Some(file);
    summary.elapsed = started.elapsed();
    Ok(summary)
}

/// `<arch>-llvm<N>`, then the profile and the mode when they are not the
/// defaults: the name of a run's results file and the start of its
/// findings' directories.
fn run_name(arch: Arch, llvm: u32, options: &Options) -> String {
    let mut name = format!("{}-llvm{llvm}", arch.name());
    if options.profile.name != "default" {
        let _ = write!(name, "-{}", options.profile.name);
    }
    if options.mode != Mode::Whole {
        let _ = write!(name, "-{}", options.mode.name());
    }
    name
}

/// The line that says what ran.
fn describe(options: &Options, bench: &Bench, jobs: usize) -> String {
    let tools = &bench.tools;
    let pool = crate::jobs::global()
        .describe()
        .map_or_else(String::new, |pool| format!(" (jobserver {pool})"));
    format!(
        "mode {}; profile {}; target {}{}; LLVM {} ({}, {}); hod {}; linker {}; csmith {}; \
         seeds {}..{}; timeout {} s; -j {jobs}{pool}",
        options.mode.name(),
        options.profile.name,
        bench.arch.triple(),
        tools.emulator.as_ref().map_or_else(String::new, |emulator| {
            format!(" (emulated by {})", emulator.program.display())
        }),
        tools.llvm_major,
        tools.clang.display(),
        tools.llc.display(),
        tools.hod.display(),
        tools.linker_name,
        tools.csmith.as_ref().map_or_else(|| "not used".to_owned(), |c| c.display().to_string()),
        options.seed,
        options.seed.saturating_add(options.count).saturating_sub(1),
        options.timeout.as_secs(),
    )
}

/// Saves a finding under `findings`: the source, the module, the commands,
/// both programs' output, the witness's verdict and the reduced module
/// with its seed test.
fn save(
    bench: &Bench,
    options: &Options,
    outcome: &Outcome,
    files: &Files,
    findings: &Path,
) -> Saved {
    let Verdict::Finding(finding) = &outcome.verdict else {
        unreachable!("save() is called for findings only");
    };
    let dir = findings.join(format!(
        "{}-seed{}",
        run_name(bench.arch, bench.tools.llvm_major, options),
        outcome.seed
    ));
    let _ = fs::remove_dir_all(&dir);
    let _ = fs::create_dir_all(findings);
    let mut saved = Saved {
        seed: outcome.seed,
        kind: finding.kind,
        detail: finding.detail.clone(),
        dir: dir.clone(),
        witness: "not asked".to_owned(),
        reduced: None,
    };
    if let Err(e) = fs::rename(&files.dir, &dir) {
        saved.witness = format!("the finding could not be saved: {e}");
        return saved;
    }
    let mut commands = String::from("#!/bin/sh\n# The commands of the seed, in order.\nset -x\n");
    for line in &outcome.commands {
        commands.push_str(line);
        commands.push('\n');
    }
    let _ = fs::write(
        dir.join("commands.sh"),
        commands.replace(&*files.dir.to_string_lossy(), &dir.to_string_lossy()),
    );
    let ir = dir.join("prog.ll");
    if let Some(oracle) = &outcome.oracle {
        let _ = fs::write(dir.join("oracle.stdout"), &oracle.stdout);
    }
    if let Some(hod) = &outcome.hod {
        let _ = fs::write(dir.join("hod.stdout"), &hod.stdout);
        let _ = fs::write(dir.join("hod.stderr"), &hod.stderr);
    }
    match (&outcome.oracle, &outcome.hod) {
        (Some(oracle), Some(hod)) => {
            saved.witness = reduce::witness(bench, &dir, &ir, oracle, hod);
            if finding.side != Side::Whole {
                let whole = split::whole_witness(bench, &dir, &ir, oracle);
                saved.witness = format!("{}; {whole}", saved.witness);
            }
        }
        (Some(oracle), None) if finding.side != Side::Whole => {
            saved.witness = split::whole_witness(bench, &dir, &ir, oracle);
        }
        _ if matches!(finding.kind, Kind::DebugVerify | Kind::DebugLines) => {
            saved.witness = "the same module built by llc is the reference".to_owned();
        }
        _ => {}
    }
    let target = reduce::Target {
        kind: finding.kind,
        oracle_end: outcome.oracle.as_ref().map(|oracle| oracle.end),
        side: finding.side,
        seed: outcome.seed,
    };
    if options.reduce_budget > 0
        && let Ok(text) = fs::read_to_string(&ir)
    {
        let reduced = reduce::reduce(bench, &dir, &text, target, options.reduce_budget);
        saved.reduced = Some((reduced.before, reduced.after, reduced.probes));
        let reduced_ll = dir.join("reduced.ll");
        let _ = fs::write(&reduced_ll, &reduced.text);
        let stdout = reduced.oracle.as_ref().or(outcome.oracle.as_ref()).map(|o| o.stdout.clone());
        let stdout = stdout.as_deref();
        let seed = match finding.side {
            // Behaviour on the whole module: a `tests/ir` seed.
            Side::Whole if !matches!(finding.kind, Kind::DebugVerify | Kind::DebugLines) => {
                Some(seed_test(&reduced.text, bench, outcome, finding, stdout, target.oracle_end))
            }
            // A seed of two modules, each the half of the reduced module.
            Side::HodA | Side::HodB => {
                let mut log = Vec::new();
                match bench.split(&mut log, outcome.seed, &reduced_ll, &dir, "reduced-") {
                    Ok(Some(halves)) => {
                        let a = fs::read_to_string(&halves.a).unwrap_or_default();
                        let b = fs::read_to_string(&halves.b).unwrap_or_default();
                        Some(mix_seed_test(
                            &[&a, &b],
                            bench,
                            outcome,
                            finding,
                            stdout,
                            target.oracle_end,
                        ))
                    }
                    _ => None,
                }
            }
            // A `-g` module: `cargo xtask debug verify` or `debug columns`
            // takes `reduced.ll` as it is.
            Side::Whole => None,
        };
        if let Some(seed) = seed {
            let _ = fs::write(dir.join("seed.ll"), seed);
        }
    }
    let _ = fs::write(
        dir.join("verdict.txt"),
        format!(
            "seed {}\nflavour {}\nmode {}\nprofile {}\n{}: {}\nwitness: {}\n",
            outcome.seed,
            outcome.flavour.name,
            bench.mode.name(),
            bench.profile.name,
            finding.kind.name(),
            finding.detail,
            saved.witness
        ),
    );
    saved
}

/// The command that finds a seed again, for a seed test's first line.
fn command_line(bench: &Bench, outcome: &Outcome) -> String {
    let mut line = format!(
        "cargo xtask fuzz --seed {} --count 1 --target {} --llvm {}",
        outcome.seed,
        bench.arch.name(),
        bench.tools.llvm_major
    );
    if bench.profile.name != "default" {
        let _ = write!(line, " --profile {}", bench.profile.name);
    }
    if bench.mode != Mode::Whole {
        let _ = write!(line, " --{}", bench.mode.name());
    }
    line
}

/// The directives of a seed test: what it covers, its LLVM, the `xfail`,
/// the oracle's end and output.
fn seed_directives(
    bench: &Bench,
    outcome: &Outcome,
    finding: &case::Finding,
    stdout: Option<&[u8]>,
    end: Option<End>,
) -> String {
    let what = if bench.profile.name == "abi" { "an ABI program" } else { "a csmith program" };
    let mut seed = format!(
        "; A reduced random program, {what} ({}; flavour {}).\n",
        command_line(bench, outcome),
        outcome.flavour.name
    );
    seed.push_str(";@ covers: docs/VALIDATION.md L10 (a random program)\n");
    // clang's text is read by its own LLVM and later ones.
    let _ = writeln!(seed, ";@ llvm: {}+", bench.tools.llvm_major);
    let _ = writeln!(
        seed,
        ";@ xfail({}): {}: {} (triage pending)",
        bench.arch.name(),
        finding.kind.name(),
        finding.detail
    );
    match end {
        Some(End::Exit(code)) if code != 0 => {
            let _ = writeln!(seed, ";@ exit: {code}");
        }
        Some(End::Signal(signal)) => {
            let _ = writeln!(seed, ";@ exit: signal {signal}");
        }
        _ => {}
    }
    if let Some(stdout) = stdout {
        let text = String::from_utf8_lossy(stdout);
        let mut lines = text.split_inclusive('\n');
        for line in lines.by_ref() {
            match line.strip_suffix('\n') {
                Some(line) => {
                    let _ = writeln!(seed, ";@ stdout: {line}");
                }
                None => {
                    let _ = writeln!(seed, ";@ stdout-no-newline: {line}");
                }
            }
        }
    }
    seed
}

/// A module's text without the target's lines and the paths of the run.
fn portable(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let dropped = [
            "target triple",
            "target datalayout",
            "; ModuleID",
            "source_filename",
            "; Function Attrs",
        ];
        if dropped.iter().any(|start| line.starts_with(start)) {
            continue;
        }
        if line.starts_with("attributes #") {
            out.push_str(&without_cpu_attributes(line));
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

/// The reduced module as a `tests/ir` seed: the directives of its
/// oracle's behaviour, an `xfail` for the target, and the module without
/// its target triple, data layout and CPU attributes ("Seed tests").
pub fn seed_test(
    text: &str,
    bench: &Bench,
    outcome: &Outcome,
    finding: &case::Finding,
    stdout: Option<&[u8]>,
    end: Option<End>,
) -> String {
    let mut seed = seed_directives(bench, outcome, finding, stdout, end);
    seed.push('\n');
    seed.push_str(&portable(text));
    seed
}

/// A `--mix` finding's seed: the reduced module's two halves as the
/// modules `a` and `b` of one test ("Mix"), with the half hod built named
/// in the `xfail`. irdiff builds every module with one compiler, which
/// cannot show such a finding: the seed is a test of
/// `xtask/tests/fuzz_seeds.rs`, which builds one module with each.
pub fn mix_seed_test(
    halves: &[&str; 2],
    bench: &Bench,
    outcome: &Outcome,
    finding: &case::Finding,
    stdout: Option<&[u8]>,
    end: Option<End>,
) -> String {
    let mut seed = seed_directives(bench, outcome, finding, stdout, end);
    let hod = if finding.side == Side::HodB { "b" } else { "a" };
    let _ = writeln!(seed, "; hod builds module {hod}, llc the other.");
    for (name, text) in ["a", "b"].iter().zip(halves) {
        let _ = writeln!(seed, "\n;@ module: {name}");
        seed.push_str(&portable(text));
    }
    seed
}

/// An attribute group without the attributes that name a target's CPU or
/// features.
fn without_cpu_attributes(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(at) = rest.find('"') {
        out.push_str(&rest[..at]);
        let quoted = &rest[at..];
        // "key"="value" or "key".
        let key_end = quoted[1..].find('"').map_or(quoted.len(), |end| end + 2);
        let mut end = key_end;
        if quoted[key_end..].starts_with("=\"") {
            end = key_end + 2 + quoted[key_end + 2..].find('"').map_or(0, |e| e + 1);
        }
        let key = &quoted[1..key_end.saturating_sub(1)];
        if !matches!(key, "target-cpu" | "target-features" | "tune-cpu") {
            out.push_str(&quoted[..end]);
        }
        rest = &quoted[end..];
    }
    out.push_str(rest);
    // The spaces the removed attributes leave.
    let mut squeezed = String::new();
    for word in out.split(' ').filter(|word| !word.is_empty()) {
        if !squeezed.is_empty() {
            squeezed.push(' ');
        }
        squeezed.push_str(word);
    }
    squeezed
}

/// `cargo xtask fuzz`.
pub fn command(args: &[String]) -> u8 {
    let mut options = Options::new(default_build_dir());
    match parse_args(args, &mut options) {
        Ok(true) => {}
        Ok(false) => {
            print!("{}", usage());
            return 0;
        }
        Err(message) => {
            eprint!("xtask fuzz: {message}\n{}", usage());
            return 2;
        }
    }
    match run(&options) {
        Ok(summary) => {
            print!("{}", summary.text());
            summary.exit_status()
        }
        Err(message) => {
            eprintln!("xtask fuzz: {message}");
            3
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(args: &[&str]) -> Result<Options, String> {
        let args: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
        let mut options = Options::new(PathBuf::from("/build"));
        parse_args(&args, &mut options).map(|run| {
            assert!(run, "{args:?} asked for the help");
            options
        })
    }

    // spec/landed/harness/WP-154.md "Options".
    #[test]
    fn options() {
        let options = parsed(&[]).unwrap();
        assert_eq!((options.seed, options.count, options.llvm, options.jobs), (1, 100, 22, 4));
        assert_eq!(options.out, Path::new("/build/fuzz"));
        assert_eq!(options.timeout, Duration::from_secs(5));
        let options = parsed(&[
            "--seed",
            "40",
            "--count=7",
            "--target",
            "x86_64",
            "--llvm",
            "19",
            "-j2",
            "--linker",
            "mold",
            "--timeout",
            "3",
            "--reduce-budget",
            "0",
            "--hod",
            "/h",
            "--out",
            "/o",
            "-v",
        ])
        .unwrap();
        assert_eq!((options.seed, options.count, options.llvm, options.jobs), (40, 7, 19, 2));
        assert_eq!(options.target, Some(Arch::X86_64));
        assert_eq!(options.linker.as_deref(), Some("mold"));
        assert_eq!((options.timeout.as_secs(), options.reduce_budget), (3, 0));
        assert_eq!(options.hod.as_deref(), Some(Path::new("/h")));
        assert_eq!(options.out, Path::new("/o"));
        assert!(options.verbose);
        assert_eq!(parsed(&["-j", "99"]).unwrap().jobs, MAX_JOBS);
        for (args, needle) in [
            (&["--llvm", "18"][..], "unknown LLVM '18'"),
            (&["--target", "arm"], "unknown target"),
            (&["-j", "0"], "invalid job count"),
            (&["--seed", "x"], "invalid number 'x'"),
            (&["--linker", "gold"], "unknown linker"),
            (&["--count"], "requires a value"),
            (&["--what"], "unknown option"),
        ] {
            let error = parsed(args).unwrap_err();
            assert!(error.contains(needle), "{args:?}: {error}");
        }
        let mut options = Options::new(PathBuf::from("/b"));
        assert_eq!(parse_args(&["--help".to_owned()], &mut options), Ok(false));
    }

    // "Seed tests": CPU attributes go, the others stay.
    #[test]
    fn attributes_without_the_cpu() {
        let line = "attributes #0 = { noinline nounwind optnone \"frame-pointer\"=\"all\" \
                    \"no-trapping-math\"=\"true\" \"target-cpu\"=\"x86-64\" \
                    \"target-features\"=\"+cx8,+sse2\" \"tune-cpu\"=\"generic\" \"x\" }";
        assert_eq!(
            without_cpu_attributes(line),
            "attributes #0 = { noinline nounwind optnone \"frame-pointer\"=\"all\" \
             \"no-trapping-math\"=\"true\" \"x\" }"
        );
    }

    // "Summary": the exit status.
    #[test]
    fn exit_status() {
        let mut summary = Summary::default();
        assert_eq!(summary.exit_status(), 0);
        summary.errors.push((1, "llc".to_owned()));
        assert_eq!(summary.exit_status(), 3);
        summary.findings.push(Saved {
            seed: 2,
            kind: Kind::Disagree,
            detail: String::new(),
            dir: PathBuf::new(),
            witness: String::new(),
            reduced: None,
        });
        assert_eq!(summary.exit_status(), 1);
        summary.refused.insert("x64:thing".to_owned(), 3);
        summary.agree = 5;
        summary.seeds = 9;
        let text = summary.text();
        assert!(text.contains("x64:thing") && text.contains("FINDINGS"), "{text}");
    }
}
