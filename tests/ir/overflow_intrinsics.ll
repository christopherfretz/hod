; The arithmetic-with-overflow intrinsics: signed and unsigned add, sub and
; mul at 8, 32 and 64 bits, with operands on both sides of every overflow
; boundary, and a branch on the overflow bit.
;@ covers: spec/ir.md §5.5 intrinsics (sadd, uadd, ssub, usub, smul, umul .with.overflow)
;@ unsupported: codegen-not-implemented
;@ stdout: i8 100,27: sadd 127/0 uadd 127/0 ssub 73/0 usub 73/0 smul -116/1 umul 140/1
;@ stdout: i8 100,28: sadd -128/1 uadd 128/0 ssub 72/0 usub 72/0 smul -16/1 umul 240/1
;@ stdout: i8 -128,-1: sadd 127/1 uadd 127/1 ssub -127/0 usub 129/1 smul -128/1 umul 128/1
;@ stdout: i8 -56,56: sadd 0/0 uadd 0/1 ssub -112/0 usub 144/0 smul -64/1 umul 192/1
;@ stdout: i8 16,16: sadd 32/0 uadd 32/0 ssub 0/0 usub 0/0 smul 0/1 umul 0/1
;@ stdout: i8 -16,8: sadd -8/0 uadd 248/0 ssub -24/0 usub 232/0 smul -128/0 umul 128/1
;@ stdout: i8 0,1: sadd 1/0 uadd 1/0 ssub -1/0 usub 255/1 smul 0/0 umul 0/0
;@ stdout: i32 2147483647,1: sadd -2147483648/1 uadd 2147483648/0 ssub 2147483646/0 usub 2147483646/0 smul 2147483647/0 umul 2147483647/0
;@ stdout: i32 -2147483648,-1: sadd 2147483647/1 uadd 2147483647/1 ssub -2147483647/0 usub 2147483649/1 smul -2147483648/1 umul 2147483648/1
;@ stdout: i32 65536,65536: sadd 131072/0 uadd 131072/0 ssub 0/0 usub 0/0 smul 0/1 umul 0/1
;@ stdout: i32 46341,46341: sadd 92682/0 uadd 92682/0 ssub 0/0 usub 0/0 smul -2147479015/1 umul 2147488281/0
;@ stdout: i32 -46341,46341: sadd 0/0 uadd 0/1 ssub -92682/0 usub 4294874614/0 smul 2147479015/1 umul 2147479015/1
;@ stdout: i32 5,7: sadd 12/0 uadd 12/0 ssub -2/0 usub 4294967294/1 smul 35/0 umul 35/0
;@ stdout: i32 0,1: sadd 1/0 uadd 1/0 ssub -1/0 usub 4294967295/1 smul 0/0 umul 0/0
;@ stdout: i64 9223372036854775807,1: sadd -9223372036854775808/1 uadd 9223372036854775808/0 ssub 9223372036854775806/0 usub 9223372036854775806/0 smul 9223372036854775807/0 umul 9223372036854775807/0
;@ stdout: i64 -9223372036854775808,-1: sadd 9223372036854775807/1 uadd 9223372036854775807/1 ssub -9223372036854775807/0 usub 9223372036854775809/1 smul -9223372036854775808/1 umul 9223372036854775808/1
;@ stdout: i64 4294967296,4294967296: sadd 8589934592/0 uadd 8589934592/0 ssub 0/0 usub 0/0 smul 0/1 umul 0/1
;@ stdout: i64 3037000500,3037000500: sadd 6074001000/0 uadd 6074001000/0 ssub 0/0 usub 0/0 smul -9223372036709301616/1 umul 9223372037000250000/0
;@ stdout: i64 -3037000500,3037000500: sadd 0/0 uadd 0/1 ssub -6074001000/0 usub 18446744067635550616/0 smul 9223372036709301616/1 umul 9223372036709301616/1
;@ stdout: i64 5,7: sadd 12/0 uadd 12/0 ssub -2/0 usub 18446744073709551614/1 smul 35/0 umul 35/0
;@ stdout: i64 0,1: sadd 1/0 uadd 1/0 ssub -1/0 usub 18446744073709551615/1 smul 0/0 umul 0/0
;@ stdout: checked sum of 100 terms: 5050000, overflowed 0
;@ stdout: checked sum of 65 terms: 2145000000, overflowed 1

