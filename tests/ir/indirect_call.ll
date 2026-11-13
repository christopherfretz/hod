; Function pointers: passed as arguments, returned from functions, stored
; and loaded, compared, and called with integer, pointer and
; floating-point signatures.
;@ covers: spec/ir.md §5.4 call (Call through a value)
;@ stdout: apply(inc, 5) = 7
;@ stdout: apply(dbl, 5) = 20
;@ stdout: fold(add) = 15
;@ stdout: fold(mul) = 120
;@ stdout: pick(0)(10, 3) = 13
;@ stdout: pick(1)(10, 3) = 7
;@ stdout: pick(2)(10, 3) = 30
;@ stdout: same: 1 0
;@ stdout: scaled: 3.125

@.apply = private unnamed_addr constant [20 x i8] c"apply(%s, %d) = %d\0A\00"
@.fold = private unnamed_addr constant [15 x i8] c"fold(%s) = %d\0A\00"
@.pick = private unnamed_addr constant [22 x i8] c"pick(%d)(10, 3) = %d\0A\00"
@.same = private unnamed_addr constant [13 x i8] c"same: %d %d\0A\00"
@.fp = private unnamed_addr constant [14 x i8] c"scaled: %.3f\0A\00"
@.inc = private unnamed_addr constant [4 x i8] c"inc\00"
@.dbl = private unnamed_addr constant [4 x i8] c"dbl\00"
@.add = private unnamed_addr constant [4 x i8] c"add\00"
@.mul = private unnamed_addr constant [4 x i8] c"mul\00"
@values = internal constant [5 x i32] [i32 1, i32 2, i32 3, i32 4, i32 5]

declare i32 @printf(ptr, ...)

define i32 @inc(i32 %v) {
  %r = add i32 %v, 1
  ret i32 %r
}

define i32 @dbl(i32 %v) {
  %r = shl i32 %v, 1
  ret i32 %r
}

define i32 @add(i32 %a, i32 %b) {
  %r = add i32 %a, %b
  ret i32 %r
}

define i32 @sub(i32 %a, i32 %b) {
  %r = sub i32 %a, %b
  ret i32 %r
}

define i32 @mul(i32 %a, i32 %b) {
  %r = mul i32 %a, %b
  ret i32 %r
}

define double @scale(double %v, double %by) {
  %r = fmul double %v, %by
  ret double %r
}

; Calls %f twice.
define i32 @apply_twice(ptr %f, i32 %v) {
  %once = call i32 %f(i32 %v)
  %twice = call i32 %f(i32 %once)
  ret i32 %twice
}

define i32 @fold(ptr %f, i32 %start) {
entry:
  br label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %next, %loop ]
  %acc = phi i32 [ %start, %entry ], [ %acc.next, %loop ]
  %slot = getelementptr inbounds [5 x i32], ptr @values, i64 0, i64 %i
  %v = load i32, ptr %slot
  %acc.next = call i32 %f(i32 %acc, i32 %v)
  %next = add i64 %i, 1
  %more = icmp ult i64 %next, 5
  br i1 %more, label %loop, label %done
done:
  ret i32 %acc.next
}

; Returns one of three functions.
define ptr @pick(i32 %which) {
entry:
  switch i32 %which, label %other [
    i32 0, label %first
    i32 1, label %second
  ]
first:
  ret ptr @add
second:
  ret ptr @sub
other:
  ret ptr @mul
}

define void @show_pick(i32 %which) {
  %slot = alloca ptr
  %f = call ptr @pick(i32 %which)
  store ptr %f, ptr %slot
  %again = load ptr, ptr %slot
  %r = call i32 %again(i32 10, i32 3)
  call i32 (ptr, ...) @printf(ptr @.pick, i32 %which, i32 %r)
  ret void
}

define double @through(ptr %f, double %v) {
  %r = call double %f(double %v, double 2.5)
  ret double %r
}

define i32 @main() {
  %a1 = call i32 @apply_twice(ptr @inc, i32 5)
  call i32 (ptr, ...) @printf(ptr @.apply, ptr @.inc, i32 5, i32 %a1)
  %a2 = call i32 @apply_twice(ptr @dbl, i32 5)
  call i32 (ptr, ...) @printf(ptr @.apply, ptr @.dbl, i32 5, i32 %a2)
  %f1 = call i32 @fold(ptr @add, i32 0)
  call i32 (ptr, ...) @printf(ptr @.fold, ptr @.add, i32 %f1)
  %f2 = call i32 @fold(ptr @mul, i32 1)
  call i32 (ptr, ...) @printf(ptr @.fold, ptr @.mul, i32 %f2)
  call void @show_pick(i32 0)
  call void @show_pick(i32 1)
  call void @show_pick(i32 2)
  %p0 = call ptr @pick(i32 0)
  %p7 = call ptr @pick(i32 7)
  %eq1 = icmp eq ptr %p0, @add
  %eq2 = icmp eq ptr %p7, @add
  %z1 = zext i1 %eq1 to i32
  %z2 = zext i1 %eq2 to i32
  call i32 (ptr, ...) @printf(ptr @.same, i32 %z1, i32 %z2)
  %s = call double @through(ptr @scale, double 1.25)
  call i32 (ptr, ...) @printf(ptr @.fp, double %s)
  ret i32 0
}
