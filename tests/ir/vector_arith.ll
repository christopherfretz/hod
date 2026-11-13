; Whole-vector arithmetic at register size (128 bits): add, sub, mul,
; and, or, xor, shl, lshr, ashr (per-lane amounts), the umin, smax,
; uadd.sat and ssub.sat intrinsics and icmp sign-extended to lanes, at
; <16 x i8>, <8 x i16>, <4 x i32> and <2 x i64>; fadd, fsub, fmul, fdiv
; at <4 x float> and <2 x double> (an overflow, a division by a signed
; zero). Operands come from memory. Lanes print in hex, or with %.17g.
; Expected values computed by a script, not a compiler.
;@ covers: spec/ir.md §5.4 integer, floating point (vector operands)
;@ stdout: i8 add: ff 81 2 b6 6d 3f 12 8f 79 b da 73 40 a1 cc 3b
;@ stdout: i8 sub: 1 81 fc 4a 91 81 76 4b 87 f7 24 8d be 1f bc 9f
;@ stdout: i8 mul: 0 80 fd 0 12 a0 b8 fa 0 a 25 80 bf 60 20 36
;@ stdout: i8 and: 0 0 3 0 6e 40 44 a0 0 0 db 80 41 40 0 4c
;@ stdout: i8 or: ff 81 ff b6 ff ff ce ef 79 b ff f3 ff 61 cc ef
;@ stdout: i8 xor: ff 81 fc b6 91 bf 8a 4f 79 b 24 73 be 21 cc a3
;@ stdout: i8 shl: 0 1 f8 0 c0 0 0 b4 0 4 f8 0 fe c0 c4 40
;@ stdout: i8 lshr: 0 1 1f 2 1 0 3 3b 0 0 1f 10 3f 30 c4 3
;@ stdout: i8 ashr: 0 1 ff fe 1 0 ff fb 0 0 ff f0 3f 30 c4 ff
;@ stdout: i8 umin: 0 1 3 36 7f 60 4e a2 0 1 db 80 7f 41 8 4e
;@ stdout: i8 smax: 0 1 3 36 7f 60 4e ed 79 a ff f3 7f 60 8 4e
;@ stdout: i8 uadd.sat: ff 81 ff b6 ff ff ff ff 79 b ff ff ff a1 cc ff
;@ stdout: i8 ssub.sat: 1 7f fc 80 7f 7f 80 4b 87 f7 24 8d 7f 1f bc 9f
;@ stdout: i8 icmp ugt: 0 0 ff ff 0 0 ff ff 0 0 ff 0 0 ff ff ff
;@ stdout: i8 icmp sle: 0 0 ff ff 0 0 ff 0 ff ff 0 ff 0 0 ff ff
;@ stdout: i16 add: ffff 8001 1fd7 136e 2fc9 4b30 419d 4b14
;@ stdout: i16 sub: 1 8001 e027 ec92 d035 d5f4 9783 e626
;@ stdout: i16 mul: 0 8000 e028 0 5036 4e1c 5350 1afb
;@ stdout: i16 and: 0 0 1fd8 8000 2fca 9092 4400 1015
;@ stdout: i16 or: ffff 8001 ffff 936e ffff ba9e fd9d 3aff
;@ stdout: i16 xor: ffff 8001 e027 136e d035 2a0c b99d 2aea
;@ stdout: i16 shl: 0 1 ff00 0 fc00 8000 0 4e80
;@ stdout: i16 lshr: 0 1 ff 2 1f 2 3 31
;@ stdout: i16 ashr: 0 1 ffff fffe 1f fffe 3 31
;@ stdout: i16 umin: 0 1 1fd8 8000 7fff 9092 6c90 189d
;@ stdout: i16 smax: 0 1 1fd8 936e 7fff ba9e 6c90 3277
;@ stdout: i16 uadd.sat: ffff 8001 ffff ffff ffff ffff ffff 4b14
;@ stdout: i16 ssub.sat: 1 7fff e027 ec92 7fff d5f4 7fff e626
;@ stdout: i16 icmp ugt: 0 0 ffff 0 0 0 0 0
;@ stdout: i16 icmp sle: 0 0 ffff ffff 0 ffff 0 ffff
;@ stdout: i32 add: ffffffff 80000001 50c4d7da 5326e9c2
;@ stdout: i32 sub: 1 80000001 af3b2824 acd9163e
;@ stdout: i32 mul: 0 80000000 af3b2825 0
;@ stdout: i32 and: 0 0 50c4d7db 80000000
;@ stdout: i32 or: ffffffff 80000001 ffffffff d326e9c2
;@ stdout: i32 xor: ffffffff 80000001 af3b2824 5326e9c2
;@ stdout: i32 shl: 0 1 f8000000 0
;@ stdout: i32 lshr: 0 1 1f 20000000
;@ stdout: i32 ashr: 0 1 ffffffff e0000000
;@ stdout: i32 umin: 0 1 50c4d7db 80000000
;@ stdout: i32 smax: 0 1 50c4d7db d326e9c2
;@ stdout: i32 uadd.sat: ffffffff 80000001 ffffffff ffffffff
;@ stdout: i32 ssub.sat: 1 7fffffff af3b2824 acd9163e
;@ stdout: i32 icmp ugt: 0 0 ffffffff 0
;@ stdout: i32 icmp sle: 0 0 ffffffff ffffffff
;@ stdout: i64 add: ffffffffffffffff 8000000000000001
;@ stdout: i64 sub: 1 8000000000000001
;@ stdout: i64 mul: 0 8000000000000000
;@ stdout: i64 and: 0 0
;@ stdout: i64 or: ffffffffffffffff 8000000000000001
;@ stdout: i64 xor: ffffffffffffffff 8000000000000001
;@ stdout: i64 shl: 0 1
;@ stdout: i64 lshr: 0 1
;@ stdout: i64 ashr: 0 1
;@ stdout: i64 umin: 0 1
;@ stdout: i64 smax: 0 1
;@ stdout: i64 uadd.sat: ffffffffffffffff 8000000000000001
;@ stdout: i64 ssub.sat: 1 7fffffffffffffff
;@ stdout: i64 icmp ugt: 0 0
;@ stdout: i64 icmp sle: 0 0
;@ stdout: float fadd: 1.75 2.9000000953674316 1.0000000150474662e+30 -1
;@ stdout: float fsub: 1.25 -3.0999999046325684 1.0000000150474662e+30 -1
;@ stdout: float fmul: 0.375 -0.30000001192092896 inf 0
;@ stdout: float fdiv: 6 -0.033333335071802139 1.0000000200408773e+20 inf
;@ stdout: double fadd: 3.3333333333333335 1.0000000000000001e+300
;@ stdout: double fsub: -2.6666666666666665 1.0000000000000001e+300
;@ stdout: double fmul: 1 inf
;@ stdout: double fdiv: 0.1111111111111111 1.0000000000000001e+290

