; i386 join, part B, by hand: the x87 80-bit rule (spec/codegen-x86.md
; section 11) across what the other packages lower as calls or branches,
; musttail to another module's function under every relocation model,
; double constants folded as llc folds them, double minnum/maxnum, i64
; conversions. Each `%m = fmul %a, %a` with a = 1 + 2^-30 is 1 + 2^-29 +
; 2^-60 at 80 bits and 1 + 2^-29 rounded: `%m - 1` printed shows which one
; a use read. Every value is printed as its bits.
target datalayout = "e-m:e-p:32:32-p270:32:32-p271:32:32-p272:64:64-i128:128-f64:32:64-f80:32-n8:16:32-S128"
target triple = "i686-unknown-linux-gnu"

@fmt = private constant [12 x i8] c"%s %016llx\0A\00"
@fmt80 = private constant [15 x i8] c"%s %04x%016llx\00"
@nl = private constant [2 x i8] c"\0A\00"
@done = private constant [5 x i8] c"done\00"
@va = global double 0x3FF0000004000000
@vone = global double 1.0
@vq = global i64 -81985529216486895
@vf = global float 0x3FE5555560000000
@tl = internal thread_local global i32 7
@ext_g = external global i32
@ext_td = external thread_local global double

@s_asm = private constant [4 x i8] c"asm\00"
@s_st = private constant [6 x i8] c"asmst\00"
@s_got = private constant [4 x i8] c"got\00"
@s_tls = private constant [4 x i8] c"tls\00"
@s_tls2 = private constant [5 x i8] c"tls2\00"
@s_ltls = private constant [5 x i8] c"ltls\00"
@s_div = private constant [4 x i8] c"div\00"
@s_sin = private constant [4 x i8] c"sin\00"
@s_add = private constant [5 x i8] c"fadd\00"
@s_sub = private constant [5 x i8] c"fsub\00"
@s_mul = private constant [5 x i8] c"fmul\00"
@s_div2 = private constant [5 x i8] c"fdiv\00"
@s_chain = private constant [6 x i8] c"chain\00"
@s_fadd = private constant [6 x i8] c"faddf\00"
@s_min = private constant [4 x i8] c"min\00"
@s_max = private constant [4 x i8] c"max\00"
@s_i2f = private constant [4 x i8] c"i2f\00"
@s_f2i = private constant [4 x i8] c"f2i\00"
@s_u2f = private constant [4 x i8] c"u2f\00"
@s_t1 = private constant [6 x i8] c"tail1\00"
@s_t2 = private constant [6 x i8] c"tail2\00"
@s_t3 = private constant [6 x i8] c"tail3\00"

declare i32 @printf(ptr, ...)
declare double @sin(double)
declare double @ext_scale(double, i64)
declare x86_fp80 @ext_ld(x86_fp80, float)
declare double @ext_inreg(i32 inreg, i32 inreg, double)
declare double @llvm.minnum.f64(double, double)
declare double @llvm.maxnum.f64(double, double)

define internal void @pd(ptr %what, double %d) #0 {
  %s = alloca double
  store double %d, ptr %s
  %b = load i64, ptr %s
  %r = call i32 (ptr, ...) @printf(ptr @fmt, ptr %what, i64 %b)
  ret void
}

define internal void @pf(ptr %what, float %f) #0 {
  %s = alloca float
  store float %f, ptr %s
  %b = load i32, ptr %s
  %w = zext i32 %b to i64
  %r = call i32 (ptr, ...) @printf(ptr @fmt, ptr %what, i64 %w)
  ret void
}

define internal void @pq(ptr %what, i64 %q) #0 {
  %r = call i32 (ptr, ...) @printf(ptr @fmt, ptr %what, i64 %q)
  ret void
}

; Uses of `%m` after inline assembly without x87 operands: no barrier.
define double @after_asm() #0 {
  %a = load volatile double, ptr @va
  %one = load volatile double, ptr @vone
  %m = fmul double %a, %a
  call void asm sideeffect "nop", "~{dirflag},~{fpsr},~{flags}"()
  %r = fsub double %m, %one
  ret double %r
}

; After inline assembly that clobbers `st(0)`: a barrier.
define double @after_asm_st() #0 {
  %a = load volatile double, ptr @va
  %one = load volatile double, ptr @vone
  %m = fmul double %a, %a
  call void asm sideeffect "nop", "~{st},~{dirflag},~{fpsr},~{flags}"()
  %r = fsub double %m, %one
  ret double %r
}

; After a GOT access (`call; pop` under PIC): no barrier.
define double @after_got(ptr %o) #0 {
  %a = load volatile double, ptr @va
  %one = load volatile double, ptr @vone
  %m = fmul double %a, %a
  %g = load i32, ptr @ext_g
  store i32 %g, ptr %o
  %r = fsub double %m, %one
  ret double %r
}

; After a thread-local of a dynamic model under PIC (`___tls_get_addr`):
; a barrier there, none in the static model.
define double @after_tls(ptr %o) #0 {
  %a = load volatile double, ptr @va
  %one = load volatile double, ptr @vone
  %m = fmul double %a, %a
  %t = load double, ptr @ext_td
  store double %t, ptr %o
  %r = fsub double %m, %one
  ret double %r
}

