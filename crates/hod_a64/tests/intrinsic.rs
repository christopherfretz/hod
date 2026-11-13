//! The intrinsic group (WP-041, `spec/codegen.md` §5.5.1 "Intrinsics"):
//!
//! - the integer intrinsics at every width, against `hod_interp` and
//!   `llc -O0 -global-isel=0 -fast-isel=0` natively
//!   (`common/intrinsic_gen.rs`): a systematic table and random chains;
//! - `memcpy`, `memmove`, `memset` and their `.inline` forms
//!   (`common/intrinsic_mem.rs`): inline and called, constant and
//!   variable lengths, against the interpreter byte for byte with the
//!   buffer at the end of a page, and with many live values across the
//!   call;
//! - what the interpreter does not run, checked natively by a C driver
//!   and against `llc`: the traps, `frameaddress`/`returnaddress`,
//!   `objectsize`, `is.constant`, `prefetch`, `ptrmask`;
//! - the exact words of the short forms; what is refused, by name;
//! - the second set's integer intrinsics (WP-070,
//!   `common/intrinsic2_gen.rs` on `aggr_gen`): the saturating
//!   arithmetic at every width, `scmp`/`ucmp` at every pair of widths,
//!   the `i128` forms, against the interpreter and `llc`, from memory
//!   and from registers with garbage above the widths.

#![allow(clippy::many_single_char_names, reason = "the C driver's parameters")]

mod common;

#[path = "common/aggr_gen.rs"]
mod aggr_gen;
#[path = "common/int_gen.rs"]
mod int_gen;
#[path = "common/intrinsic2_gen.rs"]
mod intrinsic2_gen;
#[path = "common/intrinsic_gen.rs"]
mod intrinsic_gen;
#[path = "common/intrinsic_mem.rs"]
mod intrinsic_mem;

use common::{assert_refused, code_words, compile_ll, ll_header, parse};
use hod_a64::intrinsic;
use hod_cg::Options;
use int_gen::{Report, Rng};
use intrinsic_gen::Gen;

/// Prints what a run measured and panics with the first disagreements.
fn check(name: &str, g: &Gen) {
    let report: Report = intrinsic_gen::run(name, g);
    eprintln!(
        "a64 intrinsic {name}: {} functions, {} cases compared, {} undefined (not compared); \
         {} IR instructions compiled in {:?}; interpreter {:?}, tools {:?}",
        g.suite.funcs.len(),
        report.compared,
        report.undefined,
        report.insts,
        report.compile,
        report.interp,
        report.tools,
    );
    int_gen::assert_agrees(&g.suite, &report);
}

/// Every integer intrinsic at every width, edge and random operands,
/// register and constant operands, both poison flags.
#[test]
fn every_integer_intrinsic_at_every_width_agrees() {
    let g = intrinsic_gen::table(&mut Rng(41));
    check("table", &g);
}

/// Random chains of integer intrinsics and arithmetic.
/// `HOD_INTRINSIC_FUNCTIONS=<n>` sets how many (default 1200).
#[test]
fn random_intrinsic_functions_agree() {
    let count = std::env::var("HOD_INTRINSIC_FUNCTIONS")
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(1200);
    let g = intrinsic_gen::random(&mut Rng(0x0410), count);
    check("random", &g);
}

/// Checks an `aggr_gen` suite of the second set (WP-070).
fn check2(name: &str, suite: &aggr_gen::Suite) {
    let report = aggr_gen::run(name, suite);
    eprintln!(
        "a64 intrinsic {name}: {} functions, {} cases against the interpreter, {} by llc only, \
         {} from registers, {} undefined; {} IR instructions compiled in {:?}; tools {:?}",
        suite.funcs.len(),
        report.interp,
        report.llc_only,
        report.regs,
        report.undefined,
        report.insts,
        report.compile,
        report.tools,
    );
    assert!(report.interp > 0, "{name}: nothing compared with the interpreter");
    aggr_gen::assert_agrees(&report);
}

