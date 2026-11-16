//! WP-306: what WP-298 left open for integers of 33 to 64 bits on i386:
//! the integer intrinsics (`bswap`, `ctpop`, `ctlz`, `cttz`, `bitreverse`,
//! `abs`, min/max, saturating add/sub/shift, funnel shifts), `switch` on
//! one, a `ptradd` index of one. Programs under `tests/data/i386/i64_rest/`
//! (`unary.ll`, `binary.ll`, `funnel.ll`, `compare.ll` written by `gen.py` there;
//! `switch.ll`, `ptradd.ll` by hand) move their values through memory only,
//! at widths 64, 57, 48 and 33, and print every result in hex. Each is
//! built with no `target-cpu` (`generic`: no `cmov`) and with `pentium4`,
//! position-dependent and as a PIE, by `llc` 19 and 22 and by hod (plain
//! and under `Stress::ALL`); the runs under `qemu-i386` must print the
//! same bytes.

use std::path::{Path, PathBuf};
use std::process::Command;

use hod::{Options, OutputKind, RelocModel};
use hod_testutil::target::Target;
use hod_testutil::{LLVM_MAJORS, Scratch};

mod i386_support;
use i386_support::{
    TRIPLE, back_end_object, checked, cpu_features, llc_object, llvm_tool, run_with_features, tool,
};

/// The programs (`compare.ll`, `scmp`/`ucmp` of wide operands: i386 join C).
const PROGRAMS: [&str; 6] =
    ["unary.ll", "binary.ll", "funnel.ll", "switch.ll", "ptradd.ll", "compare.ll"];

fn data(program: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/i386/i64_rest").join(program)
}

