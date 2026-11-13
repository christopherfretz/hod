//! `cargo xtask debug trees`: hod's DIE trees against `llc`'s, module by
//! module (WP-136). Contract: `spec/harness.md` "debug", "Trees";
//! `spec/dwarf.md` §6.7 (what may differ) and §6.17 (x86-64).
//!
//! Each `-g` module is compiled by hod (strict) and by `llc` (the
//! reference, of the module's LLVM major or `--llc`'s), both objects are
//! verified (`llvm-dwarfdump --verify`), and their `.debug_info` is
//! compared:
//!
//! - the DIE trees, as `llvm-dwarfdump --debug-info` prints them,
//!   normalized: addresses, code sizes, the bases and offsets of other
//!   sections and the offsets of references are masked (§6.7); a
//!   location is compared by its operations without their operands
//!   (`DW_OP_fbreg`; a location list is `list`), because the frame
//!   offsets are each writer's own; on AArch64 the frame base is masked
//!   (`DW_OP_reg29` where `llc` may write `DW_OP_reg31`), on x86-64 it is
//!   compared (`DW_OP_reg6` in both);
//! - the relocations of `.debug_info`, as a multiset of (kind, symbol):
//!   a wrong kind (`R_X86_64_64` for a `DTPOFF64`) is a difference.
//!
//! A tree that differs only by the classes of §6.7 is `permitted` (the
//! classes named); anything else is a difference. The report is the
//! evidence of a package; the gate does not run it.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

use super::build::{self, REFERENCE};
use super::verify;
use crate::irdiff::process::{self, End};
use crate::irdiff::tools::Search;

// ------------------------------------------------------------ trees ----

/// A DIE as `llvm-dwarfdump --debug-info` prints it, normalized.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub tag: String,
    pub attrs: Vec<String>,
    pub children: Vec<Node>,
}

/// The attributes whose values are masked (`spec/dwarf.md` §6.7).
const MASKED: [&str; 9] = [
    "DW_AT_low_pc",
    "DW_AT_high_pc",
    "DW_AT_ranges",
    "DW_AT_addr_base",
    "DW_AT_str_offsets_base",
    "DW_AT_loclists_base",
    "DW_AT_stmt_list",
    "DW_AT_object_pointer",
    "DW_AT_rnglists_base",
];

/// The references the dump prints as a bare offset when their target
/// has no name.
const REFERENCES: [&str; 10] = [
    "DW_AT_count",
    "DW_AT_upper_bound",
    "DW_AT_lower_bound",
    "DW_AT_abstract_origin",
    "DW_AT_type",
    "DW_AT_specification",
    "DW_AT_containing_type",
    "DW_AT_import",
    "DW_AT_friend",
    "DW_AT_sibling",
];

/// An expression's operations without their operands:
/// `(DW_OP_fbreg -20)` is `DW_OP_fbreg`; a location list (`(0x00000010:`
/// or `(indexed (0x0) loclist = ...`) is `list`.
fn operations(value: &str) -> String {
    let inner = value.trim().trim_start_matches('(').trim_end_matches(')');
    if inner.starts_with("indexed") || inner.starts_with("0x") || inner.contains("loclist") {
        return "list".to_owned();
    }
    let ops: Vec<&str> = inner
        .split(", ")
        .map(|op| op.split_whitespace().next().unwrap_or(""))
        .filter(|op| op.starts_with("DW_OP_"))
        .collect();
    ops.join(", ")
}

