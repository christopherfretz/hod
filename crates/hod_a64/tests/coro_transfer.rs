//! Symmetric transfer (WP-107, design 0005 §3.3, `spec/codegen.md` §7.6
//! "Symmetric transfer").
//!
//! - Constant stack across objects: a million awaits, each two transfers,
//!   between a coroutine of one object and a coroutine of another, with
//!   hod on both sides, on either side with `opt` + `llc` on the other,
//!   and `opt` + `llc` on both (the reference), from clang 19 and 22
//!   pre-optimisation IR, under a 1 MiB stack limit (`ulimit -s`), every
//!   linker, plain and with poisoned frees. The chain is entered from
//!   `llc` code that keeps a value in x19, which must be intact when the
//!   chain returns to it. The program also measures the stack: every
//!   activation of the awaited coroutine runs at the same depth.
//! - The transfer's shape: after the wrapper of a handle await, nothing
//!   addresses the frame (`[x19…]`) up to the call or tail jump through
//!   x16, and every tail exit restores x19 from the machine stack.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use common::linking::Toolchain;
use common::{ll_header, objdump, parse, write_object};
use hod_a64::compile_module;
use hod_cg::{Options, RelocModel, Stress};
use hod_testutil::{Scratch, run_with, tools_required};

const PIC: Options = Options {
    reloc_model: RelocModel::Pic,
    code_model: None,
    function_sections: false,
    data_sections: false,
    stress: Stress { min_pool: false, far: false, displace: false, coro_frame: false },
    debug_compression: hod_obj::Compression::None,
    split_dwarf: hod_dwarf::SplitDwarf::Off,
};

/// `llc` code that keeps `0x1234` in x19 across a call of `f(arg)`: 1
/// when it is intact afterwards, 0 when not.
const X19_CALLER: &str = r#"
define i32 @x19_call(ptr %f, ptr %arg) {
  call void asm sideeffect "mov x19, #0x1234", "~{x19}"()
  call void %f(ptr %arg)
  %v = call i64 asm sideeffect "mov $0, x19", "=r"()
  %good = icmp eq i64 %v, 4660
  %r = zext i1 %good to i32
  ret i32 %r
}
"#;

/// What both coroutine objects see: a task whose `await_suspend` returns
/// the awaited coroutine's handle, and whose final `await_suspend`
/// returns the awaiting one's (or `noop_coroutine()`).
const HEADER: &str = r#"
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

/// The driver, always compiled by clang directly.
const DRIVER: &str = r#"
#include <coroutine>
#include <cstdint>
#include <cstdio>
extern "C" {
int x19_call(void (*)(void*), void*);
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
  int r = x19_call(((entry*)h)[0], h);
  auto c = std::coroutine_handle<>::from_address(h);
  std::printf("lazy %ld done %d flat %d x19 %d\n", s, (int)c.done(), (int)(hi == lo), r);
  c.destroy();
  s = 0, lo = UINTPTR_MAX, hi = 0;
  h = make_eager(&s, 1000000);
  c = std::coroutine_handle<>::from_address(h);
  std::printf("eager %ld done %d flat %d\n", s, (int)c.done(), (int)(hi == lo));
  c.destroy();
}
"#;

const WANT: &str = "lazy 1000000 done 1 flat 1 x19 1\neager 1000000 done 1 flat 1\n";

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

/// Pre-optimisation IR of C++ `source` by clang++ `major`.
fn pre_opt(dir: &Scratch, major: u32, source: &str, name: &str) -> Option<PathBuf> {
    let clang = versioned("clang++", major)?;
    let cpp = dir.write(&format!("{name}.cpp"), source);
    let ir = dir.join(format!("{name}.ll"));
    run_ok(
        Command::new(clang)
            .args(["-std=c++20", "-O0", "-S", "-emit-llvm", "-w", "-fPIC"])
            .args(["-Xclang", "-disable-llvm-passes", "--target=aarch64-unknown-linux-gnu"])
            .arg(&cpp)
            .arg("-o")
            .arg(&ir),
        name,
    );
    Some(ir)
}

/// `opt -passes='default<O0>'` (when `pre_opt`), then `llc -O0
/// -global-isel=0 -fast-isel=0`, PIC.
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
            .args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj"])
            .arg("-relocation-model=pic")
            .arg(&post)
            .arg("-o")
            .arg(&object),
        "llc",
    );
    Some(object)
}

/// hod's object of the `.ll` file `input`.
fn hod_file(dir: &Scratch, input: &Path, name: &str) -> PathBuf {
    let text = std::fs::read_to_string(input).unwrap();
    let (module, bodies) = parse(&text);
    let emitted = compile_module(&module, &bodies, &PIC).unwrap_or_else(|e| panic!("{name}: {e}"));
    write_object(dir, &format!("{name}.o"), &emitted)
}

/// Plain, and with poisoned frees (design 0005 §5.5).
const POISONS: [&[(&str, &str)]; 2] =
    [&[], &[("GLIBC_TUNABLES", "glibc.malloc.tcache_count=0"), ("MALLOC_PERTURB_", "165")]];

