//! Stack-passed arguments of a protected function (WP-333,
//! `spec/codegen.md` §5.6.24 rule 10).
//!
//! - Shape, on both targets, of the functions of
//!   `tests/ir/stack_protector_stack_args.ll`: a protected function reads
//!   its stack-passed arguments (the incoming argument area, above the
//!   guard) at entry, before the guard is stored, and never after it; a
//!   `byval` argument's address is taken at entry too. The shapes: 7th to
//!   11th integers (a pointer, an `i32`, a `signext i8`), a `double` and a
//!   `float` beyond the FP registers, an `i128` on the stack, a `byval`
//!   struct, a variadic function (its `va_start` still addresses the
//!   incoming area after the guard), `sspreq` without a buffer, a dynamic
//!   `alloca`.
//! - The same module without the protector attributes still reads the
//!   pointer argument of `@victim` in place after its call: the copy is
//!   made only in a protected function.
//! - The program itself (in-bounds values, then an overflow that reaches
//!   the incoming area and must end in `__stack_chk_fail`'s abort, not in
//!   a store through the overwritten pointer) runs under `xtask irdiff`
//!   against `llc` on both targets.
#![cfg(target_os = "linux")]

use std::path::Path;
use std::time::Duration;

use hod::{Options, OutputKind, Report, Settings};
use hod_testutil::{Scratch, run_with, tool};

const SEED: &str = include_str!("../../../tests/ir/stack_protector_stack_args.ll");

const TARGETS: [(&str, &str); 2] = [
    ("aarch64-unknown-linux-gnu", hod_ir::AARCH64_LINUX_DATA_LAYOUT),
    ("x86_64-unknown-linux-gnu", hod_ir::X86_64_LINUX_DATA_LAYOUT),
];

/// Protected functions whose every stack argument is read before the
/// guard (and none after it).
const COPIED: [&str; 6] = ["ints", "fps", "wide", "req", "dyn", "victim"];

fn compile(body: &str, triple: &str, layout: &str) -> Vec<u8> {
    let text = format!("target datalayout = \"{layout}\"\ntarget triple = \"{triple}\"\n{body}");
    let options = Options { output_kind: OutputKind::Object, ..Options::default() };
    hod::compile_with(text.as_bytes(), &options, &Settings::default(), &mut Report::default())
        .unwrap_or_else(|error| panic!("hod declined ({triple}): {error}"))
}

/// The disassembly of `object`, one `(function, lines)` per function, the
/// relocation lines kept in place.
fn functions(object: &[u8], scratch: &Path, name: &str) -> Option<Vec<(String, Vec<String>)>> {
    let objdump = tool("llvm-objdump")?;
    let path = scratch.join(name);
    std::fs::write(&path, object).expect("write the object");
    let mut command = std::process::Command::new(objdump);
    command.args(["-dr", "--no-show-raw-insn"]).arg(&path);
    let outcome = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "llvm-objdump: {}", outcome.stderr_text());
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for line in outcome.stdout_text().lines() {
        if let Some(name) = line.split_once(" <").and_then(|(_, r)| r.strip_suffix(">:")) {
            out.push((name.to_owned(), Vec::new()));
        } else if let Some((_, lines)) = out.last_mut()
            && !line.trim().is_empty()
        {
            lines.push(line.to_owned());
        }
    }
    Some(out)
}

/// Whether an x86-64 line reads the incoming argument area: a memory
/// operand `0xNN(%rbp)` at 16 or more (above the return address), in
/// anything but a `lea`.
fn x64_reads_incoming(line: &str) -> bool {
    if line.contains("lea") {
        return false;
    }
    let Some(at) = line.find("(%rbp)") else { return false };
    let disp = line[..at].rsplit([' ', '\t', ',']).next().unwrap_or("");
    disp.strip_prefix("0x")
        .and_then(|hex| u64::from_str_radix(hex, 16).ok())
        .is_some_and(|d| d >= 16)
}

