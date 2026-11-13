//! `cargo xtask irdiff` end to end with the real tools: LLVM's `llc`, the
//! linker driver and the linker matrix (`spec/harness.md` "irdiff").
//!
//! Tools-gated like every oracle test (`docs/VALIDATION.md` "Oracles"):
//! without the tools the tests skip with a note; with `HOD_REQUIRE_TOOLS=1`
//! a missing tool is a failure.
#![cfg(target_os = "linux")]

mod common;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use common::Scratch;
use hod_testutil::write_script;
use xtask::irdiff::tools::{self, LLVM_MAJORS, REQUIRED_LINKERS};
use xtask::irdiff::{
    self, Arch, BuiltBy, Config, Mode, Outcome, Report, Search, TierFilter, engine,
};

/// The LLVM majors of the oracle that are installed, if the machine can
/// run the harness at all: an oracle, the linker driver and a linker.
fn installed() -> Option<Vec<u32>> {
    let search = Search::from_env();
    let majors: Vec<u32> = LLVM_MAJORS
        .into_iter()
        .filter(|major| matches!(tools::find_llvm(Some(&major.to_string()), &search), Ok(Some(_))))
        .collect();
    let driver = tools::find_driver(&search).is_some();
    let linkers: Vec<&str> = REQUIRED_LINKERS
        .into_iter()
        .filter(|name| tools::find_linker(name, &search).is_some())
        .collect();
    let found = format!("LLVM {majors:?}, gcc: {driver}, linkers {linkers:?}");
    if hod_testutil::tools_required() {
        assert!(
            majors == LLVM_MAJORS && driver && linkers == REQUIRED_LINKERS,
            "required tools are missing (HOD_REQUIRE_TOOLS=1); found {found}"
        );
    }
    if majors.is_empty() || !driver || linkers.is_empty() {
        eprintln!("note: irdiff's tools not found ({found}); skipping this tool-backed test");
        return None;
    }
    Some(majors)
}

/// This checkout's build directory: the parent of the directory cargo
/// gives integration tests for their files.
fn build_dir() -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).parent().expect("it is <target>/tmp").to_path_buf()
}

/// The workspace's hod, built once.
fn workspace_hod() -> PathBuf {
    static HOD: OnceLock<PathBuf> = OnceLock::new();
    HOD.get_or_init(|| irdiff::build_workspace_hod(&build_dir()).expect("hod builds")).clone()
}

/// The command's defaults with its output in `scratch`.
fn config(scratch: &Scratch) -> Config {
    let mut config = Config::new(scratch.join("build"));
    config.require_tools = hod_testutil::tools_required();
    config
}

fn other_arch() -> Arch {
    if Arch::host() == Some(Arch::Aarch64) { Arch::X86_64 } else { Arch::Aarch64 }
}

/// The number of tests in the seed corpus.
fn corpus_size() -> usize {
    irdiff::collect(&[]).unwrap().len()
}

/// The directives of every test of the seed corpus.
fn corpus_directives() -> Vec<(PathBuf, irdiff::directives::Expectations)> {
    let tests = irdiff::collect(&[]).unwrap().into_iter();
    tests
        .map(|(_, path)| {
            let text = std::fs::read_to_string(&path).unwrap();
            let expect = irdiff::directives::parse(&text)
                .unwrap_or_else(|e| panic!("{}: {e:?}", path.display()));
            (path, expect)
        })
        .collect()
}

/// The number of tests of the seed corpus in LLVM `major`'s dialect: all
/// but those of another dialect (`dialect`).
fn dialect_size(major: u32) -> usize {
    corpus_directives().iter().filter(|(_, expect)| expect.in_dialect(major)).count()
}

// spec/harness.md "Seed corpus": a test in one LLVM's dialect has its twin
// in the other's, `<stem>-19.ll` and `<stem>-22.ll`, so that the "both
// LLVMs" rule holds per dialect; and every `oracle-skip` is listed in
// spec/harness.md "Oracle skips" by its path.
#[test]
fn dialects_come_in_pairs_and_oracle_skips_are_listed() {
    let harness =
        std::fs::read_to_string(irdiff::workspace_root().join("spec/harness.md")).unwrap();
    let skips = harness.split("### Oracle skips").nth(1).expect("the section exists");
    let skips = skips.split("\n## ").next().unwrap();
    let root = irdiff::workspace_root();
    for (path, expect) in corpus_directives() {
        let relative = path.strip_prefix(&root).unwrap().to_string_lossy().into_owned();
        if let Some(dialect) = expect.dialect {
            let suffix = format!("-{dialect}.ll");
            let stem = relative.strip_suffix(&suffix).unwrap_or_else(|| {
                panic!("{relative}: a test in the LLVM {dialect} dialect ends in '{suffix}'")
            });
            let other = if dialect == 19 { 22 } else { 19 };
            let twin = root.join(format!("{stem}-{other}.ll"));
            assert!(twin.is_file(), "{relative} has no twin {}", twin.display());
        }
        for skip in &expect.oracle_skip {
            let majors: Vec<String> = skip.llvm.iter().map(u32::to_string).collect();
            let row = format!("| `{relative}` | {} | {} |", majors.join(", "), skip.reason);
            assert!(skips.contains(&row), "spec/harness.md \"Oracle skips\" has no row {row}");
        }
    }
}

