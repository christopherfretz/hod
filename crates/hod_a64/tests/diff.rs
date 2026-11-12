//! The differential harness on the real target (`spec/codegen.md` §12):
//! programs are run by `hod_interp` and, compiled by `hod_a64`, natively;
//! the result and the trace of `sink` calls must be equal.
//!
//! What is generated is the table of `common/groups.rs`: until the integer
//! group switches on `hod_interp::generate`, the programs are the
//! skeleton's own (`common/skeleton.rs`). Many programs go into one
//! module (their symbols prefixed, by the LLVM oracle's printer,
//! `hod_interp/tests/llvm_text`), one object, one C driver that defines
//! `sink` and `source` and runs each entry, one link, one process; every
//! child runs through `hod_testutil::run_with`. Each batch is compiled
//! plainly and with every stress option (`hod_cg::Stress::ALL`).
//!
//! `HOD_DIFF_SEEDS=<n>` sets the seeds of `many_programs_agree` (default
//! 1000); it prints the cost per 1000 programs.

mod common;

#[path = "../../hod_interp/tests/llvm_text/mod.rs"]
#[allow(dead_code, clippy::all, reason = "the LLVM oracle's printer, used as it is")]
mod llvm_text;

use std::fmt::Write as _;
use std::process::Command;
use std::time::{Duration, Instant};

use common::groups::{enabled, missing};
use common::linking::Toolchain;
use common::skeleton::{self, Mutation};
use common::{ll_header, parse, write_object};
use hod_a64::compile_module;
use hod_cg::{Options, Stress};
use hod_interp::Val;
use hod_interp::generate::{Generated, Options as GenOptions, generate};
use hod_ir::TargetSpec;
use hod_testutil::{Scratch, run_with};

/// The program of `seed`: the generator's when the table enables it,
/// else the skeleton's.
fn program(seed: u64, mutation: Option<Mutation>) -> Generated {
    let e = enabled();
    if e.generator && mutation.is_none() {
        let options = GenOptions { target: TargetSpec::aarch64_linux_gnu(), ..e.options };
        generate(seed, &options)
    } else {
        skeleton::generate(seed, mutation)
    }
}

/// The functions of a batch as `.ll` text for `hod_ll`: the oracle's file
/// without its host functions and its `main`, which the C driver
/// replaces.
fn functions(file: llvm_text::File) -> String {
    let text = file.finish(TargetSpec::aarch64_linux_gnu().triple());
    let prelude = text.find("@fmt_p = ").expect("the oracle's prelude");
    let source = text.find("define i64 @source()").expect("the oracle's source");
    let after = source + text[source..].find("\n}\n").expect("the end of source") + 3;
    let main = text.find("define i32 @main()").expect("the oracle's main");
    let mut out = ll_header(22);
    out.push_str("declare void @sink(i64)\ndeclare i64 @source()\n");
    out.push_str(&text[after..main]);
    debug_assert!(prelude < after);
    out
}

/// The C driver: `sink` and `source` as the oracle defines them, and a
/// `main` that prints what `llvm_text::expected` says for each program.
fn driver(batch: &[(u64, Generated)]) -> String {
    let mut c = String::from(
        "#include <stdio.h>\n#include <stdint.h>\n\
         static uint64_t calls;\n\
         void sink(uint64_t x) { printf(\"s %llu\\n\", (unsigned long long)x); }\n\
         uint64_t source(void) {\n\
           uint64_t z = ++calls * 0x9e3779b97f4a7c15ULL;\n\
           z = (z ^ (z >> 30)) * 0xbf58476d1ce4e5b9ULL;\n\
           z = (z ^ (z >> 27)) * 0x94d049bb133111ebULL;\n\
           return z ^ (z >> 31);\n\
         }\n",
    );
    for n in 0..batch.len() {
        writeln!(c, "uint64_t p{n}_entry(uint64_t, uint64_t);").unwrap();
    }
    c.push_str("int main(void) {\n");
    for (n, (label, program)) in batch.iter().enumerate() {
        let args: Vec<String> = program
            .args
            .iter()
            .map(|arg| match arg {
                Val::Int { bits: 64, value } => format!("{value}ULL"),
                other => panic!("argument {other:?}"),
            })
            .collect();
        writeln!(
            c,
            "  calls = 0; printf(\"p %llu\\n\", {label}ULL);\n  \
             printf(\"r %llu\\n\", (unsigned long long)p{n}_entry({})); fflush(stdout);",
            args.join(", ")
        )
        .unwrap();
    }
    c.push_str("  return 0;\n}\n");
    c
}

/// The lines of `output` for each program: from a `p` line to the next.
fn split(output: &str) -> Vec<String> {
    let mut programs: Vec<String> = Vec::new();
    for line in output.lines() {
        if line.starts_with("p ") || programs.is_empty() {
            programs.push(String::new());
        }
        let last = programs.last_mut().expect("just pushed");
        last.push_str(line);
        last.push('\n');
    }
    programs
}

