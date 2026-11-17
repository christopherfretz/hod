; i386 join E: edge values through every lane of WP-317's vector
; operations (§17) at SSE2 (`"target-cpu"="pentium4"`). Each family has
; a grid of edge values (integers: zero, one, minus one, the minimum, the
; maximum, a bit pattern, a small negative, a power of two; floats: both
; zeros, one, minus one, the largest finite, the smallest normal, the
; smallest and largest subnormals, both infinities, a quiet and a
; signalling NaN); vectors are windows of the grid (`@g* + k` against
; `@g* + m` for every `k`, `m`), so every lane meets every grid value in
; every other lane position. Divisors come from a grid without zero and
; without -1 (a trap, or undefined, in both compilers); shift counts are
; masked to the lane width. Results are printed as bits.
target triple = "i686-unknown-linux-gnu"

@gi = internal global [11 x i32] [i32 0, i32 1, i32 -1, i32 -2147483648, i32 2147483647, i32 1431655765, i32 -7, i32 65536, i32 0, i32 1, i32 -1], align 4
@gdi = internal global [11 x i32] [i32 1, i32 2, i32 -3, i32 7, i32 2147483647, i32 -2147483648, i32 -2, i32 65537, i32 1, i32 2, i32 -3], align 4
@gh = internal global [15 x i16] [i16 0, i16 1, i16 -1, i16 -32768, i16 32767, i16 21845, i16 -7, i16 256, i16 0, i16 1, i16 -1, i16 -32768, i16 32767, i16 21845, i16 -7], align 2
@gdh = internal global [15 x i16] [i16 1, i16 2, i16 -3, i16 7, i16 32767, i16 -32768, i16 -2, i16 255, i16 1, i16 2, i16 -3, i16 7, i16 32767, i16 -32768, i16 -2], align 2
@gb = internal global [23 x i8] [i8 0, i8 1, i8 -1, i8 -128, i8 127, i8 85, i8 -7, i8 16, i8 0, i8 1, i8 -1, i8 -128, i8 127, i8 85, i8 -7, i8 16, i8 0, i8 1, i8 -1, i8 -128, i8 127, i8 85, i8 -7], align 1
@gdb = internal global [23 x i8] [i8 1, i8 2, i8 -3, i8 7, i8 127, i8 -128, i8 -2, i8 15, i8 1, i8 2, i8 -3, i8 7, i8 127, i8 -128, i8 -2, i8 15, i8 1, i8 2, i8 -3, i8 7, i8 127, i8 -128, i8 -2], align 1
@gq = internal global [9 x i64] [i64 0, i64 1, i64 -1, i64 -9223372036854775808, i64 9223372036854775807, i64 6148914691236517205, i64 -7, i64 4294967296, i64 0], align 8
; float bits: 0, -0, 1, -1, max, min normal, min subnormal, -max subnormal,
; inf, -inf, qNaN, sNaN, then the first three again.
@gf = internal global [15 x i32] [i32 0, i32 -2147483648, i32 1065353216, i32 -1082130432, i32 2139095039, i32 8388608, i32 1, i32 -2139095041, i32 2139095040, i32 -8388608, i32 2143289344, i32 2141192193, i32 0, i32 -2147483648, i32 1065353216], align 4
@gd = internal global [13 x i64] [i64 0, i64 -9223372036854775808, i64 4607182418800017408, i64 -4616189618054758400, i64 9218868437227405311, i64 4503599627370496, i64 1, i64 -9218868437227405313, i64 9218868437227405312, i64 -4503599627370496, i64 9221120237041090560, i64 9218868437227405313, i64 0], align 8
; in range of `i32` for `fptosi`: 0, -0, 1.5, -2.5, 1e9, -2^31, 0.75, -1
@gc = internal global [7 x float] [float 0.0, float -0.0, float 1.5, float -2.5, float 1.0e9, float -2147483648.0, float 0.75], align 4
@fmt = private constant [30 x i8] c"%d %d %d %08x %08x %08x %08x\0A\00"

