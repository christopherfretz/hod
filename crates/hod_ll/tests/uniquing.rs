//! `spec/ir.md` §6.3 and `spec/ll.md` §7.4 (WP-228): two numbered nodes
//! that are not `distinct` and have equal contents are one node, as
//! LLVM's parser makes them. Each case is held against hard numbers and,
//! when LLVM is installed, against `llvm-as | llvm-dis` of 22 and 19:
//! the nodes of each kind that remain, and how many of them are
//! `distinct`.

mod common;

use std::collections::BTreeMap;

use hod_ir::{Arch, DiId, DiKind, DiNode, Module};

use common::{Llvm, header, parse};

/// Per kind: (nodes, of which distinct). Tuples, strings and constants
/// are not counted (`llvm-dis` numbers tuples hod reads in place).
type Counts = BTreeMap<String, (usize, usize)>;

fn module(case: &str) -> String {
    format!("{}{case}", header(Arch::X86_64, 22))
}

/// What hod's tables hold after reading `text`: the nodes not merged.
fn hod_counts(module: &Module) -> Counts {
    let debug = &module.debug;
    let mut counts = Counts::new();
    for raw in 0..debug.node_count() as u32 {
        let id = DiId::new(raw);
        if debug.is_merged(id) {
            continue;
        }
        let kind = debug.kind(id);
        if matches!(kind, DiKind::Tuple | DiKind::String | DiKind::Constant | DiKind::Undefined) {
            continue;
        }
        let entry = counts.entry(kind.llvm_name().to_owned()).or_default();
        entry.0 += 1;
        entry.1 += usize::from(debug.is_distinct(id));
    }
    counts
}

/// What `llvm-dis` prints: `!N = [distinct ]!DIKind(`.
fn llvm_counts(text: &str) -> Counts {
    let mut counts = Counts::new();
    for line in text.lines() {
        let Some((_, rest)) = line.split_once(" = ") else { continue };
        if !line.starts_with('!') {
            continue;
        }
        let (distinct, rest) = match rest.strip_prefix("distinct ") {
            Some(rest) => (true, rest),
            None => (false, rest),
        };
        let Some(rest) = rest.strip_prefix('!') else { continue };
        let Some((kind, _)) = rest.split_once('(') else { continue };
        if !kind.starts_with("DI") || kind == "DILocation" || kind == "DIExpression" {
            continue;
        }
        let entry = counts.entry(kind.to_owned()).or_default();
        entry.0 += 1;
        entry.1 += usize::from(distinct);
    }
    counts
}

/// Reads `case`, checks hod's counts against `want`, and `want` against
/// each installed LLVM.
fn check(name: &str, case: &str, want: &[(&str, usize, usize)]) -> Module {
    let text = module(case);
    let parsed = parse(text.as_bytes(), Arch::X86_64).unwrap_or_else(|e| panic!("{name}: {e}"));
    let want: Counts =
        want.iter().map(|&(kind, nodes, distinct)| (kind.to_owned(), (nodes, distinct))).collect();
    assert_eq!(hod_counts(&parsed.module), want, "{name}: hod");
    for llvm in Llvm::all() {
        let canonical = llvm.canonical(text.as_bytes()).unwrap_or_else(|e| panic!("{name}: {e}"));
        let canonical = String::from_utf8(canonical).unwrap();
        assert_eq!(llvm_counts(&canonical), want, "{name}: llvm-dis-{}", llvm.major);
    }
    parsed.module
}

/// The id a field of node `id` names.
fn base_type(module: &Module, id: DiId) -> Option<DiId> {
    match module.debug.node(id) {
        DiNode::DerivedType(t) => t.base_type,
        other => panic!("{id:?} is {other:?}"),
    }
}

/// Plain duplicates; duplicates that name duplicates (merged operands
/// first, a forward reference included); a node written in place equal
/// to a numbered one.
#[test]
fn equal_nodes_are_one_node() {
    let m = check(
        "duplicates",
        r#"
!named = !{!0, !1, !2, !3, !4, !5, !6}
!0 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: null, size: 64)
!1 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: null, size: 64)
!2 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !4, size: 64)
!3 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !5, size: 64)
!4 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!5 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!6 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed), size: 64)
"#,
        &[("DIBasicType", 1, 0), ("DIDerivedType", 2, 0)],
    );
    // Reserved in file order: !0..!6 are ids 0..6. The lowest id
    // survives and every reference names it.
    let d = &m.debug;
    let id = DiId::new;
    assert!(!d.is_merged(id(0)) && d.is_merged(id(1)));
    assert!(!d.is_merged(id(2)) && d.is_merged(id(3)) && d.is_merged(id(6)));
    assert!(!d.is_merged(id(4)) && d.is_merged(id(5)));
    assert_eq!(base_type(&m, id(2)), Some(id(4)));
    // A merged node's own references are rewritten too: nothing names
    // it, but it names only survivors.
    assert_eq!(base_type(&m, id(3)), Some(id(4)));
    assert_eq!(base_type(&m, id(6)), Some(id(4)), "the node in place");
}

