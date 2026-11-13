//! The integer group's own programs (WP-035): a systematic table of
//! one-instruction functions (every operation at every width, with
//! register and constant operands) and a generator of random
//! straight-line integer functions of their arguments and constants
//! (with unconditional `br`s between blocks). Both are `.ll` text, so the
//! same functions go to `hod_interp`, to `hod_a64` and to `llc`.
//!
//! [`run`] checks a [`Suite`]: every case (a function and its arguments)
//! is run by the interpreter, and natively from one C driver compiled
//! with hod (plainly and with every stress option) and with `llc -O0
//! -global-isel=0 -fast-isel=0`. A case the interpreter traps on
//! (division by zero, the smallest value divided by -1, a shift of the
//! width or more: undefined in LLVM) is run but not compared.
//!
//! Arguments arrive with the bits above their width set at random, as
//! AAPCS64 allows (`spec/abi-a64.md` §7; an `i1` is a byte holding 0 or
//! 1, with random bits above the byte). Results are compared at their
//! width; an `i1` result at 8 bits, since its producer extends it.

#![allow(clippy::many_single_char_names, clippy::too_many_lines, clippy::cast_precision_loss)]
#![allow(dead_code, reason = "each test uses part of it")]

use std::fmt::Write as _;
use std::process::Command;
use std::time::{Duration, Instant};

use hod_a64::compile_module;
use hod_cg::{Options, Stress};
use hod_interp::{Interp, Program, Val};
use hod_testutil::{Scratch, run_with};

use crate::common::linking::Toolchain;
use crate::common::{ll_header, llc, llc_major, parse, write_object};

#[path = "../../../hod_testutil/gen/int_gen.rs"]
mod table;
pub use table::*;

/// The bits of a result the driver compares: an `i1` at 8 bits.
fn result_mask(ty: Ty) -> u64 {
    if ty == Ty::I(1) { 0xff } else { mask(ty.bits()) }
}

// ---- running a suite ----

/// What one native leg printed, per case.
type Lines = Vec<u64>;

/// The C driver: every case in a table, run in order, each result
/// printed in hex at its width.
fn driver(suite: &Suite, rng: &mut Rng) -> String {
    let mut c = String::from(
        "#include <stdio.h>\n#include <stdint.h>\n\
         typedef uint64_t (*fn_t)(uint64_t, uint64_t, uint64_t);\n",
    );
    for f in &suite.funcs {
        writeln!(c, "uint64_t {}(uint64_t, uint64_t, uint64_t);", f.name).unwrap();
    }
    c.push_str("static const struct { fn_t f; uint64_t mask, a, b, c; } cases[] = {\n");
    for case in &suite.cases {
        let f = &suite.funcs[case.func];
        let mut regs = [0u64; 3];
        for (i, &ty) in f.params.iter().enumerate() {
            let garbage = rng.next();
            regs[i] = match ty {
                Ty::Ptr => case.args[i],
                // A byte holding 0 or 1 (`spec/abi-a64.md` §7).
                Ty::I(1) => garbage & !0xff | case.args[i] & 1,
                Ty::I(bits) => garbage & !mask(bits) | case.args[i],
            };
        }
        writeln!(
            c,
            "  {{ {}, {:#x}ULL, {:#x}ULL, {:#x}ULL, {:#x}ULL }},",
            f.name,
            result_mask(f.ret),
            regs[0],
            regs[1],
            regs[2]
        )
        .unwrap();
    }
    c.push_str(
        "};\nint main(void) {\n  for (unsigned long i = 0; i < sizeof cases / sizeof cases[0]; i++) {\n    \
         uint64_t r = cases[i].f(cases[i].a, cases[i].b, cases[i].c) & cases[i].mask;\n    \
         printf(\"%llx\\n\", (unsigned long long)r);\n  }\n  return 0;\n}\n",
    );
    c
}

/// Runs `program` and parses one hex number per line.
fn lines(program: &std::path::Path, what: &str) -> Lines {
    let ran = run_with(&mut Command::new(program), &[], Duration::from_mins(2));
    assert!(ran.success(), "{what}: {:?}\n{}", ran.status, ran.stderr_text());
    ran.stdout_text()
        .lines()
        .map(|line| u64::from_str_radix(line, 16).unwrap_or_else(|_| panic!("{what}: {line}")))
        .collect()
}

/// What [`run`] found.
#[derive(Debug, Default)]
pub struct Report {
    /// Cases compared.
    pub compared: usize,
    /// Cases the interpreter trapped on (undefined; run, not compared).
    pub undefined: usize,
    /// Disagreements: leg, function, arguments, interpreter, native.
    pub wrong: Vec<String>,
    /// Bytes of code hod and `llc` emitted, and the IR instructions.
    pub hod_bytes: usize,
    /// `llc`'s `.text` bytes (0 without `llc`).
    pub llc_bytes: usize,
    /// IR instructions of the suite.
    pub insts: usize,
    /// Time hod took to compile the suite, plain.
    pub compile: Duration,
    /// Time the interpreter took, and the tools (`llc`, the C compiler,
    /// the linker, the runs).
    pub interp: Duration,
    /// See `interp`.
    pub tools: Duration,
}

/// The size of `.text` in `object`, by `llvm-objdump -h`.
fn text_size(object: &std::path::Path) -> usize {
    let Some(objdump) = hod_testutil::tool("llvm-objdump") else { return 0 };
    let mut command = Command::new(objdump);
    command.arg("-h").arg(object);
    let out = run_with(&mut command, &[], Duration::from_mins(1));
    out.stdout_text()
        .lines()
        .find_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            (fields.get(1) == Some(&".text"))
                .then(|| usize::from_str_radix(fields[2], 16).unwrap_or(0))
        })
        .unwrap_or(0)
}

