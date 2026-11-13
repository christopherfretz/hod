//! Coroutine bodies on x86-64 (WP-137, design 0005 package C7,
//! `spec/codegen.md` §7.6 "x86-64"): `rbx` as the frame base, the entry
//! stubs, symmetric transfer, and the handle operations hod's own
//! drivers use.
//!
//! Every program is built from clang 19 and 22 pre-optimisation IR for
//! `x86_64-unknown-linux-gnu`, linked by GNU ld, lld and mold and run
//! under `qemu-x86_64`, plain and with poisoned frees (design 0005 §5.5);
//! the oracle is the same IR through `opt -passes='default<O0>'` and
//! `llc -O0 -fast-isel=0 -relocation-model=pic`.
//!
//! - Mixed objects: hod's coroutine resumed, destroyed and made to throw
//!   (the exception leaves the resumed body through the stub's caller)
//!   from `llc` code that keeps a value in `rbx`; hod's driver over
//!   `llc`'s coroutine; both hod. Every program prints what the all-`llc`
//!   build prints.
//! - `Stress::coro_frame`: ordinary functions with the coroutine frame
//!   (and `rbx` clobbered) called, and unwound through, from that code.
//! - Symmetric transfer in constant stack: a million awaits between
//!   coroutines of two objects (hod, `llc`, mixed) under a 1 MiB guest
//!   stack.
//! - Shapes: nothing addresses `[rbx…]` between a wrapper's call and its
//!   suspend, nor in the transfer; the stubs and the prologue's words.
//! - Refusals by name.

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hod_cg::{CgError, Options, RelocModel, Stress};
use hod_ir::{Body, Module};
use hod_testutil::{Scratch, run_with, tool, tools_required};
use linking::{Toolchain, exit_code};

fn pic() -> Options {
    Options { reloc_model: RelocModel::Pic, ..Options::default() }
}

fn coro_frame() -> Options {
    Options { stress: Stress { coro_frame: true, ..Stress::default() }, ..pic() }
}

/// The module header of handwritten IR.
fn header() -> String {
    format!(
        "target datalayout = \"{}\"\ntarget triple = \"x86_64-unknown-linux-gnu\"\n",
        hod_ir::X86_64_LINUX_DATA_LAYOUT
    )
}

/// `llc` code that keeps `0x1234` in `rbx` across a call of `f(arg)`: 1
/// when `rbx` is intact after a return, 3 when it is intact after an
/// exception was caught, 0 and 2 when it was not.
const RBX_CALLER: &str = r#"
declare i32 @__gxx_personality_v0(...)
declare ptr @__cxa_begin_catch(ptr)
declare void @__cxa_end_catch()

define i32 @rbx_call(ptr %f, ptr %arg) personality ptr @__gxx_personality_v0 {
entry:
  call void asm sideeffect "movq $$0x1234, %rbx", "~{rbx}"()
  invoke void %f(ptr %arg) to label %ok unwind label %pad
ok:
  %v = call i64 asm sideeffect "movq %rbx, $0", "=r"()
  %good = icmp eq i64 %v, 4660
  %r = zext i1 %good to i32
  ret i32 %r
pad:
  %lp = landingpad { ptr, i32 } catch ptr null
  %e = extractvalue { ptr, i32 } %lp, 0
  %x = call ptr @__cxa_begin_catch(ptr %e)
  call void @__cxa_end_catch()
  %w = call i64 asm sideeffect "movq %rbx, $0", "=r"()
  %good2 = icmp eq i64 %w, 4660
  %r2 = zext i1 %good2 to i32
  %r3 = add i32 %r2, 2
  ret i32 %r3
}
"#;

/// hod's functions for the stress test: one that returns, one that an
/// exception passes through, one that catches it.
const PLAIN: &str = r"
declare void @cxx_throw(i32)
declare void @cxx_touch(ptr)
declare i32 @__gxx_personality_v0(...)
declare ptr @__cxa_begin_catch(ptr)
declare void @__cxa_end_catch()

define void @hod_returns(ptr %p) {
  call void @cxx_touch(ptr %p)
  %v = load i32, ptr %p
  %w = add i32 %v, 1
  store i32 %w, ptr %p
  ret void
}
define void @hod_passes(ptr %p) {
  %v = load i32, ptr %p
  call void @cxx_throw(i32 %v)
  ret void
}
define void @hod_catches(ptr %p) personality ptr @__gxx_personality_v0 {
entry:
  invoke void @cxx_throw(i32 7) to label %ok unwind label %pad
ok:
  ret void
pad:
  %lp = landingpad { ptr, i32 } catch ptr null
  %e = extractvalue { ptr, i32 } %lp, 0
  %x = call ptr @__cxa_begin_catch(ptr %e)
  call void @__cxa_end_catch()
  store i32 99, ptr %p
  ret void
}
";

