; ModuleID = 'prog.c'
source_filename = "prog.c"
target datalayout = "e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128-Fn32"
target triple = "aarch64-unknown-linux-gnu"

@counter = dso_local global i32 0, align 4, !dbg !0

; Function Attrs: noinline nounwind optnone uwtable
define dso_local i32 @sum_to(i32 noundef %0) #0 !dbg !14 {
  %2 = alloca i32, align 4
  %3 = alloca i32, align 4
  %4 = alloca i32, align 4
  store i32 %0, ptr %2, align 4
    #dbg_declare(ptr %2, !18, !DIExpression(), !19)
    #dbg_declare(ptr %3, !20, !DIExpression(), !21)
  store i32 0, ptr %3, align 4, !dbg !21
    #dbg_declare(ptr %4, !22, !DIExpression(), !24)
  store i32 1, ptr %4, align 4, !dbg !24
  br label %5, !dbg !25

5:                                                ; preds = %13, %1
  %6 = load i32, ptr %4, align 4, !dbg !26
  %7 = load i32, ptr %2, align 4, !dbg !28
  %8 = icmp sle i32 %6, %7, !dbg !29
  br i1 %8, label %9, label %16, !dbg !30

9:                                                ; preds = %5
  %10 = load i32, ptr %4, align 4, !dbg !31
  %11 = load i32, ptr %3, align 4, !dbg !33
  %12 = add nsw i32 %11, %10, !dbg !33
  store i32 %12, ptr %3, align 4, !dbg !33
  br label %13, !dbg !34

13:                                               ; preds = %9
  %14 = load i32, ptr %4, align 4, !dbg !35
  %15 = add nsw i32 %14, 1, !dbg !35
  store i32 %15, ptr %4, align 4, !dbg !35
  br label %5, !dbg !36, !llvm.loop !37

16:                                               ; preds = %5
  %17 = load i32, ptr %3, align 4, !dbg !40
  ret i32 %17, !dbg !41
}

; Function Attrs: noinline nounwind optnone uwtable
define dso_local i32 @classify(i32 noundef %0) #0 !dbg !42 {
  %2 = alloca i32, align 4
  %3 = alloca i32, align 4
  store i32 %0, ptr %2, align 4
    #dbg_declare(ptr %2, !43, !DIExpression(), !44)
    #dbg_declare(ptr %3, !45, !DIExpression(), !46)
  %4 = load i32, ptr %2, align 4, !dbg !47
  switch i32 %4, label %7 [
    i32 0, label %5
    i32 1, label %6
  ], !dbg !48

5:                                                ; preds = %1
  store i32 10, ptr %3, align 4, !dbg !49
  br label %10, !dbg !51

6:                                                ; preds = %1
  store i32 20, ptr %3, align 4, !dbg !52
  br label %10, !dbg !53

7:                                                ; preds = %1
  %8 = load i32, ptr %2, align 4, !dbg !54
  %9 = mul nsw i32 %8, 3, !dbg !55
  store i32 %9, ptr %3, align 4, !dbg !56
  br label %10, !dbg !57

10:                                               ; preds = %7, %6, %5
  %11 = load i32, ptr %3, align 4, !dbg !58
  %12 = load i32, ptr @counter, align 4, !dbg !59
  %13 = add nsw i32 %12, %11, !dbg !59
  store i32 %13, ptr @counter, align 4, !dbg !59
  %14 = load i32, ptr %3, align 4, !dbg !60
  ret i32 %14, !dbg !61
}

; Function Attrs: noinline nounwind optnone uwtable
define dso_local i32 @scan(ptr noundef %0, i32 noundef %1) #0 !dbg !62 {
  %3 = alloca ptr, align 8
  %4 = alloca i32, align 4
  %5 = alloca i32, align 4
  %6 = alloca i32, align 4
  store ptr %0, ptr %3, align 8
    #dbg_declare(ptr %3, !67, !DIExpression(), !68)
  store i32 %1, ptr %4, align 4
    #dbg_declare(ptr %4, !69, !DIExpression(), !70)
    #dbg_declare(ptr %5, !71, !DIExpression(), !72)
  store i32 0, ptr %5, align 4, !dbg !72
    #dbg_declare(ptr %6, !73, !DIExpression(), !74)
  store i32 0, ptr %6, align 4, !dbg !74
  br label %7, !dbg !75

7:                                                ; preds = %28, %2
  %8 = load i32, ptr %6, align 4, !dbg !76
  %9 = load i32, ptr %4, align 4, !dbg !77
  %10 = icmp slt i32 %8, %9, !dbg !78
  br i1 %10, label %11, label %31, !dbg !75

