//! The out-of-process half of the round trip: `llvm-as`, the verifier,
//! and `irdiff`'s engine for `llc`, the link and the run.
//! Contract: `spec/harness.md` "roundtrip", "What an entry goes through"
//! steps 3 to 5.

use std::path::Path;

use crate::irdiff::process::{self, End, Limits, Output};
use crate::irdiff::{BuiltBy, Failure, Outcome as IrdiffOutcome, Record};

use super::Outcome;

/// The decline reason of the stand-in for hod that the engine runs:
/// `corpus capture`'s ([`crate::corpus::capture::DECLINE_SCRIPT`]).
pub const DECLINED: &str = "corpus-admission";

/// Runs one tool on the printed module; `refused` is the outcome of a
/// tool that ran and said no.
pub fn tool_step(
    tool: &Path,
    name: &str,
    args: &[&std::ffi::OsStr],
    limits: Limits,
    refused: Outcome,
) -> Result<(), (Outcome, String)> {
    let output = process::run(
        tool,
        |c| {
            c.args(args);
        },
        b"",
        limits,
    )
    .map_err(|e| (Outcome::HarnessError, format!("{name}: {e}")))?;
    match output.end {
        End::Exit(0) => Ok(()),
        end => Err(failure(name, end, &output, limits, refused)),
    }
}

fn failure(
    name: &str,
    end: End,
    output: &Output,
    limits: Limits,
    refused: Outcome,
) -> (Outcome, String) {
    let said = output.first_error_line();
    let said = if said.is_empty() { String::new() } else { format!(": {}", clip(&said, 300)) };
    match end {
        End::Exit(code) => (refused, format!("{name} failed (exit status {code}){said}")),
        End::Signal(signal) => {
            (Outcome::ToolCrashed, format!("{name} was terminated by signal {signal}{said}"))
        }
        End::TimedOut => (
            Outcome::ToolTimedOut,
            format!("{name} timed out after {} s", limits.time.as_secs_f64()),
        ),
    }
}

/// The outcome of an entry from the engine's records of its printed
/// module, built and run with the stand-in for hod that declines: every
/// record must be the stand-in's decline after the oracle's side passed,
/// one per linker. The first other record decides.
pub fn from_records(records: &[Record], linkers: usize) -> Result<(), (Outcome, String)> {
    let passed = |record: &Record| {
        record.outcome == IrdiffOutcome::Fail
            && record.built_by == BuiltBy::Declined
            && record.reason.as_deref() == Some(DECLINED)
            && record.failure == Some(Failure::Declined)
            && record.linker.is_some()
    };
    if let Some(record) = records.iter().find(|record| !passed(record)) {
        let with = record.linker.as_ref().map_or_else(String::new, |l| format!("with {l}: "));
        let (outcome, detail) = classify(record);
        return Err((outcome, format!("{with}{detail}")));
    }
    if records.len() == linkers {
        Ok(())
    } else {
        Err((Outcome::HarnessError, format!("{} results for {linkers} linkers", records.len())))
    }
}

/// What one record of the engine says went wrong, from its kind of
/// failure (`spec/harness.md` "irdiff", "Library"): only an ERROR of the
/// oracle's side is about the printed module. The detail is the record's,
/// without the engine's `oracle: `.
fn classify(record: &Record) -> (Outcome, String) {
    let detail = &record.detail;
    let step = detail.strip_prefix("oracle: ").unwrap_or(detail).to_owned();
    let outcome = match (record.outcome, record.failure) {
        (IrdiffOutcome::Error, Some(Failure::ReferenceBuild)) => Outcome::LlcFailed,
        (IrdiffOutcome::Error, Some(Failure::Link)) => Outcome::LinkFailed,
        // The program's own crash, hang or flood is behaviour.
        (IrdiffOutcome::Error, Some(Failure::Run | Failure::Comparison)) => {
            Outcome::BehaviourDiffers
        }
        (IrdiffOutcome::Error, Some(Failure::Crash)) => Outcome::ToolCrashed,
        (IrdiffOutcome::Error, Some(Failure::Timeout)) => Outcome::ToolTimedOut,
        (outcome, _) => {
            return (Outcome::HarnessError, format!("{}: {detail}", outcome.name()));
        }
    };
    (outcome, step)
}

/// `text`, cut to at most `max` bytes at a character boundary.
pub fn clip(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &text[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(
        outcome: IrdiffOutcome,
        built_by: BuiltBy,
        linker: Option<&str>,
        failure: Option<Failure>,
        detail: &str,
    ) -> Record {
        Record {
            outcome,
            test: "t".to_owned(),
            target: "aarch64-unknown-linux-gnu".to_owned(),
            llvm: Some(22),
            linker: linker.map(str::to_owned),
            built_by,
            reason: (built_by == BuiltBy::Declined).then(|| DECLINED.to_owned()),
            detail: detail.to_owned(),
            covers: None,
            failure,
        }
    }

    // spec/harness.md "roundtrip", "Outcomes": the kind of the engine's
    // records decides, never their wording.
    #[test]
    fn records_map_to_outcomes() {
        let declined = |linker| {
            let failure = Some(Failure::Declined);
            record(IrdiffOutcome::Fail, BuiltBy::Declined, Some(linker), failure, "x")
        };
        assert_eq!(from_records(&[declined("bfd"), declined("lld")], 2), Ok(()));
        assert_eq!(from_records(&[declined("bfd")], 2).unwrap_err().0, Outcome::HarnessError);
        let error = |linker: Option<&str>, failure: Option<Failure>, detail: &str| {
            let records = [record(IrdiffOutcome::Error, BuiltBy::Nobody, linker, failure, detail)];
            from_records(&records, 1).unwrap_err()
        };
        let cases = [
            (Failure::ReferenceBuild, Outcome::LlcFailed),
            (Failure::Link, Outcome::LinkFailed),
            (Failure::Run, Outcome::BehaviourDiffers),
            (Failure::Comparison, Outcome::BehaviourDiffers),
            (Failure::Crash, Outcome::ToolCrashed),
            (Failure::Timeout, Outcome::ToolTimedOut),
            (Failure::Harness, Outcome::HarnessError),
            (Failure::Witness, Outcome::HarnessError),
            (Failure::HodBuild, Outcome::HarnessError),
        ];
        for (failure, outcome) in cases {
            // The same words whatever the kind: the words do not decide.
            let (got, text) = error(None, Some(failure), "oracle: llc failed (exit status 1)");
            assert_eq!(got, outcome, "{failure:?}");
            assert_eq!(text.starts_with("ERROR: "), outcome == Outcome::HarnessError, "{text}");
            assert!(text.ends_with("llc failed (exit status 1)"), "{text}");
        }
        assert_eq!(error(None, None, "oracle: llc failed").0, Outcome::HarnessError);
        let (_, text) =
            error(Some("mold"), Some(Failure::Link), "oracle: the link wrote no program");
        assert_eq!(text, "with mold: the link wrote no program");
        let skip = [record(IrdiffOutcome::Skip, BuiltBy::Nobody, None, None, "another LLVM")];
        assert_eq!(from_records(&skip, 1).unwrap_err().0, Outcome::HarnessError);
        // hod's side failing is not the stand-in's decline.
        let hod = [record(
            IrdiffOutcome::Fail,
            BuiltBy::Nobody,
            Some("bfd"),
            Some(Failure::HodBuild),
            "hod failed",
        )];
        assert_eq!(from_records(&hod, 1).unwrap_err().0, Outcome::HarnessError);
        assert_eq!(clip("häh", 2), "h...");
    }
}
