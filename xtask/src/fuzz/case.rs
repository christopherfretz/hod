//! One seed: generate a program, build it with the oracle and with hod,
//! link and run both, and decide. Contract: `spec/harness.md` "Random
//! programs", "What a seed does" and "Verdicts"; the modes' steps are in
//! [`super::split`] (`--mix`) and [`Bench::debug_steps`] (`--debug`).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::profile::{Generator, Profile};
use crate::debug::lines::{self, LineTools, LinesReport};
use crate::debug::permitted::IrFacts;
use crate::irdiff::process::{self, End, Limits, Output};
use crate::irdiff::tools::{self, Emulator};
use crate::irdiff::{Arch, engine::REFERENCE_SELECTOR};

/// What a run does with each program ("Modes").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// The whole module built by hod against the oracle (WP-154).
    Whole,
    /// The module split in two halves, each built by hod with the other
    /// built by `llc` (`--mix`).
    Mix,
    /// The module with `-g`: verified, run, and its line table compared
    /// with `llc`'s (`--debug`).
    Debug,
    /// The IR's features counted; nothing is built (`--census`).
    Census,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Self::Whole => "whole",
            Self::Mix => "mix",
            Self::Debug => "debug",
            Self::Census => "census",
        }
    }
}

/// The tools of a run.
#[derive(Clone, Debug)]
pub struct Tools {
    /// csmith and the directory of `csmith.h`; `None` for a profile that
    /// does not use it.
    pub csmith: Option<PathBuf>,
    pub csmith_include: Option<PathBuf>,
    /// `clang-<N>`.
    pub clang: PathBuf,
    pub llc: PathBuf,
    pub llvm_major: u32,
    pub hod: PathBuf,
    /// The linker driver of the target (`gcc`, `x86_64-linux-gnu-gcc`).
    pub driver: PathBuf,
    /// A directory whose `ld` is the linker asked for, given to the
    /// driver with `-B`; `None` is the driver's own `ld`.
    pub linker_dir: Option<PathBuf>,
    pub linker_name: String,
    /// The emulator of a cross target's programs.
    pub emulator: Option<Emulator>,
    /// `llvm-extract` of the run's LLVM (`--mix`).
    pub extract: Option<PathBuf>,
    /// `llvm-dwarfdump` of the run's LLVM, and the tools of the line
    /// comparison (`--debug`).
    pub dwarfdump: Option<PathBuf>,
    pub lines: Option<LineTools>,
}

/// Everything that is the same for every seed of a run.
#[derive(Clone, Debug)]
pub struct Bench {
    pub tools: Tools,
    pub arch: Arch,
    /// For csmith, clang, `llc`, hod and the linker driver.
    pub tool_limits: Limits,
    /// For the programs: the same on both targets, emulated or not (a
    /// fuzzed program is useful when it is quick under the emulator).
    pub run_limits: Limits,
    pub mode: Mode,
    pub profile: &'static Profile,
}

/// The kinds of csmith program a seed of the default profile can give
/// ("Generator"). The seed picks one: `seed % FLAVOURS.len()`.
pub const FLAVOURS: [Flavour; 3] = [
    Flavour { name: "c", extra: &[] },
    Flavour { name: "c", extra: &[] },
    Flavour { name: "c-float", extra: &["--float"] },
];

/// csmith's options for every flavour: sizes that keep most programs
/// within a second or two at `-O0` under the emulator ("Generator").
pub const CSMITH_BASE: [&str; 6] =
    ["--max-funcs", "6", "--max-block-depth", "4", "--max-array-len-per-dim", "6"];

/// One kind of generated program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Flavour {
    pub name: &'static str,
    /// csmith options on top of [`CSMITH_BASE`].
    pub extra: &'static [&'static str],
}

/// The flavour of `seed` in the default profile.
pub fn flavour(seed: u64) -> Flavour {
    let at = usize::try_from(seed % FLAVOURS.len() as u64).unwrap_or(0);
    FLAVOURS[at]
}

/// What a seed came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Both programs ended the same way and wrote the same output.
    Agree,
    /// hod declined the module with this coverage-log reason: coverage,
    /// not a bug.
    Refused(String),
    /// The seed says nothing about hod: the generator or clang failed, or
    /// the programs did not finish in time. The short name of why.
    Skipped(&'static str),
    /// The oracle's side failed (`llc`, the oracle's link): says nothing
    /// about hod; the detail.
    Error(String),
    /// hod is suspected.
    Finding(Finding),
}

impl Verdict {
    /// The name in the results file.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Agree => "agree",
            Self::Refused(_) => "refused",
            Self::Skipped(_) => "skipped",
            Self::Error(_) => "error",
            Self::Finding(_) => "FINDING",
        }
    }

    /// The reason, the skip's name, or the detail.
    pub fn detail(&self) -> String {
        match self {
            Self::Agree => String::new(),
            Self::Refused(reason) => reason.clone(),
            Self::Skipped(why) => (*why).to_owned(),
            Self::Error(detail) => detail.clone(),
            Self::Finding(finding) => format!("{}: {}", finding.kind.name(), finding.detail),
        }
    }
}

/// What makes hod suspect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The programs ended differently or wrote different output.
    Disagree,
    /// hod failed on the module: a crash, a panic, a status other than 0
    /// and 2, or no object or coverage log.
    HodFailed,
    /// hod's object does not link.
    HodLink,
    /// hod's program did not finish in four times the limit while the
    /// oracle's finished within it.
    HodHang,
    /// `--debug`: `llvm-dwarfdump --verify` rejects hod's object and
    /// accepts `llc`'s.
    DebugVerify,
    /// `--debug`: the `(function, line)` comparison with `llc`'s program
    /// has a difference no class of `spec/dwarf.md` §5.7 permits.
    DebugLines,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Disagree => "disagree",
            Self::HodFailed => "hod-failed",
            Self::HodLink => "hod-link",
            Self::HodHang => "hod-hang",
            Self::DebugVerify => "debug-verify",
            Self::DebugLines => "debug-lines",
        }
    }
}

/// Which object of the program hod built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// The whole module.
    Whole,
    /// `--mix`: half A (with `llc`'s half B), or half B (with `llc`'s A).
    HodA,
    HodB,
}

