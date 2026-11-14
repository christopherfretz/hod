//! `getelementptr` with vector operands on AArch64 (WP-234,
//! `spec/codegen.md` §5.5 "Memory (WP-036)"): the module and C
//! driver of `hod_x64`'s `tests/vector_gep.rs`
//! (`hod_x64/tests/data/vector_gep/`), compiled by hod (plainly and under
//! `Stress::ALL`) and by `llc -O0 -global-isel=0 -fast-isel=0` of each
//! LLVM major present, linked with the driver (which computes every
//! expected address itself) under every linker, dynamic and static, and
//! run. The two reduced modules of the libc++ census are compiled as
//! they were found; what stays refused is refused by name.

mod common;

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use common::linking::{Toolchain, exit_code};
use common::{assert_refused, emit, ll_header, parse, write_object};
use hod_cg::{Options, RelocModel, Stress};
use hod_testutil::{LLVM_MAJORS, Scratch, run_with};

const FUNCTIONS: &str = include_str!("../../hod_x64/tests/data/vector_gep/functions.ll");
const DRIVER: &str = include_str!("../../hod_x64/tests/data/vector_gep/driver.c");
/// The initializers as hod builds them (constant vector GEPs).
const INITIALIZERS: &str = include_str!("../../hod_x64/tests/data/vector_gep/initializers.ll");
/// The same addresses one lane at a time: `llc` cannot build the vector
/// forms (see the file's comment).
const INITIALIZERS_BY_LANE: &str =
    include_str!("../../hod_x64/tests/data/vector_gep/initializers_by_lane.ll");

fn llc(dir: &Scratch, major: u32) -> Option<PathBuf> {
    let llc = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/llc"));
    if !llc.is_file() {
        return None;
    }
    let input = dir.write(
        &format!("llc-{major}.ll"),
        format!("{}{FUNCTIONS}{INITIALIZERS_BY_LANE}", ll_header(major)),
    );
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

fn pic(stress: Stress) -> Options {
    Options { reloc_model: RelocModel::Pic, stress, ..Options::default() }
}

/// Every form, built by hod (plain and stressed) and by `llc`, gives the
/// addresses the driver computes, under every linker, dynamic and static.
#[test]
fn vector_geps_run_as_c_computes_them() {
    let dir = Scratch::new("a64-vector-gep");
    let (m, bodies) = parse(&format!("{}{FUNCTIONS}{INITIALIZERS}", ll_header(22)));
    let mut objects = Vec::new();
    for (who, stress) in [("hod", Stress::default()), ("stress", Stress::ALL)] {
        let emitted = emit(&m, &bodies, &pic(stress));
        objects.push((who.to_string(), write_object(&dir, &format!("{who}.o"), &emitted)));
    }
    for major in LLVM_MAJORS {
        if let Some(theirs) = llc(&dir, major) {
            objects.push((format!("llc-{major}"), theirs));
        }
    }
    let Some(toolchain) = Toolchain::aarch64(&dir) else { return };
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

/// The reduced modules of the libc++ census (kiln `-O2`), one per
/// target, as they were found: compiled plainly and stressed.
#[test]
fn the_reduced_census_modules_compile() {
    let header = "target datalayout = \"e-m:e-p270:32:32-p271:32:32-p272:64:64-i8:8:32-\
                  i16:16:32-i64:64-i128:128-n32:64-S128-Fn32\"\n\
                  target triple = \"aarch64-unknown-linux-gnu\"\n";
    for index in ["<2 x i64> <i64 -24, i64 -4>", "<2 x i64> <i64 24, i64 4>"] {
        let text = format!(
            "{header}define <2 x ptr> @f() {{\n  \
             %1 = getelementptr i8, <2 x ptr> zeroinitializer, {index}\n  \
             ret <2 x ptr> %1\n}}\n"
        );
        let (m, bodies) = parse(&text);
        for stress in [Stress::default(), Stress::ALL] {
            emit(&m, &bodies, &pic(stress));
        }
    }
}

/// What a vector `getelementptr` does not lower is the vector group's
/// refusal: index lanes that are not addressable, more than 256 lanes.
#[test]
fn what_stays_refused_is_refused_by_name() {
    assert_refused(&[
        (
            "define void @f(ptr %p, ptr %o) {\n  %i = load <2 x i33>, ptr %p\n  \
             %r = getelementptr i8, ptr %p, <2 x i33> %i\n  store <2 x ptr> %r, ptr %o\n  \
             ret void\n}\n",
            hod_a64::vector::ODD_ELEMENT,
        ),
        (
            "define void @f(ptr %p, ptr %o) {\n  %v = load <257 x ptr>, ptr %p\n  \
             %r = getelementptr i8, <257 x ptr> %v, i64 1\n  store <257 x ptr> %r, ptr %o\n  \
             ret void\n}\n",
            hod_a64::vector::LENGTH,
        ),
    ]);
}
