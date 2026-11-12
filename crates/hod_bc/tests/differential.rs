//! `spec/bc.md` §9.2: over the captured corpus, the module `hod_bc` reads
//! from `llvm-as` output is the module `hod_ll` reads from `llvm-dis` of
//! the same bitcode, compared as the structural dump of
//! `common::dump` (symbols by name with every field, types and
//! constants written out, notes, module lists).
//!
//! The corpus is `xtask corpus`'s capture; the test reads the directory
//! `HOD_BC_CORPUS` names and skips when it is not set. `HOD_BC_EVERY=N`
//! takes every Nth module; `HOD_BC_DIFFS=dir` writes each disagreement's
//! two dumps there. Run it optimized:
//!
//! ```text
//! HOD_BC_CORPUS=.build/corpus cargo test --release -p hod_bc --test differential -- --nocapture
//! ```

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

const WORKERS: usize = 8;

#[derive(Default)]
struct Tally {
    modules: usize,
    agree: usize,
    both_refuse: usize,
    differ: usize,
    tool_failures: usize,
}

enum Outcome {
    Agree,
    BothRefuse,
    Tool(String),
    Differ(Vec<String>, String, String),
}

fn compare(path: &Path, major: u32) -> Outcome {
    let text = match std::fs::read(path) {
        Ok(text) => text,
        Err(error) => return Outcome::Tool(format!("read: {error}")),
    };
    let bitcode = match common::assemble(major, &text) {
        Ok(bitcode) => bitcode,
        Err(error) => {
            return Outcome::Tool(format!("llvm-as: {}", error.lines().next().unwrap_or("")));
        }
    };
    let canonical = match common::disassemble(major, &bitcode) {
        Ok(canonical) => canonical,
        Err(error) => {
            return Outcome::Tool(format!("llvm-dis: {}", error.lines().next().unwrap_or("")));
        }
    };
    let ll = common::read_text(&canonical);
    let bc = common::read_bitcode(&bitcode);
    match (ll, bc) {
        (Ok((ll, ll_numbered)), Ok((bc, bc_numbered))) => {
            let (a, b) = (common::dump(&ll, &ll_numbered), common::dump(&bc, &bc_numbered));
            if a == b {
                // `spec/ll.md` §5.7: the symbols are numbered alike, in
                // the order of LLVM's module.
                let (a, b) = (order(&ll, &ll_numbered), order(&bc, &bc_numbered));
                if a == b {
                    return Outcome::Agree;
                }
                return Outcome::Differ(vec!["symbol order".to_string()], a, b);
            }
            Outcome::Differ(classes(&a, &b), a, b)
        }
        (Err(_), Err(_)) => Outcome::BothRefuse,
        (Err(_), Ok(_)) if !common::reassembles(major, &canonical) => {
            Outcome::Tool("llvm-dis: text llvm-as does not take back".into())
        }
        (Err(error), Ok(_)) => Outcome::Differ(
            vec![format!("only hod_ll refuses: {}", head(&error))],
            error,
            String::new(),
        ),
        (Ok(_), Err(error)) => Outcome::Differ(
            vec![format!("only hod_bc refuses: {}", head(&error))],
            String::new(),
            error,
        ),
    }
}

/// The symbols' names in `SymId` order, one per line; an anonymous one
/// by its place among the numbered globals.
fn order(module: &hod_ir::Module, numbered: &[hod_ir::SymId]) -> String {
    let line = |symbol: hod_ir::Symbol<'_>| {
        if symbol.is_anonymous() {
            let at = numbered.iter().position(|&other| other == symbol.id());
            return format!("<anonymous {at:?}>\n");
        }
        format!("{}\n", String::from_utf8_lossy(symbol.name()))
    };
    module.symbols().map(line).collect()
}

/// An error's message without its position.
fn head(error: &str) -> String {
    let message = error.split_once("): ").map_or(error, |(_, rest)| rest);
    let message = message
        .split_once(": ")
        .map_or(message, |(first, rest)| if first.starts_with("bit ") { rest } else { message });
    message.chars().take(90).collect()
}

