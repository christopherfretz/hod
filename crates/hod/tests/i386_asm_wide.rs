//! WP-347: inline-assembly operands that do not fit one general register
//! in 32-bit mode (`spec/codegen-x86.md` §10). `tests/data/i386/asm_wide`
//! built by `llc` 19 and 22, by hod and by `hod_x86` under
//! `hod_cg::Stress::ALL`, at CPU `generic` (no `target-cpu`) and `i686`,
//! static and PIE, run under `qemu-i386`: the runs agree. Then the
//! registers of a 64-bit input as the object shows them (equal to
//! `llc`'s), and what stays refused, by name.

use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

use hod::{Options, OutputKind};
use hod_testutil::target::Target;
use hod_testutil::{LLVM_MAJORS, Scratch, linkers};

mod i386_support;
use i386_support::{
    TRIPLE, back_end_object, checked, clang_module, llc_object, llvm_tool, run, tool,
};

/// hod's object of `text`, by the facade, PIC or static.
fn hod_object(text: &[u8], pic: bool) -> Vec<u8> {
    let options = Options {
        output_kind: OutputKind::Object,
        triple: Some(TRIPLE.into()),
        relocation_model: pic.then_some(hod::RelocModel::Pic),
        ..Options::default()
    };
    hod::compile(text, &options).unwrap_or_else(|e| panic!("hod: {e}"))
}

/// `hod_x86`'s object of `text` under `Stress::ALL`, PIC or static.
fn stressed(text: &[u8], pic: bool) -> Vec<u8> {
    let options = hod_cg::Options {
        reloc_model: if pic { hod_cg::RelocModel::Pic } else { hod_cg::RelocModel::Static },
        stress: hod_cg::Stress::ALL,
        ..hod_cg::Options::default()
    };
    back_end_object(text, &options)
}

#[test]
fn wide_operands_run_as_llc_s() {
    let target = Target::I686;
    let (Some(gcc), Some(qemu)) = (tool(&target.c_driver()), tool(&target.emulator())) else {
        return;
    };
    let linkers = linkers::present(target);
    let scratch = Scratch::new("i386-asm-wide");
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/i386/asm_wide/wide.c");
    let (mut runs, mut modules) = (0, 0);
    for major in LLVM_MAJORS {
        let (Some(clang), Some(llc)) = (llvm_tool(major, "clang"), llvm_tool(major, "llc")) else {
            continue;
        };
        for pic in [false, true] {
            let flags: &[&str] = if pic { &["-fpie"] } else { &["-fno-pic", "-fno-pie"] };
            let base = format!("wide-{major}-{}", if pic { "pie" } else { "static" });
            let ll = clang_module(&scratch, &clang, &source, flags, &base);
            let i686 = std::fs::read_to_string(&ll).unwrap();
            assert!(i686.contains("\"target-cpu\"=\"i686\""), "clang wrote no CPU");
            let generic = i686.replace("\"target-cpu\"=\"i686\"", "");
            for (cpu, text) in [("i686", i686.clone()), ("generic", generic)] {
                let name = format!("{base}-{cpu}");
                let ll = scratch.write(&format!("{name}.ll"), &text);
                let llc_flags: &[&str] = if pic { &["-relocation-model=pic"] } else { &[] };
                let llc_o = llc_object(&scratch, &llc, &ll, llc_flags, &format!("{name}-llc"));
                let hod_o =
                    scratch.write(&format!("{name}-hod.o"), hod_object(text.as_bytes(), pic));
                let stress_o =
                    scratch.write(&format!("{name}-stress.o"), stressed(text.as_bytes(), pic));
                modules += 1;
                for (linker, program) in &linkers {
                    let mut results = Vec::new();
                    for (who, object) in [("llc", &llc_o), ("hod", &hod_o), ("stress", &stress_o)] {
                        let exe = scratch.join(format!("{name}-{who}-{linker}"));
                        let mut command = Command::new(&gcc);
                        let flags: Vec<OsString> =
                            linkers::driver_flags(scratch.path(), linker, program);
                        command.args(flags).arg(if pic { "-pie" } else { "-no-pie" });
                        command.arg(object).arg("-o").arg(&exe);
                        checked(&format!("{linker} link of {name} ({who})"), &mut command);
                        results.push(run(&qemu, &exe));
                    }
                    assert_eq!(results[0].0, Some(0), "{name} ({linker}): llc's run failed");
                    for (who, result) in ["hod", "stress"].iter().zip(&results[1..]) {
                        assert_eq!(
                            &results[0],
                            result,
                            "{name} ({linker}): {who}'s run differs from llc's:\n{}\n---\n{}",
                            String::from_utf8_lossy(&results[0].1),
                            String::from_utf8_lossy(&result.1)
                        );
                    }
                    runs += 3;
                }
            }
        }
    }
    eprintln!("i386 asm wide: {modules} modules, {runs} runs, hod's and llc's equal");
}

