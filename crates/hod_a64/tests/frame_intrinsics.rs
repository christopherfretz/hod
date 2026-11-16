//! The frame as an unwinder sees it on AArch64 (WP-215,
//! `spec/codegen.md` §5.6.38): `llvm.sponentry`,
//! `llvm.addressofreturnaddress`, `llvm.eh.unwind.init`, and
//! `llvm.frameaddress`/`llvm.returnaddress` above depth 0 beside them.
//!
//! - One program, built by hod (plain and stressed) and by `llc -O0
//!   -global-isel=0 -fast-isel=0` of LLVM 19 and 22, linked with one C++
//!   driver by bfd, lld and mold and run: a function walks its own frame
//!   chain to depth 3 and names the functions the return addresses are
//!   in; `llvm.sponentry` is the caller's stack pointer before the call
//!   (plain, with an over-aligned local, after a VLA, in a variadic
//!   function); `llvm.addressofreturnaddress` is where the frame record
//!   keeps the return address; `__builtin_unwind_init` puts the eighteen
//!   callee-saved registers in the frame; an exception is thrown through
//!   such a frame. Every build prints the same.
//! - The unwind rows of a function with `eh.unwind.init` save the
//!   registers `llc` saves.
//! - `llvm.eh.dwarf.cfa` (WP-294): in one program `__builtin_dwarf_cfa()`
//!   (plain, with an over-aligned local, after a VLA, in a variadic
//!   function, with outgoing and with incoming stack arguments, with
//!   constant and variable offsets) is the caller's stack pointer before
//!   the call plus the offset (where the call has no stack arguments),
//!   and the CFA libgcc's unwinder computes
//!   from the frame's own rows (`_Unwind_GetCFA`). Built by `llc` 19 and
//!   22 the same program prints every value 16 lower (their `x29 + k`):
//!   hod follows the unwind rows and GCC, not `llc`.
//! - `llvm.eh.return.i64` is refused by its row (`llc` 19 and 22 emit
//!   nothing for it), `eh.unwind.init` where the frame has no room for
//!   its area by name.

mod common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use common::linking::{Toolchain, exit_code};
use common::{assert_refused, ll_header, parse};
use hod_cg::{Options, RelocModel, Stress};
use hod_testutil::{LLVM_MAJORS, Scratch, find_tool, run, run_with, tool};

/// The functions under test.
const FUNCTIONS: &str = r#"
declare ptr @llvm.frameaddress.p0(i32)
declare ptr @llvm.returnaddress(i32)
declare ptr @llvm.sponentry.p0()
declare ptr @llvm.addressofreturnaddress.p0()
declare ptr @llvm.stacksave.p0()
declare void @llvm.eh.unwind.init()
declare void @llvm.va_start.p0(ptr)
declare void @llvm.va_end.p0(ptr)
declare void @sink(ptr)
declare i64 @count_magic(ptr, ptr)
declare i64 @leaf(i64)

define void @walk(ptr %out) #0 {
  %r0 = call ptr @llvm.returnaddress(i32 0)
  %r1 = call ptr @llvm.returnaddress(i32 1)
  %r2 = call ptr @llvm.returnaddress(i32 2)
  %r3 = call ptr @llvm.returnaddress(i32 3)
  %f1 = call ptr @llvm.frameaddress.p0(i32 1)
  %f2 = call ptr @llvm.frameaddress.p0(i32 2)
  %f3 = call ptr @llvm.frameaddress.p0(i32 3)
  store ptr %r0, ptr %out
  %o1 = getelementptr ptr, ptr %out, i64 1
  store ptr %r1, ptr %o1
  %o2 = getelementptr ptr, ptr %out, i64 2
  store ptr %r2, ptr %o2
  %o3 = getelementptr ptr, ptr %out, i64 3
  store ptr %r3, ptr %o3
  %o4 = getelementptr ptr, ptr %out, i64 4
  store ptr %f1, ptr %o4
  %o5 = getelementptr ptr, ptr %out, i64 5
  store ptr %f2, ptr %o5
  %o6 = getelementptr ptr, ptr %out, i64 6
  store ptr %f3, ptr %o6
  ret void
}

