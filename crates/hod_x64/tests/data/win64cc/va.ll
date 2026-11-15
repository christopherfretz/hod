; WP-246: a variadic `win64cc` callee for `calls.ll`, built by `llc`
; only (hod refuses a variadic `win64cc` definition,
; `cc:win64cc-variadic-definition`): `v_sum(n, ...)` adds `n` pairs of an
; `i64` and a `double`, the `i64` times the pair's number. Its `va_list` is
; a pointer that walks the home area and the stack arguments.

declare void @llvm.va_start.p0(ptr)
declare void @llvm.va_end.p0(ptr)

define win64cc double @v_sum(i32 %n, ...) {
entry:
  %ap = alloca ptr
  call void @llvm.va_start.p0(ptr %ap)
  br label %loop
loop:
  %i = phi i32 [ 0, %entry ], [ %i1, %loop ]
  %acc = phi double [ 0.0, %entry ], [ %acc2, %loop ]
  %x = va_arg ptr %ap, i64
  %d = va_arg ptr %ap, double
  %i1 = add i32 %i, 1
  %w = sext i32 %i1 to i64
  %xw = mul i64 %x, %w
  %xf = sitofp i64 %xw to double
  %acc1 = fadd double %acc, %xf
  %acc2 = fadd double %acc1, %d
  %more = icmp slt i32 %i1, %n
  br i1 %more, label %loop, label %done
done:
  call void @llvm.va_end.p0(ptr %ap)
  ret double %acc2
}
