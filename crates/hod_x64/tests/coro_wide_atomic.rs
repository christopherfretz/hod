//! 16-byte atomics in a coroutine on x86-64 (WP-147, `spec/codegen.md`
//! §5.6.16 (6)). `lock cmpxchg16b` takes the new value in `rcx:rbx`, and
//! `rbx` is a coroutine's frame base: hod pushes it before the sequence
//! writes it and pops it after the last `cmpxchg16b`, so the operands
//! and results that live in the heap frame (`[rbx + d]`) are read and
//! written with the base in place. Without `+cx16` the operations call
//! libatomic and need nothing.
//!
//! A handwritten coroutine (LLVM 22's dialect) runs every 16-byte form
//! (`store`, `atomicrmw` with and without a used result, `cmpxchg` that
//! succeeds and fails, `load`) on operands and results that cross its
//! suspends; a plain function does the same under `Stress::coro_frame`.
//! Both are entered from `llc` code that keeps a value in `rbx`. The
//! driver checks every result against C's arithmetic; hod plain, hod
//! stressed and `opt -passes='default<O0>'` + `llc` 22 print the same,
//! linked by every linker, on a CPU model with `cmpxchg16b` (and on the
//! baseline model with libatomic for the `-cx16` module).

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hod_cg::{Options, RelocModel, Stress};
use hod_ir::{Body, Module};
use hod_testutil::target::Target;
use hod_testutil::{Scratch, run_with};
use linking::{Toolchain, exit_code};

fn options(stress: Stress) -> Options {
    Options { reloc_model: RelocModel::Pic, stress, ..Options::default() }
}

fn header() -> String {
    format!(
        "target datalayout = \"{}\"\ntarget triple = \"x86_64-unknown-linux-gnu\"\n",
        hod_ir::X86_64_LINUX_DATA_LAYOUT
    )
}

/// The operations, on the cell `%q`, from `%x` and `%y`; the results
/// stored to `%out` (`a`, `b`, `c`, the `cmpxchg`'s old value, the final
/// load at 0, 16, .., 64; its flag at 80). `{suspend}` marks where the
/// coroutine suspends between the operations and the stores.
const OPS: &str = "
  store atomic i128 %x, ptr %q seq_cst, align 16
  %a = atomicrmw add ptr %q, i128 %y seq_cst, align 16
  %b = atomicrmw xchg ptr %q, i128 %x seq_cst, align 16
  %c = atomicrmw umax ptr %q, i128 %y seq_cst, align 16
  %p = cmpxchg ptr %q, i128 %y, i128 %a seq_cst seq_cst, align 16
  %unused = atomicrmw or ptr %q, i128 %x seq_cst, align 16
{suspend}
  %d = load atomic i128, ptr %q seq_cst, align 16
  %p0 = extractvalue { i128, i1 } %p, 0
  %p1 = extractvalue { i128, i1 } %p, 1
  store i128 %a, ptr %out, align 16
  %o16 = getelementptr i8, ptr %out, i64 16
  store i128 %b, ptr %o16, align 16
  %o32 = getelementptr i8, ptr %out, i64 32
  store i128 %c, ptr %o32, align 16
  %o48 = getelementptr i8, ptr %out, i64 48
  store i128 %p0, ptr %o48, align 16
  %o64 = getelementptr i8, ptr %out, i64 64
  store i128 %d, ptr %o64, align 16
  %o80 = getelementptr i8, ptr %out, i64 80
  %f8 = zext i1 %p1 to i8
  store i8 %f8, ptr %o80
";

/// `%x` from `%lo`, `%hi`; `%y` is `%x` with bit 126 flipped.
const OPERANDS: &str = "
  %l = zext i64 %lo to i128
  %u = zext i64 %hi to i128
  %us = shl i128 %u, 64
  %x = or i128 %l, %us
  %y = xor i128 %x, 85070591730234615865843651857942052864
";

