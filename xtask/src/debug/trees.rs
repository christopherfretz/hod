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

/// The attributes whose values are masked (`spec/dwarf.md` §6.7; the
/// call-site addresses since WP-260, §6.19: each writer's own code).
const MASKED: [&str; 11] = [
    "DW_AT_call_return_pc",
    "DW_AT_call_pc",
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
/// WP-195: the same for a variable whose address is in a slot (an sret
/// or byval pointer, an over-aligned slot), `llc` naming the slot from
/// `rsp` or `sp`, hod from the frame base.
const RSP_RELATIVE_DEREF: &str = "DW_AT_location DW_OP_breg7, DW_OP_deref";
const SP_RELATIVE_DEREF: &str = "DW_AT_location DW_OP_breg31, DW_OP_deref";

/// The class that permits hod's location `h` where `llc` writes `l` in
/// a DIE tagged `tag` (`spec/dwarf.md` §6.7 and §6.17), if any. `lists`:
/// `llc`'s unit has location lists; `protected`: the module has a
/// function with `ssp`, `sspstrong` or `sspreq`.
fn location_class(
    tag: &str,
    h: &str,
    l: &str,
    lists: bool,
    protected: bool,
) -> Option<&'static str> {
    let variable = tag == "DW_TAG_variable" || tag == "DW_TAG_formal_parameter";
    match (h, l) {
        _ if !variable => None,
        (SRET_HOD, SRET_LLC) if lists => Some("sret"),
        // x86-64 byval: `llc` names the argument area (`DW_OP_fbreg
        // +16`), hod the pointer to it it saved.
        (SRET_HOD, IN_SLOT) if tag == "DW_TAG_formal_parameter" => Some("byval"),
        // WP-183 (`spec/codegen.md` §5.6.24): a slot of a protected
        // function's guarded block, which hod reaches through its pointer
        // slot and `llc` names directly.
        (SRET_HOD, IN_SLOT) if protected => Some("stack protector"),
        // WP-195: also a pointer slot `llc` names from the stack pointer
        // and dereferences.
        (SRET_HOD | IN_SLOT, RSP_RELATIVE | SP_RELATIVE)
        | (SRET_HOD, RSP_RELATIVE_DEREF | SP_RELATIVE_DEREF) => Some("realigned frame"),
        _ => None,
    }
}

/// The classes of `spec/dwarf.md` §6.7 that make hod's unit `h` differ
/// from `llc`'s `l` beyond equality, or `None` when something else
/// differs too:
/// - `sret`: `llc`'s unit has `DW_AT_loclists_base` and a location list
///   where hod's variable is `DW_OP_fbreg N, DW_OP_deref` (the sret
///   pointer; a dynamic alloca's until WP-221 made it a list in hod too);
/// - `byval` (x86-64): a byval parameter `llc` places at `DW_OP_fbreg
///   +16` and hod at `DW_OP_fbreg N, DW_OP_deref`;
/// - `stack protector`: in a module with `ssp`, `sspstrong` or `sspreq`,
///   a variable or parameter hod places at `DW_OP_fbreg N, DW_OP_deref`
///   (a slot of a guarded block, through its pointer slot) and `llc` at
///   `DW_OP_fbreg N`;
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
///   concrete block;
/// - `declared value` (§6.30, [`permitted_with`] only): a variable the
///   module declares on a `phi` has hod's location list where `llc`'s
///   DIE has no location, or a DIE (or a lexical block of only such
///   DIEs) `llc` drops.
pub fn permitted(
    h: &Node,
    l: &Node,
    is_constant: bool,
    llc19: bool,
    protected: bool,
) -> Option<Vec<&'static str>> {
    permitted_with(h, l, &Facts { is_constant, llc19, protected, ..Facts::default() })
}

/// The class of `spec/dwarf.md` §6.37 (WP-260): a variable the module
/// describes by value records ([`described_by_values`]) that hod
/// describes by a location list or by none, where `llc` writes a single
/// expression, `DW_AT_const_value`, a list or nothing; or a concrete DIE
/// of such a variable with no location that `llc` drops.
pub const VALUE_RECORDS: &str = "value records";

/// §6.37 (WP-260): on x86-64, a function whose IR asks for no frame
/// pointer ([`without_frame_pointer`]): `llc`'s frame base is `rsp`
/// (`DW_OP_reg7`), hod's `rbp` (`DW_OP_reg6`, hod always keeps the frame
/// pointer).
pub const NO_FRAME_POINTER: &str = "no frame pointer";

/// §6.37 (WP-260): hod's unit has `DW_AT_rnglists_base` where `llc`'s has
/// none, because hod writes more `DW_AT_ranges` (a scope of several
/// ranges in hod's code, of one in `llc`'s; §6.7).
pub const UNIT_RANGE_LISTS: &str = "unit range lists";

/// Whether DIE `n` is a variable of value records: a variable or
/// parameter named in `values`, by its name or its abstract origin; one
/// with neither when `values` has an unnamed variable ("").
fn is_value(n: &Node, values: &[String]) -> bool {
    let named =
        |a: &String| a.starts_with("DW_AT_name ") || a.starts_with("DW_AT_abstract_origin ");
    (n.tag == "DW_TAG_variable" || n.tag == "DW_TAG_formal_parameter")
        && values.iter().any(|v| {
            if v.is_empty() {
                return !n.attrs.iter().any(named);
            }
            n.attrs.iter().any(|a| {
                *a == format!("DW_AT_name (\"{v}\")")
                    || *a == format!("DW_AT_abstract_origin (\"{v}\")")
            })
        })
}

/// A location or a constant value.
fn is_place(a: &str) -> bool {
    a.starts_with("DW_AT_location") || a.starts_with("DW_AT_const_value")
}

