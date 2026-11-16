//! The cargo steps of `cargo xtask ci` (`spec/harness.md` "ci"). Ported
//! from kiln's `xtask ci`, which met the same build directory.

use std::process::Command;

use crate::jobs::children::run_step;

/// Should a cargo step that failed with `stderr` be run again
/// (`spec/harness.md` "ci")? Once, and only for the two errors rustc gives
/// when a dependency's artifact is not in the build directory although
/// cargo had just written it, which the workspace's file system does to a
/// build under load (its directory listings lag behind fresh files):
///
/// - ``error[E0463]: can't find crate for `X` ``: neither the rlib nor the
///   rmeta of a dependency found by `-L dependency=` was listed;
/// - ``error: crate `X` required to be available in rlib format, but was not
///   found in this form``: its rmeta was listed (written earlier, at the end
///   of analysis) but its rlib (renamed into place at the end of codegen)
///   not yet, and a binary is being linked.
///
/// A rerun finds the artifacts, or rebuilds them. Every other failure is
/// real.
pub fn should_retry(stderr: &str, retried: bool) -> bool {
    !retried && stderr.lines().any(|line| vanished_artifact(line).is_some())
}

/// The rustc error that `line` is, if it is one of those [`should_retry`]
/// retries: its short name for the note.
fn vanished_artifact(line: &str) -> Option<&'static str> {
    let line = line.trim();
    if line.starts_with("error[E0463]") {
        return Some("error[E0463] (can't find crate)");
    }
    let rlib = line.strip_prefix("error: crate `")?.strip_suffix(
        "` required to be available in rlib format, but was not found in this form",
    )?;
    (!rlib.is_empty() && !rlib.contains(char::is_whitespace))
        .then_some("crate required in rlib format")
}

/// Runs `cargo <args>`, echoing its standard error while reading it, and
/// runs it once more when it failed as [`should_retry`] describes. Returns
/// whether the step passed.
///
/// The step is a jobserver client of its own and is stopped with `xtask`
/// ([`run_step`], `spec/harness.md` "Jobserver").
pub fn cargo_step(args: &[&str]) -> bool {
    let shown = args.join(" ");
    let mut retried = false;
    loop {
        let mut cargo = Command::new(env!("CARGO"));
        let (status, stderr) = match run_step(cargo.args(args), true) {
            Ok(result) => result,
            Err(e) => {
                eprintln!("ci: cannot run cargo {shown}: {e}");
                return false;
            }
        };
        if status.success() {
            return true;
        }
        if !should_retry(&stderr, retried) {
            eprintln!("ci: cargo {shown}: {status}");
            return false;
        }
        let why = stderr.lines().find_map(vanished_artifact).unwrap_or("a missing artifact");
        eprintln!(
            "ci: [retry] cargo {shown} failed with {why}: an artifact was not visible to rustc \
             mid-build; running it once more"
        );
        retried = true;
    }
}

/// The steps of `cargo xtask ci`, in the order they run (`spec/harness.md`
/// "ci"): the quick gate's, then [`FULL_ONLY`].
pub const STEPS: [&str; 13] = [
    "fmt",
    "scripts",
    "workflows",
    "clippy",
    "test",
    "irdiff",
    "roundtrip",
    "bench-aarch64",
    "bench-x86_64",
    "debug-x86_64",
    "debug-aarch64",
    "corpus-aarch64",
    "corpus-x86_64",
];

/// The steps only the full gate runs, cheapest first so that it fails
/// fast: the counts gate of the benchmark (WP-292, `bench --check
/// --counts`), L7 (`debug` full tier and `debug verify`) and then L6
/// (`corpus verify --strict` with the list of expected results) for each
/// target architecture, from this one host (`docs/VALIDATION.md` "Gates").
pub const FULL_ONLY: [&str; 6] = [
    "bench-aarch64",
    "bench-x86_64",
    "debug-x86_64",
    "debug-aarch64",
    "corpus-aarch64",
    "corpus-x86_64",
];

/// The target architecture a step of [`FULL_ONLY`] is for, by name.
pub fn step_target(step: &str) -> Option<&'static str> {
    let (_, arch) = step.split_once('-')?;
    ["aarch64", "x86_64"].into_iter().find(|known| *known == arch)
}

/// How a step of the gate ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepResult {
    Passed,
    Failed,
    /// It needed what is not here, and tools are not required: what.
    Skipped(String),
    /// Not run: an earlier step failed, or the quick gate does not run it.
    NotRun(&'static str),
}

