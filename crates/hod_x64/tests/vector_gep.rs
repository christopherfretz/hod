//! `getelementptr` with vector operands on x86-64 (WP-234,
//! `spec/codegen.md` §5.6.4 "Memory"): vector and scalar
//! bases and indices, several indices (array and struct strides, struct
//! fields, a splat field index), element sizes that are and are not
//! powers of two, indices of 8, 16, 32, 33, 64 and 128 bits, 1, 2, 3, 4
//! and 8 lanes, a scale and an offset beyond 32 bits, the result stored,
//! returned, passed by value and read by `extractelement`, and constant
//! vector `getelementptr` expressions folded lane by lane, in one module
//! (`tests/data/vector_gep/functions.ll`, shared with AArch64). The
//! module is compiled by hod (plainly and under `Stress::ALL`) and by
//! `llc -O0 -fast-isel=0` of each LLVM major present; each object is
//! linked with a C driver that computes every expected address itself
//! (`tests/data/vector_gep/driver.c`) under every linker, dynamic and
//! static, and run. The two reduced modules of the libc++ census are
//! compiled as they were found; what stays refused is refused by name.

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

#[path = "common/int_gen.rs"]
mod int_gen;

use std::path::PathBuf;

use hod_cg::{CgError, Stress};
use hod_testutil::{LLVM_MAJORS, Scratch};
use int_gen::{ll_header, llc, parse, pic};
use linking::{Toolchain, exit_code};

const FUNCTIONS: &str = include_str!("data/vector_gep/functions.ll");
const DRIVER: &str = include_str!("data/vector_gep/driver.c");

/// Every form, built by hod (plain and stressed) and by `llc`, gives the
/// addresses the driver computes, under every linker, dynamic and static.
#[test]
#[ignore = "WP-234: llc 22 does not finish on this module in 30 s (hod builds it); to narrow"]
fn vector_geps_run_as_c_computes_them() {
    let dir = Scratch::new("x64-vector-gep");
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

/// The reduced modules of the libc++ census (kiln `-O2`), one per
/// target, as they were found: compiled plainly and stressed.
#[test]
fn the_reduced_census_modules_compile() {
    let header = "target datalayout = \"e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-\
                  i128:128-f80:128-n8:16:32:64-S128\"\n\
                  target triple = \"x86_64-unknown-linux-gnu\"\n";
    for index in ["<2 x i64> <i64 24, i64 4>", "<2 x i64> <i64 -24, i64 -4>"] {
        let text = format!(
            "{header}define <2 x ptr> @f() {{\n  \
             %1 = getelementptr i8, <2 x ptr> zeroinitializer, {index}\n  \
             ret <2 x ptr> %1\n}}\n"
        );
        let (m, bodies) = parse(&text);
        for stress in [Stress::default(), Stress::ALL] {
            hod_x64::compile_to_object(&m, &bodies, &pic(stress))
                .unwrap_or_else(|e| panic!("{e}\n{text}"));
        }
    }
}

/// What a vector `getelementptr` does not lower is the vector group's
/// refusal: index lanes that are not addressable, more than 256 lanes.
#[test]
fn what_stays_refused_is_refused_by_name() {
    let one = |body: &str| {
        format!("{}define void @f(ptr %p, ptr %o) {{\n{body}\n  ret void\n}}\n", ll_header())
    };
    let cases = [
        (
            one("  %i = load <2 x i33>, ptr %p\n  \
                 %r = getelementptr i8, ptr %p, <2 x i33> %i\n  store <2 x ptr> %r, ptr %o"),
            hod_x64::vector::ODD_ELEMENT,
        ),
        (
            one("  %v = load <257 x ptr>, ptr %p\n  \
                 %r = getelementptr i8, <257 x ptr> %v, i64 1\n  store <257 x ptr> %r, ptr %o"),
            hod_x64::vector::LENGTH,
        ),
    ];
    for (text, name) in &cases {
        let (m, bodies) = parse(text);
        for stress in [Stress::default(), Stress::ALL] {
            match hod_x64::compile_to_object(&m, &bodies, &pic(stress)) {
                Err(hod_x64::ModuleError::Function { error, .. }) => {
                    assert_eq!(error, CgError::unsupported(name), "{text}");
                }
                other => panic!("not refused: {:?}\n{text}", other.map(|o| o.len())),
            }
        }
    }
}
