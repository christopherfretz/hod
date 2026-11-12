; Hand-written cases of the line policy (spec/dwarf.md 5.3, 5.4, 5.7):
; a block that starts with an instruction without a location, an
; instruction inlined from another file, an explicit line 0, two
; returns, and a function whose locations name no subprogram of its own.

@g = dso_local global i32 0, align 4, !dbg !40

define dso_local i32 @block_start(i32 %x) uwtable !dbg !10 {
entry:
  %c = icmp sgt i32 %x, 0, !dbg !11
  br i1 %c, label %a, label %b, !dbg !11
a:
  %y = add i32 %x, 1
  store i32 %y, ptr @g, align 4, !dbg !12
  br label %b, !dbg !12
b:
  %v = load i32, ptr @g, align 4, !dbg !13
  ret i32 %v, !dbg !13
}

define dso_local i32 @inlined(i32 %x) uwtable !dbg !20 {
entry:
  %a = add i32 %x, 3, !dbg !21
  store volatile i32 %a, ptr @g, align 4, !dbg !21
  %b = mul i32 %a, 5, !dbg !24
  store volatile i32 %b, ptr @g, align 4, !dbg !24
  %c = sub i32 %b, 1, !dbg !25
  ret i32 %c, !dbg !25
}

define dso_local i32 @line_zero(i32 %x) uwtable !dbg !30 {
entry:
  %a = add i32 %x, 1, !dbg !31
  store volatile i32 %a, ptr @g, align 4, !dbg !31
  %b = mul i32 %a, 7, !dbg !32
  store volatile i32 %b, ptr @g, align 4, !dbg !32
  %c = sub i32 %b, 2, !dbg !33
  ret i32 %c, !dbg !33
}

define dso_local i32 @two_rets(i32 %x) uwtable !dbg !50 {
entry:
  %c = icmp slt i32 %x, 10, !dbg !51
  br i1 %c, label %small, label %big, !dbg !51
small:
  %s = add i32 %x, 100, !dbg !52
  ret i32 %s, !dbg !52
big:
  %t = sub i32 %x, 100, !dbg !53
  ret i32 %t, !dbg !53
}

; No subprogram of its own: its locations are in `orphan`'s scope.
define dso_local i32 @no_subprogram(i32 %x) uwtable {
entry:
  %a = add i32 %x, 9, !dbg !61
  store volatile i32 %a, ptr @g, align 4, !dbg !61
  ret i32 %a, !dbg !62
}

!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!2, !3}

!0 = distinct !DICompileUnit(language: DW_LANG_C11, file: !1, producer: "hand", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, globals: !44, splitDebugInlining: false, nameTableKind: None)
!1 = !DIFile(filename: "hand.c", directory: "/hod/tests/lines")
!2 = !{i32 7, !"Dwarf Version", i32 5}
!3 = !{i32 2, !"Debug Info Version", i32 3}
!4 = !DISubroutineType(types: !5)
!5 = !{!6, !6}
!6 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!7 = !DIFile(filename: "inc/inl.h", directory: "/hod/tests/lines")

!10 = distinct !DISubprogram(name: "block_start", scope: !1, file: !1, line: 3, type: !4, scopeLine: 3, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0)
!11 = !DILocation(line: 4, column: 7, scope: !10)
!12 = !DILocation(line: 5, column: 5, scope: !10)
!13 = !DILocation(line: 6, column: 3, scope: !10)

!20 = distinct !DISubprogram(name: "inlined", scope: !1, file: !1, line: 9, type: !4, scopeLine: 9, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0)
!21 = !DILocation(line: 10, column: 11, scope: !20)
!22 = distinct !DISubprogram(name: "helper", scope: !7, file: !7, line: 2, type: !4, scopeLine: 2, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0)
!23 = !DILocation(line: 11, column: 10, scope: !20)
!24 = !DILocation(line: 3, column: 12, scope: !22, inlinedAt: !23)
!25 = !DILocation(line: 12, column: 3, scope: !20)

!30 = distinct !DISubprogram(name: "line_zero", scope: !1, file: !1, line: 15, type: !4, scopeLine: 15, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0)
!31 = !DILocation(line: 16, column: 11, scope: !30)
!32 = !DILocation(line: 0, column: 0, scope: !30)
!33 = !DILocation(line: 17, column: 3, scope: !30)

!40 = !DIGlobalVariableExpression(var: !41, expr: !DIExpression())
!41 = distinct !DIGlobalVariable(name: "g", scope: !0, file: !1, line: 1, type: !6, isLocal: false, isDefinition: true)
!44 = !{!40}

!50 = distinct !DISubprogram(name: "two_rets", scope: !1, file: !1, line: 20, type: !4, scopeLine: 20, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0)
!51 = !DILocation(line: 21, column: 7, scope: !50)
!52 = !DILocation(line: 22, column: 5, scope: !50)
!53 = !DILocation(line: 23, column: 3, scope: !50)

!60 = distinct !DISubprogram(name: "orphan", scope: !1, file: !1, line: 26, type: !4, scopeLine: 26, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0)
!61 = !DILocation(line: 27, column: 9, scope: !60)
!62 = !DILocation(line: 28, column: 3, scope: !60)
