//! `cargo xtask debug kiln`: kiln's own debugger tests
//! (`<kiln>/tests/debuginfo`, kiln's `spec/harness.md` "Debugger tests")
//! with hod as kiln's backend, against the same tests with `llc`.
//! Contract: `spec/harness.md` "debug", "kiln".
//!
//! Each test is compiled and linked by `kiln++ -O0 -g` twice: with the
//! LLVM 22 installation (its `llc`), and through hod's shim with
//! `HOD_STRICT=1` (hod must build every unit). Every debugger runs the
//! test's script on both programs; each program passes when the output
//! has the test's checks in order, as kiln's runner requires. The masked
//! transcripts are also compared.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use super::directives::{Script, first_missing_check};
use super::transcript::{self, Debugger, Ended};
use crate::irdiff::process::{self, End, Limits};
use crate::irdiff::tools::Search;

/// A kiln debugger test.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KilnTest {
    pub path: PathBuf,
    pub std: String,
    pub args: Vec<String>,
    pub gdb: Script,
    pub lldb: Script,
}

/// Reads a kiln test's directives; `None` when it is not `expect:
/// debugger`.
pub fn parse(path: &Path, source: &str) -> Option<KilnTest> {
    let mut test =
        KilnTest { path: path.to_path_buf(), std: "c++26".into(), ..KilnTest::default() };
    let mut debugger = false;
    for line in source.lines() {
        let Some(rest) = line.trim_start().strip_prefix("//@") else { continue };
        let Some((key, value)) = rest.split_once(':') else { continue };
        let value = value.trim();
        match key.trim() {
            "expect" => debugger = value == "debugger",
            "std" => value.clone_into(&mut test.std),
            "args" => test.args.extend(value.split_whitespace().map(str::to_owned)),
            "gdb" => test.gdb.commands.push(value.to_owned()),
            "lldb" => test.lldb.commands.push(value.to_owned()),
            "gdb-check" => test.gdb.checks.push(value.to_owned()),
            "lldb-check" => test.lldb.checks.push(value.to_owned()),
            _ => {}
        }
    }
    debugger.then_some(test)
}

/// One test's result with one debugger.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KilnResult {
    pub test: String,
    pub debugger: String,
    /// The program built with `llc`: `None` passes, else why not.
    pub llc: Option<String>,
    /// The program built by hod.
    pub hod: Option<String>,
    /// Whether the two masked transcripts are equal.
    pub same: bool,
    /// Where they differ first: the line number, hod's line, `llc`'s.
    pub difference: Option<(usize, String, String)>,
}

/// A run.
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub results: Vec<KilnResult>,
    pub notes: Vec<String>,
    pub missing_required: bool,
}

impl Report {
    pub fn passes(&self, hod: bool) -> usize {
        self.results.iter().filter(|r| if hod { r.hod.is_none() } else { r.llc.is_none() }).count()
    }

    /// 1 a test that passes with `llc` and not with hod; 3 a missing
    /// required tool; else 0.
    pub fn exit_status(&self) -> u8 {
        if self.results.iter().any(|r| r.llc.is_none() && r.hod.is_some()) {
            1
        } else if self.missing_required {
            3
        } else {
            0
        }
    }

    pub fn summary(&self) -> String {
        let mut out = String::new();
        for note in &self.notes {
            let _ = writeln!(out, "kiln: note: {note}");
        }
        for r in &self.results {
            let verdict = |v: &Option<String>| {
                v.as_ref().map_or("PASS".to_owned(), |w| format!("FAIL ({w})"))
            };
            let _ = writeln!(
                out,
                "{} [{}]: llc {}, hod {}, transcripts {}",
                r.test,
                r.debugger,
                verdict(&r.llc),
                verdict(&r.hod),
                if r.same { "equal" } else { "differ" }
            );
            if let Some((line, ours, theirs)) = &r.difference {
                let _ = writeln!(out, "  line {line}: hod's '{ours}', llc's '{theirs}'");
            }
        }
        let _ = writeln!(
            out,
            "kiln: {} sessions: llc {} pass, hod {} pass, {} with equal transcripts",
            self.results.len(),
            self.passes(false),
            self.passes(true),
            self.results.iter().filter(|r| r.same).count()
        );
        out
    }
}

