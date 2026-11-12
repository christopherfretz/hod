//! `spec/ll.md` §11: generated programs printed as LLVM text and read
//! back by `hod_ll` do what the originals do in the reference
//! interpreter (`spec/interp.md`).
//!
//! The generator builds a module and its bodies in `hod_ir`; the
//! printer of `hod_interp`'s LLVM oracle writes them as text; `hod_ll`
//! parses the text, module and bodies; both are run in the interpreter
//! and must make the same host calls with the same arguments, in the
//! same order, and return the same value. The interpreter's `sink` and
//! `source` stand for the functions of the same names the printed file
//! defines, so their bodies are left out.

#[path = "../../hod_interp/tests/llvm_text/mod.rs"]
#[allow(dead_code)]
mod llvm_text;

use std::fmt::Write as _;

use hod_interp::generate::{Options, generate};
use hod_interp::{Config, Interp, Outcome, Program, host};
use hod_ll::{BodyParser, ParseOptions, parse_module};

/// What a run did, as text to compare.
fn shown(outcome: &Outcome) -> String {
    let mut text = String::new();
    for event in &outcome.trace {
        let _ = writeln!(text, "{} {:?} -> {:?}", event.name, event.args, event.result);
    }
    let _ = writeln!(text, "= {:?}", outcome.result);
    text
}

/// Checks the programs of `seeds` under `options`; returns how many.
fn check(options: &Options, seeds: std::ops::Range<u64>) -> usize {
    let mut checked = 0;
    for seed in seeds {
        let program = generate(seed, options);
        let want = program.run();
        assert!(want.result.is_ok(), "seed {seed}: {:?}", want.result);
        let mut file = llvm_text::File::new();
        file.add(seed, &program);
        let text = file.finish(options.target.triple());

        let mut parsed = parse_module(text.as_bytes(), &ParseOptions::default())
            .unwrap_or_else(|error| panic!("seed {seed}: {error}\n{text}"));
        let mut parser = BodyParser::new(&parsed.text);
        let mut bodies = Vec::new();
        for function in parsed.text.functions() {
            let name = parsed.module.symbol(function.sym).name();
            if !name.starts_with(b"p0_") {
                continue;
            }
            let body = parser
                .parse(&mut parsed.module, function)
                .unwrap_or_else(|error| panic!("seed {seed}: {error}\n{text}"))
                .unwrap_or_else(|| {
                    let reasons: Vec<&str> = parser.unsupported().collect();
                    panic!("seed {seed}: unsupported {reasons:?}\n{text}")
                });
            body.verify(&parsed.module)
                .unwrap_or_else(|error| panic!("seed {seed}: {error}\n{text}"));
            bodies.push(body);
        }
        let module = &parsed.module;
        let entry_name =
            format!("p0_{}", String::from_utf8_lossy(program.module.symbol(program.entry).name()));
        let entry = module.find_symbol(entry_name.as_bytes()).expect("the entry function");
        let read = Program::with_bodies(module, &bodies).expect("bodies of the module");
        let default = Config::default();
        let functions = u32::try_from(bodies.len()).unwrap_or(u32::MAX);
        let config = Config {
            fuel: program.fuel_bound * 2 + 1000,
            max_call_depth: default.max_call_depth.max(functions),
            ..default
        };
        let mut interp = Interp::new(&read).with_config(config);
        host::register(&mut interp);
        let got = interp.run(entry, &program.args);
        assert_eq!(shown(&got), shown(&want), "seed {seed}\n{text}");
        checked += 1;
    }
    checked
}

/// The gate's batch: every switch of the generator on, small and
/// mid-sized programs.
#[test]
fn printed_programs_read_back_run_the_same() {
    let mut checked = check(&Options::default(), 0..150);
    checked += check(&Options::default().sized(2, 12), 1000..1100);
    checked += check(&Options::default().sized(6, 300), 2000..2020);
    checked += check(&Options::minimal(), 3000..3020);
    assert!(checked >= 290);
}

/// A larger batch, for a release candidate:
/// `cargo test --release -p hod_ll --test roundtrip -- --ignored`.
#[test]
#[ignore = "a long batch; run on demand"]
fn many_printed_programs_read_back_run_the_same() {
    let mut checked = check(&Options::default(), 10_000..14_000);
    checked += check(&Options::default().sized(6, 400), 20_000..20_400);
    checked += check(&Options::default().sized(3, 3000), 30_000..30_040);
    assert!(checked > 4000);
}
