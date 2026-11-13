//! One IR file and its expectations in, outcomes out: build with the
//! oracle and with hod, link each object with every linker, run, compare.
//! Contract: `spec/harness.md` "irdiff", "What a test does" and "Outcomes".
//!
//! Nothing here knows about `tests/ir/`: a [`Test`] is a path and an
//! [`Expectations`], wherever they came from. The file is one module, or
//! several that `;@ module:` lines separate ("Several modules").

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

use super::directives::{self, Arch, Exit, Expectations, Lib};
use super::process::{self, End, Limits, Output};
use super::report::{BuiltBy, Failure, Outcome, Record};
use super::tools::{self, Linker, Toolchain};

/// One test: IR with a `main`, and how its program must behave.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Test {
    /// The name in reports.
    pub name: String,
    /// The test file, LLVM text: one module, or several, each after a
    /// `;@ module: <name>` line.
    pub ir: PathBuf,
    pub expect: Expectations,
}

/// What the oracle's `llc` gets after `-O0` and hod does not: the
/// instruction selector. At `-O0` the default selectors (`GlobalISel` on
/// AArch64, `FastISel` on both targets) are the ones only `-O0` builds use,
/// and both miscompile correct programs on AArch64 (`spec/interp.md`
/// §10.1); `SelectionDAG` is the selector every optimized build uses.
pub const REFERENCE_SELECTOR: [&str; 2] = ["-global-isel=0", "-fast-isel=0"];

/// How many times longer than on the host a program may run under the
/// emulator ("Cross runs"): `qemu-user` translates every block it runs,
/// and an emulated C++ program has taken twenty times its native time at
/// load 30. The limit guards against hangs; it is never a measure of speed.
pub const EMULATION_SLOWDOWN: u32 = 6;

/// How the detail of an ERROR starts when the witness raised it: the
/// `llc -O2` build of the test did not confirm the reference's.
pub const WITNESS: &str = "witness: ";

/// The name, in the work directory, of the fallback hod gets in hybrid
/// mode: a script that runs the oracle's `llc` as the reference is run.
const REFERENCE_LLC: &str = "reference-llc";

/// The fallback of hybrid mode for a test whose input is IR before
/// optimisation: `opt` as the reference runs it, then [`REFERENCE_LLC`]'s
/// `llc`.
const REFERENCE_OPT_LLC: &str = "reference-opt-llc";

/// What the oracle runs before `llc` on IR before optimisation: the
/// passes a compiler driver runs at `-O0`, among them the coroutine
/// passes (`spec/harness.md` "Test directives", `input`).
pub const PRE_OPT_PASSES: &str = "-passes=default<O0>";

/// What hod may do with a module it does not support.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Decline it (`hod --strict`). A decline the test does not expect is
    /// a failure.
    Strict,
    /// Hand it to the oracle's `llc` (`hod --fallback=<llc>`).
    Hybrid,
}

/// Everything that is the same for every test of a run.
#[derive(Clone, Debug)]
pub struct Session {
    pub toolchain: Toolchain,
    /// The target the objects are built for.
    pub arch: Arch,
    /// The machine's own architecture. Programs are linked and run only
    /// when it equals `arch`.
    pub host: Option<Arch>,
    pub mode: Mode,
    /// For `llc`, `hod` and the linker driver.
    pub tool_limits: Limits,
    /// For the programs under test.
    pub run_limits: Limits,
    /// The run's work directory. [`Session::prepare`] creates it; removing
    /// it afterwards is the caller's business.
    pub work: PathBuf,
    /// Keep the work directory of a test with a failing outcome.
    pub keep: bool,
    /// Also build every test with `llc -O2` and require its program to
    /// behave as the reference's ("The witness").
    pub witness: bool,
}

/// Which compiler configuration builds an object.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Build {
    /// The oracle: `llc -O0` with [`REFERENCE_SELECTOR`].
    Reference,
    /// The oracle's second opinion: `llc -O2`.
    Witness,
    /// hod: `-O0`.
    Hod,
}

/// One module of a test and where its objects go.
struct Part {
    /// The module's name, for a test of several.
    name: Option<String>,
    ir: PathBuf,
    /// `opt`'s output, for a test whose input is IR before optimisation.
    post: PathBuf,
    oracle: PathBuf,
    witness: PathBuf,
    hod: PathBuf,
    /// hod's coverage log for this module.
    log: PathBuf,
}

impl Part {
    /// "module 'name': " for a test of several modules.
    fn label(&self) -> String {
        self.name.as_ref().map_or_else(String::new, |name| format!("module '{name}': "))
    }
}

/// The variables that configure hod; the harness passes options instead.
const HOD_VARIABLES: [&str; 6] = [
    "HOD_FALLBACK_LLC",
    "HOD_STRICT",
    "HOD_COVERAGE_LOG",
    "HOD_BITCODE_VIA",
    "HOD_IN_FALLBACK",
    "HOD_TIME_PHASES",
];

/// What became of hod's side of the build.
enum HodBuild {
    /// `hod.o` exists; hod or its fallback wrote it.
    Built,
    /// hod declined the module (exit status 2).
    Declined,
    /// Anything else, described.
    Failed(String),
}

/// What is known about a test once both compilers have run.
#[derive(Debug, PartialEq, Eq)]
enum Plan {
    /// Link hod's object, run it, compare.
    Compare,
    /// The outcome on hod's side is already known, whatever the linker.
    Settled { outcome: Outcome, failure: Option<Failure>, detail: String, may_xfail: bool },
}

/// Why a step did not give a result.
enum Trouble {
    /// The step ran and failed, in the way the kind says.
    Failed(Failure, String),
    /// The harness or a tool broke: an ERROR on either side.
    Broken(String),
}

impl Trouble {
    /// The kind and the detail; a broken step is the harness's.
    fn split(self) -> (Failure, String) {
        match self {
            Self::Failed(failure, detail) => (failure, detail),
            Self::Broken(detail) => (Failure::Harness, detail),
        }
    }
}

