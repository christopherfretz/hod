//! 16-byte atomics with `+cx16` (WP-133): every `atomicrmw` operation
//! under every ordering, loads, stores, `cmpxchg` strong and weak under
//! every pair of orderings, on `i128`; each function's wrapper reads its
//! operands from memory and writes `{ old, new, flag }`, run from a C
//! driver on a runner whose CPU model has `cmpxchg16b` (the baseline
//! `qemu64` model of the other tests lacks it), compared byte for byte
//! with `hod_interp` (the twin) for hod (plain and under every stress
//! option) and `llc` 22 and 19 (whose functions carry the same
//! `"target-features"="+cx16"`).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hod_interp::{Interp, Program, Stop, Val};
use hod_testutil::target::Target;
use hod_testutil::{Scratch, run_with};

use crate::aggr_gen::{PIE_FLAGS, Rng, bytes_of, edges, header, llc, parse, pic, value};
use crate::linking::Toolchain;
use crate::{FAIL_ORDERS, INT_OPS, RMW_ORDERS};

/// The attribute group every function of these modules carries.
pub const CX16: &str = "attributes #0 = { \"target-features\"=\"+cx16\" }\n";

/// The bytes of a result compared: `old` (16), `new` (16), the flag (1).
const RESULT: usize = 33;

/// The command running `program` on x86-64 with `cmpxchg16b`: the
/// emulator with the baseline model plus `cx16` on another host, the
/// program itself on an x86-64 host. `None` without the emulator.
pub fn cx16_runner(program: &Path) -> Option<Command> {
    let target = Target::X86_64;
    if target.is_host() {
        return Some(Command::new(program));
    }
    let sysroot = target.tool("sysroot")?;
    let mut command = Command::new(target.tool("emulator")?);
    command.args(target.emulator_args_with(&sysroot, &["cx16"])).arg(program);
    Some(command)
}

/// One function: its core (defining `%o`, `%n` of `i128` and `%k` of
/// `i8` from `%a`, `%b`, `%c`) and its cases (`a`, `b`, `c`).
struct Func {
    core: String,
    cases: Vec<[u128; 3]>,
}

impl Func {
    /// Whether LLVM 19 reads it: `usub_cond` and `usub_sat` are LLVM
    /// 20's.
    fn llvm19(&self) -> bool {
        !self.core.contains("usub_")
    }
}

fn word(w: &[u64]) -> u128 {
    u128::from(w[0]) | (u128::from(w[1]) << 64)
}

fn pick(rng: &mut Rng) -> u128 {
    word(&value(rng, 128))
}

