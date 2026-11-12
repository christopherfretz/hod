//! The memory group (WP-036, `spec/codegen.md` §5.2, §5.5.1 "Memory"):
//! `load`, `store`, `alloca`, `ptradd`, `llvm.stacksave` and
//! `llvm.stackrestore`.
//!
//! - `generated_functions_agree_with_the_interpreter`: straight-line
//!   functions of the group (`common::mem_gen`), many to one object, run
//!   from a C driver whose `%buf` ends at a page boundary (an access past
//!   a store size faults), compared byte for byte with `hod_interp`:
//!   plainly, with every stress option, position-independent, and the
//!   same module through `llc -O0 -global-isel=0 -fast-isel=0`.
//! - `the_stack_forms_hold_natively`: dynamic and over-aligned `alloca`,
//!   `stacksave`/`stackrestore` (which the interpreter does not run),
//!   checked by the C driver and against `llc`.
//! - the exact words of the fused addressing modes; the refusals.

mod common;
#[path = "common/mem_gen.rs"]
mod mem_gen;

use std::cell::RefCell;
use std::fmt::Write as _;
use std::process::Command;
use std::rc::Rc;
use std::time::Duration;

use common::linking::{Toolchain, exit_code};
use mem_gen::{BUF, Func, GLOBAL, generate, int_text};
use common::{assert_refused, chunk, code_words, compile_ll, ll_header, llc, parse, write_object};
use hod_a64::{compile_module, mem, target};
use hod_cg::{CgError, Options, RelocModel, Stress};
use hod_interp::{Interp, Program, Val};
use hod_mc_a64 as mc;
use hod_testutil::{Scratch, run_with};

// ---- the generated functions ----

/// The functions of `seeds`, named `m<seed>`.
fn batch(seeds: std::ops::Range<u64>) -> Vec<Func> {
    seeds.map(|seed| generate(&format!("m{seed}"), seed)).collect()
}

/// What hod compiles: the variables and the functions.
fn native_text(funcs: &[Func]) -> String {
    let mut text = ll_header(22);
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
            write!(args, ", {} {}", p.ty.name(), int_text(p.ty, p.value)).unwrap();
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
            let mut out = format!("f {}\nr {value:x}\n", f.name);
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
/// and its parameters with garbage above their widths; then the result
/// (masked to its width) and the regions are printed as `expected` does.
fn driver(funcs: &[Func]) -> String {
    let mut c = String::from(
        "#include <stdint.h>\n#include <stdio.h>\n#include <string.h>\n\
         #include <sys/mman.h>\n#include <unistd.h>\n\
         static void dump(const char *tag, const unsigned char *p, int n) {\n\
           printf(\"%s \", tag);\n  for (int i = 0; i < n; i++) printf(\"%02x\", p[i]);\n\
           printf(\"\\n\");\n}\n",
    );
    for f in funcs {
        let params: String = f.params.iter().map(|_| ", uint64_t").collect();
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
        let args: String = f.params.iter().map(|p| format!(", {}ULL", p.native())).collect();
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

/// The ways a batch is compiled: hod plainly, stressed, PIC; `llc`.
const MODES: [&str; 4] = ["plain", "stress", "pic", "llc"];

/// Runs `funcs` in every mode and returns, per mode, the names of the
/// functions whose output differs from `want`, with both outputs. `None`
/// without the tools.
fn run_modes(funcs: &[Func], want: &[String]) -> Option<Vec<(String, String)>> {
    let scratch = Scratch::new("a64-mem");
    let toolchain = Toolchain::host(&scratch)?;
    let linker = toolchain.linkers.first()?;
    let text = native_text(funcs);
    let (module, bodies) = parse(&text);
    let driver = toolchain.compile_c(&scratch, "driver", &driver(funcs), &[]);
    let mut wrong = Vec::new();
    for mode in MODES {
        let object = if mode == "llc" {
            let Some(object) = llc(&scratch, "llc", &text, &[]) else { continue };
            object
        } else {
            let options = Options {
                stress: if mode == "stress" { Stress::ALL } else { Stress::default() },
                reloc_model: if mode == "pic" { RelocModel::Pic } else { RelocModel::Static },
                ..Options::default()
            };
            let emitted = compile_module(&module, &bodies, &options)
                .unwrap_or_else(|e| panic!("{mode}: {e}\n{text}"));
            write_object(&scratch, &format!("{mode}.o"), &emitted)
        };
        let program = scratch.join(mode);
        toolchain.link_ok(linker, &[], &[&driver, &object], &program);
        let ran = run_with(&mut Command::new(&program), &[], Duration::from_mins(2));
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
            panic!("{what}: hod_a64 and the interpreter disagree\n{report}");
        }
        checked += funcs.len();
        start = end;
    }
    checked
}

/// Generated functions of the group agree with `hod_interp` in every
/// mode (and through `llc`).
#[test]
fn generated_functions_agree_with_the_interpreter() {
    let checked = check(0..1200, 400);
    eprintln!("a64 mem: {checked} functions agree in {} modes", MODES.len());
}

/// More seeds: `HOD_MEM_SEEDS=<n>` (default 10000).
#[test]
#[ignore = "a minute; run with --ignored"]
fn many_generated_functions_agree() {
    let seeds: u64 =
        std::env::var("HOD_MEM_SEEDS").ok().and_then(|t| t.parse().ok()).unwrap_or(10_000);
    let checked = check(1_000_000..1_000_000 + seeds, 1000);
    eprintln!("a64 mem: {checked} functions agree");
}

// ---- the stack forms, checked by the driver ----

const STACK: &str = r"
declare ptr @llvm.stacksave.p0()
declare void @llvm.stackrestore.p0(ptr)

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
; 12-byte element, over-aligned; both are written in full and read back.
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
  %y = load i64, ptr %end
  ret i64 %y
}

; Zero-sized allocas, a constant count outside the entry block, and one
; beyond 2^24 bytes (never touched).
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

; Over-aligned and zero-sized fixed slots; a slot past the reach of the
; scaled offsets.
define i64 @slots(ptr %out, i64 %x) {
entry:
  %a = alloca [8 x i8], align 128
  %z = alloca [0 x i8], align 64
  %far = alloca [70000 x i8], align 8
  store ptr %a, ptr %out
  %o1 = getelementptr ptr, ptr %out, i64 1
  store ptr %z, ptr %o1
  %f = getelementptr i8, ptr %far, i64 69992
  store i64 %x, ptr %f
  store i64 %x, ptr %a
  %y = load i64, ptr %f
  ret i64 %y
}
";

const STACK_DRIVER: &str = r#"
#include <stdint.h>
#include <stdio.h>
#define CHECK(c) do { if (!(c)) { printf("line %d: %s\n", __LINE__, #c); fails++; } } while (0)
void save_restore(uintptr_t *out, uint64_t n);
uint64_t dyn(uintptr_t *out, uint64_t n, uint64_t x);
void odd_sizes(uintptr_t *out, uint64_t zero);
uint64_t slots(uintptr_t *out, uint64_t x);
int main(void) {
    int fails = 0;
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
    }
    odd_sizes(out, 0xffffffff00000000ull);
    CHECK(out[0] % 32 == 0 && out[0] < here);
    CHECK(out[1] + 20 <= out[0] && out[1] % 8 == 0);
    CHECK(out[2] + 17000000 <= out[1]);
    CHECK(out[3] <= out[2]);
    CHECK(slots(out, 42) == 42);
    CHECK(out[0] % 128 == 0 && out[0] < here);
    CHECK(out[1] % 64 == 0);
    printf("checked, %d failures\n", fails);
    return fails;
}
"#;

