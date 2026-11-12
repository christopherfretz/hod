//! The benchmark gate: hod's numbers against a recorded baseline.
//! Contract: `spec/harness.md` "bench", "The gate" and "Baselines".

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use super::record::{Record, Results, Status, Tool};

/// What the gate allows. Fractions are in hundredths of a percent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Thresholds {
    /// Allowed growth of wall time: 5%.
    pub time_bp: u64,
    /// Allowed growth of peak memory: 2%.
    pub mem_bp: u64,
    /// A time difference below this many microseconds is not judged: it
    /// is inside what starting a process varies by.
    pub time_slack_us: u64,
    /// A memory difference below this many KiB is not judged: the
    /// resident set of one program on one input varies by a few hundred
    /// KiB with the state of the page cache.
    pub mem_slack_kib: u64,
    /// How much slower than in the baseline the reference tool may be
    /// before the machine is too busy to time anything: 25%.
    pub max_drift_bp: u64,
    /// How far the middle run of a measurement may be above its fastest
    /// before the measurement cannot show a time regression: 5%, the time
    /// threshold itself.
    pub max_spread_bp: u64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            time_bp: 500,
            mem_bp: 200,
            time_slack_us: 1000,
            mem_slack_kib: 512,
            max_drift_bp: 2500,
            max_spread_bp: 500,
        }
    }
}

/// What the gate says of one input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Within the thresholds.
    Ok,
    /// Faster or smaller than the baseline by more than a threshold: a
    /// hint to update the baseline, never a failure.
    Improved,
    /// Over a threshold.
    Regressed { time: bool, mem: bool },
    /// The baseline says hod built this input, and now it does not.
    Lost,
    /// The times cannot be judged: the reference tool is far slower than
    /// in the baseline, or a time that says "slower" is one whose own runs
    /// disagree by more than the threshold.
    Noisy,
    /// Nothing to compare with; the text says why.
    New(String),
    /// Compared, but the comparison does not bind; the text says why.
    Advisory(String),
    /// The run has no number for this input; the text says why.
    Unjudged(String),
}

impl Verdict {
    /// Whether the gate fails on this verdict (exit status 1).
    pub fn fails(&self) -> bool {
        matches!(self, Self::Regressed { .. } | Self::Lost)
    }

    /// Whether the input is measured again before the verdict stands.
    pub fn remeasure(&self) -> bool {
        matches!(self, Self::Regressed { .. } | Self::Noisy)
    }

    pub fn label(&self) -> String {
        match self {
            Self::Ok => "ok".to_owned(),
            Self::Improved => "improved".to_owned(),
            Self::Regressed { time: true, mem: true } => "REGRESSED (time, memory)".to_owned(),
            Self::Regressed { time: true, mem: false } => "REGRESSED (time)".to_owned(),
            Self::Regressed { .. } => "REGRESSED (memory)".to_owned(),
            Self::Lost => "LOST".to_owned(),
            Self::Noisy => "NOISY".to_owned(),
            Self::New(why) => format!("new: {why}"),
            Self::Advisory(why) => format!("advisory: {why}"),
            Self::Unjudged(why) => format!("not judged: {why}"),
        }
    }
}

/// One input, judged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Judged {
    pub input: String,
    pub verdict: Verdict,
    /// What hod did with the input in this run.
    pub status: Status,
    /// Baseline and current figures that were compared: the fastest wall
    /// time in microseconds and the peak memory in KiB on the baseline's
    /// machine, the ratios to `llc` (in 1/10000) elsewhere.
    pub time: Option<(u64, u64)>,
    pub mem: Option<(u64, u64)>,
    /// Whether `time` and `mem` are ratios to `llc`.
    pub ratios: bool,
    /// How much slower the reference tool is than in the baseline, in
    /// 1/10000 (10000 is "as fast"); `None` when it cannot be told.
    pub drift: Option<u64>,
    /// How many times the input was measured again.
    pub remeasured: u32,
}

/// The change from `base` to `now` in hundredths of a percent of `base`.
pub fn delta_bp(base: u64, now: u64) -> Option<i64> {
    if base == 0 {
        return None;
    }
    let scaled = (i128::from(now) - i128::from(base)) * 10_000 / i128::from(base);
    i64::try_from(scaled).ok()
}

/// Whether `now` is above `base` by more than `bp` hundredths of a percent
/// and by more than `slack`.
fn over(base: u64, now: u64, bp: u64, slack: u64) -> bool {
    u128::from(now) * 10_000 > u128::from(base) * u128::from(10_000 + bp) && now - base > slack
}

