//! Variable arguments before a forwarded `...` (WP-064, WP-115,
//! `spec/ir.md` §5.8, `spec/ll.md` §11.6, `spec/codegen.md` §5.5): LLVM's
//! verifier accepts `musttail call (ptr, ...) @g(ptr %f, i32 42, ...)` in
//! a variadic function, and so does hod's. `classify` learns the call's
//! stack-argument area from `hod_cg::abi::TargetStackArgs::of_call`,
//! which types the variable arguments from the body (WP-115): such a
//! call compiles on AArch64 as `llc` builds it while its arguments fit
//! the caller's area, and is `musttail:stack-args-grow` when they do not.
//! On x86-64 the facade's rule (WP-148) judges the same way, and the call
//! that fits is then `x64:musttail-variable-arguments` (`spec/abi-x64.md`
//! §8.5).

#![allow(dead_code, reason = "the linking support is shared with other tests")]

#[path = "../../hod_obj/tests/common/linking.rs"]
mod linking;

use std::process::Command;
use std::time::Duration;

use hod::{CompileError, Options, OutputKind, Report, Settings};
use hod_testutil::{LLVM_MAJORS, Scratch, run_with, tool};
use linking::{Toolchain, exit_code};

const HEADER: &str = "target datalayout = \"e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128\"\n\
                      target triple = \"aarch64-unknown-linux-gnu\"\n";

/// Thunks that put variable arguments of their own before the forwarded
/// ones: an integer and a double (each over a forwarded register), one
/// after a call that clobbers every argument register, and one that only
/// adds integers.
const PROGRAM: &str = r#"
declare i32 @printf(ptr, ...)
@fmt = private constant [28 x i8] c"%d %.2f %ld %d %.2f %s %ld\0A\00"
@fmt2 = private constant [25 x i8] c"%d %d %d %d %d %d %d %d\0A\00"
@hello = private constant [6 x i8] c"hello\00"

define void @clobber(i64 %a, i64 %b, i64 %c, i64 %d, i64 %e, i64 %f, i64 %g, i64 %h, double %i, double %j, double %k, double %l, double %m, double %n, double %o, double %p) noinline {
  ret void
}

define internal i32 @h_extra(ptr %f, ...) noinline {
  %r = musttail call i32 (ptr, ...) @printf(ptr %f, i32 42, double 1.5, ...)
  ret i32 %r
}

define internal i32 @h_extra_call(ptr %f, i32 %n, ...) noinline {
  call void @clobber(i64 -1, i64 -1, i64 -1, i64 -1, i64 -1, i64 -1, i64 -1, i64 -1, double -9.0, double -9.0, double -9.0, double -9.0, double -9.0, double -9.0, double -9.0, double -9.0)
  %m = add i32 %n, 1
  %r = musttail call i32 (ptr, i32, ...) @printf(ptr %f, i32 %m, double 2.25, ...)
  ret i32 %r
}

define internal i32 @h_ints(ptr %f, ...) noinline {
  %r = musttail call i32 (ptr, ...) @printf(ptr %f, i32 1, i32 2, i32 3, ...)
  ret i32 %r
}

define i32 @main() {
  call i32 (ptr, ...) @h_extra(ptr @fmt, i32 0, i64 7, i32 8, double 0.0, double 3.5, ptr @hello, i64 9)
  call i32 (ptr, i32, ...) @h_extra_call(ptr @fmt, i32 10, i64 77, i32 88, double 0.0, double 4.75, ptr @hello, i64 99)
  call i32 (ptr, ...) @h_ints(ptr @fmt2, i32 0, i32 0, i32 0, i32 4, i32 5, i32 6, i32 7, i32 8)
  ret i32 0
}
"#;

fn hod_object(text: &[u8]) -> Result<Vec<u8>, CompileError> {
    let options = Options { output_kind: OutputKind::Object, ..Options::default() };
    hod::compile_with(text, &options, &Settings::default(), &mut Report::default())
}

/// `llvm-as` of `major` on `text`, `None` without the tool.
fn bitcode(major: u32, text: &str) -> Option<Vec<u8>> {
    let tool = std::path::PathBuf::from(format!("/usr/lib/llvm-{major}/bin/llvm-as"));
    if !tool.is_file() {
        return None;
    }
    let mut command = Command::new(tool);
    command.args(["-o", "-", "-"]);
    let ran = run_with(&mut command, text.as_bytes(), Duration::from_mins(1));
    assert!(ran.success(), "llvm-as {major}: {}", ran.stderr_text());
    Some(ran.stdout)
}

