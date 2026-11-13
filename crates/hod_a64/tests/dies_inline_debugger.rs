//! Inlined calls under the debuggers (`spec/dwarf.md` §6.9, §8.8;
//! WP-085), as `dies_debugger.rs` drives function DIEs: the driver
//! `data/dies_inline/debug_main.cpp` (the host's compiler) calls
//! `debug.cpp`, compiled once by hod and once by `llc` (LLVM 19 IR by
//! `llc` 19, LLVM 22 IR by `llc` 22), linked with GNU ld, lld and mold,
//! driven by gdb and lldb. hod's sessions print what `llc`'s print
//! (addresses, process ids and code offsets masked) and the values the
//! source computes:
//!
//! - a `file:line` breakpoint in an inlined function stops at every
//!   instance (two, one through another inlined call);
//! - `bt` shows the inlined frames, `up` and `down` walk them, and
//!   `info args`, `info locals`, `frame variable` show an inlined
//!   frame's parameters and locals, a block's inside one included;
//! - `step` into an inlined call and `finish` out of one;
//! - a function's static.

mod common;
#[path = "dies_support/mod.rs"]
mod support;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use common::linking::{Linker, Toolchain, exit_code};
use hod_testutil::{Scratch, run_with, tool};
use support::{hod_object, ir, llc_object};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/dies_inline")
}

fn host_object(scratch: &Scratch, compiler: &Path, name: &str) -> PathBuf {
    let out = scratch.join(format!("{name}.o"));
    let mut command = Command::new(compiler);
    command.args(["-c", "-g", "-O0"]).arg(dir().join(name)).arg("-o").arg(&out);
    let outcome = run_with(&mut command, &[], Duration::from_mins(1));
    assert!(outcome.success(), "{name}: {}", outcome.stderr_text());
    out
}

/// A session's output with what may differ between hod's program and
/// `llc`'s masked: addresses, process ids, code offsets
/// (`func + 12`), and the program's directory.
fn masked(text: &str, dir: &Path) -> String {
    let dir = dir.display().to_string();
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let line = line.replace(&dir, "DIR");
        let b = line.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if b[i..].starts_with(b"0x") {
                out.push_str("0x?");
                i += 2;
                while i < b.len() && b[i].is_ascii_hexdigit() {
                    i += 1;
                }
                continue;
            }
            let after = |p: &[u8]| b[i..].starts_with(p);
            if after(b"rocess ") || after(b" + ") {
                let skip = if after(b" + ") { 3 } else { 7 };
                let digits = b[i + skip..].iter().take_while(|c| c.is_ascii_digit()).count();
                if digits > 0 {
                    out.push_str(&line[i..i + skip]);
                    out.push('?');
                    i += skip + digits;
                    continue;
                }
            }
            out.push(b[i] as char);
            i += 1;
        }
        out.push('\n');
    }
    out
}

fn gdb_session(gdb: &Path, program: &Path, commands: &[&str]) -> String {
    let mut command = Command::new(gdb);
    command.args(["-nx", "-batch", "-ex", "set pagination off", "-ex", "set confirm off"]).args([
        "-ex",
        "set debuginfod enabled off",
        "-ex",
        "set style enabled off",
    ]);
    for c in commands {
        command.arg("-ex").arg(c);
    }
    command.arg(program);
    let out = run_with(&mut command, &[], Duration::from_mins(3));
    assert!(!out.timed_out(), "gdb on {}", program.display());
    out.stdout_text()
}

fn lldb_session(lldb: &Path, program: &Path, commands: &[&str]) -> String {
    let mut command = Command::new(lldb);
    command
        .args(["--no-lldbinit", "--batch", "-o", "settings set use-color false"])
        .args(["-o", "settings set target.disable-aslr false"]);
    for c in commands {
        command.arg("-o").arg(c);
    }
    command.arg(program);
    let out = run_with(&mut command, &[], Duration::from_mins(3));
    assert!(!out.timed_out(), "lldb on {}", program.display());
    out.stdout_text()
}

/// One program's sessions: the gdb and lldb commands, and what hod's
/// gdb and lldb sessions must print.
struct Case {
    source: &'static str,
    driver: &'static str,
    gdb: &'static [&'static str],
    gdb_wants: &'static [&'static str],
    lldb: &'static [&'static str],
    lldb_wants: &'static [&'static str],
}

const INLINE: Case = Case {
    source: "debug.cpp",
    driver: "debug_main.cpp",
    gdb: &[
        "break debug.cpp:7",
        "tbreak debug.cpp:13",
        "tbreak debug.cpp:22",
        "run",
        "bt",
        "info args",
        "info locals",
        "up",
        "print calls",
        "down",
        "continue",
        "bt",
        "info locals",
        "up",
        "info args",
        "continue",
        "bt",
        "info locals",
        "continue",
        "info locals",
        "print calls",
        "continue",
    ],
    gdb_wants: &[
        "(2 locations)",
        "#0  sq (v=3) at",
        "#1  run_inline (z=3) at",
        "t = 9",
        "#1  nest (a=3) at",
        "#2  run_inline (z=3) at",
        "c = 20",
        "w = 39",
        "$1 = 1",
        "exited normally",
    ],
    lldb: &[
        "breakpoint set --file debug.cpp --line 7",
        "breakpoint set --file debug.cpp --line 13 --one-shot true",
        "run",
        "bt",
        "frame variable",
        "up",
        "frame variable calls",
        "continue",
        "bt",
        "frame variable",
        "up",
        "frame variable a",
        "continue",
        "frame variable",
        "continue",
    ],
    lldb_wants: &[
        "Breakpoint 1: 2 locations.",
        "[inlined] sq(v=3) at debug.cpp:7",
        "(int) v = 3\n(int) t = 9",
        "[inlined] nest(a=3) at debug.cpp:10",
        "(int) a = 3\n(int) b = 10\n(int) c = 20",
        "exited with status = 0",
    ],
};