/// A run's options.
#[derive(Clone, Debug)]
pub struct Options {
    pub kiln: Option<PathBuf>,
    pub kiln_repo: Option<PathBuf>,
    pub hod: Option<PathBuf>,
    pub build_dir: PathBuf,
    pub require_tools: bool,
}

const COMPILE: Limits = Limits { time: Duration::from_mins(3), memory_kib: 8 << 20 };

/// Compiles and links one test with `compiler` into `out`.
fn compile(
    compiler: &Path,
    llvm: &Path,
    repo: &Path,
    test: &KilnTest,
    out: &Path,
    hod_log: Option<&Path>,
) -> Result<(), String> {
    let source = test.path.strip_prefix(repo).unwrap_or(&test.path).to_path_buf();
    let tmp = out.parent().unwrap_or(Path::new("."));
    let configure = |c: &mut Command| {
        c.arg(crate::irdiff::tools::prefixed("--llvm-dir=", llvm));
        c.arg(format!("-std={}", test.std)).arg("-fno-crash-diagnostics").args(&test.args);
        c.args(["-O0", "-g"]).arg(&source).arg("-o").arg(out);
        // kiln's runner compiles from the repository's root, which the
        // checks' paths assume (`at tests/debuginfo/locals.cpp:53`).
        c.current_dir(repo).env("TMPDIR", tmp).env("LC_ALL", "C");
        for variable in ["HOD_FALLBACK_LLC", "HOD_COVERAGE_LOG", "HOD_STRICT", "KILN_LLVM_DIR"] {
            c.env_remove(variable);
        }
        if let Some(log) = hod_log {
            c.env("HOD_STRICT", "1").env("HOD_COVERAGE_LOG", log);
        }
    };
    let output = process::run(compiler, configure, b"", COMPILE)?;
    match output.end {
        End::Exit(0) if out.is_file() => Ok(()),
        _ => {
            let log = hod_log.and_then(|l| fs::read_to_string(l).ok()).unwrap_or_default();
            let declined = log.lines().find(|l| l.starts_with("error\t"));
            match declined {
                Some(line) => {
                    Err(format!("hod declined: {}", line.split('\t').nth(1).unwrap_or("")))
                }
                None => Err(super::build::failure("kiln++", &output, COMPILE)),
            }
        }
    }
}

fn check(debugger: &Debugger, program: &Path, script: &Script) -> (Option<String>, String) {
    match transcript::session(debugger, program, &script.commands, transcript::SESSION_LIMITS) {
        Ok(Ended::Done(text)) => {
            let masked = transcript::mask(&text, program.parent().unwrap_or(Path::new(".")));
            let missing = first_missing_check(&text, &script.checks)
                .map(|miss| format!("no line has '{miss}' after the earlier checks"));
            (missing, masked)
        }
        Ok(Ended::Crashed(_)) => (Some(format!("{} crashed", debugger.name)), String::new()),
        Ok(Ended::TimedOut(_)) => (Some("the session timed out".to_owned()), String::new()),
        Err(e) => (Some(e), String::new()),
    }
}

