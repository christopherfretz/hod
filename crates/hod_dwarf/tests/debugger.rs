//! `spec/dwarf.md` §8.3: the line table under a real debugger.
//!
//! `tests/data/lines/{main,second}.ll` (clang 19 `-O0 -g` output of the
//! sources beside them) are compiled by `llc`; each object's
//! `.debug_line`, `.rela.debug_line` and `.debug_line_str` are replaced by
//! the ones `hod_dwarf` writes from the line policy's rows, one sequence
//! per function (every other byte of the object unchanged). The program
//! is linked with GNU ld, lld and mold, and a gdb batch session (and an
//! lldb one, when lldb is installed) that breaks by `file:line` in two
//! files and steps must print what the same session prints on the
//! original program. A table with every line off by one must not.

mod common;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use common::{LlcTable, compile, decode, dwarfdumps, functions, llc, policy_rows, splice, write};
use hod_dwarf::Row;
use hod_obj::Arch;
use hod_testutil::{Scratch, find_tool, run, run_with, tool};

const MAIN: &str = include_str!("data/lines/main.ll");
const SECOND: &str = include_str!("data/lines/second.ll");
const TRIPLE: &str = "aarch64-unknown-linux-gnu";
/// The compilation directory the IR names.
const COMP_DIR: &str = "/hod/tests/lines";

/// The gdb session: breakpoints by line in the header (a COMDAT template
/// both objects define), in `main.cpp` and in `second.cpp`, then stops,
/// `next`, `step` and `finish`.
const GDB: &str = "\
break util.h:5
break main.cpp:16
break second.cpp:4
run
bt
info locals
next
next
continue
continue
continue
continue
step
next
finish
next
step
continue
next
step
step
next
next
continue
continue
continue
";

const LLDB: &str = "\
settings set target.disable-aslr false
breakpoint set --file util.h --line 5
breakpoint set --file main.cpp --line 16
breakpoint set --file second.cpp --line 4
run
bt
thread step-over
thread step-over
continue
continue
continue
continue
continue
thread step-in
thread step-over
thread step-out
thread step-over
thread step-in
continue
thread step-over
thread step-in
continue
continue
continue
";

/// The linkers, each as the `g++` flags that select it.
fn linkers(scratch: &Scratch) -> Vec<(&'static str, Vec<OsString>)> {
    let mut out = Vec::new();
    for (name, program) in [("bfd", "ld.bfd"), ("lld", "ld.lld"), ("mold", "ld.mold")] {
        let Some(found) = tool(program) else { continue };
        let dir = scratch.join(format!("use-{name}"));
        std::fs::create_dir_all(&dir).unwrap();
        let link = dir.join(format!("ld.{name}"));
        if !link.exists() {
            std::os::unix::fs::symlink(&found, &link).unwrap();
        }
        let mut prefix = OsString::from("-B");
        prefix.push(&dir);
        out.push((name, vec![prefix, format!("-fuse-ld={name}").into()]));
    }
    out
}

/// One object's line table as `hod_dwarf` writes it from the policy's
/// rows, with `adjust` applied to every row.
fn hod_object(t: &LlcTable, adjust: fn(&mut Row)) -> Vec<u8> {
    let per_function: Vec<(u32, i64, u32, Vec<Row>)> = functions(t)
        .iter()
        .map(|f| {
            let mut rows = policy_rows(f);
            rows.iter_mut().for_each(adjust);
            (f.symbol, f.start, f.size, rows)
        })
        .collect();
    assert!(!per_function.is_empty());
    splice(t, &write(t, Arch::AArch64, &per_function, false))
}

fn off_by_one(r: &mut Row) {
    if r.line != 0 {
        r.line += 1;
    }
}

