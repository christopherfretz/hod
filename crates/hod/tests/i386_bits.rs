//! WP-309: the integer intrinsics of 1 to 32 bits on i386 (`bswap`,
//! `ctpop`, `ctlz`, `cttz`, `bitreverse`, `abs`, min/max, saturating
//! add/sub/shift, funnel shifts, `scmp`/`ucmp`). Programs under
//! `tests/data/i386/bits/` (`unary.ll`, `binary.ll`, `funnel.ll`,
//! `compare.ll`, all written by `gen.py` there) load their operands as
//! `i32` and truncate them to the width under test (1, 7, 8, 16, 24, 31,
//! 32), so the bits above the width are set, and print every result in
//! hex. Each is built with no `target-cpu` (`generic`: no `cmov`), with
//! `pentium4` and with `haswell` (`popcnt`, `lzcnt`, `bmi`, which `llc`
//! uses), position-dependent and as a PIE, by `llc` 19 and 22 and by hod
//! (plain and under `Stress::ALL`); the runs under `qemu-i386` must print
//! the same bytes.

use std::path::{Path, PathBuf};
use std::process::Command;

use hod::{Options, OutputKind, RelocModel};
use hod_testutil::target::Target;
use hod_testutil::{LLVM_MAJORS, Scratch};

mod i386_support;
use i386_support::{
    TRIPLE, back_end_object, checked, cpu_features, llc_object, llvm_tool, run_with_features, tool,
};

/// The programs.
const PROGRAMS: [&str; 4] = ["unary.ll", "binary.ll", "funnel.ll", "compare.ll"];

/// The CPUs: none named, `pentium4`, `haswell`.
const CPUS: [&str; 3] = ["generic", "p4", "haswell"];

/// The QEMU features `haswell`'s code may use (`llc` writes `popcnt`,
/// `lzcnt`, `tzcnt` for these intrinsics).
const HASWELL: [&str; 14] = [
    "mmx", "sse", "sse2", "sse3", "ssse3", "sse4.1", "sse4.2", "popcnt", "abm", "bmi1", "bmi2",
    "movbe", "avx", "avx2",
];

fn data(program: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/i386/bits").join(program)
}

/// `text` for CPU `cpu`: as it is, or with its `"target-cpu"` on every
/// function.
fn for_cpu(text: &str, cpu: &str) -> String {
    let name = match cpu {
        "generic" => return text.to_owned(),
        "p4" => "pentium4",
        other => other,
    };
    let out =
        text.replace("attributes #0 = {", &format!("attributes #0 = {{ \"target-cpu\"=\"{name}\""));
    assert_ne!(out, text, "no attribute group #0");
    out
}

fn features(cpu: &str) -> &'static [&'static str] {
    if cpu == "haswell" { &HASWELL } else { cpu_features(cpu) }
}

/// hod's object of `text` through the facade, and `hod_x86`'s under
/// `Stress::ALL`.
fn hod_objects(text: &[u8], pic: bool, label: &str) -> (Vec<u8>, Vec<u8>) {
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
    (plain, back_end_object(text, &stress))
}

/// The first line where `got` differs from `want`, for the message.
fn first_difference(want: &[u8], got: &[u8]) -> String {
    let (want, got) = (String::from_utf8_lossy(want), String::from_utf8_lossy(got));
    let mut lines = want.lines().zip(got.lines()).enumerate();
    match lines.find(|(_, (a, b))| a != b) {
        Some((n, (a, b))) => format!("line {}: llc {a:?}, hod {b:?}", n + 1),
        None => format!(
            "lengths: llc {} lines, hod {} lines",
            want.lines().count(),
            got.lines().count()
        ),
    }
}

#[test]
fn bits_programs_run_as_llc_s() {
    let target = Target::I686;
    let (Some(gcc), Some(qemu)) = (tool(&target.c_driver()), tool(&target.emulator())) else {
        return;
    };
    let scratch = Scratch::new("i386-bits");
    let (mut programs, mut runs, mut lines) = (0, 0, 0);
    for major in LLVM_MAJORS {
        let Some(llc) = llvm_tool(major, "llc") else { continue };
        for program in PROGRAMS {
            let source = std::fs::read_to_string(data(program)).unwrap();
            for cpu in CPUS {
                for pic in [false, true] {
                    let name =
                        format!("{program}-{major}-{cpu}-{}", if pic { "pie" } else { "exe" });
                    let ll = scratch.write(&format!("{name}.ll"), for_cpu(&source, cpu));
                    let text = std::fs::read(&ll).unwrap();
                    let (plain, stress) = hod_objects(&text, pic, &name);
                    let reloc: &[&str] = if pic { &["-relocation-model=pic"] } else { &[] };
                    let objects = [
                        ("llc", llc_object(&scratch, &llc, &ll, reloc, &format!("{name}-llc"))),
                        ("hod", scratch.write(&format!("{name}-hod.o"), plain)),
                        ("stress", scratch.write(&format!("{name}-stress.o"), stress)),
                    ];
                    programs += 1;
                    let mut results = Vec::new();
                    for (who, object) in &objects {
                        let exe = scratch.join(format!("{name}-{who}"));
                        let mut command = Command::new(&gcc);
                        command.arg(if pic { "-pie" } else { "-no-pie" });
                        command.arg(object).arg("-o").arg(&exe);
                        checked(&format!("link of {name} ({who})"), &mut command);
                        results.push(run_with_features(&qemu, &exe, features(cpu)));
                        runs += 1;
                    }
                    assert!(
                        results[0].1.ends_with(b"done\n"),
                        "{name}: llc's run did not finish: {}",
                        first_difference(&results[0].1, b"")
                    );
                    lines += String::from_utf8_lossy(&results[0].1).lines().count();
                    for (who, result) in ["hod", "stress"].iter().zip(&results[1..]) {
                        assert!(
                            results[0].1 == result.1,
                            "{name}: {who}'s output differs from llc's at {}",
                            first_difference(&results[0].1, &result.1)
                        );
                        assert_eq!(results[0].0, result.0, "{name}: {who}'s status");
                    }
                }
            }
        }
    }
    eprintln!("i386 bits: {programs} programs, {runs} runs, {lines} lines, hod's equal llc's");
}

