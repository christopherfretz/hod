//! Bounded child processes. Contract: `spec/harness.md` "irdiff", "Child
//! processes".
//!
//! Every process the harness starts (the oracle, hod, the linker driver,
//! the programs under test) goes through [`run`]: a wall-clock limit this
//! process enforces by killing the child's whole process group, and an
//! address-space and a CPU-time limit the kernel enforces, which also hold
//! if this process dies first. No `unsafe` and no dependency: the limits
//! are set by a `/bin/sh` wrapper that then `exec`s the program.

use std::io::{Read, Write};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// What a child may use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Wall-clock time; the child's process group is killed when it passes.
    pub time: Duration,
    /// Address space, in KiB (`ulimit -v`).
    pub memory_kib: u64,
}

impl Limits {
    /// The CPU-time limit that backs the wall-clock one (`ulimit -t`), in
    /// seconds: the time limit rounded up, plus one.
    pub fn cpu_seconds(self) -> u64 {
        self.time.as_secs() + u64::from(self.time.subsec_nanos() > 0) + 1
    }
}

/// How a child ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    /// It exited with this status.
    Exit(i32),
    /// It was terminated by this signal.
    Signal(i32),
    /// It was killed at the time limit.
    TimedOut,
}

/// What a child did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    pub end: End,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// More than [`OUTPUT_CAP`] bytes were written to one of the streams;
    /// the rest was discarded.
    pub truncated: bool,
}

impl Output {
    /// The first non-empty line of standard error, for a diagnostic.
    pub fn first_error_line(&self) -> String {
        let text = String::from_utf8_lossy(&self.stderr);
        text.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("").to_owned()
    }
}

/// The most that is kept of each output stream.
pub const OUTPUT_CAP: usize = 16 << 20;

/// How long the output of a child that has ended is waited for. A
/// descendant that left the process group and holds the pipe open cannot
/// hold the harness longer than this.
const DRAIN_GRACE: Duration = Duration::from_secs(2);

/// The exit status and message of the wrapper when it cannot set a limit.
const SETUP_FAILED: i32 = 125;
const SETUP_MESSAGE: &str = "irdiff: cannot set the resource limits";

/// `$1`: the address-space limit in KiB. `$2`: the CPU limit in seconds.
/// The rest: the program and its arguments. Core files are switched off:
/// a test program may be expected to die by a signal.
const WRAPPER: &str = "ulimit -c 0 && ulimit -v \"$1\" && ulimit -t \"$2\" || \
                       { echo 'irdiff: cannot set the resource limits' >&2; exit 125; }\n\
                       shift 2\n\
                       exec \"$@\"";

/// Runs `program` under `limits` with `stdin` as its input, capturing its
/// output. `configure` adds the arguments, environment and working
/// directory to the command (its `arg` calls append the program's
/// arguments).
///
/// An error is a failure of the harness or the machine, not of the
/// program: the program is missing, the process cannot be started, or a
/// limit cannot be set.
pub fn run(
    program: &Path,
    configure: impl FnOnce(&mut Command),
    stdin: &[u8],
    limits: Limits,
) -> Result<Output, String> {
    if !program.is_file() {
        return Err(format!("cannot run '{}': no such file", program.display()));
    }
    let mut command = Command::new("/bin/sh");
    command.arg("-c").arg(WRAPPER).arg("sh");
    command.arg(limits.memory_kib.to_string()).arg(limits.cpu_seconds().to_string());
    command.arg(program);
    configure(&mut command);
    command.stdin(if stdin.is_empty() { Stdio::null() } else { Stdio::piped() });
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    // Its own process group, so that the time limit reaches everything the
    // child started (a compiler driver's linker, a script's commands).
    command.process_group(0);
    let mut child =
        command.spawn().map_err(|e| format!("cannot run '{}': {e}", program.display()))?;

    if let Some(mut pipe) = child.stdin.take() {
        let input = stdin.to_vec();
        // A child that does not read its input closes the pipe when it
        // ends; that is not an error of ours.
        std::thread::spawn(move || {
            let _ = pipe.write_all(&input);
        });
    }
    let stdout = Capture::start(child.stdout.take());
    let stderr = Capture::start(child.stderr.take());

    let status = wait_until(&mut child, Instant::now() + limits.time);
    let drained = Instant::now() + DRAIN_GRACE;
    if !(stdout.wait(drained) && stderr.wait(drained)) {
        // Something still holds the pipes: what is left of the group goes.
        kill_group(child.id());
    }
    let (stdout, out_truncated) = stdout.take();
    let (stderr, err_truncated) = stderr.take();
    let end = match status {
        None => End::TimedOut,
        Some(status) => match (status.code(), status.signal()) {
            (Some(code), _) => End::Exit(code),
            (None, Some(signal)) => End::Signal(signal),
            (None, None) => return Err(format!("'{}' ended without a status", program.display())),
        },
    };
    if end == End::Exit(SETUP_FAILED) && stderr.starts_with(SETUP_MESSAGE.as_bytes()) {
        return Err(format!(
            "cannot limit '{}' to {} KiB of address space and {} s of CPU time",
            program.display(),
            limits.memory_kib,
            limits.cpu_seconds()
        ));
    }
    Ok(Output { end, stdout, stderr, truncated: out_truncated || err_truncated })
}