/// The session's output with what differs from run to run masked:
/// addresses, process numbers, and the lines a debugger prints about
/// the host (threads, libraries, warnings).
fn masked(text: &str, program: &Path) -> String {
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

fn gdb_session(gdb: &Path, sources: &Path, program: &Path) -> String {
    let mut cmd = Command::new(gdb);
    cmd.args(["-nx", "-batch", "-ex", "set pagination off", "-ex", "set confirm off"])
        .args(["-ex", "set debuginfod enabled off", "-ex", "set style enabled off"])
        .arg("-ex")
        .arg(format!("set substitute-path {COMP_DIR} {}", sources.display()));
    for c in GDB.lines() {
        cmd.arg("-ex").arg(c);
    }
    cmd.arg(program);
    let out = run_with(&mut cmd, &[], Duration::from_mins(2));
    assert!(!out.timed_out(), "gdb on {}", program.display());
    masked(&out.stdout_text(), program)
}

fn lldb_session(lldb: &Path, sources: &Path, program: &Path) -> String {
    let mut cmd = Command::new(lldb);
    cmd.args(["--no-lldbinit", "--batch", "-o", "settings set use-color false"])
        .arg("-o")
        .arg(format!("settings set target.source-map {COMP_DIR} {}", sources.display()));
    for c in LLDB.lines() {
        cmd.arg("-o").arg(c);
    }
    cmd.arg(program);
    let out = run_with(&mut cmd, &[], Duration::from_mins(2));
    assert!(!out.timed_out(), "lldb on {}", program.display());
    masked(&out.stdout_text(), program)
}

/// How many stops a session reports at a source line, as a sanity check
/// that the session did what it says.
fn stops(session: &str) -> usize {
    session.lines().filter(|l| l.starts_with("Breakpoint ") && l.contains(" at ")).count()
        + session.lines().filter(|l| l.contains("stop reason = ")).count()
}

fn debugger(major: u32) {
    if !cfg!(target_arch = "aarch64") {
        eprintln!("note: the debugger program is built for AArch64; skipping on this host");
        return;
    }
    let Some(llc) = llc(major) else { return };
    let Some(driver) = tool("g++") else { return };
    let Some(gdb) = tool("gdb") else { return };
    let lldb = find_tool("lldb");
    if lldb.is_none() {
        eprintln!("note: `lldb` not found; the gdb sessions only");
    }
    let dumps = dwarfdumps();
    let Some((_, dwarfdump)) = dumps.iter().find(|(m, _)| *m == major) else { return };
    let scratch = Scratch::new(&format!("dwarf-debugger{major}"));
    let sources = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/lines");

    // Per variant, the two objects.
    let mut variants: Vec<(&str, Vec<PathBuf>)> =
        vec![("orig", Vec::new()), ("hod", Vec::new()), ("wrong", Vec::new())];
    for (unit, ir) in [("main", MAIN), ("second", SECOND)] {
        let label = format!("{unit}-{major}");
        let object = compile(&llc, &scratch, &label, ir, TRIPLE);
        let t = decode(dwarfdump, &scratch, &label, &object);
        let bytes = [object.clone(), hod_object(&t, |_| {}), hod_object(&t, off_by_one)];
        for ((name, objects), bytes) in variants.iter_mut().zip(bytes) {
            objects.push(scratch.write(&format!("{label}.{name}.o"), bytes));
        }
    }

    for (linker, flags) in linkers(&scratch) {
        let mut gdb_out = Vec::new();
        let mut lldb_out = Vec::new();
        for (name, objects) in &variants {
            // One name in a directory per variant: debuggers print it.
            let dir = scratch.join(name);
            std::fs::create_dir_all(&dir).unwrap();
            let program = dir.join(format!("lines-{major}-{linker}"));
            let mut args: Vec<OsString> = flags.clone();
            args.extend(objects.iter().map(Into::into));
            args.push("-o".into());
            args.push(program.clone().into());
            let linked = run(&driver, &args);
            assert!(linked.success(), "link {}: {}", program.display(), linked.stderr_text());
            // Every program behaves the same; it is only the table that
            // differs.
            let ran = run(&program, Vec::<OsString>::new());
            assert!(ran.success(), "{}: {}", program.display(), ran.stderr_text());
            gdb_out.push(gdb_session(&gdb, &sources, &program));
            if let Some(lldb) = &lldb {
                lldb_out.push(lldb_session(lldb, &sources, &program));
            }
        }
        let tag = format!("llc {major}, {linker}");
        for (debugger, out) in [("gdb", &gdb_out), ("lldb", &lldb_out)] {
            if out.is_empty() {
                continue;
            }
            if std::env::var_os("HOD_DWARF_SHOW_SESSION").is_some() {
                eprintln!("{tag}: {debugger}:\n{}", out[0]);
            }
            let (orig, hod, wrong) = (&out[0], &out[1], &out[2]);
            let n = stops(orig);
            assert!(n >= 10, "{tag}: {debugger} on the original stopped {n} times:\n{orig}");
            assert!(orig.contains("main.cpp:16"), "{tag}: {debugger}: {orig}");
            assert!(orig.contains("util.h:5"), "{tag}: {debugger}: {orig}");
            assert!(orig.contains("second.cpp:4"), "{tag}: {debugger}: {orig}");
            assert_eq!(hod, orig, "{tag}: {debugger}: hod's table against llc's");
            assert_ne!(wrong, orig, "{tag}: {debugger}: the wrong table is caught");
            eprintln!(
                "{tag}: {debugger}: {n} stops, {} lines of session identical; the off-by-one table differs",
                orig.lines().count()
            );
        }
    }
}

/// §8.3 with LLVM 19's objects.
#[test]
fn llc19_debugger_sessions_match() {
    debugger(19);
}

/// §8.3 with LLVM 22's objects.
#[test]
fn llc22_debugger_sessions_match() {
    debugger(22);
}
