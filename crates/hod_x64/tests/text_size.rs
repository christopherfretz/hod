//! Text size, the first cuts (WP-146, `spec/codegen.md` §5.6.15): the
//! save set is the callee-saved registers the body used, branches that
//! reach take their short form, and copies above 64 bytes are `rep
//! movsb`.
//!
//! - Unwinding through frames with a reduced save set, mixed objects in
//!   both directions: `llc` code that keeps a value in each of `rbx`,
//!   `r12`..`r15` calls hod functions that save none, some or all of
//!   them, an exception thrown by `llc` code that clobbers all five
//!   passes through them, and every register is intact after a return
//!   and after the catch; hod code that holds values across an `invoke`
//!   of that `llc` code finds them after the catch.
//! - Shapes over generated programs (`hod_interp::generate`, plainly and
//!   with every stress option): every callee-saved register a function
//!   mentions is pushed by its prologue and popped by each exit, no
//!   `rel32` branch is left whose label a `rel8` reaches, no run of more
//!   than four 16-byte moves through `xmm15`.
//! - Copies above 64 bytes (`byval` arguments, `Mem`-class values and
//!   their `phi`s, at sizes around the threshold and odd ones) against
//!   `llc` under the emulator, with every argument register live across
//!   the copy, hod and `llc` objects mixed in both directions.
//! - Short frame displacements (WP-151, §5.6.17): no `[rbp + disp32]`
//!   whose displacement fits a byte in any generated program; locals on
//!   both sides of `[rbp - 128]`, accessed by every instruction class of
//!   the frame (immediates after the displacement included), against
//!   `llc` plainly and stressed.

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

#[path = "common/int_gen.rs"]
mod int_gen;

#[path = "../../hod_interp/tests/llvm_text/mod.rs"]
#[allow(dead_code, clippy::all, reason = "the LLVM oracle's printer, used as it is")]
mod llvm_text;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hod_cg::Stress;
use hod_interp::generate::{Options as GenOptions, generate};
use hod_ir::TargetSpec;
use hod_testutil::{Scratch, run_with, tool};
use hod_x64::compile_module;
use int_gen::{ll_header, llc, parse, pic};
use linking::{Toolchain, exit_code};

/// hod's object of `.ll` text.
fn hod(dir: &Scratch, name: &str, text: &str, stress: Stress) -> PathBuf {
    let (module, bodies) = parse(text);
    let bytes = hod_x64::compile_to_object(&module, &bodies, &pic(stress))
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    dir.write(&format!("{name}.o"), bytes)
}

/// Links `inputs` with the first linker and runs the program: its output.
fn run(toolchain: &Toolchain, dir: &Scratch, name: &str, inputs: &[&Path]) -> String {
    let linker = toolchain.linkers.first().expect("a linker");
    let exe = dir.join(name);
    let mut all = inputs.to_vec();
    all.push(Path::new("-lstdc++"));
    toolchain.link_ok(linker, &[], &all, &exe);
    let ran = run_with(&mut toolchain.runner_command(&exe), &[], Duration::from_mins(5));
    assert!(
        exit_code(&ran) == Some(0),
        "{name}: exit {:?}\n{}{}",
        exit_code(&ran),
        ran.stdout_text(),
        ran.stderr_text()
    );
    ran.stdout_text()
}

// ---- the save set ----

/// `llc` code: `keep_all(f, arg)` puts a value in each of `rbx`,
/// `r12`..`r15`, invokes `f(arg)` and stores the five registers after the
/// return (or the catch) to `out`; 1 after a return, 2 after a catch.
/// `clobber_throw(v)` clobbers all five (so it saves them) and throws `v`
/// through `cxx_throw`.
const LLC_SIDE: &str = r#"
declare i32 @__gxx_personality_v0(...)
declare ptr @__cxa_begin_catch(ptr)
declare void @__cxa_end_catch()
declare void @cxx_throw(i32)

define i32 @keep_all(ptr %f, ptr %arg, ptr %out) personality ptr @__gxx_personality_v0 {
entry:
  call void asm sideeffect "movq $$0x1111, %rbx\0Amovq $$0x2222, %r12\0Amovq $$0x3333, %r13\0Amovq $$0x4444, %r14\0Amovq $$0x5555, %r15", "~{rbx},~{r12},~{r13},~{r14},~{r15}"()
  invoke void %f(ptr %arg) to label %ok unwind label %pad
ok:
  call void asm sideeffect "movq %rbx, 0($0)\0Amovq %r12, 8($0)\0Amovq %r13, 16($0)\0Amovq %r14, 24($0)\0Amovq %r15, 32($0)", "r,~{rbx},~{r12},~{r13},~{r14},~{r15},~{memory}"(ptr %out)
  ret i32 1
pad:
  %lp = landingpad { ptr, i32 } catch ptr null
  call void asm sideeffect "movq %rbx, 0($0)\0Amovq %r12, 8($0)\0Amovq %r13, 16($0)\0Amovq %r14, 24($0)\0Amovq %r15, 32($0)", "r,~{rbx},~{r12},~{r13},~{r14},~{r15},~{memory}"(ptr %out)
  %e = extractvalue { ptr, i32 } %lp, 0
  %x = call ptr @__cxa_begin_catch(ptr %e)
  call void @__cxa_end_catch()
  ret i32 2
}

define void @clobber_throw(i32 %v) {
  call void asm sideeffect "movq $$-1, %rbx\0Amovq $$-1, %r12\0Amovq $$-1, %r13\0Amovq $$-1, %r14\0Amovq $$-1, %r15", "~{rbx},~{r12},~{r13},~{r14},~{r15}"()
  call void @cxx_throw(i32 %v)
  ret void
}