@.row = private unnamed_addr constant [94 x i8] c"i%d %lld,%lld: sadd %lld/%d uadd %llu/%d ssub %lld/%d usub %llu/%d smul %lld/%d umul %llu/%d\0A\00"
@.checked = private unnamed_addr constant [44 x i8] c"checked sum of %d terms: %d, overflowed %d\0A\00"

declare i32 @printf(ptr, ...)
declare { i8, i1 } @llvm.sadd.with.overflow.i8(i8, i8)
declare { i8, i1 } @llvm.uadd.with.overflow.i8(i8, i8)
declare { i8, i1 } @llvm.ssub.with.overflow.i8(i8, i8)
declare { i8, i1 } @llvm.usub.with.overflow.i8(i8, i8)
declare { i8, i1 } @llvm.smul.with.overflow.i8(i8, i8)
declare { i8, i1 } @llvm.umul.with.overflow.i8(i8, i8)
declare { i32, i1 } @llvm.sadd.with.overflow.i32(i32, i32)
declare { i32, i1 } @llvm.uadd.with.overflow.i32(i32, i32)
declare { i32, i1 } @llvm.ssub.with.overflow.i32(i32, i32)
declare { i32, i1 } @llvm.usub.with.overflow.i32(i32, i32)
declare { i32, i1 } @llvm.smul.with.overflow.i32(i32, i32)
declare { i32, i1 } @llvm.umul.with.overflow.i32(i32, i32)
declare { i64, i1 } @llvm.sadd.with.overflow.i64(i64, i64)
declare { i64, i1 } @llvm.uadd.with.overflow.i64(i64, i64)
declare { i64, i1 } @llvm.ssub.with.overflow.i64(i64, i64)
declare { i64, i1 } @llvm.usub.with.overflow.i64(i64, i64)
declare { i64, i1 } @llvm.smul.with.overflow.i64(i64, i64)
declare { i64, i1 } @llvm.umul.with.overflow.i64(i64, i64)

define void @row8(i8 %a, i8 %b) {
  %sadd = call { i8, i1 } @llvm.sadd.with.overflow.i8(i8 %a, i8 %b)
  %sadd.v = extractvalue { i8, i1 } %sadd, 0
  %sadd.o = extractvalue { i8, i1 } %sadd, 1
  %sadd.w = sext i8 %sadd.v to i64
  %sadd.f = zext i1 %sadd.o to i32
  %uadd = call { i8, i1 } @llvm.uadd.with.overflow.i8(i8 %a, i8 %b)
  %uadd.v = extractvalue { i8, i1 } %uadd, 0
  %uadd.o = extractvalue { i8, i1 } %uadd, 1
  %uadd.w = zext i8 %uadd.v to i64
  %uadd.f = zext i1 %uadd.o to i32
  %ssub = call { i8, i1 } @llvm.ssub.with.overflow.i8(i8 %a, i8 %b)
  %ssub.v = extractvalue { i8, i1 } %ssub, 0
  %ssub.o = extractvalue { i8, i1 } %ssub, 1
  %ssub.w = sext i8 %ssub.v to i64
  %ssub.f = zext i1 %ssub.o to i32
  %usub = call { i8, i1 } @llvm.usub.with.overflow.i8(i8 %a, i8 %b)
  %usub.v = extractvalue { i8, i1 } %usub, 0
  %usub.o = extractvalue { i8, i1 } %usub, 1
  %usub.w = zext i8 %usub.v to i64
  %usub.f = zext i1 %usub.o to i32
  %smul = call { i8, i1 } @llvm.smul.with.overflow.i8(i8 %a, i8 %b)
  %smul.v = extractvalue { i8, i1 } %smul, 0
  %smul.o = extractvalue { i8, i1 } %smul, 1
  %smul.w = sext i8 %smul.v to i64
  %smul.f = zext i1 %smul.o to i32
  %umul = call { i8, i1 } @llvm.umul.with.overflow.i8(i8 %a, i8 %b)
  %umul.v = extractvalue { i8, i1 } %umul, 0
  %umul.o = extractvalue { i8, i1 } %umul, 1
  %umul.w = zext i8 %umul.v to i64
  %umul.f = zext i1 %umul.o to i32
  %a.w = sext i8 %a to i64
  %b.w = sext i8 %b to i64
  call i32 (ptr, ...) @printf(ptr @.row, i32 8, i64 %a.w, i64 %b.w, i64 %sadd.w, i32 %sadd.f, i64 %uadd.w, i32 %uadd.f, i64 %ssub.w, i32 %ssub.f, i64 %usub.w, i32 %usub.f, i64 %smul.w, i32 %smul.f, i64 %umul.w, i32 %umul.f)
  ret void
}

