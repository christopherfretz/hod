//! The differential harness on x86-64 (`spec/codegen.md` §12, WP-130):
//! the random whole programs of `hod_interp::generate` run by the
//! interpreter and, compiled by `hod_x64` (plainly and with every stress
//! option) and by `llc -O0 -fast-isel=0 -relocation-model=pic` of LLVM 22
//! and 19, under the emulator; the result and the trace of `sink` calls
//! must be equal on every leg.
//!
//! `hod_a64`'s `tests/diff.rs` (WP-033, WP-066) is the model: many
//! programs go into one module (their symbols prefixed by the LLVM
//! oracle's printer, `hod_interp/tests/llvm_text`), one object per leg,
//! one C driver that defines `sink` and `source` and runs each entry, one
//! link and ONE emulated process per leg and batch; every child runs
//! through `hod_testutil::run_with`.
//!
//! What is generated is [`enabled`]: every switch of the generator whose
//! constructs the landed x86-64 groups lower. `vectors` is off until the
//! vector group (WP-132) lands; `exceptions` is off as on AArch64 (its
//! programs throw through a host `hod_throw` this driver does not have).
//! A program hod refuses is **skipped** by the name it is refused by,
//! never counted as agreeing: the harness prints how many were skipped,
//! by name. A refusal by the integer group's names fails the harness.
//!
//! `HOD_DIFF_SEEDS=<n>` sets the seeds of `many_programs_agree` (default
//! 1000); it prints the cost per 1000 programs.

#[path = "../../hod_obj/tests/common/linking.rs"]
#[allow(dead_code, reason = "a shared helper: not every function is used here")]
mod linking;

#[path = "common/int_gen.rs"]
mod int_gen;

#[path = "../../hod_interp/tests/llvm_text/mod.rs"]
#[allow(dead_code, clippy::all, reason = "the LLVM oracle's printer, used as it is")]
mod llvm_text;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::Instant;

use hod_cg::{CgError, Stress};
use hod_interp::Val;
use hod_interp::generate::{Generated, Options as GenOptions, generate};
use hod_ir::TargetSpec;
use hod_testutil::Scratch;
use hod_x64::{ModuleError, compile_module, compile_to_object, int};
use int_gen::{ll_header, llc, parse, pic};
use linking::Toolchain;

/// What the landed x86-64 groups lower: the generator's defaults (every
/// scalar construct) with computed gotos; no vectors (WP-132), no
/// exceptions (no host `hod_throw`).
fn enabled() -> GenOptions {
    GenOptions {
        target: TargetSpec::x86_64_linux_gnu(),
        block_addresses: true,
        exceptions: false,
        vectors: false,
        ..GenOptions::default()
    }
}

/// The functions of a batch as `.ll` text: the oracle's file without its
/// host functions and its `main`, which the C driver replaces.
fn functions(file: llvm_text::File) -> String {
    let text = file.finish(TargetSpec::x86_64_linux_gnu().triple());
    let source = text.find("define i64 @source()").expect("the oracle's source");
    let after = source + text[source..].find("\n}\n").expect("the end of source") + 3;
    let main = text.find("define i32 @main()").expect("the oracle's main");
    let mut out = ll_header();
    out.push_str("declare void @sink(i64)\ndeclare i64 @source()\n");
    // After `source`: the programs' globals, declarations and functions.
    out.push_str(&text[after..main]);
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

/// A deliberate miscompilation of one program of a batch, made in the
/// text hod compiles, so that the harness can be shown to notice.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mutation {
    /// Every `ret` of the entry returns its value with bit 0 flipped.
    WrongResult,
    /// The entry calls `sink` once more before it returns.
    ExtraCall,
}

/// `text` with program `n`'s entry mutated.
fn mutate(text: &str, n: usize, mutation: Mutation) -> String {
    let start = text.find(&format!("define i64 @p{n}_entry(")).expect("the entry");
    let end = start + text[start..].find("\n}\n").expect("the end of the entry");
    let mut body = String::new();
    for (k, line) in text[start..end].lines().enumerate() {
        match (line.trim_start().strip_prefix("ret i64 "), mutation) {
            (Some(value), Mutation::WrongResult) => {
                writeln!(body, "  %mut{k} = xor i64 {value}, 1\n  ret i64 %mut{k}").unwrap();
            }
            (Some(_), Mutation::ExtraCall) => {
                writeln!(body, "  call void @sink(i64 7)\n{line}").unwrap();
            }
            (None, _) => writeln!(body, "{line}").unwrap(),
        }
    }
    body.pop();
    format!("{}{body}{}", &text[..start], &text[end..])
}

/// What one batch's legs printed against the interpreter.
struct Compared {
    /// `(label, report)` of each program whose run disagrees, per leg.
    wrong: Vec<(u64, String)>,
    /// The legs that ran.
    legs: Vec<String>,
}

