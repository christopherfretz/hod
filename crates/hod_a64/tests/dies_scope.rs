//! The DIEs outside function bodies (`spec/dwarf.md` §6.9, WP-059),
//! compared with `llc`'s:
//!
//! - the oracle: `data/dies_scope/{globals,templates,scope}.cpp` by
//!   clang 19 (`llc` 19 and 22), clang 22 and kiln++ (`llc` 22), with
//!   their template parameters (`dies.rs` strips them): hod's DIE trees
//!   are `llc`'s, in `llc`'s order, both verify. Unlike `dies.rs`, a
//!   global's or a template parameter's `DW_AT_location` is compared
//!   (its `DW_OP_addrx` index masked); the debuggers check the addresses
//!   (`dies_scope_debugger.rs`);
//! - what the sources exercise, counted, so a lost DIE kind fails;
//! - the refusals that need code: a template parameter that is the
//!   address of a symbol the object does not define, and an out-of-line
//!   definition whose declaration is virtual outside a class;
//! - a bug outside WP-059 (`types.rs`), ignored.

mod common;
#[path = "dies_support/mod.rs"]
mod support;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use common::{ll_header, parse};
use hod_a64::{ModuleError, compile_to_object};
use hod_cg::Options;
use hod_testutil::{Scratch, run_with, tool};
use support::{Node, count, count_tag, first_difference, hod_object, kiln, llc_object, verify};

fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/dies_scope")
}

/// `-g -O0` IR of `source` by `compiler`, with its template parameters.
fn full_ir(compiler: &Path, scratch: &Scratch, name: &str, source: &str, extra: &[&str]) -> String {
    let out = scratch.join(format!("{name}.ll"));
    let mut command = Command::new(compiler);
    command.args(["-g", "-O0", "-S", "-emit-llvm", "-w", "-std=c++20"]).args(extra);
    command.arg(data_dir().join(source)).arg("-o").arg(&out);
    let outcome = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "{command:?}: {}", outcome.stderr_text());
    std::fs::read_to_string(out).unwrap()
}

/// The trees of `object`, with a `DW_AT_location` that is an address
/// (`DW_OP_addrx`) kept under another name, its index masked.
fn trees(dwarfdump: &Path, object: &Path) -> Vec<Node> {
    let mut command = Command::new(dwarfdump);
    command.arg("--debug-info").arg(object);
    let out = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(out.success(), "{}", out.stderr_text());
    let mut text = String::new();
    for line in out.stdout_text().lines() {
        let t = line.trim_start();
        if t.starts_with("DW_AT_location") && t.contains("DW_OP_addrx") {
            let (_, value) = t.split_once('(').unwrap();
            let masked: Vec<String> =
                value
                    .trim_end_matches(')')
                    .split(", ")
                    .map(|op| {
                        if op.starts_with("DW_OP_addrx") { "DW_OP_addrx".into() } else { op.into() }
                    })
                    .collect();
            writeln!(text, "  DW_AT_address_location ({})", masked.join(", ")).unwrap();
        } else {
            text.push_str(line);
            text.push('\n');
        }
    }
    support::parse_tree(&text)
}

fn dwarfdump() -> PathBuf {
    tool("llvm-dwarfdump-22").or_else(|| tool("llvm-dwarfdump")).expect("no llvm-dwarfdump")
}

/// hod's tree of `text` against `llc` `major`'s, both verified.
fn compare(scratch: &Scratch, major: u32, name: &str, text: &str) -> Option<Node> {
    let llc = tool(&format!("llc-{major}"))?;
    let dwarfdump = dwarfdump();
    let h = hod_object(scratch, &format!("{name}-hod"), text);
    let l = llc_object(scratch, &llc, &format!("{name}-llc{major}"), text);
    verify(&dwarfdump, &h);
    verify(&dwarfdump, &l);
    let (th, tl) = (trees(&dwarfdump, &h), trees(&dwarfdump, &l));
    assert_eq!(th.len(), 1, "{name}: one unit");
    assert_eq!(tl.len(), 1, "{name}: llc's units");
    if let Some(d) = first_difference(&th[0], &tl[0]) {
        panic!("{name} (llc {major}): hod's tree, then llc's, differ at {d}");
    }
    th.into_iter().next()
}

fn has_attr(n: &Node, tag: &str, attr: &str) -> usize {
    usize::from(n.tag == tag && n.attrs.iter().any(|a| a.starts_with(attr)))
        + n.children.iter().map(|c| has_attr(c, tag, attr)).sum::<usize>()
}

/// What each source exercises, in clang's tree: `(tag, attribute or "",
/// at least)`.
/// `(tag, attribute or "", at least)`.
type Want = (&'static str, &'static str, usize);