/// Dynamic, over-aligned, zero-sized and large `alloca`s, the stack
/// intrinsics, over-aligned and far slots: hod's and `llc`'s objects
/// pass the same checks, by every linker.
#[test]
fn the_stack_forms_hold_natively() {
    let text = format!("{}{STACK}", ll_header(22));
    let (m, bodies) = parse(&text);
    let dir = Scratch::new("a64-mem-stack");
    let Some(toolchain) = Toolchain::host(&dir) else { return };
    let driver = toolchain.compile_c(&dir, "driver", STACK_DRIVER, &[]);
    let mut objects = Vec::new();
    for stress in [Stress::default(), Stress::ALL] {
        let options = Options { stress, ..Options::default() };
        let emitted = compile_module(&m, &bodies, &options).unwrap_or_else(|e| panic!("{e}"));
        objects.push(write_object(&dir, &format!("hod{}.o", objects.len()), &emitted));
    }
    objects.extend(llc(&dir, "llc", &text, &[]));
    for (k, object) in objects.iter().enumerate() {
        for linker in &toolchain.linkers {
            let program = dir.join(format!("stack-{k}-{}", linker.name));
            let ran = toolchain.link_and_run(linker, &[], &[&driver, object], &program);
            assert_eq!(
                exit_code(&ran),
                Some(0),
                "object {k}, {}:\n{}{}",
                linker.name,
                ran.stdout_text(),
                ran.stderr_text()
            );
        }
    }
}

// ---- the words ----

/// The code words of one `.ll` function.
fn words(function: &str) -> Vec<u32> {
    let (m, bodies) = parse(&format!("{}{function}", ll_header(22)));
    code_words(&m, &bodies, &Options::default())
}

fn has(words: &[u32], word: u32, what: &str) {
    assert!(words.contains(&word), "{what}: {word:08x} not in {words:08x?}");
}