impl Session {
    /// Creates the work directory and, in it, one directory per linker
    /// holding that linker under the name `ld`: the linker driver uses the
    /// `ld` of the directory it is given with `-B`.
    pub fn prepare(&self) -> Result<(), String> {
        for linker in &self.toolchain.linkers {
            let dir = self.linker_dir(linker);
            fs::create_dir_all(&dir)
                .map_err(|e| format!("cannot create '{}': {e}", dir.display()))?;
            let link = dir.join("ld");
            let _ = fs::remove_file(&link);
            std::os::unix::fs::symlink(&linker.path, &link)
                .map_err(|e| format!("cannot create '{}': {e}", link.display()))?;
        }
        fs::create_dir_all(&self.work)
            .map_err(|e| format!("cannot create '{}': {e}", self.work.display()))?;
        if self.mode == Mode::Hybrid {
            self.write_reference_llc()?;
        }
        Ok(())
    }

    /// The fallback of hybrid mode: hod runs it with its own arguments
    /// (`-O0 ...`), and it adds the reference's selector, so that what the
    /// fallback builds is what the oracle builds.
    fn reference_llc(&self) -> PathBuf {
        self.work.join(REFERENCE_LLC)
    }

    /// The fallback of hybrid mode for IR before optimisation: it runs
    /// `opt` on the input (hod's last argument) and gives the result to
    /// `llc` on its standard input, with hod's other arguments.
    fn reference_opt_llc(&self) -> PathBuf {
        self.work.join(REFERENCE_OPT_LLC)
    }

    fn write_reference_llc(&self) -> Result<(), String> {
        use std::os::unix::fs::PermissionsExt;
        let quote = |path: &Path| path.to_string_lossy().replace('\'', "'\\''");
        let llc = quote(&self.toolchain.llc);
        let opt = quote(&tools::opt_beside(&self.toolchain.llc));
        let selector = tools::reference_selector(self.arch).join(" ");
        let plain = format!("#!/bin/sh\nexec '{llc}' \"$@\" {selector}\n");
        // The word list of `for` is expanded once: `set --` in the loop
        // rebuilds the arguments without the last, the input. `opt`'s
        // output goes beside hod's output (`-o`), in the test's directory.
        let pre_opt = format!(
            "#!/bin/sh\nn=$# i=0 input= out= prev=\n\
             for a do\n  i=$((i + 1))\n  [ \"$i\" -eq 1 ] && set --\n  \
             if [ \"$i\" -lt \"$n\" ]; then set -- \"$@\" \"$a\"; else input=$a; fi\n  \
             [ \"$prev\" = -o ] && out=$a\n  prev=$a\n\
             done\n\
             post=\"$out.pre-opt.ll\"\n\
             '{opt}' '{PRE_OPT_PASSES}' -S -o \"$post\" \"$input\" || exit 1\n\
             '{llc}' \"$@\" {selector} \"$post\"\nstatus=$?\nrm -f \"$post\"\nexit $status\n"
        );
        for (path, script) in [(self.reference_llc(), plain), (self.reference_opt_llc(), pre_opt)] {
            fs::write(&path, script)
                .and_then(|()| fs::set_permissions(&path, fs::Permissions::from_mode(0o755)))
                .map_err(|e| format!("cannot write '{}': {e}", path.display()))?;
        }
        Ok(())
    }

    /// The limits of a program under the emulator: [`EMULATION_SLOWDOWN`]
    /// times the wall-clock time of [`Session::run_limits`], the same
    /// address space.
    pub fn emulated_run_limits(&self) -> Limits {
        Limits { time: self.run_limits.time * EMULATION_SLOWDOWN, ..self.run_limits }
    }

    /// The limits the programs of this session run under.
    fn program_limits(&self) -> Limits {
        if self.toolchain.emulator.is_some() { self.emulated_run_limits() } else { self.run_limits }
    }

    fn linker_dir(&self, linker: &Linker) -> PathBuf {
        self.work.join("linkers").join(&linker.name)
    }

    /// Why programs cannot be linked and run in this session, if so.
    pub fn cannot_run(&self) -> Option<String> {
        if self.host != Some(self.arch) && self.toolchain.emulator.is_none() {
            let host = self.host.map_or("this", Arch::name);
            Some(format!(
                "cross target: {} programs cannot be linked or run on {host} host",
                self.arch.name()
            ))
        } else if self.toolchain.driver.is_none() {
            Some("no linker driver (gcc) found".to_owned())
        } else if self.toolchain.linkers.is_empty() {
            Some("no linker found".to_owned())
        } else {
            None
        }
    }