/// `text` for CPU `cpu`: as it is, or with `"target-cpu"="pentium4"` on
/// every function.
fn for_cpu(text: &str, cpu: &str) -> String {
    if cpu == "generic" {
        return text.to_owned();
    }
    let out = text.replace("attributes #0 = {", "attributes #0 = { \"target-cpu\"=\"pentium4\"");
    assert_ne!(out, text, "no attribute group #0");
    out
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
fn i64_rest_programs_run_as_llc_s() {
    let target = Target::I686;
    let (Some(gcc), Some(qemu)) = (tool(&target.c_driver()), tool(&target.emulator())) else {
        return;
    };
    let scratch = Scratch::new("i386-i64-rest");
    let (mut programs, mut runs) = (0, 0);
    for major in LLVM_MAJORS {
        let Some(llc) = llvm_tool(major, "llc") else { continue };
        for program in PROGRAMS {
            let source = std::fs::read_to_string(data(program)).unwrap();
            for cpu in ["generic", "p4"] {
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
                        results.push(run_with_features(&qemu, &exe, cpu_features(cpu)));
                        runs += 1;
                    }
                    assert!(
                        results[0].1.ends_with(b"done\n"),
                        "{name}: llc's run did not finish: {}",
                        first_difference(&results[0].1, b"")
                    );
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
    eprintln!("i386 i64 rest: {programs} programs, {runs} runs, hod's equal llc's");
}

/// The error of hod on the module `body` (functions of a module for
/// i686), or `None` when it compiles.
fn refusal(body: &str) -> Option<String> {
    let text = format!(
        "target triple = \"{TRIPLE}\"\n{body}\n\
         declare i128 @llvm.ctpop.i128(i128)\n\
         declare i65 @llvm.smax.i65(i65, i65)\n\
         declare i128 @llvm.fshl.i128(i128, i128, i128)\n\
         declare i128 @llvm.uadd.sat.i128(i128, i128)\n\
         declare {{i64, i1}} @llvm.sadd.with.overflow.i64(i64, i64)\n\
         declare {{i64, i1}} @llvm.umul.with.overflow.i64(i64, i64)\n"
    );
    let options = Options {
        output_kind: OutputKind::Object,
        triple: Some(TRIPLE.into()),
        ..Options::default()
    };
    hod::compile(text.as_bytes(), &options).err().map(|e| e.to_string())
}

/// Integers wider than 64 bits stay refused as `x86:i64:wide` in every
/// construct of this package, and nothing panics on them.
#[test]
fn wider_than_64_bits_is_refused_by_name() {
    let cases = [
        "define void @f(ptr %p) {\n  %a = load i128, ptr %p\n  \
         %r = call i128 @llvm.ctpop.i128(i128 %a)\n  store i128 %r, ptr %p\n  ret void\n}",
        "define void @f(ptr %p) {\n  %a = load i65, ptr %p\n  \
         %r = call i65 @llvm.smax.i65(i65 %a, i65 %a)\n  store i65 %r, ptr %p\n  ret void\n}",
        "define void @f(ptr %p) {\n  %a = load i128, ptr %p\n  \
         %r = call i128 @llvm.fshl.i128(i128 %a, i128 %a, i128 %a)\n  store i128 %r, ptr %p\n  \
         ret void\n}",
        "define void @f(ptr %p) {\n  %a = load i128, ptr %p\n  \
         %r = call i128 @llvm.uadd.sat.i128(i128 %a, i128 %a)\n  store i128 %r, ptr %p\n  \
         ret void\n}",
        "define i32 @f(ptr %p) {\nentry:\n  %a = load i128, ptr %p\n  \
         switch i128 %a, label %d [ i128 18446744073709551616, label %c ]\n\
         c:\n  ret i32 1\nd:\n  ret i32 0\n}",
        "define ptr @f(ptr %p) {\n  %a = load i128, ptr %p\n  \
         %q = getelementptr i32, ptr %p, i128 %a\n  ret ptr %q\n}",
        "define ptr @f(ptr %p) {\n  %a = load i96, ptr %p\n  \
         %q = getelementptr i8, ptr %p, i96 %a\n  ret ptr %q\n}",
    ];
    for case in cases {
        let error = refusal(case).unwrap_or_else(|| panic!("compiled:\n{case}"));
        // The shared classifier refuses an intrinsic of an odd width above
        // 64 bits first (`int-wide`), before the back end sees it.
        let name =
            if case.contains("i65") { "intrinsic:llvm.smax/int-wide" } else { "x86:i64:wide" };
        assert!(error.contains(name), "{case}\n{error}");
    }
}

/// The new lowerings compile alone (no other construct in the module).
#[test]
fn i64_switch_ptradd_and_intrinsics_compile() {
    let cases = [
        "define i32 @f(ptr %p) {\nentry:\n  %a = load i64, ptr %p\n  \
         switch i64 %a, label %d [ i64 4294967296, label %c ]\n\
         c:\n  ret i32 1\nd:\n  ret i32 0\n}",
        "define ptr @f(ptr %p) {\n  %a = load i64, ptr %p\n  \
         %q = getelementptr i32, ptr %p, i64 %a\n  ret ptr %q\n}",
        "define void @f(ptr %p) {\n  %a = load i48, ptr %p\n  \
         %r = call i48 @llvm.ctlz.i48(i48 %a, i1 false)\n  store i48 %r, ptr %p\n  ret void\n}\n\
         declare i48 @llvm.ctlz.i48(i48, i1)",
    ];
    for case in &cases {
        assert_eq!(refusal(case), None, "{case}");
    }
}

/// The overflow intrinsics return `{i64, i1}`, an aggregate, read with
/// `extractvalue`: refused until WP-315 (design 0008 I11), which runs them
/// at every width (`i386_agg.rs`).
#[test]
fn overflow_intrinsics_compile() {
    for which in ["sadd", "umul"] {
        let case = format!(
            "define void @f(ptr %p) {{\n  %a = load i64, ptr %p\n  \
             %r = call {{i64, i1}} @llvm.{which}.with.overflow.i64(i64 %a, i64 %a)\n  \
             %v = extractvalue {{i64, i1}} %r, 0\n  store i64 %v, ptr %p\n  ret void\n}}"
        );
        assert_eq!(refusal(&case), None, "{case}");
    }
}
