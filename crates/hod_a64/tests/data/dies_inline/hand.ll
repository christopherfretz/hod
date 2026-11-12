; Inlined code the C++ sources do not reach (spec/dwarf.md 6.9):
; - @split: the code of an inlined call is interrupted by its caller's,
;   so the inlined subroutine has two ranges (DW_AT_ranges), and so does
;   the block inside it; the callee @callee has no function of its own;
; - @twice: two calls of @callee, two instances of one abstract DIE,
;   the second written by a function after the abstract instance is,
;   with a variable the first did not have (written in full);
; - @nested: a call inlined into an inlined call, whose block has no
;   variables (its label goes to the inlined subroutine).
; The test prepends the target lines.

@g = dso_local global i32 0, align 4

define dso_local i32 @split(i32 %x) !dbg !10 {
entry:
  %x.addr = alloca i32, align 4
  %v.addr = alloca i32, align 4
  %t = alloca i32, align 4
  store i32 %x, ptr %x.addr, align 4
    #dbg_declare(ptr %x.addr, !14, !DIExpression(), !15)
    #dbg_declare(ptr %v.addr, !31, !DIExpression(), !40)
  store i32 %x, ptr %v.addr, align 4, !dbg !40
  %a = load i32, ptr %x.addr, align 4, !dbg !16
  store volatile i32 %a, ptr @g, align 4, !dbg !16
    #dbg_declare(ptr %t, !32, !DIExpression(), !41)
  %b = load i32, ptr %v.addr, align 4, !dbg !41
  store i32 %b, ptr %t, align 4, !dbg !41
  store volatile i32 %a, ptr @g, align 4, !dbg !16
  %c = load i32, ptr %t, align 4, !dbg !41
  store volatile i32 %c, ptr @g, align 4, !dbg !16
  %d = load i32, ptr %v.addr, align 4, !dbg !42
  %e = add i32 %d, %a, !dbg !17
  ret i32 %e, !dbg !17
}

define dso_local i32 @twice(i32 %x) !dbg !50 {
entry:
  %x.addr = alloca i32, align 4
  %v1 = alloca i32, align 4
  %v2 = alloca i32, align 4
  %u2 = alloca i32, align 4
  store i32 %x, ptr %x.addr, align 4
    #dbg_declare(ptr %x.addr, !51, !DIExpression(), !52)
    #dbg_declare(ptr %v1, !31, !DIExpression(), !60)
  store i32 %x, ptr %v1, align 4, !dbg !60
  %a = load i32, ptr %v1, align 4, !dbg !61
  store volatile i32 %a, ptr @g, align 4, !dbg !61
    #dbg_declare(ptr %v2, !31, !DIExpression(), !62)
  store i32 %a, ptr %v2, align 4, !dbg !62
    #dbg_declare(ptr %u2, !33, !DIExpression(), !62)
  store i32 %a, ptr %u2, align 4, !dbg !62
  %b = load i32, ptr %u2, align 4, !dbg !63
  ret i32 %b, !dbg !53
}

define dso_local i32 @nested(i32 %x) !dbg !70 {
entry:
  %x.addr = alloca i32, align 4
  %p = alloca i32, align 4
  %q = alloca i32, align 4
  store i32 %x, ptr %x.addr, align 4
    #dbg_declare(ptr %x.addr, !71, !DIExpression(), !72)
    #dbg_declare(ptr %p, !81, !DIExpression(), !90)
  store i32 %x, ptr %p, align 4, !dbg !90
    #dbg_declare(ptr %q, !31, !DIExpression(), !92)
  store i32 %x, ptr %q, align 4, !dbg !92
  %a = load i32, ptr %q, align 4, !dbg !93
  store volatile i32 %a, ptr @g, align 4, !dbg !93
  br label %in, !dbg !94
in:
    #dbg_label(!84, !95)
  %b = load i32, ptr %p, align 4, !dbg !95
  ret i32 %b, !dbg !73
}

!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!2, !3}

