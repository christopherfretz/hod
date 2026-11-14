//! Inline assembly with instruction templates (WP-103, `spec/asm.md`,
//! `spec/codegen.md` §5.5.3 "WP-103"): the seeds of
//! `tests/data/inline_asm/functions.ll` (syscalls, system registers, a
//! hand-written atomic, explicit registers, tied and memory operands,
//! clobbers of registers hod uses and of callee-saved ones, flag outputs,
//! immediates, several outputs, a call from the template, `asm goto`)
//! are compiled by hod and by `llc -O0 -global-isel=0 -fast-isel=0` of
//! each LLVM major present, under the plain, stress and far options;
//! each object is linked with `tests/data/inline_asm/driver.c` under
//! every linker, dynamic and static, and run. The template's words are
//! compared with `llc`'s for the same text. What is not supported is
//! refused by name.

mod common;

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use common::linking::{Toolchain, exit_code};
use common::{assert_refused, code_words, emit, ll_header, parse, write_object};
use hod_a64::inline_asm;
use hod_cg::{Options, Stress};
use hod_testutil::{LLVM_MAJORS, Scratch, run_with};

const FUNCTIONS: &str = include_str!("data/inline_asm/functions.ll");
const DRIVER: &str = include_str!("data/inline_asm/driver.c");

fn llc(dir: &Scratch, major: u32) -> Option<PathBuf> {
    let llc = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/llc"));
    if !llc.is_file() {
        return None;
    }
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

fn options() -> [(&'static str, Options); 3] {
    [
        ("plain", Options::default()),
        ("stress", Options { stress: Stress::ALL, ..Options::default() }),
        (
            "far",
            Options {
                stress: Stress { min_pool: false, far: true, displace: false, coro_frame: false },
                ..Options::default()
            },
        ),
    ]
}

// §5.5.3 "WP-103": every seed, built by hod (plain, stress, far) and by
// llc, runs the driver's checks under every linker, dynamic and static.
#[test]
fn seeds_run_as_llc_builds_them() {
    let dir = Scratch::new("a64-inline-asm");
    let (m, bodies) = parse(&format!("{}{FUNCTIONS}", ll_header(22)));
    let mut objects = Vec::new();
    for (name, options) in options() {
        let emitted = emit(&m, &bodies, &options);
        objects
            .push((format!("hod-{name}"), write_object(&dir, &format!("hod-{name}.o"), &emitted)));
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
                // LLVM 19 reloads a `callbr` output on the indirect edge
                // before it stores it (`spec/codegen.md` §5.5.3); 22 and
                // hod store it first.
                if who == "llc-19" {
                    assert_eq!(
                        ran.stdout_text(),
                        "FAIL goto_out2(0) == 501\nchecked, 1 failures\n",
                        "{who} {}",
                        linker.name
                    );
                    continue;
                }
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

// spec/asm.md §7: the template's words are hod_asm's, which are
// llvm-mc's: a template of fixed registers appears in hod's code as it
// does in llc's.
#[test]
fn template_words_appear_in_the_code() {
    let ll = "define i64 @f(i64 %x) {\n  %r = call i64 asm \"add x10, x9, #1\\0A eor x10, x10, x9, lsl #3\\0A dmb ish\", \"={x10},{x9}\"(i64 %x)\n  ret i64 %r\n}\n";
    let (m, bodies) = parse(&format!("{}{ll}", ll_header(22)));
    let words = code_words(&m, &bodies, &Options::default());
    // llvm-mc: add x10, x9, #1; eor x10, x10, x9, lsl #3; dmb ish.
    let want = [0x9100_052a_u32, 0xca09_0d4a, 0xd503_3bbf];
    assert!(
        words.windows(3).any(|w| w == want),
        "the template's words are not in the code: {words:08x?}"
    );
}

// §5.5.3 "WP-103": what is not supported is refused by name.
#[test]
fn what_is_not_supported_is_refused_by_name() {
    let one = |ret: &str, call: &str| {
        format!(
            "define {ret} @f(ptr %p, i64 %x) {{\n  {call}\n  ret {ret} {}\n}}\n",
            if ret == "void" { "" } else { "%r" }
        )
    };
    let cases = [
        (one("void", "call void asm \"frob x0\", \"\"()"), "asm:mnemonic/frob"),
        (one("void", "call void asm \"prfm pldl1keep, [$0]\", \"r\"(ptr %p)"), "asm:mnemonic/prfm"),
        (one("void", "call void asm \".inst 0xd503201f\", \"\"()"), "asm:directive/inst"),
        (one("i64", "%r = call i64 asm \"mrs $0, midr_el2\", \"=r\"()"), "asm:operand/sysreg"),
        (one("void", "call void asm \"x: nop\", \"\"()"), "asm:label"),
        (one("void", "call void asm \"bl nowhere\", \"~{x30}\"()"), "asm:symbol"),
        (one("void", "call void asm \"nop\", \"{sp}\"(i64 %x)"), inline_asm::FRAME_REGISTER),
        (one("void", "call void asm \"nop\", \"~{x29}\"()"), inline_asm::FRAME_REGISTER),
        (one("void", "call void asm \"nop\", \"{x16}\"(i64 %x)"), inline_asm::SCRATCH_REGISTER),
        (one("void", "call void asm \"nop\", \"g\"(i64 %x)"), "asm:constraint/g"),
        (one("void", "call void asm \"nop\", \"~{dirflag}\"()"), "asm:clobber/dirflag"),
        (one("void", "call void asm \"nop $0\", \"i\"(i64 %x)"), "asm:constraint/i"),
        (one("void", "call void asm \"nop\", \"r\"(i128 0)"), inline_asm::OPERAND_TYPE),
        (
            one("i32", "%r = call i32 asm \"cmp $1, $1 // $0\", \"={@cceq},r\"(i64 %x)"),
            "asm:template",
        ),
        (one("void", "call void asm \"nop $1\", \"r\"(i64 %x)"), "asm:template"),
        (one("void", "call void asm inteldialect \"nop\", \"\"()"), "asm:dialect"),
        (one("void", "call void asm \"cas x0, x1, [x2]\", \"\"()"), "asm:feature"),
    ];
    let cases: Vec<(&str, &'static str)> = cases.iter().map(|(t, r)| (t.as_str(), *r)).collect();
    assert_refused(&cases);
}

// spec/asm.md §6: `target-features` turn on the instructions they name.
#[test]
fn features_come_from_the_function() {
    let ll = "define void @f(ptr %p) #0 {\n  call void asm sideeffect \"ldadd x0, x1, [$0]\", \"r,~{x1},~{memory}\"(ptr %p)\n  ret void\n}\nattributes #0 = { \"target-features\"=\"+lse\" }\n";
    let (m, bodies) = parse(&format!("{}{ll}", ll_header(22)));
    let words = code_words(&m, &bodies, &Options::default());
    // ldadd x0, x1, [x9]: the pointer's register is x9.
    assert!(words.contains(&0xf820_0121), "{words:08x?}");
}
