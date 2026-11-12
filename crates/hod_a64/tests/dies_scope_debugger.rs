//! The DIEs outside function bodies under the debuggers (`spec/dwarf.md`
//! §6.11, WP-059): `dies_scope.rs` compares the trees with the addresses
//! masked; here the addresses are compared by behaviour. Each program of
//! `data/dies_scope` is compiled from clang's IR (LLVM 19 IR by `llc` 19,
//! LLVM 22 IR by `llc` 22) once by hod and once by `llc`, linked with
//! GNU ld, lld and mold, and driven by gdb and lldb. A session on hod's
//! program prints what the same session on `llc`'s prints (addresses,
//! process ids and code offsets masked), and the values the sources
//! compute:
//!
//! - `globals.cpp`: namespaced, inline-namespaced and anonymous globals,
//!   static members (a definition, an in-class constant, an inline one),
//!   a variable template's instances, an aligned global, constants;
//! - `templates.cpp`: class templates with type, value (integers and
//!   addresses), pack and template template parameters, a member
//!   template, function templates;
//! - `scope.cpp`: out-of-line static member and member function
//!   definitions, nested classes, imported declarations, enumeration
//!   and unsigned template values.

mod common;
#[path = "dies_support/mod.rs"]
mod support;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use common::linking::{Linker, Toolchain, exit_code};
use hod_testutil::{Scratch, run_with, tool};
use support::{hod_object, llc_object};

fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/dies_scope")
}

/// `-g -O0` IR of `source` by `clang++-{major}`, with its template
/// parameters.
fn full_ir(scratch: &Scratch, major: u32, source: &str) -> Option<String> {
    let compiler = tool(&format!("clang++-{major}"))?;
    let stem = source.split('.').next().unwrap();
    let out = scratch.join(format!("{stem}-{major}.ll"));
    let mut command = Command::new(compiler);
    command.args(["-g", "-O0", "-S", "-emit-llvm", "-w", "-std=c++20"]);
    command.arg(data_dir().join(source)).arg("-o").arg(&out);
    let outcome = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "{source}: {}", outcome.stderr_text());
    Some(std::fs::read_to_string(out).unwrap())
}

/// A session's output with addresses, process ids, code offsets and the
/// program's directory masked.
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

struct Case {
    source: &'static str,
    gdb: &'static [&'static str],
    gdb_wants: &'static [&'static str],
    lldb: &'static [&'static str],
    lldb_wants: &'static [&'static str],
}

const GLOBALS: Case = Case {
    source: "globals.cpp",
    gdb: &[
        "break user",
        "run",
        "print outer::inner::deep",
        "print outer::versioned",
        "print outer::hidden",
        "print S::count",
        "print S::limit",
        "print S::ratio",
        "print S::live",
        "print vt<int>",
        "print vt<long>",
        "print aligned_global",
        "print &aligned_global",
        "print folded",
        "print kconst",
        "ptype S",
        "finish",
    ],
    gdb_wants: &[
        "$1 = 4\n$2 = 4\n$3 = 5\n$4 = 1\n$5 = 10\n$6 = 1.5\n$7 = 7\n$8 = 3\n$9 = 3\n$10 = 8\n",
        "<aligned_global>",
        "$12 = 42\n$13 = 9\n",
        "int get<int>(int);",
        "Value returned is $14 = 90",
    ],
    lldb: &[
        "breakpoint set -n user",
        "run",
        // lldb names the inline namespace, and finds no variable
        // template's instance by name (neither in `llc`'s program).
        "target variable outer::v1::versioned outer::inner::deep",
        "target variable S::count S::live aligned_global",
        "expression S::limit + S::count",
        "type lookup S",
    ],
    lldb_wants: &[
        "(int) outer::v1::versioned = 4",
        "(int) outer::inner::deep = 4",
        "(int) S::count = 1",
        "(int) S::live = 7",
        "(int) aligned_global = 8",
        "(int) $0 = 11",
        "static const int limit = 10;",
    ],
};

