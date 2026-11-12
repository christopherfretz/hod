//! `cargo xtask corpus reasons`: why hod declines each module of the
//! corpus, by each route a module can take, and what declining costs.
//! Contract: `spec/cli.md` "Reasons" ("The census of the corpus").
//!
//! For every entry: its text as written, its bitcode (from the `llvm-as`
//! of the entry's LLVM) read natively, and the same bitcode through that
//! LLVM's `llvm-dis`; each once for the first refusal (the cost) and once
//! with `HOD_CENSUS=1` (every reason). Optionally `llc` on the bitcode (to
//! put the cost in proportion) and an older hod (the cost before).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use super::manifest::Manifest;
use crate::irdiff::process::{self, End, Limits};

/// What to run.
#[derive(Clone, Debug)]
pub struct Options {
    /// The corpus directory.
    pub dir: PathBuf,
    /// The hod to ask.
    pub hod: PathBuf,
    /// An older hod to time the same declines with (first refusal only).
    pub before: Option<PathBuf>,
    /// Also time `llc` on each module's bitcode.
    pub llc: bool,
    /// Modules at once.
    pub jobs: usize,
    /// Only the first N entries of each LLVM major.
    pub limit: Option<usize>,
    /// Where the scratch coverage logs go.
    pub work: PathBuf,
}

const LIMITS: Limits = Limits { time: Duration::from_mins(5), memory_kib: 8 << 20 };

/// The variables of hod's that the runs set themselves.
const HOD_VARS: [&str; 7] = [
    "HOD_FALLBACK_LLC",
    "HOD_STRICT",
    "HOD_COVERAGE_LOG",
    "HOD_BITCODE_VIA",
    "HOD_IN_FALLBACK",
    "HOD_TIME_PHASES",
    "HOD_CENSUS",
];

/// One invocation of hod, from its coverage line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Run {
    outcome: String,
    reason: String,
    hod_micros: u64,
    convert_micros: Option<u64>,
    /// Field 11, `-` without a census (or from a hod that has no field 11).
    reasons: String,
}

/// The routes of one module.
#[derive(Clone, Debug, Default)]
struct Row {
    entry: String,
    llvm: u32,
    text: Run,
    native: Run,
    dis: Run,
    text_census: Run,
    native_census: Run,
    dis_census: Run,
    llc_micros: Option<u64>,
    before_text: Option<u64>,
    before_bitcode: Option<u64>,
}

fn bin(llvm: u32, tool: &str) -> PathBuf {
    PathBuf::from(format!("/usr/lib/llvm-{llvm}/bin/{tool}"))
}

fn ended_well(what: &str, output: &process::Output) -> Result<(), String> {
    match output.end {
        End::Exit(0) => Ok(()),
        end => Err(format!("{what}: {end:?}: {}", output.first_error_line())),
    }
}

/// Runs `hod` in strict mode on `input` with the entry's `llc` arguments
/// and reads its coverage line.
fn ask(
    hod: &Path,
    log: &Path,
    input: &[u8],
    llc_args: &[&str],
    via: Option<&str>,
    census: bool,
) -> Result<Run, String> {
    let _ = std::fs::remove_file(log);
    let output = process::run(
        hod,
        |command| {
            for var in HOD_VARS {
                command.env_remove(var);
            }
            command.arg("--strict").arg(format!("--coverage-log={}", log.display()));
            if let Some(via) = via {
                command.arg(format!("--bitcode-via={via}"));
            }
            command.args(llc_args).args(["-o", "/dev/null", "-"]);
            if census {
                command.env("HOD_CENSUS", "1");
            }
        },
        input,
        LIMITS,
    )?;
    let text = std::fs::read_to_string(log).unwrap_or_default();
    let fields: Vec<&str> = text.trim_end_matches('\n').split('\t').collect();
    if fields.len() < 10 || text.lines().count() != 1 {
        return Err(format!(
            "{}: no coverage line ({:?}): {}",
            hod.display(),
            output.end,
            output.first_error_line()
        ));
    }
    Ok(Run {
        outcome: fields[0].to_owned(),
        reason: fields[1].to_owned(),
        hod_micros: fields[6].parse().unwrap_or_default(),
        convert_micros: fields[8].parse().ok(),
        reasons: fields.get(10).unwrap_or(&"-").to_string(),
    })
}