/// Runs `batch` in the interpreter and, compiled by hod (plainly and
/// stressed, from the text with `mutations` applied) and by `llc` 22 and
/// 19 (from the text as it is), under the emulator. `None` without the
/// cross toolchain.
fn compare(batch: &[(u64, Generated)], mutations: &[(usize, Mutation)]) -> Option<Compared> {
    let scratch = Scratch::new("x64-diff");
    let toolchain = Toolchain::x86_64(&scratch)?;
    let linker = toolchain.linkers.first()?;
    let expected: Vec<String> = batch
        .iter()
        .map(|(label, program)| {
            let outcome = program.run();
            assert!(outcome.result.is_ok(), "program {label}: {:?}", outcome.result);
            llvm_text::expected(*label, &outcome)
        })
        .collect();
    let mut file = llvm_text::File::new();
    for (label, program) in batch {
        file.add(*label, program);
    }
    let text = functions(file);
    let mut mutated = text.clone();
    for &(n, mutation) in mutations {
        mutated = mutate(&mutated, n, mutation);
    }
    let (module, bodies) = parse(&mutated);
    let mut legs: Vec<(String, PathBuf)> = Vec::new();
    for (mode, stress) in [("plain", Stress::default()), ("stress", Stress::ALL)] {
        let object = compile_to_object(&module, &bodies, &pic(stress))
            .unwrap_or_else(|e| panic!("{mode}: {e}\n{mutated}"));
        legs.push((format!("hod {mode}"), scratch.write(&format!("hod-{mode}.o"), object)));
    }
    for major in hod_testutil::LLVM_MAJORS {
        if let Some(object) = llc(&scratch, major, &format!("llc{major}"), &text) {
            legs.push((format!("llc {major}"), object));
        }
    }
    let driver = toolchain.compile_c(&scratch, "driver", &driver(batch), &[]);
    let mut compared = Compared { wrong: Vec::new(), legs: Vec::new() };
    for (leg, object) in &legs {
        let program = scratch.join(leg.replace(' ', "-"));
        // `frem` calls `fmod`/`fmodf`; the flags precede the inputs.
        let flags = ["-Wl,--no-as-needed", "-lm"];
        toolchain.link_ok(linker, &flags, &[&driver, object], &program);
        let ran = toolchain.run(linker, &program);
        assert!(ran.success(), "{leg}: {:?}\n{}", ran.status, ran.stderr_text());
        let got = split(&ran.stdout_text());
        assert_eq!(got.len(), expected.len(), "{leg}: {}", ran.stdout_text());
        for ((label, _), (want, got)) in batch.iter().zip(expected.iter().zip(&got)) {
            if want != got {
                compared.wrong.push((*label, format!("{leg}: interpreter\n{want}x86-64\n{got}")));
            }
        }
        compared.legs.push(leg.clone());
    }
    Some(compared)
}

/// The stable names of the integer group (WP-122): a generated program
/// refused by one of them is a failure, not a skip.
const INT_NAMES: [&str; 3] = [int::BINARY, int::CAST, int::ADDRSPACECAST];

/// The name hod refuses `program` by, if it does.
///
/// # Panics
/// For a refusal by the integer group or any other failure to compile.
fn refusal(seed: u64, program: &Generated) -> Option<&'static str> {
    match compile_module(&program.module, &program.bodies, &pic(Stress::default())) {
        Ok(_) => None,
        Err(ModuleError::Function { error: CgError::Unsupported { construct }, .. }) => {
            assert!(!INT_NAMES.contains(&construct), "seed {seed}: refused by {construct}");
            Some(construct)
        }
        Err(error) => panic!("seed {seed}: {error}"),
    }
}

/// What [`check_programs`] did.
#[derive(Debug, Default)]
struct Checked {
    /// Programs that ran end to end and agree on every leg.
    agree: usize,
    /// Programs skipped, by the name hod refused them by.
    skipped: BTreeMap<&'static str, usize>,
    /// The legs of the last batch.
    legs: Vec<String>,
}

impl Checked {
    fn skipped(&self) -> usize {
        self.skipped.values().sum()
    }

    /// `"<agree> programs agree (<legs>), <n> skipped (<p>% ran end to end): <name> <count>, ..."`.
    #[allow(clippy::cast_precision_loss, reason = "a proportion to print")]
    fn summary(&self) -> String {
        let total = self.agree + self.skipped();
        let percent = 100.0 * self.agree as f64 / total.max(1) as f64;
        let reasons: Vec<String> =
            self.skipped.iter().map(|(name, count)| format!("{name} {count}")).collect();
        format!(
            "{} programs agree ({}), {} skipped ({percent:.1}% ran end to end){}{}",
            self.agree,
            self.legs.join(", "),
            self.skipped(),
            if reasons.is_empty() { "" } else { ": " },
            reasons.join(", ")
        )
    }
}