/// The interpreter's result of each case of `suite`; `None` where it
/// traps. Functions are interpreted 64 to a module: a run's cost grows
/// with the module's symbols.
fn interpret(suite: &Suite, header: &str) -> Vec<Option<u64>> {
    let mut expected = vec![None; suite.cases.len()];
    let mut by_func: Vec<Vec<usize>> = vec![Vec::new(); suite.funcs.len()];
    for (index, case) in suite.cases.iter().enumerate() {
        by_func[case.func].push(index);
    }
    for (chunk, funcs) in suite.funcs.chunks(64).enumerate() {
        let mut text = header.to_string();
        for f in funcs {
            text.push_str(&suite.text[f.text.clone()]);
        }
        let (module, bodies) = parse(&text);
        let program = Program::with_bodies(&module, &bodies).expect("a valid program");
        let mut interp = Interp::new(&program);
        for (offset, f) in funcs.iter().enumerate() {
            let sym = module.find_symbol(f.name.as_bytes()).expect("a defined function");
            for &index in &by_func[chunk * 64 + offset] {
                let args: Vec<Val> = f
                    .params
                    .iter()
                    .zip(suite.cases[index].args)
                    .map(|(&ty, v)| match ty {
                        Ty::Ptr => Val::Ptr(v),
                        Ty::I(bits) => Val::int(bits, u128::from(v)),
                    })
                    .collect();
                expected[index] = match interp.run(sym, &args).result {
                    Ok(Some(Val::Int { value, .. })) => Some(value as u64),
                    Ok(Some(Val::Ptr(addr))) => Some(addr),
                    Ok(other) => panic!("{}: {other:?}", f.name),
                    Err(_) => None,
                };
            }
        }
    }
    expected
}

/// Checks `suite` (see the module documentation). `name` labels the
/// scratch directory.
pub fn run(name: &str, suite: &Suite) -> Report {
    let major = llc_major();
    let text = format!("{}{}", ll_header(major), suite.text);
    let (module, bodies) = parse(&text);
    let mut report =
        Report { insts: bodies.iter().map(hod_ir::Body::inst_count).sum(), ..Report::default() };
    // The interpreter.
    let start = Instant::now();
    let expected = interpret(suite, &ll_header(major));
    report.interp = start.elapsed();
    let start = Instant::now();
    report.undefined = expected.iter().filter(|e| e.is_none()).count();
    report.compared = expected.len() - report.undefined;

    let scratch = Scratch::new(&format!("a64-int-{name}"));
    let mut legs: Vec<(String, std::path::PathBuf)> = Vec::new();
    for (mode, stress) in [("plain", Stress::default()), ("stress", Stress::ALL)] {
        let options = Options { stress, ..Options::default() };
        let start = Instant::now();
        let emitted =
            compile_module(&module, &bodies, &options).unwrap_or_else(|e| panic!("{mode}: {e}"));
        if mode == "plain" {
            report.compile = start.elapsed();
        }
        let object = write_object(&scratch, &format!("hod-{mode}.o"), &emitted);
        if mode == "plain" {
            report.hod_bytes = text_size(&object);
        }
        legs.push((format!("hod {mode}"), object));
    }
    if let Some(object) = llc(&scratch, "llc", &text, &[]) {
        report.llc_bytes = text_size(&object);
        legs.push(("llc".to_string(), object));
    }
    let Some(toolchain) = Toolchain::host(&scratch) else { return report };
    let Some(linker) = toolchain.linkers.first() else { return report };
    let mut rng = Rng(0x1234_5678);
    let driver = toolchain.compile_c(&scratch, "driver", &driver(suite, &mut rng), &[]);
    for (leg, object) in &legs {
        let program = scratch.join(leg.replace(' ', "-"));
        toolchain.link_ok(linker, &[], &[&driver, object], &program);
        let got = lines(&program, leg);
        assert_eq!(got.len(), suite.cases.len(), "{leg}: one line per case");
        for ((case, want), got) in suite.cases.iter().zip(&expected).zip(&got) {
            let f = &suite.funcs[case.func];
            if let Some(want) = want
                && want != got
                && (f.llc || leg != "llc")
            {
                report.wrong.push(format!(
                    "{leg}: {}({:#x}, {:#x}, {:#x}): interpreter {want:#x}, native {got:#x}",
                    f.name, case.args[0], case.args[1], case.args[2]
                ));
            }
        }
    }
    report.tools = start.elapsed();
    report
}

/// The body of function `name` in `suite`'s text, for a failure message.
pub fn body_of<'s>(suite: &'s Suite, name: &str) -> &'s str {
    let start = suite.text.find(&format!("@{name}(")).unwrap_or(0);
    let end = suite.text[start..].find("\n}\n").map_or(suite.text.len(), |e| start + e + 3);
    &suite.text[start..end]
}

/// Panics with the first disagreements of `report`, with their
/// functions' text.
pub fn assert_agrees(suite: &Suite, report: &Report) {
    if report.wrong.is_empty() {
        return;
    }
    let mut message = format!("{} disagreements; the first:\n", report.wrong.len());
    for line in report.wrong.iter().take(12) {
        let name = line.split(": ").nth(1).and_then(|s| s.split('(').next()).unwrap_or("");
        writeln!(message, "{line}\n{}", body_of(suite, name)).unwrap();
    }
    panic!("{message}");
}
