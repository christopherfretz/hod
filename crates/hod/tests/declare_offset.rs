//! WP-150 (`spec/dwarf.md` §6.21): a `#dbg_declare` of a stack slot plus
//! a constant offset is refused by name, `debug:declare-offset`, on both
//! targets, where it was an internal error of the code generator when
//! the address is also a call's operand (clang's `use(&q[1])` with the
//! declaration moved onto the `getelementptr`). The same function
//! declaring the slot itself is compiled.

use hod::{CompileError, Construct, OutputKind, Report, Settings, compile_with};

const MODULE: &str = r#"target triple = "TRIPLE"

define dso_local void @f() !dbg !10 {
  %1 = alloca [2 x i32], align 4
  %2 = getelementptr inbounds [2 x i32], ptr %1, i64 0, i64 1
    #dbg_declare(ptr ADDRESS, !15, !DIExpression(), !16)
  call void @use(ptr noundef %2), !dbg !16
  ret void, !dbg !16
}

declare void @use(ptr noundef)

!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!2, !3}

!0 = distinct !DICompileUnit(language: DW_LANG_C11, file: !1, producer: "clang", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, splitDebugInlining: false, nameTableKind: None)
!1 = !DIFile(filename: "offset.c", directory: "/hod/tests")
!2 = !{i32 7, !"Dwarf Version", i32 5}
!3 = !{i32 2, !"Debug Info Version", i32 3}
!10 = distinct !DISubprogram(name: "f", scope: !1, file: !1, line: 1, type: !11, scopeLine: 1, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !12)
!11 = !DISubroutineType(types: !12)
!12 = !{}
!13 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!15 = !DILocalVariable(name: "x", scope: !10, file: !1, line: 2, type: !13)
!16 = !DILocation(line: 2, column: 7, scope: !10)
"#;

const TRIPLES: [&str; 2] = ["aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"];

fn compile(text: &str) -> Result<Vec<u8>, CompileError> {
    let options = hod::Options { output_kind: OutputKind::Object, ..hod::Options::default() };
    compile_with(text.as_bytes(), &options, &Settings::default(), &mut Report::default())
}

#[test]
fn a_declaration_of_a_slot_plus_an_offset_is_refused_by_name() {
    for triple in TRIPLES {
        let text = MODULE.replace("TRIPLE", triple).replace("ADDRESS", "%2");
        match compile(&text) {
            Err(CompileError::Unsupported(u)) => {
                assert_eq!(
                    u.construct,
                    Construct::Named("debug:declare-offset".into()),
                    "{triple}"
                );
            }
            other => panic!("{triple}: expected a decline, got {:?}", other.map(|o| o.len())),
        }
    }
}

#[test]
fn a_declaration_of_the_slot_itself_is_compiled() {
    for triple in TRIPLES {
        let text = MODULE.replace("TRIPLE", triple).replace("ADDRESS", "%1");
        let object = compile(&text).unwrap_or_else(|e| panic!("{triple}: {e}"));
        assert!(object.windows(11).any(|w| w == b".debug_info"), "{triple}");
    }
}
