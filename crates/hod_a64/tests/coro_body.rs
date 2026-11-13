//! Coroutine bodies (WP-106, design 0005 approach B, `spec/codegen.md`
//! §7.6 "Coroutine bodies").
//!
//! - The machine frame: `Stress::coro_frame` gives ordinary functions the
//!   coroutine frame (x19 saved, the mode and frame words) and clobbers
//!   x19; `llc` code that keeps a value in x19 calls them, and unwinds
//!   through them, and finds its x19 again.
//! - Mixed objects: a C++ coroutine compiled by hod is resumed, destroyed
//!   and made to throw from `llc` code that keeps a value in x19, through
//!   its resume and destroy entries; and the reverse, hod's driver over
//!   `llc`'s coroutine. Clang 19 and 22 pre-optimisation IR, every linker,
//!   plain and with poisoned frees; every program prints what the all-`llc`
//!   build prints.
//! - The suspended region: no `[x19…]` operand between an
//!   `await_suspend` wrapper's call and its suspend.
//! - Refusals by name: `coro:outside-coroutine`, `a64:coro-preserve-cc`,
//!   `coro:overaligned-stack-slot`, symmetric transfer
//!   (`intrinsic:llvm.coro.await.suspend.handle`, WP-107), and the plan's
//!   `coro:frame-access-after-suspend` on design 0005's `post-sync` seed.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use common::linking::Toolchain;
use common::{ll_header, objdump, parse, write_object};
use hod_a64::compile_module;
use hod_cg::{CgError, Options, RelocModel, Stress};
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
const CORO_FRAME: Options = Options {
    stress: Stress { min_pool: false, far: false, displace: false, coro_frame: true },
    ..PIC
};

/// `llc` code that keeps `0x1234` in x19 across a call of `f(arg)`:
/// 1 when x19 is intact after a return, 3 when it is intact after an
/// exception was caught, 0 and 2 when it was not.
const X19_CALLER: &str = r#"
declare i32 @__gxx_personality_v0(...)
declare ptr @__cxa_begin_catch(ptr)
declare void @__cxa_end_catch()

define i32 @x19_call(ptr %f, ptr %arg) personality ptr @__gxx_personality_v0 {
entry:
  call void asm sideeffect "mov x19, #0x1234", "~{x19}"()
  invoke void %f(ptr %arg) to label %ok unwind label %pad
ok:
  %v = call i64 asm sideeffect "mov $0, x19", "=r"()
  %good = icmp eq i64 %v, 4660
  %r = zext i1 %good to i32
  ret i32 %r
pad:
  %lp = landingpad { ptr, i32 } catch ptr null
  %e = extractvalue { ptr, i32 } %lp, 0
  %x = call ptr @__cxa_begin_catch(ptr %e)
  call void @__cxa_end_catch()
  %w = call i64 asm sideeffect "mov $0, x19", "=r"()
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
int x19_call(void (*)(int*), int*);
void hod_returns(int*);
void hod_passes(int*);
void hod_catches(int*);
void cxx_touch(int* p) { *p += 10; }
void cxx_throw(int v) { throw v; }
}
int main() {
  int v = 1;
  std::printf("returns: %d %d\n", x19_call(hod_returns, &v), v);
  std::printf("passes: %d\n", x19_call(hod_passes, &v));
  std::printf("catches: %d %d\n", x19_call(hod_catches, &v), v);
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
/// [`X19_CALLER`]; `done` and `destroy` are handle operations here.
const CORO_DRIVER: &str = r#"
#include <coroutine>
#include <cstdio>
extern "C" {
int x19_call(void (*)(void*), void*);
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
    int r = x19_call(resume_of(h), h);
    std::printf("resume %d: x19 %d value %d done %d\n", i, r, co_value(h), (int)c.done());
  }
  int r = x19_call(resume_of(h), h);
  std::printf("throw: x19 %d done %d dtors %d\n", r, (int)c.done(), co_dtors());
  c.destroy();
  std::printf("destroyed: dtors %d\n", co_dtors());
  h = co_make(5, 0);
  r = x19_call(resume_of(h), h);
  r += 10 * x19_call(resume_of(h), h);
  std::printf("two steps: x19 %d value %d\n", r, co_value(h));
  r = x19_call(destroy_of(h), h);
  std::printf("destroy entry: x19 %d dtors %d\n", r, co_dtors());
  h = co_make(1, 0);
  x19_call(resume_of(h), h);
  r = x19_call(resume_of(h), h);
  c = std::coroutine_handle<>::from_address(h);
  std::printf("final: x19 %d done %d\n", r, (int)c.done());
  r = x19_call(destroy_of(h), h);
  std::printf("final destroy: x19 %d dtors %d\n", r, co_dtors());
}
"#;

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
fn hod_file(dir: &Scratch, input: &Path, name: &str, options: Options) -> PathBuf {
    let text = std::fs::read_to_string(input).unwrap();
    let (module, bodies) = parse(&text);
    let emitted =
        compile_module(&module, &bodies, &options).unwrap_or_else(|e| panic!("{name}: {e}"));
    write_object(dir, &format!("{name}.o"), &emitted)
}

