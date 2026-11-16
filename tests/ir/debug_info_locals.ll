; A module with debug information (a compile unit, subprograms, located
; instructions, `#dbg_declare` records of a parameter and a local, an
; `i64` and a `double` local) builds and runs as the oracle's on every
; target, i686 included: the debug information changes no code. `scale`
; returns 3 * 7 + 2 = 23, `main` prints it with its `i64` and `double`.
;@ covers: spec/dwarf.md §6.4 subprograms and variables (records of a declared parameter and local)
;@ stdout: 23 40000000000 2.5

@.fmt = private unnamed_addr constant [12 x i8] c"%d %lld %g\0A\00"

declare i32 @printf(ptr, ...)

define i32 @scale(i32 %n) !dbg !10 {
entry:
  %n.addr = alloca i32, align 4
  %r = alloca i32, align 4
  store i32 %n, ptr %n.addr, align 4
    #dbg_declare(ptr %n.addr, !15, !DIExpression(), !17)
  %v = load i32, ptr %n.addr, align 4, !dbg !18
  %m = mul i32 %v, 3, !dbg !18
  %a = add i32 %m, 2, !dbg !18
  store i32 %a, ptr %r, align 4, !dbg !18
    #dbg_declare(ptr %r, !16, !DIExpression(), !17)
  %out = load i32, ptr %r, align 4, !dbg !19
  ret i32 %out, !dbg !19
}

define i32 @main() !dbg !20 {
entry:
  %big = alloca i64, align 8
  %half = alloca double, align 8
    #dbg_declare(ptr %big, !23, !DIExpression(), !26)
    #dbg_declare(ptr %half, !24, !DIExpression(), !26)
  store i64 40000000000, ptr %big, align 8, !dbg !26
  store double 2.500000e+00, ptr %half, align 8, !dbg !26
  %s = call i32 @scale(i32 7), !dbg !27
  %b = load i64, ptr %big, align 8, !dbg !27
  %h = load double, ptr %half, align 8, !dbg !27
  %p = call i32 (ptr, ...) @printf(ptr @.fmt, i32 %s, i64 %b, double %h), !dbg !27
  ret i32 0, !dbg !28
}

!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!2, !3}

!0 = distinct !DICompileUnit(language: DW_LANG_C11, file: !1, producer: "hand-written", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug)
!1 = !DIFile(filename: "debug_info_locals.c", directory: "/seed")
!2 = !{i32 7, !"Dwarf Version", i32 5}
!3 = !{i32 2, !"Debug Info Version", i32 3}
!4 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!5 = !DIBasicType(name: "long long", size: 64, encoding: DW_ATE_signed)
!6 = !DIBasicType(name: "double", size: 64, encoding: DW_ATE_float)
!10 = distinct !DISubprogram(name: "scale", scope: !1, file: !1, line: 2, type: !11, scopeLine: 2, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !13)
!11 = !DISubroutineType(types: !12)
!12 = !{!4, !4}
!13 = !{}
!15 = !DILocalVariable(name: "n", arg: 1, scope: !10, file: !1, line: 2, type: !4)
!16 = !DILocalVariable(name: "r", scope: !10, file: !1, line: 3, type: !4)
!17 = !DILocation(line: 2, column: 15, scope: !10)
!18 = !DILocation(line: 3, column: 11, scope: !10)
!19 = !DILocation(line: 4, column: 3, scope: !10)
!20 = distinct !DISubprogram(name: "main", scope: !1, file: !1, line: 6, type: !21, scopeLine: 6, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !13)
!21 = !DISubroutineType(types: !22)
!22 = !{!4}
!23 = !DILocalVariable(name: "big", scope: !20, file: !1, line: 7, type: !5)
!24 = !DILocalVariable(name: "half", scope: !20, file: !1, line: 8, type: !6)
!26 = !DILocation(line: 7, column: 13, scope: !20)
!27 = !DILocation(line: 9, column: 3, scope: !20)
!28 = !DILocation(line: 10, column: 3, scope: !20)