/// The error of hod on the module `body` (functions of a module for
/// i686), or `None` when it compiles.
fn refusal(body: &str) -> Option<String> {
    let text = format!("target triple = \"{TRIPLE}\"\n{body}\n");
    let options = Options {
        output_kind: OutputKind::Object,
        triple: Some(TRIPLE.into()),
        ..Options::default()
    };
    hod::compile(text.as_bytes(), &options).err().map(|e| e.to_string())
}

/// Each new lowering compiles alone, with its operands in arguments and
/// constants (no other construct in the module).
#[test]
fn narrow_intrinsics_compile() {
    let cases = [
        "define i32 @f(i32 %a) {\n  %r = call i32 @llvm.ctlz.i32(i32 %a, i1 false)\n  \
         ret i32 %r\n}\ndeclare i32 @llvm.ctlz.i32(i32, i1)",
        "define i16 @f(i16 %a) {\n  %r = call i16 @llvm.bswap.i16(i16 %a)\n  \
         ret i16 %r\n}\ndeclare i16 @llvm.bswap.i16(i16)",
        "define i8 @f(i8 %a, i8 %b, i8 %c) {\n  %r = call i8 @llvm.fshl.i8(i8 %a, i8 %b, i8 %c)\n  \
         ret i8 %r\n}\ndeclare i8 @llvm.fshl.i8(i8, i8, i8)",
        "define i32 @f(i32 %a) {\n  %r = call i32 @llvm.fshr.i32(i32 %a, i32 %a, i32 %a)\n  \
         ret i32 %r\n}\ndeclare i32 @llvm.fshr.i32(i32, i32, i32)",
        "define i24 @f(i24 %a) {\n  %r = call i24 @llvm.sadd.sat.i24(i24 %a, i24 -5)\n  \
         ret i24 %r\n}\ndeclare i24 @llvm.sadd.sat.i24(i24, i24)",
        "define i8 @f(i16 %a, i16 %b) {\n  %r = call i8 @llvm.scmp.i8.i16(i16 %a, i16 %b)\n  \
         ret i8 %r\n}\ndeclare i8 @llvm.scmp.i8.i16(i16, i16)",
        "define void @f(i32 %a, i32 %b, ptr %p) {\n  \
         %r = call i64 @llvm.ucmp.i64.i32(i32 %a, i32 %b)\n  store i64 %r, ptr %p\n  \
         ret void\n}\ndeclare i64 @llvm.ucmp.i64.i32(i32, i32)",
        "define i7 @f(i7 %a) {\n  %r = call i7 @llvm.ushl.sat.i7(i7 3, i7 %a)\n  \
         ret i7 %r\n}\ndeclare i7 @llvm.ushl.sat.i7(i7, i7)",
    ];
    for case in &cases {
        assert_eq!(refusal(case), None, "{case}");
    }
}

/// What stays `x86:intrinsic`: `scmp`/`ucmp` of operands wider than 32
/// bits (no package lowers them yet), and the overflow intrinsics, whose
/// `{i32, i1}` result is read with `extractvalue` (`x86:aggregate`).
#[test]
fn what_stays_refused() {
    let wide = "define i8 @f(ptr %p) {\n  %a = load i64, ptr %p\n  \
                %r = call i8 @llvm.scmp.i8.i64(i64 %a, i64 %a)\n  ret i8 %r\n}\n\
                declare i8 @llvm.scmp.i8.i64(i64, i64)";
    let error = refusal(wide).unwrap_or_else(|| panic!("compiled:\n{wide}"));
    assert!(error.contains("x86:intrinsic"), "{wide}\n{error}");
    let overflow = "define i32 @f(i32 %a) {\n  \
                    %r = call {i32, i1} @llvm.sadd.with.overflow.i32(i32 %a, i32 %a)\n  \
                    %v = extractvalue {i32, i1} %r, 0\n  ret i32 %v\n}\n\
                    declare {i32, i1} @llvm.sadd.with.overflow.i32(i32, i32)";
    let error = refusal(overflow).unwrap_or_else(|| panic!("compiled:\n{overflow}"));
    assert!(
        error.contains("x86:aggregate") || error.contains("x86:intrinsic"),
        "{overflow}\n{error}"
    );
}
