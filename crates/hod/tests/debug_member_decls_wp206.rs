//! WP-206 (`spec/dwarf.md` §6.27): the member function declarations the
//! DIE writer refused (`debug:die-unsupported`) and now writes in family
//! 114, as `llc` 19 and 22 write them, on both targets and in DWARF 5 and
//! 4:
//!
//! - a calling convention (`DW_AT_calling_convention`), on an ordinary,
//!   a static, a virtual and an artificial member (clang's static invoker
//!   of a lambda under `__attribute__((preserve_most))`);
//! - `artificial` with `noreturn` (kiln's static invoker of a noreturn
//!   lambda, the saved logging library module's `__invoke`);
//! - `explicit` with `noreturn` (a conversion operator, a constructor).
//!
//! A ref-qualifier or `deleted` together with any of those, a nameless
//! one, and a virtual member without a vtable index stay refused, by name
//! with the node and its function.
//!
//! Each tree is compared with `llc`'s (`-O0 -global-isel=0
//! -fast-isel=0`) line by line, offsets and indices masked, when `llc` is
//! installed; hod's object verifies.

use std::path::Path;
use std::time::Duration;

use hod::{CompileError, Construct, OutputKind, Report, Settings, compile_with};
use hod_testutil::{Scratch, run_with, tool};

const TRIPLES: [&str; 2] = ["aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"];

/// `struct S` with one declaration of each shape (§6.27), reached through
/// a global of its type.
const DECLS: &str = r#"target triple = "TRIPLE"

@s = dso_local global [8 x i8] zeroinitializer, align 8, !dbg !5

!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!2, !3}

!0 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !1, producer: "kiln", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, globals: !4, splitDebugInlining: false, nameTableKind: None)
!1 = !DIFile(filename: "decls.cpp", directory: "/hod/tests")
!2 = !{i32 7, !"Dwarf Version", i32 5}
!3 = !{i32 2, !"Debug Info Version", i32 3}
!4 = !{!5}
!5 = !DIGlobalVariableExpression(var: !6, expr: !DIExpression())
!6 = distinct !DIGlobalVariable(name: "s", scope: !1, file: !1, line: 20, type: !10, isLocal: false, isDefinition: true)
!7 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!8 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !10, size: 64, flags: DIFlagArtificial | DIFlagObjectPointer)
!9 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !13, size: 64, flags: DIFlagArtificial | DIFlagObjectPointer)
!10 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "S", file: !1, line: 1, size: 64, flags: DIFlagTypePassByReference | DIFlagNonTrivial, elements: !11, vtableHolder: !10, identifier: "_ZTS1S")
!11 = !{!20, !21, !22, !23, !24, !25, !26}
!13 = !DIDerivedType(tag: DW_TAG_const_type, baseType: !10)
!14 = !DISubroutineType(cc: DW_CC_LLVM_PreserveMost, types: !15)
!15 = !{!7, !8}
!16 = !DISubroutineType(cc: DW_CC_LLVM_PreserveMost, types: !17)
!17 = !{null}
!18 = !DISubroutineType(types: !17)
!19 = !DISubroutineType(types: !27)
!27 = !{!7, !9}
!28 = !DISubroutineType(types: !29)
!29 = !{null, !8, !7}
!30 = !DISubroutineType(cc: DW_CC_LLVM_PreserveMost, types: !31)
!31 = !{!7, !7}
!20 = !DISubprogram(name: "f", linkageName: "_ZN1S1fEv", scope: !10, file: !1, line: 2, type: !14, scopeLine: 2, flags: DIFlagPrototyped, spFlags: 0)
!21 = !DISubprogram(name: "g", linkageName: "_ZN1S1gEv", scope: !10, file: !1, line: 3, type: !16, scopeLine: 3, flags: DIFlagPrototyped | DIFlagStaticMember | DIFlagNoReturn, spFlags: 0)
!22 = !DISubprogram(name: "__invoke", linkageName: "_ZN1S8__invokeEv", scope: !10, line: 0, type: !18, scopeLine: 0, flags: DIFlagNoReturn | DIFlagStaticMember | DIFlagArtificial | DIFlagPublic | DIFlagPrototyped, spFlags: DISPFlagLocalToUnit)
!23 = !DISubprogram(name: "__invoke", linkageName: "_ZN1S8__invokeEi", scope: !10, type: !30, flags: DIFlagArtificial | DIFlagPrototyped | DIFlagStaticMember, spFlags: DISPFlagLocalToUnit)
!24 = !DISubprogram(name: "operator int", linkageName: "_ZNK1ScviEv", scope: !10, file: !1, line: 5, type: !19, scopeLine: 5, flags: DIFlagPrototyped | DIFlagExplicit | DIFlagNoReturn, spFlags: 0)
!25 = !DISubprogram(name: "S", scope: !10, file: !1, line: 6, type: !28, scopeLine: 6, flags: DIFlagPrivate | DIFlagPrototyped | DIFlagExplicit | DIFlagNoReturn, spFlags: 0)
!26 = !DISubprogram(name: "v", linkageName: "_ZN1S1vEv", scope: !10, file: !1, line: 7, type: !14, scopeLine: 7, containingType: !10, virtualIndex: 0, flags: DIFlagPrototyped, spFlags: DISPFlagVirtual)
"#;