!0 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !1, producer: "hand", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, splitDebugInlining: false, nameTableKind: None)
!1 = !DIFile(filename: "hand.cpp", directory: "/hod/tests/dies_inline")
!2 = !{i32 7, !"Dwarf Version", i32 5}
!3 = !{i32 2, !"Debug Info Version", i32 3}
!4 = !{}
!6 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!7 = !DIFile(filename: "inc/callee.h", directory: "/hod/tests/dies_inline")
!8 = !DISubroutineType(types: !9)
!9 = !{!6, !6}

!10 = distinct !DISubprogram(name: "split", linkageName: "_Z5spliti", scope: !1, file: !1, line: 2, type: !8, scopeLine: 2, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !4)
!14 = !DILocalVariable(name: "x", arg: 1, scope: !10, file: !1, line: 2, type: !6)
!15 = !DILocation(line: 2, column: 15, scope: !10)
!16 = !DILocation(line: 3, column: 3, scope: !10)
!17 = !DILocation(line: 4, column: 3, scope: !10)
!18 = !DILocation(line: 3, column: 20, scope: !10)

!12 = distinct !DISubprogram(name: "callee", linkageName: "_Z6calleei", scope: !7, file: !7, line: 1, type: !8, scopeLine: 1, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !4)
!31 = !DILocalVariable(name: "v", arg: 1, scope: !12, file: !7, line: 1, type: !6)
!32 = !DILocalVariable(name: "t", scope: !35, file: !7, line: 3, type: !6)
!33 = !DILocalVariable(name: "u", scope: !12, file: !7, line: 2, type: !6)
!35 = distinct !DILexicalBlock(scope: !12, file: !7, line: 3, column: 3)
!40 = !DILocation(line: 2, column: 5, scope: !12, inlinedAt: !18)
!41 = !DILocation(line: 3, column: 7, scope: !35, inlinedAt: !18)
!42 = !DILocation(line: 4, column: 10, scope: !12, inlinedAt: !18)

!50 = distinct !DISubprogram(name: "twice", linkageName: "_Z5twicei", scope: !1, file: !1, line: 6, type: !8, scopeLine: 6, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !4)
!51 = !DILocalVariable(name: "x", arg: 1, scope: !50, file: !1, line: 6, type: !6)
!52 = !DILocation(line: 6, column: 15, scope: !50)
!53 = !DILocation(line: 8, column: 3, scope: !50)
!54 = !DILocation(line: 7, column: 10, scope: !50)
!55 = !DILocation(line: 7, column: 0, scope: !50)
!60 = !DILocation(line: 2, column: 5, scope: !12, inlinedAt: !54)
!61 = !DILocation(line: 4, column: 10, scope: !12, inlinedAt: !54)
!62 = !DILocation(line: 2, column: 5, scope: !12, inlinedAt: !55)
!63 = !DILocation(line: 4, column: 10, scope: !12, inlinedAt: !55)

!70 = distinct !DISubprogram(name: "nested", linkageName: "_Z6nestedi", scope: !1, file: !1, line: 10, type: !8, scopeLine: 10, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !4)
!71 = !DILocalVariable(name: "x", arg: 1, scope: !70, file: !1, line: 10, type: !6)
!72 = !DILocation(line: 10, column: 16, scope: !70)
!73 = !DILocation(line: 12, column: 3, scope: !70)
!74 = !DILocation(line: 11, column: 10, scope: !70)
!80 = distinct !DISubprogram(name: "outer", linkageName: "_Z5outeri", scope: !1, file: !1, line: 20, type: !8, scopeLine: 20, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !4)
!81 = !DILocalVariable(name: "p", arg: 1, scope: !80, file: !1, line: 20, type: !6)
!82 = distinct !DILexicalBlock(scope: !80, file: !1, line: 22, column: 3)
!83 = !DILocation(line: 21, column: 12, scope: !80, inlinedAt: !74)
!84 = !DILabel(scope: !82, name: "done", file: !1, line: 23)
!90 = !DILocation(line: 20, column: 15, scope: !80, inlinedAt: !74)
!92 = !DILocation(line: 2, column: 5, scope: !12, inlinedAt: !83)
!93 = !DILocation(line: 4, column: 10, scope: !12, inlinedAt: !83)
!94 = !DILocation(line: 22, column: 5, scope: !82, inlinedAt: !74)
!95 = !DILocation(line: 23, column: 1, scope: !82, inlinedAt: !74)
