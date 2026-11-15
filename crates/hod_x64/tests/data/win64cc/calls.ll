; WP-246 (`spec/abi-x64.md` §13): `win64cc` definitions and calls in both
; directions with the default convention, direct and through pointers.
; `tests/win64cc.rs` builds this module three times: by hod with `$S` = `_h`
; and `$O` = `_l`, by `llc` 22 with `$S` = `_l` and `$O` = `_h`, and by
; `llc` 19 with both `_r` (the reference): each `s_drive` calls the other
; build's `win64cc` functions. `v_sum` (variadic, `va_arg` over the home
; area) is `llc`'s alone (`va.ll`).

declare win64cc double @v_sum(i32, ...)

declare win64cc i64 @w_many$O(i32, double, i64, float, i64, double, i8, i64)
declare win64cc i128 @w_agg$O(i128, i64, i128, { i64, double }, [2 x i32])
declare win64cc void @w_sret$O(ptr sret({ i64, i64, i64 }), i64, double)
declare win64cc { i64, i64, i64, i64 } @w_mem$O(i64, double)
declare win64cc double @w_back$O(ptr, i64)
declare win64cc double @w_vcall$O(i64)
declare win64cc zeroext i8 @w_ext$O(i8 zeroext, i16 signext, i1 zeroext, i8 zeroext, i16 signext)

; Eight scalars: positions 0..3 in rcx/xmm1/r8/xmm3, the rest on the
; stack from offset 32.
define win64cc i64 @w_many$S(i32 %a, double %b, i64 %c, float %d, i64 %e, double %f, i8 %h, i64 %k) {
  %a1 = sext i32 %a to i64
  %b4 = fmul double %b, 4.0
  %b1 = fptosi double %b4 to i64
  %c1 = mul i64 %c, 5
  %d8 = fmul float %d, 8.0
  %d1 = fptosi float %d8 to i64
  %e1 = mul i64 %e, 11
  %f16 = fmul double %f, 16.0
  %f1 = fptosi double %f16 to i64
  %h0 = sext i8 %h to i64
  %h1 = mul i64 %h0, 23
  %k1 = mul i64 %k, 29
  %s1 = add i64 %a1, %b1
  %s2 = add i64 %s1, %c1
  %s3 = add i64 %s2, %d1
  %s4 = add i64 %s3, %e1
  %s5 = add i64 %s4, %f1
  %s6 = add i64 %s5, %h1
  %s7 = add i64 %s6, %k1
  ret i64 %s7
}

; Each half of an `i128` takes a position: %g in rcx:rdx, %c in r8, %g2 in
; r9 and the first stack slot; the aggregates' members on the stack.
define win64cc i128 @w_agg$S(i128 %g, i64 %c, i128 %g2, { i64, double } %s, [2 x i32] %t) {
  %g3 = mul i128 %g2, 3
  %c1 = sext i64 %c to i128
  %s0 = extractvalue { i64, double } %s, 0
  %s1 = extractvalue { i64, double } %s, 1
  %s0w = sext i64 %s0 to i128
  %s1i = fptosi double %s1 to i128
  %t0 = extractvalue [2 x i32] %t, 0
  %t1 = extractvalue [2 x i32] %t, 1
  %t0w = sext i32 %t0 to i128
  %t1w = mul i32 %t1, 7
  %t1x = sext i32 %t1w to i128
  %r1 = add i128 %g, %g3
  %r2 = add i128 %r1, %c1
  %r3 = add i128 %r2, %s0w
  %r4 = add i128 %r3, %s1i
  %r5 = add i128 %r4, %t0w
  %r6 = add i128 %r5, %t1x
  ret i128 %r6
}

define win64cc void @w_sret$S(ptr sret({ i64, i64, i64 }) %p, i64 %a, double %b) {
  %b1 = fptosi double %b to i64
  %a2 = mul i64 %a, 2
  store i64 %a, ptr %p
  %p1 = getelementptr i8, ptr %p, i64 8
  store i64 %b1, ptr %p1
  %p2 = getelementptr i8, ptr %p, i64 16
  store i64 %a2, ptr %p2
  ret void
}

; Returned in memory: the address in rcx, %a in rdx, %b in xmm2.
define win64cc { i64, i64, i64, i64 } @w_mem$S(i64 %a, double %b) {
  %b1 = fptosi double %b to i64
  %a1 = add i64 %a, 1
  %r0 = insertvalue { i64, i64, i64, i64 } poison, i64 %a, 0
  %r1 = insertvalue { i64, i64, i64, i64 } %r0, i64 %b1, 1
  %r2 = insertvalue { i64, i64, i64, i64 } %r1, i64 %a1, 2
  %r3 = insertvalue { i64, i64, i64, i64 } %r2, i64 7, 3
  ret { i64, i64, i64, i64 } %r3
}

