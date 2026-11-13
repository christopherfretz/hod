; Vectors as values (WP-079): extractelement and insertelement with
; constant and variable indices, shufflevector (reverse, splat, two
; operands, widening, narrowing), select and phi of vectors, constants
; with poison lanes, and vectors of register size crossing calls.
;@ covers: spec/ir.md §5.4 aggregate (ExtractElement, InsertElement, ShuffleVector)
;@ stdout: lanes: 10 20 30 40 | 40 30 20 10
;@ stdout: set: 10 99 30 40 | 7 -1 -1 -1 0 13
;@ stdout: mixed: 1.5 -2 1.5 8.25
;@ stdout: odd: 3 2 1 | 11 22 33 44 55
;@ stdout: wide: 4 3 2 1 8 7 6 5 | narrow: 6 5
;@ stdout: picked: 1 2 3 4
;@ stdout: picked: 5 6 7 8
;@ stdout: call: 44 -3 0 21

@.lanes = private unnamed_addr constant [34 x i8] c"lanes: %d %d %d %d | %d %d %d %d\0A\00"
@.set = private unnamed_addr constant [38 x i8] c"set: %d %d %d %d | %d %d %d %d %d %d\0A\00"
@.mixed = private unnamed_addr constant [20 x i8] c"mixed: %g %g %g %g\0A\00"
@.odd = private unnamed_addr constant [32 x i8] c"odd: %d %d %d | %d %d %d %d %d\0A\00"
@.wide = private unnamed_addr constant [47 x i8] c"wide: %d %d %d %d %d %d %d %d | narrow: %d %d\0A\00"
@.picked = private unnamed_addr constant [21 x i8] c"picked: %d %d %d %d\0A\00"
@.call = private unnamed_addr constant [19 x i8] c"call: %d %d %d %d\0A\00"

declare i32 @printf(ptr, ...)

define void @print4(ptr %fmt, <4 x i32> %v, <4 x i32> %w) {
  %a = extractelement <4 x i32> %v, i32 0
  %b = extractelement <4 x i32> %v, i32 1
  %c = extractelement <4 x i32> %v, i32 2
  %d = extractelement <4 x i32> %v, i32 3
  %e = extractelement <4 x i32> %w, i64 0
  %f = extractelement <4 x i32> %w, i64 1
  %g = extractelement <4 x i32> %w, i64 2
  %h = extractelement <4 x i32> %w, i64 3
  call i32 (ptr, ...) @printf(ptr %fmt, i32 %a, i32 %b, i32 %c, i32 %d, i32 %e, i32 %f, i32 %g, i32 %h)
  ret void
}

; Lane %k of %v, a variable index.
define i32 @lane(<4 x i32> %v, i32 %k) {
  %r = extractelement <4 x i32> %v, i32 %k
  ret i32 %r
}

define <4 x i32> @pick(i1 %c, <4 x i32> %a, <4 x i32> %b) {
entry:
  br i1 %c, label %one, label %two
one:
  br label %join
two:
  %s = select i1 %c, <4 x i32> %b, <4 x i32> %b
  br label %join
join:
  %r = phi <4 x i32> [ %a, %one ], [ %s, %two ]
  ret <4 x i32> %r
}

define <4 x i32> @callee(<4 x i32> %v, <2 x double> %d, <8 x i8> %b) {
  %x = extractelement <2 x double> %d, i32 1
  %xi = fptosi double %x to i32
  %r = insertelement <4 x i32> %v, i32 %xi, i32 1
  %y = extractelement <8 x i8> %b, i32 5
  %yi = sext i8 %y to i32
  %s = insertelement <4 x i32> %r, i32 %yi, i32 2
  ret <4 x i32> %s
}

