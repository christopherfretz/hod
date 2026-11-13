//! `spec/harness.md` "Cross runs": the target-aware [`Toolchain`] links and
//! runs x86-64 programs on any host, under `qemu-x86_64` with the cross
//! system root and the baseline CPU model when the host is not x86-64.
//! Nothing here needs hod's code generation: the objects come from the
//! cross gcc and g++ and from `llc -mtriple=x86_64-unknown-linux-gnu` (LLVM
//! 19 and 22), and each is linked by GNU ld, lld (22 and 19) and mold, as
//! PIE and not, dynamically and statically. A missing tool skips with a
//! note naming its Debian package, or fails under `HOD_REQUIRE_TOOLS=1`.

#![cfg(target_os = "linux")]

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use common::linking::{Linker, Toolchain, exit_code};
use hod_testutil::{Scratch, Target, find_tool, run_with, tool};

/// The ways a program is linked: PIE (the default of the drivers), not,
/// and statically, with and without PIE.
const MODES: [&[&str]; 4] = [&["-pie"], &["-no-pie"], &["-static"], &["-static-pie"]];

/// `EM_X86_64`.
const EM_X86_64: u16 = 62;

/// The machine an ELF file is for.
fn machine(file: &Path) -> u16 {
    let bytes = std::fs::read(file).unwrap();
    assert!(bytes.starts_with(b"\x7fELF"), "{} is not ELF", file.display());
    u16::from_le_bytes([bytes[18], bytes[19]])
}

/// The matrix's linkers, with lld of LLVM 19 and of 22 instead of the
/// newest lld alone.
fn linkers(tools: &Toolchain, scratch: &Scratch) -> Vec<(String, Linker)> {
    let versioned: Vec<(String, Linker)> = [19, 22]
        .into_iter()
        .filter_map(|major| Toolchain::lld(scratch, major).map(|lld| (format!("lld-{major}"), lld)))
        .collect();
    let mut all = Vec::new();
    for linker in &tools.linkers {
        if linker.name != "lld" || versioned.is_empty() {
            all.push((linker.name.to_owned(), linker.clone()));
        }
    }
    all.extend(versioned);
    all
}

const C_PROGRAM: &str = r#"
#include <stdio.h>
#include <stdlib.h>
static int counter = 40;
int bump(int by) { counter += by; return counter; }
int main(int argc, char **argv) {
    (void)argv;
    double half = argc / 2.0;
    printf("counter=%d half=%.1f\n", bump(2), half);
    char *heap = malloc(64);
    snprintf(heap, 64, "heap %s", "ok");
    puts(heap);
    free(heap);
    return counter;
}
"#;

/// A C program of the cross gcc runs with every linker, in every mode.
#[test]
fn a_c_program_runs_with_every_linker_and_mode() {
    let scratch = Scratch::new("cross-c");
    let Some(tools) = Toolchain::x86_64(&scratch) else { return };
    assert_eq!(tools.target, Target::X86_64);
    let object = tools.compile_c(&scratch, "prog", C_PROGRAM, &["-fPIE"]);
    assert_eq!(machine(&object), EM_X86_64);
    for (label, linker) in linkers(&tools, &scratch) {
        for mode in MODES {
            let program = scratch.join(format!("prog-{label}{}", mode[0]));
            let ran = tools.link_and_run(&linker, mode, &[&object], &program);
            assert_eq!(machine(&program), EM_X86_64, "{label} {mode:?}");
            let shown = format!("{label} {mode:?}: {}", ran.stderr_text());
            assert_eq!(ran.stdout_text(), "counter=42 half=0.5\nheap ok\n", "{shown}");
            assert_eq!(exit_code(&ran), Some(42), "{shown}");
        }
    }
}

const CXX_PROGRAM: &str = r#"
#include <cstdio>
#include <stdexcept>
#include <string>
#include <thread>
thread_local int local = 5;
thread_local std::string name = "main";
static int check(int x) {
    if (x > 3) throw std::runtime_error("too big: " + std::to_string(x));
    return x;
}
int main() {
    try {
        check(7);
    } catch (const std::exception &e) {
        std::printf("caught %s\n", e.what());
    }
    std::thread worker([] {
        local = 9;
        name = "worker";
        try {
            check(local);
        } catch (const std::runtime_error &e) {
            std::printf("%s caught %s\n", name.c_str(), e.what());
        }
    });
    worker.join();
    std::printf("%s local=%d\n", name.c_str(), local);
    return 3;
}
"#;

/// C++ with exceptions (thrown across frames and in a thread) and
/// thread-local variables (one with a dynamic initializer and a
/// destructor) links against the cross libstdc++ and runs.
#[test]
fn cxx_exceptions_and_thread_locals_run() {
    let scratch = Scratch::new("cross-cxx");
    let Some(tools) = Toolchain::x86_64(&scratch) else { return };
    let Some(object) = tools.compile_cxx(&scratch, "cxx", CXX_PROGRAM, &["-fPIE"]) else {
        return;
    };
    let stdcxx = Path::new("-lstdc++");
    for (label, linker) in linkers(&tools, &scratch) {
        for mode in MODES {
            let program = scratch.join(format!("cxx-{label}{}", mode[0]));
            let ran = tools.link_and_run(&linker, mode, &[&object, stdcxx], &program);
            let shown = format!("{label} {mode:?}: {}", ran.stderr_text());
            assert_eq!(
                ran.stdout_text(),
                "caught too big: 7\nworker caught too big: 9\nmain local=5\n",
                "{shown}"
            );
            assert_eq!(exit_code(&ran), Some(3), "{shown}");
        }
    }
}