    /// Runs `tests`, `jobs` at a time. The result has one list of records
    /// per test, in the order of `tests`, whatever `jobs` is.
    pub fn run_tests(&self, tests: &[Test], jobs: usize) -> Result<Vec<Vec<Record>>, String> {
        self.prepare()?;
        let next = AtomicUsize::new(0);
        let slots = Mutex::new(vec![None; tests.len()]);
        std::thread::scope(|scope| {
            for _ in 0..jobs.clamp(1, tests.len().max(1)) {
                scope.spawn(|| {
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(test) = tests.get(index) else { break };
                        let records = self.run_test(index, test);
                        slots.lock().unwrap_or_else(PoisonError::into_inner)[index] = Some(records);
                    }
                });
            }
        });
        let slots = slots.into_inner().unwrap_or_else(PoisonError::into_inner);
        Ok(slots.into_iter().map(Option::unwrap_or_default).collect())
    }

    /// Runs one test in its own directory under the work directory
    /// (`index` keeps the directories of tests with equal names apart).
    /// The session must be prepared.
    pub fn run_test(&self, index: usize, test: &Test) -> Vec<Record> {
        let safe: String = test
            .name
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c } else { '_' })
            .collect();
        let tail = &safe[safe.len().saturating_sub(60)..];
        let dir = self.work.join(format!("{index:04}-{tail}"));
        let records = match fs::create_dir_all(&dir) {
            Ok(()) => self.run_in(test, &dir),
            Err(e) => vec![self.record(
                test,
                None,
                (Outcome::Error, Some(Failure::Harness)),
                format!("cannot create '{}': {e}", dir.display()),
            )],
        };
        if !(self.keep && records.iter().any(|record| record.outcome.is_failure())) {
            let _ = fs::remove_dir_all(&dir);
        }
        records
    }

    fn record(
        &self,
        test: &Test,
        linker: Option<&Linker>,
        (outcome, failure): (Outcome, Option<Failure>),
        detail: String,
    ) -> Record {
        Record {
            outcome,
            failure,
            test: test.name.clone(),
            target: self.arch.triple().to_owned(),
            llvm: Some(self.toolchain.llvm_major),
            linker: linker.map(|linker| linker.name.clone()),
            built_by: BuiltBy::Nobody,
            reason: None,
            detail,
            covers: test.expect.covers.clone(),
        }
    }

    fn run_in(&self, test: &Test, dir: &Path) -> Vec<Record> {
        let expect = &test.expect;
        let (arch, llvm) = (self.arch, self.toolchain.llvm_major);
        if !expect.runs_on(arch) {
            let wanted: Vec<&str> = expect.targets.iter().map(|arch| arch.name()).collect();
            let detail =
                format!("requires target {} (this run: {})", wanted.join(" "), arch.name());
            return vec![self.record(test, None, (Outcome::Skip, None), detail)];
        }
        if !expect.accepts_llvm(llvm) {
            let detail = format!("requires another LLVM (this run: {llvm})");
            return vec![self.record(test, None, (Outcome::Skip, None), detail)];
        }
        if let Some(dialect) = expect.dialect.filter(|_| !expect.in_dialect(llvm)) {
            let detail = format!("in the LLVM {dialect} dialect (this run: {llvm})");
            return vec![self.record(test, None, (Outcome::Skip, None), detail)];
        }
        let oracle_skip = expect.oracle_skip_reason(llvm).map(str::to_owned);

        let error = |failure: Failure, detail: String| {
            vec![self.record(test, None, (Outcome::Error, Some(failure)), detail)]
        };
        let parts = match Self::parts(test, dir) {
            Ok(parts) => parts,
            Err(detail) => return error(Failure::Harness, detail),
        };
        let oracle_used = oracle_skip.is_none();
        for part in parts.iter().filter(|_| oracle_used) {
            let built = self
                .run_opt(test, part)
                .and_then(|()| self.compile_with_llc(test, Build::Reference, part));
            if let Err(trouble) = built {
                let (failure, detail) = trouble.split();
                return error(failure, format!("oracle: {}{detail}", part.label()));
            }
        }
        if self.witness && oracle_used {
            for part in &parts {
                if let Err(trouble) = self.compile_with_llc(test, Build::Witness, part) {
                    let detail = trouble.split().1;
                    return error(Failure::Witness, format!("{WITNESS}{}{detail}", part.label()));
                }
            }
        }

        let (build, built_by, reason) = self.compile_all_with_hod(test, &parts);
        let plan = plan(&build, built_by, reason.as_deref(), expect.unsupported_reason(arch, llvm));
        let case = Case { session: self, test, built_by, reason, plan, oracle_skip };

        if let Some(why) = self.cannot_run() {
            let note = format!("the oracle's program was not run: {why}");
            let record = case.settled(None, &note);
            return vec![record.unwrap_or_else(|| case.finish(None, (Outcome::Skip, None), why))];
        }
        let objects = |object: fn(&Part) -> &PathBuf| -> Vec<PathBuf> {
            parts.iter().map(|part| object(part).clone()).collect()
        };
        let (oracle, hod) = (objects(|part| &part.oracle), objects(|part| &part.hod));
        let witness = (self.witness && oracle_used).then(|| objects(|part| &part.witness));
        // The witness is asked once, with the first linker whose program
        // of the oracle's objects is what the directives say: what it
        // checks is the compiler. An oracle it does not confirm is an
        // ERROR for that linker and every later one.
        let mut unconfirmed: Option<String> = None;
        let mut asked = false;
        let mut records = Vec::new();
        for linker in &self.toolchain.linkers {
            if let Some(detail) = &unconfirmed {
                let error = (Outcome::Error, Some(Failure::Witness));
                records.push(case.finish(Some(linker), error, detail.clone()));
                continue;
            }
            let witness = witness.as_deref().filter(|_| !asked);
            records.push(match case.with_linker(linker, dir, &oracle, &hod, witness, &mut asked) {
                Ok(record) => record,
                Err(detail) => {
                    let error = (Outcome::Error, Some(Failure::Witness));
                    let record = case.finish(Some(linker), error, detail.clone());
                    unconfirmed = Some(detail);
                    record
                }
            });
        }
        records
    }

    /// The modules of the test and where their objects go: the file
    /// itself, or one file per `;@ module:` part, written under `dir`.
    fn parts(test: &Test, dir: &Path) -> Result<Vec<Part>, String> {
        // A file that is not text is one module; `llc` says what is wrong
        // with it.
        let modules =
            fs::read_to_string(&test.ir).map(|text| directives::modules(&text)).unwrap_or_default();
        if modules.is_empty() {
            return Ok(vec![Part {
                name: None,
                ir: test.ir.clone(),
                post: dir.join("pre-opt.ll"),
                oracle: dir.join("oracle.o"),
                witness: dir.join("witness.o"),
                hod: dir.join("hod.o"),
                log: dir.join("coverage.tsv"),
            }]);
        }
        let dir = dir.join("modules");
        fs::create_dir_all(&dir).map_err(|e| format!("cannot create '{}': {e}", dir.display()))?;
        let mut parts = Vec::new();
        for module in modules {
            let file = |suffix: &str| dir.join(format!("{}.{suffix}", module.name));
            let ir = file("ll");
            fs::write(&ir, &module.text)
                .map_err(|e| format!("cannot write '{}': {e}", ir.display()))?;
            parts.push(Part {
                ir,
                post: file("post.ll"),
                oracle: file("oracle.o"),
                witness: file("witness.o"),
                hod: file("hod.o"),
                log: file("coverage.tsv"),
                name: Some(module.name),
            });
        }
        Ok(parts)
    }

    /// The code-generation arguments of one build of one module. hod gets
    /// what a compiler driver gives `llc` at `-O0`; the selector is the
    /// oracle's own.
    fn codegen_args(&self, test: &Test, build: Build, part: &Part) -> Vec<OsString> {
        let model = if test.expect.pie { "pic" } else { "static" };
        let mut args: Vec<OsString> = match build {
            Build::Reference => std::iter::once("-O0")
                .chain(tools::reference_selector(self.arch).iter().copied())
                .map(OsString::from)
                .collect(),
            Build::Witness => vec!["-O2".into()],
            Build::Hod => vec!["-O0".into()],
        };
        args.push("-filetype=obj".into());
        args.push(format!("-mtriple={}", self.arch.triple()).into());
        args.push(format!("-relocation-model={model}").into());
        args.extend(test.expect.llc_args.iter().map(OsString::from));
        args.push("-o".into());
        args.push(
            match build {
                Build::Reference => &part.oracle,
                Build::Witness => &part.witness,
                Build::Hod => &part.hod,
            }
            .into(),
        );
        // The oracle and the witness build `opt`'s output of IR before
        // optimisation; hod builds the input.
        let input = if test.expect.pre_opt && build != Build::Hod { &part.post } else { &part.ir };
        args.push(input.clone().into());
        args
    }

    /// For a test whose input is IR before optimisation, runs `opt` with
    /// [`PRE_OPT_PASSES`] on one module: what the oracle and the witness
    /// build.
    fn run_opt(&self, test: &Test, part: &Part) -> Result<(), Trouble> {
        if !test.expect.pre_opt {
            return Ok(());
        }
        let opt = tools::opt_beside(&self.toolchain.llc);
        if !opt.is_file() {
            return Err(Trouble::Broken(format!(
                "no opt beside '{}'",
                self.toolchain.llc.display()
            )));
        }
        let triple = format!("-mtriple={}", self.arch.triple());
        let output = process::run(
            &opt,
            |c| {
                c.args([PRE_OPT_PASSES, triple.as_str(), "-S", "-o"]).arg(&part.post).arg(&part.ir);
            },
            b"",
            self.tool_limits,
        )
        .map_err(Trouble::Broken)?;
        let failed =
            |detail| Trouble::Failed(Failure::of_tool(Failure::ReferenceBuild, output.end), detail);
        match output.end {
            End::Exit(0) if part.post.is_file() => Ok(()),
            End::Exit(0) => Err(failed("opt wrote no module".to_owned())),
            end => Err(failed(tool_failure("opt", end, &output, self.tool_limits))),
        }
    }

    /// Builds one module with the oracle's `llc`, as the reference or as
    /// the witness.
    fn compile_with_llc(&self, test: &Test, build: Build, part: &Part) -> Result<(), Trouble> {
        let (tool, object) = match build {
            Build::Witness => ("llc -O2", &part.witness),
            _ => ("llc", &part.oracle),
        };
        let args = self.codegen_args(test, build, part);
        let output = process::run(
            &self.toolchain.llc,
            |c| {
                c.args(&args);
            },
            b"",
            self.tool_limits,
        )
        .map_err(Trouble::Broken)?;
        let failed =
            |detail| Trouble::Failed(Failure::of_tool(Failure::ReferenceBuild, output.end), detail);
        match output.end {
            End::Exit(0) if object.is_file() => Ok(()),
            End::Exit(0) => Err(failed(format!("{tool} wrote no object"))),
            end => Err(failed(tool_failure(tool, end, &output, self.tool_limits))),
        }
    }

    /// Runs hod on each module in order, until one is not built. A test
    /// is built by hod when every module is, and by the fallback when
    /// one is: the reason is the first such module's. The first module
    /// hod declines or fails on decides the test.
    fn compile_all_with_hod(
        &self,
        test: &Test,
        parts: &[Part],
    ) -> (HodBuild, BuiltBy, Option<String>) {
        let mut fallback: Option<Option<String>> = None;
        for part in parts {
            match self.compile_with_hod(test, part) {
                (HodBuild::Built, BuiltBy::Hod, _) => {}
                (HodBuild::Built, _, reason) => {
                    fallback.get_or_insert(reason);
                }
                (HodBuild::Failed(detail), built_by, reason) => {
                    let detail = format!("{}{detail}", part.label());
                    return (HodBuild::Failed(detail), built_by, reason);
                }
                declined => return declined,
            }
        }
        match fallback {
            Some(reason) => (HodBuild::Built, BuiltBy::Fallback, reason),
            None => (HodBuild::Built, BuiltBy::Hod, None),
        }
    }

    /// Runs hod on one module. Besides what became of the build: who built
    /// the object, and the coverage-log reason.
    fn compile_with_hod(&self, test: &Test, part: &Part) -> (HodBuild, BuiltBy, Option<String>) {
        let (log, object) = (&part.log, &part.hod);
        let mut args = vec![tools::prefixed("--coverage-log=", log)];
        args.push(match self.mode {
            Mode::Strict => "--strict".into(),
            Mode::Hybrid if test.expect.pre_opt => {
                tools::prefixed("--fallback=", &self.reference_opt_llc())
            }
            Mode::Hybrid => tools::prefixed("--fallback=", &self.reference_llc()),
        });
        args.extend(self.codegen_args(test, Build::Hod, part));
        let configure = |command: &mut std::process::Command| {
            command.args(&args);
            for variable in HOD_VARIABLES {
                command.env_remove(variable);
            }
        };
        let failed = |detail: String| (HodBuild::Failed(detail), BuiltBy::Nobody, None);
        let output = match process::run(&self.toolchain.hod, configure, b"", self.tool_limits) {
            Ok(output) => output,
            Err(detail) => return failed(detail),
        };
        let (outcome, reason) = match read_coverage(log) {
            Ok(line) => line,
            Err(problem) => {
                let detail = tool_failure("hod", output.end, &output, self.tool_limits);
                return failed(format!("{detail}; {problem}"));
            }
        };
        match (output.end, outcome.as_str(), self.mode) {
            (End::Exit(0), "hod", _) if object.is_file() => (HodBuild::Built, BuiltBy::Hod, None),
            (End::Exit(0), "fallback", Mode::Hybrid) if object.is_file() => {
                (HodBuild::Built, BuiltBy::Fallback, Some(reason))
            }
            (End::Exit(0), "hod" | "fallback", _) if !object.is_file() => {
                failed("hod exited with status 0 and wrote no object".to_owned())
            }
            (End::Exit(2), "error", _) => (HodBuild::Declined, BuiltBy::Declined, Some(reason)),
            (end, _, _) => {
                let detail = tool_failure("hod", end, &output, self.tool_limits);
                failed(format!("{detail}; coverage log: outcome '{outcome}', reason '{reason}'"))
            }
        }
    }

    /// Links `objects` with `linker` into `<dir>/<side>` and runs the
    /// program.
    fn link_and_run(
        &self,
        linker: &Linker,
        objects: &[PathBuf],
        dir: &Path,
        side: &str,
        expect: &Expectations,
    ) -> Result<Output, Trouble> {
        let driver = self.toolchain.driver.as_deref().expect("cannot_run() was checked");
        let program = dir.join(side);
        let mut args = vec![tools::prefixed("-B", &self.linker_dir(linker))];
        args.push(if expect.pie { "-pie" } else { "-no-pie" }.into());
        if !expect.allow_warnings {
            args.push("-Wl,--fatal-warnings".into());
        }
        args.extend(tools::linker_flags(&linker.name).iter().map(OsString::from));
        args.push("-o".into());
        args.push(program.clone().into());
        args.extend(objects.iter().map(OsString::from));
        for lib in [Lib::Cxx, Lib::M] {
            if expect.libs.contains(&lib) {
                args.push(if lib == Lib::Cxx { "-lstdc++" } else { "-lm" }.into());
            }
        }
        let configure = |command: &mut std::process::Command| {
            command.args(&args).env("LC_ALL", "C");
        };
        let output =
            process::run(driver, configure, b"", self.tool_limits).map_err(Trouble::Broken)?;
        let failure = Failure::of_tool(Failure::Link, output.end);
        match output.end {
            End::Exit(0) if program.is_file() => {}
            End::Exit(0) => {
                return Err(Trouble::Failed(failure, "the link wrote no program".to_owned()));
            }
            end => {
                let name = format!("link with {}", linker.name);
                let detail = tool_failure(&name, end, &output, self.tool_limits);
                return Err(Trouble::Failed(failure, detail));
            }
        }
        let bytes = fs::read(&program)
            .map_err(|e| Trouble::Broken(format!("cannot read '{}': {e}", program.display())))?;
        tools::check_signature(&linker.name, &bytes).map_err(Trouble::Broken)?;

        // A cross target's program runs under its emulator ("Cross runs"),
        // with more time.
        let (runner, limits) = match &self.toolchain.emulator {
            Some(emulator) => (&emulator.program, self.emulated_run_limits()),
            None => (&program, self.run_limits),
        };
        let configure = |command: &mut std::process::Command| {
            if let Some(emulator) = &self.toolchain.emulator {
                command.args(emulator.args_with(&expect.cpu)).arg(&program);
            }
            command.args(&expect.args).current_dir(dir);
        };
        process::run(runner, configure, &expect.stdin, limits).map_err(Trouble::Broken)
    }

    /// Compares a program's behaviour with the test's directives. The
    /// error completes "the program ...".
    fn check_directives(
        &self,
        output: &Output,
        expect: &Expectations,
    ) -> Result<(), (Failure, String)> {
        let ended = match (output.end, expect.exit) {
            (End::TimedOut, _) => {
                return Err((
                    Failure::Run,
                    format!("timed out after {}", seconds(self.program_limits())),
                ));
            }
            (End::Exit(code), Exit::Code(wanted)) => code == i32::from(wanted),
            (End::Signal(signal), Exit::Signal(wanted)) => signal == i32::from(wanted),
            _ => false,
        };
        if !ended {
            let wanted = match expect.exit {
                Exit::Code(code) => format!("exit status {code}"),
                Exit::Signal(signal) => format!("signal {signal}"),
            };
            return Err((
                Failure::Comparison,
                format!("ended with {}, the directives say {wanted}", ended_with(output.end)),
            ));
        }
        if output.truncated {
            return Err((Failure::Run, "wrote more output than the harness keeps".to_owned()));
        }
        match first_difference(&expect.stdout, &output.stdout) {
            Some(difference) => Err((
                Failure::Comparison,
                format!("wrote stdout that differs from the directives {difference}"),
            )),
            None => Ok(()),
        }
    }

    /// Compares the program of hod's object, or of the witness's, with
    /// the program of the oracle's, which is `whose` ("llc's"). The error
    /// completes "the program of hod's object ...".
    fn compare(&self, hod: &Output, oracle: &Output, whose: &str) -> Result<(), (Failure, String)> {
        if hod.end == End::TimedOut {
            return Err((
                Failure::Run,
                format!("timed out after {}", seconds(self.program_limits())),
            ));
        }
        if hod.end != oracle.end {
            return Err((
                Failure::Comparison,
                format!(
                    "ended with {}, {whose} with {}",
                    ended_with(hod.end),
                    ended_with(oracle.end)
                ),
            ));
        }
        if hod.truncated {
            return Err((Failure::Run, "wrote more output than the harness keeps".to_owned()));
        }
        match first_difference(&oracle.stdout, &hod.stdout) {
            Some(difference) => Err((
                Failure::Comparison,
                format!("wrote stdout that differs from {whose} {difference}"),
            )),
            None => Ok(()),
        }
    }
}

