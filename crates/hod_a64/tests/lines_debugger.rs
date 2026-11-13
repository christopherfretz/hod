//! Line tables under the debuggers (`spec/dwarf.md` §5.7, §8.3): programs
//! whose C or C++ driver (compiled by the host's compiler) calls code hod
//! compiled from `data/lines`, linked with GNU ld, lld and mold, then
//! driven by gdb and lldb. Each session is run on the program built from
//! hod's objects and on the one built from `llc`'s, and the source lines
//! the debugger stops at are compared, modulo `llc`'s stops at the
//! branches hod falls through (§5.7, P1). A table one line off is caught.
//!
//! - `prog`: `main.c` calls `prog.c`'s functions; stepping from `main`
//!   through all of them (`step`), and `break file:line`, `bt`,
//!   `info line` and `next` in one of them.
//! - `comdat`: `tu1.cpp` and `tu2.cpp` both define the inline function
//!   `shared` (a COMDAT group); the linker keeps one. The linked program
//!   verifies (`llvm-dwarfdump --verify --debug-line`), and stepping
//!   reaches `shared` through both translation units.

mod common;
#[path = "lines_support/mod.rs"]
mod support;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use common::linking::{Linker, Toolchain, exit_code};
use common::parse;
use hod_cg::Options;
use hod_testutil::{Scratch, run_with, tool};
use support::{COMP_DIR, branch_locations, data, data_dir, with_header};

const STEPS: usize = 160;

/// Compiles the host-compiled driver `name` with `-g -O0`.
fn host_object(scratch: &Scratch, compiler: &Path, name: &str) -> PathBuf {
    let out = scratch.join(format!("{name}.o"));
    let mut command = Command::new(compiler);
    command
        .args(["-c", "-g", "-O0"])
        .arg(format!("-fdebug-prefix-map={}={COMP_DIR}", data_dir().display()))
        .arg(data_dir().join(name))
        .arg("-o")
        .arg(&out);
    let outcome = run_with(&mut command, &[], Duration::from_mins(1));
    assert!(outcome.success(), "{name}: {}", outcome.stderr_text());
    out
}

/// hod's object of `text`.
fn hod_object(scratch: &Scratch, name: &str, text: &str) -> PathBuf {
    let (module, bodies) = parse(text);
    let bytes = hod_a64::compile_to_object(&module, &bodies, &Options::default())
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    scratch.write(&format!("{name}.o"), bytes)
}

/// `llc`'s object of `text`.
fn llc_object(scratch: &Scratch, llc: &Path, name: &str, text: &str) -> PathBuf {
    let input = scratch.write(&format!("{name}.ll"), text);
    let out = scratch.join(format!("{name}.o"));
    let mut command = Command::new(llc);
    command
        .args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj"])
        .arg(&input)
        .arg("-o")
        .arg(&out);
    let outcome = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "llc {name}: {}", outcome.stderr_text());
    out
}

/// `text` with every non-zero `DILocation` line one more.
fn off_by_one(text: &str) -> String {
    text.lines()
        .map(|l| match l.split_once("!DILocation(line: ") {
            Some((head, rest)) => {
                let (n, tail) = rest.split_once(',').unwrap();
                let n: u32 = n.parse().unwrap();
                format!("{head}!DILocation(line: {}, {tail}\n", if n == 0 { 0 } else { n + 1 })
            }
            None => format!("{l}\n"),
        })
        .collect()
}

/// A stop: the source file's name and the line.
type Stop = (String, u32);

