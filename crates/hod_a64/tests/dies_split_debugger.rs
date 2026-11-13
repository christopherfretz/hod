//! Split DWARF under the debuggers (`spec/dwarf.md` §6.15, WP-062):
//! `tests/debug/split.cpp` (its own gdb and lldb scripts and checks) by
//! clang++ 19 (`llc` 19), clang++ 22 and kiln++ (`llc` 22) with
//! `-gsplit-dwarf`, compiled by hod and by `llc` into an object and a
//! `.dwo` beside it, linked with GNU ld, lld and mold. Each gdb and lldb
//! session on hod's program prints what the same session on `llc`'s
//! prints (addresses, process ids, code offsets and the directory
//! masked) and every check of the script. Then the same through a `.dwp`
//! package (`llvm-dwp`), with the `.dwo` moved away; and without either,
//! the session must lose the locals, so the DIEs really come from the
//! `.dwo`.

mod common;
#[path = "dies_support/mod.rs"]
mod support;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use common::linking::{Toolchain, exit_code};
use common::parse;
use hod_a64::compile_module;
use hod_cg::Options;
use hod_dwarf::SplitDwarf;
use hod_testutil::{Scratch, run_with, tool};
use support::kiln;

fn source() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/debug/split.cpp")
}

/// The `//@ <key>: <value>` lines of the source with `key`.
fn directives(text: &str, key: &str) -> Vec<String> {
    let prefix = format!("//@ {key}: ");
    text.lines().filter_map(|l| l.strip_prefix(&prefix)).map(str::to_owned).collect()
}

fn ok(command: &mut Command) -> String {
    let outcome = run_with(command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "{command:?}: {}{}", outcome.stdout_text(), outcome.stderr_text());
    outcome.stdout_text()
}

/// A session's output with what may differ between hod's program and
/// `llc`'s masked: addresses, process ids, code offsets and the
/// program's directory (as `dies_debugger.rs`).
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

fn session(debugger: &Path, gdb: bool, program: &Path, commands: &[String]) -> String {
    let mut command = Command::new(debugger);
    if gdb {
        command.args(["-nx", "-batch", "-ex", "set pagination off", "-ex", "set confirm off"]);
        command.args(["-ex", "set debuginfod enabled off", "-ex", "set style enabled off"]);
        for c in commands {
            command.arg("-ex").arg(c);
        }
    } else {
        command.args(["--no-lldbinit", "--batch", "-o", "settings set use-color false"]);
        command.args(["-o", "settings set target.disable-aslr false"]);
        for c in commands {
            command.arg("-o").arg(c);
        }
    }
    command.arg(program).current_dir(program.parent().unwrap());
    let out = run_with(&mut command, &[], Duration::from_mins(3));
    assert!(!out.timed_out(), "{} on {}", debugger.display(), program.display());
    out.stdout_text()
}

/// The source's IR by `compiler`, compiled in `dir` (the unit's
/// directory, where the `.dwo` goes), then the object and `.dwo` by hod
/// (`llc` = `None`) or by `llc`.
fn build(compiler: &Path, dir: &Path, llc: Option<&Path>) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::copy(source(), dir.join("split.cpp")).unwrap();
    let mut command = Command::new(compiler);
    command.args([
        "-g",
        "-O0",
        "-gsplit-dwarf",
        "--target=aarch64-linux-gnu",
        "-S",
        "-emit-llvm",
        "-w",
        "-std=c++20",
    ]);
    command.args(["split.cpp", "-o", "split.ll"]).current_dir(dir);
    ok(&mut command);
    let text = std::fs::read_to_string(dir.join("split.ll")).unwrap();
    assert!(text.contains("splitDebugFilename: \"split.dwo\""), "{}", compiler.display());
    let (object, dwo) = (dir.join("split.o"), dir.join("split.dwo"));
    if let Some(llc) = llc {
        let mut command = Command::new(llc);
        command.args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj"]);
        command.args(["-split-dwarf-file=split.dwo"]);
        command.arg(format!("-split-dwarf-output={}", dwo.display()));
        command.arg(dir.join("split.ll")).arg("-o").arg(&object);
        ok(&mut command);
    } else {
        let (module, bodies) = parse(&text);
        let options = Options { split_dwarf: SplitDwarf::Separate, ..Options::default() };
        let mut emitted = compile_module(&module, &bodies, &options).unwrap();
        let companion = emitted.object.take_dwo().unwrap();
        std::fs::write(&object, hod_obj::elf::to_vec(&emitted.object).unwrap()).unwrap();
        std::fs::write(&dwo, hod_obj::elf::to_vec(&companion).unwrap()).unwrap();
    }
    object
}

struct Debuggers {
    gdb: PathBuf,
    lldb: PathBuf,
    scripts: [(Vec<String>, Vec<String>); 2],
}

