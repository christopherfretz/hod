; WP-324: the guard check before a `musttail` exit on i386, with arguments
; in `ecx` and `edx` (`fastcc`) and on the stack, and a protected function
; returning a `float` computed after a call (x87: rounded at the return).
target triple = "i686-unknown-linux-gnu"

@fmt = private constant [16 x i8] c"%d %d %d %d %a\0A\00"

declare i32 @printf(ptr, ...)

define internal void @fill(ptr %p, i32 %n) {
entry:
  br label %loop
loop:
  %i = phi i32 [ 0, %entry ], [ %next, %loop ]
  %at = getelementptr i8, ptr %p, i32 %i
  %c = trunc i32 %i to i8
  store i8 %c, ptr %at
  %next = add i32 %i, 1
  %done = icmp eq i32 %next, %n
  br i1 %done, label %out, label %loop
out:
  ret void
}

define internal fastcc i32 @sum(i32 %a, i32 %b) {
  %ab = add i32 %a, %b
  %r = mul i32 %ab, %b
  ret i32 %r
}

; `ecx` and `edx` hold the arguments at the tail exit (stack arguments
; together with `ecx` are `x86:abi:musttail`).
define internal fastcc i32 @guarded(i32 %a, i32 %b) sspreq {
  %buf = alloca [16 x i8]
  call void @fill(ptr %buf, i32 16)
  %at = getelementptr i8, ptr %buf, i32 5
  %v = load i8, ptr %at
  %w = zext i8 %v to i32
  %a2 = add i32 %a, %w
  %r = musttail call fastcc i32 @sum(i32 %a2, i32 %b)
  ret i32 %r
}

define internal i32 @plain_tail(i32 %a, i32 %b) sspstrong {
  %buf = alloca [8 x i8]
  call void @fill(ptr %buf, i32 8)
  %r = musttail call i32 @plain_target(i32 %a, i32 %b)
  ret i32 %r
}

define internal i32 @plain_target(i32 %a, i32 %b) {
  %r = sub i32 %a, %b
  ret i32 %r
}

define internal float @fmul_after_call(float %x, float %y) sspstrong {
  %buf = alloca [8 x i8]
  call void @fill(ptr %buf, i32 8)
  %p = fmul float %x, %y
  %q = fadd float %p, %y
  ret float %q
}

; A slot that does not escape: hod protects the function, `llc` does not
; (no array, no address taken), so the result returns at 80 bits.
define internal float @scalar_slot(float %x, float %y) sspstrong {
  %s = alloca float
  store float %x, ptr %s
  %v = load float, ptr %s
  %p = fmul float %v, %y
  %q = fadd float %p, %y
  ret float %q
}

; `ssp` and a 4-byte escaping slot: `llc` protects no array below the
; buffer size (8), so 80 bits again.
define internal float @small_ssp(float %x, float %y) ssp {
  %b = alloca [4 x i8]
  call void @fill(ptr %b, i32 4)
  %p = fmul float %x, %y
  %q = fadd float %p, %y
  ret float %q
}

@fmt2 = private constant [10 x i8] c"%a %a %a\0A\00"

define i32 @main() {
  ; Each result widened before the next call, which would spill it.
  %s1 = call float @scalar_slot(float 0x3FF19999A0000000, float 0x400A666660000000)
  %d1 = fpext float %s1 to double
  %s2 = call float @small_ssp(float 0x3FF19999A0000000, float 0x400A666660000000)
  %d2 = fpext float %s2 to double
  %s3 = call float @fmul_after_call(float 0x3FF19999A0000000, float 0x400A666660000000)
  %d3 = fpext float %s3 to double
  call i32 (ptr, ...) @printf(ptr @fmt2, double %d1, double %d2, double %d3)
  %r1 = call fastcc i32 @guarded(i32 3, i32 4)
  %r2 = call fastcc i32 @guarded(i32 -1, i32 9)
  %r3 = call i32 @plain_tail(i32 40, i32 2)
  %f = call float @fmul_after_call(float 0x3FF19999A0000000, float 0x400A666660000000)
  %d = fpext float %f to double
  %r4 = call i32 @plain_tail(i32 1, i32 2)
  call i32 (ptr, ...) @printf(ptr @fmt, i32 %r1, i32 %r2, i32 %r3, i32 %r4, double %d)
  ret i32 0
}
