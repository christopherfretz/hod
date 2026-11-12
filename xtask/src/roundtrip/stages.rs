//! The out-of-process half of the round trip: `llvm-as`, the verifier,
//! and `irdiff`'s engine for `llc`, the link and the run.
//! Contract: `spec/harness.md` "roundtrip", "What an entry goes through"
//! steps 3 to 5.

use std::path::Path;

use crate::irdiff::process::{self, End, Limits, Output};
use crate::irdiff::{BuiltBy, Outcome as IrdiffOutcome, Record};

use super::Outcome;

/// The decline reason of the stand-in for hod that the engine runs:
/// `corpus capture`'s ([`crate::corpus::capture::DECLINE_SCRIPT`]).
pub const DECLINED: &str = "corpus-admission";

/// The reference `llc` configuration (`docs/VALIDATION.md` "Oracles"):
/// `SelectionDAG` at `-O0`. The engine gives `llc` `-O0` and the rest; the
/// wrapper adds the selector flags unless they are there already.
pub const REFERENCE_FLAGS: [&str; 2] = ["-global-isel=0", "-fast-isel=0"];

/// A shell script that runs `llc` in the reference configuration.
pub fn reference_llc_script(llc: &Path) -> Result<String, String> {
    let path = llc.to_string_lossy();
    if path.contains('\'') {
        return Err(format!("cannot quote the path '{path}' for a shell script"));
    }
    let flags = REFERENCE_FLAGS.join(" ");
    Ok(format!(
        "#!/bin/sh\n\
         # Written by `cargo xtask roundtrip` (spec/harness.md \"roundtrip\"):\n\
         # llc in the reference configuration, SelectionDAG at -O0.\n\
         case \" $* \" in *\" -global-isel=\"*|*\" -fast-isel=\"*) exec '{path}' \"$@\" ;; esac\n\
         exec '{path}' {flags} \"$@\"\n"
    ))
}

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

/// What one record of the engine says went wrong. The engine reports
/// trouble on the oracle's side as an ERROR whose detail starts with
/// `oracle: ` and names the step (`spec/harness.md` "irdiff", "Outcomes").
fn classify(record: &Record) -> (Outcome, String) {
    let detail = record.detail.clone();
    let Some(step) =
        detail.strip_prefix("oracle: ").filter(|_| record.outcome == IrdiffOutcome::Error)
    else {
        return (Outcome::HarnessError, format!("{}: {detail}", record.outcome.name()));
    };
    let crashed = step.contains(" was terminated by signal ");
    let timed_out = step.contains(" timed out after ");
    let outcome = if step.starts_with("llc ") || step.starts_with("llc:") {
        Outcome::LlcFailed
    } else if step.starts_with("link with ") || step.starts_with("the link wrote no program") {
        Outcome::LinkFailed
    } else if step.starts_with("the program of llc's object ") {
        // The program's own crash or hang is behaviour.
        return (Outcome::BehaviourDiffers, step.to_owned());
    } else {
        return (Outcome::HarnessError, detail);
    };
    let outcome = if crashed {
        Outcome::ToolCrashed
    } else if timed_out {
        Outcome::ToolTimedOut
    } else {
        outcome
    };
    (outcome, step.to_owned())
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
        }
    }

    // spec/harness.md "roundtrip", "Outcomes": the engine's records name
    // the step that failed on the oracle's side.
    #[test]
    fn records_map_to_outcomes() {
        let declined = |linker| record(IrdiffOutcome::Fail, BuiltBy::Declined, Some(linker), "x");
        assert_eq!(from_records(&[declined("bfd"), declined("lld")], 2), Ok(()));
        assert_eq!(from_records(&[declined("bfd")], 2).unwrap_err().0, Outcome::HarnessError);
        let error = |linker: Option<&str>, detail: &str| {
            let records = [record(IrdiffOutcome::Error, BuiltBy::Nobody, linker, detail)];
            from_records(&records, 1).unwrap_err()
        };
        let cases = [
            ("oracle: llc failed (exit status 1): error: bad", Outcome::LlcFailed),
            ("oracle: llc wrote no object", Outcome::LlcFailed),
            ("oracle: llc was terminated by signal 11", Outcome::ToolCrashed),
            ("oracle: llc timed out after 60 s", Outcome::ToolTimedOut),
            ("oracle: link with bfd failed (exit status 1): undefined", Outcome::LinkFailed),
            ("oracle: the link wrote no program", Outcome::LinkFailed),
            ("oracle: link with mold was terminated by signal 6", Outcome::ToolCrashed),
            (
                "oracle: the program of llc's object ended with signal 11, the directives say exit status 0",
                Outcome::BehaviourDiffers,
            ),
            ("oracle: the program of llc's object timed out after 10 s", Outcome::BehaviourDiffers),
            ("oracle: the driver did not link with lld", Outcome::HarnessError),
            ("cannot create '/x': denied", Outcome::HarnessError),
        ];
        for (detail, outcome) in cases {
            let (got, text) = error(None, detail);
            assert_eq!(got, outcome, "{detail}");
            assert!(!text.starts_with("oracle: ") || outcome == Outcome::HarnessError, "{text}");
        }
        assert!(
            error(Some("mold"), "oracle: the link wrote no program").1.starts_with("with mold: ")
        );
        let skip = [record(IrdiffOutcome::Skip, BuiltBy::Nobody, None, "requires another LLVM")];
        assert_eq!(from_records(&skip, 1).unwrap_err().0, Outcome::HarnessError);
    }

    // "What an entry goes through", step 5: the reference configuration.
    #[test]
    fn the_reference_llc_script() {
        let script = reference_llc_script(Path::new("/usr/lib/llvm-22/bin/llc")).unwrap();
        assert!(script.starts_with("#!/bin/sh\n"));
        assert!(
            script.contains("exec '/usr/lib/llvm-22/bin/llc' -global-isel=0 -fast-isel=0 \"$@\"")
        );
        assert!(reference_llc_script(Path::new("/it's/llc")).is_err());
        assert_eq!(clip("häh", 2), "h...");
    }
}
