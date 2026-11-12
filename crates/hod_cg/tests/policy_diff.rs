//! `spec/codegen.md` §12: the policy against the interpreter. Generated
//! programs are compiled for the test machine and run; the result and
//! the trace of host calls must equal the interpreter's, under every
//! stress option.

mod common;

use common::harness::{Harness, MODES, option_sets};

/// Runs `seeds` seeds of every option set whose name `select` accepts
/// under every stress mode, and returns the harness for its counts.
fn batch_of(first_seed: u64, seeds: u64, select: impl Fn(&str) -> bool) -> Harness {
    let mut harness = Harness::new();
    for (index, (name, options)) in option_sets().iter().enumerate() {
        if !select(name) {
            continue;
        }
        for seed in 0..seeds {
            let seed = first_seed + index as u64 * 1_000_003 + seed;
            if let Err(what) = harness.check(seed, options, &MODES) {
                panic!("options {name}, seed {seed}: {what}");
            }
        }
    }
    harness
}

fn batch(first_seed: u64, seeds: u64) -> Harness {
    batch_of(first_seed, seeds, |_| true)
}

/// The option sets with exceptions or block addresses on.
fn unwinding(name: &str) -> bool {
    ["everything", "exceptions", "block-addresses", "only-exceptions"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

/// §12: the quick batch of the gate.
#[test]
fn generated_programs_run_as_the_interpreter_runs_them() {
    let harness = batch(1, 4);
    let tally = &harness.tally;
    // The batch compared something, and nearly everything.
    assert!(tally.compared * 10 >= tally.attempted * 9, "{}", tally.report(&harness.tm));
    assert!(tally.interp_unsupported.is_empty(), "{}", tally.report(&harness.tm));
}

/// §12 for `invoke`, `landingpad`, `resume` and `indirectbr`: the test
/// machine unwinds by the core's records alone (`exec.rs`), and every
/// program that unwinds is compiled and compared, none refused. Which pad
/// is entered, the selector and the values live into the pad are in the
/// trace: each pad sinks its number, its `phi`s and which test of the
/// selector matched (`spec/interp.md` §9.3). `HOD_CG_EH_SEEDS` seeds per
/// option set (default 12).
#[test]
fn unwinding_programs_run_as_the_interpreter_runs_them() {
    let seeds =
        std::env::var("HOD_CG_EH_SEEDS").ok().and_then(|text| text.parse().ok()).unwrap_or(12);
    let harness = batch_of(1, seeds, unwinding);
    let tally = &harness.tally;
    let report = tally.report(&harness.tm);
    println!("{report}");
    assert!(tally.rejected.is_empty(), "{report}");
    assert_eq!(tally.compared, tally.attempted, "{report}");
    assert!(tally.throws > 0 && tally.landings > tally.throws, "{report}");
    assert!(tally.lsdas > 0 && tally.fdes > tally.lsdas, "the unwinder read no tables: {report}");
    for construct in [
        "eh:invoke-throw",
        "eh:invoke-call",
        "eh:filter",
        "eh:cleanup",
        "eh:pad-phi",
        "cf:indirectbr",
    ] {
        assert!(tally.coverage.get(construct) > 0, "{construct} was not generated: {report}");
    }
}

/// §12: the long batch. `HOD_CG_SEEDS` is the number of seeds per option
/// set (default 250); every one runs under all five stress modes.
#[test]
#[ignore = "the long differential batch: minutes"]
fn long_batch() {
    let seeds =
        std::env::var("HOD_CG_SEEDS").ok().and_then(|text| text.parse().ok()).unwrap_or(250);
    let harness = batch(1_000, seeds);
    let tally = &harness.tally;
    println!("{}", tally.report(&harness.tm));
    println!("coverage:");
    for (name, count) in tally.coverage.iter() {
        println!("  {name:24} {count}");
    }
    assert!(tally.compared * 10 >= tally.attempted * 9);
}
