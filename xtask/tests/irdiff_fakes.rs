//! The outcome logic of `cargo xtask irdiff` (`spec/harness.md` "irdiff"),
//! exercised with fake tools: an `llc`, a `hod`, a linker driver and
//! linkers that are shell scripts, so every outcome can be provoked on any
//! Linux machine, without LLVM.
//!
//! How the fakes work. A "module" is any text. The fake compilers write an
//! "object" that is a first line `built-by llc` or `built-by hod`, a second
//! line with their code-generation arguments, and the module. The fake
//! linkers turn an object into a "program", a shell script made of the
//! module's `; program: <command>` lines; `; program(hod): ...` is kept
//! only for an object hod built, `; program(llc): ...` only for one `llc`
//! built, `; program(mold): ...` only by that linker, and
//! `; program(hod,mold): ...` needs both. Other comment lines make a tool
//! misbehave: `; llc: crash|hang|fail|silent`,
//! `; hod: decline <reason>|crash|error|hang|nolog|noobject|hog`,
//! `; link-fail: <builder> <linker>`.
#![cfg(target_os = "linux")]

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::Scratch;
use hod_testutil::write_script;
use xtask::irdiff::{
    self, Arch, BuiltBy, Config, Limits, Mode, Outcome, Report, Search, TierFilter,
};

const FAKE_LLC: &str = r#"PATH=/usr/bin:/bin
case "$0" in *-19) major=19 ;; *) major=22 ;; esac
if [ "$1" = --version ]; then echo "fake LLVM version $major.0.0"; exit 0; fi
out= in= args=
while [ $# -gt 0 ]; do
  case "$1" in
    -o) out=$2; shift ;;
    -*) args="$args $1" ;;
    *) in=$1 ;;
  esac
  shift
done
if grep -q '^; llc: crash' "$in"; then kill -SEGV $$; fi
if grep -q '^; llc: hang' "$in"; then exec sleep 60; fi
if grep -q '^; llc: fail' "$in"; then echo "llc: error: bad module" >&2; exit 1; fi
if grep -q '^; llc: silent' "$in"; then exit 0; fi
{ echo "built-by llc"; echo "args$args"; cat "$in"; } > "$out""#;

/// Refuses to run with any of hod's configuration variables set, or with
/// both or neither of `--strict` and `--fallback`: the harness configures
/// it by options alone.
const FAKE_HOD: &str = r#"PATH=/usr/bin:/bin
for name in HOD_FALLBACK_LLC HOD_STRICT HOD_COVERAGE_LOG HOD_IN_FALLBACK; do
  if printenv "$name" > /dev/null; then echo "fake hod: $name is set" >&2; exit 1; fi
done
log= strict= fallback= out= in= args=
while [ $# -gt 0 ]; do
  case "$1" in
    --coverage-log=*) log=${1#--coverage-log=} ;;
    --strict) strict=1 ;;
    --fallback=*) fallback=${1#--fallback=} ;;
    -o) out=$2; shift ;;
    -*) args="$args $1" ;;
    *) in=$1 ;;
  esac
  shift
done
if [ -z "$log" ]; then echo "fake hod: no coverage log" >&2; exit 1; fi
if [ -n "$strict" ] && [ -n "$fallback" ]; then echo "fake hod: both modes" >&2; exit 1; fi
if [ -z "$strict" ] && [ -z "$fallback" ]; then echo "fake hod: no mode" >&2; exit 1; fi
record() { printf '%s\t%s\ttext\t1\t%s\t-\t5\t-\n' "$1" "$2" "$out" >> "$log"; }
reason=$(sed -n 's/^; hod: decline //p' "$in")
if [ -n "$reason" ]; then
  if [ -n "$fallback" ]; then
    "$fallback" $args -o "$out" "$in" || exit 1
    record fallback "$reason"
    exit 0
  fi
  record error "$reason"
  echo "hod: error: unsupported: $reason" >&2
  exit 2
fi
if grep -q '^; hod: crash' "$in"; then kill -SEGV $$; fi
if grep -q '^; hod: hang' "$in"; then exec sleep 60; fi
if grep -q '^; hod: error' "$in"; then
  record error malformed-input
  echo "hod: error: the module is not valid" >&2
  exit 1
fi
if grep -q '^; hod: hog' "$in"; then
  if ! dd if=/dev/zero of=/dev/null bs=268435456 count=1 2> /dev/null; then
    record error internal
    echo "hod: error: out of memory" >&2
    exit 1
  fi
fi
if grep -q '^; hod: noobject' "$in"; then record hod -; exit 0; fi
{ echo "built-by hod"; echo "args$args"; cat "$in"; } > "$out"
if grep -q '^; hod: nolog' "$in"; then exit 0; fi
record hod -"#;

const FAKE_DRIVER: &str = r#"dir=
for arg in "$@"; do case "$arg" in -B*) dir=${arg#-B} ;; esac; done
exec "$dir/ld" "$@""#;

/// A driver that ignores the linker it is given and always uses GNU ld's
/// stand-in.
const DEAF_DRIVER: &str = r#"exec "$(dirname "$0")/ld.bfd" "$@""#;

/// The fake linker called `name`. Its programs carry the signature the
/// real linker leaves, and the variables `PIE`, `LIBS`, `LINKER_FLAGS` and
/// `CODEGEN` for the module's commands to print.
fn fake_linker(name: &str) -> String {
    let signature = match name {
        "lld" => "echo '# Linker: LLD 22.0.0'",
        "mold" => "echo '# mold 2.0.0 (compatible with GNU ld)'",
        _ => ":",
    };
    format!(
        r#"PATH=/usr/bin:/bin
me={name}
out= obj= pie= libs= flags=
while [ $# -gt 0 ]; do
  case "$1" in
    -o) out=$2; shift ;;
    -B*) ;;
    -pie|-no-pie) pie=$1 ;;
    -l*) libs="$libs $1" ;;
    -*) flags="$flags $1" ;;
    *) obj=$1 ;;
  esac
  shift
done
builder=$(sed -n '1s/^built-by //p' "$obj")
if grep -q "^; link-fail: $builder $me\$" "$obj"; then
  echo "$me: error: undefined symbol: main" >&2
  exit 1
fi
{{
  echo '#!/bin/sh'
  {signature}
  echo "PATH=/usr/bin:/bin PIE='$pie' LIBS='$libs' LINKER_FLAGS='$flags'"
  echo "CODEGEN='$(sed -n '2s/^args //p' "$obj")'"
  sed -n -e 's/^; program: //p' -e "s/^; program($builder): //p" \
    -e "s/^; program($me): //p" -e "s/^; program($builder,$me): //p" "$obj"
}} > "$out"
chmod 755 "$out""#
    )
}

/// A machine made of fakes: a directory of tools the harness finds, a fake
/// hod, and a directory of tests.
struct World {
    scratch: Scratch,
}

impl World {
    /// LLVM 22, the driver, and the three required linkers.
    fn new(label: &str) -> Self {
        let world = Self { scratch: Scratch::new(label) };
        for dir in ["bin", "tests", "build"] {
            std::fs::create_dir(world.scratch.join(dir)).unwrap();
        }
        world.tool("llc-22", FAKE_LLC);
        world.tool("gcc", FAKE_DRIVER);
        world.tool("ld.bfd", &fake_linker("bfd"));
        world.tool("ld.lld-22", &fake_linker("lld"));
        world.tool("mold", &fake_linker("mold"));
        write_script(&world.hod(), FAKE_HOD);
        world
    }

    fn tool(&self, name: &str, body: &str) {
        write_script(&self.scratch.join("bin").join(name), body);
    }

    fn without(&self, name: &str) {
        std::fs::remove_file(self.scratch.join("bin").join(name)).unwrap();
    }

    fn hod(&self) -> PathBuf {
        self.scratch.join("hod")
    }

