//! The debugger tests: each program built by hod and by `llc`, linked by
//! every linker, and the same gdb and lldb sessions on both. Contract:
//! `spec/harness.md` "debug", "What a test does", "Outcomes".

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use super::build::{self, HodBuild, Tools};
use super::directives::{self, DebugTest, Lang, Objects};
use super::lines::{self, LineTools, LinesReport};
use super::permitted::{self, IrFacts};
use super::transcript::{self, Debugger, Ended};
use crate::irdiff::process::Limits;
use crate::irdiff::tools::Linker;

/// The outcome of one session, or of a test decided before any session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Outcome {
    /// hod's session printed what `llc`'s printed.
    Pass,
    /// hod's program differs, or hod failed, declined, or its program did
    /// not link.
    Fail,
    /// A FAIL (or a crash on hod's program) an `xfail` covers.
    Xfail,
    /// A PASS an `xfail` covers: the directive is stale.
    Xpass,
    /// The debugger crashed. On hod's program it fails the run; on `llc`'s
    /// it is an error of the tools.
    Crash,
    /// The test or a tool is broken: the front end or `llc` failed, `llc`'s
    /// program did not link, `llc`'s session did not meet the checks or
    /// timed out, the directives do not parse.
    Error,
    /// A tool this session needs is missing.
    Skip,
}

impl Outcome {
    pub fn name(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
            Self::Xfail => "XFAIL",
            Self::Xpass => "XPASS",
            Self::Crash => "CRASH",
            Self::Error => "ERROR",
            Self::Skip => "SKIP",
        }
    }

    pub const ALL: [Self; 7] =
        [Self::Pass, Self::Fail, Self::Xfail, Self::Xpass, Self::Crash, Self::Error, Self::Skip];
}

/// One result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    /// The test's name (its path relative to the workspace).
    pub test: String,
    pub frontend: String,
    /// `llc`, `hod`, or `hod:<module>` (that module by hod, the others by
    /// `llc`).
    pub variant: String,
    pub linker: Option<String>,
    pub debugger: Option<String>,
    pub outcome: Outcome,
    pub detail: String,
}

impl Record {
    /// Whether this result fails the run (exit status 1).
    pub fn fails(&self) -> bool {
        matches!(self.outcome, Outcome::Fail | Outcome::Xpass)
            || (self.outcome == Outcome::Crash && self.variant != "llc")
    }

    /// Whether it is an error of the tools or the test (exit status 3).
    pub fn errs(&self) -> bool {
        self.outcome == Outcome::Error || (self.outcome == Outcome::Crash && self.variant == "llc")
    }

    pub fn notable(&self) -> bool {
        !matches!(self.outcome, Outcome::Pass)
    }

    fn line(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
            self.test,
            self.frontend,
            self.variant,
            self.linker.as_deref().unwrap_or("-"),
            self.debugger.as_deref().unwrap_or("-"),
            self.outcome.name(),
            self.detail.replace(['\t', '\n'], " ")
        )
    }
}

/// A run of the command.
#[derive(Clone, Debug)]
#[expect(clippy::struct_excessive_bools, reason = "one field per option of the command")]
pub struct Options {
    /// Tests and directories; empty is the workspace's `tests/debug`.
    pub paths: Vec<PathBuf>,
    pub jobs: usize,
    /// The hod to test; `None` builds the workspace's.
    pub hod: Option<PathBuf>,
    /// Linkers by name; empty is every one found.
    pub linkers: Vec<String>,
    /// Debuggers by name (`gdb`, `lldb`, `lldb-19`); empty is every one.
    pub debuggers: Vec<String>,
    /// Front ends by name; empty is every one the test allows.
    pub frontends: Vec<String>,
    /// Only the tests with `tier: quick`.
    pub quick: bool,
    /// Compare the `(function, line)` pairs too.
    pub lines: bool,
    pub keep: bool,
    pub verbose: bool,
    pub build_dir: PathBuf,
    pub results: Option<PathBuf>,
    pub require_tools: bool,
    /// kiln++; `None` is the default place.
    pub kiln: Option<PathBuf>,
    pub tool_limits: Limits,
    pub session_limits: Limits,
}

