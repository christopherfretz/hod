//! `musttail` calls with `byval` arguments aligned above 16 on x86-64
//! (WP-152, `spec/abi-x64.md` §6.2, §8.5).
//!
//! LLVM's verifier makes a `musttail` callee's parameter list the
//! caller's, attributes included, so each `byval` copy goes where the
//! caller's own arrived, in the stack-argument area the caller's caller
//! aligned (§6.2): no realignment is needed, and `call::align_stack_area`
//! does none. The thunks (identical forwarding, permuted and computed
//! arguments with stack arguments after the copy, two copies swapped, a
//! variadic thunk forwarding stack arguments) are compiled through the
//! facade, by `hod_x64` with every stress option, and by `llc` 22, and
//! linked with `llc` 22's callers and callees (the callees trap on a
//! misaligned copy) and with hod's (the variadic callee is `llc`'s),
//! under GNU ld, lld and mold. `llc` 19 stages a `musttail` call's
//! `byval` copies over the thunk's return address (`musttail_x64.rs`),
//! so it is no leg. The calls whose area would grow are refused by name.

#![allow(clippy::too_many_lines, reason = "IR text")]

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hod::{CompileError, Options, OutputKind, RelocModel, Report, Settings};
use hod_testutil::{Scratch, run_with};
use linking::{Toolchain, exit_code};

fn header() -> String {
    format!(
        "target datalayout = \"{}\"\ntarget triple = \"x86_64-unknown-linux-gnu\"\n",
        hod_ir::X86_64_LINUX_DATA_LAYOUT
    )
}

const TYPES: &str = "
%s32 = type { [4 x i64] }
%s64 = type { [8 x i64] }
";

/// The thunks.
const THUNKS: &str = "
declare i64 @c_a(i64, i64, i64, i64, i64, i64, i64, ptr byval(%s32) align 32, i64)
declare i64 @c_b(i64, i64, i64, i64, i64, i64, i64, ptr byval(%s64) align 64, i64)
declare i64 @c_c(i64, ptr byval(%s64) align 64, ptr byval(%s32) align 32, ptr byval(%s64) align 64)
declare i64 @c_d(i64, ptr byval(%s64) align 64, ...)
declare void @clobber(i64, i64, i64, i64, i64, i64)

; The same arguments on.
define i64 @t_a(i64 %a, i64 %b, i64 %c, i64 %d, i64 %e, i64 %f, i64 %g,
                ptr byval(%s32) align 32 %p, i64 %h) {
  %r = musttail call i64 @c_a(i64 %a, i64 %b, i64 %c, i64 %d, i64 %e, i64 %f, i64 %g,
                              ptr byval(%s32) align 32 %p, i64 %h)
  ret i64 %r
}
; Permuted and computed, the stack arguments around the copy swapped,
; after a call that clobbers the argument registers.
define i64 @t_b(i64 %a, i64 %b, i64 %c, i64 %d, i64 %e, i64 %f, i64 %g,
                ptr byval(%s64) align 64 %p, i64 %h) {
  call void @clobber(i64 -1, i64 -1, i64 -1, i64 -1, i64 -1, i64 -1)
  %h1 = add i64 %h, 1
  %r = musttail call i64 @c_b(i64 %b, i64 %a, i64 %c, i64 %d, i64 %f, i64 %e, i64 %h1,
                              ptr byval(%s64) align 64 %p, i64 %g)
  ret i64 %r
}
; Two copies of one type swapped, one of another between them.
define i64 @t_c(i64 %a, ptr byval(%s64) align 64 %p, ptr byval(%s32) align 32 %m,
                ptr byval(%s64) align 64 %q) {
  %r = musttail call i64 @c_c(i64 %a, ptr byval(%s64) align 64 %q, ptr byval(%s32) align 32 %m,
                              ptr byval(%s64) align 64 %p)
  ret i64 %r
}
; Variadic: the forwarded arguments in registers and on the stack after
; the copy.
define i64 @t_d(i64 %a, ptr byval(%s64) align 64 %p, ...) {
  %r = musttail call i64 (i64, ptr, ...) @c_d(i64 %a, ptr byval(%s64) align 64 %p, ...)
  ret i64 %r
}
";

