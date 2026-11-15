;@ covers: kiln tests/conformance/dcl.asm/asm-goto.cpp
;@ target: x86_64
;@ llvm: 22
;@ link: c++ m
;@ stdout: 0 1 7 8 9
; corpus entry written by `cargo xtask corpus capture`; do not edit
; source: kiln tests/conformance/dcl.asm/asm-goto.cpp
; producer-args: -std=c++26 -fno-crash-diagnostics -fclang-compat=22 --target=x86_64-unknown-linux-gnu -fdebug-prefix-map=<cwd>=. tests/conformance/dcl.asm/asm-goto.cpp
; delivered: text
; llc-args: -O0 -relocation-model=pic -filetype=obj
; ModuleID = 'tests/conformance/dcl.asm/asm-goto.cpp'
source_filename = "tests/conformance/dcl.asm/asm-goto.cpp"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

@.str = private unnamed_addr constant [16 x i8] c"%d %d %d %d %d\0A\00", align 1

define dso_local noundef i32 @_Z1fv() #0 {
  %1 = alloca i32, align 4
  callbr void asm sideeffect "", "!i,~{dirflag},~{fpsr},~{flags}"() #3
          to label %2 [label %3], !srcloc !6

2:
  store i32 0, ptr %1, align 4
  br label %4

3:
  store i32 1, ptr %1, align 4
  br label %4

4:
  %5 = load i32, ptr %1, align 4
  ret i32 %5
}

define dso_local noundef i32 @_Z1gv() #0 {
  %1 = alloca i32, align 4
  callbr void asm sideeffect "jmp ${0:l}", "!i,~{dirflag},~{fpsr},~{flags}"() #3
          to label %2 [label %3], !srcloc !7

2:
  store i32 0, ptr %1, align 4
  br label %4

3:
  store i32 1, ptr %1, align 4
  br label %4

4:
  %5 = load i32, ptr %1, align 4
  ret i32 %5
}

define dso_local noundef i32 @_Z1hi(i32 noundef %0) #0 {
  %2 = alloca i32, align 4
  %3 = alloca i32, align 4
  %4 = alloca i32, align 4
  store i32 %0, ptr %3, align 4
  %5 = load i32, ptr %3, align 4
  store i32 %5, ptr %4, align 4
  %6 = load i32, ptr %4, align 4
  %7 = callbr i32 asm "jmp ${3:l}", "=r,0,!i,!i,~{dirflag},~{fpsr},~{flags}"(i32 %6) #4
          to label %8 [label %10, label %11], !srcloc !8

8:
  store i32 %7, ptr %4, align 4
  %9 = load i32, ptr %4, align 4
  store i32 %9, ptr %2, align 4
  br label %16

10:
  store i32 %7, ptr %4, align 4
  br label %12

11:
  store i32 %7, ptr %4, align 4
  br label %13

12:
  store i32 -1, ptr %2, align 4
  br label %16

13:
  %14 = load i32, ptr %4, align 4
  %15 = add nsw i32 %14, 1
  store i32 %15, ptr %2, align 4
  br label %16

16:
  %17 = load i32, ptr %2, align 4
  ret i32 %17
}

define dso_local noundef i32 @_Z1ki(i32 noundef %0) #0 {
  %2 = alloca i32, align 4
  %3 = alloca i32, align 4
  %4 = alloca i32, align 4
  store i32 %0, ptr %3, align 4
  store i32 0, ptr %4, align 4, !annotation !9
  %5 = load i32, ptr %3, align 4
  %6 = callbr i32 asm "", "=r,0,!i,~{dirflag},~{fpsr},~{flags}"(i32 %5) #4
          to label %7 [label %10], !srcloc !10

7:
  store i32 %6, ptr %4, align 4
  %8 = load i32, ptr %4, align 4
  %9 = add nsw i32 %8, 1
  store i32 %9, ptr %2, align 4
  br label %12

10:
  store i32 %6, ptr %4, align 4
  br label %11

11:
  store i32 -1, ptr %2, align 4
  br label %12

12:
  %13 = load i32, ptr %2, align 4
  ret i32 %13
}

define dso_local noundef i32 @main() #1 {
  %1 = call noundef i32 @_Z1fv()
  %2 = call noundef i32 @_Z1gv()
  %3 = call noundef i32 @_Z1hi(i32 noundef 6)
  %4 = call noundef i32 @_Z1ki(i32 noundef 7)
  %5 = call noundef i32 @_Z1hi(i32 noundef 8)
  %6 = call i32 (ptr, ...) @printf(ptr noundef @.str, i32 noundef %1, i32 noundef %2, i32 noundef %3, i32 noundef %4, i32 noundef %5)
  ret i32 0
}

declare i32 @printf(ptr noundef, ...) #2

attributes #0 = { mustprogress noinline nounwind optnone uwtable "frame-pointer"="all" "min-legal-vector-width"="0" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="x86-64" "target-features"="+cmov,+cx8,+fxsr,+mmx,+sse,+sse2,+x87" "tune-cpu"="generic" }
attributes #1 = { mustprogress noinline norecurse optnone uwtable "frame-pointer"="all" "min-legal-vector-width"="0" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="x86-64" "target-features"="+cmov,+cx8,+fxsr,+mmx,+sse,+sse2,+x87" "tune-cpu"="generic" }
attributes #2 = { "frame-pointer"="all" "no-trapping-math"="true" "stack-protector-buffer-size"="8" "target-cpu"="x86-64" "target-features"="+cmov,+cx8,+fxsr,+mmx,+sse,+sse2,+x87" "tune-cpu"="generic" }
attributes #3 = { nounwind }
attributes #4 = { nounwind memory(none) }

!llvm.module.flags = !{!0, !1, !2, !3, !4}
!llvm.ident = !{!5}

!0 = !{i32 1, !"wchar_size", i32 4}
!1 = !{i32 8, !"PIC Level", i32 2}
!2 = !{i32 7, !"PIE Level", i32 2}
!3 = !{i32 7, !"uwtable", i32 2}
!4 = !{i32 7, !"frame-pointer", i32 2}
!5 = !{!"kiln version 1.2.0"}
!6 = !{i64 381}
!7 = !{i64 454}
!8 = !{i64 552}
!9 = !{!"auto-init"}
!10 = !{i64 683}
