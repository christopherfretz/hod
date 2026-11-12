//! The front half of `hod::compile` (`spec/cli.md` "Reasons", "The front
//! half"): every way a module is declined, for text and for bitcode.
//!
//! `data/front.ll` and `data/front.bc` (its LLVM 19 bitcode, checked in so
//! the bitcode path runs without LLVM) carry a reason in the module pass,
//! one the body reader names and one in the body pass. Bitcode for the
//! other cases is made by `llvm-as` when it is installed.
//!
//! Every reason a test names is one no code-generation package turns on
//! (`module:asm`, `alloca:scalable`, `cc:ghccc`, or a row taken away from
//! `Support`), so the tests do not move with the support matrix.

use std::path::{Path, PathBuf};
use std::process::Command;

use hod::{CompileError, Construct, Options, OutputKind, Report, Settings, Unsupported};
use hod_ir::{Arch, Support};

const FRONT_LL: &[u8] = include_bytes!("data/front.ll");
const FRONT_BC: &[u8] = include_bytes!("data/front.bc");

const TRIPLE: &str = "target triple = \"aarch64-unknown-linux-gnu\"\n";

fn obj() -> Options {
    Options { output_kind: OutputKind::Object, ..Options::default() }
}

fn run(input: &[u8], settings: &Settings) -> (Result<Vec<u8>, CompileError>, Report) {
    let mut report = Report::default();
    let result = hod::compile_with(input, &obj(), settings, &mut report);
    (result, report)
}

fn census() -> Settings {
    Settings { census: true, ..Settings::default() }
}

fn everything() -> Settings {
    Settings {
        support: Some(Support::everything_implemented(Arch::Aarch64)),
        ..Settings::default()
    }
}

fn unsupported(name: &str, function: Option<&str>) -> Result<Vec<u8>, CompileError> {
    Err(Unsupported {
        construct: Construct::Named(name.to_owned().into()),
        function: function.map(str::to_owned),
        detail: None,
    }
    .into())
}

fn module_reason(construct: Construct) -> Result<Vec<u8>, CompileError> {
    Err(Unsupported::module(construct).into())
}

fn phases(report: &Report) -> Vec<&'static str> {
    report.phases.iter().map(|&(name, _)| name).collect()
}

/// The `llvm-as` of each installed LLVM.
fn assemblers() -> Vec<PathBuf> {
    let found: Vec<PathBuf> = [22, 19]
        .iter()
        .map(|major| PathBuf::from(format!("/usr/lib/llvm-{major}/bin/llvm-as")))
        .filter(|path| path.is_file())
        .collect();
    if found.is_empty() {
        // Skips with a note, or fails under HOD_REQUIRE_TOOLS=1.
        let _ = hod_testutil::tool("llvm-as");
    }
    found
}

fn assemble(llvm_as: &Path, text: &[u8]) -> Vec<u8> {
    let out = hod_testutil::run_with(
        Command::new(llvm_as).args(["-o", "-", "-"]),
        text,
        hod_testutil::DEFAULT_TIMEOUT,
    );
    assert!(out.success(), "{}: {}", llvm_as.display(), out.stderr_text());
    out.stdout
}

// spec/cli.md "Reasons": the first refusal is the module pass's, before
// any body is read; the census has every reason, the body reader's
// among them, and the same first one. Text and bitcode agree.
#[test]
fn fixture_first_refusal_and_census() {
    for input in [FRONT_LL, FRONT_BC] {
        let (result, report) = run(input, &Settings::default());
        assert_eq!(result, unsupported("module:asm", None));
        assert_eq!(phases(&report), ["parse", "classify"], "no body is read");
        assert_eq!(report.target.as_deref(), Some("aarch64-unknown-linux-gnu"));
        assert!(report.reasons.is_empty());

        let (result, report) = run(input, &census());
        assert_eq!(result, unsupported("module:asm", None));
        assert_eq!(phases(&report), ["parse", "classify", "bodies"]);
        assert_eq!(report.reasons[0], "module:asm");
        for name in ["alloca:scalable", "cc:ghccc"] {
            assert!(report.reasons.iter().any(|r| r == name), "{name}: {:?}", report.reasons);
        }
        let mut sorted = report.reasons.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), report.reasons.len(), "each once: {:?}", report.reasons);
    }
    let text = run(FRONT_LL, &census()).1.reasons;
    assert_eq!(run(FRONT_BC, &census()).1.reasons, text, "the same census from bitcode");
}