define void @level3(ptr %out) #0 {
  %f = call ptr @llvm.frameaddress.p0(i32 0)
  %o = getelementptr ptr, ptr %out, i64 7
  store ptr %f, ptr %o
  call void @walk(ptr %out)
  ret void
}

define void @level2(ptr %out) #0 {
  %pad = alloca [40 x i8], align 16
  call void @sink(ptr %pad)
  %f = call ptr @llvm.frameaddress.p0(i32 0)
  %o = getelementptr ptr, ptr %out, i64 8
  store ptr %f, ptr %o
  call void @level3(ptr %out)
  ret void
}

define void @level1(ptr %out) #0 {
  %f = call ptr @llvm.frameaddress.p0(i32 0)
  %o = getelementptr ptr, ptr %out, i64 9
  store ptr %f, ptr %o
  call void @level2(ptr %out)
  ret void
}

define ptr @spe_plain() #0 {
  %c = call ptr @llvm.sponentry.p0()
  ret ptr %c
}

define ptr @spe_aligned() #0 {
  %a = alloca [64 x i8], align 64
  call void @sink(ptr %a)
  %c = call ptr @llvm.sponentry.p0()
  ret ptr %c
}

define ptr @spe_vla(i64 %n) #0 {
  %a = alloca i8, i64 %n, align 16
  call void @sink(ptr %a)
  %c = call ptr @llvm.sponentry.p0()
  ret ptr %c
}

define ptr @spe_va(i32 %n, ...) #0 {
  %ap = alloca [32 x i8], align 8
  call void @llvm.va_start.p0(ptr %ap)
  %c = call ptr @llvm.sponentry.p0()
  call void @llvm.va_end.p0(ptr %ap)
  ret ptr %c
}

define void @distance(ptr %out, i64 %i, ptr %sp, ptr %at) #0 {
  %s = ptrtoint ptr %sp to i64
  %c = ptrtoint ptr %at to i64
  %d = sub i64 %c, %s
  %o = getelementptr i64, ptr %out, i64 %i
  store i64 %d, ptr %o
  ret void
}

; Each entry stack pointer less the stack pointer before the call.
define void @spe_distances(ptr %out) #0 {
  %big = alloca [64 x i8], align 64
  call void @sink(ptr %big)
  %s0 = call ptr @llvm.stacksave.p0()
  %c0 = call ptr @spe_plain()
  call void @distance(ptr %out, i64 0, ptr %s0, ptr %c0)
  %s1 = call ptr @llvm.stacksave.p0()
  %c1 = call ptr @spe_aligned()
  call void @distance(ptr %out, i64 1, ptr %s1, ptr %c1)
  %s2 = call ptr @llvm.stacksave.p0()
  %c2 = call ptr @spe_vla(i64 100)
  call void @distance(ptr %out, i64 2, ptr %s2, ptr %c2)
  %s3 = call ptr @llvm.stacksave.p0()
  %c3 = call ptr (i32, ...) @spe_va(i32 2, i64 7, double 1.5)
  call void @distance(ptr %out, i64 3, ptr %s3, ptr %c3)
  ret void
}

define i32 @aora_check() #0 {
  %a = call ptr @llvm.addressofreturnaddress.p0()
  %r = call ptr @llvm.returnaddress(i32 0)
  %v = load ptr, ptr %a
  %ok1 = icmp eq ptr %v, %r
  %f = call ptr @llvm.frameaddress.p0(i32 0)
  %g = getelementptr i8, ptr %f, i64 8
  %ok2 = icmp eq ptr %a, %g
  %ok = and i1 %ok1, %ok2
  %z = zext i1 %ok to i32
  ret i32 %z
}

define i64 @scan_saved() #0 {
  call void @llvm.eh.unwind.init()
  %lo = call ptr @llvm.stacksave.p0()
  %hi = call ptr @llvm.sponentry.p0()
  %n = call i64 @count_magic(ptr %lo, ptr %hi)
  ret i64 %n
}

define i64 @saving_thrower(i64 %x) #0 {
  call void @llvm.eh.unwind.init()
  %r = call i64 @leaf(i64 %x)
  %s = add i64 %r, 1
  ret i64 %s
}

