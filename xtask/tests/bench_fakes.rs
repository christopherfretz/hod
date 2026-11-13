//! `cargo xtask bench` (`spec/harness.md` "bench") with fake tools: a
//! compiler, an `llc`, a `hod` and a GNU `time` that are shell scripts, so
//! that every outcome, and every verdict of the gate, can be provoked on
//! any Linux machine, quickly, without LLVM.
//!
//! How the fakes work. A "source" is any text; the fake compiler turns it
//! into "IR" (one `define` line, then the source). Comment lines in it make
//! a tool misbehave: `; llc: fail|hang|silent`,
//! `; hod: build|crash|nolog|badphases`, and `fail` makes the compiler
//! fail. Files beside the tools steer what cannot be in an input:
//! `llc.delay` and `hod.delay` (seconds a run sleeps; hod's only in strict
//! mode), `hod.slow-first` (how many strict runs sleep `hod.delay`; then
//! none does), `hod.decline-all`, and `rss.<tool>` (the peak memory the
//! fake `time` reports for it). Every run is appended to `calls`.
#![cfg(target_os = "linux")]

mod common;

use std::path::PathBuf;
use std::time::Duration;

use common::Scratch;
use hod_testutil::write_script;
use xtask::bench::check::Verdict;
use xtask::bench::record::{Machine, Ratio, Record, Results, Status, Tool};
use xtask::bench::{self, Action, Config, Run};
use xtask::irdiff::{Arch, Search};

/// How much a time must differ to count, and how long a slow run sleeps.
/// A fake run takes milliseconds on a quiet machine; at load 25 to 32 a
/// fork and an exec have taken a large part of a second, so a run of
/// fakes is assumed to take under 0.5 s there. Then a fast run (under the
/// baseline's fastest + 1 s) is never judged slow, and a slow one (1.5 s
/// more, against a baseline under 0.5 s) always is. With 100 ms and 0.3 s
/// both failed on busy machines.
const SLACK_US: u64 = 1_000_000;
const SLOW: &str = "1.5";

const FAKE_COMPILER: &str = r#"PATH=/usr/bin:/bin
if [ "$1" = --version ]; then echo "fake clang version 22.0.0"; exit 0; fi
out= src= args=
while [ $# -gt 0 ]; do
  case "$1" in
    -o) out=$2; shift ;;
    -*) args="$args $1" ;;
    *) src=$1 ;;
  esac
  shift
done
if grep -q '^fail' "$src"; then echo "fake clang: error: cannot compile $src" >&2; exit 1; fi
{ echo "define void @f() ;$args"; cat "$src"; } > "$out""#;

const FAKE_LLC: &str = r#"PATH=/usr/bin:/bin
dir=$(dirname "$0")
if [ "$1" = --version ]; then echo "fake LLVM version 22.0.0"; exit 0; fi
out= in= args=
while [ $# -gt 0 ]; do
  case "$1" in
    -o) out=$2; shift ;;
    -*) args="$args $1" ;;
    *) in=$1 ;;
  esac
  shift
done
echo "llc$args $(basename "$in")" >> "$dir/calls"
if [ -n "${MAKEFLAGS+set}" ]; then echo "fake llc: MAKEFLAGS is set" >&2; exit 1; fi
if [ -f "$dir/llc.delay" ]; then sleep "$(cat "$dir/llc.delay")"; fi
if grep -q '^; llc: hang' "$in"; then exec sleep 60; fi
if grep -q '^; llc: fail' "$in"; then echo "llc: error: bad module" >&2; exit 1; fi
if grep -q '^; llc: silent' "$in"; then exit 0; fi
{ echo "built-by llc"; cat "$in"; } > "$out""#;

/// Refuses to run with any of hod's settings in the environment, or with
/// both or neither of `--strict` and `--fallback`: the harness configures
/// it by options alone.
const FAKE_HOD: &str = r#"PATH=/usr/bin:/bin
dir=$(dirname "$0")
if [ "$1" = --version ]; then echo "hod version 9.9.9"; exit 0; fi
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
echo "hod ${strict:+strict}${fallback:+fallback}$args $(basename "$in")" >> "$dir/calls"
if [ -n "$strict" ] && [ -f "$dir/hod.delay" ]; then
  if [ -f "$dir/hod.slow-first" ]; then
    left=$(cat "$dir/hod.slow-first")
    if [ "$left" -gt 0 ]; then echo $((left - 1)) > "$dir/hod.slow-first"; sleep "$(cat "$dir/hod.delay")"; fi
  else
    sleep "$(cat "$dir/hod.delay")"
  fi
fi
record() { printf '%s\t%s\ttext\t1\t%s\t-\t5\t-\n' "$1" "$2" "$out" >> "$log"; }
phases() {
  if [ -n "$HOD_TIME_PHASES" ]; then printf "$1" >> "$HOD_TIME_PHASES"; fi
}
if grep -q '^; hod: crash' "$in"; then kill -SEGV $$; fi
if grep -q '^; hod: build' "$in" && [ ! -f "$dir/hod.decline-all" ]; then
  { echo "built-by hod"; cat "$in"; } > "$out"
  if grep -q '^; hod: nolog' "$in"; then exit 0; fi
  if grep -q '^; hod: badphases' "$in"; then phases 'read 12\n'; else phases 'read\t12\ncompile\t30\nwrite\t7\ntotal\t50\n'; fi
  record hod -
  exit 0
fi
if [ -n "$fallback" ]; then
  "$fallback" $args -o "$out" "$in" || exit 1
  phases 'read\t12\ncompile\t1\nfallback\t40\ntotal\t55\n'
  record fallback codegen-not-implemented
  exit 0