/// [`VALUE_RECORDS`]: hod's value variable `h` without a place or with
/// a location list, `llc`'s `l` with any place or none, otherwise equal.
fn value_placed(h: &Node, l: &Node, values: &[String]) -> bool {
    if !is_value(h, values) || h.attrs.iter().any(|a| is_place(a) && a != "DW_AT_location list") {
        return false;
    }
    let bare = |n: &Node| {
        let mut b = n.clone();
        b.attrs.retain(|a| !is_place(a));
        b
    };
    same_attrs(&bare(h), &bare(l))
}

/// [`VALUE_RECORDS`]: a DIE of hod's `llc` does not have: a concrete
/// value variable (only `DW_AT_abstract_origin`) with no place.
fn value_extra(n: &Node, values: &[String]) -> bool {
    is_value(n, values)
        && n.children.is_empty()
        && n.attrs.iter().all(|a| a.starts_with("DW_AT_abstract_origin "))
}

/// [`NO_FRAME_POINTER`]: hod's subprogram `h` with `rbp` as its frame
/// base where `llc`'s `l` has `rsp`, of a function in `frameless`,
/// otherwise equal.
fn frame_without_pointer(h: &Node, l: &Node, frameless: &[String]) -> bool {
    const RBP: &str = "DW_AT_frame_base DW_OP_reg6";
    const RSP: &str = "DW_AT_frame_base DW_OP_reg7";
    let named = frameless.iter().any(|f| {
        ["DW_AT_linkage_name", "DW_AT_name", "DW_AT_abstract_origin"]
            .iter()
            .any(|k| h.attrs.contains(&format!("{k} (\"{f}\")")))
    });
    if h.tag != "DW_TAG_subprogram" || !named || !l.attrs.iter().any(|a| a == RSP) {
        return false;
    }
    let mut placed = h.clone();
    for a in &mut placed.attrs {
        if a == RBP {
            RSP.clone_into(a);
        }
    }
    same_attrs(&placed, l)
}

/// The identities of sibling DIEs for [`VALUE_RECORDS`]'s comparison by
/// name: a DIE's tag and its name (or linkage name, abstract origin,
/// specification, call origin), or for one without, its tag and its rank
/// among the siblings of that tag without one (the order is kept there).
fn identities(ns: &[Node]) -> Vec<String> {
    let key = [
        "DW_AT_name ",
        "DW_AT_linkage_name ",
        "DW_AT_abstract_origin ",
        "DW_AT_specification ",
        "DW_AT_call_origin ",
    ];
    let mut unnamed: HashMap<&str, usize> = HashMap::new();
    ns.iter()
        .map(|n| {
            if let Some(a) = key.iter().find_map(|k| n.attrs.iter().find(|a| a.starts_with(k))) {
                format!("{} {a}", n.tag)
            } else {
                let rank = unnamed.entry(n.tag.as_str()).or_default();
                *rank += 1;
                format!("{} #{rank}", n.tag)
            }
        })
        .collect()
}

fn count_attr(n: &Node, attr: &str) -> usize {
    n.attrs.iter().filter(|a| *a == attr).count()
        + n.children.iter().map(|c| count_attr(c, attr)).sum::<usize>()
}

/// The number of hod's location lists of value variables in `n`.
fn value_lists(n: &Node, values: &[String]) -> usize {
    usize::from(is_value(n, values) && n.attrs.iter().any(|a| a == "DW_AT_location list"))
        + n.children.iter().map(|c| value_lists(c, values)).sum::<usize>()
}

/// The class of `spec/dwarf.md` §6.30 (WP-221): a variable declared on a
/// `phi`'s result ([`declared_on_phi`]), which hod describes by a
/// location list and `llc`'s reference (`SelectionDAG`) drops: hod's DIE
/// has a list where `llc`'s has no location, or `llc` has no DIE (nor
/// the lexical block holding only such variables).
pub const DECLARED_VALUE: &str = "declared value";

fn is_declared(n: &Node, declared: &[String]) -> bool {
    (n.tag == "DW_TAG_variable" || n.tag == "DW_TAG_formal_parameter")
        && n.attrs.iter().any(|a| declared.contains(a))
}

/// A DIE of hod's `llc`'s reference does not have under the class
/// [`DECLARED_VALUE`]: a declared variable, or a lexical block of them.
fn declared_extra(n: &Node, declared: &[String]) -> bool {
    if n.tag == "DW_TAG_lexical_block" {
        return !n.children.is_empty() && n.children.iter().all(|c| declared_extra(c, declared));
    }
    is_declared(n, declared) && n.children.is_empty()
}

/// The number of hod's location lists of declared variables in `n`.
fn declared_lists(n: &Node, declared: &[String]) -> usize {
    usize::from(is_declared(n, declared) && n.attrs.iter().any(|a| a == "DW_AT_location list"))
        + n.children.iter().map(|c| declared_lists(c, declared)).sum::<usize>()
}

/// The number of hod's declared variables in `n`.
fn declared_dies(n: &Node, declared: &[String]) -> usize {
    usize::from(is_declared(n, declared))
        + n.children.iter().map(|c| declared_dies(c, declared)).sum::<usize>()
}

/// Removes from `ours` at most `most` of the relocations against a
/// symbol `of` accepts that it has more of than `theirs`.
fn drop_surplus(
    ours: &mut Vec<(String, String)>,
    theirs: &[(String, String)],
    of: impl Fn(&str) -> bool,
    most: usize,
) {
    let count = |r: &[(String, String)]| r.iter().filter(|(_, s)| of(s)).count();
    let mut surplus = count(ours).saturating_sub(count(theirs)).min(most);
    ours.retain(|(_, s)| {
        let drop = surplus > 0 && of(s);
        surplus -= usize::from(drop);
        !drop
    });
}

