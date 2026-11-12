//! The coverage log. Contract: `spec/cli.md` "Coverage log".
//!
//! One line per compile invocation, appended with a single `write` on a file
//! opened in append mode, so lines from concurrent compilers never interleave.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use hod::InputKind;

/// Who produced the output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// hod compiled the module.
    Hod,
    /// The fallback ran (whatever its exit status).
    Fallback,
    /// The invocation failed in hod; no fallback ran to completion.
    Error,
}

impl Outcome {
    fn name(self) -> &'static str {
        match self {
            Outcome::Hod => "hod",
            Outcome::Fallback => "fallback",
            Outcome::Error => "error",
        }
    }
}

/// One line of the log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub outcome: Outcome,
    /// The construct or condition behind a fallback or an error.
    pub reason: Option<&'static str>,
    /// `None` when hod did not read the input.
    pub input_kind: Option<InputKind>,
    /// `None` when hod did not read the input.
    pub input_bytes: Option<usize>,
    /// The output as named on the command line (`-` is standard output, and
    /// also "not known").
    pub output: String,
    /// The target triple, when the command line or the module states one.
    pub target: Option<String>,
    /// Time spent in hod itself.
    pub hod_time: Duration,
    /// Time the fallback process ran.
    pub fallback_time: Option<Duration>,
}

/// Number of tab-separated fields in a line.
#[cfg(test)]
pub const FIELDS: usize = 8;

/// Makes a free-form field safe for one tab-separated line: backslash, tab,
/// newline and carriage return become `\\`, `\t`, `\n`, `\r`; an empty
/// field becomes `-`.
fn escape(field: &str, out: &mut String) {
    if field.is_empty() {
        out.push('-');
    }
    for c in field.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
}

impl Record {
    /// The line, with its newline.
    pub fn line(&self) -> String {
        let mut line = String::with_capacity(96 + self.output.len());
        line.push_str(self.outcome.name());
        line.push('\t');
        line.push_str(self.reason.unwrap_or("-"));
        line.push('\t');
        line.push_str(self.input_kind.map_or("-", InputKind::name));
        line.push('\t');
        line.push_str(&self.input_bytes.map_or_else(|| "-".to_owned(), |n| n.to_string()));
        line.push('\t');
        escape(&self.output, &mut line);
        line.push('\t');
        escape(self.target.as_deref().unwrap_or("-"), &mut line);
        line.push('\t');
        line.push_str(&self.hod_time.as_micros().to_string());
        line.push('\t');
        line.push_str(
            &self.fallback_time.map_or_else(|| "-".to_owned(), |t| t.as_micros().to_string()),
        );
        line.push('\n');
        line
    }
}

/// An open coverage log.
#[derive(Debug)]
pub struct Log {
    file: File,
}

impl Log {
    /// Opens `path` for appending, creating it if needed.
    pub fn open(path: &Path) -> std::io::Result<Log> {
        OpenOptions::new().append(true).create(true).open(path).map(|file| Log { file })
    }

    /// Appends one record with one `write` call. A short write is an error:
    /// finishing the line with a second call could interleave it with
    /// another process's line.
    pub fn append(&mut self, record: &Record) -> std::io::Result<()> {
        let line = record.line();
        let written = self.file.write(line.as_bytes())?;
        if written == line.len() {
            Ok(())
        } else {
            Err(std::io::Error::other(format!("short write ({written} of {} bytes)", line.len())))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hod_testutil::Scratch;

    fn record() -> Record {
        Record {
            outcome: Outcome::Fallback,
            reason: Some("codegen-not-implemented"),
            input_kind: Some(InputKind::Text),
            input_bytes: Some(1234),
            output: "/tmp/kiln-1/0-a.o".to_owned(),
            target: Some("aarch64-unknown-linux-gnu".to_owned()),
            hod_time: Duration::from_micros(250),
            fallback_time: Some(Duration::from_millis(41)),
        }
    }

    // spec/cli.md "Coverage log": the eight fields, in order.
    #[test]
    fn line_format() {
        assert_eq!(
            record().line(),
            "fallback\tcodegen-not-implemented\ttext\t1234\t/tmp/kiln-1/0-a.o\t\
             aarch64-unknown-linux-gnu\t250\t41000\n"
        );
        let hod = Record {
            outcome: Outcome::Hod,
            reason: None,
            input_kind: Some(InputKind::Bitcode),
            input_bytes: Some(0),
            output: "-".to_owned(),
            target: None,
            hod_time: Duration::from_millis(3),
            fallback_time: None,
        };
        assert_eq!(hod.line(), "hod\t-\tbitcode\t0\t-\t-\t3000\t-\n");
        let error = Record {
            outcome: Outcome::Error,
            reason: Some("usage"),
            input_kind: None,
            input_bytes: None,
            ..hod
        };
        assert_eq!(error.line(), "error\tusage\t-\t-\t-\t-\t3000\t-\n");
    }

    // spec/cli.md "Coverage log": a field never contains a tab or a newline.
    #[test]
    fn free_form_fields_are_escaped() {
        let odd =
            Record { output: "a\tb\nc\rd\\e".to_owned(), target: Some(String::new()), ..record() };
        let line = odd.line();
        assert_eq!(line.matches('\n').count(), 1);
        let fields: Vec<&str> = line.trim_end_matches('\n').split('\t').collect();
        assert_eq!(fields.len(), FIELDS);
        assert_eq!(fields[4], "a\\tb\\nc\\rd\\\\e");
        assert_eq!(fields[5], "-");
    }

    #[test]
    fn appends_to_an_existing_file() {
        let scratch = Scratch::new("coverage");
        let path = scratch.write("cov.tsv", "earlier\n");
        for _ in 0..2 {
            Log::open(&path).unwrap().append(&record()).unwrap();
        }
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text, format!("earlier\n{0}{0}", record().line()));
        assert!(Log::open(&scratch.join("missing-dir/cov.tsv")).is_err());
    }
}