; The address taken earlier in the block: the second access is no call.
define double @after_tls_again(ptr %o) #0 {
  %t0 = load double, ptr @ext_td
  %a = load volatile double, ptr @va
  %one = load volatile double, ptr @vone
  %m = fmul double %a, %a
  %t = load double, ptr @ext_td
  %s = fadd double %t, %t0
  store double %s, ptr %o
  %r = fsub double %m, %one
  ret double %r
}

; A local thread-local (local dynamic under PIC in a shared library).
define double @after_local_tls(ptr %o) #0 {
  %a = load volatile double, ptr @va
  %one = load volatile double, ptr @vone
  %m = fmul double %a, %a
  %t = load i32, ptr @tl
  store i32 %t, ptr %o
  %r = fsub double %m, %one
  ret double %r
}

; After a 64-bit division (a libgcc call): a barrier.
define double @after_div(ptr %o) #0 {
  %a = load volatile double, ptr @va
  %one = load volatile double, ptr @vone
  %q = load volatile i64, ptr @vq
  %m = fmul double %a, %a
  %d = sdiv i64 %q, 12345
  store i64 %d, ptr %o
  %r = fsub double %m, %one
  ret double %r
}

; After a libm call: a barrier.
define double @after_sin() #0 {
  %a = load volatile double, ptr @va
  %one = load volatile double, ptr @vone
  %m = fmul double %a, %a
  %s = call double @sin(double %a)
  %r = fsub double %m, %one
  %t = fadd double %r, %s
  ret double %t
}

; `musttail` to another module's functions.
define double @tail1(double %x, i64 %y) #0 {
  %r = musttail call double @ext_scale(double %x, i64 %y)
  ret double %r
}

define x86_fp80 @tail2(x86_fp80 %x, float %y) #0 {
  %r = musttail call x86_fp80 @ext_ld(x86_fp80 %x, float %y)
  ret x86_fp80 %r
}

define double @tail3(i32 inreg %a, i32 inreg %b, double %c) #0 {
  %r = musttail call double @ext_inreg(i32 inreg %a, i32 inreg %b, double %c)
  ret double %r
}

define i32 @main() #0 {
  %o = alloca i64, align 8
  %r1 = call double @after_asm()
  call void @pd(ptr @s_asm, double %r1)
  %r2 = call double @after_asm_st()
  call void @pd(ptr @s_st, double %r2)
  %r3 = call double @after_got(ptr %o)
  call void @pd(ptr @s_got, double %r3)
  %r4 = call double @after_tls(ptr %o)
  call void @pd(ptr @s_tls, double %r4)
  %r4b = call double @after_tls_again(ptr %o)
  call void @pd(ptr @s_tls2, double %r4b)
  %r4c = call double @after_local_tls(ptr %o)
  call void @pd(ptr @s_ltls, double %r4c)
  %r5 = call double @after_div(ptr %o)
  call void @pd(ptr @s_div, double %r5)
  %r6 = call double @after_sin()
  call void @pd(ptr @s_sin, double %r6)
  ; Constants llc's DAG folds with one rounding (the x87 rounds twice).
  %c1 = fadd double 1.0, 0x3CA0000008000000
  call void @pd(ptr @s_add, double %c1)
  %c2 = fsub double 1.0, 0x3C90000008000000
  call void @pd(ptr @s_sub, double %c2)
  %c3 = fmul double 0x3FF0000004000000, 0x3FF0000002000002
  call void @pd(ptr @s_mul, double %c3)
  %c4 = fdiv double 0x3FF463FF50B73799, 0x3FF7C9D9CCCBA2B0
  call void @pd(ptr @s_div2, double %c4)
  %c5 = fneg double 0x3CA0000008000000
  %c6 = fsub double 1.0, %c5
  %c7 = fmul double %c6, 3.0
  call void @pd(ptr @s_chain, double %c7)
  %c8 = fadd float 1.0, 0x3E70000020000000
  call void @pf(ptr @s_fadd, float %c8)
  ; `double` minnum/maxnum (calls of fmin/fmax).
  %a = load volatile double, ptr @va
  %one = load volatile double, ptr @vone
  %mn = call double @llvm.minnum.f64(double %a, double %one)
  call void @pd(ptr @s_min, double %mn)
  %mx = call double @llvm.maxnum.f64(double %a, double %one)
  call void @pd(ptr @s_max, double %mx)
  ; i64 conversions.
  %q = load volatile i64, ptr @vq
  %i2f = sitofp i64 %q to double
  call void @pd(ptr @s_i2f, double %i2f)
  %u2f = uitofp i64 %q to double
  call void @pd(ptr @s_u2f, double %u2f)
  %big = fmul double %i2f, 0x3F50000000000000
  %f2i = fptosi double %big to i64
  call void @pq(ptr @s_f2i, i64 %f2i)
  ; musttail exits through each relocation model.
  %t1 = call double @tail1(double %a, i64 %q)
  call void @pd(ptr @s_t1, double %t1)
  %vf = load volatile float, ptr @vf
  %x = fpext double %a to x86_fp80
  %t2 = call x86_fp80 @tail2(x86_fp80 %x, float %vf)
  %t2d = fptrunc x86_fp80 %t2 to double
  call void @pd(ptr @s_t2, double %t2d)
  %t3 = call double @tail3(i32 inreg 9, i32 inreg 4, double %a)
  call void @pd(ptr @s_t3, double %t3)
  %p = call i32 (ptr, ...) @printf(ptr @done)
  %p2 = call i32 (ptr, ...) @printf(ptr @nl)
  ret i32 0
}

attributes #0 = { noinline nounwind }
