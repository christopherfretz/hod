; WP-195 (spec/dwarf.md §6.26): `struct A { template <class U> void
; own(int *) { typedef int T; ... } };` whose `own<int>` has no code in the
; unit, `T *` retained (a cast's type): the typedef is a child of `own`'s
; abstract instance, as llc 22 writes it.
target triple = "aarch64-unknown-linux-gnu"

define dso_local i32 @main() #0 !dbg !40 {
entry:
  %p = alloca ptr, align 8
    #dbg_declare(ptr %p, !45, !DIExpression(), !41)
  store ptr null, ptr %p, align 8, !dbg !41
  ret i32 0, !dbg !44
}

attributes #0 = { noinline nounwind optnone "frame-pointer"="all" }

!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!2, !3}

!0 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !1, producer: "kiln", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, retainedTypes: !19, splitDebugInlining: false, nameTableKind: None)
!1 = !DIFile(filename: "local_type_no_code.cpp", directory: ".")
!2 = !{i32 7, !"Dwarf Version", i32 5}
!3 = !{i32 2, !"Debug Info Version", i32 3}
!4 = !{}
!13 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!19 = !{!20}
!20 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !21, size: 64)
!21 = !DIDerivedType(tag: DW_TAG_typedef, name: "T", scope: !30, file: !1, line: 3, baseType: !13)
!22 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "A", file: !1, line: 1, size: 8, flags: DIFlagTypePassByValue, elements: !23, identifier: "_ZTS1A")
!23 = !{!24}
!24 = !DISubprogram(name: "own", linkageName: "_ZN1A3ownEPi", scope: !22, file: !1, line: 2, type: !25, scopeLine: 2, flags: DIFlagPublic | DIFlagPrototyped, spFlags: 0, templateParams: !50)
!25 = !DISubroutineType(types: !26)
!26 = !{null, !27, !28}
!27 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !22, size: 64, flags: DIFlagArtificial | DIFlagObjectPointer)
!28 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !13, size: 64)
!30 = distinct !DISubprogram(name: "own", linkageName: "_ZN1A3ownEPi", scope: !22, file: !1, line: 2, type: !25, scopeLine: 2, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, declaration: !24, retainedNodes: !4, templateParams: !50)
!40 = distinct !DISubprogram(name: "main", scope: !1, file: !1, line: 6, type: !42, scopeLine: 6, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0)
!41 = !DILocation(line: 6, column: 14, scope: !40)
!42 = !DISubroutineType(types: !43)
!43 = !{!13}
!44 = !DILocation(line: 8, column: 3, scope: !40)
!45 = !DILocalVariable(name: "p", scope: !40, file: !1, line: 7, type: !20)
!50 = !{!51}
!51 = !DITemplateTypeParameter(name: "U", type: !13)
;@ gdb: break local_type_no_code.cpp:8
;@ gdb: run
;@ gdb: whatis p
;@ gdb: ptype p
;@ gdb: print p
;@ gdb: continue
;@ gdb-check: Breakpoint 1, main
;@ gdb-check: type = T *
;@ gdb-check: type = int *
;@ gdb-check: exited normally
;@ lldb: breakpoint set --file local_type_no_code.cpp --line 8
;@ lldb: run
;@ lldb: frame variable p
;@ lldb: continue
;@ lldb-check: stop reason = breakpoint 1.1
;@ lldb-check: (T *) p = 0x
;@ lldb-check: exited with status = 0