/// The stops of a session's output: every `... at <file>:<line>` the
/// debugger printed for a frame of one of the test's sources,
/// consecutive repeats merged.
fn stops(output: &str) -> Vec<Stop> {
    let mut out: Vec<Stop> = Vec::new();
    for l in output.lines() {
        // gdb: "func (...) at /dir/file.c:12"; lldb: "frame #0: ... at file.c:12:3".
        // Not the outer frames of a backtrace.
        let t = l.trim_start_matches([' ', '*']);
        let outer =
            |rest: Option<&str>| rest.is_some_and(|r| !r.starts_with("0 ") && !r.starts_with("0:"));
        if outer(t.strip_prefix('#')) || outer(t.strip_prefix("frame #")) {
            continue;
        }
        let Some((_, at)) = l.rsplit_once(" at ") else { continue };
        let mut parts = at.trim().split(':');
        let (Some(file), Some(line)) = (parts.next(), parts.next()) else { continue };
        let Ok(line) = line.trim().parse::<u32>() else { continue };
        let file = file.rsplit('/').next().unwrap_or(file).to_owned();
        if !data_dir().join(&file).exists() {
            continue;
        }
        if out.last() != Some(&(file.clone(), line)) {
            out.push((file, line));
        }
    }
    out
}

/// Whether hod's stops are `llc`'s with some of `llc`'s stops at the
/// lines in `skippable` (per file) left out.
fn matches(hod: &[Stop], llc: &[Stop], skippable: &BTreeSet<Stop>) -> bool {
    let mut i = 0;
    let mut last: Option<&Stop> = None;
    for s in llc {
        if hod.get(i) == Some(s) {
            i += 1;
            last = Some(s);
        } else if !skippable.contains(s) && last != Some(s) {
            return false;
        }
    }
    i == hod.len()
}

fn gdb_session(gdb: &Path, program: &Path, commands: &[String]) -> String {
    let mut command = Command::new(gdb);
    command
        .args(["-nx", "-batch", "-ex", "set pagination off", "-ex", "set confirm off"])
        .args(["-ex", "set debuginfod enabled off", "-ex", "set style enabled off"])
        .args(["-ex", "set print frame-info source-and-location"])
        .arg("-ex")
        .arg(format!("set substitute-path {COMP_DIR} {}", data_dir().display()));
    for c in commands {
        command.arg("-ex").arg(c);
    }
    command.arg(program);
    let out = run_with(&mut command, &[], Duration::from_mins(3));
    assert!(!out.timed_out(), "gdb on {}", program.display());
    out.stdout_text()
}

fn lldb_session(lldb: &Path, program: &Path, commands: &[String]) -> String {
    let mut command = Command::new(lldb);
    command
        .args(["--no-lldbinit", "--batch", "-o", "settings set use-color false"])
        .args(["-o", "settings set target.disable-aslr false"])
        .arg("-o")
        .arg(format!("settings set target.source-map {COMP_DIR} {}", data_dir().display()));
    for c in commands {
        command.arg("-o").arg(c);
    }
    command.arg(program);
    let out = run_with(&mut command, &[], Duration::from_mins(3));
    assert!(!out.timed_out(), "lldb on {}", program.display());
    out.stdout_text()
}

/// The debuggers present: gdb, and lldb.
fn debuggers() -> Option<(PathBuf, PathBuf)> {
    if !cfg!(target_arch = "aarch64") {
        eprintln!("note: the programs are built for AArch64; skipping on this host");
        return None;
    }
    Some((tool("gdb")?, tool("lldb")?))
}

/// The stepping session: from `main`, `STEPS` steps into everything
/// with line information.
fn step_commands(lldb: bool) -> Vec<String> {
    let mut c: Vec<String> = if lldb {
        vec!["breakpoint set --name main".into(), "run".into()]
    } else {
        vec!["break main".into(), "run".into()]
    };
    let step = if lldb { "thread step-in" } else { "step" };
    c.extend(std::iter::repeat_n(step.to_owned(), STEPS));
    c
}

/// Builds the program from `objects` with `linker`, runs it (exit 0).
fn program(
    toolchain: &Toolchain,
    linker: &Linker,
    scratch: &Scratch,
    name: &str,
    objects: &[&Path],
) -> PathBuf {
    let dir = scratch.join(format!("{name}-{}", linker.name));
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join(name);
    let ran = toolchain.link_and_run(linker, &[], objects, &out);
    assert_eq!(exit_code(&ran), Some(0), "{name} with {}", linker.name);
    out
}

