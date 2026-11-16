; WP-315: `i64` lengths of the memory intrinsics (constant, variable,
; above 2^32: truncated to the low word as `llc` passes `size_t`),
; volatile and `.inline` forms; integers of 33 to 63 bits passed and
; returned by value, also as variadic arguments, to functions of this
; module and of the C helper (helper.c).
target datalayout = "e-m:e-p:32:32-p270:32:32-p271:32:32-p272:64:64-i128:128-f64:32:64-f80:32-n8:16:32-S128"
target triple = "i686-unknown-linux-gnu"

@buf = internal global [256 x i8] zeroinitializer, align 16
@fmt = private constant [6 x i8] c"%llx\0A\00"
@done = private constant [5 x i8] c"done\00"

declare i32 @printf(ptr, ...)
declare i32 @puts(ptr)
declare i64 @h_mask(i32, i40, i32)
declare i64 @h_vsum(i32, ...)
declare i40 @h_ret(i32)
declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)
declare void @llvm.memmove.p0.p0.i64(ptr, ptr, i64, i1)
declare void @llvm.memset.p0.i64(ptr, i8, i64, i1)
declare void @llvm.memcpy.inline.p0.p0.i64(ptr, ptr, i64, i1)
declare void @llvm.memset.inline.p0.i64(ptr, i8, i64, i1)

define internal void @fill() #0 {
entry:
  br label %loop
loop:
  %i = phi i32 [ 0, %entry ], [ %i1, %loop ]
  %v = mul i32 %i, 13
  %b = trunc i32 %v to i8
  %p = getelementptr i8, ptr @buf, i32 %i
  store i8 %b, ptr %p
  %i1 = add i32 %i, 1
  %c = icmp ult i32 %i1, 256
  br i1 %c, label %loop, label %out
out:
  ret void
}

define internal void @show() #0 {
entry:
  br label %loop
loop:
  %i = phi i32 [ 0, %entry ], [ %i1, %loop ]
  %h = phi i64 [ 5381, %entry ], [ %h2, %loop ]
  %p = getelementptr i8, ptr @buf, i32 %i
  %b = load i8, ptr %p
  %b64 = zext i8 %b to i64
  %h1 = mul i64 %h, 33
  %h2 = add i64 %h1, %b64
  %i1 = add i32 %i, 1
  %c = icmp ult i32 %i1, 256
  br i1 %c, label %loop, label %out
out:
  %r = call i32 (ptr, ...) @printf(ptr @fmt, i64 %h2)
  ret void
}

define internal void @lengths(i64 %n, i40 %m) #0 {
entry:
  call void @fill()
  call void @llvm.memcpy.p0.p0.i64(ptr getelementptr (i8, ptr @buf, i32 100), ptr @buf, i64 %n, i1 false)
  call void @show()
  call void @fill()
  call void @llvm.memmove.p0.p0.i64(ptr getelementptr (i8, ptr @buf, i32 3), ptr @buf, i64 %n, i1 true)
  call void @show()
  call void @fill()
  call void @llvm.memmove.p0.p0.i64(ptr @buf, ptr getelementptr (i8, ptr @buf, i32 3), i64 %n, i1 false)
  call void @show()
  call void @fill()
  %m64 = zext i40 %m to i64
  call void @llvm.memset.p0.i64(ptr getelementptr (i8, ptr @buf, i32 7), i8 -86, i64 %m64, i1 false)
  call void @show()
  ret void
}

define internal i40 @add40(i40 %a, i33 %b, i8 %c) #0 {
  %b1 = sext i33 %b to i40
  %c1 = zext i8 %c to i40
  %s = add i40 %a, %b1
  %r = add i40 %s, %c1
  ret i40 %r
}

define internal i56 @twice56(i56 %x, i63 %y) #0 {
  %y1 = trunc i63 %y to i56
  %r = add i56 %x, %y1
  ret i56 %r
}

define i32 @main() #0 {
entry:
  call void @lengths(i64 0, i40 0)
  call void @lengths(i64 37, i40 70)
  call void @lengths(i64 4294967336, i40 4294967360)
  call void @fill()
  call void @llvm.memcpy.p0.p0.i64(ptr @buf, ptr getelementptr (i8, ptr @buf, i32 128), i64 4294967297, i1 false)
  call void @show()
  call void @llvm.memset.p0.i64(ptr @buf, i8 1, i64 4294967304, i1 true)
  call void @show()
  call void @llvm.memcpy.inline.p0.p0.i64(ptr getelementptr (i8, ptr @buf, i32 9), ptr @buf, i64 100, i1 false)
  call void @show()
  call void @llvm.memset.inline.p0.i64(ptr @buf, i8 9, i64 200, i1 false)
  call void @show()
  %a = call i40 @add40(i40 -5, i33 -4294967296, i8 200)
  %a64 = zext i40 %a to i64
  %p1 = call i32 (ptr, ...) @printf(ptr @fmt, i64 %a64)
  %t = call i56 @twice56(i56 72057594037927935, i63 -3)
  %t64 = zext i56 %t to i64
  %p2 = call i32 (ptr, ...) @printf(ptr @fmt, i64 %t64)
  %h = call i64 @h_mask(i32 40, i40 %a, i32 7)
  %p3 = call i32 (ptr, ...) @printf(ptr @fmt, i64 %h)
  %v = call i64 (i32, ...) @h_vsum(i32 3, i32 40, i40 %a, i32 56, i56 %t, i32 33, i33 -1)
  %p4 = call i32 (ptr, ...) @printf(ptr @fmt, i64 %v)
  %r = call i40 @h_ret(i32 9)
  %r64 = zext i40 %r to i64
  %p5 = call i32 (ptr, ...) @printf(ptr @fmt, i64 %r64)
  %d = call i32 @puts(ptr @done)
  ret i32 0
}

attributes #0 = { nounwind }