define void @clobber_return(ptr %p) {
  call void asm sideeffect "movq $$-1, %rbx\0Amovq $$-1, %r12\0Amovq $$-1, %r13\0Amovq $$-1, %r14\0Amovq $$-1, %r15", "~{rbx},~{r12},~{r13},~{r14},~{r15}"()
  ret void
}
"#;

/// hod's functions, each called by `keep_all`: one that uses no
/// callee-saved register, one that uses a few, one that holds five
/// values across the call; each lets the exception of `clobber_throw`
/// pass. `hod_catches` catches it; `hod_holds` holds values across an
/// `invoke` of `clobber_throw` and of `clobber_return` and stores their
/// sum to `p` after each.
const HOD_SIDE: &str = r"
declare void @clobber_throw(i32)
declare void @clobber_return(ptr)
declare i32 @__gxx_personality_v0(...)
declare ptr @__cxa_begin_catch(ptr)
declare void @__cxa_end_catch()

define void @hod_none(ptr %p) {
  call void @clobber_throw(i32 7)
  ret void
}

define void @hod_some(ptr %p) {
  %v = load i32, ptr %p
  %w = add i32 %v, 1
  call void @clobber_throw(i32 %w)
  store i32 %w, ptr %p
  ret void
}

define void @hod_all(ptr %p) {
  %a = load i32, ptr %p
  %b = add i32 %a, 1
  %c = add i32 %a, 2
  %d = add i32 %a, 3
  %e = add i32 %a, 4
  %f = add i32 %a, 5
  call void @clobber_throw(i32 %b)
  %s1 = add i32 %b, %c
  %s2 = add i32 %s1, %d
  %s3 = add i32 %s2, %e
  %s4 = add i32 %s3, %f
  store i32 %s4, ptr %p
  ret void
}

define void @hod_returns(ptr %p) {
  %a = load i32, ptr %p
  %b = add i32 %a, 1
  %c = add i32 %a, 2
  call void @clobber_return(ptr %p)
  %s = add i32 %b, %c
  store i32 %s, ptr %p
  ret void
}