/// Waits for the child; at the deadline kills its process group and
/// returns `None`.
fn wait_until(child: &mut Child, deadline: Instant) -> Option<ExitStatus> {
    let mut pause = Duration::from_micros(200);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(pause);
                pause = (pause * 2).min(Duration::from_millis(10));
            }
            _ => {
                // The leader is not reaped yet, so its process group id
                // cannot have been reused.
                kill_group(child.id());
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

/// Sends `SIGKILL` to every process of the group `pgid`. The standard
/// library can only signal one process; the shell's `kill` takes a group.
fn kill_group(pgid: u32) {
    let _ = Command::new("/bin/sh")
        .args(["-c", "kill -KILL \"-$1\"", "sh"])
        .arg(pgid.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// One output stream being read on its own thread.
struct Capture {
    kept: Arc<Mutex<(Vec<u8>, bool)>>,
    done: Receiver<()>,
}

impl Capture {
    fn start(pipe: Option<impl Read + Send + 'static>) -> Self {
        let kept = Arc::new(Mutex::new((Vec::new(), false)));
        let (finished, done) = channel();
        if let Some(mut pipe) = pipe {
            let kept = Arc::clone(&kept);
            std::thread::spawn(move || {
                let mut chunk = vec![0; 1 << 16];
                loop {
                    match pipe.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let mut kept = kept.lock().unwrap_or_else(PoisonError::into_inner);
                            let room = OUTPUT_CAP - kept.0.len();
                            kept.0.extend_from_slice(&chunk[..n.min(room)]);
                            kept.1 |= n > room;
                        }
                    }
                }
                let _ = finished.send(());
            });
        }
        Self { kept, done }
    }

    /// Whether the stream ended before `deadline`.
    fn wait(&self, deadline: Instant) -> bool {
        // A reader that is gone has dropped its sender: the stream ended.
        !matches!(
            self.done.recv_timeout(deadline.saturating_duration_since(Instant::now())),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        )
    }

    /// What was read so far, and whether anything was discarded.
    fn take(&self) -> (Vec<u8>, bool) {
        let mut kept = self.kept.lock().unwrap_or_else(PoisonError::into_inner);
        (std::mem::take(&mut kept.0), kept.1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hod_testutil::Scratch;

    const ROOMY: Limits = Limits { time: Duration::from_secs(20), memory_kib: 1 << 20 };

    fn sh(script: &str, stdin: &[u8], limits: Limits) -> Output {
        run(
            Path::new("/bin/sh"),
            |c| {
                c.arg("-c").arg(script);
            },
            stdin,
            limits,
        )
        .unwrap()
    }

    // spec/harness.md "irdiff", "Child processes": output, input, status.
    #[test]
    fn captures_output_and_status() {
        let output = sh("cat; echo err >&2; exit 3", b"input", ROOMY);
        assert_eq!(output.end, End::Exit(3));
        assert_eq!(output.stdout, b"input");
        assert_eq!(output.stderr, b"err\n");
        assert!(!output.truncated);
        assert_eq!(output.first_error_line(), "err");

        // Arguments, environment and directory come from `configure`.
        let scratch = Scratch::new("irdiff-process");
        let output = run(
            Path::new("/bin/sh"),
            |c| {
                c.args(["-c", "echo \"$0 $1 $V\"; pwd", "zero", "one"]);
                c.env("V", "value").current_dir(scratch.path());
            },
            b"",
            ROOMY,
        )
        .unwrap();
        let dir = scratch.path().canonicalize().unwrap();
        assert_eq!(output.stdout, format!("zero one value\n{}\n", dir.display()).as_bytes());
    }

    // "Child processes": a signal is told from an exit status.
    #[test]
    fn reports_a_signal() {
        assert_eq!(sh("kill -SEGV $$", b"", ROOMY).end, End::Signal(11));
        assert_eq!(sh("exit 139", b"", ROOMY).end, End::Exit(139));
    }

    // "Child processes": the time limit kills the whole process group, and
    // what the child wrote before is kept.
    #[test]
    fn time_limit_kills_the_group() {
        let scratch = Scratch::new("irdiff-timeout");
        let marker = scratch.join("grandchild-survived");
        let script = format!(
            "echo started; (sleep 3; echo late; touch '{}') & sleep 60; echo never",
            marker.display()
        );
        let limits = Limits { time: Duration::from_millis(400), ..ROOMY };
        let started = Instant::now();
        let output = sh(&script, b"", limits);
        assert_eq!(output.end, End::TimedOut);
        assert_eq!(output.stdout, b"started\n");
        assert!(started.elapsed() < Duration::from_secs(3), "{:?}", started.elapsed());
        // The background child of the script died with it.
        std::thread::sleep(Duration::from_millis(3200).saturating_sub(started.elapsed()));
        assert!(!marker.exists(), "a grandchild outlived the time limit");
    }

    // "Child processes": a child that never reads its input, and one that
    // leaves a descendant holding its output, do not hold the harness.
    #[test]
    fn never_hangs_on_pipes() {
        let big = vec![b'x'; 1 << 20];
        assert_eq!(sh("exit 0", &big, ROOMY).end, End::Exit(0));

        let started = Instant::now();
        let output = sh("echo first; (sleep 30 &) ; exit 4", b"", ROOMY);
        assert_eq!(output.end, End::Exit(4));
        assert_eq!(output.stdout, b"first\n");
        assert!(started.elapsed() < Duration::from_secs(15), "{:?}", started.elapsed());
    }

    // "Child processes": both kernel limits are in force in the child, and
    // core files are off.
    #[test]
    fn limits_are_set_in_the_child() {
        let limits = Limits { time: Duration::from_millis(2500), memory_kib: 123_456 };
        assert_eq!(limits.cpu_seconds(), 4);
        assert_eq!(Limits { time: Duration::from_secs(10), memory_kib: 1 }.cpu_seconds(), 11);
        let output = sh("ulimit -v; ulimit -t; ulimit -c", b"", limits);
        assert_eq!(output.stdout, b"123456\n4\n0\n");
        // They are inherited by what the child starts.
        let output = sh("sh -c 'ulimit -v'", b"", limits);
        assert_eq!(output.stdout, b"123456\n");
    }

    // "Child processes": the address-space limit fires. `dd` asks for a
    // 256 MiB buffer and is refused.
    #[test]
    fn memory_limit_fires() {
        let script = "dd if=/dev/zero of=/dev/null bs=268435456 count=1 2>&1; echo \"status $?\"";
        let within = sh(script, b"", ROOMY);
        assert!(within.stdout.ends_with(b"status 0\n"), "{within:?}");
        let limits = Limits { memory_kib: 64 << 10, ..ROOMY };
        let refused = sh(script, b"", limits);
        assert!(refused.stdout.ends_with(b"status 1\n"), "{refused:?}");
    }

    // "Child processes": output beyond the cap is discarded, not stored.
    #[test]
    fn output_is_capped() {
        let output = sh("dd if=/dev/zero bs=1048576 count=20 2>/dev/null", b"", ROOMY);
        assert_eq!(output.end, End::Exit(0));
        assert_eq!(output.stdout.len(), OUTPUT_CAP);
        assert!(output.truncated);
    }

    // "Child processes": what is the harness's failure, not the child's.
    #[test]
    fn harness_errors() {
        let missing = run(Path::new("/nonexistent/tool"), |_| {}, b"", ROOMY).unwrap_err();
        assert!(missing.contains("cannot run '/nonexistent/tool'"), "{missing}");

        // A limit that cannot be set (not a number) is reported as such.
        let limits = Limits { memory_kib: u64::MAX, ..ROOMY };
        match run(
            Path::new("/bin/sh"),
            |c| {
                c.args(["-c", "exit 0"]);
            },
            b"",
            limits,
        ) {
            // Some shells accept any number as "unlimited enough".
            Ok(output) => assert_eq!(output.end, End::Exit(0)),
            Err(message) => assert!(message.contains("cannot limit"), "{message}"),
        }
        // A program that exits with the wrapper's status is not mistaken
        // for a setup failure.
        assert_eq!(sh("echo oops >&2; exit 125", b"", ROOMY).end, End::Exit(125));
    }
}