impl StepResult {
    /// The word in the table.
    pub fn word(&self) -> String {
        match self {
            Self::Passed => "passed".to_owned(),
            Self::Failed => "FAILED".to_owned(),
            Self::Skipped(why) => format!("skipped ({why})"),
            Self::NotRun(why) => format!("not run ({why})"),
        }
    }
}

/// A wall time as the gate prints it: `4.2 s`, `12:03`, `1:05:36`.
pub fn wall(time: std::time::Duration) -> String {
    let seconds = time.as_secs();
    if seconds < 60 {
        format!("{:.1} s", time.as_secs_f64())
    } else if seconds < 3600 {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    } else {
        format!("{}:{:02}:{:02}", seconds / 3600, seconds / 60 % 60, seconds % 60)
    }
}

/// The table the gate ends with: one line per selected step, its result
/// and its wall time, and the total.
pub fn table(rows: &[(&str, StepResult, Option<std::time::Duration>)]) -> String {
    use std::fmt::Write as _;
    let mut text = String::from("ci: step            result                       wall\n");
    for (step, result, time) in rows {
        let time = time.map_or_else(|| "-".to_owned(), wall);
        let _ = writeln!(text, "ci: {step:<15} {:<28} {time}", result.word());
    }
    let total: std::time::Duration = rows.iter().filter_map(|(_, _, time)| *time).sum();
    let _ = writeln!(text, "ci: {:<15} {:<28} {}", "total", "", wall(total));
    text
}

/// Runs the selected steps in order, stops at the first that fails,
/// prints each step's wall time and the table, and says `ci: ok` when
/// every step passed or was skipped (`spec/harness.md` "ci"). Whether the
/// gate passed.
pub fn run(args: &Args) -> bool {
    let mut rows = Vec::new();
    let mut failed = false;
    for step in args.steps.iter().copied() {
        if args.quick && FULL_ONLY.contains(&step) {
            rows.push((step, StepResult::NotRun("full gate only"), None));
            continue;
        }
        if failed {
            rows.push((step, StepResult::NotRun("an earlier step failed"), None));
            continue;
        }
        match args.part {
            Some(part) if step == "test" => eprintln!("ci: {step} (part {part})"),
            _ => eprintln!("ci: {step}"),
        }
        let start = std::time::Instant::now();
        let result = run_one(step, args);
        let time = start.elapsed();
        eprintln!("ci: {step}: {} in {}", result.word(), wall(time));
        if result == StepResult::Failed {
            eprintln!("ci: {step} FAILED");
            failed = true;
        }
        rows.push((step, result, Some(time)));
    }
    eprint!("{}", table(&rows));
    if !failed {
        eprintln!("ci: ok");
    }
    !failed
}

/// Runs one step.
fn run_one(step: &str, args: &Args) -> StepResult {
    let passed = |ok: bool| if ok { StepResult::Passed } else { StepResult::Failed };
    match step {
        "fmt" => passed(cargo_step(&["fmt", "--all", "--check"])),
        // WP-265: the real-project report's self-test and the shell
        // scripts' syntax; the hosted workflow files through a YAML parser.
        "scripts" => crate::scripts::scripts_gate(&root(), hod_testutil::tools_required()),
        "workflows" => crate::scripts::workflows_gate(&root(), &[], hod_testutil::tools_required()),
        // spec/cli.md "Embedding": the facade without its readers, as a
        // program that builds `hod_ir` itself takes it. Part of the
        // `clippy` step, so every lane that lints runs it.
        "clippy" => passed(
            cargo_step(&["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"])
                && cargo_step(&[
                    "clippy",
                    "-p",
                    "hod",
                    "--no-default-features",
                    "--",
                    "-D",
                    "warnings",
                ]),
        ),
        "test" => {
            let test = test_args(args.part);
            passed(cargo_step(&test.iter().map(String::as_str).collect::<Vec<_>>()))
        }
        "irdiff" => passed(irdiff_gate(args.quick)),
        "roundtrip" => roundtrip_gate(args.quick),
        _ => match (step.split_once('-'), step_target(step)) {
            (Some(("debug", _)), Some(arch)) => passed(debug_gate(arch)),
            (Some(("corpus", _)), Some(arch)) => corpus_gate(arch),
            (Some(("bench", _)), Some(arch)) => bench_gate(arch),
            _ => unreachable!("Args::parse accepts only the gate's steps, not '{step}'"),
        },
    }
}

/// The workspace this harness was built from.
fn root() -> std::path::PathBuf {
    let xtask = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    xtask.parent().unwrap_or(xtask).to_path_buf()
}