declare i32 @printf(ptr, ...)
declare i32 @llvm.vector.reduce.or.v4i32(<4 x i32>)
declare i32 @llvm.vector.reduce.add.v4i32(<4 x i32>)
declare i32 @llvm.vector.reduce.mul.v4i32(<4 x i32>)
declare i32 @llvm.vector.reduce.and.v4i32(<4 x i32>)
declare i32 @llvm.vector.reduce.xor.v4i32(<4 x i32>)
declare i32 @llvm.vector.reduce.smax.v4i32(<4 x i32>)
declare i32 @llvm.vector.reduce.smin.v4i32(<4 x i32>)
declare i32 @llvm.vector.reduce.umax.v4i32(<4 x i32>)
declare i32 @llvm.vector.reduce.umin.v4i32(<4 x i32>)
declare i16 @llvm.vector.reduce.smax.v8i16(<8 x i16>)
declare i8 @llvm.vector.reduce.umin.v16i8(<16 x i8>)

define internal void @p4(i32 %tag, i32 %k, i32 %m, <4 x i32> %v) noinline #0 {
  %a = extractelement <4 x i32> %v, i32 0
  %b = extractelement <4 x i32> %v, i32 1
  %c = extractelement <4 x i32> %v, i32 2
  %d = extractelement <4 x i32> %v, i32 3
  call i32 (ptr, ...) @printf(ptr @fmt, i32 %tag, i32 %k, i32 %m, i32 %a, i32 %b, i32 %c, i32 %d)
  ret void
}

; `acc | (the four lanes of mask %s as bits) << %sh`.
define internal i32 @comb(i32 %acc, <4 x i32> %s, i32 %sh) noinline #0 {
  %a = and <4 x i32> %s, <i32 1, i32 2, i32 4, i32 8>
  %r = call i32 @llvm.vector.reduce.or.v4i32(<4 x i32> %a)
  %t = shl i32 %r, %sh
  %o = or i32 %acc, %t
  ret i32 %o
}

define internal void @i32ops(i32 %k, i32 %m) #0 {
  %pa = getelementptr i32, ptr @gi, i32 %k
  %pb = getelementptr i32, ptr @gi, i32 %m
  %pd = getelementptr i32, ptr @gdi, i32 %m
  %a = load <4 x i32>, ptr %pa, align 4
  %b = load <4 x i32>, ptr %pb, align 4
  %d = load <4 x i32>, ptr %pd, align 4
  %add = add <4 x i32> %a, %b
  call void @p4(i32 1, i32 %k, i32 %m, <4 x i32> %add)
  %sub = sub <4 x i32> %a, %b
  call void @p4(i32 2, i32 %k, i32 %m, <4 x i32> %sub)
  %mul = mul <4 x i32> %a, %b
  call void @p4(i32 3, i32 %k, i32 %m, <4 x i32> %mul)
  %sdiv = sdiv <4 x i32> %a, %d
  call void @p4(i32 4, i32 %k, i32 %m, <4 x i32> %sdiv)
  %srem = srem <4 x i32> %a, %d
  call void @p4(i32 5, i32 %k, i32 %m, <4 x i32> %srem)
  %udiv = udiv <4 x i32> %a, %d
  call void @p4(i32 6, i32 %k, i32 %m, <4 x i32> %udiv)
  %urem = urem <4 x i32> %a, %d
  call void @p4(i32 7, i32 %k, i32 %m, <4 x i32> %urem)
  %n = and <4 x i32> %b, <i32 31, i32 31, i32 31, i32 31>
  %shl = shl <4 x i32> %a, %n
  call void @p4(i32 8, i32 %k, i32 %m, <4 x i32> %shl)
  %lshr = lshr <4 x i32> %a, %n
  call void @p4(i32 9, i32 %k, i32 %m, <4 x i32> %lshr)
  %ashr = ashr <4 x i32> %a, %n
  call void @p4(i32 10, i32 %k, i32 %m, <4 x i32> %ashr)
  %bits = xor <4 x i32> %a, %b
  %bo = or <4 x i32> %bits, %d
  %ba = and <4 x i32> %bo, %a
  call void @p4(i32 11, i32 %k, i32 %m, <4 x i32> %ba)
  %c0 = icmp eq <4 x i32> %a, %b
  %s0 = sext <4 x i1> %c0 to <4 x i32>
  %w0 = call i32 @comb(i32 0, <4 x i32> %s0, i32 0)
  %c1 = icmp ne <4 x i32> %a, %b
  %s1 = sext <4 x i1> %c1 to <4 x i32>
  %w1 = call i32 @comb(i32 %w0, <4 x i32> %s1, i32 4)
  %c2 = icmp slt <4 x i32> %a, %b
  %s2 = sext <4 x i1> %c2 to <4 x i32>
  %w2 = call i32 @comb(i32 %w1, <4 x i32> %s2, i32 8)
  %c3 = icmp sle <4 x i32> %a, %b
  %s3 = sext <4 x i1> %c3 to <4 x i32>
  %w3 = call i32 @comb(i32 %w2, <4 x i32> %s3, i32 12)
  %c4 = icmp sgt <4 x i32> %a, %b
  %s4 = sext <4 x i1> %c4 to <4 x i32>
  %w4 = call i32 @comb(i32 %w3, <4 x i32> %s4, i32 16)
  %c5 = icmp ult <4 x i32> %a, %b
  %s5 = sext <4 x i1> %c5 to <4 x i32>
  %w5 = call i32 @comb(i32 %w4, <4 x i32> %s5, i32 20)
  %c6 = icmp uge <4 x i32> %a, %b
  %s6 = sext <4 x i1> %c6 to <4 x i32>
  %w6 = call i32 @comb(i32 %w5, <4 x i32> %s6, i32 24)
  %c7 = icmp ugt <4 x i32> %a, %b
  %s7 = sext <4 x i1> %c7 to <4 x i32>
  %w7 = call i32 @comb(i32 %w6, <4 x i32> %s7, i32 28)
  %min = select <4 x i1> %c2, <4 x i32> %a, <4 x i32> %b
  %umax = select <4 x i1> %c7, <4 x i32> %a, <4 x i32> %b
  %e = extractelement <4 x i32> %umax, i32 %m
  %v0 = insertelement <4 x i32> %min, i32 %w7, i32 0
  %v1 = insertelement <4 x i32> %v0, i32 %e, i32 %k
  call void @p4(i32 12, i32 %k, i32 %m, <4 x i32> %v1)
  ret void
}

