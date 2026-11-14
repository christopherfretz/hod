//! Stack protectors, `ssp`, `sspstrong` and `sspreq` (WP-183,
//! `spec/codegen.md` §5.6.24, `spec/ir.md` §5.7).
//!
//! - The modules of `tests/ir/stack_protector.ll` and
//!   `tests/ir/stack_protector_smash.ll` (which `xtask irdiff` runs against
//!   `llc`) give the same object from their text and from their bitcode by
//!   every `llvm-as` present, on both targets.
//! - A protected function's object calls `__stack_chk_fail` (and reads
//!   `__stack_chk_guard` on AArch64); the same module without the
//!   attributes names neither.
//! - A protected presplit coroutine is built (its resume and destroy
//!   stubs store the guard again) and calls `__stack_chk_fail`; the
//!   coroutine plan refuses before the guard does any coroutine that would
//!   need the low guard (`ssp:coroutine`).

use std::time::Duration;

use hod::{Options, OutputKind, Report, Settings};
use hod_testutil::{LLVM_MAJORS, run_with, tool};

const TARGETS: [(&str, &str); 2] = [
    ("aarch64-unknown-linux-gnu", hod_ir::AARCH64_LINUX_DATA_LAYOUT),
    ("x86_64-unknown-linux-gnu", hod_ir::X86_64_LINUX_DATA_LAYOUT),
];

const SEEDS: [&str; 2] = [
    include_str!("../../../tests/ir/stack_protector.ll"),
    include_str!("../../../tests/ir/stack_protector_smash.ll"),
];

fn with_target(body: &str, triple: &str, layout: &str) -> String {
    format!("target datalayout = \"{layout}\"\ntarget triple = \"{triple}\"\n{body}")
}

fn try_compile(input: &[u8]) -> Result<Vec<u8>, hod::CompileError> {
    let options = Options { output_kind: OutputKind::Object, ..Options::default() };
    hod::compile_with(input, &options, &Settings::default(), &mut Report::default())
}

fn compile(input: &[u8]) -> Vec<u8> {
    try_compile(input).unwrap_or_else(|error| panic!("hod declined: {error}"))
}

fn names(object: &[u8], name: &str) -> bool {
    object.windows(name.len() + 1).any(|w| w.starts_with(name.as_bytes()) && w[name.len()] == 0)
}

#[test]
fn text_and_bitcode_give_the_same_object() {
    for seed in SEEDS {
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
}

#[test]
fn a_protected_function_calls_the_failure_routine() {
    let body = "declare void @g(ptr)\n\
                define void @f() ATTR {\n  %b = alloca [16 x i8], align 1\n  \
                call void @g(ptr %b)\n  ret void\n}\n";
    for (triple, layout) in TARGETS {
        for attr in ["ssp", "sspstrong", "sspreq"] {
            let text = with_target(&body.replace("ATTR", attr), triple, layout);
            let object = compile(text.as_bytes());
            assert!(names(&object, "__stack_chk_fail"), "{triple} {attr}");
            let guard = triple.starts_with("aarch64");
            assert_eq!(names(&object, "__stack_chk_guard"), guard, "{triple} {attr}");
        }
        let text = with_target(&body.replace("ATTR", "nounwind"), triple, layout);
        let object = compile(text.as_bytes());
        assert!(!names(&object, "__stack_chk_fail"), "{triple}");
        assert!(!names(&object, "__stack_chk_guard"), "{triple}");
    }
}

#[test]
fn a_protected_presplit_coroutine_is_built() {
    let seed = include_str!("../../../tests/ir/coro/done-22.ll");
    let protected =
        seed.replace("presplitcoroutine uwtable", "presplitcoroutine sspstrong uwtable");
    assert_ne!(protected, seed, "the seed's attribute group moved");
    for (triple, layout) in TARGETS {
        let object = compile(with_target(&protected, triple, layout).as_bytes());
        assert!(names(&object, "__stack_chk_fail"), "{triple}");
        assert!(names(&object, "_Z4uptoi.resume"), "{triple}");
        let plain = compile(with_target(seed, triple, layout).as_bytes());
        assert!(!names(&plain, "__stack_chk_fail"), "{triple}");
    }
}
