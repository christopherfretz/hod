; spec/dwarf.md §6.8, §8.6: `_BitInt` with a `dataSize` (LLVM 22's IR),
; written by hand.
source_filename = "hand22.cpp"
target datalayout = "e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128-Fn32"
target triple = "aarch64-unknown-linux-gnu"

@b7 = dso_local global i8 3, align 1, !dbg !0
@u13 = dso_local global i16 4, align 2, !dbg !3

!llvm.dbg.cu = !{!10}
!llvm.module.flags = !{!90, !91}

!0 = !DIGlobalVariableExpression(var: !1, expr: !DIExpression())
!1 = distinct !DIGlobalVariable(name: "b7", scope: !10, file: !11, line: 1, type: !2, isLocal: false, isDefinition: true)
!2 = !DIBasicType(name: "_BitInt(7)", size: 8, dataSize: 7, encoding: DW_ATE_signed)
!3 = !DIGlobalVariableExpression(var: !4, expr: !DIExpression())
!4 = distinct !DIGlobalVariable(name: "u13", scope: !10, file: !11, line: 2, type: !5, isLocal: false, isDefinition: true)
!5 = !DIBasicType(name: "unsigned _BitInt(13)", size: 16, dataSize: 13, encoding: DW_ATE_unsigned)
!6 = !DIBasicType(name: "_BitInt(16)", size: 16, dataSize: 16, encoding: DW_ATE_signed)

!10 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !11, producer: "hand", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, retainedTypes: !12, globals: !13, splitDebugInlining: false, nameTableKind: None)
!11 = !DIFile(filename: "hand22.cpp", directory: "/hod/tests/types")
!12 = !{!6}
!13 = !{!0, !3}

!90 = !{i32 7, !"Dwarf Version", i32 5}
!91 = !{i32 2, !"Debug Info Version", i32 3}