/// The stack limit of every run, in KiB: a transfer made as a call nests
/// two machine frames per await, a million times.
const STACK_KIB: &str = "1024";

/// Links `inputs` under every linker and runs the program in every
/// environment of [`POISONS`] under the stack limit; returns the outputs.
fn link_and_run_all(
    toolchain: &Toolchain,
    dir: &Scratch,
    name: &str,
    inputs: &[&Path],
) -> Vec<(String, String)> {
    let mut outputs = Vec::new();
    for linker in &toolchain.linkers {
        let exe = dir.join(format!("{name}-{}", linker.name));
        let mut all: Vec<&Path> = inputs.to_vec();
        all.push(Path::new("-lstdc++"));
        toolchain.link_ok(linker, &[], &all, &exe);
        for (index, env) in POISONS.iter().enumerate() {
            let mut command = Command::new("sh");
            command.args(["-c", "ulimit -s \"$1\" && exec \"$0\""]).arg(&exe).arg(STACK_KIB);
            command.envs(env.iter().copied());
            let ran = run_with(&mut command, &[], Duration::from_mins(1));
            let what = format!("{name} under {} (environment {index})", linker.name);
            assert!(ran.success(), "{what}: {}{}", ran.stdout_text(), ran.stderr_text());
            outputs.push((what, ran.stdout_text()));
        }
    }
    outputs
}

#[test]
fn symmetric_transfer_between_objects_runs_in_constant_stack() {
    if !cfg!(target_arch = "aarch64") {
        return;
    }
    let dir = Scratch::new("coro-transfer-mixed");
    let Some(toolchain) = Toolchain::host(&dir) else { return };
    let mut runs = 0;
    for major in [19, 22] {
        let Some(clang) = versioned("clang++", major) else { continue };
        let name = |what: &str| format!("{what}{major}");
        let Some(step) = pre_opt(&dir, major, &format!("{HEADER}{STEP}"), &name("step")) else {
            continue;
        };
        let Some(lp) = pre_opt(&dir, major, &format!("{HEADER}{LOOP}"), &name("loop")) else {
            continue;
        };
        let caller =
            dir.write(&format!("x19-{major}.ll"), format!("{}{X19_CALLER}", ll_header(major)));
        let Some(caller) = opt_llc(&dir, major, &caller, &name("x19-"), false) else { continue };
        let cpp = dir.write(&format!("driver{major}.cpp"), DRIVER);
        let driver = dir.join(format!("driver{major}.o"));
        run_ok(
            Command::new(clang)
                .args(["-std=c++20", "-c", "-fPIC", "-O0", "-w"])
                .arg(&cpp)
                .arg("-o")
                .arg(&driver),
            "driver",
        );
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
            for (run, out) in link_and_run_all(&toolchain, &dir, &name(what), &inputs) {
                assert_eq!(out, WANT, "LLVM {major}: {run}");
                runs += 1;
            }
        }
    }
    eprintln!("{runs} runs");
}

/// After the call of a handle await's wrapper, no instruction addresses
/// the frame up to the transfer's `blr x16`, and the tail exits that end
/// in `br x16` restore x19 from the machine stack and address nothing
/// through it.
#[test]
fn the_transfer_addresses_no_frame() {
    if !cfg!(target_arch = "aarch64") {
        return;
    }
    let dir = Scratch::new("coro-transfer-shape");
    let mut checked = 0;
    for major in [19, 22] {
        let source = format!("{HEADER}{STEP}{LOOP}");
        let Some(ir) = pre_opt(&dir, major, &source, &format!("both{major}")) else { continue };
        let object = hod_file(&dir, &ir, &format!("both{major}"));
        let Some(lines) = objdump(&object) else { return };
        let mut calls = 0;
        let mut jumps = 0;
        for (at, line) in lines.iter().enumerate() {
            // ldr x16, [x0] · ldr w17, [x29, #mode] · cbnz w17 · blr x16,
            // right after the wrapper's call (other `blr x16` are the
            // handle operations' indirect calls).
            if line == "ldr x16, [x0]" {
                let transfer = &lines[at..at + 4];
                assert!(transfer[1].starts_with("ldr w17, [x29"), "{transfer:#?}");
                assert!(transfer[2].starts_with("cbnz w17"), "{transfer:#?}");
                assert_eq!(transfer[3], "blr x16", "{transfer:#?}");
                assert!(lines[at - 1].starts_with("blr "), "LLVM {major} at {at}: {lines:#?}");
                calls += 1;
            }
            if line == "br x16" {
                let start = lines[..at].iter().rposition(|l| l == "ret" || l.starts_with("br "));
                let exit = &lines[start.map_or(0, |s| s + 1)..at];
                assert!(exit.iter().any(|l| l.starts_with("ldr x19, [x29")), "{exit:#?}");
                assert!(exit.iter().all(|l| !l.contains("[x19")), "{exit:#?}");
                jumps += 1;
            }
        }
        // `step`'s final await, and `lazy`'s and `eager`'s two awaits each.
        assert_eq!((calls, jumps), (5, 5), "LLVM {major}: {lines:#?}");
        checked += 1;
    }
    assert!(checked == 0 || checked == 2);
}