/// The module: the coroutine `@co(q, out, lo, hi)`, which suspends after
/// its operands, after the operations and finally; `@plain(args)`, the
/// same from `{q, out, lo, hi}` without suspends. `features` is the
/// `target-features` of both.
fn module(features: &str) -> String {
    let coro_ops = OPS.replace(
        "{suspend}",
        "  %save2 = call token @llvm.coro.save(ptr null)
  %s2 = call i8 @llvm.coro.suspend(token %save2, i1 false)
  switch i8 %s2, label %out.end [
    i8 0, label %r2
    i8 1, label %cleanup
  ]
r2:",
    );
    let plain_ops = OPS.replace("{suspend}", "");
    format!(
        r#"{header}
declare token @llvm.coro.id(i32, ptr, ptr, ptr)
declare i64 @llvm.coro.size.i64()
declare ptr @llvm.coro.begin(token, ptr)
declare token @llvm.coro.save(ptr)
declare i8 @llvm.coro.suspend(token, i1)
declare ptr @llvm.coro.free(token, ptr)
declare void @llvm.coro.end(ptr, i1, token)
declare ptr @malloc(i64)
declare void @free(ptr)

define ptr @co(ptr %qa, ptr %outa, i64 %lo, i64 %hi) presplitcoroutine #0 {{
entry:
  %q = getelementptr i8, ptr %qa, i64 0
  %out = getelementptr i8, ptr %outa, i64 0
  %id = call token @llvm.coro.id(i32 16, ptr null, ptr null, ptr null)
  %size = call i64 @llvm.coro.size.i64()
  %mem = call ptr @malloc(i64 %size)
  %h = call ptr @llvm.coro.begin(token %id, ptr %mem)
{OPERANDS}
  %save1 = call token @llvm.coro.save(ptr null)
  %s1 = call i8 @llvm.coro.suspend(token %save1, i1 false)
  switch i8 %s1, label %out.end [
    i8 0, label %r1
    i8 1, label %cleanup
  ]
r1:
{coro_ops}
  %save3 = call token @llvm.coro.save(ptr null)
  %s3 = call i8 @llvm.coro.suspend(token %save3, i1 true)
  switch i8 %s3, label %out.end [
    i8 0, label %never
    i8 1, label %cleanup
  ]
never:
  unreachable
cleanup:
  %f = call ptr @llvm.coro.free(token %id, ptr %h)
  call void @free(ptr %f)
  br label %out.end
out.end:
  call void @llvm.coro.end(ptr null, i1 false, token none)
  ret ptr %h
}}

define void @plain(ptr %args) #0 {{
entry:
  %q = load ptr, ptr %args
  %pout = getelementptr i8, ptr %args, i64 8
  %out = load ptr, ptr %pout
  %plo = getelementptr i8, ptr %args, i64 16
  %lo = load i64, ptr %plo
  %phi = getelementptr i8, ptr %args, i64 24
  %hi = load i64, ptr %phi
{OPERANDS}
{plain_ops}
  ret void
}}

attributes #0 = {{ "target-features"="{features}" }}
"#,
        header = header()
    )
}

/// `llc` code that keeps `0x1234` in `rbx` across a call of `f(arg)`:
/// 1 when `rbx` is intact afterwards.
const RBX_CALLER: &str = r#"
define i32 @rbx_call(ptr %f, ptr %arg) {
  call void asm sideeffect "movq $$0x1234, %rbx", "~{rbx}"()
  call void %f(ptr %arg)
  %v = call i64 asm sideeffect "movq %rbx, $0", "=r"()
  %good = icmp eq i64 %v, 4660
  %r = zext i1 %good to i32
  ret i32 %r
}
"#;

