//! WP-301 (design 0008's I13): inline assembly in 32-bit mode. The
//! programs of `tests/data/i386/asm` built by `llc` 19 and 22, by hod and
//! by `hod_x86` under `hod_cg::Stress::ALL`, run under `qemu-i386`: the
//! runs agree. Then what stays refused, by name (`spec/asm.md` §22).

use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

use hod::{Options, OutputKind};
use hod_testutil::target::Target;
use hod_testutil::{LLVM_MAJORS, Scratch, linkers};

mod i386_support;
use i386_support::{
    TRIPLE, back_end_object, checked, clang_module, llc_object, llvm_tool, run, stressed_object,
    tool,
};

/// The programs of `tests/data/i386/asm`.
const PROGRAMS: [&str; 3] = ["regs.c", "memory.c", "pressure.c"];

#[test]
fn asm_programs_run_as_llc_s() {
    let target = Target::I686;
    let (Some(gcc), Some(qemu)) = (tool(&target.c_driver()), tool(&target.emulator())) else {
        return;
    };
    let linkers = linkers::present(target);
    let scratch = Scratch::new("i386-asm");
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/i386/asm");
    let (mut runs, mut programs) = (0, 0);
    for major in LLVM_MAJORS {
        let (Some(clang), Some(llc)) = (llvm_tool(major, "clang"), llvm_tool(major, "llc")) else {
            continue;
        };
        for program in PROGRAMS {
            // Without a CPU attribute (`generic`) and with one.
            for (tag, flags) in [
                ("plain", &["-fno-pic", "-fno-pie"][..]),
                ("p4", &["-fno-pic", "-fno-pie", "-march=pentium4"]),
            ] {
                let name = format!("{program}-{major}-{tag}");
                let ll = clang_module(&scratch, &clang, &dir.join(program), flags, &name);
                let text = std::fs::read(&ll).unwrap();
                let options = Options {
                    output_kind: OutputKind::Object,
                    triple: Some(TRIPLE.into()),
                    ..Options::default()
                };
                let object =
                    hod::compile(&text, &options).unwrap_or_else(|e| panic!("hod {name}: {e}"));
                let hod_o = scratch.write(&format!("{name}-hod.o"), object);
                let stress_o = scratch.write(&format!("{name}-stress.o"), stressed_object(&text));
                let llc_o = llc_object(&scratch, &llc, &ll, &[], &format!("{name}-llc"));
                programs += 1;
                for (linker, program_path) in &linkers {
                    let mut results = Vec::new();
                    for (who, object) in [("llc", &llc_o), ("hod", &hod_o), ("stress", &stress_o)] {
                        let exe = scratch.join(format!("{name}-{who}-{linker}"));
                        let mut command = Command::new(&gcc);
                        let flags: Vec<OsString> =
                            linkers::driver_flags(scratch.path(), linker, program_path);
                        command.args(flags).arg("-no-pie").arg(object).arg("-o").arg(&exe);
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
    eprintln!("i386 asm: {programs} modules, {runs} runs, hod's and llc's equal");
}

/// The name `hod_x86` refuses the module `body` (one function) with,
/// under the default options and under `Stress::ALL`.
fn refusal(body: &str) -> String {
    let text = format!("target triple = \"{TRIPLE}\"\n\n{body}\n");
    let mut names = Vec::new();
    for stress in [hod_cg::Stress::default(), hod_cg::Stress::ALL] {
        let options = hod_cg::Options { stress, ..hod_cg::Options::default() };
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
        let e = hod_x86::compile_to_object(&module, &bodies, &options)
            .expect_err("the module is refused");
        names.push(e.to_string());
    }
    assert_eq!(names[0], names[1], "the stress mode changes the refusal");
    names.swap_remove(0)
}

fn assert_refused(body: &str, name: &str) {
    let e = refusal(body);
    assert!(e.contains(name), "{body}\nrefused as {e}, not {name}");
}

#[test]
fn seven_registers_are_refused_by_name() {
    assert_refused(
        "define i32 @f(i32 %a) nounwind {\n  %r = call i32 asm \"\", \
         \"=r,r,r,r,r,r,r\"(i32 %a, i32 %a, i32 %a, i32 %a, i32 %a, i32 %a)\n  ret i32 %r\n}",
        "x86:inline-asm:registers",
    );
    // Five `q` operands: four registers have a byte form.
    assert_refused(
        "define void @f(i32 %a) nounwind {\n  call void asm \"\", \
         \"q,q,q,q,q\"(i32 %a, i32 %a, i32 %a, i32 %a, i32 %a)\n  ret void\n}",
        "x86:inline-asm:registers",
    );
    // Six registers but `ebx` clobbered.
    assert_refused(
        "define void @f(i32 %a) nounwind {\n  call void asm \"\", \
         \"r,r,r,r,r,r,~{ebx}\"(i32 %a, i32 %a, i32 %a, i32 %a, i32 %a, i32 %a)\n  ret void\n}",
        "x86:inline-asm:registers",
    );
}

#[test]
fn refusals_are_named() {
    let one = |constraints: &str, template: &str| {
        format!(
            "define void @f(i32 %a) nounwind {{\n  call void asm sideeffect \"{template}\", \
             \"{constraints}\"(i32 %a)\n  ret void\n}}"
        )
    };
    assert_refused(&one("{esp}", ""), "x86:inline-asm:frame-register");
    assert_refused(&one("r,~{ebp}", ""), "x86:inline-asm:frame-register");
    assert_refused(&one("t", ""), "x86:inline-asm:x87");
    assert_refused(&one("x", ""), "x86:inline-asm:vector");
    assert_refused(&one("r", "movq $0, %rax"), "asm:operand/register");
    assert_refused(&one("r", "pushq $$1"), "asm:mnemonic/pushq");
    assert_refused(&one("r", "movq $$0, ($0)"), "x86-mode:operand-size-64");
    assert_refused(&one("r", "movl ${0:q}, %eax"), "asm:modifier/q");
    assert_refused(&one("r", "movb ${0:h}, %al"), "asm:modifier/h");
    assert_refused(
        "define void @f() nounwind {\n  call void asm inteldialect \"nop\", \"\"()\n  ret void\n}",
        "x86:inline-asm:intel",
    );
}

/// `A`: a 64-bit output in `edx:eax`. Its consumers are I7's; the
/// lowering itself compiles, under every stress option.
#[test]
fn pair_output_compiles() {
    let text = format!(
        "target triple = \"{TRIPLE}\"\n\ndefine void @f(ptr %p) nounwind {{\n  \
         %t = call i64 asm sideeffect \"rdtsc\", \"=A,~{{dirflag}},~{{fpsr}},~{{flags}}\"()\n  \
         call void asm sideeffect \"\", \"A,~{{memory}}\"(i64 %t)\n  ret void\n}}\n"
    );
    let object = back_end_object(text.as_bytes(), &hod_cg::Options::default());
    assert!(!object.is_empty());
    assert!(!stressed_object(text.as_bytes()).is_empty());
}
