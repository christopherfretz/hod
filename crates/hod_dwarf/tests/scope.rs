//! `spec/dwarf.md` §6.11 (WP-059): the plan's refusals of what it still
//! does not write outside function bodies, each by name in a module with
//! that one construct, and the shapes it writes counted.

mod common;

use common::types::read;
use hod_dwarf::{Error, UnitPlan, scope};

/// A unit of `triple` whose globals are `!20` and whose imports are
/// `!30`, both defined in `body`.
fn module(triple: &str, prefix: &str, body: &str) -> String {
    format!(
        "target triple = \"{triple}\"\n{prefix}\n\
         !llvm.dbg.cu = !{{!0}}\n\
         !llvm.module.flags = !{{!90}}\n\
         !90 = !{{i32 2, !\"Debug Info Version\", i32 3}}\n\
         !0 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !1, producer: \"p\", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, globals: !2, imports: !3, nameTableKind: None)\n\
         !1 = !DIFile(filename: \"t.cpp\", directory: \"/hod\")\n\
         !9 = !DIBasicType(name: \"int\", size: 32, encoding: DW_ATE_signed)\n\
         {body}"
    )
}

const A64: &str = "aarch64-unknown-linux-gnu";

fn plan(text: &str) -> Result<usize, Error> {
    let module = read(text);
    UnitPlan::of_module(&module, module.debug.compile_units()[0]).map(|p| p.die_count())
}

/// The refusal of `text`: its name.
fn refused(text: &str) -> &'static str {
    match plan(text) {
        Err(Error::Unsupported { what, .. }) => what,
        other => panic!("{other:?}\n{text}"),
    }
}

/// `@g`, a global with expression `expr` and variable `var` (`!21`).
fn global(expr: &str, var: &str, extra: &str) -> String {
    module(
        A64,
        "@g = global i32 0, align 4, !dbg !20",
        &format!(
            "!2 = !{{!20}}\n!3 = !{{}}\n\
             !20 = !DIGlobalVariableExpression(var: !21, expr: !DIExpression({expr}))\n\
             !21 = distinct !DIGlobalVariable({var})\n{extra}"
        ),
    )
}

/// An import `!31` (the unit's only one) defined in `ie`.
fn import(ie: &str, extra: &str) -> String {
    module(A64, "", &format!("!2 = !{{}}\n!3 = !{{!31}}\n!31 = {ie}\n{extra}"))
}

const PLAIN: &str =
    "name: \"g\", scope: !0, file: !1, line: 1, type: !9, isLocal: false, isDefinition: true";

#[test]
fn the_base_cases_are_written() {
    // A global, a constant, a static member's definition and an import.
    assert_eq!(plan(&global("", PLAIN, "")), Ok(3));
    assert_eq!(plan(&global("DW_OP_constu, 7, DW_OP_stack_value", PLAIN, "")), Ok(3));
    let member = "!22 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: \"S\", file: !1, line: 1, size: 8, elements: !23, identifier: \"_ZTS1S\")\n\
        !23 = !{!24}\n\
        !24 = !DIDerivedType(tag: DW_TAG_variable, name: \"m\", scope: !22, file: !1, line: 2, baseType: !9, flags: DIFlagStaticMember)\n";
    let def = "name: \"m\", linkageName: \"_ZN1S1mE\", scope: !0, file: !1, line: 3, type: !9, isLocal: false, isDefinition: true, declaration: !24";
    // The unit, the definition, the class, its member, int.
    assert_eq!(plan(&global("", def, member)), Ok(5));
    let ns = "!32 = !DINamespace(name: \"n\", scope: null)\n";
    let ie =
        "!DIImportedEntity(tag: DW_TAG_imported_module, scope: !0, entity: !32, file: !1, line: 4)";
    assert_eq!(plan(&import(ie, ns)), Ok(3));
}