// spec/harness.md "Seed corpus": every test of `tests/ir/` is green with
// LLVM 22 and with LLVM 19, with every linker: the oracle's program
// behaves as the directives say, and hod either declines as the test
// expects or builds a program that behaves the same.
#[test]
fn the_seed_corpus_is_green() {
    let Some(majors) = installed() else { return };
    let scratch = Scratch::new("irdiff-corpus");
    for major in majors {
        let mut config = config(&scratch);
        config.hod = Some(workspace_hod());
        config.llvm = Some(major.to_string());
        let report = irdiff::run(&config).unwrap();
        let all = report.summary(true);
        assert_eq!(report.exit_status(), 0, "LLVM {major}:\n{all}");
        assert!(report.records.iter().all(|record| record.llvm == Some(major)), "{all}");
        // Every test is valid on both targets and both LLVMs, or in one
        // LLVM's dialect, so on a machine with the tools only the tests of
        // the other dialect are skipped, and every other test has a record
        // per linker.
        let linkers = all.split("; linkers ").nth(1).unwrap().split(';').next().unwrap();
        let linkers = linkers.split(' ').count();
        assert!(corpus_size() >= 20 && report.tests() == corpus_size(), "{all}");
        let skipped = corpus_size() - dialect_size(major);
        assert_eq!(report.count(Outcome::Skip), skipped, "{all}");
        assert_eq!(report.records.len(), dialect_size(major) * linkers + skipped, "{all}");
        for record in &report.records {
            assert!(
                matches!(
                    record.outcome,
                    Outcome::Pass | Outcome::Unsupported | Outcome::Xfail | Outcome::Skip
                ),
                "{all}"
            );
            assert!(record.covers.as_deref().is_some_and(|c| c.starts_with("spec/ir.md §")));
        }
        // The quick tier is a strict subset.
        config.tier = TierFilter::Quick;
        let quick = irdiff::run(&config).unwrap();
        assert_eq!(quick.exit_status(), 0, "{}", quick.summary(true));
        assert!(quick.tests() >= 5 && quick.tests() < report.tests(), "{}", quick.summary(true));
    }
}

// "Hybrid mode", with the real hod and the real `llc`: whatever hod does
// not build itself the oracle builds through hod's fallback, and every
// program then behaves as the oracle's.
#[test]
fn the_seed_corpus_passes_in_hybrid_mode() {
    let Some(majors) = installed() else { return };
    let scratch = Scratch::new("irdiff-hybrid");
    let mut config = config(&scratch);
    config.hod = Some(workspace_hod());
    config.llvm = Some(majors[0].to_string());
    config.mode = Mode::Hybrid;
    let report = irdiff::run(&config).unwrap();
    let all = report.summary(true);
    assert_eq!(report.exit_status(), 0, "{all}");
    assert_eq!(report.count(Outcome::Pass), report.records.len(), "{all}");
    assert!(
        report.records.iter().all(|r| matches!(r.built_by, BuiltBy::Hod | BuiltBy::Fallback)),
        "{all}"
    );
    assert!(all.contains("irdiff: coverage: hod built "), "{all}");
}

// "Cross targets": for the architecture that is not this machine's, both
// compilers build every test's object and nothing is linked or run.
#[test]
fn the_seed_corpus_builds_for_the_other_target() {
    let Some(majors) = installed() else { return };
    let scratch = Scratch::new("irdiff-cross");
    for major in majors {
        let mut config = config(&scratch);
        config.hod = Some(workspace_hod());
        config.llvm = Some(major.to_string());
        config.target = Some(other_arch());
        let report = irdiff::run(&config).unwrap();
        let all = report.summary(true);
        assert_eq!(report.exit_status(), 0, "LLVM {major}:\n{all}");
        assert_eq!(report.records.len(), corpus_size(), "{all}");
        assert_eq!(report.count(Outcome::Skip), corpus_size() - dialect_size(major), "{all}");
        assert!(report.records.iter().all(|record| record.linker.is_none()), "{all}");
        assert!(report.records.iter().all(|r| r.target == other_arch().triple()), "{all}");
        assert!(report.count(Outcome::Error) + report.count(Outcome::Fail) == 0, "{all}");
    }
}

