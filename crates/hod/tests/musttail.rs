//! `musttail` through the facade (`spec/codegen.md` §7.1, "Tail kinds";
//! `spec/ir.md` §8.6; WP-069's follow-up, WP-066): `hod::compile` hands
//! `classify` the target's stack-argument rule, so a `musttail` call
//! whose stack-argument area fits its caller's compiles. The program is
//! built by hod with no fallback and by `llc -O0 -global-isel=0
//! -fast-isel=0`, linked under every linker present, run, and the two
//! outputs compared.

#![allow(dead_code, reason = "the linking support is shared with other tests")]

#[path = "../../hod_obj/tests/common/linking.rs"]
mod linking;

use std::process::Command;
use std::time::Duration;

use hod::{CompileError, Options, OutputKind, Report, Settings};
use hod_testutil::{Scratch, run_with, tool};
use linking::{Toolchain, exit_code};

const HEADER: &str = "target triple = \"aarch64-unknown-linux-gnu\"\n";

/// Every `musttail` form the rule accepts: register and stack arguments
/// exchanged (ten `i64`, two on the stack), narrow stack arguments, a
/// `byval` forwarded to a callee of the same area, variadic thunks that
/// forward `...` to `printf` (one after a call that clobbers every
/// argument register), an indirect call, and a `Mem`-class result.
const PROGRAM: &str = r#"
@fmt = private constant [37 x i8] c"thunk %d %ld %.2f %s %d %d %d %d %d\0A\00"
@fmt2 = private constant [18 x i8] c"call %ld %ld %ld\0A\00"
@hello = private constant [6 x i8] c"hello\00"
@res = private constant [12 x i8] c"%s %ld %ld\0A\00"
@blk = global [4 x i64] [i64 11, i64 22, i64 33, i64 44]

declare i32 @printf(ptr, ...)

define internal i64 @c_mt(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6, i64 %a7, i64 %a8, i64 %a9) noinline {
  %m1 = mul i64 %a1, 3
  %m2 = mul i64 %a2, 5
  %m3 = mul i64 %a3, 7
  %m4 = mul i64 %a4, 11
  %m5 = mul i64 %a5, 13
  %m6 = mul i64 %a6, 17
  %m7 = mul i64 %a7, 19
  %m8 = mul i64 %a8, 23
  %m9 = mul i64 %a9, 29
  %s1 = add i64 %a0, %m1
  %s2 = add i64 %s1, %m2
  %s3 = add i64 %s2, %m3
  %s4 = add i64 %s3, %m4
  %s5 = add i64 %s4, %m5
  %s6 = add i64 %s5, %m6
  %s7 = add i64 %s6, %m7
  %s8 = add i64 %s7, %m8
  %s9 = add i64 %s8, %m9
  ret i64 %s9
}

define internal i64 @h_mt(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6, i64 %a7, i64 %a8, i64 %a9) noinline {
  %r = musttail call i64 @c_mt(i64 %a9, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6, i64 %a7, i64 %a0, i64 %a8)
  ret i64 %r
}

define internal i32 @c_narrow(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6, i64 %a7, i8 signext %b, i16 zeroext %c, i32 %d) noinline {
  %bx = sext i8 %b to i32
  %cx = zext i16 %c to i32
  %m = mul i32 %cx, 1000
  %s = add i32 %bx, %m
  %t = add i32 %s, %d
  ret i32 %t
}

define internal i32 @h_narrow(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6, i64 %a7, i8 signext %b, i16 zeroext %c, i32 %d) noinline {
  %b2 = add i8 %b, -100
  %r = musttail call i32 @c_narrow(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6, i64 %a7, i8 signext %b2, i16 zeroext %c, i32 %d)
  ret i32 %r
}

define internal i64 @c_byval(ptr byval([4 x i64]) align 8 %p, i64 %k) noinline {
  %q = getelementptr i64, ptr %p, i64 %k
  %v = load i64, ptr %q
  store i64 0, ptr %p
  ret i64 %v
}

define internal i64 @h_byval_or_not(i64 %k) noinline {
  %r = call i64 @h_byval(ptr byval([4 x i64]) align 8 @blk, i64 %k)
  ret i64 %r
}

define internal i32 @h_thunk(ptr %f, ...) noinline {
  %r = musttail call i32 (ptr, ...) @printf(ptr %f, ...)
  ret i32 %r
}

define internal void @c_clobber(i64 %a, i64 %b, i64 %c, i64 %d, i64 %e, i64 %f, i64 %g, i64 %h, double %x0, double %x1, double %x2, double %x3, double %x4, double %x5, double %x6, double %x7) noinline {
  ret void
}

define internal i32 @h_thunk_call(ptr %f, ...) noinline {
  call void @c_clobber(i64 -1, i64 -1, i64 -1, i64 -1, i64 -1, i64 -1, i64 -1, i64 -1, double -9.0, double -9.0, double -9.0, double -9.0, double -9.0, double -9.0, double -9.0, double -9.0)
  %r = musttail call i32 (ptr, ...) @printf(ptr %f, ...)
  ret i32 %r
}

define internal i64 @leaf(ptr %f, i64 %x) noinline {
  %m = mul i64 %x, 7
  %r = add i64 %m, 1
  ret i64 %r
}

define internal i64 @h_mti(ptr %f, i64 %x) noinline {
  %r = musttail call i64 %f(ptr %f, i64 %x)
  ret i64 %r
}

define internal { i64, i64 } @c_pair(i64 %a, i64 %b) noinline {
  %p0 = insertvalue { i64, i64 } poison, i64 %b, 0
  %p1 = insertvalue { i64, i64 } %p0, i64 %a, 1
  ret { i64, i64 } %p1
}

