source_filename = "second.cpp"
target datalayout = "e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128-Fn32"
target triple = "aarch64-unknown-linux-gnu"

$_Z5twiceIiET_S0_ = comdat any

; Function Attrs: mustprogress noinline optnone uwtable
define dso_local noundef i32 @_Z5otheri(i32 noundef %0) #0 !dbg !10 {
  %2 = alloca i32, align 4
  %3 = alloca i32, align 4
  store i32 %0, ptr %2, align 4
    #dbg_declare(ptr %2, !15, !DIExpression(), !16)
    #dbg_declare(ptr %3, !17, !DIExpression(), !18)
  %4 = load i32, ptr %2, align 4, !dbg !19
  %5 = call noundef i32 @_Z5twiceIiET_S0_(i32 noundef %4), !dbg !20
  store i32 %5, ptr %3, align 4, !dbg !18
  %6 = load i32, ptr %3, align 4, !dbg !21
  %7 = load i32, ptr %2, align 4, !dbg !22
  %8 = sub nsw i32 %6, %7, !dbg !23
  ret i32 %8, !dbg !24
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i32 @_Z5twiceIiET_S0_(i32 noundef %0) #1 comdat !dbg !25 {
  %2 = alloca i32, align 4
  %3 = alloca i32, align 4
  store i32 %0, ptr %2, align 4
    #dbg_declare(ptr %2, !29, !DIExpression(), !30)
    #dbg_declare(ptr %3, !31, !DIExpression(), !32)
  %4 = load i32, ptr %2, align 4, !dbg !33
  %5 = load i32, ptr %2, align 4, !dbg !34
  %6 = add nsw i32 %4, %5, !dbg !35
  store i32 %6, ptr %3, align 4, !dbg !32
  %7 = load i32, ptr %3, align 4, !dbg !36
  ret i32 %7, !dbg !37
}

attributes #0 = { mustprogress noinline optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #1 = { mustprogress noinline nounwind optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }

!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!2, !3, !4, !5, !6, !7, !8}

!0 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !1, producer: "Debian clang version 19.1.7 (3+b1)", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, splitDebugInlining: false, nameTableKind: None)
!1 = !DIFile(filename: "second.cpp", directory: "/hod/tests/lines", checksumkind: CSK_MD5, checksum: "d5d0dc46ef68da308e58dea07d035565")
!2 = !{i32 7, !"Dwarf Version", i32 5}
!3 = !{i32 2, !"Debug Info Version", i32 3}
!4 = !{i32 1, !"wchar_size", i32 4}
!5 = !{i32 8, !"PIC Level", i32 2}
!6 = !{i32 7, !"PIE Level", i32 2}
!7 = !{i32 7, !"uwtable", i32 2}
!8 = !{i32 7, !"frame-pointer", i32 1}
!10 = distinct !DISubprogram(name: "other", linkageName: "_Z5otheri", scope: !1, file: !1, line: 3, type: !11, scopeLine: 3, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !14)
!11 = !DISubroutineType(types: !12)
!12 = !{!13, !13}
!13 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!14 = !{}
!15 = !DILocalVariable(name: "v", arg: 1, scope: !10, file: !1, line: 3, type: !13)
!16 = !DILocation(line: 3, column: 15, scope: !10)
!17 = !DILocalVariable(name: "w", scope: !10, file: !1, line: 4, type: !13)
!18 = !DILocation(line: 4, column: 7, scope: !10)
!19 = !DILocation(line: 4, column: 17, scope: !10)
!20 = !DILocation(line: 4, column: 11, scope: !10)
!21 = !DILocation(line: 5, column: 10, scope: !10)
!22 = !DILocation(line: 5, column: 14, scope: !10)
!23 = !DILocation(line: 5, column: 12, scope: !10)
!24 = !DILocation(line: 5, column: 3, scope: !10)
!25 = distinct !DISubprogram(name: "twice<int>", linkageName: "_Z5twiceIiET_S0_", scope: !26, file: !26, line: 4, type: !11, scopeLine: 4, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, templateParams: !27, retainedNodes: !14)
!26 = !DIFile(filename: "./inc/util.h", directory: "/hod/tests/lines", checksumkind: CSK_MD5, checksum: "a073b464b4d00d9a60c2bc735f7c3e93")
!27 = !{!28}
!28 = !DITemplateTypeParameter(name: "T", type: !13)
!29 = !DILocalVariable(name: "x", arg: 1, scope: !25, file: !26, line: 4, type: !13)
!30 = !DILocation(line: 4, column: 11, scope: !25)
!31 = !DILocalVariable(name: "y", scope: !25, file: !26, line: 5, type: !13)
!32 = !DILocation(line: 5, column: 5, scope: !25)
!33 = !DILocation(line: 5, column: 9, scope: !25)
!34 = !DILocation(line: 5, column: 13, scope: !25)
!35 = !DILocation(line: 5, column: 11, scope: !25)
!36 = !DILocation(line: 6, column: 10, scope: !25)
!37 = !DILocation(line: 6, column: 3, scope: !25)