    fn build(&self) -> PathBuf {
        self.scratch.join("build")
    }

    /// Adds a test.
    fn test(&self, name: &str, text: &str) -> PathBuf {
        let path = self.scratch.join("tests").join(name);
        std::fs::write(&path, text).unwrap();
        path
    }

    /// The command's defaults, on this world.
    fn config(&self) -> Config {
        let mut config = Config::new(self.build());
        config.paths = vec![self.scratch.join("tests")];
        config.hod = Some(self.hod());
        config.search =
            Search { path: self.scratch.join("bin").into(), llvm_root: self.scratch.join("none") };
        config
    }

    fn run(&self) -> Report {
        irdiff::run(&self.config()).unwrap()
    }
}

/// Short limits for tests that make something hang.
fn short(mut config: Config) -> Config {
    config.tool_limits.time = Duration::from_secs(3);
    config.run_limits.time = Duration::from_secs(3);
    config
}

fn file_name(test: &str) -> &str {
    test.rsplit('/').next().unwrap()
}

/// Every record as `OUTCOME file linker`, in report order.
fn rows(report: &Report) -> Vec<String> {
    report
        .records
        .iter()
        .map(|record| {
            format!(
                "{} {} {}",
                record.outcome.name(),
                file_name(&record.test),
                record.linker.as_deref().unwrap_or("-")
            )
        })
        .collect()
}

/// The details of the records of one test, in linker order.
fn details<'a>(report: &'a Report, file: &str) -> Vec<&'a str> {
    report
        .records
        .iter()
        .filter(|record| file_name(&record.test) == file)
        .map(|record| record.detail.as_str())
        .collect()
}

/// The outcomes of the records of one test, in linker order.
fn outcomes(report: &Report, file: &str) -> Vec<Outcome> {
    report
        .records
        .iter()
        .filter(|record| file_name(&record.test) == file)
        .map(|record| record.outcome)
        .collect()
}

fn host() -> Arch {
    Arch::host().expect("these tests run on Linux, on aarch64 or x86_64")
}

fn other_arch() -> Arch {
    if host() == Arch::Aarch64 { Arch::X86_64 } else { Arch::Aarch64 }
}

