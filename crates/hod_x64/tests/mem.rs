//! The x86-64 memory group (WP-123, `spec/codegen.md` §5.2, §5.6.4
//! "Memory"): `load`, `store`, `alloca`, `ptradd`, `llvm.stacksave` and
//! `llvm.stackrestore`, for `Gpr` and `Fpr` values.
//!
//! - `generated_functions_agree_with_the_interpreter`: straight-line
//!   functions of the group (`common/mem_gen.rs`), many to one object,
//!   run (under `qemu-x86_64` on another host) from a C driver whose
//!   `%buf` ends at a page boundary (an access past a store size
//!   faults), compared byte for byte with `hod_interp`: hod plainly,
//!   with every stress option, position-independent, and the same module
//!   through `llc -O0 -fast-isel=0` 22 and 19.
//! - `the_stack_forms_hold_natively`: dynamic and over-aligned `alloca`,
//!   `stacksave`/`stackrestore` (which the interpreter does not run),
//!   checked by the C driver, against `llc`, and in mixed objects.
//! - the exact instructions of the fused addressing modes and of an
//!   odd-width store; the refusals.

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;
#[path = "common/mem_gen.rs"]
mod mem_gen;

use std::cell::RefCell;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::time::Duration;

use hod_cg::{CgError, Chunk, Options, RelocModel, Scratch as CgScratch, Stress, compile_function};
use hod_interp::{Interp, Program, Val};
use hod_ir::{Body, Module};
use hod_mc_x64 as mc;
use hod_testutil::{Scratch, run_with, tools_required};
use hod_x64::{X64, mem};
use linking::{Toolchain, exit_code};
use mc::{Mem, Reg, Scale, Size};
use mem_gen::{BUF, Func, GLOBAL, generate};

// ---- modules ----

/// The module header: the x86-64 data layout and triple.
fn header() -> String {
    format!(
        "target datalayout = \"{}\"\ntarget triple = \"x86_64-unknown-linux-gnu\"\n",
        hod_ir::X86_64_LINUX_DATA_LAYOUT
    )
}

/// A module and its bodies from `.ll` text (the header included).
fn parse(text: &str) -> (Module, Vec<Body>) {
    let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
        .unwrap_or_else(|e| panic!("hod_ll: {e}\n{text}"));
    let text_side = parsed.text;
    let mut module = parsed.module;
    let mut bodies = Vec::new();
    hod_ll::parse_bodies(&mut module, &text_side, |_, parsed| {
        let body = parsed
            .body
            .unwrap_or_else(|| panic!("hod_ll: unsupported {:?} in\n{text}", parsed.unsupported));
        bodies.push(body.clone());
        Ok::<(), hod_ll::LlError>(())
    })
    .unwrap_or_else(|e| panic!("hod_ll: {e}\n{text}"));
    (module, bodies)
}

/// Every body of a module compiled into one chunk.
fn chunk(module: &Module, bodies: &[Body], options: &Options) -> Result<Chunk, CgError> {
    let mut scratch = CgScratch::new();
    let mut chunk = Chunk::new();
    for body in bodies {
        compile_function(&X64, module, body, options, &mut scratch, &mut chunk)?;
    }
    Ok(chunk)
}

/// What compiling the `.ll` functions `functions` reports.
fn compile_ll(functions: &str) -> Result<(), CgError> {
    let (m, bodies) = parse(&format!("{}{functions}", header()));
    chunk(&m, &bodies, &Options::default()).map(|_| ())
}

/// The code of the `.ll` functions `functions`.
fn code(functions: &str) -> Vec<u8> {
    let (m, bodies) = parse(&format!("{}{functions}", header()));
    chunk(&m, &bodies, &Options::default()).unwrap_or_else(|e| panic!("{e}\n{functions}")).code
}

/// The object of a module, or the reason it was refused.
fn object(dir: &Scratch, name: &str, text: &str, options: &Options) -> PathBuf {
    let (module, bodies) = parse(text);
    let bytes = hod_x64::compile_to_object(&module, &bodies, options)
        .unwrap_or_else(|e| panic!("{name}: {e}\n{text}"));
    dir.write(&format!("{name}.o"), bytes)
}

/// `llc` of LLVM `major`, `/usr/lib/llvm-<major>/bin/llc` or
/// `llc-<major>`; `None` with a note when it is not installed.
fn llc_of(major: u32) -> Option<PathBuf> {
    let installed = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/llc"));
    if installed.is_file() {
        return Some(installed);
    }
    let found = hod_testutil::find_tool(&format!("llc-{major}"));
    if found.is_none() {
        assert!(!tools_required(), "llc {major} not found (HOD_REQUIRE_TOOLS=1)");
        eprintln!("note: llc {major} not found; skipping its mode");
    }
    found
}