/// Checks the programs `make` gives for `seeds`, `per_batch` to a batch;
/// panics on the first batch with a disagreement.
fn check_programs(
    seeds: std::ops::Range<u64>,
    per_batch: usize,
    make: impl Fn(u64) -> Generated,
) -> Checked {
    let mut checked = Checked::default();
    let mut batch: Vec<(u64, Generated)> = Vec::new();
    let flush = |batch: &mut Vec<(u64, Generated)>, checked: &mut Checked| -> bool {
        if batch.is_empty() {
            return true;
        }
        let Some(compared) = compare(batch, &[]) else { return false };
        if let Some((seed, report)) = compared.wrong.first() {
            panic!("seed {seed}: x86-64 and the interpreter disagree\n{report}");
        }
        checked.agree += batch.len();
        checked.legs = compared.legs;
        batch.clear();
        true
    };
    for seed in seeds {
        let generated = make(seed);
        if let Some(name) = refusal(seed, &generated) {
            *checked.skipped.entry(name).or_default() += 1;
            continue;
        }
        batch.push((seed, generated));
        if batch.len() == per_batch && !flush(&mut batch, &mut checked) {
            return checked;
        }
    }
    flush(&mut batch, &mut checked);
    checked
}

/// The generated programs of every landed group agree with the
/// interpreter on every leg; prints what was skipped.
#[test]
fn programs_agree_with_the_interpreter() {
    let options = enabled();
    let checked = check_programs(0..600, 300, |seed| generate(seed, &options));
    eprintln!("x64 diff: {}", checked.summary());
}

/// Smaller pieces of the generator, so that one refusal does not hide a
/// whole program's other constructs: without memory, without calls,
/// without floats.
#[test]
fn programs_without_one_group_agree() {
    let all = enabled();
    for (name, options) in [
        ("no memory", GenOptions { memory: false, globals: false, ptr_int: false, ..all.clone() }),
        ("no calls", GenOptions { calls: false, indirect_calls: false, ..all.clone() }),
        ("no floats", GenOptions { floats: false, ..all.clone() }),
    ] {
        let checked = check_programs(0..150, 150, |seed| generate(seed, &options));
        eprintln!("x64 diff, {name}: {}", checked.summary());
    }
}

/// The harness notices a miscompilation: a wrong result and an extra
/// call, each in one program of a batch, are reported for that program
/// and no other, for each hod leg and for no `llc` leg.
#[test]
fn a_mutated_program_is_caught() {
    let options = enabled();
    let batch: Vec<(u64, Generated)> = (0..20)
        .map(|seed| (seed, generate(seed, &options)))
        .filter(|(seed, program)| refusal(*seed, program).is_none())
        .collect();
    assert!(batch.len() >= 10, "most programs compile");
    let mutations = [(3, Mutation::WrongResult), (7, Mutation::ExtraCall)];
    let Some(compared) = compare(&batch, &mutations) else { return };
    let (a, b) = (batch[3].0, batch[7].0);
    let labels: Vec<u64> = compared.wrong.iter().map(|(label, _)| *label).collect();
    assert_eq!(labels, [a, b, a, b], "{:#?}", compared.wrong);
    assert!(compared.wrong.iter().all(|(_, report)| report.starts_with("hod ")));
}

/// `drift_call_widths.ll` (the width invariant at calls), which `irdiff`
/// builds only plainly: compiled by `hod_x64` plainly and with every
/// stress option, linked and run under the emulator, it must print its
/// `stdout` directives. Written while the classifier still refused
/// `zeroext`/`signext` on x86-64, to show the code generator lowered them.
#[test]
fn call_widths_seed_stressed() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/ir/");
    for seed in ["drift_call_widths.ll"] {
        let text = std::fs::read_to_string(format!("{root}{seed}")).expect("the seed");
        let mut want = String::new();
        for line in text.lines().filter_map(|line| line.strip_prefix(";@ stdout: ")) {
            want.push_str(line);
            want.push('\n');
        }
        let (module, bodies) = parse(&format!("{}{text}", ll_header()));
        let scratch = Scratch::new("x64-diff-seed");
        let Some(toolchain) = Toolchain::x86_64(&scratch) else { return };
        let Some(linker) = toolchain.linkers.first() else { return };
        for (mode, stress) in [("plain", Stress::default()), ("stress", Stress::ALL)] {
            let object = compile_to_object(&module, &bodies, &pic(stress))
                .unwrap_or_else(|e| panic!("{seed} {mode}: {e}"));
            let object = scratch.write(&format!("{mode}.o"), object);
            let program = scratch.join(mode);
            let ran = toolchain.link_and_run(linker, &[], &[&object], &program);
            assert!(ran.success(), "{seed} {mode}: {:?}", ran.status);
            assert_eq!(ran.stdout_text(), want, "{seed} {mode}");
        }
    }
}

/// Many seeds, and the cost per 1000 programs (interpreter, printing,
/// parsing, two hod compilations, two `llc`s, four links, four emulated
/// runs).
#[test]
#[ignore = "minutes; run with --ignored"]
fn many_programs_agree() {
    let seeds: u64 =
        std::env::var("HOD_DIFF_SEEDS").ok().and_then(|text| text.parse().ok()).unwrap_or(1000);
    let options = enabled();
    let start = Instant::now();
    let checked =
        check_programs(1_000_000..1_000_000 + seeds, 500, |seed| generate(seed, &options));
    let elapsed = start.elapsed();
    let per_1000 =
        elapsed.as_secs_f64() * 1000.0 / f64::from(u32::try_from(checked.agree.max(1)).unwrap());
    eprintln!("x64 diff: {} in {elapsed:?}: {per_1000:.2} s per 1000 run", checked.summary());
}
