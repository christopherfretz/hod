//! Test support shared by hod's crates. Not shipped: a dev-dependency of
//! the crates, and a dependency of the harness (`xtask`).
//!
//! Tests that compare against an external oracle (LLVM's tools, binutils, a
//! linker) find it with [`tool`]. A missing tool makes the test skip with a
//! note, unless `HOD_REQUIRE_TOOLS=1`, which makes it a failure
//! (`docs/VALIDATION.md` "Oracles").
//!
//! Every process a test starts through [`run_with`], [`run`] or
//! [`write_script`] holds a token of the machine's jobserver for its
//! lifetime, when there is one ([`jobs`], `spec/harness.md` "Jobserver"): a
//! test binary is a jobserver client like the cargo that built it.
//!
//! [`target`] names a target's tools, system root and emulator, for the
//! tests that link and run programs of another architecture.

pub mod jobs;
pub mod target;

pub use target::Target;

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
/// directories are tried, newest first. The plain name on `PATH` is not:
/// its LLVM major is unknown, and a stock `clang++` of another major (a
/// hosted runner's LLVM 18) writes IR hod does not read
/// (`spec/harness.md` "Toolchain for tests"). Any other name, and a
/// versioned one (`llc-22`), is also looked up as given.
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
    if is_unversioned_llvm_tool(name) {
        return None;
    }
    on_path(name)
}

/// Whether `name` is an LLVM tool named without its major
/// ([`find_tool`]): `clang++`, `llc`, `ld.lld`, `llvm-dwarfdump`, but not
/// `llc-22`, `gcc` or `ld.bfd`.
pub fn is_unversioned_llvm_tool(name: &str) -> bool {
    let versioned = name
        .rsplit_once('-')
        .is_some_and(|(_, major)| !major.is_empty() && major.bytes().all(|b| b.is_ascii_digit()));
    let llvm = name.starts_with("llvm-")
        || matches!(
            name,
            "clang" | "clang++" | "clang-cpp" | "llc" | "opt" | "lli" | "lld" | "ld.lld" | "lldb"
        );
    llvm && !versioned
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

/// `<build directory>/tmp`, when this executable is in
/// `<build directory>/<profile>/deps` or `<build directory>/<profile>`.
fn build_tmp_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let mut dir = exe.parent()?;
    if dir.file_name()? == "deps" {
        dir = dir.parent()?;
    }
    // `dir` is the profile directory if its parent holds cargo's marker.
    let build = dir.parent()?;
    build.join("CACHEDIR.TAG").is_file().then(|| build.join("tmp"))
}

/// A scratch directory removed on drop.
///
/// Created in the build directory's `tmp` (the directory cargo calls
/// `CARGO_TARGET_TMPDIR`), found from the running test executable, which
/// cargo puts in `<build directory>/<profile>/deps`. Only a process that
/// cargo did not build falls back to the system temporary directory. The
/// name is unique to this process and call.
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
        let base = std::env::var_os("CARGO_TARGET_TMPDIR")
            .map(PathBuf::from)
            .or_else(build_tmp_dir)
            .unwrap_or_else(std::env::temp_dir);
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

impl Scratch {
    /// Writes an executable `sh` script inside the directory and returns
    /// its path. See [`write_script`].
    ///
    /// # Panics
    /// Panics if the script cannot be written.
    pub fn write_script(&self, name: &str, body: &str) -> PathBuf {
        let path = self.join(name);
        write_script(&path, body);
        path
    }
}