/// Runs `clean` in the interpreter and `native`, compiled by `hod_a64`,
/// natively, plainly and stressed; returns the labels of the programs
/// whose runs disagree, with what each side printed, once per mode.
/// `None` without the tools.
fn compare(clean: &[(u64, Generated)], native: &[(u64, Generated)]) -> Option<Vec<(u64, String)>> {
    let scratch = Scratch::new("a64-diff");
    let toolchain = Toolchain::host(&scratch)?;
    let linker = toolchain.linkers.first()?;
    let expected: Vec<String> = clean
        .iter()
        .map(|(label, program)| {
            let outcome = program.run();
            assert!(outcome.result.is_ok(), "program {label}: {:?}", outcome.result);
            llvm_text::expected(*label, &outcome)
        })
        .collect();
    let mut file = llvm_text::File::new();
    for (label, program) in native {
        file.add(*label, program);
    }
    let text = functions(file);
    let (module, bodies) = parse(&text);
    let driver = toolchain.compile_c(&scratch, "driver", &driver(native), &[]);
    let mut wrong = Vec::new();
    for (mode, stress) in [("plain", Stress::default()), ("stress", Stress::ALL)] {
        let options = Options { stress, ..Options::default() };
        let emitted = compile_module(&module, &bodies, &options)
            .unwrap_or_else(|e| panic!("{mode}: {e}\n{text}"));
        let object = write_object(&scratch, &format!("{mode}.o"), &emitted);
        let program = scratch.join(mode);
        toolchain.link_ok(linker, &[], &[&driver, &object], &program);
        let ran = run_with(&mut Command::new(&program), &[], Duration::from_mins(2));
        assert!(ran.success(), "{mode}: {:?}\n{}", ran.status, ran.stderr_text());
        let got = split(&ran.stdout_text());
        assert_eq!(got.len(), expected.len(), "{mode}: {}", ran.stdout_text());
        for ((label, _), (want, got)) in clean.iter().zip(expected.iter().zip(&got)) {
            if want != got {
                wrong.push((*label, format!("{mode}: interpreter\n{want}native\n{got}")));
            }
        }
    }
    Some(wrong)
}

/// Checks `seeds`, `per_batch` programs to a batch; panics on the first
/// batch with a disagreement. Returns how many were checked.
fn check_seeds(seeds: std::ops::Range<u64>, per_batch: usize) -> usize {
    let seeds: Vec<u64> = seeds.collect();
    let mut checked = 0;
    for chunk in seeds.chunks(per_batch) {
        let batch: Vec<(u64, Generated)> =
            chunk.iter().map(|&seed| (seed, program(seed, None))).collect();
        let Some(wrong) = compare(&batch, &batch) else { return checked };
        if let Some((seed, report)) = wrong.first() {
            panic!("seed {seed}: hod_a64 and the interpreter disagree\n{report}");
        }
        checked += chunk.len();
    }
    checked
}

/// The generated programs of the enabled groups agree with the
/// interpreter; prints the missing mask.
#[test]
fn programs_agree_with_the_interpreter() {
    let e = enabled();
    eprintln!("a64 diff: missing {}", missing(&e).join(", "));
    let checked = check_seeds(0..200, 100);
    eprintln!("a64 diff: {checked} programs agree");
}

/// The harness notices a miscompilation: a wrong constant and a wrong
/// branch target, each in one program of a batch, are reported for that
/// program and no other, in both modes.
#[test]
fn a_mutated_program_is_caught() {
    let mutants = [(7, Mutation::WrongConstant), (13, Mutation::WrongTarget)];
    let clean: Vec<(u64, Generated)> =
        (0..20).map(|seed| (seed, skeleton::generate(seed, None))).collect();
    // Same seed, so the same shape and arguments.
    let native: Vec<(u64, Generated)> = (0..20)
        .map(|seed| {
            let mutation = mutants.iter().find(|(at, _)| *at == seed).map(|&(_, m)| m);
            (seed, skeleton::generate(seed, mutation))
        })
        .collect();
    let Some(wrong) = compare(&clean, &native) else { return };
    let labels: Vec<u64> = wrong.iter().map(|(label, _)| *label).collect();
    assert_eq!(labels, [7, 13, 7, 13], "{wrong:#?}");
}

/// Many seeds, and the cost per 1000 programs (interpreter, printing,
/// parsing, two compilations, two links, two runs).
#[test]
#[ignore = "a minute; run with --ignored"]
fn many_programs_agree() {
    let seeds: u64 =
        std::env::var("HOD_DIFF_SEEDS").ok().and_then(|text| text.parse().ok()).unwrap_or(1000);
    let start = Instant::now();
    let checked = check_seeds(1_000_000..1_000_000 + seeds, 500);
    let elapsed = start.elapsed();
    let per_1000 = elapsed.as_secs_f64() * 1000.0 / checked.max(1) as f64;
    eprintln!("a64 diff: {checked} programs agree in {elapsed:?}: {per_1000:.2} s per 1000");
}
