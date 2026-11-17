; Stack-passed arguments of a protected function (WP-333). Each function
; below has arguments beyond the argument registers of both 64-bit
; targets, so they arrive in the caller's argument area, above the
; callee's stack guard; each writes a local buffer before it reads them.
; In bounds every value comes back intact. Then @victim overflows its
; 16-byte buffer by 184 bytes, through its guard and into the incoming
; argument area, where its 9th argument, a pointer, lies, and then stores
; through that pointer. llc read the pointer at entry into a slot below
; the guard, so the store goes to @sink and the check before the return
; aborts (SIGABRT, "stack smashing detected"). A program that read the
; pointer in place would store through 0x4141414141414141 and die of
; SIGSEGV instead.
;@ covers: spec/codegen.md §5.6.24 stack protectors (rule 10, stack-passed arguments)
; i686's stack protector is another package's; its varargs list is a pointer.
;@ kept-off(i686): va-list-abi
;@ exit: signal 6
;@ stdout: ints 10987
;@ stdout: fps 17.5 3.25
;@ stdout: wide 12345 67
;@ stdout: byval 43
;@ stdout: va 1 2 3 9
;@ stdout: req 9
;@ stdout: dyn 9
;@ stdout: sink 7
;@ stdout: smashing

%struct.big = type { [6 x i64] }

@sink = global i32 0
@.ints = private unnamed_addr constant [10 x i8] c"ints %ld\0A\00"
@.fps = private unnamed_addr constant [15 x i8] c"fps %.1f %.2f\0A\00"
@.wide = private unnamed_addr constant [14 x i8] c"wide %ld %ld\0A\00"
@.byval = private unnamed_addr constant [11 x i8] c"byval %ld\0A\00"
@.vafmt = private unnamed_addr constant [13 x i8] c"va %d %d %d \00"
@.va = private unnamed_addr constant [5 x i8] c"%ld\0A\00"
@.req = private unnamed_addr constant [9 x i8] c"req %ld\0A\00"
@.dyn = private unnamed_addr constant [9 x i8] c"dyn %ld\0A\00"
@.sink = private unnamed_addr constant [9 x i8] c"sink %d\0A\00"
@.smashing = private unnamed_addr constant [10 x i8] c"smashing\0A\00"
@.survived = private unnamed_addr constant [10 x i8] c"survived\0A\00"

declare i32 @printf(ptr, ...)
declare i32 @vprintf(ptr, ptr)
declare i32 @fflush(ptr)
declare void @llvm.memset.p0.i64(ptr, i8, i64, i1)
declare void @llvm.va_start.p0(ptr)
declare void @llvm.va_end.p0(ptr)

; Writes %n bytes of 'A' from %p.
define void @fill(ptr %p, i32 %n) noinline {
  %w = sext i32 %n to i64
  call void @llvm.memset.p0.i64(ptr %p, i8 65, i64 %w, i1 false)
  ret void
}

define void @nothing() noinline {
  ret void
}

; Integers: the 7th to the 11th on x86-64, the 9th to the 11th on
; AArch64, an i32 and a sign-extended i8 among them.
define i64 @ints(i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6, i64 %a7, i64 %a8, i64 %a9, i32 %a10, i8 signext %a11) noinline sspstrong {
  %b = alloca [16 x i8], align 16
  call void @fill(ptr %b, i32 16)
  %t8 = mul i64 %a8, 10
  %t9 = mul i64 %a9, 100
  %w10 = sext i32 %a10 to i64
  %t10 = mul i64 %w10, 1000
  %w11 = sext i8 %a11 to i64
  %t11 = mul i64 %w11, 10000
  %s1 = add i64 %a7, %t8
  %s2 = add i64 %s1, %t9
  %s3 = add i64 %s2, %t10
  %s4 = add i64 %s3, %t11
  ret i64 %s4
}

; Floating point beyond the eight FP argument registers of both targets:
; a double and a float on the stack.
define double @fps(double %d0, double %d1, double %d2, double %d3, double %d4, double %d5, double %d6, double %d7, double %d8, float %f9, ptr %out) noinline sspstrong {
  %b = alloca [16 x i8], align 16
  call void @fill(ptr %b, i32 16)
  %w = fpext float %f9 to double
  store double %w, ptr %out
  %s = fadd double %d8, %d0
  ret double %s
}

; An i128 that does not fit in the remaining registers (one left on
; x86-64; x7 on AArch64 is not the even register of a pair).
define i64 @wide(i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6, i64 %a7, i128 %w, ptr %hi) noinline sspstrong {
  %b = alloca [16 x i8], align 16
  call void @fill(ptr %b, i32 16)
  %h = lshr i128 %w, 64
  %ht = trunc i128 %h to i64
  store i64 %ht, ptr %hi
  %lo = trunc i128 %w to i64
  ret i64 %lo
}

