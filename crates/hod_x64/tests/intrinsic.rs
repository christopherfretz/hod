//! The x86-64 intrinsic group (WP-128, `spec/codegen.md` §5.6.8):
//!
//! - the integer intrinsics at every width and at `i128`, against
//!   `hod_interp` and `llc` 22 and 19 under the emulator, from memory and
//!   from registers with garbage above the widths, and across the mixed
//!   legs (`common/intrinsic_gen.rs` on `common/aggr_gen.rs`);
//! - `memcpy`, `memmove`, `memset` and their `.inline` forms
//!   (`common/intrinsic_mem.rs`): every length 0 to 130 at walking
//!   alignments, overlaps both ways, constant and variable lengths,
//!   against the interpreter byte for byte with the buffer at the end of
//!   a page, with live values across them;
//! - what the interpreter does not run, checked by a C driver natively
//!   and against `llc`: the traps, `frameaddress`/`returnaddress`,
//!   `objectsize`, `is.constant`, `prefetch`, `ptrmask`,
//!   `readcyclecounter`, `clear_cache`;
//! - the flags rule on the disassembly, the short forms, and what is
//!   refused, by name.

#![allow(clippy::many_single_char_names, reason = "the C driver's parameters")]

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

#[path = "common/aggr_gen.rs"]
mod aggr_gen;
#[path = "common/int_gen.rs"]
mod int_gen;
#[path = "common/intrinsic_gen.rs"]
mod intrinsic_gen;
#[path = "common/intrinsic_mem.rs"]
mod intrinsic_mem;

use std::process::Command;
use std::time::Duration;

use hod_cg::{CgError, Stress};
use hod_testutil::{Scratch, run_with, tool};
use hod_x64::intrinsic;
use int_gen::{ll_header, parse, pic};

/// Prints what an `aggr_gen` run measured and panics with the first
/// disagreements.
fn check(name: &str, suite: &aggr_gen::Suite) {
    check_with(name, suite, true);
}

/// [`check`]; `interp`: whether the interpreter runs the suite (it does
/// not know `{s,u}shl.sat`: `llc` 22 and 19 are the oracle there).
#[allow(clippy::cast_precision_loss, reason = "rates to print")]
fn check_with(name: &str, suite: &aggr_gen::Suite, interp: bool) {
    let report = aggr_gen::run(name, suite);
    eprintln!(
        "x64 intrinsic {name}: {} functions, {} cases: {} against the interpreter, {} by llc \
         only, {} undefined (not compared), {} native with garbage above the widths; legs {:?}; \
         {} IR instructions compiled in {:?}; text hod {} B, llc 22 {} B (hod/llc {:.2}); tools \
         {:?}",
        suite.funcs.len(),
        suite.cases.len(),
        report.interp,
        report.llc_only,
        report.undefined,
        report.native,
        report.legs,
        report.insts,
        report.compile,
        report.text.0,
        report.text.1,
        report.text.0 as f64 / report.text.1.max(1) as f64,
        report.tools,
    );
    if interp {
        assert!(report.interp > 0, "{name}: nothing compared with the interpreter");
    } else {
        assert!(report.llc_only > 0, "{name}: nothing compared with llc");
    }
    aggr_gen::assert_agrees(&report);
}

/// `ctlz`, `cttz`, `ctpop`, `bswap`, `bitreverse`, `abs`, the min/max
/// and the funnel shifts at every width 1 to 64, every single bit and
/// every low mask, register and constant operands, both poison flags.
#[test]
fn every_integer_intrinsic_at_every_width_agrees() {
    check("narrow", &intrinsic_gen::narrow_table(&mut aggr_gen::Rng(128)));
}

/// `{s,u}{add,sub}.sat` at every width 1 to 64 and at 128.
#[test]
fn saturating_arithmetic_agrees() {
    check("sat", &intrinsic_gen::sat_table(&mut aggr_gen::Rng(1280)));
}

/// `scmp`/`ucmp` for operand widths 1 to 128 by result widths 2 to 128.
#[test]
fn three_way_compares_agree() {
    check("cmp3", &intrinsic_gen::three_way_table(&mut aggr_gen::Rng(1281)));
}

/// The unary, min/max and funnel intrinsics at `i128`.
#[test]
fn wide_integer_intrinsics_agree() {
    check("i128", &intrinsic_gen::wide_table(&mut aggr_gen::Rng(1282)));
}

