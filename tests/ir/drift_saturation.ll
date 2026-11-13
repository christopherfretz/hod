; Drift seed (design 0006 §8 risk 7, WP-130): saturation. The scalar
; saturating additions and subtractions at i7, i8, i16, i32, i33 and
; i64 (both directions of overflow, and none), and the saturating
; conversions fptosi.sat and fptoui.sat from double and float to i7, i8,
; i16, i32 and i64: NaN gives 0, an infinity or a value out of range
; gives the bound, a fraction is truncated toward zero, -0.0 gives 0.
; Each target has its own sequence for these (no instruction does it);
; both must print the same. Operands come from memory, so nothing is
; folded. Expected values computed by a script, not a compiler.
;@ covers: spec/ir.md §5.5 intrinsics (sadd.sat, uadd.sat, ssub.sat, usub.sat, fptosi.sat, fptoui.sat)
;@ stdout: sadd.i8 127 -128 uadd.i16 65535 ssub.i64 -9223372036854775808 usub.i8 0
;@ stdout: sadd.i7 63 ssub.i7 -64 uadd.i33 8589934591 sadd.i32 4
;@ stdout: fptosi.sat.i32 0 2147483647 -2147483648 2 -2
;@ stdout: fptoui.sat.i8 255 0 0 127
;@ stdout: fptosi.sat.i64 9223372036854775807 -9223372036854775808 fptoui.sat.i64 10000000000000000000 0
;@ stdout: fptosi.sat.i7 63 -64 0 fptosi.sat.i16.f32 32767 -32768 0

@in = global [14 x i64] [i64 100, i64 -100, i64 60000, i64 6000, i64 -9223372036854775808, i64 1, i64 5, i64 9, i64 60, i64 10, i64 -60, i64 8589934587, i64 7, i64 3]
@fd = global [10 x double] [double 0x7FF8000000000000, double 0x7FF0000000000000, double -1.000000e+300, double 2.900000e+00, double -2.900000e+00, double 1.000000e+19, double 3.000000e+02, double -1.000000e+00, double 1.275000e+02, double -0.000000e+00]
@ff = global [3 x float] [float 1.000000e+10, float 0xFFF0000000000000, float 0x7FF8000000000000]

@.f1 = private unnamed_addr constant [60 x i8] c"sadd.i8 %lld %lld uadd.i16 %lld ssub.i64 %lld usub.i8 %lld\0A\00"
@.f2 = private unnamed_addr constant [55 x i8] c"sadd.i7 %lld ssub.i7 %lld uadd.i33 %lld sadd.i32 %lld\0A\00"
@.f3 = private unnamed_addr constant [31 x i8] c"fptosi.sat.i32 %d %d %d %d %d\0A\00"
@.f4 = private unnamed_addr constant [27 x i8] c"fptoui.sat.i8 %d %d %d %d\0A\00"
@.f5 = private unnamed_addr constant [51 x i8] c"fptosi.sat.i64 %lld %lld fptoui.sat.i64 %llu %llu\0A\00"
@.f6 = private unnamed_addr constant [58 x i8] c"fptosi.sat.i7 %lld %lld %lld fptosi.sat.i16.f32 %d %d %d\0A\00"

declare i32 @printf(ptr, ...)
declare i8 @llvm.sadd.sat.i8(i8, i8)
declare i16 @llvm.uadd.sat.i16(i16, i16)
declare i64 @llvm.ssub.sat.i64(i64, i64)
declare i8 @llvm.usub.sat.i8(i8, i8)
declare i7 @llvm.sadd.sat.i7(i7, i7)
declare i7 @llvm.ssub.sat.i7(i7, i7)
declare i33 @llvm.uadd.sat.i33(i33, i33)
declare i32 @llvm.sadd.sat.i32(i32, i32)
declare i32 @llvm.fptosi.sat.i32.f64(double)
declare i8 @llvm.fptoui.sat.i8.f64(double)
declare i64 @llvm.fptosi.sat.i64.f64(double)
declare i64 @llvm.fptoui.sat.i64.f64(double)
declare i7 @llvm.fptosi.sat.i7.f64(double)
declare i16 @llvm.fptosi.sat.i16.f32(float)

define internal i64 @in_at(i32 %i) {
  %p = getelementptr [14 x i64], ptr @in, i32 0, i32 %i
  %v = load i64, ptr %p
  ret i64 %v
}

define internal double @fd_at(i32 %i) {
  %p = getelementptr [10 x double], ptr @fd, i32 0, i32 %i
  %v = load double, ptr %p
  ret double %v
}

define internal float @ff_at(i32 %i) {
  %p = getelementptr [3 x float], ptr @ff, i32 0, i32 %i
  %v = load float, ptr %p
  ret float %v
}

