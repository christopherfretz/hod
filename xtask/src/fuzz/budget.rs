//! `cargo xtask fuzz --budget`: the fuzzing budget of a release candidate
//! (`docs/VALIDATION.md` "Random programs (L10)", "Release"; contract:
//! `spec/harness.md` "Random programs", "Budget").
//!
//! The budget is a fixed list of slices: a target, an LLVM major, a
//! profile, a mode and a range of seeds. [`plan`] lays it out from the
//! candidate's first seed, every range disjoint from every other; [`run`]
//! runs the slices that `<out>/budget.tsv` does not list yet, one at a
//! time, appends a row per slice and rewrites `<out>/budget.md` from the
//! file. A slice that was interrupted is run again whole; one that is in
//! the file is never rerun.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use super::case::Mode;
use super::profile::Profile;
use super::{Options, Summary};
use crate::irdiff::Arch;

/// One line of the budget: seeds per target at LLVM 22 for a profile and
/// a mode. LLVM 19 runs a quarter of each ([`LLVM19_SHARE`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Line {
    pub profile: &'static str,
    pub mode: Mode,
    pub seeds: u64,
}

const fn line(profile: &'static str, mode: Mode, seeds: u64) -> Line {
    Line { profile, mode, seeds }
}

/// The budget of 1.0, per target and at LLVM 22 (`docs/VALIDATION.md`
/// "Random programs (L10)" gives the reasons for the numbers).
pub const BUDGET: &[Line] = &[
    line("default", Mode::Whole, 2000),
    line("aggregates", Mode::Whole, 1000),
    line("float", Mode::Whole, 1000),
    line("wide", Mode::Whole, 1000),
    line("volatile", Mode::Whole, 1000),
    line("narrow", Mode::Whole, 1000),
    line("abi", Mode::Whole, 2000),
    line("default", Mode::Mix, 500),
    line("abi", Mode::Mix, 500),
    line("default", Mode::Debug, 500),
];

/// LLVM 19 runs one in this many of every line's seeds (rounded up).
pub const LLVM19_SHARE: u64 = 4;

/// The most seeds a slice has: what an interruption costs at most.
pub const CHUNK: u64 = 500;

/// The first seed no recorded run has used: the old page's runs used 1
/// to 2500; a candidate starts at a multiple of 100,000 above every seed
/// an earlier budget used (`docs/status/fuzz.md` names it).
pub const STRIDE: u64 = 100_000;

/// What `--budget` does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Runs the slices not in `budget.tsv` yet.
    Run,
    /// `--plan`: prints the slices.
    Plan,
}

/// One run of the command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slice {
    pub arch: Arch,
    pub llvm: u32,
    pub profile: &'static Profile,
    pub mode: Mode,
    pub first: u64,
    pub count: u64,
}

impl Slice {
    /// The last seed.
    pub fn last(&self) -> u64 {
        self.first + self.count - 1
    }

    /// What identifies the slice in `budget.tsv`.
    pub fn key(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}\t{}-{}",
            self.arch.name(),
            self.llvm,
            self.profile.name,
            self.mode.name(),
            self.first,
            self.last()
        )
    }
}

/// The slices of the budget from seed `base`, for `targets`, in the order
/// they run: LLVM 22 then 19, each target, the lines in order, chunks of at
/// most [`CHUNK`] seeds. Every slice's seeds follow the previous slice's,
/// so no two slices share a seed, whatever `targets` is (a target left out
/// keeps its ranges).
pub fn plan(base: u64, targets: &[Arch]) -> Vec<Slice> {
    let mut slices = Vec::new();
    let mut next = base;
    for llvm in [22, 19] {
        for arch in Arch::ALL {
            for line in BUDGET {
                let profile = Profile::by_name(line.profile).expect("a profile of the budget");
                let mut seeds =
                    if llvm == 22 { line.seeds } else { line.seeds.div_ceil(LLVM19_SHARE) };
                while seeds > 0 {
                    let count = seeds.min(CHUNK);
                    if targets.contains(&arch) {
                        slices.push(Slice {
                            arch,
                            llvm,
                            profile,
                            mode: line.mode,
                            first: next,
                            count,
                        });
                    }
                    next += count;
                    seeds -= count;
                }
            }
        }
    }
    slices
}