impl Side {
    /// "hod A + llc B" and so on, for details.
    pub fn name(self) -> &'static str {
        match self {
            Self::Whole => "hod",
            Self::HodA => "hod A + llc B",
            Self::HodB => "llc A + hod B",
        }
    }
}

/// A finding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub kind: Kind,
    pub detail: String,
    pub side: Side,
}

/// A seed's result.
#[derive(Clone, Debug)]
pub struct Outcome {
    pub seed: u64,
    pub flavour: Flavour,
    pub verdict: Verdict,
    /// How long the oracle's program ran, when it ran to its end.
    pub oracle_time: Option<Duration>,
    /// The programs' output, for a finding.
    pub oracle: Option<Output>,
    pub hod: Option<Output>,
    /// The commands that were run, as shell lines, in order.
    pub commands: Vec<String>,
    /// `--census`: the module's features ([`super::profile::scan`]).
    pub census: Option<BTreeMap<String, usize>>,
    /// `--debug`: the line comparison, when it ran.
    pub lines: Option<LinesReport>,
    /// `--debug`: hod's pairs at a self-assignment ([`self_assignment`]),
    /// taken out of the comparison's report and counted apart.
    pub self_assigned: Vec<lines::Pair>,
    /// `--debug`: llc's pairs at a constant branch ([`constant_branch`]),
    /// taken out of the report and counted apart.
    pub constant_branches: Vec<lines::Pair>,
}

impl Outcome {
    /// An outcome with nothing yet.
    pub fn new(seed: u64, flavour: Flavour) -> Self {
        Self {
            seed,
            flavour,
            verdict: Verdict::Agree,
            oracle_time: None,
            oracle: None,
            hod: None,
            commands: Vec::new(),
            census: None,
            lines: None,
            self_assigned: Vec::new(),
            constant_branches: Vec::new(),
        }
    }
}

/// The files of a seed, all in one directory.
#[derive(Debug)]
pub struct Files {
    pub dir: PathBuf,
}

impl Files {
    pub fn source(&self) -> PathBuf {
        self.dir.join("prog.c")
    }
    pub fn ir(&self) -> PathBuf {
        self.dir.join("prog.ll")
    }
    pub fn at(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }
}

/// A command line for the record: each word quoted for `sh` when it needs
/// to be.
pub fn shell_line(program: &Path, args: &[OsString], cwd: Option<&Path>) -> String {
    let quote = |word: &str| {
        if !word.is_empty()
            && word.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_./=:,+%@".contains(&b))
        {
            word.to_owned()
        } else {
            format!("'{}'", word.replace('\'', "'\\''"))
        }
    };
    let mut line = String::new();
    if let Some(cwd) = cwd {
        let _ = write!(line, "cd {} && ", quote(&cwd.to_string_lossy()));
    }
    line.push_str(&quote(&program.to_string_lossy()));
    for arg in args {
        line.push(' ');
        line.push_str(&quote(&arg.to_string_lossy()));
    }
    line
}

impl Bench {
    /// Runs `program` with `args` under `limits`, recording the command.
    pub fn step(
        &self,
        log: &mut Vec<String>,
        program: &Path,
        args: &[OsString],
        cwd: Option<&Path>,
        limits: Limits,
    ) -> Result<Output, String> {
        timed_step(log, program, args, cwd, limits).map(|(output, _)| output)
    }

    /// csmith's arguments for `seed`, writing `out`.
    pub fn csmith_args(seed: u64, flavour: Flavour, out: &Path) -> Vec<OsString> {
        let mut args: Vec<OsString> = vec!["--seed".into(), seed.to_string().into()];
        args.extend(CSMITH_BASE.iter().map(OsString::from));
        args.extend(flavour.extra.iter().map(OsString::from));
        args.push("-o".into());
        args.push(out.into());
        args
    }

    /// The generator's step: csmith's program, or the ABI generator's,
    /// written to `files.source()`. `Err` is the verdict.
    pub fn generate(
        &self,
        seed: u64,
        flavour: Flavour,
        files: &Files,
        log: &mut Vec<String>,
    ) -> Result<(), Verdict> {
        let source = files.source();
        let harness = |e: String| Verdict::Error(format!("harness: {e}"));
        let Generator::Csmith { base, probabilities, .. } = self.profile.generator else {
            log.push(format!(
                "# cargo xtask fuzz --profile abi --seed {seed} wrote {}",
                source.display()
            ));
            return fs::write(&source, super::abi::program(seed))
                .map_err(|e| harness(format!("cannot write '{}': {e}", source.display())));
        };
        let Some(csmith) = &self.tools.csmith else {
            return Err(harness("csmith not found".to_owned()));
        };
        let mut args: Vec<OsString> = vec!["--seed".into(), seed.to_string().into()];
        args.extend(base.iter().map(OsString::from));
        args.extend(flavour.extra.iter().map(OsString::from));
        if !probabilities.is_empty() {
            let file = files.at("probabilities.txt");
            fs::write(&file, probabilities)
                .map_err(|e| harness(format!("cannot write '{}': {e}", file.display())))?;
            args.push("--probability-configuration".into());
            args.push(file.into());
        }
        args.push("-o".into());
        args.push(source.clone().into());
        let generated =
            self.step(log, csmith, &args, Some(&files.dir), self.tool_limits).map_err(harness)?;
        if generated.end != End::Exit(0) || !source.is_file() {
            return Err(Verdict::Skipped("csmith-failed"));
        }
        Ok(())
    }

    /// clang's arguments: the source to LLVM text at `-O0` for the target,
    /// position-independent as a compiler driver builds by default.
    ///
    /// csmith writes C that newer compilers reject by default (a `float *`
    /// passed for a `char *` to its checksum helper): those errors are
    /// turned back into the warnings they were, which `-w` silences.
    pub fn clang_args(&self, source: &Path, out: &Path) -> Vec<OsString> {
        let mut args: Vec<OsString> =
            ["-O0", "-S", "-emit-llvm", "-w"].into_iter().map(OsString::from).collect();
        args.extend(
            ["-Wno-error=incompatible-pointer-types", "-Wno-error=int-conversion"]
                .into_iter()
                .map(OsString::from),
        );
        if self.mode == Mode::Debug {
            args.push("-g".into());
        }
        args.push(format!("--target={}", self.arch.triple()).into());
        if let Some(include) = &self.tools.csmith_include {
            args.push(tools::prefixed("-I", include));
        }
        args.push("-o".into());
        args.push(out.into());
        args.push(source.into());
        args
    }