define void @row32(i32 %a, i32 %b) {
  %sadd = call { i32, i1 } @llvm.sadd.with.overflow.i32(i32 %a, i32 %b)
  %sadd.v = extractvalue { i32, i1 } %sadd, 0
  %sadd.o = extractvalue { i32, i1 } %sadd, 1
  %sadd.w = sext i32 %sadd.v to i64
  %sadd.f = zext i1 %sadd.o to i32
  %uadd = call { i32, i1 } @llvm.uadd.with.overflow.i32(i32 %a, i32 %b)
  %uadd.v = extractvalue { i32, i1 } %uadd, 0
  %uadd.o = extractvalue { i32, i1 } %uadd, 1
  %uadd.w = zext i32 %uadd.v to i64
  %uadd.f = zext i1 %uadd.o to i32
  %ssub = call { i32, i1 } @llvm.ssub.with.overflow.i32(i32 %a, i32 %b)
  %ssub.v = extractvalue { i32, i1 } %ssub, 0
  %ssub.o = extractvalue { i32, i1 } %ssub, 1
  %ssub.w = sext i32 %ssub.v to i64
  %ssub.f = zext i1 %ssub.o to i32
  %usub = call { i32, i1 } @llvm.usub.with.overflow.i32(i32 %a, i32 %b)
  %usub.v = extractvalue { i32, i1 } %usub, 0
  %usub.o = extractvalue { i32, i1 } %usub, 1
  %usub.w = zext i32 %usub.v to i64
  %usub.f = zext i1 %usub.o to i32
  %smul = call { i32, i1 } @llvm.smul.with.overflow.i32(i32 %a, i32 %b)
  %smul.v = extractvalue { i32, i1 } %smul, 0
  %smul.o = extractvalue { i32, i1 } %smul, 1
  %smul.w = sext i32 %smul.v to i64
  %smul.f = zext i1 %smul.o to i32
  %umul = call { i32, i1 } @llvm.umul.with.overflow.i32(i32 %a, i32 %b)
  %umul.v = extractvalue { i32, i1 } %umul, 0
  %umul.o = extractvalue { i32, i1 } %umul, 1
  %umul.w = zext i32 %umul.v to i64
  %umul.f = zext i1 %umul.o to i32
  %a.w = sext i32 %a to i64
  %b.w = sext i32 %b to i64
  call i32 (ptr, ...) @printf(ptr @.row, i32 32, i64 %a.w, i64 %b.w, i64 %sadd.w, i32 %sadd.f, i64 %uadd.w, i32 %uadd.f, i64 %ssub.w, i32 %ssub.f, i64 %usub.w, i32 %usub.f, i64 %smul.w, i32 %smul.f, i64 %umul.w, i32 %umul.f)
  ret void
}

