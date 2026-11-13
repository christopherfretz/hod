//! Inline assembly with templates on x86-64 (WP-187, `spec/asm.md` §11,
//! `spec/codegen.md` §5.6.28): the forms of the census in one `.ll`
//! module (`tests/data/inline_asm/functions.ll`), compiled by hod
//! (plainly and stressed) and by `llc -O0 -fast-isel=0` of each LLVM
//! major present, each object linked with a C driver that checks every
//! function by what it computes (`tests/data/inline_asm/driver.c`) and
//! run under the emulator. Where the constraints force every register
//! (the `forced_*` functions) the template's bytes are `llc`'s. What is
//! not assembled is refused by name.

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

#[path = "common/int_gen.rs"]
mod int_gen;

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use hod_cg::{CgError, Stress};
use hod_testutil::{LLVM_MAJORS, Scratch, run, run_with, tool};
use hod_x64::barrier::{self, template};
use int_gen::{ll_header, llc, llc_of, parse, pic};
use linking::{Toolchain, exit_code};

const FUNCTIONS: &str = include_str!("data/inline_asm/functions.ll");
const DRIVER: &str = include_str!("data/inline_asm/driver.c");

fn hod_object(text: &str, stress: Stress) -> Vec<u8> {
    let (m, bodies) = parse(text);
    hod_x64::compile_to_object(&m, &bodies, &pic(stress)).unwrap_or_else(|e| panic!("hod: {e}"))
}

/// Every template, built by hod and by `llc`, runs the driver's checks,
/// dynamically and statically linked.
#[test]
fn templates_run_as_llc_builds_them() {
    let dir = Scratch::new("x64-inline-asm");
    let text = format!("{}{FUNCTIONS}", ll_header());
    let mut objects: Vec<(String, PathBuf)> = Vec::new();
    for (who, stress) in [("hod", Stress::default()), ("stress", Stress::ALL)] {
        objects.push((who.to_string(), dir.write(&format!("{who}.o"), hod_object(&text, stress))));
    }
    for major in LLVM_MAJORS {
        if let Some(theirs) = llc(&dir, major, &format!("llc-{major}"), &text) {
            objects.push((format!("llc-{major}"), theirs));
        }
    }
    let Some(toolchain) = Toolchain::x86_64(&dir) else { return };
    let driver = toolchain.compile_c(&dir, "driver", DRIVER, &[]);
    let linker = &toolchain.linkers[0];
    for (who, object) in &objects {
        for (how, flags) in [("dynamic", &[][..]), ("static", &["-static"][..])] {
            let program = dir.join(format!("{who}-{how}"));
            let ran = toolchain.link_and_run(linker, flags, &[&driver, object], &program);
            assert_eq!(
                exit_code(&ran),
                Some(0),
                "{who} {how}: {}{}",
                ran.stdout_text(),
                ran.stderr_text()
            );
            assert_eq!(ran.stdout_text(), "checked, 0 failures\n", "{who} {how}");
        }
    }
}

/// The `#APP` blocks of the `forced_*` functions in `llc`'s assembly.
fn forced_blocks(asm: &str) -> Vec<(String, String)> {
    let mut blocks = Vec::new();
    let mut function = String::new();
    let mut block: Option<String> = None;
    for line in asm.lines() {
        if let Some((name, _)) = line.split_once(':')
            && !line.starts_with(['.', ' ', '\t', '#'])
        {
            function = name.to_string();
        }
        match line.trim() {
            "#APP" => block = Some(String::new()),
            "#NO_APP" => {
                if let Some(b) = block.take()
                    && function.starts_with("forced_")
                {
                    blocks.push((function.clone(), b));
                }
            }
            t => {
                if let Some(b) = &mut block
                    && !t.starts_with('#')
                    && !t.is_empty()
                {
                    b.push_str(t);
                    b.push('\n');
                }
            }
        }
    }
    blocks
}