/// Judges hod's record of one input against the baseline's.
///
/// `base` and `now` are hod's records, `base_llc` and `now_llc` the
/// reference tool's for the same input; `comparable` is whether the
/// baseline was recorded on this machine with the same reference tool.
pub fn judge(
    base: Option<&Record>,
    base_llc: Option<&Record>,
    now: &Record,
    now_llc: Option<&Record>,
    comparable: Result<(), &str>,
    thresholds: &Thresholds,
) -> Judged {
    let mut judged = Judged {
        input: now.input.clone(),
        verdict: Verdict::Ok,
        status: now.status,
        time: None,
        mem: None,
        ratios: false,
        drift: None,
        remeasured: 0,
    };
    if now.status == Status::Error {
        judged.verdict = Verdict::Unjudged("the run has an error for this input".to_owned());
        return judged;
    }
    let Some(base) = base.filter(|base| base.status != Status::Error) else {
        judged.verdict = Verdict::New("the baseline does not have this input".to_owned());
        return judged;
    };
    if base.ir_hash != now.ir_hash {
        judged.verdict = Verdict::Advisory("the input changed since the baseline".to_owned());
        return judged;
    }
    if base.status == Status::Declined && now.status == Status::Declined {
        // A decline is not a time for building an object: there is
        // nothing of hod's to hold to the thresholds. What declining
        // costs is shown where the baseline's machine is this one.
        if comparable.is_ok() {
            judged.time = base.wall_us_min.zip(now.wall_us_min);
            judged.mem = base.rss_kib.zip(now.rss_kib);
        }
        judged.verdict = Verdict::Unjudged("declined, as in the baseline".to_owned());
        return judged;
    }
    if let Err(why) = comparable {
        // Absolute numbers mean nothing here; the ratios to llc do.
        judged.ratios = true;
        judged.time = base.wall_ratio.zip(now.wall_ratio).map(|(b, n)| (b.0, n.0));
        judged.mem = base.rss_ratio.zip(now.rss_ratio).map(|(b, n)| (b.0, n.0));
        judged.verdict = Verdict::Advisory(why.to_owned());
        return judged;
    }
    judged.time = base.wall_us_min.zip(now.wall_us_min);
    judged.mem = base.rss_kib.zip(now.rss_kib);
    match (base.status == Status::Ok, now.status == Status::Ok) {
        (true, false) => {
            judged.verdict = Verdict::Lost;
            return judged;
        }
        (false, true) => {
            judged.verdict = Verdict::New(
                "hod builds this input now; the baseline has no time for that".to_owned(),
            );
            return judged;
        }
        _ => {}
    }
    // The reference tool is the control: it did not change, so what it
    // lost since the baseline, the machine lost.
    let reference = |record: Option<&Record>| {
        record
            .filter(|llc| llc.status == Status::Ok)
            .and_then(|llc| llc.wall_us_min)
            .filter(|&us| us > 0)
    };
    judged.drift = reference(base_llc)
        .zip(reference(now_llc))
        .and_then(|(base, now)| u64::try_from(u128::from(now) * 10_000 / u128::from(base)).ok());
    let drift = judged.drift.unwrap_or(10_000).max(10_000);
    let busy = drift > 10_000 + thresholds.max_drift_bp;
    let scaled =
        |us: u64| u64::try_from(u128::from(us) * u128::from(drift) / 10_000).unwrap_or(u64::MAX);
    let (slower, faster) = judged.time.map_or((false, false), |(base, now)| {
        (
            over(scaled(base), now, thresholds.time_bp, thresholds.time_slack_us),
            over(now, base, thresholds.time_bp, thresholds.time_slack_us),
        )
    });
    // "Slower" needs four fastest runs to be right: hod's and the
    // reference tool's, then and now. One whose own runs disagree by more
    // than the threshold cannot carry that.
    let unsteady = [Some(base), base_llc, Some(now), now_llc]
        .into_iter()
        .flatten()
        .any(|record| record.spread_bp().is_some_and(|bp| bp > thresholds.max_spread_bp));
    let time_unknown = busy || (slower && unsteady);
    let (time_worse, time_better) = (slower && !time_unknown, faster && !busy);
    // Memory does not depend on the load, and is judged whatever it is.
    let (mem_worse, mem_better) = judged.mem.map_or((false, false), |(base, now)| {
        (
            over(base, now, thresholds.mem_bp, thresholds.mem_slack_kib),
            over(now, base, thresholds.mem_bp, thresholds.mem_slack_kib),
        )
    });
    judged.verdict = if time_worse || mem_worse {
        Verdict::Regressed { time: time_worse, mem: mem_worse }
    } else if time_unknown {
        Verdict::Noisy
    } else if time_better || mem_better {
        Verdict::Improved
    } else {
        Verdict::Ok
    };
    judged
}

/// How many inputs of `results` have a time, of any tool, whose runs
/// disagree by more than the gate allows: against such a baseline a
/// slower time is NOISY, not REGRESSED.
pub fn unsteady(results: &Results, thresholds: &Thresholds) -> usize {
    let mut inputs: Vec<&str> = results
        .records
        .iter()
        .filter(|record| matches!(record.status, Status::Ok | Status::Fallback))
        .filter(|record| record.spread_bp().is_some_and(|bp| bp > thresholds.max_spread_bp))
        .map(|record| record.input.as_str())
        .collect();
    inputs.sort_unstable();
    inputs.dedup();
    inputs.len()
}

