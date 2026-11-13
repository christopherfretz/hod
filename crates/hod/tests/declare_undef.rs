//! WP-156 (`spec/dwarf.md` §6.22): a `#dbg_declare` whose address is
//! `poison` (clang 22) or `undef` (clang 19) has no row, and its variable
//! no DIE, as `llc` writes none (`SelectionDAGBuilder::visitDbgDeclare`
//! drops a declaration of an `UndefValue`). Clang writes one on x86-64
//! for an empty-class parameter of an `always_inline` callee (libstdc++'s
//! `std::__distance(first, last, random_access_iterator_tag)`): the SysV
//! ABI passes no argument for an empty record. hod refused the module
//! (`debug:declare-address`) before.
//!
//! - `compiles_on_both_targets`: both spellings, both targets, the module
//!   is compiled (`--strict`: no refusal);
//! - `trees_equal_llc`: the DIE tree (names, tags and attributes; code
//!   addresses and frame offsets masked) equals `llc` 22's
//!   (`-O0 -fast-isel=0`, `-global-isel=0` on AArch64), both objects
//!   `--verify` clean. The module has the inlined case and a retained
//!   local of an ordinary function declared at `poison`, which both
//!   writers give a DIE without a location (§6.4 "Retained variables").

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use hod::{CompileError, OutputKind, Report, Settings, compile_with};
use hod_testutil::{Scratch, run_with, tool, tools_required};

const MODULE: &str = r#"target triple = "TRIPLE"

%struct.tag = type { i8 }

define dso_local i64 @count(ptr noundef %0, ptr noundef %1) !dbg !13 {
  %3 = alloca ptr, align 8
  %4 = alloca ptr, align 8
  %5 = alloca i64, align 8
  store ptr %0, ptr %3, align 8
    #dbg_declare(ptr %3, !30, !DIExpression(), !34)
  store ptr %1, ptr %4, align 8
    #dbg_declare(ptr %4, !36, !DIExpression(), !37)
    #dbg_declare(ptr ADDRESS, !38, !DIExpression(), !39)
    #dbg_declare(ptr %5, !40, !DIExpression(), !41)
  %6 = load ptr, ptr %4, align 8, !dbg !42
  %7 = load ptr, ptr %3, align 8, !dbg !42
  %8 = ptrtoint ptr %6 to i64, !dbg !42
  %9 = ptrtoint ptr %7 to i64, !dbg !42
  %10 = sub i64 %8, %9, !dbg !42
  store i64 %10, ptr %5, align 8, !dbg !41
  %11 = load i64, ptr %5, align 8, !dbg !45
  ret i64 %11, !dbg !46
}

define dso_local i32 @kept(i32 noundef %0) !dbg !60 {
  %2 = alloca i32, align 4
  store i32 %0, ptr %2, align 4
    #dbg_declare(ptr %2, !63, !DIExpression(), !64)
    #dbg_declare(ptr ADDRESS, !65, !DIExpression(), !64)
  %3 = load i32, ptr %2, align 4, !dbg !64
  ret i32 %3, !dbg !64
}

!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!5, !6}

!0 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !1, producer: "clang", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, splitDebugInlining: false, nameTableKind: None)
!1 = !DIFile(filename: "tag.cpp", directory: "/hod/tests")
!3 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "tag", file: !1, line: 1, size: 8, flags: DIFlagTypePassByValue, elements: !4, identifier: "_ZTS3tag")
!4 = !{}
!5 = !{i32 7, !"Dwarf Version", i32 5}
!6 = !{i32 2, !"Debug Info Version", i32 3}
!13 = distinct !DISubprogram(name: "count", scope: !1, file: !1, line: 8, type: !14, scopeLine: 8, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !4)
!14 = !DISubroutineType(types: !15)
!15 = !{!16, !17, !17}
!16 = !DIBasicType(name: "long", size: 64, encoding: DW_ATE_signed)
!17 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !19, size: 64)
!19 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!30 = !DILocalVariable(name: "first", arg: 1, scope: !31, file: !1, line: 3, type: !17)
!31 = distinct !DISubprogram(name: "span", linkageName: "_Z4spanPiS_3tag", scope: !1, file: !1, line: 3, type: !32, scopeLine: 3, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !4)
!32 = !DISubroutineType(types: !33)
!33 = !{!16, !17, !17, !3}
!34 = !DILocation(line: 3, column: 20, scope: !31, inlinedAt: !35)
!35 = distinct !DILocation(line: 9, column: 10, scope: !13)
!36 = !DILocalVariable(name: "last", arg: 2, scope: !31, file: !1, line: 3, type: !17)
!37 = !DILocation(line: 3, column: 30, scope: !31, inlinedAt: !35)
!38 = !DILocalVariable(name: "t", arg: 3, scope: !31, file: !1, line: 3, type: !3)
!39 = !DILocation(line: 3, column: 40, scope: !31, inlinedAt: !35)
!40 = !DILocalVariable(name: "n", scope: !31, file: !1, line: 4, type: !16)
!41 = !DILocation(line: 4, column: 8, scope: !31, inlinedAt: !35)
!42 = !DILocation(line: 4, column: 17, scope: !31, inlinedAt: !35)
!45 = !DILocation(line: 5, column: 10, scope: !31, inlinedAt: !35)
!46 = !DILocation(line: 9, column: 3, scope: !13)
!60 = distinct !DISubprogram(name: "kept", scope: !1, file: !1, line: 12, type: !61, scopeLine: 12, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !66)
!61 = !DISubroutineType(types: !62)
!62 = !{!19, !19}
!63 = !DILocalVariable(name: "v", arg: 1, scope: !60, file: !1, line: 12, type: !19)
!64 = !DILocation(line: 12, column: 14, scope: !60)
!65 = !DILocalVariable(name: "empty", scope: !60, file: !1, line: 13, type: !3)
!66 = !{!65}
"#;