/// One test after both compilers ran.
struct Case<'a> {
    session: &'a Session,
    test: &'a Test,
    built_by: BuiltBy,
    reason: Option<String>,
    plan: Plan,
    /// Why the oracle is not used, if an `oracle-skip` directive applies:
    /// hod's program is then compared with the directives only.
    oracle_skip: Option<String>,
}

impl Case<'_> {
    fn finish(
        &self,
        linker: Option<&Linker>,
        outcome: (Outcome, Option<Failure>),
        detail: String,
    ) -> Record {
        Record {
            built_by: self.built_by,
            reason: self.reason.clone(),
            ..self.session.record(self.test, linker, outcome, detail)
        }
    }

    /// Applies the test's `xfail` directives to a PASS or a FAIL.
    fn expected_failure(
        &self,
        linker: Option<&Linker>,
        outcome: Outcome,
        detail: String,
    ) -> (Outcome, String) {
        let session = self.session;
        let name = linker.map(|linker| linker.name.as_str());
        let why = self.test.expect.xfail_reason(session.arch, session.toolchain.llvm_major, name);
        match (why, outcome) {
            (Some(why), Outcome::Fail) => (Outcome::Xfail, format!("{why} ({detail})")),
            (Some(why), Outcome::Pass) => {
                (Outcome::Xpass, format!("passes; remove the 'xfail' directive ({why})"))
            }
            _ => (outcome, detail),
        }
    }

    /// The record of a test whose outcome on hod's side does not depend
    /// on the linker, if this is one. `note` is added to its detail.
    fn settled(&self, linker: Option<&Linker>, note: &str) -> Option<Record> {
        let Plan::Settled { outcome, failure, detail, may_xfail } = &self.plan else { return None };
        let (outcome, detail) = if *may_xfail {
            self.expected_failure(linker, *outcome, detail.clone())
        } else {
            (*outcome, detail.clone())
        };
        let detail = match (detail.is_empty(), note.is_empty()) {
            (_, true) => detail,
            (true, false) => note.to_owned(),
            (false, false) => format!("{detail}; {note}"),
        };
        Some(self.finish(linker, (outcome, *failure), detail))
    }

    /// Links and runs both sides with one linker, and before hod's side
    /// the witness's, if its objects are given and the oracle's program is
    /// what the directives say (`asked` is then set). `Err` is the detail
    /// of an oracle the witness does not confirm.
    fn with_linker(
        &self,
        linker: &Linker,
        dir: &Path,
        oracle_objects: &[PathBuf],
        hod_objects: &[PathBuf],
        witness_objects: Option<&[PathBuf]>,
        asked: &mut bool,
    ) -> Result<Record, String> {
        let session = self.session;
        let expect = &self.test.expect;
        let error = |failure: Failure, detail: String| {
            Ok(self.finish(Some(linker), (Outcome::Error, Some(failure)), detail))
        };
        let linker_dir = dir.join(&linker.name);
        if let Err(e) = fs::create_dir_all(&linker_dir) {
            return error(
                Failure::Harness,
                format!("cannot create '{}': {e}", linker_dir.display()),
            );
        }
        if let Some(why) = &self.oracle_skip {
            return Ok(self.against_directives(linker, &linker_dir, hod_objects, why));
        }
        // The oracle's side validates the test itself: any trouble there
        // is an ERROR, whatever hod did.
        let oracle =
            match session.link_and_run(linker, oracle_objects, &linker_dir, "oracle", expect) {
                Ok(output) => output,
                Err(trouble) => {
                    let (failure, detail) = trouble.split();
                    return error(failure, format!("oracle: {detail}"));
                }
            };
        if let Err((failure, detail)) = session.check_directives(&oracle, expect) {
            return error(failure, format!("oracle: the program of llc's object {detail}"));
        }
        if let Some(objects) = witness_objects {
            *asked = true;
            match session.link_and_run(linker, objects, &linker_dir, "witness", expect) {
                Err(trouble) => {
                    return Err(format!("{WITNESS}{}", trouble.split().1));
                }
                Ok(witness) => {
                    if let Err((_, detail)) = session.compare(&witness, &oracle, "llc -O0's") {
                        return Err(format!("{WITNESS}the program of llc -O2's object {detail}"));
                    }
                }
            }
        }
        if let Some(record) = self.settled(Some(linker), "") {
            return Ok(record);
        }
        let (outcome, failure, detail) =
            match session.link_and_run(linker, hod_objects, &linker_dir, "hod", expect) {
                Err(Trouble::Broken(detail)) => return error(Failure::Harness, detail),
                Err(Trouble::Failed(failure, detail)) => {
                    (Outcome::Fail, Some(failure), format!("hod's object: {detail}"))
                }
                Ok(hod) => match session.compare(&hod, &oracle, "llc's") {
                    Ok(()) => (Outcome::Pass, None, String::new()),
                    Err((failure, detail)) => (
                        Outcome::Fail,
                        Some(failure),
                        format!("the program of hod's object {detail}"),
                    ),
                },
            };
        let (outcome, detail) = self.expected_failure(Some(linker), outcome, detail);
        let failure = failure.filter(|_| matches!(outcome, Outcome::Fail | Outcome::Xfail));
        Ok(self.finish(Some(linker), (outcome, failure), detail))
    }
}

