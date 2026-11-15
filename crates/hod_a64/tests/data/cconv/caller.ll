; WP-247 (`spec/abi-a64.md` §13): calls of the functions of `callee.ll`
; and `va.ll`, directly and through pointers. Each `t*` returns a number
; the C driver (`calls.c`) prints.

declare win64cc i64 @w_many(i64, i64, i64, i64, i64, i64, i64, i64, i64, i32, double, double,
                            double, double, double, double, double, double, double, float,
                            i8 signext)
declare win64cc void @w_sret(ptr sret([5 x i64]), i64, [2 x double])
declare win64cc [2 x double] @w_hfa(i64, [3 x float], i128)
declare win64cc [2 x i64] @w_pair(i64, i64)
declare win64cc float @w_f(float, double)
declare win64cc <4 x i32> @w_vec(<4 x i32>, <2 x double>)
declare aarch64_vector_pcs double @v_mix(double, double, [2 x i64], ptr)
declare aarch64_vector_pcs <2 x double> @v_vec(<2 x double>, i64, <4 x float>)
declare aarch64_vector_pcs [9 x i64] @v_mem(i64)
declare win64cc double @v_sum(i64, ...)
declare win64cc double @v_sum2(double, i64, ...)

@p_many = global ptr @w_many
@p_sret = global ptr @w_sret
@p_hfa = global ptr @w_hfa
@p_mix = global ptr @v_mix
@p_mem = global ptr @v_mem
@p_sum = global ptr @v_sum
@p_sum2 = global ptr @v_sum2

define i64 @t_many(i64 %k) {
  %k2 = add i64 %k, 2
  %kd = sitofp i64 %k to double
  %kf = sitofp i64 %k to float
  %r = call win64cc i64 @w_many(i64 %k, i64 2, i64 3, i64 4, i64 5, i64 6, i64 7, i64 8,
                                i64 9, i32 -10, double %kd, double 1.5, double 2.5, double 3.5,
                                double 4.5, double 5.5, double 6.5, double 7.5, double 0.5,
                                float %kf, i8 signext -3)
  %p = load ptr, ptr @p_many
  %q = call win64cc i64 %p(i64 %k2, i64 2, i64 3, i64 4, i64 5, i64 6, i64 7, i64 8,
                           i64 9, i32 -10, double %kd, double 1.5, double 2.5, double 3.5,
                           double 4.5, double 5.5, double 6.5, double 7.5, double 0.5,
                           float %kf, i8 signext -3)
  %s = mul i64 %r, 31
  %t = add i64 %s, %q
  ret i64 %t
}

define i64 @t_sret(i64 %k) {
  %buf = alloca [5 x i64], align 8
  %kd = sitofp i64 %k to double
  %h0 = insertvalue [2 x double] poison, double %kd, 0
  %h = insertvalue [2 x double] %h0, double -7.0, 1
  call win64cc void @w_sret(ptr sret([5 x i64]) %buf, i64 %k, [2 x double] %h)
  %a = load [5 x i64], ptr %buf
  %p = load ptr, ptr @p_sret
  %k1 = add i64 %k, 1
  call win64cc void %p(ptr sret([5 x i64]) %buf, i64 %k1, [2 x double] %h)
  %b = load [5 x i64], ptr %buf
  %a0 = extractvalue [5 x i64] %a, 0
  %a1 = extractvalue [5 x i64] %a, 1
  %a2 = extractvalue [5 x i64] %a, 2
  %a3 = extractvalue [5 x i64] %a, 3
  %a4 = extractvalue [5 x i64] %a, 4
  %b0 = extractvalue [5 x i64] %b, 0
  %s1 = mul i64 %a0, 7
  %s2 = add i64 %s1, %a1
  %s3 = mul i64 %s2, 7
  %s4 = add i64 %s3, %a2
  %s5 = mul i64 %s4, 7
  %s6 = add i64 %s5, %a3
  %s7 = mul i64 %s6, 7
  %s8 = add i64 %s7, %a4
  %s9 = mul i64 %s8, 7
  %s10 = add i64 %s9, %b0
  ret i64 %s10
}

define i64 @t_hfa(i64 %k) {
  %kf = sitofp i64 %k to float
  %f0 = insertvalue [3 x float] poison, float %kf, 0
  %f1 = insertvalue [3 x float] %f0, float 0.25, 1
  %f = insertvalue [3 x float] %f1, float 4.0, 2
  %kq = zext i64 %k to i128
  %q = add i128 %kq, 18446744073709551616
  %r = call win64cc [2 x double] @w_hfa(i64 %k, [3 x float] %f, i128 %q)
  %p = load ptr, ptr @p_hfa
  %r2 = call win64cc [2 x double] %p(i64 1, [3 x float] %f, i128 %q)
  %r0 = extractvalue [2 x double] %r, 0
  %r1 = extractvalue [2 x double] %r, 1
  %r21 = extractvalue [2 x double] %r2, 1
  %s = fadd double %r0, %r1
  %s2 = fmul double %s, 100.0
  %s3 = fadd double %s2, %r21
  %i = fptosi double %s3 to i64
  ret i64 %i
}

