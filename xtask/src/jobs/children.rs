//! The live children of xtask, and the cargo steps (`spec/harness.md`
//! "Jobserver", "Signals"). Ported from kiln's `xtask/src/util/process.rs`.
//!
//! Every child `irdiff::process::run` starts is registered in a
//! process-wide table of live children (each the leader of its own process
//! group) until it is reaped, so that [`kill_live_children`], which the
//! SIGINT/SIGTERM thread of [`crate::jobs`] calls, can kill and reap all of
//! them before xtask exits; after it no new child starts (spawning fails
//! with `Interrupted`). [`run_step`] runs a cargo step the same way, but
//! without a timeout or a jobserver token, and is stopped with `SIGTERM`
//! before `SIGKILL`.

use std::io::{self, Write};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

/// Runs a cargo step (the `cargo fmt`/`clippy`/`test` steps of `cargo
/// xtask ci`, the `cargo build` of `irdiff::build_workspace_hod`) to
/// completion: no timeout and no jobserver token of xtask's (cargo is a
/// jobserver client itself, its rustc and test processes take their own).
/// stdin is closed and stdout inherited. With `tee` stderr is piped,
/// copied to ours line by line as it arrives and returned; without it,
/// inherited (the returned string is empty).
///
/// When the SIGINT/SIGTERM thread of [`crate::jobs`] runs, the step is the
/// leader of its own process group and in the live-children table from
/// spawn to reap, so [`kill_live_children`] stops cargo and everything it
/// started (rustc, the test binaries): `SIGTERM` to the group first,
/// `SIGKILL` to whatever is left after [`TERM_GRACE`]. Without that
/// thread it stays in xtask's group, where a terminal ^C reaches it
/// directly, as before.
///
/// A step that ended because xtask is being interrupted does not return:
/// the calling thread waits for the signal thread to finish returning the
/// jobserver tokens and end the process, instead of racing it to `exit`.
pub fn run_step(cmd: &mut Command, tee: bool) -> io::Result<(ExitStatus, String)> {
    let table = crate::jobs::signals_caught().then_some(&LIVE);
    let r = run_step_in(table, cmd, tee);
    park_if_interrupted();
    r
}

fn run_step_in(
    table: Option<&LiveChildren>,
    cmd: &mut Command,
    tee: bool,
) -> io::Result<(ExitStatus, String)> {
    cmd.stdin(Stdio::null());
    if tee {
        cmd.stderr(Stdio::piped());
    }
    let (mut child, live) = match table {
        Some(table) => {
            #[cfg(unix)]
            std::os::unix::process::CommandExt::process_group(cmd, 0);
            let spawning = table.begin_spawn()?;
            let child = cmd.spawn();
            let live = spawning.spawned(child.as_ref().map(Child::id).ok(), Stop::Term);
            (child?, Some(live))
        }
        None => (cmd.spawn()?, None),
    };
    let mut captured = Vec::new();
    if let Some(pipe) = child.stderr.take() {
        let mut reader = io::BufReader::new(pipe);
        let mut chunk = Vec::new();
        let mut out = io::stderr();
        loop {
            chunk.clear();
            match io::BufRead::read_until(&mut reader, b'\n', &mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let _ = out.write_all(&chunk);
                    captured.extend_from_slice(&chunk);
                }
            }
        }
    }
    let status = child.wait();
    // Reaped: only now may the pid leave the table.
    drop(live);
    Ok((status?, String::from_utf8_lossy(&captured).into_owned()))
}

/// The live children of this process: the pids (each its own process-group
/// leader) between spawn and reap, the spawns in flight, and whether
/// [`kill_live_children`] has run.
struct LiveChildren {
    state: Mutex<LiveState>,
    changed: Condvar,
}

#[derive(Default)]
struct LiveState {
    pids: Vec<(u32, Stop)>,
    spawning: usize,
    shutdown: bool,
}

static LIVE: LiveChildren = LiveChildren::new();