    /// The code-generation arguments of `llc` or hod after the level and
    /// the selector.
    fn codegen_tail(&self, ir: &Path, object: &Path) -> Vec<OsString> {
        vec![
            "-filetype=obj".into(),
            format!("-mtriple={}", self.arch.triple()).into(),
            "-relocation-model=pic".into(),
            "-o".into(),
            object.into(),
            ir.into(),
        ]
    }

    /// The oracle: `llc -O0` with the reference selector
    /// (`docs/VALIDATION.md` "Oracles").
    pub fn reference_args(&self, ir: &Path, object: &Path) -> Vec<OsString> {
        let mut args: Vec<OsString> = vec!["-O0".into()];
        args.extend(REFERENCE_SELECTOR.iter().map(OsString::from));
        args.extend(self.codegen_tail(ir, object));
        args
    }

    /// The witness: `llc -O2`.
    pub fn witness_args(&self, ir: &Path, object: &Path) -> Vec<OsString> {
        let mut args: Vec<OsString> = vec!["-O2".into()];
        args.extend(self.codegen_tail(ir, object));
        args
    }

    /// hod: `--strict`, with a coverage log.
    pub fn hod_args(&self, ir: &Path, object: &Path, log: &Path) -> Vec<OsString> {
        let mut args = vec![tools::prefixed("--coverage-log=", log), "--strict".into()];
        args.push("-O0".into());
        args.extend(self.codegen_tail(ir, object));
        args
    }

    /// Links `object` into `program`. `Err` is the link's failure.
    pub fn link(&self, log: &mut Vec<String>, object: &Path, program: &Path) -> Result<(), String> {
        self.link_all(log, &[object], program)
    }

    /// Links `objects`, in order, into `program`.
    pub fn link_all(
        &self,
        log: &mut Vec<String>,
        objects: &[&Path],
        program: &Path,
    ) -> Result<(), String> {
        let mut args = Vec::new();
        if let Some(dir) = &self.tools.linker_dir {
            args.push(tools::prefixed("-B", dir));
        }
        args.push("-pie".into());
        args.extend(tools::linker_flags(&self.tools.linker_name).iter().map(OsString::from));
        args.push("-o".into());
        args.push(program.into());
        args.extend(objects.iter().map(OsString::from));
        let output = self.step(log, &self.tools.driver, &args, None, self.tool_limits)?;
        match output.end {
            End::Exit(0) if program.is_file() => Ok(()),
            end => Err(format!("{}: {}", ended(end), output.first_error_line())),
        }
    }

    /// Runs a linked program, under the emulator for a cross target.
    pub fn execute(
        &self,
        log: &mut Vec<String>,
        program: &Path,
        limits: Limits,
    ) -> Result<(Output, Duration), String> {
        let dir = program.parent().unwrap_or(Path::new("."));
        match &self.tools.emulator {
            Some(emulator) => {
                let mut args = emulator.args.clone();
                args.push(program.into());
                timed_step(log, &emulator.program, &args, Some(dir), limits)
            }
            None => timed_step(log, program, &[], Some(dir), limits),
        }
    }

    /// Builds `ir` with hod into `object`: `Ok(None)` built, `Ok(Some)`
    /// declined with that reason, `Err` hod failed (the detail).
    pub fn build_with_hod(
        &self,
        log: &mut Vec<String>,
        ir: &Path,
        object: &Path,
        coverage: &Path,
    ) -> Result<Result<(), String>, String> {
        let _ = fs::remove_file(coverage);
        let args = self.hod_args(ir, object, coverage);
        let output = self.step(log, &self.tools.hod, &args, None, self.tool_limits)?;
        let line = fs::read_to_string(coverage).unwrap_or_default();
        let mut fields = line.lines().next().unwrap_or("").split('\t');
        let (outcome, reason) = (fields.next().unwrap_or(""), fields.next().unwrap_or(""));
        Ok(match (output.end, outcome) {
            (End::Exit(0), "hod") if object.is_file() => Ok(()),
            (End::Exit(2), "error") if !reason.is_empty() => Err(reason.to_owned()),
            (end, _) => {
                return Err(format!(
                    "hod {}: {}; coverage log '{}'",
                    ended(end),
                    first_line_of_interest(&output),
                    line.trim_end()
                ));
            }
        })
    }

    /// Builds `ir` with `llc`: the reference (`witness` false) or `-O2`.
    pub fn build_with_llc(
        &self,
        log: &mut Vec<String>,
        ir: &Path,
        object: &Path,
        witness: bool,
    ) -> Result<(), String> {
        let args =
            if witness { self.witness_args(ir, object) } else { self.reference_args(ir, object) };
        let output = self.step(log, &self.tools.llc, &args, None, self.tool_limits)?;
        match output.end {
            End::Exit(0) if object.is_file() => Ok(()),
            end => Err(format!("llc {}: {}", ended(end), output.first_error_line())),
        }
    }

    /// Runs one seed in `files.dir` (which exists).
    pub fn run_seed(&self, seed: u64, files: &Files) -> Outcome {
        let flavour = self.profile.flavour(seed);
        let mut outcome = Outcome::new(seed, flavour);
        outcome.verdict = match self.seed_steps(seed, flavour, files, &mut outcome) {
            Ok(verdict) | Err(verdict) => verdict,
        };
        outcome
    }

