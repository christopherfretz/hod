//! The x86-64 integer group's programs (WP-122): a systematic table of
//! one-instruction functions (every operation at every width, with
//! register and constant operands) and a generator of random
//! straight-line integer functions of their arguments and constants
//! (with unconditional `br`s between blocks). Both are `.ll` text, so the
//! same functions go to `hod_interp`, to `hod_x64` and to `llc`.
//!
//! The tables and the generator are shared with `hod_a64`
//! (`crates/hod_testutil/gen/int_gen.rs`, WP-130); this file adds the
//! x86-64 compile, link and run. What differs: the shared edge values
//! include the boundaries of x86's immediates (`imm8`, `imm32`), a
//! plain `i1` argument has random bits above bit 0 and an `i1` result is
//! compared at bit 0 (`spec/abi-x64.md` §7: a plain `i1` may have
//! anything in bits 1..8), and [`run`] links for x86-64 and runs under
//! the emulator.
//!
//! [`run`] checks a [`Suite`]: every case (a function and its arguments)
//! is run by the interpreter, and from ONE C driver per object (one
//! emulated process per object) compiled with hod (plainly and with every
//! stress option) and with `llc -O0 -fast-isel=0 -relocation-model=pic`
//! of LLVM 22 and 19. A case the interpreter traps on (division by zero,
//! the smallest value divided by -1, a shift of the width or more:
//! undefined in LLVM) is not run (`div` traps on x86).

#![allow(clippy::many_single_char_names, clippy::too_many_lines, clippy::cast_precision_loss)]
#![allow(dead_code, reason = "each test uses part of it")]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use hod_cg::{Options, RelocModel, Stress};
use hod_interp::{Interp, Program, Val};
use hod_testutil::{Scratch, run_with};
use hod_x64::compile_to_object;

use crate::linking::Toolchain;

/// The header of a `.ll` module for x86-64 Linux.
pub fn ll_header() -> String {
    format!(
        "target datalayout = \"{}\"\ntarget triple = \"x86_64-unknown-linux-gnu\"\n",
        hod_ir::X86_64_LINUX_DATA_LAYOUT
    )
}

/// A module and its bodies from `.ll` text (target lines included).
pub fn parse(text: &str) -> (hod_ir::Module, Vec<hod_ir::Body>) {
    let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
        .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    let text_side = parsed.text;
    let mut module = parsed.module;
    let mut bodies = Vec::new();
    hod_ll::parse_bodies(&mut module, &text_side, |_, parsed| {
        let body =
            parsed.body.unwrap_or_else(|| panic!("hod_ll: unsupported {:?}", parsed.unsupported));
        bodies.push(body.clone());
        Ok::<(), hod_ll::LlError>(())
    })
    .unwrap_or_else(|e| panic!("hod_ll: {e}"));
    (module, bodies)
}

/// Position-independent code, as every x86-64 test links it (PIE).
pub fn pic(stress: Stress) -> Options {
    Options { reloc_model: RelocModel::Pic, stress, ..Options::default() }
}

/// `llc` of LLVM `major`, if installed.
pub fn llc_of(major: u32) -> Option<PathBuf> {
    let installed = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/llc"));
    if installed.is_file() {
        return Some(installed);
    }
    hod_testutil::find_tool(&format!("llc-{major}"))
}

/// `llc -O0 -fast-isel=0 -relocation-model=pic` for x86-64 of `text` to
/// an object in `dir`; `None` without that `llc`.
pub fn llc(dir: &Scratch, major: u32, name: &str, text: &str) -> Option<PathBuf> {
    let llc = llc_of(major)?;
    let input = dir.write(&format!("{name}.ll"), text);
    let output = dir.join(format!("{name}.o"));
    let mut command = Command::new(llc);
    command
        .args([
            "-O0",
            "-fast-isel=0",
            "-mtriple=x86_64-unknown-linux-gnu",
            "-relocation-model=pic",
            "-filetype=obj",
        ])
        .arg(&input)
        .arg("-o")
        .arg(&output);
    let outcome = run_with(&mut command, &[], Duration::from_mins(5));
    assert!(outcome.success(), "llc {major} failed:\n{}", outcome.stderr_text());
    Some(output)
}

#[path = "../../../hod_testutil/gen/int_gen.rs"]
mod table;
pub use table::*;

/// The bits of a result the driver compares: its width (an `i1` at
/// bit 0, `spec/abi-x64.md` §7).
fn result_mask(ty: Ty) -> u64 {
    mask(ty.bits())
}

// ---- running a suite ----

