//! The checked-in expectation file: the corpus entries that are known not
//! to pass, by outcome. Contract: `spec/harness.md` "roundtrip",
//! "Expectation file" and "Verdicts".

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use crate::corpus::manifest::{escape, unescape};

use super::Outcome;

const HEADER: &str = "\
# The corpus entries that do not survive `cargo xtask roundtrip` today, by
# outcome (spec/harness.md \"roundtrip\"). An entry not listed must pass.
# Fields: entry, outcome, detail (for the reader; not compared).
";

/// What the file says of one entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Expected {
    pub outcome: Outcome,
    pub detail: String,
}

/// The file: entry (its path in the corpus) to what it is expected to do.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Expectations {
    pub entries: BTreeMap<String, Expected>,
}

/// How an entry's outcome compares with its expectation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// As expected.
    Expected,
    /// Not as expected, and not better: a regression, or a different
    /// failure.
    Worse,
    /// Better than expected: the file is stale.
    Better,
    /// Not compared (a tool to decide it is missing).
    Unchecked,
}

impl Verdict {
    pub fn name(self) -> &'static str {
        match self {
            Self::Expected => "expected",
            Self::Worse => "WORSE",
            Self::Better => "BETTER",
            Self::Unchecked => "unchecked",
        }
    }
}

/// The verdict for `actual` when the file expects `expected` (`None`:
/// the entry is not listed, so it must pass).
pub fn verdict(actual: Outcome, expected: Option<Outcome>) -> Verdict {
    let expected = expected.unwrap_or(Outcome::Pass);
    if actual == Outcome::Skipped {
        Verdict::Unchecked
    } else if actual == expected {
        Verdict::Expected
    } else if actual.rank() < expected.rank() {
        Verdict::Better
    } else {
        Verdict::Worse
    }
}

impl Expectations {
    /// Reads the file; a missing file expects every entry to pass.
    pub fn read(path: &Path) -> Result<Self, String> {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(format!("cannot read '{}': {e}", path.display())),
        };
        Self::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Reads the file's text.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut entries = BTreeMap::new();
        for (number, line) in text.lines().enumerate() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let fields: Vec<&str> = line.split('\t').collect();
            let [entry, outcome, detail] = fields[..] else {
                return Err(format!("line {}: not three tab-separated fields", number + 1));
            };
            let outcome = Outcome::from_name(outcome)
                .filter(|outcome| !matches!(outcome, Outcome::Pass | Outcome::Skipped))
                .ok_or_else(|| {
                    format!("line {}: '{outcome}' is not a failing outcome", number + 1)
                })?;
            let expected = Expected { outcome, detail: unescape(detail) };
            if entries.insert(unescape(entry), expected).is_some() {
                return Err(format!("line {}: a second line for '{entry}'", number + 1));
            }
        }
        Ok(Self { entries })
    }

    /// The file's text: the header, then one line per entry in order.
    pub fn text(&self) -> String {
        let mut out = HEADER.to_owned();
        for (entry, expected) in &self.entries {
            let _ = writeln!(
                out,
                "{}\t{}\t{}",
                escape(entry),
                expected.outcome.name(),
                escape(&expected.detail)
            );
        }
        out
    }

    pub fn outcome(&self, entry: &str) -> Option<Outcome> {
        self.entries.get(entry).map(|expected| expected.outcome)
    }

    /// Records what a run found for `entry`: a pass removes it, a skip
    /// leaves it as it was.
    pub fn update(&mut self, entry: &str, outcome: Outcome, detail: &str) {
        match outcome {
            Outcome::Skipped => {}
            Outcome::Pass => {
                self.entries.remove(entry);
            }
            outcome => {
                let detail = super::stages::clip(detail, 200);
                self.entries.insert(entry.to_owned(), Expected { outcome, detail });
            }
        }
    }

    /// Writes the file, by writing a new one and renaming it.
    pub fn write(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)
                .map_err(|e| format!("cannot create '{}': {e}", dir.display()))?;
        }
        let temp = path.with_extension("tsv.tmp");
        fs::write(&temp, self.text())
            .map_err(|e| format!("cannot write '{}': {e}", temp.display()))?;
        fs::rename(&temp, path).map_err(|e| format!("cannot write '{}': {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec/harness.md "roundtrip", "Verdicts".
    #[test]
    fn verdicts() {
        use Outcome::{
            BehaviourDiffers, LlcFailed, ParserUnsupported, Pass, PrinterNotExpressible,
        };
        assert_eq!(verdict(Pass, None), Verdict::Expected);
        assert_eq!(verdict(LlcFailed, None), Verdict::Worse);
        assert_eq!(verdict(ParserUnsupported, None), Verdict::Worse);
        assert_eq!(verdict(LlcFailed, Some(LlcFailed)), Verdict::Expected);
        assert_eq!(verdict(Pass, Some(LlcFailed)), Verdict::Better);
        assert_eq!(verdict(ParserUnsupported, Some(LlcFailed)), Verdict::Better);
        assert_eq!(verdict(LlcFailed, Some(ParserUnsupported)), Verdict::Worse);
        // Another failure of the same rank is a change to be looked at.
        assert_eq!(verdict(BehaviourDiffers, Some(LlcFailed)), Verdict::Worse);
        assert_eq!(verdict(PrinterNotExpressible, Some(ParserUnsupported)), Verdict::Worse);
        assert_eq!(verdict(Outcome::Skipped, Some(LlcFailed)), Verdict::Unchecked);
    }

    // "Expectation file": the format, its errors, and updates.
    #[test]
    fn the_file() {
        let mut file = Expectations::default();
        file.update("kiln/llvm22/b.ll", Outcome::LlcFailed, "llc failed\tbadly");
        file.update("kiln/llvm22/a.ll", Outcome::ParserUnsupported, "call:forwarded-varargs");
        file.update("kiln/llvm22/c.ll", Outcome::Skipped, "no llc");
        let text = file.text();
        assert!(text.starts_with("# "));
        assert!(text.ends_with(
            "kiln/llvm22/a.ll\tparser-unsupported\tcall:forwarded-varargs\n\
             kiln/llvm22/b.ll\tllc-failed\tllc failed\\tbadly\n"
        ));
        let read = Expectations::parse(&text).unwrap();
        assert_eq!(read, file);
        assert_eq!(read.outcome("kiln/llvm22/b.ll"), Some(Outcome::LlcFailed));
        assert_eq!(read.outcome("kiln/llvm22/c.ll"), None);
        file.update("kiln/llvm22/b.ll", Outcome::Pass, "-");
        assert_eq!(file.entries.len(), 1);

        assert!(Expectations::parse("a\tpass\t-\n").unwrap_err().contains("not a failing"));
        assert!(Expectations::parse("a\tnonsense\t-\n").is_err());
        assert!(Expectations::parse("a\tllc-failed\n").unwrap_err().contains("three"));
        assert!(Expectations::parse("a\tllc-failed\t-\na\tlink-failed\t-\n").is_err());
        assert_eq!(Expectations::parse("# only\n\n").unwrap(), Expectations::default());
    }
}
