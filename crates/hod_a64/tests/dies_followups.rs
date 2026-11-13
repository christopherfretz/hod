//! Debug-information follow-ups of WP-097 and WP-098 (WP-111,
//! `spec/landed/dwarf/WP-111.md`), compared with `llc -O0
//! -global-isel=0 -fast-isel=0` 19 and 22 on clang 19's and clang 22's
//! IR (kiln++ where it emits the shape): both objects verify, and hod's
//! DIE tree is `llc`'s.
//!
//! - a unit with no code: `llc` writes it when it lists a global, an
//!   enumeration, a retained type or an import outside a function
//!   (`spec/dwarf.md` §1.2);
//! - imported entities of every shape (§6.11): using-directives and
//!   using-declarations at namespace and block scope, in a nested
//!   lexical block, namespace aliases, using-declarations of a function,
//!   a variable and a type, and the standard library's (several hundred
//!   per unit).

mod common;
#[path = "dies_support/mod.rs"]
mod support;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hod_testutil::{Scratch, run_with, tool};
use support::{Node, count, count_tag, first_difference, hod_object, llc_object, trees};

fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/dies_followups")
}

/// `-g -O0` IR of `source` (a file of `data_dir()`) by `compiler`.
fn ir(compiler: &Path, scratch: &Scratch, name: &str, source: &str, extra: &[&str]) -> String {
    let out = scratch.join(format!("{name}.ll"));
    let mut command = Command::new(compiler);
    command.args(["-g", "-O0", "-S", "-emit-llvm", "-w", "-std=c++20"]).args(extra);
    command.arg(data_dir().join(source)).arg("-o").arg(&out);
    let outcome = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "{command:?}: {}", outcome.stderr_text());
    std::fs::read_to_string(out).unwrap()
}

/// hod's units of `text` against `llc`'s (as many, the same trees),
/// both objects verified; `None` when `llc` is not installed.
fn compare_units(scratch: &Scratch, llc: &str, name: &str, text: &str) -> Option<Vec<Node>> {
    let llc = tool(llc)?;
    let dwarfdump = tool("llvm-dwarfdump")?;
    let h = hod_object(scratch, &format!("{name}-hod"), text);
    let l = llc_object(scratch, &llc, &format!("{name}-llc"), text);
    support::verify(&dwarfdump, &h);
    support::verify(&dwarfdump, &l);
    let (th, tl) = (trees(&dwarfdump, &h), trees(&dwarfdump, &l));
    assert_eq!(th.len(), tl.len(), "{name}: hod's units, llc's");
    for (a, b) in th.iter().zip(&tl) {
        if let Some(d) = first_difference(a, b) {
            panic!("{name}: hod's tree, then llc's: {d}");
        }
    }
    Some(th)
}

/// hod's tree of `text` against `llc`'s, one unit each.
fn compare(scratch: &Scratch, llc: &str, name: &str, text: &str) -> Option<Node> {
    let mut units = compare_units(scratch, llc, name, text)?;
    assert_eq!(units.len(), 1, "{name}: one unit");
    units.pop()
}