// §5.8: what LLVM's verifier accepts, hod's does: the text reads, its
// bodies verify, and the printer writes the call back as LLVM reads it.
#[test]
fn the_verifier_accepts_variable_arguments_before_the_forwarded_ones() {
    let text = format!("{HEADER}{PROGRAM}");
    let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
        .unwrap_or_else(|e| panic!("{e}"));
    let mut module = parsed.module;
    let mut bodies = Vec::new();
    hod_ll::parse_bodies(&mut module, &parsed.text, |module, read| {
        let body = read.body.expect("every body is read");
        body.verify(module).unwrap_or_else(|e| panic!("verify: {e}"));
        bodies.push(body.clone());
        Ok::<(), hod_ll::LlError>(())
    })
    .unwrap();
    for dialect in [hod_ir::Dialect::LLVM19, hod_ir::Dialect::LLVM22] {
        let mut printed = String::new();
        module.print(&mut printed, dialect, bodies.iter()).unwrap();
        assert!(printed.contains("i32 42, double 0x3FF8000000000000, ...)"), "{printed}");
        // `llvm-as` accepts what hod printed (its verifier runs).
        bitcode(dialect.major(), &printed);
    }
}

// `spec/codegen.md` §5.5: the thunks would run as `llc` builds them, by
// text and by bitcode, under every linker.
#[test]
fn thunks_with_variable_arguments_run_like_llc() {
    let text = format!("{HEADER}{PROGRAM}");
    let mut objects = vec![("hod", hod_object(text.as_bytes()).unwrap_or_else(|e| panic!("{e}")))];
    for major in LLVM_MAJORS {
        if let Some(bc) = bitcode(major, &text) {
            let object = hod_object(&bc).unwrap_or_else(|e| panic!("bitcode {major}: {e}"));
            assert_eq!(object, objects[0].1, "bitcode {major} gives the text's object");
        }
    }
    let dir = Scratch::new("hod-musttail-forward");
    let Some(toolchain) = Toolchain::aarch64(&dir) else { return };
    let Some(llc) = tool("llc") else { return };
    let input = dir.write("p.ll", &text);
    let llc_o = dir.join("llc.o");
    let mut command = Command::new(llc);
    command.args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj"]);
    command.arg(&input).arg("-o").arg(&llc_o);
    let built = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(built.success(), "llc: {}", built.stderr_text());
    let hod_o = dir.write("hod.o", objects.remove(0).1);
    let mut checked = 0;
    for linker in &toolchain.linkers {
        for (tag, object) in [("llc", &llc_o), ("hod", &hod_o)] {
            let exe = dir.join(format!("{tag}-{}", linker.name));
            let ran = toolchain.link_and_run(linker, &[], &[object], &exe);
            assert_eq!(exit_code(&ran), Some(0), "{tag} {}: {}", linker.name, ran.stderr_text());
            assert_eq!(
                ran.stdout_text(),
                "42 1.50 7 8 3.50 hello 9\n\
                 11 2.25 77 88 4.75 hello 99\n\
                 1 2 3 4 5 6 7 8\n",
                "{tag} under {}",
                linker.name
            );
            checked += 1;
        }
    }
    eprintln!("{checked} runs agree");
}

// `spec/codegen.md` §5.5, §7.1 "The `musttail` hook": variable
// arguments in registers compile; ones that need stack beyond the
// caller's area are refused by name, by text and by bitcode. On x86-64
// (WP-148, `spec/abi-x64.md` §8.5) the forwarding thunk compiles, one
// whose extras fit is `x64:musttail-variable-arguments` and one whose
// extras need stack is `musttail:stack-args-grow`.
#[test]
fn variable_arguments_on_the_stack_are_refused_by_name() {
    let eight = ", i64 1, i64 2, i64 3, i64 4, i64 5, i64 6, i64 7, i64 8";
    for (triple, extra, want) in [
        ("aarch64", ", i32 42", None),
        ("aarch64", ", i32 42, double 1.0, fp128 0xL00000000000000004000000000000000", None),
        ("aarch64", eight, Some("musttail:stack-args-grow")),
        ("x86_64", "", None),
        ("x86_64", ", i32 42", Some("x64:musttail-variable-arguments")),
        ("x86_64", eight, Some("musttail:stack-args-grow")),
    ] {
        judged(triple, extra, want);
    }
}

fn judged(triple: &str, extra: &str, want: Option<&str>) {
    let text = format!(
        "target triple = \"{triple}-unknown-linux-gnu\"\n\
         declare i32 @printf(ptr, ...)\n\
         define i32 @h(ptr %f, ...) {{\n  %r = musttail call i32 (ptr, ...) @printf(ptr %f{extra}, ...)\n  \
         ret i32 %r\n}}\n"
    );
    let mut inputs = vec![text.as_bytes().to_vec()];
    inputs.extend(LLVM_MAJORS.into_iter().filter_map(|major| bitcode(major, &text)));
    for input in inputs {
        match (hod_object(&input), want) {
            (Ok(_), None) => {}
            (Err(CompileError::Unsupported(unsupported)), Some(want)) => {
                assert_eq!(unsupported.construct.name(), want, "{triple} {extra}");
            }
            (outcome, _) => panic!("{triple} {extra}: {:?}", outcome.map(|_| ())),
        }
    }
}