/// `{s,u}{add,sub}.sat` at every width 1 to 64 and at 128 (WP-070).
#[test]
fn saturating_arithmetic_agrees() {
    check2("sat", &intrinsic2_gen::sat_table(&mut aggr_gen::Rng(70)));
}

/// `scmp`/`ucmp` for operand widths 1 to 128 by result widths 2 to 128
/// (WP-070).
#[test]
fn three_way_compares_agree() {
    check2("cmp3", &intrinsic2_gen::three_way_table(&mut aggr_gen::Rng(71)));
}

/// WP-041's integer intrinsics at `i128` (WP-070).
#[test]
fn wide_integer_intrinsics_agree() {
    check2("i128", &intrinsic2_gen::wide_table(&mut aggr_gen::Rng(72)));
}

/// The words of the only function of `text` (the header added).
fn words(text: &str) -> Vec<u32> {
    let (m, bodies) = parse(&format!("{}{text}", ll_header(22)));
    code_words(&m, &bodies, &Options::default())
}

/// What the group does not lower, by name.
#[test]
fn what_is_not_lowered_is_refused_by_name() {
    assert_refused(&[
        (
            "declare i256 @llvm.ctpop.i256(i256)\n\
             define i256 @f(i256 %a) {\n  %r = call i256 @llvm.ctpop.i256(i256 %a)\n  ret i256 %r\n}\n",
            intrinsic::WIDE,
        ),
        (
            "declare void @llvm.memcpy.inline.p0.p0.i64(ptr, ptr, i64, i1)\n\
             define void @f(ptr %d, ptr %s, i64 %n) {\n  call void @llvm.memcpy.inline.p0.p0.i64(ptr %d, ptr %s, i64 %n, i1 false)\n  ret void\n}\n",
            intrinsic::INLINE_LENGTH,
        ),
        (
            "declare void @llvm.memset.inline.p0.i64(ptr, i8, i64, i1)\n\
             define void @f(ptr %d, i64 %n) {\n  call void @llvm.memset.inline.p0.i64(ptr %d, i8 1, i64 %n, i1 false)\n  ret void\n}\n",
            intrinsic::INLINE_LENGTH,
        ),
        (
            "declare ptr @llvm.frameaddress.p0(i32)\n\
             define ptr @f() {\n  %r = call ptr @llvm.frameaddress.p0(i32 1000)\n  ret ptr %r\n}\n",
            intrinsic::FRAME_DEPTH,
        ),
    ]);
    // The same intrinsics at 64 and 128 bits compile (WP-070: i128).
    compile_ll(
        "declare i128 @llvm.ctpop.i128(i128)\n\
         define i128 @f(i128 %a) {\n  %r = call i128 @llvm.ctpop.i128(i128 %a)\n  ret i128 %r\n}\n",
    )
    .unwrap();
    compile_ll(
        "declare i64 @llvm.ctpop.i64(i64)\n\
         define i64 @f(i64 %a) {\n  %r = call i64 @llvm.ctpop.i64(i64 %a)\n  ret i64 %r\n}\n",
    )
    .unwrap();
}

