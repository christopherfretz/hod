//! Whole `-g` translation units (`spec/dwarf.md` §6.12, WP-087).
//!
//! `debug_census` (ignored; an aid that asserts nothing) takes the `.ll`
//! files of `HOD_DEBUG_CENSUS` (files or directories, `:`-separated) and,
//! for each module hod's code generator compiles, compares hod's object
//! with `llc`'s (LLVM 19 IR by `llc` 19, LLVM 22 IR by `llc` 22):
//! `llvm-dwarfdump --verify` on both, the DIE trees (§6.7's masks), the
//! line table of every function (§5.7), and the program linked with GNU
//! ld (its exit status and output). With `HOD_DEBUG_CENSUS_SESSIONS=1`
//! it also runs a gdb and an lldb session from `main` (breakpoint,
//! backtrace, locals, steps into callees, `finish`) on both programs
//! under GNU ld, lld and mold, compared with what may differ masked. A
//! program that includes the C++ library's headers is linked with
//! libstdc++, or with the libc++ of `HOD_LIBCXX_LIB` when its symbols
//! are libc++'s.
//!
//! `cargo test -p hod_a64 --test dies_e2e -- --ignored --nocapture`

mod common;
#[path = "lines_support/mod.rs"]
mod lines;
#[path = "dies_support/mod.rs"]
mod support;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use common::linking::{Linker, Toolchain};
use hod_cg::Options;
use hod_ir::{Body, Module};
use hod_testutil::{Scratch, run_with, tool};
use support::{first_difference, trees};

/// A module and its bodies; `None` when `hod_ll` cannot read it.
fn parse_lenient(text: &str) -> Option<(Module, Vec<Body>)> {
    let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default()).ok()?;
    let text_side = parsed.text;
    let mut module = parsed.module;
    let mut bodies = Vec::new();
    hod_ll::parse_bodies(&mut module, &text_side, |_, parsed| {
        if let Some(body) = parsed.body {
            bodies.push(body.clone());
        }
        Ok::<(), hod_ll::LlError>(())
    })
    .ok()?;
    Some((module, bodies))
}

fn inputs(var: &str) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for part in var.split(':').filter(|p| !p.is_empty()) {
        let path = PathBuf::from(part);
        if path.is_dir() {
            let mut entries: Vec<PathBuf> = std::fs::read_dir(&path)
                .unwrap()
                .map(|e| e.unwrap().path())
                .filter(|p| p.extension().is_some_and(|e| e == "ll"))
                .collect();
            entries.sort();
            files.extend(entries);
        } else {
            files.push(path);
        }
    }
    files
}

/// A session's output with what may differ between hod's program and
/// `llc`'s masked: addresses, process ids, code offsets, the program's
/// directory.
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

const GDB: &[&str] = &[
    "break main",
    "run",
    "bt",
    "info locals",
    "step",
    "step",
    "step",
    "bt",
    "info args",
    "info locals",
    "finish",
    "next",
    "step",
    "bt",
    "info locals",
    "continue",
];

const LLDB: &[&str] = &[
    "breakpoint set --name main",
    "run",
    "bt",
    "frame variable",
    "thread step-in",
    "thread step-in",
    "thread step-in",
    "bt",
    "frame variable",
    "thread step-out",
    "thread step-over",
    "thread step-in",
    "bt",
    "frame variable",
    "continue",
];

fn session(debugger: &Path, lldb: bool, program: &Path) -> String {
    let mut command = Command::new(debugger);
    if lldb {
        command
            .args(["--no-lldbinit", "--batch", "-o", "settings set use-color false"])
            .args(["-o", "settings set target.disable-aslr false"]);
        for c in LLDB {
            command.arg("-o").arg(c);
        }
    } else {
        command.args(["-nx", "-batch", "-ex", "set pagination off", "-ex", "set confirm off"]);
        command.args(["-ex", "set debuginfod enabled off", "-ex", "set style enabled off"]);
        for c in GDB {
            command.arg("-ex").arg(c);
        }
    }
    command.arg(program);
    let out = run_with(&mut command, &[], Duration::from_mins(3));
    out.stdout_text()
}

