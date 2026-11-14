//! `-mbranch-protection` (WP-185, `spec/codegen.md` §5.6.26): the
//! modules of `tests/ir/branch_protection.ll` and
//! `tests/ir/branch_protection_unwind.ll` (which `xtask irdiff` runs
//! against `llc`) give the same object from their text and from their
//! bitcode by every `llvm-as` present, on both targets.

use std::time::Duration;

use hod::{Options, OutputKind, Report, Settings};
use hod_testutil::{LLVM_MAJORS, run_with, tool};

const TARGETS: [(&str, &str); 2] = [
    ("aarch64-unknown-linux-gnu", hod_ir::AARCH64_LINUX_DATA_LAYOUT),
    ("x86_64-unknown-linux-gnu", hod_ir::X86_64_LINUX_DATA_LAYOUT),
];

const SEEDS: [(&str, &str); 2] = [
    ("branch_protection", include_str!("../../../tests/ir/branch_protection.ll")),
    ("branch_protection_unwind", include_str!("../../../tests/ir/branch_protection_unwind.ll")),
];

fn compile(input: &[u8]) -> Vec<u8> {
    let options = Options { output_kind: OutputKind::Object, ..Options::default() };
    hod::compile_with(input, &options, &Settings::default(), &mut Report::default())
        .unwrap_or_else(|error| panic!("hod declined: {error}"))
}

#[test]
fn text_and_bitcode_give_the_same_object() {
    for (name, seed) in SEEDS {
        for (triple, layout) in TARGETS {
            let text =
                format!("target datalayout = \"{layout}\"\ntarget triple = \"{triple}\"\n{seed}");
            let from_text = compile(text.as_bytes());
            for major in LLVM_MAJORS {
                let Some(llvm_as) = tool(&format!("llvm-as-{major}")) else { continue };
                let mut command = std::process::Command::new(llvm_as);
                command.args(["-o", "-", "-"]);
                let ran = run_with(&mut command, text.as_bytes(), Duration::from_mins(1));
                assert!(ran.success(), "llvm-as {major}: {}", ran.stderr_text());
                assert!(
                    compile(&ran.stdout) == from_text,
                    "{name} {triple}: bitcode {major} differs"
                );
            }
        }
    }
}
