//! Shared support of the function-DIE tests (`dies.rs`,
//! `dies_debugger.rs`, `dies_throughput.rs`): the sources in
//! `tests/data/dies`, their `-g` IR by clang and kiln++, hod's and
//! `llc`'s objects of it, and the DIE trees of an object as
//! `llvm-dwarfdump` prints them, with what `spec/dwarf.md` §8.7 lets
//! differ masked.

#![allow(dead_code, reason = "each test binary uses part of the support")]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hod_cg::Options;
use hod_testutil::{Scratch, run_with, tools_required};

use crate::common::parse;

/// The directory of the test sources.
pub fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/dies")
}

/// Whether a file name has the extension `ext`.
pub fn has_ext(name: &str, ext: &str) -> bool {
    Path::new(name).extension().is_some_and(|e| e == ext)
}

/// A test source's text.
pub fn data(name: &str) -> String {
    std::fs::read_to_string(data_dir().join(name)).unwrap()
}

fn output(command: &mut Command, timeout: Duration) -> String {
    let outcome = run_with(command, &[], timeout);
    assert!(outcome.success(), "{command:?}: {}", outcome.stderr_text());
    outcome.stdout_text()
}

/// `text` without `, templateParams: !N`: template parameters are
/// WP-059's, so neither writer is given them.
pub fn strip_template_params(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(", templateParams: !") {
        out.push_str(&rest[..at]);
        let tail = &rest[at + 19..];
        let digits = tail.bytes().take_while(u8::is_ascii_digit).count();
        rest = &tail[digits..];
    }
    out.push_str(rest);
    out
}

/// `-g -O0` IR of the test source `source` by `compiler` (a clang or
/// kiln++), without template parameters. A C source given to kiln++ is
/// compiled as C++.
pub fn ir(compiler: &Path, scratch: &Scratch, name: &str, source: &str) -> String {
    let mut input = data_dir().join(source);
    let is_kiln = compiler.file_name().is_some_and(|n| n.to_string_lossy().starts_with("kiln"));
    if is_kiln && has_ext(source, "c") {
        input = scratch.write(&format!("{name}.cpp"), data(source));
    }
    let out = scratch.join(format!("{name}.ll"));
    let mut command = Command::new(compiler);
    command.args(["-g", "-O0", "-S", "-emit-llvm", "-w"]);
    if !has_ext(source, "c") || is_kiln {
        command.arg("-std=c++20");
    }
    command.arg(&input).arg("-o").arg(&out);
    output(&mut command, Duration::from_mins(2));
    strip_template_params(&std::fs::read_to_string(out).unwrap())
}

/// hod's object of `text`.
pub fn hod_object(scratch: &Scratch, name: &str, text: &str) -> PathBuf {
    let (module, bodies) = parse(text);
    let bytes = hod_a64::compile_to_object(&module, &bodies, &Options::default())
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    scratch.write(&format!("{name}.o"), bytes)
}

/// `llc`'s object of `text` (`-O0 -global-isel=0 -fast-isel=0`).
pub fn llc_object(scratch: &Scratch, llc: &Path, name: &str, text: &str) -> PathBuf {
    let input = scratch.write(&format!("{name}.ll"), text);
    let out = scratch.join(format!("{name}.o"));
    let mut command = Command::new(llc);
    command
        .args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj"])
        .arg(&input)
        .arg("-o")
        .arg(&out);
    output(&mut command, Duration::from_mins(2));
    out
}

/// `llvm-dwarfdump --verify` of `object` is clean.
pub fn verify(dwarfdump: &Path, object: &Path) {
    let mut command = Command::new(dwarfdump);
    command.arg("--verify").arg(object);
    let out = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(
        out.success() && out.stdout_text().contains("No errors."),
        "llvm-dwarfdump --verify {}:\n{}{}",
        object.display(),
        out.stdout_text(),
        out.stderr_text()
    );
}

/// kiln++, beside the build directory or from `HOD_KILN`; `None` (a
/// failure under `HOD_REQUIRE_TOOLS=1`) when there is none.
pub fn kiln() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("HOD_KILN").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(path));
    }
    let beside = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .ancestors()
        .nth(2)
        .map(|root| root.join("target-kiln").join("release").join("kiln++"));
    match beside {
        Some(path) if path.is_file() => Some(path),
        _ => {
            assert!(!tools_required(), "no kiln++ found; set HOD_KILN (HOD_REQUIRE_TOOLS=1)");
            eprintln!("note: no kiln++ (set HOD_KILN); skipping its part");
            None
        }
    }
}

// ------------------------------------------------------------ trees ----

/// A DIE as `llvm-dwarfdump --debug-info` prints it, normalized.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub tag: String,
    pub attrs: Vec<String>,
    pub children: Vec<Node>,
}