/// The driver: each operand pair through the coroutine (resumed twice,
/// destroyed at its final suspend, every entry from [`RBX_CALLER`]) and
/// through `plain`, the results checked against C.
const DRIVER: &str = r#"
#include <stdio.h>
#include <string.h>
#include <stdint.h>
typedef unsigned __int128 u128;
typedef void (*entry)(void*);
int rbx_call(void (*)(void*), void*);
void* co(u128*, unsigned char*, uint64_t, uint64_t);
void plain(void*);
static _Alignas(16) u128 cell;
static _Alignas(16) unsigned char out[96];
struct args { u128* q; unsigned char* out; uint64_t lo, hi; };
static u128 at(int i) { u128 v; memcpy(&v, out + 16 * i, 16); return v; }
static int check(u128 x) {
  u128 y = x ^ ((u128)1 << 126);
  u128 m = x > y ? x : y;
  int ok = m == y;
  u128 last = (ok ? x : m) | x;
  return at(0) == x && at(1) == x + y && at(2) == x && at(3) == m
      && at(4) == last && out[80] == ok && cell == last;
}
int main(void) {
  static const uint64_t cases[][2] = {
    {1, 2}, {~0ull, ~0ull >> 2}, {~0ull, ~0ull}, {0x0123456789abcdefull, 0x4000000000000000ull},
    {0, 0}, {0xfedcba9876543210ull, 0x3fffffffffffffffull},
  };
  for (int i = 0; i < 6; i++) {
    uint64_t lo = cases[i][0], hi = cases[i][1];
    u128 x = ((u128)hi << 64) | lo;
    memset(out, 0xa5, sizeof out);
    cell = 0;
    void* h = co(&cell, out, lo, hi);
    int r = rbx_call(((entry*)h)[0], h);
    r += 2 * rbx_call(((entry*)h)[0], h);
    r += 4 * rbx_call(((entry*)h)[1], h);
    printf("co %d: %s rbx %d\n", i, check(x) ? "ok" : "WRONG", r);
    memset(out, 0x5a, sizeof out);
    cell = 0;
    struct args a = {&cell, out, lo, hi};
    r = rbx_call(plain, &a);
    printf("plain %d: %s rbx %d\n", i, check(x) ? "ok" : "WRONG", r);
  }
  return 0;
}
"#;

/// What [`DRIVER`] prints when every result is right and `rbx` intact.
fn want() -> String {
    (0..6).map(|i| format!("co {i}: ok rbx 7\nplain {i}: ok rbx 1\n")).collect()
}

/// A module and its bodies from `.ll` text.
fn parse(text: &str) -> (Module, Vec<Body>) {
    let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
        .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    let side = parsed.text;
    let mut module = parsed.module;
    let mut bodies = Vec::new();
    hod_ll::parse_bodies(&mut module, &side, |_, parsed| {
        let body =
            parsed.body.unwrap_or_else(|| panic!("hod_ll: unsupported {:?}", parsed.unsupported));
        bodies.push(body.clone());
        Ok::<(), hod_ll::LlError>(())
    })
    .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    (module, bodies)
}

fn hod_object(dir: &Scratch, text: &str, name: &str, stress: Stress) -> PathBuf {
    let (module, bodies) = parse(text);
    let bytes = hod_x64::compile_to_object(&module, &bodies, &options(stress))
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    dir.write(&format!("{name}.o"), bytes)
}

fn run_ok(command: &mut Command, what: &str) {
    let ran = run_with(command, &[], Duration::from_mins(3));
    assert!(ran.success(), "{what}: {}{}", ran.stdout_text(), ran.stderr_text());
}

/// `opt -passes='default<O0>'` (when `split`) and `llc -O0 -fast-isel=0`
/// of LLVM 22; `None` without them.
fn llc22(dir: &Scratch, text: &str, name: &str, split: bool) -> Option<PathBuf> {
    let llc = PathBuf::from("/usr/lib/llvm-22/bin/llc");
    let opt = PathBuf::from("/usr/lib/llvm-22/bin/opt");
    if !llc.is_file() || !opt.is_file() {
        assert!(!hod_testutil::tools_required(), "no LLVM 22 (HOD_REQUIRE_TOOLS=1)");
        return None;
    }
    let mut input = dir.write(&format!("{name}.ll"), text);
    if split {
        let post = dir.join(format!("{name}.post.ll"));
        run_ok(
            Command::new(opt).arg("-passes=default<O0>").arg("-S").arg(&input).arg("-o").arg(&post),
            "opt",
        );
        input = post;
    }
    let object = dir.join(format!("{name}.o"));
    run_ok(
        Command::new(llc)
            .args(["-O0", "-fast-isel=0", "-filetype=obj", "-mtriple=x86_64-unknown-linux-gnu"])
            .arg("-relocation-model=pic")
            .arg(&input)
            .arg("-o")
            .arg(&object),
        "llc",
    );
    Some(object)
}

