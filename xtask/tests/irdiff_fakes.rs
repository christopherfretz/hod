//! The outcome logic of `cargo xtask irdiff` (`spec/harness.md` "irdiff"),
//! exercised with fake tools: an `llc`, a `hod`, a linker driver and
//! linkers that are shell scripts, so every outcome can be provoked on any
//! Linux machine, without LLVM.
//!
//! How the fakes work. A "module" is any text. The fake compilers write an
//! "object" that is a first line `built-by llc`, `built-by O2` (`llc -O2`:
//! the witness) or `built-by hod`, a second line with their
//! code-generation arguments, and the module. The fake linkers turn the
//! objects into a "program", a shell script made of the modules'
//! `; program: <command>` lines in link order; `; program(hod): ...` is
//! kept only for an object hod built, `; program(llc): ...` only for one
//! `llc -O0` built, `; program(O2): ...` only for the witness's,
//! `; program(mold): ...` only by that linker, and
//! `; program(hod,mold): ...` needs both. Other comment lines make a tool
//! misbehave: `; llc: crash|hang|fail|silent`, `; llc(O2): fail`,
//! `; hod: decline <reason>|crash|error|hang|nolog|noobject|hog`,
//! `; link-fail: <builder> <linker>`, `; link-warn: <linker>` (a warning,
//! which fails the link under `--fatal-warnings`).
#![cfg(target_os = "linux")]

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::Scratch;
use hod_testutil::write_script;
use xtask::irdiff::{
    self, Arch, BuiltBy, Config, Failure, Limits, Mode, Outcome, Report, Search, TierFilter,
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
if grep -q '^; llc: hang' "$in"; then exec sleep 600; fi
if grep -q '^; llc: fail' "$in"; then echo "llc: error: bad module" >&2; exit 1; fi
if grep -q '^; llc: silent' "$in"; then exit 0; fi
case "$args " in *" -O2 "*) builder=O2 ;; *) builder=llc ;; esac
if grep -q "^; llc($builder): fail" "$in"; then echo "llc: error: cannot optimize" >&2; exit 1; fi
{ echo "built-by $builder"; echo "args$args"; cat "$in"; } > "$out""#;

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
if grep -q '^; hod: hang' "$in"; then exec sleep 600; fi
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

/// `opt`: requires the passes of IR before optimisation, and makes each
/// `; opt-program...` line of the module a `; program...` line, so that a
/// program line reaches the oracle's object only through `opt`.
/// `; opt: fail` makes it fail.
const FAKE_OPT: &str = r#"PATH=/usr/bin:/bin
out= in= args=
while [ $# -gt 0 ]; do
  case "$1" in
    -o) out=$2; shift ;;
    -*) args="$args $1" ;;
    *) in=$1 ;;
  esac
  shift
done
case "$args" in *" -passes=default<O0> "*) ;; *) echo "opt: wrong passes:$args" >&2; exit 1 ;; esac
if grep -q '^; opt: fail' "$in"; then echo "opt: error: bad module" >&2; exit 1; fi
{ echo "; opt$args"; sed 's/^; opt-program/; program/' "$in"; } > "$out""#;

const FAKE_DRIVER: &str = r#"dir=
for arg in "$@"; do case "$arg" in -B*) dir=${arg#-B} ;; esac; done
exec "$dir/ld" "$@""#;

/// A driver that ignores the linker it is given and always uses GNU ld's
/// stand-in.
const DEAF_DRIVER: &str = r#"exec "$(dirname "$0")/ld.bfd" "$@""#;

/// The fake linker called `name`. Its programs carry the signature the
/// real linker leaves, and the variables `PIE`, `LIBS`, `LINKER_FLAGS` and
/// `CODEGEN` (the arguments the first object was compiled with) for the
/// modules' commands to print.
fn fake_linker(name: &str) -> String {
    let signature = match name {
        "lld" => "echo '# Linker: LLD 22.0.0'",
        "mold" => "echo '# mold 2.0.0 (compatible with GNU ld)'",
        _ => ":",
    };
    format!(
        r#"PATH=/usr/bin:/bin
me={name}
out= objs= pie= libs= flags=
while [ $# -gt 0 ]; do
  case "$1" in
    -o) out=$2; shift ;;
    -B*) ;;
    -pie|-no-pie) pie=$1 ;;
    -l*) libs="$libs $1" ;;
    -*) flags="$flags $1" ;;
    *) objs="$objs $1" ;;
  esac
  shift
done
first=
for obj in $objs; do
  [ -n "$first" ] || first=$obj
  builder=$(sed -n '1s/^built-by //p' "$obj")
  if grep -q "^; link-fail: $builder $me\$" "$obj"; then
    echo "$me: error: undefined symbol: main" >&2
    exit 1
  fi
  if grep -q "^; link-warn: $me\$" "$obj"; then
    echo "$me: warning: the use of 'tmpnam' is dangerous" >&2
    case "$flags" in *--fatal-warnings*) exit 1 ;; esac
  fi
