; Recursion: one self-call (factorial), two (Fibonacci), nested calls as
; arguments (Ackermann), mutual recursion, and a deep chain that needs
; a real stack frame per call.
;@ covers: spec/ir.md §5.4 call (Call), terminator (Ret)
;@ tier: quick
;@ stdout: fact(1) = 1
;@ stdout: fact(20) = 2432902008176640000
;@ stdout: fib(20) = 6765
;@ stdout: ack(2, 3) = 9
;@ stdout: ack(3, 3) = 61
;@ stdout: gcd(1071, 462) = 21
;@ stdout: even(10) = 1
;@ stdout: even(7) = 0
;@ stdout: depth(10000) = 50005000

@.fact = private unnamed_addr constant [17 x i8] c"fact(%d) = %lld\0A\00"
@.fib = private unnamed_addr constant [14 x i8] c"fib(%d) = %d\0A\00"
@.ack = private unnamed_addr constant [18 x i8] c"ack(%d, %d) = %d\0A\00"
@.gcd = private unnamed_addr constant [18 x i8] c"gcd(%d, %d) = %d\0A\00"
@.even = private unnamed_addr constant [15 x i8] c"even(%d) = %d\0A\00"
@.depth = private unnamed_addr constant [18 x i8] c"depth(%d) = %lld\0A\00"

declare i32 @printf(ptr, ...)

define i64 @fact(i32 %n) {
entry:
  %base = icmp sle i32 %n, 1
  br i1 %base, label %one, label %more
one:
  ret i64 1
more:
  %m = sub i32 %n, 1
  %rest = call i64 @fact(i32 %m)
  %wide = sext i32 %n to i64
  %r = mul i64 %wide, %rest
  ret i64 %r
}

define i32 @fib(i32 %n) {
entry:
  %base = icmp slt i32 %n, 2
  br i1 %base, label %leaf, label %more
leaf:
  ret i32 %n
more:
  %n1 = sub i32 %n, 1
  %n2 = sub i32 %n, 2
  %a = call i32 @fib(i32 %n1)
  %b = call i32 @fib(i32 %n2)
  %r = add i32 %a, %b
  ret i32 %r
}

define i32 @ack(i32 %m, i32 %n) {
entry:
  %m0 = icmp eq i32 %m, 0
  br i1 %m0, label %case0, label %m.pos
case0:
  %r0 = add i32 %n, 1
  ret i32 %r0
m.pos:
  %m1 = sub i32 %m, 1
  %n0 = icmp eq i32 %n, 0
  br i1 %n0, label %case1, label %case2
case1:
  %r1 = call i32 @ack(i32 %m1, i32 1)
  ret i32 %r1
case2:
  %n1 = sub i32 %n, 1
  %inner = call i32 @ack(i32 %m, i32 %n1)
  %r2 = call i32 @ack(i32 %m1, i32 %inner)
  ret i32 %r2
}

define i32 @gcd(i32 %a, i32 %b) {
entry:
  %done = icmp eq i32 %b, 0
  br i1 %done, label %out, label %more
out:
  ret i32 %a
more:
  %rem = urem i32 %a, %b
  %r = tail call i32 @gcd(i32 %b, i32 %rem)
  ret i32 %r
}

define i32 @is_even(i32 %n) {
entry:
  %zero = icmp eq i32 %n, 0
  br i1 %zero, label %yes, label %more
yes:
  ret i32 1
more:
  %m = sub i32 %n, 1
  %r = call i32 @is_odd(i32 %m)
  ret i32 %r
}

define i32 @is_odd(i32 %n) {
entry:
  %zero = icmp eq i32 %n, 0
  br i1 %zero, label %no, label %more
no:
  ret i32 0
more:
  %m = sub i32 %n, 1
  %r = call i32 @is_even(i32 %m)
  ret i32 %r
}

; Each level keeps its own value in memory across the call below it.
define i64 @depth(i32 %n) {
entry:
  %slot = alloca i64
  %wide = sext i32 %n to i64
  store i64 %wide, ptr %slot
  %bottom = icmp eq i32 %n, 0
  br i1 %bottom, label %out, label %more
out:
  ret i64 0
more:
  %m = sub i32 %n, 1
  %below = call i64 @depth(i32 %m)
  %mine = load i64, ptr %slot
  %r = add i64 %below, %mine
  ret i64 %r
}

define i32 @main() {
  %f1 = call i64 @fact(i32 1)
  call i32 (ptr, ...) @printf(ptr @.fact, i32 1, i64 %f1)
  %f20 = call i64 @fact(i32 20)
  call i32 (ptr, ...) @printf(ptr @.fact, i32 20, i64 %f20)
  %b20 = call i32 @fib(i32 20)
  call i32 (ptr, ...) @printf(ptr @.fib, i32 20, i32 %b20)
  %a23 = call i32 @ack(i32 2, i32 3)
  call i32 (ptr, ...) @printf(ptr @.ack, i32 2, i32 3, i32 %a23)
  %a33 = call i32 @ack(i32 3, i32 3)
  call i32 (ptr, ...) @printf(ptr @.ack, i32 3, i32 3, i32 %a33)
  %g = call i32 @gcd(i32 1071, i32 462)
  call i32 (ptr, ...) @printf(ptr @.gcd, i32 1071, i32 462, i32 %g)
  %e10 = call i32 @is_even(i32 10)
  call i32 (ptr, ...) @printf(ptr @.even, i32 10, i32 %e10)
  %e7 = call i32 @is_even(i32 7)
  call i32 (ptr, ...) @printf(ptr @.even, i32 7, i32 %e7)
  %d = call i64 @depth(i32 10000)
  call i32 (ptr, ...) @printf(ptr @.depth, i32 10000, i64 %d)
  ret i32 0
}