/// Each refusal of `global.rs`, `template.rs`, `import.rs` and `spec.rs`
/// that a plan reaches, in a module with that one construct.
#[test]
fn refusals_are_named() {
    let member = "!22 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: \"S\", file: !1, line: 1, size: 8, elements: !23, identifier: \"_ZTS1S\")\n\
        !23 = !{!24}\n";
    let cases: Vec<(&str, String)> = vec![
        (scope::GLOBAL_EXPRESSION, global("DW_OP_plus_uconst, 4", PLAIN, "")),
        (scope::GLOBAL_EXPRESSION, global("DW_OP_constu, 7", PLAIN, "")),
        (
            scope::GLOBAL_SHAPE,
            global(
                "DW_OP_constu, 7, DW_OP_stack_value",
                "name: \"g\", scope: !0, line: 1, type: !9, isLocal: false, isDefinition: true",
                "",
            ),
        ),
        (
            scope::GLOBAL_SHAPE,
            global(
                "",
                "name: \"m\", scope: !0, file: !1, line: 3, type: !9, isLocal: false, isDefinition: false, declaration: !24",
                &format!(
                    "{member}!24 = !DIDerivedType(tag: DW_TAG_variable, name: \"m\", scope: !22, file: !1, line: 2, baseType: !9, flags: DIFlagStaticMember)\n"
                ),
            ),
        ),
        (
            scope::STATIC_MEMBER_DECLARATION,
            global(
                "",
                "name: \"m\", scope: !0, file: !1, line: 3, type: !9, isLocal: false, isDefinition: true, declaration: !24",
                &format!(
                    "{member}!24 = !DIDerivedType(tag: DW_TAG_member, name: \"m\", scope: !22, file: !1, line: 2, baseType: !9, size: 32)\n"
                ),
            ),
        ),
        (
            scope::GLOBAL_SCOPE,
            global(
                "DW_OP_constu, 7, DW_OP_stack_value",
                "name: \"g\", scope: !25, file: !1, line: 1, type: !9, isLocal: false, isDefinition: true",
                "!25 = !DIModule(scope: null, name: \"M\")\n",
            ),
        ),
        (
            scope::TEMPLATE_PARAMETER,
            global(
                "",
                "name: \"g\", scope: !0, file: !1, line: 1, type: !9, isLocal: false, isDefinition: true, templateParams: !26",
                "!26 = !{!27}\n!27 = !DITemplateValueParameter(tag: DW_TAG_GNU_template_parameter_pack, name: \"P\", type: !9, value: !{})\n",
            ),
        ),
        (
            scope::TEMPLATE_VALUE,
            global(
                "",
                "name: \"g\", scope: !0, file: !1, line: 1, type: !9, isLocal: false, isDefinition: true, templateParams: !26",
                "!26 = !{!27}\n!27 = !DITemplateValueParameter(name: \"N\", type: !28, value: i128 1)\n!28 = !DIBasicType(name: \"__int128\", size: 128, encoding: DW_ATE_signed)\n",
            ),
        ),
        (
            scope::IMPORT,
            import(
                "!DIImportedEntity(tag: DW_TAG_imported_module, name: \"x\", scope: !0, entity: !32, file: !1, line: 4)",
                "!32 = !DINamespace(name: \"n\", scope: null)\n",
            ),
        ),
        (
            scope::IMPORT_SCOPE,
            import(
                "!DIImportedEntity(tag: DW_TAG_imported_module, scope: !33, entity: !32, file: !1, line: 4)",
                "!32 = !DINamespace(name: \"n\", scope: null)\n!33 = !DIModule(scope: null, name: \"M\")\n",
            ),
        ),
        (
            scope::IMPORT_ENTITY,
            import(
                "!DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !0, entity: !33, file: !1, line: 4)",
                "!33 = !DIModule(scope: null, name: \"M\")\n",
            ),
        ),
        (
            scope::DECLARATION_SCOPE,
            import(
                "!DIImportedEntity(tag: DW_TAG_imported_declaration, scope: !0, entity: !34, file: !1, line: 4)",
                "!32 = !DINamespace(name: \"n\", scope: null)\n!34 = !DISubprogram(name: \"f\", scope: !32, file: !1, line: 1, type: !35, spFlags: DISPFlagVirtual)\n!35 = !DISubroutineType(types: !36)\n!36 = !{null}\n",
            ),
        ),
    ];
    for (name, text) in &cases {
        assert_eq!(refused(text), *name, "{text}");
    }
}

/// AArch64's `llc` writes a thread-local variable without a location;
/// so does the plan, which takes no `.debug_addr` entry for it. x86-64's
/// writes one (WP-136, §6.17), which the plan no longer refuses; its
/// bytes and relocation are `xtask debug`'s to compare with `llc`'s.
#[test]
fn thread_locals_are_written() {
    for triple in [A64, "x86_64-unknown-linux-gnu"] {
        let text = module(
            triple,
            "@g = thread_local global i32 0, align 4, !dbg !20",
            &format!(
                "!2 = !{{!20}}\n!3 = !{{}}\n!20 = !DIGlobalVariableExpression(var: !21, expr: !DIExpression())\n!21 = distinct !DIGlobalVariable({PLAIN})\n"
            ),
        );
        assert_eq!(plan(&text), Ok(3), "{triple}");
    }
}
