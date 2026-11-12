//! `spec/bc.md` §11.9: the bodies `hod_bc` reads from bitcode are the
//! bodies `hod_ll` reads from `llvm-dis` of the same bitcode, compared as
//! `common::body_dump` (every instruction, operands written out), with
//! the names that make a body unsupported and the module's dump after.
//!
//! `handwritten_*` run always, on `tests/data/bodies.ll` with every LLVM
//! installed. The corpus run reads `HOD_BC_CORPUS` and skips when it is
//! not set, as `differential.rs` does (`HOD_BC_EVERY`, `HOD_BC_DIFFS`):
//!
//! ```text
//! HOD_BC_CORPUS=.build/corpus cargo test --release -p hod_bc --test bodies -- --nocapture
//! ```

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

const WORKERS: usize = 8;
const MAJORS: [u32; 2] = [19, 22];

enum Outcome {
    Agree,
    BothRefuse(String),
    Tool(String),
    Differ(String, String, String),
}

/// The kind of a refusal: the text before the first `:`.
fn kind(error: &str) -> String {
    error.split(':').next().unwrap_or(error).to_string()
}

fn compare(text: &[u8], major: u32) -> Outcome {
    let bitcode = match common::assemble(major, text) {
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
    match (common::read_text_bodies(&canonical), common::read_bitcode_bodies(&bitcode)) {
        (Ok(a), Ok(b)) if a == b => Outcome::Agree,
        (Ok(a), Ok(b)) => Outcome::Differ(first_difference(&a, &b), a, b),
        (Err(a), Err(b)) => Outcome::BothRefuse(format!("{} / {}", kind(&a), kind(&b))),
        (Err(a), Ok(b)) => Outcome::Differ(format!("only hod_ll refuses: {}", head(&a)), a, b),
        (Ok(a), Err(b)) => Outcome::Differ(format!("only hod_bc refuses: {}", head(&b)), a, b),
    }
}

fn head(error: &str) -> String {
    error.chars().take(110).collect()
}

/// The class of the first line that differs: its instruction opcode, or
/// the kind of line.
fn first_difference(a: &str, b: &str) -> String {
    let (la, lb): (Vec<&str>, Vec<&str>) = (a.lines().collect(), b.lines().collect());
    let at = la.iter().zip(&lb).position(|(x, y)| x != y).unwrap_or(la.len().min(lb.len()));
    let line = la.get(at).or(lb.get(at)).copied().unwrap_or("");
    let words: Vec<&str> = line.split_whitespace().collect();
    match words.as_slice() {
        ["define", _, "unsupported", ..] => "unsupported names".into(),
        ["define", ..] => "function list".into(),
        ["slots", ..] => "slots".into(),
        [inst, opcode, ..] if inst.starts_with('%') => format!("inst {opcode}"),
        ["unsupported", construct, ..] => {
            format!("note {}", construct.split(':').next().unwrap_or(construct))
        }
        [word, ..] => format!("line {word}"),
        [] => "length".into(),
    }
}

fn handwritten() -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/bodies.ll"))
        .expect("tests/data/bodies.ll")
}

#[test]
fn handwritten_bodies_agree_on_every_major() {
    let text = handwritten();
    for major in MAJORS {
        if common::llvm_bin(major).is_none() {
            eprintln!("skipped LLVM {major}: not installed");
            continue;
        }
        match compare(&text, major) {
            Outcome::Agree => {}
            Outcome::Differ(class, a, b) => {
                panic!("LLVM {major}: {class}\n--- hod_ll\n{a}\n--- hod_bc\n{b}")
            }
            Outcome::BothRefuse(kinds) => panic!("LLVM {major}: both refuse: {kinds}"),
            Outcome::Tool(error) => panic!("LLVM {major}: {error}"),
        }
    }
}

/// The handwritten bodies are delivered (all but those meant to be
/// unsupported), and the unsupported ones name what they should.
#[test]
fn handwritten_bodies_are_delivered() {
    let text = handwritten();
    for major in MAJORS {
        let Ok(bitcode) = common::assemble(major, &text) else { continue };
        let dump = common::read_bitcode_bodies(&bitcode).expect("hod_bc reads the bodies");
        let unsupported: Vec<&str> =
            dump.lines().filter(|line| line.starts_with("define ") && line.contains(" unsupported ")).collect();
        assert_eq!(
            unsupported,
            [
                "define funclets unsupported [\"alloca:inalloca\", \"inst:catchpad\", \
                 \"inst:catchret\", \"inst:catchswitch\"]",
                "define musttail_va unsupported [\"call:forwarded-varargs\"]",
                "define scopes unsupported [\"syncscope:agent\"]",
            ],
            "LLVM {major}"
        );
        assert!(dump.contains("unsupported global-metadata:custom Some(\"with_md\")"), "LLVM {major}");
        assert!(!dump.contains("global-metadata:prof"), "LLVM {major}");
    }
}