/// `{s,u}shl.sat` at every width up to 64, every amount, against `llc`
/// (the interpreter does not run them).
#[test]
fn saturating_shifts_agree() {
    check_with("shlsat", &intrinsic_gen::shl_sat_table(&mut aggr_gen::Rng(1283)), false);
}

// ---- the code itself ----

/// The object of `functions` (the header added), compiled
/// position-independent with `stress`.
fn object(functions: &str, stress: Stress) -> Vec<u8> {
    let (m, bodies) = parse(&format!("{}{functions}", ll_header()));
    hod_x64::compile_to_object(&m, &bodies, &pic(stress)).unwrap_or_else(|e| panic!("{e}"))
}

/// `llvm-objdump -d -M intel` of an object: one line per instruction,
/// `mnemonic operands`, or one `<name>:` line per function. `None`
/// without the tool.
fn disassemble(object: Vec<u8>) -> Option<Vec<String>> {
    let objdump = tool("llvm-objdump")?;
    let scratch = Scratch::new("x64-intrinsic-disasm");
    let path = scratch.write("f.o", object);
    let mut command = Command::new(objdump);
    command.args(["-d", "-M", "intel", "--no-show-raw-insn", "--no-leading-addr"]).arg(&path);
    let outcome = run_with(&mut command, &[], Duration::from_mins(1));
    assert!(outcome.success(), "llvm-objdump: {}", outcome.stderr_text());
    Some(
        outcome
            .stdout_text()
            .lines()
            .filter_map(|line| {
                if line.ends_with(">:") {
                    return Some(line.to_string());
                }
                if !line.starts_with(char::is_whitespace) || line.trim().is_empty() {
                    return None;
                }
                let line = line.split('#').next().unwrap_or(line);
                Some(line.split_whitespace().collect::<Vec<_>>().join(" "))
            })
            .collect(),
    )
}

fn disassembly(functions: &str) -> Option<Vec<String>> {
    disassemble(object(functions, Stress::default()))
}

/// The mnemonic of a disassembled line.
fn mnemonic(line: &str) -> &str {
    line.split_whitespace().next().unwrap_or("")
}

/// Whether `m` reads the flags: `setcc`, `cmovcc`, `jcc`, `adc`, `sbb`.
fn reads_flags(m: &str) -> bool {
    (m.starts_with("set") || m.starts_with("cmov") || (m.starts_with('j') && m != "jmp"))
        || matches!(m, "adc" | "sbb")
}

/// What may run between a flag setter and its reader (§5.6 "Flags").
fn keeps_flags(m: &str) -> bool {
    matches!(m, "mov" | "movabs" | "lea" | "movzx" | "movsx" | "movsxd" | "push" | "pop")
}

/// The instructions that set the flags a reader here may read.
fn sets_flags(m: &str) -> bool {
    matches!(m, "cmp" | "test" | "add" | "sub" | "adc" | "sbb" | "bsr" | "bsf" | "neg")
}

/// §5.6 "Flags": in every function of the integer tables, plain and
/// under every stress option, each flag reader is reached from an
/// instruction that sets the flags it reads through flag-keeping glue
/// only (a `setcc` or `cmovcc` keeps them, so the second of a pair is
/// reached from the first's setter).
#[test]
fn every_flag_reader_keeps_the_flags() {
    let suites = [
        intrinsic_gen::narrow_table(&mut aggr_gen::Rng(1285)),
        intrinsic_gen::sat_table(&mut aggr_gen::Rng(1286)),
        intrinsic_gen::three_way_table(&mut aggr_gen::Rng(1287)),
        intrinsic_gen::wide_table(&mut aggr_gen::Rng(1288)),
        intrinsic_gen::shl_sat_table(&mut aggr_gen::Rng(1289)),
    ];
    let mut readers = 0;
    for suite in &suites {
        let text = suite.module();
        let (m, bodies) = aggr_gen::parse(&text);
        for stress in [Stress::default(), Stress::ALL] {
            let object = hod_x64::compile_to_object(&m, &bodies, &aggr_gen::pic(stress))
                .unwrap_or_else(|e| panic!("{e}"));
            let Some(lines) = disassemble(object) else { return };
            for (at, line) in lines.iter().enumerate() {
                let m = mnemonic(line);
                if !reads_flags(m) {
                    continue;
                }
                readers += 1;
                // `setcc` and `cmovcc` read the flags and keep them.
                let setter = lines[..at]
                    .iter()
                    .rev()
                    .map(|l| mnemonic(l))
                    .find(|m| !keeps_flags(m) && !m.starts_with("set") && !m.starts_with("cmov"))
                    .unwrap_or("");
                let ok = sets_flags(setter);
                assert!(
                    ok,
                    "{line} is reached from {setter}:\n{}",
                    lines[at.saturating_sub(12)..=at].join("\n")
                );
            }
        }
    }
    assert!(readers > 5000, "{readers} readers");
}