/// The command running `program`: the emulator with the baseline CPU
/// model, plus `cx16` when `cx16`; the program itself on an x86-64 host.
fn runner(program: &Path, cx16: bool) -> Option<Command> {
    let target = Target::X86_64;
    if target.is_host() {
        return Some(Command::new(program));
    }
    let sysroot = target.tool("sysroot")?;
    let mut command = Command::new(target.tool("emulator")?);
    let features: &[&str] = if cx16 { &["cx16"] } else { &[] };
    command.args(target.emulator_args_with(&sysroot, features)).arg(program);
    Some(command)
}

/// Whether the x86-64 cross toolchain has libatomic.
fn has_libatomic() -> bool {
    let mut command = Command::new(Target::X86_64.c_driver());
    command.arg("-print-file-name=libatomic.so");
    let ran = run_with(&mut command, &[], Duration::from_mins(1));
    ran.success() && ran.stdout_text().trim().starts_with('/')
}

/// Builds the legs of `features`' module and runs each under every
/// linker; returns the number of runs.
fn run_all(dir: &Scratch, features: &str, cx16: bool) -> usize {
    let text = module(features);
    let tag = if cx16 { "cx16" } else { "nocx16" };
    let mut legs = vec![
        ("hod".to_string(), hod_object(dir, &text, &format!("hod-{tag}"), Stress::default())),
        ("hod-stress".to_string(), hod_object(dir, &text, &format!("stress-{tag}"), Stress::ALL)),
    ];
    if let Some(o) = llc22(dir, &text, &format!("llc22-{tag}"), true) {
        legs.push(("llc22".to_string(), o));
    }
    let Some(caller) = llc22(dir, &format!("{}{RBX_CALLER}", header()), "rbx-caller", false) else {
        return 0;
    };
    if !cx16 && !has_libatomic() {
        eprintln!("note: the x86-64 cross toolchain has no libatomic: {tag} not linked or run");
        return 0;
    }
    let Some(toolchain) = Toolchain::x86_64(dir) else { return 0 };
    let driver = toolchain.compile_c(dir, &format!("driver-{tag}"), DRIVER, &["-fPIE"]);
    let mut runs = 0;
    for (leg, object) in &legs {
        for linker in &toolchain.linkers {
            let exe = dir.join(format!("{tag}-{leg}-{}", linker.name));
            let mut inputs: Vec<&Path> = vec![&driver, object, &caller];
            if !cx16 {
                inputs.push(Path::new("-latomic"));
            }
            toolchain.link_ok(linker, &[], &inputs, &exe);
            let Some(mut command) = runner(&exe, cx16) else { return runs };
            let ran = run_with(&mut command, &[], Duration::from_mins(2));
            let what = format!("{tag} {leg} [{}]", linker.name);
            assert_eq!(exit_code(&ran), Some(0), "{what}: {}", ran.stderr_text());
            assert_eq!(ran.stdout_text(), want(), "{what}");
            runs += 1;
        }
    }
    runs
}

/// With `+cx16`: `lock cmpxchg16b` with `rbx` saved around it.
#[test]
fn wide_atomics_in_a_coroutine_keep_the_frame_base() {
    let dir = Scratch::new("x64-coro-wide-cx16");
    let runs = run_all(&dir, "+cx16", true);
    eprintln!("coroutine 16-byte atomics with cx16: {runs} runs");
}

/// Without `+cx16`: libatomic's `__atomic_*_16`, which a coroutine needs
/// nothing for (`rbx` is callee-saved across the calls).
#[test]
fn wide_atomics_in_a_coroutine_without_cx16_call_libatomic() {
    let dir = Scratch::new("x64-coro-wide-libatomic");
    let runs = run_all(&dir, "-cx16", false);
    eprintln!("coroutine 16-byte atomics through libatomic: {runs} runs");
}