impl Case<'_> {
    /// One linker's record of a test whose oracle is skipped (`why`): the
    /// program of hod's object against the directives, never against an
    /// oracle.
    fn against_directives(
        &self,
        linker: &Linker,
        dir: &Path,
        hod_objects: &[PathBuf],
        why: &str,
    ) -> Record {
        let session = self.session;
        let note = format!("oracle skipped: {why}");
        if let Some(record) = self.settled(Some(linker), &note) {
            return record;
        }
        let (outcome, failure, detail) =
            match session.link_and_run(linker, hod_objects, dir, "hod", &self.test.expect) {
                Err(Trouble::Broken(detail)) => {
                    let error = (Outcome::Error, Some(Failure::Harness));
                    return self.finish(Some(linker), error, detail);
                }
                Err(Trouble::Failed(failure, detail)) => {
                    (Outcome::Fail, Some(failure), format!("hod's object: {detail}; {note}"))
                }
                Ok(hod) => match session.check_directives(&hod, &self.test.expect) {
                    Ok(()) => (Outcome::Pass, None, note),
                    Err((failure, detail)) => (
                        Outcome::Fail,
                        Some(failure),
                        format!("the program of hod's object {detail}; {note}"),
                    ),
                },
            };
        let (outcome, detail) = self.expected_failure(Some(linker), outcome, detail);
        let failure = failure.filter(|_| matches!(outcome, Outcome::Fail | Outcome::Xfail));
        self.finish(Some(linker), (outcome, failure), detail)
    }
}

