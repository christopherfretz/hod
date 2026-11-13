; `preserve_mostcc` and `preserve_allcc` definitions and calls: the
; arguments and results are `ccc`'s; the callees keep more registers,
; which a caller compiled as for `ccc` never relies on. Values live
; across the calls, results of each class, a void callee, a recursive
; one, and one calling a `ccc` function.
;@ covers: spec/ir.md §8.6 calling conventions (preserve_mostcc, preserve_allcc); spec/codegen.md §5.6.23
;@ stdout: most 13 -7 30 1
;@ stdout: all 2.5 -4 9 6
;@ stdout: rec 55 120
;@ stdout: sink 42

@.most = private unnamed_addr constant [21 x i8] c"most %ld %ld %ld %d\0A\00"
@.all = private unnamed_addr constant [21 x i8] c"all %.1f %ld %ld %d\0A\00"
@.rec = private unnamed_addr constant [13 x i8] c"rec %ld %ld\0A\00"
@.sink = private unnamed_addr constant [10 x i8] c"sink %ld\0A\00"
@sink = internal global i64 0

declare i32 @printf(ptr, ...)

define internal i64 @plain_add(i64 %a, i64 %b) {
entry:
  %s = add i64 %a, %b
  ret i64 %s
}

define internal preserve_mostcc i64 @m_add(i64 %a, i64 %b) {
entry:
  %s = call i64 @plain_add(i64 %a, i64 %b)
  ret i64 %s
}

define internal preserve_mostcc { i64, i64 } @m_pair(i64 %a, i64 %b) {
entry:
  %d = sub i64 %b, %a
  %r0 = insertvalue { i64, i64 } poison, i64 %d, 0
  %m = mul i64 %a, %b
  %r1 = insertvalue { i64, i64 } %r0, i64 %m, 1
  ret { i64, i64 } %r1
}

define internal preserve_mostcc i1 @m_less(i32 %a, i32 %b) {
entry:
  %c = icmp slt i32 %a, %b
  ret i1 %c
}

define internal preserve_mostcc void @m_store(i64 %v) {
entry:
  store i64 %v, ptr @sink, align 8
  ret void
}

define internal preserve_mostcc i64 @m_fib(i64 %n) {
entry:
  %small = icmp slt i64 %n, 2
  br i1 %small, label %done, label %more
more:
  %n1 = sub i64 %n, 1
  %n2 = sub i64 %n, 2
  %f1 = call preserve_mostcc i64 @m_fib(i64 %n1)
  %f2 = call preserve_mostcc i64 @m_fib(i64 %n2)
  %f = add i64 %f1, %f2
  ret i64 %f
done:
  ret i64 %n
}

define internal preserve_allcc double @a_half(double %x) {
entry:
  %h = fmul double %x, 5.000000e-01
  ret double %h
}

define internal preserve_allcc <4 x float> @a_scale(<4 x float> %v, float %k) {
entry:
  %i0 = insertelement <4 x float> poison, float %k, i32 0
  %s = shufflevector <4 x float> %i0, <4 x float> poison, <4 x i32> zeroinitializer
  %r = fmul <4 x float> %v, %s
  ret <4 x float> %r
}

define internal preserve_allcc i64 @a_mix(i64 %a, double %d) {
entry:
  %c = call i64 @plain_add(i64 %a, i64 %a)
  %f = fptosi double %d to i64
  %s = add i64 %c, %f
  ret i64 %s
}

define internal preserve_allcc i64 @a_fact(i64 %n) {
entry:
  %small = icmp slt i64 %n, 2
  br i1 %small, label %done, label %more
more:
  %n1 = sub i64 %n, 1
  %f1 = call preserve_allcc i64 @a_fact(i64 %n1)
  %f = mul i64 %n, %f1
  ret i64 %f
done:
  ret i64 1
}

define i32 @main() {
entry:
  %a = call preserve_mostcc i64 @m_add(i64 6, i64 7)
  %p = call preserve_mostcc { i64, i64 } @m_pair(i64 5, i64 -2)
  %p0 = extractvalue { i64, i64 } %p, 0
  %p1 = extractvalue { i64, i64 } %p, 1
  %m = mul i64 %p1, -3
  %l = call preserve_mostcc i1 @m_less(i32 -1, i32 3)
  %lz = zext i1 %l to i32
  %r1 = call i32 (ptr, ...) @printf(ptr @.most, i64 %a, i64 %p0, i64 %m, i32 %lz)
  %h = call preserve_allcc double @a_half(double 5.000000e+00)
  %v = call preserve_allcc <4 x float> @a_scale(<4 x float> <float 1.0, float -2.0, float 3.0, float 4.5>, float 2.0)
  %v1 = extractelement <4 x float> %v, i32 1
  %v1i = fptosi float %v1 to i64
  %x = call preserve_allcc i64 @a_mix(i64 3, double 3.75)
  %v3 = extractelement <4 x float> %v, i32 2
  %v3i = fptosi float %v3 to i32
  %r2 = call i32 (ptr, ...) @printf(ptr @.all, double %h, i64 %v1i, i64 %x, i32 %v3i)
  %f = call preserve_mostcc i64 @m_fib(i64 10)
  %g = call preserve_allcc i64 @a_fact(i64 5)
  %r3 = call i32 (ptr, ...) @printf(ptr @.rec, i64 %f, i64 %g)
  call preserve_mostcc void @m_store(i64 42)
  %s = load i64, ptr @sink, align 8
  %r4 = call i32 (ptr, ...) @printf(ptr @.sink, i64 %s)
  ret i32 0
}