/// The C driver: every case that is defined in a table, run in order,
/// each result printed in hex at its width.
fn driver(suite: &Suite, defined: &[bool], rng: &mut Rng) -> String {
    let mut c = String::from(
        "#include <stdio.h>\n#include <stdint.h>\n\
         typedef uint64_t (*fn_t)(uint64_t, uint64_t, uint64_t);\n",
    );
    for f in &suite.funcs {
        writeln!(c, "uint64_t {}(uint64_t, uint64_t, uint64_t);", f.name).unwrap();
    }
    c.push_str("static const struct { fn_t f; uint64_t mask, a, b, c; } cases[] = {\n");
    for (case, _) in suite.cases.iter().zip(defined).filter(|(_, d)| **d) {
        let f = &suite.funcs[case.func];
        let mut regs = [0u64; 3];
        for (i, &ty) in f.params.iter().enumerate() {
            let garbage = rng.next();
            regs[i] = match ty {
                Ty::Ptr => case.args[i],
                // A plain `i1`: bit 0, anything above (`spec/abi-x64.md` §7).
                Ty::I(1) => garbage & !1 | case.args[i] & 1,
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
        "  { 0, 0, 0, 0, 0 }\n};\nint main(void) {\n  for (unsigned long i = 0; cases[i].f; i++) {\n    \
         uint64_t r = cases[i].f(cases[i].a, cases[i].b, cases[i].c) & cases[i].mask;\n    \
         printf(\"%llx\\n\", (unsigned long long)r);\n  }\n  return 0;\n}\n",
    );
    c
}

/// What [`run`] found.
#[derive(Debug, Default)]
pub struct Report {
    /// Cases compared.
    pub compared: usize,
    /// Cases the interpreter trapped on (undefined; not run).
    pub undefined: usize,
    /// Disagreements: leg, function, arguments, interpreter, native.
    pub wrong: Vec<String>,
    /// Bytes of code hod and `llc` 22 emitted.
    pub hod_bytes: usize,
    /// `llc`'s `.text` bytes (0 without `llc`).
    pub llc_bytes: usize,
    /// IR instructions of the suite.
    pub insts: usize,
    /// Time hod took to compile the suite, plain.
    pub compile: Duration,
    /// Time the interpreter took.
    pub interp: Duration,
    /// Time the tools took (`llc`, the C compiler, the linker, the runs).
    pub tools: Duration,
    /// The legs that ran.
    pub legs: Vec<String>,
}

/// The size of `.text` in `object`, by `llvm-objdump -h`.
pub fn text_size(object: &Path) -> usize {
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
fn interpret(suite: &Suite) -> Vec<Option<u64>> {
    let mut expected = vec![None; suite.cases.len()];
    let mut by_func: Vec<Vec<usize>> = vec![Vec::new(); suite.funcs.len()];
    for (index, case) in suite.cases.iter().enumerate() {
        by_func[case.func].push(index);
    }
    for (chunk, funcs) in suite.funcs.chunks(64).enumerate() {
        let mut text = ll_header();
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
                    Ok(Some(Val::Int { value, .. })) => Some(value as u64 & result_mask(f.ret)),
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
/// scratch directory. Without the cross toolchain only the compile (and
/// the interpreter) runs.
pub fn run(name: &str, suite: &Suite) -> Report {
    let text = format!("{}{}", ll_header(), suite.text);
    let (module, bodies) = parse(&text);
    let mut report =
        Report { insts: bodies.iter().map(hod_ir::Body::inst_count).sum(), ..Report::default() };
    let start = Instant::now();
    let expected = interpret(suite);
    report.interp = start.elapsed();
    let defined: Vec<bool> = expected.iter().map(Option::is_some).collect();
    report.undefined = defined.iter().filter(|d| !**d).count();
    report.compared = expected.len() - report.undefined;

    let scratch = Scratch::new(&format!("x64-int-{name}"));
    let mut legs: Vec<(String, PathBuf)> = Vec::new();
    for (mode, stress) in [("plain", Stress::default()), ("stress", Stress::ALL)] {
        let start = Instant::now();
        let object = compile_to_object(&module, &bodies, &pic(stress))
            .unwrap_or_else(|e| panic!("{mode}: {e}"));
        if mode == "plain" {
            report.compile = start.elapsed();
        }
        let path = scratch.write(&format!("hod-{mode}.o"), object);
        if mode == "plain" {
            report.hod_bytes = text_size(&path);
        }
        legs.push((format!("hod {mode}"), path));
    }
    let start = Instant::now();
    for major in hod_testutil::LLVM_MAJORS {
        // `ptrtoaddr` is new in LLVM 21.
        if major < 21 && text.contains("ptrtoaddr") {
            continue;
        }
        if let Some(object) = llc(&scratch, major, &format!("llc{major}"), &text) {
            if major == 22 {
                report.llc_bytes = text_size(&object);
            }
            legs.push((format!("llc {major}"), object));
        }
    }
    let Some(toolchain) = Toolchain::x86_64(&scratch) else { return report };
    let Some(linker) = toolchain.linkers.first() else { return report };
    let mut rng = Rng(0x1234_5678);
    let driver = toolchain.compile_c(&scratch, "driver", &driver(suite, &defined, &mut rng), &[]);
    let want: Vec<(usize, u64)> =
        expected.iter().enumerate().filter_map(|(i, e)| e.map(|v| (i, v))).collect();
    for (leg, object) in &legs {
        let program = scratch.join(leg.replace(' ', "-"));
        let ran = toolchain.link_and_run(linker, &[], &[&driver, object], &program);
        assert!(ran.success(), "{leg}: {:?}\n{}", ran.status, ran.stderr_text());
        let got: Vec<u64> = ran
            .stdout_text()
            .lines()
            .map(|line| u64::from_str_radix(line, 16).unwrap_or_else(|_| panic!("{leg}: {line}")))
            .collect();
        assert_eq!(got.len(), want.len(), "{leg}: one line per defined case");
        for (&(index, want), got) in want.iter().zip(&got) {
            let case = &suite.cases[index];
            let f = &suite.funcs[case.func];
            if want != *got && (f.llc || !leg.starts_with("llc")) {
                report.wrong.push(format!(
                    "{leg}: {}({:#x}, {:#x}, {:#x}): interpreter {want:#x}, native {got:#x}",
                    f.name, case.args[0], case.args[1], case.args[2]
                ));
            }
        }
        report.legs.push(leg.clone());
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
