//! Line-tables-only and directives-only programs under the debuggers
//! (`spec/dwarf.md` §6.9, WP-061). `data/reduced/lto.cpp` is compiled
//! with `-gline-tables-only` (and the mixed program from units of every
//! kind, `llvm-link`ed) by clang 19 and 22, once by hod and once by `llc`
//! of the same major, linked with GNU ld, lld and mold, and driven by gdb
//! and lldb: `break file:line`, `bt`, `finish`/`step-out`, `info line`.
//! A session on hod's program prints what the same session on `llc`'s
//! prints (addresses, process ids, code offsets masked), and the frames
//! the source makes.

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
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/reduced")
}

fn ir_with(compiler: &Path, scratch: &Scratch, name: &str, source: &str, flag: &str) -> PathBuf {
    let out = scratch.join(format!("{name}.ll"));
    let mut command = Command::new(compiler);
    command.args([flag, "-O0", "-S", "-emit-llvm", "-w", "-std=c++20"]);
    command.arg(data_dir().join(source)).arg("-o").arg(&out);
    let outcome = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "{command:?}: {}", outcome.stderr_text());
    out
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
            // `<symbol+offset>`: the offset is the code's.
            if b[i] == b'+' && i > 0 && b[i - 1] != b' ' {
                let digits = b[i + 1..].iter().take_while(|c| c.is_ascii_digit()).count();
                if digits > 0 && b.get(i + 1 + digits) == Some(&b'>') {
                    out.push_str("+?");
                    i += 1 + digits;
                    continue;
                }
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
    name: &'static str,
    /// The sources and their debug flags, `llvm-link`ed in this order
    /// when there are several.
    parts: &'static [(&'static str, &'static str)],
    gdb: &'static [&'static str],
    gdb_wants: &'static [&'static str],
    lldb: &'static [&'static str],
    lldb_wants: &'static [&'static str],
}

const LTO: Case = Case {
    name: "lto",
    parts: &[("lto.cpp", "-gline-tables-only")],
    gdb: &[
        "break lto.cpp:14",
        "tbreak lto.cpp:21",
        "tbreak lto.cpp:7",
        "tbreak lto.cpp:24",
        "info line lto.cpp:31",
        "run",
        "bt",
        "finish",
        "continue",
        "bt",
        "delete 1",
        "continue",
        "bt",
        "up",
        "continue",
        "bt",
        "continue",
    ],
    // No DIE for these functions: gdb names them from the symbol table.
    gdb_wants: &[
        "Temporary breakpoint 3, geo::Point::sum() const () at ",
        "0x? in run(int) () at ",
        "Breakpoint 1, fact(int) () at ",
        "lto.cpp:14\n14\t    return 1;",
        "#3  0x? in fact(int) ()",
        "#4  0x? in run(int) () at ",
        "#5  0x? in main () at ",
        "int biggest<int>(int, int) () at ",
        "#0  twice(int) () at ",
        "exited normally",
    ],
    lldb: &[
        "breakpoint set --file lto.cpp --line 14 --one-shot true",
        "breakpoint set --file lto.cpp --line 21 --one-shot true",
        "breakpoint set --file lto.cpp --line 9 --one-shot true",
        "run",
        "bt",
        // Not `thread step-out`: the column of the return address's row
        // is the call's in `llc`'s table and the multiplication's in
        // hod's (`dies_debugger.rs` says the same).
        "continue",
        "bt",
        "continue",
        "bt",
        "continue",
    ],
    lldb_wants: &[
        "fact(",
        "run(",
        "main",
        "biggest<int>(",
        "geo::scale(",
        "exited with status = 0",
    ],
};