/// The L4 step of the gate (spec/harness.md "irdiff", "In the gate"): the
/// runs [`irdiff_runs`] plans for the tier, the other architecture's
/// under its emulator among them.
#[cfg(unix)]
fn irdiff_gate(quick: bool) -> bool {
    use crate::irdiff::{Arch, Config, run, workspace_root};
    let mut ok = true;
    for planned in irdiff_runs(quick, Arch::host()) {
        eprintln!("ci: {}", irdiff_command(&planned));
        let mut config = Config::for_workspace();
        config.llvm = planned.llvm.map(|major| major.to_string());
        (config.target, config.tier, config.mode) = (planned.target, planned.tier, planned.mode);
        config.witness = planned.witness;
        config.paths = planned.paths.iter().map(|path| workspace_root().join(path)).collect();
        match run(&config) {
            Ok(report) => {
                eprint!("{}", report.summary(false));
                ok &= report.exit_status() == 0;
            }
            Err(message) => {
                eprintln!("xtask irdiff: {message}");
                ok = false;
            }
        }
    }
    ok
}

/// The L2 step of the gate (spec/harness.md "roundtrip", "In the gate"):
/// the quick tier of the corpus, or all of it.
#[cfg(unix)]
fn roundtrip_gate(quick: bool) -> StepResult {
    let mut options = crate::roundtrip::Options::for_workspace();
    options.quick = quick;
    match crate::roundtrip::gate(&options) {
        Ok(text) => {
            eprint!("{text}");
            match text.strip_prefix("roundtrip: skipped: ") {
                Some(why) => StepResult::Skipped(why.split(';').next().unwrap_or(why).to_owned()),
                None => StepResult::Passed,
            }
        }
        Err(text) => {
            eprint!("{text}");
            StepResult::Failed
        }
    }
}

/// The L7 step of one target architecture (spec/harness.md "debug", "In
/// the gate").
#[cfg(unix)]
fn debug_gate(arch: &str) -> bool {
    crate::debug::gate(crate::irdiff::Arch::from_name(arch))
}

/// The L6 step of one target architecture (spec/harness.md "corpus",
/// "In the gate"): `corpus verify --strict` of that architecture's
/// default corpus, with the workspace's list of expected results. A
/// missing corpus skips the step, or fails it under `HOD_REQUIRE_TOOLS=1`.
#[cfg(unix)]
fn corpus_gate(arch: &str) -> StepResult {
    use crate::corpus::{self, manifest, verify};
    let target = crate::irdiff::Arch::from_name(arch);
    let build_dir = crate::irdiff::default_build_dir();
    let mut options = corpus::verify_defaults(&build_dir, target);
    options.config.mode = crate::irdiff::Mode::Strict;
    options.config.jobs = crate::irdiff::Config::for_workspace().jobs;
    if !options.dir.join(manifest::MANIFEST).is_file() {
        let why = format!("no corpus at '{}' (cargo xtask corpus capture)", options.dir.display());
        if options.config.require_tools {
            eprintln!("corpus: error: {why} (HOD_REQUIRE_TOOLS=1)");
            return StepResult::Failed;
        }
        eprintln!("corpus: skipped: {why}; under HOD_REQUIRE_TOOLS=1 this fails");
        return StepResult::Skipped(why);
    }
    eprintln!(
        "ci: cargo xtask corpus verify --target {arch} --strict -j {} {}",
        options.config.jobs,
        options.dir.display()
    );
    match verify::verify(&options) {
        Ok(report) => {
            eprint!("{}", report.summary(false));
            if report.exit_status() == 0 { StepResult::Passed } else { StepResult::Failed }
        }
        Err(message) => {
            eprintln!("xtask corpus: {message}");
            StepResult::Failed
        }
    }
}

/// The counts gate of the benchmark for one target architecture
/// (spec/harness.md "bench", "The counts gate"; WP-292): `bench --check
/// --counts --target <arch>` against the committed counts baseline. A
/// missing valgrind (or `llc`, or clang) skips the step with one line, or
/// fails it under `HOD_REQUIRE_TOOLS=1`.
#[cfg(unix)]
fn bench_gate(arch: &str) -> StepResult {
    use crate::bench;
    let mut config = bench::Config::for_workspace();
    (config.action, config.counts) = (bench::Action::Check, true);
    config.target = crate::irdiff::Arch::from_name(arch);
    eprintln!("ci: cargo xtask bench --check --counts --target {arch}");
    match bench::run(&config) {
        Ok(run) => {
            eprint!("{}", run.summary(false));
            // Nothing measured for want of a tool: a skip, said in a line.
            if run.results.is_none() && !run.missing_tools.is_empty() && !config.require_tools {
                let missing = &run.missing_tools[0];
                let tool = missing.split(" (").next().unwrap_or(missing);
                eprintln!("bench: skipped: no {tool}; under HOD_REQUIRE_TOOLS=1 this fails");
                return StepResult::Skipped(format!("no {tool}"));
            }
            if run.exit_status() == 0 { StepResult::Passed } else { StepResult::Failed }
        }
        Err(message) => {
            eprintln!("xtask bench: {message}");
            StepResult::Failed
        }
    }
}