/// [`permitted`] with every fact of the module.
pub fn permitted_with(h: &Node, l: &Node, facts: &Facts) -> Option<Vec<&'static str>> {
    let Facts { is_constant, llc19, protected, .. } = *facts;
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
    // Whether `llc`'s unit has location lists. hod's may have its own
    // too (WP-221: a dynamic `alloca`, a declared value), and then the
    // unit's `loclists_base` is in both.
    let sret = if l.attrs.iter().any(|a| a == "DW_AT_loclists_base") {
        if !h.attrs.iter().any(|a| a == "DW_AT_loclists_base") {
            l.attrs.retain(|a| a != "DW_AT_loclists_base");
        }
        true
    } else {
        // WP-114: a DWARF 4 unit has no base attribute; its lists are
        // in `.debug_loc`, seen as a location `list` in the tree.
        !l.attrs.iter().any(|a| a == "DW_AT_str_offsets_base")
            && has_attr(&l, "DW_AT_location list")
    };
    // WP-260 (§6.37): hod's `rnglists_base` for range lists `llc` does
    // not write (the scopes are equal by §6.7).
    let mut h = h.clone();
    let rnglists = "DW_AT_rnglists_base";
    if h.attrs.iter().any(|a| a == rnglists)
        && !l.attrs.iter().any(|a| a == rnglists)
        && count_attr(&h, "DW_AT_ranges") > count_attr(&l, "DW_AT_ranges")
    {
        h.attrs.retain(|a| a != rnglists);
        classes.push(UNIT_RANGE_LISTS);
    }
    let extras_ok =
        count_tag(&l, "DW_TAG_inlined_subroutine") > count_tag(&h, "DW_TAG_inlined_subroutine");
    let mut p = Permit {
        classes,
        is_constant,
        extras_ok,
        llc19,
        sret,
        protected,
        declared: &facts.declared,
        values: &facts.values,
        frameless: &facts.frameless,
    };
    p.eq(&h, &l).then_some(p.classes)
}

#[expect(clippy::struct_excessive_bools, reason = "one flag per class of §6.7")]
struct Permit<'f> {
    classes: Vec<&'static str>,
    is_constant: bool,
    extras_ok: bool,
    llc19: bool,
    sret: bool,
    protected: bool,
    /// WP-221: [`declared_on_phi`].
    declared: &'f [String],
    /// WP-260: [`described_by_values`].
    values: &'f [String],
    /// WP-260: [`without_frame_pointer`].
    frameless: &'f [String],
}

fn is_extra(n: &Node) -> bool {
    (n.tag == "DW_TAG_subprogram" && n.attrs.iter().any(|a| a.starts_with("DW_AT_inline ")))
        || n.tag.ends_with("_type")
        || n.tag == "DW_TAG_typedef"
}