/// kiln's `void (*p)() = []() __attribute__((noreturn)) { stop(); };`
/// and a call through `p`: the static invoker's definition, whose
/// declaration is artificial, static, noreturn and local to the unit.
const INVOKER: &str = r#"target triple = "TRIPLE"

%class.anon = type { i8 }

@p = dso_local global ptr @"_ZN3$_08__invokeEv", align 8, !dbg !0

define internal void @"_ZN3$_08__invokeEv"() #1 !dbg !16 {
  %1 = alloca %class.anon, align 1
  call void @"_ZNK3$_0clEv"(ptr noundef nonnull align 1 dereferenceable(1) %1), !dbg !20
  unreachable, !dbg !20
}

define dso_local void @_Z3usev() #0 !dbg !21 {
  %1 = load ptr, ptr @p, align 8, !dbg !22
  call void %1(), !dbg !22
  ret void, !dbg !23
}

define internal void @"_ZNK3$_0clEv"(ptr noundef nonnull align 1 dereferenceable(1) %0) #1 !dbg !24 {
  %2 = alloca ptr, align 8
  store ptr %0, ptr %2, align 8
    #dbg_declare(ptr %2, !30, !DIExpression(), !32)
  call void @_Z4stopv() #2, !dbg !33
  unreachable, !dbg !33
}

declare void @_Z4stopv() #2

attributes #0 = { noinline nounwind optnone "frame-pointer"="all" }
attributes #1 = { noinline noreturn nounwind optnone "frame-pointer"="all" }
attributes #2 = { noreturn nounwind }

!llvm.dbg.cu = !{!2}
!llvm.module.flags = !{!8, !9}