/// The table: every operation of [`INT_OPS`] under every ordering, the
/// result used and (the arithmetic ones) unused, a constant operand;
/// loads and stores under every ordering; `cmpxchg` strong and weak.
#[allow(clippy::too_many_lines, reason = "a test table: `.ll` text built by hand")]
fn table(rng: &mut Rng) -> Vec<Func> {
    let mut out = Vec::new();
    let e: Vec<u128> = edges(128).iter().map(|w| word(w)).collect();
    let cases = |rng: &mut Rng, same: bool| {
        let mut c: Vec<[u128; 3]> = Vec::new();
        for _ in 0..4 {
            let a = pick(rng);
            let b = if same && rng.chance(50) { a } else { pick(rng) };
            c.push([a, b, pick(rng)]);
        }
        c.push([e[rng.below(e.len() as u64) as usize], e[rng.below(e.len() as u64) as usize], 0]);
        c
    };
    for op in INT_OPS {
        for order in RMW_ORDERS {
            out.push(Func {
                core: format!(
                    "  store i128 %a, ptr %p, align 16\n  \
                     %o = atomicrmw {op} ptr %p, i128 %b {order}, align 16\n  \
                     %n = load i128, ptr %p, align 16\n  %k = add i8 0, 0\n"
                ),
                cases: cases(rng, true),
            });
        }
        out.push(Func {
            core: format!(
                "  store i128 %a, ptr %p, align 16\n  \
                 %u = atomicrmw {op} ptr %p, i128 %b monotonic, align 16\n  \
                 %n = load i128, ptr %p, align 16\n  %o = add i128 0, 0\n  %k = add i8 0, 0\n"
            ),
            cases: cases(rng, true),
        });
    }
    // Edges of the operations that compare.
    for op in ["max", "min", "umax", "umin", "uinc_wrap", "udec_wrap", "usub_cond", "usub_sat"] {
        let mut c = Vec::new();
        for &x in e.iter().take(10) {
            for &y in e.iter().take(10) {
                c.push([x, y, 0]);
            }
        }
        out.push(Func {
            core: format!(
                "  store i128 %a, ptr %p, align 16\n  \
                 %o = atomicrmw {op} ptr %p, i128 %b seq_cst, align 16\n  \
                 %n = load i128, ptr %p, align 16\n  %k = add i8 0, 0\n"
            ),
            cases: c,
        });
    }
    for (op, k) in [("add", "1"), ("sub", "-1"), ("xchg", "18446744073709551616"), ("and", "-2")] {
        out.push(Func {
            core: format!(
                "  store i128 %a, ptr %p, align 16\n  \
                 %o = atomicrmw {op} ptr %p, i128 {k} acq_rel, align 16\n  \
                 %n = load i128, ptr %p, align 16\n  %k = add i8 0, 0\n"
            ),
            cases: cases(rng, false),
        });
    }
    // `usub_cond` and `usub_sat` of 1..8 bytes, here rather than in the
    // main table because LLVM 19 does not read them (that table's legs
    // include `llc` 19): the operands truncated, the results extended.
    for bits in [8, 16, 32, 64] {
        let ew: Vec<u128> = edges(bits).iter().map(|w| u128::from(w[0])).collect();
        for op in ["usub_cond", "usub_sat"] {
            for order in RMW_ORDERS {
                let mut c = cases(rng, true);
                for &x in ew.iter().take(8) {
                    for &y in ew.iter().take(8) {
                        c.push([x, y, 0]);
                    }
                }
                out.push(Func {
                    core: format!(
                        "  %an = trunc i128 %a to i{bits}\n  %bn = trunc i128 %b to i{bits}\n  \
                         store i{bits} %an, ptr %p, align 16\n  \
                         %on = atomicrmw {op} ptr %p, i{bits} %bn {order}, align {a}\n  \
                         %nn = load i{bits}, ptr %p, align {a}\n  \
                         %o = zext i{bits} %on to i128\n  %n = zext i{bits} %nn to i128\n  \
                         %k = add i8 0, 0\n",
                        a = bits / 8
                    ),
                    cases: c,
                });
            }
        }
    }
    for (order, volatile) in [
        ("unordered", ""),
        ("monotonic", ""),
        ("acquire", ""),
        ("seq_cst", ""),
        ("seq_cst", "volatile "),
    ] {
        out.push(Func {
            core: format!(
                "  store i128 %a, ptr %p, align 16\n  \
                 %o = load atomic {volatile}i128, ptr %p {order}, align 16\n  \
                 %n = load i128, ptr %p, align 16\n  %k = add i8 0, 0\n"
            ),
            cases: cases(rng, false),
        });
    }
    for (order, volatile) in [
        ("unordered", ""),
        ("monotonic", ""),
        ("release", ""),
        ("seq_cst", ""),
        ("release", "volatile "),
    ] {
        out.push(Func {
            core: format!(
                "  store i128 %a, ptr %p, align 16\n  \
                 store atomic {volatile}i128 %b, ptr %p {order}, align 16\n  \
                 %n = load i128, ptr %p, align 16\n  %o = add i128 0, 0\n  %k = add i8 0, 0\n"
            ),
            cases: cases(rng, false),
        });
    }
    for weak in ["", "weak "] {
        for success in RMW_ORDERS {
            for failure in FAIL_ORDERS {
                out.push(Func {
                    core: format!(
                        "  store i128 %a, ptr %p, align 16\n  \
                         %x = cmpxchg {weak}ptr %p, i128 %b, i128 %c {success} {failure}, align 16\n  \
                         %o = extractvalue {{ i128, i1 }} %x, 0\n  \
                         %f = extractvalue {{ i128, i1 }} %x, 1\n  \
                         %k = zext i1 %f to i8\n  %n = load i128, ptr %p, align 16\n"
                    ),
                    cases: cases(rng, true),
                });
            }
        }
    }
    out
}