/// The short forms: a 64-bit `ctlz` is `bsr` + `cmove` + `xor 63`;
/// a narrow one sets the bit below the value (`bts`); a constant `fshl`
/// at 64 bits is one `shld`; `ctpop` is the SWAR sequence (no `popcnt`);
/// the traps are `ud2` and `int3`; `readcyclecounter` is `rdtsc`;
/// `frameaddress(0)` is `rbp`; `prefetch` and `clear_cache` are nothing;
/// an inline `memcpy` of 40 bytes is three 16-byte copies; a large one
/// a call.
#[test]
fn short_forms() {
    let Some(clz) = disassembly(
        "declare i64 @llvm.ctlz.i64(i64, i1)\n\
         define i64 @f(i64 %a) {\n  %r = call i64 @llvm.ctlz.i64(i64 %a, i1 false)\n  ret i64 %r\n}\n",
    ) else {
        return;
    };
    let has = |lines: &[String], want: &str| lines.iter().any(|l| l.starts_with(want));
    for want in ["mov eax, 0x7f", "bsr rdi, rdi", "cmove rdi, rax", "xor rdi, 0x3f"] {
        assert!(has(&clz, want), "{want}: {clz:#?}");
    }
    let clz17 = disassembly(
        "declare i17 @llvm.ctlz.i17(i17, i1)\n\
         define i17 @f(i17 %a) {\n  %r = call i17 @llvm.ctlz.i17(i17 %a, i1 false)\n  ret i17 %r\n}\n",
    )
    .unwrap();
    for want in ["shl edi, 0xf", "bts edi, 0xe", "bsr edi, edi", "xor edi, 0x1f"] {
        assert!(has(&clz17, want), "{want}: {clz17:#?}");
    }
    let shld = disassembly(
        "declare i64 @llvm.fshl.i64(i64, i64, i64)\n\
         define i64 @f(i64 %a, i64 %b) {\n  %r = call i64 @llvm.fshl.i64(i64 %a, i64 %b, i64 13)\n  ret i64 %r\n}\n",
    )
    .unwrap();
    assert!(has(&shld, "shld rdi, rsi, 0xd"), "{shld:#?}");
    let pop = disassembly(
        "declare i32 @llvm.ctpop.i32(i32)\n\
         define i32 @f(i32 %a) {\n  %r = call i32 @llvm.ctpop.i32(i32 %a)\n  ret i32 %r\n}\n",
    )
    .unwrap();
    assert!(has(&pop, "imul edi, edi, 0x1010101") && has(&pop, "shr edi, 0x18"), "{pop:#?}");
    assert!(!pop.iter().any(|l| mnemonic(l) == "popcnt"), "{pop:#?}");
    let misc = disassembly(
        "declare void @llvm.trap()\ndeclare void @llvm.debugtrap()\ndeclare void @llvm.ubsantrap(i8)\n\
         declare i64 @llvm.readcyclecounter()\ndeclare ptr @llvm.frameaddress.p0(i32)\n\
         declare void @llvm.prefetch.p0(ptr, i32, i32, i32)\ndeclare void @llvm.clear_cache(ptr, ptr)\n\
         define i64 @f(ptr %p) {\n  call void @llvm.prefetch.p0(ptr %p, i32 0, i32 3, i32 1)\n  \
         call void @llvm.clear_cache(ptr %p, ptr %p)\n  call void @llvm.trap()\n  \
         call void @llvm.debugtrap()\n  call void @llvm.ubsantrap(i8 7)\n  \
         %c = call i64 @llvm.readcyclecounter()\n  %f = call ptr @llvm.frameaddress.p0(i32 0)\n  \
         %i = ptrtoint ptr %f to i64\n  %r = add i64 %c, %i\n  ret i64 %r\n}\n",
    )
    .unwrap();
    let at = |want: &str| misc.iter().position(|l| l.starts_with(want));
    let order = [at("ud2"), at("int3"), at("rdtsc"), at("shl rdx, 0x20"), at("or rax, rdx")];
    assert!(order.iter().all(Option::is_some), "{misc:#?}");
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{misc:#?}");
    assert_eq!(misc.iter().filter(|l| mnemonic(l) == "ud2").count(), 2, "{misc:#?}");
    assert!(misc.iter().any(|l| l.starts_with("mov") && l.ends_with(", rbp")), "{misc:#?}");
    assert!(!misc.iter().any(|l| l.starts_with("prefetch") || l.starts_with("call")), "{misc:#?}");
    let copies = disassembly(
        "declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)\n\
         define void @f(ptr %d, ptr %s) {\n  call void @llvm.memcpy.p0.p0.i64(ptr %d, ptr %s, i64 40, i1 false)\n  ret void\n}\n\
         define void @g(ptr %d, ptr %s) {\n  call void @llvm.memcpy.p0.p0.i64(ptr %d, ptr %s, i64 65, i1 false)\n  ret void\n}\n",
    )
    .unwrap();
    let g = copies.iter().position(|l| l.ends_with("<g>:")).unwrap();
    let movups = copies[..g].iter().filter(|l| l.starts_with("movups xmm15")).count();
    assert_eq!(movups, 3, "{copies:#?}");
    assert!(!copies[..g].iter().any(|l| l.starts_with("call")), "{copies:#?}");
    assert!(copies[g..].iter().any(|l| l.starts_with("call")), "{copies:#?}");
    assert!(!copies.iter().any(|l| l.contains("rep")), "{copies:#?}");
}

/// What the group does not lower, by name.
#[test]
fn what_is_not_lowered_is_refused_by_name() {
    let refused = |text: &str, name: &'static str| {
        let (m, bodies) = parse(&format!("{}{text}", ll_header()));
        match hod_x64::compile_to_object(&m, &bodies, &pic(Stress::default())) {
            Err(hod_x64::ModuleError::Function { error, .. }) => {
                assert_eq!(error, CgError::unsupported(name), "{text}");
            }
            other => panic!("not refused: {:?}\n{text}", other.map(|o| o.len())),
        }
    };
    refused(
        "declare i256 @llvm.ctpop.i256(i256)\n\
         define i256 @f(i256 %a) {\n  %r = call i256 @llvm.ctpop.i256(i256 %a)\n  ret i256 %r\n}\n",
        intrinsic::WIDE,
    );
    refused(
        "declare void @llvm.memcpy.inline.p0.p0.i64(ptr, ptr, i64, i1)\n\
         define void @f(ptr %d, ptr %s, i64 %n) {\n  call void @llvm.memcpy.inline.p0.p0.i64(ptr %d, ptr %s, i64 %n, i1 false)\n  ret void\n}\n",
        intrinsic::INLINE_LENGTH,
    );
    refused(
        "declare void @llvm.memset.inline.p0.i64(ptr, i8, i64, i1)\n\
         define void @f(ptr %d, i64 %n) {\n  call void @llvm.memset.inline.p0.i64(ptr %d, i8 1, i64 %n, i1 false)\n  ret void\n}\n",
        intrinsic::INLINE_LENGTH,
    );
    refused(
        "declare ptr @llvm.frameaddress.p0(i32)\n\
         define ptr @f() {\n  %r = call ptr @llvm.frameaddress.p0(i32 1000)\n  ret ptr %r\n}\n",
        intrinsic::FRAME_DEPTH,
    );
    refused(
        "declare i64 @llvm.read_register.i64(metadata)\n\
         define i64 @f() {\n  %r = call i64 @llvm.read_register.i64(metadata !0)\n  ret i64 %r\n}\n\
         !0 = !{!\"rsp\"}\n",
        intrinsic::INTRINSIC,
    );
    // The same intrinsics at 64 and 128 bits compile, and the deepest
    // frame walk.
    object(
        "declare i128 @llvm.ctpop.i128(i128)\ndeclare i64 @llvm.ctpop.i64(i64)\n\
         declare ptr @llvm.returnaddress(i32)\n\
         define i128 @f(i128 %a) {\n  %r = call i128 @llvm.ctpop.i128(i128 %a)\n  ret i128 %r\n}\n\
         define i64 @g(i64 %a) {\n  %r = call i64 @llvm.ctpop.i64(i64 %a)\n  ret i64 %r\n}\n\
         define ptr @h() {\n  %r = call ptr @llvm.returnaddress(i32 256)\n  ret ptr %r\n}\n",
        Stress::default(),
    );
}

