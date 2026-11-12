; Vector masks (WP-081): <N x i1> as bit-packed values. The libc++
; mismatch and find shapes (icmp of byte vectors, bitcast of the mask to
; an integer, cttz, reduce.or), select on a mask, sext and zext of masks,
; fcmp with a NaN lane, integer reductions of lanes and of masks, and a
; <3 x i1> through memory: stored over a byte of ones (its padding bits
; are zero), and a global mask loaded and bitcast to i3.
;@ covers: spec/ir.md §5.4 integer
;@ unsupported(x86_64): *
;@ stdout: mismatch: 8 | find: 1 2 | sum: 10 48 10 0
;@ stdout: fcmp: 13 4 -3 | mem: 5 2 | parity: 0 0

@.line1 = private unnamed_addr constant [44 x i8] c"mismatch: %d | find: %d %d | sum: %d %d %d \00"
@.line2 = private unnamed_addr constant [48 x i8] c"%d\0Afcmp: %d %d %d | mem: %d %d | parity: %d %d\0A\00"

@a = global [16 x i8] c"hello, world 123"
@b = global [16 x i8] c"hello, wOrld 123"
@ints = global <8 x i32> <i32 3, i32 -1, i32 7, i32 10, i32 0, i32 5, i32 6, i32 -20>
@floats = global <4 x float> <float 1.0, float 0x7FF8000000000000, float -2.0, float 3.5>
@m3 = global <3 x i1> <i1 true, i1 false, i1 true>

declare i32 @printf(ptr, ...)
declare i16 @llvm.cttz.i16(i16, i1)
declare i1 @llvm.vector.reduce.or.v16i1(<16 x i1>)
declare i1 @llvm.vector.reduce.xor.v16i1(<16 x i1>)
declare i1 @llvm.vector.reduce.and.v4i1(<4 x i1>)
declare i8 @llvm.vector.reduce.add.v16i8(<16 x i8>)
declare i32 @llvm.vector.reduce.add.v8i32(<8 x i32>)
declare i32 @llvm.vector.reduce.add.v4i32(<4 x i32>)
declare i32 @llvm.vector.reduce.smax.v8i32(<8 x i32>)
declare i32 @llvm.vector.reduce.umin.v8i32(<8 x i32>)

define i32 @main() {
entry:
  ; mismatch: the first index where the two strings differ.
  %x = load <16 x i8>, ptr @a
  %y = load <16 x i8>, ptr @b
  %ne = icmp ne <16 x i8> %x, %y
  %bits = bitcast <16 x i1> %ne to i16
  %tz = call i16 @llvm.cttz.i16(i16 %bits, i1 false)
  %tz32 = zext i16 %tz to i32

  ; find: is there an 'o', and how many.
  %eqo = icmp eq <16 x i8> %x, <i8 111, i8 111, i8 111, i8 111, i8 111, i8 111, i8 111, i8 111, i8 111, i8 111, i8 111, i8 111, i8 111, i8 111, i8 111, i8 111>
  %any = call i1 @llvm.vector.reduce.or.v16i1(<16 x i1> %eqo)
  %any32 = zext i1 %any to i32
  %ones = zext <16 x i1> %eqo to <16 x i8>
  %count = call i8 @llvm.vector.reduce.add.v16i8(<16 x i8> %ones)
  %count32 = zext i8 %count to i32

  ; Lanes: the sum, the sum of max(x, 5) by select, smax, umin.
  %ints = load <8 x i32>, ptr @ints
  %sum = call i32 @llvm.vector.reduce.add.v8i32(<8 x i32> %ints)
  %gt = icmp sgt <8 x i32> %ints, <i32 5, i32 5, i32 5, i32 5, i32 5, i32 5, i32 5, i32 5>
  %max = select <8 x i1> %gt, <8 x i32> %ints, <8 x i32> <i32 5, i32 5, i32 5, i32 5, i32 5, i32 5, i32 5, i32 5>
  %summax = call i32 @llvm.vector.reduce.add.v8i32(<8 x i32> %max)
  %smax = call i32 @llvm.vector.reduce.smax.v8i32(<8 x i32> %ints)
  %umin = call i32 @llvm.vector.reduce.umin.v8i32(<8 x i32> %ints)
  call i32 (ptr, ...) @printf(ptr @.line1, i32 %tz32, i32 %any32, i32 %count32, i32 %sum, i32 %summax, i32 %smax)

  ; fcmp: ordered lanes as bits, lanes below zero, sext of the mask.
  %f = load <4 x float>, ptr @floats
  %ord = fcmp ord <4 x float> %f, %f
  %ordbits = bitcast <4 x i1> %ord to i4
  %ord32 = zext i4 %ordbits to i32
  %neg = fcmp olt <4 x float> %f, zeroinitializer
  %negbits = bitcast <4 x i1> %neg to i4
  %neg32 = zext i4 %negbits to i32
  %se = sext <4 x i1> %ord to <4 x i32>
  %sesum = call i32 @llvm.vector.reduce.add.v4i32(<4 x i32> %se)

  ; A <3 x i1> stored over a byte of ones, then read as the byte.
  %slot = alloca i8
  store i8 -1, ptr %slot
  %c3 = icmp slt <3 x i32> <i32 -1, i32 2, i32 -3>, zeroinitializer
  store <3 x i1> %c3, ptr %slot
  %byte = load i8, ptr %slot
  %byte32 = zext i8 %byte to i32

  ; A global mask: loaded, inverted, as an i3.
  %g = load <3 x i1>, ptr @m3
  %gx = xor <3 x i1> %g, <i1 true, i1 true, i1 true>
  %gi = bitcast <3 x i1> %gx to i3
  %g32 = zext i3 %gi to i32

  ; Reductions of masks: the parity of the 'o's, every lane ordered.
  %par = call i1 @llvm.vector.reduce.xor.v16i1(<16 x i1> %eqo)
  %par32 = zext i1 %par to i32
  %all = call i1 @llvm.vector.reduce.and.v4i1(<4 x i1> %ord)
  %all32 = zext i1 %all to i32
  call i32 (ptr, ...) @printf(ptr @.line2, i32 %umin, i32 %ord32, i32 %neg32, i32 %sesum, i32 %byte32, i32 %g32, i32 %par32, i32 %all32)
  ret i32 0
}
