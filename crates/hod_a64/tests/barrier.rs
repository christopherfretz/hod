//! Inline assembly whose template assembles to nothing (WP-075,
//! `spec/codegen.md` §5.5.1 "WP-075", `spec/ir.md` §2.3): the forms
//! front ends write for optimization barriers, and the alternatives and
//! tied operands whose effect is `llc`'s choice, in one `.ll` module
//! (`tests/data/barrier/functions.ll`). The module is compiled by hod and
//! by `llc -O0 -global-isel=0 -fast-isel=0` of each LLVM major present;
//! each object is linked with a C driver that checks every function
//! (`tests/data/barrier/driver.c`, which encodes what `llc` does) under
//! every linker, dynamic and static, and run. What is not a barrier is
//! refused by name.

mod common;

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use common::linking::{Toolchain, exit_code};
use common::{assert_refused, emit, ll_header, parse, write_object};
use hod_a64::barrier;
use hod_cg::Options;
use hod_testutil::{LLVM_MAJORS, Scratch, run_with};

const FUNCTIONS: &str = include_str!("data/barrier/functions.ll");
const DRIVER: &str = include_str!("data/barrier/driver.c");

fn llc_path(major: u32) -> Option<PathBuf> {
    let installed = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/llc"));
    installed.is_file().then_some(installed)
}

/// `llc`'s object of the functions, by `major`.
fn llc(dir: &Scratch, major: u32) -> Option<PathBuf> {
    let llc = llc_path(major)?;
    let input = dir.write(&format!("llc-{major}.ll"), format!("{}{FUNCTIONS}", ll_header(major)));
    let output = dir.join(format!("llc-{major}.o"));
    let mut command = Command::new(llc);
    command
        .args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj", "-relocation-model=pic"])
        .arg(&input)
        .arg("-o")
        .arg(&output);
    let outcome = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "llc-{major}: {}", outcome.stderr_text());
    Some(output)
}

// §5.5.1 "WP-075": every barrier form, built by hod and by llc, runs
// the driver's checks under every linker, dynamic and static.
#[test]
fn barriers_run_as_llc_builds_them() {
    let dir = Scratch::new("a64-barrier");
    let (m, bodies) = parse(&format!("{}{FUNCTIONS}", ll_header(22)));
    let emitted = emit(&m, &bodies, &Options::default());
    let ours = write_object(&dir, "hod.o", &emitted);
    let mut objects = vec![("hod".to_string(), ours)];
    for major in LLVM_MAJORS {
        if let Some(theirs) = llc(&dir, major) {
            objects.push((format!("llc-{major}"), theirs));
        }
    }
    let Some(toolchain) = Toolchain::host(&dir) else { return };
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

// spec/ir.md §2.3: `classify` accepts every barrier form, `elementtype`
// on an inline-assembly operand included (it is the operand's type, which
// the lowering reads), and so the pipeline hands them to the target.
#[test]
fn classify_accepts_every_barrier() {
    let (m, bodies) = parse(&format!("{}{FUNCTIONS}", ll_header(22)));
    let classifier = hod_ir::Classifier::new(hod_ir::Support::shipped(hod_ir::Arch::Aarch64));
    classifier.module(&m).unwrap_or_else(|r| panic!("module: {}", r.construct));
    for body in &bodies {
        classifier.body(&m, body).unwrap_or_else(|r| panic!("body: {}", r.construct));
    }
}

// §5.5.1 "WP-075": what is not a pure barrier, or not read, is refused
// by name.
#[test]
fn what_is_not_a_barrier_is_refused_by_name() {
    let one = |ret: &str, call: &str| {
        format!(
            "define {ret} @f(ptr %p, i32 %x) {{\n  {call}\n  ret {ret} {}\n}}\n",
            if ret == "void" { "" } else { "%r" }
        )
    };
    // WP-103: what the barrier module does not model goes to the
    // general inline-assembly sequence (`crate::inline_asm`), which
    // compiles these (an output nothing ties is the register's contents,
    // as with `llc`) ...
    let compiled = [
        one("i32", "%r = call i32 asm \"\", \"=r\"()"),
        one("void", "call void asm \"\", \"=*r\"(ptr elementtype(i32) %p)"),
        one("void", "call void asm \"\", \"=*rm,r\"(ptr elementtype(i32) %p, i32 %x)"),
        one("void", "%r = call { i32, i32 } asm \"\", \"=r,=r,0,1\"(i32 %x, i32 %x)"),
        one("void", "call void asm \"\", \"Q\"(ptr %p)"),
        one("i32", "%r = call i32 asm \"\", \"=r,0\"(i64 0)"),
        one("void", "call void asm \"\", \"=*r,0\"(ptr elementtype(i64) %p, i32 %x)"),
        one("void", "call void asm \"nop\", \"\"()"),
    ];
    for functions in &compiled {
        assert_eq!(common::compile_ll(functions), Ok(()), "{functions}");
    }
    // ... and refuses these by its names.
    let cases = [
        (one("void", "call void asm \"\", \"!i\"()"), "asm:constraint/count"),
        (one("void", "call void asm \"\", \"~{sp}\"()"), barrier::CLOBBER),
        (one("void", "call void asm \"\", \"~{x31}\"()"), barrier::CLOBBER),
        (one("void", "call void asm \"\", \"~{dirflag}\"()"), barrier::CLOBBER),
        (one("void", "call void asm \"\", \"i\"(i32 %x)"), "asm:constraint/i"),
        (one("void", "call void asm \"$$\", \"\"()"), "asm:mnemonic/other"),
    ];
    let cases: Vec<(&str, &'static str)> = cases.iter().map(|(t, r)| (t.as_str(), *r)).collect();
    assert_refused(&cases);
}