/// The short forms, word for word: `trap` and friends are `llc`'s `brk`s;
/// a 64-bit `ctlz` is one `clz`; a constant `fshl` at 64 bits is one
/// `extr`; `frameaddress(0)` is `mov xD, x29`.
#[test]
fn short_forms_are_llcs() {
    use hod_mc_a64 as mc;
    use hod_mc_a64::{Reg, RegSp, Size};
    let x0 = Reg::X0;
    let traps = words(
        "declare void @llvm.trap()\ndeclare void @llvm.debugtrap()\ndeclare void @llvm.ubsantrap(i8)\n\
         define void @f() {\n  call void @llvm.trap()\n  call void @llvm.debugtrap()\n  \
         call void @llvm.ubsantrap(i8 7)\n  ret void\n}\n",
    );
    let at = |w: u32| traps.iter().position(|&x| x == w).unwrap_or_else(|| panic!("{traps:08x?}"));
    assert!(at(mc::brk(1)) < at(mc::brk(0xf000)) && at(mc::brk(0xf000)) < at(mc::brk(0x5507)));
    let clz = words(
        "declare i64 @llvm.ctlz.i64(i64, i1)\n\
         define i64 @f(i64 %a) {\n  %r = call i64 @llvm.ctlz.i64(i64 %a, i1 false)\n  ret i64 %r\n}\n",
    );
    assert!(clz.contains(&mc::clz(Size::X, x0.zr(), x0.zr())), "{clz:08x?}");
    let extr = words(
        "declare i64 @llvm.fshl.i64(i64, i64, i64)\n\
         define i64 @f(i64 %a, i64 %b) {\n  %r = call i64 @llvm.fshl.i64(i64 %a, i64 %b, i64 13)\n  ret i64 %r\n}\n",
    );
    assert!(extr.contains(&mc::extr(Size::X, x0.zr(), x0.zr(), Reg::X1.zr(), 51)), "{extr:08x?}");
    let fa = words(
        "declare ptr @llvm.frameaddress.p0(i32)\n\
         define ptr @f() {\n  %r = call ptr @llvm.frameaddress.p0(i32 0)\n  ret ptr %r\n}\n",
    );
    let x29 = RegSp::new(29).unwrap();
    assert!(
        fa.iter().any(|&w| (0..31).any(|d| w == mc::mov_sp(Size::X, RegSp::new(d).unwrap(), x29))),
        "{fa:08x?}"
    );
}

// ---- the memory intrinsics ----

mod mem_intrinsics {
    use std::cell::RefCell;
    use std::collections::BTreeSet;
    use std::fmt::Write as _;
    use std::rc::Rc;
    use std::time::Duration;

    use hod_a64::compile_module;
    use hod_cg::{Options, RelocModel, Stress};
    use hod_interp::{Interp, Program, Val};
    use hod_testutil::{Scratch, run_with};

    use crate::common::linking::Toolchain;
    use crate::common::{ll_header, llc, parse, write_object};
    use crate::intrinsic_mem::{BUF, Func, generate};

    /// The functions of `seeds` and their declarations.
    fn batch(seeds: std::ops::Range<u64>) -> (Vec<Func>, String) {
        let mut decls = BTreeSet::new();
        let funcs = seeds.map(|seed| generate(&format!("m{seed}"), seed, &mut decls)).collect();
        let mut text = ll_header(22);
        for d in decls {
            writeln!(text, "{d}").unwrap();
        }
        (funcs, text)
    }

    /// What hod and `llc` compile.
    fn native_text(funcs: &[Func], header: &str) -> String {
        let mut text = header.to_string();
        for f in funcs {
            text.push_str(&f.text);
        }
        text
    }

    /// The `.ll` arguments of `f`.
    fn args(f: &Func, buf: &str) -> String {
        let [n, m, v, x] = f.ints;
        format!(
            "ptr {buf}, i64 {n}, i32 {}, i8 {}, i64 {}, double 0x{:016X}",
            m as u32 as i32,
            v as u8 as i8,
            x as i64,
            f.double.to_bits()
        )
    }