/// The `.text` bytes `llvm-mc-<major>` makes of `source`.
fn assembled(dir: &Scratch, major: u32, name: &str, source: &str) -> Option<Vec<u8>> {
    let mc = tool(&format!("llvm-mc-{major}"))?;
    let objcopy = tool(&format!("llvm-objcopy-{major}"))?;
    let src = dir.write(&format!("{name}.s"), source);
    let obj = dir.join(format!("{name}.o"));
    let bin = dir.join(format!("{name}.bin"));
    let out = run(
        &mc,
        [
            "--triple=x86_64-unknown-linux-gnu".as_ref(),
            "-filetype=obj".as_ref(),
            "-o".as_ref(),
            obj.as_os_str(),
            src.as_os_str(),
        ],
    );
    assert!(out.success(), "llvm-mc-{major}: {}", out.stderr_text());
    let out = run(
        &objcopy,
        [
            "-O".as_ref(),
            "binary".as_ref(),
            "--only-section=.text".as_ref(),
            obj.as_os_str(),
            bin.as_os_str(),
        ],
    );
    assert!(out.success(), "llvm-objcopy-{major}: {}", out.stderr_text());
    Some(std::fs::read(&bin).expect("objcopy output"))
}

/// Where every register is forced, the bytes of each template in hod's
/// object are the bytes `llc` writes for it.
#[test]
fn forced_templates_are_llcs_bytes() {
    let dir = Scratch::new("x64-inline-asm-bytes");
    let text = format!("{}{FUNCTIONS}", ll_header());
    let ours = hod_object(&text, Stress::default());
    let input = dir.write("functions.ll", &text);
    for major in LLVM_MAJORS {
        let Some(llc) = llc_of(major) else { continue };
        let mut command = Command::new(llc);
        command
            .args(["-O0", "-fast-isel=0", "-mtriple=x86_64-unknown-linux-gnu"])
            .args(["-relocation-model=pic", "-filetype=asm", "-o", "-"])
            .arg(&input);
        let ran = run_with(&mut command, &[], Duration::from_mins(5));
        assert!(ran.success(), "llc {major}: {}", ran.stderr_text());
        let blocks = forced_blocks(&ran.stdout_text());
        assert!(blocks.len() >= 12, "llc {major}: {} blocks", blocks.len());
        for (n, (function, block)) in blocks.iter().enumerate() {
            let Some(bytes) = assembled(&dir, major, &format!("b{major}-{n}"), block) else {
                return;
            };
            assert!(!bytes.is_empty(), "{function}: {block}");
            let found = ours.windows(bytes.len()).any(|w| w == bytes.as_slice());
            assert!(found, "llc {major} {function}: {bytes:02x?} not in hod's object\n{block}");
        }
        println!("llc {major}: {} forced templates, hod's bytes", blocks.len());
    }
}

