; Bit-manipulation intrinsics at i8 to i64: ctpop, ctlz and cttz (zero
; defined), bswap, bitreverse, the funnel shifts fshl/fshr (shift amounts
; past the width), abs, smin and umax. Operands come from memory, so
; nothing is folded. Expected values computed by a script, not a compiler.
;@ covers: spec/ir.md §5.5 intrinsics (ctpop, ctlz, cttz, bswap, bitreverse, fshl, fshr, abs, smin, umax)
;@ stdout: i8 x=0 y=3 pop=0 clz=8 ctz=8 brev=0 fshl=0 fshr=0 abs=0 smin=0 umax=3
;@ stdout: i8 x=1 y=9 pop=1 clz=7 ctz=0 brev=80 fshl=2 fshr=84 abs=1 smin=1 umax=9
;@ stdout: i8 x=80 y=1 pop=1 clz=0 ctz=7 brev=1 fshl=0 fshr=0 abs=80 smin=-128 umax=80
;@ stdout: i8 x=b4 y=d pop=4 clz=0 ctz=2 brev=2d fshl=81 fshr=a0 abs=4c smin=-76 umax=b4
;@ stdout: i8 x=ff y=7 pop=8 clz=0 ctz=0 brev=ff fshl=83 fshr=fe abs=1 smin=-1 umax=ff
;@ stdout: i16 x=0 y=5 pop=0 clz=16 ctz=16 bswap=0 brev=0 fshl=0 fshr=0 abs=0 smin=0 umax=5
;@ stdout: i16 x=8001 y=3 pop=2 clz=0 ctz=0 bswap=180 brev=8001 fshl=8 fshr=2000 abs=7fff smin=-32767 umax=8001
;@ stdout: i16 x=1234 y=14 pop=5 clz=3 ctz=2 bswap=3412 brev=2c48 fshl=2340 fshr=4001 abs=1234 smin=20 umax=1234
;@ stdout: i16 x=f00f y=f pop=8 clz=0 ctz=0 bswap=ff0 brev=f00f fshl=8007 fshr=e01e abs=ff1 smin=-4081 umax=f00f
;@ stdout: i32 x=0 y=1 pop=0 clz=32 ctz=32 bswap=0 brev=0 fshl=0 fshr=0 abs=0 smin=0 umax=1
;@ stdout: i32 x=1 y=1f pop=1 clz=31 ctz=0 bswap=1000000 brev=80000000 fshl=8000000f fshr=2 abs=1 smin=1 umax=1f
;@ stdout: i32 x=80000000 y=21 pop=1 clz=0 ctz=31 bswap=80 brev=1 fshl=0 fshr=10 abs=80000000 smin=-2147483648 umax=80000000
;@ stdout: i32 x=12345678 y=4 pop=13 clz=3 ctz=3 bswap=78563412 brev=1e6a2c48 fshl=23456780 fshr=80000000 abs=12345678 smin=4 umax=12345678
;@ stdout: i32 x=fffffffe y=28 pop=31 clz=0 ctz=1 bswap=feffffff brev=7fffffff fshl=fffffe00 fshr=fe000000 abs=2 smin=-2 umax=fffffffe
;@ stdout: i64 x=0 y=3f pop=0 clz=64 ctz=64 bswap=0 brev=0 fshl=1f fshr=0 abs=0 smin=0 umax=3f
;@ stdout: i64 x=1 y=40 pop=1 clz=63 ctz=0 bswap=100000000000000 brev=8000000000000000 fshl=1 fshr=40 abs=1 smin=1 umax=40
;@ stdout: i64 x=8000000000000000 y=1 pop=1 clz=0 ctz=63 bswap=80 brev=1 fshl=0 fshr=0 abs=8000000000000000 smin=-9223372036854775808 umax=8000000000000000
;@ stdout: i64 x=123456789abcdef y=c pop=32 clz=7 ctz=0 bswap=efcdab8967452301 brev=f7b3d591e6a2c480 fshl=3456789abcdef000 fshr=def0000000000000 abs=123456789abcdef smin=12 umax=123456789abcdef
;@ stdout: i64 x=fffffffffffffff0 y=64 pop=60 clz=0 ctz=4 bswap=f0ffffffffffffff brev=fffffffffffffff fshl=ffffff0000000000 fshr=ffffffff00000000 abs=10 smin=-16 umax=fffffffffffffff0

