//! Named registers and the always-inliner (WP-064, `spec/cli.md`
//! "Always-inline"): a body that reads or writes a frame register by name
//! (`sp`, `x29`, `x30`) means another register once inlined, so a module
//! that may still inline it is declined by name; a system register means
//! the same in or out of line. Text and bitcode agree.

use hod::{CompileError, OutputKind, RelocModel, inline_reason};

const HEAD: &str = "target datalayout = \"e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128\"\n\
                    target triple = \"aarch64-unknown-linux-gnu\"\n\
                    declare i64 @llvm.read_register.i64(metadata)\n\
                    declare void @llvm.write_register.i64(metadata, i64)\n";

fn options() -> hod::Options {
    hod::Options {
        output_kind: OutputKind::Object,
        relocation_model: Some(RelocModel::Pic),
        ..hod::Options::default()
    }
}

fn outcome(input: &[u8]) -> Result<Vec<u8>, String> {
    hod::compile(input, &options()).map_err(|error| match error {
        CompileError::Unsupported(unsupported) => unsupported.construct.to_string(),
        other => panic!("not a decline: {other}"),
    })
}

/// What text, and bitcode of every `llvm-as` present, give for `body`.
fn both(body: &str) -> Result<Vec<u8>, String> {
    let text = format!("{HEAD}{body}");
    let by_text = outcome(text.as_bytes());
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
        let by_bitcode = outcome(&ran.stdout);
        match (&by_text, &by_bitcode) {
            (Ok(_), Ok(_)) => {}
            _ => assert_eq!(by_text, by_bitcode, "llvm-as {major}"),
        }
    }
    by_text
}

fn module(register: &str, attrs: &str) -> String {
    format!(
        "define linkonce_odr i64 @s() #0 {{\n  %a = call i64 @llvm.read_register.i64(metadata !0)\n  \
         ret i64 %a\n}}\n\
         define i64 @caller() {{\n  %a = call i64 @s()\n  ret i64 %a\n}}\n\
         attributes #0 = {{ {attrs} }}\n!0 = !{{!\"{register}\"}}\n"
    )
}

#[test]
fn a_frame_register_read_that_may_be_inlined_is_declined() {
    for register in ["sp", "x29", "x30"] {
        let declined = both(&module(register, "alwaysinline nounwind"));
        assert_eq!(declined, Err(inline_reason::FRAME_ADDRESS.to_owned()), "{register}");
        // Without `alwaysinline`, nothing was inlined: it compiles.
        both(&module(register, "nounwind")).unwrap_or_else(|e| panic!("{register}: {e}"));
    }
    // A system register is the same register inlined.
    both(&module("cntvct_el0", "alwaysinline nounwind")).expect("compiles");
}
