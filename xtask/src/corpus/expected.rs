//! What a strict `corpus verify` expects of the corpus besides a PASS:
//! the refusals hod is known to give, and the entries whose program is
//! undefined on one target. Contract: `spec/harness.md` "corpus",
//! "Expected results".
//!
//! The entries cannot carry this themselves: they are written by
//! `capture`, checked byte for byte against the manifest, and the same
//! entry is captured for one target only. So the list is a table of the
//! repository, `docs/status/corpus-expected.tsv` (beside the table of
//! explained differences `corpus compare` reads), with one row per
//! target and test:
//!
//! ```text
//! <target> TAB refused   TAB <source> <test> TAB <refusal name> TAB <reason>
//! <target> TAB undefined TAB <source> <test> TAB -              TAB <evidence>
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::irdiff::directives::Arch;
use crate::irdiff::report::{BuiltBy, Failure, Outcome, Report};

/// The file name, in `docs/status/`.
pub const FILE: &str = "corpus-expected.tsv";

/// The list of this workspace.
pub fn default_path() -> PathBuf {
    crate::irdiff::workspace_root().join("docs").join("status").join(FILE)
}

/// The file name of the coroutine corpus's list (WP-353), in
/// `docs/status/`: its own, because a row naming a test absent from the
/// corpus verified is a list problem, and the coroutine corpus holds
/// none of the main corpus's tests' entries nor they its.
pub const CORO_FILE: &str = "corpus-coro-expected.tsv";

/// The coroutine corpus's list of this workspace.
pub fn coro_path() -> PathBuf {
    crate::irdiff::workspace_root().join("docs").join("status").join(CORO_FILE)
}

/// What a row expects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    /// hod declines the module with this refusal name, in strict mode.
    Refused(String),
    /// The program is undefined on the row's target: built by both
    /// compilers, its run not compared.
    Undefined,
}

/// One row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub target: Arch,
    /// `<source> <test>`, the test of the manifest (every LLVM major).
    pub test: String,
    pub kind: Kind,
    /// The reason (a refusal) or the evidence (undefined behaviour).
    pub note: String,
    /// The row's line in the file, for messages.
    pub line: usize,
}

/// The whole list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Expected {
    pub rows: Vec<Row>,
}

impl Expected {
    /// Reads the list from `text`.
    ///
    /// # Errors
    /// Every malformed line, at once: a line without five fields, an
    /// unknown target or kind, a test that is not `<source> <test>`, a
    /// wildcard, a refusal without a name, an undefined row with one, an
    /// empty reason, a second row for the same target and test.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut rows = Vec::new();
        let mut errors = Vec::new();
        let mut seen: BTreeMap<(Arch, String), usize> = BTreeMap::new();
        for (index, line) in text.lines().enumerate() {
            let number = index + 1;
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            match parse_row(line, number) {
                Ok(row) => {
                    if let Some(first) = seen.insert((row.target, row.test.clone()), number) {
                        errors.push(format!(
                            "line {number}: a second row for {} '{}' (the first is line {first})",
                            row.target.name(),
                            row.test
                        ));
                    } else {
                        rows.push(row);
                    }
                }
                Err(message) => errors.push(format!("line {number}: {message}")),
            }
        }
        if errors.is_empty() { Ok(Self { rows }) } else { Err(errors.join("\n")) }
    }

    /// Reads the list in `path`.
    ///
    /// # Errors
    /// The file cannot be read, or [`Self::parse`] fails.
    pub fn read(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read '{}': {e}", path.display()))?;
        Self::parse(&text).map_err(|e| format!("{}:\n{e}", path.display()))
    }

    /// The rows of `target`.
    pub fn for_target(&self, target: Arch) -> Vec<Row> {
        self.rows.iter().filter(|row| row.target == target).cloned().collect()
    }
}

