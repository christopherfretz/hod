//! `-g` modules through the whole of hod (`spec/dwarf.md` §6.12,
//! WP-087): `classify` lets a DWARF 5 debug module through on AArch64
//! and the object has its DIEs; what `hod_dwarf` cannot write is an
//! ordinary decline by its `debug:*` name, never an internal error and
//! never an object without the DIEs, and what the unit's plan refuses is
//! refused before any code is generated.
//!
//! The census of §6.12 (which `-g` modules hod builds, and what stops the
//! others) is `debug_census`, ignored: see its comment.

use hod::{CompileError, Construct, OutputKind, Report, Settings, compile_with};

/// clang 22's `-g -O0` IR of a small C function, its producer and paths
/// shortened.
const TINY: &str = r#"source_filename = "tiny.c"
target datalayout = "e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128"
target triple = "aarch64-unknown-linux-gnu"

define dso_local i32 @g(i32 noundef %0) #0 !dbg !10 {
  %2 = alloca i32, align 4
  %3 = alloca i32, align 4
  store i32 %0, ptr %2, align 4
    #dbg_declare(ptr %2, !15, !DIExpression(), !16)
    #dbg_declare(ptr %3, !17, !DIExpression(), !18)
  %4 = load i32, ptr %2, align 4, !dbg !19
  %5 = add nsw i32 %4, 1, !dbg !20
  store i32 %5, ptr %3, align 4, !dbg !18
  %6 = load i32, ptr %3, align 4, !dbg !21
  ret i32 %6, !dbg !22
}

attributes #0 = { noinline nounwind optnone "frame-pointer"="non-leaf" }

!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!2, !3}

!0 = distinct !DICompileUnit(language: DW_LANG_C11, file: !1, producer: "clang", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, splitDebugInlining: false, nameTableKind: None)
!1 = !DIFile(filename: "tiny.c", directory: "/hod/tests")
!2 = !{i32 7, !"Dwarf Version", i32 5}
!3 = !{i32 2, !"Debug Info Version", i32 3}
!10 = distinct !DISubprogram(name: "g", scope: !1, file: !1, line: 1, type: !11, scopeLine: 1, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !14)
!11 = !DISubroutineType(types: !12)
!12 = !{!13, !13}
!13 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!14 = !{}
!15 = !DILocalVariable(name: "x", arg: 1, scope: !10, file: !1, line: 1, type: !13)
!16 = !DILocation(line: 1, column: 11, scope: !10)
!17 = !DILocalVariable(name: "y", scope: !10, file: !1, line: 2, type: !13)
!18 = !DILocation(line: 2, column: 7, scope: !10)
!19 = !DILocation(line: 2, column: 11, scope: !10)
!20 = !DILocation(line: 2, column: 13, scope: !10)
!21 = !DILocation(line: 3, column: 10, scope: !10)
!22 = !DILocation(line: 3, column: 3, scope: !10)
"#;

fn options() -> hod::Options {
    hod::Options { output_kind: OutputKind::Object, ..hod::Options::default() }
}

fn compile(text: &str) -> (Result<Vec<u8>, CompileError>, Report) {
    let mut report = Report::default();
    let out = compile_with(text.as_bytes(), &options(), &Settings::default(), &mut report);
    (out, report)
}

/// The decline's construct name; panics on anything else.
fn declined(text: &str) -> (String, Report) {
    match compile(text) {
        (Err(CompileError::Unsupported(u)), report) => match u.construct {
            Construct::Named(name) => (name.into_owned(), report),
            other => panic!("declined, but not by name: {other:?}"),
        },
        (other, _) => panic!("expected a decline, got {:?}", other.map(|o| o.len())),
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// `TINY` with the unit's `retainedTypes:` set to `!{!30}` and `nodes`
/// appended.
fn with_retained(nodes: &str) -> String {
    let text = TINY.replace("nameTableKind: None)", "nameTableKind: None, retainedTypes: !29)");
    format!("{text}!29 = !{{!30}}\n{nodes}")
}

#[test]
fn a_dwarf_5_module_is_compiled_with_its_dies() {
    let (out, _) = compile(TINY);
    let object = out.unwrap_or_else(|e| panic!("{e}"));
    for section in [&b".debug_info"[..], b".debug_abbrev", b".debug_line", b".debug_str"] {
        assert!(contains(&object, section), "no {}", String::from_utf8_lossy(section));
    }
    // The variable's name is in `.debug_str`.
    assert!(contains(&object, b"\0y\0"));
}

#[test]
fn a_module_without_the_version_flag_is_declined_as_dwarf_4() {
    // No `Dwarf Version` is LLVM's 4; hod writes 4 only when the flag says
    // so (WP-114, `crates/hod/tests/dwarf4.rs`).
    let text = TINY.replace("!llvm.module.flags = !{!2, !3}", "!llvm.module.flags = !{!3}");
    assert_eq!(declined(&text).0, "debug:dwarf-version");
}

#[test]
fn a_module_without_debug_information_needs_no_version() {
    // `NoDebug` units write nothing, whatever the version.
    let text = TINY
        .replace(r#""Dwarf Version", i32 5"#, r#""Dwarf Version", i32 4"#)
        .replace("emissionKind: FullDebug", "emissionKind: NoDebug");
    let text: String = text
        .lines()
        .filter(|l| !l.contains("#dbg_declare"))
        .map(|l| {
            l.find(", !dbg !").map_or_else(|| format!("{l}\n"), |at| format!("{}\n", &l[..at]))
        })
        .collect::<String>()
        .replace(" #0 !dbg !10 {", " #0 {");
    let (out, _) = compile(&text);
    let object = out.unwrap_or_else(|e| panic!("{e}"));
    assert!(!contains(&object, b".debug_info"));
}

#[test]
fn a_refusal_of_the_unit_plan_is_declined_before_code_generation() {
    // A retained type scoped in a lexical block: the plan refuses it.
    let text = with_retained(
        "!30 = !DIDerivedType(tag: DW_TAG_typedef, name: \"T\", scope: !31, file: !1, line: 2, baseType: !13)\n\
         !31 = distinct !DILexicalBlock(scope: !10, file: !1, line: 2, column: 3)\n",
    );
    let (name, report) = declined(&text);
    assert_eq!(name, "debug:local-type-scope");
    let phases: Vec<&str> = report.phases.iter().map(|&(p, _)| p).collect();
    assert!(!phases.contains(&"emission"), "code was generated first: {phases:?}");
}

#[test]
fn malformed_debug_metadata_is_a_decline_not_an_internal_error() {
    // A typedef that is its own base type.
    let text = with_retained(
        "!30 = !DIDerivedType(tag: DW_TAG_typedef, name: \"T\", scope: !1, file: !1, line: 2, baseType: !30)\n",
    );
    match compile(&text) {
        (Err(CompileError::Unsupported(u)), _) => {
            assert_eq!(u.construct, Construct::Named("debug:malformed-metadata".into()));
        }
        (other, _) => panic!("expected a decline, got {:?}", other.map(|o| o.len())),
    }
}
