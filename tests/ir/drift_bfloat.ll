; Drift seed (design 0006 §8 risk 7, WP-130): bfloat through float.
; Neither target has bfloat arithmetic: an operation widens its operands
; to float, computes there, and rounds the result to bfloat (to nearest,
; ties to even); a conversion from double rounds once, not through float
; (1 + 2^-8 + 2^-30 rounds up; through float it would tie and round to
; even, down). fadd, fsub, fmul, fdiv, fcmp, fpext, fptrunc from float
; and double, sitofp, fptosi, bfloat in memory and as an argument and a
; result. Values print as their bits. Operands come from memory, so
; nothing is folded. Expected values computed by a script with exact
; rational arithmetic, not a compiler.
;@ covers: spec/ir.md §5.4 floating point (bfloat)
;@ stdout: bfloat 4049 bc00: add 4048 sub 404a mul bcc9 div c3c9 lt 0 gt 1
;@ stdout: bfloat from double: 0.1 3dcd 1/3 3eab 70000 4789 1+2^-8+2^-30 3f81
;@ stdout: bfloat from float: 0.1 3dcd; to float 3.140625; from i32 1000 447a; to i32 -402

@bv = global [2 x bfloat] [bfloat 0xR4049, bfloat 0xRBC00]
@dv = global [4 x double] [double 0.1, double 0x3FD5555555555555, double 7.000000e+04, double 0x3FF0100000400000]
@fv = global float 0x3FB99999A0000000
@iv = global i32 1000

@.f1 = private unnamed_addr constant [67 x i8] c"bfloat %04x %04x: add %04x sub %04x mul %04x div %04x lt %d gt %d\0A\00"
@.f2 = private unnamed_addr constant [68 x i8] c"bfloat from double: 0.1 %04x 1/3 %04x 70000 %04x 1+2^-8+2^-30 %04x\0A\00"
@.f3 = private unnamed_addr constant [76 x i8] c"bfloat from float: 0.1 %04x; to float %.17g; from i32 1000 %04x; to i32 %d\0A\00"

declare i32 @printf(ptr, ...)

define internal i32 @bits(bfloat %h) noinline {
  %b = bitcast bfloat %h to i16
  %w = zext i16 %b to i32
  ret i32 %w
}

define internal bfloat @bdiv(bfloat %a, bfloat %b) noinline {
  %r = fdiv bfloat %a, %b
  ret bfloat %r
}

define internal double @dv_at(i32 %i) {
  %p = getelementptr [4 x double], ptr @dv, i32 0, i32 %i
  %v = load double, ptr %p
  ret double %v
}

define i32 @main() {
  %pa = getelementptr [2 x bfloat], ptr @bv, i32 0, i32 0
  %pb = getelementptr [2 x bfloat], ptr @bv, i32 0, i32 1
  %a = load bfloat, ptr %pa
  %b = load bfloat, ptr %pb
  %add = fadd bfloat %a, %b
  %sub = fsub bfloat %a, %b
  %mul = fmul bfloat %a, %b
  %div = call bfloat @bdiv(bfloat %a, bfloat %b)
  %lt = fcmp olt bfloat %a, %b
  %gt = fcmp ogt bfloat %a, %b
  %ba = call i32 @bits(bfloat %a)
  %bb = call i32 @bits(bfloat %b)
  %b1 = call i32 @bits(bfloat %add)
  %b2 = call i32 @bits(bfloat %sub)
  %b3 = call i32 @bits(bfloat %mul)
  %b4 = call i32 @bits(bfloat %div)
  %lt32 = zext i1 %lt to i32
  %gt32 = zext i1 %gt to i32
  call i32 (ptr, ...) @printf(ptr @.f1, i32 %ba, i32 %bb, i32 %b1, i32 %b2, i32 %b3, i32 %b4, i32 %lt32, i32 %gt32)
  %d0 = call double @dv_at(i32 0)
  %d1 = call double @dv_at(i32 1)
  %d2 = call double @dv_at(i32 2)
  %d3 = call double @dv_at(i32 3)
  %t0 = fptrunc double %d0 to bfloat
  %t1 = fptrunc double %d1 to bfloat
  %t2 = fptrunc double %d2 to bfloat
  %t3 = fptrunc double %d3 to bfloat
  %c0 = call i32 @bits(bfloat %t0)
  %c1 = call i32 @bits(bfloat %t1)
  %c2 = call i32 @bits(bfloat %t2)
  %c3 = call i32 @bits(bfloat %t3)
  call i32 (ptr, ...) @printf(ptr @.f2, i32 %c0, i32 %c1, i32 %c2, i32 %c3)
  %f = load float, ptr @fv
  %tf = fptrunc float %f to bfloat
  %e0 = call i32 @bits(bfloat %tf)
  %wa = fpext bfloat %a to double
  %i = load i32, ptr @iv
  %fi = sitofp i32 %i to bfloat
  %e1 = call i32 @bits(bfloat %fi)
  %back = fptosi bfloat %div to i32
  call i32 (ptr, ...) @printf(ptr @.f3, i32 %e0, double %wa, i32 %e1, i32 %back)
  ret i32 0
}
