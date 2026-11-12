//! The evidence for `spec/abi-a64.md` §1.2: `llc` compiles a function,
//! and a call, of the conventions `assign` treats as `ccc` to the very
//! code it emits for `ccc`.

use crate::run::{Scratch, Tools, tail};

/// A module that defines and calls functions of the convention `CC`:
/// arguments in every register class and on the stack, values live
/// across a call (the preserved registers), a return in registers and
/// one in memory, `sret` and `byval`.
const MODULE: &str = r"
target triple = 'aarch64-unknown-linux-gnu'

declare CC { i64, double } @ext(i64, double, i128, [2 x i64], ptr, float, [3 x float], <4 x i32>, i64, i64, i64, i64, i8 zeroext, i128, fp128)
declare CC [9 x i64] @ext_mem(ptr, i64)
declare CC void @ext_sret(ptr sret([4 x i64]), ptr byval([3 x i64]) align 8, i8 signext)

define CC i64 @takes(i64 %a, double %b, i128 %c, [2 x i64] %d, ptr %p, float %e, [3 x float] %h, <4 x i32> %v, i64 %s0, i64 %s1, i64 %s2, i64 %s3, i8 zeroext %s4, i128 %s5, fp128 %q) {
  %r = call CC { i64, double } @ext(i64 %s3, double %b, i128 %s5, [2 x i64] %d, ptr %p, float %e, [3 x float] %h, <4 x i32> %v, i64 %a, i64 %s0, i64 %s1, i64 %s2, i8 zeroext %s4, i128 %c, fp128 %q)
  %r0 = extractvalue { i64, double } %r, 0
  %r1 = extractvalue { i64, double } %r, 1
  %bi = fptosi double %b to i64
  %ri = fptosi double %r1 to i64
  %s4w = zext i8 %s4 to i64
  %t0 = add i64 %a, %r0
  %t1 = add i64 %t0, %bi
  %t2 = add i64 %t1, %ri
  %t3 = add i64 %t2, %s4w
  %t4 = add i64 %t3, %s0
  %t5 = add i64 %t4, %s1
  %t6 = add i64 %t5, %s2
  ret i64 %t6
}

define CC [9 x i64] @in_memory(ptr %p, i64 %n) {
  %r = call CC [9 x i64] @ext_mem(ptr %p, i64 %n)
  %first = extractvalue [9 x i64] %r, 0
  %sum = add i64 %first, %n
  %out = insertvalue [9 x i64] %r, i64 %sum, 8
  ret [9 x i64] %out
}

define CC void @structs(ptr sret([4 x i64]) %out, ptr byval([3 x i64]) align 8 %in, i8 signext %k) {
  call CC void @ext_sret(ptr sret([4 x i64]) %out, ptr byval([3 x i64]) align 8 %in, i8 signext %k)
  %first = load i64, ptr %in
  %wide = sext i8 %k to i64
  %sum = add i64 %first, %wide
  store i64 %sum, ptr %out
  ret void
}

define CC { float, i128, <2 x double> } @returns(float %f, i128 %i, <2 x double> %v) {
  %a = insertvalue { float, i128, <2 x double> } poison, float %f, 0
  %b = insertvalue { float, i128, <2 x double> } %a, i128 %i, 1
  %c = insertvalue { float, i128, <2 x double> } %b, <2 x double> %v, 2
  ret { float, i128, <2 x double> } %c
}
";

/// `spec/abi-a64.md` §1.2: `fastcc`, `swiftcc` and `cxx_fast_tlscc`
/// compile to the code of `ccc`, with llc 22 and 19, at `-O0` and `-O2`.
#[test]
fn conventions_equal_to_c() {
    let Some(tools) = Tools::find() else { return };
    let scratch = Scratch::new("abi-cc");
    let module = MODULE.replace('\'', "\"");
    for (config, llc) in &tools.llc {
        let compile = |keyword: &str| {
            let name = format!("{}{}.ll", keyword.trim(), config.tag());
            let source = scratch.write(&name, module.replace("CC ", keyword));
            let outcome = hod_testutil::run(
                llc,
                [
                    config.opt.as_ref(),
                    "-filetype=asm".as_ref(),
                    "-relocation-model=pic".as_ref(),
                    "-o".as_ref(),
                    "-".as_ref(),
                    source.as_os_str(),
                ],
            );
            assert!(
                outcome.success(),
                "llc {} on {name}:\n{}",
                config.tag(),
                tail(&outcome.stderr_text())
            );
            // The first lines name the source file.
            let text = outcome.stdout_text();
            text.lines().filter(|line| !line.contains(".file")).collect::<Vec<_>>().join("\n")
        };
        let c = compile("");
        assert!(c.contains("takes:") && c.contains("bl\text"), "not the code expected:\n{c}");
        for keyword in ["fastcc ", "swiftcc ", "cxx_fast_tlscc "] {
            assert!(compile(keyword) == c, "llc {}: `{keyword}` differs from `ccc`", config.tag());
        }
        // The check can fail: a convention that is not `ccc` differs.
        assert!(
            compile("preserve_mostcc ") != c,
            "llc {}: preserve_mostcc equals ccc",
            config.tag()
        );
    }
}