// spec/cli.md "Reasons": past the module pass, the body reader's refusal
// and the body pass's name the function; a module accepted whole reaches
// code generation and becomes an object.
#[test]
fn body_reasons_name_the_function() {
    for input in [FRONT_LL, FRONT_BC] {
        let (result, report) = run(input, &everything());
        assert_eq!(result, unsupported("alloca:scalable", Some("scalable")));
        assert_eq!(phases(&report), ["parse", "classify"], "stopped in the first body");
    }
    // The body pass's refusal, `ghccc` taken away.
    let ghc = format!("{TRIPLE}define ghccc void @ghc() nounwind {{\n  ret void\n}}\n");
    let support = Support::everything_implemented(Arch::Aarch64).without("cc:ghccc");
    let settings = Settings { support, ..Settings::default() };
    assert_eq!(run(ghc.as_bytes(), &settings).0, unsupported("cc:ghccc", Some("ghc")));
    let add = format!(
        "{TRIPLE}define i32 @add(i32 %a) nounwind {{\n  %s = add i32 %a, 1\n  ret i32 %s\n}}\n"
    );
    let (result, report) = run(add.as_bytes(), &everything());
    assert!(result.is_ok_and(|object| object.starts_with(b"\x7FELF")));
    assert_eq!(phases(&report), ["parse", "classify", "bodies", "emission", "object"]);
    // A body pass refusal: only `inst:add` is taken away.
    let support = Support::everything_implemented(Arch::Aarch64).without("inst:add");
    let settings = Settings { support, ..Settings::default() };
    assert_eq!(run(add.as_bytes(), &settings).0, unsupported("inst:add", Some("add")));
}

// spec/cli.md "Reasons": a body on its way to code generation is verified;
// a module refused earlier is not, so the cheap reason stands.
#[test]
fn verify_is_on_the_path_to_code_generation() {
    let text = format!(
        "{TRIPLE}define i32 @f() nounwind {{\n  %a = add i32 %b, 1\n  %b = add i32 1, 1\n  ret i32 %a\n}}\n"
    );
    match run(text.as_bytes(), &everything()).0 {
        Err(CompileError::MalformedInput(message)) => {
            assert!(message.starts_with("function 'f': "), "{message}");
        }
        other => panic!("expected malformed input, got {other:?}"),
    }
    let refused_first = format!("module asm \"nop\"\n{text}");
    let without = Support::everything_implemented(Arch::Aarch64).without("module:asm");
    let settings = Settings { support: without, ..Settings::default() };
    assert_eq!(run(refused_first.as_bytes(), &settings).0, unsupported("module:asm", None));
    let (_, report) = run(text.as_bytes(), &Settings { census: true, ..everything() });
    assert_eq!(report.reasons, ["malformed-input"]);
}

// spec/cli.md "Reasons", WP-068: the cheap order. A refusal of the module
// pass ends the work before any body is read (a body the reader would
// reject is never seen), and the walk over the bodies stops at the first
// refusal (a later body the reader would reject is never seen either).
// The census reads them all, so it finds the malformed one.
#[test]
fn a_refusal_ends_the_reading() {
    let broken = "define i32 @broken() nounwind {\n  %x = frobnicate i32 1\n  ret i32 %x\n}\n";
    let ghc = "define ghccc void @ghc() nounwind {\n  ret void\n}\n";
    let support = Support::everything_implemented(Arch::Aarch64)
        .without("cc:ghccc")
        .and_then(|support| support.without("module:asm"));
    let settings = Settings { support: support.clone(), ..Settings::default() };

    let module_level = format!("{TRIPLE}module asm \"nop\"\n{broken}");
    let (result, report) = run(module_level.as_bytes(), &settings);
    assert_eq!(result, unsupported("module:asm", None));
    assert_eq!(phases(&report), ["parse", "classify"], "no body was read");

    let body_level = format!("{TRIPLE}{ghc}{broken}");
    let (result, report) = run(body_level.as_bytes(), &settings);
    assert_eq!(result, unsupported("cc:ghccc", Some("ghc")));
    assert_eq!(phases(&report), ["parse", "classify"]);

    let census = Settings { support, census: true, ..Settings::default() };
    let (result, report) = run(body_level.as_bytes(), &census);
    assert_eq!(result, unsupported("cc:ghccc", Some("ghc")), "the same answer");
    assert_eq!(report.reasons.first().map(String::as_str), Some("cc:ghccc"));
    assert!(report.reasons.iter().any(|r| r == "malformed-input"), "{:?}", report.reasons);
}

