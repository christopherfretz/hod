//! `-split-dwarf-file` and `-split-dwarf-output` through the facade
//! (`spec/cli.md` "The back half", `spec/dwarf.md` §6.14, WP-062): which
//! form each pair of options gives, the `.dwo` file written beside the
//! object, and what is refused before code generation.

use std::path::PathBuf;

use hod::{CompileError, Construct, OutputKind, Report, Settings, compile_with};
use hod_testutil::Scratch;

/// clang 22's `-g -O0 -gsplit-dwarf` IR of a small C function, its
/// producer and paths shortened.
const TINY: &str = r#"source_filename = "tiny.c"
target datalayout = "e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128"
target triple = "aarch64-unknown-linux-gnu"

define dso_local i32 @g(i32 noundef %0) #0 !dbg !10 {
  %2 = alloca i32, align 4
  store i32 %0, ptr %2, align 4
    #dbg_declare(ptr %2, !15, !DIExpression(), !16)
  %3 = load i32, ptr %2, align 4, !dbg !19
  ret i32 %3, !dbg !22
}

attributes #0 = { noinline nounwind optnone "frame-pointer"="non-leaf" }

!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!2, !3}

!0 = distinct !DICompileUnit(language: DW_LANG_C11, file: !1, producer: "clang", isOptimized: false, runtimeVersion: 0, splitDebugFilename: "tiny.dwo", emissionKind: FullDebug, splitDebugInlining: false, nameTableKind: None)
!1 = !DIFile(filename: "tiny.c", directory: "/hod/tests")
!2 = !{i32 7, !"Dwarf Version", i32 5}
!3 = !{i32 2, !"Debug Info Version", i32 3}
!10 = distinct !DISubprogram(name: "g", scope: !1, file: !1, line: 1, type: !11, scopeLine: 1, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !14)
!11 = !DISubroutineType(types: !12)
!12 = !{!13, !13}
!13 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!14 = !{}
!15 = !DILocalVariable(name: "xyzzy", arg: 1, scope: !10, file: !1, line: 1, type: !13)
!16 = !DILocation(line: 1, column: 11, scope: !10)
!19 = !DILocation(line: 2, column: 10, scope: !10)
!22 = !DILocation(line: 2, column: 3, scope: !10)
"#;

fn options(file: Option<&str>, output: Option<PathBuf>) -> hod::Options {
    hod::Options {
        output_kind: OutputKind::Object,
        split_dwarf_file: file.map(PathBuf::from),
        split_dwarf_output: output,
        ..hod::Options::default()
    }
}

fn compile(text: &str, options: &hod::Options) -> Result<Vec<u8>, CompileError> {
    compile_with(text.as_bytes(), options, &Settings::default(), &mut Report::default())
}

fn declined(text: &str, options: &hod::Options) -> String {
    match compile(text, options) {
        Err(CompileError::Unsupported(u)) => match u.construct {
            Construct::Named(name) => name.into_owned(),
            other => panic!("declined, but not by name: {other:?}"),
        },
        other => panic!("expected a decline, got {:?}", other.map(|o| o.len())),
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn both_options_write_the_dwo_file() {
    let scratch = Scratch::new("split-dwarf-facade");
    let dwo = scratch.join("out/tiny.dwo");
    std::fs::create_dir_all(dwo.parent().unwrap()).unwrap();
    let object = compile(TINY, &options(Some("tiny.dwo"), Some(dwo.clone()))).unwrap();
    // The skeleton names the `.dwo`; the DIEs are not in the object.
    assert!(contains(&object, b".debug_info\0") && contains(&object, b"tiny.dwo\0"));
    assert!(!contains(&object, b".dwo\0.") && !contains(&object, b".debug_info.dwo"));
    assert!(!contains(&object, b"xyzzy"));
    let file = std::fs::read(&dwo).unwrap();
    assert!(contains(&file, b".debug_info.dwo") && contains(&file, b"xyzzy\0"));
}

#[test]
fn the_file_alone_keeps_the_dwo_sections_in_the_object() {
    let object = compile(TINY, &options(Some("tiny.dwo"), None)).unwrap();
    assert!(contains(&object, b".debug_info.dwo") && contains(&object, b"xyzzy\0"));
}

#[test]
fn a_module_without_debug_information_still_gets_its_dwo_file() {
    let scratch = Scratch::new("split-dwarf-empty");
    let dwo = scratch.join("e.dwo");
    let text = "target triple = \"aarch64-unknown-linux-gnu\"\n\
                define i32 @f() {\n  ret i32 0\n}\n";
    compile(text, &options(Some("e.dwo"), Some(dwo.clone()))).unwrap();
    let file = std::fs::read(&dwo).unwrap();
    assert!(file.starts_with(b"\x7fELF") && !contains(&file, b".debug_"));
}

#[test]
fn what_the_facade_refuses() {
    // The output alone: `llc` writes an ordinary object and an empty file.
    let scratch = Scratch::new("split-dwarf-refusals");
    assert_eq!(
        declined(TINY, &options(None, Some(scratch.join("a.dwo")))),
        hod::OPTION_SPLIT_DWARF
    );
    // A name that is not the option's.
    assert_eq!(declined(TINY, &options(Some("other.dwo"), None)), hod_dwarf::SPLIT_NAME);
    // A unit without a name, with DIEs.
    let unnamed = TINY.replace("splitDebugFilename: \"tiny.dwo\", ", "");
    assert_eq!(declined(&unnamed, &options(Some("tiny.dwo"), None)), hod_dwarf::SPLIT_NAME);
    // A `.dwo` that cannot be written is an error of the options.
    let nowhere = scratch.join("no/such/dir/tiny.dwo");
    match compile(TINY, &options(Some("tiny.dwo"), Some(nowhere))) {
        Err(CompileError::InvalidOptions(m)) => assert!(m.contains("cannot write"), "{m}"),
        other => panic!("{:?}", other.map(|o| o.len())),
    }
}