#[cfg(not(unix))]
fn bench_gate(_: &str) -> StepResult {
    StepResult::Skipped("only available on Unix hosts".to_owned())
}

#[cfg(not(unix))]
fn irdiff_gate(_: bool) -> bool {
    eprintln!("ci: irdiff is only available on Unix hosts; skipped");
    true
}

#[cfg(not(unix))]
fn roundtrip_gate(_: bool) -> StepResult {
    StepResult::Skipped("only available on Unix hosts".to_owned())
}

#[cfg(not(unix))]
fn debug_gate(_: &str) -> bool {
    eprintln!("ci: debug is only available on Unix hosts; skipped");
    true
}

#[cfg(not(unix))]
fn corpus_gate(_: &str) -> StepResult {
    StepResult::Skipped("only available on Unix hosts".to_owned())
}

/// The steps of the quick gate the hosted CI lane runs
/// (`.github/workflows/ci-lane.yml`, `spec/harness.md` "CI").
pub const HOSTED_STEPS: [&str; 6] = ["fmt", "scripts", "workflows", "clippy", "test", "irdiff"];

/// The quick gate's steps the hosted lane does not run: `roundtrip` needs
/// the corpus, which is build output made on the development machine
/// (`spec/harness.md` "corpus") and not on a hosted runner.
pub const NOT_HOSTED: [&str; 1] = ["roundtrip"];

/// The parts of the `test` step (`--part <name>`), which the hosted lane
/// runs as parallel jobs: each names workspace packages, and the last,
/// `rest`, is every package the others do not name (`--workspace
/// --exclude ...`), so that the parts together are the whole step, also
/// for a package added later.
pub const TEST_PARTS: [(&str, &[&str]); 5] = [
    ("a64", &["hod_a64"]),
    ("x64", &["hod_x64", "hod_mc_x64"]),
    ("harness", &["xtask"]),
    ("front", &["hod", "hod_cli", "hod_interp"]),
    ("rest", &[]),
];

/// The `cargo` arguments of the `test` step, or of its part `part`.
pub fn test_args(part: Option<&str>) -> Vec<String> {
    let mut args: Vec<String> = vec!["test".into()];
    let named = part.and_then(|part| TEST_PARTS.iter().find(|(name, _)| *name == part));
    match named {
        Some((_, packages)) if !packages.is_empty() => {
            for package in *packages {
                args.extend(["-p".into(), (*package).into()]);
            }
        }
        Some(_) => {
            args.push("--workspace".into());
            for package in TEST_PARTS.iter().flat_map(|(_, packages)| packages.iter()) {
                args.extend(["--exclude".into(), (*package).into()]);
            }
        }
        None => args.push("--workspace".into()),
    }
    args.extend(["--quiet".into(), "--no-fail-fast".into()]);
    args
}

/// What `cargo xtask ci [--quick] [--step <name>]... [--part <name>]` asks
/// for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Args {
    pub quick: bool,
    /// The steps to run, in [`STEPS`] order; every step when none is named.
    pub steps: Vec<&'static str>,
    /// One part of the `test` step ([`TEST_PARTS`]); `None` is all of it.
    pub part: Option<&'static str>,
}