impl Permit<'_> {
    /// WP-221: hod's declared variable `h` with a list where `llc`'s `l`
    /// has no location, and otherwise the same.
    fn declared_unplaced(&self, h: &Node, l: &Node) -> bool {
        if !is_declared(h, self.declared) || l.attrs.iter().any(|a| a.starts_with("DW_AT_location"))
        {
            return false;
        }
        let mut bare = h.clone();
        bare.attrs.retain(|a| a != "DW_AT_location list");
        same_attrs(&bare, l)
    }

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
                if let Some(class) = location_class(&h.tag, a, b, self.sret, self.protected) {
                    found.push(class);
                    a.clone_from(b);
                }
            }
            if self.declared_unplaced(h, l) {
                self.note(DECLARED_VALUE);
            } else if self.llc19 && h.tag == "DW_TAG_lexical_block" && same_attrs(&bare, l) {
                self.note("llc 19 blocks");
            } else if !found.is_empty() && same_attrs(&placed, l) {
                for class in found {
                    self.note(class);
                }
            } else if value_placed(h, l, self.values) {
                self.note(VALUE_RECORDS);
            } else if frame_without_pointer(h, l, self.frameless) {
                self.note(NO_FRAME_POINTER);
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
        self.classes.clone_from(&saved);
        if !self.values.is_empty() && self.by_identity(&h.children, &l.children) {
            return true;
        }
        self.classes = saved;
        false
    }

    /// WP-260 (§6.37), in a module with value records: the children
    /// compared as a multiset (§6.7, "sibling order"), equal renderings
    /// first, then the rest of each side paired by identity
    /// ([`identities`]); hod's left over must be [`value_extra`], `llc`'s
    /// none.
    fn by_identity(&mut self, hs: &[Node], ls: &[Node]) -> bool {
        let mut pool: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, c) in hs.iter().enumerate().rev() {
            pool.entry(rendering(c)).or_default().push(i);
        }
        let mut used = vec![false; hs.len()];
        let mut left_l = Vec::new();
        for c in ls {
            match pool.get_mut(&rendering(c)).and_then(Vec::pop) {
                Some(i) => used[i] = true,
                None => left_l.push(c.clone()),
            }
        }
        let left_h: Vec<Node> =
            hs.iter().zip(&used).filter(|(_, u)| !**u).map(|(c, _)| c.clone()).collect();
        let (ids_h, ids_l) = (identities(&left_h), identities(&left_l));
        let mut taken = vec![false; left_h.len()];
        for (l, id) in left_l.iter().zip(&ids_l) {
            match (0..left_h.len()).find(|&i| !taken[i] && ids_h[i] == *id) {
                Some(i) if self.eq(&left_h[i], l) => taken[i] = true,
                _ => return false,
            }
        }
        for (c, t) in left_h.iter().zip(&taken) {
            if !*t {
                if !value_extra(c, self.values) {
                    return false;
                }
                self.note(VALUE_RECORDS);
            }
        }
        true
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
            } else if i < hs.len() && declared_extra(&hs[i], self.declared) {
                self.note(DECLARED_VALUE);
                i += 1;
            } else if i < hs.len() && value_extra(&hs[i], self.values) {
                self.note(VALUE_RECORDS);
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
    // A prefix is not the output: what follows the cap is not compared as
    // missing (WP-210: a large module's relocations were cut off).
    if output.truncated {
        return Err(format!("{what} wrote more than {} bytes", process::OUTPUT_CAP));
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
    let dump = |object: &Path, what: &[&std::ffi::OsStr]| {
        run_tool(
            &tools.dwarfdump,
            |c| {
                c.args(what).arg(object).env("LC_ALL", "C");
            },
            "llvm-dwarfdump",
        )
    };
    for (object, who) in [(&hod_o, "hod"), (&llc_o, "llc")] {
        if let Err(e) = dump(object, &["--verify".as_ref()]) {
            return Outcome::Failed(format!("{who}'s object does not verify: {e}"));
        }
    }
    // The DIE trees go through files: a large module's dump is more than
    // the 16 MiB a captured stream keeps.
    let info = |object: &Path, who: &str| {
        let file = dir.join(format!("{who}.info.txt"));
        dump(object, &["--debug-info".as_ref(), "-o".as_ref(), file.as_os_str()])?;
        let text =
            fs::read_to_string(&file).map_err(|e| format!("cannot read '{}': {e}", file.display()));
        let _ = fs::remove_file(&file);
        text
    };
    let (ours, theirs) = match (info(&hod_o, "hod"), info(&llc_o, "llc")) {
        (Ok(a), Ok(b)) => (parse_tree(&a), parse_tree(&b)),
        (Err(e), _) | (_, Err(e)) => return Outcome::Failed(e),
    };
    // Only `.debug_info`'s relocations: all of a large object's are more
    // than a captured stream keeps.
    let relocations = |object: &Path| {
        run_tool(
            &tools.objdump,
            |c| {
                c.args(["-r", "-j", ".rela.debug_info"]).arg(object).env("LC_ALL", "C");
            },
            "llvm-objdump",
        )
        .map(|t| info_relocations(&t))
    };
    let (ours_r, theirs_r) = match (relocations(&hod_o), relocations(&llc_o)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => return Outcome::Failed(e),
    };
    let is_constant = text.contains("@llvm.is.constant");
    let facts = Facts {
        is_constant,
        llc19: major == 19,
        protected: protects(&text),
        declared: declared_on_phi(&text),
        values: described_by_values(&text),
        frameless: without_frame_pointer(&text),
    };
    compare_with(&ours, &theirs, &ours_r, &theirs_r, &facts)
}

/// WP-221 (`spec/dwarf.md` §6.30), the condition of the class `declared
/// value`: the names of the variables a `#dbg_declare` (or a call of
/// `llvm.dbg.declare`) of module text `text` declares on a `phi`'s
/// result, each as the `DW_AT_name` line of its DIE.
pub fn declared_on_phi(text: &str) -> Vec<String> {
    let mut nodes: Vec<&str> = Vec::new();
    let mut phis: Vec<&str> = Vec::new();
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with("define ") {
            phis.clear();
        }
        if let Some((name, rest)) = t.split_once(" = ")
            && name.starts_with('%')
            && rest.starts_with("phi ")
        {
            phis.push(name);
        }
        let args = t
            .strip_prefix("#dbg_declare(ptr ")
            .or_else(|| t.split_once("@llvm.dbg.declare(metadata ptr ").map(|(_, a)| a));
        let Some(args) = args else { continue };
        let mut parts = args.split(", ");
        let (Some(address), Some(var)) = (parts.next(), parts.next()) else { continue };
        let var = var.trim_start_matches("metadata ");
        if phis.contains(&address) && var.starts_with('!') && !nodes.contains(&var) {
            nodes.push(var);
        }
    }
    let mut names = Vec::new();
    for line in text.lines() {
        let Some((node, rest)) = line.split_once(" = ") else { continue };
        if !nodes.contains(&node) {
            continue;
        }
        let Some(rest) = rest.trim_start_matches("distinct ").strip_prefix("!DILocalVariable(")
        else {
            continue;
        };
        if let Some(at) = rest.find("name: \"")
            && let Some(len) = rest[at + 7..].find('"')
        {
            let name = format!("DW_AT_name (\"{}\")", &rest[at + 7..at + 7 + len]);
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
}

/// Whether the module text `text` names a stack-protector attribute
/// (`ssp`, `sspstrong`, `sspreq` as a word), the condition of the class
/// `stack protector`.
pub fn protects(text: &str) -> bool {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '"'))
        .any(|word| matches!(word, "ssp" | "sspstrong" | "sspreq"))
}

/// What the module says about the classes that depend on it.
#[derive(Clone, Debug, Default)]
pub struct Facts {
    /// The module calls `llvm.is.constant`.
    pub is_constant: bool,
    /// The reference is `llc` 19.
    pub llc19: bool,
    /// A function has a stack-protector attribute ([`protects`]).
    pub protected: bool,
    /// [`declared_on_phi`].
    pub declared: Vec<String>,
    /// WP-260: [`described_by_values`].
    pub values: Vec<String>,
    /// WP-260: [`without_frame_pointer`].
    pub frameless: Vec<String>,
}

/// WP-260 (`spec/dwarf.md` §6.37), the condition of the class `value
/// records`: the names of the variables a `#dbg_value` record (or a call
/// of `llvm.dbg.value`) of module text `text` describes.
pub fn described_by_values(text: &str) -> Vec<String> {
    let mut nodes: Vec<&str> = Vec::new();
    for line in text.lines() {
        let t = line.trim_start();
        let args = t
            .strip_prefix("#dbg_value(")
            .or_else(|| t.split_once("@llvm.dbg.value(").map(|(_, a)| a));
        let Some(args) = args else { continue };
        // The second operand at the outer level (the first may be a
        // `!DIArgList(...)` with commas of its own).
        let mut depth = 0usize;
        let mut start = None;
        for (i, c) in args.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => depth = depth.saturating_sub(1),
                ',' if depth == 0 => {
                    if start.is_some() {
                        break;
                    }
                    start = Some(i + 1);
                }
                _ => {}
            }
        }
        let Some(start) = start else { continue };
        let var = args[start..].trim_start().trim_start_matches("metadata ");
        let end = var.find(|c: char| c != '!' && !c.is_ascii_digit()).unwrap_or(var.len());
        let var = &var[..end];
        if var.len() > 1 && !nodes.contains(&var) {
            nodes.push(var);
        }
    }
    local_variable_names(text, &nodes)
}