const TRIPLES: [&str; 2] = ["aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"];
const ADDRESSES: [&str; 2] = ["poison", "undef"];

fn module(triple: &str, address: &str) -> String {
    MODULE.replace("TRIPLE", triple).replace("ADDRESS", address)
}

fn compile(text: &str) -> Result<Vec<u8>, CompileError> {
    let options = hod::Options { output_kind: OutputKind::Object, ..hod::Options::default() };
    compile_with(text.as_bytes(), &options, &Settings::default(), &mut Report::default())
}

#[test]
fn compiles_on_both_targets() {
    for triple in TRIPLES {
        for address in ADDRESSES {
            let object = compile(&module(triple, address))
                .unwrap_or_else(|e| panic!("{triple}, {address}: {e}"));
            assert!(object.windows(11).any(|w| w == b".debug_info"), "{triple}, {address}");
        }
    }
}

fn output(command: &mut Command) -> String {
    let outcome = run_with(command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "{command:?}: {}", outcome.stderr_text());
    outcome.stdout_text()
}

/// The values `spec/dwarf.md` §6.7 masks: code addresses and sizes,
/// section bases, and locations (frame offsets are each writer's own;
/// whether a DIE has a location is kept).
const MASKED: [&str; 9] = [
    "DW_AT_low_pc",
    "DW_AT_high_pc",
    "DW_AT_location",
    "DW_AT_frame_base",
    "DW_AT_addr_base",
    "DW_AT_str_offsets_base",
    "DW_AT_stmt_list",
    "DW_AT_producer",
    "DW_AT_abstract_origin",
];

/// The DIE tree of `object`, one line per DIE or attribute, references
/// by their target's name.
fn tree(dwarfdump: &Path, object: &Path) -> Vec<String> {
    let text = output(Command::new(dwarfdump).arg("--debug-info").arg(object));
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if let Some((addr, rest)) = line.split_once(':')
            && addr.starts_with("0x")
            && !addr.contains(' ')
        {
            let depth = rest.len() - rest.trim_start().len();
            let tag = rest.trim();
            if tag.starts_with("DW_TAG_") || tag == "NULL" {
                out.push(format!("{depth:>3} {tag}"));
            }
            continue;
        }
        if !trimmed.starts_with("DW_AT_") {
            continue;
        }
        let (name, value) = trimmed.split_once(char::is_whitespace).unwrap_or((trimmed, ""));
        if MASKED.contains(&name) {
            // An abstract origin is kept by the name it points at.
            let named = value.find(" \"").map(|q| value[q..].trim().to_owned());
            match (name, named) {
                ("DW_AT_abstract_origin", Some(n)) => out.push(format!("    {name} {n}")),
                _ => out.push(format!("    {name}")),
            }
            continue;
        }
        let value = value.trim();
        let mut v =
            match value.strip_prefix("(0x").and_then(|rest| rest.find(" \"").map(|q| (rest, q))) {
                Some((rest, q)) => format!("({}", &rest[q + 1..]),
                None => value.to_owned(),
            };
        // A constant by its value: the forms (`data1`, `udata`) are each
        // writer's own (§6.7).
        if let Some(num) = value.strip_prefix('(').and_then(|r| r.strip_suffix(')')) {
            let parsed = num
                .strip_prefix("0x")
                .map_or_else(|| num.parse::<u64>().ok(), |h| u64::from_str_radix(h, 16).ok());
            if let Some(n) = parsed {
                v = format!("({n})");
            }
        }
        out.push(format!("    {name} {v}"));
    }
    out
}

fn verify(dwarfdump: &Path, object: &Path) {
    let mut command = Command::new(dwarfdump);
    command.arg("--verify").arg(object);
    let outcome = run_with(&mut command, &[], Duration::from_mins(2));
    assert!(outcome.success(), "{} --verify: {}", object.display(), outcome.stdout_text());
}

#[test]
fn trees_equal_llc() {
    let (Some(llc), Some(dwarfdump)) = (tool("llc-22"), tool("llvm-dwarfdump-22")) else {
        assert!(!tools_required(), "llc-22 and llvm-dwarfdump-22 are required");
        eprintln!("note: no llc-22 or llvm-dwarfdump-22; skipping");
        return;
    };
    let scratch = Scratch::new("declare_undef");
    for triple in TRIPLES {
        for address in ADDRESSES {
            let name = format!("{}-{address}", &triple[..triple.find('-').unwrap()]);
            let text = module(triple, address);
            let ours = scratch.write(&format!("{name}.hod.o"), compile(&text).unwrap());
            let input = scratch.write(&format!("{name}.ll"), &text);
            let theirs = scratch.join(format!("{name}.llc.o"));
            let mut command = Command::new(&llc);
            command.args(["-O0", "-fast-isel=0", "-relocation-model=pic", "-filetype=obj"]);
            if triple.starts_with("aarch64") {
                command.arg("-global-isel=0");
            }
            output(command.arg(&input).arg("-o").arg(&theirs));
            verify(&dwarfdump, &ours);
            verify(&dwarfdump, &theirs);
            let (h, l) = (tree(&dwarfdump, &ours), tree(&dwarfdump, &theirs));
            assert_eq!(h.join("\n"), l.join("\n"), "{name}: hod's tree against llc's");
            // The inlined `t` has no DIE; the retained `empty` has one
            // without a location.
            let text = h.join("\n");
            assert!(!text.contains("(\"t\")"), "{name}: a DIE for the dropped parameter");
            assert!(text.contains("DW_AT_name (\"empty\")"), "{name}: the retained local");
        }
    }
}