/// The callees and the callers (`d_*(seed)`, called from C).
const ENDS: &str = "
declare void @llvm.trap()
declare i64 @t_a(i64, i64, i64, i64, i64, i64, i64, ptr byval(%s32) align 32, i64)
declare i64 @t_b(i64, i64, i64, i64, i64, i64, i64, ptr byval(%s64) align 64, i64)
declare i64 @t_c(i64, ptr byval(%s64) align 64, ptr byval(%s32) align 32, ptr byval(%s64) align 64)
declare i64 @t_d(i64, ptr byval(%s64) align 64, ...)

define void @clobber(i64 %a, i64 %b, i64 %c, i64 %d, i64 %e, i64 %f) noinline {
  ret void
}
define i64 @mix(i64 %h, i64 %x) {
  %m = mul i64 %h, 1099511628211
  %r = xor i64 %m, %x
  ret i64 %r
}
; The hash of `n` words at `p`; a trap unless `p` is `align`-aligned.
define i64 @words(i64 %h, ptr %p, i64 %n, i64 %align) {
entry:
  %i = ptrtoint ptr %p to i64
  %m = sub i64 %align, 1
  %low = and i64 %i, %m
  %ok = icmp eq i64 %low, 0
  br i1 %ok, label %loop, label %bad
bad:
  call void @llvm.trap()
  unreachable
loop:
  %k = phi i64 [ 0, %entry ], [ %k1, %loop ]
  %acc = phi i64 [ %h, %entry ], [ %acc1, %loop ]
  %q = getelementptr i64, ptr %p, i64 %k
  %w = load i64, ptr %q
  %acc1 = call i64 @mix(i64 %acc, i64 %w)
  %k1 = add i64 %k, 1
  %more = icmp ult i64 %k1, %n
  br i1 %more, label %loop, label %done
done:
  ret i64 %acc1
}
define void @fill(ptr %p, i64 %n, i64 %s) {
entry:
  br label %loop
loop:
  %k = phi i64 [ 0, %entry ], [ %k1, %loop ]
  %q = getelementptr i64, ptr %p, i64 %k
  %m = mul i64 %k, 6364136223846793005
  %w = xor i64 %m, %s
  store i64 %w, ptr %q
  %k1 = add i64 %k, 1
  %more = icmp ult i64 %k1, %n
  br i1 %more, label %loop, label %done
done:
  ret void
}