11:                                               ; preds = %7
  %12 = load ptr, ptr %3, align 8, !dbg !79
  %13 = load i32, ptr %6, align 4, !dbg !82
  %14 = sext i32 %13 to i64, !dbg !79
  %15 = getelementptr inbounds i32, ptr %12, i64 %14, !dbg !79
  %16 = load i32, ptr %15, align 4, !dbg !79
  %17 = load i32, ptr %5, align 4, !dbg !83
  %18 = icmp sgt i32 %16, %17, !dbg !84
  br i1 %18, label %19, label %25, !dbg !85

19:                                               ; preds = %11
  %20 = load ptr, ptr %3, align 8, !dbg !86
  %21 = load i32, ptr %6, align 4, !dbg !87
  %22 = sext i32 %21 to i64, !dbg !86
  %23 = getelementptr inbounds i32, ptr %20, i64 %22, !dbg !86
  %24 = load i32, ptr %23, align 4, !dbg !86
  store i32 %24, ptr %5, align 4, !dbg !88
  br label %28, !dbg !89

25:                                               ; preds = %11
  %26 = load i32, ptr @counter, align 4, !dbg !90
  %27 = add nsw i32 %26, 1, !dbg !90
  store i32 %27, ptr @counter, align 4, !dbg !90
  br label %28

28:                                               ; preds = %25, %19
  %29 = load i32, ptr %6, align 4, !dbg !91
  %30 = add nsw i32 %29, 1, !dbg !91
  store i32 %30, ptr %6, align 4, !dbg !91
  br label %7, !dbg !75, !llvm.loop !92

31:                                               ; preds = %7
  %32 = load i32, ptr %5, align 4, !dbg !94
  ret i32 %32, !dbg !95
}

attributes #0 = { noinline nounwind optnone uwtable "frame-pointer"="non-leaf" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="generic" "target-features"="+fp-armv8,+neon,+outline-atomics,+v8a,-fmv" }

!llvm.dbg.cu = !{!2}
!llvm.module.flags = !{!6, !7, !8, !9, !10, !11, !12}
!llvm.ident = !{!13}

