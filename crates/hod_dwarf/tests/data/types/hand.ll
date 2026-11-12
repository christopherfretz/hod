; spec/dwarf.md §6.8, §8.6: the type-graph shapes C++ sources do not
; give clang `-g` (friends, template aliases, every DW_ATE, endianity,
; calling conventions on subroutine types, variable counts, explicit
; lower bounds, wide enumerators), written by hand.
source_filename = "hand.cpp"
target datalayout = "e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128-Fn32"
target triple = "aarch64-unknown-linux-gnu"

@n = dso_local global i64 4, align 8, !dbg !0
@vla_ptr = dso_local global ptr null, align 8, !dbg !3
@lost_ptr = dso_local global ptr null, align 8, !dbg !12

!llvm.dbg.cu = !{!20}
!llvm.module.flags = !{!90, !91}

!0 = !DIGlobalVariableExpression(var: !1, expr: !DIExpression())
!1 = distinct !DIGlobalVariable(name: "n", scope: !20, file: !21, line: 1, type: !2, isLocal: false, isDefinition: true)
!2 = !DIBasicType(name: "long", size: 64, encoding: DW_ATE_signed)
!3 = !DIGlobalVariableExpression(var: !4, expr: !DIExpression())
!4 = distinct !DIGlobalVariable(name: "vla_ptr", scope: !20, file: !21, line: 2, type: !5, isLocal: false, isDefinition: true)
!5 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !6, size: 64)
!6 = !DICompositeType(tag: DW_TAG_array_type, baseType: !7, elements: !8)
!7 = !DIBasicType(name: "int", size: 32, encoding: DW_ATE_signed)
!8 = !{!9}
!9 = !DISubrange(count: !1)
!12 = !DIGlobalVariableExpression(var: !13, expr: !DIExpression())
!13 = distinct !DIGlobalVariable(name: "lost_ptr", scope: !20, file: !21, line: 3, type: !14, isLocal: false, isDefinition: true)
!14 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !15, size: 64)
!15 = !DICompositeType(tag: DW_TAG_array_type, baseType: !7, elements: !16)
!16 = !{!17}
!17 = !DISubrange(count: !18)
!18 = distinct !DIGlobalVariable(name: "never_written", scope: !20, file: !21, line: 4, type: !2, isLocal: false, isDefinition: true)

!20 = distinct !DICompileUnit(language: DW_LANG_C_plus_plus_14, file: !21, producer: "hand", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, retainedTypes: !22, globals: !23, splitDebugInlining: false, nameTableKind: None)
!21 = !DIFile(filename: "hand.cpp", directory: "/hod/tests/types")
!22 = !{!30, !33, !36, !40, !41, !42, !43, !44, !45, !46, !47, !48, !49, !50, !51, !52, !53, !54, !55, !56, !57, !60, !62, !64, !68}
!23 = !{!0, !3, !12}

; a class with a friend
!30 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "WithFriend", file: !21, line: 10, size: 32, flags: DIFlagTypePassByValue, elements: !31, identifier: "_ZTS10WithFriend")
!31 = !{!32, !35}
!32 = !DIDerivedType(tag: DW_TAG_member, name: "x", scope: !30, file: !21, line: 11, baseType: !7, size: 32)
!35 = !DIDerivedType(tag: DW_TAG_friend, scope: !30, baseType: !33)
!33 = distinct !DICompositeType(tag: DW_TAG_structure_type, name: "Pal", file: !21, line: 9, flags: DIFlagFwdDecl, identifier: "_ZTS3Pal")
; a template alias (its parameters, in `extraData`, are WP-059's:
; type_graph.rs tests that hod skips them)
!36 = !DIDerivedType(tag: DW_TAG_template_alias, name: "Alias", file: !21, line: 14, baseType: !37)
!37 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !7, size: 64)
; every encoding
!40 = !DIBasicType(name: "addr", size: 64, encoding: DW_ATE_address)
!41 = !DIBasicType(name: "boolean", size: 8, encoding: DW_ATE_boolean)
!42 = !DIBasicType(name: "complex", size: 128, encoding: DW_ATE_complex_float)
!43 = !DIBasicType(name: "float", size: 32, encoding: DW_ATE_float)
!44 = !DIBasicType(name: "schar", size: 8, encoding: DW_ATE_signed_char)
!45 = !DIBasicType(name: "unsigned", size: 32, encoding: DW_ATE_unsigned)
!46 = !DIBasicType(name: "uchar", size: 8, encoding: DW_ATE_unsigned_char)
!47 = !DIBasicType(name: "imaginary", size: 64, encoding: DW_ATE_imaginary_float)
!48 = !DIBasicType(name: "packed", size: 32, encoding: DW_ATE_packed_decimal)
!49 = !DIBasicType(name: "numeric", size: 32, encoding: DW_ATE_numeric_string)
!50 = !DIBasicType(name: "edited", size: 32, encoding: DW_ATE_edited)
!51 = !DIBasicType(name: "sfixed", size: 32, encoding: DW_ATE_signed_fixed)
!52 = !DIBasicType(name: "ufixed", size: 32, encoding: DW_ATE_unsigned_fixed)
!53 = !DIBasicType(name: "decimal", size: 64, encoding: DW_ATE_decimal_float)
!54 = !DIBasicType(name: "utf", size: 32, encoding: DW_ATE_UTF)
!55 = !DIBasicType(name: "big", size: 32, encoding: DW_ATE_signed, flags: DIFlagBigEndian)
!56 = !DIBasicType(name: "little", size: 32, encoding: DW_ATE_unsigned, flags: DIFlagLittleEndian)
; calling conventions on subroutine types
!57 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !58, size: 64)
!58 = !DISubroutineType(cc: DW_CC_LLVM_vectorcall, types: !59)
!59 = !{null, !7}
!60 = !DIDerivedType(tag: DW_TAG_pointer_type, baseType: !61, size: 64)
!61 = !DISubroutineType(cc: DW_CC_BORLAND_pascal, types: !59)
; an explicit lower bound, and a bound-only subrange
!62 = !DICompositeType(tag: DW_TAG_array_type, baseType: !7, size: 96, elements: !63)
!63 = !{!66, !67}
!66 = !DISubrange(count: 3, lowerBound: 1)
!67 = !DISubrange(lowerBound: 0, upperBound: 5)
; wide and signed enumerators, an unsigned base
!64 = distinct !DICompositeType(tag: DW_TAG_enumeration_type, name: "Wide", file: !21, line: 20, baseType: !65, size: 64, flags: DIFlagEnumClass, elements: !69, identifier: "_ZTS4Wide")
!65 = !DIBasicType(name: "unsigned long", size: 64, encoding: DW_ATE_unsigned)
!69 = !{!70, !71}
!70 = !DIEnumerator(name: "Top", value: 18446744073709551615, isUnsigned: true)
!71 = !DIEnumerator(name: "Mid", value: 9223372036854775808, isUnsigned: true)
!68 = distinct !DICompositeType(tag: DW_TAG_enumeration_type, name: "Signed", file: !21, line: 21, baseType: !2, size: 64, elements: !72, identifier: "_ZTS6Signed")
!72 = !{!73, !74}
!73 = !DIEnumerator(name: "Lowest", value: -9223372036854775808)
!74 = !DIEnumerator(name: "MinusOne", value: -1)

!90 = !{i32 7, !"Dwarf Version", i32 5}
!91 = !{i32 2, !"Debug Info Version", i32 3}