/// A stand-in for hod that compiles with the real `llc` after rewriting
/// the module with a `sed` expression: a hod that builds everything, and
/// (with an expression that changes the program) one that miscompiles.
fn rewriting_hod(scratch: &Scratch, name: &str, llc: &Path, expression: &str) -> PathBuf {
    selecting_hod(scratch, name, llc, expression, "")
}

/// [`rewriting_hod`] whose `llc` also gets the arguments `extra`: a hod
/// that is `llc -O0` with the instruction selector `extra` chooses.
fn selecting_hod(
    scratch: &Scratch,
    name: &str,
    llc: &Path,
    expression: &str,
    extra: &str,
) -> PathBuf {
    let path = scratch.join(name);
    let body = format!(
        r#"PATH=/usr/bin:/bin
log= out= in= args=
while [ $# -gt 0 ]; do
  case "$1" in
    --coverage-log=*) log=${{1#--coverage-log=}} ;;
    --strict) ;;
    -o) out=$2; shift ;;
    -*) args="$args $1" ;;
    *) in=$1 ;;
  esac
  shift
done
sed -e '{expression}' "$in" > "$out.ll" || exit 1
'{llc}' $args {extra} -o "$out" "$out.ll" || exit 1
printf 'hod\t-\ttext\t1\t%s\t-\t5\t-\n' "$out" >> "$log""#,
        llc = llc.display()
    );
    write_script(&path, &body);
    path
}

const SUM: &str = r#";@ stdout: 40 + 2 = 42
;@ exit: 42
@.fmt = private unnamed_addr constant [14 x i8] c"%d + %d = %d\0A\00"

declare i32 @printf(ptr, ...)

define i32 @sum(i32 %a, i32 %b) {
  %r = add i32 %a, %b
  ret i32 %r
}

define i32 @main() {
entry:
  %r = call i32 @sum(i32 40, i32 2)
  call i32 (ptr, ...) @printf(ptr @.fmt, i32 40, i32 2, i32 %r)
  br label %done
spin:
  br label %spin
done:
  ret i32 %r
}
"#;

/// Runs the one test `SUM` with a hod that rewrites it.
fn run_rewritten(scratch: &Scratch, major: u32, name: &str, expression: &str) -> Report {
    let search = Search::from_env();
    let llvm = tools::find_llvm(Some(&major.to_string()), &search).unwrap().unwrap();
    let test = scratch.join("sum.ll");
    std::fs::write(&test, SUM).unwrap();
    let mut config = config(scratch);
    config.paths = vec![test];
    config.llvm = Some(major.to_string());
    config.hod = Some(rewriting_hod(scratch, name, &llvm.llc, expression));
    config.run_limits.time = Duration::from_secs(3);
    irdiff::run(&config).unwrap()
}

// "Outcomes", end to end: a real object that is right passes with every
// linker, and a real miscompile is a FAIL with every linker, whether it
// shows in the output, in the exit status, at link time or as a hang.
#[test]
fn a_real_miscompile_is_detected() {
    let Some(majors) = installed() else { return };
    let scratch = Scratch::new("irdiff-miscompile");
    let major = majors[0];

    let right = run_rewritten(&scratch, major, "hod-right", "s/^$//");
    let all = right.summary(true);
    assert_eq!(right.exit_status(), 0, "{all}");
    assert!(!right.records.is_empty(), "{all}");
    assert!(
        right.records.iter().all(|r| r.outcome == Outcome::Pass && r.linker.is_some()),
        "{all}"
    );
    assert!(right.records.iter().all(|r| r.built_by == BuiltBy::Hod), "{all}");

    for (name, expression, detail) in [
        (
            "hod-wrong-result",
            "s/add i32 %a, %b/sub i32 %a, %b/",
            "the program of hod's object ended with exit status 38, llc's with exit status 42",
        ),
        (
            "hod-wrong-output",
            "s/i32 2, i32 %r)/i32 2, i32 7)/",
            "the program of hod's object wrote stdout that differs from llc's at line 1: \
             expected \"40 + 2 = 42\\n\", got \"40 + 2 = 7\\n\"",
        ),
        (
            "hod-hangs",
            "s/br label %done/br label %spin/",
            "the program of hod's object timed out after 3 s",
        ),
        ("hod-no-main", "s/@main()/@not_main()/", "hod's object: link with "),
    ] {
        let wrong = run_rewritten(&scratch, major, name, expression);
        let all = wrong.summary(true);
        assert_eq!(wrong.exit_status(), 1, "{name}:\n{all}");
        assert_eq!(wrong.records.len(), right.records.len(), "{name}:\n{all}");
        for record in &wrong.records {
            assert_eq!(record.outcome, Outcome::Fail, "{name}:\n{all}");
            assert!(record.detail.starts_with(detail), "{name}:\n{all}");
        }
    }
}

