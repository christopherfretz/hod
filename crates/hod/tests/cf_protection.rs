//! `-fcf-protection` (WP-184, `spec/codegen.md` §5.6.25): the module of
//! `tests/ir/cf_protection.ll` (the flags `cf-protection-branch` and
//! `cf-protection-return`, `nocf_check` on functions and on a call site)
//! gives the same x86-64 object from its text and from its bitcode by
//! every `llvm-as` present; so does each flag alone. AArch64 refuses the
//! flags and accepts `nocf_check`, from text and bitcode alike.

use std::time::Duration;

use hod::{Options, OutputKind, Report, Settings};
use hod_testutil::{LLVM_MAJORS, run_with, tool};

const SEED: &str = include_str!("../../../tests/ir/cf_protection.ll");

fn with_target(body: &str, triple: &str, layout: &str) -> String {
    format!("target datalayout = \"{layout}\"\ntarget triple = \"{triple}\"\n{body}")
}

/// The seed's functions with `flags` as its module flags.
fn seed(flags: &str) -> String {
    let at = SEED.find("!llvm.module.flags").expect("the seed's flags");
    format!("{}{flags}", &SEED[..at])
}

fn compile(input: &[u8]) -> Result<Vec<u8>, String> {
    let options = Options { output_kind: OutputKind::Object, ..Options::default() };
    hod::compile_with(input, &options, &Settings::default(), &mut Report::default())
        .map_err(|error| error.to_string())
}

/// `text` and its bitcode by each `llvm-as`.
fn inputs(text: &str) -> Vec<(String, Vec<u8>)> {
    let mut out = vec![("text".to_owned(), text.as_bytes().to_vec())];
    for major in LLVM_MAJORS {
        let Some(llvm_as) = tool(&format!("llvm-as-{major}")) else { continue };
        let mut command = std::process::Command::new(llvm_as);
        command.args(["-o", "-", "-"]);
        let ran = run_with(&mut command, text.as_bytes(), Duration::from_mins(1));
        assert!(ran.success(), "llvm-as {major}: {}", ran.stderr_text());
        out.push((format!("bitcode {major}"), ran.stdout));
    }
    out
}

const BRANCH: &str =
    "!llvm.module.flags = !{!0}\n!0 = !{i32 8, !\"cf-protection-branch\", i32 1}\n";
const RETURN: &str =
    "!llvm.module.flags = !{!0}\n!0 = !{i32 8, !\"cf-protection-return\", i32 1}\n";

#[test]
fn text_and_bitcode_give_the_same_object() {
    let full = &SEED[SEED.find("!llvm.module.flags").unwrap()..];
    let mut objects = Vec::new();
    for flags in [full, BRANCH, RETURN, ""] {
        let text =
            with_target(&seed(flags), "x86_64-unknown-linux-gnu", hod_ir::X86_64_LINUX_DATA_LAYOUT);
        let mut from_text = None;
        for (label, input) in inputs(&text) {
            let object = compile(&input).unwrap_or_else(|e| panic!("{label}: {e}"));
            match &from_text {
                None => from_text = Some(object),
                Some(first) => assert!(*first == object, "{flags:?}: {label} differs"),
            }
        }
        objects.push(from_text.unwrap());
    }
    // The flags change the object (the marks, the note); each differently.
    for (index, object) in objects.iter().enumerate() {
        assert!(objects[index + 1..].iter().all(|other| other != object), "flag set {index}");
    }
}

#[test]
fn aarch64_refuses_the_flags_and_takes_nocf_check() {
    for flags in [BRANCH, RETURN] {
        let text = with_target(
            &seed(flags),
            "aarch64-unknown-linux-gnu",
            hod_ir::AARCH64_LINUX_DATA_LAYOUT,
        );
        for (label, input) in inputs(&text) {
            let error = compile(&input).expect_err(&label);
            assert!(error.contains("module:cf-protection-"), "{label}: {error}");
        }
    }
    let text =
        with_target(&seed(""), "aarch64-unknown-linux-gnu", hod_ir::AARCH64_LINUX_DATA_LAYOUT);
    let mut from_text = None;
    for (label, input) in inputs(&text) {
        let object = compile(&input).unwrap_or_else(|e| panic!("{label}: {e}"));
        match &from_text {
            None => from_text = Some(object),
            Some(first) => assert!(*first == object, "{label} differs"),
        }
    }
}