/// The attributes whose values §8.7 lets differ: addresses, code sizes,
/// locations (compared by behaviour, under the debuggers), the frame
/// base (`DW_OP_reg29` where `llc` may use `DW_OP_reg31`), the bases and
/// offsets of other sections, and references without a name.
const MASKED: [&str; 11] = [
    "DW_AT_low_pc",
    "DW_AT_high_pc",
    "DW_AT_location",
    "DW_AT_frame_base",
    "DW_AT_ranges",
    "DW_AT_addr_base",
    "DW_AT_rnglists_base",
    "DW_AT_str_offsets_base",
    "DW_AT_loclists_base",
    "DW_AT_stmt_list",
    "DW_AT_object_pointer",
];

/// An attribute line, normalized: a masked value dropped, a reference's
/// offset dropped (its target's name stays), a constant's radix made
/// decimal.
fn normalize(line: &str) -> String {
    let (name, value) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
    if MASKED.contains(&name) {
        return name.to_owned();
    }
    let value = value.trim();
    let mut v = value.to_owned();
    if let Some(rest) = value.strip_prefix("(0x")
        && let Some(q) = rest.find(" \"")
    {
        v = format!("({}", &rest[q + 1..]);
    }
    if let Some(num) = value.strip_prefix('(').and_then(|r| r.strip_suffix(')')) {
        let parsed = num
            .strip_prefix("0x")
            .map_or_else(|| num.parse::<u64>().ok(), |h| u64::from_str_radix(h, 16).ok());
        if let Some(n) = parsed {
            v = format!("({n})");
        }
    }
    format!("{name} {v}")
}

/// The DIE trees of the units of an object.
pub fn trees(dwarfdump: &Path, object: &Path) -> Vec<Node> {
    let mut command = Command::new(dwarfdump);
    command.arg("--debug-info").arg(object);
    parse_tree(&output(&mut command, Duration::from_mins(2)))
}

/// The DIE trees of `--debug-info` output.
pub fn parse_tree(text: &str) -> Vec<Node> {
    let mut units: Vec<Node> = Vec::new();
    let mut open: Vec<Node> = Vec::new();
    let close_to = |open: &mut Vec<Node>, units: &mut Vec<Node>, depth: usize| {
        while open.len() > depth {
            let done = open.pop().unwrap();
            match open.last_mut() {
                Some(parent) => parent.children.push(done),
                None => units.push(done),
            }
        }
    };
    for line in text.lines() {
        let t = line.trim_start();
        if line.starts_with("0x") && line.contains("Compile Unit:") {
            close_to(&mut open, &mut units, 0);
            continue;
        }
        if line.starts_with("0x") {
            let (_, rest) = line.split_once(':').unwrap();
            let spaces = rest.len() - rest.trim_start().len();
            let depth = (spaces - 1) / 2;
            let what = rest.trim();
            close_to(&mut open, &mut units, depth);
            if what != "NULL" {
                open.push(Node { tag: what.to_owned(), attrs: Vec::new(), children: Vec::new() });
            }
        } else if t.starts_with("DW_AT_rnglists_base") {
            // hod's blocks name their range lists by `DW_FORM_sec_offset`,
            // `llc`'s by `DW_FORM_rnglistx` (§8.7): `llc`'s unit has a
            // base where hod's need not.
        } else if t.starts_with("DW_AT_") {
            open.last_mut().unwrap().attrs.push(normalize(t));
        }
        // Anything else continues a masked attribute (a range list).
    }
    close_to(&mut open, &mut units, 0);
    units
}

pub fn count(n: &Node) -> usize {
    1 + n.children.iter().map(count).sum::<usize>()
}

/// The DIEs of a tree with a tag.
pub fn count_tag(n: &Node, tag: &str) -> usize {
    usize::from(n.tag == tag) + n.children.iter().map(|c| count_tag(c, tag)).sum::<usize>()
}

pub fn render(n: &Node, depth: usize, out: &mut String) {
    use std::fmt::Write;
    writeln!(out, "{}{}", "  ".repeat(depth), n.tag).unwrap();
    for a in &n.attrs {
        writeln!(out, "{}  {a}", "  ".repeat(depth)).unwrap();
    }
    for c in &n.children {
        render(c, depth + 1, out);
    }
}

/// The first place two trees differ, as the path of tags to it and the
/// two renderings there; `None` when they are equal.
pub fn first_difference(a: &Node, b: &Node) -> Option<String> {
    if a.tag != b.tag || a.attrs != b.attrs || a.children.len() != b.children.len() {
        let common = a.children.len().min(b.children.len());
        if a.tag == b.tag && a.attrs == b.attrs {
            for i in 0..common {
                if a.children[i] != b.children[i] {
                    return first_difference(&a.children[i], &b.children[i])
                        .map(|d| format!("{} > {d}", a.tag));
                }
            }
        }
        let (mut ra, mut rb) = (String::new(), String::new());
        render(a, 0, &mut ra);
        render(b, 0, &mut rb);
        return Some(format!("{}:\n--- one\n{ra}--- other\n{rb}", a.tag));
    }
    a.children
        .iter()
        .zip(&b.children)
        .find_map(|(x, y)| first_difference(x, y).map(|d| format!("{} > {d}", a.tag)))
}