define internal void @reduce(i32 %k, i32 %m) #0 {
  %pa = getelementptr i32, ptr @gi, i32 %k
  %a = load <4 x i32>, ptr %pa, align 4
  %r0 = call i32 @llvm.vector.reduce.add.v4i32(<4 x i32> %a)
  %r1 = call i32 @llvm.vector.reduce.mul.v4i32(<4 x i32> %a)
  %r2 = call i32 @llvm.vector.reduce.and.v4i32(<4 x i32> %a)
  %r3 = call i32 @llvm.vector.reduce.xor.v4i32(<4 x i32> %a)
  %v0 = insertelement <4 x i32> undef, i32 %r0, i32 0
  %v1 = insertelement <4 x i32> %v0, i32 %r1, i32 1
  %v2 = insertelement <4 x i32> %v1, i32 %r2, i32 2
  %v3 = insertelement <4 x i32> %v2, i32 %r3, i32 3
  call void @p4(i32 20, i32 %k, i32 %m, <4 x i32> %v3)
  %r4 = call i32 @llvm.vector.reduce.smax.v4i32(<4 x i32> %a)
  %r5 = call i32 @llvm.vector.reduce.smin.v4i32(<4 x i32> %a)
  %r6 = call i32 @llvm.vector.reduce.umax.v4i32(<4 x i32> %a)
  %r7 = call i32 @llvm.vector.reduce.umin.v4i32(<4 x i32> %a)
  %u0 = insertelement <4 x i32> undef, i32 %r4, i32 0
  %u1 = insertelement <4 x i32> %u0, i32 %r5, i32 1
  %u2 = insertelement <4 x i32> %u1, i32 %r6, i32 2
  %u3 = insertelement <4 x i32> %u2, i32 %r7, i32 3
  call void @p4(i32 21, i32 %k, i32 %m, <4 x i32> %u3)
  %ph = getelementptr i16, ptr @gh, i32 %k
  %h = load <8 x i16>, ptr %ph, align 2
  %hm = call i16 @llvm.vector.reduce.smax.v8i16(<8 x i16> %h)
  %pb = getelementptr i8, ptr @gb, i32 %k
  %b = load <16 x i8>, ptr %pb, align 1
  %bm = call i8 @llvm.vector.reduce.umin.v16i8(<16 x i8> %b)
  %hx = sext i16 %hm to i32
  %bx = zext i8 %bm to i32
  %t0 = insertelement <4 x i32> zeroinitializer, i32 %hx, i32 0
  %t1 = insertelement <4 x i32> %t0, i32 %bx, i32 1
  call void @p4(i32 22, i32 %k, i32 %m, <4 x i32> %t1)
  ret void
}