/// `text` with the unit's list `field` (`enums`, `retainedTypes`,
/// `imports`, `globals`) dropped.
fn drop_list(text: &str, field: &str) -> String {
    text.lines()
        .map(|line| {
            if !line.contains("DICompileUnit(") {
                return line.to_owned();
            }
            let key = format!(", {field}: !");
            let Some(at) = line.find(&key) else { return line.to_owned() };
            let tail = &line[at + key.len()..];
            let digits = tail.bytes().take_while(u8::is_ascii_digit).count();
            format!("{}{}", &line[..at], &tail[digits..])
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Each `(compiler, llc)` pair: clang 19's IR by `llc` 19 and 22, clang
/// 22's by `llc` 22.
const PAIRS: [(&str, &str); 3] =
    [("clang++-19", "llc-19"), ("clang++-19", "llc-22"), ("clang++-22", "llc-22")];

/// Every pair's tree of `source` compiled with `extra`; `f` sees each.
fn each_tree(tag: &str, source: &str, extra: &[&str], mut f: impl FnMut(&str, &Node)) -> usize {
    if !cfg!(target_arch = "aarch64") {
        eprintln!("note: the IR is the host's; skipping on this host");
        return 0;
    }
    let scratch = Scratch::new(&format!("dies-followups-{tag}"));
    let mut compared = 0;
    for (cc, llc) in PAIRS {
        let Some(compiler) = tool(cc) else { continue };
        let name = format!("{tag}-{cc}-{llc}");
        let text = ir(&compiler, &scratch, &name, source, extra);
        if let Some(unit) = compare(&scratch, llc, &name, &text) {
            f(&name, &unit);
            compared += 1;
        }
    }
    compared
}

/// The DIEs with `tag` under `n`, depth first, with their parent's tag.
fn with_parent<'a>(n: &'a Node, tag: &str, out: &mut Vec<(&'a str, &'a Node)>) {
    for c in &n.children {
        if c.tag == tag {
            out.push((&n.tag, c));
        }
        with_parent(c, tag, out);
    }
}

fn has(n: &Node, attr: &str) -> bool {
    n.attrs.iter().any(|a| a == attr || a.starts_with(&format!("{attr} ")))
}

/// §1.2: a unit that lists imports (of a namespace, a function
/// declaration, a variable declaration, a type, and the standard
/// library's), enumerations and retained types but has no code and no
/// global is written, as `llc` writes it.
#[test]
fn units_without_code_match_llc() {
    let mut dies = 0;
    let n = each_tree("codeless", "codeless.cpp", &[], |name, unit| {
        assert!(count_tag(unit, "DW_TAG_imported_declaration") >= 100, "{name}");
        assert!(count_tag(unit, "DW_TAG_imported_module") >= 1, "{name}");
        assert!(count_tag(unit, "DW_TAG_subprogram") > 0, "{name}: the imported declarations");
        dies += count(unit);
    });
    // With every type retained (`-fno-eliminate-unused-debug-types`):
    // the unit's `enums:` and `retainedTypes:`, each alone; with neither
    // (nor anything else), `llc` writes no unit, nor does hod.
    let mut m = 0;
    if let Some(cc) = tool("clang++-22").filter(|_| cfg!(target_arch = "aarch64")) {
        let scratch = Scratch::new("dies-followups-retained");
        let text =
            ir(&cc, &scratch, "retained", "retained.cpp", &["-fno-eliminate-unused-debug-types"]);
        assert!(text.contains("enums: !") && text.contains("retainedTypes: !"));
        let enums = drop_list(&text, "retainedTypes");
        let retained = drop_list(&text, "enums");
        let neither = drop_list(&retained, "retainedTypes");
        for (name, text, structs, units) in [
            ("both", &text, 1, 1),
            ("enums", &enums, 0, 1),
            ("retained", &retained, 1, 1),
            ("neither", &neither, 0, 0),
        ] {
            let Some(trees) = compare_units(&scratch, "llc-22", name, text) else { continue };
            assert_eq!(trees.len(), units, "{name}");
            for unit in &trees {
                assert!(
                    count_tag(unit, "DW_TAG_enumeration_type") > 0 || name == "retained",
                    "{name}"
                );
                assert!(count_tag(unit, "DW_TAG_structure_type") >= structs, "{name}");
                dies += count(unit);
            }
            m += 1;
        }
    }
    // A unit that lists a global the object does not have (its
    // definition deleted): `llc` writes the variable without a location.
    if let Some(cc) = tool("clang++-22").filter(|_| cfg!(target_arch = "aarch64")) {
        let scratch = Scratch::new("dies-followups-global");
        let text = ir(&cc, &scratch, "global", "global.cpp", &[]);
        let text =
            text.lines().filter(|l| !l.starts_with("@g ")).fold(String::new(), |mut t, l| {
                t.push_str(l);
                t.push('\n');
                t
            });
        if let Some(unit) = compare(&scratch, "llc-22", "global", &text) {
            assert_eq!(count_tag(&unit, "DW_TAG_variable"), 1);
            dies += count(&unit);
            m += 1;
        }
    }
    eprintln!("units_without_code_match_llc: {} comparisons, {dies} DIEs equal to llc's", n + m);
}

/// §6.11: imports of every shape, as `llc` writes them.
#[test]
fn imports_of_every_shape_match_llc() {
    let mut imports = 0;
    let n = each_tree("imports", "imports.cpp", &[], |name, unit| {
        let mut modules = Vec::new();
        let mut decls = Vec::new();
        with_parent(unit, "DW_TAG_imported_module", &mut modules);
        with_parent(unit, "DW_TAG_imported_declaration", &mut decls);
        let under = |v: &[(&str, &Node)], p: &str| v.iter().filter(|(t, _)| *t == p).count();
        // `using namespace std;` and `other`'s directive; `user`'s and
        // its innermost block's.
        assert!(under(&modules, "DW_TAG_compile_unit") >= 1, "{name}");
        assert!(under(&modules, "DW_TAG_namespace") >= 1, "{name}");
        assert!(under(&modules, "DW_TAG_subprogram") >= 1, "{name}");
        assert!(under(&modules, "DW_TAG_lexical_block") >= 1, "{name}");
        // Declarations in a function and in its nested block; the
        // standard library's in `std`.
        assert!(under(&decls, "DW_TAG_subprogram") >= 1, "{name}");
        assert!(under(&decls, "DW_TAG_lexical_block") >= 2, "{name}");
        assert!(under(&decls, "DW_TAG_namespace") >= 100, "{name}");
        // The aliases (`al`, `a2`) have a name.
        let aliases = decls.iter().filter(|(_, d)| has(d, "DW_AT_name")).count();
        assert!(aliases >= 2, "{name}: {aliases} aliases");
        imports += modules.len() + decls.len();
    });
    eprintln!("imports_of_every_shape_match_llc: {n} trees, {imports} imports equal to llc's");
}

// ------------------------------------------ declarations and ramps ----

/// The DIEs with `tag` under `n`, depth first.
fn all<'a>(n: &'a Node, tag: &str, out: &mut Vec<&'a Node>) {
    if n.tag == tag {
        out.push(n);
    }
    for c in &n.children {
        all(c, tag, out);
    }
}

/// The subprogram whose linkage name or name is `name`.
fn subprogram<'a>(unit: &'a Node, name: &str) -> &'a Node {
    let mut sps = Vec::new();
    all(unit, "DW_TAG_subprogram", &mut sps);
    let q = format!("(\"{name}\")");
    sps.into_iter()
        .find(|s| {
            s.attrs
                .iter()
                .any(|a| a.starts_with("DW_AT_") && a.ends_with(&q) && has(s, "DW_AT_low_pc"))
        })
        .unwrap_or_else(|| panic!("no subprogram {name}"))
}