/// The module: each wrapper `@w<k>(ptr %out, ptr %in)` and, with
/// `twins`, each `@w<k>_v(i128, i128, i128) -> { i128, i128, i8 }`.
fn module(funcs: &[Func], twins: bool, keep: impl Fn(&Func) -> bool) -> String {
    let mut text = header();
    for (k, f) in funcs.iter().enumerate().filter(|(_, f)| keep(f)) {
        write!(
            text,
            "define void @w{k}(ptr %out, ptr %in) #0 {{\nentry:\n  \
             %p = alloca i128, align 16\n  %a = load i128, ptr %in, align 16\n  \
             %ib = getelementptr i8, ptr %in, i64 16\n  %b = load i128, ptr %ib, align 16\n  \
             %ic = getelementptr i8, ptr %in, i64 32\n  %c = load i128, ptr %ic, align 16\n\
             {}  store i128 %o, ptr %out, align 16\n  \
             %out.n = getelementptr i8, ptr %out, i64 16\n  store i128 %n, ptr %out.n, align 16\n  \
             %out.k = getelementptr i8, ptr %out, i64 32\n  store i8 %k, ptr %out.k, align 1\n  \
             ret void\n}}\n\n",
            f.core
        )
        .unwrap();
        if twins {
            write!(
                text,
                "define {{ i128, i128, i8 }} @w{k}_v(i128 %a, i128 %b, i128 %c) #0 {{\nentry:\n  \
                 %p = alloca i128, align 16\n{}  \
                 %r0 = insertvalue {{ i128, i128, i8 }} poison, i128 %o, 0\n  \
                 %r1 = insertvalue {{ i128, i128, i8 }} %r0, i128 %n, 1\n  \
                 %r = insertvalue {{ i128, i128, i8 }} %r1, i8 %k, 2\n  \
                 ret {{ i128, i128, i8 }} %r\n}}\n\n",
                f.core
            )
            .unwrap();
        }
    }
    text.push_str(CX16);
    text.push_str(PIE_FLAGS);
    text
}

/// The interpreter's result of every case, in order.
fn expected(funcs: &[Func]) -> Vec<Vec<u8>> {
    let (module, bodies) = parse(&module(funcs, true, |_| true));
    let program = Program::with_bodies(&module, &bodies).expect("a valid program");
    let mut interp = Interp::new(&program);
    let mut out = Vec::new();
    for (k, f) in funcs.iter().enumerate() {
        let sym = module.find_symbol(format!("w{k}_v").as_bytes()).expect("a twin");
        for case in &f.cases {
            let args: Vec<Val> = case.iter().map(|&v| Val::int(128, v)).collect();
            match interp.run(sym, &args).result {
                Ok(Some(Val::Agg(agg))) => out.push(agg.bytes()[..RESULT].to_vec()),
                Err(Stop::Trap(t)) => panic!("w{k}: the interpreter trapped: {t:?}\n{}", f.core),
                other => panic!("w{k}: {other:?}\n{}", f.core),
            }
        }
    }
    out
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::new();
    for b in bytes {
        write!(out, "{b:02x}").unwrap();
    }
    out
}