/// `intrinsic::begin`/`dropped` (not yet called by `Target`): in a
/// function that calls `is.constant`, the chain only the folded call
/// used is dropped; elsewhere nothing.
#[test]
fn the_operands_of_a_folded_call_are_marked_dropped() {
    let text = format!(
        "{}declare i1 @llvm.is.constant.i8(i8)\n\
         define i32 @f(ptr %p, i32 %x) {{\n  %v = load i8, ptr %p\n  %w = add i8 %v, 1\n  \
         %c = call i1 @llvm.is.constant.i8(i8 %w)\n  %z = zext i1 %c to i32\n  %y = add i32 %x, 1\n  \
         %r = add i32 %z, %y\n  ret i32 %r\n}}\n",
        ll_header()
    );
    let (_, bodies) = parse(&text);
    let body = &bodies[0];
    intrinsic::begin_body(body);
    let marked: Vec<usize> =
        body.insts().filter(|&i| intrinsic::dropped(i)).map(|i| i.id().index()).collect();
    assert_eq!(marked, [0, 1], "the load and the add, not the rest");
}

// ---- the memory intrinsics ----

mod mem_intrinsics {
    use std::cell::RefCell;
    use std::collections::BTreeSet;
    use std::fmt::Write as _;
    use std::rc::Rc;