/// A node that names itself is distinct (LLVM stores it so, and
/// `llvm-dis` prints `distinct`), and so is never merged: the node with
/// its contents that names it stays apart. Cycles that are alike but
/// share no node stay apart; inside one cycle, merging repeats until
/// nothing more merges (the two pointers to `A`, then its two members).
#[test]
fn self_references_and_cycles() {
    check(
        "cycles",
        r#"
!named = !{!0, !1, !2, !3, !4, !5, !6, !7, !8, !9, !10}
!0 = !DICompositeType(tag: DW_TAG_class_type, name: "C", size: 8, vtableHolder: !0)
!1 = !DICompositeType(tag: DW_TAG_class_type, name: "C", size: 8, vtableHolder: !0)
!2 = !DICompositeType(tag: DW_TAG_structure_type, name: "S", size: 64, elements: !{!3})
!3 = !DIDerivedType(tag: DW_TAG_member, name: "p", scope: !2, baseType: !11, size: 64)
!4 = !DICompositeType(tag: DW_TAG_structure_type, name: "S", size: 64, elements: !{!5})
!5 = !DIDerivedType(tag: DW_TAG_member, name: "p", scope: !4, baseType: !11, size: 64)
!6 = !DICompositeType(tag: DW_TAG_structure_type, name: "A", size: 128, elements: !{!7, !9})
!7 = !DIDerivedType(tag: DW_TAG_member, name: "b", scope: !6, baseType: !8, size: 64)
!8 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !6, size: 64)
!9 = !DIDerivedType(tag: DW_TAG_member, name: "b", scope: !6, baseType: !10, size: 64)
!10 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !6, size: 64)
!11 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: null, size: 64)
"#,
        &[("DICompositeType", 5, 1), ("DIDerivedType", 5, 0)],
    );
}

/// `distinct` nodes are never merged, with each other or with an equal
/// uniqued node; the nodes they name are.
#[test]
fn distinct_nodes_stay_apart() {
    let m = check(
        "distinct",
        r#"
!named = !{!0, !1, !2, !3, !4}
!0 = distinct !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!1 = distinct !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!2 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!3 = distinct !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !4, size: 64)
!4 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
"#,
        &[("DIBasicType", 3, 2), ("DIDerivedType", 1, 1)],
    );
    assert_eq!(base_type(&m, DiId::new(3)), Some(DiId::new(2)), "rewritten to the survivor");
}

/// A composite type declared and then defined with one `identifier`:
/// LLVM's parser does not unique by identifier (that is LTO's ODR
/// uniquing), so both stay; two equal declarations are one.
#[test]
fn identifiers_do_not_merge() {
    check(
        "identifier",
        r#"
!named = !{!0, !1, !2, !3}
!0 = !DICompositeType(tag: DW_TAG_structure_type, name: "S", flags: DIFlagFwdDecl, identifier: "_ZTS1S")
!1 = !DICompositeType(tag: DW_TAG_structure_type, name: "S", flags: DIFlagFwdDecl, identifier: "_ZTS1S")
!2 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "S", size: 32, identifier: "_ZTS1S")
!3 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "S", size: 32, identifier: "_ZTS1S")
"#,
        &[("DICompositeType", 3, 2)],
    );
}

/// The kiln++ shape (WP-228): a function's type names `!5`, a variable's
/// type `!9`, two equal `void *`; the body's record names `!12`, equal
/// to `!8`. One of each survives, and the record, read in the body,
/// names the surviving variable, whose type is the surviving pointer.
#[test]
fn a_body_names_the_survivor() {
    let text = module(
        r#"
define void @f(ptr %p) !dbg !3 {
  %slot = alloca ptr, align 8
    #dbg_declare(ptr %slot, !12, !DIExpression(), !10)
  ret void
}

!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!11}
!0 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !1, producer: "t", emissionKind: FullDebug)
!1 = !DIFile(filename: "a.cpp", directory: "/")
!3 = distinct !DISubprogram(name: "f", scope: !1, file: !1, line: 1, type: !4, spFlags: DISPFlagDefinition, unit: !0)
!4 = !DISubroutineType(types: !{null, !5})
!5 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: null, size: 64)
!8 = !DILocalVariable(name: "q", scope: !3, file: !1, line: 2, type: !9)
!9 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: null, size: 64)
!10 = !DILocation(line: 2, scope: !3)
!11 = !{i32 2, !"Debug Info Version", i32 3}
!12 = !DILocalVariable(name: "q", scope: !3, file: !1, line: 2, type: !9)
"#,
    );
    let mut parsed = parse(text.as_bytes(), Arch::X86_64).unwrap();
    let mut parser = hod_ll::BodyParser::new(&parsed.text);
    let function = &parsed.text.functions()[0];
    let body = parser.parse(&mut parsed.module, function).unwrap().unwrap();
    let debug = &parsed.module.debug;
    let live = |kind: DiKind| -> Vec<DiId> {
        (0..debug.node_count() as u32)
            .map(DiId::new)
            .filter(|&id| debug.kind(id) == kind && !debug.is_merged(id))
            .collect()
    };
    let (pointers, variables) = (live(DiKind::DerivedType), live(DiKind::LocalVariable));
    assert_eq!((pointers.len(), variables.len()), (1, 1), "one void *, one q");
    // A `#dbg_declare` of an alloca is a declared slot (`ir.md` §6.6).
    let named: Vec<DiId> = body.slot_vars().iter().map(|slot| slot.var).collect();
    assert_eq!(named, variables, "the record names the survivor");
    let DiNode::LocalVariable(v) = debug.node(variables[0]) else { panic!() };
    assert_eq!(v.ty, Some(pointers[0]));
    parsed.module.verify_debug().unwrap();
}
