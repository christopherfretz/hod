//! A test binary as a jobserver client (`spec/harness.md` "Jobserver").
//!
//! The client a test binary uses is the one it inherits through
//! `MAKEFLAGS`, read once. So each test here runs this test binary again
//! as a child process, with a `MAKEFLAGS` that names a pool of the test's
//! own (never the machine's), and lets that child run one of the
//! `inner_*` tests below, which are ignored in a normal run.
//!
//! The children the inner tests start are a script that records how many
//! of its kind are alive: it makes `live.<pid>` in a directory, counts the
//! `live.*` there, and appends the count to `peaks`.
#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use hod_testutil::jobs::{self, LocalDir, PrivatePool};
use hod_testutil::{DEFAULT_TIMEOUT, Outcome, Scratch, run_with};

/// What the inner tests read from the environment.
const SCRIPT: &str = "HOD_JOBS_TEST_SCRIPT";
const THREADS: &str = "HOD_JOBS_TEST_THREADS";
const ROUNDS: &str = "HOD_JOBS_TEST_ROUNDS";
/// The time limit of each child, in seconds (default: `DEFAULT_TIMEOUT`).
const LIMIT: &str = "HOD_JOBS_TEST_LIMIT";

/// The limit of children that live until the test lets them go. Not a
/// verdict: a hang guard, far above any wait under load. (At the default
/// minute the held children of `the_pool_script_does_not_mint_tokens_...`
/// were killed while the test waited for a token of the machine's pool at
/// load 20; their client ended in the middle of a refill pass, and the
/// pass, rightly finding no client, refilled what it had last seen held.)
const HELD_LIMIT: Duration = Duration::from_mins(10);

/// Writes the child script. It stays alive for a tenth of a second, and
/// while `<marks>/hold` exists until `<marks>/release` does.
fn write_child(scratch: &Scratch, marks: &Path) -> PathBuf {
    let body = format!(
        "PATH=/usr/bin:/bin\n\
         marks='{}'\n\
         mkdir \"$marks/live.$$\" || exit 1\n\
         ls \"$marks\" | grep -c '^live\\.' >> \"$marks/peaks\"\n\
         if [ -e \"$marks/hold\" ]; then\n\
         \x20 while [ ! -e \"$marks/release\" ]; do sleep 0.05; done\n\
         fi\n\
         sleep 0.1\n\
         rmdir \"$marks/live.$$\"",
        marks.display()
    );
    scratch.write_script("child", &body)
}

/// This test binary, about to run the ignored test `inner` on `threads`
/// threads, each starting the script `rounds` times.
fn inner(inner: &str, script: &Path, threads: usize, rounds: usize) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args(["--exact", inner, "--ignored", "--nocapture", "--test-threads=1"]);
    command.env(SCRIPT, script).env(THREADS, threads.to_string()).env(ROUNDS, rounds.to_string());
    command.env_remove("MAKEFLAGS");
    command
}

/// The most children that were alive at once, and how many ran.
fn peaks(marks: &Path) -> (usize, usize) {
    let text = std::fs::read_to_string(marks.join("peaks")).unwrap_or_default();
    let counts: Vec<usize> = text.lines().map(|line| line.trim().parse().unwrap()).collect();
    (counts.iter().copied().max().unwrap_or(0), counts.len())
}

/// The `live.*` markers: the children alive now.
fn live(marks: &Path) -> usize {
    std::fs::read_dir(marks)
        .unwrap()
        .filter(|entry| {
            entry.as_ref().is_ok_and(|e| e.file_name().to_string_lossy().starts_with("live."))
        })
        .count()
}

fn ran_one_test(outcome: &Outcome) {
    let (out, err) = (outcome.stdout_text(), outcome.stderr_text());
    assert!(outcome.success(), "the inner test failed:\n{out}\n{err}");
    assert!(out.contains("test result: ok. 1 passed"), "the inner test did not run:\n{out}\n{err}");
}

