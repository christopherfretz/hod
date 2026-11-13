//! Debugger sessions on a program of another architecture (WP-136):
//! the program runs under its user-mode emulator's gdb stub
//! (`qemu-<arch> -g <port>`) and the debugger attaches to it, gdb as
//! `gdb-multiarch` (`target remote`), lldb by `gdb-remote`. Contract:
//! `spec/harness.md` "debug", "Other targets".
//!
//! One session is one child of the harness (`irdiff::process::run`, with
//! its limits and its jobserver token): a `/bin/sh` script that starts the
//! emulator in the background, waits until its stub listens, runs the
//! debugger, then kills and reaps the emulator. The whole script is one
//! process group, which the harness kills at the session's time limit, so
//! the emulator never outlives its session. Each session listens on its
//! own port, which the operating system chose ([`free_port`]): many
//! sessions (and other users of the machine) run at once.

use std::ffi::OsString;
use std::fmt::Write as _;
use std::net::{Ipv4Addr, TcpListener};
use std::path::{Path, PathBuf};

use super::transcript::{self, Debugger, Ended, GDB_PRELUDE};
use crate::irdiff::directives::Arch;
use crate::irdiff::process::{self, End, Limits};
use crate::irdiff::tools::{self, Search};

/// A run for a target that is not this machine's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cross {
    pub arch: Arch,
    pub remote: Remote,
}

/// The emulator and system root of `arch`'s programs.
///
/// # Errors
/// When either is missing: a cross run cannot run a program without them.
pub fn find(arch: Arch, search: &Search) -> Result<Cross, String> {
    let target = tools::target_of(arch);
    let emulator = tools::find_emulator(arch, search).ok_or_else(|| {
        format!(
            "--target {}: the emulator '{}' is not found (Debian package {})",
            arch.name(),
            target.emulator(),
            target.cross_package("emulator")
        )
    })?;
    let sysroot = target.sysroot();
    if !sysroot.is_dir() {
        return Err(format!(
            "--target {}: the system root '{}' is not found (Debian package {})",
            arch.name(),
            sysroot.display(),
            target.cross_package("sysroot")
        ));
    }
    let args = target.emulator_args(&sysroot);
    Ok(Cross { arch, remote: Remote { emulator, args, sysroot } })
}

/// The debuggers of a cross run: gdb is `gdb-multiarch` (still named
/// `gdb`, so that scripts and scopes name it as on the host); without
/// it, no gdb. lldb debugs every target it was built for as it is.
pub fn remote_debuggers(debuggers: &mut Vec<Debugger>, search: &Search) {
    let multiarch = search.on_path("gdb-multiarch");
    debuggers.retain(|d| !d.is_gdb());
    if let Some(path) = multiarch {
        debuggers.insert(0, Debugger { name: "gdb".to_owned(), path });
    }
}