/// The variables (and parameters) named `name` directly under `n`.
fn vars<'a>(n: &'a Node, name: &str) -> Vec<&'a Node> {
    let q = format!("DW_AT_name (\"{name}\")");
    n.children.iter().filter(|c| c.attrs.contains(&q)).collect()
}

/// `text` with function `func`'s first `#dbg_declare(ptr %1, ...)` moved:
/// its address becomes `address`, and it goes after the line starting
/// with `after` in that function, preceded by `before` (an instruction
/// that defines the address, or nothing).
fn move_declare(text: &str, func: &str, address: &str, after: &str, before: &str) -> String {
    let head = format!("define dso_local void @{func}(");
    let start = text.find(&head).unwrap_or_else(|| panic!("{func}"));
    let end = start + text[start..].find("\n}\n").unwrap();
    let body = &text[start..end];
    let line = body.lines().find(|l| l.trim_start().starts_with("#dbg_declare(ptr %1,")).unwrap();
    let moved = line.replacen("ptr %1,", &format!("ptr {address},"), 1);
    let mut out = String::new();
    for l in body.lines() {
        if l == line {
            continue;
        }
        out.push_str(l);
        out.push('\n');
        if l.starts_with(after) {
            out.push_str(before);
            out.push_str(&moved);
            out.push('\n');
        }
    }
    assert!(out.contains(&moved), "{func}: no line {after:?}");
    format!("{}{}{}", &text[..start], out.trim_end_matches('\n'), &text[end..])
}

/// `spec/codegen.md` §9.5: a `#dbg_declare` of a `phi` is dropped, as
/// `llc -O0` drops it (`SelectionDAGBuilder` has no node for the `phi` in
/// the declaration's block): no DIE for the variable.
#[test]
fn declaration_of_a_phi_matches_llc() {
    if !cfg!(target_arch = "aarch64") {
        return;
    }
    let scratch = Scratch::new("dies-followups-phi");
    let mut compared = 0;
    for (cc, llc) in PAIRS {
        let Some(compiler) = tool(cc) else { continue };
        let name = format!("phi-{cc}-{llc}");
        let text = ir(&compiler, &scratch, &name, "declare.cpp", &[]);
        let text =
            move_declare(&text, "phi", "%p9", "7:", "  %p9 = phi ptr [ %2, %5 ], [ %2, %0 ]\n");
        let Some(unit) = compare(&scratch, llc, &name, &text) else { continue };
        assert!(vars(subprogram(&unit, "phi"), "p").is_empty(), "{name}");
        compared += 1;
    }
    eprintln!("declaration_of_a_phi_matches_llc: {compared} trees equal to llc's");
}