/// An attribute line, normalized for an object of `arch`
/// (`aarch64`, `x86_64`).
fn normalize(line: &str, arch: &str) -> String {
    let (name, value) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
    if MASKED.contains(&name) || (name == "DW_AT_frame_base" && arch != "x86_64") {
        return name.to_owned();
    }
    if name == "DW_AT_location" || name == "DW_AT_frame_base" {
        return format!("{name} {}", operations(value));
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

/// A bare reference named by its target's tag (§6.7, "the offset of a
/// reference").
fn name_bare_reference(line: &str, tag_at: &HashMap<u64, &str>) -> String {
    let (name, value) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
    let hex = value.trim().strip_prefix("(0x").and_then(|h| h.strip_suffix(')'));
    if REFERENCES.contains(&name)
        && let Some(hex) = hex
        && hex.len() == 8
        && let Ok(offset) = u64::from_str_radix(hex, 16)
        && let Some(tag) = tag_at.get(&offset)
    {
        return format!("{name} (0x{hex} \"{tag}\")");
    }
    line.to_owned()
}

/// The architecture of a dump (`file format elf64-x86-64`).
fn dump_arch(text: &str) -> &'static str {
    if text.contains("file format elf64-x86-64") { "x86_64" } else { "aarch64" }
}

/// The DIE trees (one per unit) of `--debug-info` output.
pub fn parse_tree(text: &str) -> Vec<Node> {
    let arch = dump_arch(text);
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
            let Some(done) = open.pop() else { break };
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
            let Some((_, rest)) = line.split_once(':') else { continue };
            let spaces = rest.len() - rest.trim_start().len();
            let depth = spaces.saturating_sub(1) / 2;
            let what = rest.trim();
            close_to(&mut open, &mut units, depth);
            if what != "NULL" {
                open.push(Node { tag: what.to_owned(), attrs: Vec::new(), children: Vec::new() });
            }
        } else if t.starts_with("DW_AT_")
            && let Some(node) = open.last_mut()
        {
            node.attrs.push(normalize(&name_bare_reference(t, &tag_at), arch));
        }
        // Anything else continues an attribute (a range or location list).
    }
    close_to(&mut open, &mut units, 0);
    units
}

fn count(n: &Node) -> usize {
    1 + n.children.iter().map(count).sum::<usize>()
}

fn count_tag(n: &Node, tag: &str) -> usize {
    usize::from(n.tag == tag) + n.children.iter().map(|c| count_tag(c, tag)).sum::<usize>()
}

fn has_attr(n: &Node, attr: &str) -> bool {
    n.attrs.iter().any(|a| a == attr) || n.children.iter().any(|c| has_attr(c, attr))
}

fn render(n: &Node, depth: usize, out: &mut String) {
    let _ = writeln!(out, "{}{}", "  ".repeat(depth), n.tag);
    for a in &n.attrs {
        let _ = writeln!(out, "{}  {a}", "  ".repeat(depth));
    }
    for c in &n.children {
        render(c, depth + 1, out);
    }
}

fn rendering(n: &Node) -> String {
    let mut s = String::new();
    render(n, 0, &mut s);
    s
}

/// Whether two DIEs' attributes are equal: as written, or a scope (not
/// the unit) with `low_pc`, `high_pc` in one and `ranges` in the other.
fn same_attrs(a: &Node, b: &Node) -> bool {
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

fn same(a: &Node, b: &Node) -> bool {
    a.tag == b.tag
        && same_attrs(a, b)
        && a.children.len() == b.children.len()
        && a.children.iter().zip(&b.children).all(|(x, y)| same(x, y))
}

/// The first place two trees differ, as the path of tags to it and the
/// two renderings there; `None` when they are equal.
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
        return Some(format!("{}:\n--- hod\n{}--- llc\n{}", a.tag, rendering(a), rendering(b)));
    }
    a.children
        .iter()
        .zip(&b.children)
        .find_map(|(x, y)| first_difference(x, y).map(|d| format!("{} > {d}", a.tag)))
}

// ------------------------------------------------ permitted classes ----

/// hod's location of a variable whose address is in a slot: the sret
/// pointer (`spec/dwarf.md` §6.7, "sret"), a dynamic alloca's address, a
/// byval argument's, an over-aligned slot's.
const SRET_HOD: &str = "DW_AT_location DW_OP_fbreg, DW_OP_deref";
/// `llc`'s for the sret pointer and a dynamic alloca: a location list.
const SRET_LLC: &str = "DW_AT_location list";
/// hod's location of a variable in its slot.
const IN_SLOT: &str = "DW_AT_location DW_OP_fbreg";
/// `llc`'s in a function whose stack it realigned (x86-64): relative to
/// `rsp`.
const RSP_RELATIVE: &str = "DW_AT_location DW_OP_breg7";
/// The same on AArch64 (WP-150): `AArch64FrameLowering` cannot reach a
/// slot of a realigned frame from `x29`, so `llc` names it from `sp`
/// (DWARF 31) while the frame base stays `DW_OP_reg29`.
const SP_RELATIVE: &str = "DW_AT_location DW_OP_breg31";