const TEMPLATES: Case = Case {
    source: "templates.cpp",
    gdb: &[
        "break templates.cpp:36",
        "run",
        "print b",
        "print c",
        "print sizeof(c)",
        "print w",
        "ptype v",
        "print v.get()",
        "print twice<int>(4)",
        "print count<int, double, char>",
        "info functions count",
    ],
    // `v.get()`: `*P` (`gx`, 1) + `'x'` + -5.
    gdb_wants: &[
        "$1 = {items = {0, 0, 0}}",
        "$3 = 2",
        "(char)120, 123456789012ul",
        "$5 = 116",
        "$6 = 8",
    ],
    lldb: &[
        "breakpoint set -f templates.cpp -l 36",
        "run",
        "frame variable b c w",
        "expression v.get()",
        "image lookup -r -n twice",
    ],
    lldb_wants: &[
        "(Box<int, 3>) b",
        "(Box<char, 2, int, long>) c",
        "(Wrap<Box, short>) w",
        "(int) $0 = 116",
    ],
};

const SCOPE: Case = Case {
    source: "scope.cpp",
    gdb: &[
        "break user2",
        "run",
        "print lib::Base::tag",
        "print lib::Base::arr",
        "print lib::Tpl<short>::value",
        "print lib::Tpl<int>::k",
        "print Outer::Inner::n",
        "print lib::helper(4)",
        "tbreak scope.cpp:33",
        "continue",
        "ptype ev",
        "print ev.f()",
        "finish",
    ],
    // `ev.f()`: 200 + 250 - 3 + (int)-9000000000.
    gdb_wants: &[
        "$1 = \"hi\"",
        "$2 = {1, 2, 3}",
        "$3 = 1",
        "$4 = 4",
        "$5 = 5",
        "$6 = 5",
        "$7 = -410064961",
    ],
    lldb: &[
        "breakpoint set -n user2",
        "run",
        "target variable lib::Base::tag lib::Base::arr Outer::Inner::n",
        "expression lib::helper(4)",
        "breakpoint set -f scope.cpp -l 33",
        "continue",
        "frame variable ev",
        "expression ev.f()",
    ],
    lldb_wants: &["\"hi\"", "[1] = 2", "(int) Outer::Inner::n = 5", "(int) $0 = 5"],
};

fn link(
    toolchain: &Toolchain,
    linker: &Linker,
    scratch: &Scratch,
    dir: &str,
    obj: &Path,
) -> PathBuf {
    let dir = scratch.join(dir);
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("prog");
    // `-lstdc++` (the personality routine) after the object.
    let ran = toolchain.link_and_run(linker, &[], &[obj, Path::new("-lstdc++")], &out);
    assert_eq!(exit_code(&ran), Some(0), "{} with {}", out.display(), linker.name);
    out
}

fn run_case(case: &Case) {
    if !cfg!(target_arch = "aarch64") {
        eprintln!("note: the programs are built for AArch64; skipping on this host");
        return;
    }
    let (Some(gdb), Some(lldb)) = (tool("gdb"), tool("lldb")) else { return };
    let stem = case.source.split('.').next().unwrap();
    let scratch = Scratch::new(&format!("dies-scope-debug-{stem}"));
    let Some(toolchain) = Toolchain::host(&scratch) else { return };
    let mut sessions = 0;
    for major in [19, 22] {
        let Some(llc) = tool(&format!("llc-{major}")) else { continue };
        let Some(text) = full_ir(&scratch, major, case.source) else { continue };
        let hod = hod_object(&scratch, &format!("{stem}-{major}-hod"), &text);
        let llc = llc_object(&scratch, &llc, &format!("{stem}-{major}-llc"), &text);
        for linker in &toolchain.linkers {
            let tag = format!("{stem}, LLVM {major}, {}", linker.name);
            let d_hod = format!("hod-{major}-{}", linker.name);
            let d_llc = format!("llc-{major}-{}", linker.name);
            let p_hod = link(&toolchain, linker, &scratch, &d_hod, &hod);
            let p_llc = link(&toolchain, linker, &scratch, &d_llc, &llc);
            for (lldb_like, wants) in [(false, case.gdb_wants), (true, case.lldb_wants)] {
                let name = if lldb_like { "lldb" } else { "gdb" };
                let (h, l) = if lldb_like {
                    (lldb_session(&lldb, &p_hod, case.lldb), lldb_session(&lldb, &p_llc, case.lldb))
                } else {
                    (gdb_session(&gdb, &p_hod, case.gdb), gdb_session(&gdb, &p_llc, case.gdb))
                };
                let (h, l) = (masked(&h, &scratch.join(&d_hod)), masked(&l, &scratch.join(&d_llc)));
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
fn globals_sessions_match_llc() {
    run_case(&GLOBALS);
}

#[test]
fn templates_sessions_match_llc() {
    run_case(&TEMPLATES);
}

#[test]
fn scope_sessions_match_llc() {
    run_case(&SCOPE);
}