@.f8 = private unnamed_addr constant [98 x i8] c"i8 x=%llx y=%llx pop=%d clz=%d ctz=%d brev=%llx fshl=%llx fshr=%llx abs=%llx smin=%lld umax=%llx\0A\00"
@.f16 = private unnamed_addr constant [110 x i8] c"i16 x=%llx y=%llx pop=%d clz=%d ctz=%d bswap=%llx brev=%llx fshl=%llx fshr=%llx abs=%llx smin=%lld umax=%llx\0A\00"
@.f32 = private unnamed_addr constant [110 x i8] c"i32 x=%llx y=%llx pop=%d clz=%d ctz=%d bswap=%llx brev=%llx fshl=%llx fshr=%llx abs=%llx smin=%lld umax=%llx\0A\00"
@.f64 = private unnamed_addr constant [110 x i8] c"i64 x=%llx y=%llx pop=%d clz=%d ctz=%d bswap=%llx brev=%llx fshl=%llx fshr=%llx abs=%llx smin=%lld umax=%llx\0A\00"
@.v8 = private constant [10 x i8] [i8 0, i8 3, i8 1, i8 9, i8 -128, i8 1, i8 -76, i8 13, i8 -1, i8 7]
@.v16 = private constant [8 x i16] [i16 0, i16 5, i16 -32767, i16 3, i16 4660, i16 20, i16 -4081, i16 15]
@.v32 = private constant [10 x i32] [i32 0, i32 1, i32 1, i32 31, i32 -2147483648, i32 33, i32 305419896, i32 4, i32 -2, i32 40]
@.v64 = private constant [10 x i64] [i64 0, i64 63, i64 1, i64 64, i64 -9223372036854775808, i64 1, i64 81985529216486895, i64 12, i64 -16, i64 100]

declare i32 @printf(ptr, ...)
declare i8 @llvm.ctpop.i8(i8)
declare i8 @llvm.ctlz.i8(i8, i1)
declare i8 @llvm.cttz.i8(i8, i1)
declare i8 @llvm.abs.i8(i8, i1)
declare i8 @llvm.bitreverse.i8(i8)
declare i8 @llvm.fshl.i8(i8, i8, i8)
declare i8 @llvm.fshr.i8(i8, i8, i8)
declare i8 @llvm.smin.i8(i8, i8)
declare i8 @llvm.umax.i8(i8, i8)
declare i16 @llvm.ctpop.i16(i16)
declare i16 @llvm.ctlz.i16(i16, i1)
declare i16 @llvm.cttz.i16(i16, i1)
declare i16 @llvm.abs.i16(i16, i1)
declare i16 @llvm.bswap.i16(i16)
declare i16 @llvm.bitreverse.i16(i16)
declare i16 @llvm.fshl.i16(i16, i16, i16)
declare i16 @llvm.fshr.i16(i16, i16, i16)
declare i16 @llvm.smin.i16(i16, i16)
declare i16 @llvm.umax.i16(i16, i16)
declare i32 @llvm.ctpop.i32(i32)
declare i32 @llvm.ctlz.i32(i32, i1)
declare i32 @llvm.cttz.i32(i32, i1)
declare i32 @llvm.abs.i32(i32, i1)
declare i32 @llvm.bswap.i32(i32)
declare i32 @llvm.bitreverse.i32(i32)
declare i32 @llvm.fshl.i32(i32, i32, i32)
declare i32 @llvm.fshr.i32(i32, i32, i32)
declare i32 @llvm.smin.i32(i32, i32)
declare i32 @llvm.umax.i32(i32, i32)
declare i64 @llvm.ctpop.i64(i64)
declare i64 @llvm.ctlz.i64(i64, i1)
declare i64 @llvm.cttz.i64(i64, i1)
declare i64 @llvm.abs.i64(i64, i1)
declare i64 @llvm.bswap.i64(i64)
declare i64 @llvm.bitreverse.i64(i64)
declare i64 @llvm.fshl.i64(i64, i64, i64)
declare i64 @llvm.fshr.i64(i64, i64, i64)
declare i64 @llvm.smin.i64(i64, i64)
declare i64 @llvm.umax.i64(i64, i64)

define void @bits8(ptr %p) {
  %x = load i8, ptr %p
  %q = getelementptr i8, ptr %p, i64 1
  %y = load i8, ptr %q
  %pop = call i8 @llvm.ctpop.i8(i8 %x)
  %clz = call i8 @llvm.ctlz.i8(i8 %x, i1 false)
  %ctz = call i8 @llvm.cttz.i8(i8 %x, i1 false)
  %brv = call i8 @llvm.bitreverse.i8(i8 %x)
  %fl = call i8 @llvm.fshl.i8(i8 %x, i8 %y, i8 %y)
  %fr = call i8 @llvm.fshr.i8(i8 %x, i8 %y, i8 %y)
  %ab = call i8 @llvm.abs.i8(i8 %x, i1 false)
  %mn = call i8 @llvm.smin.i8(i8 %x, i8 %y)
  %mx = call i8 @llvm.umax.i8(i8 %x, i8 %y)
  %x.w = zext i8 %x to i64
  %y.w = zext i8 %y to i64
  %pop.w = zext i8 %pop to i64
  %clz.w = zext i8 %clz to i64
  %ctz.w = zext i8 %ctz to i64
  %brv.w = zext i8 %brv to i64
  %fl.w = zext i8 %fl to i64
  %fr.w = zext i8 %fr to i64
  %ab.w = zext i8 %ab to i64
  %mn.w = sext i8 %mn to i64
  %mx.w = zext i8 %mx to i64
  %pop.i = trunc i64 %pop.w to i32
  %clz.i = trunc i64 %clz.w to i32
  %ctz.i = trunc i64 %ctz.w to i32
  call i32 (ptr, ...) @printf(ptr @.f8, i64 %x.w, i64 %y.w, i32 %pop.i, i32 %clz.i, i32 %ctz.i, i64 %brv.w, i64 %fl.w, i64 %fr.w, i64 %ab.w, i64 %mn.w, i64 %mx.w)
  ret void
}