/// The C driver: every case's operands, each wrapper called, the result
/// printed in hex, one line per case.
fn driver(funcs: &[Func]) -> String {
    let mut c = String::from(
        "#include <stdio.h>\n#include <string.h>\n\
         typedef void (*fn)(void *, const void *);\n",
    );
    for k in 0..funcs.len() {
        writeln!(c, "void w{k}(void *, const void *);").unwrap();
    }
    c.push_str("static const struct { fn f; unsigned char in[48]; } cases[] = {\n");
    for (k, f) in funcs.iter().enumerate() {
        for case in &f.cases {
            let bytes: Vec<String> = case
                .iter()
                .flat_map(|v| bytes_of(&[*v as u64, (v >> 64) as u64], 128))
                .map(|b| format!("{b:#04x}"))
                .collect();
            writeln!(c, "  {{ w{k}, {{ {} }} }},", bytes.join(", ")).unwrap();
        }
    }
    write!(
        c,
        "}};\nint main(void) {{\n  \
         for (unsigned i = 0; i < sizeof cases / sizeof cases[0]; i++) {{\n    \
         _Alignas(16) unsigned char in[48], out[48];\n    \
         memcpy(in, cases[i].in, 48);\n    memset(out, 0xa5, sizeof out);\n    \
         cases[i].f(out, in);\n    \
         for (int b = 0; b < {RESULT}; b++) printf(\"%02x\", out[b]);\n    printf(\"\\n\");\n  }}\n  \
         return 0;\n}}\n"
    )
    .unwrap();
    c
}

/// Builds, links and runs every leg; every line must be the
/// interpreter's.
pub fn check() {
    let funcs = table(&mut Rng(0x0133_0016));
    let want = expected(&funcs);
    let text = module(&funcs, false, |_| true);
    let (m, bodies) = parse(&text);
    let scratch = Scratch::new("x64-atomic-wide");
    let mut legs: Vec<(String, Vec<PathBuf>)> = Vec::new();
    for (mode, stress) in [("plain", hod_cg::Stress::default()), ("stress", hod_cg::Stress::ALL)] {
        let bytes = hod_x64::compile_to_object(&m, &bodies, &pic(stress))
            .unwrap_or_else(|e| panic!("{mode}: {e}"));
        legs.push((format!("hod-{mode}"), vec![scratch.write(&format!("hod-{mode}.o"), bytes)]));
    }
    if let Some(object) = llc(&scratch, 22, "llc22", &text) {
        legs.push(("llc22".into(), vec![object]));
    }
    // `llc` 19 compiles what it reads; `llc` 22 the rest.
    if let (Some(old), Some(new)) = (
        llc(&scratch, 19, "llc19", &module(&funcs, false, Func::llvm19)),
        llc(&scratch, 22, "llc22-new", &module(&funcs, false, |f| !f.llvm19())),
    ) {
        legs.push(("llc19".into(), vec![old, new]));
    }
    let Some(toolchain) = Toolchain::x86_64(&scratch) else { return };
    let Some(linker) = toolchain.linkers.first() else { return };
    let driver = toolchain.compile_c(&scratch, "driver", &driver(&funcs), &["-fPIE"]);
    let mut wrong = Vec::new();
    for (leg, objects) in &legs {
        let program = scratch.join(leg);
        let mut inputs: Vec<&Path> = vec![driver.as_path()];
        inputs.extend(objects.iter().map(PathBuf::as_path));
        toolchain.link_ok(linker, &["-pie"], &inputs, &program);
        let Some(mut command) = cx16_runner(&program) else { return };
        let ran = run_with(&mut command, &[], Duration::from_mins(5));
        assert!(ran.success(), "{leg}: {:?}\n{}", ran.status, ran.stderr_text());
        let out = ran.stdout_text();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), want.len(), "{leg}: one line per case");
        let mut i = 0;
        for (k, f) in funcs.iter().enumerate() {
            for case in &f.cases {
                if lines[i] != hex(&want[i]) {
                    wrong.push(format!(
                        "{leg}: w{k}({case:x?}): expected {}, got {}\n{}",
                        hex(&want[i]),
                        lines[i],
                        f.core
                    ));
                }
                i += 1;
            }
        }
    }
    eprintln!(
        "x64 atomic 16 bytes (+cx16): {} functions, {} cases, legs {:?}",
        funcs.len(),
        want.len(),
        legs.iter().map(|(l, _)| l.as_str()).collect::<Vec<_>>()
    );
    assert!(
        wrong.is_empty(),
        "{} disagreements; the first:\n{}",
        wrong.len(),
        wrong[..wrong.len().min(6)].join("\n")
    );
}
