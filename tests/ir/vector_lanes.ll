; Element-wise operations on vectors, one lane at a time (WP-080):
; division and remainder, shifts and frem on register and slot vectors,
; half arithmetic, casts between classes, fneg of a slot vector, and the
; vector forms of integer and floating-point intrinsics (a libm call per
; lane among them).
;@ covers: spec/ir.md §5.4 integer
;@ unsupported(x86_64): *
;@ link: m
;@ stdout: div: 3 -3 2 1 | rem: 1 -1 0 2 | udiv: 33 1
;@ stdout: odd: 6 -6 25 | shr: 8 -2 64
;@ stdout: float: 1 -0.5 2 | half: 2.5 -1 | neg: -1.5 2 -0
;@ stdout: cast: 200 -56 255 | 1.5e+09 7 | wide: 300 -2 9
;@ stdout: intr: 3 32 7 | 127 -128 0 | -1 0 1
;@ stdout: fp: 2 -3 3 | 1 0.5 | 1 2.71828 7.38906

@.div = private unnamed_addr constant [45 x i8] c"div: %d %d %d %d | rem: %d %d %d %d | udiv: \00"
@.udiv = private unnamed_addr constant [7 x i8] c"%u %u\0A\00"
@.odd = private unnamed_addr constant [31 x i8] c"odd: %lld %lld %lld | shr: %d \00"
@.shr = private unnamed_addr constant [7 x i8] c"%d %d\0A\00"
@.float = private unnamed_addr constant [47 x i8] c"float: %g %g %g | half: %g %g | neg: %g %g %g\0A\00"
@.cast = private unnamed_addr constant [41 x i8] c"cast: %d %d %d | %g %g | wide: %d %d %d\0A\00"
@.intr = private unnamed_addr constant [38 x i8] c"intr: %d %d %d | %d %d %d | %d %d %d\0A\00"
@.fp = private unnamed_addr constant [33 x i8] c"fp: %g %g %g | %g %g | %g %g %g\0A\00"

@ints = global <4 x i32> <i32 7, i32 -7, i32 8, i32 9>
@divs = global <4 x i32> <i32 2, i32 2, i32 4, i32 7>
@wide = global <3 x i64> <i64 13, i64 -13, i64 25>
@halves = global <2 x half> <half 0xH3C00, half 0xHBC00>

declare i32 @printf(ptr, ...)
declare <3 x i8> @llvm.ctpop.v3i8(<3 x i8>)
declare <3 x i8> @llvm.sadd.sat.v3i8(<3 x i8>, <3 x i8>)
declare <3 x i8> @llvm.scmp.v3i8.v3i32(<3 x i32>, <3 x i32>)
declare <3 x float> @llvm.floor.v3f32(<3 x float>)
declare <2 x double> @llvm.sqrt.v2f64(<2 x double>)
declare <3 x double> @llvm.exp.v3f64(<3 x double>)

