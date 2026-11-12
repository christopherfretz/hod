//! Debugger sessions and their transcripts. Contract: `spec/harness.md`
//! "debug", "Sessions" and "Masking".

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::irdiff::process::{self, End, Limits};
use crate::irdiff::tools::Search;

/// gdb or one lldb.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Debugger {
    /// `gdb`, or `lldb-<N>`.
    pub name: String,
    pub path: PathBuf,
}

impl Debugger {
    pub fn is_gdb(&self) -> bool {
        self.name == "gdb"
    }
}

/// The LLVM majors whose lldb is looked for, newest first.
pub const LLDB_MAJORS: [u32; 2] = [22, 19];

/// The debuggers on this machine: `gdb` on `PATH`; for each of LLVM 22
/// and 19, `lldb-<N>` on `PATH` or `/usr/lib/llvm-<N>/bin/lldb`. A plain
/// `lldb` on `PATH` is used only when neither is found, under the name its
/// `--version` gives (`lldb-<N>`; `lldb` when it says no major).
pub fn find_debuggers(search: &Search) -> Vec<Debugger> {
    let mut found = Vec::new();
    if let Some(path) = search.on_path("gdb") {
        found.push(Debugger { name: "gdb".to_owned(), path });
    }
    for major in LLDB_MAJORS {
        if let Some(path) = search.versioned("lldb", major) {
            found.push(Debugger { name: format!("lldb-{major}"), path });
        }
    }
    if found.iter().all(Debugger::is_gdb)
        && let Some(path) = search.on_path("lldb")
    {
        let name = lldb_major(&path).map_or_else(|| "lldb".to_owned(), |m| format!("lldb-{m}"));
        found.push(Debugger { name, path });
    }
    found
}

/// The major of `lldb --version` ("lldb version 19.1.7").
fn lldb_major(lldb: &Path) -> Option<u32> {
    let probe = Limits { time: Duration::from_mins(1), memory_kib: 4 << 20 };
    let output = process::run(
        lldb,
        |c| {
            c.arg("--version");
        },
        b"",
        probe,
    )
    .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let rest = text.split("lldb version ").nth(1)?;
    rest.split('.').next()?.parse().ok()
}

/// The commands every gdb session starts with: no pager, no questions,
/// no symbol server, no styling, lines as long as they are, no notices of
/// threads starting and ending (they come when gdb notices, in no fixed
/// order with the program's own output and exit).
pub const GDB_PRELUDE: [&str; 6] = [
    "set pagination off",
    "set confirm off",
    "set width 0",
    "set debuginfod enabled off",
    "set style enabled off",
    "set print thread-events off",
];

/// The commands every lldb session starts with. A container cannot turn
/// off address randomization (`personality` is not allowed), so lldb is
/// told not to try.
pub const LLDB_PRELUDE: [&str; 3] = [
    "settings set use-color false",
    "settings set target.disable-aslr false",
    "settings set auto-confirm true",
];

/// The limits of one session: generous, because a debugger loads the
/// program's debug information and lldb its Python runtime, slower on a
/// loaded machine; no result depends on how long a session took.
pub const SESSION_LIMITS: Limits = Limits { time: Duration::from_mins(5), memory_kib: 8 << 20 };

/// How a session ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ended {
    /// The debugger exited; its output, standard output then standard
    /// error.
    Done(String),
    /// The debugger crashed (a signal, or its own crash report); what it
    /// printed.
    Crashed(String),
    /// The session ran into its time limit.
    TimedOut(String),
}

/// Runs `commands` under `debugger` on `program`, in the program's
/// directory, in batch mode.
pub fn session(
    debugger: &Debugger,
    program: &Path,
    commands: &[String],
    limits: Limits,
) -> Result<Ended, String> {
    let dir = program.parent().unwrap_or(Path::new("."));
    let configure = |c: &mut std::process::Command| {
        if debugger.is_gdb() {
            c.args(["-nx", "-batch"]);
            for command in GDB_PRELUDE.iter().copied().chain(commands.iter().map(String::as_str)) {
                c.arg("-ex").arg(command);
            }
        } else {
            c.args(["--no-lldbinit", "--batch"]);
            for command in LLDB_PRELUDE.iter().copied().chain(commands.iter().map(String::as_str)) {
                c.arg("-o").arg(command);
            }
        }
        c.arg(program).current_dir(dir);
        // No symbol-server lookups, and the same language everywhere.
        c.env_remove("DEBUGINFOD_URLS").env("LC_ALL", "C").env_remove("TERM");
    };
    let output = process::run(&debugger.path, configure, b"", limits)?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    Ok(match output.end {
        End::TimedOut => Ended::TimedOut(text),
        End::Signal(_) => Ended::Crashed(text),
        End::Exit(_) if crash_report(&text) => Ended::Crashed(text),
        End::Exit(_) => Ended::Done(text),
    })
}

/// Whether a debugger's output has its own crash report: LLVM's (lldb)
/// or gdb's internal error.
pub fn crash_report(text: &str) -> bool {
    ["PLEASE submit a bug report", "Stack dump:", "internal-error:", "A fatal error internal to"]
        .iter()
        .any(|marker| text.contains(marker))
}