define internal void @i16ops(i32 %k, i32 %m) #0 {
  %pa = getelementptr i16, ptr @gh, i32 %k
  %pb = getelementptr i16, ptr @gh, i32 %m
  %pd = getelementptr i16, ptr @gdh, i32 %m
  %a = load <8 x i16>, ptr %pa, align 2
  %b = load <8 x i16>, ptr %pb, align 2
  %d = load <8 x i16>, ptr %pd, align 2
  %add = add <8 x i16> %a, %b
  %x0 = bitcast <8 x i16> %add to <4 x i32>
  call void @p4(i32 30, i32 %k, i32 %m, <4 x i32> %x0)
  %mul = mul <8 x i16> %a, %b
  %x1 = bitcast <8 x i16> %mul to <4 x i32>
  call void @p4(i32 31, i32 %k, i32 %m, <4 x i32> %x1)
  %sdiv = sdiv <8 x i16> %a, %d
  %x2 = bitcast <8 x i16> %sdiv to <4 x i32>
  call void @p4(i32 32, i32 %k, i32 %m, <4 x i32> %x2)
  %urem = urem <8 x i16> %a, %d
  %x3 = bitcast <8 x i16> %urem to <4 x i32>
  call void @p4(i32 33, i32 %k, i32 %m, <4 x i32> %x3)
  %n = and <8 x i16> %b, <i16 15, i16 15, i16 15, i16 15, i16 15, i16 15, i16 15, i16 15>
  %ashr = ashr <8 x i16> %a, %n
  %x4 = bitcast <8 x i16> %ashr to <4 x i32>
  call void @p4(i32 34, i32 %k, i32 %m, <4 x i32> %x4)
  %shl = shl <8 x i16> %a, %n
  %x5 = bitcast <8 x i16> %shl to <4 x i32>
  call void @p4(i32 35, i32 %k, i32 %m, <4 x i32> %x5)
  %c0 = icmp slt <8 x i16> %a, %b
  %s0 = sext <8 x i1> %c0 to <8 x i16>
  %x6 = bitcast <8 x i16> %s0 to <4 x i32>
  call void @p4(i32 36, i32 %k, i32 %m, <4 x i32> %x6)
  %c1 = icmp ule <8 x i16> %a, %b
  %sel = select <8 x i1> %c1, <8 x i16> %b, <8 x i16> %a
  %x7 = bitcast <8 x i16> %sel to <4 x i32>
  call void @p4(i32 37, i32 %k, i32 %m, <4 x i32> %x7)
  ret void
}

define internal void @i8ops(i32 %k, i32 %m) #0 {
  %pa = getelementptr i8, ptr @gb, i32 %k
  %pb = getelementptr i8, ptr @gb, i32 %m
  %pd = getelementptr i8, ptr @gdb, i32 %m
  %a = load <16 x i8>, ptr %pa, align 1
  %b = load <16 x i8>, ptr %pb, align 1
  %d = load <16 x i8>, ptr %pd, align 1
  %sub = sub <16 x i8> %a, %b
  %x0 = bitcast <16 x i8> %sub to <4 x i32>
  call void @p4(i32 40, i32 %k, i32 %m, <4 x i32> %x0)
  %mul = mul <16 x i8> %a, %b
  %x1 = bitcast <16 x i8> %mul to <4 x i32>
  call void @p4(i32 41, i32 %k, i32 %m, <4 x i32> %x1)
  %srem = srem <16 x i8> %a, %d
  %x2 = bitcast <16 x i8> %srem to <4 x i32>
  call void @p4(i32 42, i32 %k, i32 %m, <4 x i32> %x2)
  %udiv = udiv <16 x i8> %a, %d
  %x3 = bitcast <16 x i8> %udiv to <4 x i32>
  call void @p4(i32 43, i32 %k, i32 %m, <4 x i32> %x3)
  %n = and <16 x i8> %b, <i8 7, i8 7, i8 7, i8 7, i8 7, i8 7, i8 7, i8 7, i8 7, i8 7, i8 7, i8 7, i8 7, i8 7, i8 7, i8 7>
  %lshr = lshr <16 x i8> %a, %n
  %x4 = bitcast <16 x i8> %lshr to <4 x i32>
  call void @p4(i32 44, i32 %k, i32 %m, <4 x i32> %x4)
  %c0 = icmp sgt <16 x i8> %a, %b
  %s0 = sext <16 x i1> %c0 to <16 x i8>
  %x5 = bitcast <16 x i8> %s0 to <4 x i32>
  call void @p4(i32 45, i32 %k, i32 %m, <4 x i32> %x5)
  ret void
}

