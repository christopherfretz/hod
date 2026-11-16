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
//!
//! A missing tool is skipped with a note, unless `HOD_REQUIRE_TOOLS=1`.

use std::path::Path;
use std::process::Command;

use hod::{Options, OutputKind, RelocModel};
use hod_testutil::{DEFAULT_TIMEOUT, LLVM_MAJORS, Scratch, run_with};

mod i386_i11b;
mod i386_support;
use i386_i11b::{Build, STATIC, Source, data, lane};
use i386_support::{TRIPLE, clang_module, llvm_tool};

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
