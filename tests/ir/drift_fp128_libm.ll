; Drift seed (design 0006 §8 risk 7, WP-130): the C library routine an
; fp128 math intrinsic and an fp128 frem call. The routine is chosen by
; type AND target: on AArch64 Linux fp128 is long double (sinl, fmodl),
; on x86-64 it is _Float128 (sinf128, fmodf128), and a table keyed by
; the type alone gets one of the two wrong. Both targets must print the
; same. Operands come from memory, so nothing is folded. Results print
; as the nearest double; sqrt (exact in IEEE arithmetic) also as its
; bits. Expected values computed by a script (Python's math module and
; integer square root), not a compiler.
;@ covers: spec/ir.md §5.5 intrinsics (sin, cos, exp, log, pow, sqrt, floor on fp128), §5.4 floating point (frem fp128); spec/codegen.md §7.2 (runtime routines by type and target: fp128 libm)
;@ link: m
;@ stdout: sin(1) 0.8414709848078965
;@ stdout: cos(0.5) 0.87758256189037276
;@ stdout: exp(1) 2.7182818284590451
;@ stdout: log(10) 2.3025850929940459
;@ stdout: pow(2,0.5) 1.4142135623730951
;@ stdout: fmod(10.5,3) 1.5
;@ stdout: floor(-2.5) -3
;@ stdout: sqrt(2) 3fff6a09e667f3bcc908b2fb1366ea95 1.4142135623730951

@q = global [6 x fp128] [fp128 0xL00000000000000003FFF000000000000, fp128 0xL00000000000000003FFE000000000000, fp128 0xL00000000000000004002400000000000, fp128 0xL00000000000000004000000000000000, fp128 0xL00000000000000004002500000000000, fp128 0xL0000000000000000C000400000000000]

@.v = private unnamed_addr constant [10 x i8] c"%s %.17g\0A\00"
@.b = private unnamed_addr constant [25 x i8] c"%s %016llx%016llx %.17g\0A\00"
@.sin = private unnamed_addr constant [7 x i8] c"sin(1)\00"
@.cos = private unnamed_addr constant [9 x i8] c"cos(0.5)\00"
@.exp = private unnamed_addr constant [7 x i8] c"exp(1)\00"
@.log = private unnamed_addr constant [8 x i8] c"log(10)\00"
@.pow = private unnamed_addr constant [11 x i8] c"pow(2,0.5)\00"
@.fmod = private unnamed_addr constant [13 x i8] c"fmod(10.5,3)\00"
@.floor = private unnamed_addr constant [12 x i8] c"floor(-2.5)\00"
@.sqrt = private unnamed_addr constant [8 x i8] c"sqrt(2)\00"

declare i32 @printf(ptr, ...)
declare fp128 @llvm.sin.f128(fp128)
declare fp128 @llvm.cos.f128(fp128)
declare fp128 @llvm.exp.f128(fp128)
declare fp128 @llvm.log.f128(fp128)
declare fp128 @llvm.pow.f128(fp128, fp128)
declare fp128 @llvm.sqrt.f128(fp128)
declare fp128 @llvm.floor.f128(fp128)

; q[i]: 1, 0.5, 10, 2, 10.5, -2.5.
define internal fp128 @at(i32 %i) {
  %p = getelementptr [6 x fp128], ptr @q, i32 0, i32 %i
  %v = load fp128, ptr %p, align 16
  ret fp128 %v
}

define internal void @show(ptr %name, fp128 %v) {
  %d = fptrunc fp128 %v to double
  call i32 (ptr, ...) @printf(ptr @.v, ptr %name, double %d)
  ret void
}

define i32 @main() {
  %one = call fp128 @at(i32 0)
  %half = call fp128 @at(i32 1)
  %ten = call fp128 @at(i32 2)
  %two = call fp128 @at(i32 3)
  %ten5 = call fp128 @at(i32 4)
  %m25 = call fp128 @at(i32 5)
  %sin = call fp128 @llvm.sin.f128(fp128 %one)
  call void @show(ptr @.sin, fp128 %sin)
  %cos = call fp128 @llvm.cos.f128(fp128 %half)
  call void @show(ptr @.cos, fp128 %cos)
  %exp = call fp128 @llvm.exp.f128(fp128 %one)
  call void @show(ptr @.exp, fp128 %exp)
  %log = call fp128 @llvm.log.f128(fp128 %ten)
  call void @show(ptr @.log, fp128 %log)
  %pow = call fp128 @llvm.pow.f128(fp128 %two, fp128 %half)
  call void @show(ptr @.pow, fp128 %pow)
  %three = fsub fp128 %two, %m25
  %three1 = fadd fp128 %three, %m25
  %three2 = fadd fp128 %three1, %one
  %fmod = frem fp128 %ten5, %three2
  call void @show(ptr @.fmod, fp128 %fmod)
  %floor = call fp128 @llvm.floor.f128(fp128 %m25)
  call void @show(ptr @.floor, fp128 %floor)
  %sqrt = call fp128 @llvm.sqrt.f128(fp128 %two)
  %bits = bitcast fp128 %sqrt to i128
  %hi128 = lshr i128 %bits, 64
  %hi = trunc i128 %hi128 to i64
  %lo = trunc i128 %bits to i64
  %d = fptrunc fp128 %sqrt to double
  call i32 (ptr, ...) @printf(ptr @.b, ptr @.sqrt, i64 %hi, i64 %lo, double %d)
  ret i32 0
}