define i32 @main() {
entry:
  ; Register vectors: sdiv, srem, udiv (no NEON instruction).
  %a = load <4 x i32>, ptr @ints
  %b = load <4 x i32>, ptr @divs
  %q = sdiv <4 x i32> %a, %b
  %r = srem <4 x i32> %a, %b
  %q0 = extractelement <4 x i32> %q, i32 0
  %q1 = extractelement <4 x i32> %q, i32 1
  %q2 = extractelement <4 x i32> %q, i32 2
  %q3 = extractelement <4 x i32> %q, i32 3
  %r0 = extractelement <4 x i32> %r, i32 0
  %r1 = extractelement <4 x i32> %r, i32 1
  %r2 = extractelement <4 x i32> %r, i32 2
  %r3 = extractelement <4 x i32> %r, i32 3
  call i32 (ptr, ...) @printf(ptr @.div, i32 %q0, i32 %q1, i32 %q2, i32 %q3, i32 %r0, i32 %r1, i32 %r2, i32 %r3)
  %u = udiv <2 x i32> <i32 100, i32 -1>, <i32 3, i32 -1>
  %u0 = extractelement <2 x i32> %u, i32 0
  %u1 = extractelement <2 x i32> %u, i32 1
  call i32 (ptr, ...) @printf(ptr @.udiv, i32 %u0, i32 %u1)

  ; Slot vectors: mul and ashr of <3 x i64>, lshr of <3 x i16>.
  %w = load <3 x i64>, ptr @wide
  %m = mul <3 x i64> %w, <i64 2, i64 2, i64 2>
  %d = sdiv <3 x i64> %m, <i64 4, i64 4, i64 2>
  %m0 = extractelement <3 x i64> %d, i32 0
  %m1 = extractelement <3 x i64> %d, i32 1
  %m2 = extractelement <3 x i64> %d, i32 2
  %sh = ashr <3 x i16> <i16 64, i16 -8, i16 256>, <i16 3, i16 2, i16 2>
  %sh0 = extractelement <3 x i16> %sh, i32 0
  %sh1 = extractelement <3 x i16> %sh, i32 1
  %sh2 = extractelement <3 x i16> %sh, i32 2
  %s0 = sext i16 %sh0 to i32
  %s1 = sext i16 %sh1 to i32
  %s2 = sext i16 %sh2 to i32
  call i32 (ptr, ...) @printf(ptr @.odd, i64 %m0, i64 %m1, i64 %m2, i32 %s0)
  call i32 (ptr, ...) @printf(ptr @.shr, i32 %s1, i32 %s2)

  ; frem of a slot vector, half arithmetic, fneg of a slot vector.
  %f = frem <3 x float> <float 7.0, float -6.5, float 2.0>, <float 3.0, float 3.0, float 5.0>
  %f0 = extractelement <3 x float> %f, i32 0
  %f1 = extractelement <3 x float> %f, i32 1
  %f2 = extractelement <3 x float> %f, i32 2
  %e0 = fpext float %f0 to double
  %e1 = fpext float %f1 to double
  %e2 = fpext float %f2 to double
  %h = load <2 x half>, ptr @halves
  %h2 = fadd <2 x half> %h, <half 0xH3E00, half 0xH0000>
  %hd = fpext <2 x half> %h2 to <2 x double>
  %hd0 = extractelement <2 x double> %hd, i32 0
  %hd1 = extractelement <2 x double> %hd, i32 1
  %n = fneg <3 x double> <double 1.5, double -2.0, double 0.0>
  %n0 = extractelement <3 x double> %n, i32 0
  %n1 = extractelement <3 x double> %n, i32 1
  %n2 = extractelement <3 x double> %n, i32 2
  call i32 (ptr, ...) @printf(ptr @.float, double %e0, double %e1, double %e2, double %hd0, double %hd1, double %n0, double %n1, double %n2)

  ; Casts: trunc to a slot vector, fptoui/sitofp across classes, zext
  ; to a wide slot vector.
  %t = trunc <3 x i32> <i32 456, i32 200, i32 -1> to <3 x i8>
  %t0 = extractelement <3 x i8> %t, i32 0
  %t1 = extractelement <3 x i8> %t, i32 1
  %t2 = extractelement <3 x i8> %t, i32 2
  %tz0 = zext i8 %t0 to i32
  %ts1 = sext i8 %t1 to i32
  %tz2 = zext i8 %t2 to i32
  %c = sitofp <2 x i64> <i64 1500000000, i64 7> to <2 x float>
  %c0 = extractelement <2 x float> %c, i32 0
  %c1 = extractelement <2 x float> %c, i32 1
  %cd0 = fpext float %c0 to double
  %cd1 = fpext float %c1 to double
  %z = sext <3 x i16> <i16 300, i16 -2, i16 9> to <3 x i64>
  %z0 = extractelement <3 x i64> %z, i32 0
  %z1 = extractelement <3 x i64> %z, i32 1
  %z2 = extractelement <3 x i64> %z, i32 2
  %zt0 = trunc i64 %z0 to i32
  %zt1 = trunc i64 %z1 to i32
  %zt2 = trunc i64 %z2 to i32
  call i32 (ptr, ...) @printf(ptr @.cast, i32 %tz0, i32 %ts1, i32 %tz2, double %cd0, double %cd1, i32 %zt0, i32 %zt1, i32 %zt2)

  ; Integer intrinsics on slot vectors.
  %p = call <3 x i8> @llvm.ctpop.v3i8(<3 x i8> <i8 7, i8 -1, i8 127>)
  %p0 = extractelement <3 x i8> %p, i32 0
  %p1 = extractelement <3 x i8> %p, i32 1
  %p2 = extractelement <3 x i8> %p, i32 2
  %pi0 = zext i8 %p0 to i32
  %pi1 = zext i8 %p1 to i32
  %pi2 = zext i8 %p2 to i32
  %pi1b = mul i32 %pi1, 4
  %sat = call <3 x i8> @llvm.sadd.sat.v3i8(<3 x i8> <i8 100, i8 -100, i8 5>, <3 x i8> <i8 100, i8 -100, i8 -5>)
  %sa0 = extractelement <3 x i8> %sat, i32 0
  %sa1 = extractelement <3 x i8> %sat, i32 1
  %sa2 = extractelement <3 x i8> %sat, i32 2
  %si0 = sext i8 %sa0 to i32
  %si1 = sext i8 %sa1 to i32
  %si2 = sext i8 %sa2 to i32
  %cmp = call <3 x i8> @llvm.scmp.v3i8.v3i32(<3 x i32> <i32 -5, i32 4, i32 9>, <3 x i32> <i32 3, i32 4, i32 -9>)
  %cm0 = extractelement <3 x i8> %cmp, i32 0
  %cm1 = extractelement <3 x i8> %cmp, i32 1
  %cm2 = extractelement <3 x i8> %cmp, i32 2
  %ci0 = sext i8 %cm0 to i32
  %ci1 = sext i8 %cm1 to i32
  %ci2 = sext i8 %cm2 to i32
  call i32 (ptr, ...) @printf(ptr @.intr, i32 %pi0, i32 %pi1b, i32 %pi2, i32 %si0, i32 %si1, i32 %si2, i32 %ci0, i32 %ci1, i32 %ci2)

  ; Floating-point intrinsics: inline on a slot vector, inline on a
  ; register vector, and a call of exp per lane.
  %fl = call <3 x float> @llvm.floor.v3f32(<3 x float> <float 2.5, float -2.5, float 3.0>)
  %fl0 = extractelement <3 x float> %fl, i32 0
  %fl1 = extractelement <3 x float> %fl, i32 1
  %fl2 = extractelement <3 x float> %fl, i32 2
  %fd0 = fpext float %fl0 to double
  %fd1 = fpext float %fl1 to double
  %fd2 = fpext float %fl2 to double
  %sq = call <2 x double> @llvm.sqrt.v2f64(<2 x double> <double 1.0, double 0.25>)
  %sq0 = extractelement <2 x double> %sq, i32 0
  %sq1 = extractelement <2 x double> %sq, i32 1
  %ex = call <3 x double> @llvm.exp.v3f64(<3 x double> <double 0.0, double 1.0, double 2.0>)
  %ex0 = extractelement <3 x double> %ex, i32 0
  %ex1 = extractelement <3 x double> %ex, i32 1
  %ex2 = extractelement <3 x double> %ex, i32 2
  call i32 (ptr, ...) @printf(ptr @.fp, double %fd0, double %fd1, double %fd2, double %sq0, double %sq1, double %ex0, double %ex1, double %ex2)
  ret i32 0
}
