//! The i386 join E (`spec/landed/codegen/i386-join-e.md`): WP-314's debug
//! information (`spec/codegen-x86.md` §15) with the constructs of join D,
//! and WP-317's SSE / SSE2 (§16, §17) with everything already there.
//! Programs in `tests/data/i386/join_e/`:
//! - `dbg_eh.cpp` (exceptions, `std::atomic` of 4 and 8 bytes, a struct
//!   with `long long` and `float` fields returned through an `invoke`),
//!   `dbg_va.cpp` (variadic functions, `va_copy`, structs by value and as
//!   results), `dbg_vla.cpp` (a VLA in a loop with a `double` and a
//!   `long long` live across it): built by clang 19 and 22 at `-O0 -g`,
//!   static and PIC. hod's objects verify (`llvm-dwarfdump --verify` of
//!   each major) and run as `llc`'s (GNU ld, lld, mold; plain and under
//!   `Stress::ALL`).
//! - `sse_eh.ll` (an `invoke` of SSE results whose pads use them),
//!   `sse_va.c` (varargs of SSE values and vectors), `sse_agg.c`
//!   (aggregates with SSE and vector leaves, `memcpy`, a dynamic
//!   `alloca`), `sse_atomic.ll` (float atomics in SSE2 functions): run as
//!   `llc`'s, static and PIE, every linker, plain and under `Stress::ALL`.
//!   `sse_dbg.c` (`-g` at SSE2): hod's DIE tree equals `llc`'s.
//!
//! A missing tool is skipped with a note, unless `HOD_REQUIRE_TOOLS=1`.

use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;

use hod::{Options, OutputKind, RelocModel};
use hod_testutil::{DEFAULT_TIMEOUT, LLVM_MAJORS, Scratch, run_with};

mod i386_i11b;
mod i386_support;
use i386_i11b::{Build, STATIC, Source, data, lane};
use i386_support::{TRIPLE, clang_module, llc_object, llvm_tool};

/// clang's flags of the `-g` programs.
const DEBUG_FLAGS: &[&str] = &["-g", "-Wno-vla-cxx-extension", "-Wno-unknown-warning-option"];

/// `llvm-dwarfdump --verify` of `object` must pass.
fn verifies(dwarfdump: &Path, object: &Path) {
    let mut command = Command::new(dwarfdump);
    command.arg("--verify").arg(object);
    let outcome = run_with(&mut command, &[], DEFAULT_TIMEOUT);
    let text = String::from_utf8_lossy(&outcome.stdout);
    assert!(outcome.success() && text.contains("No errors."), "{}: {text}", object.display());
}

/// hod's `-g` objects of `program` (clang 19 and 22, static and PIC)
/// verify; the number of objects verified.
fn debug_objects_verify(program: &str) -> usize {
    let scratch = Scratch::new(&format!("i386-join-e-verify-{program}"));
    let source = data("join_e", program);
    let mut verified = 0;
    for major in LLVM_MAJORS {
        let (Some(clang), Some(dwarfdump)) =
            (llvm_tool(major, "clang"), llvm_tool(major, "llvm-dwarfdump"))
        else {
            continue;
        };
        for pic in [false, true] {
            let label = format!("{program}-{major}-{}", if pic { "pic" } else { "static" });
            let mut flags = DEBUG_FLAGS.to_vec();
            flags.push(if pic { "-fPIE" } else { "-fno-pic" });
            let ll = clang_module(&scratch, &clang, &source, &flags, &label);
            let text = std::fs::read(&ll).unwrap();
            let options = Options {
                output_kind: OutputKind::Object,
                triple: Some(TRIPLE.into()),
                relocation_model: Some(if pic { RelocModel::Pic } else { RelocModel::Static }),
                ..Options::default()
            };
            let object = hod::compile(&text, &options).unwrap_or_else(|e| panic!("{label}: {e}"));
            let path = scratch.write(&format!("{label}.o"), object);
            verifies(&dwarfdump, &path);
            verified += 1;
        }
    }
    verified
}