/// A transcript with what legitimately differs between two builds of the
/// same program masked (`spec/harness.md` "debug", "Masking"):
///
/// - every occurrence of `dir` (the program's own directory) is `DIR`;
/// - `0x` and the hexadecimal digits after it are `0x?` (addresses,
///   pointer values, frame offsets in hexadecimal);
/// - the digits after `process `, `Process `, `LWP ` and `tid = ` are `?`
///   (process and thread ids);
/// - the digits of a code offset are `?`: after ` + ` that follows a
///   symbol after a backtick, before any ` at ` (lldb's `prog`main + 20`,
///   `prog`geo::Box::area() const + 12`), and between `+` and
///   `>` in a word that starts with `<` (gdb's `<main+20>`, `<+20>`).
///
/// Nothing else: a value, a line, a column, a name, a frame, the number
/// of stops and the order of everything must be equal.
pub fn mask(text: &str, dir: &Path) -> String {
    let dir = dir.display().to_string();
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let line = if dir.is_empty() { line.to_owned() } else { line.replace(&dir, "DIR") };
        let b = line.as_bytes();
        let mut i = 0;
        let mut kept = 0;
        while i < b.len() {
            let rest = &b[i..];
            if rest.starts_with(b"0x") {
                out.push_str(&line[kept..i]);
                out.push_str("0x?");
                i += 2;
                while i < b.len() && b[i].is_ascii_hexdigit() {
                    i += 1;
                }
                kept = i;
                continue;
            }
            // The word before the position: lldb's offsets follow
            // `module`symbol`, gdb's are `<symbol+N>`.
            let word = line[..i].rsplit(' ').next().unwrap_or("");
            let prefix = [&b"process "[..], b"Process ", b"LWP ", b"tid = "]
                .into_iter()
                .find(|p| rest.starts_with(p))
                .map(<[u8]>::len)
                .or_else(|| (rest.starts_with(b" + ") && lldb_symbol(&line[..i])).then_some(3))
                .or_else(|| (rest.first() == Some(&b'+') && word.starts_with('<')).then_some(1));
            if let Some(skip) = prefix {
                let digits = b[i + skip..].iter().take_while(|c| c.is_ascii_digit()).count();
                let gdb_open = skip == 1 && b.get(i + 1 + digits) != Some(&b'>');
                if digits > 0 && !gdb_open {
                    out.push_str(&line[kept..i + skip]);
                    out.push('?');
                    i += skip + digits;
                    kept = i;
                    continue;
                }
            }
            i += 1;
        }
        out.push_str(&line[kept..]);
        out.push('\n');
    }
    out
}

/// Whether `before` ends in lldb's module and symbol (`prog`, a backtick,
/// the symbol): a backtick, and no ` at ` (the source position, which
/// ends the symbol) after it.
fn lldb_symbol(before: &str) -> bool {
    before.rsplit_once('`').is_some_and(|(_, symbol)| !symbol.contains(" at "))
}

/// The first line where two masked transcripts differ: its number (from
/// 1) and the two lines (empty past the end).
pub fn first_difference(hod: &str, llc: &str) -> Option<(usize, String, String)> {
    let (mut a, mut b) = (hod.lines(), llc.lines());
    for number in 1.. {
        match (a.next(), b.next()) {
            (None, None) => return None,
            (x, y) if x == y => {}
            (x, y) => {
                let show = |l: Option<&str>| l.map_or_else(|| "<end>".to_owned(), str::to_owned);
                return Some((number, show(x), show(y)));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec/harness.md "debug", "Masking": each rule, and nothing else.
    #[test]
    fn masks_what_may_differ() {
        let dir = Path::new("/w/run/0001/hod-bfd");
        let text = "Breakpoint 1 at 0x7bc: file step.c, line 9.\n\
                    #1  0x0000aaaae7030828 in main () at step.c:16\n\
                    Process 39512 launched: '/w/run/0001/hod-bfd/prog' (aarch64)\n\
                    [Inferior 1 (process 4242) exited normally]\n\
                    frame #0: 0x0000aaaac8e007bc prog`fact(n=4) at step.c:9:7\n\
                    where = prog`fact + 20 at step.c:9:7, address = 0x00000000000007bc\n\
                    where = prog`geo::Box::area() const + 12 at c.cpp:2:1, n + 2\n\
                    [New Thread 0xffff (LWP 77)] tid = 78\n\
                    0x0000000000000794 <+20>:\tldr w0, [sp, #12]  <main+20> x+1 n + 2 3+4>\n\
                    $1 = 9\n";
        let wanted = "Breakpoint 1 at 0x?: file step.c, line 9.\n\
                      #1  0x? in main () at step.c:16\n\
                      Process ? launched: 'DIR/prog' (aarch64)\n\
                      [Inferior 1 (process ?) exited normally]\n\
                      frame #0: 0x? prog`fact(n=4) at step.c:9:7\n\
                      where = prog`fact + ? at step.c:9:7, address = 0x?\n\
                      where = prog`geo::Box::area() const + ? at c.cpp:2:1, n + 2\n\
                      [New Thread 0x? (LWP ?)] tid = ?\n\
                      0x? <+?>:\tldr w0, [sp, #12]  <main+?> x+1 n + 2 3+4>\n\
                      $1 = 9\n";
        assert_eq!(mask(text, dir), wanted);
        // Values, lines and columns are never masked.
        assert_eq!(mask("x = 12 at a.c:3:4\n", dir), "x = 12 at a.c:3:4\n");
    }

    #[test]
    fn first_differing_line() {
        assert_eq!(first_difference("a\nb\n", "a\nb\n"), None);
        assert_eq!(first_difference("a\nb\n", "a\nc\n"), Some((2, "b".into(), "c".into())));
        assert_eq!(first_difference("a\n", "a\nc\n"), Some((2, "<end>".into(), "c".into())));
    }

    #[test]
    fn crash_reports() {
        assert!(crash_report("PLEASE submit a bug report to https://...\n"));
        assert!(crash_report("../../gdb/dwarf2/read.c:123: internal-error: oops\n"));
        assert!(!crash_report("Breakpoint 1, main () at a.c:3\n"));
    }
}
