; select on integers, pointers and floating-point values, with conditions
; computed at run time and with constant arms.
;@ covers: spec/ir.md §5.4 other (Select)
;@ unsupported: codegen-not-implemented
;@ stdout: max(3,7)=7 min=3 abs(3)=3
;@ stdout: max(-9,-20)=-9 min=-20 abs(-9)=9
;@ stdout: max(-2147483647,0)=0 min=-2147483647 abs(-2147483647)=2147483647
;@ stdout: clamp(5)=5
;@ stdout: clamp(-5000000000)=-100
;@ stdout: clamp(5000000000)=100
;@ stdout: 7 is odd
;@ stdout: 10 is even
;@ stdout: fmax(1.50,2.50)=2.50 sign(1.50)=1
;@ stdout: fmax(-3.25,-8.00)=-3.25 sign(-3.25)=-1
;@ stdout: fmax(0.00,-0.50)=0.00 sign(0.00)=0

@.ints = private unnamed_addr constant [33 x i8] c"max(%d,%d)=%d min=%d abs(%d)=%d\0A\00"
@.wide = private unnamed_addr constant [18 x i8] c"clamp(%lld)=%lld\0A\00"
@.ptrs = private unnamed_addr constant [10 x i8] c"%d is %s\0A\00"
@.odd = private unnamed_addr constant [4 x i8] c"odd\00"
@.even = private unnamed_addr constant [5 x i8] c"even\00"
@.fp = private unnamed_addr constant [36 x i8] c"fmax(%.2f,%.2f)=%.2f sign(%.2f)=%d\0A\00"

declare i32 @printf(ptr, ...)

define i32 @max(i32 %a, i32 %b) {
  %c = icmp sgt i32 %a, %b
  %r = select i1 %c, i32 %a, i32 %b
  ret i32 %r
}

define i32 @min(i32 %a, i32 %b) {
  %c = icmp slt i32 %a, %b
  %r = select i1 %c, i32 %a, i32 %b
  ret i32 %r
}

define i32 @abs(i32 %a) {
  %c = icmp slt i32 %a, 0
  %n = sub i32 0, %a
  %r = select i1 %c, i32 %n, i32 %a
  ret i32 %r
}

; Limits %v to [-100, 100] with constant arms.
define i64 @clamp(i64 %v) {
  %low = icmp slt i64 %v, -100
  %a = select i1 %low, i64 -100, i64 %v
  %high = icmp sgt i64 %a, 100
  %b = select i1 %high, i64 100, i64 %a
  ret i64 %b
}

define ptr @parity(i32 %v) {
  %bit = and i32 %v, 1
  %c = icmp ne i32 %bit, 0
  %r = select i1 %c, ptr @.odd, ptr @.even
  ret ptr %r
}

define double @fmax(double %a, double %b) {
  %c = fcmp ogt double %a, %b
  %r = select i1 %c, double %a, double %b
  ret double %r
}

; -1, 0 or 1: a select whose arm is another select.
define i32 @sign(double %a) {
  %neg = fcmp olt double %a, 0.0
  %pos = fcmp ogt double %a, 0.0
  %p = select i1 %pos, i32 1, i32 0
  %r = select i1 %neg, i32 -1, i32 %p
  ret i32 %r
}

define void @ints(i32 %a, i32 %b) {
  %mx = call i32 @max(i32 %a, i32 %b)
  %mn = call i32 @min(i32 %a, i32 %b)
  %ab = call i32 @abs(i32 %a)
  call i32 (ptr, ...) @printf(ptr @.ints, i32 %a, i32 %b, i32 %mx, i32 %mn, i32 %a, i32 %ab)
  ret void
}

define void @wide(i64 %v) {
  %r = call i64 @clamp(i64 %v)
  call i32 (ptr, ...) @printf(ptr @.wide, i64 %v, i64 %r)
  ret void
}

define void @ptrs(i32 %v) {
  %r = call ptr @parity(i32 %v)
  call i32 (ptr, ...) @printf(ptr @.ptrs, i32 %v, ptr %r)
  ret void
}

define void @fp(double %a, double %b) {
  %m = call double @fmax(double %a, double %b)
  %s = call i32 @sign(double %a)
  call i32 (ptr, ...) @printf(ptr @.fp, double %a, double %b, double %m, double %a, i32 %s)
  ret void
}

define i32 @main() {
  call void @ints(i32 3, i32 7)
  call void @ints(i32 -9, i32 -20)
  call void @ints(i32 -2147483647, i32 0)
  call void @wide(i64 5)
  call void @wide(i64 -5000000000)
  call void @wide(i64 5000000000)
  call void @ptrs(i32 7)
  call void @ptrs(i32 10)
  call void @fp(double 1.5, double 2.5)
  call void @fp(double -3.25, double -8.0)
  call void @fp(double 0.0, double -0.5)
  ret i32 0
}