const PLAIN_DRIVER: &str = r#"
#include <cstdio>
extern "C" {
int rbx_call(void (*)(int*), int*);
void hod_returns(int*);
void hod_passes(int*);
void hod_catches(int*);
void cxx_touch(int* p) { *p += 10; }
void cxx_throw(int v) { throw v; }
}
int main() {
  int v = 1;
  int r = rbx_call(hod_returns, &v);
  std::printf("returns: %d %d\n", r, v);
  r = rbx_call(hod_passes, &v);
  std::printf("passes: %d\n", r);
  r = rbx_call(hod_catches, &v);
  std::printf("catches: %d %d\n", r, v);
}
"#;

/// The coroutine half of the mixed programs.
const COROUTINE: &str = r#"
#include <coroutine>
#include <cstdio>
#include <stdexcept>
static int dtors = 0;
struct D { int v; ~D() { ++dtors; } };
struct task {
  struct promise_type {
    int v = -1;
    task get_return_object() { return {std::coroutine_handle<promise_type>::from_promise(*this)}; }
    std::suspend_always initial_suspend() noexcept { return {}; }
    std::suspend_always final_suspend() noexcept { return {}; }
    std::suspend_always yield_value(int x) noexcept { v = x; return {}; }
    void return_void() {}
    void unhandled_exception() { throw; }
  };
  std::coroutine_handle<promise_type> h;
};
task counter(int n, bool boom) {
  D a{n};
  int sum = 0;
  for (int i = 0; i < n; i++) {
    sum += i;
    co_yield sum * 10;
  }
  D b{sum};
  if (boom) throw std::runtime_error("boom");
}
extern "C" void* co_make(int n, int boom) { return counter(n, boom != 0).h.address(); }
extern "C" int co_value(void* h) {
  return std::coroutine_handle<task::promise_type>::from_address(h).promise().v;
}
extern "C" int co_dtors() { return dtors; }
"#;

/// The driver half: the resume and destroy entries are called from
/// [`RBX_CALLER`]; `done` and `destroy` are handle operations here.
const CORO_DRIVER: &str = r#"
#include <coroutine>
#include <cstdio>
extern "C" {
int rbx_call(void (*)(void*), void*);
void* co_make(int, int);
int co_value(void*);
int co_dtors();
}
typedef void (*entry)(void*);
static entry resume_of(void* h) { return ((entry*)h)[0]; }
static entry destroy_of(void* h) { return ((entry*)h)[1]; }
int main() {
  void* h = co_make(3, 1);
  auto c = std::coroutine_handle<>::from_address(h);
  for (int i = 0; i < 3; i++) {
    int r = rbx_call(resume_of(h), h);
    std::printf("resume %d: rbx %d value %d done %d\n", i, r, co_value(h), (int)c.done());
  }
  int r = rbx_call(resume_of(h), h);
  std::printf("throw: rbx %d done %d dtors %d\n", r, (int)c.done(), co_dtors());
  c.destroy();
  std::printf("destroyed: dtors %d\n", co_dtors());
  h = co_make(5, 0);
  r = rbx_call(resume_of(h), h);
  r += 10 * rbx_call(resume_of(h), h);
  std::printf("two steps: rbx %d value %d\n", r, co_value(h));
  r = rbx_call(destroy_of(h), h);
  std::printf("destroy entry: rbx %d dtors %d\n", r, co_dtors());
  h = co_make(1, 0);
  rbx_call(resume_of(h), h);
  r = rbx_call(resume_of(h), h);
  c = std::coroutine_handle<>::from_address(h);
  std::printf("final: rbx %d done %d\n", r, (int)c.done());
  r = rbx_call(destroy_of(h), h);
  std::printf("final destroy: rbx %d dtors %d\n", r, co_dtors());
  c = std::coroutine_handle<>::from_address(co_make(2, 0));
  c.resume();
  c.resume();
  std::printf("handles: value %d done %d\n", co_value(c.address()), (int)c.done());
  c.destroy();
  std::printf("handles: dtors %d\n", co_dtors());
}
"#;