const MIXED: Case = Case {
    name: "mixed",
    parts: &[
        ("mixed_lto.cpp", "-gline-tables-only"),
        ("mixed_full.cpp", "-g"),
        ("mixed_ddo.cpp", "-gline-directives-only"),
        ("mixed_none.cpp", "-g0"),
    ],
    gdb: &[
        "tbreak mixed_lto.cpp:1",
        "tbreak mixed_full.cpp:3",
        "tbreak mixed_ddo.cpp:1",
        "run",
        "bt",
        "info locals",
        "continue",
        "bt",
        "continue",
    ],
    gdb_wants: &["lto_fn(int) () at ", "full_fn (x=1) at ", "y = 2", "exited normally"],
    lldb: &[
        "breakpoint set --file mixed_lto.cpp --line 1 --one-shot true",
        "breakpoint set --file mixed_full.cpp --line 3 --one-shot true",
        "run",
        "bt",
        "frame variable",
        "continue",
        "bt",
        "continue",
    ],
    lldb_wants: &["lto_fn(", "(int) y = 2", "exited with status = 0"],
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
    let clang = tool(&format!("clang++-{major}"))?;
    let paths: Vec<PathBuf> = case
        .parts
        .iter()
        .map(|(source, flag)| {
            let stem = source.split('.').next().unwrap();
            ir_with(&clang, scratch, &format!("{stem}-{major}"), source, flag)
        })
        .collect();
    if paths.len() == 1 {
        return Some(std::fs::read_to_string(&paths[0]).unwrap());
    }
    let mut command = Command::new(tool(&format!("llvm-link-{major}"))?);
    let out = scratch.join(format!("{}-{major}.ll", case.name));
    command.arg("-S").args(&paths).arg("-o").arg(&out);
    let outcome = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "{command:?}: {}", outcome.stderr_text());
    Some(std::fs::read_to_string(out).unwrap())
}

fn run_case(case: &Case) {
    if !cfg!(target_arch = "aarch64") {
        eprintln!("note: the programs are built for AArch64; skipping on this host");
        return;
    }
    let (Some(gdb), Some(lldb)) = (tool("gdb"), tool("lldb")) else { return };
    let scratch = Scratch::new(&format!("reduced-debug-{}", case.name));
    let Some(toolchain) = Toolchain::host(&scratch) else { return };
    let mut sessions = 0;
    for major in [19, 22] {
        let Some(llc) = tool(&format!("llc-{major}")) else { continue };
        let Some(text) = case_ir(&scratch, case, major) else { continue };
        let hod = hod_object(&scratch, &format!("{}-{major}-hod", case.name), &text);
        let llc = llc_object(&scratch, &llc, &format!("{}-{major}-llc", case.name), &text);
        for linker in &toolchain.linkers {
            let tag = format!("{}, LLVM {major}, {}", case.name, linker.name);
            let d_hod = format!("hod-{major}-{}", linker.name);
            let d_llc = format!("llc-{major}-{}", linker.name);
            let p_hod = link(&toolchain, linker, &scratch, &d_hod, &[&hod]);
            let p_llc = link(&toolchain, linker, &scratch, &d_llc, &[&llc]);
            for (lldb_like, wants) in [(false, case.gdb_wants), (true, case.lldb_wants)] {
                let name = if lldb_like { "lldb" } else { "gdb" };
                let (h, l) = if lldb_like {
                    (lldb_session(&lldb, &p_hod, case.lldb), lldb_session(&lldb, &p_llc, case.lldb))
                } else {
                    (gdb_session(&gdb, &p_hod, case.gdb), gdb_session(&gdb, &p_llc, case.gdb))
                };
                let h = masked(&h, &scratch.join(&d_hod));
                let l = masked(&l, &scratch.join(&d_llc));
                assert_eq!(h, l, "{tag}: {name}: hod's session, then llc's");
                for w in wants {
                    assert!(h.contains(w), "{tag}: {name}: {w:?} in\n{h}");
                }
                sessions += 2;
            }
        }
    }
    eprintln!("{}: {sessions} sessions, hod's equal to llc's", case.name);
}

#[test]
fn line_tables_only_sessions_match_llc() {
    run_case(&LTO);
}

#[test]
fn mixed_units_sessions_match_llc() {
    run_case(&MIXED);
}