define void @hod_catches(ptr %p) personality ptr @__gxx_personality_v0 {
entry:
  invoke void @clobber_throw(i32 7) to label %ok unwind label %pad
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

define void @hod_holds(ptr %p) personality ptr @__gxx_personality_v0 {
entry:
  %a = load i32, ptr %p
  %b = mul i32 %a, 3
  %c = add i32 %a, 100
  %d = xor i32 %a, 21845
  %e = sub i32 1000, %a
  call void @clobber_return(ptr %p)
  %s1 = add i32 %b, %c
  %s2 = add i32 %s1, %d
  %s3 = add i32 %s2, %e
  store i32 %s3, ptr %p
  invoke void @clobber_throw(i32 1) to label %ok unwind label %pad
ok:
  ret void
pad:
  %lp = landingpad { ptr, i32 } catch ptr null
  %x0 = extractvalue { ptr, i32 } %lp, 0
  %x = call ptr @__cxa_begin_catch(ptr %x0)
  call void @__cxa_end_catch()
  %t1 = add i32 %b, %c
  %t2 = add i32 %t1, %d
  %t3 = add i32 %t2, %e
  %t4 = add i32 %t3, %s3
  store i32 %t4, ptr %p
  ret void
}
";

const SAVE_DRIVER: &str = r#"
#include <cstdio>
extern "C" {
int keep_all(void (*)(int*), int*, long*);
void hod_none(int*);
void hod_some(int*);
void hod_all(int*);
void hod_returns(int*);
void hod_catches(int*);
void hod_holds(int*);
void cxx_throw(int v) { throw v; }
}
static void one(const char* name, void (*f)(int*)) {
  int x = 5;
  long regs[5] = {0, 0, 0, 0, 0};
  int how = keep_all(f, &x, regs);
  int ok = regs[0] == 0x1111 && regs[1] == 0x2222 && regs[2] == 0x3333 && regs[3] == 0x4444 &&
           regs[4] == 0x5555;
  std::printf("%s: %d %d %d\n", name, how, ok, x);
}
int main() {
  one("none", hod_none);
  one("some", hod_some);
  one("all", hod_all);
  one("returns", hod_returns);
  one("catches", hod_catches);
  one("holds", hod_holds);
  return 0;
}
"#;

/// What `hod_holds` leaves for `x`: the sum `3x + (x + 100) + (x ^
/// 0x5555) + (1000 - x)` stored, then twice it after the catch.
fn holds(x: i32) -> i32 {
    let s = 3 * x + (x + 100) + (x ^ 0x5555) + (1000 - x);
    2 * s
}

/// §5.6.15: frames that save only what they use unwind and return
/// correctly between hod and `llc` objects in both directions.
#[test]
fn a_reduced_save_set_keeps_every_callee_saved_register() {
    let dir = Scratch::new("x64-save-set");
    let Some(toolchain) = Toolchain::x86_64(&dir) else { return };
    let Some(driver) = toolchain.compile_cxx(&dir, "driver", SAVE_DRIVER, &["-fPIC"]) else {
        return;
    };
    let want = format!(
        "none: 2 1 5\nsome: 2 1 5\nall: 2 1 5\nreturns: 1 1 13\ncatches: 1 1 99\nholds: 1 1 {}\n",
        holds(5)
    );
    let text = format!("{}{HOD_SIDE}", ll_header());
    let llc_text = format!("{}{LLC_SIDE}", ll_header());
    let mut ran = 0;
    for major in hod_testutil::LLVM_MAJORS {
        let Some(llc_side) = llc(&dir, major, &format!("llc-side{major}"), &llc_text) else {
            continue;
        };
        for (mode, stress) in [("plain", Stress::default()), ("stress", Stress::ALL)] {
            let object = hod(&dir, &format!("hod-{mode}"), &text, stress);
            let name = format!("save-{mode}-{major}");
            let out = run(&toolchain, &dir, &name, &[&driver, &llc_side, &object]);
            assert_eq!(out, want, "{name}");
            ran += 1;
        }
        // The same functions by `llc`: the expectation is right.
        if let Some(both) = llc(&dir, major, &format!("llc-hod{major}"), &text) {
            let out =
                run(&toolchain, &dir, &format!("save-llc-{major}"), &[&driver, &llc_side, &both]);
            assert_eq!(out, want, "llc {major}");
        }
    }
    eprintln!("{ran} mixed programs");
    // The save sets really differ: `hod_none` pushes nothing but `rbp`.
    let object = hod(&dir, "shape", &text, Stress::default());
    if let Some(funcs) = disassemble(&object) {
        let none = &funcs.iter().find(|(n, _)| n == "hod_none").expect("hod_none").1;
        assert_eq!(pushes(none), Vec::<String>::new(), "{none:#?}");
        let all = &funcs.iter().find(|(n, _)| n == "hod_all").expect("hod_all").1;
        assert!(pushes(all).len() >= 2, "{all:#?}");
    }
}

// ---- shapes over generated programs ----

/// One instruction of `llvm-objdump -d`: its offset, bytes and text.
#[derive(Clone, Debug)]
struct Line {
    at: u64,
    bytes: Vec<u8>,
    text: String,
    /// A relocation applies to it.
    reloc: bool,
}

/// `llvm-objdump -d` (Intel syntax) of an object: per symbol, its
/// instructions as `mnemonic operands`.
fn disassemble(object: &Path) -> Option<Vec<(String, Vec<String>)>> {
    let funcs = disassemble_raw(object)?;
    Some(
        funcs
            .into_iter()
            .map(|(n, lines)| (n, lines.into_iter().map(|l| l.text).collect()))
            .collect(),
    )
}

/// [`disassemble`] with each instruction's offset and bytes.
fn disassemble_raw(object: &Path) -> Option<Vec<(String, Vec<Line>)>> {
    let objdump = tool("llvm-objdump")?;
    let mut command = Command::new(objdump);
    command.args(["-dr", "--x86-asm-syntax=intel"]).arg(object);
    let outcome = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "llvm-objdump: {}", outcome.stderr_text());
    let mut funcs: Vec<(String, Vec<Line>)> = Vec::new();
    for line in outcome.stdout_text().lines() {
        if line.contains("R_X86_64_") {
            if let Some(last) = funcs.last_mut().and_then(|(_, l)| l.last_mut()) {
                last.reloc = true;
            }
            continue;
        }
        if let Some(name) = line.split_once(" <").and_then(|(_, l)| l.strip_suffix(">:")) {
            funcs.push((name.to_owned(), Vec::new()));
            continue;
        }
        let Some((at, rest)) = line.trim_start().split_once(':') else { continue };
        let Ok(at) = u64::from_str_radix(at.trim(), 16) else { continue };
        let mut bytes = Vec::new();
        let mut text = String::new();
        let rest = rest.trim_start();
        let mut fields = rest.split(' ').peekable();
        while let Some(&f) = fields.peek() {
            if f.len() == 2
                && let Ok(b) = u8::from_str_radix(f, 16)
            {
                bytes.push(b);
                fields.next();
            } else {
                break;
            }
        }
        for f in fields {
            text.push_str(f);
            text.push(' ');
        }
        let text =
            text.split('#').next().unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" ");
        if bytes.is_empty() {
            continue;
        }
        if text.is_empty() {
            // A continuation line of a long instruction.
            if let Some(last) = funcs.last_mut().and_then(|(_, l)| l.last_mut()) {
                last.bytes.extend(bytes);
            }
            continue;
        }
        if let Some((_, lines)) = funcs.last_mut() {
            lines.push(Line { at, bytes, text, reloc: false });
        }
    }
    for (_, lines) in &mut funcs {
        while lines.last().is_some_and(|l| l.text.starts_with("nop") || l.text.starts_with("int3"))
        {
            lines.pop();
        }
    }
    Some(funcs)
}

/// The callee-saved registers of the pool, by their 64-bit names.
const CALLEE_SAVED: [&str; 5] = ["rbx", "r12", "r13", "r14", "r15"];

/// Whether `text` names `reg` (`rbx`, `r12`) at any width.
fn names(text: &str, reg: &str) -> bool {
    let forms: Vec<String> = if reg == "rbx" {
        ["rbx", "ebx", "bx", "bl"].iter().map(|s| (*s).to_owned()).collect()
    } else {
        ["", "d", "w", "b"].iter().map(|s| format!("{reg}{s}")).collect()
    };
    text.split(|c: char| !c.is_ascii_alphanumeric()).any(|word| forms.iter().any(|f| f == word))
}

/// The registers the prologue pushes after `mov rbp, rsp`.
fn pushes(insts: &[String]) -> Vec<String> {
    insts.iter().skip(2).map_while(|i| i.strip_prefix("push ").map(str::to_owned)).collect()
}

/// Whether the branch shape is checked (cut 2 of §5.6.15).
const SHORT_BRANCHES: bool = true;

/// Whether the copy shape is checked (cut 3 of §5.6.15).
const LOOPED_COPIES: bool = true;

/// Whether the frame-operand shape is checked (§5.6.17).
const SHORT_FRAME: bool = true;

/// A memory operand `xmmword ptr [base + disp]`: its base and
/// displacement.
type Place = (String, i64);

