; i386 join E: 8-byte atomic loads and stores of `i64` and `double` keep
; every bit pattern. hod writes them as `fild qword` / `fistp qword` (one
; 8-byte access: an aligned x87 access is single-copy atomic) with and
; without SSE2; `llc` uses `movq` with SSE2 and the x87 pair without.
; `fild` of a 64-bit integer is exact (the 80-bit format has a 64-bit
; significand) and `fistp` of that integral value gives it back whatever
; the rounding mode, so the round trip is the identity on all 2^64
; patterns: the classes here (zero, one, all ones, the extremes, the
; `double` signalling and quiet NaNs of both signs, infinities, a
; subnormal, signed zero) show it. `@sse2` is at `pentium4`, `@x87` has
; no `target-cpu`.
target triple = "i686-unknown-linux-gnu"

@pat = internal global [12 x i64] [i64 0, i64 1, i64 -1, i64 -9223372036854775808, i64 9223372036854775807, i64 9218868437227405313, i64 -4503599627370495, i64 9221120237041090560, i64 -4503599627370496, i64 4503599627370495, i64 81985529216486895, i64 -81985529216486896], align 8
@ai = internal global i64 0, align 8
@ad = internal global double 0.0, align 8
@fmt = private constant [35 x i8] c"%s %2d %016llx %016llx %016llx %d\0A\00"
@s2 = private constant [5 x i8] c"sse2\00"
@s0 = private constant [4 x i8] c"x87\00"

declare i32 @printf(ptr, ...)

define internal void @sse2(i32 %i) noinline #0 {
  %p = getelementptr [12 x i64], ptr @pat, i32 0, i32 %i
  %w = load i64, ptr %p
  store atomic i64 %w, ptr @ai seq_cst, align 8
  %li = load atomic i64, ptr @ai acquire, align 8
  %d = bitcast i64 %w to double
  store atomic double %d, ptr @ad release, align 8
  %ld = load atomic double, ptr @ad seq_cst, align 8
  %ldb = bitcast double %ld to i64
  %raw = load atomic i64, ptr @ad monotonic, align 8
  %same = icmp eq i64 %li, %w
  %sz = zext i1 %same to i32
  call i32 (ptr, ...) @printf(ptr @fmt, ptr @s2, i32 %i, i64 %li, i64 %ldb, i64 %raw, i32 %sz)
  ret void
}

define internal void @x87(i32 %i) noinline {
  %p = getelementptr [12 x i64], ptr @pat, i32 0, i32 %i
  %w = load i64, ptr %p
  store atomic i64 %w, ptr @ai seq_cst, align 8
  %li = load atomic i64, ptr @ai acquire, align 8
  %d = bitcast i64 %w to double
  store atomic double %d, ptr @ad release, align 8
  %ld = load atomic double, ptr @ad seq_cst, align 8
  %ldb = bitcast double %ld to i64
  %raw = load atomic i64, ptr @ad monotonic, align 8
  %same = icmp eq i64 %li, %w
  %sz = zext i1 %same to i32
  call i32 (ptr, ...) @printf(ptr @fmt, ptr @s0, i32 %i, i64 %li, i64 %ldb, i64 %raw, i32 %sz)
  ret void
}

define i32 @main() {
entry:
  br label %loop
loop:
  %i = phi i32 [ 0, %entry ], [ %i1, %loop ]
  call void @sse2(i32 %i)
  call void @x87(i32 %i)
  %i1 = add i32 %i, 1
  %done = icmp eq i32 %i1, 12
  br i1 %done, label %out, label %loop
out:
  ret i32 0
}

attributes #0 = { "target-cpu"="pentium4" }
