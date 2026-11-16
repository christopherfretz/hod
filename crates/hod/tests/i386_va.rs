//! WP-316 (design 0008 I11b): the bodies of variadic functions on i386
//! (`spec/codegen-x86.md` §12, `spec/abi-x86.md` §9). `va/va.c` (clang
//! 19 and 22 expand `va_arg` themselves: `va_start`, `va_copy`, `va_end`
//! and pointer arithmetic, every type the convention passes, a struct by
//! value, an `sret` variadic function, a list forwarded to `vprintf` and
//! one passed by pointer) at `-march=i586` and the default CPU (at
//! `-march=pentium4` its `double` arithmetic is SSE2, I12's);
//! `va/va_arg.ll` (the IR `va_arg` instruction for `i8`, `i16`, `i32`,
//! `i48`, `i64`, `float`, `double`, `x86_fp80`, `ptr`), as `llc` 19 and
//! 22 run them, static and PIE. The facade's classifier refuses the IR
//! instruction for every target (`inst:va_arg`), so `va_arg.ll` is
//! compiled by `hod_x86` directly.

use hod_testutil::Scratch;

mod i386_i11b;
mod i386_support;
use i386_i11b::{Build, STATIC, Source, back_end_refusal, data, lane, refusal};

#[test]
fn variadic_c_programs_run_as_llc_s() {
    let Some(lane) = lane() else { return };
    let scratch = Scratch::new("i386-va");
    let source = data("va", "va.c");
    let mut runs = 0;
    for (cpu, flags) in [("i586", &["-march=i586"][..]), ("i686", &[][..])] {
        for pic in [false, true] {
            let build = Build { pic, ..STATIC };
            runs += lane.check(&scratch, &format!("va-{cpu}"), &Source::C(&source, flags), build);
        }
    }
    eprintln!("i386 va: {runs} runs of va.c equal llc's");
}

#[test]
fn the_va_arg_instruction_runs_as_llc_s() {
    let Some(lane) = lane() else { return };
    let scratch = Scratch::new("i386-va-arg");
    let text = std::fs::read_to_string(data("va", "va_arg.ll")).unwrap();
    let mut runs = 0;
    for pic in [false, true] {
        let build = Build { pic, back_end: true, ..STATIC };
        runs += lane.check(&scratch, "va_arg", &Source::Ll(&text), build);
    }
    eprintln!("i386 va_arg: {runs} runs equal llc's");
}

/// `va_start` outside a variadic function is invalid IR, refused by
/// name; a `va_arg` of an aggregate or of an integer wider than 64 bits
/// is refused `x86:varargs:va-arg-type` by the back end (clang never
/// writes one); the facade refuses the instruction itself.
#[test]
fn what_stays_refused() {
    let start = "define void @f(ptr %ap) {\n  call void @llvm.va_start.p0(ptr %ap)\n  ret void\n}\n\
                 declare void @llvm.va_start.p0(ptr)";
    let error = refusal(start, None).unwrap_or_else(|| panic!("compiled:\n{start}"));
    assert!(error.contains("x86:varargs"), "{error}");
    for ty in ["{ i32, i32 }", "i128"] {
        let body = format!(
            "define void @f(ptr %ap, ptr %out) {{\n  %v = va_arg ptr %ap, {ty}\n  \
             store {ty} %v, ptr %out\n  ret void\n}}"
        );
        let error = back_end_refusal(&body).unwrap_or_else(|| panic!("compiled:\n{body}"));
        assert!(error.contains("x86:varargs:va-arg-type"), "{error}");
        let error = refusal(&body, None).unwrap_or_else(|| panic!("compiled:\n{body}"));
        assert!(error.contains("inst:va_arg"), "{error}");
    }
}