/// `llc -O0 -fast-isel=0` of `text` for x86-64, position-independent.
fn llc(dir: &Scratch, major: u32, name: &str, text: &str) -> Option<PathBuf> {
    let llc = llc_of(major)?;
    let input = dir.write(&format!("{name}.ll"), text);
    let output = dir.join(format!("{name}.o"));
    let mut command = Command::new(llc);
    command
        .args(["-O0", "-fast-isel=0", "-filetype=obj", "-relocation-model=pic"])
        .arg("-mtriple=x86_64-unknown-linux-gnu")
        .arg(&input)
        .arg("-o")
        .arg(&output);
    let outcome = run_with(&mut command, &[], Duration::from_mins(5));
    assert!(outcome.success(), "llc {major} failed:\n{}", outcome.stderr_text());
    Some(output)
}

// ---- the generated functions ----

/// The functions of `seeds`, named `m<seed>`.
fn batch(seeds: std::ops::Range<u64>) -> Vec<Func> {
    seeds.map(|seed| generate(&format!("m{seed}"), seed)).collect()
}

/// What hod compiles: the variables and the functions.
fn native_text(funcs: &[Func]) -> String {
    let mut text = header();
    for f in funcs {
        text.push_str(&f.globals_text());
        text.push_str(&f.text);
    }
    text
}

/// What the interpreter runs: the native text, each function's `%buf`
/// as a variable, and a wrapper that calls the function and dumps its
/// memory through the host function `dump`.
fn interp_text(funcs: &[Func]) -> String {
    let mut text = native_text(funcs);
    text.push_str("declare void @dump(ptr, i64)\n");
    for f in funcs {
        let name = &f.name;
        writeln!(text, "@{name}_buf = global [{BUF} x i8] {}, align 16", mem_gen::bytes(&f.buf))
            .unwrap();
        let ret = f.ret.name();
        let mut args = format!("ptr @{name}_buf");
        for p in &f.params {
            write!(args, ", {}", p.ll_arg()).unwrap();
        }
        writeln!(text, "define {ret} @{name}_w() {{\n  %r = call {ret} @{name}({args})").unwrap();
        writeln!(text, "  call void @dump(ptr @{name}_buf, i64 {BUF})").unwrap();
        for g in &f.globals {
            writeln!(text, "  call void @dump(ptr @{}, i64 {GLOBAL})", g.name).unwrap();
        }
        writeln!(text, "  ret {ret} %r\n}}").unwrap();
    }
    text
}

/// What each function's run prints: `f`, `r` (the result's low bits),
/// one line of hex per region.
fn expected(funcs: &[Func]) -> Vec<String> {
    let text = interp_text(funcs);
    let (module, bodies) = parse(&text);
    let program = Program::with_bodies(&module, &bodies).unwrap();
    let dumps: Rc<RefCell<Vec<String>>> = Rc::default();
    let mut interp = Interp::new(&program);
    let sink = Rc::clone(&dumps);
    interp.host("dump", move |cx, args| {
        let (Val::Ptr(addr), Val::Int { value: len, .. }) = (&args[0], &args[1]) else {
            panic!("dump({args:?})");
        };
        let data = cx.memory.read(*addr, *len as u64)?;
        sink.borrow_mut().push(data.iter().fold(String::new(), |mut s, b| {
            write!(s, "{b:02x}").unwrap();
            s
        }));
        Ok(None)
    });
    funcs
        .iter()
        .map(|f| {
            dumps.borrow_mut().clear();
            let entry = module.find_symbol(format!("{}_w", f.name).as_bytes()).unwrap();
            let outcome = interp.run(entry, &[]);
            let Ok(Some(Val::Int { value, .. })) = outcome.result else {
                panic!("{}: {:?}\n{}", f.name, outcome.result, f.text);
            };
            let mut out = format!("f {}\nr {:x}\n", f.name, (value as u64) & f.ret.mask());
            let dumps = dumps.borrow();
            writeln!(out, "b {}", dumps[0]).unwrap();
            for (k, d) in dumps[1..].iter().enumerate() {
                writeln!(out, "g{k} {d}").unwrap();
            }
            out
        })
        .collect()
}