define internal void @i64ops(i32 %k, i32 %m) #0 {
  %pa = getelementptr i64, ptr @gq, i32 %k
  %pb = getelementptr i64, ptr @gq, i32 %m
  %a = load <2 x i64>, ptr %pa, align 8
  %b = load <2 x i64>, ptr %pb, align 8
  %add = add <2 x i64> %a, %b
  %x0 = bitcast <2 x i64> %add to <4 x i32>
  call void @p4(i32 50, i32 %k, i32 %m, <4 x i32> %x0)
  %sub = sub <2 x i64> %a, %b
  %x1 = bitcast <2 x i64> %sub to <4 x i32>
  call void @p4(i32 51, i32 %k, i32 %m, <4 x i32> %x1)
  %xor = xor <2 x i64> %a, %b
  %or = or <2 x i64> %xor, <i64 4294967296, i64 1>
  %and = and <2 x i64> %or, %a
  %x2 = bitcast <2 x i64> %and to <4 x i32>
  call void @p4(i32 52, i32 %k, i32 %m, <4 x i32> %x2)
  %c0 = icmp slt <2 x i64> %a, %b
  %s0 = sext <2 x i1> %c0 to <2 x i64>
  %c1 = icmp ult <2 x i64> %a, %b
  %s1 = sext <2 x i1> %c1 to <2 x i64>
  %c2 = icmp eq <2 x i64> %a, %b
  %c3 = icmp sge <2 x i64> %a, %b
  %e = select <2 x i1> %c2, <2 x i64> %s0, <2 x i64> %s1
  %s3 = sext <2 x i1> %c3 to <2 x i64>
  %y = shufflevector <2 x i64> %e, <2 x i64> %s3, <2 x i32> <i32 0, i32 3>
  %x3 = bitcast <2 x i64> %y to <4 x i32>
  call void @p4(i32 53, i32 %k, i32 %m, <4 x i32> %x3)
  %x4 = bitcast <2 x i64> %s1 to <4 x i32>
  call void @p4(i32 54, i32 %k, i32 %m, <4 x i32> %x4)
  ret void
}

