//! Shared support of the function-DIE tests (`dies.rs`,
//! `dies_debugger.rs`, `dies_throughput.rs`): the sources in
//! `tests/data/dies`, their `-g` IR by clang and kiln++, hod's and
//! `llc`'s objects of it, and the DIE trees of an object as
//! `llvm-dwarfdump` prints them, with what `spec/dwarf.md` §8.7 lets
//! differ masked.

#![allow(dead_code, reason = "each test binary uses part of the support")]

use std::borrow::Cow;
use std::collections::HashMap;
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

/// The references the dump prints as a bare offset when their target
/// has no name.
const REFERENCES: [&str; 7] = [
    "DW_AT_abstract_origin",
    "DW_AT_type",
    "DW_AT_specification",
    "DW_AT_containing_type",
    "DW_AT_import",
    "DW_AT_friend",
    "DW_AT_sibling",
];

/// A bare reference (`(0x0000abcd)`, a target without a name) named by
/// its target's tag, which `tag_at` gives by offset: `spec/dwarf.md`
/// §6.7, "the offset of a reference" (the offset moves with the sizes
/// of the DIEs before it; the target is the same DIE). Any other line
/// unchanged.
fn name_bare_reference<'t>(line: &'t str, tag_at: &HashMap<u64, &str>) -> Cow<'t, str> {
    let (name, value) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
    let hex = value.trim().strip_prefix("(0x").and_then(|h| h.strip_suffix(')'));
    if REFERENCES.contains(&name)
        && let Some(hex) = hex
        && hex.len() == 8
        && let Ok(offset) = u64::from_str_radix(hex, 16)
        && let Some(tag) = tag_at.get(&offset)
    {
        return Cow::Owned(format!("{name} (0x{hex} \"{tag}\")"));
    }
    Cow::Borrowed(line)
}