fi
phases 'read\t12\ncompile\t1\ntotal\t14\n'
record error codegen-not-implemented
echo "hod: error: unsupported construct 'codegen-not-implemented' (strict mode: no fallback)" >&2
exit 2"#;

/// A stand-in for GNU time: runs the command, then writes the report GNU
/// time would, with the peak memory `rss.<tool>` says (else 1000 KiB).
const FAKE_TIME: &str = r#"PATH=/usr/bin:/bin
dir=$(dirname "$0")
out=
while [ $# -gt 0 ]; do
  case "$1" in
    -f) shift ;;
    -o) out=$2; shift ;;
    --) shift; break ;;
  esac
  shift
done
tool=$(basename "$1")
"$@"
status=$?
if [ $status -gt 128 ]; then
  echo "Command terminated by signal $((status - 128))" > "$out"; status=$((status - 128))
elif [ $status -ne 0 ]; then
  echo "Command exited with non-zero status $status" > "$out"
else
  : > "$out"
fi
rss=1000
if [ -f "$dir/rss.$tool" ]; then rss=$(cat "$dir/rss.$tool"); fi
echo "$rss 0.02 0.01" >> "$out"
exit $status"#;

/// A stand-in for valgrind's callgrind: runs the command and writes an
/// output file whose summary is 4000 instructions for `llc` and 1000 for
/// hod, which it refuses to run with other than one thread.
const FAKE_VALGRIND: &str = r#"PATH=/usr/bin:/bin
out=
while [ $# -gt 0 ]; do
  case "$1" in
    --callgrind-out-file=*) out=${1#--callgrind-out-file=} ;;
    --*) ;;
    *) break ;;
  esac
  shift
done
tool=$(basename "$1")
if [ "$tool" = hod ] && [ "${HOD_THREADS-}" != 1 ]; then echo "fake valgrind: HOD_THREADS" >&2; exit 1; fi
"$@"
status=$?
count=4000
if [ "$tool" = hod ]; then count=1000; fi
printf 'version: 1\ncreator: fake\nevents: Ir\nsummary: %s\n' "$count" > "$out"
exit $status"#;

const MANIFEST: &str = "\
common = -O0 -S -emit-llvm

[plain]
class = small
source = plain.c
flags = -std=c17

[built]
class = typical
source = built.cpp
flags = -std=c++23
";

/// A machine made of fakes: a checkout with a manifest and two sources
/// (`plain`, which hod declines, and `built`, which it builds), the tools,
/// and a build directory.
struct World {
    scratch: Scratch,
}

impl World {
    fn new(label: &str) -> Self {
        Self::with_manifest(label, MANIFEST)
    }

    fn with_manifest(label: &str, manifest: &str) -> Self {
        let world = Self { scratch: Scratch::new(label) };
        for dir in ["bin", "build", "ws/bench/src"] {
            std::fs::create_dir_all(world.scratch.join(dir)).unwrap();
        }
        for (name, body) in [
            ("llc-22", FAKE_LLC),
            ("clang-22", FAKE_COMPILER),
            ("clang++-22", FAKE_COMPILER),
            ("hod", FAKE_HOD),
            ("time", FAKE_TIME),
        ] {
            write_script(&world.bin(name), body);
        }
        std::fs::write(world.scratch.join("ws/bench/manifest"), manifest).unwrap();
        world.source("plain.c", "; a module hod declines\n");
        world.source("built.cpp", "; hod: build\n");
        world
    }

    fn bin(&self, name: &str) -> PathBuf {
        self.scratch.join("bin").join(name)
    }

    fn source(&self, name: &str, text: &str) {
        std::fs::write(self.scratch.join("ws/bench/src").join(name), text).unwrap();
    }

    /// Writes a control file beside the tools.
    fn control(&self, name: &str, value: &str) {
        std::fs::write(self.bin(name), value).unwrap();
    }

    fn clear(&self, name: &str) {
        let _ = std::fs::remove_file(self.bin(name));
    }

    fn calls(&self) -> Vec<String> {
        let text = std::fs::read_to_string(self.bin("calls")).unwrap_or_default();
        text.lines().map(str::to_owned).collect()
    }

    fn baseline(&self) -> PathBuf {
        self.scratch.join("ws/bench/baselines/test-machine-4c.tsv")
    }

    /// The command's defaults on this machine of fakes, two runs each.
    /// The thresholds are those of the contract, except that a time must
    /// differ by [`SLACK_US`] to count and that the reference tool may
    /// drift by any factor: the fakes run for a few milliseconds, and
    /// those vary by more than 5% on a busy machine. The tests that judge
    /// time make a tool sleep [`SLOW`] seconds.
    fn config(&self, action: Action) -> Config {
        let mut config = Config::new(self.scratch.join("ws"), self.scratch.join("build"));
        config.action = action;
        config.search = Search {
            path: self.scratch.join("bin").into_os_string(),
            llvm_root: self.scratch.join("no-llvm"),
        };
        config.hod = Some(self.bin("hod"));
        config.time = self.bin("time");
        config.machine = Machine {
            id: "test-machine-4c".to_owned(),
            cpu: "test machine".to_owned(),
            cores: 4,
            arch: "test".to_owned(),
        };
        config.runs = 2;
        config.limits.time = Duration::from_secs(20);
        config.thresholds.time_slack_us = SLACK_US;
        config.thresholds.max_drift_bp = 10_000_000;
        config.thresholds.max_spread_bp = 10_000_000;
        config
    }

    #[allow(clippy::unused_self)]
    fn run(&self, config: &Config) -> Run {
        bench::run(config).unwrap_or_else(|e| panic!("the run could not be made: {e}"))
    }

