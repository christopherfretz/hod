; ModuleID = '../p/eh.cpp'
source_filename = "../p/eh.cpp"
target datalayout = "e-m:e-p:32:32-p270:32:32-p271:32:32-p272:64:64-i128:128-f64:32:64-f80:32-n8:16:32-S128"
target triple = "i686-unknown-linux-gnu"

%struct.D = type { i8 }

@_ZTIi = external constant ptr

; Function Attrs: mustprogress noinline optnone uwtable
define dso_local noundef i32 @_Z1fi(i32 noundef %0) #0 personality ptr @__gxx_personality_v0 !dbg !11 {
  %2 = alloca i32, align 4
  %3 = alloca i32, align 4
  %4 = alloca %struct.D, align 1
  %5 = alloca ptr, align 4
  %6 = alloca i32, align 4
  %7 = alloca i32, align 4
  store i32 %0, ptr %3, align 4
    #dbg_declare(ptr %3, !16, !DIExpression(), !17)
    #dbg_declare(ptr %4, !18, !DIExpression(), !26)
  %8 = load i32, ptr %3, align 4, !dbg !27
  invoke void @_Z1gi(i32 noundef %8)
          to label %9 unwind label %10, !dbg !28

9:                                                ; preds = %1
  call void @_ZN1DD1Ev(ptr noundef nonnull align 1 dereferenceable(1) %4) #4, !dbg !29
  br label %23, !dbg !30

10:                                               ; preds = %1
  %11 = landingpad { ptr, i32 }
          cleanup
          catch ptr @_ZTIi, !dbg !31
  %12 = extractvalue { ptr, i32 } %11, 0, !dbg !31
  store ptr %12, ptr %5, align 4, !dbg !31
  %13 = extractvalue { ptr, i32 } %11, 1, !dbg !31
  store i32 %13, ptr %6, align 4, !dbg !31
  call void @_ZN1DD1Ev(ptr noundef nonnull align 1 dereferenceable(1) %4) #4, !dbg !29
  br label %14, !dbg !29

14:                                               ; preds = %10
  %15 = load i32, ptr %6, align 4, !dbg !30
  %16 = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTIi) #4, !dbg !30
  %17 = icmp eq i32 %15, %16, !dbg !30
  br i1 %17, label %18, label %26, !dbg !30

18:                                               ; preds = %14
    #dbg_declare(ptr %7, !32, !DIExpression(), !33)
  %19 = load ptr, ptr %5, align 4, !dbg !30
  %20 = call ptr @__cxa_begin_catch(ptr %19) #4, !dbg !30
  %21 = load i32, ptr %20, align 4, !dbg !30
  store i32 %21, ptr %7, align 4, !dbg !30
  %22 = load i32, ptr %7, align 4, !dbg !34
  store i32 %22, ptr %2, align 4, !dbg !36
  call void @__cxa_end_catch() #4, !dbg !37
  br label %24

23:                                               ; preds = %9
  store i32 0, ptr %2, align 4, !dbg !38
  br label %24, !dbg !38

24:                                               ; preds = %23, %18
  %25 = load i32, ptr %2, align 4, !dbg !39
  ret i32 %25, !dbg !39

26:                                               ; preds = %14
  %27 = load ptr, ptr %5, align 4, !dbg !30
  %28 = load i32, ptr %6, align 4, !dbg !30
  %29 = insertvalue { ptr, i32 } poison, ptr %27, 0, !dbg !30
  %30 = insertvalue { ptr, i32 } %29, i32 %28, 1, !dbg !30
  resume { ptr, i32 } %30, !dbg !30
}

declare void @_Z1gi(i32 noundef) #1

declare i32 @__gxx_personality_v0(...)

; Function Attrs: nounwind
declare void @_ZN1DD1Ev(ptr noundef nonnull align 1 dereferenceable(1)) unnamed_addr #2

; Function Attrs: nounwind memory(none)
declare i32 @llvm.eh.typeid.for.p0(ptr) #3

declare ptr @__cxa_begin_catch(ptr)

