source_filename = "main.cpp"
target datalayout = "e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128-Fn32"
target triple = "aarch64-unknown-linux-gnu"

$_Z5twiceIiET_S0_ = comdat any

$_Z5twiceIlET_S0_ = comdat any

@g = dso_local global i32 3, align 4, !dbg !0

; Function Attrs: mustprogress noinline optnone uwtable
define dso_local noundef i32 @_Z4loopi(i32 noundef %0) #0 !dbg !15 {
  %2 = alloca i32, align 4
  %3 = alloca i32, align 4
  %4 = alloca i32, align 4
  store i32 %0, ptr %2, align 4
    #dbg_declare(ptr %2, !19, !DIExpression(), !20)
    #dbg_declare(ptr %3, !21, !DIExpression(), !22)
  store i32 0, ptr %3, align 4, !dbg !22
    #dbg_declare(ptr %4, !23, !DIExpression(), !25)
  store i32 0, ptr %4, align 4, !dbg !25
  br label %5, !dbg !26

5:                                                ; preds = %14, %1
  %6 = load i32, ptr %4, align 4, !dbg !27
  %7 = load i32, ptr %2, align 4, !dbg !29
  %8 = icmp slt i32 %6, %7, !dbg !30
  br i1 %8, label %9, label %17, !dbg !31

9:                                                ; preds = %5
  %10 = load i32, ptr %4, align 4, !dbg !32
  %11 = call noundef i32 @_Z5twiceIiET_S0_(i32 noundef %10), !dbg !34
  %12 = load i32, ptr %3, align 4, !dbg !35
  %13 = add nsw i32 %12, %11, !dbg !35
  store i32 %13, ptr %3, align 4, !dbg !35
  br label %14, !dbg !36

14:                                               ; preds = %9
  %15 = load i32, ptr %4, align 4, !dbg !37
  %16 = add nsw i32 %15, 1, !dbg !37
  store i32 %16, ptr %4, align 4, !dbg !37
  br label %5, !dbg !38, !llvm.loop !39

17:                                               ; preds = %5
  %18 = load i32, ptr %3, align 4, !dbg !42
  ret i32 %18, !dbg !43
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i32 @_Z5twiceIiET_S0_(i32 noundef %0) #1 comdat !dbg !44 {
  %2 = alloca i32, align 4
  %3 = alloca i32, align 4
  store i32 %0, ptr %2, align 4
    #dbg_declare(ptr %2, !48, !DIExpression(), !49)
    #dbg_declare(ptr %3, !50, !DIExpression(), !51)
  %4 = load i32, ptr %2, align 4, !dbg !52
  %5 = load i32, ptr %2, align 4, !dbg !53
  %6 = add nsw i32 %4, %5, !dbg !54
  store i32 %6, ptr %3, align 4, !dbg !51
  %7 = load i32, ptr %3, align 4, !dbg !55
  ret i32 %7, !dbg !56
}