define internal void @f32ops(i32 %k, i32 %m) #0 {
  %pa = getelementptr i32, ptr @gf, i32 %k
  %pb = getelementptr i32, ptr @gf, i32 %m
  %a = load <4 x float>, ptr %pa, align 4
  %b = load <4 x float>, ptr %pb, align 4
  %add = fadd <4 x float> %a, %b
  %x0 = bitcast <4 x float> %add to <4 x i32>
  call void @p4(i32 60, i32 %k, i32 %m, <4 x i32> %x0)
  %sub = fsub <4 x float> %a, %b
  %x1 = bitcast <4 x float> %sub to <4 x i32>
  call void @p4(i32 61, i32 %k, i32 %m, <4 x i32> %x1)
  %mul = fmul <4 x float> %a, %b
  %x2 = bitcast <4 x float> %mul to <4 x i32>
  call void @p4(i32 62, i32 %k, i32 %m, <4 x i32> %x2)
  %div = fdiv <4 x float> %a, %b
  %x3 = bitcast <4 x float> %div to <4 x i32>
  call void @p4(i32 63, i32 %k, i32 %m, <4 x i32> %x3)
  %neg = fneg <4 x float> %b
  %x4 = bitcast <4 x float> %neg to <4 x i32>
  call void @p4(i32 64, i32 %k, i32 %m, <4 x i32> %x4)
  %c0 = fcmp oeq <4 x float> %a, %b
  %s0 = sext <4 x i1> %c0 to <4 x i32>
  %w0 = call i32 @comb(i32 0, <4 x i32> %s0, i32 0)
  %c1 = fcmp ogt <4 x float> %a, %b
  %s1 = sext <4 x i1> %c1 to <4 x i32>
  %w1 = call i32 @comb(i32 %w0, <4 x i32> %s1, i32 4)
  %c2 = fcmp oge <4 x float> %a, %b
  %s2 = sext <4 x i1> %c2 to <4 x i32>
  %w2 = call i32 @comb(i32 %w1, <4 x i32> %s2, i32 8)
  %c3 = fcmp olt <4 x float> %a, %b
  %s3 = sext <4 x i1> %c3 to <4 x i32>
  %w3 = call i32 @comb(i32 %w2, <4 x i32> %s3, i32 12)
  %c4 = fcmp ole <4 x float> %a, %b
  %s4 = sext <4 x i1> %c4 to <4 x i32>
  %w4 = call i32 @comb(i32 %w3, <4 x i32> %s4, i32 16)
  %c5 = fcmp one <4 x float> %a, %b
  %s5 = sext <4 x i1> %c5 to <4 x i32>
  %w5 = call i32 @comb(i32 %w4, <4 x i32> %s5, i32 20)
  %c6 = fcmp ord <4 x float> %a, %b
  %s6 = sext <4 x i1> %c6 to <4 x i32>
  %w6 = call i32 @comb(i32 %w5, <4 x i32> %s6, i32 24)
  %c7 = fcmp uno <4 x float> %a, %b
  %s7 = sext <4 x i1> %c7 to <4 x i32>
  %w7 = call i32 @comb(i32 %w6, <4 x i32> %s7, i32 28)
  %d0 = fcmp ueq <4 x float> %a, %b
  %t0 = sext <4 x i1> %d0 to <4 x i32>
  %v0 = call i32 @comb(i32 0, <4 x i32> %t0, i32 0)
  %d1 = fcmp ugt <4 x float> %a, %b
  %t1 = sext <4 x i1> %d1 to <4 x i32>
  %v1 = call i32 @comb(i32 %v0, <4 x i32> %t1, i32 4)
  %d2 = fcmp uge <4 x float> %a, %b
  %t2 = sext <4 x i1> %d2 to <4 x i32>
  %v2 = call i32 @comb(i32 %v1, <4 x i32> %t2, i32 8)
  %d3 = fcmp ult <4 x float> %a, %b
  %t3 = sext <4 x i1> %d3 to <4 x i32>
  %v3 = call i32 @comb(i32 %v2, <4 x i32> %t3, i32 12)
  %d4 = fcmp ule <4 x float> %a, %b
  %t4 = sext <4 x i1> %d4 to <4 x i32>
  %v4 = call i32 @comb(i32 %v3, <4 x i32> %t4, i32 16)
  %d5 = fcmp une <4 x float> %a, %b
  %t5 = sext <4 x i1> %d5 to <4 x i32>
  %v5 = call i32 @comb(i32 %v4, <4 x i32> %t5, i32 20)
  %sel = select <4 x i1> %c3, <4 x float> %a, <4 x float> %b
  %sb = bitcast <4 x float> %sel to <4 x i32>
  %r0 = insertelement <4 x i32> %sb, i32 %w7, i32 0
  %r1 = insertelement <4 x i32> %r0, i32 %v5, i32 1
  call void @p4(i32 65, i32 %k, i32 %m, <4 x i32> %r1)
  ret void
}

