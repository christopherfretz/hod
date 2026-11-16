//! WP-316 (design 0008 I11b): the dynamic stack on i386
//! (`spec/codegen-x86.md` §12). `dynstack/dyn.c` (VLAs in a loop, which
//! clang brackets with `llvm.stacksave`/`stackrestore`; `alloca`;
//! locals aligned to 32 and 64 and a VLA aligned to 128; calls with
//! stack arguments and `sret` calls, whose callee pops, made after a
//! dynamic allocation; recursion through dynamic areas) at `-march=i586`
//! and the default CPU, static and PIE, linked by GNU ld, lld and mold,
//! as `llc` 19 and 22 run it. `"probe-stack"="inline-asm"` is refused
//! (`x86:probe-stack`).

use hod_testutil::Scratch;

mod i386_i11b;
mod i386_support;
use i386_i11b::{Build, STATIC, Source, data, lane, refusal};

#[test]
fn dynamic_stack_programs_run_as_llc_s() {
    let Some(lane) = lane() else { return };
    let scratch = Scratch::new("i386-dynstack");
    let source = data("dynstack", "dyn.c");
    let mut runs = 0;
    for (cpu, flags) in [("i586", &["-march=i586"][..]), ("i686", &[][..])] {
        for pic in [false, true] {
            let build = Build { pic, all_linkers: true, ..STATIC };
            runs += lane.check(&scratch, &format!("dyn-{cpu}"), &Source::C(&source, flags), build);
        }
    }
    eprintln!("i386 dynstack: {runs} runs of dyn.c equal llc's");
}

#[test]
fn what_compiles_and_what_is_refused() {
    let fine = [
        "define ptr @f(i32 %n) {\n  %p = alloca i8, i32 %n, align 16\n  ret ptr %p\n}",
        "define ptr @f(i16 %n) {\n  %p = alloca i32, i16 %n, align 256\n  ret ptr %p\n}",
        "define void @f() {\n  %s = call ptr @llvm.stacksave.p0()\n  \
         call void @llvm.stackrestore.p0(ptr %s)\n  ret void\n}\n\
         declare ptr @llvm.stacksave.p0()\ndeclare void @llvm.stackrestore.p0(ptr)",
        "define i32 @f() {\n  %a = alloca i32, align 64\n  store i32 1, ptr %a\n  \
         %v = load i32, ptr %a\n  ret i32 %v\n}",
    ];
    for body in fine {
        assert_eq!(refusal(body, None), None, "{body}");
    }
    let probed = "define void @f() #0 {\n  ret void\n}\n\
                  attributes #0 = { \"probe-stack\"=\"inline-asm\" }";
    let error = refusal(probed, None).unwrap_or_else(|| panic!("compiled:\n{probed}"));
    assert!(error.contains("x86:probe-stack"), "{error}");
}
