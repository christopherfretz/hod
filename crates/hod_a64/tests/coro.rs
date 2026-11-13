//! Coroutine handle operations in any function and the noop coroutine
//! frame (WP-105, `spec/codegen.md` "Coroutines").
//!
//! - The lowering: the instruction shapes of `coro.resume`, `destroy`,
//!   `done`, `promise` and `noop`.
//! - The noop frame: one per object, laid out as `llc` 22 lays it out.
//! - Mixed objects: a C++ coroutine compiled by clang 19, clang 22 and
//!   kiln++ (their pre-optimisation IR through `opt -passes='default<O0>'`
//!   and `llc -O0 -global-isel=0 -fast-isel=0`) is resumed, destroyed,
//!   tested with `done` and reached through its promise (8 and 32 aligned)
//!   from hod's object, through a call and an `invoke`, and the noop
//!   handles of both objects are used by the other; every program prints
//!   what the same driver prints with `opt` + `llc`'s build of hod's
//!   module, under every linker.
//! - Refusals: coroutine bodies stay refused under their own names until
//!   WP-106; a promise alignment that is not a constant power of two.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use common::linking::Toolchain;
use common::{assert_refused, ll_header, parse, write_object};
use hod_a64::compile_module;
use hod_cg::{Options, RelocModel, Stress};
use hod_ir::{Arch, Classifier, Support};
use hod_testutil::{Scratch, run_with, tools_required};

const STATIC: Options = Options {
    reloc_model: RelocModel::Static,
    code_model: None,
    function_sections: false,
    data_sections: false,
    stress: Stress { min_pool: false, far: false, displace: false },
    debug_compression: hod_obj::Compression::None,
    split_dwarf: hod_dwarf::SplitDwarf::Off,
};
const PIC: Options = Options { reloc_model: RelocModel::Pic, ..STATIC };

/// hod's half of the mixed programs: one function per operation.
const HANDLES: &str = r"
declare void @llvm.coro.resume(ptr)
declare void @llvm.coro.destroy(ptr)
declare i1 @llvm.coro.done(ptr)
declare ptr @llvm.coro.promise(ptr, i32, i1)
declare ptr @llvm.coro.noop()
declare i32 @__gxx_personality_v0(...)
declare ptr @__cxa_begin_catch(ptr)
declare void @__cxa_end_catch()

define void @hod_resume(ptr %h) {
  call void @llvm.coro.resume(ptr %h)
  ret void
}
define void @hod_destroy(ptr %h) {
  call void @llvm.coro.destroy(ptr %h)
  ret void
}
define zeroext i1 @hod_done(ptr %h) {
  %d = call i1 @llvm.coro.done(ptr %h)
  ret i1 %d
}
define i32 @hod_resume_catch(ptr %h) personality ptr @__gxx_personality_v0 {
entry:
  invoke void @llvm.coro.resume(ptr %h) to label %ok unwind label %pad
ok:
  ret i32 0
pad:
  %lp = landingpad { ptr, i32 } catch ptr null
  %e = extractvalue { ptr, i32 } %lp, 0
  %x = call ptr @__cxa_begin_catch(ptr %e)
  call void @__cxa_end_catch()
  ret i32 1
}
define ptr @hod_promise_8(ptr %h) {
  %p = call ptr @llvm.coro.promise(ptr %h, i32 8, i1 false)
  ret ptr %p
}
define ptr @hod_from_promise_8(ptr %p) {
  %h = call ptr @llvm.coro.promise(ptr %p, i32 8, i1 true)
  ret ptr %h
}
define ptr @hod_promise_32(ptr %h) {
  %p = call ptr @llvm.coro.promise(ptr %h, i32 32, i1 false)
  ret ptr %p
}
define ptr @hod_from_promise_32(ptr %p) {
  %h = call ptr @llvm.coro.promise(ptr %p, i32 32, i1 true)
  ret ptr %h
}
define ptr @hod_noop() {
  %n = call ptr @llvm.coro.noop()
  ret ptr %n
}
define ptr @hod_noop_again() {
  %n = call ptr @llvm.coro.noop()
  ret ptr %n
}
";