/// A slice's results: a row of `budget.tsv`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    pub target: String,
    pub llvm: String,
    pub profile: String,
    pub mode: String,
    /// `first-last`.
    pub seeds: String,
    pub agree: usize,
    pub refused: usize,
    pub skipped: usize,
    pub error: usize,
    pub finding: usize,
    pub minutes: f64,
    /// `reason=count,...`, most first.
    pub refusals: String,
    /// `why=count,...`.
    pub skips: String,
    /// `seed:kind:directory;...`.
    pub findings: String,
    /// `seed:detail;...`, each detail cut to 160 characters.
    pub errors: String,
    /// `--debug`'s pairs set apart by evidence, `class=count,...`
    /// (`selector-split`); `-` for none, and for a row written before the
    /// column existed.
    pub set_apart: String,
}

/// The header of `budget.tsv`.
pub const HEADER: &str = "# target\tllvm\tprofile\tmode\tseeds\tagree\trefused\tskipped\terror\t\
                          finding\tminutes\trefusals\tskips\tfindings\terrors\tset-apart";

fn clean(text: &str) -> String {
    text.replace(['\t', '\n', ';', ','], " ")
}

fn counts(map: &BTreeMap<String, usize>) -> String {
    let mut pairs: Vec<(&String, &usize)> = map.iter().collect();
    pairs.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    let pairs: Vec<String> = pairs.iter().map(|(k, n)| format!("{}={n}", clean(k))).collect();
    if pairs.is_empty() { "-".to_owned() } else { pairs.join(",") }
}

fn parse_counts(text: &str) -> Vec<(String, usize)> {
    text.split(',')
        .filter(|p| *p != "-" && !p.is_empty())
        .filter_map(|p| {
            let (name, n) = p.rsplit_once('=')?;
            Some((name.to_owned(), n.parse().ok()?))
        })
        .collect()
}

fn list(items: &[String]) -> String {
    if items.is_empty() { "-".to_owned() } else { items.join(";") }
}

impl Row {
    /// The row of `slice` from its run's summary.
    pub fn new(slice: &Slice, summary: &Summary) -> Self {
        let mut skipped = summary.skipped.clone();
        skipped.retain(|_, n| *n > 0);
        Self {
            target: slice.arch.name().to_owned(),
            llvm: slice.llvm.to_string(),
            profile: slice.profile.name.to_owned(),
            mode: slice.mode.name().to_owned(),
            seeds: format!("{}-{}", slice.first, slice.last()),
            agree: summary.agree,
            refused: summary.refused.values().sum(),
            skipped: skipped.values().sum(),
            error: summary.errors.len(),
            finding: summary.findings.len(),
            minutes: summary.elapsed.as_secs_f64() / 60.0,
            refusals: counts(&summary.refused),
            skips: counts(&skipped),
            findings: list(
                &summary
                    .findings
                    .iter()
                    .map(|f| {
                        format!(
                            "{}:{}:{}",
                            f.seed,
                            f.kind.name(),
                            clean(&f.dir.display().to_string())
                        )
                    })
                    .collect::<Vec<_>>(),
            ),
            errors: list(
                &summary
                    .errors
                    .iter()
                    .map(|(seed, detail)| {
                        format!("{seed}:{}", clean(&detail.chars().take(160).collect::<String>()))
                    })
                    .collect::<Vec<_>>(),
            ),
            set_apart: if summary.selector_splits.is_empty() {
                "-".to_owned()
            } else {
                format!("{}={}", super::case::SELECTOR_SPLIT, summary.selector_splits.len())
            },
        }
    }

    /// The slice's key (as [`Slice::key`]).
    pub fn key(&self) -> String {
        format!("{}\t{}\t{}\t{}\t{}", self.target, self.llvm, self.profile, self.mode, self.seeds)
    }

    /// The number of seeds of the range.
    pub fn seed_count(&self) -> u64 {
        self.seeds
            .split_once('-')
            .and_then(|(a, b)| Some(b.parse::<u64>().ok()? + 1 - a.parse::<u64>().ok()?))
            .unwrap_or(0)
    }