define i64 @c_a(i64 %a, i64 %b, i64 %c, i64 %d, i64 %e, i64 %f, i64 %g,
                ptr byval(%s32) align 32 %p, i64 %h) {
  %x1 = call i64 @mix(i64 %a, i64 %b)
  %x2 = call i64 @mix(i64 %x1, i64 %c)
  %x3 = call i64 @mix(i64 %x2, i64 %d)
  %x4 = call i64 @mix(i64 %x3, i64 %e)
  %x5 = call i64 @mix(i64 %x4, i64 %f)
  %x6 = call i64 @mix(i64 %x5, i64 %g)
  %x7 = call i64 @words(i64 %x6, ptr %p, i64 4, i64 32)
  %x8 = call i64 @mix(i64 %x7, i64 %h)
  ret i64 %x8
}
define i64 @c_b(i64 %a, i64 %b, i64 %c, i64 %d, i64 %e, i64 %f, i64 %g,
                ptr byval(%s64) align 64 %p, i64 %h) {
  %x1 = call i64 @mix(i64 %a, i64 %b)
  %x2 = call i64 @mix(i64 %x1, i64 %c)
  %x3 = call i64 @mix(i64 %x2, i64 %d)
  %x4 = call i64 @mix(i64 %x3, i64 %e)
  %x5 = call i64 @mix(i64 %x4, i64 %f)
  %x6 = call i64 @mix(i64 %x5, i64 %g)
  %x7 = call i64 @words(i64 %x6, ptr %p, i64 8, i64 64)
  %x8 = call i64 @mix(i64 %x7, i64 %h)
  ret i64 %x8
}
define i64 @c_c(i64 %a, ptr byval(%s64) align 64 %p, ptr byval(%s32) align 32 %m,
                ptr byval(%s64) align 64 %q) {
  %x1 = call i64 @words(i64 %a, ptr %p, i64 8, i64 64)
  %x2 = call i64 @words(i64 %x1, ptr %m, i64 4, i64 32)
  %x3 = call i64 @words(i64 %x2, ptr %q, i64 8, i64 64)
  ret i64 %x3
}
define i64 @d_a(i64 %s) {
  %p = alloca %s32, align 32
  call void @fill(ptr %p, i64 4, i64 %s)
  %t = add i64 %s, 1
  %r = call i64 @t_a(i64 1, i64 2, i64 3, i64 4, i64 5, i64 %s, i64 %t,
                     ptr byval(%s32) align 32 %p, i64 -8)
  ret i64 %r
}
define i64 @d_b(i64 %s) {
  %p = alloca %s64, align 64
  call void @fill(ptr %p, i64 8, i64 %s)
  %t = add i64 %s, 2
  %r = call i64 @t_b(i64 %s, i64 2, i64 3, i64 4, i64 5, i64 6, i64 %t,
                     ptr byval(%s64) align 64 %p, i64 -9)
  ret i64 %r
}
define i64 @d_c(i64 %s) {
  %p = alloca %s64, align 64
  %m = alloca %s32, align 32
  %q = alloca %s64, align 64
  call void @fill(ptr %p, i64 8, i64 %s)
  %t = add i64 %s, 3
  call void @fill(ptr %m, i64 4, i64 %t)
  %u = add i64 %s, 4
  call void @fill(ptr %q, i64 8, i64 %u)
  %r = call i64 @t_c(i64 %s, ptr byval(%s64) align 64 %p, ptr byval(%s32) align 32 %m,
                     ptr byval(%s64) align 64 %q)
  ret i64 %r
}
define i64 @d_d(i64 %s) {
  %p = alloca %s64, align 64
  call void @fill(ptr %p, i64 8, i64 %s)
  %r = call i64 (i64, ptr, ...) @t_d(i64 %s, ptr byval(%s64) align 64 %p,
                     i64 11, i64 12, i64 13, i64 14, i64 15, i64 %s, i64 -17)
  ret i64 %r
}
";

/// The variadic callee, by `llc` 22 alone (hod has no `va_arg`).
const VA_CALLEE: &str = "
declare void @llvm.va_start.p0(ptr)
declare void @llvm.va_end.p0(ptr)
declare i64 @mix(i64, i64)
declare i64 @words(i64, ptr, i64, i64)
define i64 @c_d(i64 %a, ptr byval(%s64) align 64 %p, ...) {
  %ap = alloca [24 x i8], align 8
  call void @llvm.va_start.p0(ptr %ap)
  %x0 = call i64 @words(i64 %a, ptr %p, i64 8, i64 64)
  %v1 = va_arg ptr %ap, i64
  %x1 = call i64 @mix(i64 %x0, i64 %v1)
  %v2 = va_arg ptr %ap, i64
  %x2 = call i64 @mix(i64 %x1, i64 %v2)
  %v3 = va_arg ptr %ap, i64
  %x3 = call i64 @mix(i64 %x2, i64 %v3)
  %v4 = va_arg ptr %ap, i64
  %x4 = call i64 @mix(i64 %x3, i64 %v4)
  %v5 = va_arg ptr %ap, i64
  %x5 = call i64 @mix(i64 %x4, i64 %v5)
  %v6 = va_arg ptr %ap, i64
  %x6 = call i64 @mix(i64 %x5, i64 %v6)
  %v7 = va_arg ptr %ap, i64
  %x7 = call i64 @mix(i64 %x6, i64 %v7)
  call void @llvm.va_end.p0(ptr %ap)
  ret i64 %x7
}
";

