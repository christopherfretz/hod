//! `cargo xtask fuzz --budget`: the plan, the resumable run and the table
//! (`spec/harness.md` "Random programs", "Budget"), with a stand-in for
//! the run of a slice: these tests run no generator and no compiler.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use hod_testutil::Scratch;
use xtask::fuzz::budget::{self, BUDGET, CHUNK, LLVM19_SHARE, Row, Slice};
use xtask::fuzz::case::{Kind, Mode};
use xtask::fuzz::{Options, Saved, Summary, parse_args};
use xtask::irdiff::Arch;

const BASE: u64 = 100_001;

// "Budget": every slice's seeds are fresh, disjoint from every other
// slice's, at most a chunk, and the counts are the budget's.
#[test]
fn the_plan_is_the_budget_in_disjoint_ranges() {
    let slices = budget::plan(BASE, &Arch::ALL);
    let mut next = BASE;
    let mut per: BTreeMap<(Arch, u32, &str, &str), u64> = BTreeMap::new();
    for slice in &slices {
        assert_eq!(slice.first, next, "{}: ranges follow each other", slice.key());
        assert!(slice.count > 0 && slice.count <= CHUNK, "{}", slice.key());
        next = slice.last() + 1;
        *per.entry((slice.arch, slice.llvm, slice.profile.name, slice.mode.name())).or_default() +=
            slice.count;
    }
    for arch in Arch::ALL {
        for line in BUDGET {
            let (profile, mode) = (line.profile, line.mode.name());
            assert_eq!(per[&(arch, 22, profile, mode)], line.seeds);
            assert_eq!(per[&(arch, 19, profile, mode)], line.seeds.div_ceil(LLVM19_SHARE));
        }
    }
    assert_eq!(per.len(), 2 * 2 * BUDGET.len());
    // Every profile of the harness and every mode but the census is in it.
    for profile in ["default", "aggregates", "float", "wide", "volatile", "narrow", "abi"] {
        assert!(BUDGET.iter().any(|l| l.profile == profile), "{profile}");
    }
    for mode in [Mode::Whole, Mode::Mix, Mode::Debug] {
        assert!(BUDGET.iter().any(|l| l.mode == mode), "{mode:?}");
    }
    // One target keeps the seeds it has in the plan of both.
    let x86: Vec<Slice> = slices.iter().filter(|s| s.arch == Arch::X86_64).copied().collect();
    assert_eq!(budget::plan(BASE, &[Arch::X86_64]), x86);
}

fn options(scratch: &Scratch) -> Options {
    let mut options = Options::new(scratch.join("build"));
    options.out = scratch.join("fuzz");
    options
}

/// A stand-in for a slice's run: seed `first + 1` is refused, `first + 2`
/// skipped, the rest agree; with `finding` the slice's first seed is a
/// finding.
fn summary_of(options: &Options, finding: bool) -> Summary {
    let mut summary =
        Summary { seeds: usize::try_from(options.count).unwrap(), ..Summary::default() };
    summary.refused.insert("x64:some-construct".to_owned(), 1);
    summary.skipped.insert("both-timed-out".to_owned(), 1);
    summary.agree = summary.seeds - 2;
    if finding {
        summary.agree -= 1;
        summary.findings.push(Saved {
            seed: options.seed,
            kind: Kind::Disagree,
            detail: "stdout differs".to_owned(),
            dir: PathBuf::from("/f/x86_64-llvm22-seed1"),
            witness: String::new(),
            reduced: None,
        });
    }
    summary.elapsed = Duration::from_secs(90);
    let results = options.out.join(format!("results-{}.tsv", options.seed));
    fs::write(&results, "# seed\n").unwrap();
    summary.results_file = Some(results);
    summary
}