    /// The row as a line of `budget.tsv`.
    pub fn line(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{:.2}\t{}\t{}\t{}\t{}\t{}",
            self.key(),
            self.agree,
            self.refused,
            self.skipped,
            self.error,
            self.finding,
            self.minutes,
            self.refusals,
            self.skips,
            self.findings,
            self.errors,
            self.set_apart
        )
    }

    /// A line of `budget.tsv`, or `None` for the header, a blank line or
    /// a line cut short (a write the machine did not finish).
    pub fn parse(line: &str) -> Option<Self> {
        if line.starts_with('#') {
            return None;
        }
        let f: Vec<&str> = line.split('\t').collect();
        // 15 fields: a row written before the `set-apart` column.
        if f.len() != 15 && f.len() != 16 {
            return None;
        }
        let n = |i: usize| f[i].parse::<usize>().ok();
        Some(Self {
            target: f[0].to_owned(),
            llvm: f[1].to_owned(),
            profile: f[2].to_owned(),
            mode: f[3].to_owned(),
            seeds: f[4].to_owned(),
            agree: n(5)?,
            refused: n(6)?,
            skipped: n(7)?,
            error: n(8)?,
            finding: n(9)?,
            minutes: f[10].parse().ok()?,
            refusals: f[11].to_owned(),
            skips: f[12].to_owned(),
            findings: f[13].to_owned(),
            errors: f[14].to_owned(),
            set_apart: f.get(15).map_or("-", |s| *s).to_owned(),
        })
    }
}

/// The rows of `file` (none when it does not exist).
pub fn read_rows(file: &Path) -> Vec<Row> {
    fs::read_to_string(file)
        .map_or_else(|_| Vec::new(), |text| text.lines().filter_map(Row::parse).collect())
}

/// What a budget run did.
#[derive(Debug, Default)]
pub struct Outcome {
    /// Slices run now, and slices the file already had.
    pub ran: usize,
    pub kept: usize,
    /// Every row of the file, in the plan's order.
    pub rows: Vec<Row>,
    pub table: PathBuf,
}

impl Outcome {
    /// 1 with a finding, 3 with an error and no finding, 0 otherwise.
    pub fn exit_status(&self) -> u8 {
        if self.rows.iter().any(|r| r.finding > 0) {
            1
        } else if self.rows.iter().any(|r| r.error > 0) {
            3
        } else {
            0
        }
    }
}