impl Options {
    pub fn new(build_dir: PathBuf) -> Self {
        let cores = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
        Self {
            paths: Vec::new(),
            jobs: 4.min(cores),
            hod: None,
            linkers: Vec::new(),
            debuggers: Vec::new(),
            frontends: Vec::new(),
            quick: false,
            lines: true,
            keep: false,
            verbose: false,
            build_dir,
            results: None,
            require_tools: false,
            kiln: None,
            tool_limits: Limits { time: Duration::from_mins(2), memory_kib: 4 << 20 },
            session_limits: transcript::SESSION_LIMITS,
        }
    }
}

/// The `(function, line)` comparison of one test and front end.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinesResult {
    pub test: String,
    pub frontend: String,
    pub report: LinesReport,
}

/// What a run found.
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub records: Vec<Record>,
    pub lines: Vec<LinesResult>,
    /// Debugger sessions run (both programs counted).
    pub sessions: usize,
    pub notes: Vec<String>,
    /// A tool `HOD_REQUIRE_TOOLS=1` requires is missing.
    pub missing_required: bool,
    /// What was used, one line each.
    pub tools: Vec<String>,
    pub results_file: Option<PathBuf>,
    pub kept: Vec<PathBuf>,
}

impl Report {
    /// 1 a FAIL, XPASS or crash on hod's program, or an unclassified
    /// `(function, line)` difference; else 3 an ERROR, a crash on `llc`'s
    /// program or a missing required tool; else 0.
    pub fn exit_status(&self) -> u8 {
        if self.records.iter().any(Record::fails) || self.lines_total().unclassified() > 0 {
            1
        } else if self.missing_required || self.records.iter().any(Record::errs) {
            3
        } else {
            0
        }
    }

    pub fn count(&self, outcome: Outcome) -> usize {
        self.records.iter().filter(|r| r.outcome == outcome).count()
    }

    /// The lines comparison of every unit together.
    pub fn lines_total(&self) -> LinesReport {
        let mut total = LinesReport::default();
        for result in &self.lines {
            total.add(&result.report);
        }
        total
    }

    pub fn summary(&self, verbose: bool) -> String {
        let mut out = String::new();
        for line in &self.tools {
            let _ = writeln!(out, "debug: {line}");
        }
        for note in &self.notes {
            let _ = writeln!(out, "debug: note: {note}");
        }
        for record in self.records.iter().filter(|r| verbose || r.notable()) {
            let _ = writeln!(
                out,
                "{} {} [{} {} {} {}] {}",
                record.outcome.name(),
                record.test,
                record.frontend,
                record.variant,
                record.linker.as_deref().unwrap_or("-"),
                record.debugger.as_deref().unwrap_or("-"),
                record.detail
            );
        }
        for result in &self.lines {
            let shown = verbose || result.report.unclassified() > 0;
            if shown {
                let _ = writeln!(
                    out,
                    "lines {} [{}]: {}",
                    result.test,
                    result.frontend,
                    result.report.summary()
                );
                let sides =
                    [("hod only", &result.report.hod_only), ("llc only", &result.report.llc_only)];
                for (side, differences) in sides {
                    for ((function, file, line), class) in differences {
                        if verbose || *class == lines::Class::Unclassified {
                            let class = class.name();
                            let _ = writeln!(out, "  {class} {side} {file}:{line} in {function}");
                        }
                    }
                }
            }
        }
        if !self.lines.is_empty() {
            let total = self.lines_total();
            let _ = writeln!(out, "debug: lines: {}", total.summary());
            let _ = writeln!(out, "debug: columns: {}", total.columns.summary());
        }
        let counts: Vec<String> = Outcome::ALL
            .iter()
            .map(|&o| (o, self.count(o)))
            .filter(|&(_, n)| n > 0)
            .map(|(o, n)| format!("{n} {}", o.name()))
            .collect();
        let _ = writeln!(
            out,
            "debug: {} results ({}), {} debugger sessions",
            self.records.len(),
            if counts.is_empty() { "none".to_owned() } else { counts.join(", ") },
            self.sessions
        );
        if let Some(file) = &self.results_file {
            let _ = writeln!(out, "debug: results in {}", file.display());
        }
        for dir in &self.kept {
            let _ = writeln!(out, "debug: kept {}", dir.display());
        }
        out
    }
}