    /// The steps of [`Bench::run_seed`]; `Err` is a verdict reached early.
    fn seed_steps(
        &self,
        seed: u64,
        flavour: Flavour,
        files: &Files,
        outcome: &mut Outcome,
    ) -> Result<Verdict, Verdict> {
        let harness = |e: String| Verdict::Error(format!("harness: {e}"));
        self.generate(seed, flavour, files, &mut outcome.commands)?;
        let (source, ir) = (files.source(), files.ir());
        let compiled = self
            .step(
                &mut outcome.commands,
                &self.tools.clang,
                &self.clang_args(&source, &ir),
                None,
                self.tool_limits,
            )
            .map_err(harness)?;
        if compiled.end != End::Exit(0) || !ir.is_file() {
            return Err(Verdict::Skipped("clang-failed"));
        }
        match self.mode {
            Mode::Census => {
                let text = fs::read_to_string(&ir).map_err(|e| harness(e.to_string()))?;
                outcome.census = Some(super::profile::scan(&text));
                return Ok(Verdict::Agree);
            }
            Mode::Mix => return self.mix_steps(seed, files, outcome),
            Mode::Whole | Mode::Debug => {}
        }
        let log = &mut outcome.commands;
        let (oracle_o, hod_o) = (files.at("oracle.o"), files.at("hod.o"));
        self.build_with_llc(log, &ir, &oracle_o, false).map_err(Verdict::Error)?;
        match self.build_with_hod(log, &ir, &hod_o, &files.at("coverage.tsv")) {
            Ok(Ok(())) => {}
            Ok(Err(reason)) => return Err(Verdict::Refused(reason)),
            Err(detail) => return Err(finding(Kind::HodFailed, detail)),
        }
        if self.mode == Mode::Debug {
            self.verify(log, &hod_o, &oracle_o)?;
        }
        let (oracle_x, hod_x) = (files.at("oracle"), files.at("hod"));
        self.link(log, &oracle_o, &oracle_x)
            .map_err(|e| Verdict::Error(format!("the oracle's link: {e}")))?;
        if let Err(e) = self.link(log, &hod_o, &hod_x) {
            return Err(finding(Kind::HodLink, format!("hod's object does not link: {e}")));
        }
        let (oracle, oracle_time) =
            self.execute(log, &oracle_x, self.run_limits).map_err(harness)?;
        if oracle.end != End::TimedOut {
            outcome.oracle_time = Some(oracle_time);
        }
        let (verdict, hod) = self.judge(log, &oracle, oracle_time, &hod_x, "hod's program")?;
        if matches!(verdict, Verdict::Finding(_)) {
            outcome.oracle = Some(oracle);
            outcome.hod = Some(hod);
            return Ok(verdict);
        }
        if self.mode == Mode::Debug && verdict == Verdict::Agree {
            let report = self.compare_lines(&ir, &hod_x, &oracle_x).map_err(harness)?;
            // As `cargo xtask debug`: an unclassified pair fails, the
            // columns are counted. hod's pairs at a statement `x = x`,
            // which llc deletes, and llc's at a line that is only a
            // constant branch, which hod folds, are counted apart
            // ([`self_assignment`], [`constant_branch`]).
            let text = fs::read_to_string(&ir).unwrap_or_default();
            let mut report = report;
            report.hod_only.retain(|(pair, class)| {
                let apart =
                    *class == lines::Class::Unclassified && self_assignment(&text, &pair.0, pair.2);
                if apart {
                    outcome.self_assigned.push(pair.clone());
                }
                !apart
            });
            report.llc_only.retain(|(pair, class)| {
                let apart =
                    *class == lines::Class::Unclassified && constant_branch(&text, &pair.0, pair.2);
                if apart {
                    outcome.constant_branches.push(pair.clone());
                }
                !apart
            });
            let unclassified = report.unclassified();
            let first = first_unclassified(&report);
            outcome.lines = Some(report);
            if unclassified > 0 {
                return Ok(finding(
                    Kind::DebugLines,
                    format!("{unclassified} unclassified (function, line) pairs; {first}"),
                ));
            }
        }
        Ok(verdict)
    }

    /// Runs `program` and compares it with the oracle's, which ended as
    /// `oracle` after `oracle_time`. A program that times out where the
    /// oracle's did not gets four times the limit (hod's code may be
    /// slower than `llc`'s) before it is a hang. `what` names the program
    /// in a hang's detail.
    pub fn judge(
        &self,
        log: &mut Vec<String>,
        oracle: &Output,
        oracle_time: Duration,
        program: &Path,
        what: &str,
    ) -> Result<(Verdict, Output), Verdict> {
        let harness = |e: String| Verdict::Error(format!("harness: {e}"));
        if oracle.end == End::TimedOut {
            // Both, or the oracle's only: the program is not run longer.
            let (other, _) = self.execute(log, program, self.run_limits).map_err(harness)?;
            let why =
                if other.end == End::TimedOut { "both-timed-out" } else { "oracle-timed-out" };
            return Ok((Verdict::Skipped(why), other));
        }
        let (mut other, _) = self.execute(log, program, self.run_limits).map_err(harness)?;
        if other.end != End::TimedOut {
            return Ok((compare(oracle, &other), other));
        }
        let longer = Limits { time: self.run_limits.time * 4, ..self.run_limits };
        other = self.execute(log, program, longer).map_err(harness)?.0;
        if other.end == End::TimedOut {
            let detail = format!(
                "{what} did not end in {} s; the oracle's took {} ms",
                longer.time.as_secs(),
                oracle_time.as_millis()
            );
            return Ok((finding(Kind::HodHang, detail), other));
        }
        Ok((compare(oracle, &other), other))
    }

    /// `--debug`: `llvm-dwarfdump --verify` on hod's object; when it
    /// complains, on `llc`'s too (a verifier that rejects both is no
    /// finding of hod's).
    pub fn verify(
        &self,
        log: &mut Vec<String>,
        hod_o: &Path,
        oracle_o: &Path,
    ) -> Result<(), Verdict> {
        let Some(dwarfdump) = &self.tools.dwarfdump else { return Ok(()) };
        let harness = |e: String| Verdict::Error(format!("harness: {e}"));
        let check = |log: &mut Vec<String>, object: &Path| -> Result<Option<String>, Verdict> {
            let args: Vec<OsString> = vec!["--verify".into(), object.into()];
            let output =
                self.step(log, dwarfdump, &args, None, self.tool_limits).map_err(harness)?;
            if output.end == End::Exit(0) {
                return Ok(None);
            }
            let said = String::from_utf8_lossy(&output.stdout);
            let first = said.lines().find(|l| l.contains("error:")).unwrap_or("").trim().to_owned();
            Ok(Some(first))
        };
        let Some(complaint) = check(log, hod_o)? else { return Ok(()) };
        match check(log, oracle_o)? {
            None => Err(finding(
                Kind::DebugVerify,
                format!(
                    "llvm-dwarfdump --verify rejects hod's object (llc's verifies): {complaint}"
                ),
            )),
            Some(theirs) => Err(Verdict::Error(format!(
                "llvm-dwarfdump --verify rejects llc's object too: {theirs}"
            ))),
        }
    }

