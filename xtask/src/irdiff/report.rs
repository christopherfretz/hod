//! The results of a differential run: one [`Record`] per test and linker,
//! the results file, the summary and the exit status. Contract:
//! `spec/harness.md` "irdiff", "Outcomes", "Results file", "Exit status".

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// What one test did with one linker.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Outcome {
    /// hod's program behaves as the oracle's and as the directives say.
    Pass,
    /// hod's side is wrong.
    Fail,
    /// hod declined the module with the reason the test expects.
    Unsupported,
    /// A failure the test's `xfail` directive expects.
    Xfail,
    /// The test has an `xfail` directive and passed.
    Xpass,
    /// The oracle's side or a tool is wrong; this says nothing about hod.
    Error,
    /// The test does not apply here, or cannot be run here.
    Skip,
}

impl Outcome {
    /// Every outcome, in report order.
    pub const ALL: [Self; 7] = [
        Self::Pass,
        Self::Fail,
        Self::Unsupported,
        Self::Xfail,
        Self::Xpass,
        Self::Error,
        Self::Skip,
    ];

    /// The name in reports and in the results file.
    pub fn name(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
            Self::Unsupported => "UNSUPPORTED",
            Self::Xfail => "XFAIL",
            Self::Xpass => "XPASS",
            Self::Error => "ERROR",
            Self::Skip => "SKIP",
        }
    }

    /// Whether the outcome makes the run fail.
    pub fn is_failure(self) -> bool {
        matches!(self, Self::Fail | Self::Xpass | Self::Error)
    }
}

/// Who produced the object on hod's side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuiltBy {
    /// hod itself.
    Hod,
    /// hod's fallback, the real `llc` (hybrid mode).
    Fallback,
    /// Nobody: hod declined the module.
    Declined,
    /// hod was not run, or failed.
    Nobody,
}

impl BuiltBy {
    /// The name in the results file.
    pub fn name(self) -> &'static str {
        match self {
            Self::Hod => "hod",
            Self::Fallback => "fallback",
            Self::Declined => "declined",
            Self::Nobody => "-",
        }
    }
}

/// Which step of a test went wrong, for the harness commands that read
/// records (`spec/harness.md` "irdiff", "Library"). The detail says the
/// same in words; this is what a caller decides on. An ERROR is about the
/// oracle's side (or the witness, or the harness); a FAIL or XFAIL about
/// hod's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Failure {
    /// The reference `llc` failed or wrote no object.
    ReferenceBuild,
    /// The witness did not confirm the reference ("The witness").
    Witness,
    /// hod failed on a module (step 2), whatever the way.
    HodBuild,
    /// hod declined, or did what the test's `unsupported` directives do
    /// not describe.
    Declined,
    /// A link failed or wrote no program.
    Link,
    /// A program ran into a limit: its time, or the output kept.
    Run,
    /// A program ended or printed otherwise than it must.
    Comparison,
    /// `llc`, the driver or the linker ran into its time limit.
    Timeout,
    /// `llc`, the driver or the linker was killed by a signal.
    Crash,
    /// The harness could not do its work, the directives do not parse,
    /// or the program was linked by another linker.
    Harness,
}

impl Failure {
    /// The kind of a tool's failure that ended as `end`: `step`, unless
    /// the tool crashed or timed out.
    pub fn of_tool(step: Self, end: super::process::End) -> Self {
        use super::process::End;
        match end {
            End::Signal(_) => Self::Crash,
            End::TimedOut => Self::Timeout,
            End::Exit(_) => step,
        }
    }
}

/// One line of the results: a test with one linker, or a test that was
/// decided before any linker ran.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub outcome: Outcome,
    /// The test's name.
    pub test: String,
    /// The target triple.
    pub target: String,
    /// The oracle's LLVM major; `None` when no LLVM was found.
    pub llvm: Option<u32>,
    /// The linker; `None` when the outcome was decided without linking.
    pub linker: Option<String>,
    pub built_by: BuiltBy,
    /// The coverage-log reason, when hod declined or fell back.
    pub reason: Option<String>,
    /// What differed, what failed, or why the test was skipped.
    pub detail: String,
    /// The test's `covers` directive.
    pub covers: Option<String>,
    /// Which step went wrong: set on every ERROR, FAIL and XFAIL, `None`
    /// otherwise. Not in the results file.
    pub failure: Option<Failure>,
}

/// A free-form field of the results file: `\\`, `\t`, `\n` and `\r` for
/// those characters, `-` for an empty value (the escaping of the coverage
/// log, `spec/cli.md`).
fn field(value: &str) -> String {
    if value.is_empty() {
        return "-".to_owned();
    }
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
    out
}

