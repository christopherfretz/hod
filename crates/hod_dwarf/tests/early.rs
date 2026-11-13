//! `spec/dwarf.md` §6.13 (WP-094): the function-DIE refusals that depend
//! only on metadata are made by `check_functions`, before any code is
//! generated, each by name in a module with that one construct.

use hod_dwarf::check_functions;
use hod_ir::{Body, Module};

/// A `FullDebug` unit with function `@f` (subprogram `!10`, variable `!20`
/// declared, label `!40`), after `edits` (`(from, to)` replacements) and
/// with `extra` appended.
fn module(edits: &[(&str, &str)], extra: &str) -> String {
    let mut text = String::from(
        "target triple = \"aarch64-unknown-linux-gnu\"\n\
         define i32 @f(i32 %a) !dbg !10 {\n\
         entry:\n\
         \x20 %x = alloca i32, align 4\n\
         \x20   #dbg_declare(ptr %x, !20, !DIExpression(), !30)\n\
         \x20   #dbg_label(!40, !30)\n\
         \x20 ret i32 0, !dbg !30\n\
         }\n\
         !llvm.dbg.cu = !{!0}\n\
         !llvm.module.flags = !{!90}\n\
         !90 = !{i32 2, !\"Debug Info Version\", i32 3}\n\
         !0 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !1, producer: \"p\", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, nameTableKind: None)\n\
         !1 = !DIFile(filename: \"t.cpp\", directory: \"/hod\")\n\
         !9 = !DIBasicType(name: \"int\", size: 32, encoding: DW_ATE_signed)\n\
         !10 = distinct !DISubprogram(name: \"f\", scope: !1, file: !1, line: 1, type: !11, scopeLine: 1, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !12)\n\
         !11 = !DISubroutineType(types: !13)\n\
         !12 = !{}\n\
         !13 = !{!9, !9}\n\
         !20 = !DILocalVariable(name: \"x\", scope: !10, file: !1, line: 2, type: !9)\n\
         !30 = !DILocation(line: 2, scope: !10)\n\
         !40 = !DILabel(scope: !10, name: \"l\", file: !1, line: 3)\n\
         !51 = !DINamespace(name: \"ns\", scope: null)\n\
         !52 = !DILexicalBlock(scope: !10, file: !1, line: 2, column: 1)\n",
    );
    for (from, to) in edits {
        assert!(text.contains(from), "{from}");
        text = text.replace(from, to);
    }
    text.push_str(extra);
    text
}

fn read(text: &str) -> (Module, Vec<Body>) {
    let parsed = hod_ll::parse_module(text.as_bytes(), &hod_ll::ParseOptions::default())
        .unwrap_or_else(|e| panic!("hod_ll: {e}\n{text}"));
    let text_side = parsed.text;
    let mut module = parsed.module;
    let mut bodies = Vec::new();
    hod_ll::parse_bodies(&mut module, &text_side, |_, parsed| {
        if let Some(body) = parsed.body {
            bodies.push(body.clone());
        }
        Ok::<(), hod_ll::LlError>(())
    })
    .unwrap_or_else(|e| panic!("hod_ll: {e}\n{text}"));
    (module, bodies)
}

/// `check_functions` of `text`: the body it refuses and the name.
fn check(text: &str) -> Result<(), (usize, &'static str)> {
    let (module, bodies) = read(text);
    check_functions(&module, &bodies)
}

/// The function's retained nodes are import `!50`, defined by `ie`.
fn import(ie: &str) -> String {
    module(
        &[("retainedNodes: !12", "retainedNodes: !14")],
        &format!("!14 = !{{!50}}\n!50 = {ie}\n"),
    )
}

#[test]
fn a_plain_function_passes() {
    assert_eq!(check(&module(&[], "")), Ok(()));
    // An import in a block of its own function is written (§6.13).
    let ie = "!DIImportedEntity(tag: DW_TAG_imported_module, scope: !52, entity: !51, file: !1, line: 2)";
    assert_eq!(check(&import(ie)), Ok(()));
}

#[test]
fn subprogram_refusals() {
    let not_def = module(&[("spFlags: DISPFlagDefinition", "spFlags: 0")], "");
    assert_eq!(check(&not_def), Err((0, "debug:subprogram-not-definition")));
    let thrown = module(&[("retainedNodes: !12", "retainedNodes: !12, thrownTypes: !13")], "");
    assert_eq!(check(&thrown), Err((0, "debug:subprogram-attribute")));
    // WP-098: call-site entries need the code (spec/dwarf.md §6.14);
    // `DISPFlagOptimized` alone is written.
    let calls = module(
        &[(
            "spFlags: DISPFlagDefinition",
            "flags: DIFlagAllCallsDescribed, spFlags: DISPFlagDefinition",
        )],
        "",
    );
    assert_eq!(check(&calls), Err((0, "debug:call-sites")));
    let optimized = module(
        &[("spFlags: DISPFlagDefinition", "spFlags: DISPFlagDefinition | DISPFlagOptimized")],
        "",
    );
    assert_eq!(check(&optimized), Ok(()));
}

#[test]
fn record_refusals() {
    let untyped = module(&[("line: 2, type: !9)", "line: 2)")], "");
    assert_eq!(check(&untyped), Err((0, "debug:variable-without-type")));
    let aligned = module(&[("line: 2, type: !9)", "line: 2, arg: 1, type: !9, align: 64)")], "");
    assert_eq!(check(&aligned), Err((0, "debug:variable-alignment")));
    let fragment =
        module(&[("!20, !DIExpression()", "!20, !DIExpression(DW_OP_LLVM_fragment, 0, 16)")], "");
    assert_eq!(check(&fragment), Err((0, "debug:fragment")));
    let label = module(&[("line: 3)", "line: 3, isArtificial: true)")], "");
    // WP-098: every label is written (spec/dwarf.md §6.14).
    assert_eq!(check(&label), Ok(()));
}

#[test]
fn import_refusals() {
    // No scope; scoped in the file; in another function's block.
    for scope in ["scope: null, ", "scope: !1, ", "scope: !61, "] {
        let ie = format!(
            "!DIImportedEntity(tag: DW_TAG_imported_module, {scope}entity: !51, file: !1, line: 2)\n\
             !60 = distinct !DISubprogram(name: \"g\", scope: !1, file: !1, line: 9, type: !11, spFlags: 0)\n\
             !61 = !DILexicalBlock(scope: !60, file: !1, line: 9, column: 1)"
        );
        assert_eq!(check(&import(&ie)), Err((0, "debug:import-scope")), "{scope}");
    }
    // In the function's own scope, of a shape hod does not write.
    let ie = "!DIImportedEntity(tag: DW_TAG_imported_module, name: \"n\", scope: !10, entity: !51, file: !1, line: 2)";
    assert_eq!(check(&import(ie)), Err((0, "debug:import")));
}

#[test]
fn a_subprogram_with_two_bodies() {
    let text = module(&[], "define i32 @g() !dbg !10 {\n  ret i32 0, !dbg !30\n}\n");
    assert_eq!(check(&text), Err((1, "debug:subprogram-twice")));
}

#[test]
fn line_tables_only_units_are_not_checked() {
    let text = module(&[("FullDebug", "LineTablesOnly"), ("line: 2, type: !9)", "line: 2)")], "");
    assert_eq!(check(&text), Ok(()));
}