/// A remote session's transcript with what differs between two sessions
/// of the same program masked, besides [`transcript::mask`]'s: the port
/// (`127.0.0.1:PORT`, each session has its own), and the process and
/// thread ids of gdb's `Thread <pid>.<tid>` (the stub's names of threads).
pub fn mask_remote(text: &str, port: u16) -> String {
    let text = text.replace(&format!("127.0.0.1:{port}"), "127.0.0.1:PORT");
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while let Some(at) = rest.find("Thread ") {
        out.push_str(&rest[..at + 7]);
        rest = &rest[at + 7..];
        let digits = |s: &str| s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
        let pid = digits(rest);
        if pid > 0 && rest[pid..].starts_with('.') {
            let tid = digits(&rest[pid + 1..]);
            if tid > 0 {
                out.push_str("?.?");
                rest = &rest[pid + 1 + tid..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The emulator a cross session runs the program under.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Remote {
    /// `qemu-<arch>`.
    pub emulator: PathBuf,
    /// `-cpu <baseline> -L <system root>`.
    pub args: Vec<OsString>,
    /// The target's system root, where gdb finds the dynamic loader and
    /// the C library the program runs with.
    pub sysroot: PathBuf,
}

/// A TCP port on the loopback interface that was free when asked: the
/// operating system's choice for a listener bound to port 0, which is
/// then closed for the emulator to take.
///
/// # Errors
/// When no listener can be bound.
pub fn free_port() -> Result<u16, String> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .map_err(|e| format!("cannot find a free port: {e}"))?;
    listener.local_addr().map(|a| a.port()).map_err(|e| format!("cannot find a free port: {e}"))
}

/// What the emulator says when another process took the port between
/// [`free_port`] and its own bind: the session is run again on another.
const PORT_TAKEN: [&str; 2] = ["could not open gdbserver", "Address already in use"];

/// How many ports a session tries.
const ATTEMPTS: usize = 3;

/// The commands of a test's script for a remote session: the program is
/// already started (stopped at its first instruction) when the debugger
/// attaches, so `run` continues it.
pub fn remote_commands(debugger: &Debugger, commands: &[String]) -> Vec<String> {
    let gdb = debugger.is_gdb();
    commands
        .iter()
        .map(|c| match c.trim() {
            "run" | "r" if gdb => "continue".to_owned(),
            "run" | "r" | "process launch" => "process continue".to_owned(),
            _ => c.clone(),
        })
        .collect()
}

/// A word quoted for `/bin/sh`.
fn quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', "'\\''"))
}

fn quote_os(word: &std::ffi::OsStr) -> String {
    quote(&word.to_string_lossy())
}

/// The session script: the emulator in the background, a wait (bounded
/// by the session's limit, not by time of its own) until its stub
/// listens on `port` or it died, the debugger in the foreground, then the
/// emulator killed and reaped. The script's status is the debugger's, or
/// 128 plus the signal that ended it.
pub fn script(
    remote: &Remote,
    debugger: &Debugger,
    program: &Path,
    commands: &[String],
    port: u16,
) -> String {
    let mut s = String::new();
    let mut emulator = vec![quote_os(remote.emulator.as_os_str())];
    emulator.extend(remote.args.iter().map(|a| quote_os(a)));
    emulator.push("-g".to_owned());
    emulator.push(port.to_string());
    emulator.push(quote_os(program.as_os_str()));
    let _ = writeln!(s, "{} </dev/null &", emulator.join(" "));
    let _ = writeln!(s, "q=$!");
    // The stub listens once a socket of the port is in state 0A (LISTEN).
    let _ = writeln!(
        s,
        "until grep -qiE '^ *[0-9]+: [0-9A-F]+:{port:04X} [0-9A-F]+:0000 0A' /proc/net/tcp \
         /proc/net/tcp6 2>/dev/null; do kill -0 $q 2>/dev/null || break; sleep 0.01; done"
    );
    let mut words = vec![quote_os(debugger.path.as_os_str())];
    let address = format!("127.0.0.1:{port}");
    if debugger.is_gdb() {
        words.extend(["-nx".to_owned(), "-batch".to_owned()]);
        let sysroot = format!("set sysroot {}", remote.sysroot.display());
        let target = format!("target remote {address}");
        let prelude = GDB_PRELUDE.iter().map(|c| (*c).to_owned());
        for command in prelude.chain([sysroot, target]).chain(commands.iter().cloned()) {
            words.push("-ex".to_owned());
            words.push(quote(&command));
        }
    } else {
        words.extend(["--no-lldbinit".to_owned(), "--batch".to_owned()]);
        let connect = format!("gdb-remote {address}");
        let prelude = transcript::LLDB_PRELUDE.iter().map(|c| (*c).to_owned());
        for command in prelude.chain([connect]).chain(commands.iter().cloned()) {
            words.push("-o".to_owned());
            words.push(quote(&command));
        }
    }
    words.push(quote_os(program.as_os_str()));
    let _ = writeln!(s, "{} </dev/null", words.join(" "));
    let _ = writeln!(s, "status=$?");
    let _ = writeln!(s, "kill -9 $q 2>/dev/null");
    let _ = writeln!(s, "wait $q 2>/dev/null");
    let _ = writeln!(s, "exit $status");
    s
}

/// Runs `commands` (a test's script, translated by [`remote_commands`])
/// under `debugger` on `program` running under `remote`'s emulator, in
/// the program's directory: [`transcript::session`] for another
/// architecture.
///
/// # Errors
/// What `process::run` reports (the shell or the limits).
pub fn session(
    remote: &Remote,
    debugger: &Debugger,
    program: &Path,
    commands: &[String],
    limits: Limits,
) -> Result<Ended, String> {
    let dir = program.parent().unwrap_or(Path::new("."));
    let commands = remote_commands(debugger, commands);
    let mut last = String::new();
    for _ in 0..ATTEMPTS {
        let port = free_port()?;
        let text = script(remote, debugger, program, &commands, port);
        let configure = |c: &mut std::process::Command| {
            c.arg("-c").arg(&text);
            c.current_dir(dir);
            c.env_remove("DEBUGINFOD_URLS").env("LC_ALL", "C").env_remove("TERM");
        };
        let output = process::run(Path::new("/bin/sh"), configure, b"", limits)?;
        let mut printed = String::from_utf8_lossy(&output.stdout).into_owned();
        printed.push_str(&String::from_utf8_lossy(&output.stderr));
        let printed = mask_remote(&printed, port);
        if PORT_TAKEN.iter().any(|m| printed.contains(m)) {
            last = printed;
            continue;
        }
        return Ok(match output.end {
            End::TimedOut => Ended::TimedOut(printed),
            End::Signal(_) => Ended::Crashed(printed),
            // The debugger died by a signal (the shell's 128 + n).
            End::Exit(code) if (129..=192).contains(&code) => Ended::Crashed(printed),
            End::Exit(_) if transcript::crash_report(&printed) => Ended::Crashed(printed),
            End::Exit(_) => Ended::Done(printed),
        });
    }
    Err(format!("the emulator found no free port in {ATTEMPTS} attempts: {last}"))
}

/// What the gdb stub of `qemu-user` does not offer, and so what no
/// cross session uses (`spec/harness.md` "debug", "Other targets").
pub const STUB_LIMITS: [&str; 4] = [
    "thread-local variables (no `qGetTLSAddr`: gdb says `Remote target doesn't support \
     qGetTLSAddr packet`, lldb `no TLS data currently exists for this thread`)",
    "hardware watchpoints (a `watch` is a software watchpoint, single-stepped)",
    "shared libraries under lldb (it reads no link map through the stub: `image list` shows the program alone)",
    "`run` again after the program exited (the stub ends with the program)",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn debugger(name: &str) -> Debugger {
        Debugger { name: name.to_owned(), path: PathBuf::from(format!("/bin/{name}")) }
    }

    #[test]
    fn run_continues_the_stopped_program() {
        let commands: Vec<String> =
            ["break main", "run", "print x", "r"].iter().map(|s| (*s).to_owned()).collect();
        assert_eq!(
            remote_commands(&debugger("gdb"), &commands),
            ["break main", "continue", "print x", "continue"]
        );
        assert_eq!(
            remote_commands(&debugger("lldb-19"), &commands),
            ["break main", "process continue", "print x", "process continue"]
        );
    }

    #[test]
    fn script_reaps_the_emulator() {
        let remote = Remote {
            emulator: PathBuf::from("/usr/bin/qemu-x86_64"),
            args: vec!["-cpu".into(), "qemu64".into(), "-L".into(), "/usr/x86_64-linux-gnu".into()],
            sysroot: PathBuf::from("/usr/x86_64-linux-gnu"),
        };
        let text =
            script(&remote, &debugger("gdb"), Path::new("/w/it's"), &["continue".into()], 4660);
        assert!(text.contains("'/usr/bin/qemu-x86_64' '-cpu' 'qemu64' '-L' '/usr/x86_64-linux-gnu' -g 4660 '/w/it'\\''s' </dev/null &"), "{text}");
        assert!(text.contains(":1234 [0-9A-F]+:0000 0A"), "{text}");
        assert!(text.contains("'target remote 127.0.0.1:4660'"), "{text}");
        assert!(text.contains("'set sysroot /usr/x86_64-linux-gnu'"), "{text}");
        assert!(text.ends_with("kill -9 $q 2>/dev/null\nwait $q 2>/dev/null\nexit $status\n"));
        let lldb = script(&remote, &debugger("lldb-19"), Path::new("/w/p"), &[], 80);
        assert!(lldb.contains("'gdb-remote 127.0.0.1:80'"), "{lldb}");
    }

    #[test]
    fn remote_transcripts_are_masked() {
        let text = "Remote debugging using 127.0.0.1:4660\n[Switching to Thread 51559.51703]\n\
                    Thread 2 hit Breakpoint 2\nThread 7.\n";
        assert_eq!(
            mask_remote(text, 4660),
            "Remote debugging using 127.0.0.1:PORT\n[Switching to Thread ?.?]\n\
             Thread 2 hit Breakpoint 2\nThread 7.\n"
        );
    }

    /// The directives of a target (`target:`, the scope words of a
    /// target and of `remote`, `skip`).
    #[test]
    fn target_directives() {
        use crate::debug::directives::parse;
        let source = "//@ gdb: run\n//@ target: x86_64\n\
                      //@ skip(remote, gdb): no TLS through the stub\n\
                      //@ xfail(aarch64, lldb): a reason\nint main() {}\n";
        let test = parse(Path::new("t.c"), source).unwrap();
        assert!(test.runs_on("x86_64") && !test.runs_on("aarch64"));
        assert!(test.skip_on(("x86_64", true), "clang-22", Some("bfd"), Some("gdb")).is_some());
        assert!(test.skip_on(("x86_64", false), "clang-22", Some("bfd"), Some("gdb")).is_none());
        assert!(test.skip_on(("x86_64", true), "clang-22", Some("bfd"), Some("lldb-19")).is_none());
        let lldb = Some("lldb-19");
        assert!(test.xfail_on(("aarch64", false), "clang-22", Some("bfd"), lldb).is_some());
        assert!(test.xfail_on(("x86_64", true), "clang-22", Some("bfd"), lldb).is_none());
        let bad = parse(Path::new("t.c"), "//@ gdb: run\n//@ target: mips\n").unwrap_err();
        assert!(bad.contains("unknown target 'mips'"), "{bad}");
        let bad = parse(Path::new("t.c"), "//@ gdb: run\n//@ skip(remote):\n").unwrap_err();
        assert!(bad.contains("a skip needs a reason"), "{bad}");
    }

    #[test]
    fn ports_are_the_systems_choice() {
        let port = free_port().unwrap();
        assert_ne!(port, 0);
    }
}