/// What is left under `<build>/irdiff/`.
fn left_behind(world: &World) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(world.build().join("irdiff"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

// spec/harness.md "irdiff", "What a test does" and "Outcomes": PASS. Both
// compilers get the same code-generation arguments, each object is linked
// by every linker of the matrix (and by that linker), and nothing is left
// behind but the results file.
#[test]
fn pass_with_every_linker() {
    let world = World::new("irdiff-pass");
    let triple = host().triple();
    world.test(
        "pass.ll",
        &format!(
            ";@ covers: spec/harness.md irdiff\n\
             ;@ stdout: hello\n\
             ;@ stdout: -O0 -filetype=obj -mtriple={triple} -relocation-model=pic\n\
             ;@ stdout: -pie [] one thread\n\
             ; program: echo hello\n\
             ; program: echo \"$CODEGEN\"\n\
             ; program(bfd): [ \"$LINKER_FLAGS\" = \" -Wl,--fatal-warnings\" ] && threads=one\n\
             ; program(lld): case \"$LINKER_FLAGS\" in *' -Wl,--threads=1') threads=one ;; esac\n\
             ; program(mold): case \"$LINKER_FLAGS\" in *' -Wl,--thread-count=1') threads=one ;; esac\n\
             ; program: echo \"$PIE [$LIBS] $threads thread\"\n"
        ),
    );
    world.test(
        "needs.ll",
        ";@ link: c++ m no-pie\n;@ args: one two\n;@ stdin: fed\n;@ exit: 5\n\
         ;@ stdout: -no-pie [ -lstdc++ -lm] static\n;@ stdout: 2 one two\n;@ stdout: fed\n\
         ; program: echo \"$PIE [$LIBS] ${CODEGEN##*-relocation-model=}\"\n\
         ; program: echo \"$# $*\"\n; program: cat\n; program: exit 5\n",
    );
    let report = world.run();
    assert_eq!(
        rows(&report),
        [
            "PASS needs.ll bfd",
            "PASS needs.ll lld",
            "PASS needs.ll mold",
            "PASS pass.ll bfd",
            "PASS pass.ll lld",
            "PASS pass.ll mold"
        ],
        "{}",
        report.summary(true)
    );
    assert!(report.records.iter().all(|r| r.built_by == BuiltBy::Hod && r.reason.is_none()));
    assert!(report.records.iter().all(|r| r.llvm == Some(22) && r.target == triple));
    assert_eq!(report.records[3].covers.as_deref(), Some("spec/harness.md irdiff"));
    assert_eq!(report.exit_status(), 0);
    assert_eq!((report.tests(), report.count(Outcome::Pass)), (2, 6));
    // wild is not installed here: a note, not a failure.
    assert_eq!(report.notes, ["linker 'wild' not found; not in this run"]);
    assert!(report.missing_tools.is_empty());
    let summary = report.summary(false);
    assert!(summary.contains("; linkers bfd lld mold; strict; -j "), "{summary}");
    assert!(summary.contains("irdiff: 2 tests, 6 results: 6 PASS, 0 FAIL, 0 UNSUPPORTED,"));
    assert!(summary.contains("irdiff: coverage: hod built 2 of 2 objects\n"), "{summary}");
    assert!(!summary.contains("PASS        "), "passes are listed only with --verbose");
    assert!(report.summary(true).contains("pass.ll [bfd lld mold]\n"));

    // "Results file" and "Work directory".
    let results = format!("results-{}-llvm22.tsv", host().name());
    assert_eq!(report.results_file, Some(world.build().join("irdiff").join(&results)));
    assert_eq!(left_behind(&world), [results]);
    let text = std::fs::read_to_string(report.results_file.as_ref().unwrap()).unwrap();
    assert_eq!(text, report.results_text());
    assert_eq!(text.lines().count(), 6);
    let pass = world.scratch.join("tests/pass.ll");
    assert_eq!(
        text.lines().nth(3).unwrap(),
        format!("PASS\t{}\t{triple}\t22\tbfd\thod\t-\t-\tspec/harness.md irdiff", pass.display())
    );
}

// "Outcomes": FAIL when the programs write different output, or end
// differently.
#[test]
fn fail_on_a_difference() {
    let world = World::new("irdiff-fail");
    world.test(
        "stdout.ll",
        ";@ stdout: one\n;@ stdout: right\n\
         ; program: echo one\n; program(llc): echo right\n; program(hod): echo wrong\n",
    );
    world.test("status.ll", "; program(hod): exit 4\n");
    world.test("signal.ll", "; program(hod): kill -SEGV $$\n");
    world.test("extra.ll", ";@ stdout: a\n; program: echo a\n; program(hod): echo b\n");
    world.test("stderr.ll", "; program(hod): echo only on stderr >&2\n");
    let report = world.run();
    assert_eq!(outcomes(&report, "stdout.ll"), [Outcome::Fail; 3], "{}", report.summary(true));
    assert_eq!(
        details(&report, "stdout.ll")[0],
        "the program of hod's object wrote stdout that differs from llc's at line 2: \
         expected \"right\\n\", got \"wrong\\n\""
    );
    assert_eq!(
        details(&report, "status.ll"),
        ["the program of hod's object ended with exit status 4, llc's with exit status 0"; 3]
    );
    assert_eq!(
        details(&report, "signal.ll"),
        ["the program of hod's object ended with signal 11, llc's with exit status 0"; 3]
    );
    assert_eq!(
        details(&report, "extra.ll")[2],
        "the program of hod's object wrote stdout that differs from llc's at line 2: \
         expected the end of the output, got \"b\\n\""
    );
    // Standard error is not compared.
    assert_eq!(outcomes(&report, "stderr.ll"), [Outcome::Pass; 3]);
    assert_eq!(report.exit_status(), 1);
    assert_eq!((report.count(Outcome::Fail), report.count(Outcome::Error)), (12, 0));
    let summary = report.summary(false);
    assert!(summary.contains("status.ll [bfd lld mold]: the program of hod's object ended"));
}

// "Outcomes": one record per linker, so a failure with one linker only is
// a FAIL for that linker and a PASS for the others.
#[test]
fn fail_with_one_linker_only() {
    let world = World::new("irdiff-one-linker");
    world
        .test("runs.ll", ";@ stdout: ok\n; program: echo ok\n; program(hod,mold): echo and more\n");
    world.test("links.ll", ";@ stdout: ok\n; program: echo ok\n; link-fail: hod lld\n");
    let report = world.run();
    assert_eq!(
        rows(&report),
        [
            "PASS links.ll bfd",
            "FAIL links.ll lld",
            "PASS links.ll mold",
            "PASS runs.ll bfd",
            "PASS runs.ll lld",
            "FAIL runs.ll mold"
        ],
        "{}",
        report.summary(true)
    );
    assert_eq!(
        details(&report, "links.ll")[1],
        "hod's object: link with lld failed (exit status 1): lld: error: undefined symbol: main"
    );
    assert_eq!(report.exit_status(), 1);
    let summary = report.summary(false);
    assert!(summary.contains("FAIL        ") && summary.contains("runs.ll [mold]: the program"));
}

// "Outcomes": hod's own failures are FAIL: it crashes, reports an error,
// times out, writes no object or no coverage log, or runs out of memory
// under the tool limit.
#[test]
fn fail_when_hod_fails() {
    let world = World::new("irdiff-hod-fails");
    for what in ["crash", "error", "hang", "nolog", "noobject", "hog"] {
        world.test(&format!("{what}.ll"), &format!("; hod: {what}\n"));
    }
    let mut config = short(world.config());
    config.tool_limits.memory_kib = 64 << 10;
    let started = Instant::now();
    let report = irdiff::run(&config).unwrap();
    assert!(started.elapsed() < Duration::from_secs(30), "{:?}", started.elapsed());
    for (file, needle) in [
        ("crash.ll", "hod was terminated by signal 11; it wrote no coverage log"),
        (
            "error.ll",
            "hod failed (exit status 1): hod: error: the module is not valid; \
             coverage log: outcome 'error', reason 'malformed-input'",
        ),
        ("hang.ll", "hod timed out after 3 s; it wrote no coverage log"),
        ("nolog.ll", "hod exited with status 0; it wrote no coverage log"),
        ("noobject.ll", "hod exited with status 0 and wrote no object"),
        ("hog.ll", "hod failed (exit status 1): hod: error: out of memory"),
    ] {
        assert_eq!(outcomes(&report, file), [Outcome::Fail; 3], "{file}: {}", report.summary(true));
        assert!(details(&report, file)[0].starts_with(needle), "{:?}", details(&report, file));
    }
    assert!(report.records.iter().all(|record| record.built_by == BuiltBy::Nobody));
    assert_eq!(report.exit_status(), 1);

    // With room, the hog is an ordinary pass: it was the limit.
    let roomy = World::new("irdiff-hod-roomy");
    roomy.test("hog.ll", "; hod: hog\n");
    assert_eq!(outcomes(&roomy.run(), "hog.ll"), [Outcome::Pass; 3]);
}

// "Outcomes": UNSUPPORTED is hod declining with the reason the test
// expects. A decline the test does not expect, a different reason, and a
// module hod compiles although the test says it cannot, are FAIL.
#[test]
fn unsupported_expected_and_unexpected() {
    let world = World::new("irdiff-unsupported");
    let (here, elsewhere) = (host().name(), other_arch().name());
    world.test(
        "expected.ll",
        ";@ unsupported: codegen-not-implemented\n; hod: decline codegen-not-implemented\n",
    );
    world.test("unexpected.ll", "; hod: decline codegen-not-implemented\n");
    world.test("reason.ll", ";@ unsupported: vector-type\n; hod: decline inline-asm\n");
    world.test("stale.ll", ";@ unsupported: codegen-not-implemented\n");
    world.test(
        "conditional.ll",
        &format!(
            ";@ unsupported({elsewhere}): never-here\n;@ unsupported({here},llvm22): now\n\
             ;@ unsupported: later\n; hod: decline now\n"
        ),
    );
    world.test("elsewhere.ll", &format!(";@ unsupported({elsewhere}): not-on-this-target\n"));
    // An `xfail` does not excuse the directive being wrong.
    world.test("xfailed.ll", ";@ xfail: known\n; hod: decline codegen-not-implemented\n");
    // The oracle's side is checked even when hod declines.
    world.test(
        "oracle.ll",
        ";@ unsupported: codegen-not-implemented\n;@ stdout: expected\n\
         ; hod: decline codegen-not-implemented\n; program: echo actual\n",
    );
    let report = world.run();
    let all = report.summary(true);
    assert_eq!(outcomes(&report, "expected.ll"), [Outcome::Unsupported; 3], "{all}");
    assert_eq!(outcomes(&report, "conditional.ll"), [Outcome::Unsupported; 3], "{all}");
    assert_eq!(outcomes(&report, "elsewhere.ll"), [Outcome::Pass; 3], "{all}");
    let declined = &report.records.iter().find(|r| file_name(&r.test) == "expected.ll").unwrap();
    assert_eq!(declined.built_by, BuiltBy::Declined);
    assert_eq!(declined.reason.as_deref(), Some("codegen-not-implemented"));
    assert_eq!(declined.detail, "");
    for (file, detail) in [
        (
            "unexpected.ll",
            "hod declined the module and the test has no \
             'unsupported: codegen-not-implemented' directive",
        ),
        (
            "reason.ll",
            "hod declined with reason 'inline-asm'; the 'unsupported' directive says 'vector-type'",
        ),
        (
            "stale.ll",
            "hod compiled this test; remove its 'unsupported: codegen-not-implemented' directive",
        ),
        (
            "xfailed.ll",
            "hod declined the module and the test has no \
             'unsupported: codegen-not-implemented' directive",
        ),
    ] {
        assert_eq!(outcomes(&report, file), [Outcome::Fail; 3], "{file}: {all}");
        assert_eq!(details(&report, file), [detail; 3]);
    }
    assert_eq!(outcomes(&report, "oracle.ll"), [Outcome::Error; 3], "{all}");
    assert!(all.contains("UNSUPPORTED "), "{all}");
    assert!(all.contains("expected.ll [bfd lld mold]: codegen-not-implemented\n"), "{all}");
    assert!(
        all.contains(
            "irdiff: coverage: hod built 2 of 8 objects; declined codegen-not-implemented: 4; \
             declined inline-asm: 1; declined now: 1\n"
        ),
        "{all}"
    );
    // Not listed without --verbose; the failures are.
    let brief = report.summary(false);
    assert!(!brief.contains("/expected.ll") && brief.contains("/unexpected.ll"), "{brief}");
}

// "Outcomes": XFAIL is an expected failure, XPASS a test that passes
// although it says it fails; the conditions of `xfail` select the linker.
#[test]
fn xfail_and_xpass() {
    let world = World::new("irdiff-xfail");
    world.test("fails.ll", ";@ xfail: tracked as hod#12\n; program(hod): exit 1\n");
    world.test("passes.ll", ";@ xfail: tracked as hod#13\n");
    world.test("crashes.ll", ";@ xfail: hod crashes on this\n; hod: crash\n");
    world.test("mold.ll", ";@ xfail(mold): only with mold\n; program(hod,mold): exit 1\n");
    world.test("stale.ll", ";@ xfail(lld): not any more\n");
    world.test("elsewhere.ll", &format!(";@ xfail({}): another target\n", other_arch().name()));
    world.test(
        "first.ll",
        ";@ xfail(llvm19): no\n;@ xfail(bfd,llvm22): yes\n;@ xfail: the rest\n; program(hod): exit 1\n",
    );
    let report = world.run();
    let all = report.summary(true);
    assert_eq!(outcomes(&report, "fails.ll"), [Outcome::Xfail; 3], "{all}");
    assert_eq!(
        details(&report, "fails.ll")[0],
        "tracked as hod#12 (the program of hod's object ended with exit status 1, \
         llc's with exit status 0)"
    );
    assert_eq!(outcomes(&report, "passes.ll"), [Outcome::Xpass; 3], "{all}");
    assert_eq!(
        details(&report, "passes.ll")[0],
        "passes; remove the 'xfail' directive (tracked as hod#13)"
    );
    assert_eq!(outcomes(&report, "crashes.ll"), [Outcome::Xfail; 3], "{all}");
    assert!(details(&report, "crashes.ll")[0].starts_with("hod crashes on this (hod was term"));
    assert_eq!(
        outcomes(&report, "mold.ll"),
        [Outcome::Pass, Outcome::Pass, Outcome::Xfail],
        "{all}"
    );
    assert_eq!(
        outcomes(&report, "stale.ll"),
        [Outcome::Pass, Outcome::Xpass, Outcome::Pass],
        "{all}"
    );
    assert_eq!(outcomes(&report, "elsewhere.ll"), [Outcome::Pass; 3], "{all}");
    assert!(details(&report, "first.ll")[0].starts_with("yes ("), "{all}");
    assert!(details(&report, "first.ll")[1].starts_with("the rest ("), "{all}");
    // XPASS fails the run; XFAIL alone does not.
    assert_eq!(report.exit_status(), 1);
    let brief = report.summary(false);
    assert!(brief.contains("XFAIL       ") && brief.contains("XPASS       "), "{brief}");

    let quiet = World::new("irdiff-xfail-only");
    quiet.test("fails.ll", ";@ xfail: tracked as hod#12\n; program(hod): exit 1\n");
    assert_eq!(quiet.run().exit_status(), 0);
}

// "Outcomes": trouble on the oracle's side is an ERROR: not a pass, and
// not a failure of hod.
#[test]
fn oracle_trouble_is_an_error() {
    let world = World::new("irdiff-oracle");
    world.test("crash.ll", "; llc: crash\n");
    world.test("hang.ll", "; llc: hang\n");
    world.test("fail.ll", "; llc: fail\n");
    world.test("silent.ll", "; llc: silent\n");
    world.test("link.ll", "; link-fail: llc bfd\n");
    world.test("stdout.ll", ";@ stdout: expected\n; program: echo actual\n");
    world.test("exit.ll", ";@ exit: 2\n; program: exit 3\n");
    world.test("signal.ll", ";@ exit: signal 6\n; program: exit 134\n");
    world.test("runs-long.ll", "; program(llc): exec sleep 60\n");
    world.test("directives.ll", ";@ stdout: fine\n;@ exitcode: 1\n;@ tier: slow\n");
    world.test("good.ll", ";@ stdout: fine\n; program: echo fine\n");
    let started = Instant::now();
    let report = irdiff::run(&short(world.config())).unwrap();
    assert!(started.elapsed() < Duration::from_secs(30), "{:?}", started.elapsed());
    let all = report.summary(true);
    for (file, records, detail) in [
        ("crash.ll", 1, "oracle: llc was terminated by signal 11"),
        ("hang.ll", 1, "oracle: llc timed out after 3 s"),
        ("fail.ll", 1, "oracle: llc failed (exit status 1): llc: error: bad module"),
        ("silent.ll", 1, "oracle: llc wrote no object"),
        (
            "stdout.ll",
            3,
            "oracle: the program of llc's object wrote stdout that differs from the directives \
             at line 1: expected \"expected\\n\", got \"actual\\n\"",
        ),
        (
            "exit.ll",
            3,
            "oracle: the program of llc's object ended with exit status 3, \
             the directives say exit status 2",
        ),
        (
            "signal.ll",
            3,
            "oracle: the program of llc's object ended with exit status 134, \
             the directives say signal 6",
        ),
        ("runs-long.ll", 3, "oracle: the program of llc's object timed out after 3 s"),
        (
            "directives.ll",
            1,
            "directives: line 2: unknown directive 'exitcode'; \
             line 3: unknown tier 'slow': expected quick or full",
        ),
    ] {
        assert_eq!(outcomes(&report, file), vec![Outcome::Error; records], "{file}: {all}");
        assert_eq!(details(&report, file)[0], detail, "{file}");
    }
    // Only the linker whose link failed.
    assert_eq!(
        outcomes(&report, "link.ll"),
        [Outcome::Error, Outcome::Pass, Outcome::Pass],
        "{all}"
    );
    assert_eq!(
        details(&report, "link.ll")[0],
        "oracle: link with bfd failed (exit status 1): bfd: error: undefined symbol: main"
    );
    // The other tests ran, and nothing is blamed on hod.
    assert_eq!(outcomes(&report, "good.ll"), [Outcome::Pass; 3], "{all}");
    assert_eq!(report.count(Outcome::Fail), 0);
    // An ERROR alone is exit status 3.
    assert_eq!(report.exit_status(), 3);
    assert!(report.summary(false).contains("ERROR       "));
}

// "Child processes": the limits are in force for the programs under test.
// The program's own output says so: the directives only hold when the
// time and memory limits are the configured ones and the memory limit
// refuses an allocation beyond it.
#[test]
fn limits_are_in_force_for_programs() {
    let world = World::new("irdiff-limits");
    let big = "dd if=/dev/zero of=/dev/null bs=268435456 count=1 2> /dev/null";
    world.test(
        "limits.ll",
        &format!(
            ";@ stdout: 65536\n;@ stdout: 4\n;@ stdout: 0\n;@ stdout: refused\n\
             ; program: ulimit -v; ulimit -t; ulimit -c\n; program: {big} || echo refused\n"
        ),
    );
    world.test("hod-hangs.ll", "; program(hod): exec sleep 60\n");
    world.test("hod-spins.ll", "; program(hod): while :; do :; done\n");
    world.test("hod-hogs.ll", &format!("; program(hod): {big} || exit 9\n"));
    let mut config = world.config();
    config.run_limits = Limits { time: Duration::from_secs(3), memory_kib: 64 << 10 };
    let started = Instant::now();
    let report = irdiff::run(&config).unwrap();
    assert!(started.elapsed() < Duration::from_secs(40), "{:?}", started.elapsed());
    let all = report.summary(true);
    assert_eq!(outcomes(&report, "limits.ll"), [Outcome::Pass; 3], "{all}");
    for file in ["hod-hangs.ll", "hod-spins.ll"] {
        assert_eq!(outcomes(&report, file), [Outcome::Fail; 3], "{all}");
        assert_eq!(details(&report, file), ["the program of hod's object timed out after 3 s"; 3]);
    }
    assert_eq!(
        details(&report, "hod-hogs.ll"),
        ["the program of hod's object ended with exit status 9, llc's with exit status 0"; 3]
    );

    // With the default limits the same allocation succeeds, so the
    // directives no longer hold: it was the limit.
    let roomy = World::new("irdiff-limits-roomy");
    roomy.test("big.ll", &format!(";@ stdout: refused\n; program: {big} || echo refused\n"));
    let report = roomy.run();
    assert_eq!(outcomes(&report, "big.ll"), [Outcome::Error; 3], "{}", report.summary(true));
}

// "Outcomes", SKIP: a test that is not valid for the target or the LLVM.
// "Tools": without an oracle every test is skipped.
#[test]
fn skips() {
    let world = World::new("irdiff-skips");
    let (here, elsewhere) = (host().name(), other_arch().name());
    world.test("target.ll", &format!(";@ target: {elsewhere}\n; llc: crash\n"));
    world.test("here.ll", &format!(";@ target: {here}\n"));
    world.test("llvm.ll", ";@ llvm: 19 23+\n; llc: crash\n");
    world.test("llvm-ok.ll", ";@ llvm: 19 21+\n");
    world.test("broken.ll", ";@ nonsense\n");
    let report = world.run();
    let all = report.summary(false);
    assert_eq!(
        rows(&report),
        [
            "ERROR broken.ll -",
            "PASS here.ll bfd",
            "PASS here.ll lld",
            "PASS here.ll mold",
            "PASS llvm-ok.ll bfd",
            "PASS llvm-ok.ll lld",
            "PASS llvm-ok.ll mold",
            "SKIP llvm.ll -",
            "SKIP target.ll -"
        ],
        "{all}"
    );
    assert_eq!(details(&report, "llvm.ll"), ["requires another LLVM (this run: 22)"]);
    assert_eq!(
        details(&report, "target.ll"),
        [format!("requires target {elsewhere} (this run: {here})")]
    );
    assert!(all.contains("SKIP        ") && all.contains("target.ll: requires target"), "{all}");

    // No oracle: everything is skipped, with a note; nothing fails unless
    // tools are required. A test that cannot be read is still an ERROR.
    world.without("llc-22");
    let report = world.run();
    assert_eq!(
        rows(&report),
        [
            "ERROR broken.ll -",
            "SKIP here.ll -",
            "SKIP llvm-ok.ll -",
            "SKIP llvm.ll -",
            "SKIP target.ll -"
        ]
    );
    assert_eq!(details(&report, "here.ll"), ["LLVM 22 or 19 (llc) not found"]);
    assert_eq!(report.notes, ["LLVM 22 or 19 (llc) not found; every test is skipped"]);
    assert!(report.records.iter().all(|record| record.llvm.is_none()));
    let results = format!("results-{here}-nollvm.tsv");
    assert_eq!(report.results_file, Some(world.build().join("irdiff").join(results)));
    std::fs::remove_file(world.scratch.join("tests/broken.ll")).unwrap();
    assert_eq!(world.run().exit_status(), 0);
    let mut required = world.config();
    required.require_tools = true;
    let report = irdiff::run(&required).unwrap();
    assert_eq!(report.exit_status(), 3);
    assert_eq!(report.missing_tools, ["LLVM 22 or 19 (llc) not found; every test is skipped"]);
    assert!(report.summary(false).contains(
        "irdiff: error: LLVM 22 or 19 (llc) not found; every test is skipped (HOD_REQUIRE_TOOLS=1)"
    ));
    // The major that was asked for is the one that is missed.
    let mut asked = world.config();
    asked.llvm = Some("19".to_owned());
    let report = irdiff::run(&asked).unwrap();
    assert_eq!(details(&report, "here.ll"), ["LLVM 19 (llc) not found"]);
}

// "Tools": the oracle is LLVM 22 when it is installed, else 19; `--llvm`
// selects one; a test's `llvm` directive and `llvm<N>` conditions follow
// the oracle that runs.
#[test]
fn the_oracle_is_selected() {
    let world = World::new("irdiff-llvm");
    world.tool("llc-19", FAKE_LLC);
    world.test("any.ll", "");
    world.test("only19.ll", ";@ llvm: 19\n");
    world.test("from20.ll", ";@ llvm: 20+\n");
    world.test(
        "xfail19.ll",
        ";@ xfail(llvm19): fails with the older oracle\n; program(hod): exit 1\n",
    );
    let mut config = world.config();
    config.linkers = vec!["bfd".to_owned()];
    let report = irdiff::run(&config).unwrap();
    assert!(report.configuration.starts_with("LLVM 22 ("), "{}", report.configuration);
    assert_eq!(
        rows(&report),
        ["PASS any.ll bfd", "PASS from20.ll bfd", "SKIP only19.ll -", "FAIL xfail19.ll bfd"]
    );
    config.llvm = Some("19".to_owned());
    let report = irdiff::run(&config).unwrap();
    assert!(report.configuration.starts_with("LLVM 19 ("), "{}", report.configuration);
    assert_eq!(
        rows(&report),
        ["PASS any.ll bfd", "SKIP from20.ll -", "PASS only19.ll bfd", "XFAIL xfail19.ll bfd"]
    );
    assert!(report.records.iter().all(|record| record.llvm == Some(19)));
    let results = format!("results-{}-llvm19.tsv", host().name());
    assert_eq!(report.results_file, Some(world.build().join("irdiff").join(results)));

    // An installation directory.
    let installed = world.scratch.join("opt/llvm/bin");
    std::fs::create_dir_all(&installed).unwrap();
    write_script(&installed.join("llc"), FAKE_LLC);
    config.llvm = Some(world.scratch.join("opt/llvm").to_string_lossy().into_owned());
    let report = irdiff::run(&config).unwrap();
    assert!(report.configuration.contains("opt/llvm/bin/llc)"), "{}", report.configuration);
    assert_eq!(report.count(Outcome::Pass), 2);
    config.llvm = Some(world.scratch.join("opt").to_string_lossy().into_owned());
    let error = irdiff::run(&config).unwrap_err();
    assert!(error.contains("is not an LLVM installation: no bin/llc"), "{error}");

    // Without 22, the default is 19.
    world.without("llc-22");
    let report = world.run();
    assert!(report.configuration.starts_with("LLVM 19 ("), "{}", report.configuration);
}

// "Cross targets": for a target that is not the host, both objects are
// built and hod's verdict is recorded, but nothing is linked or run.
#[test]
fn cross_target_builds_and_does_not_run() {
    let world = World::new("irdiff-cross");
    world.test("compiles.ll", ";@ stdout: never checked\n");
    world.test("declines.ll", ";@ unsupported: vector-type\n; hod: decline vector-type\n");
    world.test("unexpected.ll", "; hod: decline vector-type\n");
    world.test("crashes.ll", "; hod: crash\n");
    world.test("xfailed.ll", ";@ xfail(mold): needs a linker\n;@ xfail: any\n; hod: crash\n");
    world.test("oracle.ll", "; llc: fail\n");
    world.test("native.ll", &format!(";@ target: {}\n", host().name()));
    let mut config = world.config();
    config.target = Some(other_arch());
    let report = irdiff::run(&config).unwrap();
    let all = report.summary(true);
    assert_eq!(
        rows(&report),
        [
            "SKIP compiles.ll -",
            "FAIL crashes.ll -",
            "UNSUPPORTED declines.ll -",
            "SKIP native.ll -",
            "ERROR oracle.ll -",
            "FAIL unexpected.ll -",
            "XFAIL xfailed.ll -"
        ],
        "{all}"
    );
    let why = format!(
        "cross target: {} programs cannot be linked or run on {} host",
        other_arch().name(),
        host().name()
    );
    assert_eq!(details(&report, "compiles.ll"), [why.as_str()]);
    assert_eq!(
        details(&report, "declines.ll"),
        [format!("the oracle's program was not run: {why}")]
    );
    assert!(details(&report, "xfailed.ll")[0].starts_with("any (hod was terminated"), "{all}");
    assert_eq!(report.notes[1], format!("{why}; objects are built, programs are not"));
    assert!(report.records.iter().all(|r| r.target == other_arch().triple()));
    assert_eq!(report.records[0].built_by, BuiltBy::Hod);
    let results = format!("results-{}-llvm22.tsv", other_arch().name());
    assert_eq!(report.results_file, Some(world.build().join("irdiff").join(results)));
}

// "Tools": a linker that is not found is left out with a note; under
// HOD_REQUIRE_TOOLS=1 a missing bfd, lld or mold fails the run and a
// missing wild does not. `--linker` selects, and a linker that is asked
// for must exist.
#[test]
fn the_linker_matrix() {
    let world = World::new("irdiff-matrix");
    world.test("a.ll", "");
    world.without("mold");
    let report = world.run();
    assert_eq!(rows(&report), ["PASS a.ll bfd", "PASS a.ll lld"]);
    assert_eq!(
        report.notes,
        ["linker 'mold' not found; not in this run", "linker 'wild' not found; not in this run"]
    );
    assert_eq!(report.exit_status(), 0);

    let mut required = world.config();
    required.require_tools = true;
    let report = irdiff::run(&required).unwrap();
    assert_eq!(rows(&report), ["PASS a.ll bfd", "PASS a.ll lld"]);
    assert_eq!(report.missing_tools, ["linker 'mold' not found; not in this run"]);
    assert_eq!(report.notes, ["linker 'wild' not found; not in this run"]);
    assert_eq!(report.exit_status(), 3);

    // wild joins the matrix when it is there.
    world.tool("mold", &fake_linker("mold"));
    world.tool("wild", &fake_linker("wild"));
    let report = irdiff::run(&required).unwrap();
    assert_eq!(
        rows(&report),
        ["PASS a.ll bfd", "PASS a.ll lld", "PASS a.ll mold", "PASS a.ll wild"]
    );
    assert!(report.notes.is_empty() && report.missing_tools.is_empty());
    assert_eq!(report.exit_status(), 0);

    // --linker, in matrix order whatever the order asked.
    let mut chosen = world.config();
    chosen.linkers = vec!["wild".to_owned(), "bfd".to_owned()];
    let report = irdiff::run(&chosen).unwrap();
    assert_eq!(rows(&report), ["PASS a.ll bfd", "PASS a.ll wild"]);
    assert!(report.configuration.contains("; linkers bfd wild; "), "{}", report.configuration);
    world.without("wild");
    let error = irdiff::run(&chosen).unwrap_err();
    assert_eq!(error, "linker 'wild' was asked for and is not found");
    chosen.linkers = vec!["gold".to_owned()];
    let error = irdiff::run(&chosen).unwrap_err();
    assert_eq!(error, "unknown linker 'gold': expected bfd, lld, mold or wild");

    // No driver: nothing can be linked; hod's verdict is still recorded.
    world.without("gcc");
    world.test("b.ll", ";@ unsupported: vector-type\n; hod: decline vector-type\n");
    let report = world.run();
    assert_eq!(rows(&report), ["SKIP a.ll -", "UNSUPPORTED b.ll -"]);
    assert_eq!(details(&report, "a.ll"), ["no linker driver (gcc) found"]);
    assert_eq!(report.notes[0], "linker driver 'gcc' not found; nothing is linked or run");
    assert_eq!(report.exit_status(), 0);
    let report = irdiff::run(&required).unwrap();
    assert_eq!(report.exit_status(), 3);
}

// "Linking": the harness checks which linker produced each program, so a
// driver that quietly uses another linker is an ERROR, not three passes
// of the same linker.
#[test]
fn the_wrong_linker_is_detected() {
    let world = World::new("irdiff-signature");
    world.tool("gcc", DEAF_DRIVER);
    world.test("a.ll", "");
    let report = world.run();
    assert_eq!(
        rows(&report),
        ["PASS a.ll bfd", "ERROR a.ll lld", "ERROR a.ll mold"],
        "{}",
        report.summary(true)
    );
    assert_eq!(
        details(&report, "a.ll")[1],
        "oracle: the driver did not link with lld: the program carries no linker's signature"
    );
}

// "Hybrid mode": hod falls back to the oracle for what it declines; the
// object is compared like any other and the report says who built it.
#[test]
fn hybrid_mode() {
    let world = World::new("irdiff-hybrid");
    world.test("own.ll", ";@ stdout: x\n; program: echo x\n");
    world.test("falls-back.ll", ";@ stdout: x\n; program: echo x\n; hod: decline inline-asm\n");
    world.test(
        "expected.ll",
        ";@ unsupported: vector-type\n;@ stdout: x\n; program: echo x\n; hod: decline vector-type\n",
    );
    world.test("reason.ll", ";@ unsupported: vector-type\n; hod: decline inline-asm\n");
    world.test("stale.ll", ";@ unsupported: vector-type\n");
    // The fallback's object behaves as the oracle's, so a program line for
    // hod's own objects does not apply to it.
    world.test("same.ll", "; program(hod): exit 1\n; hod: decline inline-asm\n");
    let mut config = world.config();
    config.mode = Mode::Hybrid;
    let report = irdiff::run(&config).unwrap();
    let all = report.summary(true);
    for file in ["own.ll", "falls-back.ll", "expected.ll", "same.ll"] {
        assert_eq!(outcomes(&report, file), [Outcome::Pass; 3], "{file}: {all}");
    }
    let by = |file: &str| {
        let record = report.records.iter().find(|r| file_name(&r.test) == file).unwrap();
        (record.built_by, record.reason.clone())
    };
    assert_eq!(by("own.ll"), (BuiltBy::Hod, None));
    assert_eq!(by("falls-back.ll"), (BuiltBy::Fallback, Some("inline-asm".to_owned())));
    assert_eq!(by("expected.ll"), (BuiltBy::Fallback, Some("vector-type".to_owned())));
    assert_eq!(
        details(&report, "reason.ll"),
        ["hod fell back with reason 'inline-asm'; the 'unsupported' directive says 'vector-type'";
            3]
    );
    assert_eq!(outcomes(&report, "reason.ll"), [Outcome::Fail; 3]);
    assert_eq!(outcomes(&report, "stale.ll"), [Outcome::Fail; 3]);
    assert!(all.contains("; hybrid; -j "), "{all}");
    assert!(
        all.contains(
            "irdiff: coverage: hod built 2 of 6 objects; fallback inline-asm: 3; \
             fallback vector-type: 1\n"
        ),
        "{all}"
    );
    assert!(all.contains("falls-back.ll [bfd lld mold]: inline-asm\n"), "{all}");
    let line = report.results_text();
    assert!(line.contains("\tbfd\tfallback\tinline-asm\t-\t-\n"), "{line}");
    let results = format!("results-{}-llvm22-hybrid.tsv", host().name());
    assert_eq!(report.results_file, Some(world.build().join("irdiff").join(results)));
}

/// A mix of every outcome.
fn mixed(world: &World) {
    for i in 0..3 {
        world.test(&format!("pass{i}.ll"), &format!(";@ stdout: {i}\n; program: echo {i}\n"));
        world.test(&format!("fail{i}.ll"), "; program(hod): exit 1\n");
    }
    world.test("unsupported.ll", ";@ unsupported: vector-type\n; hod: decline vector-type\n");
    world.test("xfail.ll", ";@ xfail(lld): with lld\n; program(hod,lld): exit 1\n");
    world.test("xpass.ll", ";@ xfail: no longer\n");
    world.test("error.ll", "; llc: fail\n");
    world.test("skip.ll", ";@ llvm: 19\n");
    world.test("quick.ll", ";@ tier: quick\n");
    world.test("sub/dir/nested.ll", ";@ tier: quick\n; program(hod,bfd): exit 1\n");
}

// "Options", `-j`: the report does not depend on how many tests run at
// once: same records, same order, same results file.
#[test]
fn parallel_runs_are_deterministic() {
    let world = World::new("irdiff-jobs");
    std::fs::create_dir_all(world.scratch.join("tests/sub/dir")).unwrap();
    mixed(&world);
    let run = |jobs: usize| {
        let mut config = world.config();
        config.jobs = jobs;
        let report = irdiff::run(&config).unwrap();
        let file = std::fs::read_to_string(report.results_file.as_ref().unwrap()).unwrap();
        assert_eq!(file, report.results_text());
        let summary = report.summary(true);
        let stable: Vec<String> = summary
            .lines()
            .filter(|line| !line.contains(" results: ") && !line.contains("; -j "))
            .map(str::to_owned)
            .collect();
        (report, file, stable)
    };
    let (serial, serial_file, serial_summary) = run(1);
    assert!(serial.configuration.ends_with("; -j 1"), "{}", serial.configuration);
    for jobs in [4, 16, 1000] {
        let (parallel, file, summary) = run(jobs);
        assert_eq!(parallel.records, serial.records, "-j {jobs}");
        assert_eq!(file, serial_file, "-j {jobs}");
        assert_eq!(summary, serial_summary, "-j {jobs}");
        // The cap.
        let shown = jobs.min(irdiff::MAX_JOBS);
        assert!(parallel.configuration.ends_with(&format!("; -j {shown}")));
    }
    // By test name, then by linker in matrix order.
    let names: Vec<&str> = serial.records.iter().map(|r| file_name(&r.test)).collect();
    let mut order: Vec<&str> = names.clone();
    order.dedup();
    assert_eq!(
        order,
        [
            "error.ll",
            "fail0.ll",
            "fail1.ll",
            "fail2.ll",
            "pass0.ll",
            "pass1.ll",
            "pass2.ll",
            "quick.ll",
            "skip.ll",
            "nested.ll",
            "unsupported.ll",
            "xfail.ll",
            "xpass.ll"
        ]
    );
    assert_eq!(serial.tests(), 13);
    assert_eq!(
        Outcome::ALL.map(|outcome| serial.count(outcome)),
        // PASS FAIL UNSUPPORTED XFAIL XPASS ERROR SKIP
        [9 + 3 + 2 + 2, 9 + 1, 3, 1, 3, 1, 1]
    );
    assert_eq!(serial.exit_status(), 1);
}

// "Options", `--tier` and the paths: `quick` runs the tests marked
// `tier: quick`; a path is a file or a directory searched recursively.
#[test]
fn tiers_and_paths() {
    let world = World::new("irdiff-tier");
    std::fs::create_dir_all(world.scratch.join("tests/sub/dir")).unwrap();
    mixed(&world);
    let mut config = world.config();
    config.tier = TierFilter::Quick;
    config.linkers = vec!["bfd".to_owned()];
    let report = irdiff::run(&config).unwrap();
    assert_eq!(rows(&report), ["PASS quick.ll bfd", "FAIL nested.ll bfd"]);

    config.tier = TierFilter::All;
    config.paths = vec![
        world.scratch.join("tests/sub"),
        world.scratch.join("tests/pass1.ll"),
        world.scratch.join("tests/pass1.ll"),
    ];
    let report = irdiff::run(&config).unwrap();
    assert_eq!(rows(&report), ["PASS pass1.ll bfd", "FAIL nested.ll bfd"]);

    // Nothing to run is a mistake in the request, not a green run.
    config.tier = TierFilter::Quick;
    config.paths = vec![world.scratch.join("tests/pass1.ll")];
    let error = irdiff::run(&config).unwrap_err();
    assert_eq!(error, "no test is selected (no test found is in the quick tier)");
    config.paths = vec![world.scratch.join("tests/absent.ll")];
    assert!(irdiff::run(&config).unwrap_err().contains("is not a file or a directory"));
    config.paths = vec![world.build()];
    assert!(irdiff::run(&config).unwrap_err().contains("no test (*.ll) in"));
    let mut no_hod = world.config();
    no_hod.hod = Some(world.scratch.join("absent-hod"));
    assert!(irdiff::run(&no_hod).unwrap_err().ends_with("absent-hod' is not a file"));
}

// "Work directory": each run works in its own directory under the build
// directory and removes it; `--keep` keeps the directories of the failing
// tests, and only those.
#[test]
fn work_directory_and_keep() {
    let world = World::new("irdiff-keep");
    world.test("good.ll", "");
    world.test("bad.ll", ";@ stdout: x\n; program: echo x\n; program(hod,mold): echo y\n");
    let results = format!("results-{}-llvm22.tsv", host().name());

    let report = world.run();
    assert_eq!(report.exit_status(), 1);
    assert_eq!(report.kept, None);
    assert_eq!(
        left_behind(&world),
        std::slice::from_ref(&results),
        "a failing run leaves nothing either"
    );

    let mut keeping = world.config();
    keeping.keep = true;
    let report = irdiff::run(&keeping).unwrap();
    let kept = report.kept.clone().expect("the work directory is kept");
    assert!(kept.starts_with(world.build().join("irdiff")), "{}", kept.display());
    assert!(report.summary(false).contains(&format!("irdiff: kept: {}\n", kept.display())));
    let mut inside: Vec<String> = std::fs::read_dir(&kept)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    inside.sort();
    assert_eq!(inside.len(), 2, "{inside:?}");
    assert!(inside[0].starts_with("0000-") && inside[0].ends_with("bad.ll"), "{inside:?}");
    assert_eq!(inside[1], "linkers");
    let test_dir = kept.join(&inside[0]);
    for file in ["oracle.o", "hod.o", "coverage.tsv", "mold/oracle", "mold/hod", "bfd/hod"] {
        assert!(test_dir.join(file).is_file(), "{file} is kept");
    }
    // The kept program is the one that failed.
    let program = std::fs::read_to_string(test_dir.join("mold/hod")).unwrap();
    assert!(program.contains("echo y"), "{program}");

    // Two runs at once do not share a work directory: a second run leaves
    // the first one's kept directory alone.
    let again = irdiff::run(&keeping).unwrap();
    assert_ne!(again.kept, report.kept);
    assert!(kept.is_dir());

    // --keep with nothing failing keeps nothing.
    let green = World::new("irdiff-keep-green");
    green.test("good.ll", "");
    let mut keeping = green.config();
    keeping.keep = true;
    let report = irdiff::run(&keeping).unwrap();
    assert_eq!(report.kept, None);
    assert_eq!(left_behind(&green), [results]);
}

// "Results file": `--results` names it; it is replaced, never appended to.
#[test]
fn results_file() {
    let world = World::new("irdiff-results");
    world.test(
        "a.ll",
        ";@ covers: one thing\n;@ unsupported: vector-type\n; hod: decline vector-type\n",
    );
    world.test("b.ll", "; program(hod,lld): echo \"tab\there\"\n");
    let mut config = world.config();
    config.results = Some(world.scratch.join("out/dir/results.tsv"));
    let report = irdiff::run(&config).unwrap();
    assert_eq!(report.results_file, config.results);
    let first = std::fs::read_to_string(config.results.as_ref().unwrap()).unwrap();
    let triple = host().triple();
    let (a, b) = (world.scratch.join("tests/a.ll"), world.scratch.join("tests/b.ll"));
    let (a, b) = (a.display(), b.display());
    let difference = "the program of hod's object wrote stdout that differs from llc's at \
                      line 1: expected the end of the output, got \"tab\\\\there\\\\n\"";
    assert_eq!(
        first,
        format!(
            "UNSUPPORTED\t{a}\t{triple}\t22\tbfd\tdeclined\tvector-type\t-\tone thing\n\
             UNSUPPORTED\t{a}\t{triple}\t22\tlld\tdeclined\tvector-type\t-\tone thing\n\
             UNSUPPORTED\t{a}\t{triple}\t22\tmold\tdeclined\tvector-type\t-\tone thing\n\
             PASS\t{b}\t{triple}\t22\tbfd\thod\t-\t-\t-\n\
             FAIL\t{b}\t{triple}\t22\tlld\thod\t-\t{difference}\t-\n\
             PASS\t{b}\t{triple}\t22\tmold\thod\t-\t-\t-\n"
        )
    );
    assert!(first.lines().all(|line| line.split('\t').count() == 9));
    std::fs::remove_file(world.scratch.join("tests/b.ll")).unwrap();
    irdiff::run(&config).unwrap();
    let second = std::fs::read_to_string(config.results.as_ref().unwrap()).unwrap();
    assert_eq!(second.lines().count(), 3);
    assert!(!world.build().join("irdiff").join("results.tsv").exists());
}

/// Runs `cargo xtask irdiff` as a process, in a world of fakes: `PATH` is
/// the fake tools only, and the build directory is the world's.
fn command(world: &World, args: &[&str], env: &[(&str, &str)]) -> hod_testutil::Outcome {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_xtask"));
    command.arg("irdiff").args(["--hod".as_ref(), world.hod().as_os_str()]);
    command.args(args).arg(world.scratch.join("tests"));
    command.env("PATH", world.scratch.join("bin")).env("CARGO_TARGET_DIR", world.build());
    for name in ["HOD_REQUIRE_TOOLS", "HOD_STRICT", "HOD_FALLBACK_LLC", "HOD_COVERAGE_LOG"] {
        command.env_remove(name);
    }
    for (name, value) in env {
        command.env(name, value);
    }
    hod_testutil::run_with(&mut command, b"", Duration::from_mins(2))
}

fn status_of(outcome: &hod_testutil::Outcome) -> Option<i32> {
    outcome.status.and_then(|status| status.code())
}

// "Exit status": 0, 1, 3 and 2, from the command itself; the summary goes
// to standard output and usage errors to standard error.
#[test]
fn command_exit_status() {
    let world = World::new("irdiff-command");
    world.test("pass.ll", ";@ stdout: x\n; program: echo x\n");
    let green = command(&world, &[], &[]);
    assert_eq!(status_of(&green), Some(0), "{}{}", green.stdout_text(), green.stderr_text());
    let out = green.stdout_text();
    assert!(out.starts_with("irdiff: LLVM 22 ("), "{out}");
    assert!(out.contains("irdiff: 1 tests, 3 results: 3 PASS, 0 FAIL"), "{out}");
    assert!(out.contains(&format!(
        "irdiff: results: {}/irdiff/results-{}-llvm22.tsv\n",
        world.build().display(),
        host().name()
    )));
    assert!(!out.contains("pass.ll"), "{out}");
    let verbose = command(&world, &["-v", "--linker", "mold", "-j", "2"], &[]);
    assert!(verbose.stdout_text().contains("pass.ll [mold]\n"), "{}", verbose.stdout_text());
    assert!(verbose.stdout_text().contains("; linkers mold; strict; -j 2\n"));

    world.test("error.ll", "; llc: fail\n");
    let error = command(&world, &[], &[]);
    assert_eq!(status_of(&error), Some(3), "{}", error.stdout_text());
    assert!(error.stdout_text().contains("ERROR       "), "{}", error.stdout_text());

    world.test("fail.ll", "; program(hod): exit 1\n");
    let fail = command(&world, &[], &[]);
    assert_eq!(status_of(&fail), Some(1), "{}", fail.stdout_text());
    let hybrid = command(&world, &["--hybrid", "--tier=all"], &[]);
    assert_eq!(status_of(&hybrid), Some(1));
    assert!(hybrid.stdout_text().contains("; hybrid; -j "), "{}", hybrid.stdout_text());

    for (args, needle) in [
        (&["--frobnicate"][..], "xtask irdiff: unknown option '--frobnicate'"),
        (&["--linker", "gold"], "unknown linker 'gold'"),
        (&["--tier", "quick"], "no test is selected"),
        (&["--llvm", "/nonexistent"], "is not an LLVM installation"),
        (&["/nonexistent/tests"], "is not a file or a directory"),
        (&["--linker", "wild"], "linker 'wild' was asked for and is not found"),
    ] {
        let usage = command(&world, args, &[]);
        assert_eq!(status_of(&usage), Some(2), "{args:?}: {}", usage.stderr_text());
        assert!(usage.stderr_text().contains(needle), "{args:?}: {}", usage.stderr_text());
        assert_eq!(usage.stdout_text(), "", "{args:?}");
    }
    let help = command(&world, &["--help"], &[]);
    assert_eq!(status_of(&help), Some(0));
    assert_eq!(help.stdout_text(), irdiff::USAGE);
}

// "Tools": HOD_REQUIRE_TOOLS=1 turns a missing required tool into exit
// status 3. "What a test does": hod is configured by options only, so
// the variables that configure it do not leak in from the environment
// (the fake hod refuses to run with any of them set).
#[test]
fn command_environment() {
    let world = World::new("irdiff-environment");
    world.test("pass.ll", ";@ stdout: x\n; program: echo x\n");
    let leaky = [
        ("HOD_STRICT", "1"),
        ("HOD_FALLBACK_LLC", "/nonexistent/llc"),
        ("HOD_COVERAGE_LOG", "/nonexistent/dir/log.tsv"),
        ("HOD_IN_FALLBACK", "1"),
    ];
    let clean = command(&world, &[], &leaky);
    assert_eq!(status_of(&clean), Some(0), "{}", clean.stdout_text());
    assert!(clean.stdout_text().contains("3 PASS"), "{}", clean.stdout_text());

    let required = command(&world, &[], &[("HOD_REQUIRE_TOOLS", "1")]);
    assert_eq!(status_of(&required), Some(0), "wild is optional: {}", required.stdout_text());
    world.without("mold");
    let optional = command(&world, &[], &[]);
    assert_eq!(status_of(&optional), Some(0));
    assert!(optional.stdout_text().contains("irdiff: note: linker 'mold' not found"));
    let required = command(&world, &[], &[("HOD_REQUIRE_TOOLS", "1")]);
    assert_eq!(status_of(&required), Some(3), "{}", required.stdout_text());
    assert!(
        required.stdout_text().contains(
            "irdiff: error: linker 'mold' not found; not in this run (HOD_REQUIRE_TOOLS=1)\n"
        ),
        "{}",
        required.stdout_text()
    );
    // Any other value does not require them.
    let other = command(&world, &[], &[("HOD_REQUIRE_TOOLS", "0")]);
    assert_eq!(status_of(&other), Some(0));
}

// The fakes themselves: what the tests above rely on.
#[test]
fn the_fakes_work() {
    let world = World::new("irdiff-fakes");
    let module = world.test("m.ll", "; program: echo ran\n; program(hod): echo hod\n");
    let run = |program: &Path, args: &[&str]| {
        let mut command = std::process::Command::new(program);
        command.args(args);
        hod_testutil::run_with(&mut command, b"", Duration::from_mins(1))
    };
    let (bin, module) = (world.scratch.join("bin"), module.to_string_lossy().into_owned());
    let object = world.scratch.join("m.o").to_string_lossy().into_owned();
    let log = world.scratch.join("log.tsv");
    let version = run(&bin.join("llc-22"), &["--version"]);
    assert_eq!(version.stdout_text(), "fake LLVM version 22.0.0\n");
    let coverage = format!("--coverage-log={}", log.display());
    let built = run(&world.hod(), &[coverage.as_str(), "--strict", "-O0", "-o", &object, &module]);
    assert!(built.success(), "{}", built.stderr_text());
    assert_eq!(std::fs::read_to_string(&log).unwrap().split('\t').count(), 8);
    let program = world.scratch.join("m").to_string_lossy().into_owned();
    assert!(run(&bin.join("mold"), &["-pie", "-o", &program, &object]).success());
    let ran = run(Path::new(&program), &[]);
    assert_eq!(ran.stdout_text(), "ran\nhod\n");
    let neither = run(&world.hod(), &[coverage.as_str(), "-o", &object, &module]);
    assert_eq!(status_of(&neither), Some(1));
}