/// The `unsupported` reason that accepts whatever reason hod names.
pub const ANY_REASON: &str = "*";

/// Decides what a test needs after both compilers ran: `expected` is the
/// reason of the `unsupported` directive that applies, if one does.
fn plan(build: &HodBuild, built_by: BuiltBy, reason: Option<&str>, expected: Option<&str>) -> Plan {
    let fail = |detail: String, may_xfail: bool| Plan::Settled {
        outcome: Outcome::Fail,
        failure: Some(if may_xfail { Failure::HodBuild } else { Failure::Declined }),
        detail,
        may_xfail,
    };
    let reason = reason.unwrap_or("-");
    // `*`: any reason will do ("Test directives").
    let named = |expected: &str| expected == ANY_REASON || reason == expected;
    match (build, built_by, expected) {
        (HodBuild::Failed(detail), _, _) => fail(detail.clone(), true),
        (HodBuild::Declined, _, Some(expected)) if named(expected) => Plan::Settled {
            outcome: Outcome::Unsupported,
            failure: None,
            detail: String::new(),
            may_xfail: false,
        },
        (HodBuild::Declined, _, Some(expected)) => fail(
            format!(
                "hod declined with reason '{reason}'; the 'unsupported' directive says '{expected}'"
            ),
            false,
        ),
        (HodBuild::Declined, _, None) => fail(
            format!(
                "hod declined the module and the test has no \
                 'unsupported: {reason}' directive"
            ),
            false,
        ),
        (HodBuild::Built, BuiltBy::Hod, Some(expected)) => fail(
            format!("hod compiled this test; remove its 'unsupported: {expected}' directive"),
            false,
        ),
        (HodBuild::Built, BuiltBy::Fallback, Some(expected)) if !named(expected) => fail(
            format!(
                "hod fell back with reason '{reason}'; the 'unsupported' directive says '{expected}'"
            ),
            false,
        ),
        (HodBuild::Built, _, _) => Plan::Compare,
    }
}