impl Debuggers {
    /// Both debuggers' sessions on hod's program and on `llc`'s, equal,
    /// with every check; `false`: the checks are expected to fail (no
    /// `.dwo`), and the sessions must still be equal.
    fn compare(&self, tag: &str, ours: &Path, theirs: &Path, checks: bool) -> usize {
        for (k, (commands, wants)) in self.scripts.iter().enumerate() {
            let (gdb, name) = if k == 0 { (true, "gdb") } else { (false, "lldb") };
            let debugger = if gdb { &self.gdb } else { &self.lldb };
            let h = masked(&session(debugger, gdb, ours, commands), ours.parent().unwrap());
            let l = masked(&session(debugger, gdb, theirs, commands), theirs.parent().unwrap());
            assert_eq!(h, l, "{tag}: {name}: hod's session, then llc's");
            let missing: Vec<&String> = wants.iter().filter(|w| !h.contains(w.as_str())).collect();
            if checks {
                assert!(missing.is_empty(), "{tag}: {name}: {missing:?} not in\n{h}");
            } else {
                assert!(!missing.is_empty(), "{tag}: {name}: the locals without a .dwo:\n{h}");
            }
        }
        2
    }
}

#[test]
fn split_sessions_match_llc() {
    if !cfg!(target_arch = "aarch64") {
        eprintln!("note: the programs are built for AArch64; skipping on this host");
        return;
    }
    let (Some(gdb), Some(lldb)) = (tool("gdb"), tool("lldb")) else { return };
    let text = std::fs::read_to_string(source()).unwrap();
    let debuggers = Debuggers {
        gdb,
        lldb,
        scripts: [
            (directives(&text, "gdb"), directives(&text, "gdb-check")),
            (directives(&text, "lldb"), directives(&text, "lldb-check")),
        ],
    };
    let scratch = Scratch::new("dies-split-debug");
    let Some(toolchain) = Toolchain::aarch64(&scratch) else { return };
    let dwp = tool("llvm-dwp-22").or_else(|| tool("llvm-dwp"));
    let mut frontends = vec![];
    for (name, major) in [("clang++-19", 19), ("clang++-22", 22)] {
        if let Some(c) = tool(name) {
            frontends.push((name.to_owned(), c, major));
        }
    }
    if let Some(k) = kiln() {
        frontends.push(("kiln".to_owned(), k, 22));
    }
    let (mut sessions, mut through_dwp) = (0, 0);
    for (name, compiler, major) in frontends {
        let Some(llc) = tool(&format!("llc-{major}")) else { continue };
        let (hd, ld) = (scratch.join(format!("{name}-hod")), scratch.join(format!("{name}-llc")));
        let ours = build(&compiler, &hd, None);
        let theirs = build(&compiler, &ld, Some(&llc));
        let mut programs = Vec::new();
        for linker in &toolchain.linkers {
            let tag = format!("{name}, {}", linker.name);
            let prog = |dir: &Path, object: &Path| {
                let out = dir.join(format!("prog-{}", linker.name));
                let ran = toolchain.link_and_run(linker, &[], &[object], &out);
                assert_eq!(exit_code(&ran), Some(0), "{tag}: {}", out.display());
                out
            };
            let (p_hod, p_llc) = (prog(&hd, &ours), prog(&ld, &theirs));
            sessions += debuggers.compare(&tag, &p_hod, &p_llc, true);
            programs.push((linker.name, p_hod, p_llc));
        }
        // The `.dwo` moved away: through a `.dwp` package, then nothing.
        let Some((linker, p_hod, p_llc)) = programs.first() else { continue };
        let moved = |dir: &Path, back: bool| {
            let (a, b) = (dir.join("split.dwo"), dir.join("split.dwo.away"));
            if back { std::fs::rename(b, a) } else { std::fs::rename(a, b) }.unwrap();
        };
        let mut packages = Vec::new();
        if let Some(dwp) = &dwp {
            for program in [p_hod, p_llc] {
                let package = PathBuf::from(format!("{}.dwp", program.display()));
                ok(Command::new(dwp).arg("-e").arg(program).arg("-o").arg(&package));
                packages.push(package);
            }
        }
        moved(&hd, false);
        moved(&ld, false);
        if !packages.is_empty() {
            let tag = format!("{name}, {linker}, .dwp");
            sessions += debuggers.compare(&tag, p_hod, p_llc, true);
            through_dwp += 1;
            for package in &packages {
                std::fs::remove_file(package).unwrap();
            }
        }
        let tag = format!("{name}, {linker}, no .dwo");
        sessions += debuggers.compare(&tag, p_hod, p_llc, false);
        moved(&hd, true);
        moved(&ld, true);
    }
    eprintln!("split DWARF: {sessions} sessions equal to llc's, {through_dwp} through a .dwp");
}
