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
use hod_testutil::{LLVM_MAJORS, Scratch, linkers, tools_required};

mod i386_support;
use i386_support::{
    TRIPLE, checked, clang_module, llc_object, llvm_tool, run, stressed_object, tool,
};

/// The programs of `tests/data/i386`.
const PROGRAMS: [&str; 5] = ["loops.c", "switch.c", "calls.c", "memory.c", "bytes.c"];

/// The least module: `main` returns 42.
const RET42: &str =
    "target triple = \"i686-unknown-linux-gnu\"\n\ndefine i32 @main() {\n  ret i32 42\n}\n";

/// The module of `program` by clang `major`, in text.
fn module(scratch: &Scratch, clang: &Path, major: u32, program: &str) -> PathBuf {
    if program == "ret42" {
        let out = scratch.join(format!("{program}-{major}.ll"));
        std::fs::write(&out, RET42).unwrap();
        return out;
    }
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/i386").join(program);
    clang_module(scratch, clang, &source, &["-fno-pic", "-fno-pie"], &format!("{program}-{major}"))
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
            let llc_o = llc_object(&scratch, &llc, &ll, &[], &format!("{program}-{major}-llc"));
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
