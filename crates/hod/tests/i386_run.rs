//! WP-293, design 0008 milestone A: integer-only static programs compiled
//! by hod for `i686-unknown-linux-gnu`, linked by GNU ld, lld and mold
//! with the cross C library, and run under `qemu-i386`: the exit status
//! and the output equal those of the same module compiled by `llc` 19 and
//! 22 (`-O0 -global-isel=0 -fast-isel=0`). Each C program is compiled to
//! IR by the clang of the same major, without PIC (hod's i386 back end is
//! static only, `x86:pic`).
//!
//! Each program is also compiled under `hod_cg::Stress::ALL` (the
//! `min_pool` stress mode among them) and must run the same.
//!
//! A missing tool is skipped with a note, unless `HOD_REQUIRE_TOOLS=1`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use hod::{Options, OutputKind};
use hod_testutil::target::Target;
use hod_testutil::{
    DEFAULT_TIMEOUT, LLVM_MAJORS, Outcome, Scratch, linkers, run_with, tools_required,
};

const TRIPLE: &str = "i686-unknown-linux-gnu";

/// The programs of `tests/data/i386`.
const PROGRAMS: [&str; 5] = ["loops.c", "switch.c", "calls.c", "memory.c", "bytes.c"];

/// The least module: `main` returns 42.
const RET42: &str =
    "target triple = \"i686-unknown-linux-gnu\"\n\ndefine i32 @main() {\n  ret i32 42\n}\n";

fn llvm_tool(major: u32, name: &str) -> Option<PathBuf> {
    let path = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/{name}"));
    if path.exists() {
        return Some(path);
    }
    assert!(!tools_required(), "no {name} {major} (HOD_REQUIRE_TOOLS=1)");
    eprintln!("note: no {name} {major}; skipping its part");
    None
}

fn tool(name: &str) -> Option<PathBuf> {
    let found = hod_testutil::tool(name);
    if found.is_none() {
        assert!(!tools_required(), "no {name} (HOD_REQUIRE_TOOLS=1)");
        eprintln!("note: no {name}; skipping");
    }
    found
}

fn checked(what: &str, command: &mut Command) -> Outcome {
    let outcome = run_with(command, &[], DEFAULT_TIMEOUT);
    assert!(outcome.success(), "{what}: {}", outcome.stderr_text());
    outcome
}

/// The module of `program` by clang `major`, in text.
fn module(scratch: &Scratch, clang: &Path, major: u32, program: &str) -> PathBuf {
    let out = scratch.join(format!("{program}-{major}.ll"));
    if program == "ret42" {
        std::fs::write(&out, RET42).unwrap();
        return out;
    }
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/i386").join(program);
    let mut command = Command::new(clang);
    command.args(["--target=i686-linux-gnu", "-O0", "-fno-pic", "-fno-pie", "-S", "-emit-llvm"]);
    command.arg("-o").arg(&out).arg(source);
    checked(&format!("clang {major} {program}"), &mut command);
    out
}

/// What running `exe` under `qemu-i386` did: its status and output.
fn run(qemu: &Path, exe: &Path) -> (Option<i32>, Vec<u8>) {
    let target = Target::I686;
    let mut command = Command::new(qemu);
    command.args(target.emulator_args(&target.sysroot())).arg(exe);
    let outcome = run_with(&mut command, &[], DEFAULT_TIMEOUT);
    assert!(!outcome.timed_out(), "{} timed out", exe.display());
    (outcome.status.and_then(|s| s.code()), outcome.stdout)
}

/// `hod_x86`'s object of `text` under `Stress::ALL` (`min_pool`, far
/// branches, displaced registers), from the back end directly.
fn stressed_object(text: &[u8]) -> Vec<u8> {
    let parsed = hod_ll::parse_module(text, &hod_ll::ParseOptions::default())
        .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    let side = parsed.text;
    let mut module = parsed.module;
    let mut bodies = Vec::new();
    hod_ll::parse_bodies(&mut module, &side, |_, read| {
        bodies.push(read.body.expect("every body is read").clone());
        Ok::<(), hod_ll::LlError>(())
    })
    .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    let options = hod_cg::Options { stress: hod_cg::Stress::ALL, ..hod_cg::Options::default() };
    hod_x86::compile_to_object(&module, &bodies, &options).unwrap_or_else(|e| panic!("stress: {e}"))
}

/// Each program built by `llc`, by hod, and by `hod_x86` under every
/// stress option; the three runs agree under every linker.
#[test]
fn milestone_a_programs_run_as_llc_s() {
    let target = Target::I686;
    let (Some(gcc), Some(qemu)) = (tool(&target.c_driver()), tool(&target.emulator())) else {
        return;
    };
    let linkers = linkers::present(target);
    for name in ["bfd", "lld", "mold"] {
        assert!(
            !tools_required() || linkers.iter().any(|(n, _)| *n == name),
            "linker {name} missing (HOD_REQUIRE_TOOLS=1)"
        );
    }
    let scratch = Scratch::new("i386-run");
    let mut runs = 0;
    for major in LLVM_MAJORS {
        let (Some(clang), Some(llc)) = (llvm_tool(major, "clang"), llvm_tool(major, "llc")) else {
            continue;
        };
        for program in std::iter::once("ret42").chain(PROGRAMS) {
            let ll = module(&scratch, &clang, major, program);
            let options = Options {
                output_kind: OutputKind::Object,
                triple: Some(TRIPLE.into()),
                ..Options::default()
            };
            let object = hod::compile(&std::fs::read(&ll).unwrap(), &options)
                .unwrap_or_else(|e| panic!("hod {program} ({major}): {e}"));
            let hod_o = scratch.write(&format!("{program}-{major}-hod.o"), object);
            let stressed = stressed_object(&std::fs::read(&ll).unwrap());
            let stress_o = scratch.write(&format!("{program}-{major}-stress.o"), stressed);
            let llc_o = scratch.join(format!("{program}-{major}-llc.o"));
            let mut command = Command::new(&llc);
            command.args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj"]);
            command.arg(format!("-mtriple={TRIPLE}")).arg(&ll).arg("-o").arg(&llc_o);
            checked(&format!("llc {major} {program}"), &mut command);
            for (linker, program_path) in &linkers {
                let mut results = Vec::new();
                for (who, object) in [("llc", &llc_o), ("hod", &hod_o), ("stress", &stress_o)] {
                    let exe = scratch.join(format!("{program}-{major}-{who}-{linker}"));
                    let mut command = Command::new(&gcc);
                    let flags: Vec<OsString> =
                        linkers::driver_flags(scratch.path(), linker, program_path);
                    command.args(flags).arg("-no-pie").arg(object).arg("-o").arg(&exe);
                    checked(&format!("{linker} link of {program} ({who}, {major})"), &mut command);
                    results.push(run(&qemu, &exe));
                }
                for (who, result) in ["hod", "stress"].iter().zip(&results[1..]) {
                    assert_eq!(
                        &results[0], result,
                        "{program} (clang {major}, {linker}): {who}'s run differs from llc's"
                    );
                }
                runs += 1;
            }
        }
    }
    eprintln!("i386: {runs} program runs equal llc's");
}
