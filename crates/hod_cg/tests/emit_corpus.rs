//! Module emission over the captured corpus (`spec/codegen.md` §9.7):
//! every module, its function bodies stripped (printed by `hod_ir` with
//! none), is emitted by `hod` and compiled by `llc` of its LLVM major; the
//! objects must agree, or the module is one `hod` refuses by name. The
//! report counts, per major, the modules that agree, those refused (by
//! construct) and those that differ (by class of difference, with
//! examples).
//!
//! The test reads the directory `HOD_CG_CORPUS` names (each `*.ll` under
//! it, recursively) and skips when it is not set. Run it optimized:
//!
//! ```text
//! HOD_CG_CORPUS=.build/corpus cargo test --release -p hod_cg --test emit_corpus -- --nocapture
//! ```
//!
//! `HOD_CG_CORPUS_STRICT=1` fails the test on any difference.

mod elfview;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use elfview::{Elf, diff};
use hod_cg::{Options, RelocModel, emit_module};
use hod_ir::Dialect;
use hod_testutil::{Scratch, run_with};

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
fn major(path: &Path) -> Option<u32> {
    path.components()
        .filter_map(|part| part.as_os_str().to_str())
        .find_map(|part| part.strip_prefix("llvm")?.parse().ok())
}

/// What became of one module.
enum Outcome {
    Agree,
    /// Not a module this test can strip and compare: why.
    Skipped(String),
    /// `hod` refuses it: the construct.
    Refused(String),
    /// The classes of the differences, with a line of each.
    Differ(Vec<(String, String)>),
}

/// Debug information is `hod_dwarf`'s; the stripped module has none of
/// its functions' anyway.
fn skip(name: &str) -> bool {
    name.starts_with(".debug") || name == ".eh_frame"
}

/// The class of a line of `describe`: what kind of line, of which
/// section kind, with the names generalized.
fn class(side: &str, line: &str) -> String {
    let mut words = line.split(' ');
    let what = words.next().unwrap_or("");
    let general = |name: &str| -> String {
        let name = name.trim_matches(|c| c == '[' || c == ']');
        for prefix in [
            ".rodata.str",
            ".rodata.cst",
            ".data.rel.ro",
            ".tbss",
            ".tdata",
            ".bss",
            ".data",
            ".rodata",
            ".text",
            ".init_array",
            ".fini_array",
        ] {
            if name.starts_with(prefix) {
                return prefix.to_string();
            }
        }
        name.to_string()
    };
    match what {
        "section" => format!("{side}: section {}", general(words.next().unwrap_or(""))),
        "symbol" => {
            let _name = words.next();
            let section = general(words.next().unwrap_or(""));
            let rest: Vec<&str> = words.collect();
            // type and bind
            let kind =
                rest.iter().skip_while(|w| **w != "type").take(4).copied().collect::<Vec<_>>();
            format!("{side}: symbol in {section} {}", kind.join(" "))
        }
        "reloc" => {
            let section = general(words.next().unwrap_or(""));
            let _offset = words.next();
            let _type = words.next();
            let kind = words.next().unwrap_or("");
            let target = words.next().unwrap_or("");
            let target = if target.starts_with('[') { "section symbol" } else { "symbol" };
            format!("{side}: reloc in {section} type {kind} against a {target}")
        }
        other => format!("{side}: {other}"),
    }
}

fn run_one(scratch: &Scratch, index: usize, path: &Path) -> Outcome {
    let Some(major) = major(path) else { return Outcome::Skipped("no major".into()) };
    let llc = PathBuf::from(format!("/usr/lib/llvm-{major}/bin/llc"));
    if !llc.is_file() {
        return Outcome::Skipped(format!("no llc-{major}"));
    }
    let source = std::fs::read(path).unwrap();
    let options = hod_ll::ParseOptions::default();
    let parsed = match hod_ll::parse_module(&source, &options) {
        Ok(parsed) => parsed,
        Err(e) => {
            return Outcome::Skipped(format!(
                "hod_ll: {}",
                e.to_string().lines().next().unwrap_or("")
            ));
        }
    };
    if let Some(u) = parsed.module.unsupported().next() {
        return Outcome::Refused(format!("hod_ll: {}", u.construct));
    }
    let mut text = String::new();
    if let Err(e) = parsed.module.print(&mut text, Dialect::new(major).unwrap(), []) {
        return Outcome::Skipped(format!("print: {e}"));
    }
    let stripped = match hod_ll::parse_module(text.as_bytes(), &options) {
        Ok(parsed) => parsed,
        Err(e) => return Outcome::Skipped(format!("reparse: {e}")),
    };
    let input = scratch.write(&format!("m{index}.ll"), &text);
    let output = scratch.join(format!("m{index}.o"));
    let mut command = Command::new(&llc);
    command
        .args(["-O0", "-global-isel=0", "-fast-isel=0", "-relocation-model=pic", "-filetype=obj"])
        .arg(&input)
        .arg("-o")
        .arg(&output);
    let ran = run_with(&mut command, &[], Duration::from_mins(2));
    let _ = std::fs::remove_file(&input);
    let emitted = emit_module(
        &stripped.module,
        &Options { reloc_model: RelocModel::Pic, ..Options::default() },
        &[],
    );
    if !ran.success() {
        let why = ran.stderr_text();
        let why = why.lines().find(|l| l.contains("error")).unwrap_or("").to_string();
        let why = why.rsplit("error: ").next().unwrap_or("").to_string();
        return Outcome::Skipped(format!("llc rejects the stripped module: {why}"));
    }
    let theirs = Elf::parse(std::fs::read(&output).unwrap());
    let _ = std::fs::remove_file(&output);
    let emitted = match emitted {
        Ok(emitted) => emitted,
        Err(e) => return Outcome::Refused(format!("hod_cg: {e}")),
    };
    let ours = Elf::parse(hod_obj::elf::to_vec(&emitted.object).unwrap());
    if std::env::var_os("HOD_CG_CORPUS_ONLY").is_some() {
        let keep = Path::new(env!("CARGO_TARGET_TMPDIR")).join("emit-corpus");
        std::fs::create_dir_all(&keep).unwrap();
        std::fs::write(keep.join(format!("m{index}.ll")), &text).unwrap();
        std::fs::write(keep.join(format!("m{index}.llc.o")), &theirs.bytes).unwrap();
        std::fs::write(keep.join(format!("m{index}.hod.o")), &ours.bytes).unwrap();
        eprintln!("kept {}: m{index}", path.display());
    }
    let (a, b) =
        (ours.describe_without(&|_| false, &skip), theirs.describe_without(&|_| false, &skip));
    let (only_ours, only_theirs) = diff(&a, &b);
    if only_ours.is_empty() && only_theirs.is_empty() {
        return Outcome::Agree;
    }
    let mut classes: BTreeMap<String, String> = BTreeMap::new();
    for line in &only_ours {
        classes.entry(class("hod", line)).or_insert_with(|| line.clone());
    }
    for line in &only_theirs {
        classes.entry(class("llc", line)).or_insert_with(|| line.clone());
    }
    Outcome::Differ(classes.into_iter().collect())
}

