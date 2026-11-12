//! `cargo xtask corpus verify`: the corpus is what its manifest says, and
//! every entry passes `irdiff --hybrid`. Contract: `spec/harness.md`
//! "corpus", "verify".

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use super::capture::entry_path;
use super::manifest::Manifest;
use super::sha256;
use crate::irdiff::{self, Config, Mode, Report, directives};

/// A verification.
#[derive(Clone, Debug)]
pub struct Options {
    /// The corpus directory.
    pub dir: PathBuf,
    /// How `irdiff` runs: its `paths`, `llvm`, `mode` and `results` are
    /// set here, everything else is the caller's.
    pub config: Config,
    /// Only check the manifest; run nothing.
    pub manifest_only: bool,
}

/// What a verification found.
#[derive(Clone, Debug, Default)]
pub struct VerifyReport {
    /// Entries in the manifest.
    pub entries: usize,
    /// Where the corpus differs from its manifest.
    pub problems: Vec<String>,
    /// One `irdiff` report per LLVM major of the corpus.
    pub runs: Vec<(u32, Report)>,
}

impl VerifyReport {
    /// The command's exit status, `irdiff`'s convention: 1 a FAIL or
    /// XPASS; 3 none, but an ERROR, a missing required tool, or a corpus
    /// that differs from its manifest; else 0.
    pub fn exit_status(&self) -> u8 {
        let statuses: Vec<u8> = self.runs.iter().map(|(_, report)| report.exit_status()).collect();
        if statuses.contains(&1) {
            1
        } else if !self.problems.is_empty() || statuses.iter().any(|status| *status != 0) {
            3
        } else {
            0
        }
    }

    /// The text the command prints.
    pub fn summary(&self, verbose: bool) -> String {
        let mut text = String::new();
        for problem in &self.problems {
            let _ = writeln!(text, "MANIFEST {problem}");
        }
        let _ = writeln!(
            text,
            "corpus: manifest: {} entries, {} problems",
            self.entries,
            self.problems.len()
        );
        for (major, report) in &self.runs {
            let _ = writeln!(text, "corpus: irdiff with LLVM {major}:");
            text.push_str(&report.summary(verbose));
        }
        text
    }
}

/// Every `*.ll` file under `dir`, as paths relative to it with `/`.
fn modules_under(dir: &Path) -> Result<BTreeSet<String>, String> {
    let mut found = BTreeSet::new();
    let mut pending = vec![(dir.to_path_buf(), String::new())];
    while let Some((path, prefix)) = pending.pop() {
        let failed = |e: std::io::Error| format!("cannot read '{}': {e}", path.display());
        for entry in fs::read_dir(&path).map_err(failed)? {
            let entry = entry.map_err(failed)?;
            let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
            let kind = entry.file_type().map_err(failed)?;
            if kind.is_dir() {
                pending.push((entry.path(), format!("{name}/")));
            } else if entry.path().extension().is_some_and(|extension| extension == "ll") {
                found.insert(name);
            }
        }
    }
    Ok(found)
}

/// Compares the corpus in `dir` with its manifest: every row's file is
/// there with the row's size and hash, at the path its test gives, and
/// reads as an `irdiff` test for the row's LLVM; no other `*.ll` file is
/// in the directory.
pub fn check_manifest(dir: &Path, manifest: &Manifest) -> Result<Vec<String>, String> {
    let mut problems = Vec::new();
    let mut present = modules_under(dir)?;
    for (entry, row) in &manifest.rows {
        if *entry != entry_path(&row.source, row.llvm, &row.test) {
            problems.push(format!("{entry}: not the entry of test '{}'", row.test));
        }
        if !present.remove(entry) {
            problems.push(format!("{entry}: the file is missing"));
            continue;
        }
        let bytes = match fs::read(dir.join(entry)) {
            Ok(bytes) => bytes,
            Err(e) => {
                problems.push(format!("{entry}: cannot be read: {e}"));
                continue;
            }
        };
        if bytes.len() as u64 != row.bytes || sha256::hex(&bytes) != row.sha256 {
            problems.push(format!("{entry}: the file is not the one the manifest records"));
            continue;
        }
        match directives::parse(&String::from_utf8_lossy(&bytes)) {
            Ok(expect) if expect.accepts_llvm(row.llvm) && expect.covers.is_some() => {}
            Ok(_) => problems.push(format!("{entry}: its directives are not an entry's")),
            Err(errors) => {
                let first = errors.first().map(ToString::to_string).unwrap_or_default();
                problems.push(format!("{entry}: directives: {first}"));
            }
        }
    }
    for stray in present {
        problems.push(format!("{stray}: not in the manifest"));
    }
    Ok(problems)
}

/// Runs the command. An error is a request that cannot be carried out at
/// all: there is no corpus.
pub fn verify(options: &Options) -> Result<VerifyReport, String> {
    let dir = &options.dir;
    let manifest = Manifest::read(dir)?;
    if manifest.rows.is_empty() {
        return Err(format!(
            "no corpus in '{}': run `cargo xtask corpus capture` first",
            dir.display()
        ));
    }
    let mut report = VerifyReport {
        entries: manifest.rows.len(),
        problems: check_manifest(dir, &manifest)?,
        runs: Vec::new(),
    };
    if options.manifest_only {
        return Ok(report);
    }
    let majors: BTreeSet<u32> = manifest.rows.values().map(|row| row.llvm).collect();
    for major in majors.into_iter().rev() {
        let groups: BTreeSet<String> = manifest
            .rows
            .values()
            .filter(|row| row.llvm == major)
            .map(|row| format!("{}/llvm{major}", row.source))
            .collect();
        let mut config = options.config.clone();
        config.paths = groups.iter().map(|group| dir.join(group)).filter(|p| p.is_dir()).collect();
        if config.paths.is_empty() {
            continue;
        }
        config.llvm = Some(major.to_string());
        let mode = if config.mode == Mode::Hybrid { "hybrid" } else { "strict" };
        config.results =
            Some(config.build_dir.join("corpus").join(format!("verify-llvm{major}-{mode}.tsv")));
        report.runs.push((major, irdiff::run(&config)?));
    }
    Ok(report)
}
