; An instruction inlined from another file (spec/dwarf.md 5.4): its
; row keeps its own scope's file and line, and the function's DIEs have
; a DW_TAG_inlined_subroutine for it (spec/dwarf.md 6.9, WP-085).

@g = dso_local global i32 0, align 4

define dso_local i32 @inlined(i32 %x) uwtable !dbg !20 {
entry:
  %a = add i32 %x, 3, !dbg !21
  store volatile i32 %a, ptr @g, align 4, !dbg !21
  %b = mul i32 %a, 5, !dbg !24
  store volatile i32 %b, ptr @g, align 4, !dbg !24
  %c = sub i32 %b, 1, !dbg !25
  ret i32 %c, !dbg !25
}

!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!2, !3}

!0 = distinct !DICompileUnit(language: DW_LANG_C11, file: !1, producer: "hand", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, splitDebugInlining: false, nameTableKind: None)
!1 = !DIFile(filename: "hand.c", directory: "/hod/tests/lines")
!2 = !{i32 7, !"Dwarf Version", i32 5}
!3 = !{i32 2, !"Debug Info Version", i32 3}
!4 = !DISubroutineType(types: !5)
!5 = !{!6, !6}
!6 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!7 = !DIFile(filename: "inc/inl.h", directory: "/hod/tests/lines")

!20 = distinct !DISubprogram(name: "inlined", scope: !1, file: !1, line: 9, type: !4, scopeLine: 9, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0)
!21 = !DILocation(line: 10, column: 11, scope: !20)
!22 = distinct !DISubprogram(name: "helper", scope: !7, file: !7, line: 2, type: !4, scopeLine: 2, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0)
!23 = !DILocation(line: 11, column: 10, scope: !20)
!24 = !DILocation(line: 3, column: 12, scope: !22, inlinedAt: !23)
!25 = !DILocation(line: 12, column: 3, scope: !20)