/// The names of the `!DILocalVariable` nodes `nodes` of module text `text`.
fn local_variable_names(text: &str, nodes: &[&str]) -> Vec<String> {
    let mut names = Vec::new();
    for line in text.lines() {
        let Some((node, rest)) = line.split_once(" = ") else { continue };
        if !nodes.contains(&node) {
            continue;
        }
        let Some(rest) = rest.trim_start_matches("distinct ").strip_prefix("!DILocalVariable(")
        else {
            continue;
        };
        // A variable without a name (an unnamed parameter) is "".
        let name = match rest.find("name: \"") {
            Some(at) => rest[at + 7..].split('"').next().unwrap_or_default().to_owned(),
            None => String::new(),
        };
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// WP-260 (§6.37), the condition of the class `no frame pointer`: the
/// functions module text `text` defines without asking for a frame
/// pointer (no `"frame-pointer"` of `all`, `non-leaf...` or `reserved`
/// on the definition or in one of its attribute groups).
pub fn without_frame_pointer(text: &str) -> Vec<String> {
    let keeps = |s: &str| {
        [
            "\"frame-pointer\"=\"all\"",
            "\"frame-pointer\"=\"non-leaf",
            "\"frame-pointer\"=\"reserved\"",
        ]
        .iter()
        .any(|k| s.contains(k))
    };
    let groups: Vec<&str> = text
        .lines()
        .filter_map(|l| {
            let (name, rest) = l.strip_prefix("attributes ")?.split_once(" = ")?;
            keeps(rest).then_some(name)
        })
        .collect();
    let mut names = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("define ") else { continue };
        let Some(at) = rest.find('@') else { continue };
        let name = &rest[at + 1..];
        let Some(end) = name.find('(') else { continue };
        let name = name[..end].trim_matches('"');
        let kept = keeps(rest) || rest.split_whitespace().any(|w| groups.contains(&w));
        if !kept && !names.iter().any(|n| n == name) {
            names.push(name.to_owned());
        }
    }
    names
}

/// The outcome of a module from its trees and relocations.
pub fn compare(
    ours: &[Node],
    theirs: &[Node],
    ours_r: &[(String, String)],
    theirs_r: &[(String, String)],
    is_constant: bool,
    llc19: bool,
    protected: bool,
) -> Outcome {
    let facts = Facts { is_constant, llc19, protected, ..Facts::default() };
    compare_with(ours, theirs, ours_r, theirs_r, &facts)
}

/// [`compare`] with every fact of the module.
pub fn compare_with(
    ours: &[Node],
    theirs: &[Node],
    ours_r: &[(String, String)],
    theirs_r: &[(String, String)],
    facts: &Facts,
) -> Outcome {
    if ours.len() != theirs.len() {
        return Outcome::Differs(format!("{} units, llc's {}", ours.len(), theirs.len()));
    }
    let mut classes: Vec<&'static str> = Vec::new();
    for (h, l) in ours.iter().zip(theirs) {
        if same(h, l) {
            continue;
        }
        match permitted_with(h, l, facts) {
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
    // list's offset against `.debug_loc`, WP-114), those beyond hod's own
    // (hod's lists of declared values, WP-221: a dynamic `alloca`); every
    // other must be equal.
    let lists_of = |s: &str| s == ".debug_loclists" || s == ".debug_loc";
    let mut theirs_r = theirs_r.to_vec();
    if classes.contains(&"sret") {
        drop_surplus(&mut theirs_r, ours_r, lists_of, usize::MAX);
    }
    let theirs_r = theirs_r.as_slice();
    // WP-221: each of hod's lists of a `declared value` adds its
    // relocation against `.debug_loclists` (`.debug_loc`), and in DWARF 4
    // each of its DIEs `llc` does not have its name's against `.debug_str`.
    let mut ours_r = ours_r.to_vec();
    if classes.contains(&DECLARED_VALUE) {
        let lists = ours.iter().map(|u| declared_lists(u, &facts.declared)).sum();
        let dies = ours.iter().map(|u| declared_dies(u, &facts.declared)).sum();
        drop_surplus(&mut ours_r, theirs_r, lists_of, lists);
        drop_surplus(&mut ours_r, theirs_r, |s| s == ".debug_str", dies);
    }
    // WP-260 (§6.37): each of hod's lists of a value variable is
    // `DW_FORM_sec_offset`, a relocation against `.debug_loclists` beyond
    // `llc`'s (its `loclists_base` and `loclistx` lists); hod's
    // `DW_AT_rnglists_base` of `unit range lists` one against
    // `.debug_rnglists`.
    let mut theirs_r = theirs_r.to_vec();
    if classes.contains(&VALUE_RECORDS) {
        let base = |u: &Node| u.attrs.iter().any(|a| a == "DW_AT_loclists_base");
        let mut bases = theirs.iter().zip(ours).filter(|(l, h)| base(l) && !base(h)).count();
        theirs_r.retain(|(_, s)| {
            let drop = bases > 0 && lists_of(s);
            bases -= usize::from(drop);
            !drop
        });
        let lists = ours.iter().map(|u| value_lists(u, &facts.values)).sum();
        drop_surplus(&mut ours_r, &theirs_r, lists_of, lists);
    }
    if classes.contains(&UNIT_RANGE_LISTS) {
        drop_surplus(&mut ours_r, &theirs_r, |s| s == ".debug_rnglists", ours.len());
    }
    let theirs_r = theirs_r.as_slice();
    let ours_r = ours_r.as_slice();
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
            compare(
                std::slice::from_ref(&h),
                std::slice::from_ref(&l),
                &[],
                &[],
                false,
                false,
                false
            ),
            { Outcome::Permitted(vec!["sret"]) }
        );
        // Without llc's location lists, a list is a difference.
        assert!(matches!(
            compare(
                std::slice::from_ref(&h),
                &[unit(false, SRET_LLC)],
                &[],
                &[],
                false,
                false,
                false
            ),
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
            compare(&[v4(SRET_HOD)], &[v4(SRET_LLC)], &[], &loc, false, false, false),
            Outcome::Permitted(vec!["sret"])
        );
        assert!(matches!(
            compare(&[v4(SRET_HOD)], &[v4(IN_SLOT)], &[], &[], false, false, false),
            Outcome::Differs(_)
        ));
    }

    /// WP-183: a guarded slot reached through its pointer slot is the
    /// class `stack protector`, and only in a protected module.
    #[test]
    fn a_guarded_slot_is_permitted_only_when_protected() {
        let var = |at: &str| Node {
            tag: "DW_TAG_compile_unit".into(),
            attrs: vec!["DW_AT_str_offsets_base".into()],
            children: vec![Node {
                tag: "DW_TAG_variable".into(),
                attrs: vec![at.into()],
                children: vec![],
            }],
        };
        assert_eq!(
            compare(&[var(SRET_HOD)], &[var(IN_SLOT)], &[], &[], false, false, true),
            Outcome::Permitted(vec!["stack protector"])
        );
        assert!(matches!(
            compare(&[var(SRET_HOD)], &[var(IN_SLOT)], &[], &[], false, false, false),
            Outcome::Differs(_)
        ));
        assert!(protects("define void @f() #0\nattributes #0 = { sspstrong noinline }"));
        assert!(protects("attributes #1 = { ssp }"));
        assert!(!protects("define void @sspstrong_name() {\n}\n; nossp"));
    }

    /// WP-150: `llc`'s places in a realigned frame are `sp`-relative on
    /// AArch64 as they are `rsp`-relative on x86-64; any other register
    /// is a difference.
    #[test]
    fn realigned_frames_are_permitted_on_both_targets() {
        for tag in ["DW_TAG_variable", "DW_TAG_formal_parameter"] {
            for h in [IN_SLOT, SRET_HOD] {
                for l in [RSP_RELATIVE, SP_RELATIVE] {
                    assert_eq!(location_class(tag, h, l, false, false), Some("realigned frame"));
                }
            }
            assert_eq!(
                location_class(tag, IN_SLOT, "DW_AT_location DW_OP_breg29", false, false),
                None
            );
        }
        assert_eq!(location_class("DW_TAG_member", IN_SLOT, SP_RELATIVE, false, false), None);
    }

    /// WP-195: a variable whose address is in a slot, the slot named
    /// from `sp` or `rsp` by `llc`; a direct slot there is a difference.
    #[test]
    fn realigned_indirect_places_are_permitted() {
        for l in [RSP_RELATIVE_DEREF, SP_RELATIVE_DEREF] {
            assert_eq!(
                location_class("DW_TAG_variable", SRET_HOD, l, false, false),
                Some("realigned frame")
            );
            assert_eq!(location_class("DW_TAG_variable", IN_SLOT, l, false, false), None);
        }
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
        assert_eq!(compare(&tree, &tree, &ours, &ours, false, false, false), Outcome::Same(3));
        assert!(matches!(
            compare(&tree, &tree, &wrong, &ours, false, false, false),
            Outcome::Differs(_)
        ));
    }

    /// WP-210: a tool's output past the capture cap is a failure, not a
    /// shorter listing (a large object's relocations were cut off before
    /// `.debug_info`'s and reported as missing).
    #[test]
    fn output_past_the_cap_is_a_failure() {
        let big = format!("head -c {} /dev/zero", process::OUTPUT_CAP + 1);
        let failed = run_tool(
            Path::new("/bin/sh"),
            |c| {
                c.args(["-c", &big]);
            },
            "sh",
        );
        assert!(failed.is_err_and(|e| e.contains("more than")));
        let fits = run_tool(
            Path::new("/bin/sh"),
            |c| {
                c.args(["-c", "echo ok"]);
            },
            "sh",
        );
        assert_eq!(fits.as_deref(), Ok("ok\n"));
    }

    /// WP-221 (`spec/dwarf.md` §6.30): the variables declared on a `phi`
    /// (records and calls), not those on a slot or another value.
    #[test]
    fn variables_declared_on_a_phi() {
        let text = "define void @f() {\n  %a = alloca i32\n  %h = phi ptr [ %p, %0 ]\n\
                    \x20   #dbg_declare(ptr %h, !7, !DIExpression(), !9)\n\
                    \x20   #dbg_declare(ptr %a, !8, !DIExpression(), !9)\n\
                    \x20 call void @llvm.dbg.declare(metadata ptr %h, metadata !6, metadata !DIExpression())\n}\n\
                    define void @g() {\n    #dbg_declare(ptr %h, !5, !DIExpression(), !9)\n}\n\
                    !5 = !DILocalVariable(name: \"other\", scope: !1)\n\
                    !6 = !DILocalVariable(name: \"old\", scope: !1)\n\
                    !7 = !DILocalVariable(name: \"__promise\", scope: !1)\n\
                    !8 = !DILocalVariable(name: \"slot\", scope: !1)\n";
        assert_eq!(
            declared_on_phi(text),
            ["DW_AT_name (\"old\")", "DW_AT_name (\"__promise\")"].map(str::to_owned)
        );
    }

    /// WP-221: hod's list where `llc` has no location, hod's DIE (and a
    /// block of them) where `llc` has none, and their relocations, only
    /// for the declared variables.
    #[test]
    fn declared_values_are_permitted_by_name() {
        let var = |name: &str, list: bool| Node {
            tag: "DW_TAG_variable".into(),
            attrs: [
                list.then(|| "DW_AT_location list".to_owned()),
                Some(format!("DW_AT_name (\"{name}\")")),
            ]
            .into_iter()
            .flatten()
            .collect(),
            children: vec![],
        };
        let block = |children: Vec<Node>| Node {
            tag: "DW_TAG_lexical_block".into(),
            attrs: vec!["DW_AT_low_pc".into(), "DW_AT_high_pc".into()],
            children,
        };
        let unit = |children: Vec<Node>| Node {
            tag: "DW_TAG_compile_unit".into(),
            attrs: vec!["DW_AT_str_offsets_base".into()],
            children: vec![Node { tag: "DW_TAG_subprogram".into(), attrs: vec![], children }],
        };
        let ours = unit(vec![
            var("__coro_frame", true),
            var("__promise", true),
            block(vec![var("x", true)]),
        ]);
        let theirs = unit(vec![var("__coro_frame", false)]);
        let lists = vec![("R_AARCH64_ABS32".to_owned(), ".debug_loclists".to_owned()); 3];
        let names = ["__coro_frame", "__promise", "x"].map(|n| format!("DW_AT_name (\"{n}\")"));
        let facts = Facts { declared: names.to_vec(), ..Facts::default() };
        let (h, l) = (std::slice::from_ref(&ours), std::slice::from_ref(&theirs));
        assert_eq!(
            compare_with(h, l, &lists, &[], &facts),
            Outcome::Permitted(vec![DECLARED_VALUE])
        );
        // Not declared on a `phi`: a difference.
        let some = Facts { declared: names[..2].to_vec(), ..Facts::default() };
        assert!(matches!(compare_with(h, l, &lists, &[], &some), Outcome::Differs(_)));
        // A relocation more than the lists: a difference.
        let more = vec![("R_AARCH64_ABS32".to_owned(), ".debug_loclists".to_owned()); 4];
        assert!(matches!(compare_with(h, l, &more, &[], &facts), Outcome::Differs(_)));
        // `llc` placing the variable another way is not `llc` dropping it.
        let placed = unit(vec![
            var("__coro_frame", false),
            Node {
                tag: "DW_TAG_variable".into(),
                attrs: vec![
                    "DW_AT_location DW_OP_fbreg".into(),
                    "DW_AT_name (\"__promise\")".into(),
                ],
                children: vec![],
            },
        ]);
        assert!(matches!(
            compare_with(&[ours], &[placed], &lists, &[], &facts),
            Outcome::Differs(_)
        ));
    }
}

/// WP-260 (`spec/dwarf.md` §6.37): the classes `value records`, `no
/// frame pointer` and `unit range lists`, and what they do not cover.
#[cfg(test)]
mod optimized_units {
    use super::*;

    fn node(tag: &str, attrs: &[&str], children: Vec<Node>) -> Node {
        Node { tag: tag.into(), attrs: attrs.iter().map(|a| (*a).to_owned()).collect(), children }
    }

    fn var(location: Option<&str>, name: &str) -> Node {
        let name = format!("DW_AT_name (\"{name}\")");
        let mut attrs: Vec<&str> = location.into_iter().collect();
        attrs.extend([name.as_str(), "DW_AT_type (\"int\")"]);
        node("DW_TAG_variable", &attrs, vec![])
    }

    fn this() -> Node {
        node("DW_TAG_formal_parameter", &["DW_AT_abstract_origin (\"this\")"], vec![])
    }

    const LIST: Option<&str> = Some("DW_AT_location list");

    /// hod's unit and `llc`'s of the shapes of the corpus's `-O2` unit
    /// (`lifetime-markers-O2-g-fpic-run.cpp`, reduced).
    fn units(hod_vars: Vec<Node>, llc_vars: Vec<Node>) -> (Node, Node) {
        let f = |frame: &str, vars: Vec<Node>, inlined: Vec<Node>| {
            let mut children = vars;
            children.push(node(
                "DW_TAG_inlined_subroutine",
                &["DW_AT_abstract_origin (\"_ZN5GuardD2Ev\")", "DW_AT_low_pc", "DW_AT_high_pc"],
                inlined,
            ));
            children.push(node(
                "DW_TAG_call_site",
                &["DW_AT_call_origin (\"_Z4makei\")", "DW_AT_call_return_pc"],
                vec![],
            ));
            node(
                "DW_TAG_subprogram",
                &["DW_AT_low_pc", "DW_AT_high_pc", frame, "DW_AT_linkage_name (\"_Z7guardedi\")"],
                children,
            )
        };
        let block = |ranges: bool| {
            let at: &[&str] =
                if ranges { &["DW_AT_ranges"] } else { &["DW_AT_low_pc", "DW_AT_high_pc"] };
            node("DW_TAG_lexical_block", at, vec![])
        };
        let mut hod_vars = hod_vars;
        hod_vars.push(block(true));
        let mut llc_vars = llc_vars;
        llc_vars.push(block(false));
        let int = node("DW_TAG_base_type", &["DW_AT_name (\"int\")"], vec![]);
        let ptr = node("DW_TAG_pointer_type", &["DW_AT_type (\"int\")"], vec![]);
        let hod = node(
            "DW_TAG_compile_unit",
            &["DW_AT_str_offsets_base", "DW_AT_rnglists_base"],
            vec![
                int.clone(),
                f("DW_AT_frame_base DW_OP_reg6", hod_vars, vec![this()]),
                ptr.clone(),
            ],
        );
        let llc = node(
            "DW_TAG_compile_unit",
            &["DW_AT_str_offsets_base", "DW_AT_loclists_base"],
            vec![ptr, int, f("DW_AT_frame_base DW_OP_reg7", llc_vars, vec![])],
        );
        (hod, llc)
    }

    fn facts() -> Facts {
        Facts {
            values: ["n", "g1", "g2", "s", "this"].map(str::to_owned).to_vec(),
            frameless: vec!["_Z7guardedi".to_owned()],
            ..Facts::default()
        }
    }

    fn relocations(lists: usize, rnglists: bool) -> Vec<(String, String)> {
        let r = |s: &str| ("R_X86_64_32".to_owned(), s.to_owned());
        let mut out = vec![r(".debug_loclists"); lists];
        if rnglists {
            out.push(r(".debug_rnglists"));
        }
        out
    }

    fn outcome(h: &Node, l: &Node, ours: &[(String, String)], facts: &Facts) -> Outcome {
        let theirs = relocations(1, false);
        compare_with(std::slice::from_ref(h), std::slice::from_ref(l), ours, &theirs, facts)
    }

    fn hod_vars() -> Vec<Node> {
        vec![var(LIST, "g1"), var(LIST, "g2"), var(None, "n"), var(LIST, "s")]
    }

    /// `llc`'s single expression, `DW_AT_const_value`, list and nothing
    /// against hod's lists and nothing, in another sibling order; hod's
    /// placeless `this` of an inlined instance `llc` drops; `rbp` against
    /// `rsp`; hod's `rnglists_base` for its range list.
    fn llc_vars() -> Vec<Node> {
        vec![
            var(Some("DW_AT_const_value (0)"), "s"),
            var(Some("DW_AT_location DW_OP_lit2, DW_OP_stack_value, DW_OP_piece"), "g2"),
            var(LIST, "g1"),
            var(LIST, "n"),
        ]
    }

    #[test]
    fn the_shapes_of_an_optimized_unit_are_permitted() {
        let (h, l) = units(hod_vars(), llc_vars());
        assert_eq!(
            outcome(&h, &l, &relocations(3, true), &facts()),
            Outcome::Permitted(vec![UNIT_RANGE_LISTS, NO_FRAME_POINTER, VALUE_RECORDS])
        );
    }

    #[test]
    fn what_the_classes_do_not_cover_differs() {
        let differs = |h: &Node, l: &Node, ours: &[(String, String)], facts: &Facts| {
            matches!(outcome(h, l, ours, facts), Outcome::Differs(_))
        };
        let (h, l) = units(hod_vars(), llc_vars());
        let ours = relocations(3, true);
        // Not a variable of value records (a `#dbg_declare`d one, say).
        let mut f = facts();
        f.values.retain(|v| v != "g2");
        assert!(differs(&h, &l, &ours, &f));
        // A module without value records: no comparison by name either.
        let f = Facts { values: vec![], ..facts() };
        assert!(differs(&h, &l, &ours, &f));
        // A function that keeps its frame pointer: `rbp` against `rsp`.
        let f = Facts { frameless: vec![], ..facts() };
        assert!(differs(&h, &l, &ours, &f));
        // hod writing a single expression of its own (not a list).
        let mut hv = hod_vars();
        hv[0] = var(Some("DW_AT_location DW_OP_fbreg"), "g1");
        let (h2, l2) = units(hv, llc_vars());
        assert!(differs(&h2, &l2, &ours, &facts()));
        // Another attribute differing as well (the type).
        let mut lv = llc_vars();
        lv[3].attrs[2] = "DW_AT_type (\"long\")".into();
        let (h2, l2) = units(hod_vars(), lv);
        assert!(differs(&h2, &l2, &ours, &facts()));
        // A DIE of hod's `llc` lacks that has a location, or a name.
        let mut hv = hod_vars();
        hv.push(var(LIST, "g1"));
        let (h2, l2) = units(hv, llc_vars());
        assert!(differs(&h2, &l2, &relocations(4, true), &facts()));
        // A DIE of `llc`'s hod lacks.
        let mut lv = llc_vars();
        lv.push(var(None, "g1"));
        let (h2, l2) = units(hod_vars(), lv);
        assert!(differs(&h2, &l2, &ours, &facts()));
        // A relocation against `.debug_loclists` beyond hod's lists.
        assert!(differs(&h, &l, &relocations(4, true), &facts()));
        // `rnglists_base` with no range list beyond `llc`'s.
        let mut l2 = l.clone();
        l2.children[2].children[4].attrs = vec!["DW_AT_ranges".into()];
        assert!(differs(&h, &l2, &ours, &facts()));
    }

    #[test]
    fn value_variables_and_frameless_functions_of_a_module() {
        let text = "define void @f(i32 %0, ptr %1) #0 {\n\
                    \x20   #dbg_value(i32 %0, !5, !DIExpression(), !9)\n\
                    \x20   #dbg_value(!DIArgList(ptr %1, i64 4), !6, !DIExpression(DW_OP_LLVM_arg, 0), !9)\n\
                    \x20   #dbg_value(ptr %1, !7, !DIExpression(), !9)\n\
                    \x20   #dbg_declare(ptr %1, !8, !DIExpression(), !9)\n\
                    \x20 call void @llvm.dbg.value(metadata i32 0, metadata !10, metadata !DIExpression())\n}\n\
                    define void @g() #1 {\n  ret void\n}\n\
                    define void @h() \"frame-pointer\"=\"all\" {\n  ret void\n}\n\
                    define void @\"q r\"() {\n  ret void\n}\n\
                    attributes #0 = { \"frame-pointer\"=\"none\" }\n\
                    attributes #1 = { \"frame-pointer\"=\"non-leaf\" }\n\
                    !5 = !DILocalVariable(name: \"n\", arg: 1, scope: !1)\n\
                    !6 = !DILocalVariable(name: \"__begin1\", scope: !1)\n\
                    !7 = !DILocalVariable(arg: 2, scope: !1)\n\
                    !8 = !DILocalVariable(name: \"declared\", scope: !1)\n\
                    !10 = distinct !DILocalVariable(name: \"old\", scope: !1)\n";
        assert_eq!(described_by_values(text), ["n", "__begin1", "", "old"].map(str::to_owned));
        assert_eq!(without_frame_pointer(text), ["f", "q r"].map(str::to_owned));
    }

    #[test]
    fn call_site_addresses_are_masked() {
        assert_eq!(
            normalize("DW_AT_call_return_pc\t(0x0000000000000140)", "x86_64"),
            "DW_AT_call_return_pc"
        );
        assert_eq!(normalize("DW_AT_call_pc\t(0x000000000000013c)", "aarch64"), "DW_AT_call_pc");
    }
}