const IR_PROGRAM: &str = r#"
@.fmt = private unnamed_addr constant [20 x i8] c"%s: %lld / %d = %d\0A\00"
@.who = private unnamed_addr constant [4 x i8] c"llc\00"
@table = global [3 x ptr] [ptr @half, ptr @third, ptr @half]
declare i32 @printf(ptr, ...)
define internal i32 @half(i64 %v) { %d = sdiv i64 %v, 2
  %t = trunc i64 %d to i32
  ret i32 %t }
define internal i32 @third(i64 %v) { %d = sdiv i64 %v, 3
  %t = trunc i64 %d to i32
  ret i32 %t }
define i32 @main() {
  %slot = getelementptr [3 x ptr], ptr @table, i64 0, i64 1
  %f = load ptr, ptr %slot
  %r = call i32 %f(i64 -129)
  call i32 (ptr, ...) @printf(ptr @.fmt, ptr @.who, i64 -129, i32 3, i32 %r)
  %e = add i32 %r, 50
  ret i32 %e
}
"#;

/// `llc` of LLVM `major` for `x86_64-unknown-linux-gnu`, with the oracle's
/// arguments.
fn llc_object(scratch: &Scratch, major: u32, ir: &Path, pic: bool) -> Option<PathBuf> {
    let llc = find_tool(&format!("llc-{major}"))?;
    let output = scratch.join(format!("llc{major}-{}.o", if pic { "pic" } else { "static" }));
    let mut command = Command::new(llc);
    command.args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj"]);
    command.arg("-mtriple=x86_64-unknown-linux-gnu");
    command.arg(if pic { "-relocation-model=pic" } else { "-relocation-model=static" });
    command.arg("-o").arg(&output).arg(ir);
    let outcome = run_with(&mut command, &[], Duration::from_mins(5));
    assert!(outcome.success(), "llc-{major}: {}", outcome.stderr_text());
    Some(output)
}

/// Objects of `llc -mtriple=x86_64-unknown-linux-gnu`, LLVM 19 and 22,
/// link with every linker (PIC as PIE, static as not) and run.
#[test]
fn llc_objects_run() {
    let scratch = Scratch::new("cross-llc");
    let Some(tools) = Toolchain::x86_64(&scratch) else { return };
    let ir = scratch.write("prog.ll", IR_PROGRAM);
    let linkers = linkers(&tools, &scratch);
    let mut built = 0;
    for major in [19, 22] {
        let label = format!("llc-{major}");
        if tool(&label).is_none() {
            continue;
        }
        for (pic, flags) in [(true, &["-pie"][..]), (false, &["-no-pie"][..])] {
            let object = llc_object(&scratch, major, &ir, pic).unwrap();
            assert_eq!(machine(&object), EM_X86_64);
            built += 1;
            for (name, linker) in &linkers {
                let program = scratch.join(format!("llc{major}-{pic}-{name}"));
                let ran = tools.link_and_run(linker, flags, &[&object], &program);
                let shown = format!("LLVM {major} {name} {flags:?}: {}", ran.stderr_text());
                assert_eq!(ran.stdout_text(), "llc: -129 / 3 = -43\n", "{shown}");
                assert_eq!(exit_code(&ran), Some(7), "{shown}");
            }
        }
    }
    assert!(built > 0 || !hod_testutil::tools_required());
}

const CPU_PROBE: &str = r#"
#include <cpuid.h>
#include <stdio.h>
int main(int argc, char **argv) {
    (void)argv;
    unsigned a, b, c, d;
    __get_cpuid(1, &a, &b, &c, &d);
    printf("sse2=%d sse3=%d cx16=%d popcnt=%d\n", !!(d & bit_SSE2), !!(c & bit_SSE3),
           !!(c & bit_CMPXCHG16B), !!(c & bit_POPCNT));
    if (argc > 1) {
        unsigned r;
        __asm__ volatile("popcnt %1, %0" : "=r"(r) : "r"((unsigned)argc));
        printf("popcnt ran: %u\n", r);
    }
    return 0;
}
"#;

/// The emulator models the x86-64 baseline (`x86-64-v1`): SSE2 and
/// nothing after it, and an instruction of a later feature is an illegal
/// instruction, as on such a machine.
#[test]
fn the_emulated_cpu_is_the_baseline() {
    let scratch = Scratch::new("cross-cpu");
    let Some(tools) = Toolchain::x86_64(&scratch) else { return };
    if !tools.emulated() {
        eprintln!("note: x86-64 programs run natively here; the CPU is the host's");
        return;
    }
    let object = tools.compile_c(&scratch, "probe", CPU_PROBE, &["-fPIE"]);
    let linker = &tools.linkers[0];
    let program = scratch.join("probe");
    let ran = tools.link_and_run(linker, &[], &[&object], &program);
    assert_eq!(ran.stdout_text(), "sse2=1 sse3=0 cx16=0 popcnt=0\n", "{}", ran.stderr_text());
    // With an argument the probe executes `popcnt`.
    let mut command = tools.runner_command(&program);
    command.arg("popcnt");
    let ran = run_with(&mut command, &[], Duration::from_mins(5));
    let signal = ran.status.and_then(|status| {
        use std::os::unix::process::ExitStatusExt;
        status.signal().or_else(|| status.code().map(|code| code - 128))
    });
    assert_eq!(signal, Some(4), "SIGILL expected: {ran:?}");
}