; A byval struct: the caller's copy, read in place by llc and hod alike.
define i64 @byval(ptr byval(%struct.big) align 8 %s) noinline sspstrong {
  %b = alloca [16 x i8], align 16
  call void @fill(ptr %b, i32 16)
  %f0 = load i64, ptr %s
  %p5 = getelementptr inbounds i64, ptr %s, i64 5
  %f5 = load i64, ptr %p5
  %r = add i64 %f0, %f5
  ret i64 %r
}

; A protected variadic function whose last named argument is on the
; stack on both targets: va_start's overflow area starts past it.
define i64 @va(ptr %fmt, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6, i64 %a7, i64 %a8, i64 %a9, ...) noinline sspstrong {
  %ap = alloca [32 x i8], align 16
  %b = alloca [16 x i8], align 16
  call void @llvm.va_start.p0(ptr %ap)
  call i32 @vprintf(ptr %fmt, ptr %ap)
  call void @llvm.va_end.p0(ptr %ap)
  call void @fill(ptr %b, i32 16)
  ret i64 %a9
}

; sspreq: a guard without any buffer.
define i64 @req(i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6, i64 %a7, i64 %a8, i64 %a9) noinline sspreq {
  call void @nothing()
  ret i64 %a9
}

; A dynamic alloca (the low guard of §5.6.24 rule 4).
define i64 @dyn(i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6, i64 %a7, i64 %a8, i64 %a9, i32 %n) noinline sspstrong {
  %p = alloca i8, i32 %n, align 16
  call void @fill(ptr %p, i32 %n)
  ret i64 %a9
}

; The 9th argument, a pointer, is on the stack on both targets.
define void @victim(i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6, i64 %a7, i64 %a8, ptr %dst, i32 %n) noinline sspstrong {
  %b = alloca [16 x i8], align 16
  call void @fill(ptr %b, i32 %n)
  store i32 7, ptr %dst
  ret void
}

define i32 @main() {
  %fout = alloca double, align 8
  %hi = alloca i64, align 8
  %big = alloca %struct.big, align 8

  %i = call i64 @ints(i64 1, i64 2, i64 3, i64 4, i64 5, i64 6, i64 7, i64 8, i64 9, i32 0, i8 1)
  call i32 (ptr, ...) @printf(ptr @.ints, i64 %i)

  %d = call double @fps(double 1.5, double 2.0, double 3.0, double 4.0, double 5.0, double 6.0, double 7.0, double 8.0, double 16.0, float 3.25, ptr %fout)
  %f = load double, ptr %fout
  call i32 (ptr, ...) @printf(ptr @.fps, double %d, double %f)

  %lo = call i64 @wide(i64 1, i64 2, i64 3, i64 4, i64 5, i64 6, i64 7, i128 1235931852938539970617, ptr %hi)
  %h = load i64, ptr %hi
  call i32 (ptr, ...) @printf(ptr @.wide, i64 %lo, i64 %h)

  store i64 40, ptr %big
  %b5 = getelementptr inbounds i64, ptr %big, i64 5
  store i64 3, ptr %b5
  %bv = call i64 @byval(ptr byval(%struct.big) align 8 %big)
  call i32 (ptr, ...) @printf(ptr @.byval, i64 %bv)

  %v = call i64 (ptr, i64, i64, i64, i64, i64, i64, i64, i64, ...) @va(ptr @.vafmt, i64 2, i64 3, i64 4, i64 5, i64 6, i64 7, i64 8, i64 9, i32 1, i32 2, i32 3)
  call i32 (ptr, ...) @printf(ptr @.va, i64 %v)

  %r = call i64 @req(i64 1, i64 2, i64 3, i64 4, i64 5, i64 6, i64 7, i64 8, i64 9)
  call i32 (ptr, ...) @printf(ptr @.req, i64 %r)

  %y = call i64 @dyn(i64 1, i64 2, i64 3, i64 4, i64 5, i64 6, i64 7, i64 8, i64 9, i32 24)
  call i32 (ptr, ...) @printf(ptr @.dyn, i64 %y)

  call void @victim(i64 1, i64 2, i64 3, i64 4, i64 5, i64 6, i64 7, i64 8, ptr @sink, i32 16)
  %s = load i32, ptr @sink
  call i32 (ptr, ...) @printf(ptr @.sink, i32 %s)
  call i32 (ptr, ...) @printf(ptr @.smashing)
  call i32 @fflush(ptr null)
  call void @victim(i64 1, i64 2, i64 3, i64 4, i64 5, i64 6, i64 7, i64 8, ptr @sink, i32 200)
  call i32 (ptr, ...) @printf(ptr @.survived)
  ret i32 0
}

; As clang writes them for a PIE: without "PIC Level", llc reads
; __stack_chk_guard directly on AArch64, which no PIE link accepts.
!llvm.module.flags = !{!0, !1}
!0 = !{i32 8, !"PIC Level", i32 2}
!1 = !{i32 7, !"PIE Level", i32 2}