impl Args {
    /// Parses the arguments after `ci`.
    ///
    /// # Errors
    /// An unknown argument, step or part, `--step` or `--part` without a
    /// name, or `--part` with any step but `test` alone.
    pub fn parse(args: &[String]) -> Result<Self, String> {
        let mut quick = false;
        let mut named: Vec<String> = Vec::new();
        let mut part: Option<String> = None;
        let mut rest = args.iter();
        while let Some(arg) = rest.next() {
            match arg.as_str() {
                "--quick" => quick = true,
                "--step" => named.push(rest.next().ok_or("--step needs a step name")?.clone()),
                "--part" => part = Some(rest.next().ok_or("--part needs a part name")?.clone()),
                other => {
                    if let Some(step) = other.strip_prefix("--step=") {
                        named.push(step.to_owned());
                    } else if let Some(name) = other.strip_prefix("--part=") {
                        part = Some(name.to_owned());
                    } else {
                        return Err(format!("unknown argument '{other}'"));
                    }
                }
            }
        }
        let part = match part {
            None => None,
            Some(name) => {
                let Some((known, _)) = TEST_PARTS.iter().find(|(known, _)| *known == name) else {
                    let parts: Vec<&str> = TEST_PARTS.iter().map(|(n, _)| *n).collect();
                    return Err(format!("unknown part '{name}' (parts: {})", parts.join(", ")));
                };
                if named != ["test"] {
                    return Err(
                        "--part is a part of the test step: give it with --step test alone"
                            .to_owned(),
                    );
                }
                Some(*known)
            }
        };
        if let Some(unknown) = named.iter().find(|step| !STEPS.contains(&step.as_str())) {
            return Err(format!("unknown step '{unknown}' (steps: {})", STEPS.join(", ")));
        }
        let steps = STEPS
            .into_iter()
            .filter(|step| named.is_empty() || named.iter().any(|n| n == step))
            .collect();
        Ok(Self { quick, steps, part })
    }

    /// Whether `step` runs.
    pub fn runs(&self, step: &str) -> bool {
        self.steps.contains(&step)
    }
}

/// One `irdiff` run of the gate (`spec/harness.md` "irdiff", "In the
/// gate").
#[cfg(unix)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IrdiffRun {
    /// The oracle's LLVM major; `None` is the default (the newest found).
    pub llvm: Option<u32>,
    /// `None` is the host's.
    pub target: Option<crate::irdiff::Arch>,
    pub tier: crate::irdiff::TierFilter,
    pub mode: crate::irdiff::Mode,
    pub witness: bool,
    /// The tests, relative to the workspace; empty is all of `tests/ir`.
    pub paths: &'static [&'static str],
}

/// The oracle of the quick gate's run for the target that is not the
/// host ("In the gate"): the newest LLVM, over every seed.
#[cfg(unix)]
pub const QUICK_CROSS_LLVM: u32 = crate::irdiff::tools::LLVM_MAJORS[0];

/// The `irdiff` runs of the gate, in order, on a host of architecture
/// `host` ("In the gate"):
///
/// - quick: the quick tier with the default oracle; then, for the other
///   architecture, every seed under the emulator with
///   [`QUICK_CROSS_LLVM`] (WP-139: all of them take under 20 s);
/// - full: every test with the witness, once per oracle (LLVM 22, 19);
///   every test in hybrid mode with the default oracle; and every test for
///   the other architecture under the emulator, once per oracle, so that a
///   difference between LLVM 19's and 22's programs of that target is a
///   failure of one of the two runs;
/// - both: last, the quick tier for i686 under its emulator with
///   [`QUICK_CROSS_LLVM`] (WP-302: about 2 s while hod declines it).
///
/// A host that is not a Linux machine of a known architecture runs no
/// cross run: there is no "other" target to emulate.
#[cfg(unix)]
pub fn irdiff_runs(quick: bool, host: Option<crate::irdiff::Arch>) -> Vec<IrdiffRun> {
    use crate::irdiff::tools::LLVM_MAJORS;
    use crate::irdiff::{Arch, Mode, TierFilter};
    let base = IrdiffRun {
        llvm: None,
        target: None,
        tier: TierFilter::All,
        mode: Mode::Strict,
        witness: false,
        paths: &[],
    };
    let other = host.map(|host| if host == Arch::Aarch64 { Arch::X86_64 } else { Arch::Aarch64 });
    // WP-302: the quick tier for i686 with the newest oracle, last, in
    // both tiers of the gate, on a host that can emulate it.
    let i686 = host.map(|_| IrdiffRun {
        llvm: Some(QUICK_CROSS_LLVM),
        target: Some(Arch::I686),
        tier: TierFilter::Quick,
        ..base.clone()
    });
    let mut runs = Vec::new();
    if quick {
        runs.push(IrdiffRun { tier: TierFilter::Quick, ..base.clone() });
        if let Some(other) = other {
            runs.push(IrdiffRun { llvm: Some(QUICK_CROSS_LLVM), target: Some(other), ..base });
        }
        runs.extend(i686);
        return runs;
    }
    for major in LLVM_MAJORS {
        runs.push(IrdiffRun { llvm: Some(major), witness: true, ..base.clone() });
    }
    runs.push(IrdiffRun { mode: Mode::Hybrid, ..base.clone() });
    if let Some(other) = other {
        for major in LLVM_MAJORS {
            runs.push(IrdiffRun { llvm: Some(major), target: Some(other), ..base.clone() });
        }
    }
    runs.extend(i686);
    runs
}

