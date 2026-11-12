//! `xtask` as a jobserver client (`spec/harness.md` "Jobserver"): the
//! command run as a process, with a `MAKEFLAGS` that names a pool of the
//! test's own, never the machine's.
//!
//! `irdiff` runs here over two fake tools, an `llc` and a `hod` that are
//! shell scripts (there is no linker driver, so nothing is linked or run).
//! Each records how many of the fakes are alive: it makes `live.<pid>` in a
//! directory, counts the `live.*` there, and appends the count to `peaks`.
//! A module with a `; hang` line makes `llc` stay alive.
#![cfg(target_os = "linux")]

use std::os::unix::process::ExitStatusExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use hod_testutil::jobs::{LocalDir, PrivatePool};
use hod_testutil::{Outcome, Scratch, run_with};

/// Marks this process as alive and records how many are.
const MARK: &str = r#"mkdir "$marks/live.$$" || exit 1
ls "$marks" | grep -c '^live\.' >> "$marks/peaks""#;

const FAKE_LLC: &str = r#"if [ "$1" = --version ]; then echo "fake LLVM version 22.0.0"; exit 0; fi
out= in=
while [ $# -gt 0 ]; do
  case "$1" in
    -o) out=$2; shift ;;
    -*) ;;
    *) in=$1 ;;
  esac
  shift
done
if grep -q '^; hang' "$in"; then exec sleep 60; fi
sleep 0.1
cp "$in" "$out"
rmdir "$marks/live.$$""#;

const FAKE_HOD: &str = r#"log= out= in=
while [ $# -gt 0 ]; do
  case "$1" in
    --coverage-log=*) log=${1#--coverage-log=} ;;
    -o) out=$2; shift ;;
    -*) ;;
    *) in=$1 ;;
  esac
  shift
done
sleep 0.1
cp "$in" "$out"
printf 'hod\t-\ttext\t1\t%s\t-\t5\t-\n' "$out" >> "$log"
rmdir "$marks/live.$$""#;

/// A machine with a fake `llc-22` and a fake hod, and `tests` test files.
struct World {
    scratch: Scratch,
    marks: LocalDir,
}

impl World {
    fn new(label: &str, tests: usize, text: &str) -> Self {
        let world = Self { scratch: Scratch::new(label), marks: LocalDir::new(label) };
        for dir in ["bin", "tests", "build"] {
            std::fs::create_dir(world.scratch.join(dir)).unwrap();
        }
        let fake = |body: &str| {
            format!("PATH=/usr/bin:/bin\nmarks='{}'\n{MARK}\n{body}", world.marks.path().display())
        };
        // The probe of the version is not marked: it is not a test's child.
        let llc = fake(FAKE_LLC).replacen(MARK, "", 1).replacen(
            "out= in=\n",
            &format!("{MARK}\nout= in=\n"),
            1,
        );
        hod_testutil::write_script(&world.scratch.join("bin/llc-22"), &llc);
        hod_testutil::write_script(&world.hod(), &fake(FAKE_HOD));
        for i in 0..tests {
            std::fs::write(world.scratch.join(format!("tests/t{i:02}.ll")), text).unwrap();
        }
        world
    }

    fn hod(&self) -> PathBuf {
        self.scratch.join("hod")
    }

    /// `xtask irdiff` on this world: `PATH` is the fake tools only, the
    /// build directory is the world's, and there is no jobserver unless
    /// the caller names one.
    fn irdiff(&self, jobs: usize) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_xtask"));
        command.arg("irdiff").args(["--hod".as_ref(), self.hod().as_os_str()]);
        command.args(["-j", &jobs.to_string()]).arg(self.scratch.join("tests"));
        command.env("PATH", self.scratch.join("bin")).env("CARGO_TARGET_DIR", self.build());
        for name in ["HOD_REQUIRE_TOOLS", "MAKEFLAGS"] {
            command.env_remove(name);
        }
        command
    }

    fn build(&self) -> PathBuf {
        self.scratch.join("build")
    }

    /// The most fakes that were alive at once, and how many ran.
    fn peaks(&self) -> (usize, usize) {
        let text = std::fs::read_to_string(self.marks.join("peaks")).unwrap_or_default();
        let counts: Vec<usize> = text.lines().map(|line| line.trim().parse().unwrap()).collect();
        (counts.iter().copied().max().unwrap_or(0), counts.len())
    }

    /// The pids of the fakes that made a marker and did not remove it.
    fn live(&self) -> Vec<u32> {
        std::fs::read_dir(self.marks.path())
            .unwrap()
            .filter_map(|entry| {
                entry.ok()?.file_name().to_str()?.strip_prefix("live.")?.parse().ok()
            })
            .collect()
    }
}

