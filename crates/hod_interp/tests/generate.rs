//! The program generator (`spec/interp.md` §9): what every generated
//! program is, and that each option switches its constructs.

use std::time::Instant;

use hod_interp::generate::{Coverage, Generated, Options, generate};
use hod_interp::{Config, Val};

/// Seeds the quick tests run: about a second in a debug build.
const SEEDS: u64 = 300;

/// Checks what §9.1 and §9.2 promise of one program and returns its run.
#[track_caller]
fn check(seed: u64, generated: &Generated) -> hod_interp::Outcome {
    let program = generated.program();
    if let Err(error) = program.verify() {
        panic!("seed {seed}: a body does not verify: {error}");
    }
    let outcome = generated.run();
    match &outcome.result {
        Ok(Some(Val::Int { bits: 64, .. })) => {}
        other => panic!(
            "seed {seed}: expected an i64, got {other:?} at {:?} after {} steps\n{}",
            outcome.stopped_at,
            outcome.steps,
            outcome.trace_text()
        ),
    }
    assert!(
        outcome.steps <= generated.fuel_bound,
        "seed {seed}: {} steps, bound {}",
        outcome.steps,
        generated.fuel_bound
    );
    outcome
}

/// The coverage of `seeds` programs under `options`, each checked.
#[track_caller]
fn coverage(options: &Options, seeds: u64) -> Coverage {
    let mut all = Coverage::default();
    for seed in 0..seeds {
        let generated = generate(seed, options);
        check(seed, &generated);
        all.merge(&generated.coverage);
    }
    all
}

/// §9.1, §9.2: every program verifies and runs to an `i64` without a
/// trap, in no more steps than its fuel bound.
#[test]
fn programs_verify_and_run_within_their_bound() {
    let options = Options::default();
    for seed in 0..SEEDS {
        check(seed, &generate(seed, &options));
    }
}

/// §9.1: the same seed and options give the same program and the same
/// run; another seed gives another program.
#[test]
fn generation_is_deterministic() {
    let options = Options::default();
    let mut distinct = std::collections::BTreeSet::new();
    for seed in 0..40 {
        let (a, b) = (generate(seed, &options), generate(seed, &options));
        assert_eq!(format!("{:?}", a.bodies), format!("{:?}", b.bodies), "seed {seed}");
        assert_eq!(a.args, b.args);
        assert_eq!(a.fuel_bound, b.fuel_bound);
        assert_eq!(a.coverage, b.coverage);
        let (ra, rb) = (a.run(), b.run());
        assert_eq!(ra.result, rb.result, "seed {seed}");
        assert_eq!(ra.trace, rb.trace, "seed {seed}");
        assert_eq!(ra.steps, rb.steps, "seed {seed}");
        distinct.insert(format!("{:?} {:?}", ra.result, ra.trace_text()));
    }
    assert!(distinct.len() > 30, "only {} distinct runs of 40", distinct.len());
}

/// §9.2: no result and no argument of a host call depends on where the
/// interpreter puts memory.
#[test]
fn results_do_not_depend_on_addresses() {
    let options = Options::default();
    for seed in 0..100 {
        let generated = generate(seed, &options);
        let here = generated.run();
        let fuel = generated.fuel_bound;
        let there = generated.run_with(Config {
            fuel,
            addr_base: 0x7f00_0000_1000,
            addr_gap: 4096,
            ..Config::default()
        });
        assert_eq!(here.result, there.result, "seed {seed}");
        assert_eq!(here.trace, there.trace, "seed {seed}");
        assert_eq!(here.steps, there.steps, "seed {seed}");
    }
}

/// §9.4: with everything on, every construct occurs.
#[test]
fn default_options_cover_every_construct() {
    let all = coverage(&Options::default(), SEEDS);
    let missing: Vec<_> = Coverage::NAMES.iter().filter(|name| all.get(name) == 0).collect();
    assert!(missing.is_empty(), "never generated: {missing:?}");
}