/// Functions whose templates name 64-bit register inputs (`bswap` reads
/// and writes only its register): the registers `llc` gives them.
const REGISTERS: &str = r#"
define void @rm(i64 %v) nounwind {
  call void asm sideeffect "bswap $0", "r|m,~{memory}"(i64 %v)
  ret void
}
define void @int_first(i32 %a, i64 %v) nounwind {
  call void asm sideeffect "bswap $0; bswap $1", "r,r|m"(i32 %a, i64 %v)
  ret void
}
define void @int_last(i64 %v, i32 %a) nounwind {
  call void asm sideeffect "bswap $0; bswap $1", "r|m,r"(i64 %v, i32 %a)
  ret void
}
define void @clobber(i64 %v) nounwind {
  call void asm sideeffect "bswap $0", "r|m,~{eax}"(i64 %v)
  ret void
}
define void @byte_regs(i64 %v) nounwind {
  call void asm sideeffect "bswap $0", "q|m"(i64 %v)
  ret void
}
define void @three(i64 %a, i64 %b, i32 %c) nounwind {
  call void asm sideeffect "bswap $0; bswap $1; bswap $2", "r,r,r"(i64 %a, i64 %b, i32 %c)
  ret void
}
define void @six(i64 %a, i64 %b, i64 %c) nounwind {
  call void asm sideeffect "bswap $0; bswap $1; bswap $2", "r,r,r"(i64 %a, i64 %b, i64 %c)
  ret void
}
define void @constant() nounwind {
  call void asm sideeffect "bswap $0", "r"(i64 81985529216486895)
  ret void
}
define void @modifiers(i64 %v) nounwind {
  call void asm sideeffect "bswap ${0:k}; xchgw ${0:w}, ${0:w}", "r"(i64 %v)
  ret void
}
define void @loaded(ptr %p) nounwind {
  %v = load i64, ptr %p, align 4
  call void asm sideeffect "bswap $0", "r|m,~{memory},~{dirflag},~{fpsr},~{flags}"(i64 %v)
  ret void
}
"#;

/// The template instructions (`bswap`, `xchg`) of `object`, as
/// `llvm-objdump` prints them, without addresses.
fn template_lines(objdump: &Path, object: &Path) -> Vec<String> {
    let mut command = Command::new(objdump);
    command.args(["-d", "--no-show-raw-insn", "--no-leading-addr"]).arg(object);
    let outcome = checked("llvm-objdump", &mut command);
    String::from_utf8_lossy(&outcome.stdout)
        .lines()
        .filter(|l| l.contains("bswap") || l.contains("xchg") || l.ends_with(">:"))
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect()
}

#[test]
fn registers_of_wide_inputs_are_llc_s() {
    let scratch = Scratch::new("i386-asm-wide-regs");
    let text = format!("target triple = \"{TRIPLE}\"\n{REGISTERS}");
    let ll = scratch.write("regs.ll", &text);
    let hod_o = scratch.write("regs-hod.o", hod_object(text.as_bytes(), false));
    let stress_o = scratch.write("regs-stress.o", stressed(text.as_bytes(), false));
    let mut compared = 0;
    for major in LLVM_MAJORS {
        let (Some(llc), Some(objdump)) =
            (llvm_tool(major, "llc"), llvm_tool(major, "llvm-objdump"))
        else {
            continue;
        };
        let llc_o = llc_object(&scratch, &llc, &ll, &[], &format!("regs-llc-{major}"));
        let expected = template_lines(&objdump, &llc_o);
        assert!(expected.len() > 20, "too few template lines: {expected:?}");
        for (who, object) in [("hod", &hod_o), ("stress", &stress_o)] {
            assert_eq!(template_lines(&objdump, object), expected, "{who} against llc {major}");
            compared += 1;
        }
    }
    eprintln!("i386 asm wide: {compared} objects' template registers equal llc's");
}