#[derive(Default)]
struct Tally {
    modules: usize,
    agree: usize,
    refused: BTreeMap<String, usize>,
    skipped: BTreeMap<String, usize>,
    differ: usize,
    /// Class: modules, and examples (module, line).
    classes: BTreeMap<String, (usize, Vec<(String, String)>)>,
}

#[test]
fn corpus_modules_agree_with_llc() {
    let Some(root) = std::env::var_os("HOD_CG_CORPUS") else {
        eprintln!("skipped: HOD_CG_CORPUS is not set");
        return;
    };
    let mut paths = Vec::new();
    modules(Path::new(&root), &mut paths);
    paths.sort();
    // `HOD_CG_CORPUS_ONLY=<text>`: only the modules whose path has it,
    // keeping the stripped text and `llc`'s object in the scratch
    // directory.
    if let Some(only) = std::env::var_os("HOD_CG_CORPUS_ONLY") {
        let only = only.to_string_lossy().into_owned();
        paths.retain(|p| p.to_string_lossy().contains(&only));
        eprintln!("{} modules; kept in {}", paths.len(), env!("CARGO_TARGET_TMPDIR"));
    }
    let scratch = Scratch::new("emit-corpus");
    let next = AtomicUsize::new(0);
    let tallies: Mutex<BTreeMap<u32, Tally>> = Mutex::new(BTreeMap::new());
    let threads = std::thread::available_parallelism().map_or(1, std::num::NonZero::get).min(4);
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(path) = paths.get(index) else { break };
                    let outcome = run_one(&scratch, index, path);
                    let name = path.strip_prefix(&root).unwrap_or(path).display().to_string();
                    let mut tallies = tallies.lock().unwrap();
                    let tally = tallies.entry(major(path).unwrap_or(0)).or_default();
                    tally.modules += 1;
                    match outcome {
                        Outcome::Agree => tally.agree += 1,
                        Outcome::Skipped(why) => *tally.skipped.entry(why).or_default() += 1,
                        Outcome::Refused(why) => *tally.refused.entry(why).or_default() += 1,
                        Outcome::Differ(classes) => {
                            tally.differ += 1;
                            for (class, line) in classes {
                                let entry = tally.classes.entry(class).or_default();
                                entry.0 += 1;
                                if entry.1.len() < 3 {
                                    entry.1.push((name.clone(), line));
                                }
                            }
                        }
                    }
                }
            });
        }
    });
    let tallies = tallies.into_inner().unwrap();
    let mut differ = 0;
    for (major, tally) in &tallies {
        eprintln!(
            "emit_corpus: LLVM {major}: {} modules: {} agree, {} differ, {} refused, {} skipped",
            tally.modules,
            tally.agree,
            tally.differ,
            tally.refused.values().sum::<usize>(),
            tally.skipped.values().sum::<usize>()
        );
        for (why, n) in &tally.refused {
            eprintln!("  refused {n:>5}: {why}");
        }
        for (why, n) in &tally.skipped {
            eprintln!("  skipped {n:>5}: {why}");
        }
        for (class, (n, examples)) in &tally.classes {
            eprintln!("  differ  {n:>5}: {class}");
            for (module, line) in examples {
                let line: String = line.chars().take(200).collect();
                eprintln!("      {module}: {line}");
            }
        }
        differ += tally.differ;
    }
    if std::env::var_os("HOD_CG_CORPUS_STRICT").is_some() {
        assert_eq!(differ, 0, "modules differ from llc");
    }
}