define i64 @t_small(i64 %k) {
  %pr = call win64cc [2 x i64] @w_pair(i64 %k, i64 5)
  %p0 = extractvalue [2 x i64] %pr, 0
  %p1 = extractvalue [2 x i64] %pr, 1
  %kf = sitofp i64 %k to float
  %f = call win64cc float @w_f(float %kf, double 2.5)
  %fi = fptosi float %f to i64
  %v = call win64cc <4 x i32> @w_vec(<4 x i32> <i32 1, i32 2, i32 3, i32 4>, <2 x double> <double 10.0, double 20.0>)
  %v3 = extractelement <4 x i32> %v, i32 3
  %v2 = extractelement <4 x i32> %v, i32 2
  %vs = add i32 %v3, %v2
  %vw = sext i32 %vs to i64
  %s1 = mul i64 %p0, 1000
  %s2 = add i64 %s1, %p1
  %s3 = mul i64 %s2, 1000
  %s4 = add i64 %s3, %fi
  %s5 = mul i64 %s4, 1000
  %s6 = add i64 %s5, %vw
  ret i64 %s6
}

define i64 @t_vpcs(i64 %k) {
  %cell = alloca i64, align 8
  store i64 %k, ptr %cell
  %c1 = insertvalue [2 x i64] poison, i64 10, 0
  %c = insertvalue [2 x i64] %c1, i64 4611686018427387904, 1
  %m = call aarch64_vector_pcs double @v_mix(double 4.0, double 5.0, [2 x i64] %c, ptr %cell)
  %p = load ptr, ptr @p_mix
  %m2 = call aarch64_vector_pcs double %p(double 1.0, double 5.0, [2 x i64] %c, ptr %cell)
  %kd = sitofp i64 %k to double
  %kv = insertelement <2 x double> <double 1.0, double 3.0>, double %kd, i32 1
  %v = call aarch64_vector_pcs <2 x double> @v_vec(<2 x double> %kv, i64 3, <4 x float> <float 1.0, float 2.0, float 3.0, float 0.5>)
  %v0 = extractelement <2 x double> %v, i32 0
  %v1 = extractelement <2 x double> %v, i32 1
  %pm = load ptr, ptr @p_mem
  %a = call aarch64_vector_pcs [9 x i64] %pm(i64 %k)
  %b = call aarch64_vector_pcs [9 x i64] @v_mem(i64 3)
  %a8 = extractvalue [9 x i64] %a, 8
  %a1 = extractvalue [9 x i64] %a, 1
  %b7 = extractvalue [9 x i64] %b, 7
  %d1 = fadd double %m, %m2
  %d2 = fmul double %d1, 10.0
  %d3 = fadd double %d2, %v0
  %d4 = fmul double %d3, 10.0
  %d5 = fadd double %d4, %v1
  %di = fptosi double %d5 to i64
  %s1 = mul i64 %di, 1000
  %s2 = add i64 %s1, %a8
  %s3 = mul i64 %s2, 100
  %s4 = add i64 %s3, %a1
  %s5 = mul i64 %s4, 10
  %s6 = add i64 %s5, %b7
  ret i64 %s6
}

; Variadic calls: three triples fill x1..x7 and two stack slots
; (`v_sum`); a fixed `double` in x0 (`v_sum2`); through pointers too.
define i64 @t_va(i64 %k) {
  %kd = sitofp i64 %k to double
  %kf = sitofp i64 %k to float
  %a = call win64cc double (i64, ...) @v_sum(i64 3, i64 %k, double 0.5, float 0.25,
                                             i64 2, double %kd, float 1.5,
                                             i64 -3, double 4.0, float %kf)
  %p = load ptr, ptr @p_sum
  %b = call win64cc double (i64, ...) %p(i64 1, i64 7, double -1.5, float 8.0)
  %c = call win64cc double (double, i64, ...) @v_sum2(double %kd, i64 2, i64 1, double 2.0,
                                                      float 3.0, i64 4, double 5.0, float 6.0)
  %p2 = load ptr, ptr @p_sum2
  %d = call win64cc double (double, i64, ...) %p2(double 0.75, i64 3, i64 %k, double 0.5,
                                                  float 0.25, i64 2, double %kd, float 1.5,
                                                  i64 -3, double 4.0, float %kf)
  %s1 = fmul double %a, 100.0
  %s2 = fadd double %s1, %b
  %s3 = fmul double %s2, 100.0
  %s4 = fadd double %s3, %c
  %s5 = fmul double %s4, 100.0
  %s6 = fadd double %s5, %d
  %s7 = fmul double %s6, 4.0
  %i = fptosi double %s7 to i64
  ret i64 %i
}