@a8 = global <16 x i8> <i8 0, i8 1, i8 -1, i8 -128, i8 127, i8 96, i8 -60, i8 -19, i8 0, i8 1, i8 -1, i8 -128, i8 127, i8 96, i8 -60, i8 -19>
@b8 = global <16 x i8> <i8 -1, i8 -128, i8 3, i8 54, i8 -18, i8 -33, i8 78, i8 -94, i8 121, i8 10, i8 -37, i8 -13, i8 -63, i8 65, i8 8, i8 78>
@s8 = global <16 x i8> <i8 7, i8 0, i8 3, i8 6, i8 6, i8 7, i8 6, i8 2, i8 1, i8 2, i8 3, i8 3, i8 1, i8 1, i8 0, i8 6>
@a16 = global <8 x i16> <i16 0, i16 1, i16 -1, i16 -32768, i16 32767, i16 -28526, i16 27792, i16 6301>
@b16 = global <8 x i16> <i16 -1, i16 -32768, i16 8152, i16 -27794, i16 -20534, i16 -17762, i16 -10995, i16 12919>
@s16 = global <8 x i16> <i16 15, i16 0, i16 8, i16 14, i16 10, i16 14, i16 13, i16 7>
@a32 = global <4 x i32> <i32 0, i32 1, i32 -1, i32 -2147483648>
@b32 = global <4 x i32> <i32 -1, i32 -2147483648, i32 1355077595, i32 -752424510>
@s32 = global <4 x i32> <i32 31, i32 0, i32 27, i32 2>
@a64 = global <2 x i64> <i64 0, i64 1>
@b64 = global <2 x i64> <i64 -1, i64 -9223372036854775808>
@s64 = global <2 x i64> <i64 63, i64 0>
@faf32 = global <4 x float> <float 0x3FF8000000000000, float 0xBFB99999A0000000, float 0x46293E5940000000, float 0xBFF0000000000000>
@fbf32 = global <4 x float> <float 0x3FD0000000000000, float 0x4008000000000000, float 0x4202A05F20000000, float 0x8000000000000000>
@faf64 = global <2 x double> <double 0x3FD5555555555555, double 0x7E37E43C8800759C>
@fbf64 = global <2 x double> <double 0x4008000000000000, double 0x4202A05F20000000>
@.i8_add = private unnamed_addr constant [8 x i8] c"i8 add:\00"
@.i8_sub = private unnamed_addr constant [8 x i8] c"i8 sub:\00"
@.i8_mul = private unnamed_addr constant [8 x i8] c"i8 mul:\00"
@.i8_and = private unnamed_addr constant [8 x i8] c"i8 and:\00"
@.i8_or = private unnamed_addr constant [7 x i8] c"i8 or:\00"
@.i8_xor = private unnamed_addr constant [8 x i8] c"i8 xor:\00"
@.i8_shl = private unnamed_addr constant [8 x i8] c"i8 shl:\00"
@.i8_lshr = private unnamed_addr constant [9 x i8] c"i8 lshr:\00"
@.i8_ashr = private unnamed_addr constant [9 x i8] c"i8 ashr:\00"
@.i8_umin = private unnamed_addr constant [9 x i8] c"i8 umin:\00"
@.i8_smax = private unnamed_addr constant [9 x i8] c"i8 smax:\00"
@.i8_uadd_sat = private unnamed_addr constant [13 x i8] c"i8 uadd.sat:\00"
@.i8_ssub_sat = private unnamed_addr constant [13 x i8] c"i8 ssub.sat:\00"
@.i8_icmp_ugt = private unnamed_addr constant [13 x i8] c"i8 icmp ugt:\00"
@.i8_icmp_sle = private unnamed_addr constant [13 x i8] c"i8 icmp sle:\00"
@.i16_add = private unnamed_addr constant [9 x i8] c"i16 add:\00"
@.i16_sub = private unnamed_addr constant [9 x i8] c"i16 sub:\00"
@.i16_mul = private unnamed_addr constant [9 x i8] c"i16 mul:\00"
@.i16_and = private unnamed_addr constant [9 x i8] c"i16 and:\00"
@.i16_or = private unnamed_addr constant [8 x i8] c"i16 or:\00"
@.i16_xor = private unnamed_addr constant [9 x i8] c"i16 xor:\00"
@.i16_shl = private unnamed_addr constant [9 x i8] c"i16 shl:\00"
@.i16_lshr = private unnamed_addr constant [10 x i8] c"i16 lshr:\00"
@.i16_ashr = private unnamed_addr constant [10 x i8] c"i16 ashr:\00"
@.i16_umin = private unnamed_addr constant [10 x i8] c"i16 umin:\00"
@.i16_smax = private unnamed_addr constant [10 x i8] c"i16 smax:\00"
@.i16_uadd_sat = private unnamed_addr constant [14 x i8] c"i16 uadd.sat:\00"
@.i16_ssub_sat = private unnamed_addr constant [14 x i8] c"i16 ssub.sat:\00"
@.i16_icmp_ugt = private unnamed_addr constant [14 x i8] c"i16 icmp ugt:\00"
@.i16_icmp_sle = private unnamed_addr constant [14 x i8] c"i16 icmp sle:\00"
@.i32_add = private unnamed_addr constant [9 x i8] c"i32 add:\00"
@.i32_sub = private unnamed_addr constant [9 x i8] c"i32 sub:\00"
@.i32_mul = private unnamed_addr constant [9 x i8] c"i32 mul:\00"
@.i32_and = private unnamed_addr constant [9 x i8] c"i32 and:\00"
@.i32_or = private unnamed_addr constant [8 x i8] c"i32 or:\00"
@.i32_xor = private unnamed_addr constant [9 x i8] c"i32 xor:\00"
@.i32_shl = private unnamed_addr constant [9 x i8] c"i32 shl:\00"
@.i32_lshr = private unnamed_addr constant [10 x i8] c"i32 lshr:\00"
@.i32_ashr = private unnamed_addr constant [10 x i8] c"i32 ashr:\00"
@.i32_umin = private unnamed_addr constant [10 x i8] c"i32 umin:\00"
@.i32_smax = private unnamed_addr constant [10 x i8] c"i32 smax:\00"
@.i32_uadd_sat = private unnamed_addr constant [14 x i8] c"i32 uadd.sat:\00"
@.i32_ssub_sat = private unnamed_addr constant [14 x i8] c"i32 ssub.sat:\00"
@.i32_icmp_ugt = private unnamed_addr constant [14 x i8] c"i32 icmp ugt:\00"
@.i32_icmp_sle = private unnamed_addr constant [14 x i8] c"i32 icmp sle:\00"
@.i64_add = private unnamed_addr constant [9 x i8] c"i64 add:\00"
@.i64_sub = private unnamed_addr constant [9 x i8] c"i64 sub:\00"
@.i64_mul = private unnamed_addr constant [9 x i8] c"i64 mul:\00"
@.i64_and = private unnamed_addr constant [9 x i8] c"i64 and:\00"
@.i64_or = private unnamed_addr constant [8 x i8] c"i64 or:\00"
@.i64_xor = private unnamed_addr constant [9 x i8] c"i64 xor:\00"
@.i64_shl = private unnamed_addr constant [9 x i8] c"i64 shl:\00"
@.i64_lshr = private unnamed_addr constant [10 x i8] c"i64 lshr:\00"
@.i64_ashr = private unnamed_addr constant [10 x i8] c"i64 ashr:\00"
@.i64_umin = private unnamed_addr constant [10 x i8] c"i64 umin:\00"
@.i64_smax = private unnamed_addr constant [10 x i8] c"i64 smax:\00"
@.i64_uadd_sat = private unnamed_addr constant [14 x i8] c"i64 uadd.sat:\00"
@.i64_ssub_sat = private unnamed_addr constant [14 x i8] c"i64 ssub.sat:\00"
@.i64_icmp_ugt = private unnamed_addr constant [14 x i8] c"i64 icmp ugt:\00"
@.i64_icmp_sle = private unnamed_addr constant [14 x i8] c"i64 icmp sle:\00"
@.f32_fadd = private unnamed_addr constant [12 x i8] c"float fadd:\00"
@.f32_fsub = private unnamed_addr constant [12 x i8] c"float fsub:\00"
@.f32_fmul = private unnamed_addr constant [12 x i8] c"float fmul:\00"
@.f32_fdiv = private unnamed_addr constant [12 x i8] c"float fdiv:\00"
@.f64_fadd = private unnamed_addr constant [13 x i8] c"double fadd:\00"
@.f64_fsub = private unnamed_addr constant [13 x i8] c"double fsub:\00"
@.f64_fmul = private unnamed_addr constant [13 x i8] c"double fmul:\00"
@.f64_fdiv = private unnamed_addr constant [13 x i8] c"double fdiv:\00"