    /// What each function's run prints, by the interpreter: `f`, `r`,
    /// the buffer in hex.
    fn expected(funcs: &[Func], header: &str) -> Vec<String> {
        let mut text = native_text(funcs, header);
        text.push_str("declare void @dump(ptr, i64)\n");
        for f in funcs {
            let name = &f.name;
            let init: Vec<String> = f.buf.iter().map(|b| format!("i8 {}", *b as i8)).collect();
            writeln!(text, "@{name}_buf = global [{BUF} x i8] [{}], align 16", init.join(", "))
                .unwrap();
            writeln!(
                text,
                "define i64 @{name}_w() {{\n  %r = call i64 @{name}({})\n  \
                 call void @dump(ptr @{name}_buf, i64 {BUF})\n  ret i64 %r\n}}",
                args(f, &format!("@{name}_buf"))
            )
            .unwrap();
        }
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
                format!("f {}\nr {:x}\nb {}\n", f.name, value as u64, dumps.borrow()[0])
            })
            .collect()
    }

    /// The C driver: the buffer is the last 256 bytes of a page whose
    /// next page is inaccessible; `%m` and `%v` carry garbage above their
    /// widths.
    fn driver(funcs: &[Func]) -> String {
        let mut c = String::from(
            "#include <stdint.h>\n#include <stdio.h>\n#include <string.h>\n\
             #include <sys/mman.h>\n#include <unistd.h>\n",
        );
        for f in funcs {
            writeln!(
                c,
                "uint64_t {}(unsigned char *, uint64_t, uint64_t, uint64_t, uint64_t, double);",
                f.name
            )
            .unwrap();
            let init: Vec<String> = f.buf.iter().map(u8::to_string).collect();
            writeln!(
                c,
                "static const unsigned char {}_init[{BUF}] = {{{}}};",
                f.name,
                init.join(",")
            )
            .unwrap();
        }
        c.push_str(
            "int main(void) {\n  long pg = sysconf(_SC_PAGESIZE);\n\
             unsigned char *map = mmap(0, 2 * pg, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);\n\
             if (map == MAP_FAILED || mprotect(map + pg, pg, PROT_NONE)) return 99;\n",
        );
        writeln!(c, "  unsigned char *buf = map + pg - {BUF};").unwrap();
        for f in funcs {
            let [n, m, v, x] = f.ints;
            let name = &f.name;
            writeln!(c, "  memcpy(buf, {name}_init, {BUF});").unwrap();
            writeln!(
                c,
                "  {{ uint64_t r = {name}(buf, {n:#x}ULL, {:#x}ULL, {:#x}ULL, {x:#x}ULL, {:?}); \
                 printf(\"f {name}\\nr %llx\\nb \", (unsigned long long)r); \
                 for (int i = 0; i < {BUF}; i++) printf(\"%02x\", buf[i]); printf(\"\\n\"); fflush(stdout); }}",
                m | 0xdead_beef_0000_0000,
                v | 0x5a5a_5a5a_5a5a_5a00,
                f.double
            )
            .unwrap();
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

    const MODES: [&str; 4] = ["plain", "stress", "pic", "llc"];

    /// Runs the functions of `seeds` in every mode against the
    /// interpreter; the number checked (0 without the tools).
    fn check(seeds: std::ops::Range<u64>) -> usize {
        let (funcs, header) = batch(seeds);
        let want = expected(&funcs, &header);
        let scratch = Scratch::new("a64-intrinsic-mem");
        let Some(toolchain) = Toolchain::aarch64(&scratch) else { return 0 };
        let Some(linker) = toolchain.linkers.first() else { return 0 };
        let text = native_text(&funcs, &header);
        let (module, bodies) = parse(&text);
        let driver = toolchain.compile_c(&scratch, "driver", &driver(&funcs), &[]);
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
                    .unwrap_or_else(|e| panic!("{mode}: {e}"));
                write_object(&scratch, &format!("{mode}.o"), &emitted)
            };
            let program = scratch.join(mode);
            toolchain.link_ok(linker, &[], &[&driver, &object], &program);
            let ran =
                run_with(&mut toolchain.runner_command(&program), &[], Duration::from_mins(2));
            let got = split(&ran.stdout_text());
            for (k, f) in funcs.iter().enumerate() {
                let got = got.get(k).map_or("(nothing: the run stopped)", String::as_str);
                assert_eq!(
                    got, want[k],
                    "{mode} {}: hod_a64 and the interpreter disagree ({:?})\n{}",
                    f.name, ran.status, f.text
                );
            }
            assert!(ran.success(), "{mode}: {:?}\n{}", ran.status, ran.stderr_text());
        }
        funcs.len()
    }

    /// Generated functions agree with `hod_interp` in every mode and
    /// through `llc`.
    #[test]
    fn memory_intrinsics_agree_with_the_interpreter() {
        let mut checked = 0;
        for start in [0, 300, 600] {
            checked += check(start..start + 300);
        }
        eprintln!("a64 intrinsic mem: {checked} functions agree in {} modes", MODES.len());
    }
}

