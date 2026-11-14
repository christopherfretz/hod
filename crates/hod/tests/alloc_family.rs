//! `"alloc-family"` (WP-227, `spec/ir.md` §5.7): the family of an
//! allocation function, which only the optimizer reads. clang 19 and 22
//! and kiln write it at `-O1` and above on the C library's allocation
//! functions; hod drops it.
//!
//! The seed `tests/ir/alloc_family.ll` (which `xtask irdiff` runs against
//! `llc`) has it on declarations, on definitions and in call-site
//! groups. On both targets it is accepted, its object is byte-identical
//! to the object of the same module with every `"alloc-family"` removed,
//! and its `llvm-as` 19 and 22 bitcode gives that object too.

use std::time::Duration;

use hod::{Options, OutputKind, Report, Settings};
use hod_testutil::{LLVM_MAJORS, run_with, tool};

const TARGETS: [(&str, &str); 2] = [
    ("aarch64-unknown-linux-gnu", hod_ir::AARCH64_LINUX_DATA_LAYOUT),
    ("x86_64-unknown-linux-gnu", hod_ir::X86_64_LINUX_DATA_LAYOUT),
];

const SEED: &str = include_str!("../../../tests/ir/alloc_family.ll");

fn with_target(body: &str, triple: &str, layout: &str) -> String {
    format!("target datalayout = \"{layout}\"\ntarget triple = \"{triple}\"\n{body}")
}

fn compile(input: &[u8]) -> Vec<u8> {
    let options = Options { output_kind: OutputKind::Object, ..Options::default() };
    hod::compile_with(input, &options, &Settings::default(), &mut Report::default())
        .unwrap_or_else(|error| panic!("hod declined: {error}"))
}

/// The seed with every `"alloc-family"="<family>"` removed.
fn without_alloc_family(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(" \"alloc-family\"=\"") {
        out.push_str(&rest[..at]);
        let value = &rest[at + " \"alloc-family\"=\"".len()..];
        let end = value.find('"').expect("a closing quote");
        rest = &value[end + 1..];
    }
    out.push_str(rest);
    out
}

#[test]
fn alloc_family_changes_no_byte() {
    let without = without_alloc_family(SEED);
    let code_lines = |text: &str| -> Vec<String> {
        text.lines().filter(|l| !l.starts_with(';')).map(str::to_owned).collect()
    };
    assert_eq!(
        code_lines(SEED).iter().filter(|l| l.contains("\"alloc-family\"")).count(),
        8,
        "the seed has it in eight attribute groups"
    );
    assert!(code_lines(&without).iter().all(|l| !l.contains("alloc-family")), "all removed");
    for (triple, layout) in TARGETS {
        let with = compile(with_target(SEED, triple, layout).as_bytes());
        let plain = compile(with_target(&without, triple, layout).as_bytes());
        assert!(with == plain, "{triple}: alloc-family changed the object");
    }
}

#[test]
fn text_and_bitcode_give_the_same_object() {
    for (triple, layout) in TARGETS {
        let text = with_target(SEED, triple, layout);
        let from_text = compile(text.as_bytes());
        for major in LLVM_MAJORS {
            let Some(llvm_as) = tool(&format!("llvm-as-{major}")) else { continue };
            let mut command = std::process::Command::new(llvm_as);
            command.args(["-o", "-", "-"]);
            let ran = run_with(&mut command, text.as_bytes(), Duration::from_mins(1));
            assert!(ran.success(), "llvm-as {major}: {}", ran.stderr_text());
            assert!(compile(&ran.stdout) == from_text, "{triple}: bitcode {major} differs");
        }
    }
}
