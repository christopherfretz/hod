//! Shared by the debugger tests (`spec/dwarf.md` §8.3, §8.6): the
//! linkers, and the masking of what differs between debugger sessions
//! from run to run.

use std::ffi::OsString;
use std::path::Path;

use hod_testutil::{Scratch, Target, linkers};

/// The host's linkers of the matrix, each as the `g++` flags that select
/// it ([`hod_testutil::linkers`]).
pub fn linkers(scratch: &Scratch) -> Vec<(&'static str, Vec<OsString>)> {
    let target = Target::host().unwrap_or(Target::Aarch64);
    linkers::present(target)
        .into_iter()
        .map(|(name, found)| (name, linkers::driver_flags(scratch.path(), name, &found)))
        .collect()
}

/// The session's output with what differs from run to run masked:
/// addresses, process numbers, and the lines a debugger prints about
/// the host (threads, libraries, warnings).
pub fn masked(text: &str, program: &Path) -> String {
    let program = program.display().to_string();
    let mut out = String::new();
    for line in text.lines() {
        let l = line.trim_end();
        if l.is_empty()
            || l.starts_with("warning:")
            || l.starts_with("[Thread debugging")
            || l.starts_with("Using host libthread_db")
            || l.starts_with("Download")
            || l.contains("libthread_db")
            || l.starts_with("Process ") && l.contains("launched")
            || l.starts_with("(lldb) target create")
            || l.starts_with("Current executable set to")
        {
            continue;
        }
        let l = l.replace(&program, "PROGRAM");
        out.push_str(&mask_numbers(&l));
        out.push('\n');
    }
    out
}

/// `0x...` becomes `ADDR`, and the number after `process ` or `Process `
/// becomes `PID`.
fn mask_numbers(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(b"0x") {
            i += 2;
            while i < bytes.len() && bytes[i].is_ascii_hexdigit() {
                i += 1;
            }
            out.push_str("ADDR");
            continue;
        }
        for word in ["process ", "Process "] {
            if line[i..].starts_with(word) {
                out.push_str(word);
                i += word.len();
                if i < bytes.len() && bytes[i].is_ascii_digit() {
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                    out.push_str("PID");
                }
            }
        }
        if i < bytes.len() {
            let c = line[i..].chars().next().unwrap();
            out.push(c);
            i += c.len_utf8();
        }
    }
    out
}
