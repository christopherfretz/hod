//! A definition's own metadata attachments (WP-064, `spec/bc.md` §5.8,
//! `spec/ll.md` §5.7, `spec/ir.md` §2.2): text and bitcode refuse the
//! same kinds by the same name. In bitcode they live in the function's
//! block, which the module pass does not read; the body reader names them,
//! where before it recorded them after the module had been judged and the
//! module compiled.

use hod::{CompileError, Options, OutputKind, Report, Settings};
use hod_testutil::{LLVM_MAJORS, run_with};

const HEAD: &str = "target datalayout = \"e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128\"\n\
                    target triple = \"aarch64-unknown-linux-gnu\"\n\
                    @g = global i32 0\n";

const NODES: &str = "!0 = !{i32 12345}\n!1 = !{}\n!2 = !{ptr @g}\n!3 = !{i64 0, i64 256}\n\
                     !4 = !{i64 0, !\"t\"}\n";

fn compile(input: &[u8]) -> Result<Vec<u8>, String> {
    let options = Options { output_kind: OutputKind::Object, ..Options::default() };
    hod::compile_with(input, &options, &Settings::default(), &mut Report::default()).map_err(
        |error| match error {
            CompileError::Unsupported(unsupported) => unsupported.construct.name().to_owned(),
            other => panic!("not a decline: {other}"),
        },
    )
}

/// The outcome of `text`, and of its bitcode by every `llvm-as` present.
fn outcomes(text: &str) -> Vec<(String, Result<Vec<u8>, String>)> {
    let mut all = vec![("text".to_owned(), compile(text.as_bytes()))];
    for major in LLVM_MAJORS {
        let tool = std::path::PathBuf::from(format!("/usr/lib/llvm-{major}/bin/llvm-as"));
        if !tool.is_file() {
            continue;
        }
        let mut command = std::process::Command::new(tool);
        command.args(["-o", "-", "-"]);
        let ran = run_with(&mut command, text.as_bytes(), std::time::Duration::from_mins(1));
        assert!(ran.success(), "llvm-as {major}: {}", ran.stderr_text());
        all.push((format!("bitcode {major}"), compile(&ran.stdout)));
    }
    all
}

fn definition(attachment: &str) -> String {
    format!("{HEAD}define void @f() {attachment} {{\n  ret void\n}}\n{NODES}")
}

// `spec/ir.md` §2.2: the kinds hod does not drop refuse a definition in
// both forms, by the same name.
#[test]
fn a_definitions_attachment_is_refused_alike() {
    for (attachment, name) in [
        ("!kcfi_type !0", "global-metadata:kcfi_type"),
        ("!foo !1", "global-metadata:foo"),
        ("!associated !2", "global-metadata:associated"),
        ("!absolute_symbol !3", "global-metadata:absolute_symbol"),
    ] {
        for (route, outcome) in outcomes(&definition(attachment)) {
            assert_eq!(outcome, Err(name.to_owned()), "{route}: {attachment}");
        }
    }
}

// `spec/ir.md` §2.2: the kinds hod drops compile in both forms, to the
// same object.
#[test]
fn the_dropped_kinds_compile_alike() {
    let text = definition("!type !4 !annotation !1");
    let all = outcomes(&text);
    let (_, first) = &all[0];
    let object = first.as_ref().unwrap_or_else(|e| panic!("text: {e}"));
    for (route, outcome) in &all {
        assert_eq!(outcome.as_ref(), Ok(object), "{route}");
    }
}

// The body reader names the attachment among the body's refusals, the
// attachment first.
#[test]
fn the_body_reader_names_the_attachment() {
    let text = definition("!kcfi_type !0");
    for major in LLVM_MAJORS {
        let tool = std::path::PathBuf::from(format!("/usr/lib/llvm-{major}/bin/llvm-as"));
        if !tool.is_file() {
            continue;
        }
        let mut command = std::process::Command::new(tool);
        command.args(["-o", "-", "-"]);
        let ran = run_with(&mut command, text.as_bytes(), std::time::Duration::from_mins(1));
        let read = hod_bc::read_module(&ran.stdout, &hod_bc::ReadOptions::default()).unwrap();
        let mut module = read.module;
        let mut seen = Vec::new();
        hod_bc::read_bodies(&mut module, &read.bits, |_, body| {
            assert!(body.body.is_none(), "{major}: the body is refused");
            seen.extend(body.unsupported.iter().map(ToString::to_string));
            Ok::<(), hod_bc::BcError>(())
        })
        .unwrap();
        assert_eq!(seen, ["global-metadata:kcfi_type"], "{major}");
    }
}