/// The class that permits hod's location `h` where `llc` writes `l` in
/// a DIE tagged `tag` (`spec/dwarf.md` §6.7 and §6.17), if any. `lists`:
/// `llc`'s unit has location lists.
fn location_class(tag: &str, h: &str, l: &str, lists: bool) -> Option<&'static str> {
    let variable = tag == "DW_TAG_variable" || tag == "DW_TAG_formal_parameter";
    match (h, l) {
        _ if !variable => None,
        (SRET_HOD, SRET_LLC) if lists => Some("sret"),
        // x86-64 byval: `llc` names the argument area (`DW_OP_fbreg
        // +16`), hod the pointer to it it saved.
        (SRET_HOD, IN_SLOT) if tag == "DW_TAG_formal_parameter" => Some("byval"),
        (SRET_HOD | IN_SLOT, RSP_RELATIVE | SP_RELATIVE) => Some("realigned frame"),
        _ => None,
    }
}

/// The classes of `spec/dwarf.md` §6.7 that make hod's unit `h` differ
/// from `llc`'s `l` beyond equality, or `None` when something else
/// differs too:
/// - `sret`: `llc`'s unit has `DW_AT_loclists_base` and a location list
///   where hod's variable is `DW_OP_fbreg N, DW_OP_deref` (the sret
///   pointer; on x86-64 also a dynamic alloca);
/// - `byval` (x86-64): a byval parameter `llc` places at `DW_OP_fbreg
///   +16` and hod at `DW_OP_fbreg N, DW_OP_deref`;
/// - `realigned frame`: `llc` realigned the stack and places variables
///   by `DW_OP_breg7` (`rsp`, x86-64) or `DW_OP_breg31` (`sp`, AArch64,
///   WP-150), hod by `DW_OP_fbreg`;
/// - `(b) is.constant block`: hod has a lexical block `llc` does not
///   (only where the module calls `llvm.is.constant`);
/// - `(a) fallthrough inlined`: `llc` has an inlined subroutine hod does
///   not, and then abstract instances and types hod does not;
/// - `split pubnames`: `llc`'s skeleton unit has `DW_AT_GNU_pubnames`,
///   hod writes no name index (§8);
/// - `llc 19 blocks`: `llc` 19 writes no `DW_AT_abstract_origin` on a
///   concrete block.
pub fn permitted(h: &Node, l: &Node, is_constant: bool, llc19: bool) -> Option<Vec<&'static str>> {
    let mut l = l.clone();
    let mut classes = Vec::new();
    // §8 (split units): hod writes no name index.
    let pubnames = "DW_AT_GNU_pubnames (true)";
    if l.tag == "DW_TAG_skeleton_unit" && !h.attrs.iter().any(|a| a == pubnames) {
        let before = l.attrs.len();
        l.attrs.retain(|a| a != pubnames);
        if l.attrs.len() < before {
            classes.push("split pubnames");
        }
    }
    let mut sret = false;
    if !h.attrs.iter().any(|a| a == "DW_AT_loclists_base")
        && l.attrs.iter().any(|a| a == "DW_AT_loclists_base")
    {
        l.attrs.retain(|a| a != "DW_AT_loclists_base");
        sret = true;
    } else if !l.attrs.iter().any(|a| a == "DW_AT_str_offsets_base") {
        // WP-114: a DWARF 4 unit has no base attribute; its lists are
        // in `.debug_loc`, seen as a location `list` in the tree.
        sret = has_attr(&l, "DW_AT_location list") && !has_attr(h, "DW_AT_location list");
    }
    let extras_ok =
        count_tag(&l, "DW_TAG_inlined_subroutine") > count_tag(h, "DW_TAG_inlined_subroutine");
    let mut p = Permit { classes, is_constant, extras_ok, llc19, sret };
    p.eq(h, &l).then_some(p.classes)
}