declare i32 @printf(ptr, ...)
declare i32 @putchar(i32)
declare <16 x i8> @llvm.smax.v16i8(<16 x i8>, <16 x i8>)
declare <16 x i8> @llvm.ssub.sat.v16i8(<16 x i8>, <16 x i8>)
declare <16 x i8> @llvm.uadd.sat.v16i8(<16 x i8>, <16 x i8>)
declare <16 x i8> @llvm.umin.v16i8(<16 x i8>, <16 x i8>)
declare <2 x i64> @llvm.smax.v2i64(<2 x i64>, <2 x i64>)
declare <2 x i64> @llvm.ssub.sat.v2i64(<2 x i64>, <2 x i64>)
declare <2 x i64> @llvm.uadd.sat.v2i64(<2 x i64>, <2 x i64>)
declare <2 x i64> @llvm.umin.v2i64(<2 x i64>, <2 x i64>)
declare <4 x i32> @llvm.smax.v4i32(<4 x i32>, <4 x i32>)
declare <4 x i32> @llvm.ssub.sat.v4i32(<4 x i32>, <4 x i32>)
declare <4 x i32> @llvm.uadd.sat.v4i32(<4 x i32>, <4 x i32>)
declare <4 x i32> @llvm.umin.v4i32(<4 x i32>, <4 x i32>)
declare <8 x i16> @llvm.smax.v8i16(<8 x i16>, <8 x i16>)
declare <8 x i16> @llvm.ssub.sat.v8i16(<8 x i16>, <8 x i16>)
declare <8 x i16> @llvm.uadd.sat.v8i16(<8 x i16>, <8 x i16>)
declare <8 x i16> @llvm.umin.v8i16(<8 x i16>, <8 x i16>)

