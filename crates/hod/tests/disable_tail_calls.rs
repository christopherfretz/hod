//! `"disable-tail-calls"` on a function (WP-179, `spec/ir.md` §5.7,
//! `spec/codegen.md` §7.1 "Tail kinds"). LLVM's rule: in a function with
//! `"true"` no call is a tail call except a `musttail` one; `"false"` is
//! the default. hod makes no tail call but a `musttail` one anywhere, so
//! the attribute is kept and changes no code.
//!
//! - The module of `tests/ir/disable_tail_calls.ll` (which `xtask irdiff`
//!   runs against `llc`) gives the same object from its text and from its
//!   bitcode by every `llvm-as` present, on both targets.
//! - In a module whose calls all go to a declared function, each call is
//!   a call or a jump in hod's object exactly where it is in the object of
//!   `llc -O0 -global-isel=0 -fast-isel=0` 19 and 22, on both targets,
//!   except a `tail` call in a function without `"true"`: `llc` makes it
//!   a jump, hod a call (both correct: `tail` permits, it does not
//!   oblige).

use std::path::Path;
use std::time::Duration;

use hod::{Options, OutputKind, Report, Settings};
use hod_testutil::{LLVM_MAJORS, Scratch, run_with, tool};

const TARGETS: [(&str, &str); 2] = [
    ("aarch64-unknown-linux-gnu", hod_ir::AARCH64_LINUX_DATA_LAYOUT),
    ("x86_64-unknown-linux-gnu", hod_ir::X86_64_LINUX_DATA_LAYOUT),
];

fn with_target(body: &str, triple: &str, layout: &str) -> String {
    format!("target datalayout = \"{layout}\"\ntarget triple = \"{triple}\"\n{body}")
}

fn compile(input: &[u8]) -> Vec<u8> {
    let options = Options { output_kind: OutputKind::Object, ..Options::default() };
    hod::compile_with(input, &options, &Settings::default(), &mut Report::default())
        .unwrap_or_else(|error| panic!("hod declined: {error}"))
}