    /// `--debug`: the `(function, line)` comparison of `cargo xtask debug`
    /// ([`lines::compare`]) of the two programs built from `ir`.
    pub fn compare_lines(
        &self,
        ir: &Path,
        hod_x: &Path,
        oracle_x: &Path,
    ) -> Result<LinesReport, String> {
        let tools = self
            .tools
            .lines
            .as_ref()
            .ok_or("no line tools (llvm-dwarfdump, llvm-nm, llvm-objdump)")?;
        let text =
            fs::read_to_string(ir).map_err(|e| format!("cannot read '{}': {e}", ir.display()))?;
        let facts = IrFacts::of_modules([text.as_str()]);
        lines::compare(hod_x, oracle_x, tools, &facts)
    }
}

/// The integer operations whose right operand leaves the left one as it
/// is, with that operand; the commutative ones also with it on the left.
const IDENTITIES: [(&str, &str, bool); 11] = [
    ("add", "0", true),
    ("sub", "0", false),
    ("or", "0", true),
    ("xor", "0", true),
    ("shl", "0", false),
    ("lshr", "0", false),
    ("ashr", "0", false),
    ("and", "-1", true),
    ("mul", "1", true),
    ("sdiv", "1", false),
    ("udiv", "1", false),
];

/// The flags an instruction's opcode may carry before its type.
const FLAGS: [&str; 9] =
    ["nsw", "nuw", "exact", "disjoint", "inbounds", "nusw", "inrange", "samesign", "volatile"];

/// The casts [`value_number`] looks through.
const CASTS: [&str; 6] = ["sext", "zext", "trunc", "bitcast", "ptrtoint", "inttoptr"];

/// A typed operand's type and value: `i32 %5`, `ptr getelementptr (...)`,
/// `{ i32, i8 } %v`.
fn split_typed(item: &str) -> (&str, &str) {
    let item = item.trim();
    let end = match item.as_bytes().first() {
        Some(b'{' | b'<' | b'[') => {
            let mut depth = 0i32;
            item.char_indices()
                .find_map(|(i, c)| {
                    depth += i32::from(matches!(c, '{' | '<' | '['))
                        - i32::from(matches!(c, '}' | '>' | ']'));
                    (depth == 0).then_some(i + 1)
                })
                .unwrap_or(item.len())
        }
        _ => item.find(' ').unwrap_or(item.len()),
    };
    (&item[..end], item[end..].trim())
}

/// What the value `value` of a function is, as a text equal for two
/// values `llc`'s DAG would number the same ([`self_assignment`]): a
/// non-volatile load at the line (`local`) is its type and its address's
/// number; `getelementptr`s, casts, integer operations and comparisons
/// are their operands' numbers, an operation with its identity
/// ([`IDENTITIES`]) its other operand's, a truncation of an extension
/// back to its type the extended value's, a comparison of a value with
/// itself a constant; anything else (another line's load, a call, an
/// argument, a constant) is itself.
fn value_number(value: &str, defs: &BTreeMap<&str, &str>, local: &[&str], depth: usize) -> String {
    let opaque = || value.to_owned();
    let Some(def) = defs.get(value).filter(|_| depth < 24) else { return opaque() };
    let number = |v: &str| value_number(v, defs, local, depth + 1);
    let items: Vec<&str> =
        super::profile::top_level(def).into_iter().filter(|i| !i.trim().starts_with('!')).collect();
    let Some(first) = items.first() else { return opaque() };
    let mut words: Vec<&str> = first.split_whitespace().collect();
    let op = words.first().copied().unwrap_or("");
    words.retain(|w| !FLAGS.contains(w));
    match op {
        "load" if words.len() >= 2 && local.contains(&value) && !first.contains(" atomic ") => {
            let Some(address) = items.get(1) else { return opaque() };
            format!("*{}({})", words[1..].join(" "), number(split_typed(address).1))
        }
        "getelementptr" => {
            let operands: Vec<String> = items[1..]
                .iter()
                .map(|i| {
                    let (ty, v) = split_typed(i);
                    format!("{ty} {}", number(v))
                })
                .collect();
            format!("gep({}; {})", words[1..].join(" "), operands.join(", "))
        }
        cast if CASTS.contains(&cast) => {
            let rest = words[1..].join(" ");
            let Some((from, to)) = rest.rsplit_once(" to ") else { return opaque() };
            let (from_ty, inner) = split_typed(from);
            let (to, inner) = (to.trim(), number(inner));
            if cast == "trunc" {
                let suffix = format!(" to {from_ty})");
                for ext in ["sext(", "zext("] {
                    let prefix = format!("{ext}{to} ");
                    if inner.starts_with(&prefix) && inner.ends_with(&suffix) {
                        return inner[prefix.len()..inner.len() - suffix.len()].to_owned();
                    }
                }
            }
            if from_ty == to && cast == "bitcast" {
                return inner;
            }
            match (cast, inner.as_str()) {
                ("zext" | "sext", "false" | "0") => return "0".to_owned(),
                ("zext", "true") => return "1".to_owned(),
                ("sext", "true") => return "-1".to_owned(),
                _ => {}
            }
            format!("{cast}({from_ty} {inner} to {to})")
        }
        "icmp" if words.len() >= 4 => {
            let (pred, ty, left) = (words[1], words[2], words[3]);
            let Some(right) = items.get(1).map(|r| r.trim()) else { return opaque() };
            let (l, r) = (number(left), number(right));
            if l == r {
                let holds = matches!(pred, "eq" | "ule" | "uge" | "sle" | "sge");
                return if holds { "true".to_owned() } else { "false".to_owned() };
            }
            format!("icmp({pred} {ty} {l}, {r})")
        }
        op => {
            let Some(&(_, identity, commutes)) = IDENTITIES.iter().find(|(name, ..)| *name == op)
            else {
                return opaque();
            };
            let (Some(left), Some(right)) = (words.last(), items.get(1).map(|r| r.trim())) else {
                return opaque();
            };
            let (l, r) = (number(left), number(right));
            if r == identity {
                return l;
            }
            if commutes && l == identity {
                return r;
            }
            let ty = words.get(words.len().saturating_sub(2)).copied().unwrap_or("");
            format!("{op}({ty} {l}, {r})")
        }
    }
}