fn measure(
    options: &Options,
    log: &Path,
    entry: &str,
    llvm: u32,
    llc_args: &str,
) -> Result<Row, String> {
    let path = options.dir.join(entry);
    let text =
        std::fs::read(&path).map_err(|e| format!("cannot read '{}': {e}", path.display()))?;
    let args: Vec<&str> = llc_args.split_whitespace().collect();
    let assembled =
        process::run(&bin(llvm, "llvm-as"), |c| _ = c.args(["-o", "-", "-"]), &text, LIMITS)?;
    ended_well(&format!("llvm-as of {entry}"), &assembled)?;
    let bitcode = assembled.stdout;
    let dis = bin(llvm, "llvm-dis").display().to_string();
    let hod = &options.hod;
    let mut row = Row {
        entry: entry.to_owned(),
        llvm,
        text: ask(hod, log, &text, &args, None, false)?,
        native: ask(hod, log, &bitcode, &args, Some("native"), false)?,
        dis: ask(hod, log, &bitcode, &args, Some(&dis), false)?,
        text_census: ask(hod, log, &text, &args, None, true)?,
        native_census: ask(hod, log, &bitcode, &args, Some("native"), true)?,
        dis_census: ask(hod, log, &bitcode, &args, Some(&dis), true)?,
        ..Row::default()
    };
    if let Some(before) = &options.before {
        row.before_text = Some(ask(before, log, &text, &args, None, false)?.hod_micros);
        row.before_bitcode = Some(ask(before, log, &bitcode, &args, None, false)?.hod_micros);
    }
    if options.llc {
        let llc = bin(llvm, "llc");
        let (output, took) = process::run_timed(
            &llc,
            |c| _ = c.args(&args).args(["-o", "/dev/null", "-"]),
            &bitcode,
            LIMITS,
        )?;
        ended_well(&format!("llc of {entry}"), &output)?;
        row.llc_micros = Some(u64::try_from(took.as_micros()).unwrap_or(u64::MAX));
    }
    Ok(row)
}

/// The median, the 99th percentile and the largest of `values`.
fn spread(values: &mut [u64]) -> Option<(u64, u64, u64)> {
    values.sort_unstable();
    let at = |p: usize| values[(values.len() - 1) * p / 100];
    (!values.is_empty()).then(|| (at(50), at(99), at(100)))
}

fn spread_f(values: &mut [f64]) -> Option<(f64, f64, f64)> {
    values.sort_by(f64::total_cmp);
    let at = |p: usize| values[(values.len() - 1) * p / 100];
    (!values.is_empty()).then(|| (at(50), at(99), at(100)))
}

/// Runs every entry and returns the report (Markdown).
///
/// # Errors
/// A corpus that cannot be read, or a tool that fails on an entry.
pub fn run(options: &Options) -> Result<String, String> {
    let manifest = Manifest::read(&options.dir)?;
    let mut entries: Vec<(String, u32, String)> = Vec::new();
    let mut per_major: BTreeMap<u32, usize> = BTreeMap::new();
    for row in manifest.rows.values() {
        let taken = per_major.entry(row.llvm).or_default();
        if options.limit.is_some_and(|limit| *taken >= limit) {
            continue;
        }
        *taken += 1;
        entries.push((row.entry.clone(), row.llvm, row.llc_args.clone()));
    }
    std::fs::create_dir_all(&options.work)
        .map_err(|e| format!("cannot create '{}': {e}", options.work.display()))?;
    let next = AtomicUsize::new(0);
    let rows = Mutex::new(Vec::new());
    let failures = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for worker in 0..options.jobs.clamp(1, entries.len().max(1)) {
            let (next, rows, failures, entries) = (&next, &rows, &failures, &entries);
            scope.spawn(move || {
                let log = options.work.join(format!("worker-{worker}.tsv"));
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some((entry, llvm, llc_args)) = entries.get(index) else { break };
                    match measure(options, &log, entry, *llvm, llc_args) {
                        Ok(row) => rows.lock().unwrap_or_else(PoisonError::into_inner).push(row),
                        Err(message) => {
                            failures.lock().unwrap_or_else(PoisonError::into_inner).push(message);
                        }
                    }
                }
                let _ = std::fs::remove_file(&log);
            });
        }
    });
    let failures = failures.into_inner().unwrap_or_else(PoisonError::into_inner);
    if let Some(first) = failures.first() {
        return Err(format!("{} entries failed; the first: {first}", failures.len()));
    }
    let mut rows = rows.into_inner().unwrap_or_else(PoisonError::into_inner);
    rows.sort_by(|a, b| (b.llvm, &a.entry).cmp(&(a.llvm, &b.entry)));
    Ok(report(&rows, options))
}