/// The C++ half: coroutines with promises aligned to 8 and 32, driven
/// through hod's functions.
const DRIVER: &str = r#"
#include <coroutine>
#include <cstdio>
extern "C" {
void hod_resume(void*);
void hod_destroy(void*);
bool hod_done(void*);
int hod_resume_catch(void*);
void* hod_promise_8(void*);
void* hod_from_promise_8(void*);
void* hod_promise_32(void*);
void* hod_from_promise_32(void*);
void* hod_noop(void);
void* hod_noop_again(void);
}
static int steps, destroyed, failures;
static void check(bool ok, int line) {
  if (!ok) { std::printf("check at line %d failed\n", line); failures++; }
}
#define CHECK(x) check((x), __LINE__)
template <unsigned A, bool Throws>
struct Task {
  struct alignas(A) promise_type {
    int value = 0;
    Task get_return_object() { return {std::coroutine_handle<promise_type>::from_promise(*this)}; }
    std::suspend_always initial_suspend() noexcept { return {}; }
    std::suspend_always final_suspend() noexcept { return {}; }
    void return_void() {}
    void unhandled_exception() { throw; }
    ~promise_type() { destroyed++; }
  };
  std::coroutine_handle<promise_type> h;
};
template <unsigned A, bool Throws>
Task<A, Throws> body(int n) {
  for (int i = 0; i < n; i++) {
    steps++;
    co_await std::suspend_always{};
  }
  if (Throws) throw 42;
}
template <unsigned A>
void run(void* (*promise)(void*), void* (*from)(void*)) {
  steps = destroyed = 0;
  auto t = body<A, false>(2);
  void* h = t.h.address();
  CHECK(!hod_done(h));
  CHECK(promise(h) == &t.h.promise());
  CHECK(from(&t.h.promise()) == h);
  static_cast<typename Task<A, false>::promise_type*>(promise(h))->value = 7;
  CHECK(t.h.promise().value == 7);
  hod_resume(h);
  CHECK(steps == 1 && !hod_done(h) && !t.h.done());
  hod_resume(h);
  CHECK(steps == 2 && !hod_done(h));
  hod_resume(h);
  CHECK(steps == 2 && hod_done(h) && t.h.done());
  hod_destroy(h);
  CHECK(destroyed == 1);
  auto u = body<A, true>(1);
  h = u.h.address();
  CHECK(hod_resume_catch(h) == 0 && steps == 3);
  CHECK(hod_resume_catch(h) == 1 && hod_done(h));
  hod_destroy(h);
  CHECK(destroyed == 2);
  auto v = body<A, false>(5);
  h = v.h.address();
  hod_resume(h);
  hod_destroy(h);
  CHECK(destroyed == 3 && steps == 4);
}
extern "C" void* cpp_noop(void) { return std::noop_coroutine().address(); }
int main() {
  run<8>(hod_promise_8, hod_from_promise_8);
  run<32>(hod_promise_32, hod_from_promise_32);
  void* mine = hod_noop();
  auto theirs = std::coroutine_handle<>::from_address(mine);
  CHECK(!theirs.done());
  theirs.resume();
  theirs.destroy();
  void* cpp = cpp_noop();
  CHECK(!hod_done(cpp));
  hod_resume(cpp);
  hod_destroy(cpp);
  CHECK(!hod_done(mine));
  hod_resume(mine);
  hod_destroy(mine);
  std::printf("same object: %d %d\n", mine == hod_noop_again(), cpp == cpp_noop());
  std::printf("across objects: %d\n", mine == cpp);
  std::printf("failures: %d\n", failures);
  return failures != 0;
}
"#;

/// `.ll` text for `major`: the header, then `body`.
fn module_text(major: u32, body: &str) -> String {
    format!("{}{body}", ll_header(major))
}

/// hod's object of `body` under `options`.
fn hod_object(dir: &Scratch, name: &str, body: &str, options: Options) -> PathBuf {
    let (module, bodies) = parse(&module_text(22, body));
    let emitted = compile_module(&module, &bodies, &options).unwrap_or_else(|e| panic!("{e}"));
    write_object(dir, &format!("{name}.o"), &emitted)
}

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

/// kiln++, beside the build directory or from `HOD_KILN`.
fn kiln() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("HOD_KILN").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(path));
    }
    let beside = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .ancestors()
        .nth(2)
        .map(|root| root.join("target-kiln").join("release").join("kiln++"));
    match beside {
        Some(path) if path.is_file() => Some(path),
        _ => {
            assert!(!tools_required(), "no kiln++ found; set HOD_KILN (HOD_REQUIRE_TOOLS=1)");
            eprintln!("note: no kiln++ (set HOD_KILN); skipping its part");
            None
        }
    }
}

fn run_ok(command: &mut Command, what: &str) {
    let ran = run_with(command, &[], Duration::from_mins(3));
    assert!(ran.success(), "{what}: {}{}", ran.stdout_text(), ran.stderr_text());
}

