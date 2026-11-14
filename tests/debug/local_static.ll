; WP-195 (spec/dwarf.md §6.25): `struct P { static auto w() { struct L {
; int v; }; static int calls; return L{++calls}; } };` with the local class
; scoped in `w`: making the static's subprogram makes `P`, whose `w`
; returns `L`, whose scope is the subprogram being made.
target datalayout = "e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128-Fn32"
target triple = "aarch64-unknown-linux-gnu"

@_ZZN1P1wEvE5calls = linkonce_odr dso_local global i32 0, align 4, !dbg !20

define linkonce_odr dso_local i32 @_ZN1P1wEv() #0 !dbg !30 {
entry:
  %n = load i32, ptr @_ZZN1P1wEvE5calls, align 4, !dbg !33
  %m = add i32 %n, 1, !dbg !33
  store i32 %m, ptr @_ZZN1P1wEvE5calls, align 4, !dbg !33
  ret i32 %m, !dbg !34
}

define dso_local i32 @main() #0 !dbg !40 {
entry:
  %a = call i32 @_ZN1P1wEv(), !dbg !41
  %b = call i32 @_ZN1P1wEv(), !dbg !44
  %c = sub i32 %b, 2, !dbg !44
  ret i32 %c, !dbg !44
}

attributes #0 = { noinline nounwind optnone "frame-pointer"="all" }

!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!2, !3}

!0 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !1, producer: "kiln", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, globals: !19, splitDebugInlining: false, nameTableKind: None)
!1 = !DIFile(filename: "local_static.cpp", directory: ".")
!2 = !{i32 7, !"Dwarf Version", i32 5}
!3 = !{i32 2, !"Debug Info Version", i32 3}
!4 = !{}
!13 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!19 = !{!20}
!20 = !DIGlobalVariableExpression(var: !21, expr: !DIExpression())
!21 = distinct !DIGlobalVariable(name: "calls", scope: !30, file: !1, line: 4, type: !13, isLocal: false, isDefinition: true)
!22 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "P", file: !1, line: 1, size: 8, flags: DIFlagTypePassByValue, elements: !23, identifier: "_ZTS1P")
!23 = !{!24}
!24 = !DISubprogram(name: "w", linkageName: "_ZN1P1wEv", scope: !22, file: !1, line: 2, type: !25, scopeLine: 2, flags: DIFlagPrototyped | DIFlagStaticMember, spFlags: 0)
!25 = !DISubroutineType(types: !26)
!26 = !{!27}
!27 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "L", scope: !30, file: !1, line: 3, size: 32, flags: DIFlagTypePassByValue, elements: !28, identifier: "_ZTSZN1P1wEvE1L")
!28 = !{!29}
!29 = !DIDerivedType(tag: DW_TAG_member, name: "v", scope: !27, file: !1, line: 3, baseType: !13, size: 32)
!30 = distinct !DISubprogram(name: "w", linkageName: "_ZN1P1wEv", scope: !22, file: !1, line: 2, type: !25, scopeLine: 5, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0, declaration: !24, retainedNodes: !4)
!33 = !DILocation(line: 5, column: 14, scope: !30)
!34 = !DILocation(line: 5, column: 5, scope: !30)
!40 = distinct !DISubprogram(name: "main", scope: !1, file: !1, line: 8, type: !42, scopeLine: 8, flags: DIFlagPrototyped, spFlags: DISPFlagDefinition, unit: !0)
!41 = !DILocation(line: 9, column: 3, scope: !40)
!42 = !DISubroutineType(types: !43)
!43 = !{!13}
!44 = !DILocation(line: 10, column: 10, scope: !40)
;@ gdb: break P::w
;@ gdb: run
;@ gdb: print calls
;@ gdb: info locals
;@ gdb: continue
;@ gdb: finish
;@ gdb: print 'P::w()::calls'
;@ gdb: continue
;@ gdb-check: Breakpoint 1, P::w
;@ gdb-check: $1 = 0
;@ gdb-check: $3 = 2
;@ gdb-check: exited normally
;@ lldb: breakpoint set --name P::w
;@ lldb: run
;@ lldb: frame variable calls
;@ lldb: continue
;@ lldb: frame variable calls
;@ lldb: continue
;@ lldb-check: stop reason = breakpoint 1.1
;@ lldb-check: (int) calls = 0
;@ lldb-check: (int) calls = 1
;@ lldb-check: exited with status = 0