/// The classes of difference between two dumps: for a symbol both have,
/// the fields that differ; otherwise the kind of line one alone has.
fn classes(ll: &str, bc: &str) -> Vec<String> {
    let key = |line: &str| line.split(' ').next().unwrap_or("").to_string();
    let mut ll_lines: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    let mut bc_lines: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for line in ll.lines() {
        ll_lines.entry(key(line)).or_default().push(line);
    }
    for line in bc.lines() {
        bc_lines.entry(key(line)).or_default().push(line);
    }
    let mut out = Vec::new();
    let keys: std::collections::BTreeSet<&String> =
        ll_lines.keys().chain(bc_lines.keys()).collect();
    for key in keys {
        let a = ll_lines.get(key).cloned().unwrap_or_default();
        let b = bc_lines.get(key).cloned().unwrap_or_default();
        if a == b {
            continue;
        }
        match key.as_str() {
            "unsupported" => {
                for line in a.iter().filter(|line| !b.contains(line)) {
                    out.push(format!("note only from hod_ll: {}", note_kind(line)));
                }
                for line in b.iter().filter(|line| !a.contains(line)) {
                    out.push(format!("note only from hod_bc: {}", note_kind(line)));
                }
            }
            "props" | "flag" | "ctor" | "dtor" | "linker" | "library" | "module" | "target" => {
                out.push(format!("{key} differs"));
            }
            _ if a.len() == 1 && b.len() == 1 => {
                let (fa, fb): (Vec<&str>, Vec<&str>) =
                    (a[0].split(' ').collect(), b[0].split(' ').collect());
                let field = fa
                    .iter()
                    .zip(&fb)
                    .find(|(x, y)| x != y)
                    .map_or("length".to_string(), |(x, _)| {
                        x.split('=').next().unwrap_or(x).chars().take(24).collect()
                    });
                out.push(format!("symbol field {field}"));
            }
            _ if a.is_empty() => out.push("symbol only from hod_bc".into()),
            _ if b.is_empty() => out.push("symbol only from hod_ll".into()),
            _ => out.push("symbol lines differ".into()),
        }
    }
    out.sort();
    out.dedup();
    out
}

fn note_kind(line: &str) -> String {
    let construct = line.split(' ').nth(1).unwrap_or("");
    let construct = construct.split(':').next().unwrap_or(construct);
    let symbol = if line.ends_with(" None") { "module" } else { "symbol" };
    format!("{construct} on a {symbol}")
}

#[test]
fn bitcode_and_text_deliver_the_same_module() {
    let Some(root) = std::env::var_os("HOD_BC_CORPUS") else {
        eprintln!("skipped: HOD_BC_CORPUS is not set");
        return;
    };
    let every: usize =
        std::env::var("HOD_BC_EVERY").ok().and_then(|n| n.parse().ok()).unwrap_or(1).max(1);
    let diffs = std::env::var_os("HOD_BC_DIFFS").map(PathBuf::from);
    let paths: Vec<PathBuf> = common::modules(Path::new(&root))
        .into_iter()
        .enumerate()
        .filter(|(i, _)| i % every == 0)
        .map(|(_, p)| p)
        .collect();
    assert!(!paths.is_empty(), "no .ll files under {}", Path::new(&root).display());
    if let Some(dir) = &diffs {
        std::fs::create_dir_all(dir).expect("the HOD_BC_DIFFS directory");
    }
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<(usize, u32, Outcome)>> = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..WORKERS {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(path) = paths.get(index) else { return };
                    let Some(major) = common::major_of(path) else { continue };
                    let outcome = compare(path, major);
                    results.lock().expect("results").push((index, major, outcome));
                }
            });
        }
    });
    let mut results = results.into_inner().expect("results");
    results.sort_by_key(|&(index, ..)| index);
    let mut tallies: BTreeMap<u32, Tally> = BTreeMap::new();
    let mut classes: BTreeMap<(u32, String), (usize, String)> = BTreeMap::new();
    let mut tools: BTreeMap<String, usize> = BTreeMap::new();
    for (index, major, outcome) in results {
        let tally = tallies.entry(major).or_default();
        tally.modules += 1;
        let path = &paths[index];
        match outcome {
            Outcome::Agree => tally.agree += 1,
            Outcome::BothRefuse => tally.both_refuse += 1,
            Outcome::Tool(error) => {
                tally.tool_failures += 1;
                *tools.entry(error).or_default() += 1;
            }
            Outcome::Differ(found, a, b) => {
                tally.differ += 1;
                for class in found {
                    let entry =
                        classes.entry((major, class)).or_insert((0, path.display().to_string()));
                    entry.0 += 1;
                }
                if let Some(dir) = &diffs {
                    let stem = format!("{index:05}-{major}");
                    let _ = std::fs::write(dir.join(format!("{stem}.ll.txt")), a);
                    let _ = std::fs::write(dir.join(format!("{stem}.bc.txt")), b);
                    let _ = std::fs::write(
                        dir.join(format!("{stem}.path")),
                        path.display().to_string(),
                    );
                }
            }
        }
    }
    eprintln!("| LLVM | modules | agree | both refuse | differ | tool failures |");
    eprintln!("|---|---|---|---|---|---|");
    for (major, t) in &tallies {
        eprintln!(
            "| {major} | {} | {} | {} | {} | {} |",
            t.modules, t.agree, t.both_refuse, t.differ, t.tool_failures
        );
    }
    for ((major, class), (count, example)) in &classes {
        eprintln!("  llvm{major} {count:5} {class}  (e.g. {example})");
    }
    for (error, count) in &tools {
        eprintln!("  tool {count:5} {error}");
    }
    let differ: usize = tallies.values().map(|t| t.differ).sum();
    assert_eq!(differ, 0, "{differ} modules differ");
}