/// How [`kill_live_children`] stops a live child's process group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    /// `SIGKILL` at once: a tool or a test's program of
    /// `irdiff::process::run`.
    Kill,
    /// `SIGTERM`, then `SIGKILL` after [`TERM_GRACE`]: a cargo step of
    /// [`run_step`], whose processes may still clean up (a test binary's
    /// temporary files, cargo's own `Drop`s).
    Term,
}

/// How long a [`Stop::Term`] group gets after `SIGTERM` before `SIGKILL`.
const TERM_GRACE: Duration = Duration::from_secs(3);

/// A spawn in flight: counted so that a shutdown waits for (and kills) a
/// child whose pid is not known yet. Always ended by [`Spawning::spawned`]
/// right after the `spawn` call.
#[must_use]
pub struct Spawning<'a>(&'a LiveChildren);

/// A registered child, removed from the table on drop (after it is reaped,
/// or on any early return or panic).
#[must_use]
pub struct LiveChild<'a> {
    table: &'a LiveChildren,
    pid: Option<u32>,
}

impl std::fmt::Debug for Spawning<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Spawning")
    }
}

impl std::fmt::Debug for LiveChild<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveChild").field("pid", &self.pid).finish()
    }
}

impl LiveChildren {
    const fn new() -> Self {
        Self {
            state: Mutex::new(LiveState { pids: Vec::new(), spawning: 0, shutdown: false }),
            changed: Condvar::new(),
        }
    }

    /// The state is a pid list and two counters updated under the lock, so
    /// a poisoned lock still holds a consistent value.
    fn lock(&self) -> MutexGuard<'_, LiveState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn begin_spawn(&self) -> io::Result<Spawning<'_>> {
        let mut st = self.lock();
        if st.shutdown {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "xtask is shutting down"));
        }
        st.spawning += 1;
        Ok(Spawning(self))
    }

    /// Sets the shutdown flag, stops the process group of every live child
    /// ([`Stop`]: `SIGKILL`, or `SIGTERM` and `SIGKILL` after `term_grace`
    /// for one still live then), and waits (at most `grace`) until each is
    /// reaped and no spawn is in flight; a child spawned meanwhile is
    /// killed as it registers. Returns how many groups were stopped.
    fn shutdown(&self, grace: Duration, term_grace: Duration) -> usize {
        let start = Instant::now();
        let deadline = start + grace;
        let mut escalate_at = Some(start + term_grace.min(grace));
        let mut st = self.lock();
        st.shutdown = true;
        // Signalled under the lock: a pid leaves the table only once reaped,
        // so none of these can have been reused.
        let stopped = st.pids.len();
        for &(pid, stop) in &st.pids {
            signal_group(pid, if stop == Stop::Term { "TERM" } else { "KILL" });
        }
        while !(st.pids.is_empty() && st.spawning == 0) {
            let now = Instant::now();
            if escalate_at.is_some_and(|at| now >= at) {
                for &(pid, stop) in &st.pids {
                    if stop == Stop::Term {
                        signal_group(pid, "KILL");
                    }
                }
                escalate_at = None;
            }
            let left = escalate_at.unwrap_or(deadline).saturating_duration_since(now);
            if left.is_zero() {
                break;
            }
            st = self.changed.wait_timeout(st, left).unwrap_or_else(PoisonError::into_inner).0;
        }
        stopped
    }
}

impl<'a> Spawning<'a> {
    /// Ends the spawn: registers `pid` (the spawn's result) with how a
    /// shutdown stops it, killing it at once if a shutdown started
    /// meanwhile (it has had no time to do anything worth cleaning up).
    pub fn spawned(self, pid: Option<u32>, stop: Stop) -> LiveChild<'a> {
        let table = self.0;
        let mut st = table.lock();
        st.spawning -= 1;
        if let Some(pid) = pid {
            st.pids.push((pid, stop));
            if st.shutdown {
                kill_group(pid);
            }
        }
        drop(st);
        table.changed.notify_all();
        LiveChild { table, pid }
    }
}