/// Everything a unit (a test and a front end) needs.
struct Run<'a> {
    options: &'a Options,
    tools: &'a Tools,
    debuggers: &'a [Debugger],
    line_tools: Option<&'a LineTools>,
    work: PathBuf,
}

/// A test's name: its path relative to the workspace when it is inside.
fn test_name(path: &Path) -> String {
    let root = crate::irdiff::workspace_root();
    path.strip_prefix(&root).unwrap_or(path).display().to_string()
}

/// Finds the tools of a run, with notes for what is missing.
fn find_tools(options: &Options, report: &mut Report) -> Result<(Tools, Vec<Debugger>), String> {
    use crate::irdiff::tools::{REQUIRED_LINKERS, Search, find_driver, find_linker};
    let search = Search::from_env();
    let missing = |what: String, required: bool, report: &mut Report| {
        report.notes.push(format!("{what} not found"));
        report.missing_required |= required && options.require_tools;
    };
    let mut frontends = Vec::new();
    for major in [19, 22] {
        for cxx in [false, true] {
            let name = format!("{}-{major}", if cxx { "clang++" } else { "clang" });
            match build::find_clang(&search, cxx, major) {
                Some(path) => frontends.push((name, path)),
                None => missing(format!("`{name}`"), true, report),
            }
        }
    }
    let kiln = options
        .kiln
        .clone()
        .unwrap_or_else(|| crate::corpus::capture_defaults(&options.build_dir).kiln);
    if kiln.is_file() {
        frontends.push(("kiln".to_owned(), kiln));
    } else {
        // kiln is a front end of this harness, not a tool of the gate.
        missing(format!("kiln++ ('{}')", kiln.display()), false, report);
    }
    let mut llc = Vec::new();
    for major in [19, 22] {
        match search.versioned("llc", major) {
            Some(path) => llc.push((major, path)),
            None => missing(format!("`llc-{major}`"), true, report),
        }
    }
    let driver = find_driver(&search);
    if driver.is_none() {
        missing("the linker driver `gcc`".to_owned(), true, report);
    }
    let mut linkers = Vec::new();
    for name in ["bfd", "lld", "mold", "wild"] {
        let wanted = options.linkers.is_empty() || options.linkers.iter().any(|l| l == name);
        if !wanted {
            continue;
        }
        match find_linker(name, &search) {
            Some(path) => linkers.push(Linker { name: name.to_owned(), path }),
            None if options.linkers.iter().any(|l| l == name) => {
                return Err(format!("the linker '{name}' was asked for and is not found"));
            }
            None => {
                missing(format!("the linker `{name}`"), REQUIRED_LINKERS.contains(&name), report);
            }
        }
    }
    let mut debuggers = transcript::find_debuggers(&search);
    if !debuggers.iter().any(Debugger::is_gdb) {
        missing("`gdb`".to_owned(), true, report);
    }
    if !debuggers.iter().any(|d| !d.is_gdb()) {
        missing("`lldb`".to_owned(), true, report);
    }
    debuggers.retain(|d| {
        options.debuggers.is_empty()
            || options.debuggers.iter().any(|w| *w == d.name || (w == "lldb" && !d.is_gdb()))
    });
    let hod = match &options.hod {
        Some(hod) => hod.clone(),
        None => crate::irdiff::build_workspace_hod(&options.build_dir)?,
    };
    let kiln_llvm = search.versioned("llc", 22).and_then(|llc| {
        let dir = llc.canonicalize().ok()?.parent()?.parent()?.to_path_buf();
        dir.join("bin/clang++").is_file().then_some(dir)
    });
    report.tools.push(format!("hod {}", hod.display()));
    for (name, path) in &frontends {
        report.tools.push(format!("front end {name}: {}", path.display()));
    }
    for (major, path) in &llc {
        report.tools.push(format!("llc {major}: {}", path.display()));
    }
    for linker in &linkers {
        report.tools.push(format!("linker {}: {}", linker.name, linker.path.display()));
    }
    for debugger in &debuggers {
        report.tools.push(format!("debugger {}: {}", debugger.name, debugger.path.display()));
    }
    Ok((Tools { frontends, llc, hod, driver, linkers, kiln_llvm }, debuggers))
}