; Function Attrs: mustprogress noinline norecurse optnone uwtable
define dso_local noundef i32 @main(i32 noundef %0, ptr noundef %1) #2 !dbg !57 {
  %3 = alloca i32, align 4
  %4 = alloca i32, align 4
  %5 = alloca ptr, align 8
  %6 = alloca i32, align 4
  %7 = alloca i64, align 8
  store i32 0, ptr %3, align 4
  store i32 %0, ptr %4, align 4
    #dbg_declare(ptr %4, !63, !DIExpression(), !64)
  store ptr %1, ptr %5, align 8
    #dbg_declare(ptr %5, !65, !DIExpression(), !66)
    #dbg_declare(ptr %6, !67, !DIExpression(), !68)
  %8 = load i32, ptr %4, align 4, !dbg !69
  %9 = add nsw i32 %8, 3, !dbg !70
  %10 = call noundef i32 @_Z4loopi(i32 noundef %9), !dbg !71
  store i32 %10, ptr %6, align 4, !dbg !68
  %11 = load i32, ptr %6, align 4, !dbg !72
  %12 = load i32, ptr @g, align 4, !dbg !73
  %13 = call noundef i32 @_ZL6helperii(i32 noundef %11, i32 noundef %12), !dbg !74
  %14 = load i32, ptr %6, align 4, !dbg !75
  %15 = add nsw i32 %14, %13, !dbg !75
  store i32 %15, ptr %6, align 4, !dbg !75
    #dbg_declare(ptr %7, !76, !DIExpression(), !78)
  %16 = load i32, ptr %6, align 4, !dbg !79
  %17 = sext i32 %16 to i64, !dbg !79
  %18 = call noundef i64 @_Z5twiceIlET_S0_(i64 noundef %17), !dbg !80
  store i64 %18, ptr %7, align 8, !dbg !78
  %19 = load i32, ptr %6, align 4, !dbg !81
  %20 = call noundef i32 @_Z5otheri(i32 noundef %19), !dbg !82
  %21 = load i32, ptr %6, align 4, !dbg !83
  %22 = add nsw i32 %21, %20, !dbg !83
  store i32 %22, ptr %6, align 4, !dbg !83
  %23 = load i64, ptr %7, align 8, !dbg !84
  %24 = load i32, ptr %6, align 4, !dbg !85
  %25 = sext i32 %24 to i64, !dbg !85
  %26 = add nsw i64 %23, %25, !dbg !86
  %27 = and i64 %26, 1, !dbg !87
  %28 = trunc i64 %27 to i32, !dbg !88
  ret i32 %28, !dbg !89
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define internal noundef i32 @_ZL6helperii(i32 noundef %0, i32 noundef %1) #1 !dbg !90 {
  %3 = alloca i32, align 4
  %4 = alloca i32, align 4
  %5 = alloca i32, align 4
  store i32 %0, ptr %4, align 4
    #dbg_declare(ptr %4, !93, !DIExpression(), !94)
  store i32 %1, ptr %5, align 4
    #dbg_declare(ptr %5, !95, !DIExpression(), !96)
  %6 = load i32, ptr %4, align 4, !dbg !97
  %7 = load i32, ptr %5, align 4, !dbg !99
  %8 = icmp sgt i32 %6, %7, !dbg !100
  br i1 %8, label %9, label %13, !dbg !101

9:                                                ; preds = %2
  %10 = load i32, ptr %4, align 4, !dbg !102
  %11 = load i32, ptr %5, align 4, !dbg !103
  %12 = sub nsw i32 %10, %11, !dbg !104
  store i32 %12, ptr %3, align 4, !dbg !105
  br label %17, !dbg !105

13:                                               ; preds = %2
  %14 = load i32, ptr %5, align 4, !dbg !106
  %15 = load i32, ptr %4, align 4, !dbg !107
  %16 = sub nsw i32 %14, %15, !dbg !108
  store i32 %16, ptr %3, align 4, !dbg !109
  br label %17, !dbg !109

17:                                               ; preds = %13, %9
  %18 = load i32, ptr %3, align 4, !dbg !110
  ret i32 %18, !dbg !110
}

; Function Attrs: mustprogress noinline nounwind optnone uwtable
define linkonce_odr dso_local noundef i64 @_Z5twiceIlET_S0_(i64 noundef %0) #1 comdat !dbg !111 {
  %2 = alloca i64, align 8
  %3 = alloca i64, align 8
  store i64 %0, ptr %2, align 8
    #dbg_declare(ptr %2, !116, !DIExpression(), !117)
    #dbg_declare(ptr %3, !118, !DIExpression(), !119)
  %4 = load i64, ptr %2, align 8, !dbg !120
  %5 = load i64, ptr %2, align 8, !dbg !121
  %6 = add nsw i64 %4, %5, !dbg !122
  store i64 %6, ptr %3, align 8, !dbg !119
  %7 = load i64, ptr %3, align 8, !dbg !123
  ret i64 %7, !dbg !124
}

declare noundef i32 @_Z5otheri(i32 noundef) #3

attributes #0 = { mustprogress noinline optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #1 = { mustprogress noinline nounwind optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #2 = { mustprogress noinline norecurse optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }
attributes #3 = { "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }

!llvm.dbg.cu = !{!2}
!llvm.module.flags = !{!7, !8, !9, !10, !11, !12, !13}