    use hod_cg::Stress;
    use hod_interp::{Interp, Program, Val};
    use hod_testutil::Scratch;

    use crate::int_gen::{ll_header, llc, parse, pic};
    use crate::intrinsic_mem::{BUF, Func, generate, systematic};
    use crate::linking::Toolchain;

    /// The module header and the declarations.
    fn header(decls: BTreeSet<String>) -> String {
        let mut text = ll_header();
        for d in decls {
            writeln!(text, "{d}").unwrap();
        }
        text
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
            "ptr {buf}, i64 {n}, i32 {}, i8 {}, i64 {}",
            m as u32 as i32, v as u8 as i8, x as i64
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

    /// The C driver: the buffer is the last `BUF` bytes of a page whose
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
                "uint64_t {}(unsigned char *, uint64_t, uint64_t, uint64_t, uint64_t);",
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
                "  {{ uint64_t r = {name}(buf, {n:#x}ULL, {:#x}ULL, {:#x}ULL, {x:#x}ULL); \
                 printf(\"f {name}\\nr %llx\\nb \", (unsigned long long)r); \
                 for (int i = 0; i < {BUF}; i++) printf(\"%02x\", buf[i]); printf(\"\\n\"); }}",
                m | 0xdead_beef_0000_0000,
                v | 0x5a5a_5a5a_5a5a_5a00,
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

    /// Runs `funcs` in every mode against the interpreter; the number
    /// checked (0 without the tools).
    fn check(label: &str, funcs: &[Func], decls: BTreeSet<String>) -> usize {
        let header = header(decls);
        let want = expected(funcs, &header);
        let scratch = Scratch::new(&format!("x64-intrinsic-mem-{label}"));
        let Some(toolchain) = Toolchain::x86_64(&scratch) else { return 0 };
        let Some(linker) = toolchain.linkers.first() else { return 0 };
        let text = native_text(funcs, &header);
        let (module, bodies) = parse(&text);
        let driver = toolchain.compile_c(&scratch, "driver", &driver(funcs), &[]);
        let mut objects = Vec::new();
        for (mode, stress) in [("plain", Stress::default()), ("stress", Stress::ALL)] {
            let object = hod_x64::compile_to_object(&module, &bodies, &pic(stress))
                .unwrap_or_else(|e| panic!("{mode}: {e}"));
            objects.push((mode.to_string(), scratch.write(&format!("{mode}.o"), object)));
        }
        for major in hod_testutil::LLVM_MAJORS {
            if let Some(object) = llc(&scratch, major, &format!("llc{major}"), &text) {
                objects.push((format!("llc{major}"), object));
            }
        }
        for (mode, object) in &objects {
            let program = scratch.join(mode);
            let ran = toolchain.link_and_run(linker, &[], &[&driver, object], &program);
            let got = split(&ran.stdout_text());
            for (k, f) in funcs.iter().enumerate() {
                let got = got.get(k).map_or("(nothing: the run stopped)", String::as_str);
                assert_eq!(
                    got, want[k],
                    "{mode} {}: hod_x64 and the interpreter disagree ({:?})\n{}",
                    f.name, ran.status, f.text
                );
            }
            assert!(ran.success(), "{mode}: {:?}\n{}", ran.status, ran.stderr_text());
        }
        funcs.len()
    }

    /// Every operation at every length 0 to 130, at walking alignments
    /// and overlaps, constant and variable lengths.
    #[test]
    fn every_length_and_alignment_agrees() {
        let mut decls = BTreeSet::new();
        let funcs = systematic(&mut decls);
        let checked = check("systematic", &funcs, decls);
        eprintln!("x64 intrinsic mem: {checked} systematic functions agree in 4 modes");
    }

    /// Random functions of one to four operations agree with
    /// `hod_interp`, plainly, stressed and through `llc` 22 and 19.
    #[test]
    fn random_memory_intrinsics_agree() {
        let mut decls = BTreeSet::new();
        let funcs: Vec<Func> =
            (0..600).map(|seed| generate(&format!("m{seed}"), seed, &mut decls)).collect();
        let checked = check("random", &funcs, decls);
        eprintln!("x64 intrinsic mem: {checked} random functions agree in 4 modes");
    }
}

// ---- what the interpreter does not run: checked by the driver ----

mod native_forms {
    use std::path::PathBuf;