/// The DIE trees of `--debug-info` output.
pub fn parse_tree(text: &str) -> Vec<Node> {
    let mut tag_at: HashMap<u64, &str> = HashMap::new();
    for line in text.lines() {
        if let Some((off, rest)) = line.split_once(':')
            && let Some(hex) = off.strip_prefix("0x")
            && !rest.contains("Compile Unit:")
            && let Ok(o) = u64::from_str_radix(hex, 16)
        {
            tag_at.insert(o, rest.trim());
        }
    }
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
            open.last_mut().unwrap().attrs.push(normalize(&name_bare_reference(t, &tag_at)));
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

/// Whether two DIEs' attributes are equal: as written, or (WP-098) a
/// scope that is not the unit with `DW_AT_low_pc`, `DW_AT_high_pc` in
/// one tree and `DW_AT_ranges` at the same place in the other. That is
/// `spec/dwarf.md` §6.7, "`low_pc`, `high_pc`, `ranges` and `location`
/// are masked": the scope's code is one address range in one writer's
/// code and several in the other's, the same information (where the
/// scope's code is) for code laid out differently (§6.14).
pub fn same_attrs(a: &Node, b: &Node) -> bool {
    if a.attrs == b.attrs {
        return true;
    }
    if a.tag == "DW_TAG_compile_unit" || a.tag != b.tag {
        return false;
    }
    let code = |attrs: &[String]| -> Vec<String> {
        let mut out = Vec::with_capacity(attrs.len());
        let mut i = 0;
        while i < attrs.len() {
            let pair = attrs[i] == "DW_AT_low_pc"
                && attrs.get(i + 1).is_some_and(|n| n == "DW_AT_high_pc");
            if pair || attrs[i] == "DW_AT_ranges" {
                out.push("DW_AT_low_pc, DW_AT_high_pc | DW_AT_ranges".to_owned());
                i += 1 + usize::from(pair);
            } else {
                out.push(attrs[i].clone());
                i += 1;
            }
        }
        out
    };
    code(&a.attrs) == code(&b.attrs)
}

/// Whether two trees are equal under [`same_attrs`].
pub fn same(a: &Node, b: &Node) -> bool {
    a.tag == b.tag
        && same_attrs(a, b)
        && a.children.len() == b.children.len()
        && a.children.iter().zip(&b.children).all(|(x, y)| same(x, y))
}

/// The first place two trees differ (under [`same_attrs`]), as the path
/// of tags to it and the two renderings there; `None` when they are
/// equal.
pub fn first_difference(a: &Node, b: &Node) -> Option<String> {
    let attrs = a.tag == b.tag && same_attrs(a, b);
    if !attrs || a.children.len() != b.children.len() {
        let common = a.children.len().min(b.children.len());
        if attrs {
            for i in 0..common {
                if !same(&a.children[i], &b.children[i]) {
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

// ------------------------------------------------ permitted classes ----

/// The classes of `spec/dwarf.md` §6.7 that make hod's tree `h` differ
/// from `llc`'s `l` (one unit each) beyond [`same`], or `None` when
/// something else differs too. Only the census uses this (an aid that
/// counts; `dies_decline.rs` and `dies_inline.rs` hold each class to its
/// exact shape):
/// - `sret`: `llc`'s unit has `DW_AT_loclists_base`, hod's none;
/// - `(b) is.constant block`: hod has a lexical block `llc` does not
///   (only where the module calls `llvm.is.constant`, `is_constant`);
/// - `(a) fallthrough inlined`: `llc` has an inlined subroutine hod does
///   not, and then abstract instances and types hod does not (in the
///   unit or a namespace or class), the children of a DIE compared as a
///   multiset where their order differs;
/// - `llc 19 blocks`: `llc` 19 (`llc19`) writes no
///   `DW_AT_abstract_origin` on a concrete block.
pub fn permitted(h: &Node, l: &Node, is_constant: bool, llc19: bool) -> Option<Vec<&'static str>> {
    let mut l = l.clone();
    let mut classes = Vec::new();
    if !h.attrs.iter().any(|a| a == "DW_AT_loclists_base")
        && l.attrs.iter().any(|a| a == "DW_AT_loclists_base")
    {
        l.attrs.retain(|a| a != "DW_AT_loclists_base");
        classes.push("sret");
    }
    let extras_ok =
        count_tag(&l, "DW_TAG_inlined_subroutine") > count_tag(h, "DW_TAG_inlined_subroutine");
    let mut p = Permit { classes, is_constant, extras_ok, llc19 };
    p.eq(h, &l).then_some(p.classes)
}

struct Permit {
    classes: Vec<&'static str>,
    is_constant: bool,
    extras_ok: bool,
    llc19: bool,
}

/// What `llc` writes for an inlined call hod does not have: its abstract
/// instance and the types (and member typedefs) only that instance
/// names.
fn is_extra(n: &Node) -> bool {
    (n.tag == "DW_TAG_subprogram" && n.attrs.iter().any(|a| a.starts_with("DW_AT_inline ")))
        || n.tag.ends_with("_type")
        || n.tag == "DW_TAG_typedef"
}

fn rendering(n: &Node) -> String {
    let mut s = String::new();
    render(n, 0, &mut s);
    s
}

impl Permit {
    fn note(&mut self, class: &'static str) {
        if !self.classes.contains(&class) {
            self.classes.push(class);
        }
    }

    fn eq(&mut self, h: &Node, l: &Node) -> bool {
        if h.tag != l.tag {
            return false;
        }
        if !same_attrs(h, l) {
            let origin = |a: &String| !a.starts_with("DW_AT_abstract_origin");
            let mut bare = h.clone();
            bare.attrs.retain(origin);
            if !(self.llc19 && h.tag == "DW_TAG_lexical_block" && same_attrs(&bare, l)) {
                return false;
            }
            self.note("llc 19 blocks");
        }
        let saved = self.classes.clone();
        if self.aligned(&h.children, &l.children) {
            return true;
        }
        self.classes.clone_from(&saved);
        if self.extras_ok && self.multiset(&h.children, &l.children) {
            return true;
        }
        self.classes = saved;
        false
    }

    fn aligned(&mut self, hs: &[Node], ls: &[Node]) -> bool {
        let (mut i, mut j) = (0, 0);
        while i < hs.len() || j < ls.len() {
            if i < hs.len() && j < ls.len() && self.eq(&hs[i], &ls[j]) {
                i += 1;
                j += 1;
            } else if i < hs.len() && self.is_constant && hs[i].tag == "DW_TAG_lexical_block" {
                self.note("(b) is.constant block");
                i += 1;
            } else if j < ls.len()
                && (ls[j].tag == "DW_TAG_inlined_subroutine" || self.extras_ok && is_extra(&ls[j]))
            {
                self.note("(a) fallthrough inlined");
                j += 1;
            } else {
                return false;
            }
        }
        true
    }

    /// The children as a multiset: those rendered alike pair up, the
    /// rest in order, `llc`'s extras left over.
    fn multiset(&mut self, hs: &[Node], ls: &[Node]) -> bool {
        let mut pool: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, c) in hs.iter().enumerate().rev() {
            pool.entry(rendering(c)).or_default().push(i);
        }
        let mut used = vec![false; hs.len()];
        let mut left_l = Vec::new();
        for c in ls {
            match pool.get_mut(&rendering(c)).and_then(Vec::pop) {
                Some(i) => used[i] = true,
                None => left_l.push(c),
            }
        }
        let left_h: Vec<&Node> =
            hs.iter().zip(&used).filter(|(_, u)| !**u).map(|(c, _)| c).collect();
        let (extras, left_l): (Vec<&Node>, Vec<&Node>) = left_l.into_iter().partition(|n| {
            n.tag == "DW_TAG_inlined_subroutine"
                || (is_extra(n) && !left_h.iter().any(|h| h.tag == n.tag && same_attrs(h, n)))
        });
        if !extras.is_empty() {
            self.note("(a) fallthrough inlined");
        }
        left_h.len() == left_l.len() && left_h.iter().zip(&left_l).all(|(x, y)| self.eq(x, y))
    }
}