fn text(outcome: &Outcome) -> String {
    format!("{}\n{}", outcome.stdout_text(), outcome.stderr_text())
}

// "Jobserver", "`-j`" and "The implicit token": `irdiff -j 8` on a pool of
// 2 tokens has at most 3 children alive, the header names the pool, and
// the pool is left as it was found.
#[test]
fn irdiff_is_bounded_by_the_pool() {
    const POOL: usize = 2;
    let world = World::new("jobs-irdiff", 12, "; a module\n");
    let pool = PrivatePool::new("irdiff", POOL);
    let mut command = world.irdiff(8);
    command.env("MAKEFLAGS", pool.makeflags());
    let outcome = run_with(&mut command, b"", Duration::from_mins(5));
    assert_eq!(outcome.status.and_then(|s| s.code()), Some(0), "{}", text(&outcome));
    let out = outcome.stdout_text();
    let header = format!("; strict; -j 8 (jobserver fifo:{})\n", pool.path().display());
    assert!(out.contains(&header), "{out}");
    assert!(out.contains("irdiff: 12 tests, 12 results: "), "{out}");
    let (peak, ran) = world.peaks();
    assert_eq!(ran, 24, "an llc and a hod per test: {out}");
    assert!(peak <= POOL + 1, "{peak} children at once on {POOL} tokens and the implicit one");
    assert!(peak >= 2, "peak {peak}: the pool was never used");
    assert_eq!(pool.idle(), POOL, "every token came back");
    assert_eq!(world.live(), [0u32; 0]);
}

// "Jobserver", "Fallback": without `MAKEFLAGS`, with the descriptor form,
// and with a path that is not a fifo (one warning), `-j` alone bounds the
// children and the header is the one of a run without a jobserver.
#[test]
fn irdiff_without_a_usable_pool() {
    let plain = Scratch::new("jobs-irdiff-plain");
    let plain = plain.write("not-a-fifo", "++");
    for (label, makeflags, warns) in [
        ("none", None, false),
        ("fds", Some("-j8 --jobserver-auth=3,4".to_owned()), false),
        ("plain", Some(format!("--jobserver-auth=fifo:{}", plain.display())), true),
    ] {
        let world = World::new(&format!("jobs-irdiff-{label}"), 8, "; a module\n");
        let mut command = world.irdiff(4);
        if let Some(makeflags) = makeflags {
            command.env("MAKEFLAGS", makeflags);
        }
        let outcome = run_with(&mut command, b"", Duration::from_mins(5));
        assert_eq!(outcome.status.and_then(|s| s.code()), Some(0), "{label}: {}", text(&outcome));
        assert!(outcome.stdout_text().contains("; strict; -j 4\n"), "{label}: {}", text(&outcome));
        let (peak, ran) = world.peaks();
        assert_eq!(ran, 16, "{label}");
        assert!((1..=4).contains(&peak), "{label}: {peak}");
        let warnings = outcome.stderr_text().matches("cannot use jobserver fifo").count();
        assert_eq!(warnings, usize::from(warns), "{label}: {}", outcome.stderr_text());
    }
}