define void @bits16(ptr %p) {
  %x = load i16, ptr %p
  %q = getelementptr i16, ptr %p, i64 1
  %y = load i16, ptr %q
  %pop = call i16 @llvm.ctpop.i16(i16 %x)
  %clz = call i16 @llvm.ctlz.i16(i16 %x, i1 false)
  %ctz = call i16 @llvm.cttz.i16(i16 %x, i1 false)
  %bsw = call i16 @llvm.bswap.i16(i16 %x)
  %brv = call i16 @llvm.bitreverse.i16(i16 %x)
  %fl = call i16 @llvm.fshl.i16(i16 %x, i16 %y, i16 %y)
  %fr = call i16 @llvm.fshr.i16(i16 %x, i16 %y, i16 %y)
  %ab = call i16 @llvm.abs.i16(i16 %x, i1 false)
  %mn = call i16 @llvm.smin.i16(i16 %x, i16 %y)
  %mx = call i16 @llvm.umax.i16(i16 %x, i16 %y)
  %x.w = zext i16 %x to i64
  %y.w = zext i16 %y to i64
  %pop.w = zext i16 %pop to i64
  %clz.w = zext i16 %clz to i64
  %ctz.w = zext i16 %ctz to i64
  %bsw.w = zext i16 %bsw to i64
  %brv.w = zext i16 %brv to i64
  %fl.w = zext i16 %fl to i64
  %fr.w = zext i16 %fr to i64
  %ab.w = zext i16 %ab to i64
  %mn.w = sext i16 %mn to i64
  %mx.w = zext i16 %mx to i64
  %pop.i = trunc i64 %pop.w to i32
  %clz.i = trunc i64 %clz.w to i32
  %ctz.i = trunc i64 %ctz.w to i32
  call i32 (ptr, ...) @printf(ptr @.f16, i64 %x.w, i64 %y.w, i32 %pop.i, i32 %clz.i, i32 %ctz.i, i64 %bsw.w, i64 %brv.w, i64 %fl.w, i64 %fr.w, i64 %ab.w, i64 %mn.w, i64 %mx.w)
  ret void
}

define void @bits32(ptr %p) {
  %x = load i32, ptr %p
  %q = getelementptr i32, ptr %p, i64 1
  %y = load i32, ptr %q
  %pop = call i32 @llvm.ctpop.i32(i32 %x)
  %clz = call i32 @llvm.ctlz.i32(i32 %x, i1 false)
  %ctz = call i32 @llvm.cttz.i32(i32 %x, i1 false)
  %bsw = call i32 @llvm.bswap.i32(i32 %x)
  %brv = call i32 @llvm.bitreverse.i32(i32 %x)
  %fl = call i32 @llvm.fshl.i32(i32 %x, i32 %y, i32 %y)
  %fr = call i32 @llvm.fshr.i32(i32 %x, i32 %y, i32 %y)
  %ab = call i32 @llvm.abs.i32(i32 %x, i1 false)
  %mn = call i32 @llvm.smin.i32(i32 %x, i32 %y)
  %mx = call i32 @llvm.umax.i32(i32 %x, i32 %y)
  %x.w = zext i32 %x to i64
  %y.w = zext i32 %y to i64
  %pop.w = zext i32 %pop to i64
  %clz.w = zext i32 %clz to i64
  %ctz.w = zext i32 %ctz to i64
  %bsw.w = zext i32 %bsw to i64
  %brv.w = zext i32 %brv to i64
  %fl.w = zext i32 %fl to i64
  %fr.w = zext i32 %fr to i64
  %ab.w = zext i32 %ab to i64
  %mn.w = sext i32 %mn to i64
  %mx.w = zext i32 %mx to i64
  %pop.i = trunc i64 %pop.w to i32
  %clz.i = trunc i64 %clz.w to i32
  %ctz.i = trunc i64 %ctz.w to i32
  call i32 (ptr, ...) @printf(ptr @.f32, i64 %x.w, i64 %y.w, i32 %pop.i, i32 %clz.i, i32 %ctz.i, i64 %bsw.w, i64 %brv.w, i64 %fl.w, i64 %fr.w, i64 %ab.w, i64 %mn.w, i64 %mx.w)
  ret void
}