/// The C driver: `%buf` is the last 256 bytes of a page whose next page
/// is inaccessible; each function gets it filled with its initial bytes
/// and its integer parameters with garbage above their widths; then the
/// result (masked to its width) and the regions are printed as
/// `expected` does.
fn driver(funcs: &[Func]) -> String {
    let mut c = String::from(
        "#include <stdint.h>\n#include <stdio.h>\n#include <string.h>\n\
         #include <sys/mman.h>\n#include <unistd.h>\n\
         static void dump(const char *tag, const unsigned char *p, int n) {\n\
           printf(\"%s \", tag);\n  for (int i = 0; i < n; i++) printf(\"%02x\", p[i]);\n\
           printf(\"\\n\");\n}\n\
         static float f32(uint32_t b) { float f; memcpy(&f, &b, 4); return f; }\n\
         static double f64(uint64_t b) { double d; memcpy(&d, &b, 8); return d; }\n",
    );
    for f in funcs {
        let params: String = f.params.iter().fold(String::new(), |mut out, p| {
            write!(out, ", {}", p.ty.c_name()).unwrap();
            out
        });
        writeln!(c, "uint64_t {}(unsigned char *{params});", f.name).unwrap();
        for g in &f.globals {
            writeln!(c, "extern unsigned char {}[{GLOBAL}];", g.name).unwrap();
        }
        let init: Vec<String> = f.buf.iter().map(u8::to_string).collect();
        writeln!(c, "static const unsigned char {}_init[{BUF}] = {{{}}};", f.name, init.join(","))
            .unwrap();
    }
    c.push_str(
        "int main(void) {\n  long pg = sysconf(_SC_PAGESIZE);\n\
         unsigned char *map = mmap(0, 2 * pg, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);\n\
         if (map == MAP_FAILED || mprotect(map + pg, pg, PROT_NONE)) return 99;\n",
    );
    writeln!(c, "  unsigned char *buf = map + pg - {BUF};").unwrap();
    for f in funcs {
        let name = &f.name;
        let args = f.params.iter().fold(String::new(), |mut out, p| {
            write!(out, ", {}", p.c_arg()).unwrap();
            out
        });
        writeln!(c, "  memcpy(buf, {name}_init, {BUF});").unwrap();
        writeln!(
            c,
            "  {{ uint64_t r = {name}(buf{args}) & {}ULL; printf(\"f {name}\\nr %llx\\n\", (unsigned long long)r); }}",
            f.ret.mask()
        )
        .unwrap();
        writeln!(c, "  dump(\"b\", buf, {BUF});").unwrap();
        for (k, g) in f.globals.iter().enumerate() {
            writeln!(c, "  dump(\"g{k}\", {}, {GLOBAL});", g.name).unwrap();
        }
        c.push_str("  fflush(stdout);\n");
    }
    c.push_str("  return 0;\n}\n");
    c
}

/// The output of each function: from an `f` line to the next.
fn split(output: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in output.lines() {
        if line.starts_with("f ") || out.is_empty() {
            out.push(String::new());
        }
        let last = out.last_mut().expect("pushed");
        last.push_str(line);
        last.push('\n');
    }
    out
}

/// The ways a batch is compiled: hod plainly, stressed, PIC; `llc` 22
/// and 19.
const MODES: [&str; 5] = ["plain", "stress", "pic", "llc22", "llc19"];

/// The bytes of an ELF64 object's executable sections.
fn text_bytes(object: &Path) -> u64 {
    let elf = std::fs::read(object).expect("an object");
    let u = |at: usize, n: usize| {
        elf[at..at + n].iter().rev().fold(0u64, |acc, &b| (acc << 8) | u64::from(b))
    };
    let (table, entry, count) = (u(0x28, 8) as usize, u(0x3a, 2) as usize, u(0x3c, 2) as usize);
    (0..count)
        .map(|k| table + k * entry)
        .filter(|&h| u(h + 8, 8) & 4 != 0)
        .map(|h| u(h + 32, 8))
        .sum()
}

/// Runs `funcs` in every mode and returns the first difference from
/// `want` per mode, as `(what, report)`. `None` without the tools.
fn run_modes(funcs: &[Func], want: &[String]) -> Option<Vec<(String, String)>> {
    let scratch = Scratch::new("x64-mem");
    let toolchain = Toolchain::x86_64(&scratch)?;
    let linker = toolchain.linkers.first()?;
    let text = native_text(funcs);
    let driver = toolchain.compile_c(&scratch, "driver", &driver(funcs), &["-fPIE"]);
    let mut wrong = Vec::new();
    for mode in MODES {
        let object = match mode {
            "llc22" | "llc19" => {
                let major = if mode == "llc22" { 22 } else { 19 };
                let Some(object) = llc(&scratch, major, mode, &text) else { continue };
                object
            }
            _ => {
                let options = Options {
                    stress: if mode == "stress" { Stress::ALL } else { Stress::default() },
                    reloc_model: if mode == "plain" { RelocModel::Static } else { RelocModel::Pic },
                    ..Options::default()
                };
                object(&scratch, mode, &text, &options)
            }
        };
        if mode == "pic" || mode == "llc22" {
            eprintln!("x64 mem: {mode}: {} text bytes", text_bytes(&object));
        }
        let program = scratch.join(mode);
        toolchain.link_ok(linker, &["-pie"], &[&driver, &object], &program);
        let ran = run_with(&mut toolchain.runner_command(&program), &[], Duration::from_mins(5));
        let got = split(&ran.stdout_text());
        for (k, f) in funcs.iter().enumerate() {
            let got = got.get(k).map_or("(nothing: the run stopped)", String::as_str);
            if got != want[k] {
                wrong.push((
                    format!("{mode} {}", f.name),
                    format!("{}\ninterpreter\n{}native ({:?})\n{got}", f.text, want[k], ran.status),
                ));
                break;
            }
        }
        if !ran.success() && wrong.last().is_none_or(|(m, _)| !m.starts_with(mode)) {
            wrong.push((mode.into(), format!("{:?}\n{}", ran.status, ran.stderr_text())));
        }
    }
    Some(wrong)
}