/// What happened to one module.
#[derive(Default)]
struct Tally {
    modules: usize,
    unread: usize,
    /// hod's code generator's refusal, by reason.
    refused: BTreeMap<String, usize>,
    llc_failed: usize,
    built: usize,
    verify_hod: usize,
    verify_llc: usize,
    trees_equal: usize,
    functions: usize,
    lines_equal: usize,
    /// Line-table differences by kind.
    line_kinds: BTreeMap<String, usize>,
    linked: usize,
    ran_alike: usize,
    sessions: usize,
    sessions_equal: usize,
    /// The first difference of each kind, per module.
    notes: Vec<String>,
}

fn verify_ok(dwarfdump: &Path, object: &Path) -> bool {
    let out = run_with(Command::new(dwarfdump).arg("--verify").arg(object), &[], Duration::from_mins(2));
    out.success() && out.stdout_text().contains("No errors.")
}

fn llc_object(llc: &Path, input: &Path, out: &Path) -> bool {
    let mut command = Command::new(llc);
    command.args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj"]).arg(input).arg("-o").arg(out);
    run_with(&mut command, &[], Duration::from_mins(5)).success()
}

fn link_flags(text: &str) -> Vec<String> {
    let mut flags = Vec::new();
    if text.contains("_ZNSt3__1") {
        if let Some(dir) = std::env::var_os("HOD_LIBCXX_LIB") {
            let dir = dir.to_string_lossy().into_owned();
            flags.extend([format!("-L{dir}"), format!("-Wl,-rpath,{dir}")]);
        }
        flags.extend(["-lc++", "-lc++abi"].map(String::from));
    } else {
        flags.push("-lstdc++".into());
    }
    flags.extend(["-lm", "-lpthread"].map(String::from));
    flags
}

fn link(toolchain: &Toolchain, linker: &Linker, flags: &[String], object: &Path, out: &Path) -> bool {
    // The libraries after the object, as inputs.
    let mut inputs = vec![object];
    inputs.extend(flags.iter().map(Path::new));
    toolchain.link(linker, &["-no-pie"], &inputs, out).success()
}

fn run(program: &Path) -> (Option<i32>, String) {
    let out = run_with(&mut Command::new(program), &[], Duration::from_mins(1));
    (out.status.and_then(|s| s.code()), out.stdout_text())
}

