//! WP-324: indirect branches on i386 (`spec/codegen-x86.md` §15).
//! `ibr/interp.c` (computed-goto interpreters: a static table of label
//! addresses, a table built at run time, a label address selected and
//! compared) at `-march=i586` and the default CPU, static and PIE, linked
//! by GNU ld, lld and mold, as `llc` 19 and 22 run it (also under
//! `Stress::ALL`).

use hod_testutil::Scratch;

mod i386_i11b;
mod i386_support;
use i386_i11b::{Build, STATIC, Source, back_end_refusal, data, lane, refusal};

#[test]
fn computed_goto_programs_run_as_llc_s() {
    let Some(lane) = lane() else { return };
    let scratch = Scratch::new("i386-ibr");
    let source = data("ibr", "interp.c");
    let mut runs = 0;
    for (cpu, flags) in [("i586", &["-march=i586"][..]), ("i686", &[][..])] {
        for pic in [false, true] {
            let build = Build { pic, all_linkers: true, ..STATIC };
            runs += lane.check(&scratch, &format!("ibr-{cpu}"), &Source::C(&source, flags), build);
        }
    }
    eprintln!("i386 ibr: {runs} runs of interp.c equal llc's");
}

/// A `blockaddress` in a register and in data, an `indirectbr` on a
/// register and on a constant; every one compiles.
#[test]
fn what_compiles() {
    let fine = [
        "define ptr @f() {\nentry:\n  br label %b\nb:\n  ret ptr blockaddress(@f, %b)\n}",
        "define i32 @f(ptr %p) {\nentry:\n  indirectbr ptr %p, [label %a, label %b]\n\
         a:\n  ret i32 1\nb:\n  ret i32 2\n}",
        "define i32 @f() {\nentry:\n  indirectbr ptr blockaddress(@f, %b), [label %a, label %b]\n\
         a:\n  ret i32 1\nb:\n  ret i32 2\n}",
        "@t = global [2 x ptr] [ptr blockaddress(@f, %a), ptr blockaddress(@f, %b)]\n\
         define i32 @f(i32 %i) {\nentry:\n  %s = getelementptr [2 x ptr], ptr @t, i32 0, i32 %i\n  \
         %p = load ptr, ptr %s\n  indirectbr ptr %p, [label %a, label %b]\n\
         a:\n  ret i32 1\nb:\n  ret i32 2\n}",
    ];
    for body in fine {
        assert_eq!(refusal(body, None), None, "{body}");
        assert_eq!(back_end_refusal(body), None, "{body}");
    }
}

/// The address of another function's block is no `x86:indirect-branch`
/// and compiles in no function of this target (`x86:blockaddress` when
/// it reaches the back end).
#[test]
fn another_function_s_block_is_refused() {
    let body = "define void @g() {\nentry:\n  br label %b\nb:\n  ret void\n}\n\
                define ptr @f() {\n  ret ptr blockaddress(@g, %b)\n}";
    if let Some(error) = back_end_refusal(body) {
        assert!(error.contains("x86:blockaddress"), "{error}");
    }
}
