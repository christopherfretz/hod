; WP-306: `getelementptr` with i64, i48 and i33 indices on i386 (truncated
; to the 32 bits of a pointer), with high halves that are not zero, scales
; of 1, 4, 8, 12 and 40, two variable indices, a constant index above 2^32.
; Each function returns the byte offset from its base; one line per value.
target datalayout = "e-m:e-p:32:32-p270:32:32-p271:32:32-p272:64:64-i128:128-f64:32:64-f80:32-n8:16:32-S128"
target triple = "i686-unknown-linux-gnu"

%pair = type { i32, [3 x i32], i64 }

@vals = internal global [16 x i64] [i64 0, i64 1, i64 -1, i64 2147483648, i64 4294967295, i64 4294967296, i64 9223372036854775807, i64 -9223372036854775808, i64 4294967299, i64 -4294967293, i64 81985529216486896, i64 -2401053088876216593, i64 281474976710661, i64 12345, i64 -12345, i64 8589934597]
@base = internal global [64 x i8] zeroinitializer
@fmt = private constant [12 x i8] c"%s %d %08x\0A\00"
@done = private constant [5 x i8] c"done\00"
@n1 = private constant [3 x i8] c"i8\00"
@n4 = private constant [4 x i8] c"i32\00"
@n8 = private constant [4 x i8] c"i64\00"
@n12 = private constant [4 x i8] c"a12\00"
@n40 = private constant [4 x i8] c"s40\00"
@n2 = private constant [4 x i8] c"two\00"
@n48 = private constant [4 x i8] c"i48\00"
@n33 = private constant [4 x i8] c"i33\00"
@nk = private constant [4 x i8] c"cst\00"

declare i32 @printf(ptr, ...) nounwind
declare i32 @puts(ptr) nounwind

define internal i32 @off(ptr %b, ptr %q) #0 {
entry:
  %x = ptrtoint ptr %q to i32
  %y = ptrtoint ptr %b to i32
  %d = sub i32 %x, %y
  ret i32 %d
}

define internal i32 @g1(ptr %b, ptr %p, ptr %p2) #0 {
entry:
  %i = load i64, ptr %p
  %q = getelementptr i8, ptr %b, i64 %i
  %d = call i32 @off(ptr %b, ptr %q)
  ret i32 %d
}

define internal i32 @g4(ptr %b, ptr %p, ptr %p2) #0 {
entry:
  %i = load i64, ptr %p
  %q = getelementptr i32, ptr %b, i64 %i
  %d = call i32 @off(ptr %b, ptr %q)
  ret i32 %d
}

define internal i32 @g8(ptr %b, ptr %p, ptr %p2) #0 {
entry:
  %i = load i64, ptr %p
  %q = getelementptr inbounds i64, ptr %b, i64 %i
  %d = call i32 @off(ptr %b, ptr %q)
  ret i32 %d
}

define internal i32 @g12(ptr %b, ptr %p, ptr %p2) #0 {
entry:
  %i = load i64, ptr %p
  %q = getelementptr [3 x i32], ptr %b, i64 %i, i64 2
  %d = call i32 @off(ptr %b, ptr %q)
  ret i32 %d
}

define internal i32 @g40(ptr %b, ptr %p, ptr %p2) #0 {
entry:
  %i = load i64, ptr %p
  %q = getelementptr %pair, ptr %b, i64 %i, i32 2
  %d = call i32 @off(ptr %b, ptr %q)
  ret i32 %d
}

define internal i32 @g2(ptr %b, ptr %p, ptr %p2) #0 {
entry:
  %i = load i64, ptr %p
  %j = load i64, ptr %p2
  %q = getelementptr [5 x i64], ptr %b, i64 %i, i64 %j
  %d = call i32 @off(ptr %b, ptr %q)
  ret i32 %d
}

define internal i32 @g48(ptr %b, ptr %p, ptr %p2) #0 {
entry:
  %w = load i64, ptr %p
  %i = trunc i64 %w to i48
  %q = getelementptr i16, ptr %b, i48 %i
  %d = call i32 @off(ptr %b, ptr %q)
  ret i32 %d
}

define internal i32 @g33(ptr %b, ptr %p, ptr %p2) #0 {
entry:
  %w = load i64, ptr %p
  %i = trunc i64 %w to i33
  %q = getelementptr i32, ptr %b, i33 %i
  %d = call i32 @off(ptr %b, ptr %q)
  ret i32 %d
}

define internal i32 @gk(ptr %b, ptr %p, ptr %p2) #0 {
entry:
  %q = getelementptr i32, ptr %b, i64 4294967299
  %r = getelementptr i8, ptr %q, i64 -4294967295
  %d = call i32 @off(ptr %b, ptr %r)
  ret i32 %d
}

define internal void @each(ptr %name, ptr %f) #0 {
entry:
  br label %loop
loop:
  %i = phi i32 [ 0, %entry ], [ %i1, %loop ]
  %p = getelementptr [16 x i64], ptr @vals, i32 0, i32 %i
  %k = sub i32 15, %i
  %p2 = getelementptr [16 x i64], ptr @vals, i32 0, i32 %k
  %r = call i32 %f(ptr @base, ptr %p, ptr %p2)
  %x = call i32 (ptr, ...) @printf(ptr @fmt, ptr %name, i32 %i, i32 %r)
  %i1 = add i32 %i, 1
  %c = icmp slt i32 %i1, 16
  br i1 %c, label %loop, label %end
end:
  ret void
}

define i32 @main() #0 {
entry:
  call void @each(ptr @n1, ptr @g1)
  call void @each(ptr @n4, ptr @g4)
  call void @each(ptr @n8, ptr @g8)
  call void @each(ptr @n12, ptr @g12)
  call void @each(ptr @n40, ptr @g40)
  call void @each(ptr @n2, ptr @g2)
  call void @each(ptr @n48, ptr @g48)
  call void @each(ptr @n33, ptr @g33)
  call void @each(ptr @nk, ptr @gk)
  %d = call i32 @puts(ptr @done)
  ret i32 0
}

attributes #0 = { noinline nounwind }