/// The tools of the lines comparison: LLVM 22's, else 19's.
pub fn find_line_tools() -> Option<LineTools> {
    let search = crate::irdiff::tools::Search::from_env();
    let find = |tool: &str| [22, 19].into_iter().find_map(|m| search.versioned(tool, m));
    Some(LineTools {
        dwarfdump: find("llvm-dwarfdump")?,
        nm: find("llvm-nm")?,
        objdump: find("llvm-objdump")?,
    })
}

/// Runs made by this process, for unique work directories.
static RUNS: AtomicUsize = AtomicUsize::new(0);

/// Makes a record of a unit: the variant, the linker, the debugger.
type MakeRecord<'a> = dyn Fn(&str, Option<&str>, Option<&str>, Outcome, String) -> Record + 'a;

/// Runs the debugger tests.
pub fn run(options: &Options) -> Result<Report, String> {
    let mut report = Report::default();
    let paths = if options.paths.is_empty() {
        vec![crate::irdiff::workspace_root().join("tests").join("debug")]
    } else {
        options.paths.clone()
    };
    let files = directives::discover(&paths)?;
    let (tools, debuggers) = find_tools(options, &mut report)?;
    let line_tools = if options.lines { find_line_tools() } else { None };
    if options.lines && line_tools.is_none() {
        report.notes.push("llvm-dwarfdump, llvm-nm or llvm-objdump not found: no lines".into());
        report.missing_required |= options.require_tools;
    }
    if !cfg!(target_os = "linux") {
        report.notes.push("programs are linked and run on Linux only".to_owned());
        return Ok(report);
    }
    let mut units: Vec<(usize, &'static str)> = Vec::new();
    let mut tests = Vec::new();
    for path in &files {
        let name = test_name(path);
        let parsed = fs::read_to_string(path)
            .map_err(|e| format!("cannot read: {e}"))
            .and_then(|text| directives::parse(path, &text));
        match parsed {
            Ok(test) if options.quick && !test.quick => {}
            Ok(test) => {
                for frontend in test.frontends() {
                    if options.frontends.is_empty()
                        || options.frontends.iter().any(|f| f == frontend)
                    {
                        units.push((tests.len(), frontend));
                    }
                }
                tests.push((name, test));
            }
            Err(e) => report.records.push(Record {
                test: name,
                frontend: "-".into(),
                variant: "-".into(),
                linker: None,
                debugger: None,
                outcome: Outcome::Error,
                detail: format!("directives: {e}"),
            }),
        }
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    // Unique to the process and the run: tests make several runs at once.
    let nth = RUNS.fetch_add(1, Ordering::Relaxed);
    let work =
        options.build_dir.join("l7").join(format!("run-{}-{stamp}-{nth}", std::process::id()));
    fs::create_dir_all(&work).map_err(|e| format!("cannot create '{}': {e}", work.display()))?;
    build::prepare_linkers(&work, &tools.linkers)?;
    let run = Run {
        options,
        tools: &tools,
        debuggers: &debuggers,
        line_tools: line_tools.as_ref(),
        work: work.clone(),
    };
    let next = AtomicUsize::new(0);
    let slots = Mutex::new(vec![None; units.len()]);
    std::thread::scope(|scope| {
        for _ in 0..options.jobs.clamp(1, units.len().max(1)) {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(&(test, frontend)) = units.get(index) else { break };
                    let (name, test) = &tests[test];
                    let done = run.unit(index, name, test, frontend);
                    slots.lock().unwrap_or_else(PoisonError::into_inner)[index] = Some(done);
                }
            });
        }
    });
    for done in slots.into_inner().unwrap_or_else(PoisonError::into_inner).into_iter().flatten() {
        report.records.extend(done.records);
        report.sessions += done.sessions;
        report.lines.extend(done.lines);
        report.kept.extend(done.kept);
    }
    if report.kept.is_empty() {
        let _ = fs::remove_dir_all(&work);
    } else {
        let _ = fs::remove_dir_all(work.join("linkers"));
    }
    let results =
        options.results.clone().unwrap_or_else(|| options.build_dir.join("l7").join("results.tsv"));
    let mut text = String::from("test\tfrontend\tvariant\tlinker\tdebugger\toutcome\tdetail\n");
    for record in &report.records {
        text.push_str(&record.line());
        text.push('\n');
    }
    fs::write(&results, text).map_err(|e| format!("cannot write '{}': {e}", results.display()))?;
    report.results_file = Some(results);
    Ok(report)
}