    fn record_baseline(&self) {
        let mut config = self.config(Action::UpdateBaseline);
        config.reason = Some("the first baseline".to_owned());
        let run = self.run(&config);
        assert_eq!(
            (run.exit_status(), run.updated.clone()),
            (0, Some(self.baseline())),
            "{}",
            run.summary(true)
        );
    }
}

fn record<'a>(run: &'a Run, input: &str, tool: Tool) -> &'a Record {
    let results = run.results.as_ref().expect("the run measured");
    results.find(input, tool).unwrap_or_else(|| panic!("no record of {input} with {}", tool.name()))
}

fn verdicts(run: &Run) -> Vec<(String, Verdict, u32)> {
    let check = run.check.as_ref().expect("the run checked");
    check.judged.iter().map(|j| (j.input.clone(), j.verdict.clone(), j.remeasured)).collect()
}

// "Inputs", "Measurement", "Ratios and throughput", "Results file": every
// input of the manifest, with and without debug information, measured
// with each tool; fastest of N; ratios to llc; hod's decline reported as a
// decline; the file reads back as the run.
#[test]
#[allow(clippy::too_many_lines)]
fn a_run_measures_every_tool() {
    let world = World::new("bench-run");
    let run = world.run(&world.config(Action::Measure));
    let summary = run.summary(true);
    assert_eq!(run.exit_status(), 0, "{summary}");
    let results = run.results.as_ref().unwrap();
    let names: Vec<String> =
        results.records.iter().map(|r| format!("{} {}", r.input, r.tool.name())).collect();
    let expected: Vec<String> = ["plain", "plain-g", "built", "built-g"]
        .iter()
        .flat_map(|input| ["llc", "hod", "hod+fallback"].map(|tool| format!("{input} {tool}")))
        .collect();
    assert_eq!(names, expected);

    // The header says where, when and with what.
    for (key, value) in [
        ("machine", "test-machine-4c"),
        ("cores", "4"),
        ("llc_version", "LLVM version 22.0.0"),
        ("hod_version", "hod version 9.9.9"),
        ("hod_build", "given with --hod"),
        ("producer", "fake clang version 22.0.0"),
        ("runs", "2"),
    ] {
        assert_eq!(results.get(key), Some(value), "{key}");
    }
    assert!(results.get("date").is_some_and(|date| date.len() == 20 && date.ends_with('Z')));
    assert!(results.get("usage").unwrap().starts_with("gnu-time ("));
    assert!(results.get("floor_us").is_some_and(|us| us.parse::<u64>().is_ok_and(|us| us > 0)));

    // llc: absolute numbers and throughput.
    let llc = record(&run, "plain-g", Tool::Llc);
    assert_eq!((llc.status, llc.runs, llc.debug), (Status::Ok, 2, true));
    let (min, median, max) =
        (llc.wall_us_min.unwrap(), llc.wall_us_median.unwrap(), llc.wall_us_max.unwrap());
    assert!(0 < min && min <= median && median <= max, "{llc:?}");
    assert_eq!(
        (llc.cpu_us, llc.rss_kib, llc.rss_kib_min, llc.rss_kib_max),
        (Some(30_000), Some(1000), Some(1000), Some(1000))
    );
    let ir = std::fs::read(world.scratch.join("build/bench/inputs/clang22/plain-g.ll")).unwrap();
    assert_eq!(
        String::from_utf8_lossy(&ir),
        "define void @f() ; -O0 -S -emit-llvm -std=c17 -g\n; a module hod declines\n"
    );
    assert_eq!((llc.ir_bytes, llc.functions), (Some(ir.len() as u64), Some(1)));
    assert_eq!(llc.ir_hash.as_deref(), Some(bench::record::fnv1a64(&ir).as_str()));
    assert_eq!(llc.object_bytes, Some(ir.len() as u64 + "built-by llc\n".len() as u64));
    assert_eq!(llc.ir_bytes_per_s(), Some(ir.len() as u64 * 1_000_000 / min));
    assert_eq!(llc.wall_ratio, None, "the reference has no ratio to itself");

    // hod alone declines the module: a decline with its reason, no
    // object, no ratio and no throughput. Never a build that took no time.
    let declined = record(&run, "plain-g", Tool::Hod);
    assert_eq!(
        (declined.status, declined.detail.as_str(), declined.runs),
        (Status::Declined, "codegen-not-implemented", 2)
    );
    assert_eq!(
        (declined.object_bytes, declined.wall_ratio, declined.ir_bytes_per_s()),
        (None, None, None)
    );
    assert!(declined.wall_us_min.is_some(), "what declining cost is kept");
    assert!(summary.contains("  declined: codegen-not-implemented (in "), "{summary}");
    // With its fallback it produces llc's object, at llc's price and more.
    let hybrid = record(&run, "plain-g", Tool::HodFallback);
    assert_eq!(
        (hybrid.status, hybrid.detail.as_str()),
        (Status::Fallback, "codegen-not-implemented")
    );
    assert_eq!(hybrid.wall_ratio, Ratio::of(hybrid.wall_us_min.unwrap(), min));
    assert_eq!(
        (hybrid.rss_ratio, hybrid.size_ratio, hybrid.cpu_ratio),
        (Ratio::of(1, 1), Ratio::of(1, 1), Ratio::of(1, 1))
    );
    assert_eq!(hybrid.phases.last(), Some(&("total".to_owned(), 55)));

    // What hod builds is a time, with its ratios and its phases.
    let built = record(&run, "built", Tool::Hod);
    assert_eq!(
        (built.status, built.detail.as_str(), built.class.name()),
        (Status::Ok, "", "typical")
    );
    let reference = record(&run, "built", Tool::Llc);
    assert_eq!(
        built.wall_ratio,
        Ratio::of(built.wall_us_min.unwrap(), reference.wall_us_min.unwrap())
    );
    assert_eq!(
        built.size_ratio,
        Ratio::of(built.object_bytes.unwrap(), reference.object_bytes.unwrap())
    );
    assert_eq!(
        built.phases,
        [("read", 12), ("compile", 30), ("write", 7), ("total", 50)]
            .map(|(n, us)| (n.to_owned(), us))
    );
    assert!(summary.contains("bench: phases: built hod: read 0.01 ms, compile 0.03 ms, write 0.00 ms, total 0.05 ms\n"), "{summary}");
    assert!(summary.contains("bench: llc built 4; hod built 2, declined 2; hod+fallback built 2, fell back 2; 0 errors\n"), "{summary}");

    // A round runs each tool once: llc, hod, hod with its fallback (which
    // runs llc), then the next round. The tools get the same arguments,
    // but for what `llc` alone is told on this machine (`llc_options`:
    // the selector on an x86-64 host, nothing on an AArch64 one).
    let args = "-O0 -filetype=obj -relocation-model=pic";
    let mut own = String::new();
    for option in world.config(Action::Measure).llc_options() {
        own.push(' ');
        own.push_str(&option);
    }
    let round = [
        format!("llc {args}{own} plain.ll"),
        format!("hod strict {args} plain.ll"),
        format!("hod fallback {args} plain.ll"),
        format!("llc {args} plain.ll"),
    ];
    assert_eq!(world.calls()[..8], [round.clone(), round].concat());
    assert_eq!(world.calls().len(), 4 * 2 * 4 - 2 * 2, "hod builds 'built' without its fallback");

    // The results file is the run; the work directory is gone.
    let path = world.scratch.join("build/bench/results-llvm22.tsv");
    assert_eq!(run.results_path.as_deref(), Some(path.as_path()));
    assert_eq!(&Results::read(&path).unwrap(), results);
    assert!(summary.ends_with(&format!("bench: results: {}\n", path.display())), "{summary}");
    let left: Vec<String> = std::fs::read_dir(world.scratch.join("build/bench"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("run-"))
        .collect();
    assert_eq!(left, Vec::<String>::new());

    // --filter and --results; inputs that are up to date are not made again.
    let mut config = world.config(Action::Measure);
    (config.filters, config.runs) = (vec!["lain".to_owned()], 1);
    config.results = Some(world.scratch.join("elsewhere/r.tsv"));
    let filtered = world.run(&config);
    let inputs: Vec<&str> =
        filtered.results.as_ref().unwrap().records.iter().map(|r| r.input.as_str()).collect();
    assert_eq!(inputs, ["plain", "plain", "plain", "plain-g", "plain-g", "plain-g"]);
    assert_eq!(
        record(&filtered, "plain", Tool::Llc).ir_hash,
        record(&run, "plain", Tool::Llc).ir_hash
    );
    assert!(world.scratch.join("elsewhere/r.tsv").is_file());
    config.filters = vec!["absent".to_owned()];
    assert_eq!(bench::run(&config).unwrap_err(), "no input's name contains 'absent'");
}

// "Outcomes": a tool that fails, is killed, hangs or lies is an `error`
// record with no numbers, and the run's exit status is 3. Never a zero.
#[test]
fn failures_are_error_lines_not_zeros() {
    let manifest = "common = -O0\n".to_owned()
        + &["llcfail", "silent", "crash", "nolog", "badphases", "nocompile", "fine"]
            .map(|name| format!("[{name}]\nclass = small\nsource = {name}.c\n"))
            .concat();
    let world = World::with_manifest("bench-errors", &manifest);
    world.source("llcfail.c", "; llc: fail\n");
    world.source("silent.c", "; llc: silent\n");
    world.source("crash.c", "; hod: crash\n");
    world.source("nolog.c", "; hod: build\n; hod: nolog\n");
    world.source("badphases.c", "; hod: build\n; hod: badphases\n");
    world.source("nocompile.c", "fail\n");
    world.source("fine.c", "; hod: build\n");
    let mut config = world.config(Action::Measure);
    config.filters = ["llcfail", "silent", "crash", "nolog", "badphases", "nocompile", "fine"]
        .map(str::to_owned)
        .to_vec();
    let run = world.run(&config);
    let summary = run.summary(false);
    assert_eq!(run.exit_status(), 3, "{summary}");

    let error = |input: &str, tool: Tool| {
        let record = record(&run, input, tool);
        assert_eq!(record.status, Status::Error, "{input} {}", tool.name());
        assert_eq!(
            (record.runs, record.wall_us_min, record.rss_kib, record.object_bytes),
            (0, None, None, None)
        );
        assert_eq!((record.wall_ratio, record.ir_bytes_per_s()), (None, None));
        assert!(record.ir_bytes.is_some(), "the input is still identified");
        record.detail.clone()
    };
    assert_eq!(error("llcfail", Tool::Llc), "llc exited with status 1: llc: error: bad module");
    assert_eq!(
        error("llcfail", Tool::HodFallback),
        "hod+fallback exited with status 1: llc: error: bad module"
    );
    assert_eq!(
        record(&run, "llcfail", Tool::Hod).status,
        Status::Declined,
        "one tool's failure is its own"
    );
    assert_eq!(error("silent", Tool::Llc), "llc exited with status 0 and wrote no object");
    assert!(error("crash", Tool::Hod).starts_with("hod was terminated by signal 11"));
    assert!(
        error("crash", Tool::HodFallback).starts_with("hod+fallback was terminated by signal 11")
    );
    assert_eq!(record(&run, "crash", Tool::Llc).status, Status::Ok);
    assert_eq!(error("nolog", Tool::Hod), "hod exited with status 0 and wrote no coverage record");
    assert_eq!(error("badphases-g", Tool::Hod), "hod wrote a malformed phase report: 'read 12'");
    // A failing tool is run once, not N times.
    assert_eq!(
        world
            .calls()
            .iter()
            .filter(|call| call.starts_with("llc ") && call.ends_with(" llcfail.ll"))
            .count(),
        2
    );

    // An input that cannot be produced is one error record.
    let results = run.results.as_ref().unwrap();
    let unmade: Vec<&Record> = results.records.iter().filter(|r| r.input == "nocompile").collect();
    assert_eq!(unmade.len(), 1);
    assert_eq!(
        (unmade[0].status, unmade[0].tool, unmade[0].ir_bytes),
        (Status::Error, Tool::Generate, None)
    );
    assert_eq!(
        unmade[0].detail,
        "the compiler exited with status 1: fake clang: error: cannot compile nocompile.c"
    );

    for line in [
        "bench: ERROR llcfail llc: llc exited with status 1: llc: error: bad module\n",
        "bench: ERROR nocompile generate: the compiler exited with status 1",
        "; 22 errors\n",
    ] {
        assert!(summary.contains(line), "missing {line:?} in:\n{summary}");
    }
    assert_eq!(record(&run, "fine", Tool::Hod).status, Status::Ok, "the others are measured");
    assert_eq!(&Results::read(run.results_path.as_ref().unwrap()).unwrap(), results);

    // A run with an error is not a baseline.
    config.action = Action::UpdateBaseline;
    config.reason = Some("why".to_owned());
    let refused = world.run(&config);
    assert_eq!((refused.exit_status(), refused.updated.clone()), (3, None));
    assert!(
        refused.refused.as_ref().unwrap().starts_with("the run has an error ("),
        "{:?}",
        refused.refused
    );
    assert!(!world.baseline().exists());
}

// "Outcomes", "Child processes": a tool at the time limit is killed and is
// an error.
#[test]
fn a_tool_that_hangs_is_an_error() {
    let world = World::new("bench-hang");
    world.source("plain.c", "; llc: hang\n");
    let mut config = world.config(Action::Measure);
    (config.filters, config.runs) = (vec!["plain-g".to_owned()], 3);
    config.limits.time = Duration::from_secs(1);
    let run = world.run(&config);
    assert_eq!(run.exit_status(), 3);
    let llc = record(&run, "plain-g", Tool::Llc);
    assert_eq!(
        (llc.status, llc.detail.as_str(), llc.wall_us_min),
        (Status::Error, "llc timed out after 1 s", None)
    );
    assert_eq!(
        record(&run, "plain-g", Tool::HodFallback).detail,
        "hod+fallback timed out after 1 s"
    );
    assert_eq!(
        (record(&run, "plain-g", Tool::Hod).status, record(&run, "plain-g", Tool::Hod).runs),
        (Status::Declined, 3)
    );
}

// "Baselines", "The gate": a baseline is recorded with a reason; the same
// tools pass against it; a slower or a larger hod fails, after being
// measured again; a hod that no longer builds what it built fails.
#[test]
fn check_pass_and_fail() {
    let world = World::new("bench-check");
    // Without a baseline there is nothing to fail.
    let mut config = world.config(Action::Check);
    (config.filters, config.runs) = (vec!["built".to_owned()], 1);
    let none = world.run(&config);
    assert_eq!((none.exit_status(), none.check.is_none()), (0, true));
    assert!(none.summary(false).contains("bench: check: no baseline is recorded"));

    world.record_baseline();
    let baseline = Results::read(&world.baseline()).unwrap();
    assert_eq!(baseline.records.len(), 12);
    let log = std::fs::read_to_string(world.scratch.join("ws/bench/baselines/test-machine-4c.log"))
        .unwrap();
    let fields: Vec<&str> = log.trim_end().split('\t').collect();
    assert_eq!(fields[0], baseline.get("date").unwrap());
    assert_eq!(
        fields[1..],
        ["the first baseline", "4 inputs, hod built 2; hod version 9.9.9; 2 runs"]
    );

    // The same tools: every input passes, none is measured again.
    let config = world.config(Action::Check);
    let pass = world.run(&config);
    assert_eq!(pass.exit_status(), 0, "{}", pass.summary(false));
    let judged = verdicts(&pass);
    assert_eq!(
        judged.iter().map(|(input, ..)| input.as_str()).collect::<Vec<_>>(),
        ["plain", "plain-g", "built", "built-g"]
    );
    // What hod declines, then and now, is not judged: a decline is not a
    // build time.
    let declined = Verdict::Unjudged("declined, as in the baseline".to_owned());
    assert!(judged[..2].iter().all(|(_, verdict, again)| *verdict == declined && *again == 0));
    assert!(
        judged[2..]
            .iter()
            .all(|(_, verdict, again)| matches!(verdict, Verdict::Ok | Verdict::Improved)
                && *again == 0),
        "{judged:?}"
    );
    let check = pass.check.as_ref().unwrap();
    assert_eq!((check.baseline.clone(), check.advisory.clone()), (world.baseline(), None));
    assert!(pass.summary(false).contains("bench: check: 4 inputs: "));

    // hod takes 1.5 s longer: a failure that being measured again twice
    // does not cure. A slower decline is still a decline, and not judged.
    world.control("hod.delay", SLOW);
    let mut config = world.config(Action::Check);
    (config.filters, config.runs) = (vec!["built-g".to_owned(), "plain-g".to_owned()], 1);
    let slow = world.run(&config);
    assert_eq!(slow.exit_status(), 1, "{}", slow.summary(false));
    let regressed = Verdict::Regressed { time: true, mem: false };
    assert_eq!(
        verdicts(&slow),
        [("plain-g".to_owned(), declined, 0), ("built-g".to_owned(), regressed, 2)]
    );
    assert_eq!(record(&slow, "built-g", Tool::Hod).runs, 3, "the new runs join the old ones");
    assert_eq!(record(&slow, "built-g", Tool::Llc).runs, 3);
    let text = slow.summary(false);
    assert!(text.contains("REGRESSED (time); measured again 2x\n"), "{text}");
    assert!(text.contains("not judged: declined, as in the baseline\n"), "{text}");
    world.clear("hod.delay");

    // hod takes more memory: a failure, whatever the time.
    world.control("rss.hod", "5000");
    let mut config = world.config(Action::Check);
    (config.filters, config.runs, config.retries) = (vec!["built-g".to_owned()], 1, 1);
    let large = world.run(&config);
    assert_eq!(large.exit_status(), 1);
    assert_eq!(
        verdicts(&large),
        [("built-g".to_owned(), Verdict::Regressed { time: false, mem: true }, 1)]
    );
    world.clear("rss.hod");

    // hod declines what the baseline says it built: lost, at once.
    world.control("hod.decline-all", "");
    let lost = world.run(&config);
    assert_eq!(lost.exit_status(), 1);
    assert_eq!(verdicts(&lost), [("built-g".to_owned(), Verdict::Lost, 0)]);
    world.clear("hod.decline-all");

    // --judge hod+fallback judges the hybrid path instead.
    config.judge = Tool::HodFallback;
    let hybrid = world.run(&config);
    assert_eq!(hybrid.exit_status(), 0, "{}", hybrid.summary(false));
    assert_eq!(verdicts(&hybrid).len(), 1);
}

// "The gate", "Measuring again": a failure must survive being measured
// again. Here the first runs are slow and the later ones are not: the
// fastest of all of them is judged, and the input passes.
#[test]
fn the_remeasure_rule() {
    let world = World::new("bench-remeasure");
    world.record_baseline();
    world.control("hod.delay", SLOW);
    world.control("hod.slow-first", "2");
    let mut config = world.config(Action::Check);
    config.filters = vec!["built-g".to_owned()];
    let run = world.run(&config);
    assert_eq!(run.exit_status(), 0, "{}", run.summary(false));
    let judged = verdicts(&run);
    // One more round is enough on a quiet machine. Under load a fast run
    // can itself come out slow, and the rule then measures once more: the
    // number of rounds is the machine's, that the input passes is the rule's.
    let [(_, Verdict::Ok | Verdict::Improved, again @ 1..)] = judged[..] else {
        panic!("{judged:?}");
    };
    let hod = record(&run, "built-g", Tool::Hod);
    assert_eq!(hod.runs, 2 * (1 + again), "{hod:?}");
    assert!(
        hod.wall_us_min.unwrap() < 1_000_000 && hod.wall_us_max.unwrap() >= 1_500_000,
        "{hod:?}"
    );
    assert!(run.summary(false).contains(&format!("; measured again {again}x\n")));
    // The results file holds what was judged.
    assert_eq!(
        Results::read(run.results_path.as_ref().unwrap()).unwrap().find("built-g", Tool::Hod),
        Some(hod)
    );

    // With no retry allowed the first measurement stands.
    world.control("hod.slow-first", "2");
    config.retries = 0;
    let strict = world.run(&config);
    assert_eq!(strict.exit_status(), 1, "{}", strict.summary(false));
    assert_eq!(
        verdicts(&strict),
        [("built-g".to_owned(), Verdict::Regressed { time: true, mem: false }, 0)]
    );
}

// "The gate", "On another machine": the baseline of another machine does
// not bind. The comparison is made on the ratios to llc and reported; the
// exit status is 0 and nothing is measured again.
#[test]
fn advisory_on_another_machine() {
    let world = World::new("bench-advisory");
    world.record_baseline();
    world.control("hod.delay", SLOW);
    let mut config = world.config(Action::Check);
    (config.filters, config.runs) = (vec!["built-g".to_owned()], 1);
    config.machine.id = "another-machine-8c".to_owned();
    // No baseline for this machine: the one that is recorded is used.
    let run = world.run(&config);
    let text = run.summary(false);
    assert_eq!(run.exit_status(), 0, "{text}");
    let why = "another machine (test-machine-4c)".to_owned();
    assert_eq!(verdicts(&run), [("built-g".to_owned(), Verdict::Advisory(why.clone()), 0)]);
    let check = run.check.as_ref().unwrap();
    assert_eq!((check.baseline.clone(), check.advisory.clone()), (world.baseline(), Some(why)));
    assert!(check.judged[0].ratios && check.judged[0].time.is_some_and(|(base, now)| now > base));
    assert!(text.contains("; advisory: another machine (test-machine-4c)\n"), "{text}");
    assert!(text.contains("  base x llc  now x llc  "), "{text}");
    assert_eq!(record(&run, "built-g", Tool::Hod).runs, 1);

    // The same with the baseline named, and on its own machine it binds.
    config.baseline = Some(world.baseline());
    assert_eq!(world.run(&config).exit_status(), 0);
    config.machine.id = "test-machine-4c".to_owned();
    config.retries = 0;
    assert_eq!(world.run(&config).exit_status(), 1);
    config.baseline = Some(world.scratch.join("absent.tsv"));
    assert!(bench::run(&config).unwrap_err().starts_with("cannot read '"));
}

// "The gate", "Drift": when llc itself is far slower than in the baseline
// the machine is too busy to time anything; after measuring again the
// verdict is NOISY and the exit status 3, not a failure of hod.
#[test]
fn a_busy_machine_is_not_judged() {
    let world = World::new("bench-noisy");
    world.record_baseline();
    world.control("llc.delay", "0.3");
    let mut config = world.config(Action::Check);
    (config.filters, config.runs, config.retries) = (vec!["built-g".to_owned()], 1, 1);
    config.thresholds.max_drift_bp = 2500;
    let run = world.run(&config);
    let text = run.summary(false);
    assert_eq!(run.exit_status(), 3, "{text}");
    assert_eq!(verdicts(&run), [("built-g".to_owned(), Verdict::Noisy, 1)]);
    assert!(text.contains("NOISY; measured again 1x\n") && text.contains(" 1 NOISY, "), "{text}");
}

// "Baselines": an update replaces the baseline and appends to its log.
#[test]
fn baseline_update_with_reason() {
    let world = World::new("bench-update");
    world.record_baseline();
    let first = Results::read(&world.baseline()).unwrap();
    world.control("hod.decline-all", "");
    let mut config = world.config(Action::UpdateBaseline);
    config.reason = Some("hod declines everything again".to_owned());
    let run = world.run(&config);
    let text = run.summary(false);
    assert_eq!(run.exit_status(), 0, "{text}");
    assert!(
        text.ends_with(&format!("bench: baseline written: {}\n", world.baseline().display())),
        "{text}"
    );
    let second = Results::read(&world.baseline()).unwrap();
    assert_eq!(Some(&second), run.results.as_ref());
    assert_ne!(first, second);
    assert_eq!(second.find("built", Tool::Hod).unwrap().status, Status::Declined);
    let log = std::fs::read_to_string(world.baseline().with_extension("log")).unwrap();
    let lines: Vec<&str> = log.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(
        lines[0]
            .ends_with("\tthe first baseline\t4 inputs, hod built 2; hod version 9.9.9; 2 runs"),
        "{log}"
    );
    assert!(
        lines[1].ends_with(
            "\thod declines everything again\t4 inputs, hod built 0; hod version 9.9.9; 2 runs"
        ),
        "{log}"
    );
    // And the new baseline is what a check now compares with.
    let check = world.run(&world.config(Action::Check));
    assert_eq!(check.exit_status(), 0, "{}", check.summary(false));
}

// "Tools": without llc or its clang nothing is measured, which is a skip,
// or a failure under HOD_REQUIRE_TOOLS=1; without GNU time only the wall
// clock is read.
#[test]
fn missing_tools() {
    let world = World::new("bench-tools");
    let mut config = world.config(Action::Measure);
    (config.filters, config.runs) = (vec!["built-g".to_owned()], 1);
    config.time = world.scratch.join("no-time");
    let run = world.run(&config);
    assert_eq!(run.exit_status(), 0);
    let llc = record(&run, "built-g", Tool::Llc);
    assert_eq!((llc.status, llc.rss_kib, llc.cpu_us), (Status::Ok, None, None));
    assert!(llc.wall_us_min.is_some());
    assert_eq!(run.results.as_ref().unwrap().get("usage"), Some("none"));
    let text = run.summary(false);
    assert!(
        text.contains("bench: GNU time (")
            && text.contains("): no memory or CPU figures: not found; skipped\n"),
        "{text}"
    );
    config.require_tools = true;
    assert_eq!(world.run(&config).exit_status(), 3);

    config.time = world.bin("time");
    std::fs::remove_file(world.bin("clang++-22")).unwrap();
    std::fs::remove_file(world.bin("clang-22")).unwrap();
    let run = world.run(&config);
    assert_eq!((run.exit_status(), run.results.is_none()), (3, true));
    assert!(
        run.summary(false)
            .starts_with("bench: clang (no clang of LLVM 22 found to produce the inputs): MISSING")
    );

    std::fs::remove_file(world.bin("llc-22")).unwrap();
    let run = world.run(&config);
    assert_eq!(run.summary(false), "bench: llc (LLVM 22 or 19): MISSING (HOD_REQUIRE_TOOLS=1)\n");
    assert_eq!(run.exit_status(), 3);
    config.require_tools = false;
    config.llvm = Some("19".to_owned());
    let run = world.run(&config);
    assert_eq!(
        (run.exit_status(), run.summary(false).as_str()),
        (0, "bench: llc (LLVM 19): not found; skipped\n")
    );
    config.hod = Some(world.scratch.join("no-hod"));
    config.llvm = None;
    write_script(&world.bin("llc-22"), FAKE_LLC);
    write_script(&world.bin("clang++-22"), FAKE_COMPILER);
    assert!(bench::run(&config).unwrap_err().ends_with("no-hod' is not a file"));
}

// "Inputs" (`--generate`) and "The report" (`--report`).
#[test]
fn generate_and_report() {
    let world = World::new("bench-generate-report");
    world.source("built.cpp", "fail\n");
    let run = world.run(&world.config(Action::Generate));
    let text = run.summary(false);
    assert_eq!(run.exit_status(), 3, "{text}");
    assert!(run.results.is_none() && world.calls().is_empty(), "nothing is measured");
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines[0].starts_with("bench: inputs by fake clang version 22.0.0 in "), "{text}");
    assert!(lines[1].starts_with("input    class    IR bytes  functions  hash"), "{text}");
    assert!(
        lines[2].starts_with("plain    small  ") && lines[2].contains("  1  fnv1a64:"),
        "{text}"
    );
    assert!(lines[2].ends_with("  produced"), "{text}");
    assert!(lines[4].starts_with("built    typical         -          -  -"), "{text}");
    assert!(lines[4].contains("ERROR: the compiler exited with status 1"), "{text}");
    // --generate makes every input again.
    world.source("built.cpp", "; hod: build\n");
    let again = world.run(&world.config(Action::Generate));
    assert_eq!(again.exit_status(), 0);
    assert!(
        again.listed.iter().all(|listed| listed.generated.as_ref().is_ok_and(|made| made.fresh))
    );

    // The report is made from a results file, not from a measurement.
    let mut config = world.config(Action::Report);
    config.out = Some(world.scratch.join("docs/perf.md"));
    assert!(bench::run(&config).unwrap_err().starts_with("cannot read '"));
    let measured = world.run(&world.config(Action::Measure));
    let calls = world.calls().len();
    let run = world.run(&config);
    assert_eq!((run.exit_status(), world.calls().len()), (0, calls));
    let written = std::fs::read_to_string(world.scratch.join("docs/perf.md")).unwrap();
    assert_eq!(written, bench::report::perf_md(measured.results.as_ref().unwrap(), &[]));
    assert!(
        written.contains("| plain-g | small | ")
            && written.contains("declined: codegen-not-implemented"),
        "{written}"
    );
    assert!(run.summary(false).ends_with(&format!(
        "bench: report written: {}\n",
        world.scratch.join("docs/perf.md").display()
    )));
    // Its default place is docs/status/perf.md of the checkout.
    config.out = None;
    world.run(&config);
    assert!(world.scratch.join("ws/docs/status/perf.md").is_file());
}

