//! The phase report. Contract: `spec/cli.md` "Phase report".
//!
//! Where the time of one invocation went, for the benchmark harness
//! (`spec/harness.md` "bench", "Phases"): one line per phase, appended to
//! the file `HOD_TIME_PHASES` names with a single `write`.

use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::Path;
use std::time::{Duration, Instant};

/// The phases of one invocation, in the order they ended.
#[derive(Debug)]
pub struct Phases {
    start: Instant,
    last: Instant,
    ended: Vec<(&'static str, Duration)>,
}

impl Phases {
    /// No phase yet; the first one starts at `start`.
    pub fn new(start: Instant) -> Self {
        Self { start, last: start, ended: Vec::new() }
    }

    /// Ends the phase `name`: it lasted from the end of the phase before
    /// it (or from the start) until now.
    pub fn end(&mut self, name: &'static str) {
        self.end_at(name, Instant::now());
    }

    /// Ends the phase `name` at `at`, a moment no earlier than the end of
    /// the phase before it: one that `hod::compile_with` reported.
    pub fn end_at(&mut self, name: &'static str, at: Instant) {
        self.ended.push((name, at.saturating_duration_since(self.last)));
        self.last = self.last.max(at);
    }

    /// The report: `<phase><TAB><microseconds>` per phase, then `total`,
    /// the time since the start.
    pub fn report(&self) -> String {
        let mut report = String::new();
        let total = ("total", self.start.elapsed());
        for (name, time) in self.ended.iter().chain([&total]) {
            let _ = writeln!(report, "{name}\t{}", time.as_micros());
        }
        report
    }

    /// Appends the report to `path` (created if missing) with one `write`
    /// call, so that the reports of concurrent processes do not interleave.
    pub fn append(&self, path: &Path) -> std::io::Result<()> {
        let report = self.report();
        let mut file = OpenOptions::new().append(true).create(true).open(path)?;
        let written = file.write(report.as_bytes())?;
        if written == report.len() {
            Ok(())
        } else {
            Err(std::io::Error::other(format!("short write ({written} of {} bytes)", report.len())))
        }
    }
}
