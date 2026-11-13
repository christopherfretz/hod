; Scalar half and fp128. Half: fadd, fsub, fmul, fdiv, fcmp, rounding
; to the nearest half (a subnormal result, an overflow to infinity),
; fptrunc from double, sitofp and fptosi, half through memory and as an
; argument and a result. fp128 (soft float: libgcc's __addtf3 and kin on
; both targets): sitofp, fdiv, fmul, fadd and fsub with an fpext double,
; fneg, a subnormal product, sitofp from i128, fcmp, fptosi, fptrunc.
; Values print as their bits (and fp128 also as the nearest double).
; Expected values computed by a script with exact rational arithmetic,
; not a compiler.
;@ covers: spec/ir.md §5.4 floating point (half, fp128)
;@ unsupported(x86_64): *
;@ stdout: half 1.5 -2.25: add ba00 sub 4380 mul c2c0 div b955 lt 0
;@ stdout: half 0.0999755859375 1000: add 63d0 sub e3d0 mul 5640 div 068d lt 1
;@ stdout: half 1000 5.9604644775390625e-08: add 63d0 sub 63d0 mul 03e8 div 7c00 lt 0
;@ stdout: half -2.25 0.0999755859375: add c04d sub c0b3 mul b333 div cda0 lt 1
;@ stdout: half conv: 0.1 2e66 70000 7c00 2049 6800 32768+65472 7c00 to i32 1000 -2.25 to i32 -2
;@ stdout: fp128 third 3ffd5555555555555555555555555555 0.33333333333333331
;@ stdout: fp128 third*3 3fff0000000000000000000000000000 1
;@ stdout: fp128 third+double 3ffe55555555555552aaaaaaaaaaaaaa 0.66666666666666663
;@ stdout: fp128 third-double 3fc75555555555555540000000000000 1.8503717077085941e-17
;@ stdout: fp128 neg bffd5555555555555555555555555555 -0.33333333333333331
;@ stdout: fp128 2^-8200*2^-8200 00000000400000000000000000000000 0
;@ stdout: fp128 1e30 from i128 406293e5939a08ce9dbd480000000000 1e+30
;@ stdout: fp128 third>double 1 third*3==1 1 fptosi(third*3e18) 1000000000000000000

@hv = global [5 x half] [half 1.5, half -2.25, half 0xH2E66, half 1000.0, half 0xH0001]
@dv = global [3 x double] [double 0.1, double 70000.0, double 0x3FD5555555555555]
@iv = global [3 x i32] [i32 2049, i32 0, i32 0]
@qsmall = global fp128 0xL00000000000000001FF7000000000000
@big = global i128 1000000000000000000000000000000
@e18 = global i64 3000000000000000000

@.pair = private unnamed_addr constant [54 x i8] c"half %.17g %.17g: add %04x sub %04x mul %04x div %04x\00"
@.lt = private unnamed_addr constant [8 x i8] c" lt %d\0A\00"
@.conv = private unnamed_addr constant [85 x i8] c"half conv: 0.1 %04x 70000 %04x 2049 %04x 32768+65472 %04x to i32 %d -2.25 to i32 %d\0A\00"
@.q = private unnamed_addr constant [31 x i8] c"fp128 %s %016llx%016llx %.17g\0A\00"
@.qc = private unnamed_addr constant [61 x i8] c"fp128 third>double %d third*3==1 %d fptosi(third*3e18) %lld\0A\00"
@.n0 = private unnamed_addr constant [6 x i8] c"third\00"
@.n1 = private unnamed_addr constant [8 x i8] c"third*3\00"
@.n2 = private unnamed_addr constant [13 x i8] c"third+double\00"
@.n3 = private unnamed_addr constant [13 x i8] c"third-double\00"
@.n4 = private unnamed_addr constant [4 x i8] c"neg\00"
@.n5 = private unnamed_addr constant [16 x i8] c"2^-8200*2^-8200\00"
@.n6 = private unnamed_addr constant [15 x i8] c"1e30 from i128\00"

declare i32 @printf(ptr, ...)

; Bits of a half, as an i32 for printf.
define internal i32 @bits(half %h) {
  %b = bitcast half %h to i16
  %w = zext i16 %b to i32
  ret i32 %w
}

define internal half @hadd(half %a, half %b) {
  %r = fadd half %a, %b
  ret half %r
}

