//! Shared support of the line-table tests (`lines.rs`,
//! `lines_debugger.rs`): the sources in `tests/data/lines`, the line
//! tables of an object as `llvm-dwarfdump` reads them, per function, and
//! the comparison with `llc`'s of `spec/dwarf.md` §5.7.

#![allow(dead_code, reason = "each test binary uses part of the support")]

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hod_testutil::run_with;

/// The directory of the test sources.
pub fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/lines")
}

/// A test source's text.
pub fn data(name: &str) -> String {
    std::fs::read_to_string(data_dir().join(name)).unwrap()
}

/// The compile directory the `.ll` files name (`-fdebug-prefix-map`).
pub const COMP_DIR: &str = "/hod/tests/lines";

/// `text` with its target lines replaced by those of LLVM `major`.
pub fn with_header(text: &str, major: u32) -> String {
    let body: String = text
        .lines()
        .filter(|l| !l.starts_with("target datalayout") && !l.starts_with("target triple"))
        .flat_map(|l| [l, "\n"])
        .collect();
    format!("{}{body}", crate::common::ll_header(major))
}

/// One row of a line table, without its address.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DRow {
    pub address: u64,
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub stmt: bool,
    pub prologue_end: bool,
    pub epilogue_begin: bool,
}

impl DRow {
    /// The row's (file, line, column).
    pub fn loc(&self) -> (&str, u32, u32) {
        (&self.file, self.line, self.column)
    }
}

fn output(command: &mut Command) -> String {
    let outcome = run_with(command, &[], Duration::from_mins(1));
    assert!(outcome.success(), "{command:?}: {}", outcome.stderr_text());
    outcome.stdout_text()
}

/// The rows of every line table of `object` (end-of-sequence rows
/// dropped), their file named `dir/name` from the table's header.
pub fn rows(dwarfdump: &Path, object: &Path) -> Vec<DRow> {
    let text = output(Command::new(dwarfdump).arg("--debug-line").arg(object));
    let mut dirs: Vec<String> = Vec::new();
    let mut files: Vec<String> = Vec::new();
    let mut name = String::new();
    let mut out = Vec::new();
    let quoted = |l: &str| l.split('"').nth(1).unwrap_or("").to_owned();
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("debug_line[") {
            dirs.clear();
            files.clear();
        } else if t.starts_with("include_directories[") {
            dirs.push(quoted(t));
        } else if let Some(n) = t.strip_prefix("name: ") {
            name = quoted(n);
        } else if let Some(d) = t.strip_prefix("dir_index: ") {
            let d: usize = d.trim().parse().unwrap();
            files.push(format!("{}/{name}", dirs.get(d).map_or("", String::as_str)));
        } else if t.starts_with("0x") {
            let f: Vec<&str> = t.split_whitespace().collect();
            if f.contains(&"end_sequence") {
                continue;
            }
            let file: usize = f[3].parse().unwrap();
            out.push(DRow {
                address: u64::from_str_radix(&f[0][2..], 16).unwrap(),
                file: files.get(file).cloned().unwrap_or_else(|| format!("#{file}")),
                line: f[1].parse().unwrap(),
                column: f[2].parse().unwrap(),
                stmt: f.contains(&"is_stmt"),
                prologue_end: f.contains(&"prologue_end"),
                epilogue_begin: f.contains(&"epilogue_begin"),
            });
        }
    }
    out
}

/// The defined functions of an object with one code section: name,
/// address, size (`llvm-nm -S`).
pub fn functions(nm: &Path, object: &Path) -> Vec<(String, u64, u64)> {
    let text = output(Command::new(nm).args(["-S", "--defined-only"]).arg(object));
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            (f.len() == 4 && matches!(f[2], "T" | "t" | "W")).then(|| {
                (
                    f[3].to_owned(),
                    u64::from_str_radix(f[0], 16).unwrap(),
                    u64::from_str_radix(f[1], 16).unwrap(),
                )
            })
        })
        .collect()
}

/// The rows of each function of an object whose code is in one
/// section, by name.
pub fn rows_by_function(dwarfdump: &Path, nm: &Path, object: &Path) -> BTreeMap<String, Vec<DRow>> {
    let all = rows(dwarfdump, object);
    let mut out = BTreeMap::new();
    for (name, start, size) in functions(nm, object) {
        let rows: Vec<DRow> = all
            .iter()
            .filter(|r| r.address >= start && r.address < start + size)
            .cloned()
            .collect();
        if !rows.is_empty() {
            out.insert(name, rows);
        }
    }
    out
}

/// The (line, column) of every `DILocation` an unconditional branch of
/// `ll` carries: where `llc` may have a row hod has not, for the
/// branches hod falls through (§5.7, P1).
pub fn branch_locations(ll: &str) -> BTreeSet<(u32, u32)> {
    let mut locs: HashMap<&str, (u32, u32)> = HashMap::new();
    for l in ll.lines() {
        let Some((id, rest)) = l.split_once(" = !DILocation(") else { continue };
        let field = |name: &str| -> u32 {
            rest.split(name)
                .nth(1)
                .and_then(|r| r.split([',', ')']).next())
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(0)
        };
        locs.insert(id.trim(), (field("line:"), field("column:")));
    }
    ll.lines()
        .filter_map(|l| {
            let (inst, id) = l.rsplit_once(", !dbg ")?;
            inst.trim_start().starts_with("br label ").then(|| locs.get(id.trim()).copied())?
        })
        .collect()
}