#[expect(clippy::struct_excessive_bools, reason = "one flag per class of §6.7")]
struct Permit {
    classes: Vec<&'static str>,
    is_constant: bool,
    extras_ok: bool,
    llc19: bool,
    sret: bool,
}

fn is_extra(n: &Node) -> bool {
    (n.tag == "DW_TAG_subprogram" && n.attrs.iter().any(|a| a.starts_with("DW_AT_inline ")))
        || n.tag.ends_with("_type")
        || n.tag == "DW_TAG_typedef"
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
            let mut placed = h.clone();
            let mut found = Vec::new();
            for (a, b) in placed.attrs.iter_mut().zip(&l.attrs) {
                if let Some(class) = location_class(&h.tag, a, b, self.sret) {
                    found.push(class);
                    a.clone_from(b);
                }
            }
            if self.llc19 && h.tag == "DW_TAG_lexical_block" && same_attrs(&bare, l) {
                self.note("llc 19 blocks");
            } else if !found.is_empty() && same_attrs(&placed, l) {
                for class in found {
                    self.note(class);
                }
            } else {
                return false;
            }
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

// ----------------------------------------------------- relocations ----

/// The relocations of `.debug_info` of an `llvm-objdump -r` listing, as
/// sorted (kind, symbol) pairs: the offsets and addends are each
/// writer's own; a section symbol is its section's name.
pub fn info_relocations(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("RELOCATION RECORDS FOR [") {
            inside = rest.starts_with(".debug_info]");
            continue;
        }
        let mut fields = line.split_whitespace();
        let (Some(offset), Some(kind), Some(value)) = (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        if !inside || !offset.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        let symbol = value.split(['+', '-']).next().unwrap_or(value);
        out.push((kind.to_owned(), symbol.to_owned()));
    }
    out.sort();
    out
}

// ------------------------------------------------------------ runs ----

/// One module's outcome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Every unit equal, and the relocations; the number of DIEs.
    Same(usize),
    /// Equal but for the classes named.
    Permitted(Vec<&'static str>),
    /// A difference: where.
    Differs(String),
    /// hod declined, with its reason.
    Declined(String),
    /// A tool failed, or an object does not verify.
    Failed(String),
}

/// What `debug trees` is given.
#[derive(Clone, Debug)]
pub struct Options {
    pub paths: Vec<PathBuf>,
    pub jobs: usize,
    pub hod: Option<PathBuf>,
    pub build_dir: PathBuf,
    /// The `llc` major; `None` is the module's (`llvm19` in its path, or
    /// `clang-19` / `c19` in its name: 19; else 22).
    pub llc: Option<u32>,
    pub verbose: bool,
}

struct Tools {
    hod: PathBuf,
    llc: Vec<(u32, PathBuf)>,
    dwarfdump: PathBuf,
    objdump: PathBuf,
}

/// The LLVM major of a module, from its path.
fn module_major(module: &Path) -> u32 {
    let text = module.display().to_string();
    if ["llvm19", "clang-19", ".c19.", "-19."].iter().any(|w| text.contains(w)) { 19 } else { 22 }
}

fn run_tool(tool: &Path, configure: impl Fn(&mut Command), what: &str) -> Result<String, String> {
    let output = process::run(tool, configure, b"", verify::LIMITS)?;
    if output.end != End::Exit(0) {
        let said = String::from_utf8_lossy(&output.stdout);
        let first = said.lines().find(|l| l.contains("error:")).unwrap_or("").trim().to_owned();
        return Err(format!("{}: {first}", build::failure(what, &output, verify::LIMITS)));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn one(module: &Path, tools: &Tools, major: u32, dir: &Path) -> Outcome {
    let text = fs::read_to_string(module).unwrap_or_default();
    let extra: String = text
        .lines()
        .take_while(|l| l.starts_with(';') || l.is_empty())
        .find_map(|l| l.strip_prefix(";@")?.trim().strip_prefix("llc-args:").map(str::to_owned))
        .unwrap_or_default();
    let extra: Vec<&str> = extra.split_whitespace().collect();
    let (hod_o, llc_o) = (dir.join("hod.o"), dir.join("llc.o"));
    match build::hod_object(&tools.hod, module, &hod_o, verify::LIMITS) {
        build::HodBuild::Built => {}
        build::HodBuild::Declined(reason) => return Outcome::Declined(reason),
        build::HodBuild::Failed(e) => return Outcome::Failed(e),
    }
    let Some((_, llc)) = tools.llc.iter().find(|(m, _)| *m == major) else {
        return Outcome::Failed(format!("no llc-{major}"));
    };
    let reference = run_tool(
        llc,
        |c| {
            c.args(REFERENCE).args(["-filetype=obj", "-relocation-model=pic"]).args(&extra);
            c.args(build::split_dwarf_args(module, &llc_o));
            c.arg("-o").arg(&llc_o).arg(module);
        },
        "llc",
    );
    if let Err(e) = reference {
        return Outcome::Failed(e);
    }
    let dump = |object: &Path, what: &[&str]| {
        run_tool(
            &tools.dwarfdump,
            |c| {
                c.args(what).arg(object).env("LC_ALL", "C");
            },
            "llvm-dwarfdump",
        )
    };
    for (object, who) in [(&hod_o, "hod"), (&llc_o, "llc")] {
        if let Err(e) = dump(object, &["--verify"]) {
            return Outcome::Failed(format!("{who}'s object does not verify: {e}"));
        }
    }
    let (ours, theirs) = match (dump(&hod_o, &["--debug-info"]), dump(&llc_o, &["--debug-info"])) {
        (Ok(a), Ok(b)) => (parse_tree(&a), parse_tree(&b)),
        (Err(e), _) | (_, Err(e)) => return Outcome::Failed(e),
    };
    let relocations = |object: &Path| {
        run_tool(
            &tools.objdump,
            |c| {
                c.arg("-r").arg(object).env("LC_ALL", "C");
            },
            "llvm-objdump",
        )
        .map(|t| info_relocations(&t))
    };
    let (ours_r, theirs_r) = match (relocations(&hod_o), relocations(&llc_o)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => return Outcome::Failed(e),
    };
    compare(&ours, &theirs, &ours_r, &theirs_r, text.contains("@llvm.is.constant"), major == 19)
}

/// The outcome of a module from its trees and relocations.
pub fn compare(
    ours: &[Node],
    theirs: &[Node],
    ours_r: &[(String, String)],
    theirs_r: &[(String, String)],
    is_constant: bool,
    llc19: bool,
) -> Outcome {
    if ours.len() != theirs.len() {
        return Outcome::Differs(format!("{} units, llc's {}", ours.len(), theirs.len()));
    }
    let mut classes: Vec<&'static str> = Vec::new();
    for (h, l) in ours.iter().zip(theirs) {
        if same(h, l) {
            continue;
        }
        match permitted(h, l, is_constant, llc19) {
            Some(found) => {
                for class in found {
                    if !classes.contains(&class) {
                        classes.push(class);
                    }
                }
            }
            None => {
                return Outcome::Differs(first_difference(h, l).unwrap_or_default());
            }
        }
    }
    // `llc`'s location lists (the `sret` class) add its relocation of
    // `DW_AT_loclists_base` against `.debug_loclists` (in DWARF 4, of each
    // list's offset against `.debug_loc`, WP-114); every other must be
    // equal.
    let mut theirs_r = theirs_r.to_vec();
    if classes.contains(&"sret") {
        theirs_r.retain(|(_, symbol)| symbol != ".debug_loclists" && symbol != ".debug_loc");
    }
    let theirs_r = theirs_r.as_slice();
    if ours_r != theirs_r {
        let only = |a: &[(String, String)], b: &[(String, String)]| {
            a.iter().find(|x| !b.contains(x)).map(|(k, s)| format!("{k} {s}"))
        };
        return Outcome::Differs(format!(
            ".debug_info relocations: hod only {:?}, llc only {:?}",
            only(ours_r, theirs_r),
            only(theirs_r, ours_r)
        ));
    }
    if classes.is_empty() {
        Outcome::Same(ours.iter().map(count).sum())
    } else {
        Outcome::Permitted(classes)
    }
}

/// Runs the comparison; the modules and their outcomes, in name order.
pub fn run(options: &Options) -> Result<Vec<(PathBuf, Outcome)>, String> {
    let search = Search::from_env();
    let find = |tool: &str| [22, 19].into_iter().find_map(|m| search.versioned(tool, m));
    let dwarfdump = find("llvm-dwarfdump").ok_or("no llvm-dwarfdump")?;
    let objdump = find("llvm-objdump").ok_or("no llvm-objdump")?;
    let llc: Vec<(u32, PathBuf)> =
        [22, 19].into_iter().filter_map(|m| Some((m, search.versioned("llc", m)?))).collect();
    let paths = if options.paths.is_empty() {
        vec![crate::corpus::default_dir(&options.build_dir)]
    } else {
        options.paths.clone()
    };
    let modules = verify::debug_modules(&paths)?;
    let hod = match &options.hod {
        Some(hod) => hod.clone(),
        None => crate::irdiff::build_workspace_hod(&options.build_dir)?,
    };
    let tools = Tools { hod, llc, dwarfdump, objdump };
    let work = options.build_dir.join("l7").join(format!("trees-{}", std::process::id()));
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
                    let major = options.llc.unwrap_or_else(|| module_major(module));
                    let outcome = match fs::create_dir_all(&dir) {
                        Ok(()) => one(module, &tools, major, &dir),
                        Err(e) => {
                            Outcome::Failed(format!("cannot create '{}': {e}", dir.display()))
                        }
                    };
                    let _ = fs::remove_dir_all(&dir);
                    slots.lock().unwrap_or_else(PoisonError::into_inner)[index] = Some(outcome);
                }
            });
        }
    });
    let _ = fs::remove_dir_all(&work);
    let outcomes = slots.into_inner().unwrap_or_else(PoisonError::into_inner);
    Ok(modules
        .into_iter()
        .zip(outcomes)
        .map(|(m, o)| (m, o.unwrap_or_else(|| Outcome::Failed("not run".to_owned()))))
        .collect())
}

