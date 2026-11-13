//! Function DIEs under the debuggers (`spec/dwarf.md` §6.4, §8.7): the
//! variable locations `dies.rs` masks are compared by behaviour here.
//! Each program's driver (compiled by the host's compiler) calls code
//! compiled from `data/dies`, once by hod and once by `llc` (LLVM 19 IR
//! by `llc` 19, LLVM 22 IR by `llc` 22), linked with GNU ld, lld and
//! mold, then driven by gdb and lldb. A session on hod's program prints
//! what the same session on `llc`'s prints (addresses, process ids and
//! code offsets masked), and the values the sources compute.
//!
//! - `vars.c`: recursion (`bt`, `up`, `down`, `finish`), shadowed
//!   scopes, by-value structures in registers, on the stack and by
//!   reference, `float` and `double`, a variadic function, pointers and
//!   arrays, a label (`break labels:again`), a function-local type;
//! - `members.cpp`: a constructor, `this` in const and non-const
//!   members, a static member with a local type, a namespace function,
//!   a function template's instances and an inline (COMDAT) function;
//! - `hand.ll`: a block with two ranges, whose variable is out of scope
//!   in the parent's code between them, and an unused parameter.

mod common;
#[path = "dies_support/mod.rs"]
mod support;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use common::linking::{Linker, Toolchain, exit_code};
use common::ll_header;
use hod_testutil::{Scratch, run_with, tool};
use support::{data, data_dir, has_ext, hod_object, ir, llc_object};

fn host_object(scratch: &Scratch, compiler: &Path, name: &str) -> PathBuf {
    let out = scratch.join(format!("{name}.o"));
    let mut command = Command::new(compiler);
    command.args(["-c", "-g", "-O0"]).arg(data_dir().join(name)).arg("-o").arg(&out);
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
    /// The test source hod and `llc` compile, and its driver.
    source: &'static str,
    driver: &'static str,
    gdb: &'static [&'static str],
    gdb_wants: &'static [&'static str],
    lldb: &'static [&'static str],
    lldb_wants: &'static [&'static str],
}

const VARS: Case = Case {
    source: "vars.c",
    driver: "main.c",
    gdb: &[
        "tbreak vars.c:16",
        "tbreak vars.c:26",
        "tbreak vars.c:31",
        "tbreak vars.c:39",
        "tbreak vars.c:45",
        "tbreak vars.c:50",
        "tbreak vars.c:60",
        "tbreak vars.c:67",
        "tbreak labels:again",
        "tbreak vars.c:76",
        "tbreak vars.c:83",
        "run",
        "bt",
        "info args",
        "info locals",
        "up",
        "info locals",
        "print n * 10",
        "down",
        "finish",
        "continue",
        "info locals",
        "bt",
        "continue",
        "info locals",
        "print y",
        "continue",
        "info args",
        "info locals",
        "print big.x[4]",
        "print p.b",
        "continue",
        "info args",
        "info locals",
        "continue",
        "info args",
        "info locals",
        "print m.f",
        "continue",
        "info args",
        "print total",
        "finish",
        "continue",
        "info args",
        "info locals",
        "print *q",
        "print pair",
        "print local[3]",
        "continue",
        "info locals",
        "print n",
        "continue",
        "info locals",
        "continue",
        "info locals",
        "ptype l",
        "whatis m",
        "finish",
        "continue",
    ],
    gdb_wants: &[
        "fact (n=1)",
        "fact (n=4)",
        "below = 0\nresult = 1",
        "$1 = 20",
        "Value returned is $2 = 1",
        "x = 200\nx = 20\nx = 3",
        "y = 3\ni = 0\nx = 3",
        "p = {a = 3, b = 40}",
        "big = {x = {1, 2, 3, 4, 5}}",
        "d = 1.25\ng = 2.5\nsum = 53",
        "spilled = {a = 3, b = 40}\nlast = 9\nt = 61",
        "m = {f = {1.5, 2.5, 3.5, 4.5}}\nr = 5.25",
        "count = 3",
        "= 60",
        "local = {11, 12, 13, 104}",
        "pair = {a = 11, b = 77}",
        "tries = 0",
        "tries = 3",
        "type = struct Local {\n    int q;\n    short r;\n}",
        "type = Mine",
        "Value returned is $",
        "exited normally",
    ],
    lldb: &[
        "breakpoint set --file vars.c --line 16 --one-shot true",
        "breakpoint set --file vars.c --line 26 --one-shot true",
        "breakpoint set --file vars.c --line 39 --one-shot true",
        "breakpoint set --file vars.c --line 45 --one-shot true",
        "breakpoint set --file vars.c --line 50 --one-shot true",
        "breakpoint set --file vars.c --line 60 --one-shot true",
        "breakpoint set --file vars.c --line 67 --one-shot true",
        "breakpoint set --file vars.c --line 83 --one-shot true",
        "run",
        "bt",
        "frame variable",
        "up",
        "frame variable",
        "down",
        "thread step-out",
        "continue",
        "frame variable",
        "continue",
        "frame variable",
        "continue",
        "frame variable",
        "continue",
        "frame variable",
        "continue",
        "frame variable",
        "continue",
        "frame variable",
        "continue",
        "frame variable",
        "continue",
    ],
    lldb_wants: &[
        "(int) n = 1\n(int) below = 0\n(int) result = 1",
        "prog`fact(n=4)",
        "(int) x = 3\n(int) x = 20\n(int) x = 200",
        "(Pair) p = (a = 3, b = 40)",
        "(double) d = 1.25\n(float) g = 2.5\n(long) sum = 53",
        "(Pair) spilled = (a = 3, b = 40)\n(long) last = 9",
        "(Many) m = (f = (1.5, 2.5, 3.5, 4.5))",
        "(int) count = 3",
        "(int[4]) local = ([0] = 11, [1] = 12, [2] = 13, [3] = 104)",
        "(Local) l = (q = 6, r = 2)",
        "(Mine) m = 8",
        "exited with status = 0",
    ],
};