/// "<tool> failed (exit status 1): <first line of its diagnostics>".
fn tool_failure(tool: &str, end: End, output: &Output, limits: Limits) -> String {
    let said = output.first_error_line();
    let said = if said.is_empty() { String::new() } else { format!(": {}", clip(&said, 200)) };
    match end {
        End::Exit(0) => format!("{tool} exited with status 0{said}"),
        End::Exit(code) => format!("{tool} failed (exit status {code}){said}"),
        End::Signal(signal) => format!("{tool} was terminated by signal {signal}{said}"),
        End::TimedOut => format!("{tool} timed out after {}", seconds(limits)),
    }
}

/// The outcome and reason of hod's coverage log, which must hold exactly
/// one line of tab-separated fields (`spec/cli.md` "Coverage log"). Fields
/// are only ever appended to that format, and a reader takes the ones it
/// knows: this one was written for the first eight, so it accepts a line
/// of eight or more (hod writes ten today; the stand-ins for hod that the
/// harness and its tests use write eight).
fn read_coverage(log: &Path) -> Result<(String, String), String> {
    const FIELDS: usize = 8;
    let text = match fs::read_to_string(log) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err("it wrote no coverage log".to_owned());
        }
        Err(e) => return Err(format!("its coverage log cannot be read: {e}")),
    };
    let lines: Vec<&str> = text.lines().collect();
    let [line] = lines[..] else {
        return Err(format!("its coverage log has {} lines, not one", lines.len()));
    };
    let fields: Vec<&str> = line.split('\t').collect();
    if fields.len() < FIELDS || !text.ends_with('\n') {
        return Err("its coverage log line is malformed".to_owned());
    }
    Ok((fields[0].to_owned(), fields[1].to_owned()))
}

fn seconds(limits: Limits) -> String {
    format!("{} s", limits.time.as_secs_f64())
}

fn ended_with(end: End) -> String {
    match end {
        End::Exit(code) => format!("exit status {code}"),
        End::Signal(signal) => format!("signal {signal}"),
        End::TimedOut => "a timeout".to_owned(),
    }
}

/// `text`, cut to at most `max` bytes at a character boundary.
fn clip(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &text[..end])
}