const DRIVER: &str = r#"
#include <stdio.h>
#include <stdint.h>
int64_t d_a(int64_t), d_b(int64_t), d_c(int64_t), d_d(int64_t);
int main(void) {
    int64_t (*fns[])(int64_t) = {d_a, d_b, d_c, d_d};
    int64_t seeds[] = {0, 1, -1, 0x123456789abcdefLL};
    for (unsigned f = 0; f < 4; f++) {
        for (unsigned s = 0; s < 4; s++) printf("%llx ", (unsigned long long)fns[f](seeds[s]));
        printf("\n");
        fflush(stdout);
    }
    return 0;
}
"#;

fn options() -> Options {
    Options {
        output_kind: OutputKind::Object,
        relocation_model: Some(RelocModel::Pic),
        ..Options::default()
    }
}

/// The facade's answer for `text` (a module without its header).
fn facade(text: &str) -> Result<Vec<u8>, CompileError> {
    let text = format!("{}{text}", header());
    hod::compile_with(text.as_bytes(), &options(), &Settings::default(), &mut Report::default())
}

/// The name a refusal gives, `None` when the module compiles.
fn refusal(text: &str) -> Option<String> {
    match facade(text) {
        Ok(_) => None,
        Err(CompileError::Unsupported(unsupported)) => Some(unsupported.construct.name().into()),
        Err(other) => panic!("{other}\n{text}"),
    }
}

/// `hod_x64` below the facade with every stress option.
fn stressed(text: &str) -> Vec<u8> {
    let text = format!("{}{text}", header());
    let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
        .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    let side = parsed.text;
    let mut module = parsed.module;
    let mut bodies = Vec::new();
    hod_ll::parse_bodies(&mut module, &side, |_, read| {
        bodies.push(read.body.expect("every body is read").clone());
        Ok::<(), hod_ll::LlError>(())
    })
    .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    let options = hod_cg::Options {
        reloc_model: hod_cg::RelocModel::Pic,
        stress: hod_cg::Stress::ALL,
        ..hod_cg::Options::default()
    };
    hod_x64::compile_to_object(&module, &bodies, &options).unwrap_or_else(|e| panic!("{e}"))
}

/// `llc` 22, `-O0 -fast-isel=0 -relocation-model=pic`, if installed.
fn llc22(dir: &Scratch, name: &str, text: &str) -> Option<PathBuf> {
    let llc = PathBuf::from("/usr/lib/llvm-22/bin/llc");
    if !llc.is_file() {
        assert!(!hod_testutil::tools_required(), "llc 22 is required");
        eprintln!("note: llc 22 not found");
        return None;
    }
    let input = dir.write(&format!("{name}.ll"), format!("{}{text}", header()));
    let output = dir.join(format!("{name}.o"));
    let mut command = Command::new(llc);
    command.args(["-O0", "-fast-isel=0", "-relocation-model=pic", "-filetype=obj"]);
    command.arg("-mtriple=x86_64-unknown-linux-gnu").arg(&input).arg("-o").arg(&output);
    let ran = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(ran.success(), "llc 22: {}", ran.stderr_text());
    Some(output)
}

