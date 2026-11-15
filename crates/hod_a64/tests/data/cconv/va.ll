; WP-247 (`spec/abi-a64.md` §13.3): variadic `win64cc` functions, built by
; `llc` only (hod refuses the definition, `cc:win64cc-variadic-definition`).
; Their `va_list` is a pointer to 8-byte slots, the general registers'
; home area followed by the stack arguments: a `float` or a `double` is
; there as the integer of its bits. Each reads `n` triples of an `i64`, a
; `double` and a `float`.

declare void @llvm.va_start.p0(ptr)
declare void @llvm.va_end.p0(ptr)

define internal double @walk(ptr %p0, i64 %n, double %start) {
entry:
  br label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %i1, %loop ]
  %p = phi ptr [ %p0, %entry ], [ %pn, %loop ]
  %acc = phi double [ %start, %entry ], [ %acc3, %loop ]
  %iv = load i64, ptr %p
  %pd = getelementptr i8, ptr %p, i64 8
  %dv = load double, ptr %pd
  %pf = getelementptr i8, ptr %p, i64 16
  %fv = load float, ptr %pf
  %pn = getelementptr i8, ptr %p, i64 24
  %ivd = sitofp i64 %iv to double
  %acc1 = fadd double %acc, %ivd
  %acc1s = fmul double %acc1, 2.0
  %acc2 = fadd double %acc1s, %dv
  %fvd = fpext float %fv to double
  %acc3 = fadd double %acc2, %fvd
  %i1 = add i64 %i, 1
  %done = icmp uge i64 %i1, %n
  br i1 %done, label %exit, label %loop
exit:
  ret double %acc3
}

define win64cc double @v_sum(i64 %n, ...) {
  %ap = alloca ptr
  call void @llvm.va_start.p0(ptr %ap)
  %p = load ptr, ptr %ap
  %r = call double @walk(ptr %p, i64 %n, double 0.0)
  call void @llvm.va_end.p0(ptr %ap)
  ret double %r
}

; A fixed `double` first: in `x0`, as every floating-point argument of a
; variadic call.
define win64cc double @v_sum2(double %first, i64 %n, ...) {
  %ap = alloca ptr
  call void @llvm.va_start.p0(ptr %ap)
  %p = load ptr, ptr %ap
  %start = fmul double %first, 10.0
  %r = call double @walk(ptr %p, i64 %n, double %start)
  call void @llvm.va_end.p0(ptr %ap)
  ret double %r
}