impl Drop for LiveChild<'_> {
    fn drop(&mut self) {
        let Some(pid) = self.pid else { return };
        let mut st = self.table.lock();
        if let Some(i) = st.pids.iter().position(|&(p, _)| p == pid) {
            st.pids.swap_remove(i);
        }
        drop(st);
        self.table.changed.notify_all();
    }
}

/// Starts a spawn in the table of live children: call it right before
/// `Command::spawn`, and [`Spawning::spawned`] right after. The child must
/// be the leader of its own process group. Fails with `Interrupted` once
/// xtask is being interrupted.
pub fn begin_spawn() -> io::Result<Spawning<'static>> {
    LIVE.begin_spawn()
}

/// Whether [`kill_live_children`] has run: xtask is being interrupted.
pub fn interrupted() -> bool {
    LIVE.lock().shutdown
}

/// Does not return once xtask is being interrupted: the calling thread
/// leaves the end of the process to the signal thread, instead of going on
/// with the result of a killed child, or racing the signal thread to
/// `exit`.
pub fn park_if_interrupted() {
    if interrupted() {
        loop {
            thread::park();
        }
    }
}

/// Kills (`SIGKILL`, by process group) every child started by
/// `irdiff::process::run` that is still running, stops every [`run_step`]
/// cargo step (`SIGTERM`, then `SIGKILL` after [`TERM_GRACE`]), waits up
/// to `grace` for their reaping, and makes every later spawn fail. Called
/// once, by the SIGINT/SIGTERM thread, before the jobserver tokens are
/// returned. Returns how many children were stopped.
pub fn kill_live_children(grace: Duration) -> usize {
    LIVE.shutdown(grace, TERM_GRACE)
}

/// Sends `SIGKILL` to every process of the group `pgid`.
pub fn kill_group(pgid: u32) {
    signal_group(pgid, "KILL");
}