// "Jobserver", "Signals": an interrupted xtask kills its children, returns
// its tokens, and ends by the signal; no child outlives it, no result is
// written, and the pool has all its tokens.
#[test]
fn an_interrupted_irdiff_kills_its_children_and_returns_its_tokens() {
    const POOL: usize = 2;
    for (name, number) in [("TERM", 15), ("INT", 2)] {
        let world = World::new(&format!("jobs-signal-{name}"), 8, "; hang\n");
        let pool = PrivatePool::new("signal", POOL);
        // A shell that writes its pid and becomes xtask, so that the test
        // can signal a process `run_with` started.
        let pid_file = world.scratch.join("pid");
        let mut wrapped = Command::new("/bin/sh");
        wrapped.args(["-c", "echo $$ > \"$0\" && exec \"$@\""]).arg(&pid_file);
        let command = world.irdiff(8);
        wrapped.arg(command.get_program()).args(command.get_args());
        for (name, value) in command.get_envs() {
            match value {
                Some(value) => wrapped.env(name, value),
                None => wrapped.env_remove(name),
            };
        }
        wrapped.env("MAKEFLAGS", pool.makeflags());
        let (outcome, children) = std::thread::scope(|scope| {
            let xtask = scope.spawn(move || run_with(&mut wrapped, b"", Duration::from_mins(5)));
            let waited = Instant::now();
            while world.live().len() < POOL + 1 {
                assert!(!xtask.is_finished(), "{name}: xtask ended early");
                assert!(waited.elapsed() < Duration::from_mins(2), "{name}: nothing started");
                std::thread::sleep(Duration::from_millis(20));
            }
            // The other five workers wait for a token; nothing more starts.
            std::thread::sleep(Duration::from_millis(300));
            let children = world.live();
            assert_eq!(children.len(), POOL + 1, "{name}");
            assert_eq!(pool.idle(), 0, "{name}: the children hold the pool");

            let pid = std::fs::read_to_string(&pid_file).unwrap();
            // The shell's own `kill`: a small machine has no other.
            let kill = ["-c", "kill \"-$1\" \"$2\"", "sh", name, pid.trim()];
            let signalled = hod_testutil::run("/bin/sh", kill);
            assert!(signalled.success(), "{name}: {}", signalled.stderr_text());
            (xtask.join().unwrap(), children)
        });
        let errors = outcome.stderr_text();
        let status = outcome.status.expect("xtask ended long before the time limit");
        assert_eq!(status.signal(), Some(number), "{name}: {status:?}\n{errors}");
        let killed = format!("xtask: signal {number}: killed {} child process(es)\n", POOL + 1);
        assert!(errors.contains(&killed), "{name}: {errors}");
        for pid in children {
            assert!(!Path::new(&format!("/proc/{pid}")).exists(), "{name}: child {pid} lives on");
        }
        assert_eq!(pool.idle(), POOL, "{name}: the pool has all its tokens\n{errors}");
        assert_eq!(outcome.stdout_text(), "", "{name}: no summary");
        // The killed children were nobody's result.
        let results: Vec<_> = std::fs::read_dir(world.build().join("irdiff"))
            .map(|entries| entries.flatten().map(|entry| entry.file_name()).collect())
            .unwrap_or_default();
        assert!(
            results.iter().all(|name| !name.to_string_lossy().starts_with("results")),
            "{name}: {results:?}"
        );
    }
}

fn jobserver(args: &[&str], env: &[(&str, &str)]) -> Outcome {
    let mut command = Command::new(env!("CARGO_BIN_EXE_xtask"));
    command.arg("jobserver").args(args);
    for name in ["MAKEFLAGS", "HOD_JOBSERVER_FIFO", "HOD_JOBS"] {
        command.env_remove(name);
    }
    for (name, value) in env {
        command.env(name, value);
    }
    run_with(&mut command, b"", Duration::from_mins(1))
}