define internal void @ints() {
  %v0 = call i64 @in_at(i32 0)
  %v1 = call i64 @in_at(i32 1)
  %v2 = call i64 @in_at(i32 2)
  %v3 = call i64 @in_at(i32 3)
  %v4 = call i64 @in_at(i32 4)
  %v5 = call i64 @in_at(i32 5)
  %v6 = call i64 @in_at(i32 6)
  %v7 = call i64 @in_at(i32 7)
  %v8 = call i64 @in_at(i32 8)
  %v9 = call i64 @in_at(i32 9)
  %v10 = call i64 @in_at(i32 10)
  %v11 = call i64 @in_at(i32 11)
  %v12 = call i64 @in_at(i32 12)
  %v13 = call i64 @in_at(i32 13)
  %a8 = trunc i64 %v0 to i8
  %b8 = trunc i64 %v1 to i8
  %s1 = call i8 @llvm.sadd.sat.i8(i8 %a8, i8 %a8)
  %s2 = call i8 @llvm.sadd.sat.i8(i8 %b8, i8 %b8)
  %a16 = trunc i64 %v2 to i16
  %b16 = trunc i64 %v3 to i16
  %s3 = call i16 @llvm.uadd.sat.i16(i16 %a16, i16 %b16)
  %s4 = call i64 @llvm.ssub.sat.i64(i64 %v4, i64 %v5)
  %c8 = trunc i64 %v6 to i8
  %d8 = trunc i64 %v7 to i8
  %s5 = call i8 @llvm.usub.sat.i8(i8 %c8, i8 %d8)
  %w1 = sext i8 %s1 to i64
  %w2 = sext i8 %s2 to i64
  %w3 = zext i16 %s3 to i64
  %w5 = zext i8 %s5 to i64
  call i32 (ptr, ...) @printf(ptr @.f1, i64 %w1, i64 %w2, i64 %w3, i64 %s4, i64 %w5)
  %a7 = trunc i64 %v8 to i7
  %b7 = trunc i64 %v9 to i7
  %c7 = trunc i64 %v10 to i7
  %s6 = call i7 @llvm.sadd.sat.i7(i7 %a7, i7 %b7)
  %s7 = call i7 @llvm.ssub.sat.i7(i7 %c7, i7 %b7)
  %a33 = trunc i64 %v11 to i33
  %b33 = trunc i64 %v9 to i33
  %s8 = call i33 @llvm.uadd.sat.i33(i33 %a33, i33 %b33)
  %a32 = trunc i64 %v12 to i32
  %b32 = trunc i64 %v13 to i32
  %n32 = sub i32 0, %b32
  %s9 = call i32 @llvm.sadd.sat.i32(i32 %a32, i32 %n32)
  %w6 = sext i7 %s6 to i64
  %w7 = sext i7 %s7 to i64
  %w8 = zext i33 %s8 to i64
  %w9 = sext i32 %s9 to i64
  call i32 (ptr, ...) @printf(ptr @.f2, i64 %w6, i64 %w7, i64 %w8, i64 %w9)
  ret void
}

define internal void @convs() {
  %nan = call double @fd_at(i32 0)
  %inf = call double @fd_at(i32 1)
  %mbig = call double @fd_at(i32 2)
  %p29 = call double @fd_at(i32 3)
  %m29 = call double @fd_at(i32 4)
  %e19 = call double @fd_at(i32 5)
  %d300 = call double @fd_at(i32 6)
  %m1 = call double @fd_at(i32 7)
  %d127 = call double @fd_at(i32 8)
  %mz = call double @fd_at(i32 9)
  %i1 = call i32 @llvm.fptosi.sat.i32.f64(double %nan)
  %i2 = call i32 @llvm.fptosi.sat.i32.f64(double %inf)
  %i3 = call i32 @llvm.fptosi.sat.i32.f64(double %mbig)
  %i4 = call i32 @llvm.fptosi.sat.i32.f64(double %p29)
  %i5 = call i32 @llvm.fptosi.sat.i32.f64(double %m29)
  call i32 (ptr, ...) @printf(ptr @.f3, i32 %i1, i32 %i2, i32 %i3, i32 %i4, i32 %i5)
  %u1 = call i8 @llvm.fptoui.sat.i8.f64(double %d300)
  %u2 = call i8 @llvm.fptoui.sat.i8.f64(double %m1)
  %u3 = call i8 @llvm.fptoui.sat.i8.f64(double %nan)
  %u4 = call i8 @llvm.fptoui.sat.i8.f64(double %d127)
  %x1 = zext i8 %u1 to i32
  %x2 = zext i8 %u2 to i32
  %x3 = zext i8 %u3 to i32
  %x4 = zext i8 %u4 to i32
  call i32 (ptr, ...) @printf(ptr @.f4, i32 %x1, i32 %x2, i32 %x3, i32 %x4)
  %l1 = call i64 @llvm.fptosi.sat.i64.f64(double %e19)
  %l2 = call i64 @llvm.fptosi.sat.i64.f64(double %mbig)
  %l3 = call i64 @llvm.fptoui.sat.i64.f64(double %e19)
  %l4 = call i64 @llvm.fptoui.sat.i64.f64(double %m1)
  call i32 (ptr, ...) @printf(ptr @.f5, i64 %l1, i64 %l2, i64 %l3, i64 %l4)
  %o1 = call i7 @llvm.fptosi.sat.i7.f64(double %d127)
  %o2 = call i7 @llvm.fptosi.sat.i7.f64(double %mbig)
  %o3 = call i7 @llvm.fptosi.sat.i7.f64(double %mz)
  %y1 = sext i7 %o1 to i64
  %y2 = sext i7 %o2 to i64
  %y3 = sext i7 %o3 to i64
  %f0 = call float @ff_at(i32 0)
  %f1 = call float @ff_at(i32 1)
  %f2 = call float @ff_at(i32 2)
  %h0 = call i16 @llvm.fptosi.sat.i16.f32(float %f0)
  %h1 = call i16 @llvm.fptosi.sat.i16.f32(float %f1)
  %h2 = call i16 @llvm.fptosi.sat.i16.f32(float %f2)
  %z0 = sext i16 %h0 to i32
  %z1 = sext i16 %h1 to i32
  %z2 = sext i16 %h2 to i32
  call i32 (ptr, ...) @printf(ptr @.f6, i64 %y1, i64 %y2, i64 %y3, i32 %z0, i32 %z1, i32 %z2)
  ret void
}

define i32 @main() {
  call void @ints()
  call void @convs()
  ret i32 0
}
