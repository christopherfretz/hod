; `!llvm.gcov`, as `clang -coverage` writes it: one tuple per compile
; unit with the paths of the .gcno and .gcda files and the unit. It is
; the input of the GCOVProfiler IR pass, which has run (or not) before
; code generation; llc 19 and 22 write byte-identical objects with and
; without it (x86-64, AArch64, i686; plain and PIC; with and without
; debug information; spec/landed/harness/WP-353.md). hod drops it.
;@ covers: spec/ir.md §2.1 named metadata, §12 producer kit (llvm.gcov dropped)
;@ stdout: sum 49

@.fmt = private unnamed_addr constant [8 x i8] c"sum %d\0A\00"
@counter = global i32 7

declare i32 @printf(ptr, ...)

define i32 @scale(i32 %a) !dbg !14 {
entry:
  %c = load i32, ptr @counter, !dbg !20
  %r = mul i32 %c, %a, !dbg !20
  ret i32 %r, !dbg !20
}

define i32 @main() {
entry:
  %s = call i32 @scale(i32 7)
  %p = call i32 (ptr, ...) @printf(ptr @.fmt, i32 %s)
  ret i32 0
}

!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!3, !4}
!llvm.gcov = !{!8}
!llvm.ident = !{!9}

!0 = distinct !DICompileUnit(language: DW_LANG_C11, file: !1, producer: "clang", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, splitDebugInlining: false, nameTableKind: None)
!1 = !DIFile(filename: "gcov.c", directory: "/x")
!3 = !{i32 2, !"Debug Info Version", i32 3}
!4 = !{i32 7, !"Dwarf Version", i32 5}
!8 = !{!"/x/gcov.gcno", !"/x/gcov.gcda", !0}
!9 = !{!"clang"}
!14 = distinct !DISubprogram(name: "scale", scope: !1, file: !1, line: 1, type: !15, scopeLine: 1, spFlags: DISPFlagDefinition, unit: !0)
!15 = !DISubroutineType(types: !16)
!16 = !{!17, !17}
!17 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!20 = !DILocation(line: 2, column: 3, scope: !14)
