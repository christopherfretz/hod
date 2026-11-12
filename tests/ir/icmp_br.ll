; The ten icmp predicates at i8, i32 and i64 (as values and as branch
; conditions), and conditional branches that form an if/else-if chain.
; Each line prints eq ne ugt uge ult ule sgt sge slt sle as 0 or 1.
;@ covers: spec/ir.md §5.4 compare, terminator (Br, CondBr)
;@ tier: quick
;@ unsupported(x86_64): *
;@ stdout: i8  0100110011
;@ stdout: i8  0111000011
;@ stdout: i8  0111000011
;@ stdout: i32 0100110011
;@ stdout: i32 0111001100
;@ stdout: i32 1001010101
;@ stdout: i32 0111000011
;@ stdout: i64 0100110011
;@ stdout: i64 1001010101
;@ stdout: i64 0111000011
;@ stdout: i64 0111001100
;@ stdout: classify(-5) = negative
;@ stdout: classify(0) = zero
;@ stdout: classify(999) = small
;@ stdout: classify(1000) = big

@.fmt = private unnamed_addr constant [25 x i8] c"%s %d%d%d%d%d%d%d%d%d%d\0A\00"
@.i8 = private unnamed_addr constant [4 x i8] c"i8 \00"
@.i32 = private unnamed_addr constant [4 x i8] c"i32\00"
@.i64 = private unnamed_addr constant [4 x i8] c"i64\00"
@.class = private unnamed_addr constant [21 x i8] c"classify(%lld) = %s\0A\00"
@.neg = private unnamed_addr constant [9 x i8] c"negative\00"
@.zero = private unnamed_addr constant [5 x i8] c"zero\00"
@.small = private unnamed_addr constant [6 x i8] c"small\00"
@.big = private unnamed_addr constant [4 x i8] c"big\00"

declare i32 @printf(ptr, ...)

define void @cmp8(i8 %a, i8 %b) {
  %c0 = icmp eq i8 %a, %b
  %c1 = icmp ne i8 %a, %b
  %c2 = icmp ugt i8 %a, %b
  %c3 = icmp uge i8 %a, %b
  %c4 = icmp ult i8 %a, %b
  %c5 = icmp ule i8 %a, %b
  %c6 = icmp sgt i8 %a, %b
  %c7 = icmp sge i8 %a, %b
  %c8 = icmp slt i8 %a, %b
  %c9 = icmp sle i8 %a, %b
  %z0 = zext i1 %c0 to i32
  %z1 = zext i1 %c1 to i32
  %z2 = zext i1 %c2 to i32
  %z3 = zext i1 %c3 to i32
  %z4 = zext i1 %c4 to i32
  %z5 = zext i1 %c5 to i32
  %z6 = zext i1 %c6 to i32
  %z7 = zext i1 %c7 to i32
  %z8 = zext i1 %c8 to i32
  %z9 = zext i1 %c9 to i32
  call i32 (ptr, ...) @printf(ptr @.fmt, ptr @.i8, i32 %z0, i32 %z1, i32 %z2, i32 %z3, i32 %z4, i32 %z5, i32 %z6, i32 %z7, i32 %z8, i32 %z9)
  ret void
}

define void @cmp32(i32 %a, i32 %b) {
  %c0 = icmp eq i32 %a, %b
  %c1 = icmp ne i32 %a, %b
  %c2 = icmp ugt i32 %a, %b
  %c3 = icmp uge i32 %a, %b
  %c4 = icmp ult i32 %a, %b
  %c5 = icmp ule i32 %a, %b
  %c6 = icmp sgt i32 %a, %b
  %c7 = icmp sge i32 %a, %b
  %c8 = icmp slt i32 %a, %b
  %c9 = icmp sle i32 %a, %b
  %z0 = zext i1 %c0 to i32
  %z1 = zext i1 %c1 to i32
  %z2 = zext i1 %c2 to i32
  %z3 = zext i1 %c3 to i32
  %z4 = zext i1 %c4 to i32
  %z5 = zext i1 %c5 to i32
  %z6 = zext i1 %c6 to i32
  %z7 = zext i1 %c7 to i32
  %z8 = zext i1 %c8 to i32
  %z9 = zext i1 %c9 to i32
  call i32 (ptr, ...) @printf(ptr @.fmt, ptr @.i32, i32 %z0, i32 %z1, i32 %z2, i32 %z3, i32 %z4, i32 %z5, i32 %z6, i32 %z7, i32 %z8, i32 %z9)
  ret void
}