fn check(seeds: std::ops::Range<u64>, per_batch: u64) -> usize {
    let mut checked = 0;
    let mut start = seeds.start;
    while start < seeds.end {
        let end = (start + per_batch).min(seeds.end);
        let funcs = batch(start..end);
        let want = expected(&funcs);
        let Some(wrong) = run_modes(&funcs, &want) else { return checked };
        if let Some((what, report)) = wrong.first() {
            let all: Vec<&str> = wrong.iter().map(|(w, _)| w.as_str()).collect();
            panic!("{what}: hod_x64 and the interpreter disagree (all: {all:?})\n{report}");
        }
        checked += funcs.len();
        start = end;
    }
    checked
}

/// Generated functions of the group agree with `hod_interp` in every
/// mode (and through `llc` 22 and 19).
#[test]
fn generated_functions_agree_with_the_interpreter() {
    let checked = check(0..1500, 1500);
    eprintln!("x64 mem: {checked} functions agree in {} modes", MODES.len());
}

/// More seeds: `HOD_MEM_SEEDS=<n>` (default 10000).
#[test]
#[ignore = "minutes under the emulator; run with --ignored"]
fn many_generated_functions_agree() {
    let seeds: u64 =
        std::env::var("HOD_MEM_SEEDS").ok().and_then(|t| t.parse().ok()).unwrap_or(10_000);
    let checked = check(1_000_000..1_000_000 + seeds, 2500);
    eprintln!("x64 mem: {checked} functions agree");
}

// ---- the stack forms, checked by the driver ----

const STACK: &str = r"
declare ptr @llvm.stacksave.p0()
declare void @llvm.stackrestore.p0(ptr)
declare void @seen(ptr, i64)

; out[0] = a dynamic alloca of n bytes, out[1] = the one after a restore
; to before it (the same address), out[2] = the saved stack pointer.
define void @save_restore(ptr %out, i64 %n) {
entry:
  br label %b
b:
  %s = call ptr @llvm.stacksave.p0()
  %a = alloca i8, i64 %n, align 16
  call void @llvm.stackrestore.p0(ptr %s)
  %c = alloca i8, i64 %n, align 16
  store ptr %a, ptr %out
  %o1 = getelementptr ptr, ptr %out, i64 1
  store ptr %c, ptr %o1
  %o2 = getelementptr ptr, ptr %out, i64 2
  store ptr %s, ptr %o2
  ret void
}