/// The [`Place`] of an operand as `llvm-objdump` prints it.
fn place(operand: &str) -> Option<Place> {
    let inner = operand.strip_prefix("xmmword ptr [")?.strip_suffix(']')?;
    let mut words = inner.split(' ');
    let base = words.next()?.to_owned();
    let disp = match (words.next(), words.next()) {
        (None, _) => 0,
        (Some(sign), Some(n)) => {
            let n = i64::from_str_radix(n.strip_prefix("0x")?, 16).ok()?;
            if sign == "-" { -n } else { n }
        }
        _ => return None,
    };
    Some((base, disp))
}

/// What [`check_shapes`] counted.
#[derive(Default, Debug)]
struct Shapes {
    functions: usize,
    pushes: usize,
    long_branches: usize,
    short_branches: usize,
    /// `[rbp ± d]` operands with a displacement that fits a byte
    /// (§5.6.17).
    disp8: usize,
    /// `[rbp ± d]` operands with one that does not.
    disp32: usize,
}

/// §5.6.15 over one object's functions; panics on a violation.
fn check_shapes(object: &Path, shapes: &mut Shapes) {
    let Some(funcs) = disassemble_raw(object) else { return };
    for (name, lines) in &funcs {
        let texts: Vec<String> = lines.iter().map(|l| l.text.clone()).collect();
        if texts.first().map(String::as_str) != Some("push rbp") {
            continue;
        }
        shapes.functions += 1;
        let pushed = pushes(&texts);
        shapes.pushes += pushed.len();
        // Every callee-saved register the body names is pushed.
        let body = &texts[2 + pushed.len()..];
        for reg in CALLEE_SAVED {
            if body.iter().any(|t| names(t, reg)) {
                assert!(
                    pushed.iter().any(|p| p == reg),
                    "{name}: {reg} used, not saved\n{texts:#?}"
                );
            }
        }
        // Each exit pops them in reverse, after `lea rsp, [rbp - 8n]`.
        for (i, t) in texts.iter().enumerate() {
            if t != "pop rbp" {
                continue;
            }
            let popped: Vec<&str> =
                texts[..i].iter().rev().map_while(|t| t.strip_prefix("pop ")).collect();
            // Read backward from `pop rbp`: the push order.
            let want: Vec<&str> = pushed.iter().map(String::as_str).collect();
            assert_eq!(popped, want, "{name}: pops\n{texts:#?}");
        }
        // No `rel32` branch whose label a `rel8` reaches.
        for line in lines.iter().filter(|_| SHORT_BRANCHES) {
            let b = &line.bytes;
            let (long, disp) = match b.as_slice() {
                [0xe9, d @ ..] if d.len() == 4 => (5, i32::from_le_bytes([d[0], d[1], d[2], d[3]])),
                [0x0f, op, d @ ..] if (0x80..0x90).contains(op) && d.len() == 4 => {
                    (6, i32::from_le_bytes([d[0], d[1], d[2], d[3]]))
                }
                [0xeb | 0x70..=0x7f, _] => {
                    shapes.short_branches += 1;
                    continue;
                }
                _ => continue,
            };
            // A branch through a relocation (a tail call) has no
            // displacement yet.
            if line.reloc {
                continue;
            }
            shapes.long_branches += 1;
            // As a short branch: from the end of two bytes.
            let short = i64::from(disp) + long - 2;
            assert!(
                i8::try_from(short).is_err(),
                "{name}+{:#x}: {} could be short ({short})\n{texts:#?}",
                line.at,
                line.text
            );
        }
        // §5.6.17: no `[rbp + disp32]` whose displacement fits a byte.
        for line in lines.iter().filter(|_| SHORT_FRAME) {
            let Some(disp) = rbp_disp(&line.text) else { continue };
            if i8::try_from(disp).is_ok() {
                shapes.disp8 += 1;
                assert!(
                    !has_rbp_disp32(&line.bytes, disp),
                    "{name}+{:#x}: {} has a disp32 that fits a byte\n{texts:#?}",
                    line.at,
                    line.text
                );
            } else {
                shapes.disp32 += 1;
            }
        }
        // No run of more than four 16-byte moves through `xmm15` that
        // copy consecutive addresses: one copy above 64 bytes.
        let mut run = 0;
        let mut last: Option<(Place, Place)> = None;
        for pair in texts.windows(2).filter(|_| LOOPED_COPIES) {
            let load = pair[0].strip_prefix("movups xmm15, ").and_then(place);
            let store = pair[1]
                .strip_prefix("movups ")
                .and_then(|t| t.strip_suffix(", xmm15"))
                .and_then(place);
            let (Some(load), Some(store)) = (load, store) else { continue };
            let next = last.is_some_and(|(l, s)| {
                (l.0 == load.0 && l.1 + 16 == load.1) && (s.0 == store.0 && s.1 + 16 == store.1)
            });
            run = if next { run + 1 } else { 1 };
            last = Some((load, store));
            assert!(run <= 4, "{name}: an unrolled copy above 64 bytes\n{texts:#?}");
        }
    }
}

/// The functions of a batch of generated programs as `.ll` text (as
/// `tests/diff.rs` builds them).
fn generated_text(seeds: std::ops::Range<u64>) -> String {
    let options = GenOptions {
        target: TargetSpec::x86_64_linux_gnu(),
        block_addresses: true,
        exceptions: false,
        vectors: false,
        ..GenOptions::default()
    };
    let mut file = llvm_text::File::new();
    for seed in seeds {
        let program = generate(seed, &options);
        // Programs a landed group refuses by name are left out.
        if compile_module(&program.module, &program.bodies, &pic(Stress::default())).is_ok() {
            file.add(seed, &program);
        }
    }
    let text = file.finish(TargetSpec::x86_64_linux_gnu().triple());
    let source = text.find("define i64 @source()").expect("the oracle's source");
    let after = source + text[source..].find("\n}\n").expect("the end of source") + 3;
    let main = text.find("define i32 @main()").expect("the oracle's main");
    let mut out = ll_header();
    out.push_str("declare void @sink(i64)\ndeclare i64 @source()\n");
    out.push_str(&text[after..main]);
    out
}

