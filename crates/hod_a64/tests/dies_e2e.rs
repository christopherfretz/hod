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
//!
//! The tests that run: `data/dies_e2e/e2e.cpp` (the shapes WP-087 writes:
//! an unnamed catch parameter, an `alignas` local, a function-local
//! typedef the front end retains, an inlined function template whose
//! abstract instance has template parameters, a static member of an
//! unsigned enumeration, a function template) by clang 19 (`llc` 19),
//! clang 22 and kiln++ (`llc` 22), with its template parameters: hod's
//! DIE trees are `llc`'s and verify; the shapes are there; and gdb and
//! lldb sessions on hod's program print what they print on `llc`'s,
//! under GNU ld, lld and mold.

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
use support::{Node, first_difference, hod_object, kiln, llc_object, trees, verify};

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
    /// Trees that differ only by §6.7's code-generation classes, by the
    /// classes (WP-098).
    trees_permitted: BTreeMap<String, usize>,
    /// Trees that differ otherwise.
    trees_differ: usize,
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
    let out =
        run_with(Command::new(dwarfdump).arg("--verify").arg(object), &[], Duration::from_mins(2));
    out.success() && out.stdout_text().contains("No errors.")
}

fn llc_census(llc: &Path, input: &Path, out: &Path) -> bool {
    let mut command = Command::new(llc);
    command
        .args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj"])
        .arg(input)
        .arg("-o")
        .arg(out);
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

fn link(
    toolchain: &Toolchain,
    linker: &Linker,
    flags: &[String],
    object: &Path,
    out: &Path,
) -> bool {
    // The libraries after the object, as inputs.
    let mut inputs = vec![object];
    inputs.extend(flags.iter().map(Path::new));
    toolchain.link(linker, &["-no-pie"], &inputs, out).success()
}

fn run(program: &Path) -> (Option<i32>, String) {
    let out = run_with(&mut Command::new(program), &[], Duration::from_mins(1));
    (out.status.and_then(|s| s.code()), out.stdout_text())
}

#[allow(
    clippy::too_many_lines,
    clippy::many_single_char_names,
    reason = "one pass per module, step by step; hod's and llc's side by side"
)]
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
    if !llc_census(&llc, file, &llc_o) {
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
            let classes = (th.len() == 1 && tl.len() == 1)
                .then(|| {
                    let is_constant = text.contains("@llvm.is.constant");
                    support::permitted(&th[0], &tl[0], is_constant, major == 19)
                })
                .flatten();
            if let Some(classes) = classes {
                let classes = classes.join(", ");
                t.notes.push(format!("{name}: tree: permitted: {classes}"));
                *t.trees_permitted.entry(classes).or_default() += 1;
            } else {
                t.trees_differ += 1;
                let d: String = d.lines().take(40).collect::<Vec<_>>().join("\n");
                t.notes.push(format!("{name}: tree: {d}"));
            }
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
    let (lh, ll) = (
        lines::rows_by_function(&dwarfdump, &nm, &ph),
        lines::rows_by_function(&dwarfdump, &nm, &pl),
    );
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
        if !(link(toolchain, linker, &flags, &hod, &ph)
            && link(toolchain, linker, &flags, &llc_o, &pl))
        {
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
    let toolchain = if cfg!(target_arch = "aarch64") { Toolchain::aarch64(&scratch) } else { None };
    let mut t = Tally::default();
    for file in inputs(&var.to_string_lossy()) {
        census_one(&file, &scratch, toolchain.as_ref(), sessions, &mut t);
    }
    for note in &t.notes {
        eprintln!("note: {note}");
    }
    eprintln!(
        "modules {}, unread {}, refused by codegen {:?}, llc failed {}, built {}\n\
         verify clean: hod {} llc {}; trees equal {}, permitted {:?}, differ {}; \
         linked {}; runs alike {}\n\
         functions {}, lines equal {} (differences {:?}); sessions {}, equal {}",
        t.modules,
        t.unread,
        t.refused,
        t.llc_failed,
        t.built,
        t.verify_hod,
        t.verify_llc,
        t.trees_equal,
        t.trees_permitted,
        t.trees_differ,
        t.linked,
        t.ran_alike,
        t.functions,
        t.lines_equal,
        t.line_kinds,
        t.sessions,
        t.sessions_equal
    );
}

// ------------------------------------------------- `e2e.cpp` oracle ----

fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/dies_e2e")
}