/// Runs kiln's debugger tests.
pub fn run(options: &Options) -> Result<Report, String> {
    let mut report = Report::default();
    let defaults = crate::corpus::capture_defaults(&options.build_dir);
    let kiln = options.kiln.clone().unwrap_or(defaults.kiln);
    let repo = options.kiln_repo.clone().unwrap_or(defaults.kiln_repo);
    let repo = repo.canonicalize().unwrap_or(repo);
    let search = Search::from_env();
    let llvm = search.versioned("llc", 22).and_then(|llc| {
        let dir = llc.canonicalize().ok()?.parent()?.parent()?.to_path_buf();
        dir.join("bin/clang++").is_file().then_some(dir)
    });
    let tests_dir = repo.join("tests").join("debuginfo");
    let (Some(llvm), true, true) = (llvm, kiln.is_file(), tests_dir.is_dir()) else {
        // kiln is not a tool of the gate; LLVM 22 is.
        report.notes.push(format!(
            "needs kiln++ ('{}'), kiln's tests ('{}') and LLVM 22",
            kiln.display(),
            tests_dir.display()
        ));
        report.missing_required = options.require_tools && search.versioned("llc", 22).is_none();
        return Ok(report);
    };
    let debuggers = transcript::find_debuggers(&search);
    if debuggers.is_empty() {
        report.notes.push("no gdb or lldb".to_owned());
        report.missing_required = options.require_tools;
        return Ok(report);
    }
    let hod = match &options.hod {
        Some(hod) => hod.clone(),
        None => crate::irdiff::build_workspace_hod(&options.build_dir)?,
    };
    let work = options.build_dir.join("l7").join(format!("kiln-{}", std::process::id()));
    let shim = crate::shim::build(&crate::shim::Request {
        llvm: llvm.clone(),
        hod,
        out: work.join("llvm-22"),
        kiln: Some(kiln.clone()),
    })?;
    let mut files: Vec<PathBuf> = fs::read_dir(&tests_dir)
        .map_err(|e| format!("cannot read '{}': {e}", tests_dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "cpp"))
        .collect();
    files.sort();
    for (index, file) in files.iter().enumerate() {
        let Some(test) = fs::read_to_string(file).ok().and_then(|t| parse(file, &t)) else {
            continue;
        };
        let name = file.strip_prefix(&repo).unwrap_or(file).display().to_string();
        let dir = work.join(format!("{index:03}"));
        let (llc_dir, hod_dir) = (dir.join("llc"), dir.join("hod"));
        for d in [&llc_dir, &hod_dir] {
            fs::create_dir_all(d).map_err(|e| format!("cannot create '{}': {e}", d.display()))?;
        }
        let (llc_prog, hod_prog) = (llc_dir.join("test.out"), hod_dir.join("test.out"));
        let built_llc = compile(&kiln, &llvm, &repo, &test, &llc_prog, None);
        let log = dir.join("coverage.tsv");
        let built_hod = compile(&kiln, &shim.dir, &repo, &test, &hod_prog, Some(&log));
        for debugger in &debuggers {
            let script = if debugger.is_gdb() { &test.gdb } else { &test.lldb };
            if script.commands.is_empty() {
                continue;
            }
            let (llc, theirs) = match &built_llc {
                Ok(()) => check(debugger, &llc_prog, script),
                Err(e) => (Some(e.clone()), String::new()),
            };
            let (ours_result, ours) = match &built_hod {
                Ok(()) => check(debugger, &hod_prog, script),
                Err(e) => (Some(e.clone()), String::new()),
            };
            let same = !ours.is_empty() && ours == theirs;
            let difference = if same { None } else { transcript::first_difference(&ours, &theirs) };
            report.results.push(KilnResult {
                test: name.clone(),
                debugger: debugger.name.clone(),
                same,
                difference,
                llc,
                hod: ours_result,
            });
        }
        let _ = fs::remove_dir_all(&dir);
    }
    let _ = fs::remove_dir_all(&work);
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_kiln_directives() {
        let source = "//@ expect: debugger\n//@ args: -freflection -I tests/support\n\
                    //@ gdb: break a.cpp:3\n//@ gdb-check: Breakpoint 1\n//@ lldb: run\n\
                    //@ skip(clang): no\nint main() {}\n";
        let test = parse(Path::new("a.cpp"), source).unwrap();
        assert_eq!(test.std, "c++26");
        assert_eq!(test.args, vec!["-freflection", "-I", "tests/support"]);
        assert_eq!(test.gdb.commands, vec!["break a.cpp:3"]);
        assert_eq!(test.gdb.checks, vec!["Breakpoint 1"]);
        assert_eq!(test.lldb.commands, vec!["run"]);
        assert!(parse(Path::new("b.cpp"), "//@ expect: run\n").is_none());
    }
}