/// A session that steps into an inlined call and finishes out of it.
/// A session that steps into an inlined call and finishes out of it.
const STEP: Case = Case {
    source: "debug.cpp",
    driver: "debug_main.cpp",
    gdb: &[
        "tbreak debug.cpp:20",
        "run",
        "step",
        "bt",
        "info args",
        "finish",
        "next",
        "step",
        "bt",
        "next",
        "next",
        "info locals",
        "continue",
    ],
    gdb_wants: &["sq (v=3) at", "21\t  w += nest(z);", "nest (a=3) at", "exited normally"],
    lldb: &[
        "breakpoint set --file debug.cpp --line 20 --one-shot true",
        "run",
        "thread step-in",
        "bt",
        "frame variable v",
        "thread step-out",
        "bt",
        "continue",
    ],
    lldb_wants: &["[inlined] sq(v=3) at debug.cpp:6", "exited with status = 0"],
};

fn link(
    toolchain: &Toolchain,
    linker: &Linker,
    scratch: &Scratch,
    dir: &str,
    objects: &[&Path],
) -> PathBuf {
    let dir = scratch.join(dir);
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("prog");
    let ran = toolchain.link_and_run(linker, &[], objects, &out);
    assert_eq!(exit_code(&ran), Some(0), "{} with {}", out.display(), linker.name);
    out
}

/// The IR of a case for LLVM `major`.
fn case_ir(scratch: &Scratch, case: &Case, major: u32) -> Option<String> {
    let stem = case.source.split('.').next().unwrap();
    let source = format!("../dies_inline/{}", case.source);
    Some(ir(&tool(&format!("clang++-{major}"))?, scratch, &format!("{stem}-{major}"), &source))
}

/// `llc` 19's session without the lines its blocks without
/// `DW_AT_abstract_origin` add (§6.7): gdb then lists the abstract
/// block's variable again, as `c = <optimized out>`.
fn origin_less(session: &str) -> String {
    let mut out = String::with_capacity(session.len());
    for line in session.lines().filter(|l| *l != "c = <optimized out>") {
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn run_case(case: &Case) {
    if !cfg!(target_arch = "aarch64") {
        eprintln!("note: the programs are built for AArch64; skipping on this host");
        return;
    }
    let (Some(gdb), Some(lldb)) = (tool("gdb"), tool("lldb")) else { return };
    let Some(cc) = tool("g++") else { return };
    let stem = case.source.split('.').next().unwrap();
    let scratch = Scratch::new(&format!("dies-debug-{stem}"));
    let Some(toolchain) = Toolchain::aarch64(&scratch) else { return };
    let driver = host_object(&scratch, &cc, case.driver);
    let mut sessions = 0;
    for major in [19, 22] {
        let Some(llc) = tool(&format!("llc-{major}")) else { continue };
        let Some(text) = case_ir(&scratch, case, major) else { continue };
        let hod = hod_object(&scratch, &format!("{stem}-{major}-hod"), &text);
        let llc = llc_object(&scratch, &llc, &format!("{stem}-{major}-llc"), &text);
        for linker in &toolchain.linkers {
            let tag = format!("{stem}, LLVM {major}, {}", linker.name);
            let d_hod = format!("hod-{major}-{}", linker.name);
            let d_llc = format!("llc-{major}-{}", linker.name);
            let p_hod = link(&toolchain, linker, &scratch, &d_hod, &[&driver, &hod]);
            let p_llc = link(&toolchain, linker, &scratch, &d_llc, &[&driver, &llc]);
            let sessions_of = |lldb_like: bool| {
                let (h, l) = if lldb_like {
                    (lldb_session(&lldb, &p_hod, case.lldb), lldb_session(&lldb, &p_llc, case.lldb))
                } else {
                    (gdb_session(&gdb, &p_hod, case.gdb), gdb_session(&gdb, &p_llc, case.gdb))
                };
                let l = masked(&l, &scratch.join(&d_llc));
                (masked(&h, &scratch.join(&d_hod)), if major < 21 { origin_less(&l) } else { l })
            };
            for (lldb_like, wants) in [(false, case.gdb_wants), (true, case.lldb_wants)] {
                let name = if lldb_like { "lldb" } else { "gdb" };
                let (h, l) = sessions_of(lldb_like);
                assert_eq!(h, l, "{tag}: {name}: hod's session, then llc's");
                for w in wants {
                    assert!(h.contains(w), "{tag}: {name}: {w:?} in\n{h}");
                }
                sessions += 2;
            }
        }
    }
    eprintln!("{stem}: {sessions} sessions, hod's equal to llc's");
}

#[test]
fn inline_sessions_match_llc() {
    run_case(&INLINE);
}

#[test]
fn step_sessions_match_llc() {
    run_case(&STEP);
}
