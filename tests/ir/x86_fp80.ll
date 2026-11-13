; x86_fp80, the x87 extended type that is x86-64's long double: sitofp,
; fdiv, fmul, fpext from and fptrunc to double, fsub, a product near the
; top of the range, an i64 converted exactly (64-bit significand), fcmp,
; fptosi; printed as bits and through printf's %Lg, which takes it as a
; variadic argument in memory. x86-64 only: on AArch64 LLVM has no
; x86_fp80 code generation. Expected values computed by a script with
; exact rational arithmetic, not a compiler.
;@ covers: spec/ir.md §5.4 floating point (x86_fp80)
;@ target: x86_64
;@ unsupported: *
;@ stdout: x86_fp80 third 3ffdaaaaaaaaaaaaaaab 0.333333333333333333342
;@ stdout: x86_fp80 third*3 3fff8000000000000000 1
;@ stdout: x86_fp80 third-double 3fc7aac0000000000000 1.85127520951899882107e-17
;@ stdout: x86_fp80 2^16000*2^383 7ffe8000000000000000 5.94865747678615882543e+4931
;@ stdout: x86_fp80 i64 max 403dfffffffffffffffe 9223372036854775807
;@ stdout: x86_fp80 -0.1 from double bffbccccccccccccd000 -0.100000000000000005551
;@ stdout: x86_fp80 to double 0.33333333333333331, third>double 1, fptosi(-2.75) -2

@iv = global [2 x i32] [i32 1, i32 3]
@imax = global i64 9223372036854775807
@dv = global [3 x double] [double 0x3FD5555555555555, double -0.1, double -2.75]
@xbig = global [2 x x86_fp80] [x86_fp80 0xK7E7F8000000000000000, x86_fp80 0xK417E8000000000000000]

@.x = private unnamed_addr constant [32 x i8] c"x86_fp80 %s %04x%016llx %.21Lg\0A\00"
@.end = private unnamed_addr constant [61 x i8] c"x86_fp80 to double %.17g, third>double %d, fptosi(-2.75) %d\0A\00"
@.n0 = private unnamed_addr constant [6 x i8] c"third\00"
@.n1 = private unnamed_addr constant [8 x i8] c"third*3\00"
@.n2 = private unnamed_addr constant [13 x i8] c"third-double\00"
@.n3 = private unnamed_addr constant [14 x i8] c"2^16000*2^383\00"
@.n4 = private unnamed_addr constant [8 x i8] c"i64 max\00"
@.n5 = private unnamed_addr constant [17 x i8] c"-0.1 from double\00"

declare i32 @printf(ptr, ...)

define internal void @show(ptr %name, x86_fp80 %v) {
  %b = bitcast x86_fp80 %v to i80
  %hi = lshr i80 %b, 64
  %h16 = trunc i80 %hi to i16
  %h = zext i16 %h16 to i32
  %l = trunc i80 %b to i64
  call i32 (ptr, ...) @printf(ptr @.x, ptr %name, i32 %h, i64 %l, x86_fp80 %v)
  ret void
}

define i32 @main() {
  %one = load i32, ptr @iv
  %p3 = getelementptr [2 x i32], ptr @iv, i32 0, i32 1
  %three = load i32, ptr %p3
  %x1 = sitofp i32 %one to x86_fp80
  %x3 = sitofp i32 %three to x86_fp80
  %third = fdiv x86_fp80 %x1, %x3
  call void @show(ptr @.n0, x86_fp80 %third)
  %t3 = fmul x86_fp80 %third, %x3
  call void @show(ptr @.n1, x86_fp80 %t3)
  %d3 = load double, ptr @dv
  %xd = fpext double %d3 to x86_fp80
  %dif = fsub x86_fp80 %third, %xd
  call void @show(ptr @.n2, x86_fp80 %dif)
  %b0 = load x86_fp80, ptr @xbig
  %pb1 = getelementptr [2 x x86_fp80], ptr @xbig, i32 0, i32 1
  %b1 = load x86_fp80, ptr %pb1
  %big = fmul x86_fp80 %b0, %b1
  call void @show(ptr @.n3, x86_fp80 %big)
  %im = load i64, ptr @imax
  %xm = sitofp i64 %im to x86_fp80
  call void @show(ptr @.n4, x86_fp80 %xm)
  %pd1 = getelementptr [3 x double], ptr @dv, i32 0, i32 1
  %dt = load double, ptr %pd1
  %xt = fpext double %dt to x86_fp80
  call void @show(ptr @.n5, x86_fp80 %xt)
  %back = fptrunc x86_fp80 %third to double
  %gt = fcmp ogt x86_fp80 %third, %xd
  %gtw = zext i1 %gt to i32
  %pd2 = getelementptr [3 x double], ptr @dv, i32 0, i32 2
  %dm = load double, ptr %pd2
  %xmm = fpext double %dm to x86_fp80
  %tr = fptosi x86_fp80 %xmm to i32
  call i32 (ptr, ...) @printf(ptr @.end, double %back, i32 %gtw, i32 %tr)
  ret i32 0
}