!0 = !DIGlobalVariableExpression(var: !1, expr: !DIExpression())
!1 = distinct !DIGlobalVariable(name: "p", scope: !2, file: !3, line: 2, type: !5, isLocal: false, isDefinition: true)
!2 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !3, producer: "kiln", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, splitDebugInlining: false, nameTableKind: None, globals: !4)
!3 = !DIFile(filename: "invoker.cpp", directory: "/hod/tests")
!4 = !{!0}
!5 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !6, size: 64)
!6 = !DISubroutineType(types: !7)
!7 = !{null}
!8 = !{i32 7, !"Dwarf Version", i32 5}
!9 = !{i32 2, !"Debug Info Version", i32 3}
!16 = distinct !DISubprogram(name: "__invoke", linkageName: "_ZN3$_08__invokeEv", scope: !17, file: !3, line: 2, type: !6, scopeLine: 2, flags: DIFlagPrototyped | DIFlagNoReturn | DIFlagArtificial, spFlags: DISPFlagLocalToUnit | DISPFlagDefinition, unit: !2, declaration: !19)
!17 = distinct !DICompositeType(tag: DW_TAG_class_type, file: !3, line: 2, size: 8, flags: DIFlagTypePassByValue, elements: !18)
!18 = !{}
!19 = !DISubprogram(name: "__invoke", linkageName: "_ZN3$_08__invokeEv", scope: !17, line: 0, type: !6, scopeLine: 0, flags: DIFlagNoReturn | DIFlagStaticMember | DIFlagArtificial | DIFlagPublic | DIFlagPrototyped, spFlags: DISPFlagLocalToUnit)
!20 = !DILocation(line: 2, column: 15, scope: !16)
!21 = distinct !DISubprogram(name: "use", linkageName: "_Z3usev", scope: !3, file: !3, line: 3, type: !6, scopeLine: 3, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !2)
!22 = !DILocation(line: 3, column: 14, scope: !21)
!23 = !DILocation(line: 3, column: 19, scope: !21)
!24 = distinct !DISubprogram(name: "operator()", linkageName: "_ZNK3$_0clEv", scope: !17, file: !3, line: 2, type: !25, scopeLine: 2, flags: DIFlagPrototyped | DIFlagNoReturn, spFlags: DISPFlagLocalToUnit | DISPFlagDefinition, unit: !2, declaration: !29, retainedNodes: !18)
!25 = !DISubroutineType(types: !26)
!26 = !{null, !27}
!27 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !28, size: 64, flags: DIFlagArtificial | DIFlagObjectPointer)
!28 = !DIDerivedType(tag: DW_TAG_const_type, baseType: !17)
!29 = !DISubprogram(name: "operator()", linkageName: "_ZNK3$_0clEv", scope: !17, file: !3, line: 2, type: !25, scopeLine: 2, flags: DIFlagNoReturn | DIFlagPublic | DIFlagPrototyped, spFlags: DISPFlagLocalToUnit)
!30 = !DILocalVariable(name: "this", arg: 1, scope: !24, line: 0, type: !31, flags: DIFlagArtificial | DIFlagObjectPointer)
!31 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !28, size: 64)
!32 = !DILocation(line: 0, scope: !24)
!33 = !DILocation(line: 2, column: 36, scope: !24)
"#;

fn module(template: &str, triple: &str, version: u32) -> String {
    template
        .replace("TRIPLE", triple)
        .replace("!\"Dwarf Version\", i32 5}", &format!("!\"Dwarf Version\", i32 {version}}}"))
}

fn compile(text: &str) -> Result<Vec<u8>, CompileError> {
    let options = hod::Options { output_kind: OutputKind::Object, ..hod::Options::default() };
    compile_with(text.as_bytes(), &options, &Settings::default(), &mut Report::default())
}

fn run_tool(tool: &Path, args: &[&std::ffi::OsStr]) -> String {
    let mut command = std::process::Command::new(tool);
    command.args(args);
    let out = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(out.success(), "{command:?}: {}", out.stderr_text());
    out.stdout_text()
}

/// The DIE tree of `object`, one attribute per line, offsets, addresses,
/// sizes and indices masked; without the code ranges, the producer, the
/// places of locals and frames (hod's frames are not `llc`'s) and the
/// object pointer's offset (§6.7: compared as present).
fn tree(dwarfdump: &Path, object: &Path) -> Vec<String> {
    let text = run_tool(dwarfdump, &["--debug-info".as_ref(), object.as_os_str()]);
    let mut lines = Vec::new();
    for line in text.lines().skip_while(|l| !l.contains("DW_TAG_compile_unit")) {
        let line = line.trim();
        if line.is_empty()
            || line.starts_with("DW_AT_high_pc")
            || line.starts_with("DW_AT_low_pc")
            || line.starts_with("DW_AT_producer")
        {
            continue;
        }
        let line = line
            .split_once(": ")
            .map_or(line, |(at, rest)| if at.starts_with("0x") { rest } else { line });
        let kept = ["DW_AT_location", "DW_AT_frame_base", "DW_AT_object_pointer"]
            .into_iter()
            .find(|a| line.starts_with(a));
        lines.push(kept.map_or_else(|| mask_hex(line), str::to_owned));
    }
    lines
}