/// The AArch64 incoming base slot of a function: the prologue saves the
/// entry stack pointer from `x16` at `[x29, #N]` (§5.5).
fn a64_base_slot(lines: &[String]) -> Option<String> {
    lines.iter().find_map(|line| {
        let (_, rest) = line.split_once("str\tx16, [x29, ")?;
        Some(rest.trim_end_matches(']').to_owned())
    })
}

/// The indices of the lines of `lines` that read the incoming area.
fn incoming_reads(arch: &str, lines: &[String]) -> Vec<usize> {
    let slot = a64_base_slot(lines);
    (0..lines.len())
        .filter(|&i| {
            let line = &lines[i];
            if arch == "x86_64" {
                return x64_reads_incoming(line);
            }
            slot.as_ref().is_some_and(|slot| {
                line.contains("\tldr\t") && line.ends_with(&format!("[x29, {slot}]"))
            })
        })
        .collect()
}

/// The index of the first line that reads the guard value.
fn guard_line(arch: &str, lines: &[String]) -> Option<usize> {
    let mark = if arch == "x86_64" { "%fs:0x28" } else { "__stack_chk_guard" };
    lines.iter().position(|line| line.contains(mark))
}

fn arch_of(triple: &str) -> &str {
    triple.split('-').next().unwrap_or(triple)
}

#[test]
fn a_protected_function_reads_its_stack_arguments_before_the_guard() {
    let scratch = Scratch::new("ssp-stack-args");
    for (triple, layout) in TARGETS {
        let arch = arch_of(triple);
        let object = compile(SEED, triple, layout);
        let Some(functions) = functions(&object, scratch.path(), &format!("{arch}.o")) else {
            return;
        };
        for name in COPIED.iter().chain(&["byval", "va"]) {
            let (_, lines) = functions
                .iter()
                .find(|(f, _)| f == name)
                .unwrap_or_else(|| panic!("{arch}: no function {name}"));
            let guard = guard_line(arch, lines)
                .unwrap_or_else(|| panic!("{arch}: {name} reads no guard:\n{}", lines.join("\n")));
            let reads = incoming_reads(arch, lines);
            if COPIED.contains(name) || *name == "va" {
                assert!(
                    reads.iter().any(|&i| i < guard),
                    "{arch}: {name} copies no stack argument before the guard:\n{}",
                    lines.join("\n")
                );
            }
            // `va_start` addresses the overflow area after the guard.
            if *name != "va" {
                assert!(
                    reads.iter().all(|&i| i < guard),
                    "{arch}: {name} reads the incoming area after the guard:\n{}",
                    lines.join("\n")
                );
            }
        }
    }
}

#[test]
fn an_unprotected_function_reads_its_stack_arguments_in_place() {
    let scratch = Scratch::new("ssp-stack-args-off");
    let plain = SEED.replace(" sspstrong", "").replace(" sspreq", "");
    for (triple, layout) in TARGETS {
        let arch = arch_of(triple);
        let object = compile(&plain, triple, layout);
        assert!(
            !object.windows(16).any(|w| w == b"__stack_chk_fail"),
            "{arch}: an unprotected module calls __stack_chk_fail"
        );
        let Some(functions) = functions(&object, scratch.path(), &format!("{arch}.o")) else {
            return;
        };
        let (_, lines) = functions
            .iter()
            .find(|(f, _)| f == "victim")
            .unwrap_or_else(|| panic!("{arch}: no function victim"));
        let call = lines
            .iter()
            .position(|line| line.contains("\tcall") || line.contains("\tbl\t"))
            .unwrap_or_else(|| panic!("{arch}: victim calls nothing"));
        assert!(
            incoming_reads(arch, lines).iter().any(|&i| i > call),
            "{arch}: unprotected victim no longer reads its pointer in place:\n{}",
            lines.join("\n")
        );
    }
}
