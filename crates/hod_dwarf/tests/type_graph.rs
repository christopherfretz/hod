//! `spec/dwarf.md` §6.8: the type graph's named errors, its cycles and
//! its depth, on modules read by `hod_ll`.

mod common;

use common::types::read;
use hod_dwarf::{Error, UnitPlan};

const HEAD: &str = "target datalayout = \"e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128-Fn32\"\n\
    target triple = \"aarch64-unknown-linux-gnu\"\n\
    !llvm.dbg.cu = !{!0}\n\
    !llvm.module.flags = !{!90}\n\
    !90 = !{i32 2, !\"Debug Info Version\", i32 3}\n\
    !0 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !1, producer: \"p\", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, retainedTypes: !2, nameTableKind: None)\n\
    !1 = !DIFile(filename: \"t.cpp\", directory: \"/hod\")\n\
    !2 = !{!3}\n\
    !9 = !DIBasicType(name: \"int\", size: 32, encoding: DW_ATE_signed)\n";

/// The plan of a unit whose one retained type is `!3`, defined in `body`.
fn plan(body: &str) -> Result<usize, Error> {
    let module = read(&format!("{HEAD}{body}"));
    UnitPlan::of_module(&module, module.debug.compile_units()[0]).map(|p| p.die_count())
}

/// §6.8: a member whose scope is not a class is `MemberOutsideType`.
#[test]
fn member_outside_a_type_is_named() {
    let body = "!3 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: \"S\", file: !1, line: 1, size: 32, elements: !4, identifier: \"_ZTS1S\")\n\
        !4 = !{!5}\n\
        !5 = !DIDerivedType(tag: DW_TAG_member, name: \"m\", scope: !6, file: !1, line: 2, baseType: !9, size: 32)\n\
        !6 = !DINamespace(name: \"ns\", scope: null)\n";
    assert!(matches!(plan(body), Err(Error::MemberOutsideType { .. })), "{:?}", plan(body));
}

/// §6.8: a typedef (or qualifier) `baseType` cycle is `TypeCycle`, not a
/// hang or an overflow.
#[test]
fn value_cycles_are_named() {
    for body in [
        "!3 = !DIDerivedType(tag: DW_TAG_typedef, name: \"a\", file: !1, line: 1, baseType: !4)\n\
         !4 = !DIDerivedType(tag: DW_TAG_typedef, name: \"b\", file: !1, line: 2, baseType: !3)\n",
        "!3 = !DIDerivedType(tag: DW_TAG_typedef, name: \"self\", file: !1, line: 1, baseType: !3)\n",
        "!3 = !DIDerivedType(tag: DW_TAG_const_type, baseType: !4)\n\
         !4 = !DIDerivedType(tag: DW_TAG_volatile_type, baseType: !3)\n",
        "!3 = !DICompositeType(tag: DW_TAG_array_type, baseType: !3, size: 32, elements: !4)\n\
         !4 = !{!5}\n\
         !5 = !DISubrange(count: 1)\n",
    ] {
        assert!(matches!(plan(body), Err(Error::TypeCycle { .. })), "{body}: {:?}", plan(body));
    }
}

/// §6.8: cycles through a pointer, a reference or a class are the
/// ordinary shape of C++ types and cost one DIE per node.
#[test]
fn reference_cycles_cost_nothing() {
    // The unit, `S`, its two members, the pointer and the reference:
    // each node once, whichever way it is reached.
    let body = "!3 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: \"S\", file: !1, line: 1, size: 128, elements: !4, identifier: \"_ZTS1S\")\n\
        !4 = !{!5, !6}\n\
        !5 = !DIDerivedType(tag: DW_TAG_member, name: \"next\", scope: !3, file: !1, line: 2, baseType: !7, size: 64)\n\
        !6 = !DIDerivedType(tag: DW_TAG_member, name: \"self\", scope: !3, file: !1, line: 3, baseType: !8, size: 64, offset: 64)\n\
        !7 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !3, size: 64)\n\
        !8 = !DIDerivedType(tag: DW_TAG_reference_type, baseType: !3, size: 64)\n";
    assert_eq!(plan(body), Ok(6));
    // A pointer to itself.
    assert_eq!(
        plan("!3 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !3, size: 64)\n"),
        Ok(2)
    );
}

/// §6.8: a type that must have a size and has none is `MissingSize`.
#[test]
fn missing_sizes_are_named() {
    for body in [
        "!3 = !DIBasicType(name: \"zero\", encoding: DW_ATE_signed)\n",
        "!3 = distinct !DICompositeType(tag: DW_TAG_enumeration_type, name: \"E\", file: !1, line: 1, baseType: !9, elements: !4, identifier: \"_ZTS1E\")\n\
         !4 = !{}\n",
        "!3 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: \"B\", file: !1, line: 1, size: 32, elements: !4, identifier: \"_ZTS1B\")\n\
         !4 = !{!5}\n\
         !5 = !DIDerivedType(tag: DW_TAG_member, name: \"f\", scope: !3, file: !1, line: 2, baseType: !9, flags: DIFlagBitField, extraData: i64 0)\n",
    ] {
        assert!(matches!(plan(body), Err(Error::MissingSize { .. })), "{body}: {:?}", plan(body));
    }
}

/// §6.8: a type scoped in a function that has no DIE (here, a retained
/// type of a function that was not placed) is refused by name.
#[test]
fn function_local_types_are_refused() {
    let body = "!3 = !DIDerivedType(tag: DW_TAG_typedef, name: \"t\", scope: !4, file: !1, line: 3, baseType: !9)\n\
        !4 = distinct !DISubprogram(name: \"f\", scope: !1, file: !1, line: 2, type: !5, scopeLine: 2, spFlags: DISPFlagDefinition, unit: !0)\n\
        !5 = !DISubroutineType(types: !6)\n\
        !6 = !{null}\n";
    match plan(body) {
        Err(e @ Error::Unsupported { .. }) => {
            assert!(e.to_string().contains("debug:local-type-scope"), "{e}");
        }
        other => panic!("{other:?}"),
    }
}

/// §6.8, §6.11: template parameters (`templateParams`, a template alias's
/// `extraData`) are children of their owner (WP-059).
#[test]
fn template_parameters_are_children() {
    let alias = "!3 = !DIDerivedType(tag: DW_TAG_template_alias, name: \"A\", file: !1, line: 1, baseType: !9, extraData: !4)\n\
        !4 = !{!5}\n\
        !5 = !DITemplateTypeParameter(name: \"T\", type: !9)\n";
    assert_eq!(plan(alias), Ok(4));
    let class = "!3 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: \"C<int>\", file: !1, line: 1, size: 8, elements: !4, templateParams: !6, identifier: \"_ZTS1CIiE\")\n\
        !4 = !{}\n\
        !6 = !{!5}\n\
        !5 = !DITemplateTypeParameter(name: \"T\", type: !9)\n";
    assert_eq!(plan(class), Ok(4)); // the unit, the class, T and int
}

/// §6.8: the graph is walked with an explicit work list, so a deep chain
/// is linear and does not overflow the stack.
#[test]
fn deep_chains_are_linear() {
    use std::fmt::Write as _;
    let depth = 20_000;
    let mut body = String::new();
    for i in 0..depth {
        let next = if i + 1 == depth { 9 } else { 100 + i + 1 };
        let n = if i == 0 { 3 } else { 100 + i };
        let _ = writeln!(
            body,
            "!{n} = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !{next}, size: 64)"
        );
    }
    assert_eq!(plan(&body), Ok(depth + 2));
}
