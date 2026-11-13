//! `cargo xtask debug verify`: every `-g` module of the corpus that hod
//! builds is `llvm-dwarfdump --verify` clean. Contract: `spec/harness.md`
//! "debug", "verify".

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use super::build;
use crate::irdiff::process::{self, End, Limits};
use crate::irdiff::tools::Search;

/// What became of one module.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// hod built it and every `llvm-dwarfdump --verify` is clean.
    Clean,
    /// hod built it and a verifier complained: which, and its first error.
    Dirty(String),
    /// hod declined it, with this reason.
    Declined(String),
    /// hod failed on it (a crash, a wrong status).
    Failed(String),
    /// Not for this host: the module's `target` directive names another.
    Skipped(String),
}

/// A run.
#[derive(Clone, Debug)]
pub struct Options {
    /// Corpus directories and `.ll` files.
    pub paths: Vec<PathBuf>,
    pub jobs: usize,
    pub hod: Option<PathBuf>,
    pub build_dir: PathBuf,
    pub require_tools: bool,
    pub limits: Limits,
    /// WP-136: the target whose modules are verified (`--target`); `None`
    /// is this machine's.
    pub target: Option<crate::irdiff::directives::Arch>,
}

/// What a run found, by module in name order.
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub modules: Vec<(String, Verdict)>,
    pub notes: Vec<String>,
    pub missing_required: bool,
}

impl Report {
    /// 1 a module hod built does not verify or hod failed; 3 a missing
    /// required tool; else 0.
    pub fn exit_status(&self) -> u8 {
        let bad = |v: &Verdict| matches!(v, Verdict::Dirty(_) | Verdict::Failed(_));
        if self.modules.iter().any(|(_, v)| bad(v)) {
            1
        } else if self.missing_required {
            3
        } else {
            0
        }
    }

    pub fn summary(&self, verbose: bool) -> String {
        let mut out = String::new();
        for note in &self.notes {
            let _ = writeln!(out, "verify: note: {note}");
        }
        let mut declined: BTreeMap<&str, usize> = BTreeMap::new();
        let (mut clean, mut dirty, mut failed, mut skipped) = (0, 0, 0, 0);
        for (name, verdict) in &self.modules {
            match verdict {
                Verdict::Clean => {
                    clean += 1;
                    if verbose {
                        let _ = writeln!(out, "CLEAN {name}");
                    }
                }
                Verdict::Dirty(why) => {
                    dirty += 1;
                    let _ = writeln!(out, "DIRTY {name}: {why}");
                }
                Verdict::Failed(why) => {
                    failed += 1;
                    let _ = writeln!(out, "FAIL {name}: {why}");
                }
                Verdict::Declined(reason) => {
                    *declined.entry(reason).or_default() += 1;
                    if verbose {
                        let _ = writeln!(out, "DECLINED {name}: {reason}");
                    }
                }
                Verdict::Skipped(why) => {
                    skipped += 1;
                    if verbose {
                        let _ = writeln!(out, "SKIP {name}: {why}");
                    }
                }
            }
        }
        for (reason, n) in &declined {
            let _ = writeln!(out, "verify: declined {n}: {reason}");
        }
        let total_declined: usize = declined.values().sum();
        let _ = writeln!(
            out,
            "verify: {} -g modules: {} built ({clean} clean, {dirty} not clean), \
             {total_declined} declined, {failed} hod failures, {skipped} skipped",
            self.modules.len(),
            clean + dirty
        );
        out
    }
}

/// The `.ll` files under `paths` that have a compile unit
/// (`!llvm.dbg.cu`), sorted.
pub fn debug_modules(paths: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
        let entries =
            fs::read_dir(dir).map_err(|e| format!("cannot read '{}': {e}", dir.display()))?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out)?;
            } else if path.extension().is_some_and(|e| e == "ll") {
                out.push(path);
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    for path in paths {
        if path.is_dir() {
            walk(path, &mut files)?;
        } else if path.is_file() {
            files.push(path.clone());
        } else {
            return Err(format!("no such module or directory: '{}'", path.display()));
        }
    }
    files.sort();
    files.dedup();
    files.retain(|f| fs::read_to_string(f).is_ok_and(|t| t.contains("!llvm.dbg.cu")));
    Ok(files)
}

/// The value of a module's `;@ <key>:` directive.
fn directive<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines()
        .take_while(|l| l.starts_with(";@") || l.starts_with(';') || l.is_empty())
        .filter_map(|l| l.strip_prefix(";@"))
        .find_map(|l| l.trim().strip_prefix(key)?.strip_prefix(':').map(str::trim))
}