/// Writes an executable `sh` script (`#!/bin/sh`, then `body`) at `path`.
///
/// The file is written by a child process, not by this one: a file this
/// process has open for writing is inherited by every process another test
/// thread forks at that moment, and running the script (here or in a
/// grandchild) then fails with "text file busy" until that process calls
/// `exec`.
///
/// # Panics
/// Panics if the script cannot be written.
pub fn write_script(path: &Path, body: &str) {
    let mut writer = Command::new("sh");
    writer.args(["-c", "cat > \"$0\" && chmod 755 \"$0\""]).arg(path);
    let script = format!("#!/bin/sh\n{body}\n");
    let outcome = run_with(&mut writer, script.as_bytes(), DEFAULT_TIMEOUT);
    assert!(outcome.success(), "cannot write {}: {}", path.display(), outcome.stderr_text());
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
/// The process holds one token of the jobserver this test binary inherited
/// ([`jobs::global`]) from before it starts until it has ended, however it
/// ends; the time limit starts once the token is held, so waiting for the
/// pool is never a timeout. Without a jobserver nothing waits.
///
/// # Panics
/// Panics if the process cannot be started.
pub fn run_with(command: &mut Command, stdin: &[u8], timeout: Duration) -> Outcome {
    run_on(jobs::global(), command, stdin, timeout)
}

/// [`run_with`] as a client of `client`.
fn run_on(
    client: &jobs::Client,
    command: &mut Command,
    stdin: &[u8],
    timeout: Duration,
) -> Outcome {
    // One jobserver slot for the child's lifetime (released on every return
    // path, kill or panic by `Drop`); the time limit starts once it is held.
    let _slot = client.acquire();
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = spawn(command)
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

/// Starts a process, trying again while the program is "busy".
///
/// A test that writes a script and runs it can find it busy (`ETXTBSY`) for
/// a moment: a process forked by another test thread while the script was
/// being written holds it open for writing until that process calls `exec`.
fn spawn(command: &mut Command) -> std::io::Result<Child> {
    let mut attempts = 0;
    loop {
        match command.spawn() {
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy && attempts < 200 => {
                attempts += 1;
                std::thread::sleep(Duration::from_millis(5));
            }
            result => return result,
        }
    }
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

    // spec/harness.md "Toolchain for tests": an LLVM tool is found only
    // with a supported major, never as the plain name of whatever LLVM is
    // on PATH.
    #[test]
    fn llvm_tools_are_found_only_by_supported_major() {
        for name in ["clang++", "clang", "llc", "ld.lld", "lldb", "llvm-dwarfdump", "llvm-mc"] {
            assert!(is_unversioned_llvm_tool(name), "{name}");
        }
        for name in ["llc-22", "clang++-19", "llvm-as-19", "gcc", "g++", "ld.bfd", "mold", "gdb"] {
            assert!(!is_unversioned_llvm_tool(name), "{name}");
        }
        if let Some(found) = find_tool("clang++") {
            let file = found.file_name().unwrap().to_string_lossy().into_owned();
            let dir = found.parent().unwrap().to_string_lossy().into_owned();
            let supported = LLVM_MAJORS.iter().any(|major| {
                file == format!("clang++-{major}") || dir == format!("/usr/lib/llvm-{major}/bin")
            });
            assert!(supported, "{}", found.display());
        }
    }

    #[test]
    fn scratch_is_created_and_removed() {
        let path = {
            let scratch = Scratch::new("selftest");
            let file = scratch.write("a.txt", "x");
            assert!(file.is_file());
            // A test built by cargo keeps its scratch in the build
            // directory, not in the system's temporary directory.
            let build_tmp = build_tmp_dir().expect("tests run from a cargo build directory");
            assert!(scratch.path().starts_with(&build_tmp), "{:?}", scratch.path());
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
    fn scripts_are_runnable_at_once() {
        let scratch = Scratch::new("script");
        let script = scratch.write_script("hello", "echo \"hello $1\"");
        let outcome = run(&script, ["world"]);
        assert!(outcome.success(), "{}", outcome.stderr_text());
        assert_eq!(outcome.stdout_text(), "hello world\n");
    }

    #[test]
    fn run_kills_at_the_time_limit() {
        let outcome = run_with(Command::new("sleep").arg("30"), &[], Duration::from_millis(100));
        assert!(outcome.timed_out());
    }

    // spec/harness.md "Jobserver", "Scope" and "Release": a child of
    // `run_with` holds a token from before it starts until it has ended,
    // and a child killed at the time limit returns it too.
    #[test]
    fn run_holds_a_token_for_the_childs_lifetime() {
        let scratch = Scratch::new("run-token");
        let pool = jobs::PrivatePool::new("run-token", 1);
        let client = jobs::Client::open(pool.path()).unwrap();
        // With the implicit token taken, the child needs the pool's.
        let implicit = client.acquire();
        let (started, release) = (scratch.join("started"), scratch.join("release"));
        let script = format!(
            ": > '{}'; while [ ! -e '{}' ]; do sleep 0.02; done; echo done",
            started.display(),
            release.display()
        );
        std::thread::scope(|scope| {
            let child = scope.spawn(|| {
                run_on(&client, Command::new("sh").args(["-c", &script]), &[], DEFAULT_TIMEOUT)
            });
            let waited = Instant::now();
            while !started.exists() {
                assert!(waited.elapsed() < DEFAULT_TIMEOUT, "the child never started");
                std::thread::sleep(Duration::from_millis(5));
            }
            assert_eq!(pool.idle(), 0, "the running child holds the pool's token");
            std::fs::write(&release, "").unwrap();
            let outcome = child.join().unwrap();
            assert_eq!(outcome.stdout_text(), "done\n");
        });
        assert_eq!(pool.idle(), 1, "the token came back when the child ended");

        let killed =
            run_on(&client, Command::new("sleep").arg("30"), &[], Duration::from_millis(100));
        assert!(killed.timed_out());
        assert_eq!(pool.idle(), 1, "a child killed at the time limit returns its token");
        drop(implicit);
    }

    // "Jobserver", "Scope": the time limit starts once the token is held:
    // a child that waited for the pool longer than its limit is not timed
    // out.
    #[test]
    fn the_time_limit_starts_once_the_token_is_held() {
        let pool = jobs::PrivatePool::new("run-wait", 0);
        let client = jobs::Client::open(pool.path()).unwrap();
        let implicit = client.acquire();
        // The limit is also the time `true` has to start and end once the
        // token is free: 3 s, not less, because a spawn at load 30 has taken
        // a large part of a second.
        let limit = Duration::from_secs(3);
        std::thread::scope(|scope| {
            let child = scope.spawn(|| run_on(&client, Command::new("true").arg("x"), &[], limit));
            // Longer than the child's limit, with no slot for it.
            std::thread::sleep(limit + Duration::from_millis(300));
            assert!(!child.is_finished(), "the child ran without a token");
            drop(implicit);
            let outcome = child.join().unwrap();
            assert!(outcome.success() && !outcome.timed_out());
        });
    }
}
