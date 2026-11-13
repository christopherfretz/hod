//! The evidence for `spec/abi-x64.md` §1.2: `llc` compiles a function,
//! and a call, of the conventions `assign` treats as `ccc` to the very
//! code it emits for `ccc`.

use crate::run::{Scratch, Tools, tail};

/// A module that defines and calls functions of the convention `CC`:
/// arguments in every register class and on the stack, values live
/// across a call (the preserved registers), a return in registers and
/// one in memory, `sret` and `byval`.
const MODULE: &str = r"
target triple = 'x86_64-unknown-linux-gnu'

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

define CC { float, i64, <2 x double> } @returns(float %f, i64 %i, <2 x double> %v) {
  %a = insertvalue { float, i64, <2 x double> } poison, float %f, 0
  %b = insertvalue { float, i64, <2 x double> } %a, i64 %i, 1
  %c = insertvalue { float, i64, <2 x double> } %b, <2 x double> %v, 2
  ret { float, i64, <2 x double> } %c
}

define CC { x86_fp80, x86_fp80 } @x87(x86_fp80 %a, i64 %n, x86_fp80 %b) {
  %x = insertvalue { x86_fp80, x86_fp80 } poison, x86_fp80 %b, 0
  %y = insertvalue { x86_fp80, x86_fp80 } %x, x86_fp80 %a, 1
  ret { x86_fp80, x86_fp80 } %y
}
";

/// `spec/abi-x64.md` §1.2: `fastcc` compiles to the code of `ccc`, with
/// llc 22 and 19, at `-O0` (both selectors) and `-O2`; `cxx_fast_tlscc`
/// does not (it preserves more registers), which is why it is refused.
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
                    if config.dag { "-fast-isel=0" } else { "-fast-isel=1" }.as_ref(),
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
        assert!(
            c.contains("takes:") && c.contains("callq\text@PLT"),
            "not the code expected:\n{c}"
        );
        assert!(compile("fastcc ") == c, "llc {}: `fastcc` differs from `ccc`", config.tag());
        assert!(compile("cxx_fast_tlscc ") != c, "llc {}: cxx_fast_tlscc equals ccc", config.tag());
        // The check can fail: a convention that is not `ccc` differs.
        assert!(
            compile("preserve_mostcc ") != c,
            "llc {}: preserve_mostcc equals ccc",
            config.tag()
        );
    }
}

/// A variadic thunk that calls something and then forwards.
const THUNKS: &str = r"
target triple = 'x86_64-unknown-linux-gnu'

declare void @gv(ptr, ...)
declare void @ext(ptr)

define void @fv(ptr %a, ...) {
  call void @ext(ptr %a)
  musttail call void (ptr, ...) @gv(ptr %a, ...)
  ret void
}
";

/// A `musttail` pair that differs in an attribute that changes places.
const MISMATCH: &str = r"
target triple = 'x86_64-unknown-linux-gnu'

declare void @g([2 x i64] alignstack(16))

define void @f([2 x i64] %a) {
  musttail call void @g([2 x i64] alignstack(16) %a)
  ret void
}
";

/// The registers the instructions of `lines` write: the last operand of
/// a move (AT&T order), the low byte or the whole of it.
fn written(lines: &[&str]) -> Vec<String> {
    let mut all = Vec::new();
    for line in lines {
        let code = line.split('#').next().unwrap_or("");
        let mut words = code.split_whitespace();
        if words.next().is_some_and(|op| op.starts_with("mov"))
            && let Some(last) = code.rsplit(',').next()
        {
            all.push(last.trim().to_string());
        }
    }
    all
}

/// `spec/abi-x64.md` §8.5: LLVM refuses a `musttail` pair that differs
/// in `alignstack`; and its variadic `musttail` thunk hands on
/// `rdi`..`r9`, `xmm0`..`xmm7` and `al` as it received them, which is
/// what a variadic callee compiled by LLVM therefore relies on.
#[test]
fn musttail_in_llvm() {
    let Some(tools) = Tools::find() else { return };
    let scratch = Scratch::new("abi-musttail");
    for (config, llc) in &tools.llc {
        let compile = |name: &str, module: &str| {
            let name = format!("{name}{}.ll", config.tag());
            let source = scratch.write(&name, module.replace('\'', "\""));
            hod_testutil::run(
                llc,
                [
                    config.opt.as_ref(),
                    if config.dag { "-fast-isel=0" } else { "-fast-isel=1" }.as_ref(),
                    "-relocation-model=pic".as_ref(),
                    "-filetype=asm".as_ref(),
                    "-o".as_ref(),
                    "-".as_ref(),
                    source.as_os_str(),
                ],
            )
        };
        let refused = compile("mismatch", MISMATCH);
        assert!(
            !refused.success() && refused.stderr_text().contains("mismatched ABI impacting"),
            "llc {}: a pair differing in `alignstack` was not refused:\n{}",
            config.tag(),
            tail(&refused.stderr_text())
        );

        let outcome = compile("thunks", THUNKS);
        assert!(outcome.success(), "llc {}:\n{}", config.tag(), tail(&outcome.stderr_text()));
        let text = outcome.stdout_text();
        let lines: Vec<&str> = text.lines().map(str::trim).collect();
        let start = lines.iter().position(|line| line.starts_with("fv:")).expect("the thunk");
        let call = start
            + lines[start..]
                .iter()
                .position(|line| line.starts_with("callq\text@PLT"))
                .expect("the call");
        let exit = call
            + lines[call..]
                .iter()
                .position(|line| line.starts_with("jmp\tgv@PLT"))
                .expect("the tail call");
        let restored = written(&lines[call..exit]);
        let wanted = ["%rdi", "%rsi", "%rdx", "%rcx", "%r8", "%r9"]
            .map(String::from)
            .into_iter()
            .chain((0..8).map(|n| format!("%xmm{n}")));
        for register in wanted {
            assert!(
                restored.contains(&register),
                "llc {}: the thunk does not restore {register}:\n{}",
                config.tag(),
                lines[call..=exit].join("\n")
            );
        }
        assert!(
            restored.iter().any(|reg| reg == "%al" || reg == "%eax"),
            "llc {}: the thunk does not restore al:\n{}",
            config.tag(),
            lines[call..=exit].join("\n")
        );
    }
}
