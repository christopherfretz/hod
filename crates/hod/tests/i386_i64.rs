//! WP-298 (design 0008 I7): integers of 33 to 64 bits on i386. Programs
//! under `tests/data/i386/i64/` move their 64-bit values through memory
//! only (no `i64` argument or result crosses a call), are compiled by
//! clang for `i686-linux-gnu` with no `target-cpu` and with
//! `-march=pentium4`, and run under `qemu-i386` as `llc`'s object, hod's,
//! and hod's under `Stress::ALL`, linked by every linker present: the runs
//! must be equal.

use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

use hod::{Options, OutputKind};
use hod_testutil::target::Target;
use hod_testutil::{LLVM_MAJORS, Scratch, linkers};

mod i386_support;
use i386_support::{
    TRIPLE, checked, clang_module, cpu_features, llc_object, llvm_tool, run_with_features,
    stressed_object, tool,
};

/// The programs (C, or IR written by hand).
const PROGRAMS: [&str; 3] = ["arith.c", "bitint.c", "select.ll"];

/// The CPU variants: none (`generic`: no `cmov`) and `pentium4`.
const CPUS: [(&str, &[&str]); 2] = [("generic", &[]), ("p4", &["-march=pentium4"])];

fn data(program: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/i386/i64").join(program)
}

/// hod's object of the module `ll` through the facade.
fn hod_object(ll: &[u8], program: &str) -> Vec<u8> {
    let options = Options {
        output_kind: OutputKind::Object,
        triple: Some(TRIPLE.into()),
        ..Options::default()
    };
    hod::compile(ll, &options).unwrap_or_else(|e| panic!("hod {program}: {e}"))
}

#[test]
fn i64_programs_run_as_llc_s() {
    let target = Target::I686;
    let (Some(gcc), Some(qemu)) = (tool(&target.c_driver()), tool(&target.emulator())) else {
        return;
    };
    let linkers = linkers::present(target);
    let scratch = Scratch::new("i386-i64");
    let (mut programs, mut runs) = (0, 0);
    for major in LLVM_MAJORS {
        let (Some(clang), Some(llc)) = (llvm_tool(major, "clang"), llvm_tool(major, "llc")) else {
            continue;
        };
        for program in PROGRAMS {
            for (cpu, flags) in CPUS {
                let name = format!("{program}-{major}-{cpu}");
                let ll = if Path::new(program).extension().is_some_and(|e| e == "ll") {
                    let text = std::fs::read_to_string(data(program)).unwrap();
                    let text = if flags.is_empty() {
                        text
                    } else {
                        text.replace(
                            "attributes #0 = {",
                            "attributes #0 = { \"target-cpu\"=\"pentium4\"",
                        )
                    };
                    scratch.write(&format!("{name}.ll"), text)
                } else {
                    let mut all = vec!["-fno-pic", "-fno-pie"];
                    all.extend_from_slice(flags);
                    clang_module(&scratch, &clang, &data(program), &all, &name)
                };
                let text = std::fs::read(&ll).unwrap();
                let hod_o = scratch.write(&format!("{name}-hod.o"), hod_object(&text, &name));
                let stress_o = scratch.write(&format!("{name}-stress.o"), stressed_object(&text));
                let llc_o = llc_object(&scratch, &llc, &ll, &[], &format!("{name}-llc"));
                programs += 1;
                for (linker, path) in &linkers {
                    let mut results = Vec::new();
                    for (who, object) in [("llc", &llc_o), ("hod", &hod_o), ("stress", &stress_o)] {
                        let exe = scratch.join(format!("{name}-{who}-{linker}"));
                        let mut command = Command::new(&gcc);
                        let flags: Vec<OsString> =
                            linkers::driver_flags(scratch.path(), linker, path);
                        command.args(flags).arg("-no-pie").arg(object).arg("-o").arg(&exe);
                        checked(&format!("{linker} link of {name} ({who})"), &mut command);
                        results.push(run_with_features(&qemu, &exe, cpu_features(cpu)));
                    }
                    assert!(
                        results[0].1.ends_with(b"done\n"),
                        "{name}: llc's run did not finish: {:?}",
                        String::from_utf8_lossy(&results[0].1)
                    );
                    for (who, result) in ["hod", "stress"].iter().zip(&results[1..]) {
                        assert_eq!(
                            String::from_utf8_lossy(&results[0].1),
                            String::from_utf8_lossy(&result.1),
                            "{name} ({linker}): {who}'s output differs from llc's"
                        );
                        assert_eq!(results[0].0, result.0, "{name} ({linker}): {who}'s status");
                    }
                    runs += 1;
                }
            }
        }
    }
    eprintln!("i386 i64: {programs} programs, {runs} runs (llc, hod, stress each) equal llc's");
}

/// An `i128` division is refused by name (WP-346 lowers the rest of
/// `i128`, `i386_wide_int.rs`).
#[test]
fn i128_is_refused_by_name() {
    let text = "target triple = \"i686-unknown-linux-gnu\"\n\
                define void @f(ptr %p) {\n  %a = load i128, ptr %p\n  %b = udiv i128 %a, %a\n  \
                store i128 %b, ptr %p\n  ret void\n}\n";
    let options = Options {
        output_kind: OutputKind::Object,
        triple: Some(TRIPLE.into()),
        ..Options::default()
    };
    let error = hod::compile(text.as_bytes(), &options).unwrap_err().to_string();
    assert!(error.contains("x86:i64:wide:div"), "{error}");
}