/// The environments every program runs in: plain, and with poisoned
/// frees (design 0005 5.5).
const POISONS: [&[(&str, &str)]; 3] = [
    &[],
    &[("GLIBC_TUNABLES", "glibc.malloc.tcache_count=0"), ("MALLOC_PERTURB_", "165")],
    &[("MALLOC_CHECK_", "3")],
];

/// Links `inputs` under every linker and runs the program in every
/// environment of [`POISONS`]; returns the outputs.
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
            let mut command = Command::new(&exe);
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
fn the_coroutine_frame_keeps_x19_for_callers_and_unwinders() {
    if !cfg!(target_arch = "aarch64") {
        return;
    }
    let dir = Scratch::new("coro-x19-stress");
    let Some(toolchain) = Toolchain::host(&dir) else { return };
    let Some(major) = [22, 19].into_iter().find(|&m| versioned("llc", m).is_some()) else {
        return;
    };
    let Some(clang) = versioned("clang++", major) else { return };
    let caller = dir.write("x19.ll", format!("{}{X19_CALLER}", ll_header(major)));
    let Some(caller) = opt_llc(&dir, major, &caller, "x19", false) else { return };
    let cpp = dir.write("driver.cpp", PLAIN_DRIVER);
    let driver = dir.join("driver.o");
    run_ok(
        Command::new(clang).args(["-c", "-fPIC", "-O0"]).arg(&cpp).arg("-o").arg(&driver),
        "c++",
    );
    let want = "returns: 1 12\npasses: 3\ncatches: 1 99\n";
    for (mode, options) in [("plain", PIC), ("coro-frame", CORO_FRAME)] {
        let (module, bodies) = parse(&format!("{}{PLAIN}", ll_header(22)));
        let emitted = compile_module(&module, &bodies, &options).unwrap_or_else(|e| panic!("{e}"));
        let object = write_object(&dir, &format!("{mode}.o"), &emitted);
        let inputs = [driver.as_path(), caller.as_path(), object.as_path()];
        for (what, out) in link_and_run_all(&toolchain, &dir, mode, &inputs) {
            assert_eq!(out, want, "{what}");
        }
    }
    // The stress is real: the frame-base save and the clobber are there.
    let (module, bodies) = parse(&format!("{}{PLAIN}", ll_header(22)));
    let emitted = compile_module(&module, &bodies, &CORO_FRAME).unwrap();
    let object = write_object(&dir, "shape.o", &emitted);
    if let Some(lines) = objdump(&object) {
        assert!(lines.iter().any(|l| l == "add x19, x29, #0x0"), "{lines:#?}");
        assert!(lines.iter().any(|l| l.starts_with("str x19, [x29")), "{lines:#?}");
        assert!(lines.iter().any(|l| l.starts_with("ldr x19, [x29")), "{lines:#?}");
    }
}

#[test]
fn coroutines_mixed_with_llc_objects_keep_x19() {
    if !cfg!(target_arch = "aarch64") {
        return;
    }
    let dir = Scratch::new("coro-body-mixed");
    let Some(toolchain) = Toolchain::host(&dir) else { return };
    let mut runs = 0;
    for major in [19, 22] {
        let Some(coro) = pre_opt(&dir, major, COROUTINE, &format!("coro{major}")) else { continue };
        let Some(driver) = pre_opt(&dir, major, CORO_DRIVER, &format!("driver{major}")) else {
            continue;
        };
        let caller =
            dir.write(&format!("x19-{major}.ll"), format!("{}{X19_CALLER}", ll_header(major)));
        let Some(caller) = opt_llc(&dir, major, &caller, &format!("x19-{major}"), false) else {
            continue;
        };
        let Some(coro_llc) = opt_llc(&dir, major, &coro, &format!("coro{major}-llc"), true) else {
            continue;
        };
        let Some(driver_llc) = opt_llc(&dir, major, &driver, &format!("driver{major}-llc"), true)
        else {
            continue;
        };
        let coro_hod = hod_file(&dir, &coro, &format!("coro{major}-hod"), PIC);
        let driver_hod = hod_file(&dir, &driver, &format!("driver{major}-hod"), PIC);
        let reference = link_and_run_all(
            &toolchain,
            &dir,
            &format!("llc{major}"),
            &[&driver_llc, &coro_llc, &caller],
        );
        let want = reference[0].1.clone();
        assert!(want.contains("resume 2: x19 1 value 30 done 0\n"), "{want}");
        assert!(want.contains("throw: x19 3 done 1"), "{want}");
        for (what, out) in &reference {
            assert_eq!(out, &want, "{what}");
        }
        for (name, inputs) in [
            (format!("hod-coro{major}"), [&driver_llc, &coro_hod, &caller]),
            (format!("hod-driver{major}"), [&driver_hod, &coro_llc, &caller]),
            (format!("hod-both{major}"), [&driver_hod, &coro_hod, &caller]),
        ] {
            let inputs = inputs.map(PathBuf::as_path);
            for (what, out) in link_and_run_all(&toolchain, &dir, &name, &inputs) {
                assert_eq!(out, want, "{what}");
                runs += 1;
            }
        }
    }
    eprintln!("{runs} mixed runs");
}