/// A line with every hexadecimal number (an offset, an index, an
/// address) masked, a constant `(0x08)` read as `(8)`.
fn mask_hex(line: &str) -> String {
    if let Some((at, value)) = line.split_once("\t(0x")
        && let Some(digits) = value.strip_suffix(')')
        && let Ok(n) = u64::from_str_radix(digits, 16)
    {
        return format!("{at}\t({n})");
    }
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(at) = rest.find("0x") {
        out.push_str(&rest[..at]);
        out.push_str("0x_");
        rest = rest[at + 2..].trim_start_matches(|c: char| c.is_ascii_hexdigit());
    }
    out.push_str(rest);
    out
}

/// hod's object of `text`, compared with `llc` 19's and 22's when they
/// are installed: the same DIE tree, hod's verified. Returns hod's tree.
fn compare(scratch: &Scratch, name: &str, text: &str) -> Vec<String> {
    let object = compile(text).unwrap_or_else(|e| panic!("{name}: {e}"));
    let hod = scratch.write(&format!("{name}.hod.o"), &object);
    let Some(dwarfdump) = tool("llvm-dwarfdump") else { return Vec::new() };
    let verified = run_tool(&dwarfdump, &["--verify".as_ref(), hod.as_os_str()]);
    assert!(verified.contains("No errors."), "{name}: {verified}");
    let ours = tree(&dwarfdump, &hod);
    let ll = scratch.write(&format!("{name}.ll"), text);
    for llc in ["llc-19", "llc-22"] {
        let Some(llc_path) = tool(llc) else { continue };
        let reference = scratch.join(format!("{name}.{llc}.o"));
        let mut command = std::process::Command::new(&llc_path);
        command
            .args(["-O0", "-global-isel=0", "-fast-isel=0", "-filetype=obj", "-o"])
            .arg(&reference)
            .arg(&ll);
        let out = run_with(&mut command, &[], Duration::from_mins(2));
        assert!(out.success(), "{name}: {llc}: {}", out.stderr_text());
        let theirs = tree(&dwarfdump, &reference);
        if let Some(i) = (0..ours.len().max(theirs.len())).find(|&i| ours.get(i) != theirs.get(i)) {
            panic!(
                "{name}: {llc}: line {i}: hod {:?}, llc {:?}\nhod:\n{}\nllc:\n{}",
                ours.get(i),
                theirs.get(i),
                ours.join("\n"),
                theirs.join("\n")
            );
        }
    }
    ours
}

/// The `debug:` construct and detail `text` is refused with.
fn refusal(text: &str) -> (String, String) {
    match compile(text) {
        Err(CompileError::Unsupported(u)) => {
            let Construct::Named(name) = u.construct else { panic!("{:?}", u.construct) };
            (name.to_string(), u.detail.unwrap_or_default())
        }
        other => panic!("expected a decline, got {:?}", other.map(|o| o.len())),
    }
}

fn count(tree: &[String], line: &str) -> usize {
    tree.iter().filter(|l| l.trim() == line).count()
}

/// §6.27: every shape of family 114 in one class, as `llc` writes it.
#[test]
fn member_declarations_of_family_114_are_written_as_llc_writes_them() {
    let scratch = Scratch::new("wp206-decls");
    for triple in TRIPLES {
        for version in [5, 4] {
            let name = format!("decls-{}-v{version}", &triple[..6]);
            let ours = compare(&scratch, &name, &module(DECLS, triple, version));
            if ours.is_empty() {
                continue;
            }
            assert_eq!(
                count(&ours, "DW_AT_calling_convention\t(DW_CC_LLVM_PreserveMost)"),
                4,
                "{name}\n{ours:?}"
            );
            assert_eq!(count(&ours, "DW_AT_noreturn\t(true)"), 4, "{name}\n{ours:?}");
            assert_eq!(count(&ours, "DW_AT_explicit\t(true)"), 2, "{name}\n{ours:?}");
            assert_eq!(count(&ours, "DW_AT_name\t(\"__invoke\")"), 2, "{name}\n{ours:?}");
        }
    }
}