define internal void @f64ops(i32 %k, i32 %m) #0 {
  %pa = getelementptr i64, ptr @gd, i32 %k
  %pb = getelementptr i64, ptr @gd, i32 %m
  %a = load <2 x double>, ptr %pa, align 8
  %b = load <2 x double>, ptr %pb, align 8
  %add = fadd <2 x double> %a, %b
  %x0 = bitcast <2 x double> %add to <4 x i32>
  call void @p4(i32 70, i32 %k, i32 %m, <4 x i32> %x0)
  %sub = fsub <2 x double> %a, %b
  %x1 = bitcast <2 x double> %sub to <4 x i32>
  call void @p4(i32 71, i32 %k, i32 %m, <4 x i32> %x1)
  %mul = fmul <2 x double> %a, %b
  %x2 = bitcast <2 x double> %mul to <4 x i32>
  call void @p4(i32 72, i32 %k, i32 %m, <4 x i32> %x2)
  %div = fdiv <2 x double> %a, %b
  %x3 = bitcast <2 x double> %div to <4 x i32>
  call void @p4(i32 73, i32 %k, i32 %m, <4 x i32> %x3)
  %neg = fneg <2 x double> %a
  %x4 = bitcast <2 x double> %neg to <4 x i32>
  call void @p4(i32 74, i32 %k, i32 %m, <4 x i32> %x4)
  %c0 = fcmp olt <2 x double> %a, %b
  %c1 = fcmp uge <2 x double> %a, %b
  %c2 = fcmp one <2 x double> %a, %b
  %c3 = fcmp ueq <2 x double> %a, %b
  %c4 = fcmp ord <2 x double> %a, %b
  %c5 = fcmp ule <2 x double> %a, %b
  %l = shufflevector <2 x i1> %c0, <2 x i1> %c1, <4 x i32> <i32 0, i32 1, i32 2, i32 3>
  %h = shufflevector <2 x i1> %c2, <2 x i1> %c3, <4 x i32> <i32 0, i32 1, i32 2, i32 3>
  %o = shufflevector <2 x i1> %c4, <2 x i1> %c5, <4 x i32> <i32 0, i32 1, i32 2, i32 3>
  %ls = sext <4 x i1> %l to <4 x i32>
  %w0 = call i32 @comb(i32 0, <4 x i32> %ls, i32 0)
  %hs = sext <4 x i1> %h to <4 x i32>
  %w1 = call i32 @comb(i32 %w0, <4 x i32> %hs, i32 4)
  %os = sext <4 x i1> %o to <4 x i32>
  %w2 = call i32 @comb(i32 %w1, <4 x i32> %os, i32 8)
  %sel = select <2 x i1> %c0, <2 x double> %b, <2 x double> %a
  %sb = bitcast <2 x double> %sel to <4 x i32>
  %r = insertelement <4 x i32> %sb, i32 %w2, i32 1
  call void @p4(i32 75, i32 %k, i32 %m, <4 x i32> %r)
  ret void
}

; Casts of the lanes: `sitofp` of the integer grid, `fptosi` of values in
; range, `fpext` / `fptrunc` of the float grids (NaNs, infinities,
; subnormals, overflow to infinity), `uitofp` of `i16` lanes, `trunc`,
; `zext`, `sext`.
define internal void @casts(i32 %k, i32 %m) #0 {
  %pi = getelementptr i32, ptr @gi, i32 %k
  %i = load <4 x i32>, ptr %pi, align 4
  %f = sitofp <4 x i32> %i to <4 x float>
  %x0 = bitcast <4 x float> %f to <4 x i32>
  call void @p4(i32 80, i32 %k, i32 %m, <4 x i32> %x0)
  %km = and i32 %k, 3
  %pc = getelementptr float, ptr @gc, i32 %km
  %c = load <4 x float>, ptr %pc, align 4
  %ci = fptosi <4 x float> %c to <4 x i32>
  call void @p4(i32 81, i32 %k, i32 %m, <4 x i32> %ci)
  %pf = getelementptr i32, ptr @gf, i32 %m
  %g = load <2 x float>, ptr %pf, align 4
  %e = fpext <2 x float> %g to <2 x double>
  %x2 = bitcast <2 x double> %e to <4 x i32>
  call void @p4(i32 82, i32 %k, i32 %m, <4 x i32> %x2)
  %pd = getelementptr i64, ptr @gd, i32 %m
  %d = load <2 x double>, ptr %pd, align 8
  %t = fptrunc <2 x double> %d to <2 x float>
  %tw = shufflevector <2 x float> %t, <2 x float> %g, <4 x i32> <i32 0, i32 1, i32 2, i32 3>
  %x3 = bitcast <4 x float> %tw to <4 x i32>
  call void @p4(i32 83, i32 %k, i32 %m, <4 x i32> %x3)
  %ph = getelementptr i16, ptr @gh, i32 %k
  %h = load <4 x i16>, ptr %ph, align 2
  %hu = uitofp <4 x i16> %h to <4 x float>
  %x4 = bitcast <4 x float> %hu to <4 x i32>
  call void @p4(i32 84, i32 %k, i32 %m, <4 x i32> %x4)
  %tr = trunc <4 x i32> %i to <4 x i16>
  %trw = shufflevector <4 x i16> %tr, <4 x i16> %h, <8 x i32> <i32 0, i32 1, i32 2, i32 3, i32 4, i32 5, i32 6, i32 7>
  %x5 = bitcast <8 x i16> %trw to <4 x i32>
  call void @p4(i32 85, i32 %k, i32 %m, <4 x i32> %x5)
  %pb = getelementptr i8, ptr @gb, i32 %m
  %b = load <4 x i8>, ptr %pb, align 1
  %bz = zext <4 x i8> %b to <4 x i32>
  call void @p4(i32 86, i32 %k, i32 %m, <4 x i32> %bz)
  %bs = sext <4 x i8> %b to <4 x i32>
  call void @p4(i32 87, i32 %k, i32 %m, <4 x i32> %bs)
  ret void
}