/// Bodies read on several threads, each with its own clone of the module
/// and its own reader, are the bodies read on one.
#[test]
fn bodies_read_in_parallel_are_the_same() {
    let text = handwritten();
    let Ok(bitcode) = common::assemble(MAJORS[1], &text).or_else(|_| common::assemble(MAJORS[0], &text))
    else {
        eprintln!("skipped: no llvm-as");
        return;
    };
    let read = hod_bc::read_module(&bitcode, &hod_bc::ReadOptions::default()).expect("module");
    let numbered = read.bits.numbered_globals().to_vec();
    let one = |module: &mut hod_ir::Module, reader: &mut hod_bc::BodyReader<'_, '_>, index: usize| {
        let function = &read.bits.functions()[index];
        let body = reader.read(module, function).expect("body");
        body.map(|body| common::body_dump(module, &numbered, &body))
    };
    let mut module = read.module.clone();
    let mut reader = hod_bc::BodyReader::new(&read.bits);
    let count = read.bits.functions().len();
    let sequential: Vec<Option<String>> = (0..count).map(|i| one(&mut module, &mut reader, i)).collect();
    let parallel: Mutex<Vec<(usize, Option<String>)>> = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for worker in 0..3 {
            let (base, bits, parallel, one) = (&read.module, &read.bits, &parallel, &one);
            scope.spawn(move || {
                let mut module = base.clone();
                let mut reader = hod_bc::BodyReader::new(bits);
                // Each worker its own share, in reverse order.
                for index in (0..count).rev().filter(|i| i % 3 == worker) {
                    let dump = one(&mut module, &mut reader, index);
                    parallel.lock().expect("results").push((index, dump));
                }
            });
        }
    });
    let mut parallel = parallel.into_inner().expect("results");
    parallel.sort_by_key(|&(index, _)| index);
    let parallel: Vec<Option<String>> = parallel.into_iter().map(|(_, dump)| dump).collect();
    assert_eq!(sequential, parallel);
}

#[test]
fn corpus_bodies_agree() {
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
                    let outcome = match std::fs::read(path) {
                        Ok(text) => compare(&text, major),
                        Err(error) => Outcome::Tool(format!("read: {error}")),
                    };
                    results.lock().expect("results").push((index, major, outcome));
                }
            });
        }
    });
    let mut results = results.into_inner().expect("results");
    results.sort_by_key(|&(index, ..)| index);
    // Per major: modules, agree, both refuse, differ, tool failures.
    let mut tallies: BTreeMap<u32, [usize; 5]> = BTreeMap::new();
    let mut classes: BTreeMap<(u32, String), (usize, String)> = BTreeMap::new();
    for (index, major, outcome) in results {
        let tally = tallies.entry(major).or_default();
        tally[0] += 1;
        let path = paths[index].display().to_string();
        let (slot, class) = match outcome {
            Outcome::Agree => (1, None),
            Outcome::BothRefuse(kinds) => (2, Some(format!("both refuse {kinds}"))),
            Outcome::Tool(error) => (4, Some(format!("tool {error}"))),
            Outcome::Differ(class, a, b) => {
                if let Some(dir) = &diffs {
                    let stem = format!("{index:05}-{major}");
                    let _ = std::fs::write(dir.join(format!("{stem}.ll.txt")), a);
                    let _ = std::fs::write(dir.join(format!("{stem}.bc.txt")), b);
                    let _ = std::fs::write(dir.join(format!("{stem}.path")), &path);
                }
                (3, Some(class))
            }
        };
        tally[slot] += 1;
        if let Some(class) = class {
            classes.entry((major, class)).or_insert((0, path)).0 += 1;
        }
    }
    eprintln!("| LLVM | modules | agree | both refuse | differ | tool failures |");
    eprintln!("|---|---|---|---|---|---|");
    for (major, t) in &tallies {
        eprintln!("| {major} | {} | {} | {} | {} | {} |", t[0], t[1], t[2], t[3], t[4]);
    }
    for ((major, class), (count, example)) in &classes {
        eprintln!("  llvm{major} {count:5} {class}  (e.g. {example})");
    }
    let differ: usize = tallies.values().map(|t| t[3]).sum();
    assert_eq!(differ, 0, "{differ} modules differ");
}