/// The skippable stops of `ll` in `file`: the lines of its
/// unconditional branches.
fn skippable(file: &str, ll: &str) -> BTreeSet<Stop> {
    branch_locations(ll).into_iter().map(|(line, _)| (file.to_owned(), line)).collect()
}

#[test]
fn prog_sessions_match_llc() {
    let Some((gdb, lldb)) = debuggers() else { return };
    let Some(llc) = tool("llc-22") else { return };
    let Some(gcc) = tool("gcc") else { return };
    let scratch = Scratch::new("lines-debug-prog");
    let Some(toolchain) = Toolchain::aarch64(&scratch) else { return };
    let main = host_object(&scratch, &gcc, "main.c");
    let text = with_header(&data("prog.ll"), 22);
    let hod = hod_object(&scratch, "prog-hod", &text);
    let llc = llc_object(&scratch, &llc, "prog-llc", &text);
    let wrong = hod_object(&scratch, "prog-wrong", &off_by_one(&text));
    let skip = skippable("prog.c", &text);

    let breakpoint_gdb: Vec<String> =
        ["break prog.c:18", "run", "bt", "info line prog.c:18", "next", "next", "next"]
            .map(str::to_owned)
            .to_vec();
    let breakpoint_lldb: Vec<String> = [
        "breakpoint set --file prog.c --line 18",
        "run",
        "bt",
        "thread step-over",
        "thread step-over",
        "thread step-over",
    ]
    .map(str::to_owned)
    .to_vec();

    for linker in &toolchain.linkers {
        let tag = linker.name;
        let p_hod = program(&toolchain, linker, &scratch, "hod", &[&main, &hod]);
        let p_llc = program(&toolchain, linker, &scratch, "llc", &[&main, &llc]);
        let p_wrong = program(&toolchain, linker, &scratch, "wrong", &[&main, &wrong]);
        for (debugger, lldb_like) in [(&gdb, false), (&lldb, true)] {
            let name = if lldb_like { "lldb" } else { "gdb" };
            let session = |p: &Path, c: &[String]| {
                if lldb_like { lldb_session(debugger, p, c) } else { gdb_session(debugger, p, c) }
            };
            // Stepping through everything.
            let c = step_commands(lldb_like);
            let (h, l, w) = (session(&p_hod, &c), session(&p_llc, &c), session(&p_wrong, &c));
            let (sh, sl, sw) = (stops(&h), stops(&l), stops(&w));
            let in_prog = sh.iter().filter(|s| s.0 == "prog.c").count();
            assert!(in_prog >= 30, "{tag}: {name}: {in_prog} stops in prog.c:\n{h}");
            for line in [4, 13, 28] {
                assert!(
                    sh.contains(&("prog.c".into(), line)),
                    "{tag}: {name}: prog.c:{line}:\n{h}"
                );
            }
            assert!(
                matches(&sh, &sl, &skip),
                "{tag}: {name}: hod's stops against llc's:\n hod {sh:?}\n llc {sl:?}"
            );
            assert!(!matches(&sw, &sl, &skip), "{tag}: {name}: the wrong table is not caught");
            // A breakpoint by line, the backtrace, `info line`, `next`.
            let c = if lldb_like { &breakpoint_lldb } else { &breakpoint_gdb };
            let (h, l) = (session(&p_hod, c), session(&p_llc, c));
            let (bh, bl) = (stops(&h), stops(&l));
            assert_eq!(bh.first(), Some(&("prog.c".into(), 18)), "{tag}: {name}:\n{h}");
            assert!(matches(&bh, &bl, &skip), "{tag}: {name}:\n hod {bh:?}\n llc {bl:?}\n{h}");
            assert!(bh.contains(&("prog.c".into(), 23)), "{tag}: {name}:\n{h}");
            assert!(h.contains("classify"), "{tag}: {name}: bt:\n{h}");
            assert!(h.contains("main.c:9"), "{tag}: {name}: bt:\n{h}");
            if !lldb_like {
                let info = h.lines().find(|l| l.starts_with("Line 18 of \"")).unwrap_or("");
                assert!(info.contains("prog.c\" starts at address"), "{tag}: info line:\n{h}");
            }
            eprintln!(
                "{tag}: {name}: {} stops ({in_prog} in hod's code), llc {}; breakpoint session {} stops",
                sh.len(),
                sl.len(),
                bh.len()
            );
        }
    }
}