    use hod_cg::Stress;
    use hod_testutil::Scratch;

    use crate::int_gen::{ll_header, llc, parse, pic};
    use crate::linking::Toolchain;

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
declare i64 @llvm.readcyclecounter()
declare void @llvm.clear_cache(ptr, ptr)

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
  call void @llvm.clear_cache(ptr %p, ptr %p)
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

; Two readings of the counter, 1 if the second is not below the first.
define i32 @cycles() {
  %a = call i64 @llvm.readcyclecounter()
  %b = call i64 @llvm.readcyclecounter()
  %c = icmp uge i64 %b, %a
  %r = zext i1 %c to i32
  ret i32 %r
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
int cycles(void);

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
  printf("cycles %d\n", cycles());
  return 0;
}
"#;

    const WANT: &str = "frame1 1\nchain0 1\nchain1 1\nret0 1\nret1 1\nlive 42\n\
                        sizes fffffffe00000001\nconstants 102\nmasked 1\ncycles 1\n";

    /// The frame walk, the folded intrinsics, the counter and the traps,
    /// natively: hod plainly and stressed, and `llc` 22 and 19, under
    /// every linker, against what the driver expects. `trap` and
    /// `ubsantrap` end the process with `SIGILL` (`ud2`; `llc`'s `ud1`
    /// for the second), `debugtrap` with `SIGTRAP`.
    #[test]
    fn the_native_forms_hold() {
        use std::os::unix::process::ExitStatusExt as _;
        let scratch = Scratch::new("x64-intrinsic-native");
        let Some(toolchain) = Toolchain::x86_64(&scratch) else { return };
        let text = format!("{}{FUNCTIONS}", ll_header());
        let (module, bodies) = parse(&text);
        let driver = toolchain.compile_c(&scratch, "driver", DRIVER, &["-fno-omit-frame-pointer"]);
        let mut objects: Vec<(String, PathBuf)> = Vec::new();
        for (mode, stress) in [("plain", Stress::default()), ("stress", Stress::ALL)] {
            let object = hod_x64::compile_to_object(&module, &bodies, &pic(stress)).unwrap();
            objects.push((mode.to_string(), scratch.write(&format!("{mode}.o"), object)));
        }
        for major in hod_testutil::LLVM_MAJORS {
            if let Some(object) = llc(&scratch, major, &format!("llc{major}"), &text) {
                objects.push((format!("llc{major}"), object));
            }
        }
        for linker in &toolchain.linkers {
            for (mode, object) in &objects {
                let program = scratch.join(format!("{mode}-{}", linker.name));
                let ran = toolchain.link_and_run(linker, &[], &[&driver, object], &program);
                assert!(ran.success(), "{mode}: {:?}\n{}", ran.status, ran.stderr_text());
                assert_eq!(ran.stdout_text(), WANT, "{mode} with {}", linker.name);
                for (arg, signal) in [("trap", 4), ("debugtrap", 5), ("ubsantrap", 4)] {
                    let ran = hod_testutil::run_with(
                        toolchain.runner_command(&program).arg(arg),
                        &[],
                        std::time::Duration::from_mins(1),
                    );
                    assert_eq!(
                        ran.status.and_then(|s| s.signal()),
                        Some(signal),
                        "{mode} {arg}: {:?}",
                        ran.status
                    );
                }
            }
        }
    }
}