impl Record {
    /// The record as a line of the results file, without the newline.
    pub fn to_line(&self) -> String {
        [
            self.outcome.name().to_owned(),
            field(&self.test),
            field(&self.target),
            self.llvm.map_or_else(|| "-".to_owned(), |major| major.to_string()),
            field(self.linker.as_deref().unwrap_or("")),
            self.built_by.name().to_owned(),
            field(self.reason.as_deref().unwrap_or("")),
            field(&self.detail),
            field(self.covers.as_deref().unwrap_or("")),
        ]
        .join("\t")
    }
}

/// A differential run.
#[derive(Clone, Debug, Default)]
pub struct Report {
    /// One line that says what ran: the oracle, the target, the linkers,
    /// the mode.
    pub configuration: String,
    /// In order: by test name, then by linker in matrix order.
    pub records: Vec<Record>,
    /// Things the reader should know: tools that were not found and what
    /// was skipped for it.
    pub notes: Vec<String>,
    /// Tools that are missing and required (`HOD_REQUIRE_TOOLS=1`).
    pub missing_tools: Vec<String>,
    /// Where the results file was written.
    pub results_file: Option<PathBuf>,
    /// The work directory, if it was kept (`--keep`, and a test failed).
    pub kept: Option<PathBuf>,
    pub elapsed: Duration,
}

impl Report {
    /// The number of records with this outcome.
    pub fn count(&self, outcome: Outcome) -> usize {
        self.records.iter().filter(|record| record.outcome == outcome).count()
    }

    /// The number of tests: distinct names among the records.
    pub fn tests(&self) -> usize {
        self.per_test().count()
    }

    /// The first record of each test (a test's records are adjacent).
    fn per_test(&self) -> impl Iterator<Item = &Record> {
        self.records
            .iter()
            .enumerate()
            .filter(|(i, record)| *i == 0 || self.records[i - 1].test != record.test)
            .map(|(_, record)| record)
    }

    /// The process exit status of `cargo xtask irdiff` for this run:
    /// 1 if a record is FAIL or XPASS; else 3 if a record is ERROR or a
    /// required tool is missing; else 0.
    pub fn exit_status(&self) -> u8 {
        if self.count(Outcome::Fail) + self.count(Outcome::Xpass) > 0 {
            1
        } else if self.count(Outcome::Error) > 0 || !self.missing_tools.is_empty() {
            3
        } else {
            0
        }
    }

    /// The results file: one line per record, each ended by a newline.
    pub fn results_text(&self) -> String {
        self.records.iter().fold(String::new(), |mut text, record| {
            text.push_str(&record.to_line());
            text.push('\n');
            text
        })
    }