/// Whether a baseline's absolute numbers bind a run: the same machine and
/// the same reference tool. The error says what differs.
pub fn comparable(baseline: &Results, run: &Results) -> Result<(), String> {
    for (key, what) in [("machine", "another machine"), ("llc_version", "another llc")] {
        let (base, now) = (baseline.get(key).unwrap_or("?"), run.get(key).unwrap_or("?"));
        if base != now {
            return Err(format!("{what} ({base})"));
        }
    }
    Ok(())
}

/// Judges every record of `tool` (hod, or hod with its fallback) in
/// `run`, in the run's order.
pub fn judge_all(
    baseline: &Results,
    run: &Results,
    tool: Tool,
    thresholds: &Thresholds,
) -> Vec<Judged> {
    let comparable = comparable(baseline, run);
    let mut all = Vec::new();
    for now in run.records.iter().filter(|record| record.tool == tool) {
        all.push(judge(
            baseline.find(&now.input, tool),
            baseline.find(&now.input, Tool::Llc),
            now,
            run.find(&now.input, Tool::Llc),
            comparable.as_ref().copied().map_err(String::as_str),
            thresholds,
        ));
    }
    all
}

/// The outcome of the gate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Check {
    pub baseline: PathBuf,
    /// When the baseline was recorded.
    pub recorded: String,
    /// `None`: the baseline binds this run. Else why it does not.
    pub advisory: Option<String>,
    pub judged: Vec<Judged>,
    pub thresholds: Thresholds,
}

impl Check {
    pub fn failures(&self) -> usize {
        self.judged.iter().filter(|j| j.verdict.fails()).count()
    }

    /// Inputs the machine was too busy to judge, after every retry.
    pub fn noisy(&self) -> usize {
        self.judged.iter().filter(|j| j.verdict == Verdict::Noisy).count()
    }

    /// The table and the count line.
    pub fn text(&self) -> String {
        let mut text = format!(
            "bench: check against {} (recorded {}){}\n",
            self.baseline.display(),
            self.recorded,
            self.advisory.as_ref().map_or_else(String::new, |why| format!("; advisory: {why}"))
        );
        let ratios = self.judged.iter().any(|j| j.ratios);
        let (time_unit, mem_unit) = if ratios { ("x llc", "x llc") } else { ("ms", "MiB") };
        let header = [
            "input".to_owned(),
            format!("base {time_unit}"),
            format!("now {time_unit}"),
            "time".to_owned(),
            "llc drift".to_owned(),
            format!("base {mem_unit}"),
            format!("now {mem_unit}"),
            "memory".to_owned(),
            "verdict".to_owned(),
        ];
        let mut rows = Vec::new();
        for judged in &self.judged {
            let pair = |values: Option<(u64, u64)>, absolute: fn(u64) -> String| match values {
                Some((base, now)) if judged.ratios => {
                    [times(base), times(now), percent(delta_bp(base, now))]
                }
                Some((base, now)) => [absolute(base), absolute(now), percent(delta_bp(base, now))],
                None => ["-".to_owned(), "-".to_owned(), "-".to_owned()],
            };
            let [base_time, now_time, time] = pair(judged.time, millis);
            let [base_mem, now_mem, mem] = pair(judged.mem, mib);
            let mut verdict = judged.verdict.label();
            if judged.status == Status::Declined && !matches!(judged.verdict, Verdict::Unjudged(_))
            {
                verdict.push_str("; declined");
            }
            if judged.remeasured > 0 {
                let _ = write!(verdict, "; measured again {}x", judged.remeasured);
            }
            rows.push(vec![
                judged.input.clone(),
                base_time,
                now_time,
                time,
                judged.drift.map_or_else(|| "-".to_owned(), |d| percent(Some(d as i64 - 10_000))),
                base_mem,
                now_mem,
                mem,
                verdict,
            ]);
        }
        text.push_str(&table(&header, &rows, &[0, 8]));
        let count = |wanted: fn(&Verdict) -> bool| {
            self.judged.iter().filter(|j| wanted(&j.verdict)).count()
        };
        let _ = writeln!(
            text,
            "bench: check: {} inputs: {} ok, {} improved, {} new, {} advisory, {} REGRESSED, {} LOST, \
             {} NOISY, {} not judged; {} measured again; thresholds: time +{}, memory +{}",
            self.judged.len(),
            count(|v| *v == Verdict::Ok),
            count(|v| *v == Verdict::Improved),
            count(|v| matches!(v, Verdict::New(_))),
            count(|v| matches!(v, Verdict::Advisory(_))),
            count(|v| matches!(v, Verdict::Regressed { .. })),
            count(|v| *v == Verdict::Lost),
            count(|v| *v == Verdict::Noisy),
            count(|v| matches!(v, Verdict::Unjudged(_))),
            self.judged.iter().filter(|j| j.remeasured > 0).count(),
            percent(Some(self.thresholds.time_bp as i64)).trim_start_matches('+'),
            percent(Some(self.thresholds.mem_bp as i64)).trim_start_matches('+'),
        );
        if self.noisy() > 0 {
            text.push_str(
                "bench: check: NOISY: the machine is too busy to time those inputs; \
                 neither a failure of hod nor a pass\n",
            );
        }
        if count(|v| *v == Verdict::Improved) > 0 {
            text.push_str("bench: check: an improvement is a reason to update the baseline (--update-baseline --reason)\n");
        }
        text
    }
}