const ALLOCATES: &str = r#";@ stdout: refused
@.refused = private unnamed_addr constant [8 x i8] c"refused\00"
@.granted = private unnamed_addr constant [8 x i8] c"granted\00"

declare ptr @malloc(i64)
declare i32 @puts(ptr)

define i32 @main() {
  %p = call ptr @malloc(i64 2147483648)
  %none = icmp eq ptr %p, null
  %text = select i1 %none, ptr @.refused, ptr @.granted
  call i32 @puts(ptr %text)
  ret i32 0
}
"#;

// "Child processes", on a real program: the default memory limit of a
// test program (1 GiB of address space) refuses a 2 GiB allocation, and
// the same program is granted it when the limit is raised. A program that
// never ends is an ERROR of the test when it is the oracle's.
#[test]
fn limits_hold_for_real_programs() {
    let Some(majors) = installed() else { return };
    let scratch = Scratch::new("irdiff-real-limits");
    let search = Search::from_env();
    let llvm = tools::find_llvm(Some(&majors[0].to_string()), &search).unwrap().unwrap();
    let test = scratch.join("allocates.ll");
    std::fs::write(&test, ALLOCATES).unwrap();
    let mut config = config(&scratch);
    config.paths = vec![test];
    config.llvm = Some(llvm.major.to_string());
    config.hod = Some(rewriting_hod(&scratch, "hod", &llvm.llc, "s/^$//"));
    let limited = irdiff::run(&config).unwrap();
    assert_eq!(limited.exit_status(), 0, "{}", limited.summary(true));
    assert_eq!(limited.count(Outcome::Pass), limited.records.len());

    config.run_limits.memory_kib = 16 << 20;
    let roomy = irdiff::run(&config).unwrap();
    let all = roomy.summary(true);
    assert_eq!(roomy.exit_status(), 3, "{all}");
    assert!(all.contains("expected \"refused\\n\", got \"granted\\n\""), "{all}");

    let spins = scratch.join("spins.ll");
    std::fs::write(&spins, "define i32 @main() {\n  br label %l\nl:\n  br label %l\n}\n").unwrap();
    config.paths = vec![spins];
    config.run_limits.time = Duration::from_secs(2);
    let report = irdiff::run(&config).unwrap();
    let all = report.summary(true);
    assert_eq!(report.count(Outcome::Error), report.records.len(), "{all}");
    assert!(all.contains("oracle: the program of llc's object timed out after 2 s"), "{all}");
    assert_eq!(report.exit_status(), 3);
}

/// A copy of a test of the seed corpus without its `unsupported`
/// directives, and with `replace`d text: the test for a hod that builds
/// everything.
fn seed_test(scratch: &Scratch, name: &str, replace: &[(&str, &str)]) -> PathBuf {
    let source = irdiff::workspace_root().join("tests/ir").join(name);
    let text = std::fs::read_to_string(&source).unwrap();
    let mut text: String = text
        .lines()
        .filter(|line| !line.starts_with(";@ unsupported"))
        .flat_map(|line| [line, "\n"])
        .collect();
    for (from, to) in replace {
        assert!(text.contains(from), "{name} has no '{from}'");
        text = text.replace(from, to);
    }
    let path = scratch.join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, text).unwrap();
    path
}

/// An LLVM installation directory whose `llc` is the real one without the
/// arguments `dropped`: the oracle the harness would have with another
/// instruction selector.
fn llvm_without(scratch: &Scratch, name: &str, llc: &Path, dropped: &[&str]) -> PathBuf {
    let dir = scratch.join(name);
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    let body = format!(
        "for arg; do\n  shift\n  case \"$arg\" in\n    {}) ;;\n    *) set -- \"$@\" \"$arg\" ;;\n  \
         esac\ndone\nexec '{}' \"$@\"",
        dropped.join("|"),
        llc.display()
    );
    write_script(&dir.join("bin/llc"), &body);
    dir
}

/// A line on this process's stderr that the test harness does not capture:
/// what a passing test has to tell whoever reads the gate's log.
fn report(line: &str) {
    use std::io::Write;
    let _ = writeln!(std::io::stderr(), "{line}");
}

