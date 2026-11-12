//! `spec/bc.md` §6 and §11.9: the debug information `hod_bc` reads from
//! bitcode is what `hod_ll` reads from `llvm-dis` of the same bitcode:
//! the debug tables (every node reachable from the compile units, the
//! global-variable expressions and the functions' subprograms, labelled
//! in the order a walk meets them), every instruction's location and
//! every debug record, compared as `common::read_*_bodies` write them.
//!
//! The sources are clang 19 and 22 at `-O0` and `-O2` on `hod_ll`'s
//! debug sources, and kiln's debug module; the corpus run is
//! `corpus_bodies_agree` in `bodies.rs`.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use hod_testutil::{DEFAULT_TIMEOUT, run_with};

const MAJORS: [u32; 2] = [19, 22];

fn ll_data() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../hod_ll/tests/data")
}

/// clang `major`'s bitcode of `source` (in `hod_ll`'s test data).
fn clang(major: u32, source: &str, options: &[&str]) -> Option<Vec<u8>> {
    let bin = common::llvm_bin(major)?;
    let mut command = Command::new(bin.join("clang"));
    command.args(["--target=aarch64-linux-gnu", "-c", "-emit-llvm", "-w", "-g", "-o", "-"]);
    if Path::new(source).extension().is_some_and(|ext| ext.eq_ignore_ascii_case("cpp")) {
        command.arg("-std=c++20");
    }
    command.args(options).arg(ll_data().join(source));
    let out = run_with(&mut command, &[], DEFAULT_TIMEOUT);
    assert!(out.success(), "clang-{major} {source} {options:?}: {}", out.stderr_text());
    Some(out.stdout)
}

/// The first line where the two dumps differ, with its neighbours.
fn difference(a: &str, b: &str) -> String {
    let (la, lb): (Vec<&str>, Vec<&str>) = (a.lines().collect(), b.lines().collect());
    let at = la.iter().zip(&lb).position(|(x, y)| x != y).unwrap_or(la.len().min(lb.len()));
    format!(
        "line {at}:\n  hod_ll: {}\n  hod_bc: {}",
        la.get(at).copied().unwrap_or("<end>"),
        lb.get(at).copied().unwrap_or("<end>")
    )
}

/// Compares the two readings of `bitcode`; the number of debug nodes
/// and of located instructions.
fn agree(what: &str, major: u32, bitcode: &[u8]) -> (usize, usize) {
    let text = common::disassemble(major, bitcode).expect("llvm-dis");
    let a = common::read_text_bodies(&text).unwrap_or_else(|e| panic!("{what}: hod_ll: {e}"));
    let b = common::read_bitcode_bodies(bitcode).unwrap_or_else(|e| panic!("{what}: hod_bc: {e}"));
    if a != b {
        if let Some(dir) = std::env::var_os("HOD_BC_DIFFS") {
            let dir = PathBuf::from(dir);
            let _ = std::fs::write(dir.join(format!("{what}.ll.txt")), &a);
            let _ = std::fs::write(dir.join(format!("{what}.bc.txt")), &b);
        }
        panic!("{what}: {}", difference(&a, &b));
    }
    let nodes = a.lines().filter(|line| line.starts_with('#')).count();
    let located = a.lines().filter(|line| line.contains(" !dbg (")).count();
    let records = a.lines().filter(|line| line.starts_with("  #dbg ")).count();
    let refused: Vec<&str> = a
        .lines()
        .filter(|line| line.starts_with("unsupported ") || line.contains(" unsupported ["))
        .collect();
    eprintln!(
        "{what}: {records} records; {} refusals: {:?}",
        refused.len(),
        &refused[..refused.len().min(4)]
    );
    (nodes, located)
}

fn sources(major: u32, source: &str, extra: &[&[&str]]) {
    for options in [&["-O0"][..], &["-O2"][..]].iter().chain(extra) {
        let Some(bitcode) = clang(major, source, options) else {
            eprintln!("skipped LLVM {major}: not installed");
            return;
        };
        let what = format!("{source}-{major}{}", options.join(""));
        let (nodes, located) = agree(&what, major, &bitcode);
        eprintln!("{what}: {nodes} nodes, {located} located instructions");
        assert!(nodes > 0 && located > 0, "{what}: the debug information is read");
    }
}

#[test]
fn clang_debug_types_agree() {
    for major in MAJORS {
        sources(major, "debug_types.cpp", &[]);
    }
}

#[test]
fn clang_debug_c_agrees() {
    for major in MAJORS {
        sources(major, "debug_c.c", &[]);
    }
}

#[test]
fn clang_stl_agrees() {
    for major in MAJORS {
        sources(major, "stl.cpp", &[]);
    }
}

/// LLVM 22's newer layouts: DWARF 6 language names, key instructions
/// (`DEBUG_LOC`'s atom group and rank).
#[test]
fn clang_22_new_layouts_agree() {
    sources(22, "debug_types.cpp", &[&["-O2", "-gkey-instructions"], &["-O0", "-gdwarf-6"]]);
}

/// kiln's debug module, assembled by each LLVM that accepts it.
#[test]
fn kiln_debug_module_agrees() {
    let text = std::fs::read(ll_data().join("debug/kiln_types.ll")).expect("kiln_types.ll");
    let mut read = 0;
    for major in MAJORS {
        let Ok(bitcode) = common::assemble(major, &text) else { continue };
        agree(&format!("kiln_types-{major}"), major, &bitcode);
        read += 1;
    }
    assert!(read > 0, "some LLVM assembles kiln_types.ll");
}