/// `-g -O0` IR of `e2e.cpp` by `compiler`, with template parameters.
fn e2e_ir(compiler: &Path, scratch: &Scratch, name: &str) -> String {
    let out = scratch.join(format!("{name}.ll"));
    let mut command = Command::new(compiler);
    command.args([
        "-g",
        "-O0",
        "--target=aarch64-linux-gnu",
        "-S",
        "-emit-llvm",
        "-w",
        "-std=c++20",
    ]);
    command.arg(data_dir().join("e2e.cpp")).arg("-o").arg(&out);
    let outcome = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "{command:?}: {}", outcome.stderr_text());
    std::fs::read_to_string(out).unwrap()
}

/// The (compiler, `llc`) pairs that are present: clang 19's IR by `llc`
/// 19, clang 22's and kiln++'s by `llc` 22.
fn pairs() -> Vec<(&'static str, PathBuf, PathBuf)> {
    let mut out = Vec::new();
    for (name, compiler, llc) in [("c19", "clang++-19", "llc-19"), ("c22", "clang++-22", "llc-22")]
    {
        if let (Some(c), Some(l)) = (tool(compiler), tool(llc)) {
            out.push((name, c, l));
        }
    }
    if let (Some(k), Some(l)) = (kiln(), tool("llc-22")) {
        out.push(("kiln", k, l));
    }
    out
}

/// The DIEs of `tree` with `tag` whose attributes include `attr`
/// (any, when empty), with their parents' tags.
fn find<'a>(
    tree: &'a Node,
    tag: &str,
    attr: &str,
    parent: &'a str,
    out: &mut Vec<(&'a str, &'a Node)>,
) {
    if tree.tag == tag && (attr.is_empty() || tree.attrs.iter().any(|a| a.contains(attr))) {
        out.push((parent, tree));
    }
    for c in &tree.children {
        find(c, tag, attr, &tree.tag, out);
    }
}

#[test]
fn e2e_trees_match_llc() {
    let Some(dwarfdump) = tool("llvm-dwarfdump") else { return };
    let scratch = Scratch::new("dies-e2e-trees");
    let mut compared = 0;
    for (name, compiler, llc) in pairs() {
        let text = e2e_ir(&compiler, &scratch, name);
        let hod = hod_object(&scratch, &format!("{name}-hod"), &text);
        let llc = llc_object(&scratch, &llc, &format!("{name}-llc"), &text);
        verify(&dwarfdump, &hod);
        verify(&dwarfdump, &llc);
        let (th, tl) = (trees(&dwarfdump, &hod), trees(&dwarfdump, &llc));
        assert_eq!(th.len(), 1, "{name}");
        if let Some(d) = first_difference(&th[0], &tl[0]) {
            panic!("{name}: hod's tree, then llc's: {d}");
        }
        let unit = &th[0];
        // What WP-087 writes, so that a lost shape fails.
        let mut v = Vec::new();
        find(unit, "DW_TAG_typedef", "(\"narrow\")", "", &mut v);
        assert!(v.iter().any(|(p, _)| *p == "DW_TAG_subprogram"), "{name}: the local typedef");
        v.clear();
        find(unit, "DW_TAG_variable", "", "", &mut v);
        let unnamed =
            v.iter().filter(|(_, n)| !n.attrs.iter().any(|a| a.starts_with("DW_AT_name"))).count();
        assert!(unnamed >= 1, "{name}: the unnamed catch parameter");
        if name != "kiln" {
            // kiln++ gives the array no `align:`.
            v.clear();
            find(unit, "DW_TAG_variable", "DW_AT_alignment (32)", "", &mut v);
            assert_eq!(v.len(), 1, "{name}: the aligned local");
        }
        v.clear();
        find(unit, "DW_TAG_subprogram", "DW_AT_inline", "", &mut v);
        assert!(
            v.iter().any(|(_, n)| n
                .children
                .first()
                .is_some_and(|c| c.tag == "DW_TAG_template_type_parameter")),
            "{name}: the abstract instance's template parameter, first"
        );
        compared += 1;
    }
    eprintln!("e2e.cpp: {compared} trees equal to llc's");
}

const E2E_GDB: &[&str] = &[
    "break e2e.cpp:19",
    "break clamp_to<int>",
    "break e2e.cpp:33",
    "break e2e.cpp:40",
    "break e2e.cpp:46",
    "run",
    "bt",
    "info locals",
    "print Config::level",
    "continue",
    "info args",
    "bt",
    "continue",
    "bt",
    "continue",
    "info locals",
    "continue",
    "info locals",
    "ptype narrow",
    "continue",
];

