; Loops whose state lives in phi nodes: a counted sum, two mutually
; dependent phis (Fibonacci), phis that swap each other's values (the
; parallel-copy case), a nested loop, and a loop with two exits.
;@ covers: spec/ir.md §5.4 other (Phi), terminator (CondBr)
;@ tier: quick
;@ unsupported(aarch64): global:function-definition
;@ unsupported(x86_64): global:variable-definition
;@ stdout: sum(1..100) = 5050
;@ stdout: sum(1..1) = 1
;@ stdout: fib(0) = 0
;@ stdout: fib(1) = 1
;@ stdout: fib(30) = 832040
;@ stdout: fib(90) = 2880067194370816120
;@ stdout: swap x1: 1 2
;@ stdout: swap x4: 2 1
;@ stdout: swap x5: 1 2
;@ stdout: nested: pairs=45 products=2025
;@ stdout: find(15) = 2
;@ stdout: find(42) = 5
;@ stdout: find(7) = -1

@.sum = private unnamed_addr constant [17 x i8] c"sum(1..%d) = %d\0A\00"
@.fib = private unnamed_addr constant [16 x i8] c"fib(%d) = %lld\0A\00"
@.swap = private unnamed_addr constant [17 x i8] c"swap x%d: %d %d\0A\00"
@.nest = private unnamed_addr constant [30 x i8] c"nested: pairs=%d products=%d\0A\00"
@.find = private unnamed_addr constant [15 x i8] c"find(%d) = %d\0A\00"
@data = internal constant [6 x i32] [i32 4, i32 8, i32 15, i32 16, i32 23, i32 42]

declare i32 @printf(ptr, ...)

define i32 @sum(i32 %n) {
entry:
  br label %loop
loop:
  %i = phi i32 [ 1, %entry ], [ %i.next, %loop ]
  %acc = phi i32 [ 0, %entry ], [ %acc.next, %loop ]
  %acc.next = add i32 %acc, %i
  %i.next = add i32 %i, 1
  %more = icmp sle i32 %i.next, %n
  br i1 %more, label %loop, label %done
done:
  ret i32 %acc.next
}

define i64 @fib(i32 %n) {
entry:
  %none = icmp eq i32 %n, 0
  br i1 %none, label %done, label %loop
loop:
  %a = phi i64 [ 0, %entry ], [ %b, %loop ]
  %b = phi i64 [ 1, %entry ], [ %c, %loop ]
  %i = phi i32 [ 1, %entry ], [ %i.next, %loop ]
  %c = add i64 %a, %b
  %i.next = add i32 %i, 1
  %more = icmp ult i32 %i, %n
  br i1 %more, label %loop, label %done
done:
  %r = phi i64 [ 0, %entry ], [ %b, %loop ]
  ret i64 %r
}

; Each iteration exchanges x and y: every phi reads the other's old value.
define void @swap(i32 %times) {
entry:
  br label %loop
loop:
  %x = phi i32 [ 1, %entry ], [ %y, %loop ]
  %y = phi i32 [ 2, %entry ], [ %x, %loop ]
  %i = phi i32 [ 0, %entry ], [ %i.next, %loop ]
  %i.next = add i32 %i, 1
  %more = icmp ult i32 %i.next, %times
  br i1 %more, label %loop, label %done
done:
  call i32 (ptr, ...) @printf(ptr @.swap, i32 %times, i32 %x, i32 %y)
  ret void
}

define void @nested(i32 %n) {
entry:
  br label %outer
outer:
  %i = phi i32 [ 0, %entry ], [ %i.next, %outer.latch ]
  %pairs = phi i32 [ 0, %entry ], [ %pairs.out, %outer.latch ]
  %prods = phi i32 [ 0, %entry ], [ %prods.out, %outer.latch ]
  br label %inner
inner:
  %j = phi i32 [ 0, %outer ], [ %j.next, %inner ]
  %pairs.in = phi i32 [ %pairs, %outer ], [ %pairs.next, %inner ]
  %prods.in = phi i32 [ %prods, %outer ], [ %prods.next, %inner ]
  %less = icmp slt i32 %i, %j
  %one = zext i1 %less to i32
  %pairs.next = add i32 %pairs.in, %one
  %p = mul i32 %i, %j
  %prods.next = add i32 %prods.in, %p
  %j.next = add i32 %j, 1
  %inner.more = icmp slt i32 %j.next, %n
  br i1 %inner.more, label %inner, label %outer.latch
outer.latch:
  %pairs.out = phi i32 [ %pairs.next, %inner ]
  %prods.out = phi i32 [ %prods.next, %inner ]
  %i.next = add i32 %i, 1
  %outer.more = icmp slt i32 %i.next, %n
  br i1 %outer.more, label %outer, label %done
done:
  call i32 (ptr, ...) @printf(ptr @.nest, i32 %pairs.out, i32 %prods.out)
  ret void
}

; The index of `key` in @data, or -1: a loop left from two places.
define i32 @find(i32 %key) {
entry:
  br label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %i.next, %next ]
  %slot = getelementptr inbounds [6 x i32], ptr @data, i64 0, i64 %i
  %v = load i32, ptr %slot
  %hit = icmp eq i32 %v, %key
  br i1 %hit, label %done, label %next
next:
  %i.next = add i64 %i, 1
  %more = icmp ult i64 %i.next, 6
  br i1 %more, label %loop, label %done
done:
  %r = phi i64 [ %i, %loop ], [ -1, %next ]
  %r32 = trunc i64 %r to i32
  ret i32 %r32
}

define i32 @main() {
  %s = call i32 @sum(i32 100)
  call i32 (ptr, ...) @printf(ptr @.sum, i32 100, i32 %s)
  %s1 = call i32 @sum(i32 1)
  call i32 (ptr, ...) @printf(ptr @.sum, i32 1, i32 %s1)
  %f0 = call i64 @fib(i32 0)
  call i32 (ptr, ...) @printf(ptr @.fib, i32 0, i64 %f0)
  %f1 = call i64 @fib(i32 1)
  call i32 (ptr, ...) @printf(ptr @.fib, i32 1, i64 %f1)
  %f30 = call i64 @fib(i32 30)
  call i32 (ptr, ...) @printf(ptr @.fib, i32 30, i64 %f30)
  %f90 = call i64 @fib(i32 90)
  call i32 (ptr, ...) @printf(ptr @.fib, i32 90, i64 %f90)
  call void @swap(i32 1)
  call void @swap(i32 4)
  call void @swap(i32 5)
  call void @nested(i32 10)
  %i15 = call i32 @find(i32 15)
  call i32 (ptr, ...) @printf(ptr @.find, i32 15, i32 %i15)
  %i42 = call i32 @find(i32 42)
  call i32 (ptr, ...) @printf(ptr @.find, i32 42, i32 %i42)
  %i7 = call i32 @find(i32 7)
  call i32 (ptr, ...) @printf(ptr @.find, i32 7, i32 %i7)
  ret i32 0
}