define internal { i64, i64 } @h_pair(i64 %a, i64 %b) noinline {
  %a2 = sub i64 %a, 5
  %r = musttail call { i64, i64 } @c_pair(i64 %a2, i64 %b)
  ret { i64, i64 } %r
}

define i32 @main() {
  %r1 = call i64 @h_mt(i64 1, i64 2, i64 3, i64 4, i64 5, i64 6, i64 7, i64 8, i64 9, i64 10)
  %n = call i32 @h_narrow(i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i8 -7, i16 65000, i32 -123456)
  %n64 = sext i32 %n to i64
  %bv = call i64 @h_byval_or_not(i64 1)
  call i32 @printf(ptr @fmt2, i64 %r1, i64 %n64, i64 %bv)
  call i32 (ptr, ...) @h_thunk(ptr @fmt, i32 -3, i64 123456789012, double 2.5, ptr @hello, i32 1, i32 2, i32 3, i32 4, i32 5)
  call i32 (ptr, ...) @h_thunk_call(ptr @fmt, i32 7, i64 -1, double -0.25, ptr @hello, i32 9, i32 8, i32 7, i32 6, i32 5)
  %i = call i64 @h_mti(ptr @leaf, i64 6)
  %p = call { i64, i64 } @h_pair(i64 100, i64 200)
  %p0 = extractvalue { i64, i64 } %p, 0
  %p1 = extractvalue { i64, i64 } %p, 1
  %b0 = load i64, ptr @blk
  call i32 @printf(ptr @res, ptr @hello, i64 %i, i64 %p0)
  call i32 @printf(ptr @res, ptr @hello, i64 %p1, i64 %b0)
  ret i32 0
}
"#;

/// `musttail` forwarding a `byval` argument to a callee of the same
/// area. `llc` 22 cannot lower it on AArch64 ("failed to perform tail
/// call elimination on a call site marked musttail"), so its build gets
/// the same function with a plain call; the output must not differ.
const BYVAL: &str = "
define internal i64 @h_byval(ptr byval([4 x i64]) align 8 %p, i64 %k) noinline {
  %k2 = add i64 %k, 1
  %r = KIND call i64 @c_byval(ptr byval([4 x i64]) align 8 %p, i64 %k2)
  ret i64 %r
}
";

fn hod_object(text: &str) -> Result<Vec<u8>, CompileError> {
    let options = Options { output_kind: OutputKind::Object, ..Options::default() };
    hod::compile_with(text.as_bytes(), &options, &Settings::default(), &mut Report::default())
}

/// `spec/codegen.md` §7.1, "Tail kinds": the facade decides `musttail`
/// with `hod_cg::abi::TargetStackArgs`, and what it compiles runs as the
/// `llc` build does under every linker.
#[test]
fn musttail_programs_compile_and_run_like_llc() {
    let text = format!("{HEADER}{PROGRAM}{}", BYVAL.replace("KIND", "musttail"));
    let object = hod_object(&text).unwrap_or_else(|e| panic!("hod declines the program: {e}"));
    let dir = Scratch::new("hod-musttail");
    let Some(toolchain) = Toolchain::host(&dir) else { return };
    let Some(llc) = tool("llc") else { return };
    let hod_o = dir.write("hod.o", object);
    let input = dir.write("p.ll", format!("{HEADER}{PROGRAM}{}", BYVAL.replace("KIND", "")));
    let llc_o = dir.join("llc.o");
    let mut command = Command::new(llc);
    command.args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj"]);
    command.arg(&input).arg("-o").arg(&llc_o);
    let built = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(built.success(), "llc: {}", built.stderr_text());
    let mut want = None;
    for linker in &toolchain.linkers {
        for (tag, object) in [("llc", &llc_o), ("hod", &hod_o)] {
            let exe = dir.join(format!("{tag}-{}", linker.name));
            let ran = toolchain.link_and_run(linker, &[], &[object], &exe);
            assert_eq!(exit_code(&ran), Some(0), "{tag} {}: {}", linker.name, ran.stderr_text());
            let out = ran.stdout_text();
            let want = want.get_or_insert_with(|| out.clone());
            assert_eq!(&out, want, "{tag} under {}", linker.name);
        }
    }
    let want = want.unwrap_or_default();
    let expect = "call 747 64876437 33\n\
                  thunk -3 123456789012 2.50 hello 1 2 3 4 5\n\
                  thunk 7 -1 -0.25 hello 9 8 7 6 5\n\
                  hello 43 200\n\
                  hello 95 11\n";
    assert_eq!(want, expect);
}

/// The rule still refuses what it must: a `musttail` call that needs a
/// larger stack-argument area than its caller's
/// (`musttail:stack-args-grow`).
#[test]
fn musttail_that_grows_the_stack_area_is_refused_by_name() {
    let nine = "i64, i64, i64, i64, i64, i64, i64, i64, i64";
    let text = format!(
        "{HEADER}declare i64 @g({nine})\n\
         define i64 @f(i64 %x) {{\n  %r = musttail call i64 @g(i64 0, i64 0, i64 0, i64 0, \
         i64 0, i64 0, i64 0, i64 0, i64 %x)\n  ret i64 %r\n}}\n"
    );
    let error = hod_object(&text).expect_err("the call grows the area");
    let CompileError::Unsupported(unsupported) = error else { panic!("{error}") };
    assert_eq!(unsupported.construct.name(), "musttail:stack-args-grow");
}