; Two allocas of an i17 count (garbage above it from the caller) of a
; 12-byte element, over-aligned; both are written in full, handed to C
; (which checks the stack pointer's alignment at the call) and read back.
define i64 @dyn(ptr %out, i17 %n, i64 %x) {
entry:
  br label %b
b:
  %a = alloca { i8, i32, i16 }, i17 %n, align 64
  %c = alloca [3 x i8], i17 %n, align 4096
  store ptr %a, ptr %out
  %o1 = getelementptr ptr, ptr %out, i64 1
  store ptr %c, ptr %o1
  %last = getelementptr { i8, i32, i16 }, ptr %a, i17 %n
  %end = getelementptr i8, ptr %last, i64 -8
  store i64 %x, ptr %end
  %lastc = getelementptr [3 x i8], ptr %c, i17 %n
  %endc = getelementptr i8, ptr %lastc, i64 -3
  store i24 1193046, ptr %endc
  call void @seen(ptr %a, i64 64)
  call void @seen(ptr %c, i64 4096)
  %y = load i64, ptr %end
  %yc = load i24, ptr %endc
  %bad = getelementptr i8, ptr %out, i64 24
  store i24 %yc, ptr %bad
  ret i64 %y
}

; Zero-sized allocas, a constant count outside the entry block, and one
; beyond 2^24 bytes (never touched); an i64 count of a 24-byte element
; (a multiply) with a byte count above 32 bits that wraps to 48.
define void @odd_sizes(ptr %out, i32 %zero) {
entry:
  br label %b
b:
  %z = alloca i64, i32 %zero, align 32
  %k = alloca i32, i32 5, align 8
  %big = alloca i8, i64 17000000, align 16
  %z2 = alloca i8, i64 0
  store ptr %z, ptr %out
  %o1 = getelementptr ptr, ptr %out, i64 1
  store ptr %k, ptr %o1
  %o2 = getelementptr ptr, ptr %out, i64 2
  store ptr %big, ptr %o2
  %o3 = getelementptr ptr, ptr %out, i64 3
  store ptr %z2, ptr %o3
  ret void
}

; Over-aligned and zero-sized fixed slots; a far slot; a dynamic alloca
; whose size is a multiply.
define i64 @slots(ptr %out, i64 %x, i64 %n) {
entry:
  %a = alloca [8 x i8], align 128
  %z = alloca [0 x i8], align 64
  %far = alloca [70000 x i8], align 8
  br label %b
b:
  %m = alloca [3 x i64], i64 %n, align 8
  store ptr %a, ptr %out
  %o1 = getelementptr ptr, ptr %out, i64 1
  store ptr %z, ptr %o1
  %o2 = getelementptr ptr, ptr %out, i64 2
  store ptr %m, ptr %o2
  %f = getelementptr i8, ptr %far, i64 69992
  store i64 %x, ptr %f
  store i64 %x, ptr %a
  %mi = getelementptr [3 x i64], ptr %m, i64 %n, i64 -1
  store i64 %x, ptr %mi
  call void @seen(ptr %a, i64 128)
  call void @seen(ptr %m, i64 16)
  %y = load i64, ptr %f
  %ym = load i64, ptr %mi
  %o3 = getelementptr ptr, ptr %out, i64 3
  store i64 %ym, ptr %o3
  ret i64 %y
}
";

const STACK_DRIVER: &str = r#"
#include <stdint.h>
#include <stdio.h>
#define CHECK(c) do { if (!(c)) { printf("line %d: %s\n", __LINE__, #c); fails++; } } while (0)
static int fails;
void save_restore(uintptr_t *out, uint64_t n);
uint64_t dyn(uintptr_t *out, uint64_t n, uint64_t x);
void odd_sizes(uintptr_t *out, uint64_t zero);
uint64_t slots(uintptr_t *out, uint64_t x, uint64_t n);
/* Called with an allocation: it is aligned, below the caller's frame, and
   the stack pointer is 16-aligned at the call (the frame address of this
   function is then 16-aligned). */
void seen(uintptr_t p, uint64_t align) {
    uintptr_t sp = (uintptr_t)__builtin_frame_address(0);
    CHECK(p % align == 0);
    CHECK(sp % 16 == 0);
    CHECK(sp < p);
}
int main(void) {
    uintptr_t out[4];
    uintptr_t here = (uintptr_t)&out;
    for (uint64_t n = 0; n < 40; n += 13) {
        save_restore(out, n);
        CHECK(out[0] == out[1]);
        CHECK(out[2] - out[0] == ((n + 15) & ~15ull));
        CHECK(out[0] % 16 == 0 && out[0] < here);
    }
    for (uint64_t n = 1; n < 9; n += 3) {
        /* garbage above the 17 bits of the count */
        uint64_t got = dyn(out, n | 0xabcd00000000ull, 0x1122334455667788ull);
        CHECK(got == 0x1122334455667788ull);
        CHECK(out[0] % 64 == 0 && out[1] % 4096 == 0);
        CHECK(out[1] + 3 * n <= out[0]);
        CHECK(out[0] < here);
        CHECK((out[3] & 0xffffff) == 1193046);
    }
    odd_sizes(out, 0xffffffff00000000ull);
    CHECK(out[0] % 32 == 0 && out[0] < here);
    CHECK(out[1] + 20 <= out[0] && out[1] % 8 == 0);
    CHECK(out[2] + 17000000 <= out[1]);
    CHECK(out[3] <= out[2]);
    CHECK(slots(out, 42, 3) == 42);
    CHECK(out[0] % 128 == 0 && out[0] < here);
    CHECK(out[1] % 64 == 0);
    CHECK(out[2] + 72 <= out[0] && out[3] == 42);
    printf("checked, %d failures\n", fails);
    return fails;
}
"#;

/// Dynamic, over-aligned, zero-sized and large `alloca`s, the stack
/// intrinsics, over-aligned and far slots: hod's objects (plain and
/// stressed) and `llc`'s pass the same checks; hod's by every linker.
#[test]
fn the_stack_forms_hold_natively() {
    let text = format!("{}{STACK}", header());
    let dir = Scratch::new("x64-mem-stack");
    let Some(toolchain) = Toolchain::x86_64(&dir) else { return };
    let driver = toolchain.compile_c(&dir, "driver", STACK_DRIVER, &["-fPIE"]);
    let mut objects = Vec::new();
    for (k, stress) in [Stress::default(), Stress::ALL].into_iter().enumerate() {
        let options = Options { stress, reloc_model: RelocModel::Pic, ..Options::default() };
        objects.push((format!("hod{k}"), object(&dir, &format!("hod{k}"), &text, &options)));
    }
    for major in [22, 19] {
        if let Some(o) = llc(&dir, major, &format!("llc{major}"), &text) {
            objects.push((format!("llc{major}"), o));
        }
    }
    for (k, (name, object)) in objects.iter().enumerate() {
        let linkers = if k == 0 { &toolchain.linkers[..] } else { &toolchain.linkers[..1] };
        for linker in linkers {
            let program = dir.join(format!("stack-{name}-{}", linker.name));
            let ran = toolchain.link_and_run(linker, &["-pie"], &[&driver, object], &program);
            assert_eq!(
                exit_code(&ran),
                Some(0),
                "{name}, {}:\n{}{}",
                linker.name,
                ran.stdout_text(),
                ran.stderr_text()
            );
        }
    }
}

/// A store through a pointer computed in another object, and the
/// reverse: hod's functions call `llc`'s and `llc`'s call hod's, over
/// pointers into each other's allocas.
const MIXED_CALLER: &str = r"
declare void @fill(ptr, i64, i17)
define i32 @outer(i64 %n, i17 %v) {
entry:
  br label %b
b:
  %a = alloca i32, i64 %n, align 32
  call void @fill(ptr %a, i64 %n, i17 %v)
  %last = getelementptr i32, ptr %a, i64 %n
  %p = getelementptr i8, ptr %last, i64 -4
  %r = load i32, ptr %p
  ret i32 %r
}
";

const MIXED_CALLEE: &str = r"
define void @fill(ptr %a, i64 %n, i17 %v) {
  %last = getelementptr i32, ptr %a, i64 %n
  %p = getelementptr i8, ptr %last, i64 -4
  store i17 %v, ptr %p
  %h = getelementptr i8, ptr %p, i64 3
  store i8 0, ptr %h
  ret void
}
";

const MIXED_DRIVER: &str = r#"
#include <stdint.h>
#include <stdio.h>
uint32_t outer(uint64_t n, uint64_t v);
int main(void) {
    int fails = 0;
    for (uint64_t n = 1; n < 6; n++) {
        uint64_t got = outer(n, 0xfffffffffffe5a5aull + n);
        /* the 17 bits stored, the bits above them zero, the top byte 0 */
        if (got != 0x5a5a + n) {
            printf("n %llu: %llx\n", (unsigned long long)n, (unsigned long long)got);
            fails++;
        }
    }
    return fails;
}
"#;

/// hod's caller with `llc`'s callee and the reverse.
#[test]
fn mixed_objects_agree() {
    let dir = Scratch::new("x64-mem-mixed");
    let Some(toolchain) = Toolchain::x86_64(&dir) else { return };
    let Some(linker) = toolchain.linkers.first() else { return };
    let driver = toolchain.compile_c(&dir, "driver", MIXED_DRIVER, &["-fPIE"]);
    let pic = Options { reloc_model: RelocModel::Pic, ..Options::default() };
    let outer = format!("{}{MIXED_CALLER}", header());
    let fill = format!("{}{MIXED_CALLEE}", header());
    let hod = (object(&dir, "hod-outer", &outer, &pic), object(&dir, "hod-fill", &fill, &pic));
    let Some(llc_outer) = llc(&dir, 22, "llc-outer", &outer) else { return };
    let Some(llc_fill) = llc(&dir, 22, "llc-fill", &fill) else { return };
    for (name, a, b) in [("hod-llc", &hod.0, &llc_fill), ("llc-hod", &llc_outer, &hod.1)] {
        let program = dir.join(name);
        let ran = toolchain.link_and_run(linker, &["-pie"], &[&driver, a, b], &program);
        assert_eq!(exit_code(&ran), Some(0), "{name}:\n{}{}", ran.stdout_text(), ran.stderr_text());
    }
}

/// `half`, `bfloat` and `fp128`, which `hod_interp` does not run: copied
/// through registers at unaligned addresses (one loaded value stored
/// twice) and stored as constants; the driver checks every byte of a
/// guarded buffer. `llc`'s objects pass the same checks.
const FLOATS: &str = r"
define void @copies(ptr %d, ptr %s) {
  %s3 = getelementptr i8, ptr %s, i64 3
  %s8 = getelementptr i8, ptr %s, i64 8
  %h = load half, ptr %s, align 1
  %b = load bfloat, ptr %s3, align 1
  %q = load fp128, ptr %s8, align 1
  %d1 = getelementptr i8, ptr %d, i64 1
  store half %h, ptr %d1, align 1
  %d5 = getelementptr i8, ptr %d, i64 5
  store bfloat %b, ptr %d5, align 1
  %d17 = getelementptr i8, ptr %d, i64 17
  store fp128 %q, ptr %d17, align 1
  %d40 = getelementptr i8, ptr %d, i64 40
  store fp128 %q, ptr %d40, align 8
  ret void
}

define void @constants(ptr %d) {
  store half 0xH3C01, ptr %d, align 1
  %d2 = getelementptr i8, ptr %d, i64 2
  store bfloat 0xR4049, ptr %d2, align 1
  %d4 = getelementptr i8, ptr %d, i64 4
  store fp128 0xL0123456789ABCDEFFEDCBA9876543210, ptr %d4, align 1
  %d20 = getelementptr i8, ptr %d, i64 20
  store fp128 zeroinitializer, ptr %d20, align 4
  %d36 = getelementptr i8, ptr %d, i64 36
  store half zeroinitializer, ptr %d36, align 2
  ret void
}
";

const FLOATS_DRIVER: &str = r#"
#include <stdint.h>
#include <stdio.h>
#include <string.h>
void copies(uint8_t *d, const uint8_t *s);
void constants(uint8_t *d);
static int fails;
static void same(const uint8_t *got, const uint8_t *want, int n, int line) {
    for (int i = 0; i < n; i++)
        if (got[i] != want[i]) {
            printf("line %d byte %d: %02x, not %02x\n", line, i, got[i], want[i]);
            fails++;
        }
}
int main(void) {
    uint8_t s[64], d[64], want[64];
    for (int i = 0; i < 64; i++) s[i] = (uint8_t)(0x35 * i + 0x81);
    memset(d, 0xaa, 64);
    memset(want, 0xaa, 64);
    copies(d, s);
    memcpy(want + 1, s, 2);
    memcpy(want + 5, s + 3, 2);
    memcpy(want + 17, s + 8, 16);
    memcpy(want + 40, s + 8, 16);
    same(d, want, 64, __LINE__);
    memset(d, 0xaa, 64);
    memset(want, 0xaa, 64);
    constants(d);
    uint64_t lo = 0x0123456789abcdefull, hi = 0xfedcba9876543210ull;
    want[0] = 0x01; want[1] = 0x3c; want[2] = 0x49; want[3] = 0x40;
    memcpy(want + 4, &lo, 8);
    memcpy(want + 12, &hi, 8);
    memset(want + 20, 0, 16);
    memset(want + 36, 0, 2);
    same(d, want, 64, __LINE__);
    return fails;
}
"#;

#[test]
fn the_interpreterless_floats_move_their_bits() {
    let text = format!("{}{FLOATS}", header());
    let dir = Scratch::new("x64-mem-floats");
    let Some(toolchain) = Toolchain::x86_64(&dir) else { return };
    let Some(linker) = toolchain.linkers.first() else { return };
    let driver = toolchain.compile_c(&dir, "driver", FLOATS_DRIVER, &["-fPIE"]);
    let mut objects = Vec::new();
    for (k, stress) in [Stress::default(), Stress::ALL].into_iter().enumerate() {
        let options = Options { stress, reloc_model: RelocModel::Pic, ..Options::default() };
        objects.push((format!("hod{k}"), object(&dir, &format!("hod{k}"), &text, &options)));
    }
    for major in [22, 19] {
        if let Some(o) = llc(&dir, major, &format!("llc{major}"), &text) {
            objects.push((format!("llc{major}"), o));
        }
    }
    for (name, object) in &objects {
        let program = dir.join(format!("floats-{name}"));
        let ran = toolchain.link_and_run(linker, &["-pie"], &[&driver, object], &program);
        assert_eq!(exit_code(&ran), Some(0), "{name}:\n{}{}", ran.stdout_text(), ran.stderr_text());
    }
}

// ---- the instructions ----

/// Whether `code` contains `inst`'s bytes.
fn has(code: &[u8], inst: mc::Inst, what: &str) -> bool {
    let bytes = inst.bytes();
    let found = code.windows(bytes.len()).any(|w| w == bytes);
    assert!(found || what.is_empty(), "{what}: {bytes:02x?} not in {code:02x?}");
    found
}

/// The general registers a loaded value may be put in.
const POOL: [Reg; 5] = [Reg::Rbx, Reg::R12, Reg::R13, Reg::R14, Reg::R15];

/// §4.6: a `ptradd` fuses into the addressing mode of the load or store
/// that is its only use: the scaled index (sign-extended from 32 bits),
/// the constant offset of a stack slot (a frame access, no `lea`).
#[test]
fn ptradd_fuses_into_the_addressing_mode() {
    let rcx = Reg::Rcx.index().unwrap();
    let c = code(
        "define i64 @f(ptr %p, i64 %i) {\n %a = getelementptr i64, ptr %p, i64 %i\n %v = load i64, ptr %a\n ret i64 %v }",
    );
    let m = Mem::base_index(Reg::Rdi, rcx, Scale::S8, 0);
    assert!(
        POOL.iter().any(|&r| has(&c, mc::mov_rm(Size::Q, r, m), "")),
        "[rdi + rcx*8] in {c:02x?}"
    );
    assert!(!has(&c, mc::lea(Size::Q, Reg::Rcx, m), ""), "not computed by itself");
    let c = code(
        "define void @f(ptr %p, i32 %i, i16 %v) {\n %a = getelementptr i16, ptr %p, i32 %i\n store i16 %v, ptr %a\n ret void }",
    );
    has(&c, mc::movsxd_rr(Reg::Rcx, Reg::Rcx), "movsxd");
    let m = Mem::base_index(Reg::Rdi, rcx, Scale::S2, 0);
    assert!(POOL.iter().any(|&r| has(&c, mc::mov_mr(Size::W, m, r), "")), "store in {c:02x?}");
    // A three-term address with a scale no mode has: `imul`, then `lea`
    // to add, then one access.
    let c = code(
        "define i8 @f(ptr %p, i64 %i, i64 %j) {\n %a = getelementptr [3 x [12 x i8]], ptr %p, i64 %i, i64 %j, i64 5\n %v = load i8, ptr %a\n ret i8 %v }",
    );
    has(&c, mc::imul_rri(Size::Q, Reg::Rcx, Reg::Rcx, 36), "imul 36");
    has(&c, mc::imul_rri(Size::Q, Reg::Rdx, Reg::Rdx, 12), "imul 12");
    let rdx = Reg::Rdx.index().unwrap();
    has(&c, mc::lea(Size::Q, Reg::Rcx, Mem::base_index(Reg::Rcx, rdx, Scale::S1, 0)), "lea");
    let m = Mem::base_index(Reg::Rdi, rcx, Scale::S1, 5);
    assert!(POOL.iter().any(|&r| has(&c, mc::movzx_rm(Size::D, r, Size::B, m), "")));
    // Not fused (two uses): computed once with `lea`, then plain accesses.
    let c = code(
        "define i64 @f(ptr %p, i64 %i) {\n %a = getelementptr i64, ptr %p, i64 %i\n %v = load i64, ptr %a\n store i64 %v, ptr %a\n ret i64 %v }",
    );
    let m = Mem::base_index(Reg::Rdi, rcx, Scale::S8, 0);
    assert!(POOL.iter().any(|&r| has(&c, mc::lea(Size::Q, r, m), "")), "lea in {c:02x?}");
}

/// §2.2: a store of an odd width masks and writes exactly the store
/// size, as `llc` does: `and`, then `mov word`, `shr`, `mov byte` for
/// `i17`; no 4-byte store.
#[test]
fn an_odd_width_store_writes_its_store_size() {
    let c = code("define void @f(i17 %v, ptr %p) {\n store i17 %v, ptr %p\n ret void }");
    has(&c, mc::and_ri(Size::D, Reg::Rax, 0x1_ffff), "mask");
    has(&c, mc::mov_mr(Size::W, Mem::base(Reg::Rsi, 0), Reg::Rax), "word");
    has(&c, mc::shr_ri(Size::D, Reg::Rax, 16), "shr");
    has(&c, mc::mov_mr(Size::B, Mem::base(Reg::Rsi, 2), Reg::Rax), "byte");
    let dword = mc::mov_mr(Size::D, Mem::base(Reg::Rsi, 0), Reg::Rax);
    let count = |b: &[u8]| c.windows(b.len()).filter(|w| *w == b).count();
    let word = mc::mov_mr(Size::W, Mem::base(Reg::Rsi, 0), Reg::Rax);
    assert_eq!(count(dword.bytes()), count(word.bytes()), "only the word's own bytes");
    // A 7-byte load reads 4 + 2 + 1 bytes.
    let c = code("define i56 @f(ptr %p) {\n %v = load i56, ptr %p\n ret i56 %v }");
    has(&c, mc::movzx_rm(Size::D, Reg::Rax, Size::B, Mem::base(Reg::Rdi, 6)), "byte 6");
    has(&c, mc::movzx_rm(Size::D, Reg::Rdx, Size::W, Mem::base(Reg::Rdi, 4)), "word 4");
    // A `half` is two bytes each way.
    let c = code(
        "define void @f(ptr %p, ptr %q) {\n %v = load half, ptr %p\n store half %v, ptr %q\n ret void }",
    );
    assert!((8..14).any(|x| has(
        &c,
        mc::pinsrw_rmi(mc::Xmm::new(x).unwrap(), Mem::base(Reg::Rdi, 0), 0),
        ""
    )));
    has(&c, mc::mov_mr(Size::W, Mem::base(Reg::Rsi, 0), Reg::Rax), "store half");
}

/// A volatile load whose result is unused is still emitted, once.
#[test]
fn a_dead_volatile_load_is_emitted_once() {
    let count = |c: &[u8]| {
        POOL.iter()
            .map(|&r| {
                let b = mc::mov_rm(Size::Q, r, Mem::base(Reg::Rdi, 0)).bytes().to_vec();
                c.windows(b.len()).filter(|w| *w == b.as_slice()).count()
            })
            .sum::<usize>()
    };
    let c = code("define void @f(ptr %p) {\n %v = load volatile i64, ptr %p\n ret void }");
    assert_eq!(count(&c), 1, "{c:02x?}");
    let c = code("define void @f(ptr %p) {\n %v = load i64, ptr %p\n ret void }");
    assert_eq!(count(&c), 0, "a plain dead load is skipped");
}

// ---- refusals ----

/// What the group does not lower is refused by name.
#[test]
fn the_other_forms_are_refused_by_name() {
    assert_eq!(
        compile_ll(
            "define void @f(ptr %p, ptr %q) {\n %i = load <2 x i64>, ptr %q\n %r = getelementptr i8, ptr %p, <2 x i64> %i\n store <2 x ptr> %r, ptr %q\n ret void }"
        ),
        Err(CgError::unsupported(mem::PTRADD_VECTOR))
    );
}

/// A frame beyond what the prologue expresses is `limit:frame-size`; a
/// code model other than small is `x64:code-model`.
#[test]
fn the_limits_are_refused_by_name() {
    assert_eq!(
        compile_ll(
            "define void @f(i64 %x) {\n %s = alloca [4294967296 x i8]\n store i64 %x, ptr %s\n ret void }"
        ),
        Err(CgError::unsupported(hod_cg::LIMIT_FRAME_SIZE))
    );
    let (m, bodies) = parse(&format!(
        "{}@g = global i64 0\ndefine i64 @f() {{\n %v = load i64, ptr @g\n ret i64 %v }}",
        header()
    ));
    let options = Options { code_model: Some(hod_ir::CodeModel::Large), ..Options::default() };
    assert_eq!(
        chunk(&m, &bodies, &options).map(|_| ()),
        Err(CgError::unsupported(hod_x64::common::CODE_MODEL))
    );
}