/// `function`'s named values, its instructions at `line` (any column) of
/// `text`, a module's text, and all its instructions.
type AtLine<'t> = (BTreeMap<&'t str, &'t str>, Vec<&'t str>, Vec<&'t str>);

fn at_line<'t>(text: &'t str, function: &str, line: u32) -> Option<AtLine<'t>> {
    let ids: std::collections::BTreeSet<&str> = text
        .lines()
        .filter(|l| l.contains(&format!("!DILocation(line: {line},")))
        .filter_map(|l| l.split_once(" = ").map(|(id, _)| id))
        .collect();
    let header = format!(" @{function}(");
    let body =
        text.split("\ndefine ").find(|f| f.lines().next().is_some_and(|h| h.contains(&header)))?;
    let body: Vec<&str> = body.lines().skip(1).take_while(|l| *l != "}").collect();
    let defs = body
        .iter()
        .filter_map(|l| l.trim_start().split_once(" = "))
        .filter(|(n, _)| n.starts_with('%'))
        .collect();
    let at = body
        .iter()
        .map(|l| l.trim_start())
        .filter(|l| {
            l.rsplit_once("!dbg ")
                .is_some_and(|(_, id)| ids.contains(id.split([',', ' ']).next().unwrap_or("")))
        })
        .collect();
    let body = body.iter().map(|l| l.trim_start()).collect();
    Some((defs, at, body))
}

/// Whether `function`'s instructions at `line` of `text` are a branch
/// whose destination is known, `br label %b` or `br i1 true|false, ...`,
/// and loads (non-volatile) and `getelementptr`s whose values nothing
/// uses: csmith's `if ((x, -6L))`. hod folds the branch (and omits a
/// branch to the block it emits next, `spec/codegen.md` §6.2) and drops
/// the unused loads, so its object has no row at the line; `llc -O0`
/// keeps a test and a jump. `spec/dwarf.md` §5.7 has no class for this
/// yet (`xtask/src/debug/permitted.rs` `pair_class`): until it has,
/// `--debug` counts llc's pairs there apart instead of calling them
/// findings.
pub fn constant_branch(text: &str, function: &str, line: u32) -> bool {
    let Some((_, at, body)) = at_line(text, function, line) else { return false };
    let used = |name: &str| {
        body.iter().filter_map(|l| l.split_once(" = ").map_or(Some(*l), |(_, r)| Some(r))).any(
            |l| {
                l.match_indices(name).any(|(i, _)| {
                    !l[i + name.len()..]
                        .starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_' || c == '.')
                })
            },
        )
    };
    let mut branches = 0;
    for inst in &at {
        if let Some((name, rest)) = inst.split_once(" = ").filter(|(n, _)| n.starts_with('%')) {
            let unused = !used(name);
            let pure = rest.starts_with("getelementptr")
                || rest.starts_with("load ") && !rest.starts_with("load volatile");
            if pure && unused {
                continue;
            }
            return false;
        }
        let known = inst.strip_prefix("br ").is_some_and(|b| {
            b.starts_with("label ") || b.starts_with("i1 true,") || b.starts_with("i1 false,")
        });
        if !known {
            return false;
        }
        branches += 1;
    }
    branches > 0
}

/// Whether `function`'s instructions at `line` of `text` store back what
/// they loaded: loads, `getelementptr`s, casts, integer operations and
/// comparisons, and stores, none volatile or atomic, no load after a
/// store, each store storing `poison` or `undef`, or a value numbered
/// ([`value_number`]) as a load of its type from the address it stores
/// to. csmith writes many: `x = x;`, `(*p) = (*p);`, `a[i] = a[i];`, `x ^=
/// 0;`, `x &= -1L;`, `x ^= (&y != &y);`, an out-of-range conversion.
/// `llc -O0` folds such a store away (its DAG combiner removes a store of
/// a load of the same address, and a store of `poison`), and with it the
/// line's only row; hod keeps it. `spec/dwarf.md` §5.7 has no class for
/// this yet (`xtask/src/debug/permitted.rs` `pair_class`): until it has,
/// `--debug` counts these pairs apart instead of calling them findings.
pub fn self_assignment(text: &str, function: &str, line: u32) -> bool {
    let Some((defs, at, _)) = at_line(text, function, line) else { return false };
    let local: Vec<&str> = at.iter().filter_map(|i| i.split_once(" = ").map(|(n, _)| n)).collect();
    let (mut store_count, mut stored) = (0, false);
    for inst in &at {
        if let Some((_, rest)) = inst.split_once(" = ").filter(|(n, _)| n.starts_with('%')) {
            let op = rest.split_whitespace().next().unwrap_or("");
            let pure = ["getelementptr", "icmp"].contains(&op)
                || CASTS.contains(&op)
                || IDENTITIES.iter().any(|(name, ..)| *name == op);
            let load = op == "load"
                && !stored
                && !rest.starts_with("load volatile")
                && !rest.starts_with("load atomic");
            if pure || load {
                continue;
            }
            return false;
        }
        let Some(store) = inst
            .strip_prefix("store ")
            .filter(|r| !r.starts_with("volatile") && !r.starts_with("atomic"))
        else {
            return false;
        };
        let items = super::profile::top_level(store);
        let (ty, value) = split_typed(items.first().unwrap_or(&""));
        let pointer = split_typed(items.get(1).unwrap_or(&"")).1;
        let same = matches!(value, "poison" | "undef")
            || value_number(value, &defs, &local, 0)
                == format!("*{ty}({})", value_number(pointer, &defs, &local, 0));
        if !same {
            return false;
        }
        (store_count, stored) = (store_count + 1, true);
    }
    store_count > 0
}

