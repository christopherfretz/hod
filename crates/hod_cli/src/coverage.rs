//! The coverage log. Contract: `spec/cli.md` "Coverage log".
//!
//! One line per compile invocation, appended with a single `write` on a file
//! opened in append mode, so lines from concurrent compilers never interleave.

use std::borrow::Cow;
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
    pub reason: Option<Cow<'static, str>>,
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
    /// Time the bitcode conversion took, successful or not; `None` when
    /// none was attempted.
    pub conversion_time: Option<Duration>,
    /// How the fallback ended; `None` when none ran.
    pub fallback_status: Option<FallbackStatus>,
    /// With `HOD_CENSUS=1`: every reason hod found in the module, in the
    /// order first met (field 11). `None` otherwise.
    pub reasons: Option<Vec<String>>,
}

/// How a fallback process ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FallbackStatus {
    /// It exited with this code.
    Exited(i32),
    /// A signal ended it.
    Signaled(i32),
    /// Neither is known (no Unix host reports this).
    Unknown,
}

impl FallbackStatus {
    /// The exit code in decimal, `signal:<N>`, or `unknown`.
    fn write(self, out: &mut String) {
        match self {
            FallbackStatus::Exited(code) => out.push_str(&code.to_string()),
            FallbackStatus::Signaled(signal) => {
                out.push_str("signal:");
                out.push_str(&signal.to_string());
            }
            FallbackStatus::Unknown => out.push_str("unknown"),
        }
    }
}

/// Number of tab-separated fields in a line.
#[cfg(test)]
pub const FIELDS: usize = 11;

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

/// A time in whole microseconds, or `-`.
fn micros(time: Option<Duration>) -> String {
    time.map_or_else(|| "-".to_owned(), |t| t.as_micros().to_string())
}

impl Record {
    /// The line, with its newline.
    pub fn line(&self) -> String {
        let mut line = String::with_capacity(96 + self.output.len());
        line.push_str(self.outcome.name());
        line.push('\t');
        escape(self.reason.as_deref().unwrap_or("-"), &mut line);
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
        line.push_str(&micros(self.fallback_time));
        line.push('\t');
        line.push_str(&micros(self.conversion_time));
        line.push('\t');
        match self.fallback_status {
            Some(status) => status.write(&mut line),
            None => line.push('-'),
        }
        line.push('\t');
        match self.reasons.as_deref() {
            Some(reasons) if !reasons.is_empty() => {
                for (at, reason) in reasons.iter().enumerate() {
                    if at > 0 {
                        line.push(',');
                    }
                    let mut name = String::new();
                    escape(reason, &mut name);
                    line.push_str(&name.replace(',', "\\,"));
                }
            }
            _ => line.push('-'),
        }
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
            reason: Some("codegen-not-implemented".into()),
            input_kind: Some(InputKind::Text),
            input_bytes: Some(1234),
            output: "/tmp/kiln-1/0-a.o".to_owned(),
            target: Some("aarch64-unknown-linux-gnu".to_owned()),
            hod_time: Duration::from_micros(250),
            fallback_time: Some(Duration::from_millis(41)),
            conversion_time: None,
            fallback_status: Some(FallbackStatus::Exited(0)),
            reasons: None,
        }
    }

    // spec/cli.md "Coverage log": the eleven fields, in order.
    #[test]
    fn line_format() {
        assert_eq!(
            record().line(),
            "fallback\tcodegen-not-implemented\ttext\t1234\t/tmp/kiln-1/0-a.o\t\
             aarch64-unknown-linux-gnu\t250\t41000\t-\t0\t-\n"
        );
        // A converted module: the kind and the size are the bitcode's, the
        // conversion's time is field 9.
        let converted = Record {
            input_kind: Some(InputKind::Bitcode),
            conversion_time: Some(Duration::from_micros(9300)),
            fallback_status: Some(FallbackStatus::Exited(1)),
            ..record()
        };
        assert_eq!(
            converted.line(),
            "fallback\tcodegen-not-implemented\tbitcode\t1234\t/tmp/kiln-1/0-a.o\t\
             aarch64-unknown-linux-gnu\t250\t41000\t9300\t1\t-\n"
        );
        // Field 10 for a fallback that a signal ended.
        let killed = Record { fallback_status: Some(FallbackStatus::Signaled(9)), ..record() };
        assert!(killed.line().ends_with("\t41000\t-\tsignal:9\t-\n"), "{}", killed.line());
        let unknown = Record { fallback_status: Some(FallbackStatus::Unknown), ..record() };
        assert!(unknown.line().ends_with("\t-\tunknown\t-\n"), "{}", unknown.line());
        let hod = Record {
            outcome: Outcome::Hod,
            reason: None,
            input_kind: Some(InputKind::Bitcode),
            input_bytes: Some(0),
            output: "-".to_owned(),
            target: None,
            hod_time: Duration::from_millis(3),
            fallback_time: None,
            conversion_time: None,
            fallback_status: None,
            reasons: None,
        };
        assert_eq!(hod.line(), "hod\t-\tbitcode\t0\t-\t-\t3000\t-\t-\t-\t-\n");
        let error = Record {
            outcome: Outcome::Error,
            reason: Some("usage".into()),
            input_kind: None,
            input_bytes: None,
            ..hod
        };
        assert_eq!(error.line(), "error\tusage\t-\t-\t-\t-\t3000\t-\t-\t-\t-\n");
        // Field 11, the census of `HOD_CENSUS=1`: the reasons in order; an
        // empty census is `-`.
        let census = Record {
            reason: Some("module:pic".into()),
            reasons: Some(vec!["module:pic".into(), "cc:ccc".into(), "attr:\"a,b\"".into()]),
            ..record()
        };
        assert!(
            census.line().ends_with("\t0\tmodule:pic,cc:ccc,attr:\"a\\,b\"\n"),
            "{}",
            census.line()
        );
        let empty = Record { reasons: Some(Vec::new()), ..record() };
        assert!(empty.line().ends_with("\t0\t-\n"), "{}", empty.line());
    }

    // spec/cli.md "Coverage log": a field never contains a tab or a newline.
    #[test]
    fn free_form_fields_are_escaped() {
        let odd = Record {
            output: "a\tb\nc\rd\\e".to_owned(),
            target: Some(String::new()),
            reason: Some("attr:\"x\ty\"".into()),
            reasons: Some(vec!["intrinsic:a\nb".into()]),
            ..record()
        };
        let line = odd.line();
        assert_eq!(line.matches('\n').count(), 1);
        let fields: Vec<&str> = line.trim_end_matches('\n').split('\t').collect();
        assert_eq!(fields.len(), FIELDS);
        assert_eq!(fields[4], "a\\tb\\nc\\rd\\\\e");
        assert_eq!(fields[5], "-");
        assert_eq!(fields[1], "attr:\"x\\ty\"");
        assert_eq!(fields[10], "intrinsic:a\\nb");
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