/// `cargo xtask irdiff` arguments that make the same run, for the log.
#[cfg(unix)]
pub fn irdiff_command(run: &IrdiffRun) -> String {
    let mut words = vec!["cargo xtask irdiff".to_owned()];
    if let Some(major) = run.llvm {
        words.push(format!("--llvm {major}"));
    }
    if let Some(target) = run.target {
        words.push(format!("--target {}", target.name()));
    }
    if run.tier == crate::irdiff::TierFilter::Quick {
        words.push("--tier quick".to_owned());
    }
    if run.mode == crate::irdiff::Mode::Hybrid {
        words.push("--hybrid".to_owned());
    }
    if run.witness {
        words.push("--witness".to_owned());
    }
    words.extend(run.paths.iter().map(|path| (*path).to_owned()));
    words.join(" ")
}

#[cfg(test)]
mod tests {
    use super::should_retry;

    // spec/harness.md "ci": `--step` selects steps, which run in the
    // gate's order whatever the order they are named in.
    #[test]
    fn steps_are_selected_by_name() {
        use super::{Args, STEPS};
        let parse =
            |args: &[&str]| Args::parse(&args.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>());
        let all = parse(&["--quick"]).unwrap();
        assert!(all.quick);
        assert_eq!(all.steps, STEPS);
        let two = parse(&["--step", "test", "--step=fmt"]).unwrap();
        assert!(!two.quick);
        assert_eq!(two.steps, ["fmt", "test"]);
        assert!(two.runs("fmt") && !two.runs("clippy"));
        assert!(parse(&["--step", "lint"]).unwrap_err().contains("unknown step 'lint'"));
        assert!(parse(&["--step"]).is_err());
        assert!(parse(&["--fast"]).is_err());
        assert_eq!(all.part, None);
        let part = parse(&["--quick", "--step", "test", "--part", "a64"]).unwrap();
        assert_eq!((part.steps.as_slice(), part.part), (&["test"][..], Some("a64")));
        assert_eq!(parse(&["--step", "test", "--part=rest"]).unwrap().part, Some("rest"));
        assert!(parse(&["--step", "test", "--part", "b"]).unwrap_err().contains("unknown part"));
        assert!(parse(&["--part", "a64"]).is_err(), "a part of every step");
        assert!(parse(&["--step", "fmt", "--step", "test", "--part", "a64"]).is_err());
        // The full gate's steps by name, in the gate's order.
        let full = parse(&["--step", "corpus-x86_64", "--step=debug-aarch64", "--step", "fmt"]);
        assert_eq!(full.unwrap().steps, ["fmt", "debug-aarch64", "corpus-x86_64"]);
        assert!(parse(&["--step", "debug"]).unwrap_err().contains("unknown step 'debug'"));
        assert!(parse(&["--step", "corpus-i386"]).is_err());
    }

    // spec/harness.md "ci": the full gate's own steps come after the quick
    // gate's, L7 before L6 (cheapest first), one per target architecture.
    #[test]
    fn the_full_gates_steps() {
        use super::{FULL_ONLY, STEPS, step_target};
        assert_eq!(STEPS[STEPS.len() - FULL_ONLY.len()..], FULL_ONLY);
        assert_eq!(
            FULL_ONLY.map(step_target),
            [
                Some("aarch64"),
                Some("x86_64"),
                Some("x86_64"),
                Some("aarch64"),
                Some("aarch64"),
                Some("x86_64")
            ]
        );
        // WP-292: the counts gate first (about two minutes per target).
        assert!(FULL_ONLY.iter().take(2).all(|s| s.starts_with("bench-")));
        assert!(FULL_ONLY.iter().skip(2).take(2).all(|s| s.starts_with("debug-")));
        assert!(FULL_ONLY.iter().skip(4).all(|s| s.starts_with("corpus-")));
        // The usage names every step.
        let main = include_str!("main.rs");
        for step in STEPS {
            assert!(main.contains(&format!(" {step}")), "{step} in the usage of main.rs");
        }
        assert_eq!(step_target("fmt"), None);
        assert_eq!(step_target("corpus-arm"), None);
    }