declare void @__cxa_end_catch()

attributes #0 = { mustprogress noinline optnone uwtable "frame-pointer"="all" "min-legal-vector-width"="0" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="i686" "target-features"="+cmov,+cx8,+x87" "tune-cpu"="generic" }
attributes #1 = { "frame-pointer"="all" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="i686" "target-features"="+cmov,+cx8,+x87" "tune-cpu"="generic" }
attributes #2 = { nounwind "frame-pointer"="all" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="i686" "target-features"="+cmov,+cx8,+x87" "tune-cpu"="generic" }
attributes #3 = { nounwind memory(none) }
attributes #4 = { nounwind }

!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!2, !3, !4, !5, !6, !7, !8, !9}
!llvm.ident = !{!10}

!0 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !1, producer: "Debian clang version 22.1.8 (1~deb13u4)", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, splitDebugInlining: false, nameTableKind: None)
!1 = !DIFile(filename: "../p/eh.cpp", directory: "/Volumes/AgentWork/hod-helper/.build/target-wp-240/abi", checksumkind: CSK_MD5, checksum: "5da45fceb6604dc66cbce63e76f270b7")
!2 = !{i32 1, !"NumRegisterParameters", i32 0}
!3 = !{i32 7, !"Dwarf Version", i32 5}
!4 = !{i32 2, !"Debug Info Version", i32 3}
!5 = !{i32 1, !"wchar_size", i32 4}
!6 = !{i32 8, !"PIC Level", i32 2}
!7 = !{i32 7, !"PIE Level", i32 2}
!8 = !{i32 7, !"uwtable", i32 2}
!9 = !{i32 7, !"frame-pointer", i32 2}
!10 = !{!"Debian clang version 22.1.8 (1~deb13u4)"}
!11 = distinct !DISubprogram(name: "f", linkageName: "_Z1fi", scope: !1, file: !1, line: 3, type: !12, scopeLine: 3, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !15)
!12 = !DISubroutineType(types: !13)
!13 = !{!14, !14}
!14 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!15 = !{}
!16 = !DILocalVariable(name: "x", arg: 1, scope: !11, file: !1, line: 3, type: !14)
!17 = !DILocation(line: 3, column: 11, scope: !11)
!18 = !DILocalVariable(name: "d", scope: !19, file: !1, line: 3, type: !20)
!19 = distinct !DILexicalBlock(scope: !11, file: !1, line: 3, column: 20)
!20 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "D", file: !1, line: 2, size: 8, flags: DIFlagTypePassByReference | DIFlagNonTrivial, elements: !21, identifier: "_ZTS1D")
!21 = !{!22}
!22 = !DISubprogram(name: "~D", linkageName: "_ZN1DD4Ev", scope: !20, file: !1, line: 2, type: !23, scopeLine: 2, flags: DIFlagPrototyped, spFlags: 0)
!23 = !DISubroutineType(types: !24)
!24 = !{null, !25}
!25 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !20, size: 32, flags: DIFlagArtificial | DIFlagObjectPointer)
!26 = !DILocation(line: 3, column: 24, scope: !19)
!27 = !DILocation(line: 3, column: 29, scope: !19)
!28 = !DILocation(line: 3, column: 27, scope: !19)
!29 = !DILocation(line: 3, column: 33, scope: !11)
!30 = !DILocation(line: 3, column: 33, scope: !19)
!31 = !DILocation(line: 3, column: 73, scope: !19)
!32 = !DILocalVariable(name: "e", scope: !11, file: !1, line: 3, type: !14)
!33 = !DILocation(line: 3, column: 46, scope: !11)
!34 = !DILocation(line: 3, column: 58, scope: !35)
!35 = distinct !DILexicalBlock(scope: !11, file: !1, line: 3, column: 49)
!36 = !DILocation(line: 3, column: 51, scope: !35)
!37 = !DILocation(line: 3, column: 61, scope: !35)
!38 = !DILocation(line: 3, column: 63, scope: !11)
!39 = !DILocation(line: 3, column: 73, scope: !11)