/// Microseconds as milliseconds, two decimals.
pub fn millis(us: u64) -> String {
    format!("{}.{:02}", us / 1000, us % 1000 / 10)
}

/// KiB as MiB, one decimal.
pub fn mib(kib: u64) -> String {
    format!("{}.{}", kib / 1024, kib % 1024 * 10 / 1024)
}

/// A ratio in 1/10000 as `1.05x`.
pub fn times(ratio: u64) -> String {
    format!("{}.{:02}x", ratio / 10_000, ratio % 10_000 / 100)
}

/// Hundredths of a percent as `+5.0%`.
pub fn percent(bp: Option<i64>) -> String {
    bp.map_or_else(
        || "-".to_owned(),
        |bp| {
            format!("{}{}.{}%", if bp < 0 { '-' } else { '+' }, bp.abs() / 100, bp.abs() % 100 / 10)
        },
    )
}

/// A plain-text table. The columns named in `text` are left-aligned, the
/// others right-aligned; columns are two spaces apart.
pub fn table(header: &[String], rows: &[Vec<String>], text: &[usize]) -> String {
    let mut width: Vec<usize> = header.iter().map(|cell| cell.chars().count()).collect();
    for row in rows {
        for (index, cell) in row.iter().enumerate().take(width.len()) {
            width[index] = width[index].max(cell.chars().count());
        }
    }
    let mut out = String::new();
    for row in std::iter::once(header).chain(rows.iter().map(Vec::as_slice)) {
        let mut line = String::new();
        for (index, cell) in row.iter().enumerate().take(width.len()) {
            let gap = if index == 0 { "" } else { "  " };
            if text.contains(&index) {
                let _ = write!(line, "{gap}{cell:<w$}", w = width[index]);
            } else {
                let _ = write!(line, "{gap}{cell:>w$}", w = width[index]);
            }
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

/// The baseline of a machine: `<dir>/<machine id>.tsv`.
pub fn baseline_path(dir: &Path, machine_id: &str) -> PathBuf {
    dir.join(format!("{machine_id}.tsv"))
}

/// The log beside a baseline: the same name with `.log`.
pub fn log_path(baseline: &Path) -> PathBuf {
    baseline.with_extension("log")
}

/// Writes `run` as the baseline and appends the reason to its log: one
/// line, `<date><TAB><reason><TAB><what was recorded>`.
pub fn update_baseline(baseline: &Path, run: &Results, reason: &str) -> Result<(), String> {
    let reason = reason.trim();
    if reason.is_empty() {
        return Err("a baseline is updated with a reason (--reason \"<text>\")".to_owned());
    }
    if let Some(bad) = run.records.iter().find(|record| record.status == Status::Error) {
        return Err(format!(
            "the run has an error ({} with {}): it is not recorded as a baseline",
            bad.input,
            bad.tool.name()
        ));
    }
    run.write(baseline)?;
    let hod: Vec<&Record> = run.records.iter().filter(|record| record.tool == Tool::Hod).collect();
    let built = hod.iter().filter(|record| record.status == Status::Ok).count();
    let what = format!(
        "{} inputs, hod built {built}; {}; {} runs",
        hod.len(),
        run.get("hod_version").unwrap_or("hod ?"),
        run.get("runs").unwrap_or("?")
    );
    let flat = |text: &str| text.replace(['\t', '\n', '\r'], " ");
    let line = format!("{}\t{}\t{}\n", run.get("date").unwrap_or("-"), flat(reason), flat(&what));
    let log = log_path(baseline);
    let failed = |e: std::io::Error| format!("cannot write '{}': {e}", log.display());
    let mut file =
        std::fs::OpenOptions::new().append(true).create(true).open(&log).map_err(failed)?;
    file.write_all(line.as_bytes()).map_err(failed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::record::{Class, Ratio};

    fn record(tool: Tool, status: Status, wall: u64, rss: u64) -> Record {
        Record {
            ir_bytes: Some(1000),
            functions: Some(10),
            ir_hash: Some("fnv1a64:0000000000000001".to_owned()),
            runs: 3,
            wall_us_min: Some(wall),
            wall_us_median: Some(wall + wall / 20),
            wall_us_max: Some(wall * 2),
            cpu_us: Some(wall),
            rss_kib: Some(rss),
            rss_kib_min: Some(rss),
            rss_kib_max: Some(rss),
            object_bytes: (status == Status::Ok).then_some(500),
            ..Record::blank(status, "in", Class::Typical, false, tool)
        }
    }

    fn hod(wall: u64, rss: u64) -> Record {
        record(Tool::Hod, Status::Ok, wall, rss)
    }

    fn llc(wall: u64) -> Record {
        record(Tool::Llc, Status::Ok, wall, 60_000)
    }

    fn verdict(base: &Record, now: &Record, base_llc: u64, now_llc: u64) -> Verdict {
        judge(
            Some(base),
            Some(&llc(base_llc)),
            now,
            Some(&llc(now_llc)),
            Ok(()),
            &Thresholds::default(),
        )
        .verdict
    }

    // spec/harness.md "bench", "The gate": more than 5% slower, or more
    // than 2% larger, than the baseline fails; at the threshold passes.
    #[test]
    fn check_pass_and_fail() {
        let base = hod(100_000, 50_000);
        let time = |now: u64| verdict(&base, &hod(now, 50_000), 500_000, 500_000);
        assert_eq!(time(100_000), Verdict::Ok);
        assert_eq!(time(105_000), Verdict::Ok);
        assert_eq!(time(105_001), Verdict::Regressed { time: true, mem: false });
        assert_eq!(time(200_000), Verdict::Regressed { time: true, mem: false });
        assert_eq!(time(96_000), Verdict::Ok);
        assert_eq!(time(95_000), Verdict::Improved);

        let mem = |now: u64| verdict(&base, &hod(100_000, now), 500_000, 500_000);
        assert_eq!(mem(51_000), Verdict::Ok);
        assert_eq!(mem(51_001), Verdict::Regressed { time: false, mem: true });
        assert_eq!(mem(48_000), Verdict::Improved);
        assert_eq!(
            verdict(&base, &hod(120_000, 60_000), 500_000, 500_000),
            Verdict::Regressed { time: true, mem: true }
        );
        // An improvement in one does not excuse a regression in the other.
        assert_eq!(
            verdict(&base, &hod(50_000, 60_000), 500_000, 500_000),
            Verdict::Regressed { time: false, mem: true }
        );
        assert!(Verdict::Regressed { time: true, mem: false }.fails() && Verdict::Lost.fails());
        for passing in [Verdict::Ok, Verdict::Improved, Verdict::Noisy, Verdict::New(String::new())]
        {
            assert!(!passing.fails(), "{passing:?}");
        }
    }

    // "The gate": differences inside the slack are not judged, however
    // large they are in percent.
    #[test]
    fn small_differences_are_not_judged() {
        let base = hod(1_300, 1_900);
        // +70% in time, but 0.9 ms; +26% in memory, but 500 KiB.
        assert_eq!(verdict(&base, &hod(2_200, 2_400), 500_000, 500_000), Verdict::Ok);
        assert_eq!(
            verdict(&base, &hod(2_301, 1_900), 500_000, 500_000),
            Verdict::Regressed { time: true, mem: false }
        );
        assert_eq!(
            verdict(&base, &hod(1_300, 2_413), 500_000, 500_000),
            Verdict::Regressed { time: false, mem: true }
        );
        assert_eq!(verdict(&hod(2_200, 2_400), &base, 500_000, 500_000), Verdict::Ok);
    }

    // "The gate", "Drift": when llc itself is slower than in the baseline
    // the machine is, and hod is allowed as much; never less; and past 25%
    // nothing is judged.
    #[test]
    fn the_reference_tool_is_the_control() {
        let base = hod(100_000, 50_000);
        // llc is 10% slower than in the baseline: hod may be 1.10 * 1.05.
        assert_eq!(verdict(&base, &hod(115_000, 50_000), 500_000, 550_000), Verdict::Ok);
        assert_eq!(
            verdict(&base, &hod(115_600, 50_000), 500_000, 550_000),
            Verdict::Regressed { time: true, mem: false }
        );
        // llc faster than in the baseline does not tighten the threshold.
        assert_eq!(verdict(&base, &hod(105_000, 50_000), 500_000, 400_000), Verdict::Ok);
        // Memory does not depend on the load.
        assert_eq!(
            verdict(&base, &hod(100_000, 51_001), 500_000, 550_000),
            Verdict::Regressed { time: false, mem: true }
        );
        // Too busy to judge.
        assert_eq!(verdict(&base, &hod(100_000, 50_000), 500_000, 625_000), Verdict::Ok);
        assert_eq!(verdict(&base, &hod(100_000, 50_000), 500_000, 625_100), Verdict::Noisy);
        assert_eq!(verdict(&base, &hod(900_000, 50_000), 500_000, 700_000), Verdict::Noisy);
        assert_eq!(verdict(&base, &hod(50_000, 50_000), 500_000, 700_000), Verdict::Noisy);
        // ... except memory, which a busy machine does not change.
        assert_eq!(
            verdict(&base, &hod(900_000, 90_000), 500_000, 700_000),
            Verdict::Regressed { time: false, mem: true }
        );
        assert!(
            Verdict::Noisy.remeasure() && Verdict::Regressed { time: true, mem: false }.remeasure()
        );
        assert!(!Verdict::Ok.remeasure() && !Verdict::Lost.remeasure());

        let judged = judge(
            Some(&base),
            Some(&llc(500_000)),
            &hod(110_000, 50_000),
            Some(&llc(550_000)),
            Ok(()),
            &Thresholds::default(),
        );
        assert_eq!(
            (judged.drift, judged.time, judged.mem),
            (Some(11_000), Some((100_000, 110_000)), Some((50_000, 50_000)))
        );
        // Without a control on either side there is no allowance.
        let alone = judge(
            Some(&base),
            None,
            &hod(110_000, 50_000),
            Some(&llc(550_000)),
            Ok(()),
            &Thresholds::default(),
        );
        assert_eq!(
            (alone.drift, alone.verdict),
            (None, Verdict::Regressed { time: true, mem: false })
        );
    }

    // "The gate", "Spread": a time that says "slower" does not fail the
    // gate when one of the four measurements behind it is unsteady.
    #[test]
    fn an_unsteady_measurement_is_not_a_regression() {
        let unsteady = |mut record: Record| {
            // The middle run is 5.01% above the fastest.
            record.wall_us_median = record.wall_us_min.map(|us| us + us * 501 / 10_000);
            record
        };
        let (base, now) = (hod(100_000, 50_000), hod(130_000, 50_000));
        let (base_llc, now_llc) = (llc(500_000), llc(500_000));
        let t = Thresholds::default();
        let of = |base: &Record, base_llc: &Record, now: &Record, now_llc: &Record| {
            judge(Some(base), Some(base_llc), now, Some(now_llc), Ok(()), &t).verdict
        };
        let regressed = Verdict::Regressed { time: true, mem: false };
        // At the limit (the helper's records are at 5.00%) the time stands.
        assert_eq!(of(&base, &base_llc, &now, &now_llc), regressed);
        assert_eq!(of(&unsteady(base.clone()), &base_llc, &now, &now_llc), Verdict::Noisy);
        assert_eq!(of(&base, &unsteady(base_llc.clone()), &now, &now_llc), Verdict::Noisy);
        assert_eq!(of(&base, &base_llc, &unsteady(now.clone()), &now_llc), Verdict::Noisy);
        assert_eq!(of(&base, &base_llc, &now, &unsteady(now_llc.clone())), Verdict::Noisy);
        // Unsteady runs that do not say "slower" pass: noise only adds time.
        let same = unsteady(hod(101_000, 50_000));
        assert_eq!(of(&base, &base_llc, &same, &unsteady(now_llc.clone())), Verdict::Ok);
        let faster = unsteady(hod(80_000, 50_000));
        assert_eq!(of(&base, &base_llc, &faster, &now_llc), Verdict::Improved);
        // Memory is judged all the same.
        let larger = unsteady(hod(130_000, 60_000));
        assert_eq!(
            of(&base, &base_llc, &larger, &now_llc),
            Verdict::Regressed { time: false, mem: true }
        );

        // A baseline says how many of its inputs are like that.
        let named = |name: &str, mut record: Record| {
            record.input = name.to_owned();
            record
        };
        let records = vec![
            named("a", llc(500_000)),
            named("a", hod(100_000, 50_000)),
            named("b", unsteady(llc(500_000))),
            named("b", unsteady(hod(100_000, 50_000))),
            named("c", llc(500_000)),
            named("c", unsteady(record(Tool::Hod, Status::Declined, 1_300, 1_900))),
            named("d", llc(500_000)),
            named("d", unsteady(record(Tool::HodFallback, Status::Fallback, 510_000, 60_000))),
        ];
        let results = Results { records, ..Results::default() };
        assert_eq!(super::unsteady(&results, &t), 2);
    }

    // "The gate": on another machine, or against another llc, the check
    // is advisory and compares the ratios to llc.
    #[test]
    fn advisory_on_another_machine() {
        let mut base = hod(100_000, 50_000);
        (base.wall_ratio, base.rss_ratio) = (Ratio::of(1, 5), Ratio::of(1, 2));
        let mut now = hod(900_000, 90_000);
        (now.wall_ratio, now.rss_ratio) = (Ratio::of(1, 4), Ratio::of(3, 5));
        let judged = judge(
            Some(&base),
            None,
            &now,
            None,
            Err("another machine (x86-64-other-8c)"),
            &Thresholds::default(),
        );
        assert_eq!(
            judged.verdict,
            Verdict::Advisory("another machine (x86-64-other-8c)".to_owned())
        );
        assert!(judged.ratios && !judged.verdict.fails() && !judged.verdict.remeasure());
        assert_eq!((judged.time, judged.mem), (Some((2000, 2500)), Some((5000, 6000))));

        let header = |machine: &str, llc: &str| {
            let mut results = Results::default();
            results.set("machine", machine);
            results.set("llc_version", llc);
            results
        };
        let here = header("aarch64-a-24c", "LLVM version 22.1.8");
        assert_eq!(comparable(&here, &here.clone()), Ok(()));
        assert_eq!(
            comparable(&header("x86-64-b-8c", "LLVM version 22.1.8"), &here),
            Err("another machine (x86-64-b-8c)".to_owned())
        );
        assert_eq!(
            comparable(&header("aarch64-a-24c", "LLVM version 22.1.7"), &here),
            Err("another llc (LLVM version 22.1.7)".to_owned())
        );
        assert_eq!(comparable(&Results::default(), &here), Err("another machine (?)".to_owned()));
    }

    // "The gate": what is not a comparison of two times.
    #[test]
    fn inputs_without_a_comparison() {
        let t = Thresholds::default();
        let base = hod(100_000, 50_000);
        let declined = record(Tool::Hod, Status::Declined, 1_300, 1_900);
        let of =
            |base: Option<&Record>, now: &Record| judge(base, None, now, None, Ok(()), &t).verdict;

        assert_eq!(
            of(None, &base),
            Verdict::New("the baseline does not have this input".to_owned())
        );
        let other = Record { ir_hash: Some("fnv1a64:0000000000000002".to_owned()), ..base.clone() };
        assert_eq!(
            of(Some(&base), &other),
            Verdict::Advisory("the input changed since the baseline".to_owned())
        );
        // hod built it in the baseline and declines it now: a failure.
        assert_eq!(of(Some(&base), &declined), Verdict::Lost);
        // The other way round is progress the baseline does not know.
        assert!(
            matches!(of(Some(&declined), &base), Verdict::New(why) if why.starts_with("hod builds this input now"))
        );
        // Two declines are not judged: a decline is not a build time.
        let slow_decline = record(Tool::Hod, Status::Declined, 60_000, 90_000);
        for now in [&declined, &slow_decline] {
            assert_eq!(
                of(Some(&declined), now),
                Verdict::Unjudged("declined, as in the baseline".to_owned())
            );
        }
        // ... but what declining costs is shown.
        let shown = judge(Some(&declined), None, &slow_decline, None, Ok(()), &t);
        assert_eq!((shown.time, shown.mem), (Some((1_300, 60_000)), Some((1_900, 90_000))));
        let elsewhere =
            judge(Some(&declined), None, &slow_decline, None, Err("another machine"), &t);
        assert_eq!((elsewhere.time, elsewhere.mem, elsewhere.ratios), (None, None, false));
        // What hod hands to its fallback is a build, and is compared.
        let fallback = |wall: u64| record(Tool::HodFallback, Status::Fallback, wall, 60_000);
        assert_eq!(of(Some(&fallback(100_000)), &fallback(104_000)), Verdict::Ok);
        assert_eq!(
            of(Some(&fallback(100_000)), &fallback(120_000)),
            Verdict::Regressed { time: true, mem: false }
        );
        // An error in the run is the run's failure, not the gate's.
        let error = Record::blank(Status::Error, "in", Class::Typical, false, Tool::Hod);
        assert!(matches!(of(Some(&base), &error), Verdict::Unjudged(_)));
        assert!(matches!(of(Some(&error), &base), Verdict::New(_)));
    }

    // "The gate": every hod record of the run is judged, and the table
    // names each verdict.
    #[test]
    fn judging_a_run_and_its_text() {
        let named = |name: &str, mut record: Record| {
            record.input = name.to_owned();
            record
        };
        let mut baseline = Results::default();
        baseline.set("machine", "m");
        baseline.set("llc_version", "v");
        baseline.set("date", "2026-11-11T10:00:00Z");
        let mut run = baseline.clone();
        for (name, base, now) in
            [("a", 100_000, 100_500), ("b", 100_000, 130_000), ("c", 100_000, 80_000)]
        {
            baseline.records.push(named(name, llc(500_000)));
            baseline.records.push(named(name, hod(base, 50_000)));
            run.records.push(named(name, llc(500_000)));
            run.records.push(named(name, hod(now, 50_000)));
            run.records
                .push(named(name, record(Tool::HodFallback, Status::Fallback, 510_000, 60_000)));
        }
        run.records.push(named("d", record(Tool::Hod, Status::Declined, 1_000, 2_000)));
        let thresholds = Thresholds::default();
        let mut judged = judge_all(&baseline, &run, Tool::Hod, &thresholds);
        let verdicts: Vec<String> =
            judged.iter().map(|j| format!("{} {}", j.input, j.verdict.label())).collect();
        assert_eq!(
            verdicts,
            [
                "a ok",
                "b REGRESSED (time)",
                "c improved",
                "d new: the baseline does not have this input"
            ]
        );
        judged[1].remeasured = 2;
        let check = Check {
            baseline: PathBuf::from("bench/baselines/m.tsv"),
            recorded: "2026-11-11T10:00:00Z".to_owned(),
            advisory: None,
            judged,
            thresholds,
        };
        assert_eq!((check.failures(), check.noisy()), (1, 0));
        assert_eq!(
            check.text(),
            "\
bench: check against bench/baselines/m.tsv (recorded 2026-11-11T10:00:00Z)
input  base ms  now ms    time  llc drift  base MiB  now MiB  memory  verdict
a       100.00  100.50   +0.5%      +0.0%      48.8     48.8   +0.0%  ok
b       100.00  130.00  +30.0%      +0.0%      48.8     48.8   +0.0%  REGRESSED (time); measured again 2x
c       100.00   80.00  -20.0%      +0.0%      48.8     48.8   +0.0%  improved
d            -       -       -          -         -        -       -  new: the baseline does not have this input; declined
bench: check: 4 inputs: 1 ok, 1 improved, 1 new, 0 advisory, 1 REGRESSED, 0 LOST, 0 NOISY, 0 not judged; \
1 measured again; thresholds: time +5.0%, memory +2.0%
bench: check: an improvement is a reason to update the baseline (--update-baseline --reason)
"
        );

        // On another machine every comparison is advisory, in ratios.
        run.set("machine", "other");
        let advisory = judge_all(&baseline, &run, Tool::Hod, &thresholds);
        assert!(
            advisory[..3]
                .iter()
                .all(|j| j.verdict == Verdict::Advisory("another machine (m)".to_owned()))
        );
    }

    #[test]
    fn number_formats() {
        assert_eq!(
            (millis(0), millis(1_234), millis(650_009)),
            ("0.00".into(), "1.23".into(), "650.00".into())
        );
        assert_eq!((mib(0), mib(1_536), mib(100_324)), ("0.0".into(), "1.5".into(), "97.9".into()));
        assert_eq!(
            (times(10_000), times(2_512), times(123_456)),
            ("1.00x".into(), "0.25x".into(), "12.34x".into())
        );
        assert_eq!(percent(Some(512)), "+5.1%");
        assert_eq!(percent(Some(-2_000)), "-20.0%");
        assert_eq!(percent(Some(0)), "+0.0%");
        assert_eq!(percent(None), "-");
        assert_eq!(
            (delta_bp(200, 210), delta_bp(200, 100), delta_bp(0, 5)),
            (Some(500), Some(-5000), None)
        );
    }

    // "Baselines": an update replaces the baseline and appends its reason
    // to the log beside it; without a reason, or with an error in the
    // run, nothing is written.
    #[test]
    fn baseline_update_with_reason() {
        let scratch = hod_testutil::Scratch::new("bench-baseline");
        let path = baseline_path(&scratch.join("baselines"), "aarch64-x-24c");
        assert_eq!(path, scratch.join("baselines/aarch64-x-24c.tsv"));
        assert_eq!(log_path(&path), scratch.join("baselines/aarch64-x-24c.log"));

        let mut run = Results::default();
        run.set("date", "2026-11-11T10:00:00Z");
        run.set("hod_version", "hod version 0.0.0");
        run.set("runs", "3");
        run.records.push(llc(500_000));
        run.records.push(hod(100_000, 50_000));
        assert_eq!(
            update_baseline(&path, &run, "  ").unwrap_err(),
            "a baseline is updated with a reason (--reason \"<text>\")"
        );
        assert!(!path.exists());

        update_baseline(&path, &run, "first baseline").unwrap();
        assert_eq!(Results::read(&path).unwrap(), run);
        run.set("date", "2026-11-12T08:30:00Z");
        run.records[1] = record(Tool::Hod, Status::Declined, 1_000, 2_000);
        update_baseline(&path, &run, "hod parses the module\tbefore it declines\n").unwrap();
        assert_eq!(Results::read(&path).unwrap(), run);
        assert_eq!(
            std::fs::read_to_string(log_path(&path)).unwrap(),
            "2026-11-11T10:00:00Z\tfirst baseline\t1 inputs, hod built 1; hod version 0.0.0; 3 runs\n\
             2026-11-12T08:30:00Z\thod parses the module before it declines\t1 inputs, hod built 0; hod version 0.0.0; 3 runs\n"
        );

        let mut broken = run.clone();
        broken.records.push(Record::blank(Status::Error, "bad", Class::Small, true, Tool::Llc));
        assert_eq!(
            update_baseline(&path, &broken, "why").unwrap_err(),
            "the run has an error (bad with llc): it is not recorded as a baseline"
        );
        assert_eq!(Results::read(&path).unwrap(), run);
        assert_eq!(std::fs::read_to_string(log_path(&path)).unwrap().lines().count(), 2);
    }
}
