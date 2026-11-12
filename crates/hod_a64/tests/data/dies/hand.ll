; Function DIEs the C and C++ sources do not reach (spec/dwarf.md 6.4):
; - @split: a lexical block whose code is interrupted by its parent's,
;   so the block has two ranges (DW_AT_ranges, a range list);
; - @declared_only: a #dbg_declare of a variable whose scope has no code
;   is dropped, as llc drops it;
; - @with_label: a label in a nested block, and a parameter that is never
;   used but is named by a #dbg_declare;
; - @no_vars: a subprogram without variables, and an unused block.
; The test prepends the target lines.

@g = dso_local global i32 0, align 4

define dso_local i32 @split(i32 %v) !dbg !10 {
entry:
  %v.addr = alloca i32, align 4
  %r = alloca i32, align 4
  %t = alloca i32, align 4
  store i32 %v, ptr %v.addr, align 4
    #dbg_declare(ptr %v.addr, !14, !DIExpression(), !15)
    #dbg_declare(ptr %r, !16, !DIExpression(), !17)
  store i32 0, ptr %r, align 4, !dbg !17
    #dbg_declare(ptr %t, !18, !DIExpression(), !20)
  %0 = load i32, ptr %v.addr, align 4, !dbg !20
  store i32 %0, ptr %t, align 4, !dbg !20
  %1 = load i32, ptr %r, align 4, !dbg !21
  store volatile i32 %1, ptr @g, align 4, !dbg !21
  %2 = load i32, ptr %t, align 4, !dbg !22
  %3 = add i32 %1, %2, !dbg !22
  store i32 %3, ptr %r, align 4, !dbg !22
  %4 = load i32, ptr %r, align 4, !dbg !23
  ret i32 %4, !dbg !23
}

define dso_local i32 @declared_only(i32 %v) !dbg !30 {
entry:
  %v.addr = alloca i32, align 4
  %gone = alloca i32, align 4
  store i32 %v, ptr %v.addr, align 4
    #dbg_declare(ptr %v.addr, !31, !DIExpression(), !32)
    #dbg_declare(ptr %gone, !33, !DIExpression(), !35)
  %0 = load i32, ptr %v.addr, align 4, !dbg !36
  ret i32 %0, !dbg !36
}

define dso_local i32 @with_label(i32 %v, i32 %unused) !dbg !40 {
entry:
  %v.addr = alloca i32, align 4
  %unused.addr = alloca i32, align 4
  store i32 %v, ptr %v.addr, align 4
    #dbg_declare(ptr %v.addr, !41, !DIExpression(), !42)
  store i32 %unused, ptr %unused.addr, align 4
    #dbg_declare(ptr %unused.addr, !43, !DIExpression(), !42)
  br label %again, !dbg !44

again:
    #dbg_label(!45, !46)
  %0 = load i32, ptr %v.addr, align 4, !dbg !46
  %1 = add i32 %0, 1, !dbg !46
  store i32 %1, ptr %v.addr, align 4, !dbg !46
  %2 = icmp slt i32 %1, 3, !dbg !46
  br i1 %2, label %again, label %done, !dbg !46

done:
  ret i32 %1, !dbg !47
}

define dso_local void @no_vars() !dbg !50 {
entry:
  store volatile i32 1, ptr @g, align 4, !dbg !51
  store volatile i32 2, ptr @g, align 4, !dbg !53
  ret void, !dbg !54
}

!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!2, !3}

!0 = distinct !DICompileUnit(language: DW_LANG_C11, file: !1, producer: "hand", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, splitDebugInlining: false, nameTableKind: None)
!1 = !DIFile(filename: "hand.c", directory: "/hod/tests/dies")
!2 = !{i32 7, !"Dwarf Version", i32 5}
!3 = !{i32 2, !"Debug Info Version", i32 3}
!4 = !DISubroutineType(types: !5)
!5 = !{!6, !6}
!6 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!7 = !{}
!8 = !DISubroutineType(types: !9)
!9 = !{!6, !6, !6}

!10 = distinct !DISubprogram(name: "split", scope: !1, file: !1, line: 1, type: !4, scopeLine: 1, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !7)
!14 = !DILocalVariable(name: "v", arg: 1, scope: !10, file: !1, line: 1, type: !6)
!15 = !DILocation(line: 1, column: 15, scope: !10)
!16 = !DILocalVariable(name: "r", scope: !10, file: !1, line: 2, type: !6)
!17 = !DILocation(line: 2, column: 7, scope: !10)
!18 = !DILocalVariable(name: "t", scope: !19, file: !1, line: 4, type: !6)
!19 = distinct !DILexicalBlock(scope: !10, file: !1, line: 3, column: 3)
!20 = !DILocation(line: 4, column: 9, scope: !19)
!21 = !DILocation(line: 5, column: 5, scope: !10)
!22 = !DILocation(line: 6, column: 7, scope: !19)
!23 = !DILocation(line: 8, column: 3, scope: !10)

!30 = distinct !DISubprogram(name: "declared_only", scope: !1, file: !1, line: 10, type: !4, scopeLine: 10, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !7)
!31 = !DILocalVariable(name: "v", arg: 1, scope: !30, file: !1, line: 10, type: !6)
!32 = !DILocation(line: 10, column: 23, scope: !30)
!33 = !DILocalVariable(name: "gone", scope: !34, file: !1, line: 12, type: !6)
!34 = distinct !DILexicalBlock(scope: !30, file: !1, line: 11, column: 3)
!35 = !DILocation(line: 12, column: 9, scope: !34)
!36 = !DILocation(line: 13, column: 3, scope: !30)

!40 = distinct !DISubprogram(name: "with_label", scope: !1, file: !1, line: 20, type: !8, scopeLine: 20, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !7)
!41 = !DILocalVariable(name: "v", arg: 1, scope: !40, file: !1, line: 20, type: !6)
!42 = !DILocation(line: 20, column: 20, scope: !40)
!43 = !DILocalVariable(name: "unused", arg: 2, scope: !40, file: !1, line: 20, type: !6)
!44 = !DILocation(line: 21, column: 3, scope: !40)
!45 = !DILabel(scope: !48, name: "again", file: !1, line: 22)
!46 = !DILocation(line: 23, column: 5, scope: !48)
!47 = !DILocation(line: 25, column: 3, scope: !40)
!48 = distinct !DILexicalBlock(scope: !40, file: !1, line: 21, column: 3)

!50 = distinct !DISubprogram(name: "no_vars", scope: !1, file: !1, line: 30, type: !55, scopeLine: 30, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !7)
!51 = !DILocation(line: 31, column: 3, scope: !50)
!52 = distinct !DILexicalBlock(scope: !50, file: !1, line: 32, column: 3)
!53 = !DILocation(line: 33, column: 5, scope: !52)
!54 = !DILocation(line: 34, column: 1, scope: !50)
!55 = !DISubroutineType(types: !56)
!56 = !{null}
