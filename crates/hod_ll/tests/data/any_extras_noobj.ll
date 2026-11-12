; Accepted input that `llc` cannot turn into an ELF object, or that the
; object does not show: everything that is dropped or recorded as
; unsupported (spec/ll.md §3.3, §3.4), and module-level odds and ends.

source_filename = "a \22quoted\22 name.c"
module asm ".globl from_asm"
module asm "from_asm = 7"

%opaque = type opaque
%ext = type { target("spirv.Image", i32, 0), target("aarch64.svcount") }

$c_exact = comdat exactmatch
$c_largest = comdat largest
$c_same = comdat samesize

@in_exact = global i32 1, comdat($c_exact)
@in_largest = global i32 2, comdat($c_largest)
@in_same = global i32 3, comdat($c_same)

@ppc = global ppc_fp128 0xM00000000000000000000000000000000
@imported = external dllimport global i32
@exported = dllexport global i32 1
@partitioned = global i32 1, partition "part"
@modelled = global i32 1, code_model "large"
@tagged = global i32 1, sanitize_memtag
@attributed = global i32 0, section "named" #0
@signed = global ptr ptrauth (ptr @target_fn, i32 0)
@signed_full = global ptr ptrauth (ptr @target_fn, i32 1, i64 1234, ptr @signed_full)
@typed = global i32 1, !type !20, !type !21, !vcall_visibility !22
@with_other_md = global i32 1, !custom !20, !associated !23
@addrspace_var = addrspace(1) global i32 1
@addrspace_ptr = global ptr addrspace(1) @addrspace_var
@addrspace_cast = global ptr addrspacecast (ptr addrspace(1) @addrspace_var to ptr)
@opaque_user = external global %opaque
@0 = global i32 1
@1 = external global i32
@2 = weak_odr global ptr @0
@anonymous_users = global [2 x ptr] [ptr @1, ptr @3]
@llvm.global.annotations = appending global [1 x { ptr, ptr, ptr, i32, ptr }] [{ ptr, ptr, ptr, i32, ptr } { ptr @tagged, ptr null, ptr null, i32 1, ptr null }], section "llvm.metadata"
@llvm.global_ctors = appending global [0 x { i32, ptr, ptr }] zeroinitializer
@llvm.global_dtors = appending global [2 x { i32, ptr, ptr }] [{ i32, ptr, ptr } zeroinitializer, { i32, ptr, ptr } { i32 1, ptr @target_fn, ptr null }]
@llvm.used = appending global [1 x ptr] [ptr @target_fn], section "llvm.metadata"

declare void @target_fn()
declare dllimport void @fn_imported()
declare void @fn_gc() gc "statepoint-example"
define void @fn_gc_defined() gc "shadow-stack" {
  ret void
}
define void @fn_partition() partition "part" {
  ret void
}
define void @3() {
  ret void
}
define void @fn_prefix() prefix i32 1 prologue i8 144 {
  ret void
}
define void @fn_personality_expr() personality ptr getelementptr (i8, ptr @target_fn, i64 0) {
  ret void
}
define x86_fp80 @fn_metadata(ptr %p) !dbg !12 !prof !24 !custom !20 {
  %v = load x86_fp80, ptr %p, align 16, !dbg !14
  ret x86_fp80 %v
}
declare !dbg !15 void @fn_declared_dbg()

uselistorder ptr @target_fn, { 1, 0, 2, 3, 4 }

attributes #0 = { "bss-section"=".mybss" "data-section"=".mydata" }

!llvm.dbg.cu = !{!10}
!llvm.module.flags = !{!0, !1, !2, !3, !4, !5, !6, !7}
!llvm.linker.options = !{!30, !31, !32}
!llvm.ident = !{!33}
!llvm.errno.tbaa = !{!34}
!custom.named = !{!20, !21}
!llvm.commandline = !{!33}

!0 = !{i32 7, !"Dwarf Version", i32 5}
!1 = !{i32 2, !"Debug Info Version", i32 3}
!2 = !{i32 1, !"wchar_size", i32 4}
!3 = !{i32 4, !"made-up flag", i32 1}
!4 = !{i32 1, !"string flag", !"value"}
!5 = !{i32 5, !"list flag", !{!"a", !"b"}}
!6 = !{i32 3, !"required", !{!"wchar_size", i32 4}}
!7 = !{i32 6, !"appended", !{i32 1}}
!10 = distinct !DICompileUnit(language: DW_LANG_C11, file: !11, producer: "hand {written}", isOptimized: false, runtimeVersion: 0, emissionKind: FullDebug, splitDebugInlining: false, nameTableKind: None)
!11 = !DIFile(filename: "a } file.c", directory: "/a/\22dir\22")
!12 = distinct !DISubprogram(name: "fn_metadata", scope: !11, file: !11, line: 1, type: !13, scopeLine: 1, spFlags: DISPFlagDefinition, unit: !10)
!13 = !DISubroutineType(types: !{null})
!14 = !DILocation(line: 2, column: 3, scope: !12)
!15 = !DISubprogram(name: "fn_declared_dbg", scope: !11, file: !11, line: 5, type: !13, spFlags: DISPFlagOptimized)
!20 = !{i64 0, !"type one"}
!21 = !{i64 8, !"type two"}
!22 = !{i64 1}
!23 = !{ptr @typed}
!24 = !{!"function_entry_count", i64 10}
!30 = !{!"-lone"}
!31 = !{!"-u", !"two"}
!32 = !{}
!33 = !{!"hand-written 1.0"}
!34 = !{!35, !35, i64 0}
!35 = !{!"int", !36, i64 0}
!36 = !{!"root"}

^0 = module: (path: "x.ll", hash: (0, 0, 0, 0, 0))
^1 = flags: 8
^2 = blockcount: 0