// "Test directives", `input: pre-opt`, with the real tools: the
// coroutine handle operations are what only `opt`'s coroutine passes
// lower, so `llc` alone cannot build the seed, and the oracle that does
// went through `opt`.
#[test]
fn a_pre_opt_seed_goes_through_opt() {
    let Some(majors) = installed() else { return };
    let scratch = Scratch::new("irdiff-pre-opt");
    let with = seed_test(&scratch, "coro/noop.ll", &[]);
    std::fs::create_dir_all(scratch.join("plain")).unwrap();
    let without = scratch.join("plain/noop.ll");
    let text = std::fs::read_to_string(&with).unwrap();
    std::fs::write(&without, text.replace(";@ input: pre-opt\n", "")).unwrap();
    for major in majors {
        let mut config = config(&scratch);
        config.hod = Some(workspace_hod());
        config.llvm = Some(major.to_string());
        config.linkers = vec!["bfd".to_owned()];
        config.paths = vec![with.clone(), without.clone()];
        let report = irdiff::run(&config).unwrap();
        let all = report.summary(true);
        let of = |path: &Path| {
            let name = path.to_string_lossy();
            let record = report.records.iter().find(|r| name.ends_with(&r.test)).unwrap();
            (record.outcome, record.detail.clone())
        };
        assert_eq!(of(&with).0, Outcome::Pass, "LLVM {major}:\n{all}");
        let (outcome, detail) = of(&without);
        assert_eq!(outcome, Outcome::Error, "LLVM {major}:\n{all}");
        // LLVM 19 and 22 abort: "Cannot select: intrinsic %llvm.coro.noop".
        assert!(
            detail.starts_with("oracle: llc ") && detail.contains("llvm.coro."),
            "LLVM {major}: {detail}"
        );
    }
}

const NARROW_LOAD: &str = "selector_narrow_load.ll";
const I1_COMPARE: &str = "selector_i1_compare.ll";

// "The oracle": the reference is `llc -O0` with SelectionDAG because each
// of the other two selectors `llc -O0` can use miscompiles one of the two
// reproducers of spec/interp.md §10.1 on AArch64. A hod that is the
// reference passes both; a hod that is plain `llc -O0` (GlobalISel on
// AArch64) fails the narrow load, and one that is `llc -O0 -global-isel=0`
// (FastISel) fails the i1 compare. When an LLVM no longer does, the test
// says so and passes: the workaround can then be reconsidered.
#[test]
fn the_reference_is_not_plain_o0() {
    let Some(majors) = installed() else { return };
    let scratch = Scratch::new("irdiff-reference");
    let search = Search::from_env();
    let tests = [seed_test(&scratch, NARROW_LOAD, &[]), seed_test(&scratch, I1_COMPARE, &[])];
    for major in majors {
        let llvm = tools::find_llvm(Some(&major.to_string()), &search).unwrap().unwrap();
        let run = |name: &str, extra: &str| {
            let mut config = config(&scratch);
            config.paths = tests.to_vec();
            config.llvm = Some(major.to_string());
            config.hod = Some(selecting_hod(&scratch, name, &llvm.llc, "s/^$//", extra));
            irdiff::run(&config).unwrap()
        };
        let of = |report: &Report, test: &str| -> Vec<(Outcome, String)> {
            let records = report.records.iter().filter(|record| record.test.ends_with(test));
            records.map(|record| (record.outcome, record.detail.clone())).collect()
        };

        // The reference agrees with the directives and with itself.
        let reference = run("hod-reference", &engine::REFERENCE_SELECTOR.join(" "));
        let all = reference.summary(true);
        assert_eq!(reference.exit_status(), 0, "LLVM {major}:\n{all}");
        assert_eq!(reference.count(Outcome::Pass), reference.records.len(), "{all}");
        let linkers = of(&reference, NARROW_LOAD).len();
        assert!(linkers >= 1 && reference.records.len() == 2 * linkers, "{all}");

        // The other two selectors, each on the reproducer it gets wrong.
        for (name, extra, selector, wrong, got) in [
            ("hod-plain", "", "GlobalISel (plain llc -O0)", NARROW_LOAD, "-5408"),
            ("hod-fast", "-global-isel=0", "FastISel (llc -O0 -global-isel=0)", I1_COMPARE, "1"),
        ] {
            let other = run(name, extra);
            let all = other.summary(true);
            assert_eq!(other.count(Outcome::Error), 0, "LLVM {major}, {name}:\n{all}");
            let records = of(&other, wrong);
            assert_eq!(records.len(), linkers, "{all}");
            if Arch::host() != Some(Arch::Aarch64) {
                // The two bugs are AArch64's; elsewhere every selector of
                // `llc -O0` is expected to be right about these programs.
                assert_eq!(other.count(Outcome::Pass), other.records.len(), "{all}");
            } else if records.iter().all(|(outcome, _)| *outcome == Outcome::Pass) {
                report(&format!(
                    "note: LLVM {major}: {selector} no longer miscompiles tests/ir/{wrong} on \
                     AArch64; the oracle's '-global-isel=0 -fast-isel=0' can be reconsidered \
                     (spec/harness.md \"irdiff\", \"The oracle\")"
                ));
            } else {
                let expected = printed(wrong);
                let detail = format!(
                    "the program of hod's object wrote stdout that differs from llc's at line \
                     1: expected \"{expected}\\n\", got \"{got}\\n\""
                );
                assert_eq!(records, vec![(Outcome::Fail, detail); linkers], "{all}");
                assert_eq!(other.exit_status(), 1, "{all}");
            }
        }
    }
}