/// §5.6.15 on generated programs, plain and stressed (the stress pool
/// has fewer registers, `far` and the coroutine frame change the
/// prologue).
#[test]
fn generated_programs_have_the_small_shapes() {
    let dir = Scratch::new("x64-text-shapes");
    let text = generated_text(0..300);
    let mut shapes = Shapes::default();
    for (mode, stress) in [("plain", Stress::default()), ("stress", Stress::ALL)] {
        let (module, bodies) = parse(&text);
        let bytes = hod_x64::compile_to_object(&module, &bodies, &pic(stress))
            .unwrap_or_else(|e| panic!("{mode}: {e}"));
        let object = dir.write(&format!("gen-{mode}.o"), bytes);
        check_shapes(&object, &mut shapes);
    }
    eprintln!("{shapes:?}");
    assert!(shapes.functions > 100, "{shapes:?}");
    assert!(shapes.pushes < 5 * shapes.functions, "{shapes:?}");
    assert!(!SHORT_BRANCHES || shapes.short_branches > 0, "{shapes:?}");
    assert!(!SHORT_FRAME || shapes.disp8 > 0, "{shapes:?}");
}

// ---- copies above 64 bytes ----

/// The sizes copied: around the inline limit, odd, and large.
const SIZES: [u32; 9] = [63, 64, 65, 72, 100, 127, 128, 200, 4099];

/// Functions passing a `byval` struct of each size after six register
/// arguments (live in `rdi`..`r9` while the copy is made), receiving it,
/// and moving a `Mem` value of each size through a `phi` and a store;
/// the callees `sumN` are defined when `sums`, the callers `callN` when
/// `calls`, each declared otherwise.
fn copy_ir(sums: bool, calls: bool) -> String {
    let mut ir = String::new();
    for &n in &SIZES {
        let ty = format!("[{n} x i8]");
        let _ = writeln!(ir, "%s{n} = type {{ {ty} }}");
    }
    for &n in &SIZES {
        // The callee: a checksum of the bytes and of the six integers.
        if sums {
            let _ = write!(
                ir,
                r"
define i64 @sum{n}(i64 %a, i64 %b, i64 %c, i64 %d, i64 %e, i64 %f, ptr byval(%s{n}) align 8 %s) {{
entry:
  br label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %i1, %loop ]
  %acc = phi i64 [ 0, %entry ], [ %acc2, %loop ]
  %p = getelementptr i8, ptr %s, i64 %i
  %v = load i8, ptr %p
  %w = zext i8 %v to i64
  %m = mul i64 %acc, 31
  %acc2 = add i64 %m, %w
  %i1 = add i64 %i, 1
  %done = icmp eq i64 %i1, {n}
  br i1 %done, label %out, label %loop
out:
  %x1 = mul i64 %acc2, 7
  %x2 = add i64 %x1, %a
  %x3 = add i64 %x2, %b
  %x4 = add i64 %x3, %c
  %x5 = add i64 %x4, %d
  %x6 = add i64 %x5, %e
  %x7 = add i64 %x6, %f
  ret i64 %x7
}}
"
            );
        } else {
            let _ = writeln!(
                ir,
                "declare i64 @sum{n}(i64, i64, i64, i64, i64, i64, ptr byval(%s{n}) align 8)"
            );
        }
        // The caller: a struct value through a `phi` and a store, then
        // passed `byval`.
        if calls {
            let _ = write!(
                ir,
                r"
define i64 @call{n}(ptr %src, ptr %alt, i1 %which, i64 %a) {{
entry:
  %x = load %s{n}, ptr %src
  %y = load %s{n}, ptr %alt
  br i1 %which, label %one, label %two
one:
  br label %join
two:
  br label %join
join:
  %v = phi %s{n} [ %x, %one ], [ %y, %two ]
  %tmp = alloca %s{n}
  store %s{n} %v, ptr %tmp
  %b = add i64 %a, 1
  %c = add i64 %a, 2
  %d = add i64 %a, 3
  %e = add i64 %a, 4
  %f = add i64 %a, 5
  %r = call i64 @sum{n}(i64 %a, i64 %b, i64 %c, i64 %d, i64 %e, i64 %f, ptr byval(%s{n}) align 8 %tmp)
  ret i64 %r
}}
"
            );
        }
    }
    ir
}

fn copy_driver() -> String {
    let mut c = String::from("#include <stdio.h>\n#include <stdint.h>\n");
    for &n in &SIZES {
        let _ = writeln!(c, "int64_t call{n}(const void*, const void*, _Bool, int64_t);");
    }
    let _ = writeln!(c, "static unsigned char a[5000], b[5000];");
    c.push_str("int main(void) {\n  for (int i = 0; i < 5000; i++) { a[i] = (unsigned char)(i * 7 + 1); b[i] = (unsigned char)(i * 13 + 5); }\n");
    for &n in &SIZES {
        let _ = writeln!(
            c,
            "  printf(\"{n}: %lld %lld\\n\", (long long)call{n}(a, b, 1, 10), (long long)call{n}(a, b, 0, 20));"
        );
    }
    c.push_str("  return 0;\n}\n");
    c
}

