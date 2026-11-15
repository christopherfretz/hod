; WP-247 (`spec/abi-a64.md` §13): `win64cc` and `aarch64_vector_pcs`
; functions of many signatures, built by hod and by `llc`; `caller.ll`
; calls them, directly and through pointers.

; Eight general and eight vector registers, then the stack.
define win64cc i64 @w_many(i64 %a, i64 %b, i64 %c, i64 %d, i64 %e, i64 %f, i64 %g, i64 %h,
                           i64 %s1, i32 %s2, double %d0, double %d1, double %d2, double %d3,
                           double %d4, double %d5, double %d6, double %d7, double %s3,
                           float %s4, i8 signext %s5) {
  %t1 = add i64 %a, %b
  %t2 = mul i64 %t1, %c
  %t3 = sub i64 %t2, %d
  %t4 = xor i64 %t3, %e
  %t5 = add i64 %t4, %f
  %t6 = mul i64 %t5, %g
  %t7 = add i64 %t6, %h
  %t8 = mul i64 %t7, 3
  %t9 = add i64 %t8, %s1
  %w2 = sext i32 %s2 to i64
  %u0 = add i64 %t9, %w2
  %f0 = fadd double %d0, %d1
  %f1 = fmul double %f0, %d2
  %f2 = fsub double %f1, %d3
  %f3 = fadd double %f2, %d4
  %f4 = fmul double %f3, %d5
  %f5 = fadd double %f4, %d6
  %f6 = fsub double %f5, %d7
  %f7 = fmul double %f6, %s3
  %fw = fpext float %s4 to double
  %f8 = fadd double %f7, %fw
  %fi = fptosi double %f8 to i64
  %u1 = mul i64 %u0, 1000003
  %u2 = add i64 %u1, %fi
  %w5 = sext i8 %s5 to i64
  %u3 = add i64 %u2, %w5
  ret i64 %u3
}

; `sret` and an array of `double`s.
define win64cc void @w_sret(ptr sret([5 x i64]) %out, i64 %x, [2 x double] %h) {
  %h0 = extractvalue [2 x double] %h, 0
  %h1 = extractvalue [2 x double] %h, 1
  %i0 = fptosi double %h0 to i64
  %i1 = fptosi double %h1 to i64
  store i64 %x, ptr %out
  %p1 = getelementptr i64, ptr %out, i64 1
  store i64 %i0, ptr %p1
  %p2 = getelementptr i64, ptr %out, i64 2
  store i64 %i1, ptr %p2
  %p3 = getelementptr i64, ptr %out, i64 3
  %x3 = mul i64 %x, 3
  store i64 %x3, ptr %p3
  %p4 = getelementptr i64, ptr %out, i64 4
  store i64 -1, ptr %p4
  ret void
}

; Arrays of `float`s in, an array of `double`s out, an `i128` at an
; even register.
define win64cc [2 x double] @w_hfa(i64 %k, [3 x float] %f, i128 %q) {
  %f0 = extractvalue [3 x float] %f, 0
  %f1 = extractvalue [3 x float] %f, 1
  %f2 = extractvalue [3 x float] %f, 2
  %s = fadd float %f0, %f1
  %t = fmul float %s, %f2
  %td = fpext float %t to double
  %qt = trunc i128 %q to i64
  %qh = lshr i128 %q, 64
  %qhi = trunc i128 %qh to i64
  %qs = add i64 %qt, %qhi
  %qk = add i64 %qs, %k
  %qd = sitofp i64 %qk to double
  %r0 = insertvalue [2 x double] poison, double %td, 0
  %r1 = insertvalue [2 x double] %r0, double %qd, 1
  ret [2 x double] %r1
}

define win64cc [2 x i64] @w_pair(i64 %a, i64 %b) {
  %s = add i64 %a, %b
  %d = sub i64 %a, %b
  %r0 = insertvalue [2 x i64] poison, i64 %s, 0
  %r1 = insertvalue [2 x i64] %r0, i64 %d, 1
  ret [2 x i64] %r1
}

define win64cc float @w_f(float %a, double %b) {
  %bf = fptrunc double %b to float
  %r = fmul float %a, %bf
  ret float %r
}

define win64cc <4 x i32> @w_vec(<4 x i32> %a, <2 x double> %b) {
  %bi = fptosi <2 x double> %b to <2 x i32>
  %bw = shufflevector <2 x i32> %bi, <2 x i32> zeroinitializer, <4 x i32> <i32 0, i32 1, i32 0, i32 1>
  %r = add <4 x i32> %a, %bw
  ret <4 x i32> %r
}

; The corpus module's shape: `double`s, an array, a pointer.
define aarch64_vector_pcs double @v_mix(double %a, double %b, [2 x i64] %c, ptr %p) {
  %c0 = extractvalue [2 x i64] %c, 0
  %c1 = extractvalue [2 x i64] %c, 1
  %cb = bitcast i64 %c1 to double
  %ci = sitofp i64 %c0 to double
  %l = load i64, ptr %p
  %ld = sitofp i64 %l to double
  %m = fmul double %a, %b
  %s1 = fadd double %m, %ci
  %s2 = fadd double %s1, %cb
  %s3 = fadd double %s2, %ld
  ret double %s3
}

define aarch64_vector_pcs <2 x double> @v_vec(<2 x double> %a, i64 %k, <4 x float> %b) {
  %kd = sitofp i64 %k to double
  %ks = insertelement <2 x double> poison, double %kd, i32 0
  %kv = shufflevector <2 x double> %ks, <2 x double> poison, <2 x i32> zeroinitializer
  %m = fmul <2 x double> %a, %kv
  %b0 = extractelement <4 x float> %b, i32 3
  %bd = fpext float %b0 to double
  %r = insertelement <2 x double> %m, double %bd, i32 1
  ret <2 x double> %r
}

; Returned in memory through `x8`.
define aarch64_vector_pcs [9 x i64] @v_mem(i64 %k) {
  %a0 = insertvalue [9 x i64] poison, i64 %k, 0
  %k1 = add i64 %k, 1
  %a1 = insertvalue [9 x i64] %a0, i64 %k1, 1
  %k8 = mul i64 %k, 8
  %a8 = insertvalue [9 x i64] %a1, i64 %k8, 8
  %a2 = insertvalue [9 x i64] %a8, i64 2, 2
  %a3 = insertvalue [9 x i64] %a2, i64 3, 3
  %a4 = insertvalue [9 x i64] %a3, i64 4, 4
  %a5 = insertvalue [9 x i64] %a4, i64 5, 5
  %a6 = insertvalue [9 x i64] %a5, i64 6, 6
  %a7 = insertvalue [9 x i64] %a6, i64 7, 7
  ret [9 x i64] %a7
}
