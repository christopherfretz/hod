//! Stack probing, `"probe-stack"="inline-asm"` and `"stack-probe-size"`
//! (WP-186, `spec/ir.md` §5.7, `spec/codegen.md` §5.6.27), by execution
//! on both targets (x86-64 under `qemu-x86_64` on another host) and by
//! shape against `llc -O0 -global-isel=0 -fast-isel=0` 19 and 22.
//!
//! - Values: functions whose frames are around every boundary (the probe
//!   size `P` ± 16, `2P`, `llc`'s unroll limits, 1 MiB), a dynamic
//!   `alloca` (16- and 64-byte aligned) and an over-aligned slot fill
//!   their buffer, call, and check it: a stack pointer left above the
//!   frame shows as a byte the call overwrote.
//! - The guard: on a thread whose stack has a `PROT_NONE` guard below
//!   it, a function with a 1 MiB frame (and one with a 1 MiB `alloca`)
//!   faults with the attribute within `P` below the guard's top, and the
//!   signal handler's backtrace (`backtrace()`, the unwinder through the
//!   signal frame and hod's CFI rows at the faulting probe, inside the
//!   loop) reaches the caller; without the attribute the frame lands past
//!   the guard, and the first fault is the buffer's fill reaching the
//!   guard's bottom from below. The same program built by `llc` 19 and
//!   22 prints the same.
//! - Shape: per size class, the number of probes and whether there is a
//!   loop, as `llc`'s on x86-64; on AArch64 hod's own (its frame record
//!   is at the bottom of the frame, `spec/codegen.md` §5.6.27).
//! - Refusals by name: another `probe-stack` value, a probe size hod
//!   does not take.
//! - Text and bitcode give the same object.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hod::{Options, OutputKind, Report, Settings};
use hod_testutil::{LLVM_MAJORS, Scratch, Target, run_with, tool};

const P: u64 = 4096;

fn triple(target: Target) -> &'static str {
    match target {
        Target::Aarch64 => "aarch64-unknown-linux-gnu",
        Target::X86_64 => "x86_64-unknown-linux-gnu",
    }
}

fn with_target(body: &str, target: Target) -> String {
    let layout = match target {
        Target::Aarch64 => hod_ir::AARCH64_LINUX_DATA_LAYOUT,
        Target::X86_64 => hod_ir::X86_64_LINUX_DATA_LAYOUT,
    };
    format!("target datalayout = \"{layout}\"\ntarget triple = \"{}\"\n{body}", triple(target))
}

fn compile(input: &[u8]) -> Result<Vec<u8>, String> {
    let options = Options { output_kind: OutputKind::Object, ..Options::default() };
    hod::compile_with(input, &options, &Settings::default(), &mut Report::default())
        .map_err(|error| error.to_string())
}

/// The local-array sizes of the value tests: around `P`, `2P`, x86-64's
/// unroll limit `8P`, AArch64's `4P` and `5P`, and 1 MiB.
fn sizes() -> Vec<u64> {
    let mut out = Vec::new();
    for base in [P, 2 * P, 4 * P, 5 * P, 8 * P, 9 * P] {
        for delta in [-32i64, -16, 0, 16, 32] {
            out.push(base.checked_add_signed(delta).expect("positive"));
        }
    }
    out.extend([16, 1000, 3000, 3072, 3088, 1 << 20]);
    out
}

/// A function `@<name>(i64 %x)` with an `N`-byte local array: `@fill`
/// writes `x`'s pattern over it, `@check` reads it back (0 when intact).
fn fixed(name: &str, n: u64, attrs: &str) -> String {
    format!(
        "define i64 @{name}(i64 %x) {attrs} {{
  %b = alloca [{n} x i8], align 16
  call void @fill(ptr %b, i64 {n}, i64 %x)
  %r = call i64 @check(ptr %b, i64 {n}, i64 %x)
  ret i64 %r
}}
"
    )
}

/// `@<name>(i64 %n, i64 %x)`: the same with an `n`-byte `alloca`.
fn dynamic(name: &str, align: u64, attrs: &str) -> String {
    format!(
        "define i64 @{name}(i64 %n, i64 %x) {attrs} {{
  %b = alloca i8, i64 %n, align {align}
  %p = ptrtoint ptr %b to i64
  %m = and i64 %p, {mask}
  call void @fill(ptr %b, i64 %n, i64 %x)
  %r = call i64 @check(ptr %b, i64 %n, i64 %x)
  %s = add i64 %r, %m
  ret i64 %s
}}
",
        mask = align - 1
    )
}