const WANTS: [(&str, &[Want]); 3] = [
    (
        "globals.cpp",
        &[
            ("DW_TAG_namespace", "", 5),
            ("DW_TAG_variable", "DW_AT_specification", 2),
            ("DW_TAG_variable", "DW_AT_const_value", 4),
            ("DW_TAG_variable", "DW_AT_alignment", 1),
            ("DW_TAG_template_type_parameter", "", 2),
            ("DW_TAG_imported_module", "", 2),
            ("DW_TAG_imported_declaration", "", 3),
            ("DW_TAG_subprogram", "DW_AT_specification", 2),
        ],
    ),
    (
        "templates.cpp",
        &[
            ("DW_TAG_template_value_parameter", "DW_AT_const_value", 5),
            ("DW_TAG_template_value_parameter", "DW_AT_address_location", 2),
            ("DW_TAG_template_value_parameter", "DW_AT_default_value", 1),
            ("DW_TAG_GNU_template_parameter_pack", "", 3),
            ("DW_TAG_GNU_template_template_param", "", 1),
            ("DW_TAG_subprogram", "DW_AT_specification", 6),
        ],
    ),
    (
        "scope.cpp",
        &[
            ("DW_TAG_template_value_parameter", "DW_AT_const_value", 4),
            ("DW_TAG_imported_declaration", "", 3),
            ("DW_TAG_variable", "DW_AT_specification", 3),
            ("DW_TAG_subprogram", "DW_AT_specification", 4),
        ],
    ),
];

fn check_wants(source: &str, unit: &Node) {
    let (_, wants) = WANTS.iter().find(|w| w.0 == source).unwrap();
    for &(tag, attr, least) in *wants {
        let n = if attr.is_empty() { count_tag(unit, tag) } else { has_attr(unit, tag, attr) };
        assert!(n >= least, "{source}: {n} {tag} with {attr:?}, want {least}");
    }
}

/// §6.9: each source by every compiler, by default and with
/// `-fstandalone-debug`; by clang, also with `-gtemplate-alias` (which
/// kiln++ does not take), where `scope.cpp` has a template alias with
/// its parameters.
#[test]
fn trees_match_llc() {
    if !cfg!(target_arch = "aarch64") {
        eprintln!("note: the IR is the host's; skipping on this host");
        return;
    }
    let scratch = Scratch::new("dies-scope-oracle");
    let (Some(c19), Some(c22)) = (tool("clang++-19"), tool("clang++-22")) else { return };
    let k = kiln();
    let (mut objects, mut dies) = (0, 0);
    for (source, _) in WANTS {
        let stem = source.split('.').next().unwrap();
        for (flags, tag) in [
            (&[][..], ""),
            (&["-fstandalone-debug"][..], "-sa"),
            (&["-gtemplate-alias"][..], "-ta"),
        ] {
            let text = full_ir(&c19, &scratch, &format!("{stem}{tag}-c19"), source, flags);
            for major in [19, 22] {
                if let Some(u) = compare(&scratch, major, &format!("{stem}{tag}-c19"), &text) {
                    objects += 2;
                    dies += count(&u);
                }
            }
            let text = full_ir(&c22, &scratch, &format!("{stem}{tag}-c22"), source, flags);
            if let Some(u) = compare(&scratch, 22, &format!("{stem}{tag}-c22"), &text) {
                if tag.is_empty() {
                    check_wants(source, &u);
                }
                if tag == "-ta" && source == "scope.cpp" {
                    let n = has_attr(&u, "DW_TAG_template_alias", "DW_AT_type");
                    assert!(n >= 1, "scope.cpp: a template alias");
                    assert!(count_tag(&u, "DW_TAG_template_type_parameter") >= 7);
                }
                objects += 2;
                dies += count(&u);
            }
            if let Some(k) = k.as_ref().filter(|_| tag != "-ta") {
                let text = full_ir(k, &scratch, &format!("{stem}{tag}-kiln"), source, flags);
                if let Some(u) = compare(&scratch, 22, &format!("{stem}{tag}-kiln"), &text) {
                    objects += 2;
                    dies += count(&u);
                }
            }
        }
    }
    eprintln!("dies_scope: {objects} objects, {dies} DIEs equal to llc's");
}

const UNIT: &str = "!llvm.dbg.cu = !{!0}\n!llvm.module.flags = !{!2, !3}\n\
!0 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !1, producer: \"hand\", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, retainedTypes: !5, splitDebugInlining: false, nameTableKind: None)\n\
!1 = !DIFile(filename: \"s.cpp\", directory: \"/hod/tests/dies_scope\")\n\
!2 = !{i32 7, !\"Dwarf Version\", i32 5}\n!3 = !{i32 2, !\"Debug Info Version\", i32 3}\n\
!4 = !{}\n!6 = !DIBasicType(name: \"int\", size: 32, encoding: DW_ATE_signed)\n\
!11 = !DISubroutineType(types: !12)\n!12 = !{null}\n\
!15 = !DILocation(line: 1, column: 1, scope: !10)\n";

fn one_function(prefix: &str, meta: &str) -> String {
    format!(
        "{}{prefix}\ndefine dso_local void @f() !dbg !10 {{\nentry:\n  ret void, !dbg !15\n}}\n\n{UNIT}{meta}",
        ll_header(22)
    )
}

