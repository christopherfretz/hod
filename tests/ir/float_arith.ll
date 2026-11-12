; fadd, fsub, fmul, fdiv, frem and fneg on double and float, with operands
; that arrive as arguments so nothing folds: exact results, results that
; round, single precision losing what double keeps, infinities from
; division by zero, and a loop that accumulates rounding error. frem is a
; call to fmod/fmodf, hence the math library.
;@ covers: spec/ir.md §5.4 float
;@ tier: quick
;@ link: m
;@ unsupported: *
;@ stdout: double: add=9.7500 sub=5.2500 mul=16.8750 div=3.333333 rem=0.7500 neg=-7.5000
;@ stdout: double: add=-9999999997.0000 sub=-10000000003.0000 mul=-30000000000.0000 div=-3333333333.333333 rem=-1.0000 neg=10000000000.0000
;@ stdout: float: add=1.7500 sub=1.2500 mul=0.3750 div=6.000000 rem=0.0000 neg=-1.5000
;@ stdout: float: add=14.0000 sub=6.0000 mul=40.0000 div=2.500000 rem=2.0000 neg=-10.0000
;@ stdout: float: add=16777216.0000 sub=16777215.0000 mul=16777216.0000 div=16777216.000000 rem=0.0000 neg=-16777216.0000
;@ stdout: by zero: inf -inf
;@ stdout: ten tenths: float 1.000000119 double 0.99999999999999989
;@ stdout: poly(2.00)=-2.0000
;@ stdout: poly(-1.50)=-21.2500

@.d = private unnamed_addr constant [63 x i8] c"double: add=%.4f sub=%.4f mul=%.4f div=%.6f rem=%.4f neg=%.4f\0A\00"
@.f = private unnamed_addr constant [62 x i8] c"float: add=%.4f sub=%.4f mul=%.4f div=%.6f rem=%.4f neg=%.4f\0A\00"
@.inf = private unnamed_addr constant [16 x i8] c"by zero: %f %f\0A\00"
@.sum = private unnamed_addr constant [37 x i8] c"ten tenths: float %.9f double %.17f\0A\00"
@.poly = private unnamed_addr constant [17 x i8] c"poly(%.2f)=%.4f\0A\00"

declare i32 @printf(ptr, ...)

define void @doubles(double %a, double %b) {
  %add = fadd double %a, %b
  %sub = fsub double %a, %b
  %mul = fmul double %a, %b
  %div = fdiv double %a, %b
  %rem = frem double %a, %b
  %neg = fneg double %a
  call i32 (ptr, ...) @printf(ptr @.d, double %add, double %sub, double %mul, double %div, double %rem, double %neg)
  ret void
}

define void @floats(float %a, float %b) {
  %add = fadd float %a, %b
  %sub = fsub float %a, %b
  %mul = fmul float %a, %b
  %div = fdiv float %a, %b
  %rem = frem float %a, %b
  %neg = fneg float %a
  %add.d = fpext float %add to double
  %sub.d = fpext float %sub to double
  %mul.d = fpext float %mul to double
  %div.d = fpext float %div to double
  %rem.d = fpext float %rem to double
  %neg.d = fpext float %neg to double
  call i32 (ptr, ...) @printf(ptr @.f, double %add.d, double %sub.d, double %mul.d, double %div.d, double %rem.d, double %neg.d)
  ret void
}

define void @by_zero(double %a, double %zero) {
  %pos = fdiv double %a, %zero
  %na = fneg double %a
  %neg = fdiv double %na, %zero
  call i32 (ptr, ...) @printf(ptr @.inf, double %pos, double %neg)
  ret void
}

; Adds a tenth ten times, in both precisions.
define void @tenths(float %tf, double %td) {
entry:
  br label %loop
loop:
  %i = phi i32 [ 0, %entry ], [ %i.next, %loop ]
  %sf = phi float [ 0.0, %entry ], [ %sf.next, %loop ]
  %sd = phi double [ 0.0, %entry ], [ %sd.next, %loop ]
  %sf.next = fadd float %sf, %tf
  %sd.next = fadd double %sd, %td
  %i.next = add i32 %i, 1
  %done = icmp eq i32 %i.next, 10
  br i1 %done, label %exit, label %loop
exit:
  %sf.d = fpext float %sf.next to double
  call i32 (ptr, ...) @printf(ptr @.sum, double %sf.d, double %sd.next)
  ret void
}

; 2x^3 - 3x^2 + 0.5x - 7 by Horner's rule; every step is exact for the
; arguments used.
define double @poly(double %x) {
  %t0 = fmul double %x, 2.0
  %t1 = fsub double %t0, 3.0
  %t2 = fmul double %t1, %x
  %t3 = fadd double %t2, 0.5
  %t4 = fmul double %t3, %x
  %t5 = fsub double %t4, 7.0
  ret double %t5
}

define i32 @main() {
  call void @doubles(double 7.5, double 2.25)
  call void @doubles(double -1.0e10, double 3.0)
  call void @floats(float 1.5, float 0.25)
  call void @floats(float 10.0, float 4.0)
  call void @floats(float 16777216.0, float 1.0)
  call void @by_zero(double 1.0, double 0.0)
  call void @tenths(float 0x3FB99999A0000000, double 0x3FB999999999999A)
  %p1 = call double @poly(double 2.0)
  call i32 (ptr, ...) @printf(ptr @.poly, double 2.0, double %p1)
  %p2 = call double @poly(double -1.5)
  call i32 (ptr, ...) @printf(ptr @.poly, double -1.5, double %p2)
  ret i32 0
}