// "Jobserver", "Scope" and "The implicit token": a test binary whose
// threads each start children through `hod_testutil::run` has, on a pool
// of P tokens, never more than P + 1 children alive; and it leaves the
// pool as it found it.
#[test]
fn a_test_binary_is_bounded_by_the_pool() {
    const POOL: usize = 2;
    let scratch = Scratch::new("jobs-bounded");
    let marks = LocalDir::new("bounded-marks");
    let pool = PrivatePool::new("bounded", POOL);
    let script = write_child(&scratch, marks.path());
    let mut command = inner("inner_children", &script, 8, 3);
    command.env("MAKEFLAGS", pool.makeflags());
    let outcome = run_with(&mut command, b"", Duration::from_mins(5));
    ran_one_test(&outcome);
    let (peak, ran) = peaks(marks.path());
    assert_eq!(ran, 24);
    assert!(peak <= POOL + 1, "{peak} children at once on {POOL} tokens and the implicit one");
    assert!(peak >= 2, "peak {peak}: the pool was never used");
    assert_eq!(pool.idle(), POOL, "every token came back");
    assert_eq!(live(marks.path()), 0);
}

// "Jobserver", "Fallback": without `MAKEFLAGS`, with the descriptor form,
// and with a path that is not a fifo (one warning, however many children),
// the threads alone bound the children, as before.
#[test]
fn without_a_usable_pool_nothing_waits() {
    let scratch = Scratch::new("jobs-fallback");
    let script_in = |marks: &LocalDir| write_child(&scratch, marks.path());
    let plain = scratch.write("not-a-fifo", "++");
    let missing = scratch.join("missing.fifo");
    for (label, makeflags, warns) in [
        ("none", None, false),
        ("fds", Some("-j8 --jobserver-auth=3,4".to_owned()), false),
        ("old-fds", Some("--jobserver-fds=3,4 -j".to_owned()), false),
        ("plain", Some(format!("--jobserver-auth=fifo:{}", plain.display())), true),
        ("missing", Some(format!("--jobserver-auth=fifo:{}", missing.display())), true),
    ] {
        let marks = LocalDir::new(&format!("fallback-{label}"));
        let mut command = inner("inner_children", &script_in(&marks), 4, 2);
        if let Some(makeflags) = makeflags {
            command.env("MAKEFLAGS", makeflags);
        }
        let outcome = run_with(&mut command, b"", Duration::from_mins(5));
        ran_one_test(&outcome);
        let (peak, ran) = peaks(marks.path());
        assert_eq!(ran, 8, "{label}");
        assert!((1..=4).contains(&peak), "{label}: {peak}");
        let warnings = outcome.stderr_text().matches("cannot use jobserver fifo").count();
        assert_eq!(warnings, usize::from(warns), "{label}: {}", outcome.stderr_text());
    }
}

// "Jobserver", "Fallback": a fifo without a token and without a holder
// (what is left when the pool's holder is gone) does not hang a client:
// its children run one at a time, on the implicit token.
#[test]
fn a_pool_without_tokens_serializes_and_does_not_hang() {
    let scratch = Scratch::new("jobs-stale");
    let marks = LocalDir::new("stale-marks");
    let (fifo, _dir) = PrivatePool::new("stale", 3).abandon();
    let script = write_child(&scratch, marks.path());
    let mut command = inner("inner_children", &script, 4, 2);
    command.env("MAKEFLAGS", format!("--jobserver-auth=fifo:{}", fifo.display()));
    let outcome = run_with(&mut command, b"", Duration::from_mins(5));
    ran_one_test(&outcome);
    assert_eq!(peaks(marks.path()), (1, 8));
    assert_eq!(jobs::idle(&fifo), 0);
}

/// Makes the file that lets the held children end, on drop.
struct Release(PathBuf);

impl Drop for Release {
    fn drop(&mut self) {
        let _ = std::fs::write(&self.0, "");
    }
}

/// `scripts/jobserver.sh` with a pool of its own. The script's idea of
/// the machine's shared pool (`HOD_JOBSERVER_SHARED_FIFO`) is moved into
/// the test's directory as well, so no command of these tests can reach
/// the machine's pool, even one that loses its `HOD_JOBSERVER_FIFO`.
struct PoolScript {
    fifo: PathBuf,
    shared: PathBuf,
    build_root: PathBuf,
    jobs: usize,
}

