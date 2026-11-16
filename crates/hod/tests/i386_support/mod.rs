//! Helpers shared by the i386 run tests (`tests/i386_*.rs`): programs
//! compiled for `i686-unknown-linux-gnu` by `llc` and by hod, linked with
//! the cross C library and run under `qemu-i386` (design 0008 section 4).
//! A test file takes them with `mod i386_support;`.
//!
//! A missing tool is skipped with a note, unless `HOD_REQUIRE_TOOLS=1`.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

use hod_testutil::target::Target;
use hod_testutil::{DEFAULT_TIMEOUT, Outcome, Scratch, run_with, tools_required};

/// The triple every i386 run test compiles for.
pub const TRIPLE: &str = "i686-unknown-linux-gnu";

/// The LLVM tool `name` of `major`, or `None` with a note.
pub fn llvm_tool(major: u32, name: &str) -> Option<PathBuf> {
    let path = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/{name}"));
    if path.exists() {
        return Some(path);
    }
    assert!(!tools_required(), "no {name} {major} (HOD_REQUIRE_TOOLS=1)");
    eprintln!("note: no {name} {major}; skipping its part");
    None
}

/// The tool `name` on the search path, or `None` with a note.
pub fn tool(name: &str) -> Option<PathBuf> {
    let found = hod_testutil::tool(name);
    if found.is_none() {
        assert!(!tools_required(), "no {name} (HOD_REQUIRE_TOOLS=1)");
        eprintln!("note: no {name}; skipping");
    }
    found
}

/// Runs `command` under the harness's limits; it must succeed.
pub fn checked(what: &str, command: &mut Command) -> Outcome {
    let outcome = run_with(command, &[], DEFAULT_TIMEOUT);
    assert!(outcome.success(), "{what}: {}", outcome.stderr_text());
    outcome
}

/// The module of `source` (C or C++) by `clang` for [`TRIPLE`] at `-O0`,
/// in text, as `<name>.ll` in `scratch`. `flags` are the caller's
/// (`-fno-pic`, `-fPIE`, `-march=pentium4`, ...).
pub fn clang_module(
    scratch: &Scratch,
    clang: &Path,
    source: &Path,
    flags: &[&str],
    name: &str,
) -> PathBuf {
    let out = scratch.join(format!("{name}.ll"));
    let mut command = Command::new(clang);
    command.args(["--target=i686-linux-gnu", "-O0", "-S", "-emit-llvm"]).args(flags);
    command.arg("-o").arg(&out).arg(source);
    checked(&format!("clang {name}"), &mut command);
    out
}

/// `llc`'s object of the module `ll` for [`TRIPLE`] with the reference
/// options (`-O0 -global-isel=0 -fast-isel=0`) and `flags`
/// (`-relocation-model=pic`, `-mcpu=...`), as `<name>.o` in `scratch`.
pub fn llc_object(scratch: &Scratch, llc: &Path, ll: &Path, flags: &[&str], name: &str) -> PathBuf {
    let out = scratch.join(format!("{name}.o"));
    let mut command = Command::new(llc);
    command.args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj"]).args(flags);
    command.arg(format!("-mtriple={TRIPLE}")).arg(ll).arg("-o").arg(&out);
    checked(&format!("llc {name}"), &mut command);
    out
}

/// What running `exe` under `qemu-i386` did: its status and output.
pub fn run(qemu: &Path, exe: &Path) -> (Option<i32>, Vec<u8>) {
    let target = Target::I686;
    let mut command = Command::new(qemu);
    command.args(target.emulator_args(&target.sysroot())).arg(exe);
    let outcome = run_with(&mut command, &[], DEFAULT_TIMEOUT);
    assert!(!outcome.timed_out(), "{} timed out", exe.display());
    (outcome.status.and_then(|s| s.code()), outcome.stdout)
}

/// The QEMU CPU features a program built for `cpu` needs: `"p4"`
/// (`-march=pentium4`, whose code `llc` writes with SSE2) gets `mmx`,
/// `sse`, `sse2` on top of [`run`]'s CPU; any other name none.
pub fn cpu_features(cpu: &str) -> &'static [&'static str] {
    if cpu == "p4" { &["mmx", "sse", "sse2"] } else { &[] }
}

/// What running `exe` under `qemu-i386` with the extra CPU `features`
/// did ([`run`] when there are none): its status and output.
pub fn run_with_features(qemu: &Path, exe: &Path, features: &[&str]) -> (Option<i32>, Vec<u8>) {
    if features.is_empty() {
        return run(qemu, exe);
    }
    let target = Target::I686;
    let mut command = Command::new(qemu);
    command.args(target.emulator_args_with(&target.sysroot(), features)).arg(exe);
    let outcome = run_with(&mut command, &[], DEFAULT_TIMEOUT);
    assert!(!outcome.timed_out(), "{} timed out", exe.display());
    (outcome.status.and_then(|s| s.code()), outcome.stdout)
}

/// `hod_x86`'s object of `text` under `options`, from the back end
/// directly (the facade has no stress options).
pub fn back_end_object(text: &[u8], options: &hod_cg::Options) -> Vec<u8> {
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
    hod_x86::compile_to_object(&module, &bodies, options).unwrap_or_else(|e| panic!("hod_x86: {e}"))
}

/// `hod_x86`'s object of `text` under `Stress::ALL` (`min_pool`, far
/// branches, displaced registers).
pub fn stressed_object(text: &[u8]) -> Vec<u8> {
    let options = hod_cg::Options { stress: hod_cg::Stress::ALL, ..hod_cg::Options::default() };
    back_end_object(text, &options)
}