/// The first difference of a line comparison no class permits, for a
/// finding's detail.
pub fn first_unclassified(report: &LinesReport) -> String {
    let pair = |side: &str, list: &[(lines::Pair, lines::Class)]| {
        list.iter().find(|(_, c)| *c == lines::Class::Unclassified).map(|((f, file, line), _)| {
            format!("{side} only: {f} at {}:{line}", file.rsplit('/').next().unwrap_or(file))
        })
    };
    pair("hod", &report.hod_only).or_else(|| pair("llc", &report.llc_only)).unwrap_or_default()
}

/// [`Bench::step`], and how long the child ran once it held its
/// jobserver token.
fn timed_step(
    log: &mut Vec<String>,
    program: &Path,
    args: &[OsString],
    cwd: Option<&Path>,
    limits: Limits,
) -> Result<(Output, Duration), String> {
    log.push(shell_line(program, args, cwd));
    process::run_timed(
        program,
        |command| {
            command.args(args).env("LC_ALL", "C");
            if let Some(cwd) = cwd {
                command.current_dir(cwd);
            }
            // hod is configured by its options only.
            for variable in ["HOD_FALLBACK_LLC", "HOD_STRICT", "HOD_COVERAGE_LOG"] {
                command.env_remove(variable);
            }
        },
        b"",
        limits,
    )
}

fn finding(kind: Kind, detail: String) -> Verdict {
    Verdict::Finding(Finding { kind, detail, side: Side::Whole })
}

/// Agree, or a disagreement with where.
pub fn compare(oracle: &Output, hod: &Output) -> Verdict {
    if oracle.end != hod.end {
        return finding(
            Kind::Disagree,
            format!("hod's program {}, the oracle's {}", ended(hod.end), ended(oracle.end)),
        );
    }
    if oracle.stdout != hod.stdout || oracle.truncated != hod.truncated {
        let at = oracle.stdout.iter().zip(&hod.stdout).take_while(|(a, b)| a == b).count();
        let line = oracle.stdout[..at].split(|b| *b == b'\n').count();
        return finding(
            Kind::Disagree,
            format!("stdout differs at byte {at} (line {line}); both {}", ended(oracle.end)),
        );
    }
    Verdict::Agree
}

/// "exited with status 1", "was killed by signal 11", "timed out".
pub fn ended(end: End) -> String {
    match end {
        End::Exit(code) => format!("exited with status {code}"),
        End::Signal(signal) => format!("was killed by signal {signal}"),
        End::TimedOut => "timed out".to_owned(),
    }
}

/// The line of hod's standard error that says most: a panic's location
/// and message, else the first line.
fn first_line_of_interest(output: &Output) -> String {
    let text = String::from_utf8_lossy(&output.stderr);
    let lines: Vec<&str> = text.lines().collect();
    if let Some(at) = lines.iter().position(|line| line.contains("panicked at")) {
        let message = lines.get(at + 1).copied().unwrap_or("");
        return format!("{} {}", lines[at].trim(), message.trim());
    }
    output.first_error_line()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(end: End, stdout: &[u8]) -> Output {
        Output { end, stdout: stdout.to_vec(), stderr: Vec::new(), truncated: false }
    }

    // spec/landed/harness/WP-154.md "Verdicts": the comparison.
    #[test]
    fn comparing_programs() {
        let a = output(End::Exit(0), b"checksum = 1\n");
        assert_eq!(compare(&a, &a.clone()), Verdict::Agree);
        let Verdict::Finding(differs) = compare(&a, &output(End::Exit(0), b"checksum = 2\n"))
        else {
            panic!("a difference");
        };
        assert_eq!(differs.kind, Kind::Disagree);
        assert!(differs.detail.contains("byte 11 (line 1)"), "{}", differs.detail);
        let Verdict::Finding(ends) = compare(&a, &output(End::Signal(11), b"checksum = 1\n"))
        else {
            panic!("a difference");
        };
        assert!(ends.detail.contains("signal 11"), "{}", ends.detail);
    }

    // "Generator": every flavour is picked by some seed, deterministically.
    #[test]
    fn flavours_by_seed() {
        assert_eq!(flavour(0), flavour(3));
        assert_eq!(flavour(0), flavour(1));
        assert_eq!(flavour(2).extra, ["--float"]);
        assert_eq!(flavour(5), flavour(2));
        let args = Bench::csmith_args(8, flavour(8), Path::new("/w/prog.c"));
        let words: Vec<String> = args.iter().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(&words[..2], ["--seed", "8"]);
        assert!(words.contains(&"--float".to_owned()));
        assert_eq!(&words[words.len() - 2..], ["-o", "/w/prog.c"]);
    }

    // spec/harness.md "Random programs", "Modes" (Debug): hod's pairs at
    // `x = x`, and only those, are taken apart.
    #[test]
    fn self_assignments() {
        let text = "\
define internal i32 @func_22(ptr %0) #0 !dbg !5 {
  %5 = alloca ptr, align 8
  %433 = load ptr, ptr %5, align 8, !dbg !10
  %434 = load i32, ptr %433, align 4, !dbg !11
  %435 = load ptr, ptr %5, align 8, !dbg !12
  store i32 %434, ptr %435, align 4, !dbg !13
  %436 = load i32, ptr @g, align 4, !dbg !14
  store i32 %436, ptr @h, align 4, !dbg !15
  ret i32 0
}
!10 = !DILocation(line: 231, column: 3, scope: !5)
!11 = !DILocation(line: 231, column: 4, scope: !5)
!12 = !DILocation(line: 231, column: 5, scope: !5)
!13 = !DILocation(line: 231, column: 6, scope: !5)
!14 = !DILocation(line: 232, column: 3, scope: !5)
!15 = !DILocation(line: 232, column: 4, scope: !5)
";
        let text = format!("source\n{text}");
        assert!(self_assignment(&text, "func_22", 231));
        assert!(!self_assignment(&text, "func_22", 232));
        assert!(!self_assignment(&text, "func_22", 233));
        assert!(!self_assignment(&text, "func_9", 231));
        let text = "\
define i32 @f(ptr %0) {
  %1 = getelementptr inbounds [1 x i32], ptr %0, i64 0, i64 0, !dbg !10
  %2 = load i32, ptr %1, align 4, !dbg !10
  %3 = sext i32 %2 to i64, !dbg !11
  %4 = and i64 %3, -1, !dbg !11
  %5 = trunc i64 %4 to i32, !dbg !11
  %6 = getelementptr inbounds [1 x i32], ptr %0, i64 0, i64 0, !dbg !11
  store i32 %5, ptr %6, align 4, !dbg !11
  %7 = load i32, ptr %0, align 4, !dbg !12
  %8 = trunc i32 %7 to i8, !dbg !12
  %9 = zext i8 %8 to i32, !dbg !12
  store i32 %9, ptr %0, align 4, !dbg !12
  %10 = load i32, ptr %0, align 4, !dbg !13
  %11 = xor i32 %10, 1, !dbg !13
  store i32 %11, ptr %0, align 4, !dbg !13
  %12 = load i32, ptr %0, align 4, !dbg !14
  %13 = sub i32 0, %12, !dbg !14
  store i32 %13, ptr %0, align 4, !dbg !14
  %14 = load i32, ptr %0, align 4, !dbg !15
  %15 = or i32 0, %14, !dbg !15
  store i32 %15, ptr %0, align 4, !dbg !15
  ret i32 0
}
!10 = !DILocation(line: 5, column: 3, scope: !5)
!11 = !DILocation(line: 5, column: 4, scope: !5)
!12 = !DILocation(line: 6, column: 3, scope: !5)
!13 = !DILocation(line: 7, column: 3, scope: !5)
!14 = !DILocation(line: 8, column: 3, scope: !5)
!15 = !DILocation(line: 9, column: 3, scope: !5)
";
        let text = format!("source\n{text}");
        // x &= -1 through the usual conversions, and 0 | x: yes.
        assert!(self_assignment(&text, "f", 5));
        assert!(self_assignment(&text, "f", 9));
        // A narrowing on the way, x ^= 1, 0 - x: no.
        assert!(!self_assignment(&text, "f", 6));
        assert!(!self_assignment(&text, "f", 7));
        assert!(!self_assignment(&text, "f", 8));
        // a[i] = a[i], i loaded twice; x ^= (&y != &y); a store of poison;
        // a[i] = a[j]: the first three only.
        let text = "source