/// §4.6: a `ptradd` fuses into the addressing mode of the load or store
/// that is its only use: the scaled register form, the `sxtw` form of a
/// 32-bit index, the constant offset of a stack slot.
#[test]
fn ptradd_fuses_into_the_addressing_mode() {
    use mc::{Index, Load, MemSize, Reg, RegSp};
    let x = |n: u8| Reg::new(n).unwrap();
    let w = words(
        "define i64 @f(ptr %p, i64 %i) {\n %a = getelementptr i64, ptr %p, i64 %i\n %v = load i64, ptr %a\n ret i64 %v }",
    );
    has(&w, mc::ldr_reg(Load::X, x(9).zr(), x(0).sp(), x(1).zr(), Index::Lsl, true), "lsl #3");
    let w = words(
        "define void @f(ptr %p, i32 %i, i16 %v) {\n %a = getelementptr i16, ptr %p, i32 %i\n store i16 %v, ptr %a\n ret void }",
    );
    has(&w, mc::str_reg(MemSize::H, x(2).zr(), x(0).sp(), x(1).zr(), Index::Sxtw, true), "sxtw #1");
    let w = words(
        "define void @f(i32 %v) {\n %s = alloca [16 x i8]\n %a = getelementptr i8, ptr %s, i64 12\n store i32 %v, ptr %a\n ret void }",
    );
    let fp = RegSp::new(29).unwrap();
    // The slot is at `fp + 16`, after the frame record.
    has(&w, mc::str_off(MemSize::W, x(0).zr(), fp, 28), "[x29, #28]");
    // Not fused (two uses): computed once, then `[x9]`-style accesses.
    let w = words(
        "define i64 @f(ptr %p, i64 %i) {\n %a = getelementptr i64, ptr %p, i64 %i\n %v = load i64, ptr %a\n store i64 %v, ptr %a\n ret i64 %v }",
    );
    assert!(!w.contains(&mc::ldr_reg(Load::X, x(9).zr(), x(0).sp(), x(1).zr(), Index::Lsl, true)));
}

/// §2.2: a store of an odd width masks and writes exactly the store
/// size, as `llc` does: `and` then `strh` and `strb` for `i17`.
#[test]
fn an_odd_width_store_writes_its_store_size() {
    let w = words("define void @f(i17 %v, ptr %p) {\n store i17 %v, ptr %p\n ret void }");
    let (x0, x1, x16) = (mc::Reg::X0, mc::Reg::X1, mc::Reg::X16);
    has(&w, mc::and_imm(mc::Size::W, x0.sp(), x0.zr(), 0x1_ffff), "mask");
    has(&w, mc::str_off(mc::MemSize::H, x0.zr(), x1.sp(), 0), "strh");
    has(&w, mc::lsr_imm(mc::Size::X, x16.zr(), x0.zr(), 16), "lsr");
    has(&w, mc::str_off(mc::MemSize::B, x16.zr(), x1.sp(), 2), "strb");
    assert!(!w.iter().any(|&word| word == mc::str_off(mc::MemSize::W, x0.zr(), x1.sp(), 0)));
}

/// A volatile load whose result is unused is still emitted, once.
#[test]
fn a_dead_volatile_load_is_emitted_once() {
    let ldr = mc::ldr_off(mc::Load::X, mc::Reg::X9.zr(), mc::Reg::X0.sp(), 0);
    let w = words("define void @f(ptr %p) {\n %v = load volatile i64, ptr %p\n ret void }");
    assert_eq!(w.iter().filter(|&&word| word == ldr).count(), 1, "{w:08x?}");
    let w = words("define void @f(ptr %p) {\n %v = load i64, ptr %p\n ret void }");
    assert!(!w.contains(&ldr), "a plain dead load is skipped");
}

// ---- refusals ----

/// What the group does not lower is refused by name.
#[test]
fn the_other_forms_are_refused_by_name() {
    assert_refused(&[
        ("define double @f(ptr %p) { %r = load double, ptr %p\n ret double %r }", mem::LOAD_FPR),
        ("define void @f(ptr %p, float %x) { store float %x, ptr %p\n ret void }", mem::STORE_FPR),
        (
            "define i64 @f(ptr %p) { %r = load atomic i64, ptr %p seq_cst, align 8\n ret i64 %r }",
            target::ATOMIC,
        ),
        (
            "define void @f(ptr %p) { store atomic i64 0, ptr %p release, align 8\n ret void }",
            target::ATOMIC,
        ),
        (
            "define <2 x ptr> @f(ptr %p, <2 x i64> %i) { %r = getelementptr i8, ptr %p, <2 x i64> %i\n ret <2 x ptr> %r }",
            mem::PTRADD_VECTOR,
        ),
    ]);
}

/// A frame beyond what the prologue expresses is the skeleton's
/// `limit:frame-size`; a code model other than small is `a64:code-model`.
#[test]
fn the_limits_are_refused_by_name() {
    assert_eq!(
        compile_ll(
            "define void @f(i64 %x) {\n %s = alloca [16777216 x i8]\n store i64 %x, ptr %s\n ret void }"
        ),
        Err(CgError::unsupported(hod_cg::LIMIT_FRAME_SIZE))
    );
    let (m, bodies) = parse(&format!(
        "{}@g = global i64 0\ndefine i64 @f() {{\n %v = load i64, ptr @g\n ret i64 %v }}",
        ll_header(22)
    ));
    let options = Options { code_model: Some(hod_ir::CodeModel::Large), ..Options::default() };
    assert_eq!(
        chunk(&m, &bodies, &options).map(|_| ()),
        Err(CgError::unsupported(hod_a64::common::CODE_MODEL))
    );
}