/// The report: per module (with `verbose`, also the equal ones and the
/// whole first difference), then the totals.
pub fn summary(outcomes: &[(PathBuf, Outcome)], verbose: bool) -> String {
    let mut out = String::new();
    let (mut equal, mut permitted, mut differs, mut declined, mut failed) = (0, 0, 0, 0, 0);
    let mut dies = 0;
    for (module, outcome) in outcomes {
        let name = module.display();
        match outcome {
            Outcome::Same(n) => {
                equal += 1;
                dies += n;
                if verbose {
                    let _ = writeln!(out, "SAME {name}: {n} DIEs");
                }
            }
            Outcome::Permitted(classes) => {
                permitted += 1;
                let _ = writeln!(out, "PERMITTED {name}: {}", classes.join(", "));
            }
            Outcome::Differs(where_) => {
                differs += 1;
                let first =
                    if verbose { where_.as_str() } else { where_.lines().next().unwrap_or("") };
                let _ = writeln!(out, "DIFFERS {name}: {first}");
            }
            Outcome::Declined(reason) => {
                declined += 1;
                let _ = writeln!(out, "DECLINED {name}: {reason}");
            }
            Outcome::Failed(e) => {
                failed += 1;
                let _ = writeln!(out, "FAILED {name}: {e}");
            }
        }
    }
    let _ = writeln!(
        out,
        "trees: {} -g modules: {equal} same ({dies} DIEs), {permitted} permitted, {differs} \
         differ, {declined} declined, {failed} failed",
        outcomes.len()
    );
    out
}