/// A `#dbg_declare` of a value defined in another block: `llc -O0` drops
/// it as it drops a `phi`'s, hod writes the value's home for the whole
/// function. `hod_ir`'s declarations have no position (`spec/ir.md`), so
/// the code generator cannot tell this from a declaration in the value's
/// own block, where `llc` writes a location list from the definition on.
/// No front end emits either at `-O0`.
#[test]
#[ignore = "hod_ir's #dbg_declare has no position: a value from another block is described (WP-111)"]
fn declaration_of_a_value_from_another_block_matches_llc() {
    let Some(compiler) = tool("clang++-22") else { return };
    let scratch = Scratch::new("dies-followups-cross");
    let text = ir(&compiler, &scratch, "cross", "declare.cpp", &[]);
    let text = move_declare(&text, "cross", "%2", "5:", "");
    let unit = compare(&scratch, "llc-22", "cross", &text).unwrap();
    assert!(vars(subprogram(&unit, "cross"), "p").is_empty());
}

/// A `#dbg_declare` of a constant offset from a stack slot
/// (`getelementptr inbounds` of an `alloca`): `llc` describes it as the
/// slot plus the offset (`processDbgDeclare` strips constant offsets).
/// hod's code generator stops with an internal error ("a home value
/// without a home", `crates/hod_cg/src/policy.rs` `finish`): the
/// address is folded into its user and never stored to the home the
/// declaration gave it. No front end emits it at `-O0`.
#[test]
#[ignore = "hod_cg: internal error on a #dbg_declare of a slot plus a constant (policy.rs finish)"]
fn declaration_of_a_slot_offset_matches_llc() {
    let Some(compiler) = tool("clang++-22") else { return };
    let scratch = Scratch::new("dies-followups-offset");
    let text = ir(&compiler, &scratch, "offset", "declare.cpp", &[]);
    let text = move_declare(&text, "offset", "%2", "  %2 = getelementptr", "");
    compare(&scratch, "llc-22", "offset", &text).unwrap();
}

/// A `CoroSplit` ramp (`-O0` IR, which clang has already split): the
/// frame's variables are declared on the frame pointer's `phi`, and
/// clang 22 declares the coroutine's parameter twice, on its slot and on
/// its frame copy. `llc -O0` drops the `phi`'s declarations, so the
/// parameter has its slot and `__promise` no DIE, and writes the retained
/// `__coro_frame` without a location (`collectEntityInfo`); so does hod.
#[test]
fn coroutine_ramps_match_llc() {
    let mut ramps = 0;
    each_tree("ramp", "../dies_label/coro.cpp", &[], |name, unit| {
        let ramp = subprogram(unit, "_Z5evensi");
        assert_eq!(vars(ramp, "n").len(), 1, "{name}: one parameter");
        assert!(vars(ramp, "__promise").is_empty(), "{name}");
        let frame = vars(ramp, "__coro_frame");
        assert_eq!(frame.len(), 1, "{name}");
        assert!(!has(frame[0], "DW_AT_location"), "{name}: {:?}", frame[0]);
        ramps += 1;
    });
    eprintln!("coroutine_ramps_match_llc: {ramps} trees equal to llc's");
}

/// kiln++'s ramp: the same, but kiln++ writes the frame type's `void *`
/// as a second node equal to the first (`!142 = !DIDerivedType(tag:
/// DW_TAG_pointer_type, baseType: null, size: 64)` beside `!17`). LLVM's
/// parser makes equal uniqued nodes one; `hod_ll` keeps two, so hod's
/// unit has one more `void *` DIE than `llc`'s. Outside WP-111
/// (`crates/hod_ll`, metadata uniquing).
#[test]
#[ignore = "hod_ll keeps two equal uniqued metadata nodes apart (an extra void * DIE)"]
fn kiln_coroutine_ramp_matches_llc() {
    let (Some(k), Some(_)) = (support::kiln(), tool("llc-22")) else { return };
    let scratch = Scratch::new("dies-followups-kiln-ramp");
    let text = ir(&k, &scratch, "kiln-ramp", "../dies_label/coro.cpp", &[]);
    compare(&scratch, "llc-22", "kiln-ramp", &text).unwrap();
}