/// What the reproducer `test` prints, without the newline.
fn printed(test: &str) -> &'static str {
    if test == NARROW_LOAD { "60128" } else { "0" }
}

// "The witness", on real tools: the seed corpus is confirmed by `llc -O2`
// with every LLVM (what the full gate runs), and an oracle that is plain
// `llc -O0` is caught on the narrow load even when the test's directives
// say what that oracle prints, which is how such a program would have
// entered the corpus.
#[test]
fn the_witness_confirms_the_reference_and_catches_plain_o0() {
    let Some(majors) = installed() else { return };
    let scratch = Scratch::new("irdiff-witness");
    let search = Search::from_env();
    for major in majors {
        let mut seed = config(&scratch);
        seed.hod = Some(workspace_hod());
        seed.llvm = Some(major.to_string());
        seed.witness = true;
        let report_of_seed = irdiff::run(&seed).unwrap();
        let all = report_of_seed.summary(true);
        assert_eq!(report_of_seed.exit_status(), 0, "LLVM {major}:\n{all}");
        assert_eq!(report_of_seed.tests(), corpus_size(), "{all}");
        assert!(all.contains("; witness"), "{all}");

        // The same oracle without its selector arguments, and a test that
        // was written from what that oracle's program prints.
        let llvm = tools::find_llvm(Some(&major.to_string()), &search).unwrap().unwrap();
        let plain = llvm_without(
            &scratch,
            &format!("plain-llvm-{major}"),
            &llvm.llc,
            &engine::REFERENCE_SELECTOR,
        );
        let wrong = seed_test(&scratch, NARROW_LOAD, &[(";@ stdout: 60128", ";@ stdout: -5408")]);
        let mut config = config(&scratch);
        config.paths = vec![wrong];
        config.llvm = Some(plain.to_str().unwrap().to_owned());
        config.hod = Some(rewriting_hod(&scratch, "hod-plain", &llvm.llc, "s/^$//"));
        let unwitnessed = irdiff::run(&config).unwrap();
        config.witness = true;
        let witnessed = irdiff::run(&config).unwrap();
        let all = format!("{}{}", unwitnessed.summary(true), witnessed.summary(true));
        assert!(!witnessed.records.is_empty(), "{all}");
        assert_eq!(witnessed.records.len(), unwitnessed.records.len(), "{all}");
        if unwitnessed.count(Outcome::Pass) == unwitnessed.records.len() {
            // Plain `-O0` prints -5408 and hod agrees with it: without the
            // witness the wrong program is the expectation.
            assert_eq!(Arch::host(), Some(Arch::Aarch64), "{all}");
            let detail = "witness: the program of llc -O2's object wrote stdout that differs \
                          from llc -O0's at line 1: expected \"-5408\\n\", got \"60128\\n\"";
            for record in &witnessed.records {
                assert_eq!((record.outcome, record.detail.as_str()), (Outcome::Error, detail));
            }
            assert_eq!(witnessed.exit_status(), 3, "{all}");
        } else {
            // This `llc -O0` prints 60128: the test is the wrong one, with
            // or without the witness.
            for record in unwitnessed.records.iter().chain(&witnessed.records) {
                assert_eq!(record.outcome, Outcome::Error, "{all}");
                assert!(record.detail.starts_with("oracle: the program of llc's object"), "{all}");
            }
            if Arch::host() == Some(Arch::Aarch64) {
                report(&format!(
                    "note: LLVM {major}: plain llc -O0 no longer miscompiles \
                     tests/ir/{NARROW_LOAD} on AArch64; the oracle's '-global-isel=0 \
                     -fast-isel=0' can be reconsidered (spec/harness.md \"irdiff\", \"The \
                     oracle\")"
                ));
            }
        }
    }
}

const SECTIONS: &str = r#";@ llc-args: -code-model=small -data-sections -debugger-tune=gdb -fp-contract=off -function-sections -mcpu=generic -x86-branches-within-32B-boundaries
;@ stdout: 42
@value = global i32 40
@.fmt = private unnamed_addr constant [4 x i8] c"%d\0A\00"