/// A table's rows as compared (§5.7): line-0 rows and, for `llc`'s, the
/// rows of branch-only locations dropped, then consecutive rows of one
/// location merged (their flags or-ed). Each row says whether a row was
/// dropped right before it (its `is_stmt` then follows a row the other
/// table may not have).
fn normalized(rows: &[DRow], drop: &BTreeSet<(u32, u32)>) -> Vec<(DRow, bool)> {
    let mut out: Vec<(DRow, bool)> = Vec::new();
    let mut dropped = false;
    for r in rows {
        if r.line == 0 || drop.contains(&(r.line, r.column)) {
            dropped = true;
            continue;
        }
        if let Some((last, _)) = out.last_mut()
            && last.loc() == r.loc()
        {
            last.prologue_end |= r.prologue_end;
            last.epilogue_begin |= r.epilogue_begin;
            dropped = false;
            continue;
        }
        out.push((r.clone(), dropped));
        dropped = false;
    }
    out
}

/// Compares hod's rows of one function with `llc`'s, modulo addresses
/// and the permitted differences of `spec/dwarf.md` §5.7: `llc`'s rows
/// of the unconditional branches hod falls through (P1, at a location of
/// `branches`), `llc`'s line-0 rows at the starts of its own machine
/// blocks (P2), hod's start row and epilogue where `llc`'s function has
/// no frame (P3), and the line-0 row of hod's shared epilogue. Returns
/// what differs.
pub fn compare(hod: &[DRow], llc: &[DRow], branches: &BTreeSet<(u32, u32)>) -> Option<String> {
    let mut h = normalized(hod, &BTreeSet::new());
    let l = normalized(llc, &BTreeSet::new());
    // P3: hod's start row at the scope line, column 0, where `llc`'s
    // first row is another.
    if h.len() > 1 && h[0].0.column == 0 && l.first().is_none_or(|r| r.0.loc() != h[0].0.loc()) {
        h.remove(0);
    }
    let show = |v: &[(DRow, bool)]| -> Vec<String> {
        v.iter().map(|(r, _)| format!("{}:{}:{}", r.file, r.line, r.column)).collect()
    };
    let differ = || Some(format!("locations differ:\n hod {:?}\n llc {:?}", show(&h), show(&l)));
    // Align: every hod row is the next `llc` row, but for `llc`'s rows
    // of branches (and a row that repeats the last location after one).
    let (mut i, mut skipped) = (0, false);
    let mut last: Option<(&str, u32, u32)> = None;
    for (b, bd) in &l {
        if let Some((a, ad)) = h.get(i)
            && a.loc() == b.loc()
        {
            if !ad && !bd && !skipped && a.stmt != b.stmt {
                return Some(format!("is_stmt differs at {a:?} / {b:?}"));
            }
            (i, skipped, last) = (i + 1, false, Some(b.loc()));
        } else if branches.contains(&(b.line, b.column)) || last == Some(b.loc()) {
            skipped = true;
        } else {
            return differ();
        }
    }
    if i != h.len() {
        return differ();
    }
    // The end of the prologue at the same location, where `llc` has one.
    let first = |v: &[(DRow, bool)], f: fn(&DRow) -> bool| {
        v.iter().find(|r| f(&r.0)).map(|r| r.0.loc().to_owned_loc())
    };
    let (hp, lp) = (first(&h, |r| r.prologue_end), first(&l, |r| r.prologue_end));
    if lp.is_some() && hp != lp {
        return Some(format!("prologue_end at {hp:?}, llc's at {lp:?}"));
    }
    // The epilogue at a location of one of `llc`'s, where `llc` has one.
    let epilogues: BTreeSet<_> =
        l.iter().filter(|r| r.0.epilogue_begin).map(|r| r.0.loc().to_owned_loc()).collect();
    if !epilogues.is_empty() {
        for (a, _) in h.iter().filter(|r| r.0.epilogue_begin) {
            if !epilogues.contains(&a.loc().to_owned_loc()) {
                return Some(format!("epilogue_begin at {a:?}, llc's at {epilogues:?}"));
            }
        }
    }
    // hod's line-0 rows, but for its shared epilogue's, are rows `llc`
    // writes too.
    let zeros = |v: &[DRow]| -> BTreeSet<(String, u32)> {
        v.iter()
            .filter(|r| r.line == 0 && !r.epilogue_begin)
            .map(|r| (r.file.clone(), r.column))
            .collect()
    };
    let (hz, lz) = (zeros(hod), zeros(llc));
    if !hz.is_subset(&lz) {
        return Some(format!("line-0 rows {hz:?} not among llc's {lz:?}"));
    }
    None
}

trait OwnedLoc {
    fn to_owned_loc(&self) -> (String, u32, u32);
}

impl OwnedLoc for (&str, u32, u32) {
    fn to_owned_loc(&self) -> (String, u32, u32) {
        (self.0.to_owned(), self.1, self.2)
    }
}
