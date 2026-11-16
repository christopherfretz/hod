//! WP-315 (`spec/codegen-x86.md` §13, `spec/landed/codegen/WP-315.md`):
//! the memory intrinsics with `i32` and `i64` lengths, integers of 33 to
//! 63 bits by value, and `llc`'s choice between an inline expansion and
//! a libc call (the x87 barrier, `hod_x86::bytes::calls_libc`).
//!
//! Programs under `tests/data/i386/mem/`: `mem.c` through clang 19 and
//! 22 at CPU `i586` (no `cmov`) and clang's default (`i686`), `wide.ll`
//! written by hand at CPU `generic` and `i686`, with `helper.c` built by
//! the cross C compiler. Each compiled by `llc`, hod and `hod_x86` under
//! `hod_cg::Stress::ALL`, static and PIE, linked by GNU ld (PIE also by
//! lld and mold), run under `qemu-i386`: the outputs are equal.
//!
//! A missing tool is skipped with a note, unless `HOD_REQUIRE_TOOLS=1`.

use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use hod::{Options, OutputKind, RelocModel};
use hod_testutil::target::Target;
use hod_testutil::{LLVM_MAJORS, Scratch, linkers};

mod i386_support;
use i386_support::{
    TRIPLE, back_end_object, checked, clang_module, llc_object, llvm_tool, run, tool,
};

fn data(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/i386/mem").join(path)
}

/// hod's object through the facade and `hod_x86`'s under `Stress::ALL`.
fn hod_objects(text: &[u8], pic: bool, label: &str) -> [Vec<u8>; 2] {
    let options = Options {
        output_kind: OutputKind::Object,
        triple: Some(TRIPLE.into()),
        relocation_model: Some(if pic { RelocModel::Pic } else { RelocModel::Static }),
        ..Options::default()
    };
    let plain = hod::compile(text, &options).unwrap_or_else(|e| panic!("hod {label}: {e}"));
    let stress = hod_cg::Options {
        reloc_model: if pic { hod_cg::RelocModel::Pic } else { hod_cg::RelocModel::Static },
        stress: hod_cg::Stress::ALL,
        ..hod_cg::Options::default()
    };
    [plain, back_end_object(text, &stress)]
}

/// Runs `ll` (static or PIE) by `llc` and hod with every linker that
/// applies, against `llc`'s run with GNU ld; `extra` objects linked too.
/// Answers the runs compared.
fn run_module(
    scratch: &Scratch,
    llc: &Path,
    ll: &Path,
    pic: bool,
    extra: &[PathBuf],
    label: &str,
) -> usize {
    let target = Target::I686;
    let (Some(gcc), Some(qemu)) = (tool(&target.c_driver()), tool(&target.emulator())) else {
        return 0;
    };
    let text = std::fs::read(ll).unwrap();
    let reloc: &[&str] = if pic { &["-relocation-model=pic"] } else { &[] };
    let [plain, stress] = hod_objects(&text, pic, label);
    let objects = [
        ("llc", llc_object(scratch, llc, ll, reloc, &format!("{label}-llc"))),
        ("hod", scratch.write(&format!("{label}-hod.o"), plain)),
        ("stress", scratch.write(&format!("{label}-stress.o"), stress)),
    ];
    let present = linkers::present(target);
    let mut reference = None;
    let mut runs = 0;
    for (who, object) in &objects {
        for linker in &present {
            if !pic && linker.0 != "bfd" {
                continue;
            }
            let exe = scratch.join(format!("{label}-{who}-{}", linker.0));
            let mut command = Command::new(&gcc);
            let driver: Vec<OsString> = linkers::driver_flags(scratch.path(), linker.0, &linker.1);
            command.args(driver).arg(if pic { "-pie" } else { "-no-pie" }).arg(object).args(extra);
            command.args(["-lm", "-o"]).arg(&exe);
            checked(&format!("link {label} {who} {}", linker.0), &mut command);
            let result = run(&qemu, &exe);
            let reference = reference.get_or_insert_with(|| result.clone());
            assert_eq!(result.0, Some(0), "{label}: {who} with {}: status", linker.0);
            assert!(result.1.ends_with(b"done\n"), "{label}: {who} did not finish");
            if result.1 != reference.1 {
                let (want, got) =
                    (String::from_utf8_lossy(&reference.1), String::from_utf8_lossy(&result.1));
                let diff: Vec<_> = want
                    .lines()
                    .zip(got.lines())
                    .filter(|(a, b)| a != b)
                    .take(12)
                    .map(|(a, b)| format!("  llc {a}\n  {who} {b}"))
                    .collect();
                panic!("{label}: {who} with {} differs from llc:\n{}", linker.0, diff.join("\n"));
            }
            runs += 1;
        }
    }
    runs
}