define internal void @pair(i32 %i, i32 %j) {
  %pa = getelementptr [5 x half], ptr @hv, i32 0, i32 %i
  %a = load half, ptr %pa
  %pb = getelementptr [5 x half], ptr @hv, i32 0, i32 %j
  %b = load half, ptr %pb
  %add = call half @hadd(half %a, half %b)
  %sub = fsub half %a, %b
  %mul = fmul half %a, %b
  %div = fdiv half %a, %b
  %da = fpext half %a to double
  %fb = fpext half %b to float
  %db = fpext float %fb to double
  %x0 = call i32 @bits(half %add)
  %x1 = call i32 @bits(half %sub)
  %x2 = call i32 @bits(half %mul)
  %x3 = call i32 @bits(half %div)
  call i32 (ptr, ...) @printf(ptr @.pair, double %da, double %db, i32 %x0, i32 %x1, i32 %x2, i32 %x3)
  %lt = fcmp olt half %a, %b
  %ltw = zext i1 %lt to i32
  call i32 (ptr, ...) @printf(ptr @.lt, i32 %ltw)
  ret void
}

define internal void @showq(ptr %name, fp128 %v) {
  %b = bitcast fp128 %v to i128
  %hi = lshr i128 %b, 64
  %h = trunc i128 %hi to i64
  %l = trunc i128 %b to i64
  %d = fptrunc fp128 %v to double
  call i32 (ptr, ...) @printf(ptr @.q, ptr %name, i64 %h, i64 %l, double %d)
  ret void
}

define i32 @main() {
  call void @pair(i32 0, i32 1)
  call void @pair(i32 2, i32 3)
  call void @pair(i32 3, i32 4)
  call void @pair(i32 1, i32 2)
  %d0 = load double, ptr @dv
  %h0 = fptrunc double %d0 to half
  %c0 = call i32 @bits(half %h0)
  %pd1 = getelementptr [3 x double], ptr @dv, i32 0, i32 1
  %d1 = load double, ptr %pd1
  %h1 = fptrunc double %d1 to half
  %c1 = call i32 @bits(half %h1)
  %i0 = load i32, ptr @iv
  %h2 = sitofp i32 %i0 to half
  %c2 = call i32 @bits(half %h2)
  %m = call half @hadd(half 0xH7800, half 0xH7BFE)
  %c3 = call i32 @bits(half %m)
  %p3 = getelementptr [5 x half], ptr @hv, i32 0, i32 3
  %k = load half, ptr %p3
  %ki = fptosi half %k to i32
  %p1 = getelementptr [5 x half], ptr @hv, i32 0, i32 1
  %n = load half, ptr %p1
  %ni = fptosi half %n to i32
  call i32 (ptr, ...) @printf(ptr @.conv, i32 %c0, i32 %c1, i32 %c2, i32 %c3, i32 %ki, i32 %ni)
  %pi1 = getelementptr [3 x i32], ptr @iv, i32 0, i32 1
  %z = load i32, ptr %pi1
  %one = add i32 %z, 1
  %three = add i32 %z, 3
  %q1 = sitofp i32 %one to fp128
  %q3 = sitofp i32 %three to fp128
  %third = fdiv fp128 %q1, %q3
  call void @showq(ptr @.n0, fp128 %third)
  %t3 = fmul fp128 %third, %q3
  call void @showq(ptr @.n1, fp128 %t3)
  %pd2 = getelementptr [3 x double], ptr @dv, i32 0, i32 2
  %dt = load double, ptr %pd2
  %qt = fpext double %dt to fp128
  %sum = fadd fp128 %third, %qt
  call void @showq(ptr @.n2, fp128 %sum)
  %dif = fsub fp128 %third, %qt
  call void @showq(ptr @.n3, fp128 %dif)
  %neg = fneg fp128 %third
  call void @showq(ptr @.n4, fp128 %neg)
  %qs = load fp128, ptr @qsmall
  %sub = fmul fp128 %qs, %qs
  call void @showq(ptr @.n5, fp128 %sub)
  %bi = load i128, ptr @big
  %qb = sitofp i128 %bi to fp128
  call void @showq(ptr @.n6, fp128 %qb)
  %gt = fcmp ogt fp128 %third, %qt
  %gtw = zext i1 %gt to i32
  %eq = fcmp oeq fp128 %t3, 0xL00000000000000003FFF000000000000
  %eqw = zext i1 %eq to i32
  %e18 = load i64, ptr @e18
  %qe = sitofp i64 %e18 to fp128
  %p = fmul fp128 %third, %qe
  %pi = fptosi fp128 %p to i64
  call i32 (ptr, ...) @printf(ptr @.qc, i32 %gtw, i32 %eqw, i64 %pi)
  ret i32 0
}