declare i32 @printf(ptr, ...)

define i32 @sum_of_two(i32 %a, i32 %b) {
  %r = add i32 %a, %b
  ret i32 %r
}

define i32 @main() {
  %v = load i32, ptr @value
  %r = call i32 @sum_of_two(i32 %v, i32 2)
  call i32 (ptr, ...) @printf(ptr @.fmt, i32 %r)
  ret i32 0
}
"#;

/// Whether the file has `needle` in it.
fn contains(path: &Path, needle: &str) -> bool {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    bytes.windows(needle.len()).any(|window| window == needle.as_bytes())
}

// "Test directives", `llc-args`: the real `llc` takes every argument of
// the allow-list, and the oracle, the witness and hod all get them: each
// object has a section per function and per variable.
#[test]
fn llc_args_reach_every_compiler() {
    let Some(majors) = installed() else { return };
    let scratch = Scratch::new("irdiff-llc-args");
    let search = Search::from_env();
    for major in majors {
        let llvm = tools::find_llvm(Some(&major.to_string()), &search).unwrap().unwrap();
        let test = scratch.join("sections.ll");
        std::fs::write(&test, SECTIONS).unwrap();
        let mut config = config(&scratch);
        config.paths = vec![test.clone()];
        config.llvm = Some(major.to_string());
        config.witness = true;
        config.hod = Some(rewriting_hod(&scratch, "hod-right", &llvm.llc, "s/^$//"));
        let right = irdiff::run(&config).unwrap();
        let all = right.summary(true);
        assert_eq!(right.exit_status(), 0, "LLVM {major}:\n{all}");
        assert!(right.count(Outcome::Pass) == right.records.len() && !right.records.is_empty());

        // A failing run keeps its files: the three objects.
        config.keep = true;
        config.hod = Some(rewriting_hod(&scratch, "hod-wrong", &llvm.llc, "s/i32 2)/i32 3)/"));
        let wrong = irdiff::run(&config).unwrap();
        let all = wrong.summary(true);
        assert_eq!(wrong.count(Outcome::Fail), wrong.records.len(), "{all}");
        let kept = wrong.kept.clone().expect("a failing test is kept");
        let dir = std::fs::read_dir(&kept)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path.to_string_lossy().ends_with("sections.ll"))
            .expect("the test's directory is kept");
        for object in ["oracle.o", "witness.o", "hod.o"] {
            for section in [".text.sum_of_two", ".text.main", ".data.value"] {
                assert!(contains(&dir.join(object), section), "{object} has no {section}");
            }
        }
        std::fs::remove_dir_all(&kept).unwrap();

        // Without the directive there is one `.text`.
        let plain = scratch.join("plain.ll");
        std::fs::write(&plain, SECTIONS.split_once('\n').unwrap().1).unwrap();
        config.paths = vec![plain];
        let wrong = irdiff::run(&config).unwrap();
        assert_eq!(wrong.count(Outcome::Fail), wrong.records.len(), "{}", wrong.summary(true));
        let kept = wrong.kept.clone().expect("a failing test is kept");
        let dir = std::fs::read_dir(&kept)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path.to_string_lossy().ends_with("plain.ll"))
            .expect("the test's directory is kept");
        for object in ["oracle.o", "witness.o", "hod.o"] {
            assert!(!contains(&dir.join(object), ".text.sum_of_two"), "{object}");
        }
        std::fs::remove_dir_all(&kept).unwrap();
    }
}

const WARNS: &str = r#";@ stdout-no-newline: named
@.s = private unnamed_addr constant [6 x i8] c"named\00"
@buffer = global [64 x i8] zeroinitializer

declare ptr @tmpnam(ptr)
declare i32 @printf(ptr, ...)

define i32 @main() {
  %p = call ptr @tmpnam(ptr @buffer)
  call i32 (ptr, ...) @printf(ptr @.s)
  ret i32 0
}
"#;