/// The module: the value functions, the guard functions with and without
/// the attribute.
fn module() -> String {
    let mut ir =
        String::from("declare void @fill(ptr, i64, i64)\ndeclare i64 @check(ptr, i64, i64)\n");
    for n in sizes() {
        ir += &fixed(&format!("f{n}"), n, "#0");
    }
    ir += &dynamic("dyn16", 16, "#0");
    ir += &dynamic("dyn64", 64, "#0");
    // An over-aligned slot: hod's entry allocation (`dynamic_slot`).
    ir += "define i64 @overaligned(i64 %x) #0 {
  %b = alloca [40000 x i8], align 128
  %p = ptrtoint ptr %b to i64
  %m = and i64 %p, 127
  call void @fill(ptr %b, i64 40000, i64 %x)
  %r = call i64 @check(ptr %b, i64 40000, i64 %x)
  %s = add i64 %r, %m
  ret i64 %s
}
";
    ir += &fixed("guard_fixed", 1 << 20, "#0");
    ir += &fixed("plain_fixed", 1 << 20, "#1");
    ir += &dynamic("guard_dyn", 16, "#0");
    ir += &dynamic("plain_dyn", 16, "#1");
    // `uwtable` as clang writes it: `llc` then makes its CFI precise at
    // every instruction (without it, AArch64's rows come after the loop).
    ir += "attributes #0 = { uwtable \"probe-stack\"=\"inline-asm\" \"frame-pointer\"=\"all\" }\n";
    ir += "attributes #1 = { uwtable \"frame-pointer\"=\"all\" }\n";
    ir
}

/// The C side: `fill`, `check`, the value calls, and the guard runs on a
/// thread whose stack is `[guard | stack]` above a mapped region the
/// frames of the plain functions land in.
fn harness() -> String {
    let mut c = String::from(HARNESS);
    c += "static int values(void) {\n  int bad = 0;\n";
    for n in sizes() {
        writeln!(c, "  {{ extern long f{n}(long); bad |= f{n}({n}) != 0; }}").unwrap();
    }
    c += "  bad |= dyn16(100000, 3) != 0 || dyn16(0, 4) != 0 || dyn16(4096, 5) != 0;\n";
    c += "  bad |= dyn64(100000, 6) != 0 || dyn64(1 << 20, 7) != 0 || dyn64(16, 8) != 0;\n";
    c += "  bad |= overaligned(9) != 0;\n  return bad;\n}\n";
    c += MAIN;
    c
}

const HARNESS: &str = r#"#define _GNU_SOURCE
#include <execinfo.h>
#include <pthread.h>
#include <setjmp.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/mman.h>

long dyn16(long, long);
long dyn64(long, long);
long overaligned(long);
long guard_fixed(long);
long plain_fixed(long);
long guard_dyn(long, long);
long plain_dyn(long, long);

void fill(unsigned char *p, long n, long x) {
  for (long i = 0; i < n; i++) p[i] = (unsigned char)(i * 7 + x);
}
long check(unsigned char *p, long n, long x) {
  for (long i = 0; i < n; i++)
    if (p[i] != (unsigned char)(i * 7 + x)) return i + 1;
  return 0;
}

#define VICTIM (4L << 20)
#define GUARD (64L << 10)
#define STACK (256L << 10)
static char *guard_top;
static sigjmp_buf back;
static volatile uintptr_t fault;
static volatile int unwound;
static int which;

__attribute__((noinline)) long caller_of_probe(void) {
  long r;
  switch (which) {
  case 0: r = guard_fixed(1); break;
  case 1: r = plain_fixed(1); break;
  case 2: r = guard_dyn(1L << 20, 1); break;
  default: r = plain_dyn(1L << 20, 1); break;
  }
  return r + 0;
}
__attribute__((noinline)) void caller_end(void) {}

static void on_fault(int sig, siginfo_t *info, void *context) {
  (void)sig;
  (void)context;
  fault = (uintptr_t)info->si_addr;
  void *frames[32];
  int n = backtrace(frames, 32);
  for (int i = 0; i < n; i++) {
    uintptr_t at = (uintptr_t)frames[i];
    if (at > (uintptr_t)caller_of_probe && at < (uintptr_t)caller_of_probe + 512) unwound = 1;
  }
  siglongjmp(back, 1);
}

static void *on_thread(void *arg) {
  (void)arg;
  static char alt[1 << 16];
  stack_t ss = {.ss_sp = alt, .ss_size = sizeof alt, .ss_flags = 0};
  sigaltstack(&ss, 0);
  if (sigsetjmp(back, 1) == 0) {
    long r = caller_of_probe();
    printf("%s: returned %ld\n", which & 1 ? "plain" : "probed", r);
  } else {
    uintptr_t top = (uintptr_t)guard_top;
    int near = fault < top && fault >= top - 4096;
    printf("%s: fault near=%d unwound=%d\n", which & 1 ? "plain" : "probed", near, unwound);
  }
  return 0;
}