    // spec/harness.md "ci": every step's wall time, and the table the gate
    // ends with names a step that did not run.
    #[test]
    fn the_gates_table() {
        use super::{StepResult, table, wall};
        use std::time::Duration;
        assert_eq!(wall(Duration::from_millis(4_240)), "4.2 s");
        assert_eq!(wall(Duration::from_secs(723)), "12:03");
        assert_eq!(wall(Duration::from_secs(3936)), "1:05:36");
        let rows = [
            ("fmt", StepResult::Passed, Some(Duration::from_secs(2))),
            ("roundtrip", StepResult::Skipped("no corpus at '/c'".into()), Some(Duration::ZERO)),
            ("test", StepResult::Failed, Some(Duration::from_secs(70))),
            ("corpus-aarch64", StepResult::NotRun("an earlier step failed"), None),
        ];
        let text = table(&rows);
        assert!(text.contains("ci: fmt             passed"), "{text}");
        assert!(text.contains("skipped (no corpus at '/c')"), "{text}");
        assert!(text.contains("ci: test            FAILED                       1:10"), "{text}");
        assert!(text.contains("ci: corpus-aarch64  not run (an earlier step failed) -"), "{text}");
        assert!(text.lines().last().unwrap().ends_with("1:12"), "{text}");
    }

    // The test step's parts are the whole step: each package of the
    // workspace is in exactly one part (`rest` takes the ones the others
    // do not name), and without a part the step is the whole workspace.
    #[test]
    fn the_test_parts_cover_the_workspace_once() {
        use super::{TEST_PARTS, test_args};
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut packages = vec!["xtask".to_owned()];
        for entry in std::fs::read_dir(root.join("crates")).unwrap() {
            let entry = entry.unwrap();
            if entry.path().join("Cargo.toml").is_file() {
                packages.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
        let named: Vec<&str> = TEST_PARTS.iter().flat_map(|(_, p)| p.iter().copied()).collect();
        for package in &named {
            assert!(packages.iter().any(|p| p == package), "{package} is a workspace package");
        }
        let mut unique = named.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), named.len(), "no package in two parts");
        assert_eq!(TEST_PARTS.iter().filter(|(_, p)| p.is_empty()).count(), 1, "one rest part");
        assert_eq!(TEST_PARTS.last().unwrap().0, "rest");
        let rest = test_args(Some("rest"));
        assert!(rest.contains(&"--workspace".to_owned()));
        assert_eq!(rest.iter().filter(|a| *a == "--exclude").count(), named.len());
        assert_eq!(test_args(Some("a64"))[..3], ["test", "-p", "hod_a64"]);
        assert_eq!(test_args(None), ["test", "--workspace", "--quiet", "--no-fail-fast"]);
    }