/// The name `hod_x86` refuses `body` (one function) with.
fn refusal(body: &str) -> String {
    let text = format!("target triple = \"{TRIPLE}\"\n\n{body}\n");
    let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
        .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    let side = parsed.text;
    let mut module = parsed.module;
    let mut bodies = Vec::new();
    hod_ll::parse_bodies(&mut module, &side, |_, read| {
        bodies.push(read.body.expect("every body is read").clone());
        Ok::<(), hod_ll::LlError>(())
    })
    .unwrap();
    hod_x86::compile_to_object(&module, &bodies, &hod_cg::Options::default())
        .expect_err("the module is refused")
        .to_string()
}

#[test]
fn what_stays_refused_is_named() {
    let cases = [
        // A 64-bit register output, direct or tied.
        (
            "define i64 @f() nounwind {\n  %r = call i64 asm \"\", \"=r\"()\n  ret i64 %r\n}",
            "x86:inline-asm:operand-type",
        ),
        (
            "define i64 @f(i64 %v) nounwind {\n  %r = call i64 asm \"\", \"=r,0\"(i64 %v)\n  \
             ret i64 %r\n}",
            "x86:inline-asm:operand-type",
        ),
        // A `double` with `r` only (`llc` takes two registers).
        (
            "define void @f(double %v) nounwind {\n  call void asm sideeffect \"\", \"r\"(double \
             %v)\n  ret void\n}",
            "x86:inline-asm:operand-type",
        ),
        // An integer of 33 to 63 bits in a register.
        (
            "define void @f(ptr %p) nounwind {\n  %v = load i48, ptr %p\n  call void asm \
             sideeffect \"\", \"r\"(i48 %v)\n  ret void\n}",
            "x86:inline-asm:operand-type",
        ),
        // An indirect 64-bit register input (`llc` cannot either).
        (
            "define void @f(ptr %p) nounwind {\n  call void asm sideeffect \"\", \
             \"*r\"(ptr elementtype(i64) %p)\n  ret void\n}",
            "x86:inline-asm:operand-type",
        ),
        // Four 64-bit inputs: eight registers.
        (
            "define void @f(i64 %a) nounwind {\n  call void asm sideeffect \"\", \
             \"r,r,r,r\"(i64 %a, i64 %a, i64 %a, i64 %a)\n  ret void\n}",
            "x86:inline-asm:registers",
        ),
        // Three 64-bit inputs and `ebx` clobbered.
        (
            "define void @f(i64 %a) nounwind {\n  call void asm sideeffect \"\", \
             \"r,r,r,~{ebx}\"(i64 %a, i64 %a, i64 %a)\n  ret void\n}",
            "x86:inline-asm:registers",
        ),
    ];
    for (body, name) in cases {
        let e = refusal(body);
        assert!(e.contains(name), "{body}\nrefused as {e}, not {name}");
    }
}

/// The module of the libc++ run for i686 that found the gap compiles.
#[test]
fn do_not_optimize_of_an_i64_compiles() {
    let text = format!(
        "target triple = \"{TRIPLE}\"\ndefine void @f(ptr %p) {{\n  %v = load i64, ptr %p, \
         align 4\n  call void asm sideeffect \"\", \"r|m,~{{memory}},~{{dirflag}},~{{fpsr}},\
         ~{{flags}}\"(i64 %v)\n  ret void\n}}\n"
    );
    assert!(!hod_object(text.as_bytes(), false).is_empty());
    assert!(!stressed(text.as_bytes(), true).is_empty());
}