define void @bits64(ptr %p) {
  %x = load i64, ptr %p
  %q = getelementptr i64, ptr %p, i64 1
  %y = load i64, ptr %q
  %pop = call i64 @llvm.ctpop.i64(i64 %x)
  %clz = call i64 @llvm.ctlz.i64(i64 %x, i1 false)
  %ctz = call i64 @llvm.cttz.i64(i64 %x, i1 false)
  %bsw = call i64 @llvm.bswap.i64(i64 %x)
  %brv = call i64 @llvm.bitreverse.i64(i64 %x)
  %fl = call i64 @llvm.fshl.i64(i64 %x, i64 %y, i64 %y)
  %fr = call i64 @llvm.fshr.i64(i64 %x, i64 %y, i64 %y)
  %ab = call i64 @llvm.abs.i64(i64 %x, i1 false)
  %mn = call i64 @llvm.smin.i64(i64 %x, i64 %y)
  %mx = call i64 @llvm.umax.i64(i64 %x, i64 %y)
  %x.w = add i64 %x, 0
  %y.w = add i64 %y, 0
  %pop.w = add i64 %pop, 0
  %clz.w = add i64 %clz, 0
  %ctz.w = add i64 %ctz, 0
  %bsw.w = add i64 %bsw, 0
  %brv.w = add i64 %brv, 0
  %fl.w = add i64 %fl, 0
  %fr.w = add i64 %fr, 0
  %ab.w = add i64 %ab, 0
  %mn.w = add i64 %mn, 0
  %mx.w = add i64 %mx, 0
  %pop.i = trunc i64 %pop.w to i32
  %clz.i = trunc i64 %clz.w to i32
  %ctz.i = trunc i64 %ctz.w to i32
  call i32 (ptr, ...) @printf(ptr @.f64, i64 %x.w, i64 %y.w, i32 %pop.i, i32 %clz.i, i32 %ctz.i, i64 %bsw.w, i64 %brv.w, i64 %fl.w, i64 %fr.w, i64 %ab.w, i64 %mn.w, i64 %mx.w)
  ret void
}

define i32 @main() {
entry:
  %p8.0 = getelementptr i8, ptr @.v8, i64 0
  call void @bits8(ptr %p8.0)
  %p8.1 = getelementptr i8, ptr @.v8, i64 2
  call void @bits8(ptr %p8.1)
  %p8.2 = getelementptr i8, ptr @.v8, i64 4
  call void @bits8(ptr %p8.2)
  %p8.3 = getelementptr i8, ptr @.v8, i64 6
  call void @bits8(ptr %p8.3)
  %p8.4 = getelementptr i8, ptr @.v8, i64 8
  call void @bits8(ptr %p8.4)
  %p16.0 = getelementptr i16, ptr @.v16, i64 0
  call void @bits16(ptr %p16.0)
  %p16.1 = getelementptr i16, ptr @.v16, i64 2
  call void @bits16(ptr %p16.1)
  %p16.2 = getelementptr i16, ptr @.v16, i64 4
  call void @bits16(ptr %p16.2)
  %p16.3 = getelementptr i16, ptr @.v16, i64 6
  call void @bits16(ptr %p16.3)
  %p32.0 = getelementptr i32, ptr @.v32, i64 0
  call void @bits32(ptr %p32.0)
  %p32.1 = getelementptr i32, ptr @.v32, i64 2
  call void @bits32(ptr %p32.1)
  %p32.2 = getelementptr i32, ptr @.v32, i64 4
  call void @bits32(ptr %p32.2)
  %p32.3 = getelementptr i32, ptr @.v32, i64 6
  call void @bits32(ptr %p32.3)
  %p32.4 = getelementptr i32, ptr @.v32, i64 8
  call void @bits32(ptr %p32.4)
  %p64.0 = getelementptr i64, ptr @.v64, i64 0
  call void @bits64(ptr %p64.0)
  %p64.1 = getelementptr i64, ptr @.v64, i64 2
  call void @bits64(ptr %p64.1)
  %p64.2 = getelementptr i64, ptr @.v64, i64 4
  call void @bits64(ptr %p64.2)
  %p64.3 = getelementptr i64, ptr @.v64, i64 6
  call void @bits64(ptr %p64.3)
  %p64.4 = getelementptr i64, ptr @.v64, i64 8
  call void @bits64(ptr %p64.4)
  ret i32 0
}
