//! `spec/codegen.md` §12: the policy against the interpreter. Generated
//! programs are compiled for the test machine and run; the result and
//! the trace of host calls must equal the interpreter's, under every
//! stress option.

mod common;

use common::harness::{Harness, MODES, option_sets};

/// Runs `seeds` seeds of every option set under every stress mode, and
/// returns the harness for its counts.
fn batch(first_seed: u64, seeds: u64) -> Harness {
    let mut harness = Harness::new();
    for (index, (name, options)) in option_sets().iter().enumerate() {
        for seed in 0..seeds {
            let seed = first_seed + index as u64 * 1_000_003 + seed;
            if let Err(what) = harness.check(seed, options, &MODES) {
                panic!("options {name}, seed {seed}: {what}");
            }
        }
    }
    harness
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