/// `program` (C++, `-O0 -g`) runs as `llc`'s and its objects verify.
fn debug_program(program: &str) {
    let Some(lane) = lane() else { return };
    let verified = debug_objects_verify(program);
    let scratch = Scratch::new(&format!("i386-join-e-{program}"));
    let source = data("join_e", program);
    let mut runs = 0;
    for pic in [false, true] {
        let build = Build { pic, link: &["-lstdc++"], all_linkers: true, ..STATIC };
        runs += lane.check(&scratch, program, &Source::C(&source, DEBUG_FLAGS), build);
    }
    assert!(runs > 0 && verified > 0, "nothing was run or verified");
    eprintln!("i386 join E {program}: {verified} objects verify, {runs} runs equal llc's");
}

#[test]
fn debug_info_of_exceptions_and_atomics() {
    debug_program("dbg_eh.cpp");
}

#[test]
fn debug_info_of_varargs_and_structs_by_value() {
    debug_program("dbg_va.cpp");
}

#[test]
fn debug_info_of_a_dynamic_alloca() {
    debug_program("dbg_vla.cpp");
}

// ---- Stage 2: WP-317's SSE / SSE2 with everything already there ----

/// QEMU's features of an SSE2 build (`pentium4`).
const P4: &[&str] = &["mmx", "sse", "sse2"];

/// `program` built as `build` with every LLVM version, static and PIC,
/// plain and under `Stress::ALL`: every run equal to `llc`'s.
fn sse_program(program: &str, source: &Source<'_>, mcpus: &[Option<&str>], link: &[&str]) {
    let Some(lane) = lane() else { return };
    let scratch = Scratch::new(&format!("i386-join-e-{program}"));
    let mut runs = 0;
    for &mcpu in mcpus {
        for pic in [false, true] {
            let build = Build { pic, mcpu, features: P4, link, all_linkers: true, ..STATIC };
            let name = format!("{program}-{}", mcpu.unwrap_or("default"));
            runs += lane.check(&scratch, &name, source, build);
        }
    }
    assert!(runs > 0, "no run was made");
    eprintln!("i386 join E {program}: {runs} runs equal llc's");
}

/// An `.ll` program of `join_e/` whose functions carry their own
/// `target-cpu`, at the default CPU and at `-mcpu` `generic` and `i686`
/// (the attribute wins).
fn ll_program(program: &str, link: &[&str]) {
    let text = std::fs::read_to_string(data("join_e", program)).unwrap();
    sse_program(program, &Source::Ll(&text), &[None, Some("i686")], link);
}

/// A C or C++ program of `join_e/` built by clang with `flags` (SSE2).
fn c_program(program: &str, flags: &[&str], link: &[&str]) {
    let path = data("join_e", program);
    sse_program(program, &Source::C(&path, flags), &[None], link);
}

#[test]
fn invoke_of_sse_results_with_pads_using_them() {
    ll_program("sse_eh.ll", &["-lstdc++"]);
}

#[test]
fn varargs_with_sse_values_and_vectors() {
    c_program("sse_va.c", &["-march=pentium4"], &[]);
}

#[test]
fn aggregates_with_sse_and_vector_leaves_memcpy_and_dynamic_alloca() {
    c_program("sse_agg.c", &["-march=pentium4"], &[]);
}

#[test]
fn float_atomics_in_sse2_functions() {
    ll_program("sse_atomic.ll", &[]);
}

/// The value of an attribute: a reference's quoted name, a location with
/// each `DW_OP_fbreg`'s offset dropped (`spec/dwarf.md` §6.4).
fn value(raw: &str) -> String {
    let raw = raw.trim().trim_start_matches('(').trim_end_matches(')');
    if let Some(start) = raw.find('"') {
        return raw[start..].to_owned();
    }
    let ops: Vec<&str> = raw
        .split(", ")
        .map(|op| if op.starts_with("DW_OP_fbreg ") { "DW_OP_fbreg" } else { op })
        .collect();
    ops.join(", ")
}