#[test]
fn memory_intrinsics_and_wide_integers_by_value_run_as_llc_s() {
    let target = Target::I686;
    let Some(gcc) = tool(&target.c_driver()) else { return };
    let scratch = Scratch::new("i386-mem");
    let mut helpers = Vec::new();
    for pic in [false, true] {
        let out = scratch.join(format!("helper-{pic}.o"));
        let mut command = Command::new(&gcc);
        command.args(["-c", "-O1", if pic { "-fPIE" } else { "-fno-pic" }]).arg(data("helper.c"));
        checked("helper", command.arg("-o").arg(&out));
        helpers.push(out);
    }
    let (mut modules, mut runs) = (0, 0);
    for major in LLVM_MAJORS {
        let (Some(clang), Some(llc)) = (llvm_tool(major, "clang"), llvm_tool(major, "llc")) else {
            continue;
        };
        for pic in [false, true] {
            let kind = if pic { "pie" } else { "exe" };
            for (cpu, flags) in [("i586", &["-march=i586"][..]), ("i686", &[][..])] {
                let label = format!("mem-{major}-{cpu}-{kind}");
                let mut all = vec![if pic { "-fPIE" } else { "-fno-pic" }];
                all.extend_from_slice(flags);
                let ll = clang_module(&scratch, &clang, &data("mem.c"), &all, &label);
                runs += run_module(&scratch, &llc, &ll, pic, &[], &label);
                modules += 1;
            }
            for cpu in ["generic", "i686"] {
                let label = format!("wide-{major}-{cpu}-{kind}");
                let mut text = std::fs::read_to_string(data("wide.ll")).unwrap();
                if cpu != "generic" {
                    text = text.replace(
                        "attributes #0 = {",
                        &format!("attributes #0 = {{ \"target-cpu\"=\"{cpu}\""),
                    );
                }
                let ll = scratch.write(&format!("{label}.ll"), text);
                runs += run_module(
                    &scratch,
                    &llc,
                    &ll,
                    pic,
                    &helpers[usize::from(pic)..=usize::from(pic)],
                    &label,
                );
                modules += 1;
            }
        }
    }
    eprintln!("i386 mem: {modules} modules, {runs} runs equal to llc's");
}

/// `hod_x86::bytes::calls_libc` against `llc` 19 and 22 over a grid of
/// lengths, alignments, the volatile flag and the three operations
/// (`llvm.memcpy`, `memmove`, `memset` of a constant byte), at CPU
/// `generic` and `i686`: a function of `llc`'s holds a `call` exactly
/// where the rule says so.
#[test]
fn the_libc_call_rule_is_llc_s() {
    use hod_x86::bytes::{MemOp, calls_libc};
    let mut text = String::from("target triple = \"i686-unknown-linux-gnu\"\n");
    let mut expect = Vec::new();
    let lens: Vec<u64> = (0..=140).chain([200, 255, 256, 257, 1000, 1 << 32 | 8]).collect();
    for (op, name) in [(MemOp::Copy, "cpy"), (MemOp::Move, "mov"), (MemOp::Set, "set")] {
        for align in [1u64, 2, 4, 8, 16] {
            for volatile in [false, true] {
                for &n in &lens {
                    let f = format!("{name}_{n}_{align}_{volatile}");
                    let call = match op {
                        MemOp::Copy | MemOp::Move => format!(
                            "call void @llvm.mem{}.p0.p0.i64(ptr align {align} %d, ptr align {align} %s, i64 {n}, i1 {volatile})",
                            if op == MemOp::Copy { "cpy" } else { "move" }
                        ),
                        MemOp::Set => format!(
                            "call void @llvm.memset.p0.i64(ptr align {align} %d, i8 7, i64 {n}, i1 {volatile})"
                        ),
                    };
                    writeln!(text, "define void @{f}(ptr %d, ptr %s) {{\n  {call}\n  ret void\n}}")
                        .unwrap();
                    expect.push((f, calls_libc(op, Some(n), align, volatile)));
                }
            }
        }
    }
    let scratch = Scratch::new("i386-mem-rule");
    let ll = scratch.write("grid.ll", text);
    let mut checked_with = 0;
    for major in LLVM_MAJORS {
        let Some(llc) = llvm_tool(major, "llc") else { continue };
        for cpu in ["generic", "i686"] {
            let mut command = Command::new(&llc);
            command.args([
                "-O0",
                "-global-isel=0",
                "-fast-isel=0",
                &format!("-mcpu={cpu}"),
                "-o",
                "-",
            ]);
            let out = checked("llc grid", command.arg(&ll));
            let asm = String::from_utf8_lossy(&out.stdout);
            let mut calls = std::collections::HashMap::new();
            let mut current = String::new();
            for line in asm.lines() {
                let label = line.split_once(':').map(|(name, _)| name);
                if let Some(name) =
                    label.filter(|n| !n.is_empty() && !n.starts_with(['.', ' ', '\t', '#']))
                {
                    current = name.to_string();
                    calls.insert(current.clone(), false);
                } else if line.trim_start().starts_with("call") {
                    calls.insert(current.clone(), true);
                }
            }
            for (f, want) in &expect {
                assert_eq!(calls.get(f), Some(want), "llc {major} -mcpu={cpu}: {f}");
            }
            checked_with += 1;
        }
    }
    eprintln!("calls_libc: {} shapes equal llc's in {checked_with} runs", expect.len());
}
