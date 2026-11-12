//! Inlined subroutines, abstract instances and functions' statics
//! (`spec/dwarf.md` §6.9, §8.8; WP-085), compared with `llc`'s.
//!
//! - the oracle: `data/dies_inline/inline.cpp` and `statics.cpp` by
//!   clang 19 (`llc` 19 and 22), clang 22 and kiln++ (`llc` 22), and the
//!   hand-written `data/dies_inline/hand.ll`: hod's trees are `llc`'s
//!   modulo §8.8's masks, with `DW_AT_abstract_origin` compared by its
//!   target (a block's, which the dump prints as a bare offset, by the
//!   target's path) and `llc` 19's blocks without one (§6.7);
//! - the shapes one by one: nested inlining, an inlined scope with two
//!   ranges, an inlined block, the abstract instance's children, a
//!   function out of line and inlined, statics first;
//! - each refusal of WP-085 by name, in a module with that one
//!   construct;
//! - the corpus and `stl.cpp` survey (`HOD_DIES_CORPUS`).

mod common;
#[path = "dies_support/mod.rs"]
mod support;

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use common::{ll_header, parse};
use hod_a64::{ModuleError, compile_to_object};
use hod_cg::{CgError, Options};
use hod_testutil::{Scratch, run_with, tool};
use support::{Node, count, count_tag, first_difference, hod_object, ir, kiln, llc_object, verify};

fn data(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/dies_inline").join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn dwarfdump() -> Option<std::path::PathBuf> {
    tool("llvm-dwarfdump-22").or_else(|| tool("llvm-dwarfdump"))
}

/// `--debug-info` of `object`, with every bare-offset reference (a
/// block's `DW_AT_abstract_origin`) given its target's tag, so the tree
/// compares it by what it names; for `llc` 19 (`strip`), a block's
/// `DW_AT_abstract_origin` is dropped (§6.7).
fn dump(dwarfdump: &Path, object: &Path, strip: bool) -> String {
    let mut command = Command::new(dwarfdump);
    command.arg("--debug-info").arg(object);
    let out = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(out.success(), "{}", out.stderr_text());
    let text = out.stdout_text();
    let mut tag_at = HashMap::new();
    for line in text.lines() {
        if let Some((off, rest)) = line.split_once(':')
            && let Some(hex) = off.strip_prefix("0x")
            && let Ok(o) = u64::from_str_radix(hex, 16)
        {
            tag_at.insert(o, rest.trim().to_owned());
        }
    }
    let mut out = String::with_capacity(text.len());
    let mut tag = String::new();
    for line in text.lines() {
        if let Some((off, rest)) = line.split_once(':')
            && off.starts_with("0x")
        {
            rest.trim().clone_into(&mut tag);
        }
        let t = line.trim_start();
        if let Some(v) = t.strip_prefix("DW_AT_abstract_origin")
            && let Some(hex) = v.trim().strip_prefix("(0x").and_then(|h| h.strip_suffix(')'))
            && let Ok(o) = u64::from_str_radix(hex, 16)
        {
            if strip && tag == "DW_TAG_lexical_block" {
                continue;
            }
            let target = tag_at.get(&o).map_or("?", String::as_str);
            out.push_str(&line.replace(&format!("(0x{hex})"), &format!("(0x{hex} \"{target}\")")));
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

fn tree(dwarfdump: &Path, object: &Path, strip: bool) -> Node {
    let mut units = support::parse_tree(&dump(dwarfdump, object, strip));
    assert_eq!(units.len(), 1, "{}: one unit", object.display());
    units.remove(0)
}

#[derive(Default)]
struct Tally {
    objects: usize,
    dies: usize,
    inlined: usize,
    abstracts: usize,
}

/// hod's tree of `text` against `llc` `major`'s, both verified.
fn compare(scratch: &Scratch, major: u32, name: &str, text: &str, tally: &mut Tally) -> Node {
    let Some(llc) = tool(&format!("llc-{major}")) else { panic!("no llc-{major}") };
    let Some(dd) = dwarfdump() else { panic!("no llvm-dwarfdump") };
    let h = hod_object(scratch, &format!("{name}-hod"), text);
    let l = llc_object(scratch, &llc, &format!("{name}-llc{major}"), text);
    verify(&dd, &h);
    verify(&dd, &l);
    let th = tree(&dd, &h, major < 21);
    let tl = tree(&dd, &l, false);
    if let Some(d) = first_difference(&th, &tl) {
        panic!("{name} (llc {major}): hod's tree, then llc's, differ at {d}");
    }
    tally.objects += 2;
    tally.dies += count(&th);
    tally.inlined += count_tag(&th, "DW_TAG_inlined_subroutine");
    tally.abstracts += abstracts(&th);
    th
}

fn abstracts(n: &Node) -> usize {
    usize::from(n.attrs.iter().any(|a| a.starts_with("DW_AT_inline")))
        + n.children.iter().map(abstracts).sum::<usize>()
}

fn has(n: &Node, attr: &str) -> bool {
    n.attrs.iter().any(|a| a.starts_with(attr))
}

/// The subprogram of `unit` whose `DW_AT_name` is `name`, and that has code.
fn concrete<'a>(unit: &'a Node, name: &str) -> &'a Node {
    let want = format!("DW_AT_name (\"{name}\")");
    unit.children
        .iter()
        .find(|c| c.tag == "DW_TAG_subprogram" && has(c, "DW_AT_low_pc") && c.attrs.contains(&want))
        .unwrap_or_else(|| panic!("no concrete {name}"))
}

fn children<'a>(n: &'a Node, tag: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
    n.children.iter().filter(move |c| c.tag == tag)
}