/// The DIEs of `--debug-info` text in order (as `i386_debug.rs` reads
/// them): depth, tag, name, type, location, frame base, and whether the
/// code addresses are there.
fn dies(text: &str) -> Vec<String> {
    const VALUES: [&str; 4] = ["DW_AT_name", "DW_AT_type", "DW_AT_location", "DW_AT_frame_base"];
    const PRESENT: [&str; 3] = ["DW_AT_low_pc", "DW_AT_high_pc", "DW_AT_ranges"];
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        if let Some((offset, rest)) = line.split_once(": ")
            && offset.starts_with("0x")
            && !rest.contains("Compile Unit:")
        {
            let tag = rest.trim_start();
            out.push(format!("{} {tag}", rest.len() - tag.len()));
            continue;
        }
        let (Some(last), Some((attr, raw))) =
            (out.last_mut(), line.trim_start().split_once(char::is_whitespace))
        else {
            continue;
        };
        if VALUES.contains(&attr) {
            let _ = write!(last, " {attr}={}", value(raw));
        } else if PRESENT.contains(&attr) {
            let _ = write!(last, " {attr}");
        }
    }
    out
}

/// `llvm-dwarfdump --debug-info` of `object`.
fn debug_info(dwarfdump: &Path, object: &Path) -> String {
    let mut command = Command::new(dwarfdump);
    command.arg("--debug-info").arg(object);
    let outcome = run_with(&mut command, &[], DEFAULT_TIMEOUT);
    assert!(outcome.success(), "llvm-dwarfdump {}", object.display());
    String::from_utf8_lossy(&outcome.stdout).into_owned()
}

#[test]
fn debug_info_of_sse2_functions_equals_llc() {
    let Some(lane) = lane() else { return };
    let path = data("join_e", "sse_dbg.c");
    let flags: &[&str] = &["-g", "-march=pentium4", "-msse2"];
    let scratch = Scratch::new("i386-join-e-sse-dbg");
    let mut compared = 0;
    for major in LLVM_MAJORS {
        let (Some(clang), Some(llc), Some(dwarfdump)) = (
            llvm_tool(major, "clang"),
            llvm_tool(major, "llc"),
            llvm_tool(major, "llvm-dwarfdump"),
        ) else {
            continue;
        };
        for pic in [false, true] {
            let label = format!("sse_dbg-{major}-{}", if pic { "pic" } else { "static" });
            let mut all = flags.to_vec();
            all.push(if pic { "-fPIE" } else { "-fno-pic" });
            let ll = clang_module(&scratch, &clang, &path, &all, &label);
            let model = if pic { "-relocation-model=pic" } else { "-relocation-model=static" };
            let llc_o = llc_object(&scratch, &llc, &ll, &[model], &format!("{label}-llc"));
            let text = std::fs::read(&ll).unwrap();
            let options = Options {
                output_kind: OutputKind::Object,
                triple: Some(TRIPLE.into()),
                relocation_model: Some(if pic { RelocModel::Pic } else { RelocModel::Static }),
                ..Options::default()
            };
            let object = hod::compile(&text, &options).unwrap_or_else(|e| panic!("{label}: {e}"));
            let hod_o = scratch.write(&format!("{label}-hod.o"), object);
            verifies(&dwarfdump, &llc_o);
            verifies(&dwarfdump, &hod_o);
            let h = dies(&debug_info(&dwarfdump, &hod_o));
            let l = dies(&debug_info(&dwarfdump, &llc_o));
            assert!(h.len() > 20 && h.len() == l.len(), "{label}: {h:#?}\n{l:#?}");
            for (h, l) in h.iter().zip(&l) {
                assert_eq!(h, l, "{label}: hod's DIE differs from llc's");
            }
            compared += 1;
        }
    }
    let build = Build { features: P4, ..STATIC };
    let runs = lane.check(&scratch, "sse_dbg.c", &Source::C(&path, flags), build);
    assert!(compared > 0 && runs > 0, "nothing was compared or run");
    eprintln!("i386 join E sse_dbg.c: {compared} DIE trees equal llc's, {runs} runs equal llc's");
}