/// kiln's static invoker of a noreturn lambda, with its code: the
/// declaration is artificial and noreturn, the definition names it.
#[test]
fn a_noreturn_lambdas_static_invoker_is_declared_in_its_class() {
    let scratch = Scratch::new("wp206-invoker");
    for triple in TRIPLES {
        let name = format!("invoker-{}", &triple[..6]);
        let ours = compare(&scratch, &name, &module(INVOKER, triple, 5));
        if ours.is_empty() {
            continue;
        }
        assert_eq!(count(&ours, "DW_AT_name\t(\"__invoke\")"), 1, "{name}\n{ours:?}");
        assert_eq!(count(&ours, "DW_AT_artificial\t(true)"), 3, "{name}\n{ours:?}");
    }
}

/// What family 114 does not hold stays `debug:die-unsupported`, its
/// detail naming the node, the member and what it has.
#[test]
fn ref_qualified_deleted_and_nameless_combinations_are_refused() {
    let edits: [(&str, &str, &str); 5] = [
        (
            "!20 = !DISubprogram(name: \"f\", linkageName: \"_ZN1S1fEv\", scope: !10, file: !1, line: 2, type: !14, scopeLine: 2, flags: DIFlagPrototyped,",
            "!20 = !DISubprogram(name: \"f\", linkageName: \"_ZN1S1fEv\", scope: !10, file: !1, line: 2, type: !14, scopeLine: 2, flags: DIFlagPrototyped | DIFlagLValueReference,",
            "'f' in function '_ZN1S1fEv': a member function declaration with a ref-qualifier or deleted",
        ),
        (
            "flags: DIFlagNoReturn | DIFlagStaticMember | DIFlagArtificial | DIFlagPublic | DIFlagPrototyped, spFlags: DISPFlagLocalToUnit)",
            "flags: DIFlagNoReturn | DIFlagArtificial | DIFlagRValueReference | DIFlagPublic | DIFlagPrototyped, spFlags: DISPFlagLocalToUnit)",
            "'__invoke' in function '_ZN1S8__invokeEv': a member function declaration with a ref-qualifier or deleted",
        ),
        (
            "flags: DIFlagPrototyped | DIFlagExplicit | DIFlagNoReturn, spFlags: 0)",
            "flags: DIFlagPrototyped | DIFlagExplicit | DIFlagNoReturn, spFlags: DISPFlagDeleted)",
            "'operator int' in function '_ZNK1ScviEv': a member function declaration with a ref-qualifier or deleted",
        ),
        (
            "!20 = !DISubprogram(name: \"f\", linkageName: \"_ZN1S1fEv\",",
            "!20 = !DISubprogram(linkageName: \"_ZN1S1fEv\",",
            "in function '_ZN1S1fEv': a member function declaration without a name",
        ),
        (
            "containingType: !10, virtualIndex: 0, flags",
            "containingType: !10, virtualIndex: 4294967295, flags",
            "'v' in function '_ZN1S1vEv': a member function declaration with a virtual function without a vtable index",
        ),
    ];
    for triple in TRIPLES {
        for (from, to, detail) in edits {
            let text = module(DECLS, triple, 5);
            assert!(text.contains(from), "{from}");
            let (construct, got) = refusal(&text.replacen(from, to, 1));
            assert_eq!(construct, "debug:die-unsupported", "{triple}: {got}");
            assert!(got.starts_with("DISubprogram (node "), "{triple}: {got}");
            assert!(got.contains(detail), "{triple}: {detail:?} not in {got:?}");
        }
    }
}
