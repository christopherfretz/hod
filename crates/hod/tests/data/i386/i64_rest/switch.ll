; WP-306: `switch` on i64, i48 and i33 on i386 (cases above 2^32, negative
; cases, cases that share a half, a default, many cases, no case), and a
; constant condition. Each value of @vals goes through every switch; the
; output is one line per value and switch.
target datalayout = "e-m:e-p:32:32-p270:32:32-p271:32:32-p272:64:64-i128:128-f64:32:64-f80:32-n8:16:32-S128"
target triple = "i686-unknown-linux-gnu"

@vals = internal global [30 x i64] [i64 0, i64 1, i64 -1, i64 2147483648, i64 4294967295, i64 4294967296, i64 9223372036854775807, i64 -9223372036854775808, i64 5, i64 4294967301, i64 -4294967291, i64 81985529216486896, i64 -2401053088876216593, i64 100, i64 -5, i64 7, i64 4294967303, i64 2, i64 3, i64 140737488355327, i64 140737488355328, i64 281474976710655, i64 -281474976710651, i64 281474976710661, i64 8589934591, i64 8589934592, i64 4294967298, i64 -8589934590, i64 99, i64 12884901893]
@fmt = private constant [12 x i8] c"%s %d %08x\0A\00"
@done = private constant [5 x i8] c"done\00"
@n64 = private constant [4 x i8] c"s64\00"
@n48 = private constant [4 x i8] c"s48\00"
@n33 = private constant [4 x i8] c"s33\00"
@nd = private constant [4 x i8] c"def\00"
@nk = private constant [4 x i8] c"cst\00"
@n1 = private constant [4 x i8] c"one\00"

declare i32 @printf(ptr, ...) nounwind
declare i32 @puts(ptr) nounwind

define internal i32 @s64(ptr %p) #0 {
entry:
  %v = load i64, ptr %p
  switch i64 %v, label %def [
    i64 0, label %c0
    i64 1, label %c1
    i64 -1, label %c2
    i64 2147483648, label %c3
    i64 4294967295, label %c4
    i64 4294967296, label %c5
    i64 9223372036854775807, label %c6
    i64 -9223372036854775808, label %c7
    i64 5, label %c8
    i64 4294967301, label %c9
    i64 -4294967291, label %c10
    i64 81985529216486896, label %c11
    i64 -5, label %c12
    i64 100, label %c13
    i64 7, label %c1
    i64 4294967303, label %c2
  ]
c0:
  ret i32 10
c1:
  ret i32 11
c2:
  ret i32 12
c3:
  ret i32 13
c4:
  ret i32 14
c5:
  ret i32 15
c6:
  ret i32 16
c7:
  ret i32 17
c8:
  ret i32 18
c9:
  ret i32 19
c10:
  ret i32 20
c11:
  ret i32 21
c12:
  ret i32 22
c13:
  ret i32 23
def:
  ret i32 99
}

; i48 from the low 48 bits of the value (the slot's bits above stay).
define internal i32 @s48(ptr %p) #0 {
entry:
  %w = load i64, ptr %p
  %v = trunc i64 %w to i48
  br label %sw
sw:
  %phi = phi i32 [ 0, %entry ]
  switch i48 %v, label %def [
    i48 0, label %c0
    i48 -1, label %c1
    i48 140737488355327, label %c2
    i48 -140737488355328, label %c3
    i48 5, label %c4
    i48 4294967301, label %c5
    i48 -5, label %c6
    i48 4294967296, label %c7
  ]
c0:
  ret i32 30
c1:
  ret i32 31
c2:
  ret i32 32
c3:
  ret i32 33
c4:
  ret i32 34
c5:
  ret i32 35
c6:
  %x = add i32 %phi, 36
  ret i32 %x
c7:
  ret i32 37
def:
  ret i32 98
}

define internal i32 @s33(ptr %p) #0 {
entry:
  %w = load i64, ptr %p
  %v = trunc i64 %w to i33
  switch i33 %v, label %def [
    i33 0, label %c0
    i33 -1, label %c1
    i33 4294967295, label %c2
    i33 -4294967296, label %c3
    i33 2, label %c4
    i33 -4294967294, label %c5
  ]
c0:
  ret i32 40
c1:
  ret i32 41
c2:
  ret i32 42
c3:
  ret i32 43
c4:
  ret i32 44
c5:
  ret i32 45
def:
  ret i32 97
}

; Only a default.
define internal i32 @sd(ptr %p) #0 {
entry:
  %v = load i64, ptr %p
  switch i64 %v, label %def [
  ]
def:
  ret i32 96
}

; One case, high word only differs.
define internal i32 @s1(ptr %p) #0 {
entry:
  %v = load i64, ptr %p
  switch i64 %v, label %def [
    i64 8589934592, label %c0
  ]
c0:
  ret i32 50
def:
  ret i32 95
}

; A constant condition.
define internal i32 @sk(ptr %p) #0 {
entry:
  switch i64 4294967301, label %def [
    i64 5, label %c0
    i64 4294967301, label %c1
  ]
c0:
  ret i32 60
c1:
  ret i32 61
def:
  ret i32 94
}

define internal void @show(ptr %name, i32 %i, i32 %r) #0 {
entry:
  %x = call i32 (ptr, ...) @printf(ptr @fmt, ptr %name, i32 %i, i32 %r)
  ret void
}

define i32 @main() #0 {
entry:
  br label %loop
loop:
  %i = phi i32 [ 0, %entry ], [ %i1, %loop ]
  %p = getelementptr [30 x i64], ptr @vals, i32 0, i32 %i
  %r64 = call i32 @s64(ptr %p)
  call void @show(ptr @n64, i32 %i, i32 %r64)
  %r48 = call i32 @s48(ptr %p)
  call void @show(ptr @n48, i32 %i, i32 %r48)
  %r33 = call i32 @s33(ptr %p)
  call void @show(ptr @n33, i32 %i, i32 %r33)
  %rd = call i32 @sd(ptr %p)
  call void @show(ptr @nd, i32 %i, i32 %rd)
  %r1 = call i32 @s1(ptr %p)
  call void @show(ptr @n1, i32 %i, i32 %r1)
  %rk = call i32 @sk(ptr %p)
  call void @show(ptr @nk, i32 %i, i32 %rk)
  %i1 = add i32 %i, 1
  %c = icmp slt i32 %i1, 30
  br i1 %c, label %loop, label %end
end:
  %d = call i32 @puts(ptr @done)
  ret i32 0
}

attributes #0 = { noinline nounwind }