/// `cargo xtask debug trees ...`. Exit status 1 when a module differs or
/// fails.
pub fn command(args: &[String], build_dir: PathBuf) -> u8 {
    let mut options =
        Options { paths: Vec::new(), jobs: 4, hod: None, build_dir, llc: None, verbose: false };
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        let mut take = || {
            i += 1;
            args.get(i).cloned().ok_or_else(|| format!("{arg} needs a value"))
        };
        let got = match arg {
            "-v" | "--verbose" => {
                options.verbose = true;
                Ok(())
            }
            "-j" | "--jobs" => match take().map(|n| n.parse::<usize>()) {
                Ok(Ok(n)) if n > 0 => {
                    options.jobs = n.min(4);
                    Ok(())
                }
                _ => Err("-j needs a count".to_owned()),
            },
            "--hod" => take().map(|v| options.hod = Some(PathBuf::from(v))),
            "--llc" => match take().map(|n| n.parse::<u32>()) {
                Ok(Ok(m @ (19 | 22))) => {
                    options.llc = Some(m);
                    Ok(())
                }
                _ => Err("--llc needs 19 or 22".to_owned()),
            },
            _ if !arg.starts_with('-') => {
                options.paths.push(PathBuf::from(arg));
                Ok(())
            }
            _ => Err(format!("unknown option '{arg}'")),
        };
        if let Err(e) = got {
            eprint!("xtask debug trees: {e}\n{}", super::USAGE);
            return 2;
        }
        i += 1;
    }
    match run(&options) {
        Ok(outcomes) => {
            print!("{}", summary(&outcomes, options.verbose));
            u8::from(
                outcomes.iter().any(|(_, o)| matches!(o, Outcome::Differs(_) | Outcome::Failed(_))),
            )
        }
        Err(e) => {
            eprintln!("xtask debug trees: {e}");
            3
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const X64: &str = "a.o:\tfile format elf64-x86-64\n\n.debug_info contents:\n\
0x00000000: Compile Unit: length = 0x10\n\n\
0x0000000c: DW_TAG_compile_unit\n              DW_AT_producer\t(\"clang\")\n\
              DW_AT_low_pc\t(0x0000000000000000)\n\n\
0x00000020:   DW_TAG_subprogram\n                DW_AT_frame_base\t(DW_OP_reg6 RBP)\n\
                DW_AT_name\t(\"f\")\n\n\
0x00000030:     DW_TAG_variable\n                  DW_AT_location\t(DW_OP_fbreg -20)\n\
                  DW_AT_type\t(0x00000040 \"int\")\n\n\
0x0000003e:     NULL\n\n0x0000003f:   NULL\n";

    #[test]
    fn x86_frame_bases_are_compared_and_locations_by_their_operations() {
        let trees = parse_tree(X64);
        assert_eq!(trees.len(), 1);
        let f = &trees[0].children[0];
        assert_eq!(f.attrs[0], "DW_AT_frame_base DW_OP_reg6");
        assert_eq!(f.children[0].attrs[0], "DW_AT_location DW_OP_fbreg");
        // The register of the frame base is compared on x86-64 ...
        let rsp = parse_tree(&X64.replace("DW_OP_reg6 RBP", "DW_OP_reg7 RSP"));
        assert!(first_difference(&trees[0], &rsp[0]).is_some());
        // ... and masked on AArch64.
        let a64 = X64.replace("elf64-x86-64", "elf64-littleaarch64");
        let a64_rsp = a64.replace("DW_OP_reg6 RBP", "DW_OP_reg7 RSP");
        assert_eq!(parse_tree(&a64), parse_tree(&a64_rsp));
        // Another frame offset is the same shape; another operation is not.
        assert_eq!(trees, parse_tree(&X64.replace("fbreg -20", "fbreg -36")));
        let breg = parse_tree(&X64.replace("DW_OP_fbreg -20", "DW_OP_breg7 RSP+8"));
        assert!(first_difference(&trees[0], &breg[0]).is_some());
        assert_eq!(operations("(0x00000010: \n"), "list");
        assert_eq!(
            operations("(DW_OP_const8u 0x0, DW_OP_GNU_push_tls_address)"),
            "DW_OP_const8u, DW_OP_GNU_push_tls_address"
        );
    }

    #[test]
    fn sret_location_lists_are_permitted() {
        // A DWARF 5 unit has `str_offsets_base`, and `loclists_base` with
        // lists; a DWARF 4 unit neither (WP-114).
        let unit = |loclists: bool, location: &str| Node {
            tag: "DW_TAG_compile_unit".into(),
            attrs: if loclists {
                vec!["DW_AT_str_offsets_base".into(), "DW_AT_loclists_base".into()]
            } else {
                vec!["DW_AT_str_offsets_base".into()]
            },
            children: vec![Node {
                tag: "DW_TAG_variable".into(),
                attrs: vec![location.into()],
                children: vec![],
            }],
        };
        let (h, l) = (unit(false, SRET_HOD), unit(true, SRET_LLC));
        assert_eq!(
            compare(std::slice::from_ref(&h), std::slice::from_ref(&l), &[], &[], false, false),
            { Outcome::Permitted(vec!["sret"]) }
        );
        // Without llc's location lists, a list is a difference.
        assert!(matches!(
            compare(std::slice::from_ref(&h), &[unit(false, SRET_LLC)], &[], &[], false, false),
            Outcome::Differs(_)
        ));
        // DWARF 4: no base attribute; `llc`'s list is relocated against
        // `.debug_loc`.
        let v4 = |location: &str| {
            let mut n = unit(false, location);
            n.attrs.clear();
            n
        };
        let loc = [("R_X86_64_32".to_owned(), ".debug_loc".to_owned())];
        assert_eq!(
            compare(&[v4(SRET_HOD)], &[v4(SRET_LLC)], &[], &loc, false, false),
            Outcome::Permitted(vec!["sret"])
        );
        assert!(matches!(
            compare(&[v4(SRET_HOD)], &[v4(IN_SLOT)], &[], &[], false, false),
            Outcome::Differs(_)
        ));
    }

    /// WP-150: `llc`'s places in a realigned frame are `sp`-relative on
    /// AArch64 as they are `rsp`-relative on x86-64; any other register
    /// is a difference.
    #[test]
    fn realigned_frames_are_permitted_on_both_targets() {
        for tag in ["DW_TAG_variable", "DW_TAG_formal_parameter"] {
            for h in [IN_SLOT, SRET_HOD] {
                for l in [RSP_RELATIVE, SP_RELATIVE] {
                    assert_eq!(location_class(tag, h, l, false), Some("realigned frame"));
                }
            }
            assert_eq!(location_class(tag, IN_SLOT, "DW_AT_location DW_OP_breg29", false), None);
        }
        assert_eq!(location_class("DW_TAG_member", IN_SLOT, SP_RELATIVE, false), None);
    }

    #[test]
    fn relocation_kinds_are_compared() {
        let listing = "RELOCATION RECORDS FOR [.debug_info]:\nOFFSET           TYPE                     VALUE\n\
0000000000000008 R_X86_64_32              .debug_abbrev\n\
0000000000000040 R_X86_64_DTPOFF64        counter\n\
0000000000000050 R_X86_64_64              .text+0x10\n\n\
RELOCATION RECORDS FOR [.debug_line]:\n0000000000000000 R_X86_64_64 .text\n";
        let ours = info_relocations(listing);
        assert_eq!(ours.len(), 3);
        assert!(ours.contains(&("R_X86_64_64".into(), ".text".into())));
        let wrong = info_relocations(&listing.replace("R_X86_64_DTPOFF64", "R_X86_64_64      "));
        let tree = parse_tree(X64);
        assert_eq!(compare(&tree, &tree, &ours, &ours, false, false), Outcome::Same(3));
        assert!(matches!(compare(&tree, &tree, &wrong, &ours, false, false), Outcome::Differs(_)));
    }
}