// spec/cli.md "Reasons": the target, from the command line or the module,
// text or bitcode.
#[test]
fn target_reasons() {
    let target = |input: &[u8]| run(input, &Settings::default()).0;
    let declare = "declare i32 @g()\n";
    let unsupported_target = module_reason(Construct::UnsupportedTarget);
    for triple in
        ["riscv64-unknown-linux-gnu", "aarch64-unknown-linux-musl", "x86_64-pc-windows-msvc"]
    {
        let text = format!("target triple = \"{triple}\"\n{declare}");
        assert_eq!(target(text.as_bytes()), unsupported_target, "{triple}");
    }
    assert_eq!(target(declare.as_bytes()), unsupported_target, "no triple, no -mtriple");
    let layout = format!("target datalayout = \"E-m:e\"\n{TRIPLE}{declare}");
    assert_eq!(target(layout.as_bytes()), unsupported_target, "another data layout");
    for llvm_as in assemblers() {
        for text in [
            format!("target triple = \"riscv64-unknown-linux-gnu\"\n{declare}"),
            declare.to_owned(),
        ] {
            let bitcode = assemble(&llvm_as, text.as_bytes());
            assert_eq!(target(&bitcode), unsupported_target, "{}: {text}", llvm_as.display());
        }
        let bitcode = assemble(&llvm_as, format!("{TRIPLE}{declare}").as_bytes());
        let (result, report) = run(&bitcode, &Settings::default());
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(report.target.as_deref(), Some("aarch64-unknown-linux-gnu"));
        let x64 = Options { triple: Some("x86_64-unknown-linux-gnu".into()), ..obj() };
        let mut report = Report::default();
        let retargeted = hod::compile_with(&bitcode, &x64, &Settings::default(), &mut report);
        assert_eq!(retargeted, unsupported_target, "-mtriple of another target");
        assert_eq!(report.target.as_deref(), Some("x86_64-unknown-linux-gnu"));
    }
}

// spec/cli.md "Reasons": what the readers refuse.
#[test]
fn reader_refusals() {
    let malformed = |input: &[u8]| match run(input, &Settings::default()).0 {
        Err(CompileError::MalformedInput(message)) => message,
        other => panic!("expected malformed input, got {other:?}"),
    };
    let message = malformed(format!("{TRIPLE}define i32 @f( {{\n").as_bytes());
    assert!(message.starts_with("line 2:"), "{message}");
    for cut in [8, 64, FRONT_BC.len() / 2, FRONT_BC.len() - 1] {
        let message = malformed(&FRONT_BC[..cut]);
        assert!(message.starts_with("bit "), "{cut}: {message}");
    }
    // A text nested deeper than hod_ll's limit (LLVM takes it).
    let depth = hod_ll::limits::MAX_NESTING as usize + 1;
    let ty = format!("{}i8{}", "[1 x ".repeat(depth), "]".repeat(depth));
    let deep = format!("{TRIPLE}@v = external global {ty}\n");
    assert_eq!(run(deep.as_bytes(), &Settings::default()).0, unsupported(hod::LIMIT_READER, None));
    // The census of a module refused by its reader is that reason.
    let (_, report) = run(&FRONT_BC[..64], &census());
    assert_eq!(report.reasons, ["malformed-input"]);
}

// spec/cli.md "Bitcode conversion": bitcode from each installed LLVM gives
// the reasons of its text.
#[test]
fn installed_llvm_bitcode_agrees_with_text() {
    for llvm_as in assemblers() {
        let bitcode = assemble(&llvm_as, FRONT_LL);
        for settings in [Settings::default(), census(), everything()] {
            assert_eq!(
                run(&bitcode, &settings).0,
                run(FRONT_LL, &settings).0,
                "{}",
                llvm_as.display()
            );
            assert_eq!(run(&bitcode, &settings).1.reasons, run(FRONT_LL, &settings).1.reasons);
        }
    }
}