/// The reference pipeline: `opt -passes='default<O0>'` (which runs the
/// coroutine passes), then `llc -O0 -global-isel=0 -fast-isel=0`, PIC.
fn opt_llc(dir: &Scratch, major: u32, input: &Path, name: &str) -> Option<PathBuf> {
    let opt = versioned("opt", major)?;
    let llc = versioned("llc", major)?;
    let post = dir.join(format!("{name}.post.ll"));
    run_ok(
        Command::new(opt).arg("-passes=default<O0>").arg("-S").arg(input).arg("-o").arg(&post),
        "opt",
    );
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

/// The C++ driver's object by `compiler` (pre-optimisation IR, then
/// [`opt_llc`] of LLVM `major`).
fn driver_object(dir: &Scratch, compiler: &Path, major: u32, name: &str) -> Option<PathBuf> {
    let source = dir.write(&format!("{name}.cpp"), DRIVER);
    let ir = dir.join(format!("{name}.ll"));
    run_ok(
        Command::new(compiler)
            .args(["-std=c++20", "-O0", "-S", "-emit-llvm", "-w"])
            .args(["-Xclang", "-disable-llvm-passes", "--target=aarch64-unknown-linux-gnu"])
            .arg(&source)
            .arg("-o")
            .arg(&ir),
        name,
    );
    let text = std::fs::read_to_string(&ir).unwrap();
    assert!(text.contains("@llvm.coro.suspend"), "{name}: the IR is not pre-optimisation");
    opt_llc(dir, major, &ir, name)
}

#[test]
fn coroutines_of_other_compilers_are_driven_from_hod() {
    if !cfg!(target_arch = "aarch64") {
        return;
    }
    let dir = Scratch::new("coro-mixed");
    let Some(toolchain) = Toolchain::host(&dir) else { return };
    let mut drivers = Vec::new();
    for major in [19, 22] {
        if let Some(clang) = versioned("clang++", major) {
            let name = format!("clang{major}");
            drivers.extend(driver_object(&dir, &clang, major, &name).map(|o| (name, o)));
        }
    }
    if let Some(kiln) = kiln() {
        drivers.extend(driver_object(&dir, &kiln, 22, "kiln").map(|o| ("kiln".to_owned(), o)));
    }
    let hod = hod_object(&dir, "hod", HANDLES, PIC);
    let mut references = Vec::new();
    for major in [19, 22] {
        let input = dir.write(&format!("handles{major}.ll"), module_text(major, HANDLES));
        references.extend(opt_llc(&dir, major, &input, &format!("handles{major}")));
    }
    let mut runs = 0;
    for (name, driver) in &drivers {
        for linker in &toolchain.linkers {
            let mut outputs = Vec::new();
            for (half, object) in std::iter::once(&hod).chain(&references).enumerate() {
                let exe = dir.join(format!("{name}-{half}-{}", linker.name));
                let inputs = [driver.as_path(), object.as_path(), Path::new("-lstdc++")];
                let ran = toolchain.link_and_run(linker, &[], &inputs, &exe);
                let out = ran.stdout_text();
                assert!(ran.success(), "{name} with object {half} under {}:\n{out}", linker.name);
                outputs.push(out);
                runs += 1;
            }
            assert!(outputs[0].contains("failures: 0\n"), "{name}: {}", outputs[0]);
            // Two noop handles of one object are equal, of two objects
            // are not: what `llc` makes of the same module.
            assert!(outputs[0].contains("same object: 1 1\nacross objects: 0\n"), "{name}");
            for other in &outputs[1..] {
                assert_eq!(&outputs[0], other, "{name} under {}: hod and llc differ", linker.name);
            }
        }
    }
    eprintln!("{runs} mixed programs ran");
}

/// The functions of an object, by name, each as its instruction lines
/// (`mnemonic operands`).
fn functions(object: &Path) -> Option<Vec<(String, Vec<String>)>> {
    let objdump = hod_testutil::tool("llvm-objdump")?;
    let mut command = Command::new(objdump);
    command.args(["-d", "--no-show-raw-insn", "--no-leading-addr"]).arg(object);
    let ran = run_with(&mut command, &[], Duration::from_mins(1));
    assert!(ran.success(), "llvm-objdump: {}", ran.stderr_text());
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for line in ran.stdout_text().lines() {
        if let Some(name) = line.strip_prefix('<').and_then(|l| l.strip_suffix(">:")) {
            out.push((name.to_owned(), Vec::new()));
        } else if line.starts_with(char::is_whitespace) && !line.trim().is_empty() {
            let line = line.split("//").next().unwrap_or(line);
            let words = line.split_whitespace().collect::<Vec<_>>().join(" ");
            if let Some((_, lines)) = out.last_mut() {
                lines.push(words);
            }
        }
    }
    Some(out)
}

/// The lowering's shapes, which mutations of the frame layout break:
/// `resume` calls through the pointer at offset 0, `destroy` through the
/// one at 8, `done` compares the one at 0 with null, `promise` adds or
/// subtracts `alignTo(16, align)`. The argument is in `x0` on entry.
#[test]
fn the_handle_operations_have_their_shapes() {
    let dir = Scratch::new("coro-shapes");
    let object = hod_object(&dir, "shapes", HANDLES, PIC);
    let Some(functions) = functions(&object) else { return };
    let body = |name: &str| {
        let found = functions.iter().find(|(n, _)| n == name);
        found.unwrap_or_else(|| panic!("no {name}")).1.join("\n")
    };
    let loads = |text: &str, operand: &str| {
        text.lines().filter(|l| l.starts_with("ldr x") && l.ends_with(operand)).count()
    };
    for (name, operand) in
        [("hod_resume", ", [x0]"), ("hod_resume_catch", ", [x0]"), ("hod_destroy", ", [x0, #0x8]")]
    {
        let text = body(name);
        assert_eq!(loads(&text, operand), 1, "{name}:\n{text}");
        assert_eq!(text.matches("blr ").count(), 1, "{name}:\n{text}");
    }
    let done = body("hod_done");
    assert_eq!(loads(&done, ", [x0]"), 1, "{done}");
    assert!(done.contains(", #0x0\ncset w") && done.contains(", eq"), "{done}");
    for (name, shape) in [
        ("hod_promise_8", "add x"),
        ("hod_from_promise_8", "sub x"),
        ("hod_promise_32", "add x"),
        ("hod_from_promise_32", "sub x"),
    ] {
        let text = body(name);
        let offset = if name.ends_with("32") { ", x0, #0x20" } else { ", x0, #0x10" };
        let found = text.lines().any(|l| l.starts_with(shape) && l.ends_with(offset));
        assert!(found, "{name}: {shape}..{offset}\n{text}");
    }
}

/// `readelf -S -s -r` of an object, wide.
fn readelf(object: &Path) -> Option<String> {
    let readelf = hod_testutil::tool("llvm-readelf")?;
    let mut command = Command::new(readelf);
    command.args(["-S", "-s", "-r", "-W"]).arg(object);
    let ran = run_with(&mut command, &[], Duration::from_mins(1));
    assert!(ran.success(), "llvm-readelf: {}", ran.stderr_text());
    Some(ran.stdout_text())
}

/// One frame and one function per object, however many functions refer
/// to it, in the sections `llc` 22 chooses; none without a reference.
#[test]
fn the_noop_frame_is_written_once_per_object() {
    let dir = Scratch::new("coro-noop");
    let split = Options { function_sections: true, data_sections: true, ..PIC };
    let cases = [
        ("pic", PIC, ".text", ".data.rel.ro"),
        ("static", STATIC, ".text", ".rodata"),
        ("split", split, ".text.__NoopCoro_ResumeDestroy", ".data.rel.ro..LNoopCoro.Frame.Const"),
    ];
    for (name, options, text_section, data_section) in cases {
        let object = hod_object(&dir, name, HANDLES, options);
        let Some(elf) = readelf(&object) else { return };
        let functions: Vec<&str> =
            elf.lines().filter(|l| l.ends_with(" __NoopCoro_ResumeDestroy")).collect();
        assert_eq!(functions.len(), 1, "{name}:\n{elf}");
        let fields: Vec<&str> = functions[0].split_whitespace().collect();
        // Num: Value Size Type Bind Vis Ndx Name
        assert_eq!(&fields[2..6], ["4", "FUNC", "LOCAL", "DEFAULT"], "{name}: {}", functions[0]);
        let value = u64::from_str_radix(fields[1], 16).unwrap();
        let header = format!("] {data_section} ");
        let frame: Vec<&str> = elf.lines().filter(|l| l.contains(&header)).collect();
        assert_eq!(frame.len(), 1, "{name}: one {data_section}\n{elf}");
        assert!(frame[0].split_whitespace().any(|f| f == "000010"), "{name}: {}", frame[0]);
        let relocs = format!("Relocation section '.rela{data_section}'");
        let at = elf.find(&relocs).unwrap_or_else(|| panic!("{name}: no {relocs}\n{elf}"));
        let rows: Vec<&str> = elf[at..]
            .lines()
            .skip(2)
            .take_while(|l| !l.trim().is_empty())
            .filter(|l| l.contains("R_AARCH64_ABS64"))
            .collect();
        let target = format!("{text_section} + {value:x}");
        assert_eq!(rows.len(), 2, "{name}:\n{}", &elf[at..]);
        assert!(rows.iter().all(|r| r.ends_with(&target)), "{name}: {rows:?} to {target}");
    }
    let plain = hod_object(&dir, "plain", "define void @f() {\n  ret void\n}\n", PIC);
    if let Some(elf) = readelf(&plain) {
        assert!(!elf.contains("NoopCoro"), "no frame without a reference:\n{elf}");
    }
}

/// A presplit coroutine stays refused, under the names of its attribute
/// and its body intrinsics, until WP-106; the handle operations are
/// accepted wherever they are.
#[test]
fn coroutine_bodies_stay_refused() {
    let text = module_text(
        22,
        r"
declare token @llvm.coro.id(i32, ptr, ptr, ptr)
declare ptr @llvm.coro.begin(token, ptr)
declare i8 @llvm.coro.suspend(token, i1)
declare i1 @llvm.coro.done(ptr)
declare void @llvm.coro.resume(ptr)
define ptr @co(ptr %mem) presplitcoroutine {
  %id = call token @llvm.coro.id(i32 16, ptr null, ptr null, ptr null)
  %h = call ptr @llvm.coro.begin(token %id, ptr %mem)
  %s = call i8 @llvm.coro.suspend(token none, i1 false)
  %d = call i1 @llvm.coro.done(ptr %h)
  call void @llvm.coro.resume(ptr %h)
  ret ptr %h
}
",
    );
    let (module, bodies) = parse(&text);
    let census = hod_ir::census(&module, &bodies);
    for refused in [
        "attr:presplitcoroutine",
        "intrinsic:llvm.coro.id",
        "intrinsic:llvm.coro.begin",
        "intrinsic:llvm.coro.suspend",
    ] {
        assert!(census.get(refused).is_some(), "{refused}: {:?}", census.entries());
    }
    for accepted in ["intrinsic:llvm.coro.done", "intrinsic:llvm.coro.resume"] {
        assert!(census.get(accepted).is_none(), "{accepted}: {:?}", census.entries());
    }
    let (module, bodies) = parse(&module_text(22, HANDLES));
    let classifier = Classifier::new(Support::shipped(Arch::Aarch64));
    classifier.module(&module).unwrap_or_else(|e| panic!("{e:?}"));
    for body in &bodies {
        classifier.body(&module, body).unwrap_or_else(|e| panic!("{e:?}"));
    }
}

/// A promise alignment that is not a constant power of two of at most
/// 2^30, or a direction that is not a constant; `coro.noop` outside the
/// small code model.
#[test]
fn promises_without_a_meaningful_offset_are_refused() {
    let promise = |align: &str, from: &str| {
        format!(
            "declare ptr @llvm.coro.promise(ptr, i32, i1)\n\
             define ptr @f(ptr %h, i32 %a, i1 %b) {{\n  \
             %p = call ptr @llvm.coro.promise(ptr %h, i32 {align}, i1 {from})\n  ret ptr %p\n}}\n"
        )
    };
    let cases = [
        promise("%a", "false"),
        promise("24", "false"),
        promise("0", "true"),
        promise("-2147483648", "false"),
        promise("16", "%b"),
    ];
    let cases: Vec<(&str, &'static str)> =
        cases.iter().map(|c| (c.as_str(), hod_a64::coro::PROMISE)).collect();
    assert_refused(&cases);
    let ok = promise("1073741824", "true");
    assert_eq!(common::compile_ll(&ok), Ok(()));
    let noop = module_text(
        22,
        "declare ptr @llvm.coro.noop()\n\
         define ptr @f() {\n  %n = call ptr @llvm.coro.noop()\n  ret ptr %n\n}\n",
    );
    let (module, bodies) = parse(&noop);
    let large = Options { code_model: Some(hod_ir::CodeModel::Large), ..STATIC };
    let refused = common::chunk(&module, &bodies, &large).map(|_| ());
    assert_eq!(refused, Err(hod_cg::CgError::unsupported("a64:code-model")));
}