/// §8.5: the thunks compile through the facade and run as `llc` 22's do,
/// every copy aligned, with `llc` 22's callers and callees and with
/// hod's, hod plain and stressed, under every linker.
#[test]
fn musttail_thunks_with_aligned_byval_run_like_llc() {
    let dir = Scratch::new("hod-musttail-byval");
    let thunks = format!("{TYPES}{THUNKS}");
    let ends = format!("{TYPES}{ENDS}");
    let mut thunk_objects: Vec<(&str, PathBuf)> = vec![
        ("facade", dir.write("thunks-hod.o", facade(&thunks).unwrap_or_else(|e| panic!("{e}")))),
        ("stress", dir.write("thunks-stress.o", stressed(&thunks))),
    ];
    let mut end_objects: Vec<(&str, PathBuf)> =
        vec![("facade", dir.write("ends-hod.o", facade(&ends).unwrap_or_else(|e| panic!("{e}"))))];
    let Some(llc_thunks) = llc22(&dir, "thunks-llc22", &thunks) else { return };
    let llc_ends = llc22(&dir, "ends-llc22", &ends).expect("llc 22 compiled the thunks");
    let va = llc22(&dir, "va-llc22", &format!("{TYPES}{VA_CALLEE}")).expect("llc 22");
    thunk_objects.insert(0, ("llc22", llc_thunks));
    end_objects.insert(0, ("llc22", llc_ends));
    let Some(toolchain) = Toolchain::x86_64(&dir) else { return };
    let driver = toolchain.compile_c(&dir, "driver", DRIVER, &["-fPIE"]);
    let mut expected: Option<String> = None;
    let mut runs = 0;
    for (e, ends) in &end_objects {
        for (t, thunks) in &thunk_objects {
            for linker in &toolchain.linkers {
                let exe = dir.join(format!("p-{t}-{e}-{}", linker.name));
                let inputs: [&Path; 4] = [&driver, thunks, ends, &va];
                let ran = toolchain.link_and_run(linker, &["-pie"], &inputs, &exe);
                assert_eq!(
                    exit_code(&ran),
                    Some(0),
                    "thunks {t}, ends {e} [{}]: {}{}",
                    linker.name,
                    ran.stdout_text(),
                    ran.stderr_text()
                );
                let out = ran.stdout_text();
                match &expected {
                    Some(want) => assert_eq!(&out, want, "thunks {t}, ends {e} [{}]", linker.name),
                    None => expected = Some(out),
                }
                runs += 1;
            }
        }
    }
    eprintln!("musttail byval: {runs} programs agree with llc 22 alone");
}

/// §8.5: a `musttail` call that adds stack arguments of its own before
/// the forwarded `...` after an aligned copy grows the area and is
/// refused; one whose own arguments go in registers is
/// `x64:musttail-variable-arguments`. A callee whose copy is aligned
/// otherwise than the caller's is malformed (`hod_ir`'s verifier, as
/// LLVM's): a callee with fewer or more stack bytes than the caller is
/// not expressible but through the variable arguments.
#[test]
fn musttail_with_aligned_byval_that_grows_is_refused_by_name() {
    let thunk = |extra: &str| {
        format!(
            "{TYPES}declare i64 @c_d(i64, ptr byval(%s64) align 64, ...)\n\
             define i64 @t(i64 %a, ptr byval(%s64) align 64 %p, ...) {{\n  \
             %r = musttail call i64 (i64, ptr, ...) @c_d(i64 %a, ptr byval(%s64) align 64 %p{extra}, ...)\n  \
             ret i64 %r\n}}\n"
        )
    };
    assert_eq!(refusal(&thunk("")), None);
    assert_eq!(refusal(&thunk(", i64 1")).as_deref(), Some("x64:musttail-variable-arguments"));
    assert_eq!(
        refusal(&thunk(", i64 1, i64 2, i64 3, i64 4, i64 5, i64 6")).as_deref(),
        Some("musttail:stack-args-grow")
    );
    for (caller, callee) in [(16, 64), (64, 32)] {
        let text = format!(
            "{TYPES}declare i64 @c(i64, ptr byval(%s64) align {callee})\n\
             define i64 @t(i64 %a, ptr byval(%s64) align {caller} %p) {{\n  \
             %r = musttail call i64 @c(i64 %a, ptr byval(%s64) align {callee} %p)\n  \
             ret i64 %r\n}}\n"
        );
        match facade(&text) {
            Err(CompileError::MalformedInput(why)) => {
                assert!(why.contains("different ABI attributes"), "{why}");
            }
            other => panic!("align {caller} -> {callee}: {:?}", other.map(|o| o.len())),
        }
    }
}