#[test]
fn text_and_bitcode_give_the_same_object() {
    let seed = include_str!("../../../tests/ir/disable_tail_calls.ll");
    for (triple, layout) in TARGETS {
        let text = with_target(seed, triple, layout);
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

/// Every call goes to `@g`, declared, so each is a branch with a
/// relocation in both objects.
const SHAPES: &str = "declare void @g(i32)
define void @none_tail(i32 %x) {\n  tail call void @g(i32 %x)\n  ret void\n}
define void @none_plain(i32 %x) {\n  call void @g(i32 %x)\n  ret void\n}
define void @none_notail(i32 %x) {\n  notail call void @g(i32 %x)\n  ret void\n}
define void @none_musttail(i32 %x) {\n  musttail call void @g(i32 %x)\n  ret void\n}
define void @true_tail(i32 %x) #0 {\n  tail call void @g(i32 %x)\n  ret void\n}
define void @true_plain(i32 %x) #0 {\n  call void @g(i32 %x)\n  ret void\n}
define void @true_notail(i32 %x) #0 {\n  notail call void @g(i32 %x)\n  ret void\n}
define void @true_musttail(i32 %x) #0 {\n  musttail call void @g(i32 %x)\n  ret void\n}
define void @true_two(i32 %x) #0 {\n  tail call void @g(i32 %x)\n  tail call void @g(i32 %x)\n  ret void\n}
define void @false_tail(i32 %x) #1 {\n  tail call void @g(i32 %x)\n  ret void\n}
define void @false_plain(i32 %x) #1 {\n  call void @g(i32 %x)\n  ret void\n}
define void @false_musttail(i32 %x) #1 {\n  musttail call void @g(i32 %x)\n  ret void\n}
attributes #0 = { \"disable-tail-calls\"=\"true\" }
attributes #1 = { \"disable-tail-calls\"=\"false\" }
";

/// Per function, in order, `call` or `jump` for each branch with a
/// relocation against `g`, read from `llvm-objdump -dr`.
fn shapes(objdump: &Path, object: &Path) -> Vec<(String, Vec<&'static str>)> {
    let mut command = std::process::Command::new(objdump);
    command.arg("-dr").arg("--no-show-raw-insn").arg(object);
    let ran = run_with(&mut command, b"", Duration::from_mins(1));
    assert!(ran.success(), "llvm-objdump: {}", ran.stderr_text());
    let text = String::from_utf8_lossy(&ran.stdout).into_owned();
    let mut out: Vec<(String, Vec<&'static str>)> = Vec::new();
    let mut last = "";
    for line in text.lines() {
        if let Some(name) = line.strip_suffix(">:").and_then(|l| l.split_once('<')) {
            out.push((name.1.to_owned(), Vec::new()));
            continue;
        }
        let fields: Vec<&str> = line.split('\t').map(str::trim).filter(|f| !f.is_empty()).collect();
        if line.contains("R_AARCH64_") || line.contains("R_X86_64_") {
            let symbol = fields.last().copied().unwrap_or("");
            let symbol = symbol.split(['-', '+']).next().unwrap_or("");
            if symbol == "g" {
                let kind = match last {
                    "bl" | "call" | "callq" => "call",
                    "b" | "jmp" | "jmpq" => "jump",
                    other => panic!("a relocation against g on '{other}'"),
                };
                out.last_mut().expect("in a function").1.push(kind);
            }
        } else if fields.len() >= 2 && fields[0].ends_with(':') {
            last = match fields[1] {
                "bl" => "bl",
                "b" => "b",
                "call" | "callq" => "call",
                "jmp" | "jmpq" => "jmp",
                _ => "",
            };
        }
    }
    out
}

#[test]
fn calls_and_jumps_are_where_llc_puts_them() {
    let Some(objdump) = tool("llvm-objdump") else { return };
    let dir = Scratch::new("disable-tail-calls");
    for (triple, layout) in TARGETS {
        let text = with_target(SHAPES, triple, layout);
        let hod_object = dir.path().join("hod.o");
        std::fs::write(&hod_object, compile(text.as_bytes())).unwrap();
        let ours = shapes(&objdump, &hod_object);
        assert_eq!(ours.len(), 12, "{triple}: {ours:?}");
        for major in LLVM_MAJORS {
            let Some(llc) = tool(&format!("llc-{major}")) else { continue };
            let llc_object = dir.path().join(format!("llc{major}.o"));
            let mut command = std::process::Command::new(llc);
            command
                .args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj", "-o"])
                .arg(&llc_object);
            let ran = run_with(&mut command, text.as_bytes(), Duration::from_mins(1));
            assert!(ran.success(), "llc {major}: {}", ran.stderr_text());
            let theirs = shapes(&objdump, &llc_object);
            assert_eq!(ours.len(), theirs.len(), "{triple} llc {major}");
            for ((name, hod), (llc_name, llc)) in ours.iter().zip(&theirs) {
                assert_eq!(name, llc_name);
                let expected: Vec<&str> = if matches!(name.as_str(), "none_tail" | "false_tail") {
                    // `llc` makes a permitted tail call; hod makes none.
                    assert_eq!(llc, &["jump"], "{triple} llc {major} {name}");
                    vec!["call"]
                } else {
                    llc.clone()
                };
                assert_eq!(hod, &expected, "{triple} llc {major} {name}");
            }
            let true_two = theirs.iter().find(|(name, _)| name == "true_two").unwrap();
            assert_eq!(true_two.1, ["call", "call"], "{triple} llc {major}");
        }
    }
}

/// `noredzone` (WP-179, `spec/ir.md` §5.7): hod keeps no data below the
/// stack pointer, so the seed of `tests/ir/noredzone.ll` gives the same
/// object with the attribute removed, from text and from bitcode.
#[test]
fn noredzone_changes_no_code() {
    let seed = include_str!("../../../tests/ir/noredzone.ll");
    let without = seed.replace("attributes #0 = { noredzone }", "attributes #0 = { noinline }");
    let without = without.replace(" noredzone {", " {");
    assert!(
        without.lines().filter(|l| !l.starts_with(';')).all(|l| !l.contains("noredzone")),
        "every use is removed"
    );
    for (triple, layout) in TARGETS {
        let with = compile(with_target(seed, triple, layout).as_bytes());
        let plain = compile(with_target(&without, triple, layout).as_bytes());
        assert!(with == plain, "{triple}: noredzone changed the object");
        for major in LLVM_MAJORS {
            let Some(llvm_as) = tool(&format!("llvm-as-{major}")) else { continue };
            let mut command = std::process::Command::new(llvm_as);
            command.args(["-o", "-", "-"]);
            let text = with_target(seed, triple, layout);
            let ran = run_with(&mut command, text.as_bytes(), Duration::from_mins(1));
            assert!(ran.success(), "llvm-as {major}: {}", ran.stderr_text());
            assert!(compile(&ran.stdout) == with, "{triple}: bitcode {major} differs");
        }
    }
}

/// A function attribute the readers refuse (`AttrClass::Rejected`) is
/// noted by the readers with the function's symbol and reported by
/// `classify`'s module pass (`walk.rs`, `module.unsupported()`), whose
/// rejection the facade turns into an answer with `from_rejection(..,
/// false)` (`crates/hod/src/session.rs:442`), which names no function:
/// the answer is `unsupported construct 'attr:<name>'` without `in
/// function '<f>'`. A kept attribute that the matrix refuses is judged
/// in the body pass and is named with its function. Ignored until the
/// facade names it (outside WP-179's ownership).
#[test]
#[ignore = "the facade's module pass does not name the function (crates/hod/src/session.rs:442)"]
fn a_refused_function_attribute_names_its_function() {
    let body = "define void @f() noimplicitfloat {\n  ret void\n}\n";
    for (triple, layout) in TARGETS {
        let text = with_target(body, triple, layout);
        let options = Options { output_kind: OutputKind::Object, ..Options::default() };
        let error = hod::compile_with(
            text.as_bytes(),
            &options,
            &Settings::default(),
            &mut Report::default(),
        )
        .expect_err("noimplicitfloat is refused");
        let hod::CompileError::Unsupported(unsupported) = error else {
            panic!("not a decline: {error}");
        };
        assert_eq!(unsupported.construct.name(), "attr:noimplicitfloat");
        assert_eq!(unsupported.function.as_deref(), Some("f"), "{triple}");
    }
}
