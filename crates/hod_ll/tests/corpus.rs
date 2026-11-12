//! `spec/ll.md` §11: every body of the captured corpus parses and
//! verifies, or is unsupported by name.
//!
//! The corpus is `xtask corpus`'s capture; the test reads the directory
//! `HOD_LL_CORPUS` names (each `*.ll` under it, recursively) and skips
//! with a note when the variable is not set. Run it optimized:
//!
//! ```text
//! HOD_LL_CORPUS=.build/corpus cargo test --release -p hod_ll --test corpus -- --nocapture
//! ```

#![allow(clippy::cast_precision_loss)] // Rates for display.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use hod_ll::{BodyParser, ParseOptions, parse_module};

fn modules(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            modules(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "ll") {
            out.push(path);
        }
    }
}

/// The LLVM major a corpus path is under (`…/llvm19/…`).
fn major(path: &Path) -> String {
    path.components()
        .filter_map(|part| part.as_os_str().to_str())
        .find(|part| part.starts_with("llvm"))
        .unwrap_or("other")
        .to_string()
}

#[derive(Default)]
struct Tally {
    modules: usize,
    module_errors: usize,
    bodies: usize,
    supported: usize,
    unsupported: usize,
    errors: usize,
    bytes: usize,
    time: Duration,
}

#[test]
fn every_corpus_body_parses_and_verifies() {
    let Some(root) = std::env::var_os("HOD_LL_CORPUS") else {
        eprintln!("skipped: HOD_LL_CORPUS is not set");
        return;
    };
    let mut paths = Vec::new();
    modules(Path::new(&root), &mut paths);
    paths.sort();
    assert!(!paths.is_empty(), "no .ll files under {}", Path::new(&root).display());

    let mut tallies: BTreeMap<String, Tally> = BTreeMap::new();
    let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
    let mut failures = Vec::new();
    for path in &paths {
        let tally = tallies.entry(major(path)).or_default();
        tally.modules += 1;
        let source = std::fs::read(path).expect("a readable corpus file");
        let mut parsed = match parse_module(&source, &ParseOptions::default()) {
            Ok(parsed) => parsed,
            Err(error) => {
                tally.module_errors += 1;
                failures.push(format!("{}: module: {error}", path.display()));
                continue;
            }
        };
        let mut parser = BodyParser::new(&parsed.text);
        for function in parsed.text.functions() {
            tally.bodies += 1;
            tally.bytes += (function.body.end - function.body.start) as usize;
            let start = Instant::now();
            let result = parser.parse(&mut parsed.module, function);
            tally.time += start.elapsed();
            match result {
                Ok(Some(body)) => {
                    if let Err(error) = body.verify(&parsed.module) {
                        tally.errors += 1;
                        failures.push(format!("{}: verify: {error}", path.display()));
                    } else {
                        tally.supported += 1;
                    }
                    parser.recycle(body);
                }
                Ok(None) => {
                    tally.unsupported += 1;
                    for reason in parser.unsupported() {
                        *reasons.entry(reason.to_string()).or_default() += 1;
                    }
                }
                Err(error) => {
                    tally.errors += 1;
                    failures.push(format!("{}: {error}", path.display()));
                }
            }
        }
    }
    eprintln!("| LLVM | modules | bodies | supported | unsupported | errors | body MiB | MiB/s |");
    eprintln!("|---|---|---|---|---|---|---|---|");
    for (major, t) in &tallies {
        let mib = t.bytes as f64 / (1024.0 * 1024.0);
        eprintln!(
            "| {major} | {} | {} | {} | {} | {} | {mib:.1} | {:.0} |",
            t.modules,
            t.bodies,
            t.supported,
            t.unsupported,
            t.errors + t.module_errors,
            mib / t.time.as_secs_f64().max(1e-9),
        );
    }
    eprintln!("unsupported, by bodies:");
    for (reason, count) in &reasons {
        eprintln!("  {count:6} {reason}");
    }
    for failure in failures.iter().take(40) {
        eprintln!("{failure}");
    }
    assert!(failures.is_empty(), "{} failures", failures.len());
}