define void @row64(i64 %a, i64 %b) {
  %sadd = call { i64, i1 } @llvm.sadd.with.overflow.i64(i64 %a, i64 %b)
  %sadd.v = extractvalue { i64, i1 } %sadd, 0
  %sadd.o = extractvalue { i64, i1 } %sadd, 1
  %sadd.f = zext i1 %sadd.o to i32
  %uadd = call { i64, i1 } @llvm.uadd.with.overflow.i64(i64 %a, i64 %b)
  %uadd.v = extractvalue { i64, i1 } %uadd, 0
  %uadd.o = extractvalue { i64, i1 } %uadd, 1
  %uadd.f = zext i1 %uadd.o to i32
  %ssub = call { i64, i1 } @llvm.ssub.with.overflow.i64(i64 %a, i64 %b)
  %ssub.v = extractvalue { i64, i1 } %ssub, 0
  %ssub.o = extractvalue { i64, i1 } %ssub, 1
  %ssub.f = zext i1 %ssub.o to i32
  %usub = call { i64, i1 } @llvm.usub.with.overflow.i64(i64 %a, i64 %b)
  %usub.v = extractvalue { i64, i1 } %usub, 0
  %usub.o = extractvalue { i64, i1 } %usub, 1
  %usub.f = zext i1 %usub.o to i32
  %smul = call { i64, i1 } @llvm.smul.with.overflow.i64(i64 %a, i64 %b)
  %smul.v = extractvalue { i64, i1 } %smul, 0
  %smul.o = extractvalue { i64, i1 } %smul, 1
  %smul.f = zext i1 %smul.o to i32
  %umul = call { i64, i1 } @llvm.umul.with.overflow.i64(i64 %a, i64 %b)
  %umul.v = extractvalue { i64, i1 } %umul, 0
  %umul.o = extractvalue { i64, i1 } %umul, 1
  %umul.f = zext i1 %umul.o to i32
  call i32 (ptr, ...) @printf(ptr @.row, i32 64, i64 %a, i64 %b, i64 %sadd.v, i32 %sadd.f, i64 %uadd.v, i32 %uadd.f, i64 %ssub.v, i32 %ssub.f, i64 %usub.v, i32 %usub.f, i64 %smul.v, i32 %smul.f, i64 %umul.v, i32 %umul.f)
  ret void
}

; Adds step, 2*step, 3*step, ... and stops at the first signed overflow.
define void @checked_sum(i32 %step, i32 %limit) {
entry:
  br label %loop
loop:
  %i = phi i32 [ 1, %entry ], [ %i.next, %continue ]
  %acc = phi i32 [ 0, %entry ], [ %sum, %continue ]
  %term = mul i32 %i, %step
  %r = call { i32, i1 } @llvm.sadd.with.overflow.i32(i32 %acc, i32 %term)
  %sum = extractvalue { i32, i1 } %r, 0
  %over = extractvalue { i32, i1 } %r, 1
  br i1 %over, label %overflowed, label %continue
continue:
  %i.next = add i32 %i, 1
  %more = icmp ule i32 %i.next, %limit
  br i1 %more, label %loop, label %finished
overflowed:
  %terms = sub i32 %i, 1
  call i32 (ptr, ...) @printf(ptr @.checked, i32 %terms, i32 %acc, i32 1)
  ret void
finished:
  call i32 (ptr, ...) @printf(ptr @.checked, i32 %limit, i32 %sum, i32 0)
  ret void
}

define i32 @main() {
  call void @row8(i8 100, i8 27)
  call void @row8(i8 100, i8 28)
  call void @row8(i8 -128, i8 -1)
  call void @row8(i8 -56, i8 56)
  call void @row8(i8 16, i8 16)
  call void @row8(i8 -16, i8 8)
  call void @row8(i8 0, i8 1)
  call void @row32(i32 2147483647, i32 1)
  call void @row32(i32 -2147483648, i32 -1)
  call void @row32(i32 65536, i32 65536)
  call void @row32(i32 46341, i32 46341)
  call void @row32(i32 -46341, i32 46341)
  call void @row32(i32 5, i32 7)
  call void @row32(i32 0, i32 1)
  call void @row64(i64 9223372036854775807, i64 1)
  call void @row64(i64 -9223372036854775808, i64 -1)
  call void @row64(i64 4294967296, i64 4294967296)
  call void @row64(i64 3037000500, i64 3037000500)
  call void @row64(i64 -3037000500, i64 3037000500)
  call void @row64(i64 5, i64 7)
  call void @row64(i64 0, i64 1)
  call void @checked_sum(i32 1000, i32 100)
  call void @checked_sum(i32 1000000, i32 100)
  ret i32 0
}
