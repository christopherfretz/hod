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
            // columns are counted.
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

    #[test]
    fn shell_lines_are_quoted() {
        let args: Vec<OsString> = vec!["-o".into(), "a b".into(), "it's".into(), "-x=1".into()];
        assert_eq!(
            shell_line(Path::new("/bin/cc"), &args, Some(Path::new("/w d"))),
            "cd '/w d' && /bin/cc -o 'a b' 'it'\\''s' -x=1"
        );
    }
}
