; The convergent attribute, as clang and kiln emit it: on the declaration
; of llvm.is.constant (libc++'s __builtin_constant_p) through an attribute
; group, on a defined function both ways, and on call sites, in a branch
; and in a loop. convergent constrains transformations only, so the code
; is the code without it. llvm.is.constant folds as llc's
; LowerConstantIntrinsics does at -O0: true for a literal operand, false
; for a value computed at run time or a symbol's address.
;@ covers: spec/ir.md §5.7 attributes (convergent), §5.5 intrinsics (is.constant)
;@ stdout: lit 1 1 1 1 1
;@ stdout: run 0 0 0 0 0
;@ stdout: step 0 3
;@ stdout: step 1 4
;@ stdout: step 2 5
;@ stdout: odd 7
;@ stdout: total 24

@.lit = private unnamed_addr constant [20 x i8] c"lit %d %d %d %d %d\0A\00"
@.run = private unnamed_addr constant [20 x i8] c"run %d %d %d %d %d\0A\00"
@.step = private unnamed_addr constant [12 x i8] c"step %d %d\0A\00"
@.odd = private unnamed_addr constant [8 x i8] c"odd %d\0A\00"
@.total = private unnamed_addr constant [10 x i8] c"total %d\0A\00"
@g = global i32 5

declare i32 @printf(ptr, ...)
declare i1 @llvm.is.constant.i1(i1) #0
declare i1 @llvm.is.constant.i32(i32) #0
declare i1 @llvm.is.constant.i64(i64) #0
declare i1 @llvm.is.constant.f64(double) #0
declare i1 @llvm.is.constant.p0(ptr) #0

define i32 @bump(i32 %x) #1 {
  %r = add i32 %x, 3
  ret i32 %r
}

define i32 @twice(i32 %x) convergent {
  %r = shl i32 %x, 1
  ret i32 %r
}

define i32 @zext1(i1 %b) {
  %r = zext i1 %b to i32
  ret i32 %r
}

define i32 @main(i32 %argc, ptr %argv) {
entry:
  %l1 = call i1 @llvm.is.constant.i1(i1 true)
  %l2 = call i1 @llvm.is.constant.i32(i32 7)
  %l3 = call i1 @llvm.is.constant.i64(i64 -1)
  %l4 = call i1 @llvm.is.constant.f64(double 2.5)
  %l5 = call i1 @llvm.is.constant.p0(ptr null)
  %a1 = zext i1 %l1 to i32
  %a2 = zext i1 %l2 to i32
  %a3 = zext i1 %l3 to i32
  %a4 = zext i1 %l4 to i32
  %a5 = zext i1 %l5 to i32
  call i32 (ptr, ...) @printf(ptr @.lit, i32 %a1, i32 %a2, i32 %a3, i32 %a4, i32 %a5)
  %one = icmp eq i32 %argc, 1
  %wide = sext i32 %argc to i64
  %fp = sitofp i32 %argc to double
  %r1 = call i1 @llvm.is.constant.i1(i1 %one) #0
  %r2 = call i1 @llvm.is.constant.i32(i32 %argc) convergent
  %r3 = call i1 @llvm.is.constant.i64(i64 %wide)
  %r4 = call i1 @llvm.is.constant.f64(double %fp)
  %r5 = call i1 @llvm.is.constant.p0(ptr @g)
  %b1 = call i32 @zext1(i1 %r1)
  %b2 = call i32 @zext1(i1 %r2)
  %b3 = call i32 @zext1(i1 %r3)
  %b4 = call i32 @zext1(i1 %r4)
  %b5 = call i32 @zext1(i1 %r5)
  call i32 (ptr, ...) @printf(ptr @.run, i32 %b1, i32 %b2, i32 %b3, i32 %b4, i32 %b5)
  br label %loop

loop:
  %i = phi i32 [ 0, %entry ], [ %next, %loop ]
  %acc = phi i32 [ 0, %entry ], [ %acc.next, %loop ]
  %v = call i32 @bump(i32 %i) convergent
  call i32 (ptr, ...) @printf(ptr @.step, i32 %i, i32 %v)
  %acc.next = add i32 %acc, %v
  %next = add i32 %i, 1
  %done = icmp eq i32 %next, 3
  br i1 %done, label %after, label %loop

after:
  %g.v = load i32, ptr @g
  %odd = and i32 %g.v, 1
  %isodd = icmp ne i32 %odd, 0
  br i1 %isodd, label %then, label %join

then:
  %t = call i32 @twice(i32 %g.v) #0
  %t1 = sub i32 %t, 3
  call i32 (ptr, ...) @printf(ptr @.odd, i32 %t1)
  br label %join

join:
  %extra = phi i32 [ %t1, %then ], [ 0, %after ]
  %sum = add i32 %acc.next, %extra
  %total = add i32 %sum, %g.v
  call i32 (ptr, ...) @printf(ptr @.total, i32 %total)
  ret i32 0
}

attributes #0 = { convergent nocallback nofree nosync nounwind willreturn memory(none) }
attributes #1 = { convergent noinline nounwind optnone }