// ---- what the interpreter does not run: checked by the driver ----

mod native_forms {
    use std::os::unix::process::ExitStatusExt as _;
    use std::time::Duration;

    use hod_a64::compile_module;
    use hod_cg::{Options, Stress};
    use hod_testutil::{Scratch, run_with};

    use crate::common::linking::Toolchain;
    use crate::common::{ll_header, llc, parse, write_object};

    const FUNCTIONS: &str = r"
declare ptr @llvm.frameaddress.p0(i32)
declare ptr @llvm.returnaddress(i32)
declare void @llvm.trap()
declare void @llvm.debugtrap()
declare void @llvm.ubsantrap(i8)
declare i64 @llvm.objectsize.i64.p0(ptr, i1, i1, i1)
declare i32 @llvm.objectsize.i32.p0(ptr, i1, i1, i1)
declare i1 @llvm.is.constant.i32(i32)
declare i1 @llvm.is.constant.p0(ptr)
declare void @llvm.prefetch.p0(ptr, i32, i32, i32)
declare ptr @llvm.ptrmask.p0.i64(ptr, i64)

; out = [frame 0, frame 1, frame 2, return 0, return 1, the two words of
; frame 0's record]
define void @frames(ptr %out) {
  %f0 = call ptr @llvm.frameaddress.p0(i32 0)
  %f1 = call ptr @llvm.frameaddress.p0(i32 1)
  %f2 = call ptr @llvm.frameaddress.p0(i32 2)
  %r0 = call ptr @llvm.returnaddress(i32 0)
  %r1 = call ptr @llvm.returnaddress(i32 1)
  store ptr %f0, ptr %out
  %o1 = getelementptr ptr, ptr %out, i64 1
  store ptr %f1, ptr %o1
  %o2 = getelementptr ptr, ptr %out, i64 2
  store ptr %f2, ptr %o2
  %o3 = getelementptr ptr, ptr %out, i64 3
  store ptr %r0, ptr %o3
  %o4 = getelementptr ptr, ptr %out, i64 4
  store ptr %r1, ptr %o4
  %w0 = load ptr, ptr %f0
  %o5 = getelementptr ptr, ptr %out, i64 5
  store ptr %w0, ptr %o5
  %a1 = getelementptr ptr, ptr %f0, i64 1
  %w1 = load ptr, ptr %a1
  %o6 = getelementptr ptr, ptr %out, i64 6
  store ptr %w1, ptr %o6
  ret void
}

; A trap is not a terminator: the code after it is emitted.
define i32 @maybe_trap(i32 %c, i32 %x) {
  %t = icmp ne i32 %c, 0
  br i1 %t, label %die, label %live
die:
  call void @llvm.trap()
  %y = add i32 %x, 1
  ret i32 %y
live:
  %z = add i32 %x, 2
  ret i32 %z
}

define void @debug_trap() {
  call void @llvm.debugtrap()
  ret void
}

define void @ubsan_trap() {
  call void @llvm.ubsantrap(i8 42)
  unreachable
}

define i64 @sizes(ptr %p) {
  %a = call i64 @llvm.objectsize.i64.p0(ptr %p, i1 false, i1 false, i1 false)
  %b = call i64 @llvm.objectsize.i64.p0(ptr %p, i1 true, i1 true, i1 false)
  %c = call i32 @llvm.objectsize.i32.p0(ptr %p, i1 false, i1 true, i1 true)
  %cz = zext i32 %c to i64
  %s = shl i64 %cz, 1
  %t = add i64 %a, %b
  %r = xor i64 %t, %s
  ret i64 %r
}

define i32 @constants(i32 %x, ptr %p) {
  %a = call i1 @llvm.is.constant.i32(i32 %x)
  %b = call i1 @llvm.is.constant.i32(i32 5)
  %c = call i1 @llvm.is.constant.p0(ptr %p)
  call void @llvm.prefetch.p0(ptr %p, i32 0, i32 3, i32 1)
  call void @llvm.prefetch.p0(ptr null, i32 1, i32 0, i32 0)
  %az = zext i1 %a to i32
  %bz = zext i1 %b to i32
  %cz = zext i1 %c to i32
  %s = shl i32 %bz, 1
  %t = shl i32 %cz, 2
  %u = or i32 %az, %s
  %v = or i32 %u, %t
  %r = add i32 %v, %x
  ret i32 %r
}

define ptr @masked(ptr %p, i64 %m) {
  %a = call ptr @llvm.ptrmask.p0.i64(ptr %p, i64 %m)
  %b = call ptr @llvm.ptrmask.p0.i64(ptr %a, i64 -16)
  ret ptr %b
}
";

    const DRIVER: &str = r#"
#include <stdint.h>
#include <stdio.h>
#include <string.h>
void frames(void **);
int maybe_trap(int, int);
void debug_trap(void);
void ubsan_trap(void);
uint64_t sizes(void *);
uint32_t constants(uint32_t, void *);
void *masked(void *, uint64_t);

__attribute__((noinline)) void *caller(void **out, void **ra) {
  frames(out);
  *ra = __builtin_return_address(0);
  return __builtin_frame_address(0);
}

int main(int argc, char **argv) {
  if (argc > 1 && !strcmp(argv[1], "trap")) return maybe_trap(1, 3);
  if (argc > 1 && !strcmp(argv[1], "debugtrap")) { debug_trap(); return 0; }
  if (argc > 1 && !strcmp(argv[1], "ubsantrap")) { ubsan_trap(); return 0; }
  void *out[7], *ra;
  void *fa = caller(out, &ra);
  printf("frame1 %d\n", out[1] == fa);
  printf("chain0 %d\n", out[5] == out[1] && (char *)out[0] < (char *)out[1]);
  printf("chain1 %d\n", out[2] == __builtin_frame_address(0));
  printf("ret0 %d\n", out[6] == out[3]
      && (char *)out[3] > (char *)caller && (char *)out[3] < (char *)caller + 256);
  printf("ret1 %d\n", out[4] == ra);
  printf("live %d\n", maybe_trap(0, 40));
  printf("sizes %llx\n", (unsigned long long)sizes(out));
  printf("constants %u\n", constants(100, out));
  printf("masked %d\n", masked((char *)out + 0x37, ~(uint64_t)0xf0) == (void *)(((uintptr_t)out + 0x37) & ~(uintptr_t)0xff));
  return 0;
}
"#;

    const WANT: &str = "frame1 1\nchain0 1\nchain1 1\nret0 1\nret1 1\nlive 42\n\
                        sizes fffffffe00000001\nconstants 102\nmasked 1\n";

    /// The frame walk, the folded intrinsics and the traps, natively:
    /// hod plainly and stressed, and `llc`, against what the driver
    /// expects. A trap ends the process with `SIGTRAP`.
    #[test]
    fn the_native_forms_hold() {
        let scratch = Scratch::new("a64-intrinsic-native");
        let Some(toolchain) = Toolchain::aarch64(&scratch) else { return };
        let text = format!("{}{FUNCTIONS}", ll_header(22));
        let (module, bodies) = parse(&text);
        let driver = toolchain.compile_c(&scratch, "driver", DRIVER, &["-fno-omit-frame-pointer"]);
        let mut objects = Vec::new();
        for (mode, stress) in [("plain", Stress::default()), ("stress", Stress::ALL)] {
            let options = Options { stress, ..Options::default() };
            let emitted = compile_module(&module, &bodies, &options).unwrap();
            objects.push((mode, write_object(&scratch, &format!("{mode}.o"), &emitted)));
        }
        if let Some(object) = llc(&scratch, "llc", &text, &[]) {
            objects.push(("llc", object));
        }
        for linker in &toolchain.linkers {
            for (mode, object) in &objects {
                let program = scratch.join(format!("{mode}-{}", linker.name));
                toolchain.link_ok(linker, &[], &[&driver, object], &program);
                let ran =
                    run_with(&mut toolchain.runner_command(&program), &[], Duration::from_mins(1));
                assert!(ran.success(), "{mode}: {:?}\n{}", ran.status, ran.stderr_text());
                assert_eq!(ran.stdout_text(), WANT, "{mode} with {}", linker.name);
                for arg in ["trap", "debugtrap", "ubsantrap"] {
                    let ran = run_with(
                        toolchain.runner_command(&program).arg(arg),
                        &[],
                        Duration::from_mins(1),
                    );
                    assert_eq!(
                        ran.status.and_then(|s| s.signal()),
                        Some(5),
                        "{mode} {arg}: {:?}",
                        ran.status
                    );
                }
            }
        }
    }
}

/// `llvm.eh.typeid.for` in real C++ (WP-070): `tests/data/intrinsic/
/// typeid.ll` is clang++'s `-O0` code of `typeid.cpp` (catches by
/// several types in one landing pad, nested try blocks, a rethrow, a
/// cleanup); `typeid_driver.cpp`, built with g++, throws each type.
/// hod plainly and stressed, and `llc` as the control, under every
/// linker.
#[test]
fn eh_typeid_for_selects_the_clause() {
    use common::linking::{Toolchain, exit_code};
    use common::{emit, llc, llc_major, write_object};
    let dir = hod_testutil::Scratch::new("a64-intrinsic-typeid");
    let Some(toolchain) = Toolchain::aarch64(&dir) else { return };
    let Some(gxx) = hod_testutil::tool("g++") else { return };
    let data = |name: &str| {
        let path = format!("{}/tests/data/intrinsic/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
    };
    let source = dir.write("typeid_driver.cpp", data("typeid_driver.cpp"));
    let driver = dir.join("typeid-driver.o");
    let mut command = std::process::Command::new(&gxx);
    command.args(["-c", "-O1"]).arg(&source).arg("-o").arg(&driver);
    let built = hod_testutil::run_with(&mut command, &[], std::time::Duration::from_mins(2));
    assert!(built.success(), "g++: {}", built.stderr_text());
    let libstdcxx = hod_testutil::run(&gxx, ["-print-file-name=libstdc++.so"]).stdout_text();
    let libstdcxx = std::path::PathBuf::from(libstdcxx.trim());
    let text = data("typeid.ll");
    let (module, bodies) = parse(&format!("{}{text}", ll_header(22)));
    let mut objects = Vec::new();
    for (tag, stress) in [("hod", hod_cg::Stress::default()), ("stress", hod_cg::Stress::ALL)] {
        let emitted = emit(&module, &bodies, &Options { stress, ..Options::default() });
        objects.push((tag, write_object(&dir, &format!("typeid-{tag}.o"), &emitted)));
    }
    if let Some(object) = llc(&dir, "typeid-llc", &format!("{}{text}", ll_header(llc_major())), &[])
    {
        objects.push(("llc", object));
    }
    for (tag, object) in &objects {
        for linker in &toolchain.linkers {
            let exe = dir.join(format!("typeid-{tag}-{}", linker.name));
            let ran = toolchain.link_and_run(linker, &[], &[&driver, object, &libstdcxx], &exe);
            let out = ran.stdout_text();
            assert!(
                exit_code(&ran) == Some(0) && out.contains("typeid done"),
                "typeid-{tag} {}: exit {:?}\n{out}{}",
                linker.name,
                exit_code(&ran),
                ran.stderr_text()
            );
        }
    }
}