; The same with a stack protector, probes and a VLA.
define i64 @guarded_thrower(i64 %x) #1 {
  %buf = alloca [8192 x i8], align 16
  call void @sink(ptr %buf)
  %v = alloca i8, i64 %x, align 16
  call void @sink(ptr %v)
  call void @llvm.eh.unwind.init()
  %e = call ptr @llvm.sponentry.p0()
  %s0 = ptrtoint ptr %e to i64
  %r = call i64 @leaf(i64 %x)
  %e2 = call ptr @llvm.sponentry.p0()
  %s1 = ptrtoint ptr %e2 to i64
  %d = sub i64 %s1, %s0
  %s = add i64 %r, %d
  ret i64 %s
}

attributes #0 = { noinline "frame-pointer"="all" }
attributes #1 = { noinline sspreq "frame-pointer"="all" "probe-stack"="inline-asm" }

; Without it `llc` reaches `__stack_chk_guard` directly, which a PIE
; cannot link.
!llvm.module.flags = !{!0}
!0 = !{i32 8, !"PIC Level", i32 2}
"#;

const DRIVER: &str = r#"
#include <stdio.h>
#include <string.h>
extern "C" {
void walk(void **);
void level1(void **);
void level2(void **);
void level3(void **);
void spe_distances(long *);
int aora_check(void);
long scan_saved(void);
long run_scan(void);
long saving_thrower(long);
long guarded_thrower(long);
__attribute__((noinline)) void sink(void *p) { asm volatile("" :: "r"(p) : "memory"); }
long count_magic(void **lo, void **hi) {
  long n = 0;
  for (void **p = lo; p < hi; p++)
    if (((unsigned long)*p >> 8) == 0xfeedfacecafe00UL) n++;
  return n;
}
long leaf(long x) {
  if (x == 42) throw 7;
  return x + 1;
}
__attribute__((noinline)) void run_walk(void **out) { level1(out); asm volatile(""); }
}
// Magic values in x19..x28 and d8..d15 around a call of scan_saved; every
// callee-saved register checked unchanged after it (0 if one is not).
asm(R"(
  .text
  .globl run_scan
  .type run_scan, %function
run_scan:
  stp x29, x30, [sp, #-176]!
  mov x29, sp
  stp x19, x20, [sp, #16]
  stp x21, x22, [sp, #32]
  stp x23, x24, [sp, #48]
  stp x25, x26, [sp, #64]
  stp x27, x28, [sp, #80]
  stp d8, d9, [sp, #96]
  stp d10, d11, [sp, #112]
  stp d12, d13, [sp, #128]
  stp d14, d15, [sp, #144]
  movz x16, #0xfeed, lsl #48
  movk x16, #0xface, lsl #32
  movk x16, #0xcafe, lsl #16
  add x19, x16, #1
  add x20, x16, #2
  add x21, x16, #3
  add x22, x16, #4
  add x23, x16, #5
  add x24, x16, #6
  add x25, x16, #7
  add x26, x16, #8
  add x27, x16, #9
  add x28, x16, #10
  add x17, x16, #11
  fmov d8, x17
  add x17, x16, #12
  fmov d9, x17
  add x17, x16, #13
  fmov d10, x17
  add x17, x16, #14
  fmov d11, x17
  add x17, x16, #15
  fmov d12, x17
  add x17, x16, #16
  fmov d13, x17
  add x17, x16, #17
  fmov d14, x17
  add x17, x16, #18
  fmov d15, x17
  str x16, [sp, #160]
  bl scan_saved
  ldr x16, [sp, #160]
  mov x2, #1
  add x17, x16, #1
  cmp x19, x17
  csel x2, xzr, x2, ne
  add x17, x16, #10
  cmp x28, x17
  csel x2, xzr, x2, ne
  fmov x3, d8
  add x17, x16, #11
  cmp x3, x17
  csel x2, xzr, x2, ne
  fmov x3, d15
  add x17, x16, #18
  cmp x3, x17
  csel x2, xzr, x2, ne
  mul x0, x0, x2
  ldp x19, x20, [sp, #16]
  ldp x21, x22, [sp, #32]
  ldp x23, x24, [sp, #48]
  ldp x25, x26, [sp, #64]
  ldp x27, x28, [sp, #80]
  ldp d8, d9, [sp, #96]
  ldp d10, d11, [sp, #112]
  ldp d12, d13, [sp, #128]
  ldp d14, d15, [sp, #144]
  ldp x29, x30, [sp], #176
  ret
  .size run_scan, .-run_scan
)");
static const char *name_of(void *a) {
  struct { const char *name; void *at; } known[] = {
    {"walk", (void *)walk}, {"level3", (void *)level3}, {"level2", (void *)level2},
    {"level1", (void *)level1}, {"run_walk", (void *)run_walk},
  };
  const char *best = "?";
  unsigned long at = 0;
  for (auto &k : known)
    if ((unsigned long)k.at <= (unsigned long)a && (unsigned long)k.at >= at) {
      at = (unsigned long)k.at;
      best = k.name;
    }
  return best;
}
int main() {
  void *out[10];
  memset(out, 0, sizeof out);
  run_walk(out);
  printf("walk: %s %s %s %s chain %d\n", name_of(out[0]), name_of(out[1]), name_of(out[2]),
         name_of(out[3]), out[4] == out[7] && out[5] == out[8] && out[6] == out[9]);
  long d[4];
  spe_distances(d);
  printf("sponentry: %ld %ld %ld %ld\n", d[0], d[1], d[2], d[3]);
  printf("aora: %d\n", aora_check());
  printf("saved by unwind.init: %ld\n", run_scan());
  long t = saving_thrower(1);
  int caught = 0;
  try {
    saving_thrower(42);
  } catch (int v) {
    caught = v;
  }
  printf("thrown through: %ld caught %d\n", t, caught);
  long g = guarded_thrower(5);
  caught = 0;
  try {
    guarded_thrower(42);
  } catch (int v) {
    caught = v;
  }
  printf("guarded, probed: %ld caught %d\n", g, caught);
  return 0;
}
"#;

const EXPECTED: &str = "walk: level3 level2 level1 run_walk chain 1
sponentry: 0 0 0 0
aora: 1
saved by unwind.init: 18
thrown through: 3 caught 7
guarded, probed: 6 caught 7
";

fn pic(stress: Stress) -> Options {
    Options { reloc_model: RelocModel::Pic, stress, ..Options::default() }
}

fn hod_object(dir: &Scratch, name: &str, text: &str, stress: Stress) -> PathBuf {
    let (m, bodies) = parse(text);
    let object = hod_a64::compile_to_object(&m, &bodies, &pic(stress))
        .unwrap_or_else(|e| panic!("hod: {e}"));
    dir.write(&format!("{name}.o"), object)
}

/// The `llc` of LLVM `major`, if present.
fn llc_of(major: u32) -> Option<PathBuf> {
    let installed = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/llc"));
    if installed.is_file() {
        return Some(installed);
    }
    find_tool(&format!("llc-{major}"))
}

/// `llc -O0 -global-isel=0 -fast-isel=0` of LLVM `major` for AArch64,
/// PIC, of `functions` (its header added) to an object in `dir`.
fn llc(dir: &Scratch, major: u32, name: &str, functions: &str) -> Option<PathBuf> {
    let llc = llc_of(major)?;
    let input = dir.write(&format!("{name}.ll"), format!("{}{functions}", ll_header(major)));
    let output = dir.join(format!("{name}.o"));
    let mut command = Command::new(llc);
    command
        .args(["-O0", "-global-isel=0", "-fast-isel=0", "-relocation-model=pic"])
        .args(["-mtriple=aarch64-unknown-linux-gnu", "-filetype=obj"])
        .arg(&input)
        .arg("-o")
        .arg(&output);
    let outcome = run_with(&mut command, &[], Duration::from_mins(5));
    assert!(outcome.success(), "llc {major} failed:\n{}", outcome.stderr_text());
    Some(output)
}

/// The objects of `functions`: hod plain and stressed, `llc` of each
/// LLVM present.
fn objects(dir: &Scratch, functions: &str) -> Vec<(String, PathBuf)> {
    let text = format!("{}{functions}", ll_header(22));
    let mut objects = vec![
        ("hod".to_owned(), hod_object(dir, "hod", &text, Stress::default())),
        ("stress".to_owned(), hod_object(dir, "stress", &text, Stress::ALL)),
    ];
    for major in LLVM_MAJORS {
        if let Some(object) = llc(dir, major, &format!("llc-{major}"), functions) {
            objects.push((format!("llc-{major}"), object));
        }
    }
    objects
}

/// §5.6.38: the program prints the same built by hod and by `llc`,
/// under every linker, dynamic and static.
#[test]
fn frames_walked_and_saved_as_llc() {
    let dir = Scratch::new("a64-frame-intrinsics");
    let Some(toolchain) = Toolchain::aarch64(&dir) else { return };
    let Some(driver) =
        toolchain.compile_cxx(&dir, "driver", DRIVER, &["-fPIC", "-fno-omit-frame-pointer"])
    else {
        return;
    };
    let stdcxx = Path::new("-lstdc++");
    let mut runs = 0;
    for (who, object) in objects(&dir, FUNCTIONS) {
        for linker in &toolchain.linkers {
            for (how, flags) in [("dynamic", &[][..]), ("static", &["-static"][..])] {
                let program = dir.join(format!("{who}-{how}-{}", linker.name));
                toolchain.link_ok(linker, flags, &[&driver, &object, stdcxx], &program);
                let mut command = toolchain.runner_command(&program);
                let ran = run_with(&mut command, &[], Duration::from_mins(5));
                assert_eq!(
                    (exit_code(&ran), ran.stdout_text()),
                    (Some(0), EXPECTED.to_owned()),
                    "{who} {how} {}: {}",
                    linker.name,
                    ran.stderr_text()
                );
                runs += 1;
            }
        }
    }
    eprintln!("AArch64 frame-intrinsic programs that printed the expected text: {runs}");
}

/// The registers `llvm-dwarfdump --eh-frame` shows saved (`REG=[CFA...`)
/// in the FDEs of `object`.
fn saved_registers(object: &Path) -> Option<BTreeSet<String>> {
    let dwarfdump = tool("llvm-dwarfdump")?;
    let out = run(&dwarfdump, ["--eh-frame".as_ref(), object.as_os_str()]);
    assert!(out.success(), "llvm-dwarfdump: {}", out.stderr_text());
    let mut saved = BTreeSet::new();
    for row in out.stdout_text().lines().filter(|l| l.contains("CFA=")) {
        for rule in row.split(", ").chain(row.split(": ")) {
            if let Some((reg, rest)) = rule.trim().split_once('=')
                && rest.starts_with("[CFA")
            {
                saved.insert(reg.trim().to_owned());
            }
        }
    }
    Some(saved)
}

const SAVING_ALONE: &str = r#"
declare void @llvm.eh.unwind.init()
declare i64 @leaf(i64)
define i64 @saving(i64 %x) #0 {
  call void @llvm.eh.unwind.init()
  %r = call i64 @leaf(i64 %x)
  ret i64 %r
}
attributes #0 = { noinline "frame-pointer"="all" }
"#;

/// §5.6.38: the rows of an `eh.unwind.init` frame name every register
/// `llc`'s do: `x19`..`x28`, `d8`..`d15`, the frame record.
#[test]
fn the_unwinder_finds_the_registers_llc_saves() {
    let dir = Scratch::new("a64-frame-intrinsics-rows");
    let mut compared = 0;
    let mut sets = Vec::new();
    for (who, object) in objects(&dir, SAVING_ALONE) {
        let Some(saved) = saved_registers(&object) else { return };
        assert_eq!(saved.len(), 20, "{who}: {saved:?}");
        sets.push((who, saved));
    }
    let (first, want) = &sets[0];
    for (who, saved) in &sets[1..] {
        assert_eq!(saved, want, "{who} against {first}");
        compared += 1;
    }
    eprintln!("register sets compared: {compared}");
}

/// §5.6.38 and `spec/ir.md` §8: what `llc` miscompiles here is refused by
/// its row.
#[test]
fn what_llc_miscompiles_is_refused_by_its_row() {
    let classify = |functions: &str| {
        let (module, bodies) = parse(&format!("{}{functions}", ll_header(22)));
        let module_pass = hod_ir::classify(&module).err();
        let body_pass = bodies.iter().find_map(|b| hod_ir::classify_body(&module, b).err());
        module_pass.or(body_pass).map(|r| r.construct.into_owned())
    };
    let cases = [(
        "declare void @llvm.eh.return.i64(i64, ptr)\ndefine void @f(i64 %o, ptr %h) {\n  call void @llvm.eh.return.i64(i64 %o, ptr %h)\n  unreachable\n}\n",
        "intrinsic:llvm.eh.return.i64",
    )];
    for (functions, want) in cases {
        assert_eq!(classify(functions).as_deref(), Some(want), "{functions}");
    }
    for ok in [FUNCTIONS, SAVING_ALONE, CFA_FUNCTIONS] {
        assert_eq!(classify(ok), None);
    }
}

/// The shapes of `llvm.eh.dwarf.cfa` (WP-294). Each takes the CFA, asks
/// the unwinder for its own (`check`: the value less `_Unwind_GetCFA` of
/// the caller of `check`, stored at `%u`) and returns the value.
const CFA_FUNCTIONS: &str = r#"
declare ptr @llvm.eh.dwarf.cfa(i32)
declare ptr @llvm.stacksave.p0()
declare void @llvm.va_start.p0(ptr)
declare void @llvm.va_end.p0(ptr)
declare void @sink(ptr)
declare i64 @check(ptr)
declare void @ten(i64, i64, i64, i64, i64, i64, i64, i64, i64, i64)

define ptr @cfa_plain(ptr %u) #0 {
  %c = call ptr @llvm.eh.dwarf.cfa(i32 0)
  %d = call i64 @check(ptr %c)
  store i64 %d, ptr %u
  ret ptr %c
}

define ptr @cfa_aligned(ptr %u) #0 {
  %a = alloca [64 x i8], align 64
  call void @sink(ptr %a)
  %c = call ptr @llvm.eh.dwarf.cfa(i32 0)
  %d = call i64 @check(ptr %c)
  store i64 %d, ptr %u
  ret ptr %c
}

define ptr @cfa_vla(ptr %u, i64 %n) #0 {
  %a = alloca i8, i64 %n, align 16
  call void @sink(ptr %a)
  %c = call ptr @llvm.eh.dwarf.cfa(i32 0)
  %d = call i64 @check(ptr %c)
  store i64 %d, ptr %u
  ret ptr %c
}

define ptr @cfa_va(ptr %u, ...) #0 {
  %ap = alloca [32 x i8], align 8
  call void @llvm.va_start.p0(ptr %ap)
  %c = call ptr @llvm.eh.dwarf.cfa(i32 0)
  %d = call i64 @check(ptr %c)
  store i64 %d, ptr %u
  call void @llvm.va_end.p0(ptr %ap)
  ret ptr %c
}

define ptr @cfa_outgoing(ptr %u) #0 {
  call void @ten(i64 1, i64 2, i64 3, i64 4, i64 5, i64 6, i64 7, i64 8, i64 9, i64 10)
  %c = call ptr @llvm.eh.dwarf.cfa(i32 0)
  %d = call i64 @check(ptr %c)
  store i64 %d, ptr %u
  ret ptr %c
}

define ptr @cfa_incoming(ptr %u, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6, i64 %a7, i64 %a8, i64 %a9) #0 {
  %c = call ptr @llvm.eh.dwarf.cfa(i32 0)
  %d = call i64 @check(ptr %c)
  store i64 %d, ptr %u
  ret ptr %c
}

define ptr @cfa_at(ptr %u, i32 %k) #0 {
  %c = call ptr @llvm.eh.dwarf.cfa(i32 %k)
  %d = call i64 @check(ptr %c)
  store i64 %d, ptr %u
  ret ptr %c
}

define ptr @cfa_minus8(ptr %u) #0 {
  %c = call ptr @llvm.eh.dwarf.cfa(i32 -8)
  %d = call i64 @check(ptr %c)
  store i64 %d, ptr %u
  ret ptr %c
}

define ptr @cfa_plus40(ptr %u) #0 {
  %c = call ptr @llvm.eh.dwarf.cfa(i32 40)
  %d = call i64 @check(ptr %c)
  store i64 %d, ptr %u
  ret ptr %c
}

define void @distance(ptr %out, i64 %i, ptr %sp, ptr %at) #0 {
  %s = ptrtoint ptr %sp to i64
  %c = ptrtoint ptr %at to i64
  %d = sub i64 %c, %s
  %o = getelementptr i64, ptr %out, i64 %i
  store i64 %d, ptr %o
  ret void
}

; Each value less the stack pointer before the call; the unwinder's
; differences at `%u`. No dynamic slot here: hod's calls with stack
; arguments then use the outgoing area, the stack pointer does not move.
define void @cfa_distances(ptr %out, ptr %u) #0 {
  %u1 = getelementptr i64, ptr %u, i64 1
  %u2 = getelementptr i64, ptr %u, i64 2
  %u3 = getelementptr i64, ptr %u, i64 3
  %u4 = getelementptr i64, ptr %u, i64 4
  %u5 = getelementptr i64, ptr %u, i64 5
  %u6 = getelementptr i64, ptr %u, i64 6
  %u7 = getelementptr i64, ptr %u, i64 7
  %u8 = getelementptr i64, ptr %u, i64 8
  %u9 = getelementptr i64, ptr %u, i64 9
  %s0 = call ptr @llvm.stacksave.p0()
  %c0 = call ptr @cfa_plain(ptr %u)
  call void @distance(ptr %out, i64 0, ptr %s0, ptr %c0)
  %s1 = call ptr @llvm.stacksave.p0()
  %c1 = call ptr @cfa_aligned(ptr %u1)
  call void @distance(ptr %out, i64 1, ptr %s1, ptr %c1)
  %s2 = call ptr @llvm.stacksave.p0()
  %c2 = call ptr @cfa_vla(ptr %u2, i64 100)
  call void @distance(ptr %out, i64 2, ptr %s2, ptr %c2)
  %s3 = call ptr @llvm.stacksave.p0()
  %c3 = call ptr (ptr, ...) @cfa_va(ptr %u3, i64 7, double 1.5, i64 1, i64 2, i64 3, i64 4, i64 5, i64 6, i64 7, i64 8)
  call void @distance(ptr %out, i64 3, ptr %s3, ptr %c3)
  %s4 = call ptr @llvm.stacksave.p0()
  %c4 = call ptr @cfa_outgoing(ptr %u4)
  call void @distance(ptr %out, i64 4, ptr %s4, ptr %c4)
  %s5 = call ptr @llvm.stacksave.p0()
  %c5 = call ptr @cfa_incoming(ptr %u5, i64 1, i64 2, i64 3, i64 4, i64 5, i64 6, i64 7, i64 8, i64 9)
  call void @distance(ptr %out, i64 5, ptr %s5, ptr %c5)
  %s6 = call ptr @llvm.stacksave.p0()
  %c6 = call ptr @cfa_at(ptr %u6, i32 24)
  call void @distance(ptr %out, i64 6, ptr %s6, ptr %c6)
  %s7 = call ptr @llvm.stacksave.p0()
  %c7 = call ptr @cfa_at(ptr %u7, i32 -16)
  call void @distance(ptr %out, i64 7, ptr %s7, ptr %c7)
  %s8 = call ptr @llvm.stacksave.p0()
  %c8 = call ptr @cfa_minus8(ptr %u8)
  call void @distance(ptr %out, i64 8, ptr %s8, ptr %c8)
  %s9 = call ptr @llvm.stacksave.p0()
  %c9 = call ptr @cfa_plus40(ptr %u9)
  call void @distance(ptr %out, i64 9, ptr %s9, ptr %c9)
  ret void
}

attributes #0 = { noinline "frame-pointer"="all" }
"#;

const CFA_DRIVER: &str = r#"
#include <stdio.h>
#include <stdint.h>
#include <unwind.h>
extern "C" {
void cfa_distances(long *, long *);
__attribute__((noinline)) void sink(void *p) { asm volatile("" :: "r"(p) : "memory"); }
__attribute__((noinline)) void ten(long, long, long, long, long, long, long, long, long, long) {
  asm volatile("");
}
struct Walk { int n; uintptr_t cfa; };
static _Unwind_Reason_Code step(struct _Unwind_Context *c, void *arg) {
  Walk *w = (Walk *)arg;
  // libgcc's context `n` has the return address into frame `n` (0 is
  // `check`) and the CFA of frame `n - 1`: the CFA of `check`'s caller
  // is in the context whose address is in that caller's caller.
  if (w->n++ == 2) {
    w->cfa = _Unwind_GetCFA(c);
    return _URC_END_OF_STACK;
  }
  return _URC_NO_REASON;
}
__attribute__((noinline)) long check(void *c) {
  Walk w = {0, 0};
  _Unwind_Backtrace(step, &w);
  return w.cfa == 0 ? 999999 : (long)((uintptr_t)c - w.cfa);
}
}
int main() {
  long d[10], u[10];
  cfa_distances(d, u);
  // The calls with stack arguments (3, 5) are left out of the first
  // line: hod's caller moves its stack pointer for them, `llc`'s does
  // not, so the stack pointer before the call is not the callee's CFA.
  printf("cfa:");
  for (int i = 0; i < 10; i++)
    if (i != 3 && i != 5) printf(" %ld", d[i]);
  printf("\nunwinder:");
  for (long v : u) printf(" %ld", v);
  printf("\n");
  return 0;
}
"#;

/// The offsets the shapes add: plain, aligned, VLA, variadic, outgoing,
/// incoming, `24`, `-16` (both variable), `-8`, `40`.
const CFA_OFFSETS: [i64; 10] = [0, 0, 0, 0, 0, 0, 24, -16, -8, 40];

/// What the program prints when every value is the CFA less `bias` plus
/// its offset.
fn cfa_expected(bias: i64) -> String {
    let line = |name: &str, skip: &[usize]| {
        let values: Vec<String> = (0..CFA_OFFSETS.len())
            .filter(|i| !skip.contains(i))
            .map(|i| (CFA_OFFSETS[i] - bias).to_string())
            .collect();
        format!("{name}: {}\n", values.join(" "))
    };
    format!("{}{}", line("cfa", &[3, 5]), line("unwinder", &[]))
}

/// WP-294: `llvm.eh.dwarf.cfa(k)` is the CFA the unwinder computes from
/// the frame's rows, plus `k`, in every frame shape; `llc` 19 and 22 give
/// 16 less (`x29 + k`) in every one, so they are compared with that
/// bias.
#[test]
fn dwarf_cfa_is_the_unwinders_cfa() {
    let dir = Scratch::new("a64-dwarf-cfa");
    let Some(toolchain) = Toolchain::aarch64(&dir) else { return };
    let Some(driver) =
        toolchain.compile_cxx(&dir, "driver", CFA_DRIVER, &["-fPIC", "-fno-omit-frame-pointer"])
    else {
        return;
    };
    let mut runs = 0;
    for (who, object) in objects(&dir, CFA_FUNCTIONS) {
        let want = cfa_expected(if who.starts_with("llc") { 16 } else { 0 });
        for linker in &toolchain.linkers {
            for (how, flags) in [("dynamic", &[][..]), ("static", &["-static"][..])] {
                let program = dir.join(format!("cfa-{who}-{how}-{}", linker.name));
                toolchain.link_ok(linker, flags, &[&driver, &object], &program);
                let mut command = toolchain.runner_command(&program);
                let ran = run_with(&mut command, &[], Duration::from_mins(5));
                assert_eq!(
                    (exit_code(&ran), ran.stdout_text()),
                    (Some(0), want.clone()),
                    "{who} {how} {}: {}",
                    linker.name,
                    ran.stderr_text()
                );
                runs += 1;
            }
        }
    }
    eprintln!("AArch64 dwarf.cfa programs that printed the expected text: {runs}");
}

/// §5.6.38: `eh.unwind.init` where the bytes after the frame record or
/// `x19` belong to the convention.
#[test]
fn unwind_init_without_room_is_refused() {
    assert_refused(&[
        (
            "declare void @llvm.eh.unwind.init()\ndefine preserve_mostcc void @f() {\n  call void @llvm.eh.unwind.init()\n  ret void\n}\n",
            "a64:unwind-init-convention",
        ),
        (
            "declare void @llvm.eh.unwind.init()\ndefine preserve_allcc void @f() {\n  call void @llvm.eh.unwind.init()\n  ret void\n}\n",
            "a64:unwind-init-convention",
        ),
    ]);
}