    /// Writes the results file through a temporary file beside it.
    pub fn write_results(&self, path: &Path) -> Result<(), String> {
        let failed = |e: std::io::Error| format!("cannot write '{}': {e}", path.display());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(failed)?;
        }
        let mut temporary = path.as_os_str().to_owned();
        temporary.push(format!(".tmp-{}", std::process::id()));
        std::fs::write(&temporary, self.results_text()).map_err(failed)?;
        std::fs::rename(&temporary, path).map_err(failed)
    }

    /// The human summary. Failures (FAIL, XPASS, ERROR), XFAIL and SKIP are
    /// always listed; PASS and UNSUPPORTED only when `verbose`.
    pub fn summary(&self, verbose: bool) -> String {
        let mut text = String::new();
        if !self.configuration.is_empty() {
            let _ = writeln!(text, "irdiff: {}", self.configuration);
        }
        for note in &self.notes {
            let _ = writeln!(text, "irdiff: note: {note}");
        }
        for tool in &self.missing_tools {
            let _ = writeln!(text, "irdiff: error: {tool} (HOD_REQUIRE_TOOLS=1)");
        }

        // Adjacent records of a test that differ only in the linker are
        // one line.
        let mut start = 0;
        while start < self.records.len() {
            let first = &self.records[start];
            let same = |other: &Record| {
                other.test == first.test
                    && other.outcome == first.outcome
                    && other.detail == first.detail
                    && other.reason == first.reason
            };
            let run = self.records[start..].iter().take_while(|record| same(record)).count();
            let shown = verbose || !matches!(first.outcome, Outcome::Pass | Outcome::Unsupported);
            if shown {
                let _ = write!(text, "{:<11} {}", first.outcome.name(), first.test);
                let linkers: Vec<&str> = self.records[start..start + run]
                    .iter()
                    .filter_map(|record| record.linker.as_deref())
                    .collect();
                if !linkers.is_empty() {
                    let _ = write!(text, " [{}]", linkers.join(" "));
                }
                let said = match (&first.reason, first.detail.is_empty()) {
                    (Some(reason), true) => reason.clone(),
                    (Some(reason), false) => format!("{reason}: {}", first.detail),
                    (None, _) => first.detail.clone(),
                };
                if !said.is_empty() {
                    let _ = write!(text, ": {said}");
                }
                text.push('\n');
            }
            start += run;
        }

        let counts: Vec<String> = Outcome::ALL
            .into_iter()
            .map(|outcome| format!("{} {}", self.count(outcome), outcome.name()))
            .collect();
        let _ = writeln!(
            text,
            "irdiff: {} tests, {} results: {} in {:.1}s",
            self.tests(),
            self.records.len(),
            counts.join(", "),
            self.elapsed.as_secs_f64()
        );
        text.push_str(&self.coverage_line());
        if let Some(path) = &self.results_file {
            let _ = writeln!(text, "irdiff: results: {}", path.display());
        }
        if let Some(path) = &self.kept {
            let _ = writeln!(text, "irdiff: kept: {}", path.display());
        }
        text
    }

    /// Coverage: how many of the objects hod was asked for it built
    /// itself, and why not the others, by reason.
    fn coverage_line(&self) -> String {
        let mut asked = 0;
        let mut built = 0;
        let mut reasons: Vec<(String, usize)> = Vec::new();
        for record in self.per_test() {
            match record.built_by {
                BuiltBy::Nobody => continue,
                BuiltBy::Hod => built += 1,
                BuiltBy::Fallback | BuiltBy::Declined => {
                    let verb =
                        if record.built_by == BuiltBy::Fallback { "fallback" } else { "declined" };
                    let key = format!("{verb} {}", record.reason.as_deref().unwrap_or("-"));
                    match reasons.iter_mut().find(|(seen, _)| *seen == key) {
                        Some((_, count)) => *count += 1,
                        None => reasons.push((key, 1)),
                    }
                }
            }
            asked += 1;
        }
        if asked == 0 {
            return String::new();
        }
        reasons.sort();
        let mut line = format!("irdiff: coverage: hod built {built} of {asked} objects");
        for (reason, count) in reasons {
            let _ = write!(line, "; {reason}: {count}");
        }
        line.push('\n');
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(outcome: Outcome, test: &str, linker: Option<&str>) -> Record {
        Record {
            outcome,
            test: test.to_owned(),
            target: "aarch64-unknown-linux-gnu".to_owned(),
            llvm: Some(22),
            linker: linker.map(str::to_owned),
            built_by: BuiltBy::Hod,
            reason: None,
            detail: String::new(),
            covers: None,
            failure: None,
        }
    }

    fn report(records: Vec<Record>) -> Report {
        Report { records, ..Report::default() }
    }

    // spec/harness.md "irdiff", "Exit status".
    #[test]
    fn exit_status_mapping() {
        use Outcome::{Error, Fail, Pass, Skip, Unsupported, Xfail, Xpass};
        let status = |outcomes: &[Outcome]| {
            report(outcomes.iter().map(|o| record(*o, "t", Some("bfd"))).collect()).exit_status()
        };
        assert_eq!(status(&[]), 0);
        assert_eq!(status(&[Pass, Unsupported, Xfail, Skip]), 0);
        assert_eq!(status(&[Pass, Fail]), 1);
        assert_eq!(status(&[Xpass]), 1);
        assert_eq!(status(&[Error, Pass]), 3);
        assert_eq!(status(&[Error, Fail]), 1);
        assert_eq!(status(&[Error, Xpass, Skip]), 1);

        let mut missing = report(vec![record(Pass, "t", Some("bfd"))]);
        missing.missing_tools.push("linker 'mold' not found".to_owned());
        assert_eq!(missing.exit_status(), 3);
        missing.records.push(record(Fail, "u", Some("bfd")));
        assert_eq!(missing.exit_status(), 1);

        for outcome in Outcome::ALL {
            assert_eq!(outcome.is_failure(), matches!(outcome, Fail | Xpass | Error));
        }
    }

    // "Results file": nine fields, escaped, none empty.
    #[test]
    fn results_lines() {
        let plain = record(Outcome::Pass, "tests/ir/a.ll", Some("mold"));
        assert_eq!(
            plain.to_line(),
            "PASS\ttests/ir/a.ll\taarch64-unknown-linux-gnu\t22\tmold\thod\t-\t-\t-"
        );
        let full = Record {
            outcome: Outcome::Unsupported,
            test: "dir with\ttab/b.ll".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            llvm: None,
            linker: None,
            built_by: BuiltBy::Declined,
            reason: Some("codegen-not-implemented".to_owned()),
            detail: "two\nlines\r and a \\ backslash".to_owned(),
            covers: Some("spec/ir.md §5.4".to_owned()),
            failure: None,
        };
        assert_eq!(
            full.to_line(),
            "UNSUPPORTED\tdir with\\ttab/b.ll\tx86_64-unknown-linux-gnu\t-\t-\tdeclined\t\
             codegen-not-implemented\ttwo\\nlines\\r and a \\\\ backslash\tspec/ir.md §5.4"
        );
        let both = report(vec![plain, full]);
        let text = both.results_text();
        assert!(text.ends_with('\n'));
        for line in text.lines() {
            let fields: Vec<&str> = line.split('\t').collect();
            assert_eq!(fields.len(), 9, "{line:?}");
            assert!(fields.iter().all(|f| !f.is_empty()), "{line:?}");
        }
        assert_eq!(text.lines().count(), 2);
        assert_eq!(BuiltBy::Fallback.name(), "fallback");
        assert_eq!(BuiltBy::Nobody.name(), "-");
        let names: Vec<&str> = Outcome::ALL.iter().map(|o| o.name()).collect();
        assert_eq!(names, ["PASS", "FAIL", "UNSUPPORTED", "XFAIL", "XPASS", "ERROR", "SKIP"]);
    }

    // "Results file": written whole, replacing an older one.
    #[test]
    fn results_file_is_replaced() {
        let scratch = hod_testutil::Scratch::new("irdiff-results");
        let path = scratch.join("deep/er/results.tsv");
        report(vec![record(Outcome::Pass, "a", None), record(Outcome::Pass, "b", None)])
            .write_results(&path)
            .unwrap();
        report(vec![record(Outcome::Fail, "c", Some("lld"))]).write_results(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text, "FAIL\tc\taarch64-unknown-linux-gnu\t22\tlld\thod\t-\t-\t-\n");
        let entries = std::fs::read_dir(path.parent().unwrap()).unwrap().count();
        assert_eq!(entries, 1, "no temporary file is left");
    }

    // "Summary": what is listed, how linkers are grouped, the counts and
    // the coverage line.
    #[test]
    fn summary_text() {
        let declined = |test: &str, linker: &str| Record {
            built_by: BuiltBy::Declined,
            reason: Some("codegen-not-implemented".to_owned()),
            ..record(Outcome::Unsupported, test, Some(linker))
        };
        let failed = |linker: &str, detail: &str| Record {
            detail: detail.to_owned(),
            ..record(Outcome::Fail, "t/c.ll", Some(linker))
        };
        let mut run = report(vec![
            declined("t/a.ll", "bfd"),
            declined("t/a.ll", "lld"),
            record(Outcome::Pass, "t/b.ll", Some("bfd")),
            record(Outcome::Pass, "t/b.ll", Some("lld")),
            failed("bfd", "exit status 1, llc's program gives 0"),
            failed("lld", "exit status 1, llc's program gives 0"),
            failed("mold", "link failed"),
            Record {
                built_by: BuiltBy::Nobody,
                detail: "requires target x86_64".to_owned(),
                ..record(Outcome::Skip, "t/d.ll", None)
            },
            Record {
                built_by: BuiltBy::Fallback,
                reason: Some("bitcode-input".to_owned()),
                ..record(Outcome::Pass, "t/e.ll", Some("bfd"))
            },
        ]);
        run.configuration = "LLVM 22; strict".to_owned();
        run.notes.push("linker 'wild' not found".to_owned());
        run.elapsed = Duration::from_millis(1234);
        run.results_file = Some(PathBuf::from("/b/results.tsv"));
        run.kept = Some(PathBuf::from("/b/run-1"));
        assert_eq!(run.tests(), 5);
        let quiet = "\
irdiff: LLVM 22; strict
irdiff: note: linker 'wild' not found
FAIL        t/c.ll [bfd lld]: exit status 1, llc's program gives 0
FAIL        t/c.ll [mold]: link failed
SKIP        t/d.ll: requires target x86_64
irdiff: 5 tests, 9 results: 3 PASS, 3 FAIL, 2 UNSUPPORTED, 0 XFAIL, 0 XPASS, 0 ERROR, 1 SKIP in 1.2s
irdiff: coverage: hod built 2 of 4 objects; declined codegen-not-implemented: 1; fallback bitcode-input: 1
irdiff: results: /b/results.tsv
irdiff: kept: /b/run-1
";
        assert_eq!(run.summary(false), quiet);
        let verbose = run.summary(true);
        assert!(verbose.contains("UNSUPPORTED t/a.ll [bfd lld]: codegen-not-implemented\n"));
        assert!(verbose.contains("PASS        t/b.ll [bfd lld]\n"));
        assert!(verbose.contains("PASS        t/e.ll [bfd]: bitcode-input\n"));

        let mut missing = report(Vec::new());
        missing.missing_tools.push("linker 'mold' not found".to_owned());
        let text = missing.summary(false);
        assert!(text.contains("irdiff: error: linker 'mold' not found (HOD_REQUIRE_TOOLS=1)\n"));
        assert!(text.contains("irdiff: 0 tests, 0 results: 0 PASS"));
        assert!(!text.contains("coverage"));
    }
}