#[allow(clippy::too_many_lines, reason = "one pass per module, step by step")]
fn census_one(
    file: &Path,
    scratch: &Scratch,
    toolchain: Option<&Toolchain>,
    sessions: bool,
    t: &mut Tally,
) {
    let (Some(dwarfdump), Some(nm)) = (tool("llvm-dwarfdump"), tool("llvm-nm")) else { return };
    t.modules += 1;
    let name = file.file_stem().unwrap().to_string_lossy().into_owned();
    let text = std::fs::read_to_string(file).unwrap();
    let Some((module, bodies)) = parse_lenient(&text) else {
        t.unread += 1;
        return;
    };
    let major = if text.contains("clang version 19") { 19 } else { 22 };
    let hod = match hod_a64::compile_to_object(&module, &bodies, &Options::default()) {
        Ok(bytes) => scratch.write(&format!("{name}.hod.o"), bytes),
        Err(e) => {
            let reason = e.to_string();
            let key = reason.split('\'').nth(1).unwrap_or(&reason).to_owned();
            *t.refused.entry(key).or_default() += 1;
            return;
        }
    };
    let Some(llc) = tool(&format!("llc-{major}")) else { return };
    let llc_o = scratch.join(format!("{name}.llc.o"));
    if !llc_object(&llc, file, &llc_o) {
        t.llc_failed += 1;
        return;
    }
    t.built += 1;
    t.verify_hod += usize::from(verify_ok(&dwarfdump, &hod));
    t.verify_llc += usize::from(verify_ok(&dwarfdump, &llc_o));
    let (th, tl) = (trees(&dwarfdump, &hod), trees(&dwarfdump, &llc_o));
    let diff = if th.len() == tl.len() {
        th.iter().zip(&tl).find_map(|(a, b)| first_difference(a, b))
    } else {
        Some(format!("{} units, llc's {}", th.len(), tl.len()))
    };
    match diff {
        None => t.trees_equal += 1,
        Some(d) => {
            let d: String = d.lines().take(40).collect::<Vec<_>>().join("\n");
            t.notes.push(format!("{name}: tree: {d}"));
        }
    }
    let Some(toolchain) = toolchain else { return };
    let flags = link_flags(&text);
    let bfd = &toolchain.linkers[0];
    let (ph, pl) = (scratch.join(format!("{name}.hod")), scratch.join(format!("{name}.llc")));
    if !(link(toolchain, bfd, &flags, &hod, &ph) && link(toolchain, bfd, &flags, &llc_o, &pl)) {
        t.notes.push(format!("{name}: does not link"));
        return;
    }
    t.linked += 1;
    // Lines, per function of the linked programs (one code section).
    let (lh, ll) =
        (lines::rows_by_function(&dwarfdump, &nm, &ph), lines::rows_by_function(&dwarfdump, &nm, &pl));
    let branches = lines::branch_locations(&text);
    let mut first_line_note = None;
    for (f, rows) in &ll {
        t.functions += 1;
        match lh.get(f).map(|h| lines::compare(h, rows, &branches)) {
            Some(None) => t.lines_equal += 1,
            Some(Some(d)) => {
                let kind: String = d.split([':', '(']).next().unwrap_or("").into();
                *t.line_kinds.entry(kind).or_default() += 1;
                first_line_note.get_or_insert_with(|| format!("{name}: lines of {f}: {d}"));
            }
            None => {
                *t.line_kinds.entry("no rows".into()).or_default() += 1;
                first_line_note.get_or_insert_with(|| format!("{name}: no rows for {f}"));
            }
        }
    }
    t.notes.extend(first_line_note);
    let (rh, rl) = (run(&ph), run(&pl));
    if rh == rl {
        t.ran_alike += 1;
    } else {
        t.notes.push(format!("{name}: runs differ: {:?} / {:?}", rh.0, rl.0));
    }
    if !sessions {
        return;
    }
    let (Some(gdb), Some(lldb)) = (tool("gdb"), tool("lldb")) else { return };
    for linker in &toolchain.linkers {
        let dir_h = scratch.join(format!("{name}-{}-hod", linker.name));
        let dir_l = scratch.join(format!("{name}-{}-llc", linker.name));
        std::fs::create_dir_all(&dir_h).unwrap();
        std::fs::create_dir_all(&dir_l).unwrap();
        let (ph, pl) = (dir_h.join("prog"), dir_l.join("prog"));
        if !(link(toolchain, linker, &flags, &hod, &ph) && link(toolchain, linker, &flags, &llc_o, &pl)) {
            t.notes.push(format!("{name}: does not link with {}", linker.name));
            continue;
        }
        for (debugger, is_lldb) in [(&gdb, false), (&lldb, true)] {
            t.sessions += 1;
            let h = masked(&session(debugger, is_lldb, &ph), &dir_h);
            let l = masked(&session(debugger, is_lldb, &pl), &dir_l);
            if h == l {
                t.sessions_equal += 1;
            } else {
                let at = h.lines().zip(l.lines()).position(|(a, b)| a != b).unwrap_or(0);
                let which = if is_lldb { "lldb" } else { "gdb" };
                let (a, b) = (h.lines().nth(at).unwrap_or(""), l.lines().nth(at).unwrap_or(""));
                t.notes.push(format!("{name}: {which}/{}: line {at}: {a:?} / {b:?}", linker.name));
            }
        }
    }
}

#[test]
#[ignore = "an aid: HOD_DEBUG_CENSUS=<.ll files or directories>"]
fn debug_census() {
    let Some(var) = std::env::var_os("HOD_DEBUG_CENSUS") else {
        eprintln!("set HOD_DEBUG_CENSUS");
        return;
    };
    let sessions = std::env::var_os("HOD_DEBUG_CENSUS_SESSIONS").is_some_and(|v| v == "1");
    let scratch = Scratch::new("dies-e2e-census");
    let toolchain = if cfg!(target_arch = "aarch64") { Toolchain::host(&scratch) } else { None };
    let mut t = Tally::default();
    for file in inputs(&var.to_string_lossy()) {
        census_one(&file, &scratch, toolchain.as_ref(), sessions, &mut t);
    }
    for note in &t.notes {
        eprintln!("note: {note}");
    }
    eprintln!(
        "modules {}, unread {}, refused by codegen {:?}, llc failed {}, built {}\n\
         verify clean: hod {} llc {}; trees equal {}; linked {}; runs alike {}\n\
         functions {}, lines equal {} (differences {:?}); sessions {}, equal {}",
        t.modules,
        t.unread,
        t.refused,
        t.llc_failed,
        t.built,
        t.verify_hod,
        t.verify_llc,
        t.trees_equal,
        t.linked,
        t.ran_alike,
        t.functions,
        t.lines_equal,
        t.line_kinds,
        t.sessions,
        t.sessions_equal
    );
}