; A `win64cc` function calling a System V one through a pointer.
define win64cc double @w_back$S(ptr %fp, i64 %x) {
  %r = call double %fp(i64 %x, double 2.5)
  %r2 = fmul double %r, 2.0
  ret double %r2
}

; A `win64cc` function calling a variadic `win64cc` one.
define win64cc double @w_vcall$S(i64 %x) {
  %r = call win64cc double (i32, ...) @v_sum(i32 3, i64 %x, double 2.5, i64 3, double 4.5, i64 5, double 6.5)
  ret double %r
}

; The extensions (§13.3): the callee extends what it uses itself.
define win64cc zeroext i8 @w_ext$S(i8 zeroext %a, i16 signext %b, i1 zeroext %c, i8 zeroext %d, i16 signext %e) {
  %a1 = zext i8 %a to i32
  %b1 = sext i16 %b to i32
  %c1 = zext i1 %c to i32
  %d1 = zext i8 %d to i32
  %e1 = sext i16 %e to i32
  %s1 = add i32 %a1, %b1
  %s2 = add i32 %s1, %c1
  %s3 = add i32 %s2, %d1
  %s4 = add i32 %s3, %e1
  %r = trunc i32 %s4 to i8
  ret i8 %r
}

define double @s_helper$S(i64 %x, double %d) {
  %x1 = sitofp i64 %x to double
  %r = fadd double %x1, %d
  ret double %r
}

; The System V side: every `win64cc` function of the other build, one of
; them through the pointer %wfp, and the variadic one.
define i64 @s_drive$S(ptr %wfp, ptr %cell) {
  %v = load i64, ptr %cell
  %r1 = call win64cc i64 @w_many$O(i32 -3, double 1.25, i64 %v, float 0.5, i64 -7, double 3.0625, i8 -5, i64 100)
  %r2 = call win64cc i64 %wfp(i32 9, double -2.5, i64 4, float 1.5, i64 %v, double 0.25, i8 120, i64 -1)
  %g = mul i128 18446744073709551621, 3
  %ra = call win64cc i128 @w_agg$O(i128 %g, i64 %v, i128 -77, { i64, double } { i64 13, double 9.75 }, [2 x i32] [i32 -4, i32 6])
  %ra0 = trunc i128 %ra to i64
  %ra1s = lshr i128 %ra, 64
  %ra1 = trunc i128 %ra1s to i64
  %buf = alloca { i64, i64, i64 }
  call win64cc void @w_sret$O(ptr sret({ i64, i64, i64 }) %buf, i64 %v, double 17.5)
  %q0 = load i64, ptr %buf
  %bp1 = getelementptr i8, ptr %buf, i64 8
  %q1 = load i64, ptr %bp1
  %bp2 = getelementptr i8, ptr %buf, i64 16
  %q2 = load i64, ptr %bp2
  %m = call win64cc { i64, i64, i64, i64 } @w_mem$O(i64 %v, double 6.5)
  %m0 = extractvalue { i64, i64, i64, i64 } %m, 0
  %m1 = extractvalue { i64, i64, i64, i64 } %m, 1
  %m2 = extractvalue { i64, i64, i64, i64 } %m, 2
  %m3 = extractvalue { i64, i64, i64, i64 } %m, 3
  %bk = call win64cc double @w_back$O(ptr @s_helper$S, i64 %v)
  %bki = fptosi double %bk to i64
  %vs = call win64cc double (i32, ...) @v_sum(i32 2, i64 %v, double 0.5, i64 8, double 1.25)
  %vsi = fptosi double %vs to i64
  %vc = call win64cc double @w_vcall$O(i64 %v)
  %vci = fptosi double %vc to i64
  %vt = trunc i64 %v to i8
  %vt16 = trunc i64 %v to i16
  %ex = call win64cc zeroext i8 @w_ext$O(i8 zeroext %vt, i16 signext -300, i1 zeroext true, i8 zeroext 200, i16 signext %vt16)
  %exi = zext i8 %ex to i64
  %t1 = mul i64 %r1, 3
  %t2 = add i64 %t1, %r2
  %t3 = mul i64 %t2, 5
  %t4 = add i64 %t3, %ra0
  %t5 = add i64 %t4, %ra1
  %t6 = mul i64 %t5, 7
  %t7 = add i64 %t6, %q0
  %t8 = add i64 %t7, %q1
  %t9 = add i64 %t8, %q2
  %t10 = mul i64 %t9, 11
  %t11 = add i64 %t10, %m0
  %t12 = add i64 %t11, %m1
  %t13 = add i64 %t12, %m2
  %t14 = add i64 %t13, %m3
  %t15 = mul i64 %t14, 13
  %t16 = add i64 %t15, %bki
  %t17 = mul i64 %t16, 17
  %t18 = add i64 %t17, %vsi
  %t19 = mul i64 %t18, 19
  %t20 = add i64 %t19, %vci
  %t21 = mul i64 %t20, 23
  %t22 = add i64 %t21, %exi
  ret i64 %t22
}