/// The shapes `hand.ll` exists for, in hod's tree (which equals `llc`'s).
fn hand_shapes(unit: &Node) {
    const INLINED: &str = "DW_TAG_inlined_subroutine";
    // One abstract instance of `callee`, with its block.
    let abs: Vec<_> =
        children(unit, "DW_TAG_subprogram").filter(|c| has(c, "DW_AT_inline")).collect();
    assert_eq!(abs.len(), 2, "callee and outer: abstract instances");
    // @split: the inlined call and its block have two ranges or more.
    let split = concrete(unit, "split");
    let call = children(split, INLINED).next().expect("split: an inlined call");
    assert!(has(call, "DW_AT_ranges") && !has(call, "DW_AT_low_pc"), "split: ranges");
    let block = children(call, "DW_TAG_lexical_block").next().expect("split: the inlined block");
    assert!(has(block, "DW_AT_ranges"), "split: the block's ranges");
    assert_eq!(children(block, "DW_TAG_variable").count(), 1, "split: t");
    // @twice: two instances, the second at column 0 and with `u`.
    let twice = concrete(unit, "twice");
    let calls: Vec<_> = children(twice, INLINED).collect();
    assert_eq!(calls.len(), 2, "twice: two inlined calls");
    assert!(has(calls[0], "DW_AT_call_column") && !has(calls[1], "DW_AT_call_column"));
    assert_eq!(calls[0].children.len() + 1, calls[1].children.len(), "twice: u in the second");
    // @nested: callee inlined into the inlined outer, whose var-less
    // block's label is the inlined subroutine's.
    let nested = concrete(unit, "nested");
    let outer = children(nested, INLINED).next().expect("nested: outer");
    assert_eq!(children(outer, INLINED).count(), 1, "nested: callee in outer");
    assert_eq!(children(outer, "DW_TAG_label").count(), 1, "nested: the label");
    assert_eq!(children(outer, "DW_TAG_lexical_block").count(), 0, "nested: no block");
}

/// `hand.ll` for LLVM `major`.
fn hand(major: u32) -> String {
    format!("{}{}", ll_header(major), data("hand.ll"))
}

/// §8.8: the sources by every compiler, and `hand.ll`.
#[test]
fn inline_trees_match_llc() {
    let scratch = Scratch::new("dies-inline-oracle");
    let mut tally = Tally::default();
    let Some(clangxx19) = tool("clang++-19") else { return };
    let Some(clangxx22) = tool("clang++-22") else { return };
    for source in ["inline.cpp", "statics.cpp"] {
        let stem = source.split('.').next().unwrap();
        let path = format!("../dies_inline/{source}");
        let text = ir(&clangxx19, &scratch, &format!("{stem}-c19"), &path);
        for major in [19, 22] {
            // `llc` 19 puts the static of a function that is only
            // inlined under a DIE of its own (§6.7): not compared.
            if major == 19 && stem == "statics" {
                continue;
            }
            compare(&scratch, major, &format!("{stem}-c19"), &text, &mut tally);
        }
        let text = ir(&clangxx22, &scratch, &format!("{stem}-c22"), &path);
        compare(&scratch, 22, &format!("{stem}-c22"), &text, &mut tally);
        if let Some(k) = kiln() {
            let text = ir(&k, &scratch, &format!("{stem}-kiln"), &path);
            compare(&scratch, 22, &format!("{stem}-kiln"), &text, &mut tally);
        }
    }
    for major in [19, 22] {
        let t = compare(&scratch, major, &format!("hand{major}"), &hand(major), &mut tally);
        hand_shapes(&t);
    }
    assert!(tally.inlined >= 20, "{} inlined subroutines", tally.inlined);
    eprintln!(
        "trees equal to llc's: {} objects, {} DIEs of hod's ({} inlined subroutines, {} abstract instances)",
        tally.objects, tally.dies, tally.inlined, tally.abstracts
    );
}

