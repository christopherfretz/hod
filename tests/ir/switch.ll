; switch: a dense range (a jump table at higher optimization levels),
; sparse and negative case values, cases that share a target, the default,
; and i8 and i64 conditions.
;@ covers: spec/ir.md §5.4 terminator (Switch)
;@ tier: quick
;@ stdout: dense(-1) = -1
;@ stdout: dense(0) = 100
;@ stdout: dense(1) = 11
;@ stdout: dense(2) = 22
;@ stdout: dense(3) = 33
;@ stdout: dense(4) = 44
;@ stdout: dense(5) = 55
;@ stdout: dense(6) = 22
;@ stdout: dense(7) = 100
;@ stdout: dense(8) = -1
;@ stdout: sparse(1) = one
;@ stdout: sparse(10) = ten
;@ stdout: sparse(100) = hundred
;@ stdout: sparse(1000) = thousand
;@ stdout: sparse(-5) = minus five
;@ stdout: sparse(5) = other
;@ stdout: sparse(1001) = other
;@ stdout: small(-128) = 1
;@ stdout: small(127) = 2
;@ stdout: small(65) = 3
;@ stdout: small(0) = 0
;@ stdout: wide(4294967296) = 1
;@ stdout: wide(-9223372036854775808) = 2
;@ stdout: wide(0) = 3
;@ stdout: wide(1) = 0

@.dense = private unnamed_addr constant [16 x i8] c"dense(%d) = %d\0A\00"
@.sparse = private unnamed_addr constant [17 x i8] c"sparse(%d) = %s\0A\00"
@.small = private unnamed_addr constant [16 x i8] c"small(%d) = %d\0A\00"
@.wide = private unnamed_addr constant [17 x i8] c"wide(%lld) = %d\0A\00"
@.one = private unnamed_addr constant [4 x i8] c"one\00"
@.ten = private unnamed_addr constant [4 x i8] c"ten\00"
@.hundred = private unnamed_addr constant [8 x i8] c"hundred\00"
@.thousand = private unnamed_addr constant [9 x i8] c"thousand\00"
@.minus = private unnamed_addr constant [11 x i8] c"minus five\00"
@.other = private unnamed_addr constant [6 x i8] c"other\00"

declare i32 @printf(ptr, ...)

define i32 @dense(i32 %v) {
entry:
  switch i32 %v, label %default [
    i32 0, label %c0
    i32 1, label %c1
    i32 2, label %c2
    i32 3, label %c3
    i32 4, label %c4
    i32 5, label %c5
    i32 6, label %c2
    i32 7, label %c0
  ]
c0:
  ret i32 100
c1:
  ret i32 11
c2:
  ret i32 22
c3:
  ret i32 33
c4:
  ret i32 44
c5:
  ret i32 55
default:
  ret i32 -1
}

define ptr @sparse(i32 %v) {
entry:
  switch i32 %v, label %default [
    i32 1, label %one
    i32 10, label %ten
    i32 100, label %hundred
    i32 1000, label %thousand
    i32 -5, label %minus
  ]
one:
  br label %done
ten:
  br label %done
hundred:
  br label %done
thousand:
  br label %done
minus:
  br label %done
default:
  br label %done
done:
  %r = phi ptr [ @.one, %one ], [ @.ten, %ten ], [ @.hundred, %hundred ], [ @.thousand, %thousand ], [ @.minus, %minus ], [ @.other, %default ]
  ret ptr %r
}

define i32 @small(i8 %v) {
entry:
  switch i8 %v, label %default [
    i8 -128, label %lowest
    i8 127, label %highest
    i8 65, label %letter
  ]
lowest:
  ret i32 1
highest:
  ret i32 2
letter:
  ret i32 3
default:
  ret i32 0
}

define i32 @wide(i64 %v) {
entry:
  switch i64 %v, label %default [
    i64 4294967296, label %a
    i64 -9223372036854775808, label %b
    i64 0, label %c
  ]
a:
  ret i32 1
b:
  ret i32 2
c:
  ret i32 3
default:
  ret i32 0
}

define void @show_dense(i32 %v) {
  %r = call i32 @dense(i32 %v)
  call i32 (ptr, ...) @printf(ptr @.dense, i32 %v, i32 %r)
  ret void
}

define void @show_sparse(i32 %v) {
  %r = call ptr @sparse(i32 %v)
  call i32 (ptr, ...) @printf(ptr @.sparse, i32 %v, ptr %r)
  ret void
}

define void @show_small(i8 %v) {
  %r = call i32 @small(i8 %v)
  %x = sext i8 %v to i32
  call i32 (ptr, ...) @printf(ptr @.small, i32 %x, i32 %r)
  ret void
}

define void @show_wide(i64 %v) {
  %r = call i32 @wide(i64 %v)
  call i32 (ptr, ...) @printf(ptr @.wide, i64 %v, i32 %r)
  ret void
}

define i32 @main() {
entry:
  br label %loop
loop:
  %i = phi i32 [ -1, %entry ], [ %next, %loop ]
  call void @show_dense(i32 %i)
  %next = add i32 %i, 1
  %more = icmp sle i32 %next, 8
  br i1 %more, label %loop, label %rest
rest:
  call void @show_sparse(i32 1)
  call void @show_sparse(i32 10)
  call void @show_sparse(i32 100)
  call void @show_sparse(i32 1000)
  call void @show_sparse(i32 -5)
  call void @show_sparse(i32 5)
  call void @show_sparse(i32 1001)
  call void @show_small(i8 -128)
  call void @show_small(i8 127)
  call void @show_small(i8 65)
  call void @show_small(i8 0)
  call void @show_wide(i64 4294967296)
  call void @show_wide(i64 -9223372036854775808)
  call void @show_wide(i64 0)
  call void @show_wide(i64 1)
  ret i32 0
}