done
{{
  echo '#!/bin/sh'
  {signature}
  echo "PATH=/usr/bin:/bin PIE='$pie' LIBS='$libs' LINKER_FLAGS='$flags'"
  echo "CODEGEN='$(sed -n '2s/^args //p' "$first")'"
  for obj in $objs; do
    builder=$(sed -n '1s/^built-by //p' "$obj")
    sed -n -e 's/^; program: //p' -e "s/^; program($builder): //p" \
      -e "s/^; program($me): //p" -e "s/^; program($builder,$me): //p" "$obj"
  done
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

/// How long a run with hanging tools may take. A hang sleeps 600 s, and a
/// run that waited for one instead of killing it at its limit would take
/// that long; a run that kills them takes its 3 s limits plus whatever the
/// machine adds: waits for the shared pool's tokens are in the elapsed
/// time, and at load 25 to 32 runs of under 10 s have taken 32 to 48 s. So
/// the bound is far from both, and says only that nothing waited for a
/// hang (the details say that each hang was killed at its limit).
const HANG_BOUND: Duration = Duration::from_mins(4);

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

/// The failure kinds of the records of one test, in linker order.
fn failures(report: &Report, file: &str) -> Vec<Option<Failure>> {
    report
        .records
        .iter()
        .filter(|record| file_name(&record.test) == file)
        .map(|record| record.failure)
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

// spec/harness.md "irdiff", "What a test does" and "Outcomes": PASS. The
// oracle gets its selector and hod does not ("The oracle"); otherwise both
// get the same code-generation arguments, each object is linked by every
// linker of the matrix (and by that linker), and nothing is left behind
// but the results file.
#[test]
fn pass_with_every_linker() {
    let world = World::new("irdiff-pass");
    let triple = host().triple();
    let common = format!("-filetype=obj -mtriple={triple} -relocation-model=pic");
    world.test(
        "pass.ll",
        &format!(
            ";@ covers: spec/harness.md irdiff\n\
             ;@ stdout: hello\n\
             ;@ stdout: -pie [] one thread\n\
             ; program: echo hello\n\
             ; program(llc): [ \"$CODEGEN\" = \"-O0 -global-isel=0 -fast-isel=0 {common}\" ] || echo \"llc: $CODEGEN\"\n\
             ; program(hod): [ \"$CODEGEN\" = \"-O0 {common}\" ] || echo \"hod: $CODEGEN\"\n\
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
    for file in ["stdout.ll", "status.ll", "signal.ll", "extra.ll"] {
        assert_eq!(failures(&report, file), [Some(Failure::Comparison); 3], "{file}");
    }
    assert_eq!(failures(&report, "stderr.ll"), [None; 3]);
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
    assert!(started.elapsed() < HANG_BOUND, "{:?}", started.elapsed());
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
    assert!(report.records.iter().all(|record| record.failure == Some(Failure::HodBuild)));
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
    // `*`: any reason will do, but hod must still decline.
    world.test("any.ll", ";@ unsupported: *\n; hod: decline module:asm\n");
    world.test("any-stale.ll", ";@ unsupported: *\n");
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
    assert_eq!(outcomes(&report, "any.ll"), [Outcome::Unsupported; 3], "{all}");
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
        ("any-stale.ll", "hod compiled this test; remove its 'unsupported: *' directive"),
        (
            "xfailed.ll",
            "hod declined the module and the test has no \
             'unsupported: codegen-not-implemented' directive",
        ),
    ] {
        assert_eq!(outcomes(&report, file), [Outcome::Fail; 3], "{file}: {all}");
        assert_eq!(details(&report, file), [detail; 3]);
        assert_eq!(failures(&report, file), [Some(Failure::Declined); 3], "{file}");
    }
    assert_eq!(failures(&report, "expected.ll"), [None; 3]);
    assert_eq!(outcomes(&report, "oracle.ll"), [Outcome::Error; 3], "{all}");
    assert!(all.contains("UNSUPPORTED "), "{all}");
    assert!(all.contains("expected.ll [bfd lld mold]: codegen-not-implemented\n"), "{all}");
    assert!(
        all.contains(
            "irdiff: coverage: hod built 3 of 10 objects; declined codegen-not-implemented: 4; \
             declined inline-asm: 1; declined module:asm: 1; declined now: 1\n"
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
    world.test("runs-long.ll", "; program(llc): exec sleep 600\n");
    world.test("directives.ll", ";@ stdout: fine\n;@ exitcode: 1\n;@ tier: slow\n");
    world.test("good.ll", ";@ stdout: fine\n; program: echo fine\n");
    let started = Instant::now();
    let report = irdiff::run(&short(world.config())).unwrap();
    assert!(started.elapsed() < HANG_BOUND, "{:?}", started.elapsed());
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
    // spec/harness.md "irdiff", "Library": the kind of each, which callers
    // decide on instead of the words.
    for (file, failure) in [
        ("crash.ll", Failure::Crash),
        ("hang.ll", Failure::Timeout),
        ("fail.ll", Failure::ReferenceBuild),
        ("silent.ll", Failure::ReferenceBuild),
        ("stdout.ll", Failure::Comparison),
        ("exit.ll", Failure::Comparison),
        ("signal.ll", Failure::Comparison),
        ("runs-long.ll", Failure::Run),
        ("directives.ll", Failure::Harness),
    ] {
        assert!(failures(&report, file).iter().all(|f| *f == Some(failure)), "{file}");
    }
    assert_eq!(failures(&report, "link.ll"), [Some(Failure::Link), None, None]);
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
    world.test("hod-hangs.ll", "; program(hod): exec sleep 600\n");
    world.test("hod-spins.ll", "; program(hod): while :; do :; done\n");
    world.test("hod-hogs.ll", &format!("; program(hod): {big} || exit 9\n"));
    let mut config = world.config();
    config.run_limits = Limits { time: Duration::from_secs(3), memory_kib: 64 << 10 };
    let started = Instant::now();
    let report = irdiff::run(&config).unwrap();
    assert!(started.elapsed() < HANG_BOUND, "{:?}", started.elapsed());
    let all = report.summary(true);
    assert_eq!(outcomes(&report, "limits.ll"), [Outcome::Pass; 3], "{all}");
    for file in ["hod-hangs.ll", "hod-spins.ll"] {
        assert_eq!(outcomes(&report, file), [Outcome::Fail; 3], "{all}");
        assert_eq!(details(&report, file), ["the program of hod's object timed out after 3 s"; 3]);
        assert_eq!(failures(&report, file), [Some(Failure::Run); 3]);
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
    let built_only = format!("{why}; objects are built, programs are not");
    assert!(report.notes.contains(&built_only), "{all}");
    // What is missing for the programs to run under the emulator is said
    // with the Debian package to install.
    let emulator = format!("qemu-{}", other_arch().name());
    let missing = format!("emulator '{emulator}' not found (Debian package qemu-user)");
    assert!(report.notes.iter().any(|note| note.starts_with(&missing)), "{all}");
    let driver = format!("linker driver '{}-gcc' not found (Debian package gcc-", prefix());
    assert!(report.notes.iter().any(|note| note.starts_with(&driver)), "{all}");
    assert!(report.records.iter().all(|r| r.target == other_arch().triple()));
    assert_eq!(report.records[0].built_by, BuiltBy::Hod);
    let results = format!("results-{}-llvm22.tsv", other_arch().name());
    assert_eq!(report.results_file, Some(world.build().join("irdiff").join(results)));
}

/// The GNU prefix of the other architecture's cross tools.
fn prefix() -> &'static str {
    if other_arch() == Arch::X86_64 { "x86_64-linux-gnu" } else { "aarch64-linux-gnu" }
}

/// A fake `qemu-<arch>`: requires `-cpu <model> -L <existing directory>`
/// before the program, says what it was given in `$EMULATED` and runs the
/// program with the rest of its arguments.
const FAKE_QEMU: &str = r#"[ "$1" = -cpu ] && [ "$3" = -L ] && [ -d "$4" ] || { echo "qemu: usage: $*" >&2; exit 1; }
EMULATED="$(basename "$0") $2 $4"
export EMULATED
shift 4
exec "$@""#;

impl World {
    /// The other architecture's cross driver and GNU ld, its emulator and
    /// a system root: what a cross run needs.
    fn with_cross_tools(&self) -> Config {
        self.tool(&format!("{}-gcc", prefix()), FAKE_DRIVER);
        self.tool(&format!("{}-ld.bfd", prefix()), &fake_linker("bfd"));
        self.tool(&format!("qemu-{}", other_arch().name()), FAKE_QEMU);
        let sysroot = self.scratch.join("sysroot");
        std::fs::create_dir_all(&sysroot).unwrap();
        let mut config = self.config();
        config.target = Some(other_arch());
        config.sysroot = Some(sysroot);
        config
    }
}

// spec/harness.md "irdiff", "Cross runs": with the other architecture's
// cross driver, its GNU ld, its emulator and its system root, both sides of
// a test are linked by every linker and run under the emulator, which gets
// the baseline CPU model and the system root; the program gets its
// arguments and input as on the host, and six times the run limit.
#[test]
fn cross_target_runs_under_the_emulator() {
    let world = World::new("irdiff-emulated");
    let config = world.with_cross_tools();
    world.test(
        "seen.ll",
        &[
            ";@ args: one two",
            ";@ stdin: typed",
            ";@ stdout: one two",
            ";@ stdout: typed",
            &format!(
                ";@ stdout: qemu-{} {} {}",
                other_arch().name(),
                irdiff::tools::target_of(other_arch()).emulated_cpu(),
                world.scratch.join("sysroot").display()
            ),
            ";@ exit: 3",
            "; program: echo \"$@\"; cat; echo \"$EMULATED\"; exit 3",
        ]
        .join("\n"),
    );
    world.test(
        "differs.ll",
        ";@ stdout: same\n; program(llc): echo same\n; program(hod): echo other\n",
    );
    world.test("declines.ll", ";@ unsupported: vector-type\n; hod: decline vector-type\n");
    let report = irdiff::run(&config).unwrap();
    let all = report.summary(true);
    assert_eq!(
        rows(&report),
        [
            "UNSUPPORTED declines.ll bfd",
            "UNSUPPORTED declines.ll lld",
            "UNSUPPORTED declines.ll mold",
            "FAIL differs.ll bfd",
            "FAIL differs.ll lld",
            "FAIL differs.ll mold",
            "PASS seen.ll bfd",
            "PASS seen.ll lld",
            "PASS seen.ll mold",
        ],
        "{all}"
    );
    assert!(report.records.iter().all(|r| r.target == other_arch().triple()), "{all}");
    assert!(
        all.contains(&format!(
            "(emulated: {}",
            world.scratch.join("bin").join(format!("qemu-{}", other_arch().name())).display()
        )),
        "{all}"
    );
    assert!(report.notes.iter().all(|note| !note.contains("not run")), "{all}");

    // A hang is killed at six times the run limit, and says so.
    world.test("hangs.ll", "; program(hod): exec sleep 600\n");
    let mut short = config.clone();
    short.paths = vec![world.scratch.join("tests/hangs.ll")];
    short.run_limits.time = Duration::from_secs(1);
    short.linkers = vec!["bfd".to_owned()];
    let report = irdiff::run(&short).unwrap();
    assert_eq!(rows(&report), ["FAIL hangs.ll bfd"], "{}", report.summary(true));
    assert_eq!(
        details(&report, "hangs.ll"),
        ["the program of hod's object timed out after 6 s"],
        "{}",
        report.summary(true)
    );
}

// "Cross runs": without the emulator or the system root the other
// architecture's objects are built and not run, with a note naming what to
// install; under HOD_REQUIRE_TOOLS=1 that fails the run (exit status 3).
#[test]
fn cross_runs_need_the_emulator_and_the_system_root() {
    let world = World::new("irdiff-emulator-missing");
    let mut config = world.with_cross_tools();
    world.test("a.ll", "");
    world.without(&format!("qemu-{}", other_arch().name()));
    let report = irdiff::run(&config).unwrap();
    assert_eq!(rows(&report), ["SKIP a.ll -"], "{}", report.summary(true));
    let note = format!(
        "emulator 'qemu-{0}' not found (Debian package qemu-user); {0} programs are not run",
        other_arch().name()
    );
    assert_eq!(report.notes[1], note, "{}", report.summary(true));
    config.require_tools = true;
    let report = irdiff::run(&config).unwrap();
    assert_eq!(report.missing_tools, [note]);
    assert_eq!(report.exit_status(), 3);

    world.tool(&format!("qemu-{}", other_arch().name()), FAKE_QEMU);
    config.sysroot = Some(world.scratch.join("no-such-root"));
    let report = irdiff::run(&config).unwrap();
    assert_eq!(rows(&report), ["SKIP a.ll -"]);
    assert!(report.missing_tools[0].starts_with("system root '"), "{:?}", report.missing_tools);
    assert!(
        report.missing_tools[0].contains("(Debian package libc6-dev-"),
        "{:?}",
        report.missing_tools
    );
    config.sysroot = Some(world.scratch.join("sysroot"));
    let report = irdiff::run(&config).unwrap();
    assert_eq!(rows(&report), ["PASS a.ll bfd", "PASS a.ll lld", "PASS a.ll mold"]);
    // The host's GNU ld is not the cross one.
    world.without(&format!("{}-ld.bfd", prefix()));
    let report = irdiff::run(&config).unwrap();
    assert_eq!(rows(&report), ["PASS a.ll lld", "PASS a.ll mold"]);
    assert!(
        report.missing_tools[0]
            .starts_with(&format!("linker 'bfd' not found (Debian package binutils-")),
        "{:?}",
        report.missing_tools
    );
}

// "Cross runs": a target hod declines every test of is one note, not one
// line per test.
#[test]
fn a_target_hod_declines_wholesale_is_one_note() {
    let world = World::new("irdiff-wholesale");
    let config = world.with_cross_tools();
    for name in ["a.ll", "b.ll", "c.ll"] {
        world.test(name, ";@ unsupported: *\n; hod: decline codegen-not-implemented\n");
    }
    let report = irdiff::run(&config).unwrap();
    let summary = report.summary(false);
    assert_eq!(report.count(Outcome::Unsupported), 9, "{summary}");
    let note = format!(
        "irdiff: note: hod declined all 3 tests it was asked to build for {}; only the \
         reference side (llc, the linkers, the programs) was checked",
        other_arch().name()
    );
    assert!(summary.contains(&note), "{summary}");
    assert!(!summary.lines().any(|line| line.starts_with("UNSUPPORTED")), "{summary}");
    // One test hod builds: no such note.
    world.test("d.ll", "");
    let report = irdiff::run(&config).unwrap();
    assert!(!report.summary(false).contains("declined all"), "{}", report.summary(false));
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
    // What the fallback builds is built as the oracle builds: with its
    // selector ("Hybrid mode").
    world.test(
        "falls-back.ll",
        ";@ stdout: x\n; program: echo x\n; hod: decline inline-asm\n\
         ; program(llc): case \"$CODEGEN\" in *' -global-isel=0 -fast-isel=0'*) ;; *) echo \"$CODEGEN\" ;; esac\n",
    );
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

// "The witness": with `--witness` every test is also built by `llc -O2`,
// linked with the first linker and run; a program that does not behave as
// the reference's is an ERROR with every linker, whatever hod did. Off by
// default.
#[test]
#[allow(clippy::too_many_lines)]
fn the_witness() {
    let world = World::new("irdiff-witness");
    let triple = host().triple();
    world.test(
        "agrees.ll",
        &format!(
            ";@ stdout: x\n; program: echo x\n\
             ; program(O2): [ \"$CODEGEN\" = \"-O2 -filetype=obj -mtriple={triple} -relocation-model=pic\" ] || echo \"$CODEGEN\"\n"
        ),
    );
    world.test("stdout.ll", ";@ stdout: x\n; program: echo x\n; program(O2): echo more\n");
    world.test("status.ll", "; program(O2): exit 3\n");
    world.test("hangs.ll", "; program(O2): exec sleep 60\n");
    world.test("build.ll", "; llc(O2): fail\n");
    world.test("link.ll", "; link-fail: O2 bfd\n");
    // Only one linker links the witness: this one would fail.
    world.test("later-linker.ll", "; link-fail: O2 mold\n");
    // The first linker whose program of the oracle's objects is good: bfd
    // does not link the oracle's, so lld is asked.
    world.test("next-linker.ll", "; link-fail: llc bfd\n; program(O2,lld): exit 3\n");
    // The oracle is in doubt: nothing is said about hod.
    world.test("hod-wrong.ll", "; program(O2): exit 3\n; program(hod): exit 4\n");
    world.test("hod-declines.ll", "; program(O2): exit 3\n; hod: decline vector-type\n");
    // The directives are checked first; a confirmed oracle leaves hod's
    // outcome as it is.
    world.test("directives.ll", ";@ stdout: x\n; program(O2): exit 3\n");
    world.test("hod-fails.ll", "; program(hod): exit 4\n");
    world.test("unsupported.ll", ";@ unsupported: vector-type\n; hod: decline vector-type\n");
    let mut config = short(world.config());
    config.witness = true;
    config.keep = true;
    let report = irdiff::run(&config).unwrap();
    let all = report.summary(true);
    assert!(all.contains("; strict; witness; -j "), "{all}");
    for file in ["agrees.ll", "later-linker.ll"] {
        assert_eq!(outcomes(&report, file), [Outcome::Pass; 3], "{file}: {all}");
    }
    assert_eq!(outcomes(&report, "hod-fails.ll"), [Outcome::Fail; 3], "{all}");
    assert_eq!(outcomes(&report, "unsupported.ll"), [Outcome::Unsupported; 3], "{all}");
    assert_eq!(outcomes(&report, "next-linker.ll"), [Outcome::Error; 3], "{all}");
    assert_eq!(
        details(&report, "next-linker.ll"),
        [
            "oracle: link with bfd failed (exit status 1): bfd: error: undefined symbol: main",
            "witness: the program of llc -O2's object ended with exit status 3, \
             llc -O0's with exit status 0",
            "witness: the program of llc -O2's object ended with exit status 3, \
             llc -O0's with exit status 0",
        ]
    );
    for (file, records, detail) in [
        (
            "stdout.ll",
            3,
            "witness: the program of llc -O2's object wrote stdout that differs from llc -O0's \
             at line 2: expected the end of the output, got \"more\\n\"",
        ),
        (
            "status.ll",
            3,
            "witness: the program of llc -O2's object ended with exit status 3, \
             llc -O0's with exit status 0",
        ),
        ("hangs.ll", 3, "witness: the program of llc -O2's object timed out after 3 s"),
        ("build.ll", 1, "witness: llc -O2 failed (exit status 1): llc: error: cannot optimize"),
        (
            "link.ll",
            3,
            "witness: link with bfd failed (exit status 1): bfd: error: undefined symbol: main",
        ),
        (
            "hod-wrong.ll",
            3,
            "witness: the program of llc -O2's object ended with exit status 3, \
             llc -O0's with exit status 0",
        ),
        (
            "hod-declines.ll",
            3,
            "witness: the program of llc -O2's object ended with exit status 3, \
             llc -O0's with exit status 0",
        ),
        (
            "directives.ll",
            3,
            "oracle: the program of llc's object wrote stdout that differs from the directives \
             at line 1: expected \"x\\n\", got the end of the output",
        ),
    ] {
        assert_eq!(outcomes(&report, file), vec![Outcome::Error; records], "{file}: {all}");
        assert_eq!(details(&report, file), vec![detail; records], "{file}");
        assert!(details(&report, file)[0].starts_with(irdiff::engine::WITNESS) || records == 3);
        let witness = details(&report, file).iter().all(|d| d.starts_with(irdiff::engine::WITNESS));
        let kind = if witness { Failure::Witness } else { Failure::Comparison };
        assert!(failures(&report, file).iter().all(|f| *f == Some(kind)), "{file}");
    }
    // The witness is its own object and one program, by the first linker.
    let kept = report.kept.clone().expect("failing tests are kept");
    let dir = std::fs::read_dir(&kept)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.to_string_lossy().ends_with("status.ll"))
        .expect("the test's directory is kept");
    for file in ["oracle.o", "witness.o", "bfd/oracle", "bfd/witness"] {
        assert!(dir.join(file).is_file(), "{file} is kept");
    }
    assert!(!dir.join("lld/witness").exists() && !dir.join("lld/oracle").exists());
    std::fs::remove_dir_all(&kept).unwrap();

    // Alone, an oracle the witness does not confirm is exit status 3.
    let alone = World::new("irdiff-witness-alone");
    alone.test("status.ll", "; program(O2): exit 3\n");
    let mut config = alone.config();
    config.witness = true;
    assert_eq!(irdiff::run(&config).unwrap().exit_status(), 3);
    // Without `--witness` nobody asks llc -O2.
    let report = alone.run();
    assert_eq!(outcomes(&report, "status.ll"), [Outcome::Pass; 3], "{}", report.summary(true));
    assert!(report.summary(true).contains("; strict; -j "));
    // For a target that cannot run here the witness is still built.
    alone.test("build.ll", "; llc(O2): fail\n");
    config.target = Some(other_arch());
    let report = irdiff::run(&config).unwrap();
    assert_eq!(rows(&report), ["ERROR build.ll -", "SKIP status.ll -"], "{}", report.summary(true));
}

// "Test directives": `llc-args` reaches both compilers and the witness,
// `link: allow-warnings` takes `--fatal-warnings` off the link, and
// `stdout-no-newline` is output without a final newline.
#[test]
fn directives_reach_the_tools() {
    let world = World::new("irdiff-directives");
    let triple = host().triple();
    let tail = format!(
        "-filetype=obj -mtriple={triple} -relocation-model=pic -debugger-tune=gdb -function-sections"
    );
    world.test(
        "llc-args.ll",
        &format!(
            ";@ llc-args: -debugger-tune=gdb  -function-sections\n\
             ; program(llc): [ \"$CODEGEN\" = \"-O0 -global-isel=0 -fast-isel=0 {tail}\" ] || echo \"llc: $CODEGEN\"\n\
             ; program(O2): [ \"$CODEGEN\" = \"-O2 {tail}\" ] || echo \"O2: $CODEGEN\"\n\
             ; program(hod): [ \"$CODEGEN\" = \"-O0 {tail}\" ] || echo \"hod: $CODEGEN\"\n"
        ),
    );
    world.test("llc-args-bad.ll", ";@ llc-args: -O2 -filetype=asm\n");
    world.test("warns.ll", "; link-warn: bfd\n; link-warn: mold\n");
    world.test(
        "warns-allowed.ll",
        ";@ link: m allow-warnings\n;@ stdout:  -lm\n\
         ; link-warn: bfd\n; link-warn: mold\n\
         ; program: case \"$LINKER_FLAGS\" in *fatal-warnings*) echo fatal ;; esac\n\
         ; program: echo \"$LIBS\"\n",
    );
    world.test(
        "no-newline.ll",
        ";@ stdout: a\n;@ stdout-no-newline: b \n; program: printf 'a\\nb '\n",
    );
    world.test("only-no-newline.ll", ";@ stdout-no-newline: b\n; program: printf b\n");
    world.test("newline.ll", ";@ stdout-no-newline: b\n; program: echo b\n");
    world.test(
        "hod-newline.ll",
        ";@ stdout-no-newline: b\n; program: printf b\n; program(hod): echo\n",
    );
    let mut config = world.config();
    config.witness = true;
    let report = irdiff::run(&config).unwrap();
    let all = report.summary(true);
    for file in ["llc-args.ll", "warns-allowed.ll", "no-newline.ll", "only-no-newline.ll"] {
        assert_eq!(outcomes(&report, file), [Outcome::Pass; 3], "{file}: {all}");
    }
    assert_eq!(
        details(&report, "llc-args-bad.ll"),
        ["directives: line 1: llc argument '-O2' is the harness's to give"]
    );
    assert_eq!(
        outcomes(&report, "warns.ll"),
        [Outcome::Error, Outcome::Pass, Outcome::Error],
        "{all}"
    );
    assert_eq!(
        details(&report, "warns.ll")[0],
        "oracle: link with bfd failed (exit status 1): bfd: warning: the use of 'tmpnam' is dangerous"
    );
    assert_eq!(
        details(&report, "newline.ll"),
        ["oracle: the program of llc's object wrote stdout that differs from the directives \
          at line 1: expected \"b\", got \"b\\n\""; 3]
    );
    assert_eq!(outcomes(&report, "hod-newline.ll"), [Outcome::Fail; 3], "{all}");
    assert_eq!(
        details(&report, "hod-newline.ll")[0],
        "the program of hod's object wrote stdout that differs from llc's at line 1: \
         expected \"b\", got \"b\\n\""
    );
}

// "Test directives", "Several modules": each module is compiled on its
// own by each compiler and the objects are linked in order into one
// program. The first module hod does not build decides the test.
#[test]
#[allow(clippy::too_many_lines)]
fn several_modules() {
    let world = World::new("irdiff-modules");
    world.test(
        "pass.ll",
        "; a comment before the first module\n;@ stdout: from a\n;@ stdout: from b\n\n\
         ;@ module: a\n; program: echo from a\n;@ stdout: last\n\
         ;@ module: b.cpp\n; program: echo from b\n; program: echo last\n",
    );
    world.test("one.ll", ";@ stdout: only\n;@ module: only\n; program: echo only\n");
    world.test(
        "declines.ll",
        ";@ unsupported: vector-type\n;@ module: a\n;@ module: b\n; hod: decline vector-type\n",
    );
    world.test("unexpected.ll", ";@ module: a\n; hod: decline vector-type\n;@ module: b\n");
    // The first module that is not built decides: `b` declines before `c`
    // is tried.
    world.test(
        "first.ll",
        ";@ unsupported: vector-type\n;@ module: a\n;@ module: b\n; hod: decline vector-type\n\
         ;@ module: c\n; hod: crash\n",
    );
    world.test("crashes.ll", ";@ module: a\n;@ module: b\n; hod: crash\n");
    world.test("differs.ll", ";@ module: a\n;@ module: b\n; program(hod): exit 4\n");
    world.test("hod-link.ll", ";@ module: a\n;@ module: b\n; link-fail: hod lld\n");
    world.test("oracle-link.ll", ";@ module: a\n;@ module: b\n; link-fail: llc bfd\n");
    world.test("oracle-build.ll", ";@ module: a\n;@ module: b\n; llc: fail\n");
    world.test("witness.ll", ";@ module: a\n;@ module: b\n; program(O2): exit 3\n");
    world.test("witness-build.ll", ";@ module: a\n; llc(O2): fail\n;@ module: b\n");
    world.test("twice.ll", ";@ module: a\n;@ module: b\n;@ module: a\n");
    world.test("code-first.ll", "@g = global i32 0\n;@ module: a\n");
    let mut config = world.config();
    config.witness = true;
    config.keep = true;
    let report = irdiff::run(&config).unwrap();
    let all = report.summary(true);
    for file in ["pass.ll", "one.ll"] {
        assert_eq!(outcomes(&report, file), [Outcome::Pass; 3], "{file}: {all}");
    }
    for file in ["declines.ll", "first.ll"] {
        assert_eq!(outcomes(&report, file), [Outcome::Unsupported; 3], "{file}: {all}");
    }
    for (file, outcome, detail) in [
        (
            "unexpected.ll",
            Outcome::Fail,
            "hod declined the module and the test has no 'unsupported: vector-type' directive",
        ),
        (
            "differs.ll",
            Outcome::Fail,
            "the program of hod's object ended with exit status 4, llc's with exit status 0",
        ),
        (
            "witness.ll",
            Outcome::Error,
            "witness: the program of llc -O2's object ended with exit status 3, \
             llc -O0's with exit status 0",
        ),
    ] {
        assert_eq!(outcomes(&report, file), [outcome; 3], "{file}: {all}");
        assert_eq!(details(&report, file), [detail; 3], "{file}");
    }
    assert_eq!(outcomes(&report, "crashes.ll"), [Outcome::Fail; 3], "{all}");
    assert!(
        details(&report, "crashes.ll")[0]
            .starts_with("module 'b': hod was terminated by signal 11"),
        "{all}"
    );
    assert_eq!(
        outcomes(&report, "hod-link.ll"),
        [Outcome::Pass, Outcome::Fail, Outcome::Pass],
        "{all}"
    );
    assert_eq!(
        details(&report, "hod-link.ll")[1],
        "hod's object: link with lld failed (exit status 1): lld: error: undefined symbol: main"
    );
    assert_eq!(
        outcomes(&report, "oracle-link.ll"),
        [Outcome::Error, Outcome::Pass, Outcome::Pass],
        "{all}"
    );
    for (file, detail) in [
        (
            "oracle-build.ll",
            "oracle: module 'b': llc failed (exit status 1): llc: error: bad module",
        ),
        (
            "witness-build.ll",
            "witness: module 'a': llc -O2 failed (exit status 1): llc: error: cannot optimize",
        ),
        ("twice.ll", "directives: line 3: module 'a' is named twice"),
        (
            "code-first.ll",
            "directives: line 2: module 'a' starts after code: only directives, comments and \
             empty lines may come before the first 'module' directive",
        ),
    ] {
        assert_eq!(outcomes(&report, file), [Outcome::Error], "{file}: {all}");
        assert_eq!(details(&report, file), [detail], "{file}");
    }
    // Counted per test, not per module.
    assert!(
        all.contains("irdiff: coverage: hod built 6 of 9 objects; declined vector-type: 3\n"),
        "{all}"
    );

    // "Work directory": the modules, each with its objects and its log; a
    // module's file has the test file's line numbers.
    let kept = report.kept.clone().expect("failing tests are kept");
    let dir = std::fs::read_dir(&kept)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.to_string_lossy().ends_with("differs.ll"))
        .expect("the test's directory is kept");
    for file in [
        "modules/a.ll",
        "modules/a.oracle.o",
        "modules/a.witness.o",
        "modules/a.hod.o",
        "modules/a.coverage.tsv",
        "modules/b.ll",
        "modules/b.hod.o",
        "mold/hod",
    ] {
        assert!(dir.join(file).is_file(), "{file} is kept");
    }
    assert!(!dir.join("oracle.o").exists());
    assert_eq!(
        std::fs::read_to_string(dir.join("modules/b.ll")).unwrap(),
        "\n;@ module: b\n; program(hod): exit 4\n"
    );
    std::fs::remove_dir_all(&kept).unwrap();

    // "Hybrid mode": a program of objects hod built and objects its
    // fallback built; the test is the fallback's, with the first reason.
    let hybrid = World::new("irdiff-modules-hybrid");
    hybrid.test(
        "mixed.ll",
        ";@ stdout: hod llc llc\n\
         ;@ module: a\n; program(hod): printf 'hod '\n; program(llc): printf 'llc '\n\
         ;@ module: b\n; hod: decline inline-asm\n; program(hod): printf 'hod '\n; program(llc): printf 'llc '\n\
         ;@ module: c\n; hod: decline vector-type\n; program(hod): echo hod\n; program(llc): echo llc\n",
    );
    let mut config = hybrid.config();
    config.mode = Mode::Hybrid;
    let report = irdiff::run(&config).unwrap();
    // The oracle's program prints `llc llc llc`: the directives say what
    // hod's prints, so the oracle's side is an ERROR that names both.
    assert_eq!(outcomes(&report, "mixed.ll"), [Outcome::Error; 3], "{}", report.summary(true));
    let record = &report.records[0];
    assert_eq!(
        (record.built_by, record.reason.as_deref()),
        (BuiltBy::Fallback, Some("inline-asm"))
    );
    assert_eq!(
        record.detail,
        "oracle: the program of llc's object wrote stdout that differs from the directives \
         at line 1: expected \"hod llc llc\\n\", got \"llc llc llc\\n\""
    );
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
    // The jobs shown: the last field, without the jobserver pool that
    // follows it when this test runs under one ("Jobserver").
    let jobs_shown = |report: &Report| {
        let last = report.configuration.rsplit("; ").next().unwrap_or_default();
        last.split(" (jobserver ").next().unwrap_or_default().to_owned()
    };
    let (serial, serial_file, serial_summary) = run(1);
    assert_eq!(jobs_shown(&serial), "-j 1", "{}", serial.configuration);
    for jobs in [4, 16, 1000] {
        let (parallel, file, summary) = run(jobs);
        assert_eq!(parallel.records, serial.records, "-j {jobs}");
        assert_eq!(file, serial_file, "-j {jobs}");
        assert_eq!(summary, serial_summary, "-j {jobs}");
        // The cap.
        let shown = jobs.min(irdiff::MAX_JOBS);
        assert_eq!(jobs_shown(&parallel), format!("-j {shown}"), "{}", parallel.configuration);
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
    assert!(verbose.stdout_text().contains("; linkers mold; strict; -j 2"));

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

// "Test directives", `input: pre-opt` (WP-105): the oracle and the
// witness build `opt -passes='default<O0>'`'s output of the test, hod and
// its fallback the test itself; hybrid mode's fallback runs `opt` first.
#[test]
fn a_pre_opt_test_goes_through_opt() {
    let world = World::new("irdiff-pre-opt");
    world.tool("opt-22", FAKE_OPT);
    let body = ";@ stdout: opted\n; opt-program(llc): echo opted\n; opt-program(O2): echo opted\n\
                ; program(hod): echo opted\n";
    world.test("pre-opt.ll", &format!(";@ input: pre-opt\n{body}"));
    // Without the directive the oracle builds the test as it is.
    world.test("plain.ll", body);
    world.test("broken.ll", ";@ input: pre-opt\n; opt: fail\n");
    world.test(
        "falls-back.ll",
        &format!(";@ input: pre-opt\n;@ unsupported: coro\n; hod: decline coro\n{body}"),
    );
    let mut config = world.config();
    config.witness = true;
    let report = irdiff::run(&config).unwrap();
    let all = report.summary(true);
    assert_eq!(outcomes(&report, "pre-opt.ll"), [Outcome::Pass; 3], "{all}");
    assert_eq!(outcomes(&report, "plain.ll"), [Outcome::Error; 3], "{all}");
    assert!(details(&report, "plain.ll")[0].starts_with("oracle: the program of llc's object"));
    assert_eq!(outcomes(&report, "broken.ll"), [Outcome::Error], "{all}");
    assert_eq!(
        details(&report, "broken.ll"),
        ["oracle: opt failed (exit status 1): opt: error: bad module"]
    );
    // Strict: the decline is expected.
    assert_eq!(outcomes(&report, "falls-back.ll"), [Outcome::Unsupported; 3], "{all}");

    config.witness = false;
    config.mode = Mode::Hybrid;
    let report = irdiff::run(&config).unwrap();
    let all = report.summary(true);
    assert_eq!(outcomes(&report, "falls-back.ll"), [Outcome::Pass; 3], "{all}");
    let record = report.records.iter().find(|r| file_name(&r.test) == "falls-back.ll").unwrap();
    assert_eq!((record.built_by, record.reason.as_deref()), (BuiltBy::Fallback, Some("coro")));

    // An oracle without its opt is the harness's trouble.
    world.without("opt-22");
    let report = world.run();
    assert_eq!(outcomes(&report, "pre-opt.ll"), [Outcome::Error]);
    let detail = details(&report, "pre-opt.ll")[0];
    assert!(
        detail.starts_with("oracle: no opt beside '") && detail.ends_with("llc-22'"),
        "{detail}"
    );
    assert_eq!(failures(&report, "pre-opt.ll"), [Some(Failure::Harness)]);
}

// "Test directives", `dialect` (WP-105): a test in one LLVM's dialect runs
// only with that LLVM's oracle, and is SKIP with the other.
#[test]
fn a_dialect_test_runs_only_with_its_llvm() {
    let world = World::new("irdiff-dialect");
    world.tool("llc-19", FAKE_LLC);
    world.test("d19.ll", ";@ dialect: 19\n;@ stdout: x\n; program: echo x\n");
    world.test("d22.ll", ";@ dialect: 22\n;@ stdout: x\n; program: echo x\n");
    world.test("both.ll", ";@ stdout: x\n; program: echo x\n");
    let mut config = world.config();
    config.linkers = vec!["bfd".to_owned()];
    let report = irdiff::run(&config).unwrap();
    assert_eq!(rows(&report), ["PASS both.ll bfd", "SKIP d19.ll -", "PASS d22.ll bfd"]);
    assert_eq!(details(&report, "d19.ll"), ["in the LLVM 19 dialect (this run: 22)"]);
    config.llvm = Some("19".to_owned());
    let report = irdiff::run(&config).unwrap();
    assert_eq!(rows(&report), ["PASS both.ll bfd", "PASS d19.ll bfd", "SKIP d22.ll -"]);
    assert_eq!(details(&report, "d22.ll"), ["in the LLVM 22 dialect (this run: 19)"]);
}

// "Test directives", `oracle-skip` (WP-105): with an LLVM the directive
// names, the oracle is neither built nor run, nor the witness: the
// program of hod's object is compared with the directives only.
#[test]
fn an_oracle_skip_compares_with_the_directives_only() {
    let world = World::new("irdiff-oracle-skip");
    // The oracle's and the witness's builds would fail: they must not run.
    let wrong = "; llc: fail\n";
    world.test(
        "right.ll",
        &format!(
            ";@ oracle-skip(llvm=19,22): the oracle is wrong\n;@ stdout: right\n{wrong}\
                 ; program(hod): echo right\n"
        ),
    );
    world.test(
        "differs.ll",
        &format!(
            ";@ oracle-skip(llvm=22): the oracle is wrong\n;@ stdout: right\n{wrong}\
                 ; program(hod): echo wrong\n"
        ),
    );
    world.test(
        "exit.ll",
        ";@ oracle-skip(llvm=22): the oracle is wrong\n; program(hod): exit 3\n; program(llc): exit 3\n",
    );
    world.test(
        "xfail.ll",
        ";@ oracle-skip(llvm=22): the oracle is wrong\n;@ xfail(mold): mold\n\
         ;@ stdout: right\n; program(hod): echo right\n; program(mold): echo more\n",
    );
    // Another LLVM's mark: the oracle is used, and fails.
    world.test("other.ll", &format!(";@ oracle-skip(llvm=19): the oracle is wrong\n{wrong}"));
    let mut config = world.config();
    config.witness = true;
    let report = irdiff::run(&config).unwrap();
    let all = report.summary(true);
    assert_eq!(outcomes(&report, "right.ll"), [Outcome::Pass; 3], "{all}");
    assert_eq!(details(&report, "right.ll"), ["oracle skipped: the oracle is wrong"; 3]);
    assert_eq!(outcomes(&report, "differs.ll"), [Outcome::Fail; 3], "{all}");
    assert!(
        details(&report, "differs.ll")
            .iter()
            .all(|d| d.starts_with(
                "the program of hod's object wrote stdout that differs from the directives"
            ) && d.ends_with("; oracle skipped: the oracle is wrong")),
        "{all}"
    );
    assert_eq!(failures(&report, "differs.ll"), [Some(Failure::Comparison); 3]);
    assert_eq!(outcomes(&report, "exit.ll"), [Outcome::Fail; 3], "{all}");
    assert!(
        details(&report, "exit.ll")[0].starts_with(
            "the program of hod's object ended with exit status 3, the directives say exit status 0"
        ),
        "{all}"
    );
    assert_eq!(
        outcomes(&report, "xfail.ll"),
        [Outcome::Pass, Outcome::Pass, Outcome::Xfail],
        "{all}"
    );
    assert_eq!(outcomes(&report, "other.ll"), [Outcome::Error], "{all}");
    assert!(details(&report, "other.ll")[0].starts_with("oracle: llc failed"), "{all}");
}