!0 = !DIGlobalVariableExpression(var: !1, expr: !DIExpression())
!1 = distinct !DIGlobalVariable(name: "counter", scope: !2, file: !3, line: 1, type: !5, isLocal: false, isDefinition: true)
!2 = distinct !DICompileUnit(language: DW_LANG_C11, file: !3, producer: "Debian clang version 19.1.7 (3+b1)", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, globals: !4, splitDebugInlining: false, nameTableKind: None)
!3 = !DIFile(filename: "prog.c", directory: "/hod/tests/lines", checksumkind: CSK_MD5, checksum: "13f687eb73cc69341e05f6d0a22992d7")
!4 = !{!0}
!5 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!6 = !{i32 7, !"Dwarf Version", i32 5}
!7 = !{i32 2, !"Debug Info Version", i32 3}
!8 = !{i32 1, !"wchar_size", i32 4}
!9 = !{i32 8, !"PIC Level", i32 2}
!10 = !{i32 7, !"PIE Level", i32 2}
!11 = !{i32 7, !"uwtable", i32 2}
!12 = !{i32 7, !"frame-pointer", i32 1}
!13 = !{!"Debian clang version 19.1.7 (3+b1)"}
!14 = distinct !DISubprogram(name: "sum_to", scope: !3, file: !3, line: 3, type: !15, scopeLine: 3, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !2, retainedNodes: !17)
!15 = !DISubroutineType(types: !16)
!16 = !{!5, !5}
!17 = !{}
!18 = !DILocalVariable(name: "n", arg: 1, scope: !14, file: !3, line: 3, type: !5)
!19 = !DILocation(line: 3, column: 16, scope: !14)
!20 = !DILocalVariable(name: "s", scope: !14, file: !3, line: 4, type: !5)
!21 = !DILocation(line: 4, column: 7, scope: !14)
!22 = !DILocalVariable(name: "i", scope: !23, file: !3, line: 5, type: !5)
!23 = distinct !DILexicalBlock(scope: !14, file: !3, line: 5, column: 3)
!24 = !DILocation(line: 5, column: 12, scope: !23)
!25 = !DILocation(line: 5, column: 8, scope: !23)
!26 = !DILocation(line: 5, column: 19, scope: !27)
!27 = distinct !DILexicalBlock(scope: !23, file: !3, line: 5, column: 3)
!28 = !DILocation(line: 5, column: 24, scope: !27)
!29 = !DILocation(line: 5, column: 21, scope: !27)
!30 = !DILocation(line: 5, column: 3, scope: !23)
!31 = !DILocation(line: 6, column: 10, scope: !32)
!32 = distinct !DILexicalBlock(scope: !27, file: !3, line: 5, column: 32)
!33 = !DILocation(line: 6, column: 7, scope: !32)
!34 = !DILocation(line: 7, column: 3, scope: !32)
!35 = !DILocation(line: 5, column: 28, scope: !27)
!36 = !DILocation(line: 5, column: 3, scope: !27)
!37 = distinct !{!37, !30, !38, !39}
!38 = !DILocation(line: 7, column: 3, scope: !23)
!39 = !{!"llvm.loop.mustprogress"}
!40 = !DILocation(line: 8, column: 10, scope: !14)
!41 = !DILocation(line: 8, column: 3, scope: !14)
!42 = distinct !DISubprogram(name: "classify", scope: !3, file: !3, line: 11, type: !15, scopeLine: 11, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !2, retainedNodes: !17)
!43 = !DILocalVariable(name: "v", arg: 1, scope: !42, file: !3, line: 11, type: !5)
!44 = !DILocation(line: 11, column: 18, scope: !42)
!45 = !DILocalVariable(name: "r", scope: !42, file: !3, line: 12, type: !5)
!46 = !DILocation(line: 12, column: 7, scope: !42)
!47 = !DILocation(line: 13, column: 11, scope: !42)
!48 = !DILocation(line: 13, column: 3, scope: !42)
!49 = !DILocation(line: 15, column: 7, scope: !50)
!50 = distinct !DILexicalBlock(scope: !42, file: !3, line: 13, column: 14)
!51 = !DILocation(line: 16, column: 5, scope: !50)
!52 = !DILocation(line: 18, column: 7, scope: !50)
!53 = !DILocation(line: 19, column: 5, scope: !50)
!54 = !DILocation(line: 21, column: 9, scope: !50)
!55 = !DILocation(line: 21, column: 11, scope: !50)
!56 = !DILocation(line: 21, column: 7, scope: !50)
!57 = !DILocation(line: 22, column: 3, scope: !50)
!58 = !DILocation(line: 23, column: 14, scope: !42)
!59 = !DILocation(line: 23, column: 11, scope: !42)
!60 = !DILocation(line: 24, column: 10, scope: !42)
!61 = !DILocation(line: 24, column: 3, scope: !42)
!62 = distinct !DISubprogram(name: "scan", scope: !3, file: !3, line: 27, type: !63, scopeLine: 27, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !2, retainedNodes: !17)
!63 = !DISubroutineType(types: !64)
!64 = !{!5, !65, !5}
!65 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !66, size: 64)
!66 = !DIDerivedType(tag: DW_TAG_const_type, baseType: !5)
!67 = !DILocalVariable(name: "v", arg: 1, scope: !62, file: !3, line: 27, type: !65)
!68 = !DILocation(line: 27, column: 21, scope: !62)
!69 = !DILocalVariable(name: "n", arg: 2, scope: !62, file: !3, line: 27, type: !5)
!70 = !DILocation(line: 27, column: 28, scope: !62)
!71 = !DILocalVariable(name: "best", scope: !62, file: !3, line: 28, type: !5)
!72 = !DILocation(line: 28, column: 7, scope: !62)
!73 = !DILocalVariable(name: "i", scope: !62, file: !3, line: 29, type: !5)
!74 = !DILocation(line: 29, column: 7, scope: !62)
!75 = !DILocation(line: 30, column: 3, scope: !62)
!76 = !DILocation(line: 30, column: 10, scope: !62)
!77 = !DILocation(line: 30, column: 14, scope: !62)
!78 = !DILocation(line: 30, column: 12, scope: !62)
!79 = !DILocation(line: 31, column: 9, scope: !80)
!80 = distinct !DILexicalBlock(scope: !81, file: !3, line: 31, column: 9)
!81 = distinct !DILexicalBlock(scope: !62, file: !3, line: 30, column: 17)
!82 = !DILocation(line: 31, column: 11, scope: !80)
!83 = !DILocation(line: 31, column: 16, scope: !80)
!84 = !DILocation(line: 31, column: 14, scope: !80)
!85 = !DILocation(line: 31, column: 9, scope: !81)
!86 = !DILocation(line: 32, column: 14, scope: !80)
!87 = !DILocation(line: 32, column: 16, scope: !80)
!88 = !DILocation(line: 32, column: 12, scope: !80)
!89 = !DILocation(line: 32, column: 7, scope: !80)
!90 = !DILocation(line: 34, column: 14, scope: !80)
!91 = !DILocation(line: 35, column: 6, scope: !81)
!92 = distinct !{!92, !75, !93, !39}
!93 = !DILocation(line: 36, column: 3, scope: !62)
!94 = !DILocation(line: 37, column: 10, scope: !62)
!95 = !DILocation(line: 37, column: 3, scope: !62)