// "Test directives", `link: allow-warnings` and `stdout-no-newline`, on a
// real program: the C library warns at link time about `tmpnam`, which
// `ld.bfd` reports and `--fatal-warnings` makes an error of the oracle's
// side; with the directive every linker links it, and the output without
// a final newline is compared exactly.
#[test]
fn a_link_that_warns_and_an_output_without_a_newline() {
    let Some(majors) = installed() else { return };
    let scratch = Scratch::new("irdiff-warnings");
    let search = Search::from_env();
    let llvm = tools::find_llvm(Some(&majors[0].to_string()), &search).unwrap().unwrap();
    let mut config = config(&scratch);
    config.llvm = Some(llvm.major.to_string());
    config.hod = Some(rewriting_hod(&scratch, "hod", &llvm.llc, "s/^$//"));

    let strict = scratch.join("strict.ll");
    std::fs::write(&strict, WARNS).unwrap();
    config.paths = vec![strict];
    let report = irdiff::run(&config).unwrap();
    let all = report.summary(true);
    let bfd: Vec<_> =
        report.records.iter().filter(|record| record.linker.as_deref() == Some("bfd")).collect();
    for record in &bfd {
        assert_eq!(record.outcome, Outcome::Error, "{all}");
        // The detail is clipped before the warning (the paths are long):
        // that lld and mold link the same object is what shows it is one.
        assert!(record.detail.starts_with("oracle: link with bfd failed"), "{all}");
    }
    let others = report.records.iter().filter(|record| record.linker.as_deref() != Some("bfd"));
    for record in others {
        assert_eq!(record.outcome, Outcome::Pass, "{all}");
    }
    if bfd.is_empty() {
        eprintln!("note: ld.bfd not found; the link that warns is not shown to fail");
    }

    let tolerant = scratch.join("tolerant.ll");
    std::fs::write(&tolerant, format!(";@ link: allow-warnings\n{WARNS}")).unwrap();
    config.paths = vec![tolerant.clone()];
    let report = irdiff::run(&config).unwrap();
    let all = report.summary(true);
    assert_eq!(report.exit_status(), 0, "{all}");
    assert!(report.count(Outcome::Pass) == report.records.len() && !report.records.is_empty());

    // The output is compared to the last byte.
    let text = format!(
        ";@ link: allow-warnings\n{}",
        WARNS.replace("stdout-no-newline: named", "stdout: named")
    );
    std::fs::write(&tolerant, text).unwrap();
    let report = irdiff::run(&config).unwrap();
    let all = report.summary(true);
    assert_eq!(report.count(Outcome::Error), report.records.len(), "{all}");
    assert!(
        all.contains(
            "oracle: the program of llc's object wrote stdout that differs from the directives"
        ),
        "{all}"
    );
}

// "Several modules", with the real tools: every module of
// `tests/ir/two_modules.ll` is compiled by the oracle, the witness and a
// hod that builds everything, the objects are linked in the test's order
// with every linker, and a hod that miscompiles one module is a FAIL.
#[test]
fn a_real_program_of_two_modules() {
    let Some(majors) = installed() else { return };
    let scratch = Scratch::new("irdiff-modules");
    let search = Search::from_env();
    let test = seed_test(&scratch, "two_modules.ll", &[]);
    for major in majors {
        let llvm = tools::find_llvm(Some(&major.to_string()), &search).unwrap().unwrap();
        let mut config = config(&scratch);
        config.paths = vec![test.clone()];
        config.llvm = Some(major.to_string());
        config.witness = true;
        config.hod = Some(rewriting_hod(&scratch, "hod-right", &llvm.llc, "s/^$//"));
        let right = irdiff::run(&config).unwrap();
        let all = right.summary(true);
        assert_eq!(right.exit_status(), 0, "LLVM {major}:\n{all}");
        assert!(right.count(Outcome::Pass) == right.records.len() && !right.records.is_empty());
        assert!(right.records.iter().all(|r| r.built_by == BuiltBy::Hod && r.linker.is_some()));
        // Coverage counts tests, not modules ("Summary").
        assert!(all.contains("hod built 1 of 1 objects"), "{all}");

        // A miscompile in the module that is not `main`'s.
        config.hod =
            Some(rewriting_hod(&scratch, "hod-wrong", &llvm.llc, "s/ret i32 202/ret i32 203/"));
        let wrong = irdiff::run(&config).unwrap();
        let all = wrong.summary(true);
        assert_eq!(wrong.exit_status(), 1, "{all}");
        assert_eq!(wrong.records.len(), right.records.len(), "{all}");
        for record in &wrong.records {
            assert_eq!(record.outcome, Outcome::Fail, "{all}");
            assert_eq!(
                record.detail,
                "the program of hod's object wrote stdout that differs from llc's at line 4: \
                 expected \"local=101 other=202\\n\", got \"local=101 other=203\\n\"",
                "{all}"
            );
        }

        // Hybrid: the real hod builds what it can and the reference the
        // rest, module by module.
        config.hod = Some(workspace_hod());
        config.mode = Mode::Hybrid;
        let hybrid = irdiff::run(&config).unwrap();
        let all = hybrid.summary(true);
        assert_eq!(hybrid.exit_status(), 0, "{all}");
        assert_eq!(hybrid.count(Outcome::Pass), hybrid.records.len(), "{all}");
    }
}