// "Budget": a slice in the file is not run again; an interrupted run
// keeps what it finished; a line cut short is run again.
#[test]
fn the_run_resumes() {
    let scratch = Scratch::new("fuzz-budget-resume");
    let options = options(&scratch);
    let slices: Vec<Slice> = budget::plan(BASE, &[Arch::Aarch64]).into_iter().take(5).collect();
    let mut calls = Vec::new();
    let first = budget::run(&options, &slices, |o| {
        calls.push(o.seed);
        if calls.len() == 3 { Err("stopped".to_owned()) } else { Ok(summary_of(o, false)) }
    });
    assert_eq!(first.unwrap_err(), "stopped");
    assert_eq!(budget::read_rows(&options.out.join("budget.tsv")).len(), 2);
    // The machine stopped in the middle of a write.
    let file = options.out.join("budget.tsv");
    let mut text = fs::read_to_string(&file).unwrap();
    text.push_str("aarch64\t22\tdefault\twhole\t");
    fs::write(&file, text).unwrap();

    // ... and left its work directory behind.
    let stale = options.out.join("run-12-34").join("seed-5");
    fs::create_dir_all(&stale).unwrap();

    let mut seen = Vec::new();
    let outcome = budget::run(&options, &slices, |o| {
        seen.push((o.seed, o.count, o.profile.name, o.mode, o.target, o.llvm));
        Ok(summary_of(o, o.seed == slices[4].first))
    })
    .unwrap();
    assert_eq!((outcome.ran, outcome.kept), (3, 2));
    assert!(!options.out.join("run-12-34").exists());
    let expected: Vec<_> = slices[2..]
        .iter()
        .map(|s| (s.first, s.count, s.profile.name, s.mode, Some(s.arch), s.llvm))
        .collect();
    assert_eq!(seen, expected);
    assert_eq!(outcome.rows.len(), 5);
    assert_eq!(outcome.exit_status(), 1);
    assert!(options.out.join("results").read_dir().unwrap().count() == 5);

    let again = budget::run(&options, &slices, |_| panic!("nothing is left to run")).unwrap();
    assert_eq!((again.ran, again.kept, again.rows.len()), (0, 5, 5));
    let table = fs::read_to_string(&again.table).unwrap();
    assert!(table.contains("5 of 5 slices run"), "{table}");
    assert!(table.contains("| `x64:some-construct` | 5 |"), "{table}");
    assert!(table.contains("| `both-timed-out` | 5 |"), "{table}");
    assert!(table.contains(&format!("{}:disagree:/f/x86_64-llvm22-seed1", slices[4].first)));
}

// "Budget": a row reads back as it was written; the table adds rows up.
#[test]
fn rows_and_the_table() {
    let scratch = Scratch::new("fuzz-budget-rows");
    let mut options = options(&scratch);
    fs::create_dir_all(&options.out).unwrap();
    let slice = budget::plan(BASE, &[Arch::X86_64])[0];
    (options.seed, options.count) = (slice.first, slice.count);
    let mut summary = summary_of(&options, true);
    summary.errors.push((7, "llc failed:\tsomething; else".to_owned()));
    // A seed set apart by a class (`llc19-int128-last-register`): counted
    // in the set-apart column and listed with its seed.
    summary.set_apart.push((9, "llc19-int128-last-register", "the evidence".to_owned()));
    let row = Row::new(&slice, &summary);
    assert_eq!(row.seed_count(), slice.count);
    assert_eq!((row.refused, row.skipped, row.error, row.finding), (1, 1, 1, 1));
    assert_eq!(row.set_apart, "llc19-int128-last-register=1");
    assert_eq!(row.set_apart_seeds, "9:llc19-int128-last-register");
    assert_eq!(Row::parse(&row.line()), Some(row.clone()));
    assert_eq!(Row::parse(budget::HEADER), None);
    // A row written before the set-apart-seeds column still parses.
    let old = row.line().rsplit_once('\t').unwrap().0.to_owned();
    assert_eq!(Row::parse(&old).map(|r| r.set_apart_seeds), Some("-".to_owned()));
    let table = budget::markdown(&[row.clone(), row], 3);
    assert!(table.contains("2 of 3 slices run"), "{table}");
    let total = format!("| x86_64 | 22 | {} | ", 2 * slice.count);
    assert!(table.contains(&total), "{table}");
    // refused, skipped, error, finding, set apart; minutes.
    assert!(table.contains(" | 2 | 2 | 2 | 2 | 2 | 3 | "), "{table}");
    assert!(table.contains("| `llc19-int128-last-register` | 2 |"), "{table}");
    assert!(table.contains("## Set apart (seeds)\n\n- x86_64 LLVM 22"), "{table}");
    assert!(table.contains(": 9:llc19-int128-last-register"), "{table}");
    assert!(table.contains("7:llc failed: something  else"), "{table}");
}

#[test]
fn the_options_of_the_budget() {
    let parse = |args: &[&str]| {
        let args: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
        let mut options = Options::new(PathBuf::from("/b"));
        parse_args(&args, &mut options).map(|_| options)
    };
    let options = parse(&["--budget", "--seed", "100001", "--hod", "/h", "-j", "2"]).unwrap();
    assert_eq!(options.budget, Some(budget::Action::Run));
    assert_eq!(
        parse(&["--budget", "--plan", "--seed", "1"]).unwrap().budget,
        Some(budget::Action::Plan)
    );
    assert_eq!(parse(&["--seed", "1"]).unwrap().budget, None);
    for (args, needle) in [
        (&["--budget", "--hod", "/h"][..], "needs '--seed"),
        (&["--budget", "--seed", "5"], "needs '--hod"),
        (&["--budget", "--seed", "5", "--hod", "/h", "--mix"], "do not apply"),
        (&["--budget", "--seed", "5", "--hod", "/h", "--count", "3"], "do not apply"),
        (&["--plan"], "needs '--budget'"),
    ] {
        let error = parse(args).unwrap_err();
        assert!(error.contains(needle), "{args:?}: {error}");
    }
}