/// Runs the slices of `slices` that `<out>/budget.tsv` does not have,
/// with `run_one` (the command's [`super::run`], or a test's stand-in),
/// each with `options` narrowed to the slice and its results file kept as
/// `<out>/results/<slice>.tsv`.
pub fn run(
    options: &Options,
    slices: &[Slice],
    mut run_one: impl FnMut(&Options) -> Result<Summary, String>,
) -> Result<Outcome, String> {
    let out = &options.out;
    fs::create_dir_all(out.join("results"))
        .map_err(|e| format!("cannot create '{}': {e}", out.display()))?;
    // The work directories (`run-<pid>-<time>`) of a budget run that was
    // stopped: nothing reads them, and a run cleans only its own.
    if let Ok(entries) = fs::read_dir(out) {
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with("run-") && entry.path().is_dir() {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
    }
    let file = out.join("budget.tsv");
    let mut done: BTreeMap<String, Row> =
        read_rows(&file).into_iter().map(|row| (row.key(), row)).collect();
    // Rewrite the file without lines cut short, so appends start clean.
    let mut text = format!("{HEADER}\n");
    for row in done.values() {
        let _ = writeln!(text, "{}", row.line());
    }
    fs::write(&file, &text).map_err(|e| format!("cannot write '{}': {e}", file.display()))?;
    let mut outcome = Outcome { table: out.join("budget.md"), ..Outcome::default() };
    let total = slices.len();
    for (index, slice) in slices.iter().enumerate() {
        if done.contains_key(&slice.key()) {
            outcome.kept += 1;
            continue;
        }
        eprintln!(
            "fuzz budget: slice {}/{total}: {} LLVM {} {} {} seeds {}-{}",
            index + 1,
            slice.arch.name(),
            slice.llvm,
            slice.profile.name,
            slice.mode.name(),
            slice.first,
            slice.last()
        );
        let mut narrowed = options.clone();
        narrowed.target = Some(slice.arch);
        narrowed.llvm = slice.llvm;
        narrowed.profile = slice.profile;
        narrowed.mode = slice.mode;
        narrowed.seed = slice.first;
        narrowed.count = slice.count;
        let summary = run_one(&narrowed)?;
        if let Some(results) = &summary.results_file {
            let name = format!("{}.tsv", slice.key().replace('\t', "-"));
            let _ = fs::rename(results, out.join("results").join(name));
        }
        let row = Row::new(slice, &summary);
        eprintln!(
            "fuzz budget: {} agree {} refused {} skipped {} error {} finding {} in {:.1} min",
            row.seeds, row.agree, row.refused, row.skipped, row.error, row.finding, row.minutes
        );
        // The evidence of each pair set apart, in the run's log.
        for (seed, split) in &summary.selector_splits {
            eprintln!("fuzz budget: {}", split.line(*seed));
        }
        let mut appended = fs::OpenOptions::new()
            .append(true)
            .open(&file)
            .map_err(|e| format!("cannot open '{}': {e}", file.display()))?;
        writeln!(appended, "{}", row.line())
            .and_then(|()| appended.sync_all())
            .map_err(|e| format!("cannot write '{}': {e}", file.display()))?;
        done.insert(row.key(), row);
        outcome.ran += 1;
        let rows = ordered(slices, &done);
        fs::write(&outcome.table, markdown(&rows, slices.len()))
            .map_err(|e| format!("cannot write '{}': {e}", outcome.table.display()))?;
    }
    outcome.rows = ordered(slices, &done);
    fs::write(&outcome.table, markdown(&outcome.rows, slices.len()))
        .map_err(|e| format!("cannot write '{}': {e}", outcome.table.display()))?;
    Ok(outcome)
}

/// The rows of the plan's slices that ran, in the plan's order.
fn ordered(slices: &[Slice], done: &BTreeMap<String, Row>) -> Vec<Row> {
    slices.iter().filter_map(|slice| done.get(&slice.key()).cloned()).collect()
}

#[derive(Default)]
struct Sum {
    seeds: u64,
    agree: usize,
    refused: usize,
    skipped: usize,
    error: usize,
    finding: usize,
    minutes: f64,
    ranges: Vec<String>,
}

impl Sum {
    fn add(&mut self, row: &Row) {
        self.seeds += row.seed_count();
        self.agree += row.agree;
        self.refused += row.refused;
        self.skipped += row.skipped;
        self.error += row.error;
        self.finding += row.finding;
        self.minutes += row.minutes;
        self.ranges.push(row.seeds.clone());
    }

    fn cells(&self) -> String {
        #[expect(clippy::cast_precision_loss, reason = "a rate, for people")]
        let rate = if self.minutes > 0.0 { self.seeds as f64 / self.minutes } else { 0.0 };
        format!(
            "{} | {} | {} | {} | {} | {} | {:.0} | {rate:.1}",
            self.seeds,
            self.agree,
            self.refused,
            self.skipped,
            self.error,
            self.finding,
            self.minutes
        )
    }
}

/// `budget.md`: the table per target, LLVM, profile and mode, the totals
/// per target and LLVM, the census of refusals and skips, the findings
/// and the errors.
pub fn markdown(rows: &[Row], planned: usize) -> String {
    let mut text = String::new();
    let _ = writeln!(text, "# Fuzzing budget\n\n{} of {planned} slices run.\n", rows.len());
    let head = "seeds | agree | refused | skipped | error | finding | minutes | programs/min";
    let rule = "---:|---:|---:|---:|---:|---:|---:|---:";
    let mut by_line: BTreeMap<(String, String, String, String), Sum> = BTreeMap::new();
    let mut order: Vec<(String, String, String, String)> = Vec::new();
    let mut totals: BTreeMap<(String, String), Sum> = BTreeMap::new();
    let mut refusals: BTreeMap<String, usize> = BTreeMap::new();
    let mut skips: BTreeMap<String, usize> = BTreeMap::new();
    let mut set_apart: BTreeMap<String, usize> = BTreeMap::new();
    let mut all = Sum::default();
    for row in rows {
        let key = (row.target.clone(), row.llvm.clone(), row.profile.clone(), row.mode.clone());
        if !by_line.contains_key(&key) {
            order.push(key.clone());
        }
        by_line.entry(key).or_default().add(row);
        totals.entry((row.target.clone(), row.llvm.clone())).or_default().add(row);
        all.add(row);
        for (name, n) in parse_counts(&row.refusals) {
            *refusals.entry(name).or_default() += n;
        }
        for (name, n) in parse_counts(&row.skips) {
            *skips.entry(name).or_default() += n;
        }
        for (name, n) in parse_counts(&row.set_apart) {
            *set_apart.entry(name).or_default() += n;
        }
    }
    let _ = writeln!(text, "| target | LLVM | {head} |\n|---|---|{rule}|");
    for ((target, llvm), sum) in totals.iter().rev() {
        let _ = writeln!(text, "| {target} | {llvm} | {} |", sum.cells());
    }
    let _ = writeln!(text, "| all | | {} |\n", all.cells());
    let _ = writeln!(
        text,
        "| target | LLVM | profile | mode | seed ranges | {head} |\n|---|---|---|---|---|{rule}|"
    );
    for key in &order {
        let sum = &by_line[key];
        let ranges = merge_ranges(&sum.ranges);
        let _ = writeln!(
            text,
            "| {} | {} | {} | {} | {ranges} | {} |",
            key.0,
            key.1,
            key.2,
            key.3,
            sum.cells()
        );
    }
    let census = |text: &mut String, title: &str, map: &BTreeMap<String, usize>, unit: &str| {
        let _ = writeln!(text, "\n## {title}\n");
        if map.is_empty() {
            let _ = writeln!(text, "None.");
            return;
        }
        let mut pairs: Vec<(&String, &usize)> = map.iter().collect();
        pairs.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
        let _ = writeln!(text, "| name | {unit} |\n|---|---:|");
        for (name, n) in pairs {
            let _ = writeln!(text, "| `{name}` | {n} |");
        }
    };
    census(&mut text, "Refusals", &refusals, "seeds");
    census(&mut text, "Skipped", &skips, "seeds");
    // The evidence of each pair is in the slice's summary in the run's log.
    census(&mut text, "Set apart (debug pairs)", &set_apart, "pairs");
    let _ = writeln!(text, "\n## Findings\n");
    let mut any = false;
    for row in rows.iter().filter(|r| r.findings != "-") {
        for item in row.findings.split(';') {
            any = true;
            let _ = writeln!(
                text,
                "- {} LLVM {} {} {}: {item}",
                row.target, row.llvm, row.profile, row.mode
            );
        }
    }
    if !any {
        let _ = writeln!(text, "None.");
    }
    let _ = writeln!(text, "\n## Errors (the oracle's side)\n");
    any = false;
    for row in rows.iter().filter(|r| r.errors != "-") {
        for item in row.errors.split(';') {
            any = true;
            let _ = writeln!(
                text,
                "- {} LLVM {} {} {}: {item}",
                row.target, row.llvm, row.profile, row.mode
            );
        }
    }
    if !any {
        let _ = writeln!(text, "None.");
    }
    text
}

/// `a-b` ranges joined where one ends where the next starts.
fn merge_ranges(ranges: &[String]) -> String {
    let mut merged: Vec<(u64, u64)> = Vec::new();
    for range in ranges {
        let Some((a, b)) = range.split_once('-') else { continue };
        let (Ok(a), Ok(b)) = (a.parse::<u64>(), b.parse::<u64>()) else { continue };
        match merged.last_mut() {
            Some(last) if last.1 + 1 == a => last.1 = b,
            _ => merged.push((a, b)),
        }
    }
    merged.iter().map(|(a, b)| format!("{a}-{b}")).collect::<Vec<_>>().join(", ")
}

/// The plan as a table, for `--budget --plan`.
pub fn plan_text(slices: &[Slice]) -> String {
    let mut text = String::from("target\tllvm\tprofile\tmode\tseeds\tcount\n");
    let mut total = 0;
    for slice in slices {
        total += slice.count;
        let _ = writeln!(text, "{}\t{}", slice.key(), slice.count);
    }
    let _ = writeln!(text, "{} slices, {total} seeds", slices.len());
    text
}