/// `reason -> major -> modules`, in order of the most modules.
fn counts<'r>(
    rows: &'r [Row],
    names: impl Fn(&'r Row) -> Vec<&'r str>,
) -> Vec<(&'r str, BTreeMap<u32, usize>)> {
    let mut by_name: BTreeMap<&str, BTreeMap<u32, usize>> = BTreeMap::new();
    for row in rows {
        for name in names(row) {
            *by_name.entry(name).or_default().entry(row.llvm).or_default() += 1;
        }
    }
    let mut list: Vec<_> = by_name.into_iter().collect();
    list.sort_by_key(|(name, majors)| (std::cmp::Reverse(majors.values().sum::<usize>()), *name));
    list
}

fn census_names(run: &Run) -> Vec<&str> {
    if run.reasons == "-" { Vec::new() } else { run.reasons.split(',').collect() }
}

fn report(rows: &[Row], options: &Options) -> String {
    let mut majors: Vec<u32> = rows.iter().map(|row| row.llvm).collect();
    majors.sort_unstable_by(|a, b| b.cmp(a));
    majors.dedup();
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# Why hod declines the corpus\n\n<!-- Generated by `cargo xtask corpus reasons --report <this file>`. \
         Do not edit: run the command again. -->\n\n\
         Every module of the captured corpus (`docs/status/corpus.md`), given to hod in strict mode \
         with its `llc` arguments: as bitcode from its LLVM's `llvm-as`, read natively (the default \
         route). The first refusal is the reason the coverage log records; the census \
         (`HOD_CENSUS=1`) is every reason, the list of what to implement next \
         (`spec/cli.md` \"Reasons\"). A count is of modules.\n"
    );
    let header = |out: &mut String, first: &str| {
        let _ = write!(out, "| {first} |");
        for major in &majors {
            let _ = write!(out, " LLVM {major} |");
        }
        let _ = write!(out, "\n|---|");
        for _ in &majors {
            let _ = write!(out, "---:|");
        }
        out.push('\n');
    };
    let table = |out: &mut String, list: &[(&str, BTreeMap<u32, usize>)]| {
        for (name, by_major) in list {
            let _ = write!(out, "| `{name}` |");
            for major in &majors {
                let _ = write!(out, " {} |", by_major.get(major).copied().unwrap_or(0));
            }
            out.push('\n');
        }
    };
    let total: BTreeMap<u32, usize> =
        majors.iter().map(|&m| (m, rows.iter().filter(|r| r.llvm == m).count())).collect();
    let _ = writeln!(out, "## First refusal\n");
    header(&mut out, "reason");
    table(&mut out, &counts(rows, |row| vec![row.native.reason.as_str()]));
    let _ = write!(out, "| all |");
    for major in &majors {
        let _ = write!(out, " {} |", total[major]);
    }
    let _ = writeln!(out, "\n\n## Every reason (census)\n");
    header(&mut out, "reason");
    table(&mut out, &counts(rows, |row| census_names(&row.native_census)));
    agreement(&mut out, rows, &majors);
    costs(&mut out, rows, &majors, options);
    out
}

/// The section on the routes' agreement.
fn agreement(out: &mut String, rows: &[Row], majors: &[u32]) {
    let _ = writeln!(
        out,
        "\n## The routes agree\n\nModules whose reason (and outcome) is the same by two routes, \
         and whose census is.\n"
    );
    let _ = writeln!(
        out,
        "| LLVM | modules | text = native | native = `llvm-dis` | census: native = `llvm-dis` | census: text = native |\n|---|---:|---:|---:|---:|---:|"
    );
    let same = |a: &Run, b: &Run| a.outcome == b.outcome && a.reason == b.reason;
    let mut differ = Vec::new();
    for &major in majors {
        let of: Vec<&Row> = rows.iter().filter(|r| r.llvm == major).collect();
        let count = |f: &dyn Fn(&Row) -> bool| of.iter().filter(|r| f(r)).count();
        let _ = writeln!(
            out,
            "| {major} | {} | {} | {} | {} | {} |",
            of.len(),
            count(&|r| same(&r.text, &r.native)),
            count(&|r| same(&r.native, &r.dis)),
            count(&|r| r.native_census.reasons == r.dis_census.reasons),
            count(&|r| r.text_census.reasons == r.native_census.reasons),
        );
        for row in of {
            if !same(&row.text, &row.native)
                || !same(&row.native, &row.dis)
                || row.native_census.reasons != row.dis_census.reasons
                || row.text_census.reasons != row.native_census.reasons
            {
                differ.push(row);
            }
        }
    }
    if !differ.is_empty() {
        let _ = writeln!(out, "\nThe modules that differ (at most 20):\n");
        for row in differ.iter().take(20) {
            let _ = writeln!(
                out,
                "- `{}`: text `{}`, native `{}`, `llvm-dis` `{}`",
                row.entry, row.text.reason, row.native.reason, row.dis.reason
            );
        }
    }
}