/// Where two outputs first differ, by line: `at line N: expected "..",
/// got ".."`. `None` when they are equal.
fn first_difference(expected: &[u8], got: &[u8]) -> Option<String> {
    if expected == got {
        return None;
    }
    let show = |line: Option<&[u8]>| match line {
        None => "the end of the output".to_owned(),
        Some(bytes) => format!("\"{}\"", clip(&bytes.escape_ascii().to_string(), 80)),
    };
    let mut expected = expected.split_inclusive(|b| *b == b'\n');
    let mut got = got.split_inclusive(|b| *b == b'\n');
    let mut number = 1;
    loop {
        match (expected.next(), got.next()) {
            (Some(a), Some(b)) if a == b => number += 1,
            (a, b) => {
                return Some(format!("at line {number}: expected {}, got {}", show(a), show(b)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    // spec/harness.md "Test directives": `unsupported: *` takes any
    // reason, declined or fallen back; a test hod compiles still fails.
    #[test]
    fn any_reason_plans() {
        use super::{ANY_REASON, BuiltBy, HodBuild, Outcome, Plan, plan};
        let unsupported =
            |p: &Plan| matches!(p, Plan::Settled { outcome: Outcome::Unsupported, .. });
        let outcome = |p: Plan| match p {
            Plan::Settled { outcome, detail, .. } => (Some(outcome), detail),
            Plan::Compare => (None, String::new()),
        };
        for reason in ["module:asm", "cc:ghccc", "malformed-input"] {
            let declined = plan(&HodBuild::Declined, BuiltBy::Hod, Some(reason), Some(ANY_REASON));
            assert!(unsupported(&declined), "{declined:?}");
            let fell_back =
                plan(&HodBuild::Built, BuiltBy::Fallback, Some(reason), Some(ANY_REASON));
            assert_eq!(fell_back, Plan::Compare);
        }
        let built = plan(&HodBuild::Built, BuiltBy::Hod, None, Some(ANY_REASON));
        let (got, detail) = outcome(built);
        assert_eq!(got, Some(Outcome::Fail));
        assert!(detail.contains("remove its 'unsupported: *' directive"), "{detail}");
        // A named reason is still checked exactly.
        let named = plan(&HodBuild::Declined, BuiltBy::Hod, Some("module:asm"), Some("cc:ghccc"));
        assert_eq!(outcome(named).0, Some(Outcome::Fail));
        let fell = plan(&HodBuild::Built, BuiltBy::Fallback, Some("module:asm"), Some("cc:ghccc"));
        assert_eq!(outcome(fell).0, Some(Outcome::Fail));
        // A failure is a failure whatever the directive.
        let failed = plan(&HodBuild::Failed("x".into()), BuiltBy::Hod, None, Some(ANY_REASON));
        assert_eq!(outcome(failed).0, Some(Outcome::Fail));
    }

    use super::*;

    // spec/harness.md "irdiff", "Outcomes": how a difference is described.
    #[test]
    fn differences_are_located() {
        assert_eq!(first_difference(b"a\nb\n", b"a\nb\n"), None);
        assert_eq!(first_difference(b"", b""), None);
        assert_eq!(
            first_difference(b"a\nb\nc\n", b"a\nB\nc\n").unwrap(),
            "at line 2: expected \"b\\n\", got \"B\\n\""
        );
        assert_eq!(
            first_difference(b"a\n", b"a\nmore\n").unwrap(),
            "at line 2: expected the end of the output, got \"more\\n\""
        );
        assert_eq!(
            first_difference(b"a\nb\n", b"a\n").unwrap(),
            "at line 2: expected \"b\\n\", got the end of the output"
        );
        assert_eq!(
            first_difference(b"a\n", b"a").unwrap(),
            "at line 1: expected \"a\\n\", got \"a\""
        );
        let long = first_difference(b"x\n", &[0xff; 200]).unwrap();
        assert!(long.contains("got \"\\xff\\xff") && long.ends_with("...\""), "{long}");
        assert!(long.len() < 200, "{long}");
        assert_eq!(clip("häh", 2), "h...");
    }

    // "What a test does", step 3: the coverage log is the source of what
    // hod did; anything but one well-formed line is hod's failure.
    #[test]
    fn coverage_log_reading() {
        let scratch = hod_testutil::Scratch::new("irdiff-coverage");
        let log = scratch.join("coverage.tsv");
        assert_eq!(read_coverage(&log).unwrap_err(), "it wrote no coverage log");
        fs::write(&log, "error\tcodegen-not-implemented\ttext\t10\ta.o\t-\t5\t-\t-\t-\n").unwrap();
        assert_eq!(
            read_coverage(&log).unwrap(),
            ("error".to_owned(), "codegen-not-implemented".to_owned())
        );
        // A converted bitcode module that fell back: fields 9 and 10 are set.
        fs::write(&log, "fallback\tinline-asm\tbitcode\t10\ta.o\t-\t5\t90\t40\tsignal:9\n")
            .unwrap();
        assert_eq!(read_coverage(&log).unwrap(), ("fallback".to_owned(), "inline-asm".to_owned()));
        // Fields are only ever appended: a later hod's longer line is read,
        // and so is the line of the eight fields this reader knows.
        fs::write(&log, "hod\t-\ttext\t1\ta.o\t-\t5\t-\t-\t-\tlater\n").unwrap();
        assert_eq!(read_coverage(&log).unwrap(), ("hod".to_owned(), "-".to_owned()));
        fs::write(&log, "error\tinline-asm\ttext\t1\ta.o\t-\t5\t-\n").unwrap();
        assert_eq!(read_coverage(&log).unwrap(), ("error".to_owned(), "inline-asm".to_owned()));
        // Fewer than it knows is not the format.
        fs::write(&log, "hod\t-\ttext\t1\ta.o\t-\t5\n").unwrap();
        assert_eq!(read_coverage(&log).unwrap_err(), "its coverage log line is malformed");
        fs::write(&log, "").unwrap();
        assert_eq!(read_coverage(&log).unwrap_err(), "its coverage log has 0 lines, not one");
        fs::write(&log, "hod\t-\n").unwrap();
        assert_eq!(read_coverage(&log).unwrap_err(), "its coverage log line is malformed");
        fs::write(&log, "hod\t-\ttext\t1\ta.o\t-\t5\t-\t-\t-").unwrap();
        assert_eq!(read_coverage(&log).unwrap_err(), "its coverage log line is malformed");
        let two = "hod\t-\ttext\t1\ta.o\t-\t5\t-\t-\t-\n".repeat(2);
        fs::write(&log, two).unwrap();
        assert_eq!(read_coverage(&log).unwrap_err(), "its coverage log has 2 lines, not one");
    }
}
