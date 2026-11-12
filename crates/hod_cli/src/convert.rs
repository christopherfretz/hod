//! Bitcode to text with LLVM's own `llvm-dis`. Contract: `spec/cli.md`
//! "Bitcode conversion".
//!
//! hod has no bitcode reader yet. Until it has one, a bitcode module is
//! piped through the `llvm-dis` of the LLVM that wrote it and goes on as
//! text. Nothing touches the file system: the bitcode goes to the child's
//! standard input and the text comes back on its standard output, both at
//! once, so a module larger than a pipe's buffer cannot block either side.

use std::io::{ErrorKind, Read, Write};
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::thread;

use crate::driver::{ENV_IN_FALLBACK, SETTINGS};

/// The converter's arguments: text to standard output, bitcode from
/// standard input.
pub const ARGS: [&str; 3] = ["-o", "-", "-"];

/// What `llvm-dis` prints first for a module read from standard input
/// (`; ModuleID = '<stdin>'`).
const HEADER: &[u8] = b"; ModuleID = ";

/// How much of the converter's standard error is kept for a diagnostic.
/// The rest is read and dropped, so the converter never blocks on it.
const STDERR_KEPT: usize = 4096;

/// How much of the converter's first line of standard error a diagnostic
/// quotes, in characters.
const QUOTED: usize = 200;

/// An estimate of the text's size for the first allocation: a little over
/// the largest text-to-bitcode ratio measured (`spec/landed/cli/WP-020.md`).
/// A wrong guess costs a reallocation, nothing else.
const TEXT_PER_BITCODE_BYTE: usize = 8;

/// Why a conversion produced no text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The converter could not be started, or its pipes failed.
    Unavailable(String),
    /// It exited with a status other than 0; the first line it wrote to
    /// standard error.
    Failed { code: i32, said: String },
    /// A signal ended it (`None`: it ended without an exit status).
    Killed(Option<i32>),
    /// It exited with status 0 and wrote this (first line) to standard
    /// error: a warning hod cannot judge.
    Warned(String),
    /// It exited with status 0, silently, and its output is not one whole
    /// module of text.
    Output(&'static str),
}

impl Failure {
    /// The reason in the coverage log (`spec/cli.md` "Reasons").
    pub fn reason(&self) -> &'static str {
        match self {
            Failure::Unavailable(_) => "bitcode-converter-unavailable",
            Failure::Failed { .. } => "bitcode-converter-failed",
            Failure::Killed(_) => "bitcode-converter-killed",
            Failure::Warned(_) => "bitcode-converter-stderr",
            Failure::Output(_) => "bitcode-converter-output",
        }
    }

    /// The diagnostic, naming the converter.
    pub fn message(&self, llvm_dis: &Path) -> String {
        let path = llvm_dis.display();
        match self {
            Failure::Unavailable(cause) => {
                format!("cannot run bitcode converter '{path}': {cause}")
            }
            Failure::Failed { code, said } if said.is_empty() => {
                format!("bitcode converter '{path}' failed (exit status {code})")
            }
            Failure::Failed { code, said } => {
                format!("bitcode converter '{path}' failed (exit status {code}): {said}")
            }
            Failure::Killed(Some(signal)) => {
                format!("bitcode converter '{path}' was terminated by signal {signal}")
            }
            Failure::Killed(None) => {
                format!("bitcode converter '{path}' ended without an exit status")
            }
            Failure::Warned(said) => {
                format!("bitcode converter '{path}' wrote to standard error: {said}")
            }
            Failure::Output(what) => format!("bitcode converter '{path}' {what}"),
        }
    }
}

/// Reads `stream` to its end, keeping the first [`STDERR_KEPT`] bytes.
fn read_bounded(mut stream: impl Read) -> Vec<u8> {
    let mut kept = Vec::new();
    let mut chunk = [0_u8; 1024];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                let room = STDERR_KEPT - kept.len();
                kept.extend_from_slice(&chunk[..n.min(room)]);
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    kept
}

