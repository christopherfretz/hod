target triple = "i686-unknown-linux-gnu"
declare i32 @printf(ptr, ...)
@f4 = private constant [13 x i8] c"%d %d %d %d\0A\00"
@ff = private constant [13 x i8] c"%a %a %a %a\0A\00"

define void @p4(<4 x i32> %v) #0 {
  %a = extractelement <4 x i32> %v, i32 0
  %b = extractelement <4 x i32> %v, i32 1
  %c = extractelement <4 x i32> %v, i32 2
  %d = extractelement <4 x i32> %v, i32 3
  call i32 (ptr, ...) @printf(ptr @f4, i32 %a, i32 %b, i32 %c, i32 %d)
  ret void
}

define void @pf(<4 x float> %v) #0 {
  %a = extractelement <4 x float> %v, i32 0
  %b = extractelement <4 x float> %v, i32 1
  %c = extractelement <4 x float> %v, i32 2
  %d = extractelement <4 x float> %v, i32 3
  %a2 = fpext float %a to double
  %b2 = fpext float %b to double
  %c2 = fpext float %c to double
  %d2 = fpext float %d to double
  call i32 (ptr, ...) @printf(ptr @ff, double %a2, double %b2, double %c2, double %d2)
  ret void
}

define <4 x i32> @five(<4 x i32> %a, i32 %i, <2 x double> %b, i64 %q, <4 x float> %c, <16 x i8> %d, <8 x i16> %e) #0 {
  %x = bitcast <8 x i16> %e to <4 x i32>
  %y = add <4 x i32> %x, %a
  %z = bitcast <16 x i8> %d to <4 x i32>
  %w = xor <4 x i32> %y, %z
  %s = insertelement <4 x i32> %w, i32 %i, i32 2
  %t = trunc i64 %q to i32
  %u = insertelement <4 x i32> %s, i32 %t, i32 3
  ret <4 x i32> %u
}

define i32 @main() #0 {
  %a = add <4 x i32> <i32 1, i32 -2, i32 2147483647, i32 4>, <i32 10, i32 20, i32 1, i32 -40>
  call void @p4(<4 x i32> %a)
  %m = mul <4 x i32> %a, <i32 3, i32 -5, i32 2, i32 7>
  %sd = sdiv <4 x i32> %m, <i32 2, i32 -3, i32 5, i32 7>
  call void @p4(<4 x i32> %sd)
  %sh = ashr <4 x i32> %a, <i32 1, i32 2, i32 3, i32 31>
  call void @p4(<4 x i32> %sh)
  %c = icmp slt <4 x i32> %a, <i32 0, i32 0, i32 100, i32 0>
  %cs = sext <4 x i1> %c to <4 x i32>
  call void @p4(<4 x i32> %cs)
  %sel = select <4 x i1> %c, <4 x i32> %a, <4 x i32> %sh
  call void @p4(<4 x i32> %sel)
  %sh2 = shufflevector <4 x i32> %a, <4 x i32> %sh, <4 x i32> <i32 7, i32 0, i32 5, i32 2>
  call void @p4(<4 x i32> %sh2)
  %fv = sitofp <4 x i32> %a to <4 x float>
  %fd = fdiv <4 x float> %fv, <float 0.0, float -0.0, float 3.0, float 0x3730000000000000>
  call void @pf(<4 x float> %fd)
  %fn = fneg <4 x float> %fd
  call void @pf(<4 x float> %fn)
  %fc = fcmp ult <4 x float> %fd, <float 0.0, float 1.0, float 0x7FF8000000000000, float 2.0>
  %fz = zext <4 x i1> %fc to <4 x i32>
  call void @p4(<4 x i32> %fz)
  %r = call i32 @llvm.vector.reduce.smax.v4i32(<4 x i32> %a)
  %r2 = call i1 @llvm.vector.reduce.xor.v4i1(<4 x i1> %c)
  %r3 = zext i1 %r2 to i32
  %b8 = bitcast <4 x i32> %a to <16 x i8>
  %e8 = extractelement <16 x i8> %b8, i32 %r3
  %e32 = sext i8 %e8 to i32
  %ins = insertelement <4 x i32> %a, i32 %r, i32 %r3
  call void @p4(<4 x i32> %ins)
  %f = call <4 x i32> @five(<4 x i32> %a, i32 %e32, <2 x double> <double 1.0, double 2.0>, i64 77, <4 x float> %fd, <16 x i8> %b8, <8 x i16> <i16 1, i16 2, i16 3, i16 4, i16 5, i16 6, i16 7, i16 8>)
  call void @p4(<4 x i32> %f)
  %w = sext <4 x i32> %a to <4 x i64>
  %w2 = add <4 x i64> %w, %w
  %wc = icmp sgt <4 x i64> %w2, <i64 0, i64 -5, i64 4294967296, i64 0>
  %wz = zext <4 x i1> %wc to <4 x i32>
  call void @p4(<4 x i32> %wz)
  %wt = trunc <4 x i64> %w2 to <4 x i32>
  call void @p4(<4 x i32> %wt)
  ret i32 0
}
declare i32 @llvm.vector.reduce.smax.v4i32(<4 x i32>)
declare i1 @llvm.vector.reduce.xor.v4i1(<4 x i1>)
attributes #0 = { nounwind "target-cpu"="pentium4" }