const MEMBERS: Case = Case {
    source: "members.cpp",
    driver: "main.cpp",
    gdb: &[
        "tbreak members.cpp:14",
        "tbreak members.cpp:23",
        "tbreak members.cpp:18",
        "tbreak members.cpp:28",
        "tbreak members.cpp:34",
        "tbreak members.cpp:41",
        "tbreak members.cpp:46",
        "run",
        "info args",
        "continue",
        "info args",
        "print *this",
        "continue",
        "info locals",
        "print this->x",
        "print *this",
        "bt",
        "up",
        "print p",
        "down",
        "continue",
        "info args",
        "info locals",
        "ptype h",
        "continue",
        "info args",
        "print a",
        "info locals",
        "continue",
        "info args",
        "info locals",
        "bt",
        "continue",
        "info args",
        "info locals",
        "finish",
        "continue",
    ],
    gdb_wants: &[
        "x0=4, y0=5",
        "dx = 2\ndy = 3",
        "$1 = {x = 6, y = 5}",
        "s = 14",
        "geo::Point::sum (this=0x?)",
        "run_members (v=4)",
        "$4 = {x = 6, y = 8}",
        "type = struct Helper {\n    int k;\n}",
        "b = {x = 10, y = 20}",
        "w = 4\nh = 12",
        "biggest<int> (a=",
        "doubled = 8",
        "Value returned is $",
        "exited normally",
    ],
    lldb: &[
        "breakpoint set --file members.cpp --line 18 --one-shot true",
        "breakpoint set --file members.cpp --line 34 --one-shot true",
        "breakpoint set --file members.cpp --line 46 --one-shot true",
        "run",
        "frame variable",
        "frame variable *this",
        "bt",
        "continue",
        "frame variable",
        "continue",
        "frame variable",
        // Not `thread step-out`: the column of the return address's row
        // is the call's in `llc`'s table and the addition's in hod's.
        "continue",
    ],
    lldb_wants: &[
        "(int) s = 14",
        "(const geo::Point) *this = (x = 6, y = 8)",
        "(geo::Point) b = (x = 10, y = 20)",
        "(int) w = 4\n(int) h = 12",
        "(int) doubled = 8",
        "exited with status = 0",
    ],
};

const HAND: Case = Case {
    source: "hand.ll",
    driver: "hand_main.c",
    gdb: &[
        "tbreak hand.c:5",
        "tbreak hand.c:6",
        "tbreak hand.c:23",
        "run",
        "info locals",
        "continue",
        "info locals",
        "continue",
        "info args",
        "continue",
    ],
    gdb_wants: &["split (v=5)", "\nr = 0\n\n", "t = 5\nr = 0", "unused = 9", "exited normally"],
    lldb: &[
        "breakpoint set --file hand.c --line 5 --one-shot true",
        "breakpoint set --file hand.c --line 6 --one-shot true",
        "run",
        "frame variable",
        "continue",
        "frame variable",
        "continue",
    ],
    lldb_wants: &[
        "(lldb) frame variable\n(int) v = 5\n(int) r = 0\n(lldb) continue",
        "(int) v = 5\n(int) r = 0\n(int) t = 5",
        "exited with status = 0",
    ],
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
    if has_ext(case.source, "ll") {
        return Some(format!("{}{}", ll_header(major), data(case.source)));
    }
    let compiler = if has_ext(case.source, "c") {
        format!("clang-{major}")
    } else {
        format!("clang++-{major}")
    };
    let stem = case.source.split('.').next().unwrap();
    Some(ir(&tool(&compiler)?, scratch, &format!("{stem}-{major}"), case.source))
}

fn run_case(case: &Case) {
    if !cfg!(target_arch = "aarch64") {
        eprintln!("note: the programs are built for AArch64; skipping on this host");
        return;
    }
    let (Some(gdb), Some(lldb)) = (tool("gdb"), tool("lldb")) else { return };
    let Some(cc) = tool(if has_ext(case.driver, "c") { "gcc" } else { "g++" }) else { return };
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
                (masked(&h, &scratch.join(&d_hod)), masked(&l, &scratch.join(&d_llc)))
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
fn vars_sessions_match_llc() {
    run_case(&VARS);
}

#[test]
fn members_sessions_match_llc() {
    run_case(&MEMBERS);
}

#[test]
fn hand_sessions_match_llc() {
    run_case(&HAND);
}