impl PoolScript {
    /// A pool at `local/jobserver.fifo`; the "shared" one is elsewhere.
    fn new(local: &LocalDir, build_root: &Path, jobs: usize) -> Self {
        Self {
            fifo: local.join("jobserver.fifo"),
            shared: local.join("not-this-pool.fifo"),
            build_root: build_root.to_owned(),
            jobs,
        }
    }

    fn command(&self, verb: &str) -> Command {
        let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/jobserver.sh");
        let mut command = Command::new("bash");
        command.arg(script).args(verb.split(' '));
        command.env("HOD_JOBSERVER_FIFO", &self.fifo).env("HOD_BUILD_ROOT", &self.build_root);
        command.env("HOD_JOBSERVER_SHARED_FIFO", &self.shared).env_remove("HOD_JOBSERVER_SHARED");
        command.env("HOD_JOBS", self.jobs.to_string());
        command
    }

    fn run(&self, verb: &str) -> String {
        let outcome = run_with(&mut self.command(verb), b"", DEFAULT_TIMEOUT);
        let (out, err) = (outcome.stdout_text(), outcome.stderr_text());
        assert!(outcome.success(), "jobserver.sh {verb}:\n{out}\n{err}");
        out
    }
}

impl Drop for PoolScript {
    /// The holder and the refill loop must not outlive the test, whatever
    /// became of it. (`--shared`: the guard test makes its pool the
    /// "shared" one.)
    fn drop(&mut self) {
        let _slot = jobs::global().acquire();
        let mut stop = self.command("stop --shared");
        let _ = stop.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
}

/// The tools `scripts/jobserver.sh` needs, present.
fn pool_script_tools() -> bool {
    ["bash", "python3", "ps", "pgrep", "mkfifo", "nohup", "realpath"]
        .iter()
        .all(|tool| hod_testutil::tool(tool).is_some())
}

// "Jobserver", "What the pool's refill sees": `scripts/jobserver.sh refill`
// counts as held one token per child process of each process that has the
// fifo open. This client holds a pool token only for a live child and
// keeps the fifo open, so two refill passes in a row (a deficit is
// refilled when two passes saw it) while it holds tokens for sleeping
// children add nothing, and the pool is exactly full when they are done.
#[test]
fn the_pool_script_does_not_mint_tokens_for_this_client() {
    const POOL: usize = 4;
    const CHILDREN: usize = 3;
    if !pool_script_tools() {
        return;
    }
    let scratch = Scratch::new("jobs-script");
    let local = LocalDir::new("script");
    let script = PoolScript::new(&local, scratch.path(), POOL);
    let started = script.run("start");
    assert!(started.contains(&format!("with {POOL} tokens")), "{started}");
    assert_eq!(jobs::idle(&script.fifo), POOL);
    let status = script.run("status");
    assert!(status.contains(&format!("idle tokens {POOL} of {POOL}")), "{status}");

    // A client with one sleeping child per thread: one on the implicit
    // token, the others on the pool's.
    let marks = LocalDir::new("script-marks");
    std::fs::write(marks.join("hold"), "").unwrap();
    let child = write_child(&scratch, marks.path());
    let mut client = inner("inner_children", &child, CHILDREN, 1);
    client.env("MAKEFLAGS", format!("--jobserver-auth=fifo:{}", script.fifo.display()));
    client.env(LIMIT, HELD_LIMIT.as_secs().to_string());
    std::thread::scope(|scope| {
        let client = scope.spawn(move || run_with(&mut client, b"", HELD_LIMIT * 2));
        // Whatever fails below, the children are let go.
        let _release = Release(marks.join("release"));
        let waited = Instant::now();
        while live(marks.path()) < CHILDREN {
            assert!(!client.is_finished(), "the client ended early");
            assert!(waited.elapsed() < Duration::from_mins(2), "its children never started");
            std::thread::sleep(Duration::from_millis(20));
        }
        let held = CHILDREN - 1;
        assert_eq!(jobs::idle(&script.fifo), POOL - held);

        for pass in 1..=2 {
            let log = script.run("refill");
            // A pass is judged only if the client lived through it: one
            // that ended in the middle has given its tokens back.
            assert!(!client.is_finished(), "pass {pass}: the client ended early: {log}");
            assert!(!log.contains("refilled"), "pass {pass} minted tokens: {log}");
            assert!(!log.contains("drained"), "pass {pass}: {log}");
            let idle = jobs::idle(&script.fifo);
            assert!(idle + held <= POOL, "pass {pass}: {idle} idle, {held} held of {POOL}: {log}");
            assert_eq!(idle, POOL - held, "pass {pass}: {log}");
        }
        let status = script.run("status");
        assert!(status.contains(&format!("idle tokens {} of {POOL}", POOL - held)), "{status}");

        std::fs::write(marks.join("release"), "").unwrap();
        ran_one_test(&client.join().unwrap());
    });
    assert_eq!(peaks(marks.path()), (CHILDREN, CHILDREN));
    assert_eq!(jobs::idle(&script.fifo), POOL, "the client returned what it held");
    // With no client left the script resets the pool to its size: nothing
    // to add, nothing to drain.
    let log = script.run("refill");
    assert!(log.contains("no refill"), "{log}");
    assert_eq!(jobs::idle(&script.fifo), POOL);
}

// "Jobserver", "The shared pool's guard": `refill`, `resize` and `stop`
// refuse the shared pool (here a pool of the test's own that the script is
// told is the shared one) without `--shared` or `HOD_JOBSERVER_SHARED=1`,
// also when `HOD_JOBSERVER_FIFO` is missing; `start` and `status` do not.
#[test]
fn the_pool_script_refuses_the_shared_pool_unless_told() {
    if !pool_script_tools() {
        return;
    }
    let scratch = Scratch::new("jobs-guard");
    let local = LocalDir::new("guard");
    let mut script = PoolScript::new(&local, scratch.path(), 3);
    script.shared.clone_from(&script.fifo);
    assert!(script.run("start").contains("with 3 tokens"));
    let refused = |verb: &str, lose_fifo: bool| {
        let mut command = script.command(verb);
        if lose_fifo {
            command.env_remove("HOD_JOBSERVER_FIFO");
        }
        let outcome = run_with(&mut command, b"", DEFAULT_TIMEOUT);
        let err = outcome.stderr_text();
        assert_eq!(outcome.status.and_then(|s| s.code()), Some(3), "{verb}: {err}");
        assert!(
            err.contains(&format!(
                "refusing '{}' on the shared pool",
                verb.split(' ').next().unwrap()
            )),
            "{err}"
        );
        assert!(err.contains("--shared") && err.contains("HOD_JOBSERVER_FIFO=<path>"), "{err}");
    };
    for lose_fifo in [false, true] {
        for verb in ["stop", "refill", "resize 5"] {
            refused(verb, lose_fifo);
        }
    }
    // Nothing happened to the pool.
    assert!(script.run("status").contains("idle tokens 3 of 3"));
    assert_eq!(jobs::idle(&script.fifo), 3);
    // Confirmed, each acts.
    assert!(script.run("refill --shared").contains("no refill"));
    assert!(script.run("resize 4 --shared").contains("pool size now 4"));
    let mut confirmed = script.command("stop");
    confirmed.env("HOD_JOBSERVER_SHARED", "1");
    let stopped = run_with(&mut confirmed, b"", DEFAULT_TIMEOUT);
    assert!(
        stopped.success() && stopped.stdout_text().contains("stopped"),
        "{}",
        stopped.stderr_text()
    );
    assert!(!script.fifo.exists());
}

/// Run by the tests above in a child process: `THREADS` threads, each
/// running the script `ROUNDS` times through `hod_testutil::run`.
#[test]
#[ignore = "run as a child process by the other tests of this file"]
fn inner_children() {
    let Some(script) = std::env::var_os(SCRIPT) else { return };
    let number = |name: &str| std::env::var(name).unwrap().parse::<usize>().unwrap();
    let (threads, rounds) = (number(THREADS), number(ROUNDS));
    let limit = std::env::var(LIMIT)
        .map_or(DEFAULT_TIMEOUT, |secs| Duration::from_secs(secs.parse().unwrap()));
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                for _ in 0..rounds {
                    let outcome = run_with(&mut Command::new(&script), b"", limit);
                    assert!(outcome.success(), "{}", outcome.stderr_text());
                }
            });
        }
    });
}