/// §9.3: with everything off there is straight-line `i64` arithmetic and
/// nothing else, and the bound is the exact number of steps.
#[test]
fn minimal_options_leave_only_integer_arithmetic() {
    let options = Options::minimal();
    let mut all = Coverage::default();
    for seed in 0..100 {
        let generated = generate(seed, &options);
        let outcome = check(seed, &generated);
        assert_eq!(generated.bodies.len(), 1);
        assert_eq!(generated.bodies[0].block_count(), 1);
        assert_eq!(outcome.steps, generated.fuel_bound, "seed {seed}");
        assert!(outcome.trace.is_empty());
        all.merge(&generated.coverage);
    }
    let allowed = ["int:i64", "freeze", "use:dead", "use:next", "use:once-later", "use:many"];
    for (name, count) in all.iter() {
        assert!(allowed.contains(&name), "{name} occurs {count} times");
    }
    for name in allowed {
        assert!(all.get(name) > 0, "{name} never occurs");
    }
}

/// An option's name, how to switch it off, and the counters that must
/// then be zero.
type Switch = (&'static str, fn(&mut Options), &'static [&'static str]);

/// Every switch of §9.3.
const SWITCHES: &[Switch] = &[
    ("narrow_ints", |o| o.narrow_ints = false, &["int:i8", "int:i16", "int:i32"]),
    ("odd_ints", |o| o.odd_ints = false, &["int:i7", "int:i33", "mem:odd-width"]),
    ("i128", |o| o.i128 = false, &["int:i128"]),
    ("floats", |o| o.floats = false, &["float:op", "float:fcmp", "float:convert"]),
    (
        "compares",
        |o| o.compares = false,
        &[
            "icmp:value",
            "icmp:branch",
            "select",
            "cf:diamond",
            "cf:critical-edge",
            "cf:condbr-twice",
            "cf:dead-trap",
            "cf:loop",
            "cf:self-loop",
            "cf:swap",
            "cf:switch-dense",
            "cf:switch-sparse",
            "cf:switch-shared",
        ],
    ),
    ("selects", |o| o.selects = false, &["select"]),
    (
        "casts",
        |o| o.casts = false,
        &[
            "cast:trunc",
            "cast:zext",
            "cast:sext",
            "int:i8",
            "int:i16",
            "int:i32",
            "int:i7",
            "int:i33",
            "int:i128",
        ],
    ),
    (
        "memory",
        |o| o.memory = false,
        &[
            "mem:slot",
            "mem:load",
            "mem:store",
            "mem:odd-width",
            "ptradd:const",
            "ptradd:scaled",
            "global:load",
            "global:store",
            "intrinsic:mem",
            "cast:ptrtoint",
            "cast:inttoptr",
        ],
    ),
    ("globals", |o| o.globals = false, &["global:load", "global:store"]),
    ("ptr_int", |o| o.ptr_int = false, &["cast:ptrtoint", "cast:inttoptr"]),
    (
        "intrinsics",
        |o| o.intrinsics = false,
        &["intrinsic:int", "intrinsic:checked", "intrinsic:mem"],
    ),
    (
        "branches",
        |o| o.branches = false,
        &["cf:diamond", "cf:critical-edge", "cf:condbr-twice", "cf:dead-trap"],
    ),
    ("loops", |o| o.loops = false, &["cf:loop", "cf:self-loop", "cf:swap"]),
    (
        "switches",
        |o| o.switches = false,
        &["cf:switch-dense", "cf:switch-sparse", "cf:switch-shared"],
    ),
    ("unreachable_blocks", |o| o.unreachable_blocks = false, &["cf:unreachable-block"]),
    (
        "calls",
        |o| o.calls = false,
        &["call:direct", "call:indirect", "call:many-args", "call:unused-result"],
    ),
    ("indirect_calls", |o| o.indirect_calls = false, &["call:indirect"]),
    ("many_args", |o| o.many_args = false, &["call:many-args"]),
    ("hosts", |o| o.hosts = false, &["call:sink", "call:source"]),
];

/// §9.3: each option that is off takes its constructs out, and nothing
/// else needs them.
#[test]
fn each_option_switches_its_constructs() {
    for (name, off, gone) in SWITCHES {
        let mut options = Options::default();
        off(&mut options);
        let all = coverage(&options, 60);
        for counter in *gone {
            assert_eq!(all.get(counter), 0, "{counter} occurs with {name} off");
        }
        // Everything else still occurs, except what needs the option.
        let effective = options.effective();
        let needs_it: &[&str] = match *name {
            "compares" | "memory" | "casts" | "calls" => &[],
            _ => gone,
        };
        if !needs_it.is_empty() {
            assert_eq!(effective, options, "{name} switches nothing else");
            for counter in Coverage::NAMES {
                if !gone.contains(counter) {
                    assert!(all.get(counter) > 0, "{counter} never occurs with {name} off");
                }
            }
        }
    }
    // Without any control flow there is one block and no `phi`.
    let options = Options {
        branches: false,
        loops: false,
        switches: false,
        unreachable_blocks: false,
        ..Options::default()
    };
    let all = coverage(&options, 60);
    assert_eq!(all.get("phi"), 0);
    assert_eq!(all.get("use:other-block"), 0);
}

/// §9.3: the size knobs go from one instruction to thousands, and from
/// one function to dozens.
#[test]
fn size_knobs() {
    for seed in 0..20 {
        let tiny = generate(seed, &Options::default().sized(1, 1));
        check(seed, &tiny);
        assert_eq!(tiny.bodies.len(), 1);
    }
    for seed in 0..3 {
        let options = Options::default().sized(3, 3000);
        let large = generate(seed, &options);
        check(seed, &large);
        for body in &large.bodies {
            assert!(body.inst_count() >= 3000, "{} instructions", body.inst_count());
            assert!(body.inst_count() < 6000, "{} instructions", body.inst_count());
        }
        let many = generate(seed, &Options::default().sized(40, 20));
        check(seed, &many);
        assert_eq!(many.bodies.len(), 40);
    }
    // No function is far from the size asked for.
    for seed in 0..50 {
        let generated = generate(seed, &Options::default().sized(4, 100));
        for body in &generated.bodies {
            assert!((100..400).contains(&body.inst_count()), "{}", body.inst_count());
        }
    }
}

/// Seconds of `time` for each of `count`.
fn per(time: std::time::Duration, count: u64) -> f64 {
    time.as_secs_f64() / f64::from(u32::try_from(count.max(1)).unwrap_or(u32::MAX))
}

/// §9.5: how fast programs are generated and run. Prints the figures the
/// spec quotes; fails only if something is slower by orders of magnitude.
#[test]
fn throughput() {
    for (label, options, seeds) in [
        ("default (4 x 60)", Options::default(), 200u64),
        ("large (4 x 2000)", Options::default().sized(4, 2000), 10),
    ] {
        let start = Instant::now();
        let programs: Vec<Generated> = (0..seeds).map(|seed| generate(seed, &options)).collect();
        let generating = start.elapsed();
        let insts: usize =
            programs.iter().flat_map(|g| &g.bodies).map(hod_ir::Body::inst_count).sum();
        let start = Instant::now();
        let steps: u64 = programs.iter().map(|g| g.run().steps).sum();
        let running = start.elapsed();
        println!(
            "{label}: {seeds} programs, {insts} instructions generated in {generating:?} \
             ({:.0} ns/instruction); {steps} steps run in {running:?} ({:.0} ns/step, \
             {:.0} us/program)",
            per(generating, insts as u64) * 1e9,
            per(running, steps) * 1e9,
            per(running, seeds) * 1e6,
        );
        assert!(generating.as_secs() < 30 && running.as_secs() < 30);
    }
}