const E2E_GDB_WANTS: &[&str] = &[
    "twice<int> (x=3) at e2e.cpp:19",
    "run (n=3) at e2e.cpp:50",
    "doubled = 6",
    "= Level::high",
    "clamp_to<int> (v=6, hi=50)",
    "caught (n=3) at e2e.cpp:33",
    "buf = {0, 0, 0, 3, 0, 0, 0, 0}",
    "s = 4467",
    "type = short",
    "exited normally",
];

const E2E_LLDB: &[&str] = &[
    "breakpoint set --file e2e.cpp --line 19",
    "breakpoint set --name clamp_to<int>",
    "breakpoint set --file e2e.cpp --line 40",
    "breakpoint set --file e2e.cpp --line 46",
    "run",
    "bt",
    "frame variable",
    "expr Config::level",
    "continue",
    // Not `result`: not yet assigned at the function's breakpoint.
    "frame variable v hi",
    "continue",
    "frame variable",
    "continue",
    "frame variable",
    "continue",
];

const E2E_LLDB_WANTS: &[&str] = &[
    "[inlined] int twice<int>(int)",
    "(int) doubled = 6",
    "(Level) $0 = high",
    "(int) v = 6\n(int) hi = 50",
    "(int[8]) buf = ([0] = 0, [1] = 0, [2] = 0, [3] = 3,",
    "(narrow) s = 4467",
    "exited with status = 0",
];

fn e2e_session(debugger: &Path, lldb: bool, program: &Path) -> String {
    let mut command = Command::new(debugger);
    if lldb {
        command
            .args(["--no-lldbinit", "--batch", "-o", "settings set use-color false"])
            .args(["-o", "settings set target.disable-aslr false"]);
        for c in E2E_LLDB {
            command.arg("-o").arg(c);
        }
    } else {
        command.args(["-nx", "-batch", "-ex", "set pagination off", "-ex", "set confirm off"]);
        command.args(["-ex", "set debuginfod enabled off", "-ex", "set style enabled off"]);
        for c in E2E_GDB {
            command.arg("-ex").arg(c);
        }
    }
    command.arg(program);
    let out = run_with(&mut command, &[], Duration::from_mins(3));
    assert!(!out.timed_out(), "{} on {}", debugger.display(), program.display());
    // The source's directory, which gdb prints when it is absolute.
    out.stdout_text().replace(&format!("{}/", data_dir().display()), "")
}

#[test]
fn e2e_sessions_match_llc() {
    if !cfg!(target_arch = "aarch64") {
        eprintln!("note: the programs are built for AArch64; skipping on this host");
        return;
    }
    let (Some(gdb), Some(lldb)) = (tool("gdb"), tool("lldb")) else { return };
    let scratch = Scratch::new("dies-e2e-sessions");
    let Some(toolchain) = Toolchain::aarch64(&scratch) else { return };
    let mut sessions = 0;
    for (name, compiler, llc) in pairs() {
        let text = e2e_ir(&compiler, &scratch, name);
        let hod = hod_object(&scratch, &format!("{name}-hod"), &text);
        let llc = llc_object(&scratch, &llc, &format!("{name}-llc"), &text);
        for linker in &toolchain.linkers {
            let tag = format!("{name}, {}", linker.name);
            let mut programs = Vec::new();
            for (who, object) in [("hod", &hod), ("llc", &llc)] {
                let dir = scratch.join(format!("{who}-{name}-{}", linker.name));
                std::fs::create_dir_all(&dir).unwrap();
                let program = dir.join("prog");
                let inputs = [object.as_path(), Path::new("-lstdc++")];
                let ran = toolchain.link_and_run(linker, &["-no-pie"], &inputs, &program);
                assert_eq!(common::linking::exit_code(&ran), Some(0), "{tag}: {who}'s program");
                programs.push((dir, program));
            }
            for (debugger, is_lldb, wants) in
                [(&gdb, false, E2E_GDB_WANTS), (&lldb, true, E2E_LLDB_WANTS)]
            {
                let which = if is_lldb { "lldb" } else { "gdb" };
                let h = masked(&e2e_session(debugger, is_lldb, &programs[0].1), &programs[0].0);
                let l = masked(&e2e_session(debugger, is_lldb, &programs[1].1), &programs[1].0);
                assert_eq!(h, l, "{tag}: {which}: hod's session, then llc's");
                for w in wants {
                    assert!(h.contains(w), "{tag}: {which}: {w:?} in\n{h}");
                }
                sessions += 2;
            }
        }
    }
    eprintln!("e2e.cpp: {sessions} sessions, hod's equal to llc's");
}