/// Symmetric transfer: what both coroutine objects see, a task whose
/// `await_suspend` returns the awaited coroutine's handle, and whose final
/// `await_suspend` returns the awaiting one's (or `noop_coroutine()`).
const TRANSFER_HEADER: &str = r#"
#include <coroutine>
extern "C" void probe();
template <bool Eager> struct task {
  struct promise_type {
    std::coroutine_handle<> cont;
    task get_return_object() { return task{std::coroutine_handle<promise_type>::from_promise(*this)}; }
    auto initial_suspend() noexcept {
      if constexpr (Eager) return std::suspend_never{};
      else return std::suspend_always{};
    }
    struct fin {
      bool await_ready() noexcept { return false; }
      std::coroutine_handle<> await_suspend(std::coroutine_handle<promise_type> h) noexcept {
        auto c = h.promise().cont;
        return c ? c : std::noop_coroutine();
      }
      void await_resume() noexcept {}
    };
    fin final_suspend() noexcept { return {}; }
    void return_void() {}
    void unhandled_exception() {}
  };
  std::coroutine_handle<promise_type> h;
  bool await_ready() { return false; }
  std::coroutine_handle<> await_suspend(std::coroutine_handle<> c) {
    h.promise().cont = c;
    return h;
  }
  void await_resume() {}
  ~task() { if (h) h.destroy(); }
};
task<false> step(long& s);
"#;

/// The awaited side.
const STEP: &str = r"
task<false> step(long& s) { ++s; probe(); co_return; }
";

/// The awaiting side: `lazy` transfers from resumed activations (tail
/// jumps), `eager` makes its first transfer from its ramp (a call).
const LOOP: &str = r#"
task<false> lazy(long& s, long n) { for (long i = 0; i < n; ++i) co_await step(s); }
task<true> eager(long& s, long n) { for (long i = 0; i < n; ++i) co_await step(s); }
template <class T> static void* release(T t) { void* a = t.h.address(); t.h = {}; return a; }
extern "C" void* make_lazy(long* s, long n) { return release(lazy(*s, n)); }
extern "C" void* make_eager(long* s, long n) { return release(eager(*s, n)); }
"#;

/// The transfer program's driver, compiled by the cross g++.
const TRANSFER_DRIVER: &str = r#"
#include <coroutine>
#include <cstdint>
#include <cstdio>
extern "C" {
int rbx_call(void (*)(void*), void*);
void* make_lazy(long*, long);
void* make_eager(long*, long);
}
__attribute__((noinline)) static std::uintptr_t here() {
  volatile char c = 0;
  return reinterpret_cast<std::uintptr_t>(&c);
}
static std::uintptr_t lo = UINTPTR_MAX, hi = 0;
extern "C" void probe() {
  std::uintptr_t p = here();
  if (p < lo) lo = p;
  if (p > hi) hi = p;
}
typedef void (*entry)(void*);
int main() {
  long s = 0;
  void* h = make_lazy(&s, 1000000);
  int r = rbx_call(((entry*)h)[0], h);
  auto c = std::coroutine_handle<>::from_address(h);
  std::printf("lazy %ld done %d flat %d rbx %d\n", s, (int)c.done(), (int)(hi == lo), r);
  c.destroy();
  s = 0, lo = UINTPTR_MAX, hi = 0;
  h = make_eager(&s, 1000000);
  c = std::coroutine_handle<>::from_address(h);
  std::printf("eager %ld done %d flat %d\n", s, (int)c.done(), (int)(hi == lo));
  c.destroy();
}
"#;

const TRANSFER_WANT: &str = "lazy 1000000 done 1 flat 1 rbx 1\neager 1000000 done 1 flat 1\n";

/// The guest stack of the transfer runs: a transfer made as a call nests
/// two machine frames per await, a million times.
const STACK_BYTES: &str = "1048576";