/// What one unit gave.
#[derive(Clone, Debug, Default)]
struct Done {
    records: Vec<Record>,
    sessions: usize,
    lines: Vec<LinesResult>,
    kept: Vec<PathBuf>,
}

/// A program to debug: its variant and objects.
struct Variant {
    name: String,
    objects: Vec<PathBuf>,
}

impl Run<'_> {
    fn unit(&self, index: usize, name: &str, test: &DebugTest, frontend: &str) -> Done {
        let stem = test.path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let dir = self.work.join(format!("{index:04}-{stem}-{frontend}"));
        let mut done = Done::default();
        let record = |variant: &str,
                      linker: Option<&str>,
                      debugger: Option<&str>,
                      outcome: Outcome,
                      detail: String| {
            // An xfail turns a FAIL (or a crash on hod's program) into an
            // XFAIL and a PASS into an XPASS.
            let xfail = test.xfail(frontend, linker, debugger);
            let (outcome, detail) = match (outcome, xfail) {
                (Outcome::Fail, Some(x)) => (Outcome::Xfail, format!("{}; {detail}", x.reason)),
                (Outcome::Crash, Some(x)) if variant != "llc" => {
                    (Outcome::Xfail, format!("{}; crashed: {detail}", x.reason))
                }
                (Outcome::Pass, Some(x)) => (Outcome::Xpass, x.reason.clone()),
                (outcome, _) => (outcome, detail),
            };
            Record {
                test: name.to_owned(),
                frontend: frontend.to_owned(),
                variant: variant.to_owned(),
                linker: linker.map(str::to_owned),
                debugger: debugger.map(str::to_owned),
                outcome,
                detail,
            }
        };
        if let Err(e) = fs::create_dir_all(&dir) {
            done.records.push(record("-", None, None, Outcome::Error, format!("{e}")));
            return done;
        }
        self.unit_in(&dir, test, frontend, &record, &mut done);
        if self.options.keep && done.records.iter().any(|r| r.fails() || r.errs()) {
            done.kept.push(dir);
        } else {
            let _ = fs::remove_dir_all(&dir);
        }
        done
    }

    #[expect(clippy::too_many_lines, reason = "the steps of one test, in order")]
    fn unit_in(
        &self,
        dir: &Path,
        test: &DebugTest,
        frontend: &str,
        record: &MakeRecord<'_>,
        done: &mut Done,
    ) {
        let limits = self.options.tool_limits;
        let major = build::frontend_major(frontend);
        let tool = build::frontend_tool(frontend, test.lang);
        let skip = |why: String| record("-", None, None, Outcome::Skip, why);
        if test.lang != Lang::Ir && self.tools.frontend(&tool).is_none() {
            done.records.push(skip(format!("no {tool}")));
            return;
        }
        let Some(llc) = self.tools.llc(major) else {
            done.records.push(skip(format!("no llc-{major}")));
            return;
        };
        // 1. IR, and both objects of every module.
        let (mut ours, mut theirs) = (Vec::new(), Vec::new());
        let mut facts = IrFacts::default();
        // Split DWARF: each object with a `.dwo`, and the name it records.
        let mut dwos: Vec<(PathBuf, String)> = Vec::new();
        for (i, module) in test.modules.iter().enumerate() {
            let ir = dir.join(format!("m{i}.ll"));
            if let Err(e) = build::frontend_ir(self.tools, test, frontend, module, &ir, limits) {
                done.records.push(record("llc", None, None, Outcome::Error, e));
                return;
            }
            facts.add_module(&fs::read_to_string(&ir).unwrap_or_default());
            let llc_o = dir.join(format!("m{i}.llc.o"));
            if let Err(e) = build::llc_object(llc, &ir, &llc_o, limits) {
                done.records.push(record("llc", None, None, Outcome::Error, e));
                return;
            }
            let hod_o = dir.join(format!("m{i}.hod.o"));
            match build::hod_object(&self.tools.hod, &ir, &hod_o, limits) {
                HodBuild::Built => {}
                HodBuild::Declined(reason) => {
                    let what = format!("hod declined module {i}: {reason}");
                    done.records.push(record("hod", None, None, Outcome::Fail, what));
                    return;
                }
                HodBuild::Failed(why) => {
                    let what = format!("module {i}: {why}");
                    done.records.push(record("hod", None, None, Outcome::Fail, what));
                    return;
                }
            }
            if let Some(name) = build::split_dwarf_name(&ir) {
                dwos.push((hod_o.clone(), name.clone()));
                dwos.push((llc_o.clone(), name));
            }
            ours.push(hod_o);
            theirs.push(llc_o);
        }
        let mut variants = vec![Variant { name: "hod".to_owned(), objects: ours.clone() }];
        if test.objects == Objects::Mixed {
            for (i, module) in test.modules.iter().enumerate() {
                let mut objects = theirs.clone();
                objects[i].clone_from(&ours[i]);
                let file = module.file_name().unwrap_or_default().to_string_lossy();
                variants.push(Variant { name: format!("hod:{file}"), objects });
            }
        }
        let cxx =
            test.lang == Lang::Cxx || test.modules.iter().any(|m| Lang::of(m) == Some(Lang::Cxx));
        // 2. Per linker: link, then every debugger on every program.
        for (number, linker) in self.tools.linkers.iter().enumerate() {
            let linker_dir = self.work.join("linkers").join(&linker.name);
            let program = |variant: &str| {
                dir.join(format!("{}-{}", variant.replace(':', "_"), linker.name)).join("prog")
            };
            let link = |name: &str, objects: &[PathBuf]| {
                let out = program(name);
                fs::create_dir_all(out.parent().unwrap_or(dir))
                    .map_err(|e| format!("{e}"))
                    .and_then(|()| {
                        build::link(
                            self.tools,
                            &linker_dir,
                            linker,
                            objects,
                            &test.link,
                            cxx,
                            &out,
                            limits,
                        )
                    })
                    .and_then(|()| build::place_dwos(objects, &dwos, &out))
                    .map(|()| out)
            };
            let at_linker = Some(linker.name.as_str());
            let reference = match link("llc", &theirs) {
                Ok(p) => p,
                Err(e) => {
                    done.records.push(record("llc", at_linker, None, Outcome::Error, e));
                    continue;
                }
            };
            let mut programs = Vec::new();
            for variant in &variants {
                match link(&variant.name, &variant.objects) {
                    Ok(p) => programs.push((variant.name.clone(), p)),
                    Err(e) => {
                        done.records.push(record(&variant.name, at_linker, None, Outcome::Fail, e));
                    }
                }
            }
            // The (function, line) comparison, with the first linker.
            if number == 0
                && let Some(line_tools) = self.line_tools
                && let Some((_, ours)) = programs.iter().find(|(v, _)| v == "hod")
            {
                match lines::compare(ours, &reference, line_tools, &facts) {
                    Ok(report) => done.lines.push(LinesResult {
                        test: test_name(&test.path),
                        frontend: frontend.to_owned(),
                        report,
                    }),
                    Err(e) => done.records.push(record("hod", at_linker, None, Outcome::Error, e)),
                }
            }
            for debugger in self.debuggers {
                let script = if debugger.is_gdb() { &test.gdb } else { &test.lldb };
                if script.commands.is_empty() {
                    continue;
                }
                let at_debugger = Some(debugger.name.as_str());
                let limits = self.options.session_limits;
                let session = |p: &Path| transcript::session(debugger, p, &script.commands, limits);
                let save = |p: &Path, printed: &str| {
                    let _ = fs::write(p.with_file_name(format!("{}.txt", debugger.name)), printed);
                };
                done.sessions += 1;
                // When `llc`'s session is not what the test says, hod's
                // sessions still run (their transcripts are kept for
                // `--keep`), and each is an ERROR.
                let mut broken = None;
                let theirs_text = match session(&reference) {
                    Ok(Ended::Done(printed)) => {
                        let masked = transcript::mask(&printed, reference.parent().unwrap_or(dir));
                        save(&reference, &masked);
                        if let Some(miss) = directives::first_missing_check(&masked, &script.checks)
                        {
                            broken = Some(format!("llc's session lacks the check '{miss}'"));
                        }
                        masked
                    }
                    Ok(Ended::Crashed(printed)) => {
                        save(&reference, &printed);
                        let why = format!("{} crashed on llc's program", debugger.name);
                        done.records.push(record(
                            "llc",
                            at_linker,
                            at_debugger,
                            Outcome::Crash,
                            why,
                        ));
                        continue;
                    }
                    Ok(Ended::TimedOut(_)) => {
                        let why = "llc's session timed out".to_owned();
                        done.records.push(record(
                            "llc",
                            at_linker,
                            at_debugger,
                            Outcome::Error,
                            why,
                        ));
                        continue;
                    }
                    Err(e) => {
                        done.records.push(record("llc", at_linker, at_debugger, Outcome::Error, e));
                        continue;
                    }
                };
                for (variant, ours) in &programs {
                    done.sessions += 1;
                    let (outcome, detail) = match session(ours) {
                        Ok(Ended::Done(printed)) => {
                            let masked = transcript::mask(&printed, ours.parent().unwrap_or(dir));
                            save(ours, &masked);
                            match transcript::first_difference(&masked, &theirs_text) {
                                None => (Outcome::Pass, String::new()),
                                // `spec/dwarf.md` §5.7 `return-column`.
                                Some(_)
                                    if !debugger.is_gdb()
                                        && permitted::only_return_columns(
                                            &masked,
                                            &theirs_text,
                                            &facts,
                                        ) =>
                                {
                                    (Outcome::Pass, "permitted: return-column".to_owned())
                                }
                                Some((number, hod_line, llc_line)) => (
                                    Outcome::Fail,
                                    format!(
                                        "line {number}: hod's '{hod_line}', llc's '{llc_line}'"
                                    ),
                                ),
                            }
                        }
                        Ok(Ended::Crashed(printed)) => {
                            save(ours, &printed);
                            (Outcome::Crash, format!("{} crashed on hod's program", debugger.name))
                        }
                        Ok(Ended::TimedOut(_)) => {
                            (Outcome::Fail, "hod's session timed out".to_owned())
                        }
                        Err(e) => (Outcome::Error, e),
                    };
                    let (outcome, detail) = match &broken {
                        Some(why) => (Outcome::Error, format!("{why} (hod's: {})", outcome.name())),
                        None => (outcome, detail),
                    };
                    done.records.push(record(variant, at_linker, at_debugger, outcome, detail));
                }
            }
        }
    }
}
