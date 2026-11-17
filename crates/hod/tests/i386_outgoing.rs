//! WP-354: the i386 outgoing argument area reserved in the prologue
//! (`spec/codegen-x86.md` §3). `outgoing/calls.c` (calls of several
//! argument sizes, a hidden-pointer result, variadic calls, byval
//! aggregates, `regparm`, a stack protector, a
//! deep recursion, calls in a loop that check `esp` does not creep and is
//! 16-aligned at every call, `alloca` and a VLA on the old path, an
//! over-aligned local) and an `.ll` program (`musttail`, `invoke`,
//! `llvm.stacksave`, an `sret` call whose callee pops), static and PIE, as `llc` 19 and 22
//! run them; the shape: nothing moves `esp` after a call of a qualifying
//! function but a callee pop's re-reservation.

use std::process::Command;

use hod_testutil::Scratch;

mod i386_i11b;
mod i386_support;
use i386_i11b::{Build, STATIC, Source, data, lane};
use i386_support::{TRIPLE, checked, llvm_tool};

const LL: &str = r#"target triple = "i686-unknown-linux-gnu"
@fmt = private constant [13 x i8] c"%d %d %d %d\0A\00"
declare i32 @printf(ptr, ...)
declare i32 @__gcc_personality_v0(...)

define internal i32 @add5(i32 %a, i32 %b, i32 %c, i32 %d, i32 %e) noinline {
  %1 = add i32 %a, %b
  %2 = add i32 %1, %c
  %3 = add i32 %2, %d
  %4 = mul i32 %3, %e
  ret i32 %4
}
%big = type { i32, i32, i32, i32, i32 }
define internal void @mk(ptr sret(%big) %r, i32 %a, i32 %b) noinline {
  %1 = sub i32 %a, %b
  %e = getelementptr inbounds %big, ptr %r, i32 0, i32 4
  store i32 %1, ptr %e
  ret void
}
define internal i32 @std2(i32 %a, i32 %b) noinline {
  %r = alloca %big
  call void @mk(ptr sret(%big) %r, i32 %a, i32 %b)
  %q = call i32 @add5(i32 %a, i32 %b, i32 0, i32 0, i32 1)
  %e = getelementptr inbounds %big, ptr %r, i32 0, i32 4
  %v = load i32, ptr %e
  %w = add i32 %v, %q
  ret i32 %w
}
define internal i32 @tail(i32 %a, i32 %b, i32 %c, i32 %d, i32 %e) noinline {
  %r = musttail call i32 @add5(i32 %e, i32 %d, i32 %c, i32 %b, i32 %a)
  ret i32 %r
}
define internal i32 @saved(i32 %n) noinline {
  %s = call ptr @llvm.stacksave.p0()
  %x = call i32 @add5(i32 %n, i32 1, i32 2, i32 3, i32 4)
  call void @llvm.stackrestore.p0(ptr %s)
  %y = call i32 @std2(i32 %x, i32 7)
  ret i32 %y
}
define internal i32 @inv(i32 %n) noinline personality ptr @__gcc_personality_v0 {
  %x = invoke i32 @add5(i32 %n, i32 1, i32 2, i32 3, i32 4) to label %ok unwind label %pad
ok:
  %y = call i32 @add5(i32 %x, i32 %x, i32 1, i32 1, i32 1)
  ret i32 %y
pad:
  %l = landingpad { ptr, i32 } cleanup
  ret i32 -1
}
define i32 @main() {
  %a = call i32 @tail(i32 1, i32 2, i32 3, i32 4, i32 5)
  %b = call i32 @saved(i32 6)
  %c = call i32 @inv(i32 7)
  %d = call i32 @std2(i32 %a, i32 %b)
  %p = call i32 (ptr, ...) @printf(ptr @fmt, i32 %a, i32 %b, i32 %c, i32 %d)
  ret i32 0
}
declare ptr @llvm.stacksave.p0()
declare void @llvm.stackrestore.p0(ptr)
"#;