fn refusal(text: &str) -> String {
    let (module, bodies) = parse(text);
    match compile_to_object(&module, &bodies, &Options::default()) {
        Err(ModuleError::Emit(e) | ModuleError::Function { error: e, .. }) => e.to_string(),
        other => panic!("{:?}\n{text}", other.map(|b| b.len())),
    }
}

/// The base compiles; each case differs from it by one construct.
#[test]
fn refusals_are_named() {
    let sp = "!10 = distinct !DISubprogram(name: \"f\", scope: !1, file: !1, line: 1, type: !11, scopeLine: 1, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !4)\n";
    let class = |param: &str| {
        format!(
            "{sp}!5 = !{{!20}}\n!20 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: \"C\", file: !1, line: 1, size: 8, elements: !4, templateParams: !21, identifier: \"_ZTS1C\")\n!21 = !{{!22}}\n!22 = !DITemplateValueParameter(name: \"F\", type: !23, value: {param})\n!23 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !11, size: 64)\n"
        )
    };
    let base = one_function("", &class("ptr @f"));
    let (module, bodies) = parse(&base);
    compile_to_object(&module, &bodies, &Options::default()).expect("the base case");

    let cases = [
        ("debug:template-address", one_function("declare void @ext()", &class("ptr @ext"))),
        (
            "debug:declaration-scope",
            one_function(
                "",
                "!5 = !{}\n!10 = distinct !DISubprogram(name: \"f\", linkageName: \"_ZN1n1fEv\", scope: !30, file: !1, line: 1, type: !11, scopeLine: 1, spFlags: DISPFlagDefinition, unit: !0, declaration: !31, retainedNodes: !4)\n\
                 !30 = !DINamespace(name: \"n\", scope: null)\n\
                 !31 = !DISubprogram(name: \"f\", linkageName: \"_ZN1n1fEv\", scope: !30, file: !1, line: 1, type: !11, scopeLine: 1, spFlags: DISPFlagVirtual)\n",
            ),
        ),
    ];
    for (name, text) in &cases {
        let e = refusal(text);
        assert!(e.contains(name), "{name}: {e}\n{text}");
    }
}

/// An out-of-line definition whose return type differs from its
/// declaration's (`auto` declared, `int` deduced): llc writes the
/// definition's own `DW_AT_type` beside `DW_AT_specification`. Neither
/// clang nor kiln++ emits this today, so the IR is written by hand.
#[test]
fn definition_with_its_own_return_type() {
    let scratch = Scratch::new("dies-scope-auto");
    let text = format!(
        "{}define dso_local i32 @_ZN1C3getEv() !dbg !10 {{\nentry:\n  ret i32 1, !dbg !15\n}}\n\n{UNIT}\
         !5 = !{{}}\n\
         !10 = distinct !DISubprogram(name: \"get\", linkageName: \"_ZN1C3getEv\", scope: !20, file: !1, line: 3, type: !40, scopeLine: 3, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, declaration: !22, retainedNodes: !4)\n\
         !20 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: \"C\", file: !1, line: 1, size: 8, flags: DIFlagTypePassByValue, elements: !21, identifier: \"_ZTS1C\")\n\
         !21 = !{{!22}}\n\
         !22 = !DISubprogram(name: \"get\", linkageName: \"_ZN1C3getEv\", scope: !20, file: !1, line: 2, type: !42, scopeLine: 2, flags: DIFlagPrototyped | DIFlagStaticMember, spFlags: 0)\n\
         !40 = !DISubroutineType(types: !41)\n!41 = !{{!6}}\n\
         !42 = !DISubroutineType(types: !43)\n!43 = !{{!44}}\n\
         !44 = !DIBasicType(tag: DW_TAG_unspecified_type, name: \"auto\")\n",
        ll_header(22)
    );
    let Some(u) = compare(&scratch, 22, "auto-return", &text) else { return };
    assert_eq!(has_attr(&u, "DW_TAG_subprogram", "DW_AT_specification"), 1);
    assert_eq!(count_tag(&u, "DW_TAG_unspecified_type"), 1);
}

/// A bug outside WP-059: `crates/hod_dwarf/src/types.rs:613` (`is_unsigned`)
/// treats every enumeration as signed, where `llc` 19 and 22 follow an
/// enumeration's base type (`isUnsignedDIType`): a static member
/// `constexpr E e = E::B` of `enum class E : uint8_t` with `B = 200` is
/// `DW_AT_const_value (-56)` in hod's tree, `(200)` in `llc`'s.
/// WP-059's template values follow the base type (`template.rs`).
#[test]
#[ignore = "bug in crates/hod_dwarf/src/types.rs:613 (is_unsigned of an enumeration), not WP-059's"]
fn static_member_of_unsigned_enumeration() {
    let Some(c22) = tool("clang++-22") else { return };
    let scratch = Scratch::new("dies-scope-enum");
    let text = full_ir(&c22, &scratch, "enum_member", "enum_member.cpp", &[]);
    compare(&scratch, 22, "enum_member", &text);
}