#[test]
fn comdat_sessions_and_verify() {
    let Some((gdb, lldb)) = debuggers() else { return };
    let Some(llc) = tool("llc-22") else { return };
    let Some(gxx) = tool("g++") else { return };
    let Some(dwarfdump) = tool("llvm-dwarfdump") else { return };
    let scratch = Scratch::new("lines-debug-comdat");
    let Some(toolchain) = Toolchain::aarch64(&scratch) else { return };
    let main = host_object(&scratch, &gxx, "comdat_main.cpp");
    let (t1, t2) = (with_header(&data("tu1.ll"), 22), with_header(&data("tu2.ll"), 22));
    let hod = [hod_object(&scratch, "tu1-hod", &t1), hod_object(&scratch, "tu2-hod", &t2)];
    let llc =
        [llc_object(&scratch, &llc, "tu1-llc", &t1), llc_object(&scratch, &llc, "tu2-llc", &t2)];
    let mut skip = skippable("tu1.cpp", &t1);
    skip.extend(skippable("shared.h", &t1));
    skip.extend(skippable("tu2.cpp", &t2));
    skip.extend(skippable("shared.h", &t2));

    for linker in &toolchain.linkers {
        let tag = linker.name;
        let p_hod = program(&toolchain, linker, &scratch, "hod", &[&main, &hod[0], &hod[1]]);
        let p_llc = program(&toolchain, linker, &scratch, "llc", &[&main, &llc[0], &llc[1]]);
        for p in [&p_hod, &p_llc] {
            let mut command = Command::new(&dwarfdump);
            command.args(["--verify", "--debug-line"]).arg(p);
            let out = run_with(&mut command, &[], Duration::from_mins(1));
            assert!(
                out.success(),
                "{tag}: verify {}:\n{}{}",
                p.display(),
                out.stdout_text(),
                out.stderr_text()
            );
        }
        for (debugger, lldb_like) in [(&gdb, false), (&lldb, true)] {
            let name = if lldb_like { "lldb" } else { "gdb" };
            let session = |p: &Path, c: &[String]| {
                if lldb_like { lldb_session(debugger, p, c) } else { gdb_session(debugger, p, c) }
            };
            let c = step_commands(lldb_like);
            let (h, l) = (session(&p_hod, &c), session(&p_llc, &c));
            let (sh, sl) = (stops(&h), stops(&l));
            for file in ["shared.h", "tu1.cpp", "tu2.cpp"] {
                assert!(sh.iter().any(|s| s.0 == file), "{tag}: {name}: no stop in {file}:\n{h}");
            }
            assert!(matches(&sh, &sl, &skip), "{tag}: {name}:\n hod {sh:?}\n llc {sl:?}");
            // The kept copy of `shared`: one breakpoint location, hit
            // from both units.
            let c: Vec<String> = if lldb_like {
                ["breakpoint set --file shared.h --line 3", "run", "continue", "breakpoint list"]
            } else {
                ["break shared.h:3", "run", "continue", "info breakpoints"]
            }
            .map(str::to_owned)
            .to_vec();
            let h = session(&p_hod, &c);
            let hits = stops(&h).len();
            let hit_twice = if lldb_like {
                h.contains("hit count = 2")
            } else {
                h.contains("already hit 2 times")
            };
            assert!(hit_twice, "{tag}: {name}: shared.h:3 hit twice ({hits} stops):\n{h}");
            eprintln!("{tag}: {name}: COMDAT program, {} stops (llc {})", sh.len(), sl.len());
        }
    }
}