/// One module: hod (strict) with the module's `llc-args`, then every
/// verifier.
fn verify_one(
    module: &Path,
    hod: &Path,
    verifiers: &[PathBuf],
    dir: &Path,
    (limits, target): (Limits, Option<crate::irdiff::directives::Arch>),
) -> Verdict {
    let text = fs::read_to_string(module).unwrap_or_default();
    let host = target.map_or(std::env::consts::ARCH, |t| t.name());
    if let Some(target) = directive(&text, "target")
        && !target.split_whitespace().any(|t| t == host)
    {
        return Verdict::Skipped(format!("target {target}"));
    }
    let object = dir.join("hod.o");
    let log = dir.join("coverage.tsv");
    let extra: Vec<&str> = directive(&text, "llc-args").unwrap_or("").split_whitespace().collect();
    let configure = |c: &mut Command| {
        c.arg(crate::irdiff::tools::prefixed("--coverage-log=", &log)).arg("--strict");
        c.args(["-O0", "-filetype=obj", "-relocation-model=pic"]).args(&extra);
        c.arg("-o").arg(&object).arg(module);
        for variable in ["HOD_FALLBACK_LLC", "HOD_STRICT", "HOD_COVERAGE_LOG", "HOD_IN_FALLBACK"] {
            c.env_remove(variable);
        }
    };
    let output = match process::run(hod, configure, b"", limits) {
        Ok(output) => output,
        Err(e) => return Verdict::Failed(e),
    };
    let line = fs::read_to_string(&log).unwrap_or_default();
    let mut fields = line.lines().next().unwrap_or("").split('\t');
    let (outcome, reason) = (fields.next().unwrap_or(""), fields.next().unwrap_or(""));
    match (output.end, outcome) {
        (End::Exit(0), "hod") if object.is_file() => {}
        (End::Exit(2), "error") => return Verdict::Declined(reason.to_owned()),
        _ => return Verdict::Failed(build::failure("hod", &output, limits)),
    }
    for verifier in verifiers {
        let configure = |c: &mut Command| {
            c.arg("--verify").arg(&object).env("LC_ALL", "C");
        };
        let verified = match process::run(verifier, configure, b"", limits) {
            Ok(output) => output,
            Err(e) => return Verdict::Failed(e),
        };
        if verified.end != End::Exit(0) {
            let said = String::from_utf8_lossy(&verified.stdout);
            let first = said.lines().find(|l| l.contains("error:")).unwrap_or("").trim();
            return Verdict::Dirty(format!("{}: {first}", verifier.display()));
        }
    }
    Verdict::Clean
}

/// Runs the verification.
pub fn run(options: &Options) -> Result<Report, String> {
    let mut report = Report::default();
    let search = Search::from_env();
    let verifiers: Vec<PathBuf> =
        [22, 19].into_iter().filter_map(|m| search.versioned("llvm-dwarfdump", m)).collect();
    if verifiers.is_empty() {
        report.notes.push("no llvm-dwarfdump: nothing is verified".to_owned());
        report.missing_required = options.require_tools;
        return Ok(report);
    }
    let paths = if options.paths.is_empty() {
        let corpus = crate::corpus::default_dir(&options.build_dir);
        if !corpus.is_dir() {
            report.notes.push(format!("no corpus at '{}'", corpus.display()));
            report.missing_required = options.require_tools;
            return Ok(report);
        }
        vec![corpus]
    } else {
        options.paths.clone()
    };
    let modules = debug_modules(&paths)?;
    let hod = match &options.hod {
        Some(hod) => hod.clone(),
        None => crate::irdiff::build_workspace_hod(&options.build_dir)?,
    };
    let work = options.build_dir.join("l7").join(format!("verify-{}", std::process::id()));
    fs::create_dir_all(&work).map_err(|e| format!("cannot create '{}': {e}", work.display()))?;
    let next = AtomicUsize::new(0);
    let slots = Mutex::new(vec![None; modules.len()]);
    std::thread::scope(|scope| {
        for _ in 0..options.jobs.clamp(1, modules.len().max(1)) {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(module) = modules.get(index) else { break };
                    let dir = work.join(format!("{index:05}"));
                    let verdict = match fs::create_dir_all(&dir) {
                        Ok(()) => verify_one(
                            module,
                            &hod,
                            &verifiers,
                            &dir,
                            (options.limits, options.target),
                        ),
                        Err(e) => {
                            Verdict::Failed(format!("cannot create '{}': {e}", dir.display()))
                        }
                    };
                    let _ = fs::remove_dir_all(&dir);
                    slots.lock().unwrap_or_else(PoisonError::into_inner)[index] = Some(verdict);
                }
            });
        }
    });
    let _ = fs::remove_dir_all(&work);
    let verdicts = slots.into_inner().unwrap_or_else(PoisonError::into_inner);
    for (module, verdict) in modules.iter().zip(verdicts) {
        let verdict = verdict.unwrap_or_else(|| Verdict::Failed("not run".to_owned()));
        report.modules.push((module.display().to_string(), verdict));
    }
    Ok(report)
}

/// The default limits of hod and the verifier.
pub const LIMITS: Limits = Limits { time: Duration::from_mins(2), memory_kib: 4 << 20 };

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_module_directives() {
        let text = ";@ covers: x\n;@ target: aarch64\n;@ llc-args: -debugger-tune=gdb\n\n\
                    ; ModuleID = 'x'\n;@ llc-args: late\n";
        assert_eq!(directive(text, "target"), Some("aarch64"));
        assert_eq!(directive(text, "llc-args"), Some("-debugger-tune=gdb"));
        assert_eq!(directive(text, "llvm"), None);
    }

    #[test]
    fn summary_and_status() {
        let report = Report {
            modules: vec![
                ("a.ll".into(), Verdict::Clean),
                ("b.ll".into(), Verdict::Declined("debug:import-scope".into())),
                ("c.ll".into(), Verdict::Declined("debug:import-scope".into())),
            ],
            ..Report::default()
        };
        assert_eq!(report.exit_status(), 0);
        let text = report.summary(false);
        assert!(text.contains("verify: declined 2: debug:import-scope"), "{text}");
        assert!(
            text.contains("3 -g modules: 1 built (1 clean, 0 not clean), 2 declined"),
            "{text}"
        );
        let dirty = Report { modules: vec![("a.ll".into(), Verdict::Dirty("x".into()))], ..report };
        assert_eq!(dirty.exit_status(), 1);
    }
}