fn parse_row(line: &str, number: usize) -> Result<Row, String> {
    let fields: Vec<&str> = line.split('\t').collect();
    let [target, kind, test, name, note] = fields[..] else {
        return Err(format!("{} fields, not 5 separated by tabs", fields.len()));
    };
    let target = Arch::from_name(target).ok_or_else(|| format!("unknown target '{target}'"))?;
    if target == Arch::I686 {
        return Err(super::NO_I686_CORPUS.to_owned());
    }
    if test.contains(['*', '?', '[']) || test.ends_with('/') {
        return Err(format!("'{test}' is not one test (no wildcard or directory rows)"));
    }
    match test.split_once(' ') {
        Some((source, name)) if !source.is_empty() && !name.trim().is_empty() => {}
        _ => return Err(format!("'{test}' is not '<source> <test>'")),
    }
    if note.trim().is_empty() || note == "-" {
        return Err("no reason or evidence".to_owned());
    }
    let kind = match (kind, name) {
        ("refused", "-" | "") => return Err("a refused row needs the refusal's name".to_owned()),
        ("refused", name) if name.contains(['*', '?']) => {
            return Err(format!("'{name}' is not one refusal name"));
        }
        ("refused", name) => Kind::Refused(name.to_owned()),
        ("undefined", "-") => Kind::Undefined,
        ("undefined", other) => {
            return Err(format!("an undefined row has no refusal name ('{other}'): write '-'"));
        }
        (other, _) => return Err(format!("unknown kind '{other}' (refused or undefined)")),
    };
    Ok(Row { target, test: test.to_owned(), kind, note: note.to_owned(), line: number })
}

/// What the list did to a verification's runs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Applied {
    /// Results counted as an expected refusal, by row: `(test, refusal,
    /// results)`.
    pub refused: Vec<(String, String, usize)>,
    /// Results of undefined entries that were built and not compared, by
    /// row: `(test, results)`.
    pub undefined: Vec<(String, usize)>,
    /// What makes the verification fail: a refusal under another name,
    /// a listed test hod builds (stale), a row for a test the corpus does
    /// not have.
    pub problems: Vec<String>,
}

impl Applied {
    /// The lines the summary prints.
    pub fn summary(&self, target: Arch) -> String {
        let mut text = String::new();
        for (test, name, results) in &self.refused {
            let _ = writeln!(text, "EXPECTED    {test}: refused {name} ({results} results)");
        }
        for (test, results) in &self.undefined {
            let _ = writeln!(
                text,
                "UNDEFINED   {test}: built, not compared on {} ({results} results)",
                target.name()
            );
        }
        for problem in &self.problems {
            let _ = writeln!(text, "LIST        {problem}");
        }
        let refused: usize = self.refused.iter().map(|(_, _, n)| n).sum();
        let undefined: usize = self.undefined.iter().map(|(_, n)| n).sum();
        let _ = writeln!(
            text,
            "corpus: expected on {}: {} refused tests ({refused} results), {} undefined \
             ({undefined} results), {} list problems",
            target.name(),
            self.refused.len(),
            self.undefined.len(),
            self.problems.len()
        );
        text
    }
}

/// Whether a result of an undefined entry is about its run alone: the
/// build of both sides and the links went through.
fn only_the_run(record: &crate::irdiff::report::Record) -> bool {
    match record.outcome {
        Outcome::Pass => true,
        Outcome::Fail => {
            matches!(record.failure, Some(Failure::Comparison | Failure::Run))
                && matches!(record.built_by, BuiltBy::Hod | BuiltBy::Fallback)
        }
        _ => false,
    }
}