define i32 @main() {
entry:
  %v = insertelement <4 x i32> <i32 10, i32 poison, i32 30, i32 40>, i32 20, i32 1
  %rev = shufflevector <4 x i32> %v, <4 x i32> poison, <4 x i32> <i32 3, i32 2, i32 1, i32 0>
  call void @print4(ptr @.lanes, <4 x i32> %v, <4 x i32> %rev)

  ; insertelement with a variable index into a register vector and a
  ; slot vector (<6 x i32>).
  %k = call i32 @lane(<4 x i32> <i32 1, i32 0, i32 0, i32 0>, i32 0)
  %v2 = insertelement <4 x i32> %v, i32 99, i32 %k
  %w = insertelement <6 x i32> <i32 7, i32 -1, i32 -1, i32 -1, i32 -1, i32 0>, i32 13, i32 %k
  %w5 = shufflevector <6 x i32> %w, <6 x i32> poison, <6 x i32> <i32 0, i32 2, i32 3, i32 4, i32 5, i32 1>
  %s0 = extractelement <4 x i32> %v2, i32 0
  %s1 = extractelement <4 x i32> %v2, i32 1
  %s2 = extractelement <4 x i32> %v2, i32 2
  %s3 = extractelement <4 x i32> %v2, i32 3
  %t0 = extractelement <6 x i32> %w5, i32 0
  %t1 = extractelement <6 x i32> %w5, i32 1
  %t2 = extractelement <6 x i32> %w5, i32 2
  %t3 = extractelement <6 x i32> %w5, i32 3
  %t4 = extractelement <6 x i32> %w5, i32 4
  %t5 = extractelement <6 x i32> %w5, i32 %k
  %t5b = extractelement <6 x i32> %w5, i32 5
  call i32 (ptr, ...) @printf(ptr @.set, i32 %s0, i32 %s1, i32 %s2, i32 %s3, i32 %t0, i32 %t1, i32 %t2, i32 %t3, i32 %t4, i32 %t5b)

  ; floating-point lanes: a splat and a two-operand shuffle.
  %f = insertelement <2 x float> poison, float 1.5, i32 0
  %splat = shufflevector <2 x float> %f, <2 x float> poison, <2 x i32> zeroinitializer
  %g = shufflevector <2 x float> %splat, <2 x float> <float -2.0, float 8.25>, <4 x i32> <i32 0, i32 2, i32 1, i32 3>
  %g0 = extractelement <4 x float> %g, i32 0
  %g1 = extractelement <4 x float> %g, i32 1
  %g2 = extractelement <4 x float> %g, i32 2
  %g3 = extractelement <4 x float> %g, i32 3
  %d0 = fpext float %g0 to double
  %d1 = fpext float %g1 to double
  %d2 = fpext float %g2 to double
  %d3 = fpext float %g3 to double
  call i32 (ptr, ...) @printf(ptr @.mixed, double %d0, double %d1, double %d2, double %d3)

  ; odd lane counts (class Mem).
  %o = shufflevector <4 x i32> <i32 1, i32 2, i32 3, i32 4>, <4 x i32> poison, <3 x i32> <i32 2, i32 1, i32 0>
  %o0 = extractelement <3 x i32> %o, i32 0
  %o1 = extractelement <3 x i32> %o, i32 1
  %o2 = extractelement <3 x i32> %o, i32 2
  %p = shufflevector <3 x i8> <i8 11, i8 22, i8 33>, <3 x i8> <i8 44, i8 55, i8 66>, <5 x i32> <i32 0, i32 1, i32 2, i32 3, i32 4>
  %p0 = extractelement <5 x i8> %p, i32 0
  %p1 = extractelement <5 x i8> %p, i32 1
  %p2 = extractelement <5 x i8> %p, i32 2
  %p3 = extractelement <5 x i8> %p, i32 3
  %p4 = extractelement <5 x i8> %p, i32 %k
  %p4b = extractelement <5 x i8> %p, i32 4
  %q0 = zext i8 %p0 to i32
  %q1 = zext i8 %p1 to i32
  %q2 = zext i8 %p2 to i32
  %q3 = zext i8 %p3 to i32
  %q4 = zext i8 %p4b to i32
  call i32 (ptr, ...) @printf(ptr @.odd, i32 %o0, i32 %o1, i32 %o2, i32 %q0, i32 %q1, i32 %q2, i32 %q3, i32 %q4)

  ; widening to 8 lanes (class Mem) and narrowing back to 2 (class Fpr).
  %x = shufflevector <4 x i16> <i16 1, i16 2, i16 3, i16 4>, <4 x i16> <i16 5, i16 6, i16 7, i16 8>, <8 x i32> <i32 3, i32 2, i32 1, i32 0, i32 7, i32 6, i32 5, i32 4>
  %y = shufflevector <8 x i16> %x, <8 x i16> poison, <2 x i32> <i32 6, i32 7>
  %x0 = extractelement <8 x i16> %x, i32 0
  %x1 = extractelement <8 x i16> %x, i32 1
  %x2 = extractelement <8 x i16> %x, i32 2
  %x3 = extractelement <8 x i16> %x, i32 3
  %x4 = extractelement <8 x i16> %x, i32 4
  %x5 = extractelement <8 x i16> %x, i32 5
  %x6 = extractelement <8 x i16> %x, i32 6
  %x7 = extractelement <8 x i16> %x, i32 7
  %y0 = extractelement <2 x i16> %y, i32 0
  %y1 = extractelement <2 x i16> %y, i32 1
  %e0 = sext i16 %x0 to i32
  %e1 = sext i16 %x1 to i32
  %e2 = sext i16 %x2 to i32
  %e3 = sext i16 %x3 to i32
  %e4 = sext i16 %x4 to i32
  %e5 = sext i16 %x5 to i32
  %e6 = sext i16 %x6 to i32
  %e7 = sext i16 %x7 to i32
  %f0 = sext i16 %y0 to i32
  %f1 = sext i16 %y1 to i32
  call i32 (ptr, ...) @printf(ptr @.wide, i32 %e0, i32 %e1, i32 %e2, i32 %e3, i32 %e4, i32 %e5, i32 %e6, i32 %e7, i32 %f0, i32 %f1)

  ; phi and select of vectors.
  %c1 = trunc i32 %k to i1
  %a = call <4 x i32> @pick(i1 %c1, <4 x i32> <i32 1, i32 2, i32 3, i32 4>, <4 x i32> <i32 5, i32 6, i32 7, i32 8>)
  %na = xor i1 %c1, true
  %b = call <4 x i32> @pick(i1 %na, <4 x i32> <i32 1, i32 2, i32 3, i32 4>, <4 x i32> <i32 5, i32 6, i32 7, i32 8>)
  %a0 = extractelement <4 x i32> %a, i32 0
  %a1 = extractelement <4 x i32> %a, i32 1
  %a2 = extractelement <4 x i32> %a, i32 2
  %a3 = extractelement <4 x i32> %a, i32 3
  call i32 (ptr, ...) @printf(ptr @.picked, i32 %a0, i32 %a1, i32 %a2, i32 %a3)
  %b0 = extractelement <4 x i32> %b, i32 0
  %b1 = extractelement <4 x i32> %b, i32 1
  %b2 = extractelement <4 x i32> %b, i32 2
  %b3 = extractelement <4 x i32> %b, i32 3
  call i32 (ptr, ...) @printf(ptr @.picked, i32 %b0, i32 %b1, i32 %b2, i32 %b3)

  ; register vectors through a call.
  %r = call <4 x i32> @callee(<4 x i32> <i32 44, i32 0, i32 0, i32 21>, <2 x double> <double 9.5, double -3.75>, <8 x i8> <i8 0, i8 0, i8 0, i8 0, i8 0, i8 0, i8 0, i8 0>)
  %r0 = extractelement <4 x i32> %r, i32 0
  %r1 = extractelement <4 x i32> %r, i32 1
  %r2 = extractelement <4 x i32> %r, i32 2
  %r3 = extractelement <4 x i32> %r, i32 3
  call i32 (ptr, ...) @printf(ptr @.call, i32 %r0, i32 %r1, i32 %r2, i32 %r3)
  ret i32 0
}