static void guard_run(int what) {
  char *base = mmap(0, VICTIM + GUARD + STACK, PROT_READ | PROT_WRITE,
                    MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
  if (base == MAP_FAILED) { printf("mmap failed\n"); return; }
  mprotect(base + VICTIM, GUARD, PROT_NONE);
  guard_top = base + VICTIM + GUARD;
  which = what;
  fault = 0;
  unwound = 0;
  pthread_attr_t attr;
  pthread_attr_init(&attr);
  pthread_attr_setstack(&attr, guard_top, STACK);
  pthread_t thread;
  pthread_create(&thread, &attr, on_thread, 0);
  pthread_join(thread, 0);
  munmap(base, VICTIM + GUARD + STACK);
}
"#;

const MAIN: &str = r#"
int main(void) {
  void *warm[4];
  backtrace(warm, 4);
  struct sigaction sa;
  memset(&sa, 0, sizeof sa);
  sa.sa_sigaction = on_fault;
  sa.sa_flags = SA_SIGINFO | SA_ONSTACK;
  sigaction(SIGSEGV, &sa, 0);
  printf("values: %s\n", values() ? "WRONG" : "ok");
  for (int what = 0; what < 4; what++) {
    printf("%s ", what < 2 ? "fixed" : "alloca");
    fflush(stdout);
    guard_run(what);
  }
  return 0;
}
"#;

/// What every build prints: the attribute makes the 1 MiB frame and the
/// 1 MiB `alloca` fault within one probe size below the guard's top, with
/// a backtrace that reaches the caller; without it the frame lands below
/// the guard and the first fault is `fill`'s, writing upwards, at the
/// guard's bottom.
const EXPECTED: &str = "values: ok
fixed probed: fault near=1 unwound=1
fixed plain: fault near=0 unwound=1
alloca probed: fault near=1 unwound=1
alloca plain: fault near=0 unwound=1
";

/// Links `object` with the harness for `target` and runs it; its output.
fn link_and_run(dir: &Scratch, target: Target, name: &str, object: &Path) -> Option<String> {
    let gcc = target.tool("gcc")?;
    let c = dir.write("harness.c", harness());
    let program = dir.join(name);
    let mut link = Command::new(gcc);
    link.args(["-O0", "-w"]).arg(&c).arg(object).args(["-pthread", "-o"]).arg(&program);
    let linked = run_with(&mut link, b"", Duration::from_mins(2));
    assert!(linked.success(), "{name}: the program links: {}", linked.stderr_text());
    let mut run = if target.is_host() {
        Command::new(&program)
    } else {
        let (emulator, root) = (target.tool("emulator")?, target.tool("sysroot")?);
        let mut run = Command::new(emulator);
        run.args(target.emulator_args(&root)).arg(&program);
        run
    };
    let ran = run_with(&mut run, b"", Duration::from_mins(2));
    assert!(ran.success(), "{name}: {ran:?}");
    Some(String::from_utf8_lossy(&ran.stdout).into_owned())
}

fn llc_object(dir: &Scratch, major: u32, text: &str, name: &str) -> Option<PathBuf> {
    let llc = tool(&format!("llc-{major}"))?;
    let object = dir.join(name);
    let mut command = Command::new(llc);
    command.args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj", "-o"]).arg(&object);
    let ran = run_with(&mut command, text.as_bytes(), Duration::from_mins(2));
    assert!(ran.success(), "llc {major}: {}", ran.stderr_text());
    Some(object)
}

/// §5.6.27: hod's program and `llc`'s print [`EXPECTED`], on both targets.
#[test]
fn probed_frames_fault_in_the_guard_and_unwind() {
    let dir = Scratch::new("stack-probe-run");
    for target in Target::ALL {
        let text = with_target(&module(), target);
        let object = compile(text.as_bytes()).expect("hod compiles the module");
        let path = dir.write(&format!("hod-{}.o", target.name()), object);
        let Some(out) = link_and_run(&dir, target, &format!("hod-{}", target.name()), &path) else {
            return;
        };
        assert_eq!(out, EXPECTED, "hod, {}", target.name());
        for major in LLVM_MAJORS {
            let name = format!("llc{major}-{}", target.name());
            let Some(object) = llc_object(&dir, major, &text, &format!("{name}.o")) else {
                continue;
            };
            let out = link_and_run(&dir, target, &name, &object).expect("the tools were found");
            assert_eq!(out, EXPECTED, "llc {major}, {}", target.name());
        }
    }
}

/// The module's text and its bitcode by every `llvm-as` give one object.
#[test]
fn text_and_bitcode_give_the_same_object() {
    for target in Target::ALL {
        let text = with_target(&module(), target);
        let from_text = compile(text.as_bytes()).expect("hod compiles the text");
        for major in LLVM_MAJORS {
            let Some(llvm_as) = tool(&format!("llvm-as-{major}")) else { continue };
            let mut command = Command::new(llvm_as);
            command.args(["-o", "-", "-"]);
            let ran = run_with(&mut command, text.as_bytes(), Duration::from_mins(1));
            assert!(ran.success(), "llvm-as {major}: {}", ran.stderr_text());
            let from_bitcode = compile(&ran.stdout).expect("hod compiles the bitcode");
            assert!(from_bitcode == from_text, "{}: bitcode {major} differs", target.name());
        }
    }
}

/// `"zero-call-used-regs"="skip"` zeroes nothing: `llc` 19 and 22 give the
/// same object with it as without it, and so does hod; another value
/// stays refused by name.
#[test]
fn zero_call_used_regs_skip_changes_nothing() {
    let dir = Scratch::new("stack-probe-zero");
    let body = |attrs: &str| {
        format!(
            "declare i64 @g(i64, double)\n\
             define i64 @f(i64 %a, double %b) #0 {{\n\
             \x20 %c = call i64 @g(i64 %a, double %b)\n\
             \x20 %d = add i64 %c, %a\n\
             \x20 ret i64 %d\n}}\n\
             attributes #0 = {{ nounwind \"frame-pointer\"=\"all\" {attrs} }}\n"
        )
    };
    for target in Target::ALL {
        let plain = with_target(&body(""), target);
        let skip = with_target(&body("\"zero-call-used-regs\"=\"skip\""), target);
        let ours = compile(plain.as_bytes()).expect("hod compiles the plain function");
        assert!(compile(skip.as_bytes()).expect("hod accepts skip") == ours, "{}", target.name());
        for major in LLVM_MAJORS {
            let name = format!("llc{major}-{}", target.name());
            let Some(a) = llc_object(&dir, major, &plain, &format!("{name}-plain.o")) else {
                continue;
            };
            let b = llc_object(&dir, major, &skip, &format!("{name}-skip.o")).expect("llc");
            let (a, b) = (std::fs::read(a).unwrap(), std::fs::read(b).unwrap());
            assert!(a == b, "llc {major}, {}: skip changes the object", target.name());
        }
        for value in ["used-gpr", "all"] {
            let text = with_target(&body(&format!("\"zero-call-used-regs\"=\"{value}\"")), target);
            let error = compile(text.as_bytes()).unwrap_err();
            assert!(
                error.contains("attr:zero-call-used-regs"),
                "{}: {value}: {error}",
                target.name()
            );
        }
    }
}

/// Another `probe-stack` value and a probe size hod does not take are
/// refused by name; `"stack-probe-size"` alone is accepted.
#[test]
fn other_values_are_refused_by_name() {
    let body = |attrs: &str| {
        format!("define void @f() #0 {{\n  ret void\n}}\nattributes #0 = {{ {attrs} }}\n")
    };
    for target in Target::ALL {
        for (attrs, name) in [
            ("\"probe-stack\"=\"__probestack\"", "attr:probe-stack-function"),
            (
                "\"probe-stack\"=\"inline-asm\" \"stack-probe-size\"=\"1000\"",
                "attr:stack-probe-size-value",
            ),
            (
                "\"probe-stack\"=\"inline-asm\" \"stack-probe-size\"=\"0\"",
                "attr:stack-probe-size-value",
            ),
            (
                "\"probe-stack\"=\"inline-asm\" \"stack-probe-size\"=\"0x1000\"",
                "attr:stack-probe-size-value",
            ),
        ] {
            let error = compile(with_target(&body(attrs), target).as_bytes()).unwrap_err();
            assert!(error.contains(name), "{}: {attrs}: {error}", target.name());
        }
        for attrs in [
            "\"stack-probe-size\"=\"1000\"",
            "\"probe-stack\"=\"inline-asm\" \"stack-probe-size\"=\"65536\"",
        ] {
            compile(with_target(&body(attrs), target).as_bytes())
                .unwrap_or_else(|e| panic!("{}: {attrs}: {e}", target.name()));
        }
    }
}

/// Per function of `object`, the probes of its prologue (stores of zero
/// to the stack pointer before the first call) and whether it has a loop
/// (a backward conditional branch), from `llvm-objdump -d`.
fn shapes(objdump: &Path, object: &Path) -> Vec<(String, usize, bool)> {
    let mut command = Command::new(objdump);
    command.arg("-d").arg("--no-show-raw-insn").arg(object);
    let ran = run_with(&mut command, b"", Duration::from_mins(1));
    assert!(ran.success(), "llvm-objdump: {}", ran.stderr_text());
    let text = String::from_utf8_lossy(&ran.stdout).into_owned();
    let mut out: Vec<(String, usize, bool)> = Vec::new();
    let mut done = false;
    for line in text.lines() {
        if let Some(name) = line.strip_suffix(">:").and_then(|l| l.split_once('<')) {
            out.push((name.1.to_owned(), 0, false));
            done = false;
            continue;
        }
        let Some(last) = out.last_mut() else { continue };
        let fields: Vec<&str> = line.split('\t').map(str::trim).filter(|f| !f.is_empty()).collect();
        if done || fields.len() < 2 {
            continue;
        }
        let (op, args) = (fields[1], fields.get(2).copied().unwrap_or(""));
        match op {
            "callq" | "call" | "bl" => done = true,
            "movq" if args == "$0x0, (%rsp)" => last.1 += 1,
            "str" if args == "xzr, [sp]" => last.1 += 1,
            "jne" | "b.ne" => last.2 = true,
            _ => {}
        }
    }
    out
}

/// x86-64: hod's prologue probes as `llc`'s does (the count of unrolled
/// probes and the loop, per size class), 19 and 22. AArch64: hod's own
/// shape (§5.6.27), against `llc`'s loop decision only where both put
/// the same bytes above the frame record.
#[test]
fn prologue_shapes_are_llc_s() {
    let Some(objdump) = tool("llvm-objdump") else { return };
    let dir = Scratch::new("stack-probe-shape");
    let mut ir = String::from("declare void @g(ptr)\n");
    let classes: Vec<u64> = [1u64, 2, 4, 5, 8, 9, 256].iter().map(|k| k * P).collect();
    for &n in &classes {
        for (tag, delta) in [("lo", -64i64), ("hi", 64)] {
            let size = n.checked_add_signed(delta).unwrap();
            writeln!(
                ir,
                "define void @s{n}_{tag}() #0 {{\n  %b = alloca [{size} x i8], align 16\n  \
                 call void @g(ptr %b)\n  ret void\n}}"
            )
            .unwrap();
        }
    }
    ir += "attributes #0 = { \"probe-stack\"=\"inline-asm\" \"frame-pointer\"=\"all\" }\n";
    for target in Target::ALL {
        let text = with_target(&ir, target);
        let hod = dir.write("hod.o", compile(text.as_bytes()).expect("hod compiles"));
        let ours = shapes(&objdump, &hod);
        assert_eq!(ours.len(), 2 * classes.len(), "{}: {ours:?}", target.name());
        if target == Target::Aarch64 {
            // hod: `str xzr, [sp, #-16]!` (not counted) and one probe per
            // whole page below it, unrolled up to 4.
            for (name, probes, looped) in &ours {
                let k = expected_a64(name);
                assert_eq!(
                    (*probes, *looped),
                    if k > 4 { (1, true) } else { (k, false) },
                    "{name}"
                );
            }
        }
        for major in LLVM_MAJORS {
            let Some(object) = llc_object(&dir, major, &text, &format!("llc{major}.o")) else {
                continue;
            };
            let theirs = shapes(&objdump, &object);
            assert_eq!(ours.len(), theirs.len());
            for (hod, llc) in ours.iter().zip(&theirs) {
                assert_eq!(hod.0, llc.0);
                match target {
                    Target::X86_64 => assert_eq!(hod, llc, "llc {major}"),
                    // The loop decisions agree; llc's probe of the rest
                    // above 1024 bytes is hod's `stp`.
                    Target::Aarch64 => assert_eq!(hod.2, llc.2, "llc {major}: {hod:?} {llc:?}"),
                }
            }
        }
    }
}

/// hod's AArch64 frame for `@s<n>_<tag>`: the array, 16 for the frame
/// record; `k` = whole pages below the top 16 bytes.
fn expected_a64(name: &str) -> usize {
    let (n, tag) = name[1..].split_once('_').unwrap();
    let n: u64 = n.parse().unwrap();
    let size = if tag == "lo" { n - 64 } else { n + 64 };
    let frame = (size + 16).next_multiple_of(16);
    if frame + 1024 <= P { 0 } else { ((frame - 16) / P) as usize }
}