/// The suspended region (design 0005 §3.3): between the call of an
/// `await_suspend` wrapper and its suspend (`mov w0, #-1`) no instruction
/// addresses the heap frame.
#[test]
fn nothing_addresses_the_frame_between_a_wrapper_and_its_suspend() {
    if !cfg!(target_arch = "aarch64") {
        return;
    }
    let dir = Scratch::new("coro-region");
    let mut checked = 0;
    for major in [19, 22] {
        let Some(ir) = pre_opt(&dir, major, COROUTINE, &format!("coro{major}")) else { continue };
        let object = hod_file(&dir, &ir, &format!("coro{major}"), PIC);
        let Some(lines) = objdump(&object) else { return };
        for (at, line) in lines.iter().enumerate() {
            if line != "mov w0, #-1" {
                continue;
            }
            let call =
                lines[..at].iter().rposition(|l| l.starts_with("blr ") || l.starts_with("bl "));
            let call = call.unwrap_or_else(|| panic!("no call before the suspend at {at}"));
            for between in &lines[call + 1..at] {
                assert!(!between.contains("[x19"), "LLVM {major}: {between} at {at}:\n{lines:#?}");
            }
            checked += 1;
        }
    }
    assert!(checked == 0 || checked >= 6, "{checked} suspends");
}

/// A coroutine of LLVM 22's dialect, with `{cc}`, a stack slot aligned
/// to `{align}` that no suspend crosses, and `{before}` just before its
/// suspend.
fn coroutine(cc: &str, align: u32, before: &str) -> String {
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

define {cc} ptr @co(i32 %n) presplitcoroutine {{
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

fn compiled(functions: &str) -> Result<(), String> {
    let (module, bodies) = parse(&format!("{}{functions}", ll_header(22)));
    compile_module(&module, &bodies, &PIC).map(|_| ()).map_err(|e| e.to_string())
}

/// The construct name a module is refused by.
fn refused_as(text: &str) -> Option<&'static str> {
    let (module, bodies) = parse(text);
    let mut scratch = hod_cg::Scratch::new();
    let mut chunk = hod_cg::Chunk::new();
    for body in &bodies {
        match hod_cg::compile_function(&hod_a64::A64, &module, body, &PIC, &mut scratch, &mut chunk)
        {
            Ok(()) => {}
            Err(CgError::Unsupported { construct }) => return Some(construct),
            Err(other) => panic!("not a refusal: {other}"),
        }
    }
    None
}

#[test]
fn what_is_not_compiled_is_refused_by_name() {
    assert_eq!(compiled(&coroutine("", 4, "")), Ok(()));
    let header = ll_header(22);
    for (text, name) in [
        (coroutine("preserve_mostcc", 4, ""), "a64:coro-preserve-cc"),
        (coroutine("preserve_allcc", 4, ""), "a64:coro-preserve-cc"),
        (coroutine("", 32, ""), "coro:overaligned-stack-slot"),
        (
            coroutine(
                "",
                4,
                "  call void @llvm.coro.await.suspend.handle(ptr null, ptr %h, ptr @wrapper)",
            ),
            "intrinsic:llvm.coro.await.suspend.handle",
        ),
        (
            "declare token @llvm.coro.id(i32, ptr, ptr, ptr)\n\
             define void @f() {\n  %t = call token @llvm.coro.id(i32 0, ptr null, ptr null, ptr null)\n  ret void\n}\n"
                .to_owned(),
            "coro:outside-coroutine",
        ),
    ] {
        assert_eq!(refused_as(&format!("{header}{text}")), Some(name), "{text}");
    }
    // Design 0005's post-sync: the plan refuses it, in both dialects.
    for text in [
        include_str!("../../hod_xform/tests/data/seeds/llvm19/post-sync-19.ll"),
        include_str!("../../hod_xform/tests/data/seeds/llvm22/post-sync-22.ll"),
    ] {
        assert_eq!(refused_as(text), Some("coro:frame-access-after-suspend"));
    }
}
