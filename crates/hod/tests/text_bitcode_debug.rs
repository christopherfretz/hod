//! WP-192 (`spec/dwarf.md` §4.4, `spec/bc.md` §9.2): the text of a
//! module with debug information and its bitcode build to the same
//! object, on both targets, DWARF 5 and 4, with the bitcode of LLVM 19's
//! and of LLVM 22's `llvm-as`.
//!
//! The two inputs number their metadata differently: the text's numbers
//! are its author's, the bitcode's are `llvm-as`'s enumeration. A line
//! table whose files were taken in node order (as before WP-192) had its
//! files and directories in another order for each input, and so other
//! file indices in `.debug_line` and in every `DW_AT_decl_file`. The
//! files are now in the order of their contents.

use std::time::Duration;

use hod::{OutputKind, Report, Settings, compile_with};
use hod_testutil::{run_with, tool};

const TRIPLES: [&str; 2] = ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"];

/// Kiln-shaped: the text numbers the files in the opposite order to the
/// one `llvm-as` enumerates them in (`b.h` before `a.h`, the global's
/// header last although `llvm-as` reaches it first). The files cover the
/// three shapes of the table: an absolute name with no directory, a
/// relative name in the compilation directory (split into a directory
/// of its own), and a name in another directory.
const SEED: &str = r#"target triple = "TRIPLE"

%struct.Pair = type { i32, i32 }

@counter = dso_local global i32 0, align 4, !dbg !40

define dso_local i32 @first(i32 %x) #0 !dbg !20 {
entry:
  %p = alloca %struct.Pair, align 4
  #dbg_declare(ptr %p, !23, !DIExpression(), !22)
  %r = add i32 %x, 1, !dbg !22
  ret i32 %r, !dbg !22
}

define dso_local i32 @second(i32 %x) #0 !dbg !30 {
entry:
  %r = mul i32 %x, 3, !dbg !31
  %c = load i32, ptr @counter, align 4, !dbg !32
  %s = add i32 %r, %c, !dbg !32
  ret i32 %s, !dbg !32
}

attributes #0 = { noinline nounwind optnone "frame-pointer"="all" }

!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!2, !3}

!0 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !1, producer: "kiln", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, globals: !44, splitDebugInlining: false, nameTableKind: None)
!1 = !DIFile(filename: "src/main.cc", directory: "/work", checksumkind: CSK_MD5, checksum: "00112233445566778899aabbccddeeff")
!2 = !{i32 7, !"Dwarf Version", i32 VERSION}
!3 = !{i32 2, !"Debug Info Version", i32 3}
!10 = !DIFile(filename: "/usr/include/b.h", directory: "", checksumkind: CSK_MD5, checksum: "0123456789abcdef0123456789abcdef")
!11 = !DIFile(filename: "/usr/include/a.h", directory: "", checksumkind: CSK_MD5, checksum: "fedcba9876543210fedcba9876543210")
!12 = !DISubroutineType(types: !13)
!13 = !{!14, !14}
!14 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!15 = !DIFile(filename: "lib/pair.h", directory: "/opt/pair", checksumkind: CSK_MD5, checksum: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
!16 = !DICompositeType(tag: DW_TAG_structure_type, name: "Pair", file: !15, line: 4, size: 64, flags: DIFlagTypePassByValue, elements: !17, identifier: "_ZTS4Pair")
!17 = !{!18, !19}
!18 = !DIDerivedType(tag: DW_TAG_member, name: "a", scope: !16, file: !15, line: 5, baseType: !14, size: 32)
!19 = !DIDerivedType(tag: DW_TAG_member, name: "b", scope: !16, file: !15, line: 6, baseType: !14, size: 32, offset: 32)
!20 = distinct !DISubprogram(name: "first", linkageName: "first", scope: !11, file: !11, line: 2, type: !12, scopeLine: 2, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0)
!22 = !DILocation(line: 3, column: 5, scope: !20)
!23 = !DILocalVariable(name: "p", scope: !20, file: !11, line: 3, type: !16)
!30 = distinct !DISubprogram(name: "second", linkageName: "second", scope: !10, file: !10, line: 7, type: !12, scopeLine: 7, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0)
!31 = !DILocation(line: 8, column: 5, scope: !30)
!32 = !DILocation(line: 9, column: 7, scope: !30)
!40 = !DIGlobalVariableExpression(var: !41, expr: !DIExpression())
!41 = distinct !DIGlobalVariable(name: "counter", scope: !0, file: !42, line: 1, type: !14, isLocal: false, isDefinition: true)
!42 = !DIFile(filename: "src/counter.h", directory: "/work", checksumkind: CSK_MD5, checksum: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
!44 = !{!40}
"#;

fn module(triple: &str, version: u32) -> String {
    SEED.replace("TRIPLE", triple).replace("VERSION", &version.to_string())
}

fn object(input: &[u8]) -> Vec<u8> {
    let options = hod::Options { output_kind: OutputKind::Object, ..hod::Options::default() };
    compile_with(input, &options, &Settings::default(), &mut Report::default())
        .unwrap_or_else(|e| panic!("hod builds the seed: {e}"))
}

fn run(tool: &std::path::Path, args: &[&str], stdin: &[u8]) -> Vec<u8> {
    let mut command = std::process::Command::new(tool);
    command.args(args);
    let out = run_with(&mut command, stdin, Duration::from_mins(2));
    assert!(out.success(), "{command:?}: {}", out.stderr_text());
    out.stdout
}

/// The files' names in the order a text numbers them.
fn file_order(text: &str) -> Vec<&str> {
    text.lines()
        .filter(|l| l.contains("!DIFile("))
        .filter_map(|l| l.split("filename: \"").nth(1)?.split('"').next())
        .collect()
}

#[test]
fn text_and_bitcode_with_debug_info_build_one_object() {
    let mut checked = 0;
    for major in [22, 19] {
        let Some(llvm_as) = tool(&format!("llvm-as-{major}")) else { continue };
        let llvm_dis = tool("llvm-dis");
        for triple in TRIPLES {
            for version in [5, 4] {
                let text = module(triple, version);
                let bitcode = run(&llvm_as, &["-", "-o", "-"], text.as_bytes());
                // The seed is the case: the bitcode numbers the files in
                // another order than the text.
                if let Some(dis) = &llvm_dis {
                    let back = run(dis, &["-", "-o", "-"], &bitcode);
                    let back = String::from_utf8(back).expect("llvm-dis writes text");
                    assert_ne!(
                        file_order(&text),
                        file_order(&back),
                        "llvm-as {major} keeps the text's order of files: the seed tests nothing"
                    );
                }
                let from_text = object(text.as_bytes());
                let from_bitcode = object(&bitcode);
                assert!(
                    from_text == from_bitcode,
                    "{triple}, DWARF {version}, llvm-as {major}: the text and the bitcode build \
                     different objects"
                );
                checked += 1;
            }
        }
    }
    eprintln!("{checked} text and bitcode pairs compared");
}