/// A tool of LLVM `major`: `/usr/lib/llvm-N/bin/<name>` or `<name>-N`.
fn versioned(name: &str, major: u32) -> Option<PathBuf> {
    let installed = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/{name}"));
    if installed.is_file() {
        return Some(installed);
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    let found = std::env::split_paths(&path)
        .map(|dir| dir.join(format!("{name}-{major}")))
        .find(|p| p.is_file());
    if found.is_none() {
        assert!(!tools_required(), "no {name} {major} (HOD_REQUIRE_TOOLS=1)");
        eprintln!("note: no {name} {major}; skipping its part");
    }
    found
}

fn run_ok(command: &mut Command, what: &str) {
    let ran = run_with(command, &[], Duration::from_mins(3));
    assert!(ran.success(), "{what}: {}{}", ran.stdout_text(), ran.stderr_text());
}

/// Pre-optimisation IR of C++ `source` by clang++ `major`, for x86-64.
fn pre_opt(dir: &Scratch, major: u32, source: &str, name: &str) -> Option<PathBuf> {
    let clang = versioned("clang++", major)?;
    let cpp = dir.write(&format!("{name}.cpp"), source);
    let ir = dir.join(format!("{name}.ll"));
    run_ok(
        Command::new(clang)
            .args(["-std=c++20", "-O0", "-S", "-emit-llvm", "-w", "-fPIC"])
            .args(["-Xclang", "-disable-llvm-passes", "--target=x86_64-unknown-linux-gnu"])
            .arg(&cpp)
            .arg("-o")
            .arg(&ir),
        name,
    );
    Some(ir)
}

/// `opt -passes='default<O0>'` (when `pre_opt`), then `llc -O0
/// -fast-isel=0`, PIC, for x86-64.
fn opt_llc(dir: &Scratch, major: u32, input: &Path, name: &str, pre_opt: bool) -> Option<PathBuf> {
    let llc = versioned("llc", major)?;
    let mut post = input.to_path_buf();
    if pre_opt {
        let opt = versioned("opt", major)?;
        post = dir.join(format!("{name}.post.ll"));
        run_ok(
            Command::new(opt).arg("-passes=default<O0>").arg("-S").arg(input).arg("-o").arg(&post),
            "opt",
        );
    }
    let object = dir.join(format!("{name}.o"));
    run_ok(
        Command::new(llc)
            .args(["-O0", "-fast-isel=0", "-filetype=obj", "-mtriple=x86_64-unknown-linux-gnu"])
            .arg("-relocation-model=pic")
            .arg(&post)
            .arg("-o")
            .arg(&object),
        "llc",
    );
    Some(object)
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

/// hod's object of `.ll` text.
fn hod_text(dir: &Scratch, text: &str, name: &str, options: &Options) -> PathBuf {
    let (module, bodies) = parse(text);
    let bytes = hod_x64::compile_to_object(&module, &bodies, options)
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    dir.write(&format!("{name}.o"), bytes)
}

/// hod's object of the `.ll` file `input`.
fn hod_file(dir: &Scratch, input: &Path, name: &str) -> PathBuf {
    hod_text(dir, &std::fs::read_to_string(input).unwrap(), name, &pic())
}

/// The environments every program runs in: plain, and with poisoned
/// frees (design 0005 §5.5).
const POISONS: [&[(&str, &str)]; 3] = [
    &[],
    &[("GLIBC_TUNABLES", "glibc.malloc.tcache_count=0"), ("MALLOC_PERTURB_", "165")],
    &[("MALLOC_CHECK_", "3")],
];

/// Links `inputs` under every linker and runs the program in every
/// environment of `envs` (with `extra` added); returns the outputs.
fn link_and_run_all(
    toolchain: &Toolchain,
    dir: &Scratch,
    name: &str,
    inputs: &[&Path],
    envs: &[&[(&str, &str)]],
    extra: &[(&str, &str)],
) -> Vec<(String, String)> {
    let mut outputs = Vec::new();
    for linker in &toolchain.linkers {
        let exe = dir.join(format!("{name}-{}", linker.name));
        let mut all: Vec<&Path> = inputs.to_vec();
        all.push(Path::new("-lstdc++"));
        toolchain.link_ok(linker, &[], &all, &exe);
        for (index, env) in envs.iter().enumerate() {
            let mut command = toolchain.runner_command(&exe);
            command.envs(env.iter().copied()).envs(extra.iter().copied());
            let ran = run_with(&mut command, &[], Duration::from_mins(5));
            let what = format!("{name} under {} (environment {index})", linker.name);
            assert!(
                exit_code(&ran) == Some(0),
                "{what}: exit {:?}\n{}{}",
                exit_code(&ran),
                ran.stdout_text(),
                ran.stderr_text()
            );
            outputs.push((what, ran.stdout_text()));
        }
    }
    outputs
}

/// `llc`'s object of the handwritten [`RBX_CALLER`].
fn rbx_caller(dir: &Scratch, major: u32) -> Option<PathBuf> {
    let caller = dir.write(&format!("rbx-{major}.ll"), format!("{}{RBX_CALLER}", header()));
    opt_llc(dir, major, &caller, &format!("rbx-{major}"), false)
}

/// `Stress::coro_frame` (§12): every function gets the coroutine machine
/// frame and has `rbx` clobbered after the prologue; `llc` code that keeps
/// a value in `rbx` calls them, has an exception pass through one, and
/// finds its `rbx` again.
#[test]
fn the_coroutine_frame_keeps_rbx_for_callers_and_unwinders() {
    let dir = Scratch::new("x64-coro-rbx-stress");
    let Some(toolchain) = Toolchain::x86_64(&dir) else { return };
    let Some(major) = [22, 19].into_iter().find(|&m| versioned("llc", m).is_some()) else {
        return;
    };
    let Some(caller) = rbx_caller(&dir, major) else { return };
    let Some(driver) = toolchain.compile_cxx(&dir, "driver", PLAIN_DRIVER, &["-fPIC"]) else {
        return;
    };
    let want = "returns: 1 12\npasses: 3\ncatches: 1 99\n";
    for (mode, options) in [("plain", pic()), ("coro-frame", coro_frame())] {
        let object = hod_text(&dir, &format!("{}{PLAIN}", header()), mode, &options);
        let inputs = [driver.as_path(), caller.as_path(), object.as_path()];
        for (what, out) in link_and_run_all(&toolchain, &dir, mode, &inputs, &POISONS[..1], &[]) {
            assert_eq!(out, want, "{what}");
        }
    }
    // The stress is real: `rbx` is clobbered after the prologue.
    let object = hod_text(&dir, &format!("{}{PLAIN}", header()), "shape", &coro_frame());
    if let Some(funcs) = disassemble(&object) {
        for (name, insts) in &funcs {
            assert!(insts.iter().any(|i| i == "mov rbx, rbp"), "{name}: {insts:#?}");
        }
    }
}

/// Mixed objects (design 0005 §5.4): hod's coroutine driven by `llc`'s
/// driver, and the reverse, and both hod's, with the stubs called from
/// `llc` code that keeps a value in `rbx`, an exception thrown out of a
/// resumed body to it, under every linker, plain and poisoned.
#[test]
fn coroutines_mixed_with_llc_objects_keep_rbx() {
    let dir = Scratch::new("x64-coro-mixed");
    let Some(toolchain) = Toolchain::x86_64(&dir) else { return };
    let mut runs = 0;
    for major in [19, 22] {
        let name = |what: &str| format!("{what}{major}");
        let Some(coro) = pre_opt(&dir, major, COROUTINE, &name("coro")) else { continue };
        let Some(driver) = pre_opt(&dir, major, CORO_DRIVER, &name("driver")) else { continue };
        let Some(caller) = rbx_caller(&dir, major) else { continue };
        let Some(coro_llc) = opt_llc(&dir, major, &coro, &name("coro-llc"), true) else {
            continue;
        };
        let Some(driver_llc) = opt_llc(&dir, major, &driver, &name("driver-llc"), true) else {
            continue;
        };
        let coro_hod = hod_file(&dir, &coro, &name("coro-hod"));
        let driver_hod = hod_file(&dir, &driver, &name("driver-hod"));
        let reference = link_and_run_all(
            &toolchain,
            &dir,
            &name("llc"),
            &[&driver_llc, &coro_llc, &caller],
            &POISONS[..1],
            &[],
        );
        let want = reference[0].1.clone();
        assert!(want.contains("resume 2: rbx 1 value 30 done 0\n"), "{want}");
        assert!(want.contains("throw: rbx 3 done 1"), "{want}");
        assert!(want.contains("handles: value 10 done 0\n"), "{want}");
        for (what, out) in &reference {
            assert_eq!(out, &want, "{what}");
        }
        for (what, inputs) in [
            ("hod-coro", [&driver_llc, &coro_hod, &caller]),
            ("hod-driver", [&driver_hod, &coro_llc, &caller]),
            ("hod-both", [&driver_hod, &coro_hod, &caller]),
        ] {
            let inputs = inputs.map(PathBuf::as_path);
            for (run, out) in
                link_and_run_all(&toolchain, &dir, &name(what), &inputs, &POISONS, &[])
            {
                assert_eq!(out, want, "{run}");
                runs += 1;
            }
        }
    }
    eprintln!("{runs} mixed runs");
}

/// Symmetric transfer (WP-107's obligation): a million awaits, each two
/// transfers, between coroutines of two objects, hod and `llc` in every
/// combination, under a 1 MiB guest stack, every linker, plain and
/// poisoned. Every activation of the awaited coroutine runs at the same
/// depth, and `rbx` of the `llc` code that entered the chain is intact.
#[test]
fn symmetric_transfer_between_objects_runs_in_constant_stack() {
    let dir = Scratch::new("x64-coro-transfer");
    let Some(toolchain) = Toolchain::x86_64(&dir) else { return };
    let Some(driver) =
        toolchain.compile_cxx(&dir, "driver", TRANSFER_DRIVER, &["-fPIC", "-O0", "-std=c++20"])
    else {
        return;
    };
    let mut runs = 0;
    for major in [19, 22] {
        let name = |what: &str| format!("{what}{major}");
        let step_source = format!("{TRANSFER_HEADER}{STEP}");
        let Some(step) = pre_opt(&dir, major, &step_source, &name("step")) else { continue };
        let loop_source = format!("{TRANSFER_HEADER}{LOOP}");
        let Some(lp) = pre_opt(&dir, major, &loop_source, &name("loop")) else { continue };
        let Some(caller) = rbx_caller(&dir, major) else { continue };
        let Some(step_llc) = opt_llc(&dir, major, &step, &name("step-llc"), true) else { continue };
        let Some(loop_llc) = opt_llc(&dir, major, &lp, &name("loop-llc"), true) else { continue };
        let step_hod = hod_file(&dir, &step, &name("step-hod"));
        let loop_hod = hod_file(&dir, &lp, &name("loop-hod"));
        for (what, sides) in [
            ("llc", [&loop_llc, &step_llc]),
            ("hod-loop", [&loop_hod, &step_llc]),
            ("hod-step", [&loop_llc, &step_hod]),
            ("hod-both", [&loop_hod, &step_hod]),
        ] {
            let inputs = [driver.as_path(), caller.as_path(), sides[0], sides[1]];
            let stack = [("QEMU_STACK_SIZE", STACK_BYTES)];
            let envs = &POISONS[..2];
            for (run, out) in link_and_run_all(&toolchain, &dir, &name(what), &inputs, envs, &stack)
            {
                assert_eq!(out, TRANSFER_WANT, "LLVM {major}: {run}");
                runs += 1;
            }
        }
    }
    eprintln!("{runs} runs");
}

/// `llvm-objdump -d` (Intel syntax) of an object: per symbol, its
/// instructions as `mnemonic operands`.
fn disassemble(object: &Path) -> Option<Vec<(String, Vec<String>)>> {
    let objdump = tool("llvm-objdump")?;
    let mut command = Command::new(objdump);
    command
        .args(["-d", "--no-show-raw-insn", "--no-leading-addr", "--x86-asm-syntax=intel"])
        .arg(object);
    let outcome = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "llvm-objdump: {}", outcome.stderr_text());
    let mut funcs: Vec<(String, Vec<String>)> = Vec::new();
    for line in outcome.stdout_text().lines() {
        if let Some(name) = line.strip_prefix('<').and_then(|l| l.strip_suffix(">:")) {
            funcs.push((name.to_owned(), Vec::new()));
        } else if line.starts_with(char::is_whitespace) && !line.trim().is_empty() {
            let text = line.split('#').next().unwrap_or(line);
            if let Some((_, insts)) = funcs.last_mut() {
                insts.push(text.split_whitespace().collect::<Vec<_>>().join(" "));
            }
        }
    }
    // The padding after a function is not its code.
    for (_, insts) in &mut funcs {
        while insts.last().is_some_and(|i| i.starts_with("nop")) {
            insts.pop();
        }
    }
    Some(funcs)
}

/// The suspended region (design 0005 §3.3) and the transfer's shape:
/// between the call of an `await_suspend` wrapper and its suspend (`mov
/// eax, 0xffffffff`) no instruction addresses `[rbx…]`; after a handle
/// wrapper, `mov r11, [rax]` · `mov rdi, rax` · `cmp [rbp + mode], 0` ·
/// `jne` · `call r11`; every `jmp r11` exit pops the save set and
/// addresses nothing through `rbx`. The stubs set `rbx`, the mode and the
/// frame word, then dispatch; the prologue zeroes both words.
#[test]
fn nothing_addresses_the_frame_after_a_wrapper() {
    let dir = Scratch::new("x64-coro-shape");
    let mut checked = 0;
    for major in [19, 22] {
        let sources =
            [("coro", COROUTINE.to_owned()), ("both", format!("{TRANSFER_HEADER}{STEP}{LOOP}"))];
        for (what, source) in sources {
            let name = format!("{what}{major}");
            let Some(ir) = pre_opt(&dir, major, &source, &name) else { continue };
            let object = hod_file(&dir, &ir, &name);
            let Some(funcs) = disassemble(&object) else { return };
            let (mut suspends, mut calls, mut jumps, mut stubs) = (0, 0, 0, 0);
            for (func, insts) in &funcs {
                let is_stub = func.ends_with(".resume") || func.ends_with(".destroy");
                if is_stub {
                    assert!(insts.starts_with(&["push rbp".to_owned()]), "{func}: {insts:#?}");
                    let at = insts.iter().position(|i| i == "mov rbx, rdi");
                    let at = at.unwrap_or_else(|| panic!("{func}: {insts:#?}"));
                    assert!(insts[at + 1].ends_with(", 0x1"), "{func}: {insts:#?}");
                    assert!(insts[at + 2].ends_with(", rdi"), "{func}: {insts:#?}");
                    let which = if func.ends_with(".resume") { "0x0" } else { "0x1" };
                    assert_eq!(insts[at + 3], format!("mov eax, {which}"), "{func}");
                    assert!(insts[at + 4].starts_with("cmp dword ptr [rbx"), "{func}");
                    // One `cmp` / `je` per suspend, then `ud2` (code without
                    // a symbol of its own may follow).
                    let end = insts.iter().position(|i| i == "ud2");
                    let end = end.unwrap_or_else(|| panic!("{func}: {insts:#?}"));
                    for (k, inst) in insts[at + 4..end].iter().enumerate() {
                        let want = if k % 2 == 0 { "cmp dword ptr [rbx" } else { "je " };
                        assert!(inst.starts_with(want), "{func}: {insts:#?}");
                    }
                    stubs += 1;
                    continue;
                }
                for (at, inst) in insts.iter().enumerate() {
                    if inst == "mov eax, 0xffffffff" {
                        let call = insts[..at].iter().rposition(|i| i.starts_with("call "));
                        let call = call.unwrap_or_else(|| panic!("{func}: no call before {at}"));
                        for between in &insts[call + 1..at] {
                            assert!(!between.contains("[rbx"), "{func}: {between} at {at}");
                        }
                        suspends += 1;
                    }
                    if inst == "mov r11, qword ptr [rax]" {
                        let transfer = &insts[at..at + 5];
                        assert_eq!(transfer[1], "mov rdi, rax", "{transfer:#?}");
                        assert!(transfer[2].starts_with("cmp qword ptr [rbp - "), "{transfer:#?}");
                        assert!(transfer[2].ends_with(", 0x0"), "{transfer:#?}");
                        assert!(transfer[3].starts_with("jne "), "{transfer:#?}");
                        assert_eq!(transfer[4], "call r11", "{transfer:#?}");
                        assert!(insts[at - 1].starts_with("call "), "{func} at {at}");
                        calls += 1;
                    }
                    if inst == "jmp r11" {
                        let start = insts[..at].iter().rposition(|i| i.starts_with("lea rsp"));
                        let exit = &insts[start.unwrap_or_else(|| panic!("{func}"))..at];
                        assert!(exit.iter().any(|i| i == "pop rbx"), "{exit:#?}");
                        assert!(exit.iter().all(|i| !i.contains("[rbx")), "{exit:#?}");
                        jumps += 1;
                    }
                }
                // A coroutine body: the prologue zeroes the mode and frame
                // words right after `sub rsp`.
                if insts.iter().any(|i| i == "mov eax, 0xffffffff") {
                    let sub = insts.iter().position(|i| i.starts_with("sub rsp, "));
                    let sub = sub.unwrap_or_else(|| panic!("{func}"));
                    for word in &insts[sub + 1..sub + 3] {
                        assert!(word.starts_with("mov qword ptr [rbp - "), "{func}: {word}");
                        assert!(word.ends_with(", 0x0"), "{func}: {word}");
                    }
                }
            }
            if what == "both" {
                // `step`'s final await, and `lazy`'s and `eager`'s two
                // awaits each.
                assert_eq!((calls, jumps), (5, 5), "LLVM {major}");
                assert_eq!(stubs, 6, "LLVM {major}");
            } else {
                assert!(suspends >= 3, "LLVM {major}: {suspends}");
                assert_eq!(stubs, 2, "LLVM {major}");
            }
            checked += 1;
        }
    }
    assert!(checked == 0 || checked == 4, "{checked}");
}

/// A coroutine of LLVM 22's dialect with a stack slot aligned to
/// `{align}` that no suspend crosses, and `{before}` just before its
/// suspend.
fn coroutine(align: u32, before: &str) -> String {
    format!(
        r"
declare token @llvm.coro.id(i32, ptr, ptr, ptr)
declare i64 @llvm.coro.size.i64()
declare ptr @llvm.coro.begin(token, ptr)
declare token @llvm.coro.save(ptr)
declare i8 @llvm.coro.suspend(token, i1)
declare ptr @llvm.coro.free(token, ptr)
declare void @llvm.coro.end(ptr, i1, token)
declare void @llvm.coro.await.suspend.handle(ptr, ptr, ptr)
declare ptr @malloc(i64)
declare void @free(ptr)
declare void @use(ptr)

define ptr @wrapper(ptr %a, ptr %h) {{
  ret ptr %h
}}

define ptr @co(i32 %n, ptr %q) presplitcoroutine {{
entry:
  %slot = alloca i32, align {align}
  %id = call token @llvm.coro.id(i32 16, ptr null, ptr null, ptr null)
  %size = call i64 @llvm.coro.size.i64()
  %mem = call ptr @malloc(i64 %size)
  %h = call ptr @llvm.coro.begin(token %id, ptr %mem)
  store i32 %n, ptr %slot
  %v = load i32, ptr %slot
  call void @use(ptr null)
  %save = call token @llvm.coro.save(ptr null)
{before}
  %s = call i8 @llvm.coro.suspend(token %save, i1 true)
  switch i8 %s, label %out [
    i8 0, label %cleanup
    i8 1, label %cleanup
  ]
cleanup:
  %f = call ptr @llvm.coro.free(token %id, ptr %h)
  call void @free(ptr %f)
  br label %out
out:
  call void @llvm.coro.end(ptr null, i1 false, token none)
  ret ptr %h
}}
"
    )
}

/// The construct name a module is refused by, or `None` when it compiles.
fn refused_as(text: &str) -> Option<&'static str> {
    let (module, bodies) = parse(text);
    match hod_x64::compile_to_object(&module, &bodies, &pic()) {
        Ok(_) => None,
        Err(hod_x64::ModuleError::Function {
            error: CgError::Unsupported { construct }, ..
        }) => Some(construct),
        Err(other) => panic!("not a refusal: {other}"),
    }
}