/// What is not assembled, or not read, is refused by name.
#[test]
fn what_is_not_assembled_is_refused_by_name() {
    let one = |ret: &str, call: &str| {
        format!(
            "{}define {ret} @f(ptr %p, i32 %x) {{\n  {call}\n  ret {ret} {}\n}}\n",
            ll_header(),
            if ret == "void" { "" } else { "%r" }
        )
    };
    let cases = [
        (one("void", "call void asm inteldialect \"nop\", \"\"()"), template::INTEL),
        (one("void", "call void asm \"nop\", \"{rsp}\"(i32 %x)"), template::FRAME_REGISTER),
        (one("void", "call void asm \"nop\", \"~{rbp}\"()"), template::FRAME_REGISTER),
        (one("void", "call void asm \"nop\", \"~{ebp}\"()"), template::FRAME_REGISTER),
        (one("i32", "%r = call i32 asm \"movl $1, $0\", \"=r,r\"(i1 0)"), template::OPERAND_TYPE),
        (one("i32", "%r = call i32 asm \"movl $1, $0\", \"=r,0\"(i64 0)"), template::OPERAND_TYPE),
        (
            one("void", "call void asm \"nop\", \"r\"({ i32, i32 } zeroinitializer)"),
            template::OPERAND_TYPE,
        ),
        // `llc` refuses the letters clang spells `{ax}` and the like.
        (one("void", "call void asm \"nop\", \"a\"(i32 %x)"), "asm:constraint/a"),
        (one("void", "call void asm \"nop\", \"D\"(i32 %x)"), "asm:constraint/d"),
        (one("void", "call void asm \"nop\", \"g\"(i32 %x)"), "asm:constraint/g"),
        (one("void", "call void asm \"nop\", \"t\"(i32 %x)"), "asm:constraint/t"),
        (one("void", "call void asm \"nop\", \"{ah}\"(i32 %x)"), "asm:constraint/ah"),
        (one("void", "call void asm \"nop\", \"{st}\"(i32 %x)"), "asm:constraint/st"),
        (one("void", "call void asm \"nop\", \"~{k1}\"()"), "asm:clobber/k1"),
        (one("void", "call void asm \"nop\", \"i\"(i32 %x)"), "asm:constraint/i"),
        (one("void", "call void asm \"nop\", \"I\"(i32 99)"), "asm:constraint/range"),
        (one("void", "call void asm \"nop\", \"i\"(ptr @f)"), "asm:operand/symbol"),
        (one("void", "call void asm \"frob $0\", \"r\"(i32 %x)"), "asm:mnemonic/frob"),
        (one("void", "call void asm \"vzeroupper\", \"\"()"), "asm:mnemonic/vzeroupper"),
        (one("void", "call void asm \"movb ${0:h}, %al\", \"r\"(i32 %x)"), "asm:modifier/h"),
        (one("void", "call void asm \"call foo\", \"\"()"), "asm:mnemonic/call"),
        (one("void", "call void asm \"x: jmp x\", \"\"()"), "asm:label"),
        (one("void", "call void asm \".p2align 4\", \"\"()"), "asm:directive/p2align"),
        (one("void", "call void asm \"jmp 1f\", \"\"()"), "asm:operand/label"),
        (one("void", "call void asm \"nop $1\", \"r\"(i32 %x)"), "asm:template"),
        (one("i32", "%r = call i32 asm \"nop\", \"=r,r\"()"), "asm:constraint/count"),
        (one("void", "call void asm \"nop\", \"\"() [ \"a\"(i32 0) ]"), barrier::BUNDLE),
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

/// `unwind` inline assembly called by `invoke` is refused; without
/// `unwind` the template cannot throw and the call continues at the
/// normal destination.
#[test]
fn an_invoke_of_a_template() {
    let module = |flags: &str| {
        format!(
            "{}declare i32 @hod_test_personality(...)\n\
             define i32 @f(i32 %x) personality ptr @hod_test_personality {{\n\
             entry:\n  %r = invoke i32 asm {flags}\"movl $1, $0\", \"=r,r\"(i32 %x)\n\
             \x20         to label %ok unwind label %lp\nok:\n  ret i32 %r\n\
             lp:\n  %l = landingpad {{ ptr, i32 }} cleanup\n  ret i32 0\n}}\n",
            ll_header()
        )
    };
    let (m, bodies) = parse(&module("sideeffect unwind "));
    match hod_x64::compile_to_object(&m, &bodies, &pic(Stress::default())) {
        Err(hod_x64::ModuleError::Function { error, .. }) => {
            assert_eq!(error, CgError::unsupported(template::UNWIND));
        }
        other => panic!("not refused: {:?}", other.map(|o| o.len())),
    }
    hod_object(&module("sideeffect "), Stress::default());
    hod_object(&module("sideeffect "), Stress::ALL);
}

/// A `callbr` whose template is not empty stays refused.
#[test]
fn a_callbr_with_a_template_stays_refused() {
    let text = format!(
        "{}define void @f() {{\nentry:\n  callbr void asm \"jmp ${{0:l}}\", \"!i\"()\n\
         \x20         to label %a [label %b]\na:\n  ret void\nb:\n  ret void\n}}\n",
        ll_header()
    );
    let (m, bodies) = parse(&text);
    match hod_x64::compile_to_object(&m, &bodies, &pic(Stress::default())) {
        Err(hod_x64::ModuleError::Function { error, .. }) => {
            assert_eq!(error, CgError::unsupported(barrier::INLINE_ASM));
        }
        other => panic!("not refused: {:?}", other.map(|o| o.len())),
    }
}

/// `classify` accepts every template of the module on x86-64, so the
/// pipeline hands them to the target.
#[test]
fn classify_accepts_every_template() {
    let (m, bodies) = parse(&format!("{}{FUNCTIONS}", ll_header()));
    let classifier = hod_ir::Classifier::new(hod_ir::Support::shipped(hod_ir::Arch::X86_64));
    classifier.module(&m).unwrap_or_else(|r| panic!("module: {}", r.construct));
    for body in &bodies {
        classifier.body(&m, body).unwrap_or_else(|r| panic!("body: {}", r.construct));
    }
}