define i32 @g(ptr %0, ptr %1) {
  %3 = load i32, ptr %1, align 4, !dbg !10
  %4 = zext i32 %3 to i64, !dbg !10
  %5 = getelementptr inbounds nuw [5 x i32], ptr @a, i64 0, i64 %4, !dbg !10
  %6 = load i32, ptr %5, align 4, !dbg !10
  %7 = load i32, ptr %1, align 4, !dbg !10
  %8 = zext i32 %7 to i64, !dbg !10
  %9 = getelementptr inbounds nuw [5 x i32], ptr @a, i64 0, i64 %8, !dbg !10
  store i32 %6, ptr %9, align 4, !dbg !10
  %10 = icmp ne ptr %0, %0, !dbg !11
  %11 = zext i1 %10 to i32, !dbg !11
  %12 = load i32, ptr %1, align 4, !dbg !11
  %13 = xor i32 %12, %11, !dbg !11
  store i32 %13, ptr %1, align 4, !dbg !11
  store i32 poison, ptr %1, align 4, !dbg !12
  %14 = load i32, ptr %1, align 4, !dbg !13
  %15 = zext i32 %14 to i64, !dbg !13
  %16 = getelementptr inbounds nuw [5 x i32], ptr @a, i64 0, i64 %15, !dbg !13
  %17 = load i32, ptr %16, align 4, !dbg !13
  %18 = load i32, ptr %0, align 4, !dbg !13
  %19 = zext i32 %18 to i64, !dbg !13
  %20 = getelementptr inbounds nuw [5 x i32], ptr @a, i64 0, i64 %19, !dbg !13
  store i32 %17, ptr %20, align 4, !dbg !13
  ret i32 0
}
!10 = !DILocation(line: 5, column: 3, scope: !5)
!11 = !DILocation(line: 6, column: 3, scope: !5)
!12 = !DILocation(line: 7, column: 3, scope: !5)
!13 = !DILocation(line: 8, column: 3, scope: !5)
";
        assert!(self_assignment(text, "g", 5));
        assert!(self_assignment(text, "g", 6));
        assert!(self_assignment(text, "g", 7));
        assert!(!self_assignment(text, "g", 8));
    }

    // spec/harness.md "Random programs", "Modes" (Debug): llc's pairs at a
    // branch whose destination is known, and only those, are taken apart.
    #[test]
    fn constant_branches() {
        let text = "source
define i32 @f(ptr %0) {
  %1 = load i32, ptr %0, align 4, !dbg !10
  br i1 true, label %2, label %3, !dbg !11

2:
  %4 = load i32, ptr %0, align 4, !dbg !12
  br i1 true, label %3, label %5, !dbg !12

3:
  %6 = load i1, ptr %0, align 4, !dbg !13
  br i1 %6, label %5, label %5, !dbg !13

5:
  ret i32 %4, !dbg !14
}
!10 = !DILocation(line: 5, column: 3, scope: !5)
!11 = !DILocation(line: 5, column: 4, scope: !5)
!12 = !DILocation(line: 6, column: 3, scope: !5)
!13 = !DILocation(line: 7, column: 3, scope: !5)
!14 = !DILocation(line: 8, column: 3, scope: !5)
";
        assert!(constant_branch(text, "f", 5), "the first");
        // A load something uses, a branch on a value, a return: no.
        assert!(!constant_branch(text, "f", 6));
        assert!(!constant_branch(text, "f", 7));
        assert!(!constant_branch(text, "f", 8));
    }

    #[test]
    fn shell_lines_are_quoted() {
        let args: Vec<OsString> = vec!["-o".into(), "a b".into(), "it's".into(), "-x=1".into()];
        assert_eq!(
            shell_line(Path::new("/bin/cc"), &args, Some(Path::new("/w d"))),
            "cd '/w d' && /bin/cc -o 'a b' 'it'\\''s' -x=1"
        );
    }
}