// "Jobserver", "`cargo xtask jobserver`": the command, its defaults from
// the environment, and that it never blocks.
#[test]
fn the_jobserver_command() {
    let pool = PrivatePool::new("command", 2);
    let path = pool.path().display().to_string();
    let makeflags = pool.makeflags();

    let status = jobserver(&["status"], &[("MAKEFLAGS", &makeflags)]);
    assert_eq!(status.stdout_text(), format!("jobserver {path}: 2 idle token(s) of 12\n"));
    let status = jobserver(&["status"], &[("HOD_JOBSERVER_FIFO", &path), ("HOD_JOBS", "5")]);
    assert_eq!(status.stdout_text(), format!("jobserver {path}: 2 idle token(s) of 5\n"));
    // The size the pool script recorded beside the fifo.
    std::fs::write(pool.path().with_file_name("jobserver.jobs"), "3\n").unwrap();
    let refilled = jobserver(&["refill", "--fifo", &path], &[]);
    assert_eq!(refilled.stdout_text(), format!("jobserver {path}: 2 idle, added 1: now 3\n"));
    assert_eq!(pool.idle(), 3);
    let full = jobserver(&["refill", "--fifo", &path, "--jobs=3"], &[]);
    assert_eq!(full.stdout_text(), format!("jobserver {path}: 3 idle; full\n"));

    let help = jobserver(&["--help"], &[]);
    assert!(help.success() && help.stdout_text().starts_with("usage: cargo xtask jobserver"));
    for bad in [&[][..], &["drain"], &["status", "--jobs", "x"], &["status", "--fifo"]] {
        let refused = jobserver(bad, &[]);
        assert_eq!(refused.status.and_then(|s| s.code()), Some(2), "{bad:?}");
        assert!(refused.stderr_text().starts_with("xtask jobserver: "), "{bad:?}");
        assert_eq!(refused.stdout_text(), "", "{bad:?}");
    }
    let missing = pool.path().with_file_name("missing.fifo").display().to_string();
    let refused = jobserver(&["status", "--fifo", &missing], &[]);
    assert_eq!(refused.status.and_then(|s| s.code()), Some(2));

    // A fifo nobody holds open, without a token, as a restart of the
    // machine would leave one on a disk: counted and refilled at once.
    // (Whoever then reads it gets what `refill` wrote only while somebody
    // keeps the fifo open; the pool script's holder does.)
    let (stale, _dir) = PrivatePool::new("command-stale", 3).abandon();
    let stale = stale.display().to_string();
    for verb in ["status", "refill"] {
        let outcome = jobserver(&[verb, "--fifo", &stale, "--jobs", "2"], &[]);
        assert!(outcome.success() && !outcome.timed_out(), "{verb}: {}", text(&outcome));
        assert!(outcome.stdout_text().contains(": 0 idle"), "{verb}: {}", outcome.stdout_text());
    }
}

// "Jobserver", "The pool": `scripts/env.sh` makes cargo, rustc, the test
// binaries and xtask clients of the pool exactly while its fifo exists.
#[test]
fn env_sh_names_the_pool_while_its_fifo_exists() {
    if hod_testutil::tool("bash").is_none() {
        return;
    }
    let env_sh = Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/env.sh");
    let pool = PrivatePool::new("env", 1);
    let sourced = |fifo: &Path, off: bool| {
        let mut command = Command::new("bash");
        command.args(["-c", "source \"$0\"; echo \"${MAKEFLAGS-none} ${CARGO_BUILD_JOBS-none}\""]);
        command.arg(&env_sh).env("HOD_JOBSERVER_FIFO", fifo);
        for name in ["MAKEFLAGS", "CARGO_BUILD_JOBS", "HOD_NO_JOBSERVER"] {
            command.env_remove(name);
        }
        if off {
            command.env("HOD_NO_JOBSERVER", "1");
        }
        let outcome = run_with(&mut command, b"", Duration::from_mins(1));
        assert!(outcome.success(), "{}", text(&outcome));
        outcome.stdout_text()
    };
    assert_eq!(sourced(pool.path(), false), format!("{} none\n", pool.makeflags()));
    assert_eq!(sourced(pool.path(), true), "none 8\n");
    assert_eq!(sourced(&pool.path().with_file_name("missing.fifo"), false), "none 8\n");
}
