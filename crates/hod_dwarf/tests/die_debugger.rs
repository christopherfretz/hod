//! `spec/dwarf.md` §8.6: hod's DIEs under a real debugger.
//!
//! `tests/data/dies/{globals,main}.ll` (clang 19 `-O0 -g` output of the
//! sources beside them) are compiled by `llc`; the debug sections of
//! `globals.o` (`.debug_info`, `.debug_abbrev`, `.debug_str`,
//! `.debug_str_offsets`, `.debug_addr`, and the line table) are replaced
//! by the ones `hod_dwarf` writes from `hod_ir`'s tables, every other
//! byte of the object unchanged. The program is linked with GNU ld, lld
//! and mold; a gdb session (`info variables`, `print` of globals in
//! namespaces, `ptype` and `whatis` of base and pointer types, `info
//! address`) and an lldb one (`target variable`, `expression`) must print
//! what they print on the original program. A table whose addresses are
//! four bytes off must not.

mod common;

use std::ffi::OsString;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use common::dies::{die_section_list, hod_sections, splice};
use common::meta::load;
use common::session::{linkers, masked};
use common::{Elf, compile, llc};
use hod_dwarf::GlobalAddr;
use hod_obj::Arch;
use hod_testutil::{Scratch, find_tool, run, run_with, tool};

const GLOBALS: &str = include_str!("data/dies/globals.ll");
const MAIN: &str = include_str!("data/dies/main.ll");
const TRIPLE: &str = "aarch64-unknown-linux-gnu";

const PRINTED: [&str; 34] = [
    "counter",
    "hidden",
    "*hidden_ptr",
    "dval",
    "*dp",
    "*ip",
    "**pp",
    "letter",
    "*lp",
    "sc",
    "uc",
    "flag",
    "sh",
    "us",
    "lg",
    "ull",
    "fl",
    "ld",
    "wc",
    "c16",
    "c32",
    "i128",
    "aligned",
    "outer::depth",
    "outer::inner::deep",
    "*outer::inner::deep_ptr",
    "outer::half",
    "outer::inner::innermost::bottom",
    "v1::inl",
    "inl",
    "'(anonymous namespace)::anon'",
    "*anon_ptr",
    "(long)&aligned % 32",
    "sizeof(ld)",
];

const TYPED: [&str; 16] = [
    "counter",
    "hidden_ptr",
    "dp",
    "pp",
    "vp",
    "lp",
    "sc",
    "uc",
    "flag",
    "ull",
    "ld",
    "c16",
    "i128",
    "outer::inner::deep_ptr",
    "outer::half",
    "anon_ptr",
];

/// Names `target variable` does not find in the original program.
const LLDB_MISSES: [&str; 1] = ["outer::half"];

fn gdb_session(gdb: &Path, program: &Path) -> String {
    let mut cmd = Command::new(gdb);
    cmd.args(["-nx", "-batch", "-ex", "set pagination off", "-ex", "set confirm off"])
        .args(["-ex", "set debuginfod enabled off", "-ex", "set style enabled off"])
        .args(["-ex", "info variables -n"]);
    for t in TYPED {
        cmd.arg("-ex").arg(format!("ptype {t}"));
        cmd.arg("-ex").arg(format!("whatis {t}"));
    }
    cmd.args(["-ex", "info address counter", "-ex", "info address outer::inner::deep"]);
    cmd.args(["-ex", "break main", "-ex", "run"]);
    for p in PRINTED {
        cmd.arg("-ex").arg(format!("print {p}"));
    }
    cmd.arg(program);
    let out = run_with(&mut cmd, &[], Duration::from_mins(2));
    assert!(!out.timed_out(), "gdb on {}", program.display());
    // Errors (a name the debugger cannot find) are part of the session.
    masked(&(out.stdout_text() + &out.stderr_text()), program)
}

