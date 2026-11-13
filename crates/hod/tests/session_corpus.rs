//! The corpus through the facade, for comparing two builds of hod
//! (`spec/cli.md` "Session"; `spec/landed/cli/WP-090.md`).
//!
//! Ignored: it reads the corpus of `cargo xtask corpus` (`spec/harness.md`
//! "corpus"), which is not in the repository. Run it with
//!
//! ```text
//! HOD_SESSION_CORPUS=<corpus dir> HOD_SESSION_DUMP=<file> \
//!     cargo test --release -p hod --test session_corpus -- --ignored
//! ```
//!
//! For every module of every entry, its text and its bitcode (from the
//! `llvm-as` of the entry's LLVM) are compiled with the entry's `llc`
//! arguments, once for the first refusal and once with the census, and
//! one line per compile is written to the dump, sorted: the object's
//! length and hash, or the reason, the error's text and the census. Two
//! builds of hod that write the same dump give the same objects and the
//! same declines on the whole corpus. `HOD_SESSION_JOBS` modules at once
//! (default 4).

use std::collections::hash_map::DefaultHasher;
use std::fmt::Write as _;
use std::hash::Hasher;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use hod::{CompileError, OptLevel, Options, OutputKind, Report, Settings};

/// The `llc` arguments the corpus's entries carry.
fn options(llc_args: &str) -> Options {
    let mut options = Options::default();
    for arg in llc_args.split_whitespace() {
        match arg {
            "-O0" => options.opt_level = OptLevel::O0,
            "-O1" => options.opt_level = OptLevel::O1,
            "-O2" => options.opt_level = OptLevel::O2,
            "-O3" => options.opt_level = OptLevel::O3,
            "-relocation-model=pic" => options.relocation_model = Some(hod::RelocModel::Pic),
            "-filetype=obj" => options.output_kind = OutputKind::Object,
            "-debugger-tune=gdb" => options.debugger_tuning = Some(hod::DebuggerTuning::Gdb),
            other => panic!("an llc argument this walker does not know: {other}"),
        }
    }
    options
}

/// One compile as a line of the dump.
fn outcome(result: &Result<Vec<u8>, CompileError>, report: &Report) -> String {
    match result {
        Ok(object) => {
            let mut hasher = DefaultHasher::new();
            hasher.write(object);
            format!("obj {} {:016x}", object.len(), hasher.finish())
        }
        Err(error) => {
            let reason = match error {
                CompileError::Unsupported(u) => u.construct.name().to_owned(),
                CompileError::MalformedInput(_) => "malformed-input".into(),
                CompileError::InvalidOptions(_) => "invalid-options".into(),
                CompileError::Internal(_) => "internal-error".into(),
                CompileError::Panic(_) => "internal-panic".into(),
            };
            format!("err {reason} [{error}] [{}]", report.reasons.join(","))
        }
    }
}

/// The lines of one module: text and bitcode, first refusal and census.
pub fn module_lines(name: &str, input: &[u8], bitcode: &[u8], options: &Options) -> Vec<String> {
    let census = Settings { census: true, ..Settings::default() };
    let mut lines = Vec::new();
    for (route, bytes) in [("text", input), ("bitcode", bitcode)] {
        for (mode, settings) in [("first", Settings::default()), ("census", census.clone())] {
            let mut report = Report::default();
            let result = hod::compile_with(bytes, options, &settings, &mut report);
            lines.push(format!("{name}\t{route}\t{mode}\t{}", outcome(&result, &report)));
        }
    }
    lines
}

/// The modules of an entry: the whole file, or each after its
/// `;@ module:` line, preceded by as many newlines as lines before it
/// (`xtask/src/irdiff/directives.rs`, `modules`).
fn modules_of(entry: &str, text: &str) -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = Vec::new();
    for (index, line) in text.split_inclusive('\n').enumerate() {
        if let Some(name) = line.strip_prefix(";@ module:") {
            found.push((format!("{entry}#{}", name.trim()), "\n".repeat(index)));
        }
        if let Some((_, module)) = found.last_mut() {
            module.push_str(line);
        }
    }
    if found.is_empty() {
        found.push((entry.to_owned(), text.to_owned()));
    }
    found
}

fn llvm_as(llvm: &str, text: &[u8]) -> Result<Vec<u8>, String> {
    let tool = PathBuf::from(format!("/usr/lib/llvm-{llvm}/bin/llvm-as"));
    let mut command = Command::new(&tool);
    command.args(["-o", "-", "-"]);
    let ran = hod_testutil::run_with(&mut command, text, Duration::from_mins(5));
    if ran.success() {
        Ok(ran.stdout)
    } else {
        Err(format!("{}: {}", tool.display(), ran.stderr_text()))
    }
}

fn entry_lines(dir: &Path, entry: &str, llvm: &str, llc_args: &str) -> Result<Vec<String>, String> {
    let path = dir.join(entry);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let options = options(llc_args);
    let mut lines = Vec::new();
    for (name, module) in modules_of(entry, &text) {
        let bitcode = llvm_as(llvm, module.as_bytes()).map_err(|e| format!("{name}: {e}"))?;
        lines.extend(module_lines(&name, module.as_bytes(), &bitcode, &options));
    }
    Ok(lines)
}

#[test]
#[ignore = "reads the corpus of `cargo xtask corpus`; see the module's documentation"]
fn corpus_dump() {
    let dir = PathBuf::from(std::env::var("HOD_SESSION_CORPUS").expect("HOD_SESSION_CORPUS"));
    let dump = PathBuf::from(std::env::var("HOD_SESSION_DUMP").expect("HOD_SESSION_DUMP"));
    let jobs: usize =
        std::env::var("HOD_SESSION_JOBS").ok().and_then(|j| j.parse().ok()).unwrap_or(4);
    let manifest = std::fs::read_to_string(dir.join("MANIFEST.tsv")).expect("MANIFEST.tsv");
    let rows: Vec<Vec<&str>> =
        manifest.lines().skip(1).map(|line| line.split('\t').collect()).collect();
    let next = AtomicUsize::new(0);
    let lines = Mutex::new(Vec::new());
    let failures = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..jobs.max(1) {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(row) = rows.get(index) else { break };
                    match entry_lines(&dir, row[0], row[2], row[7]) {
                        Ok(found) => lines.lock().unwrap().extend(found),
                        Err(message) => failures.lock().unwrap().push(message),
                    }
                }
            });
        }
    });
    let mut lines = lines.into_inner().unwrap();
    lines.sort();
    let mut out = String::new();
    for line in &lines {
        let _ = writeln!(out, "{line}");
    }
    std::fs::write(&dump, out).expect("the dump");
    let failures = failures.into_inner().unwrap();
    assert!(failures.is_empty(), "{} entries failed, first: {}", failures.len(), failures[0]);
    eprintln!("{} entries, {} lines in {}", rows.len(), lines.len(), dump.display());
}
