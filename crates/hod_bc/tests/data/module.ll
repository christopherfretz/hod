; spec/bc.md §5: one of most of what the module level holds.
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

module asm "nop"
module asm ".globl asm_symbol"

$c1 = comdat any
$aw = comdat largest

%T = type { i32, ptr, [2 x i16] }
%Op = type opaque
%Wrap = type { %T, ppc_fp128 }

@g0 = global i32 42, align 4
@g1 = internal constant %T { i32 -1, ptr @g0, [2 x i16] [i16 1, i16 2] }, section ".mysec", comdat($c1)
@g2 = thread_local(initialexec) global i64 -5
@g3 = external global i8
@g4 = weak_odr hidden unnamed_addr global x86_fp80 0xK3FFF8000000000000000
@g5 = global fp128 0xL00000000000000003FFF000000000000
@g6 = global half 0xH3C00
@g7 = global <4 x float> <float 1.0, float 2.0, float 3.0, float 4.0>
@g8 = global <4 x i32> <i32 7, i32 7, i32 7, i32 7>
@g9 = global i128 -170141183460469231731687303715884105728
@g10 = global ptr getelementptr inbounds (i8, ptr @g0, i64 4)
@g11 = global i64 ptrtoint (ptr @g0 to i64)
@g12 = global i64 sub (i64 ptrtoint (ptr @g1 to i64), i64 ptrtoint (ptr @g0 to i64))
@g13 = global ptr addrspace(1) addrspacecast (ptr @g0 to ptr addrspace(1))
@g14 = global [3 x i8] c"ab\00"
@g15 = global ptr blockaddress(@f, %bb)
@g16 = global ptr dso_local_equivalent @decl
@g17 = global ptr no_cfi @decl
@vt = global { [4 x ptr] } zeroinitializer
@g18 = global ptr getelementptr inrange(-8, 16) ({ [4 x ptr] }, ptr @vt, i32 0, i32 0, i32 2)
@g19 = global { i32, float, double } { i32 1, float 5.000000e-01, double -2.0 } #2
@g20 = global ptr ptrauth (ptr @f, i32 0)
@g21 = protected global i1 true, align 1
@g22 = global [2 x i64] [i64 -1, i64 9223372036854775807]
@g23 = global i8 undef
@g24 = global ptr poison
@g25 = dllexport global i32 0, partition "part", code_model "large"
@g26 = global ppc_fp128 0xM3FF00000000000000000000000000000
@g27 = global %Wrap zeroinitializer
@llvm.foo = global i32 3
@0 = private constant i32 1
@1 = global i32 2
@llvm.used = appending global [1 x ptr] [ptr @g0], section "llvm.metadata"
@llvm.compiler.used = appending global [1 x ptr] [ptr @g3], section "llvm.metadata"
@llvm.global_ctors = appending global [2 x { i32, ptr, ptr }] [{ i32, ptr, ptr } { i32 65535, ptr @f, ptr null }, { i32, ptr, ptr } { i32 101, ptr @aw, ptr @g0 }]

@a1 = alias i32, ptr @g0
@a2 = internal alias i8, getelementptr (i8, ptr @g0, i64 1)
@a3 = weak hidden alias i32, ptr @0
@i1 = ifunc void (), ptr @resolver

define void @f() #0 personality ptr @pers {
entry:
  br label %bb
bb:
  ret void
}

define internal ptr @resolver() {
  ret ptr @f
}

declare !annotation !0 !foo !0 void @decl(ptr sret(%T) align 8, i32 signext, ptr byval(i64), ptr noundef nonnull) #1

declare i32 @pers(...)

define weak void @aw() section ".text.x" comdat align 16 {
  ret void
}

define internal fastcc zeroext i8 @g(i32 inreg %x, ptr dereferenceable(8) %p) uwtable(sync) "frame-pointer"="all" "unknown-thing" {
  ret i8 0
}

attributes #0 = { noinline nounwind optnone uwtable "frame-pointer"="all" "target-cpu"="x86-64" }
attributes #1 = { cold "no-builtins" }
attributes #2 = { "bss-section"="x" }

!llvm.module.flags = !{!1, !2, !3, !4}
!llvm.linker.options = !{!5}
!llvm.dependent-libraries = !{!6}
!llvm.ident = !{!7}
!other = !{}
!0 = !{}
!1 = !{i32 1, !"wchar_size", i32 4}
!2 = !{i32 8, !"PIC Level", i32 2}
!3 = !{i32 1, !"some string", !"value"}
!4 = !{i32 1, !"a node", !0}
!5 = !{!"-lfoo", !"-lbar"}
!6 = !{!"m"}
!7 = !{!"some compiler"}
