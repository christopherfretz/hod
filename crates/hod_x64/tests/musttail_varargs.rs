//! Variable arguments before a forwarded `...` in a `musttail` call on
//! x86-64 (WP-115, `spec/abi-x64.md` §8.5): refused as
//! `x64:musttail-variable-arguments`, because `llc` gives them no meaning
//! to follow. Its x86-64 lowering places them, then writes the forwarded
//! registers over them, and passes `al` as it arrived: the callee gets
//! the caller's own variable arguments where the new ones should be. The
//! second test pins that, so that a version of LLVM that changes it is
//! noticed (the refusal can then be revisited).

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

use std::process::Command;
use std::time::Duration;

use hod_cg::{CgError, Options, RelocModel};
use hod_ir::{Body, Module};
use hod_testutil::{LLVM_MAJORS, Scratch, run_with};
use linking::{Toolchain, exit_code};

fn header() -> String {
    format!(
        "target datalayout = \"{}\"\ntarget triple = \"x86_64-unknown-linux-gnu\"\n",
        hod_ir::X86_64_LINUX_DATA_LAYOUT
    )
}

fn parse(text: &str) -> (Module, Vec<Body>) {
    let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
        .unwrap_or_else(|e| panic!("hod_ll: {e}\n{text}"));
    let side = parsed.text;
    let mut module = parsed.module;
    let mut bodies = Vec::new();
    hod_ll::parse_bodies(&mut module, &side, |_, parsed| {
        bodies.push(parsed.body.expect("every body is read").clone());
        Ok::<(), hod_ll::LlError>(())
    })
    .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    (module, bodies)
}

fn compile(text: &str) -> Result<Vec<u8>, CgError> {
    let (module, bodies) = parse(&format!("{}{text}", header()));
    let options = Options { reloc_model: RelocModel::Pic, ..Options::default() };
    hod_x64::compile_to_object(&module, &bodies, &options).map_err(|e| match e {
        hod_x64::ModuleError::Function { error, .. } => error,
        other => panic!("{other}"),
    })
}

fn thunk(extra: &str) -> String {
    format!(
        "declare i32 @printf(ptr, ...)\n\
         define i32 @h(ptr %f, ...) {{\n  %r = musttail call i32 (ptr, ...) @printf(ptr %f{extra}, ...)\n  \
         ret i32 %r\n}}\n"
    )
}

// §8.5: refused by name, whatever the class of the extras; the thunk
// without them compiles.
#[test]
fn variable_arguments_before_the_forwarded_ones_are_refused() {
    assert!(compile(&thunk("")).is_ok());
    for extra in [", i32 42", ", double 1.5", ", i64 1, i64 2, i64 3, i64 4, i64 5, i64 6"] {
        assert_eq!(
            compile(&thunk(extra)).map(|_| ()),
            Err(CgError::unsupported(hod_x64::call::MUSTTAIL_VARIABLE_ARGUMENTS)),
            "{extra}"
        );
    }
    assert!(hod_x64::call::UNSUPPORTED.contains(&hod_x64::call::MUSTTAIL_VARIABLE_ARGUMENTS));
}

const PROGRAM: &str = r#"
declare i32 @printf(ptr, ...)
@fmt = private constant [12 x i8] c"%d %.2f %d\0A\00"

define internal i32 @h(ptr %f, ...) noinline {
  %r = musttail call i32 (ptr, ...) @printf(ptr %f, i32 42, double 1.5, ...)
  ret i32 %r
}

define i32 @main() {
  call i32 (ptr, ...) @h(ptr @fmt, i32 7, double 2.5, i32 8)
  ret i32 0
}
"#;

// The reason of §8.5, as `llc` 19 and 22 build it, run under every
// linker: the callee prints the caller's own `7 2.50 8`, not `42 1.50`.
#[test]
fn llc_writes_the_forwarded_registers_over_the_variable_arguments() {
    let dir = Scratch::new("x64-musttail-varargs");
    let Some(toolchain) = Toolchain::x86_64(&dir) else { return };
    let input = dir.write("p.ll", format!("{}{PROGRAM}", header()));
    let mut runs = 0;
    for major in LLVM_MAJORS {
        let llc = std::path::PathBuf::from(format!("/usr/lib/llvm-{major}/bin/llc"));
        if !llc.is_file() {
            assert!(!hod_testutil::tools_required(), "llc {major} is required");
            continue;
        }
        let object = dir.join(format!("llc{major}.o"));
        let mut command = Command::new(llc);
        command.args(["-O0", "-fast-isel=0", "-relocation-model=pic", "-filetype=obj"]);
        command.arg(&input).arg("-o").arg(&object);
        let built = run_with(&mut command, &[], Duration::from_mins(2));
        assert!(built.success(), "llc {major}: {}", built.stderr_text());
        for linker in &toolchain.linkers {
            let exe = dir.join(format!("llc{major}-{}", linker.name));
            let ran = toolchain.link_and_run(linker, &["-pie"], &[&object], &exe);
            assert_eq!(exit_code(&ran), Some(0), "llc {major} {}", linker.name);
            assert_eq!(ran.stdout_text(), "7 2.50 8\n", "llc {major} {}", linker.name);
            runs += 1;
        }
    }
    eprintln!("{runs} runs");
}