; The same ten predicates, each as the condition of a branch.
define i32 @bit(i64 %a, i64 %b, i32 %pred) {
entry:
  switch i32 %pred, label %no [
    i32 0, label %p0
    i32 1, label %p1
    i32 2, label %p2
    i32 3, label %p3
    i32 4, label %p4
    i32 5, label %p5
    i32 6, label %p6
    i32 7, label %p7
    i32 8, label %p8
    i32 9, label %p9
  ]
p0:
  %c0 = icmp eq i64 %a, %b
  br i1 %c0, label %yes, label %no
p1:
  %c1 = icmp ne i64 %a, %b
  br i1 %c1, label %yes, label %no
p2:
  %c2 = icmp ugt i64 %a, %b
  br i1 %c2, label %yes, label %no
p3:
  %c3 = icmp uge i64 %a, %b
  br i1 %c3, label %yes, label %no
p4:
  %c4 = icmp ult i64 %a, %b
  br i1 %c4, label %yes, label %no
p5:
  %c5 = icmp ule i64 %a, %b
  br i1 %c5, label %yes, label %no
p6:
  %c6 = icmp sgt i64 %a, %b
  br i1 %c6, label %yes, label %no
p7:
  %c7 = icmp sge i64 %a, %b
  br i1 %c7, label %yes, label %no
p8:
  %c8 = icmp slt i64 %a, %b
  br i1 %c8, label %yes, label %no
p9:
  %c9 = icmp sle i64 %a, %b
  br i1 %c9, label %yes, label %no
yes:
  ret i32 1
no:
  ret i32 0
}

define void @cmp64(i64 %a, i64 %b) {
  %z0 = call i32 @bit(i64 %a, i64 %b, i32 0)
  %z1 = call i32 @bit(i64 %a, i64 %b, i32 1)
  %z2 = call i32 @bit(i64 %a, i64 %b, i32 2)
  %z3 = call i32 @bit(i64 %a, i64 %b, i32 3)
  %z4 = call i32 @bit(i64 %a, i64 %b, i32 4)
  %z5 = call i32 @bit(i64 %a, i64 %b, i32 5)
  %z6 = call i32 @bit(i64 %a, i64 %b, i32 6)
  %z7 = call i32 @bit(i64 %a, i64 %b, i32 7)
  %z8 = call i32 @bit(i64 %a, i64 %b, i32 8)
  %z9 = call i32 @bit(i64 %a, i64 %b, i32 9)
  call i32 (ptr, ...) @printf(ptr @.fmt, ptr @.i64, i32 %z0, i32 %z1, i32 %z2, i32 %z3, i32 %z4, i32 %z5, i32 %z6, i32 %z7, i32 %z8, i32 %z9)
  ret void
}

define ptr @classify(i64 %v) {
entry:
  %neg = icmp slt i64 %v, 0
  br i1 %neg, label %is_neg, label %not_neg
is_neg:
  ret ptr @.neg
not_neg:
  %zero = icmp eq i64 %v, 0
  br i1 %zero, label %is_zero, label %positive
is_zero:
  ret ptr @.zero
positive:
  %small = icmp ult i64 %v, 1000
  br i1 %small, label %is_small, label %is_big
is_small:
  ret ptr @.small
is_big:
  ret ptr @.big
}

define void @show(i64 %v) {
  %name = call ptr @classify(i64 %v)
  call i32 (ptr, ...) @printf(ptr @.class, i64 %v, ptr %name)
  ret void
}

define i32 @main() {
  call void @cmp8(i8 1, i8 2)
  call void @cmp8(i8 -1, i8 1)
  call void @cmp8(i8 -128, i8 127)
  call void @cmp32(i32 1, i32 2)
  call void @cmp32(i32 2, i32 1)
  call void @cmp32(i32 5, i32 5)
  call void @cmp32(i32 -1, i32 1)
  call void @cmp64(i64 1, i64 2)
  call void @cmp64(i64 5, i64 5)
  call void @cmp64(i64 -1, i64 1)
  call void @cmp64(i64 4294967296, i64 1)
  call void @show(i64 -5)
  call void @show(i64 0)
  call void @show(i64 999)
  call void @show(i64 1000)
  ret i32 0
}