// "Cross runs" and "Instructions": --target another architecture compiles
// the inputs for it into a directory of their own, gives llc and hod its
// triple, and writes its own results file and baseline; --instructions
// counts one run of llc and strict hod per input under callgrind.
#[test]
fn a_cross_run_with_instructions() {
    let world = World::new("bench-cross");
    let arch = Arch::ALL.into_iter().find(|&arch| Some(arch) != Arch::host()).unwrap();
    let triple = arch.triple();
    let mut config = world.config(Action::Measure);
    (config.target, config.instructions, config.runs) = (Some(arch), true, 1);

    // Without valgrind there are no counts, and the summary says why.
    let run = world.run(&config);
    assert_eq!(run.exit_status(), 0, "{}", run.summary(false));
    assert!(run.missing_tools.iter().any(|tool| tool.starts_with("valgrind")));
    assert_eq!(record(&run, "built", Tool::Llc).instructions, None);

    write_script(&world.bin("valgrind"), FAKE_VALGRIND);
    world.clear("calls");
    let run = world.run(&config);
    let summary = run.summary(false);
    assert_eq!((run.exit_status(), run.notes.clone()), (0, Vec::<String>::new()), "{summary}");
    let results = run.results.as_ref().unwrap();
    assert_eq!(results.get("target"), Some(triple));
    let mut llc_options = format!("-mtriple={triple}");
    if arch == Arch::X86_64 {
        llc_options.push_str(" -fast-isel=0");
    }
    assert_eq!(results.get("llc_options"), Some(llc_options.as_str()));
    let ir = world.scratch.join(format!("build/bench/inputs/clang22-{}/plain.ll", arch.name()));
    let ir = std::fs::read_to_string(ir).unwrap();
    assert!(ir.starts_with(&format!("define void @f() ; --target={triple} -O0 ")), "{ir}");
    let codegen = "-O0 -filetype=obj -relocation-model=pic";
    let calls = world.calls();
    assert!(calls.contains(&format!("llc {codegen} {llc_options} plain.ll")), "{calls:?}");
    assert!(
        calls.contains(&format!("hod strict {codegen} -mtriple={triple} plain.ll")),
        "{calls:?}"
    );

    // llc and strict hod are counted, a decline too; hod+fallback is not.
    let counts = |input| {
        [Tool::Llc, Tool::Hod, Tool::HodFallback].map(|tool| record(&run, input, tool).instructions)
    };
    assert_eq!(counts("built"), [Some(4000), Some(1000), None]);
    assert_eq!(counts("plain-g"), [Some(4000), Some(1000), None]);
    assert!(summary.contains("bench: instructions under callgrind, over the 2 inputs hod built: hod 0.0 M, llc 0.0 M: 0.25x\n"), "{summary}");
    let path = world.scratch.join(format!("build/bench/results-{}-llvm22.tsv", arch.name()));
    assert_eq!(run.results_path.as_deref(), Some(path.as_path()));
    assert_eq!(&Results::read(&path).unwrap(), results);

    // Its baseline is its own; the gate binds it only to a run for the
    // same target.
    let mut update = world.config(Action::UpdateBaseline);
    (update.target, update.reason) = (Some(arch), Some("cross".to_owned()));
    let updated = world.run(&update);
    let own = world.scratch.join(format!("ws/bench/baselines/test-machine-4c-{}.tsv", arch.name()));
    assert_eq!(updated.updated.as_deref(), Some(own.as_path()), "{}", updated.summary(false));
    assert!(!world.baseline().exists());
    let check = world.run(&world.config(Action::Check));
    assert!(check.check.is_none(), "a native run has no baseline of its own target");
    let mut check = world.config(Action::Check);
    (check.target, check.instructions) = (Some(arch), true);
    let checked = world.run(&check);
    assert_eq!(checked.check.as_ref().map(|check| check.baseline.clone()), Some(own));

    // The report: the native results, then the cross run's section.
    let mut report = world.config(Action::Report);
    report.out = Some(world.scratch.join("docs/perf.md"));
    world.run(&report);
    let written = std::fs::read_to_string(world.scratch.join("docs/perf.md")).unwrap();
    let section = format!("\n## {} objects\n", arch.name());
    let (native, cross) = written.split_once(&section).expect(&written);
    assert!(!native.contains("### "), "{native}");
    assert!(cross.contains("### Instructions\n") && cross.contains("- target: `"), "{cross}");
}
