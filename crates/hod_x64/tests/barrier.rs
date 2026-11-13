//! Inline assembly whose template assembles to nothing on x86-64 (WP-128,
//! `spec/codegen.md` §5.6.8, `spec/ir.md` §2.3): the forms front ends
//! write for optimization barriers, and the alternatives and tied
//! operands whose effect is `llc`'s choice, in one `.ll` module
//! (`tests/data/barrier/functions.ll`, the AArch64 group's with x86
//! constraint codes and clobbers, without floating point). The module is
//! compiled by hod (plainly and stressed) and by `llc -O0 -fast-isel=0`
//! of each LLVM major present; each object is linked with a C driver
//! that checks every function (`tests/data/barrier/driver.c`, which
//! encodes what `llc` does) under every linker, dynamic and static, and
//! run under the emulator. What is not a barrier is refused by name.

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

#[path = "common/int_gen.rs"]
mod int_gen;

use std::path::PathBuf;

use hod_cg::{CgError, Stress};
use hod_testutil::{LLVM_MAJORS, Scratch};
use hod_x64::barrier;
use int_gen::{ll_header, llc, parse, pic};
use linking::{Toolchain, exit_code};

const FUNCTIONS: &str = include_str!("data/barrier/functions.ll");
const DRIVER: &str = include_str!("data/barrier/driver.c");

/// Every barrier form, built by hod and by `llc`, runs the driver's
/// checks under every linker, dynamic and static.
#[test]
fn barriers_run_as_llc_builds_them() {
    let dir = Scratch::new("x64-barrier");
    let text = format!("{}{FUNCTIONS}", ll_header());
    let (m, bodies) = parse(&text);
    let mut objects: Vec<(String, PathBuf)> = Vec::new();
    for (who, stress) in [("hod", Stress::default()), ("stress", Stress::ALL)] {
        let object = hod_x64::compile_to_object(&m, &bodies, &pic(stress))
            .unwrap_or_else(|e| panic!("{who}: {e}"));
        objects.push((who.to_string(), dir.write(&format!("{who}.o"), object)));
    }
    for major in LLVM_MAJORS {
        if let Some(theirs) = llc(&dir, major, &format!("llc-{major}"), &text) {
            objects.push((format!("llc-{major}"), theirs));
        }
    }
    let Some(toolchain) = Toolchain::x86_64(&dir) else { return };
    let driver = toolchain.compile_c(&dir, "driver", DRIVER, &[]);
    for (who, object) in &objects {
        for linker in &toolchain.linkers {
            for (how, flags) in [("dynamic", &[][..]), ("static", &["-static"][..])] {
                let program = dir.join(format!("{who}-{}-{how}", linker.name));
                let ran = toolchain.link_and_run(linker, flags, &[&driver, object], &program);
                assert_eq!(
                    exit_code(&ran),
                    Some(0),
                    "{who} {} {how}: {}{}",
                    linker.name,
                    ran.stdout_text(),
                    ran.stderr_text()
                );
                assert_eq!(ran.stdout_text(), "checked, 0 failures\n", "{who} {}", linker.name);
            }
        }
    }
}

/// `classify` accepts every barrier form on x86-64, so the pipeline
/// hands them to the target.
#[test]
fn classify_accepts_every_barrier() {
    let (m, bodies) = parse(&format!("{}{FUNCTIONS}", ll_header()));
    let classifier = hod_ir::Classifier::new(hod_ir::Support::shipped(hod_ir::Arch::X86_64));
    classifier.module(&m).unwrap_or_else(|r| panic!("module: {}", r.construct));
    for body in &bodies {
        classifier.body(&m, body).unwrap_or_else(|r| panic!("body: {}", r.construct));
    }
}

/// What is not a pure barrier, or not read, is refused by name.
#[test]
fn what_is_not_a_barrier_is_refused_by_name() {
    let one = |ret: &str, call: &str| {
        format!(
            "{}define {ret} @f(ptr %p, i32 %x) {{\n  {call}\n  ret {ret} {}\n}}\n",
            ll_header(),
            if ret == "void" { "" } else { "%r" }
        )
    };
    let cases = [
        (one("i32", "%r = call i32 asm \"\", \"=r\"()"), barrier::OUTPUT),
        (one("void", "call void asm \"\", \"=*r\"(ptr elementtype(i32) %p)"), barrier::OUTPUT),
        (
            one("void", "%r = call { i32, i32 } asm \"\", \"=r,=r,0,1\"(i32 %x, i32 %x)"),
            barrier::OUTPUTS,
        ),
        (one("void", "call void asm \"\", \"!i\"()"), barrier::CONSTRAINT),
        (one("void", "call void asm \"\", \"y\"(i32 %x)"), barrier::CONSTRAINT),
        // Front ends spell `a` as `{ax}`; `llc` does not read `a`.
        (one("void", "call void asm \"\", \"a\"(i32 %x)"), barrier::CONSTRAINT),
        (one("void", "call void asm \"\", \"~{x0}\"()"), barrier::CLOBBER),
        (one("void", "call void asm \"\", \"~{xmm32}\"()"), barrier::CLOBBER),
        (one("void", "call void asm \"\", \"~{r16}\"()"), barrier::CLOBBER),
        (one("void", "call void asm \"\", \"i\"(i32 %x)"), barrier::OPERAND),
        (one("void", "call void asm \"\", \"I\"(i32 %x)"), barrier::OPERAND),
        (one("i32", "%r = call i32 asm \"\", \"=r,0\"(i64 0)"), barrier::OPERAND),
    ];
    for (text, name) in &cases {
        let (m, bodies) = parse(text);
        match hod_x64::compile_to_object(&m, &bodies, &pic(Stress::default())) {
            Err(hod_x64::ModuleError::Function { error, .. }) => {
                assert_eq!(error, CgError::unsupported(name), "{text}");
            }
            other => panic!("not refused: {:?}\n{text}", other.map(|o| o.len())),
        }
    }
}