fn refusal(text: &str) -> CgError {
    let (module, bodies) = parse(text);
    match compile_to_object(&module, &bodies, &Options::default()) {
        Err(ModuleError::Emit(e) | ModuleError::Function { error: e, .. }) => e,
        other => panic!("{:?}\n{text}", other.map(|b| b.len())),
    }
}

/// `hand.ll` for LLVM 22 with each `(from, to)` replaced once.
fn hand_with(edits: &[(&str, &str)]) -> String {
    let mut text = hand(22);
    for (from, to) in edits {
        assert_eq!(text.matches(from).count(), 1, "{from}");
        text = text.replacen(from, to, 1);
    }
    text
}

/// Each refusal of WP-085 (§6.9), in `hand.ll` with that one construct.
/// (`debug:inlined-member`, an abstract instance of a member function,
/// is not tested: it stands for WP-059's declarations, and goes with them.)
#[test]
fn refusals_are_named() {
    let base = hand(22);
    let (module, bodies) = parse(&base);
    compile_to_object(&module, &bodies, &Options::default()).expect("the base case");
    let cases: [(&str, String, &str); 5] = [
        (
            "debug:inlined-discriminator",
            hand_with(&[(
                "!18 = !DILocation(line: 3, column: 20, scope: !10)",
                "!18 = !DILocation(line: 3, column: 20, scope: !19)\n!19 = !DILexicalBlockFile(scope: !10, file: !1, discriminator: 1)",
            )]),
            "a call in a block file with a discriminator",
        ),
        (
            "debug:retained-nodes",
            hand_with(&[(
                "unit: !0, retainedNodes: !4)\n!31",
                "unit: !0, retainedNodes: !36)\n!36 = !{!33}\n!31",
            )]),
            "an inlined subprogram with retained nodes",
        ),
        (
            "debug:inlined-foreign-unit",
            hand_with(&[
                ("!llvm.dbg.cu = !{!0}", "!llvm.dbg.cu = !{!0, !100}"),
                (
                    "nameTableKind: None)\n!1 =",
                    "nameTableKind: None)\n!100 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !7, producer: \"hand\", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, splitDebugInlining: false, nameTableKind: None)\n!1 =",
                ),
                ("unit: !0, retainedNodes: !4)\n!31", "unit: !100, retainedNodes: !4)\n!31"),
            ]),
            "code inlined from another unit's subprogram",
        ),
        (
            "debug:local-static",
            hand_with(&[
                (
                    "@g = dso_local global i32 0, align 4",
                    "@g = dso_local global i32 0, align 4\n@s = internal global i32 0, align 4, !dbg !37",
                ),
                ("nameTableKind: None)\n!1 =", "nameTableKind: None, globals: !39)\n!1 ="),
                (
                    "!40 =",
                    "!37 = !DIGlobalVariableExpression(var: !38, expr: !DIExpression())\n!38 = distinct !DIGlobalVariable(name: \"s\", scope: !35, file: !7, line: 3, type: !6, isLocal: true, isDefinition: true)\n!39 = !{!37}\n!40 =",
                ),
            ]),
            "a static scoped in an inlined function's block",
        ),
        (
            "debug:inlined-local-type",
            hand_with(&[
                (
                    "define dso_local i32 @split",
                    "define dso_local i32 @_Z6calleei(i32 %v) !dbg !12 {\nentry:\n  %v.addr = alloca i32, align 4\n  store i32 %v, ptr %v.addr, align 4\n    #dbg_declare(ptr %v.addr, !31, !DIExpression(), !37)\n  ret i32 %v, !dbg !37\n}\n\ndefine dso_local i32 @split",
                ),
                (
                    "line: 1, type: !6)\n!32",
                    "line: 1, type: !36)\n!36 = !DIDerivedType(tag: DW_TAG_typedef, name: \"T\", scope: !12, file: !7, line: 1, baseType: !6)\n!37 = !DILocation(line: 1, column: 1, scope: !12)\n!32",
                ),
            ]),
            "a type local to a function out of line and inlined",
        ),
    ];
    for (name, text, what) in &cases {
        assert_eq!(refusal(text), CgError::unsupported(name), "{what}");
    }
}