#[test]
fn programs_with_calls_run_as_llc_s() {
    let Some(lane) = lane() else { return };
    let scratch = Scratch::new("i386-outgoing");
    let source = data("outgoing", "calls.c");
    let mut runs = 0;
    for pic in [false, true] {
        let build = Build { pic, all_linkers: true, ..STATIC };
        runs += lane.check(&scratch, "calls", &Source::C(&source, &[]), build);
        runs += lane.check(&scratch, "calls-ll", &Source::Ll(LL), build);
    }
    eprintln!("i386 outgoing: {runs} runs equal llc's");
}

/// The disassembly of hod's object for `body` (`llvm-objdump -d`, AT&T).
fn disassembly(scratch: &Scratch, name: &str, body: &str) -> Option<String> {
    let objdump = llvm_tool(22, "llvm-objdump").or_else(|| llvm_tool(19, "llvm-objdump"))?;
    let text = format!("target triple = \"{TRIPLE}\"\n{body}\n");
    let options = hod::Options {
        output_kind: hod::OutputKind::Object,
        triple: Some(TRIPLE.into()),
        ..hod::Options::default()
    };
    let object = hod::compile(text.as_bytes(), &options).unwrap_or_else(|e| panic!("{e}"));
    let path = scratch.write(&format!("{name}.o"), &object);
    let mut command = Command::new(objdump);
    command.args(["-d", "--no-show-raw-insn"]).arg(&path);
    Some(checked("llvm-objdump", &mut command).stdout_text())
}

/// The instruction after each `call`.
fn after_calls(asm: &str) -> Vec<String> {
    // `      38:      \tcalll\t0x39 <f+0x39>`: an address, then the text.
    let insts: Vec<&str> = asm
        .lines()
        .filter_map(|l| l.trim_start().split_once(':'))
        .filter(|(at, _)| !at.is_empty() && at.chars().all(|c| c.is_ascii_hexdigit()))
        .map(|(_, text)| text.trim())
        .collect();
    let mut after = Vec::new();
    for pair in insts.windows(2) {
        if pair[0].starts_with("call") {
            after.push(pair[1].to_owned());
        }
    }
    after
}

#[test]
fn the_area_is_reserved_in_the_prologue() {
    let scratch = Scratch::new("i386-outgoing-shape");
    let plain = "declare void @g(i32, i32, i32, i32, i32)\ndeclare void @h(i32)\n\
                 define void @f() {\n  call void @g(i32 1, i32 2, i32 3, i32 4, i32 5)\n  \
                 call void @h(i32 6)\n  ret void\n}";
    let Some(asm) = disassembly(&scratch, "plain", plain) else { return };
    // The largest area (20 bytes, 32 reserved) in the prologue: `FRAME`
    // is 12 for no locals, 12 + 32 with the area.
    assert!(asm.contains("subl\t$0x2c, %esp"), "{asm}");
    let after = after_calls(&asm);
    // The second call is followed by the epilogue (`lea esp, [ebp - 12]`).
    assert_eq!(after, ["movl\t$0x6, %ecx", "leal\t-0xc(%ebp), %esp"], "{asm}");
    // A callee that pops its hidden pointer: re-reserved, as `llc` does.
    let sret = "%big = type { i32, i32, i32, i32, i32 }\ndeclare void @s(ptr sret(%big))\n\
                define void @f() {\n  %r = alloca %big\n  call void @s(ptr sret(%big) %r)\n  \
                ret void\n}";
    let Some(asm) = disassembly(&scratch, "sret", sret) else { return };
    let after = after_calls(&asm);
    assert_eq!(after, ["leal\t-0x4(%esp), %esp"], "{asm}");
    // A dynamic `alloca`: the old path, the area around the call.
    let dynamic = "declare void @g(ptr, i32)\n\
                   define void @f(i32 %n) {\n  %p = alloca i8, i32 %n\n  \
                   call void @g(ptr %p, i32 1)\n  ret void\n}";
    let Some(asm) = disassembly(&scratch, "dynamic", dynamic) else { return };
    let after = after_calls(&asm);
    assert_eq!(after, ["leal\t0x10(%esp), %esp"], "{asm}");
}