!0 = !DIGlobalVariableExpression(var: !1, expr: !DIExpression())
!1 = distinct !DIGlobalVariable(name: "g", scope: !2, file: !3, line: 5, type: !5, isLocal: false, isDefinition: true)
!2 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !3, producer: "Debian clang version 19.1.7 (3+b1)", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, retainedTypes: !4, globals: !6, splitDebugInlining: false, nameTableKind: None)
!3 = !DIFile(filename: "main.cpp", directory: "/hod/tests/lines", checksumkind: CSK_MD5, checksum: "facf5018fc762a6df4a2b160f0aaa3e7")
!4 = !{!5}
!5 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!6 = !{!0}
!7 = !{i32 7, !"Dwarf Version", i32 5}
!8 = !{i32 2, !"Debug Info Version", i32 3}
!9 = !{i32 1, !"wchar_size", i32 4}
!10 = !{i32 8, !"PIC Level", i32 2}
!11 = !{i32 7, !"PIE Level", i32 2}
!12 = !{i32 7, !"uwtable", i32 2}
!13 = !{i32 7, !"frame-pointer", i32 1}
!15 = distinct !DISubprogram(name: "loop", linkageName: "_Z4loopi", scope: !3, file: !3, line: 6, type: !16, scopeLine: 6, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !2, retainedNodes: !18)
!16 = !DISubroutineType(types: !17)
!17 = !{!5, !5}
!18 = !{}
!19 = !DILocalVariable(name: "n", arg: 1, scope: !15, file: !3, line: 6, type: !5)
!20 = !DILocation(line: 6, column: 14, scope: !15)
!21 = !DILocalVariable(name: "s", scope: !15, file: !3, line: 7, type: !5)
!22 = !DILocation(line: 7, column: 7, scope: !15)
!23 = !DILocalVariable(name: "i", scope: !24, file: !3, line: 8, type: !5)
!24 = distinct !DILexicalBlock(scope: !15, file: !3, line: 8, column: 3)
!25 = !DILocation(line: 8, column: 12, scope: !24)
!26 = !DILocation(line: 8, column: 8, scope: !24)
!27 = !DILocation(line: 8, column: 19, scope: !28)
!28 = distinct !DILexicalBlock(scope: !24, file: !3, line: 8, column: 3)
!29 = !DILocation(line: 8, column: 23, scope: !28)
!30 = !DILocation(line: 8, column: 21, scope: !28)
!31 = !DILocation(line: 8, column: 3, scope: !24)
!32 = !DILocation(line: 9, column: 16, scope: !33)
!33 = distinct !DILexicalBlock(scope: !28, file: !3, line: 8, column: 31)
!34 = !DILocation(line: 9, column: 10, scope: !33)
!35 = !DILocation(line: 9, column: 7, scope: !33)
!36 = !DILocation(line: 10, column: 3, scope: !33)
!37 = !DILocation(line: 8, column: 26, scope: !28)
!38 = !DILocation(line: 8, column: 3, scope: !28)
!39 = distinct !{!39, !31, !40, !41}
!40 = !DILocation(line: 10, column: 3, scope: !24)
!41 = !{!"llvm.loop.mustprogress"}
!42 = !DILocation(line: 11, column: 10, scope: !15)
!43 = !DILocation(line: 11, column: 3, scope: !15)
!44 = distinct !DISubprogram(name: "twice<int>", linkageName: "_Z5twiceIiET_S0_", scope: !45, file: !45, line: 4, type: !16, scopeLine: 4, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !2, templateParams: !46, retainedNodes: !18)
!45 = !DIFile(filename: "./inc/util.h", directory: "/hod/tests/lines", checksumkind: CSK_MD5, checksum: "a073b464b4d00d9a60c2bc735f7c3e93")
!46 = !{!47}
!47 = !DITemplateTypeParameter(name: "T", type: !5)
!48 = !DILocalVariable(name: "x", arg: 1, scope: !44, file: !45, line: 4, type: !5)
!49 = !DILocation(line: 4, column: 11, scope: !44)
!50 = !DILocalVariable(name: "y", scope: !44, file: !45, line: 5, type: !5)
!51 = !DILocation(line: 5, column: 5, scope: !44)
!52 = !DILocation(line: 5, column: 9, scope: !44)
!53 = !DILocation(line: 5, column: 13, scope: !44)
!54 = !DILocation(line: 5, column: 11, scope: !44)
!55 = !DILocation(line: 6, column: 10, scope: !44)
!56 = !DILocation(line: 6, column: 3, scope: !44)
!57 = distinct !DISubprogram(name: "main", scope: !3, file: !3, line: 14, type: !58, scopeLine: 14, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !2, retainedNodes: !18)
!58 = !DISubroutineType(types: !59)
!59 = !{!5, !5, !60}
!60 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !61, size: 64)
!61 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !62, size: 64)
!62 = !DIBasicType(name: "char", size: 8, encoding: DW_ATE_unsigned_char)
!63 = !DILocalVariable(name: "argc", arg: 1, scope: !57, file: !3, line: 14, type: !5)
!64 = !DILocation(line: 14, column: 14, scope: !57)
!65 = !DILocalVariable(name: "argv", arg: 2, scope: !57, file: !3, line: 14, type: !60)
!66 = !DILocation(line: 14, column: 27, scope: !57)
!67 = !DILocalVariable(name: "r", scope: !57, file: !3, line: 15, type: !5)
!68 = !DILocation(line: 15, column: 7, scope: !57)
!69 = !DILocation(line: 15, column: 16, scope: !57)
!70 = !DILocation(line: 15, column: 21, scope: !57)
!71 = !DILocation(line: 15, column: 11, scope: !57)
!72 = !DILocation(line: 16, column: 15, scope: !57)
!73 = !DILocation(line: 16, column: 18, scope: !57)
!74 = !DILocation(line: 16, column: 8, scope: !57)
!75 = !DILocation(line: 16, column: 5, scope: !57)
!76 = !DILocalVariable(name: "t", scope: !57, file: !3, line: 17, type: !77)
!77 = !DIBasicType(name: "long", size: 64, encoding: DW_ATE_signed)
!78 = !DILocation(line: 17, column: 8, scope: !57)
!79 = !DILocation(line: 17, column: 24, scope: !57)
!80 = !DILocation(line: 17, column: 12, scope: !57)
!81 = !DILocation(line: 18, column: 14, scope: !57)
!82 = !DILocation(line: 18, column: 8, scope: !57)
!83 = !DILocation(line: 18, column: 5, scope: !57)
!84 = !DILocation(line: 19, column: 17, scope: !57)
!85 = !DILocation(line: 19, column: 21, scope: !57)
!86 = !DILocation(line: 19, column: 19, scope: !57)
!87 = !DILocation(line: 19, column: 24, scope: !57)
!88 = !DILocation(line: 19, column: 15, scope: !57)
!89 = !DILocation(line: 19, column: 3, scope: !57)
!90 = distinct !DISubprogram(name: "helper", linkageName: "_ZL6helperii", scope: !45, file: !45, line: 8, type: !91, scopeLine: 8, flags: DIFlagPrototyped, spFlags: DISPFlagLocalToUnit | DISPFlagDefinition, unit: !2, retainedNodes: !18)
!91 = !DISubroutineType(types: !92)
!92 = !{!5, !5, !5}
!93 = !DILocalVariable(name: "a", arg: 1, scope: !90, file: !45, line: 8, type: !5)
!94 = !DILocation(line: 8, column: 30, scope: !90)
!95 = !DILocalVariable(name: "b", arg: 2, scope: !90, file: !45, line: 8, type: !5)
!96 = !DILocation(line: 8, column: 37, scope: !90)
!97 = !DILocation(line: 9, column: 7, scope: !98)
!98 = distinct !DILexicalBlock(scope: !90, file: !45, line: 9, column: 7)
!99 = !DILocation(line: 9, column: 11, scope: !98)
!100 = !DILocation(line: 9, column: 9, scope: !98)
!101 = !DILocation(line: 9, column: 7, scope: !90)
!102 = !DILocation(line: 10, column: 12, scope: !98)
!103 = !DILocation(line: 10, column: 16, scope: !98)
!104 = !DILocation(line: 10, column: 14, scope: !98)
!105 = !DILocation(line: 10, column: 5, scope: !98)
!106 = !DILocation(line: 11, column: 10, scope: !90)
!107 = !DILocation(line: 11, column: 14, scope: !90)
!108 = !DILocation(line: 11, column: 12, scope: !90)
!109 = !DILocation(line: 11, column: 3, scope: !90)
!110 = !DILocation(line: 12, column: 1, scope: !90)
!111 = distinct !DISubprogram(name: "twice<long>", linkageName: "_Z5twiceIlET_S0_", scope: !45, file: !45, line: 4, type: !112, scopeLine: 4, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !2, templateParams: !114, retainedNodes: !18)
!112 = !DISubroutineType(types: !113)
!113 = !{!77, !77}
!114 = !{!115}
!115 = !DITemplateTypeParameter(name: "T", type: !77)
!116 = !DILocalVariable(name: "x", arg: 1, scope: !111, file: !45, line: 4, type: !77)
!117 = !DILocation(line: 4, column: 11, scope: !111)
!118 = !DILocalVariable(name: "y", scope: !111, file: !45, line: 5, type: !77)
!119 = !DILocation(line: 5, column: 5, scope: !111)
!120 = !DILocation(line: 5, column: 9, scope: !111)
!121 = !DILocation(line: 5, column: 13, scope: !111)
!122 = !DILocation(line: 5, column: 11, scope: !111)
!123 = !DILocation(line: 6, column: 10, scope: !111)
!124 = !DILocation(line: 6, column: 3, scope: !111)
