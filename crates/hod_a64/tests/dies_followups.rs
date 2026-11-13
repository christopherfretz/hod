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
        let text: String =
            text.lines().filter(|l| !l.starts_with("@g ")).map(|l| format!("{l}\n")).collect();
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