    // spec/harness.md "CI": the hosted lane runs every step of the quick
    // gate but `roundtrip` (no corpus on a hosted runner) once, each with
    // `--quick --step <name>`, and every job of the lane requires tools.
    #[test]
    fn the_hosted_lane_runs_the_quick_gates_steps() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let lane = std::fs::read_to_string(root.join(".github/workflows/ci-lane.yml")).unwrap();
        for step in super::HOSTED_STEPS {
            let command = format!("cargo xtask ci --quick --step {step}");
            assert_eq!(lane.matches(&command).count(), 1, "{command} once in ci-lane.yml");
        }
        // The test step runs as one job per part, every part once.
        assert_eq!(
            lane.matches("cargo xtask ci --quick --step test --part ${{ matrix.part }}").count(),
            1
        );
        let parts: Vec<&str> = super::TEST_PARTS.iter().map(|(name, _)| *name).collect();
        let matrix = format!("part: [{}]", parts.join(", "));
        assert_eq!(lane.matches(&matrix).count(), 1, "{matrix} once in ci-lane.yml");
        let mut steps: Vec<&str> = super::HOSTED_STEPS.to_vec();
        steps.extend(super::NOT_HOSTED);
        steps.sort_unstable();
        let mut quick: Vec<&str> =
            super::STEPS.into_iter().filter(|s| !super::FULL_ONLY.contains(s)).collect();
        quick.sort_unstable();
        assert_eq!(steps, quick, "every quick step is hosted or said not to be");
        assert_eq!(lane.matches("run: cargo xtask ci").count(), super::HOSTED_STEPS.len());
        assert!(lane.contains("HOD_REQUIRE_TOOLS: \"1\""), "the lane requires tools");
        let ci = std::fs::read_to_string(root.join(".github/workflows/ci.yml")).unwrap();
        for runner in ["runner: ubuntu-24.04-arm,", "runner: ubuntu-24.04,"] {
            assert_eq!(ci.matches(runner).count(), 1, "{runner} once in ci.yml");
        }
        let sweep = std::fs::read_to_string(root.join(".github/workflows/sweep.yml")).unwrap();
        // The sweep never gates: no pull_request or merge_group trigger.
        let trigger = |line: &str| line == "  pull_request:" || line == "  merge_group:";
        assert!(!sweep.lines().any(trigger), "sweep.yml gates a merge");
        assert!(ci.lines().any(trigger), "the gate's triggers are recognized");
    }

    // spec/harness.md "irdiff", "In the gate": the runs of each tier. The
    // full gate runs the other architecture with BOTH oracles: one run
    // with LLVM 22 alone would let a program LLVM 19 builds differently
    // for that target go unseen.
    #[cfg(unix)]
    #[test]
    fn the_gates_irdiff_runs() {
        use super::{irdiff_command, irdiff_runs};
        use crate::irdiff::Arch;
        let commands = |quick, host| -> Vec<String> {
            irdiff_runs(quick, host).iter().map(irdiff_command).collect()
        };
        // The quick gate runs every seed for the other architecture, with
        // LLVM 22 (WP-139).
        // Both tiers end with the quick tier for i686 (WP-302).
        assert_eq!(
            commands(true, Some(Arch::Aarch64)),
            [
                "cargo xtask irdiff --tier quick",
                "cargo xtask irdiff --llvm 22 --target x86_64",
                "cargo xtask irdiff --llvm 22 --target i686 --tier quick",
            ]
        );
        assert_eq!(
            commands(false, Some(Arch::Aarch64)),
            [
                "cargo xtask irdiff --llvm 22 --witness",
                "cargo xtask irdiff --llvm 19 --witness",
                "cargo xtask irdiff --hybrid",
                "cargo xtask irdiff --llvm 22 --target x86_64",
                "cargo xtask irdiff --llvm 19 --target x86_64",
                "cargo xtask irdiff --llvm 22 --target i686 --tier quick",
            ]
        );
        let on_x86 = commands(false, Some(Arch::X86_64));
        assert_eq!(on_x86[3], "cargo xtask irdiff --llvm 22 --target aarch64");
        assert_eq!(on_x86[5], "cargo xtask irdiff --llvm 22 --target i686 --tier quick");
        assert_eq!(commands(true, Some(Arch::X86_64)).len(), 3);
        assert_eq!(commands(true, None), ["cargo xtask irdiff --tier quick"]);
        assert_eq!(commands(false, None).len(), 3);
    }

    // spec/harness.md "ci": one rerun, only for a missing crate, and only
    // when rustc said so at the start of a line.
    #[test]
    fn retries_only_missing_crates_once() {
        let missing = "   Compiling hod_ir v0.0.0\n\
                       error[E0463]: can't find crate for `hod_ir`\n \
                       --> crates/hod_interp/tests/common/mod.rs:7:5\n";
        assert!(should_retry(missing, false));
        assert!(!should_retry(missing, true), "a second failure is real");
        assert!(should_retry("warning: x\n  error[E0463]: can't find crate for `std`\n", false));
        assert!(!should_retry("error[E0308]: mismatched types\n", false));
        assert!(!should_retry("test result: FAILED. 1 passed; 1 failed\n", false));
        assert!(!should_retry("note: the test checks that error[E0463] is retried\n", false));
        assert!(!should_retry("", false));
    }

    // spec/harness.md "ci": the rlib form of the same cause, exactly as
    // rustc prints it, is retried once; its relatives are not.
    #[test]
    fn retries_a_crate_seen_only_as_rmeta_once() {
        let rlib = "   Compiling hod v0.0.0\n\
                    error: crate `hod_cg` required to be available in rlib format, but was not \
                    found in this form\n\n\
                    error: could not compile `hod` (test \"cli\") due to 1 previous error\n";
        assert!(should_retry(rlib, false));
        assert!(!should_retry(rlib, true), "a second failure is real");
        let unit = "error: crate `hod_cg` required to be available in dylib format, but was not \
                    found in this form\n";
        assert!(!should_retry(unit, false), "hod links no dylib: that is a real error");
        assert!(!should_retry(
            "error: crate `` required to be available in rlib format, but was \
                               not found in this form\n",
            false
        ));
        assert!(!should_retry(
            "note: error: crate `a` required to be available in rlib format, \
                               but was not found in this form\n",
            false
        ));
        assert!(!should_retry("error: crate `a` required to be available in rlib format\n", false));
        assert!(!should_retry("error: linking with `cc` failed: exit status: 1\n", false));
    }
}