/// §5.6.15: copies above 64 bytes are `rep movsb` with `rcx`, `rsi` and
/// `rdi` kept; hod and `llc` agree, mixed in both directions, plainly
/// and stressed.
#[test]
fn copies_above_64_bytes_agree_with_llc() {
    let dir = Scratch::new("x64-copies");
    let Some(toolchain) = Toolchain::x86_64(&dir) else { return };
    let driver = toolchain.compile_c(&dir, "driver", &copy_driver(), &[]);
    let header = ll_header();
    let whole = format!("{header}{}", copy_ir(true, true));
    let calls = format!("{header}{}", copy_ir(false, true));
    let sums = format!("{header}{}", copy_ir(true, false));
    let mut want = None;
    for major in hod_testutil::LLVM_MAJORS {
        let Some(by_llc) = llc(&dir, major, &format!("llc-whole{major}"), &whole) else { continue };
        let out = run(&toolchain, &dir, &format!("copy-llc{major}"), &[&driver, &by_llc]);
        assert_eq!(*want.get_or_insert_with(|| out.clone()), out, "llc {major}");
        let llc_calls = llc(&dir, major, &format!("llc-calls{major}"), &calls).expect("llc");
        let llc_sums = llc(&dir, major, &format!("llc-sums{major}"), &sums).expect("llc");
        for (mode, stress) in [("plain", Stress::default()), ("stress", Stress::ALL)] {
            let hod_whole = hod(&dir, &format!("hod-whole-{mode}"), &whole, stress);
            let hod_calls = hod(&dir, &format!("hod-calls-{mode}"), &calls, stress);
            let hod_sums = hod(&dir, &format!("hod-sums-{mode}"), &sums, stress);
            for (what, inputs) in [
                ("hod", vec![driver.as_path(), &hod_whole]),
                ("hod-calls", vec![driver.as_path(), &hod_calls, &llc_sums]),
                ("hod-sums", vec![driver.as_path(), &llc_calls, &hod_sums]),
            ] {
                let out = run(&toolchain, &dir, &format!("copy-{what}-{mode}-{major}"), &inputs);
                assert_eq!(Some(&out), want.as_ref(), "{what} {mode} {major}");
            }
        }
    }
    // The shape: one `rep movsb` per copy above 64 bytes, none below.
    let object = hod(&dir, "copy-shape", &whole, Stress::default());
    if let Some(funcs) = disassemble(&object) {
        let call63 = &funcs.iter().find(|(n, _)| n == "call63").expect("call63").1;
        assert!(!call63.iter().any(|i| i.contains("rep movsb")), "{call63:#?}");
        let call65 = &funcs.iter().find(|(n, _)| n == "call65").expect("call65").1;
        assert!(call65.iter().any(|i| i.contains("rep movsb")), "{call65:#?}");
    }
}

// ---- frame operands (§5.6.17) ----

/// The displacement of a plain `[rbp]` / `[rbp ± d]` operand of an
/// instruction as `llvm-objdump` prints it; `None` for any other operand
/// (an index, another base, none).
fn rbp_disp(text: &str) -> Option<i64> {
    let at = text.find("[rbp")?;
    let inner = text[at + 4..].split(']').next()?.trim();
    if inner.is_empty() {
        return Some(0);
    }
    let (sign, n) = inner.split_once(' ')?;
    let n = i64::from_str_radix(n.trim().strip_prefix("0x")?, 16).ok()?;
    match sign {
        "-" => Some(-n),
        "+" => Some(n),
        _ => None,
    }
}

/// Whether `bytes` hold the `ModRM` of `[rbp + disp32]` followed by a
/// field of value `disp`.
fn has_rbp_disp32(bytes: &[u8], disp: i64) -> bool {
    bytes.windows(5).any(|w| {
        w[0] & 0xc7 == 0x85 && i64::from(i32::from_le_bytes([w[1], w[2], w[3], w[4]])) == disp
    })
}

/// The padding bytes of [`frame_ir`]'s functions: the padding is the
/// last slot, nearest `rbp` (slots are laid out upward in order), so
/// the slots before it move away from `rbp` with it and cross the
/// 128-byte boundary one after the other.
const PADS: [u32; 32] = {
    let mut pads = [1; 32];
    let mut i = 1;
    while i < 32 {
        pads[i] = 4 * i as u32;
        i += 1;
    }
    pads
};