/// A route's number for one module.
type Value<'a> = &'a dyn Fn(&Row) -> Option<u64>;

#[allow(clippy::cast_precision_loss, reason = "microseconds, far below 2^52")]
fn share(part: u64, whole: u64) -> f64 {
    part as f64 / whole as f64
}

/// The section on the cost of a decline.
fn costs(out: &mut String, rows: &[Row], majors: &[u32], options: &Options) {
    let _ = writeln!(
        out,
        "\n## What a decline costs\n\nCoverage-log field 7 (from the process's start to the decline) \
         of the first-refusal runs, in microseconds{}; `llc` is its wall time on the bitcode \
         (`-o /dev/null`, from the jobserver token to its end, a shell wrapper included).\n",
        if options.before.is_some() {
            ", with `before` an older hod on the same input"
        } else {
            ""
        }
    );
    let _ = writeln!(
        out,
        "| LLVM | route | median | p99 | worst | share of `llc`: median | p99 | worst |\n|---|---|---:|---:|---:|---:|---:|---:|"
    );
    for &major in majors {
        let of: Vec<&Row> = rows.iter().filter(|r| r.llvm == major).collect();
        let routes: [(&str, Value<'_>); 6] = [
            ("text", &|r| Some(r.text.hod_micros)),
            ("bitcode, native", &|r| Some(r.native.hod_micros)),
            ("bitcode, `llvm-dis`", &|r| Some(r.dis.hod_micros)),
            ("before: text", &|r| r.before_text),
            ("before: bitcode", &|r| r.before_bitcode),
            ("`llc -O0`", &|r| r.llc_micros),
        ];
        for (route, value) in routes {
            let mut values: Vec<u64> = of.iter().filter_map(|r| value(r)).collect();
            let mut shares: Vec<f64> = of
                .iter()
                .filter_map(|r| Some(share(value(r)?, r.llc_micros.filter(|&l| l > 0)?)))
                .collect();
            let Some((median, p99, worst)) = spread(&mut values) else { continue };
            let _ = write!(out, "| {major} | {route} | {median} | {p99} | {worst} |");
            match spread_f(&mut shares) {
                Some((m, p, w)) if !route.contains("llc") => {
                    let _ = writeln!(
                        out,
                        " {:.1}% | {:.1}% | {:.1}% |",
                        m * 100.0,
                        p * 100.0,
                        w * 100.0
                    );
                }
                _ => out.push_str(" | | |\n"),
            }
        }
        let mut converts: Vec<u64> = of.iter().filter_map(|r| r.dis.convert_micros).collect();
        if let Some((median, p99, worst)) = spread(&mut converts) {
            let _ = writeln!(
                out,
                "| {major} | the `convert` of the `llvm-dis` route | {median} | {p99} | {worst} | | | |"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spreads_and_census_fields() {
        let mut values: Vec<u64> = (1..=200).rev().collect();
        assert_eq!(spread(&mut values), Some((100, 198, 200)));
        assert_eq!(spread(&mut []), None);
        assert_eq!(spread(&mut [7]), Some((7, 7, 7)));
        let run = |reasons: &str| Run { reasons: reasons.to_owned(), ..Run::default() };
        assert!(census_names(&run("-")).is_empty());
        assert_eq!(census_names(&run("module:pic,cc:ccc")), ["module:pic", "cc:ccc"]);
    }

    #[test]
    fn arguments() {
        let args = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        let build = Path::new("/b/target-x");
        let (options, report) = crate::corpus::parse_reasons(&args(&[]), build).unwrap();
        assert_eq!(options.hod, Path::new("/b/target-x/release/hod"));
        assert_eq!(options.dir, Path::new("/b/corpus"));
        assert!(report.is_none() && !options.llc && options.before.is_none());
        let given = args(&["--llc", "--limit", "3", "-j", "2", "--report", "/r.md", "/c"]);
        let (options, report) = crate::corpus::parse_reasons(&given, build).unwrap();
        assert!(options.llc);
        assert_eq!((options.limit, options.jobs), (Some(3), 2));
        assert_eq!((report.unwrap(), options.dir), (PathBuf::from("/r.md"), PathBuf::from("/c")));
        assert!(crate::corpus::parse_reasons(&args(&["--bogus"]), build).is_err());
        assert!(crate::corpus::parse_reasons(&args(&["/a", "/b"]), build).is_err());
    }
}