fn lldb_session(lldb: &Path, program: &Path) -> String {
    let mut cmd = Command::new(lldb);
    cmd.args(["--no-lldbinit", "--batch", "-o", "settings set use-color false"])
        .args(["-o", "settings set target.disable-aslr false"]);
    cmd.args(["-o", "breakpoint set --name main", "-o", "run"]);
    // One name per command: lldb stops a command at its first error, and
    // the batch at its first failed command. It cannot find some names
    // on `llc`'s program either (`LLDB_MISSES`).
    let names =
        PRINTED.iter().filter(|p| !p.contains(['*', '(', '\'']) && !LLDB_MISSES.contains(p));
    for n in names.chain(&["ip", "pp", "vp", "dp", "lp", "anon_ptr"]) {
        cmd.arg("-o").arg(format!("target variable {n}"));
    }
    for p in PRINTED.iter().filter(|p| p.starts_with('*')) {
        cmd.arg("-o").arg(format!("expression -- {p}"));
    }
    cmd.args(["-o", "type lookup int", "-o", "continue"]);
    cmd.arg(program);
    let out = run_with(&mut cmd, &[], Duration::from_mins(2));
    assert!(!out.timed_out(), "lldb on {}", program.display());
    masked(&(out.stdout_text() + &out.stderr_text()), program)
}

fn four_off(g: &mut GlobalAddr) {
    g.addend += 4;
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
    let scratch = Scratch::new(&format!("dies-debugger{major}"));
    let loaded = load(GLOBALS);
    let globals = compile(&llc, &scratch, &format!("globals-{major}"), GLOBALS, TRIPLE);
    let main = scratch.write(
        &format!("main-{major}.o"),
        compile(&llc, &scratch, &format!("main-{major}"), MAIN, TRIPLE),
    );
    let elf = Elf::parse(globals.clone());
    let variant = |adjust: fn(&mut GlobalAddr)| {
        let (obj, line, dies) = hod_sections(&elf, Arch::AArch64, &loaded, adjust);
        splice(&elf, &obj, &die_section_list(line, &dies))
    };
    let variants: Vec<(&str, Vec<u8>)> =
        vec![("orig", globals), ("hod", variant(|_| {})), ("wrong", variant(four_off))];

    for (linker, flags) in linkers(&scratch) {
        let mut gdb_out = Vec::new();
        let mut lldb_out = Vec::new();
        for (name, bytes) in &variants {
            // One name in a directory per variant: debuggers print it.
            let dir = scratch.join(name);
            std::fs::create_dir_all(&dir).unwrap();
            let object = dir.join(format!("globals-{major}-{linker}.o"));
            std::fs::write(&object, bytes).unwrap();
            let program = dir.join(format!("dies-{major}-{linker}"));
            let mut args: Vec<OsString> = flags.clone();
            args.extend([main.clone().into(), object.into(), "-o".into()]);
            args.push(program.clone().into());
            let linked = run(&driver, &args);
            assert!(linked.success(), "link {}: {}", program.display(), linked.stderr_text());
            gdb_out.push(gdb_session(&gdb, &program));
            if let Some(lldb) = &lldb {
                lldb_out.push(lldb_session(lldb, &program));
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
            // The session did what it says.
            let wants: &[&str] = if debugger == "gdb" {
                &["= -1234567890123", "= 7", "type = int **", "type = void *", "= 1.25", "= 42"]
            } else {
                &["(long) outer::inner::deep = 7", "(int **) pp = ADDR", "(__int128) i128 = 42"]
            };
            for w in wants {
                assert!(orig.contains(w), "{tag}: {debugger} on the original: no {w:?}:\n{orig}");
            }
            assert_eq!(hod, orig, "{tag}: {debugger}: hod's DIEs against llc's");
            assert_ne!(wrong, orig, "{tag}: {debugger}: the wrong table is caught");
            eprintln!(
                "{tag}: {debugger}: {} lines of session identical; the four-bytes-off table differs",
                orig.lines().count()
            );
        }
    }
}

/// §8.6 with LLVM 19's objects.
#[test]
fn llc19_debugger_sees_the_same_globals() {
    debugger(19);
}

/// §8.6 with LLVM 22's objects.
#[test]
fn llc22_debugger_sees_the_same_globals() {
    debugger(22);
}