/// The operations of [`frame_ir`], each a small function over a few
/// locals (so that its spills do not push them all past `[rbp - 128]`):
/// its name, its slots (the padding follows them) and its body, which
/// may use `%x` and the padding `%pad` of `{k}` bytes.
const FRAME_OPS: [(&str, &str, &str); 7] = [
    (
        // Two 8-byte stores read back at their odd bytes: a byte whose
        // displacement is written wrong reads another byte than the
        // store wrote (the odd displacements around -128 are met).
        "bytes",
        "%m = alloca [16 x i8], align 8",
        r"store i64 %x, ptr %m
  %hi = getelementptr i8, ptr %m, i64 8
  %y = mul i64 %x, 40503
  store i64 %y, ptr %hi
  %p1 = getelementptr i8, ptr %m, i64 1
  %b1 = load i8, ptr %p1
  %z1 = zext i8 %b1 to i64
  %p3 = getelementptr i8, ptr %m, i64 3
  %b3 = load i8, ptr %p3
  %z3 = zext i8 %b3 to i64
  %p5 = getelementptr i8, ptr %m, i64 5
  %b5 = load i8, ptr %p5
  %z5 = zext i8 %b5 to i64
  %p7 = getelementptr i8, ptr %m, i64 7
  %b7 = load i8, ptr %p7
  %z7 = zext i8 %b7 to i64
  %p9 = getelementptr i8, ptr %m, i64 9
  %b9 = load i8, ptr %p9
  %z9 = zext i8 %b9 to i64
  %p11 = getelementptr i8, ptr %m, i64 11
  %b11 = load i8, ptr %p11
  %z11 = zext i8 %b11 to i64
  %p13 = getelementptr i8, ptr %m, i64 13
  %b13 = load i8, ptr %p13
  %z13 = zext i8 %b13 to i64
  %p15 = getelementptr i8, ptr %m, i64 15
  %b15 = load i8, ptr %p15
  %z15 = zext i8 %b15 to i64
  %m3 = mul i64 %z1, 263
  %s3 = add i64 %m3, %z3
  %m5 = mul i64 %s3, 263
  %s5 = add i64 %m5, %z5
  %m7 = mul i64 %s5, 263
  %s7 = add i64 %m7, %z7
  %m9 = mul i64 %s7, 263
  %s9 = add i64 %m9, %z9
  %m11 = mul i64 %s9, 263
  %s11 = add i64 %m11, %z11
  %m13 = mul i64 %s11, 263
  %s13 = add i64 %m13, %z13
  %m15 = mul i64 %s13, 263
  %s15 = add i64 %m15, %z15
  ret i64 %s15",
    ),
    (
        // Stores of immediates of each width (the immediate follows the
        // displacement), loads by `mov`, `movzx`, `movsx`, a store of a
        // register; `%c` is at an odd offset, so that some function
        // addresses `[rbp - 129]`.
        "imm",
        "%q = alloca i64\n  %d = alloca i32\n  %w = alloca i16\n  %b = alloca i8\n  %c = alloca i8",
        r"store i64 -5, ptr %q
  store i8 99, ptr %c
  store i32 305419896, ptr %d
  store i16 -4660, ptr %w
  store i8 -85, ptr %b
  %q0 = load i64, ptr %q
  %q1 = add i64 %q0, %x
  store i64 %q1, ptr %q
  %r0 = load i64, ptr %q
  %r1 = load i32, ptr %d
  %r1x = zext i32 %r1 to i64
  %r2 = load i16, ptr %w
  %r2x = sext i16 %r2 to i64
  %r3 = load i8, ptr %b
  %r3x = zext i8 %r3 to i64
  %s0 = mul i64 %r0, 3
  %s1 = add i64 %s0, %r1x
  %s2 = mul i64 %s1, 5
  %s3 = add i64 %s2, %r2x
  %s4 = mul i64 %s3, 7
  %s5 = add i64 %s4, %r3x
  %r8 = load i8, ptr %c
  %r8x = sext i8 %r8 to i64
  %s6 = mul i64 %s5, 11
  %s7 = add i64 %s6, %r8x
  ret i64 %s7",
    ),
    (
        // `lock xadd` and `lock cmpxchg` on slots.
        "atomic",
        "%q = alloca i64\n  %d = alloca i32",
        r"store i64 %x, ptr %q
  %xd = trunc i64 %x to i32
  store i32 %xd, ptr %d
  %old = atomicrmw add ptr %q, i64 3 seq_cst
  %cx = cmpxchg ptr %d, i32 %xd, i32 7 seq_cst seq_cst
  %cxo = extractvalue { i32, i1 } %cx, 0
  %cxs = extractvalue { i32, i1 } %cx, 1
  %r0 = load i64, ptr %q
  %r1 = load i32, ptr %d
  %r1x = sext i32 %r1 to i64
  %cxb = zext i1 %cxs to i64
  %cxa = sext i32 %cxo to i64
  %s0 = mul i64 %r0, 3
  %s1 = add i64 %s0, %r1x
  %s2 = add i64 %s1, %old
  %s3 = mul i64 %s2, 5
  %s4 = add i64 %s3, %cxb
  %s5 = add i64 %s4, %cxa
  ret i64 %s5",
    ),
    (
        // `movss`, `movsd` both ways.
        "fp",
        "%f = alloca float\n  %g = alloca double",
        r"%ff = sitofp i64 %x to float
  store float %ff, ptr %f
  %gg = sitofp i64 %x to double
  %gh = fmul double %gg, 2.5
  store double %gh, ptr %g
  %r4 = load float, ptr %f
  %r4x = fptosi float %r4 to i64
  %r5 = load double, ptr %g
  %r5x = fptosi double %r5 to i64
  %s = mul i64 %r4x, 3
  %t = add i64 %s, %r5x
  ret i64 %t",
    ),
    (
        // `movups` both ways.
        "vec",
        "%v = alloca <4 x i32>",
        r"%xd = trunc i64 %x to i32
  %vv = insertelement <4 x i32> <i32 1, i32 2, i32 3, i32 4>, i32 %xd, i32 2
  store <4 x i32> %vv, ptr %v
  %r6 = load <4 x i32>, ptr %v
  %r6a = extractelement <4 x i32> %r6, i32 2
  %r6b = extractelement <4 x i32> %r6, i32 3
  %r6x = sext i32 %r6a to i64
  %r6y = sext i32 %r6b to i64
  %s = mul i64 %r6x, 3
  %t = add i64 %s, %r6y
  ret i64 %t",
    ),
    (
        // x87: `fild`, `fld`, `fadd`, `fstp`, and the sign byte's `xor`
        // and `and` with an 8-bit immediate after the displacement.
        "x87",
        "%e = alloca x86_fp80, align 16",
        r"%ee = sitofp i64 %x to x86_fp80
  %en = fneg x86_fp80 %ee
  %ea = call x86_fp80 @llvm.fabs.f80(x86_fp80 %en)
  %es = fadd x86_fp80 %ea, %en
  %eb = fadd x86_fp80 %es, %ea
  store x86_fp80 %eb, ptr %e
  %r7 = load x86_fp80, ptr %e
  %r7x = fptosi x86_fp80 %r7 to i64
  ret i64 %r7x",
    ),
    (
        // `lea` of a slot passed to a call, its first and last bytes.
        "lea",
        "%q = alloca i64",
        r"store i64 %x, ptr %q
  %xb = trunc i64 %x to i8
  store i8 %xb, ptr %pad
  %pl = getelementptr i8, ptr %pad, i64 {last}
  store i8 7, ptr %pl
  %t = call i64 @touch(ptr %pad, i64 {k})
  %t2 = call i64 @touch(ptr %q, i64 8)
  %s = mul i64 %t, 3
  %u = add i64 %s, %t2
  ret i64 %u",
    ),
];

