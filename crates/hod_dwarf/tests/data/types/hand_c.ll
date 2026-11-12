; spec/dwarf.md §6.8, §8.6: a C unit, where subroutine types are
; prototyped and an array's default lower bound is 0, written by hand.
source_filename = "hand.c"
target datalayout = "e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128-Fn32"
target triple = "aarch64-unknown-linux-gnu"

@fp = dso_local global ptr null, align 8, !dbg !0
@vp = dso_local global ptr null, align 8, !dbg !8
@kr = dso_local global ptr null, align 8, !dbg !13
@arr = dso_local global [4 x i32] zeroinitializer, align 4, !dbg !17

!llvm.dbg.cu = !{!30}
!llvm.module.flags = !{!90, !91}

!0 = !DIGlobalVariableExpression(var: !1, expr: !DIExpression())
!1 = distinct !DIGlobalVariable(name: "fp", scope: !30, file: !31, line: 1, type: !2, isLocal: false, isDefinition: true)
!2 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !3, size: 64)
!3 = !DISubroutineType(flags: DIFlagPrototyped, types: !4)
!4 = !{!5, !5, !6}
!5 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!6 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !7, size: 64)
!7 = !DIDerivedType(tag: DW_TAG_restrict_type, baseType: !21)
!8 = !DIGlobalVariableExpression(var: !9, expr: !DIExpression())
!9 = distinct !DIGlobalVariable(name: "vp", scope: !30, file: !31, line: 2, type: !10, isLocal: false, isDefinition: true)
!10 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !11, size: 64)
!11 = !DISubroutineType(flags: DIFlagPrototyped, types: !12)
!12 = !{null, !5, null}
!13 = !DIGlobalVariableExpression(var: !14, expr: !DIExpression())
!14 = distinct !DIGlobalVariable(name: "kr", scope: !30, file: !31, line: 3, type: !15, isLocal: false, isDefinition: true)
!15 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !16, size: 64)
!16 = !DISubroutineType(types: !22)
!17 = !DIGlobalVariableExpression(var: !18, expr: !DIExpression())
!18 = distinct !DIGlobalVariable(name: "arr", scope: !30, file: !31, line: 4, type: !19, isLocal: false, isDefinition: true)
!19 = !DICompositeType(tag: DW_TAG_array_type, baseType: !5, size: 128, elements: !20)
!20 = !{!23}
!21 = !DIBasicType(name: "char", size: 8, encoding: DW_ATE_signed_char)
!22 = !{!5}
!23 = !DISubrange(count: 4, lowerBound: 0)
!24 = !DICompositeType(tag: DW_TAG_array_type, baseType: !5, size: 128, elements: !25)
!25 = !{!26}
!26 = !DISubrange(count: 4, lowerBound: 1)
!27 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "bits", file: !31, line: 6, size: 32, elements: !28)
!28 = !{!29, !32}
!29 = !DIDerivedType(tag: DW_TAG_member, name: "a", scope: !27, file: !31, line: 7, baseType: !33, size: 3, flags: DIFlagBitField, extraData: i64 0)
!32 = !DIDerivedType(tag: DW_TAG_member, name: "b", scope: !27, file: !31, line: 8, baseType: !33, size: 9, offset: 3, flags: DIFlagBitField, extraData: i64 0)
!33 = !DIBasicType(name: "unsigned int", size: 32, encoding: DW_ATE_unsigned)

!30 = distinct !DICompileUnit(language: DW_LANG_C11, file: !31, producer: "hand", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, retainedTypes: !34, globals: !35, splitDebugInlining: false, nameTableKind: None)
!31 = !DIFile(filename: "hand.c", directory: "/hod/tests/types")
!34 = !{!24, !27}
!35 = !{!0, !8, !13, !17}

!90 = !{i32 7, !"Dwarf Version", i32 5}
!91 = !{i32 2, !"Debug Info Version", i32 3}