/// Applies `rows` (one target's) to `runs`, the verification's `irdiff`
/// reports, whose records are named by `test_of` (`<source> <test>` of a
/// record's test, `None` for a module that is no entry). `corpus` is
/// every test of the manifest; `strict` whether the runs were strict (the
/// refusals apply only then: hybrid mode falls back).
///
/// A listed refusal's results with that name become UNSUPPORTED; a FAIL
/// of an undefined entry whose build went through, and its PASSes,
/// become SKIP with the evidence. Everything else stays as it was.
pub fn apply(
    rows: &[Row],
    runs: &mut [Report],
    test_of: &dyn Fn(&str) -> Option<String>,
    corpus: &BTreeSet<String>,
    strict: bool,
) -> Applied {
    let mut applied = Applied::default();
    for row in rows {
        if !corpus.contains(&row.test) {
            applied.problems.push(format!(
                "{} '{}' (line {}): no such test in the corpus",
                row.target.name(),
                row.test,
                row.line
            ));
            continue;
        }
        let mut counted = 0;
        let mut built = false;
        let mut other_names = BTreeSet::new();
        for report in runs.iter_mut() {
            for record in &mut report.records {
                if test_of(&record.test).as_deref() != Some(row.test.as_str()) {
                    continue;
                }
                match &row.kind {
                    Kind::Refused(name) if strict => {
                        if record.built_by == BuiltBy::Hod {
                            built = true;
                        } else if record.outcome == Outcome::Fail
                            && record.failure == Some(Failure::Declined)
                        {
                            if record.reason.as_deref() == Some(name.as_str()) {
                                record.outcome = Outcome::Unsupported;
                                record.failure = None;
                                record.detail = format!("expected ({})", row.note);
                                counted += 1;
                            } else {
                                other_names
                                    .insert(record.reason.clone().unwrap_or_else(|| "-".into()));
                            }
                        }
                    }
                    Kind::Refused(_) => {}
                    Kind::Undefined => {
                        if only_the_run(record) {
                            record.outcome = Outcome::Skip;
                            record.failure = None;
                            record.detail = format!(
                                "undefined on {}, built, not compared ({})",
                                row.target.name(),
                                row.note
                            );
                            counted += 1;
                        }
                    }
                }
            }
        }
        match &row.kind {
            Kind::Refused(name) if strict => {
                if built {
                    applied.problems.push(format!(
                        "{} '{}' (line {}): hod builds it now: the row is stale, remove it",
                        row.target.name(),
                        row.test,
                        row.line
                    ));
                }
                if !other_names.is_empty() {
                    let names: Vec<String> = other_names.into_iter().collect();
                    applied.problems.push(format!(
                        "{} '{}' (line {}): refused with {}, the list says {name}",
                        row.target.name(),
                        row.test,
                        row.line,
                        names.join(", ")
                    ));
                }
                if counted > 0 {
                    applied.refused.push((row.test.clone(), name.clone(), counted));
                }
            }
            Kind::Refused(_) => {}
            Kind::Undefined => {
                if counted > 0 {
                    applied.undefined.push((row.test.clone(), counted));
                }
            }
        }
    }
    applied
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::irdiff::report::Record;

    const TEST: &str = "kiln tests/a/cc-run.cpp";

    fn record(test: &str, outcome: Outcome, built_by: BuiltBy, reason: Option<&str>) -> Record {
        let failure = match (outcome, built_by) {
            (Outcome::Fail, BuiltBy::Declined) => Some(Failure::Declined),
            (Outcome::Fail, BuiltBy::Nobody) => Some(Failure::HodBuild),
            (Outcome::Fail, _) => Some(Failure::Comparison),
            _ => None,
        };
        Record {
            outcome,
            test: format!("/c/{test}.ll"),
            target: "x86_64-unknown-linux-gnu".into(),
            llvm: Some(22),
            linker: Some("bfd".into()),
            built_by,
            reason: reason.map(str::to_owned),
            detail: "d".into(),
            covers: None,
            failure,
        }
    }

    fn test_of(name: &str) -> Option<String> {
        let entry = name.strip_prefix("/c/")?.strip_suffix(".ll")?;
        Some(entry.to_owned())
    }

    fn run(rows: &str, records: Vec<Record>, strict: bool) -> (Applied, Report) {
        let list = Expected::parse(rows).unwrap();
        let corpus: BTreeSet<String> =
            [TEST, "gcc-torture builtins/strncmp.c", "gcc-torture other.c"]
                .into_iter()
                .map(str::to_owned)
                .collect();
        let mut runs = vec![Report { records, ..Report::default() }];
        let applied = apply(&list.for_target(Arch::X86_64), &mut runs, &test_of, &corpus, strict);
        (applied, runs.remove(0))
    }

    const REFUSED: &str = "x86_64\trefused\tkiln tests/a/cc-run.cpp\tcc:x86_regcallcc\tby design\n";
    const UNDEFINED: &str = "x86_64\tundefined\tgcc-torture builtins/strncmp.c\t-\tuninit read\n";

    // spec/harness.md "Expected results": a listed refusal under its name
    // is counted, not failed.
    #[test]
    fn a_listed_refusal_is_expected() {
        let records = vec![
            record(TEST, Outcome::Fail, BuiltBy::Declined, Some("cc:x86_regcallcc")),
            record(TEST, Outcome::Fail, BuiltBy::Declined, Some("cc:x86_regcallcc")),
        ];
        let (applied, report) = run(REFUSED, records, true);
        assert!(applied.problems.is_empty(), "{:?}", applied.problems);
        assert_eq!(applied.refused, [(TEST.to_owned(), "cc:x86_regcallcc".to_owned(), 2)]);
        assert_eq!(report.count(Outcome::Unsupported), 2);
        assert_eq!(report.exit_status(), 0);
        assert!(applied.summary(Arch::X86_64).contains("EXPECTED    kiln tests/a/cc-run.cpp"));
    }

    // A refusal of a test the list does not name still fails.
    #[test]
    fn an_unlisted_refusal_fails() {
        let other = "gcc-torture other.c";
        let records = vec![record(other, Outcome::Fail, BuiltBy::Declined, Some("x64:inline-asm"))];
        let (applied, report) = run(REFUSED, records, true);
        assert_eq!(report.exit_status(), 1);
        assert!(applied.refused.is_empty());
    }

    // The same test refused under another name fails, and says both.
    #[test]
    fn a_refusal_under_another_name_fails() {
        let records = vec![record(TEST, Outcome::Fail, BuiltBy::Declined, Some("cc:win64cc"))];
        let (applied, report) = run(REFUSED, records, true);
        assert_eq!(report.exit_status(), 1, "the record stays a FAIL");
        assert_eq!(applied.problems.len(), 1);
        assert!(applied.problems[0].contains("refused with cc:win64cc, the list says"));
    }

    // A listed test hod builds makes the row stale: a problem, whatever
    // the result of the build.
    #[test]
    fn a_listed_test_hod_builds_is_stale() {
        let records = vec![record(TEST, Outcome::Pass, BuiltBy::Hod, None)];
        let (applied, report) = run(REFUSED, records, true);
        assert_eq!(report.exit_status(), 0);
        assert_eq!(applied.problems.len(), 1);
        assert!(applied.problems[0].contains("stale"));
    }

    // A row whose test is not in the corpus (a directory, a typo, a test
    // gone) is a problem too.
    #[test]
    fn a_row_for_no_test_is_a_problem() {
        let row = "x86_64\trefused\tkiln tests/a\tcc:x\tr\n";
        let (applied, _) = run(row, Vec::new(), true);
        assert!(applied.problems[0].contains("no such test in the corpus"));
    }

    // In hybrid mode the refusals do not apply (hod falls back).
    #[test]
    fn refusals_apply_in_strict_mode_only() {
        let records =
            vec![record(TEST, Outcome::Pass, BuiltBy::Fallback, Some("cc:x86_regcallcc"))];
        let (applied, report) = run(REFUSED, records, false);
        assert_eq!(applied, Applied::default());
        assert_eq!(report.records[0].outcome, Outcome::Pass);
    }

    // An undefined entry hod built: its results are not compared, PASS
    // or FAIL, and are counted.
    #[test]
    fn an_undefined_entry_is_built_and_not_compared() {
        let strncmp = "gcc-torture builtins/strncmp.c";
        let mut timeout = record(strncmp, Outcome::Fail, BuiltBy::Hod, None);
        timeout.failure = Some(Failure::Run);
        let records = vec![
            record(strncmp, Outcome::Fail, BuiltBy::Hod, None),
            record(strncmp, Outcome::Pass, BuiltBy::Hod, None),
            timeout,
        ];
        for strict in [true, false] {
            let (applied, report) = run(UNDEFINED, records.clone(), strict);
            assert!(applied.problems.is_empty());
            assert_eq!(applied.undefined, [(strncmp.to_owned(), 3)]);
            assert_eq!(report.count(Outcome::Skip), 3);
            assert_eq!(report.exit_status(), 0);
            assert!(report.records[0].detail.contains("not compared (uninit read)"));
        }
    }

    // The mark does not excuse a build: a refusal, a failure of hod, a
    // failed link, or a failure of the reference still fail or err.
    #[test]
    fn an_undefined_entry_that_does_not_build_still_fails() {
        let strncmp = "gcc-torture builtins/strncmp.c";
        let mut link = record(strncmp, Outcome::Fail, BuiltBy::Hod, None);
        link.failure = Some(Failure::Link);
        let mut reference = record(strncmp, Outcome::Error, BuiltBy::Nobody, None);
        reference.failure = Some(Failure::ReferenceBuild);
        let records = vec![
            record(strncmp, Outcome::Fail, BuiltBy::Declined, Some("x64:inline-asm")),
            record(strncmp, Outcome::Fail, BuiltBy::Nobody, None),
            link,
            reference,
        ];
        let (applied, report) = run(UNDEFINED, records, true);
        assert!(applied.undefined.is_empty());
        assert_eq!(report.count(Outcome::Fail), 3);
        assert_eq!(report.count(Outcome::Error), 1);
        assert_eq!(report.exit_status(), 1);
    }

    // The format: five tab-separated fields, one test per row, no
    // wildcards, no directory rows, one row per target and test.
    #[test]
    fn the_list_format() {
        let both = format!("# a comment\n\n{REFUSED}{UNDEFINED}");
        let list = Expected::parse(&both).unwrap();
        assert_eq!(list.rows.len(), 2);
        assert_eq!(list.rows[0].kind, Kind::Refused("cc:x86_regcallcc".into()));
        assert_eq!(list.rows[1].kind, Kind::Undefined);
        assert_eq!(list.rows[1].line, 4);
        assert!(list.for_target(Arch::Aarch64).is_empty());
        let bad = |text: &str| Expected::parse(text).unwrap_err();
        assert!(bad("x86_64\trefused\tkiln tests/*.cpp\tcc:x\tr").contains("wildcard"));
        assert!(bad("x86_64\trefused\tkiln tests/a/\tcc:x\tr").contains("directory"));
        assert!(bad("x86_64\trefused\tkiln\tcc:x\tr").contains("<source> <test>"));
        assert!(bad("x86_64\trefused\tkiln a.c\t-\tr").contains("refusal's name"));
        assert!(bad("x86_64\trefused\tkiln a.c\tcc:*\tr").contains("one refusal name"));
        assert!(bad("x86_64\tundefined\tkiln a.c\tcc:x\tr").contains("write '-'"));
        assert!(bad("x86_64\tundefined\tkiln a.c\t-\t-").contains("evidence"));
        assert!(bad("riscv64\tundefined\tkiln a.c\t-\te").contains("unknown target"));
        assert!(bad("i386\tundefined\tkiln a.c\t-\te").contains("has no corpus"));
        assert!(bad("x86_64\txfail\tkiln a.c\t-\te").contains("unknown kind"));
        assert!(bad("x86_64 refused kiln a.c cc:x r").contains("not 5"));
        let twice = format!("{REFUSED}{REFUSED}");
        assert!(bad(&twice).contains("a second row"));
        // The same test on two targets is two rows.
        let two = format!("{REFUSED}{}", REFUSED.replacen("x86_64", "aarch64", 1));
        assert_eq!(Expected::parse(&two).unwrap().rows.len(), 2);
    }

    // The workspace's list reads.
    #[test]
    fn the_workspaces_list_reads() {
        let list = Expected::read(&default_path()).unwrap();
        assert!(list.rows.iter().all(|row| !row.note.is_empty()));
        // WP-353: the coroutine corpus's list reads too, and is its own.
        let coro = Expected::read(&coro_path()).unwrap();
        assert!(coro.rows.iter().all(|row| !row.note.is_empty()));
        assert_ne!(coro_path(), default_path());
    }
}
