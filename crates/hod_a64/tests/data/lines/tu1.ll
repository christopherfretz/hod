; ModuleID = 'tu1.cpp'
source_filename = "tu1.cpp"
target datalayout = "e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128-Fn32"
target triple = "aarch64-unknown-linux-gnu"

$_Z6sharedi = comdat any

@tu1_ptr = dso_local global ptr @_Z6sharedi, align 8, !dbg !0

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i32 @_Z6sharedi(i32 noundef %0) #0 comdat !dbg !15 {
  %2 = alloca i32, align 4
  %3 = alloca i32, align 4
  store i32 %0, ptr %2, align 4
    #dbg_declare(ptr %2, !21, !DIExpression(), !22)
    #dbg_declare(ptr %3, !23, !DIExpression(), !24)
  %4 = load i32, ptr %2, align 4, !dbg !25
  %5 = mul nsw i32 %4, 3, !dbg !26
  store i32 %5, ptr %3, align 4, !dbg !24
  %6 = load i32, ptr %3, align 4, !dbg !27
  %7 = icmp sgt i32 %6, 20, !dbg !29
  br i1 %7, label %8, label %11, !dbg !30

8:                                                ; preds = %1
  %9 = load i32, ptr %3, align 4, !dbg !31
  %10 = sub nsw i32 %9, 20, !dbg !31
  store i32 %10, ptr %3, align 4, !dbg !31
  br label %11, !dbg !32

11:                                               ; preds = %8, %1
  %12 = load i32, ptr %3, align 4, !dbg !33
  ret i32 %12, !dbg !34
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define dso_local noundef i32 @_Z6tu1_fni(i32 noundef %0) #0 !dbg !35 {
  %2 = alloca i32, align 4
  store i32 %0, ptr %2, align 4
    #dbg_declare(ptr %2, !36, !DIExpression(), !37)
  %3 = load i32, ptr %2, align 4, !dbg !38
  %4 = add nsw i32 %3, 1, !dbg !39
  ret i32 %4, !dbg !40
}

attributes #0 = { mustprogress noinline nounwind optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }

!llvm.dbg.cu = !{!2}
!llvm.module.flags = !{!7, !8, !9, !10, !11, !12, !13}
!llvm.ident = !{!14}

!0 = !DIGlobalVariableExpression(var: !1, expr: !DIExpression())
!1 = distinct !DIGlobalVariable(name: "tu1_ptr", scope: !2, file: !3, line: 3, type: !5, isLocal: false, isDefinition: true)
!2 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !3, producer: "Debian clang version 19.1.7 (3+b1)", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, retainedTypes: !4, globals: !6, splitDebugInlining: false, nameTableKind: None)
!3 = !DIFile(filename: "tu1.cpp", directory: "/hod/tests/lines", checksumkind: CSK_MD5, checksum: "87d899f3ded0be088d4aa1950db7f06c")
!4 = !{!5}
!5 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: null, size: 64)
!6 = !{!0}
!7 = !{i32 7, !"Dwarf Version", i32 5}
!8 = !{i32 2, !"Debug Info Version", i32 3}
!9 = !{i32 1, !"wchar_size", i32 4}
!10 = !{i32 8, !"PIC Level", i32 2}
!11 = !{i32 7, !"PIE Level", i32 2}
!12 = !{i32 7, !"uwtable", i32 2}
!13 = !{i32 7, !"frame-pointer", i32 1}
!14 = !{!"Debian clang version 19.1.7 (3+b1)"}
!15 = distinct !DISubprogram(name: "shared", linkageName: "_Z6sharedi", scope: !16, file: !16, line: 1, type: !17, scopeLine: 1, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !2, retainedNodes: !20)
!16 = !DIFile(filename: "./shared.h", directory: "/hod/tests/lines", checksumkind: CSK_MD5, checksum: "2ad0b7f1cb7d490dcc65c37bbe52db66")
!17 = !DISubroutineType(types: !18)
!18 = !{!19, !19}
!19 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!20 = !{}
!21 = !DILocalVariable(name: "x", arg: 1, scope: !15, file: !16, line: 1, type: !19)
!22 = !DILocation(line: 1, column: 23, scope: !15)
!23 = !DILocalVariable(name: "r", scope: !15, file: !16, line: 2, type: !19)
!24 = !DILocation(line: 2, column: 7, scope: !15)
!25 = !DILocation(line: 2, column: 11, scope: !15)
!26 = !DILocation(line: 2, column: 13, scope: !15)
!27 = !DILocation(line: 3, column: 7, scope: !28)
!28 = distinct !DILexicalBlock(scope: !15, file: !16, line: 3, column: 7)
!29 = !DILocation(line: 3, column: 9, scope: !28)
!30 = !DILocation(line: 3, column: 7, scope: !15)
!31 = !DILocation(line: 4, column: 7, scope: !28)
!32 = !DILocation(line: 4, column: 5, scope: !28)
!33 = !DILocation(line: 5, column: 10, scope: !15)
!34 = !DILocation(line: 5, column: 3, scope: !15)
!35 = distinct !DISubprogram(name: "tu1_fn", linkageName: "_Z6tu1_fni", scope: !3, file: !3, line: 5, type: !17, scopeLine: 5, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !2, retainedNodes: !20)
!36 = !DILocalVariable(name: "y", arg: 1, scope: !35, file: !3, line: 5, type: !19)
!37 = !DILocation(line: 5, column: 16, scope: !35)
!38 = !DILocation(line: 6, column: 10, scope: !35)
!39 = !DILocation(line: 6, column: 12, scope: !35)
!40 = !DILocation(line: 6, column: 3, scope: !35)