; An `x86_fp80` value (x87, at 80 bits) live across vector code that uses
; `eax`, `ecx`, `edx` and `xmm6`, and an `x86_fp80` computed from a lane.
define internal void @fp80_across(i32 %k, i32 %m) #0 {
  %kf = sitofp i32 %k to x86_fp80
  %x = fdiv x86_fp80 %kf, 0xK4000C000000000000000
  %pa = getelementptr i32, ptr @gi, i32 %k
  %pb = getelementptr i32, ptr @gdi, i32 %m
  %a = load <4 x i32>, ptr %pa, align 4
  %b = load <4 x i32>, ptr %pb, align 4
  %q = sdiv <4 x i32> %a, %b
  %r = mul <4 x i32> %q, %a
  %f = sitofp <4 x i32> %r to <4 x float>
  %g = fmul <4 x float> %f, <float 0.5, float 0.5, float 0.5, float 0.5>
  %l = extractelement <4 x float> %g, i32 2
  %le = fpext float %l to x86_fp80
  %y = fadd x86_fp80 %x, %le
  %z = fmul x86_fp80 %y, %x
  %yd = fptrunc x86_fp80 %z to double
  %yb = bitcast double %yd to <4 x i16>
  %w = shufflevector <4 x i16> %yb, <4 x i16> zeroinitializer, <8 x i32> <i32 0, i32 1, i32 2, i32 3, i32 4, i32 5, i32 6, i32 7>
  %wx = bitcast <8 x i16> %w to <4 x i32>
  %rx = bitcast <4 x float> %g to <4 x i32>
  %o = or <4 x i32> %wx, zeroinitializer
  call void @p4(i32 90, i32 %k, i32 %m, <4 x i32> %o)
  call void @p4(i32 91, i32 %k, i32 %m, <4 x i32> %rx)
  ret void
}

define internal void @grid(ptr %f, i32 %n) #0 {
entry:
  br label %outer
outer:
  %k = phi i32 [ 0, %entry ], [ %k1, %inner.end ]
  br label %inner
inner:
  %m = phi i32 [ 0, %outer ], [ %m1, %inner ]
  call void %f(i32 %k, i32 %m)
  %m1 = add i32 %m, 1
  %md = icmp eq i32 %m1, %n
  br i1 %md, label %inner.end, label %inner
inner.end:
  %k1 = add i32 %k, 1
  %kd = icmp eq i32 %k1, %n
  br i1 %kd, label %out, label %outer
out:
  ret void
}

define i32 @main() #0 {
  call void @grid(ptr @i32ops, i32 8)
  call void @grid(ptr @reduce, i32 8)
  call void @grid(ptr @i16ops, i32 8)
  call void @grid(ptr @i8ops, i32 8)
  call void @grid(ptr @i64ops, i32 8)
  call void @grid(ptr @f32ops, i32 12)
  call void @grid(ptr @f64ops, i32 12)
  call void @grid(ptr @casts, i32 8)
  call void @grid(ptr @fp80_across, i32 8)
  ret i32 0
}

attributes #0 = { "target-cpu"="pentium4" }