/// Sends the signal named `sig` to every process of the group `pgid`. The
/// standard library can only signal one process; the shell's `kill` takes
/// a group (and `/bin/sh` is there whatever `PATH` is). The leader is not
/// reaped while it is in the table, so its group id cannot have been
/// reused.
#[cfg(unix)]
fn signal_group(pgid: u32, sig: &str) {
    let _ = Command::new("/bin/sh")
        .args(["-c", "kill \"-$1\" \"-$2\"", "sh", sig])
        .arg(pgid.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(not(unix))]
fn signal_group(_pgid: u32, _sig: &str) {}

// spec/harness.md "Jobserver", "Signals" step 1. Ported from kiln's tests
// of the same names. Each test has a table of its own: the process's table
// is shared with the other tests of this process.
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use hod_testutil::Scratch;
    use std::os::unix::process::{CommandExt as _, ExitStatusExt as _};
    use std::path::Path;

    /// The shutdown kills a registered child's whole group and waits for its
    /// reaping; a spawn in flight when it starts is killed as it registers;
    /// later spawns fail.
    #[test]
    fn shutdown_kills_and_reaps_live_children() {
        static TABLE: LiveChildren = LiveChildren::new();
        let start = |spawning: Spawning<'static>| {
            let mut c = Command::new("sh");
            c.args(["-c", "sleep 30; :"]).process_group(0);
            let mut child = c.spawn().unwrap();
            let live = spawning.spawned(Some(child.id()), Stop::Kill);
            thread::spawn(move || {
                let s = child.wait().unwrap();
                drop(live);
                s
            })
        };
        let first = start(TABLE.begin_spawn().unwrap());
        let late = TABLE.begin_spawn().unwrap();
        let late = thread::spawn(move || {
            thread::sleep(Duration::from_millis(300));
            start(late).join().unwrap()
        });
        let t = Instant::now();
        assert_eq!(TABLE.shutdown(Duration::from_secs(20), TERM_GRACE), 1);
        assert!(t.elapsed() < Duration::from_secs(10), "{:?}", t.elapsed());
        assert_eq!(first.join().unwrap().signal(), Some(9));
        assert_eq!(late.join().unwrap().signal(), Some(9));
        assert_eq!(TABLE.lock().pids, Vec::new());
        let refused = TABLE.begin_spawn().map(|_| ()).unwrap_err();
        assert_eq!(refused.kind(), io::ErrorKind::Interrupted);
    }

    /// Starts `script` (under `sh -c`, after it made `<dir>/ready`) as a
    /// `run_step` cargo step in `table`, on a thread, and waits until it is
    /// registered and ready.
    fn start_step(
        table: &'static LiveChildren,
        dir: &Path,
        script: &str,
    ) -> thread::JoinHandle<(ExitStatus, String)> {
        let ready = dir.join("ready");
        let mut c = Command::new("sh");
        c.args([
            "-c",
            &format!("{script}; : > '{}'; while :; do sleep 0.05; done", ready.display()),
        ]);
        let step = thread::spawn(move || run_step_in(Some(table), &mut c, true).unwrap());
        let t = Instant::now();
        while !(ready.exists() && table.lock().pids.len() == 1) {
            assert!(t.elapsed() < Duration::from_secs(10), "the step never got ready");
            thread::sleep(Duration::from_millis(10));
        }
        step
    }

    /// A cargo step is in the live-children table from spawn to reap (as a
    /// `Stop::Term` group leader), and a shutdown stops it with `SIGTERM`
    /// first: its `trap` runs, and the stderr it wrote is still copied and
    /// returned.
    #[test]
    fn run_step_is_registered_and_terminated() {
        static TABLE: LiveChildren = LiveChildren::new();
        let dir = Scratch::new("step-term");
        let step = start_step(&TABLE, dir.path(), "trap 'echo term >&2; exit 7' TERM; echo up >&2");
        assert_eq!(TABLE.lock().pids.first().map(|&(_, stop)| stop), Some(Stop::Term));
        let t = Instant::now();
        assert_eq!(TABLE.shutdown(Duration::from_secs(20), Duration::from_secs(10)), 1);
        assert!(t.elapsed() < Duration::from_secs(8), "{:?}", t.elapsed());
        let (status, stderr) = step.join().unwrap();
        assert_eq!(status.code(), Some(7), "{status:?}");
        // (The shell may report its killed `sleep` in between.)
        assert!(stderr.starts_with("up\n") && stderr.ends_with("term\n"), "{stderr:?}");
        assert_eq!(TABLE.lock().pids, Vec::new());
    }

    /// A step that ignores `SIGTERM` is killed once the grace is over.
    #[test]
    fn run_step_ignoring_term_is_killed() {
        static TABLE: LiveChildren = LiveChildren::new();
        let dir = Scratch::new("step-kill");
        let step = start_step(&TABLE, dir.path(), "trap '' TERM");
        let t = Instant::now();
        assert_eq!(TABLE.shutdown(Duration::from_secs(20), Duration::from_millis(300)), 1);
        assert!(t.elapsed() < Duration::from_secs(10), "{:?}", t.elapsed());
        assert_eq!(step.join().unwrap().0.signal(), Some(9));
    }

    /// Without a table (no signal thread) a step just runs: its status and
    /// stderr come back.
    #[test]
    fn run_step_without_table() {
        let mut c = Command::new("sh");
        c.args(["-c", "echo e >&2; exit 3"]);
        let (status, stderr) = run_step_in(None, &mut c, true).unwrap();
        assert_eq!((status.code(), stderr.as_str()), (Some(3), "e\n"));
        let (status, stderr) = run_step_in(None, &mut Command::new("true"), false).unwrap();
        assert!(status.success() && stderr.is_empty());
    }

    /// The step of the process's own table: it runs, and nothing is
    /// interrupted.
    #[test]
    fn run_step_runs_a_command() {
        let (status, stderr) = run_step(Command::new("sh").args(["-c", "exit 4"]), false).unwrap();
        assert_eq!((status.code(), stderr.as_str()), (Some(4), ""));
        assert!(!interrupted());
        park_if_interrupted();
    }
}