/// The first line of what the converter said that has anything on it, as
/// one line of text of at most [`QUOTED`] characters.
fn first_line(said: &[u8]) -> String {
    let text = String::from_utf8_lossy(said);
    let line = text.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default();
    let mut quoted: String =
        line.chars().take(QUOTED).map(|c| if c.is_control() { ' ' } else { c }).collect();
    if line.chars().nth(QUOTED).is_some() {
        quoted.push_str("...");
    }
    quoted
}

/// The signal that ended a process, where the host has signals.
fn signal(status: ExitStatus) -> Option<i32> {
    #[cfg(unix)]
    {
        std::os::unix::process::ExitStatusExt::signal(&status)
    }
    #[cfg(not(unix))]
    {
        let _ = status;
        None
    }
}

/// Why `text` is not what a successful `llvm-dis` writes, if it is not:
/// one whole module, which starts with its `; ModuleID` line and ends with
/// a newline. Whether the text between is a valid module is the text
/// reader's to say.
fn unusable(text: &[u8]) -> Option<&'static str> {
    if text.is_empty() {
        Some("wrote nothing")
    } else if !text.starts_with(HEADER) || !text.ends_with(b"\n") {
        Some("wrote something that is not one whole module of LLVM text")
    } else {
        None
    }
}

/// Converts one bitcode module to text with `llvm_dis`.
///
/// The converter runs with [`ARGS`], with hod's settings removed from its
/// environment and `HOD_IN_FALLBACK` set, so that a hod standing where the
/// converter should be stops at once. It is always waited for. hod sets no
/// time limit.
pub fn run(llvm_dis: &Path, bitcode: &[u8]) -> Result<Vec<u8>, Failure> {
    let unavailable = |e: std::io::Error| Failure::Unavailable(e.to_string());
    let mut command = Command::new(llvm_dis);
    command.args(ARGS).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    for variable in SETTINGS {
        command.env_remove(variable);
    }
    command.env(ENV_IN_FALLBACK, "1");
    let mut child = command.spawn().map_err(unavailable)?;
    let (Some(stdin), Some(mut stdout), Some(stderr)) =
        (child.stdin.take(), child.stdout.take(), child.stderr.take())
    else {
        // The three streams were requested as pipes, so this is not
        // expected; if it happens, the converter is still not left behind.
        let _ = child.kill();
        let _ = child.wait();
        return Err(Failure::Unavailable("its standard streams are not pipes".to_owned()));
    };

    let mut text = Vec::with_capacity(bitcode.len().saturating_mul(TEXT_PER_BITCODE_BYTE));
    let piped = thread::scope(|scope| {
        // The module goes in while the text comes out. A converter that
        // exits without reading everything closes the pipe; the write then
        // fails, and the converter's status and output say what happened.
        let feed = move || {
            let mut stdin = stdin;
            let _ = stdin.write_all(bitcode);
        };
        let helpers = thread::Builder::new()
            .spawn_scoped(scope, feed)
            .and_then(|_| thread::Builder::new().spawn_scoped(scope, move || read_bounded(stderr)));
        let said = match helpers {
            Ok(said) => said,
            Err(e) => {
                // Without the helpers the pipes cannot be served: end the
                // converter, so that whichever helper did start finishes.
                let _ = child.kill();
                return Err(e);
            }
        };
        let read = stdout.read_to_end(&mut text);
        if read.is_err() {
            let _ = child.kill();
        }
        let said = said.join().unwrap_or_default();
        read.map(|_| said)
    });
    let status = child.wait().map_err(unavailable)?;
    let said = piped.map_err(unavailable)?;
    match status.code() {
        Some(0) => {}
        Some(code) => return Err(Failure::Failed { code, said: first_line(&said) }),
        None => return Err(Failure::Killed(signal(status))),
    }
    if !said.is_empty() {
        return Err(Failure::Warned(first_line(&said)));
    }
    match unusable(&text) {
        Some(what) => Err(Failure::Output(what)),
        None => Ok(text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec/cli.md "Bitcode conversion": what counts as the converter's
    // output.
    #[test]
    fn usable_output() {
        assert_eq!(unusable(b""), Some("wrote nothing"));
        let not_a_module = Some("wrote something that is not one whole module of LLVM text");
        assert_eq!(unusable(b"; ModuleID = '<stdin>'\ndefine void @f() {\n  ret void\n}\n"), None);
        assert_eq!(unusable(b"; ModuleID = '<stdin>'\n"), None);
        // Bitcode, a module cut short, and text that is not a module.
        assert_eq!(unusable(b"BC\xC0\xDE\x35\x14\x00\x00"), not_a_module);
        assert_eq!(unusable(b"; ModuleID = '<stdin>'\ndefine void @f() {"), not_a_module);
        assert_eq!(unusable(b"hello\n"), not_a_module);
        assert_eq!(unusable(b"\n"), not_a_module);
    }

    // spec/cli.md "Bitcode conversion": a diagnostic quotes one bounded
    // line of what the converter said.
    #[test]
    fn quoted_line() {
        assert_eq!(first_line(b""), "");
        assert_eq!(first_line(b"\n  \nllvm-dis: error: bad\nmore\n"), "llvm-dis: error: bad");
        assert_eq!(first_line(b"a\tb\x1b[0m\r\n"), "a b [0m");
        assert_eq!(first_line(b"caf\xE9\n"), "caf\u{FFFD}");
        let long = first_line("é".repeat(QUOTED + 1).as_bytes());
        assert_eq!(long, format!("{}...", "é".repeat(QUOTED)));
        assert_eq!(first_line("é".repeat(QUOTED).as_bytes()), "é".repeat(QUOTED));
    }

    // spec/cli.md "Bitcode conversion": only the start of standard error is
    // kept, and all of it is read.
    #[test]
    fn standard_error_is_bounded() {
        let long = vec![b'x'; 3 * STDERR_KEPT + 17];
        let mut stream = long.as_slice();
        assert_eq!(read_bounded(&mut stream), vec![b'x'; STDERR_KEPT]);
        assert!(stream.is_empty(), "everything was read");
        assert_eq!(read_bounded(&b"short"[..]), b"short");
    }

    // spec/cli.md "Reasons": one name per way a conversion fails.
    #[test]
    fn reasons_and_messages() {
        let dis = Path::new("/l/llvm-dis");
        let cases = [
            (
                Failure::Unavailable("No such file or directory (os error 2)".into()),
                "bitcode-converter-unavailable",
                "cannot run bitcode converter '/l/llvm-dis': No such file or directory (os error 2)",
            ),
            (
                Failure::Failed { code: 1, said: "llvm-dis: error: bad".into() },
                "bitcode-converter-failed",
                "bitcode converter '/l/llvm-dis' failed (exit status 1): llvm-dis: error: bad",
            ),
            (
                Failure::Failed { code: 3, said: String::new() },
                "bitcode-converter-failed",
                "bitcode converter '/l/llvm-dis' failed (exit status 3)",
            ),
            (
                Failure::Killed(Some(9)),
                "bitcode-converter-killed",
                "bitcode converter '/l/llvm-dis' was terminated by signal 9",
            ),
            (
                Failure::Killed(None),
                "bitcode-converter-killed",
                "bitcode converter '/l/llvm-dis' ended without an exit status",
            ),
            (
                Failure::Warned("warning: ignoring invalid debug info".into()),
                "bitcode-converter-stderr",
                "bitcode converter '/l/llvm-dis' wrote to standard error: \
                 warning: ignoring invalid debug info",
            ),
            (
                Failure::Output("wrote nothing"),
                "bitcode-converter-output",
                "bitcode converter '/l/llvm-dis' wrote nothing",
            ),
        ];
        for (failure, reason, message) in cases {
            assert_eq!(failure.reason(), reason);
            assert_eq!(failure.message(dis), message);
        }
    }
}