/// Functions `frame_OP_K(x)`, one per operation of [`FRAME_OPS`] and
/// padding `K` of [`PADS`]: the instruction classes of
/// `common::frame_inst`'s callers on locals around `[rbp - 128]`.
fn frame_ir() -> String {
    let mut ir =
        String::from("declare i64 @touch(ptr, i64)\ndeclare x86_fp80 @llvm.fabs.f80(x86_fp80)\n");
    for &k in &PADS {
        for (op, slots, body) in FRAME_OPS {
            let body = body.replace("{k}", &k.to_string()).replace("{last}", &(k - 1).to_string());
            let _ = write!(
                ir,
                "\ndefine i64 @frame_{op}_{k}(i64 %x) {{\nentry:\n  {slots}\n  %pad = alloca [{k} x i8]\n  {body}\n}}\n"
            );
        }
    }
    ir
}

fn frame_driver() -> String {
    let mut c = String::from(
        "#include <stdio.h>\n#include <stdint.h>\n\
         int64_t touch(unsigned char* p, int64_t n) {\n\
         \x20 int64_t s = p[0] * 1000 + p[n - 1];\n\
         \x20 for (int64_t i = 0; i < n; i++) p[i] = 0xee;\n\
         \x20 return s;\n}\n",
    );
    for &k in &PADS {
        for (op, _, _) in FRAME_OPS {
            let _ = writeln!(c, "int64_t frame_{op}_{k}(int64_t);");
        }
    }
    c.push_str(
        "int main(void) {\n  static const int64_t xs[] = {0, 1, -1, 1000, -77, 123456789};\n",
    );
    c.push_str("  for (int i = 0; i < 6; i++) {\n");
    for &k in &PADS {
        for (op, _, _) in FRAME_OPS {
            let _ =
                writeln!(c, "    printf(\"{op} {k} %lld\\n\", (long long)frame_{op}_{k}(xs[i]));");
        }
    }
    c.push_str("  }\n  return 0;\n}\n");
    c
}

/// §5.6.17: locals on both sides of `[rbp - 128]`, accessed by every
/// instruction class of the frame, give `llc`'s results plainly and
/// stressed; a displacement that fits a byte is `disp8`, one that does
/// not is `disp32`, and a store of an immediate to a `disp8` slot keeps
/// its immediate.
#[test]
fn frame_operands_at_the_disp8_boundary_agree_with_llc() {
    let dir = Scratch::new("x64-disp8");
    let text = format!("{}{}", ll_header(), frame_ir());
    // The shape first: it needs no emulator.
    let mut shapes = Shapes::default();
    let mut imm_after_disp8 = 0;
    let mut edges = [false; 2];
    for (mode, stress) in [("plain", Stress::default()), ("stress", Stress::ALL)] {
        let object = hod(&dir, &format!("disp8-shape-{mode}"), &text, stress);
        check_shapes(&object, &mut shapes);
        let Some(funcs) = disassemble_raw(&object) else { continue };
        for line in funcs.iter().flat_map(|(_, l)| l) {
            match rbp_disp(&line.text) {
                Some(-128) => edges[0] = true,
                Some(-129) => edges[1] = true,
                _ => {}
            }
            // `mov dword ptr [rbp - d], 0x12345678`: `C7 45 d imm32`.
            if line.bytes.len() == 7 && line.bytes[..2] == [0xc7, 0x45] {
                assert_eq!(line.bytes[3..], 305_419_896u32.to_le_bytes(), "{}", line.text);
                imm_after_disp8 += 1;
            }
        }
    }
    eprintln!("{shapes:?}");
    assert!(shapes.disp8 > 0 && shapes.disp32 > 0, "both sides of the boundary: {shapes:?}");
    assert!(imm_after_disp8 > 0, "no `mov [rbp + disp8], imm32`");
    assert_eq!(edges, [true; 2], "accesses at [rbp - 128] and [rbp - 129]");

    let Some(toolchain) = Toolchain::x86_64(&dir) else { return };
    let driver = toolchain.compile_c(&dir, "driver", &frame_driver(), &[]);
    let mut want = None;
    for major in hod_testutil::LLVM_MAJORS {
        let Some(by_llc) = llc(&dir, major, &format!("disp8-llc{major}"), &text) else { continue };
        let out = run(&toolchain, &dir, &format!("disp8-llc{major}"), &[&driver, &by_llc]);
        assert_eq!(*want.get_or_insert_with(|| out.clone()), out, "llc {major}");
    }
    let Some(want) = want else { return };
    for (mode, stress) in [("plain", Stress::default()), ("stress", Stress::ALL)] {
        let object = hod(&dir, &format!("disp8-{mode}"), &text, stress);
        let out = run(&toolchain, &dir, &format!("disp8-{mode}"), &[&driver, &object]);
        let wrong = out.lines().zip(want.lines()).find(|(a, b)| a != b);
        assert!(wrong.is_none() && out == want, "{mode}: hod {wrong:?} (hod, llc)");
    }
}
