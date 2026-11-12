//! Test support shared by hod's crates. Not shipped; a dev-dependency only.
//!
//! Tests that compare against an external oracle (LLVM's tools, binutils, a
//! linker) find it with [`tool`]. A missing tool makes the test skip with a
//! note, unless `HOD_REQUIRE_TOOLS=1`, which makes it a failure
//! (`docs/VALIDATION.md` "Oracles").

use std::ffi::OsStr;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

/// LLVM majors searched, preferred first.
pub const LLVM_MAJORS: [u32; 2] = [22, 19];

/// Whether a missing external tool is a failure rather than a skip.
pub fn tools_required() -> bool {
    std::env::var_os("HOD_REQUIRE_TOOLS").is_some_and(|v| v == "1")
}

/// Finds an external tool by base name.
///
/// For an LLVM tool (`llvm-mc`, `llc`, `ld.lld`, ...) the versioned names
/// `<name>-22` and `<name>-19` on `PATH` and the `/usr/lib/llvm-N/bin`
/// directories are tried before the plain name, newest first.
pub fn find_tool(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let on_path =
        |file: &str| std::env::split_paths(&path).map(|dir| dir.join(file)).find(|p| p.is_file());
    for major in LLVM_MAJORS {
        if let Some(found) = on_path(&format!("{name}-{major}")) {
            return Some(found);
        }
        let installed = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/{name}"));
        if installed.is_file() {
            return Some(installed);
        }
    }
    on_path(name)
}

/// Finds a tool for a test, or says the test must skip.
///
/// Returns `None` after printing a note when the tool is missing and tools
/// are optional.
///
/// # Panics
/// Panics when the tool is missing and `HOD_REQUIRE_TOOLS=1`.
pub fn tool(name: &str) -> Option<PathBuf> {
    let found = find_tool(name);
    if found.is_none() {
        assert!(!tools_required(), "required tool `{name}` not found (HOD_REQUIRE_TOOLS=1)");
        eprintln!("note: `{name}` not found; skipping the tool-backed part of this test");
    }
    found
}

/// A scratch directory removed on drop.
///
/// Created under `CARGO_TARGET_TMPDIR` when cargo provides it (integration
/// tests), otherwise under the system temporary directory, with a name
/// unique to this process and call.
#[derive(Debug)]
pub struct Scratch {
    path: PathBuf,
}

impl Scratch {
    /// Creates a new scratch directory whose name starts with `label`.
    ///
    /// # Panics
    /// Panics if the directory cannot be created.
    pub fn new(label: &str) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let base =
            std::env::var_os("CARGO_TARGET_TMPDIR").map_or_else(std::env::temp_dir, PathBuf::from);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = base.join(format!("hod-{label}-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&path)
            .unwrap_or_else(|e| panic!("cannot create {}: {e}", path.display()));
        Self { path }
    }

    /// The directory.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// A path inside the directory.
    pub fn join(&self, name: impl AsRef<Path>) -> PathBuf {
        self.path.join(name)
    }

    /// Writes a file inside the directory and returns its path.
    ///
    /// # Panics
    /// Panics if the file cannot be written.
    pub fn write(&self, name: &str, contents: impl AsRef<[u8]>) -> PathBuf {
        let path = self.join(name);
        std::fs::write(&path, contents)
            .unwrap_or_else(|e| panic!("cannot write {}: {e}", path.display()));
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// What a bounded child process did.
#[derive(Debug)]
pub struct Outcome {
    /// `None` when the process was killed at the time limit.
    pub status: Option<ExitStatus>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl Outcome {
    /// Exited with status zero.
    pub fn success(&self) -> bool {
        self.status.is_some_and(|s| s.success())
    }

    /// Killed at the time limit.
    pub fn timed_out(&self) -> bool {
        self.status.is_none()
    }

    /// Standard output as text, lossily decoded.
    pub fn stdout_text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    /// Standard error as text, lossily decoded.
    pub fn stderr_text(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }
}

/// Default time limit for a child process in a test.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_mins(1);

/// Runs a command with `stdin` as its input and a time limit, capturing its
/// output. The process is killed when the limit passes.
///
/// # Panics
/// Panics if the process cannot be started.
pub fn run_with(command: &mut Command, stdin: &[u8], timeout: Duration) -> Outcome {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("cannot run {}: {e}", command.get_program().display()));
    let mut input = child.stdin.take().expect("piped stdin");
    let mut out = child.stdout.take().expect("piped stdout");
    let mut err = child.stderr.take().expect("piped stderr");
    let stdin = stdin.to_vec();
    let (stdout, stderr, status) = std::thread::scope(|scope| {
        // A child that exits early closes the pipe; that is not our error.
        scope.spawn(move || {
            let _ = input.write_all(&stdin);
        });
        let out_reader = scope.spawn(move || {
            let mut buf = Vec::new();
            let _ = out.read_to_end(&mut buf);
            buf
        });
        let err_reader = scope.spawn(move || {
            let mut buf = Vec::new();
            let _ = err.read_to_end(&mut buf);
            buf
        });
        let status = wait_until(&mut child, Instant::now() + timeout);
        (out_reader.join().unwrap_or_default(), err_reader.join().unwrap_or_default(), status)
    });
    Outcome { status, stdout, stderr }
}

/// Runs `program` with `args`, no input, and the default time limit.
pub fn run<I, S>(program: impl AsRef<OsStr>, args: I) -> Outcome
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    run_with(Command::new(program).args(args), &[], DEFAULT_TIMEOUT)
}

fn wait_until(child: &mut Child, deadline: Instant) -> Option<ExitStatus> {
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(2));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scratch_is_created_and_removed() {
        let path = {
            let scratch = Scratch::new("selftest");
            let file = scratch.write("a.txt", "x");
            assert!(file.is_file());
            scratch.path().to_path_buf()
        };
        assert!(!path.exists());
    }

    #[test]
    fn run_captures_output_and_status() {
        let outcome = run_with(
            Command::new("sh").args(["-c", "cat; echo err >&2; exit 3"]),
            b"in",
            DEFAULT_TIMEOUT,
        );
        assert_eq!(outcome.stdout, b"in");
        assert_eq!(outcome.stderr_text(), "err\n");
        assert_eq!(outcome.status.and_then(|s| s.code()), Some(3));
        assert!(!outcome.success() && !outcome.timed_out());
    }

    #[test]
    fn run_kills_at_the_time_limit() {
        let outcome = run_with(Command::new("sleep").arg("30"), &[], Duration::from_millis(100));
        assert!(outcome.timed_out());
    }
}