@.fmt = private unnamed_addr constant [3 x i8] c"%s\00"
@.x = private unnamed_addr constant [6 x i8] c" %llx\00"
@.gf = private unnamed_addr constant [7 x i8] c" %.17g\00"
@.z = private unnamed_addr constant [4 x i8] c" -0\00"
@.z0 = private unnamed_addr constant [3 x i8] c" 0\00"
@out = global <2 x i64> zeroinitializer, align 16

; Lanes of %n x %size bytes at @out, zero-extended, in hex.
define internal void @lanes(ptr %label, i32 %n, i32 %size) {
entry:
  call i32 (ptr, ...) @printf(ptr @.fmt, ptr %label)
  br label %loop
loop:
  %i = phi i32 [ 0, %entry ], [ %next, %print ]
  %off = mul i32 %i, %size
  %p = getelementptr i8, ptr @out, i32 %off
  switch i32 %size, label %s8 [ i32 1, label %s1  i32 2, label %s2  i32 4, label %s4 ]
s1:
  %v1 = load i8, ptr %p
  %w1 = zext i8 %v1 to i64
  br label %print
s2:
  %v2 = load i16, ptr %p
  %w2 = zext i16 %v2 to i64
  br label %print
s4:
  %v4 = load i32, ptr %p
  %w4 = zext i32 %v4 to i64
  br label %print
s8:
  %w8 = load i64, ptr %p
  br label %print
print:
  %w = phi i64 [ %w1, %s1 ], [ %w2, %s2 ], [ %w4, %s4 ], [ %w8, %s8 ]
  call i32 (ptr, ...) @printf(ptr @.x, i64 %w)
  %next = add i32 %i, 1
  %more = icmp ult i32 %next, %n
  br i1 %more, label %loop, label %done
done:
  call i32 @putchar(i32 10)
  ret void
}

; Lanes of floating point at @out (float if %f32), signed zeros shown.
define internal void @flanes(ptr %label, i32 %n, i1 %f32) {
entry:
  call i32 (ptr, ...) @printf(ptr @.fmt, ptr %label)
  br label %loop
loop:
  %i = phi i32 [ 0, %entry ], [ %next, %shown ]
  br i1 %f32, label %single, label %double
single:
  %ps = getelementptr float, ptr @out, i32 %i
  %s = load float, ptr %ps
  %sd = fpext float %s to double
  br label %value
double:
  %pd = getelementptr double, ptr @out, i32 %i
  %d = load double, ptr %pd
  br label %value
value:
  %v = phi double [ %sd, %single ], [ %d, %double ]
  %zero = fcmp oeq double %v, 0.0
  br i1 %zero, label %signed, label %number
signed:
  %bits = bitcast double %v to i64
  %neg = icmp slt i64 %bits, 0
  %z = select i1 %neg, ptr @.z, ptr @.z0
  call i32 (ptr, ...) @printf(ptr %z)
  br label %shown
number:
  call i32 (ptr, ...) @printf(ptr @.gf, double %v)
  br label %shown
shown:
  %next = add i32 %i, 1
  %more = icmp ult i32 %next, %n
  br i1 %more, label %loop, label %done
done:
  call i32 @putchar(i32 10)
  ret void
}

