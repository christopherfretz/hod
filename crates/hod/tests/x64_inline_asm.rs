//! x86-64 inline assembly with templates (WP-187, `spec/codegen.md`
//! §5.6.28): the module of `hod_x64`'s `tests/inline_asm.rs` gives the
//! same object from its text and from its bitcode (`llvm-as` of each
//! LLVM major present), under `--strict` (no fallback).

use hod::{OutputKind, RelocModel};

const HEAD: &str = "target datalayout = \"e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128\"\n\
                    target triple = \"x86_64-unknown-linux-gnu\"\n";
const FUNCTIONS: &str = include_str!("../../hod_x64/tests/data/inline_asm/functions.ll");

fn options() -> hod::Options {
    hod::Options {
        output_kind: OutputKind::Object,
        relocation_model: Some(RelocModel::Pic),
        ..hod::Options::default()
    }
}

#[test]
fn text_and_bitcode_give_the_same_object() {
    let text = format!("{HEAD}{FUNCTIONS}");
    let by_text = hod::compile(text.as_bytes(), &options()).unwrap_or_else(|e| panic!("text: {e}"));
    for major in hod_testutil::LLVM_MAJORS {
        let tool = std::path::PathBuf::from(format!("/usr/lib/llvm-{major}/bin/llvm-as"));
        if !tool.is_file() {
            continue;
        }
        let mut command = std::process::Command::new(tool);
        command.args(["-o", "-", "-"]);
        let ran = hod_testutil::run_with(
            &mut command,
            text.as_bytes(),
            std::time::Duration::from_mins(1),
        );
        assert!(ran.success(), "llvm-as {major}: {}", ran.stderr_text());
        let by_bitcode = hod::compile(&ran.stdout, &options())
            .unwrap_or_else(|e| panic!("bitcode {major}: {e}"));
        assert!(by_text == by_bitcode, "llvm-as {major}: the objects differ");
    }
}
