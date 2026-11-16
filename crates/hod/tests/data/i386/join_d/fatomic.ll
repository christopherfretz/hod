; `atomicrmw fadd` / `fsub` of `float` and `double` (i386 join D, the
; small work WP-316 left): `llc`'s loop, a `lock cmpxchg` (`cmpxchg8b` for
; `double`) around an x87 operation whose result is stored rounded to the
; type. Values that round (0.1 steps, a `double` sum rounded from 64 to
; 53 bits), signed zeros, infinities, a quiet NaN; the old value used and
; unused; an x87 product computed before the loop and used after it (the
; operation is a barrier, `spec/codegen-x86.md` §11, §12.4).
target datalayout = "e-m:e-p:32:32-p270:32:32-p271:32:32-p272:64:64-i128:128-f64:32:64-f80:32-n8:16:32-S128"
target triple = "i686-unknown-linux-gnu"

@f = global float 0.0
@d = global double 0.0
@fmt = private constant [36 x i8] c"%s %a %a | %a %a | %08x %016llx %a\0A\00"
@n.add = private constant [4 x i8] c"add\00"
@n.sub = private constant [4 x i8] c"sub\00"
@n.sum = private constant [4 x i8] c"sum\00"

declare i32 @printf(ptr, ...)

define void @step(ptr %name, float %x, double %y, i1 %sub) noinline {
entry:
  %k = fmul double %y, 3.0
  br i1 %sub, label %s, label %a
a:
  %fa = atomicrmw fadd ptr @f, float %x seq_cst
  %da = atomicrmw fadd ptr @d, double %y seq_cst
  br label %j
s:
  %fs = atomicrmw fsub ptr @f, float %x acq_rel
  %ds = atomicrmw fsub ptr @d, double %y monotonic
  br label %j
j:
  %fo = phi float [ %fa, %a ], [ %fs, %s ]
  %do = phi double [ %da, %a ], [ %ds, %s ]
  %fn = load atomic float, ptr @f seq_cst, align 4
  %dn = load atomic double, ptr @d seq_cst, align 8
  %fbits = load i32, ptr @f
  %dbits = load i64, ptr @d
  %fo.d = fpext float %fo to double
  %fn.d = fpext float %fn to double
  call i32 (ptr, ...) @printf(ptr @fmt, ptr %name, double %fo.d, double %do, double %fn.d, double %dn, i32 %fbits, i64 %dbits, double %k)
  ret void
}

; Results unused; a product live across the loops.
define double @sum(i32 %n) noinline {
entry:
  %p = fmul double 1.25, 3.0
  br label %loop
loop:
  %i = phi i32 [ 0, %entry ], [ %i.next, %loop ]
  %x = sitofp i32 %i to float
  %y = fdiv float %x, 10.0
  %0 = atomicrmw fadd ptr @f, float %y seq_cst
  %yd = fpext float %y to double
  %1 = atomicrmw fsub ptr @d, double %yd seq_cst
  %i.next = add i32 %i, 1
  %done = icmp eq i32 %i.next, %n
  br i1 %done, label %exit, label %loop
exit:
  %q = fadd double %p, 1.0
  ret double %q
}

define i32 @main() {
  store float 0x3FB99999A0000000, ptr @f
  store double 0x3FB999999999999A, ptr @d
  call void @step(ptr @n.add, float 0x3FC99999A0000000, double 0x3FC999999999999A, i1 false)
  call void @step(ptr @n.add, float 0x4415AF1D80000000, double 1.0e+300, i1 false)
  call void @step(ptr @n.sub, float 0x4415AF1D80000000, double 1.0e+300, i1 true)
  store float -0.0, ptr @f
  store double -0.0, ptr @d
  call void @step(ptr @n.add, float -0.0, double -0.0, i1 false)
  call void @step(ptr @n.sub, float 0.0, double 0.0, i1 true)
  call void @step(ptr @n.add, float 0x7FF0000000000000, double 0x7FF0000000000000, i1 false)
  call void @step(ptr @n.sub, float 0x7FF0000000000000, double 0x7FF0000000000000, i1 true)
  store float 1.0, ptr @f
  store double 0x3FF0000000000001, ptr @d
  call void @step(ptr @n.add, float 0x3E70000000000000, double 0x3CA0000000000000, i1 false)
  store float 0.0, ptr @f
  store double 0.0, ptr @d
  %r = call double @sum(i32 1000)
  call void @step(ptr @n.sum, float 0.0, double %r, i1 false)
  ret i32 0
}