define i32 @main() {
  %t1 = load <16 x i8>, ptr @a8
  %t2 = load <16 x i8>, ptr @b8
  %t3 = add <16 x i8> %t1, %t2
  store <16 x i8> %t3, ptr @out
  call void @lanes(ptr @.i8_add, i32 16, i32 1)
  %t4 = load <16 x i8>, ptr @a8
  %t5 = load <16 x i8>, ptr @b8
  %t6 = sub <16 x i8> %t4, %t5
  store <16 x i8> %t6, ptr @out
  call void @lanes(ptr @.i8_sub, i32 16, i32 1)
  %t7 = load <16 x i8>, ptr @a8
  %t8 = load <16 x i8>, ptr @b8
  %t9 = mul <16 x i8> %t7, %t8
  store <16 x i8> %t9, ptr @out
  call void @lanes(ptr @.i8_mul, i32 16, i32 1)
  %t10 = load <16 x i8>, ptr @a8
  %t11 = load <16 x i8>, ptr @b8
  %t12 = and <16 x i8> %t10, %t11
  store <16 x i8> %t12, ptr @out
  call void @lanes(ptr @.i8_and, i32 16, i32 1)
  %t13 = load <16 x i8>, ptr @a8
  %t14 = load <16 x i8>, ptr @b8
  %t15 = or <16 x i8> %t13, %t14
  store <16 x i8> %t15, ptr @out
  call void @lanes(ptr @.i8_or, i32 16, i32 1)
  %t16 = load <16 x i8>, ptr @a8
  %t17 = load <16 x i8>, ptr @b8
  %t18 = xor <16 x i8> %t16, %t17
  store <16 x i8> %t18, ptr @out
  call void @lanes(ptr @.i8_xor, i32 16, i32 1)
  %t19 = load <16 x i8>, ptr @a8
  %t20 = load <16 x i8>, ptr @s8
  %t21 = shl <16 x i8> %t19, %t20
  store <16 x i8> %t21, ptr @out
  call void @lanes(ptr @.i8_shl, i32 16, i32 1)
  %t22 = load <16 x i8>, ptr @a8
  %t23 = load <16 x i8>, ptr @s8
  %t24 = lshr <16 x i8> %t22, %t23
  store <16 x i8> %t24, ptr @out
  call void @lanes(ptr @.i8_lshr, i32 16, i32 1)
  %t25 = load <16 x i8>, ptr @a8
  %t26 = load <16 x i8>, ptr @s8
  %t27 = ashr <16 x i8> %t25, %t26
  store <16 x i8> %t27, ptr @out
  call void @lanes(ptr @.i8_ashr, i32 16, i32 1)
  %t28 = load <16 x i8>, ptr @a8
  %t29 = load <16 x i8>, ptr @b8
  %t30 = call <16 x i8> @llvm.umin.v16i8(<16 x i8> %t28, <16 x i8> %t29)
  store <16 x i8> %t30, ptr @out
  call void @lanes(ptr @.i8_umin, i32 16, i32 1)
  %t31 = load <16 x i8>, ptr @a8
  %t32 = load <16 x i8>, ptr @b8
  %t33 = call <16 x i8> @llvm.smax.v16i8(<16 x i8> %t31, <16 x i8> %t32)
  store <16 x i8> %t33, ptr @out
  call void @lanes(ptr @.i8_smax, i32 16, i32 1)
  %t34 = load <16 x i8>, ptr @a8
  %t35 = load <16 x i8>, ptr @b8
  %t36 = call <16 x i8> @llvm.uadd.sat.v16i8(<16 x i8> %t34, <16 x i8> %t35)
  store <16 x i8> %t36, ptr @out
  call void @lanes(ptr @.i8_uadd_sat, i32 16, i32 1)
  %t37 = load <16 x i8>, ptr @a8
  %t38 = load <16 x i8>, ptr @b8
  %t39 = call <16 x i8> @llvm.ssub.sat.v16i8(<16 x i8> %t37, <16 x i8> %t38)
  store <16 x i8> %t39, ptr @out
  call void @lanes(ptr @.i8_ssub_sat, i32 16, i32 1)
  %t40 = load <16 x i8>, ptr @a8
  %t41 = load <16 x i8>, ptr @b8
  %t43 = icmp ugt <16 x i8> %t40, %t41
  %t42 = sext <16 x i1> %t43 to <16 x i8>
  store <16 x i8> %t42, ptr @out
  call void @lanes(ptr @.i8_icmp_ugt, i32 16, i32 1)
  %t44 = load <16 x i8>, ptr @a8
  %t45 = load <16 x i8>, ptr @b8
  %t47 = icmp sle <16 x i8> %t44, %t45
  %t46 = sext <16 x i1> %t47 to <16 x i8>
  store <16 x i8> %t46, ptr @out
  call void @lanes(ptr @.i8_icmp_sle, i32 16, i32 1)
  %t48 = load <8 x i16>, ptr @a16
  %t49 = load <8 x i16>, ptr @b16
  %t50 = add <8 x i16> %t48, %t49
  store <8 x i16> %t50, ptr @out
  call void @lanes(ptr @.i16_add, i32 8, i32 2)
  %t51 = load <8 x i16>, ptr @a16
  %t52 = load <8 x i16>, ptr @b16
  %t53 = sub <8 x i16> %t51, %t52
  store <8 x i16> %t53, ptr @out
  call void @lanes(ptr @.i16_sub, i32 8, i32 2)
  %t54 = load <8 x i16>, ptr @a16
  %t55 = load <8 x i16>, ptr @b16
  %t56 = mul <8 x i16> %t54, %t55
  store <8 x i16> %t56, ptr @out
  call void @lanes(ptr @.i16_mul, i32 8, i32 2)
  %t57 = load <8 x i16>, ptr @a16
  %t58 = load <8 x i16>, ptr @b16
  %t59 = and <8 x i16> %t57, %t58
  store <8 x i16> %t59, ptr @out
  call void @lanes(ptr @.i16_and, i32 8, i32 2)
  %t60 = load <8 x i16>, ptr @a16
  %t61 = load <8 x i16>, ptr @b16
  %t62 = or <8 x i16> %t60, %t61
  store <8 x i16> %t62, ptr @out
  call void @lanes(ptr @.i16_or, i32 8, i32 2)
  %t63 = load <8 x i16>, ptr @a16
  %t64 = load <8 x i16>, ptr @b16
  %t65 = xor <8 x i16> %t63, %t64
  store <8 x i16> %t65, ptr @out
  call void @lanes(ptr @.i16_xor, i32 8, i32 2)
  %t66 = load <8 x i16>, ptr @a16
  %t67 = load <8 x i16>, ptr @s16
  %t68 = shl <8 x i16> %t66, %t67
  store <8 x i16> %t68, ptr @out
  call void @lanes(ptr @.i16_shl, i32 8, i32 2)
  %t69 = load <8 x i16>, ptr @a16
  %t70 = load <8 x i16>, ptr @s16
  %t71 = lshr <8 x i16> %t69, %t70
  store <8 x i16> %t71, ptr @out
  call void @lanes(ptr @.i16_lshr, i32 8, i32 2)
  %t72 = load <8 x i16>, ptr @a16
  %t73 = load <8 x i16>, ptr @s16
  %t74 = ashr <8 x i16> %t72, %t73
  store <8 x i16> %t74, ptr @out
  call void @lanes(ptr @.i16_ashr, i32 8, i32 2)
  %t75 = load <8 x i16>, ptr @a16
  %t76 = load <8 x i16>, ptr @b16
  %t77 = call <8 x i16> @llvm.umin.v8i16(<8 x i16> %t75, <8 x i16> %t76)
  store <8 x i16> %t77, ptr @out
  call void @lanes(ptr @.i16_umin, i32 8, i32 2)
  %t78 = load <8 x i16>, ptr @a16
  %t79 = load <8 x i16>, ptr @b16
  %t80 = call <8 x i16> @llvm.smax.v8i16(<8 x i16> %t78, <8 x i16> %t79)
  store <8 x i16> %t80, ptr @out
  call void @lanes(ptr @.i16_smax, i32 8, i32 2)
  %t81 = load <8 x i16>, ptr @a16
  %t82 = load <8 x i16>, ptr @b16
  %t83 = call <8 x i16> @llvm.uadd.sat.v8i16(<8 x i16> %t81, <8 x i16> %t82)
  store <8 x i16> %t83, ptr @out
  call void @lanes(ptr @.i16_uadd_sat, i32 8, i32 2)
  %t84 = load <8 x i16>, ptr @a16
  %t85 = load <8 x i16>, ptr @b16
  %t86 = call <8 x i16> @llvm.ssub.sat.v8i16(<8 x i16> %t84, <8 x i16> %t85)
  store <8 x i16> %t86, ptr @out
  call void @lanes(ptr @.i16_ssub_sat, i32 8, i32 2)
  %t87 = load <8 x i16>, ptr @a16
  %t88 = load <8 x i16>, ptr @b16
  %t90 = icmp ugt <8 x i16> %t87, %t88
  %t89 = sext <8 x i1> %t90 to <8 x i16>
  store <8 x i16> %t89, ptr @out
  call void @lanes(ptr @.i16_icmp_ugt, i32 8, i32 2)
  %t91 = load <8 x i16>, ptr @a16
  %t92 = load <8 x i16>, ptr @b16
  %t94 = icmp sle <8 x i16> %t91, %t92
  %t93 = sext <8 x i1> %t94 to <8 x i16>
  store <8 x i16> %t93, ptr @out
  call void @lanes(ptr @.i16_icmp_sle, i32 8, i32 2)
  %t95 = load <4 x i32>, ptr @a32
  %t96 = load <4 x i32>, ptr @b32
  %t97 = add <4 x i32> %t95, %t96
  store <4 x i32> %t97, ptr @out
  call void @lanes(ptr @.i32_add, i32 4, i32 4)
  %t98 = load <4 x i32>, ptr @a32
  %t99 = load <4 x i32>, ptr @b32
  %t100 = sub <4 x i32> %t98, %t99
  store <4 x i32> %t100, ptr @out
  call void @lanes(ptr @.i32_sub, i32 4, i32 4)
  %t101 = load <4 x i32>, ptr @a32
  %t102 = load <4 x i32>, ptr @b32
  %t103 = mul <4 x i32> %t101, %t102
  store <4 x i32> %t103, ptr @out
  call void @lanes(ptr @.i32_mul, i32 4, i32 4)
  %t104 = load <4 x i32>, ptr @a32
  %t105 = load <4 x i32>, ptr @b32
  %t106 = and <4 x i32> %t104, %t105
  store <4 x i32> %t106, ptr @out
  call void @lanes(ptr @.i32_and, i32 4, i32 4)
  %t107 = load <4 x i32>, ptr @a32
  %t108 = load <4 x i32>, ptr @b32
  %t109 = or <4 x i32> %t107, %t108
  store <4 x i32> %t109, ptr @out
  call void @lanes(ptr @.i32_or, i32 4, i32 4)
  %t110 = load <4 x i32>, ptr @a32
  %t111 = load <4 x i32>, ptr @b32
  %t112 = xor <4 x i32> %t110, %t111
  store <4 x i32> %t112, ptr @out
  call void @lanes(ptr @.i32_xor, i32 4, i32 4)
  %t113 = load <4 x i32>, ptr @a32
  %t114 = load <4 x i32>, ptr @s32
  %t115 = shl <4 x i32> %t113, %t114
  store <4 x i32> %t115, ptr @out
  call void @lanes(ptr @.i32_shl, i32 4, i32 4)
  %t116 = load <4 x i32>, ptr @a32
  %t117 = load <4 x i32>, ptr @s32
  %t118 = lshr <4 x i32> %t116, %t117
  store <4 x i32> %t118, ptr @out
  call void @lanes(ptr @.i32_lshr, i32 4, i32 4)
  %t119 = load <4 x i32>, ptr @a32
  %t120 = load <4 x i32>, ptr @s32
  %t121 = ashr <4 x i32> %t119, %t120
  store <4 x i32> %t121, ptr @out
  call void @lanes(ptr @.i32_ashr, i32 4, i32 4)
  %t122 = load <4 x i32>, ptr @a32
  %t123 = load <4 x i32>, ptr @b32
  %t124 = call <4 x i32> @llvm.umin.v4i32(<4 x i32> %t122, <4 x i32> %t123)
  store <4 x i32> %t124, ptr @out
  call void @lanes(ptr @.i32_umin, i32 4, i32 4)
  %t125 = load <4 x i32>, ptr @a32
  %t126 = load <4 x i32>, ptr @b32
  %t127 = call <4 x i32> @llvm.smax.v4i32(<4 x i32> %t125, <4 x i32> %t126)
  store <4 x i32> %t127, ptr @out
  call void @lanes(ptr @.i32_smax, i32 4, i32 4)
  %t128 = load <4 x i32>, ptr @a32
  %t129 = load <4 x i32>, ptr @b32
  %t130 = call <4 x i32> @llvm.uadd.sat.v4i32(<4 x i32> %t128, <4 x i32> %t129)
  store <4 x i32> %t130, ptr @out
  call void @lanes(ptr @.i32_uadd_sat, i32 4, i32 4)
  %t131 = load <4 x i32>, ptr @a32
  %t132 = load <4 x i32>, ptr @b32
  %t133 = call <4 x i32> @llvm.ssub.sat.v4i32(<4 x i32> %t131, <4 x i32> %t132)
  store <4 x i32> %t133, ptr @out
  call void @lanes(ptr @.i32_ssub_sat, i32 4, i32 4)
  %t134 = load <4 x i32>, ptr @a32
  %t135 = load <4 x i32>, ptr @b32
  %t137 = icmp ugt <4 x i32> %t134, %t135
  %t136 = sext <4 x i1> %t137 to <4 x i32>
  store <4 x i32> %t136, ptr @out
  call void @lanes(ptr @.i32_icmp_ugt, i32 4, i32 4)
  %t138 = load <4 x i32>, ptr @a32
  %t139 = load <4 x i32>, ptr @b32
  %t141 = icmp sle <4 x i32> %t138, %t139
  %t140 = sext <4 x i1> %t141 to <4 x i32>
  store <4 x i32> %t140, ptr @out
  call void @lanes(ptr @.i32_icmp_sle, i32 4, i32 4)
  %t142 = load <2 x i64>, ptr @a64
  %t143 = load <2 x i64>, ptr @b64
  %t144 = add <2 x i64> %t142, %t143
  store <2 x i64> %t144, ptr @out
  call void @lanes(ptr @.i64_add, i32 2, i32 8)
  %t145 = load <2 x i64>, ptr @a64
  %t146 = load <2 x i64>, ptr @b64
  %t147 = sub <2 x i64> %t145, %t146
  store <2 x i64> %t147, ptr @out
  call void @lanes(ptr @.i64_sub, i32 2, i32 8)
  %t148 = load <2 x i64>, ptr @a64
  %t149 = load <2 x i64>, ptr @b64
  %t150 = mul <2 x i64> %t148, %t149
  store <2 x i64> %t150, ptr @out
  call void @lanes(ptr @.i64_mul, i32 2, i32 8)
  %t151 = load <2 x i64>, ptr @a64
  %t152 = load <2 x i64>, ptr @b64
  %t153 = and <2 x i64> %t151, %t152
  store <2 x i64> %t153, ptr @out
  call void @lanes(ptr @.i64_and, i32 2, i32 8)
  %t154 = load <2 x i64>, ptr @a64
  %t155 = load <2 x i64>, ptr @b64
  %t156 = or <2 x i64> %t154, %t155
  store <2 x i64> %t156, ptr @out
  call void @lanes(ptr @.i64_or, i32 2, i32 8)
  %t157 = load <2 x i64>, ptr @a64
  %t158 = load <2 x i64>, ptr @b64
  %t159 = xor <2 x i64> %t157, %t158
  store <2 x i64> %t159, ptr @out
  call void @lanes(ptr @.i64_xor, i32 2, i32 8)
  %t160 = load <2 x i64>, ptr @a64
  %t161 = load <2 x i64>, ptr @s64
  %t162 = shl <2 x i64> %t160, %t161
  store <2 x i64> %t162, ptr @out
  call void @lanes(ptr @.i64_shl, i32 2, i32 8)
  %t163 = load <2 x i64>, ptr @a64
  %t164 = load <2 x i64>, ptr @s64
  %t165 = lshr <2 x i64> %t163, %t164
  store <2 x i64> %t165, ptr @out
  call void @lanes(ptr @.i64_lshr, i32 2, i32 8)
  %t166 = load <2 x i64>, ptr @a64
  %t167 = load <2 x i64>, ptr @s64
  %t168 = ashr <2 x i64> %t166, %t167
  store <2 x i64> %t168, ptr @out
  call void @lanes(ptr @.i64_ashr, i32 2, i32 8)
  %t169 = load <2 x i64>, ptr @a64
  %t170 = load <2 x i64>, ptr @b64
  %t171 = call <2 x i64> @llvm.umin.v2i64(<2 x i64> %t169, <2 x i64> %t170)
  store <2 x i64> %t171, ptr @out
  call void @lanes(ptr @.i64_umin, i32 2, i32 8)
  %t172 = load <2 x i64>, ptr @a64
  %t173 = load <2 x i64>, ptr @b64
  %t174 = call <2 x i64> @llvm.smax.v2i64(<2 x i64> %t172, <2 x i64> %t173)
  store <2 x i64> %t174, ptr @out
  call void @lanes(ptr @.i64_smax, i32 2, i32 8)
  %t175 = load <2 x i64>, ptr @a64
  %t176 = load <2 x i64>, ptr @b64
  %t177 = call <2 x i64> @llvm.uadd.sat.v2i64(<2 x i64> %t175, <2 x i64> %t176)
  store <2 x i64> %t177, ptr @out
  call void @lanes(ptr @.i64_uadd_sat, i32 2, i32 8)
  %t178 = load <2 x i64>, ptr @a64
  %t179 = load <2 x i64>, ptr @b64
  %t180 = call <2 x i64> @llvm.ssub.sat.v2i64(<2 x i64> %t178, <2 x i64> %t179)
  store <2 x i64> %t180, ptr @out
  call void @lanes(ptr @.i64_ssub_sat, i32 2, i32 8)
  %t181 = load <2 x i64>, ptr @a64
  %t182 = load <2 x i64>, ptr @b64
  %t184 = icmp ugt <2 x i64> %t181, %t182
  %t183 = sext <2 x i1> %t184 to <2 x i64>
  store <2 x i64> %t183, ptr @out
  call void @lanes(ptr @.i64_icmp_ugt, i32 2, i32 8)
  %t185 = load <2 x i64>, ptr @a64
  %t186 = load <2 x i64>, ptr @b64
  %t188 = icmp sle <2 x i64> %t185, %t186
  %t187 = sext <2 x i1> %t188 to <2 x i64>
  store <2 x i64> %t187, ptr @out
  call void @lanes(ptr @.i64_icmp_sle, i32 2, i32 8)
  %t189 = load <4 x float>, ptr @faf32
  %t190 = load <4 x float>, ptr @fbf32
  %t191 = fadd <4 x float> %t189, %t190
  store <4 x float> %t191, ptr @out
  call void @flanes(ptr @.f32_fadd, i32 4, i1 1)
  %t192 = load <4 x float>, ptr @faf32
  %t193 = load <4 x float>, ptr @fbf32
  %t194 = fsub <4 x float> %t192, %t193
  store <4 x float> %t194, ptr @out
  call void @flanes(ptr @.f32_fsub, i32 4, i1 1)
  %t195 = load <4 x float>, ptr @faf32
  %t196 = load <4 x float>, ptr @fbf32
  %t197 = fmul <4 x float> %t195, %t196
  store <4 x float> %t197, ptr @out
  call void @flanes(ptr @.f32_fmul, i32 4, i1 1)
  %t198 = load <4 x float>, ptr @faf32
  %t199 = load <4 x float>, ptr @fbf32
  %t200 = fdiv <4 x float> %t198, %t199
  store <4 x float> %t200, ptr @out
  call void @flanes(ptr @.f32_fdiv, i32 4, i1 1)
  %t201 = load <2 x double>, ptr @faf64
  %t202 = load <2 x double>, ptr @fbf64
  %t203 = fadd <2 x double> %t201, %t202
  store <2 x double> %t203, ptr @out
  call void @flanes(ptr @.f64_fadd, i32 2, i1 0)
  %t204 = load <2 x double>, ptr @faf64
  %t205 = load <2 x double>, ptr @fbf64
  %t206 = fsub <2 x double> %t204, %t205
  store <2 x double> %t206, ptr @out
  call void @flanes(ptr @.f64_fsub, i32 2, i1 0)
  %t207 = load <2 x double>, ptr @faf64
  %t208 = load <2 x double>, ptr @fbf64
  %t209 = fmul <2 x double> %t207, %t208
  store <2 x double> %t209, ptr @out
  call void @flanes(ptr @.f64_fmul, i32 2, i1 0)
  %t210 = load <2 x double>, ptr @faf64
  %t211 = load <2 x double>, ptr @fbf64
  %t212 = fdiv <2 x double> %t210, %t211
  store <2 x double> %t212, ptr @out
  call void @flanes(ptr @.f64_fdiv, i32 2, i1 0)
  ret i32 0
}