#[test]
fn what_is_not_compiled_is_refused_by_name() {
    let h = header();
    assert_eq!(refused_as(&format!("{h}{}", coroutine(4, ""))), None);
    let transfer = "  call void @llvm.coro.await.suspend.handle(ptr null, ptr %h, ptr @wrapper)";
    assert_eq!(refused_as(&format!("{h}{}", coroutine(4, transfer))), None);
    // WP-147: a 16-byte atomic compiles (`rbx` saved around it).
    let wide = "  %old = atomicrmw add ptr %q, i128 1 seq_cst, align 16";
    assert_eq!(refused_as(&format!("{h}{}", coroutine(4, wide))), None);
    let outside = "declare token @llvm.coro.id(i32, ptr, ptr, ptr)\n\
         define void @f() {\n  %t = call token @llvm.coro.id(i32 0, ptr null, ptr null, ptr null)\n  ret void\n}\n";
    let promise = "declare ptr @llvm.coro.promise(ptr, i32, i1)\n\
         define ptr @f(ptr %p, i32 %a) {\n  %r = call ptr @llvm.coro.promise(ptr %p, i32 %a, i1 false)\n  ret ptr %r\n}\n";
    for (text, name) in [
        (coroutine(32, ""), "coro:overaligned-stack-slot"),
        (outside.to_owned(), "coro:outside-coroutine"),
        (promise.to_owned(), "x64:coro-promise"),
    ] {
        assert_eq!(refused_as(&format!("{h}{text}")), Some(name), "{text}");
    }
    // Design 0005's post-sync: the plan refuses it, in both dialects.
    for text in [
        include_str!("../../hod_xform/tests/data/seeds/llvm19/post-sync-19.ll"),
        include_str!("../../hod_xform/tests/data/seeds/llvm22/post-sync-22.ll"),
    ] {
        assert_eq!(refused_as(text), Some("coro:frame-access-after-suspend"));
    }
}

/// A coroutine's pool has no `rbx`; every other function's has.
#[test]
fn a_coroutine_takes_rbx_out_of_its_pool() {
    use hod_cg::Target;
    let rbx = hod_cg::PReg::new(3);
    let (module, bodies) = parse(&format!("{}{}", header(), coroutine(4, "")));
    let mut coroutines = 0;
    for body in &bodies {
        let own = hod_x64::X64.for_function(&module, body).unwrap();
        let coro = module.symbol(body.symbol()).name() == b"co";
        assert_eq!(own.coro, coro);
        assert_eq!(own.reg_info().pool.contains(rbx), !coro);
        assert_eq!(own.reg_info().holdable.contains(rbx), !coro);
        coroutines += usize::from(coro);
    }
    assert_eq!(coroutines, 1);
}
