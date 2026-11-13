; 128-bit integers: add, sub and mul with carries across the halves,
; division and remainder (library calls), shifts by amounts below, at and
; above 64, bitwise operations, compares, and conversions to and from
; narrower integers. Values are printed as two 64-bit halves.
;@ covers: spec/ir.md §5.4 integer, compare, cast (at i128)
;@ stdout: a=0123456789abcdef0123456789abcdef b=00000000000000010000000000000001 n=4
;@ stdout: add   0123456789abcdf00123456789abcdf0
;@ stdout: sub   0123456789abcdee0123456789abcdee
;@ stdout: mul   02468acf13579bde0123456789abcdef
;@ stdout: udiv  00000000000000000123456789abcdef
;@ stdout: urem  00000000000000000000000000000000
;@ stdout: sdiv  00000000000000000123456789abcdef
;@ stdout: srem  00000000000000000000000000000000
;@ stdout: shl   123456789abcdef0123456789abcdef0
;@ stdout: lshr  00123456789abcdef0123456789abcde
;@ stdout: ashr  00123456789abcdef0123456789abcde
;@ stdout: and   00000000000000010000000000000001
;@ stdout: or    0123456789abcdef0123456789abcdef
;@ stdout: xor   0123456789abcdee0123456789abcdee
;@ stdout: ult=0 slt=0 eq=0 ne=1 uge=1 sgt=1
;@ stdout: a=fedcba9876543210fedcba9876543211 b=0000000000000000000000003b9aca07 n=64
;@ stdout: add   fedcba9876543210fedcba98b1eefc18
;@ stdout: sub   fedcba9876543210fedcba983ab9680a
;@ stdout: mul   8641fdb9b4c4f75a8641fdb9b508c877
;@ stdout: udiv  00000004469ff98ff093d633ac545033
;@ stdout: urem  000000000000000000000000390fc2ac
;@ stdout: sdiv  fffffffffb1d00075b7f570fccb518b7
;@ stdout: srem  ffffffffffffffffffffffffecca1f10
;@ stdout: shl   fedcba98765432110000000000000000
;@ stdout: lshr  0000000000000000fedcba9876543210
;@ stdout: ashr  fffffffffffffffffedcba9876543210
;@ stdout: and   00000000000000000000000032100201
;@ stdout: or    fedcba9876543210fedcba987fdefa17
;@ stdout: xor   fedcba9876543210fedcba984dcef816
;@ stdout: ult=0 slt=1 eq=0 ne=1 uge=1 sgt=0
;@ stdout: a=fffffffffffffffffffffffffffffffe b=80000000000000000000000000000000 n=100
;@ stdout: add   7ffffffffffffffffffffffffffffffe
;@ stdout: sub   7ffffffffffffffffffffffffffffffe
;@ stdout: mul   00000000000000000000000000000000
;@ stdout: udiv  00000000000000000000000000000001
;@ stdout: urem  7ffffffffffffffffffffffffffffffe
;@ stdout: sdiv  00000000000000000000000000000000
;@ stdout: srem  fffffffffffffffffffffffffffffffe
;@ stdout: shl   ffffffe0000000000000000000000000
;@ stdout: lshr  0000000000000000000000000fffffff
;@ stdout: ashr  ffffffffffffffffffffffffffffffff
;@ stdout: and   80000000000000000000000000000000
;@ stdout: or    fffffffffffffffffffffffffffffffe
;@ stdout: xor   7ffffffffffffffffffffffffffffffe
;@ stdout: ult=0 slt=0 eq=0 ne=1 uge=1 sgt=1
;@ stdout: sext  fffffffffffffffffffffffffffffffb
;@ stdout: zext  0000000000000000fffffffffffffffb
;@ stdout: wide  fffffffffffffffe0000000000000001

@.show = private unnamed_addr constant [21 x i8] c"%-5s %016llx%016llx\0A\00"
@.cmp = private unnamed_addr constant [41 x i8] c"ult=%d slt=%d eq=%d ne=%d uge=%d sgt=%d\0A\00"
@.heading = private unnamed_addr constant [40 x i8] c"a=%016llx%016llx b=%016llx%016llx n=%d\0A\00"
@.add = private unnamed_addr constant [4 x i8] c"add\00"
@.sub = private unnamed_addr constant [4 x i8] c"sub\00"
@.mul = private unnamed_addr constant [4 x i8] c"mul\00"
@.udiv = private unnamed_addr constant [5 x i8] c"udiv\00"
@.urem = private unnamed_addr constant [5 x i8] c"urem\00"
@.sdiv = private unnamed_addr constant [5 x i8] c"sdiv\00"
@.srem = private unnamed_addr constant [5 x i8] c"srem\00"
@.shl = private unnamed_addr constant [4 x i8] c"shl\00"
@.lshr = private unnamed_addr constant [5 x i8] c"lshr\00"
@.ashr = private unnamed_addr constant [5 x i8] c"ashr\00"
@.and = private unnamed_addr constant [4 x i8] c"and\00"
@.or = private unnamed_addr constant [3 x i8] c"or\00"
@.xor = private unnamed_addr constant [4 x i8] c"xor\00"
@.sext = private unnamed_addr constant [5 x i8] c"sext\00"
@.zext = private unnamed_addr constant [5 x i8] c"zext\00"
@.square = private unnamed_addr constant [5 x i8] c"wide\00"

declare i32 @printf(ptr, ...)

define void @show(ptr %label, i128 %v) {
  %hi.w = lshr i128 %v, 64
  %hi = trunc i128 %hi.w to i64
  %lo = trunc i128 %v to i64
  call i32 (ptr, ...) @printf(ptr @.show, ptr %label, i64 %hi, i64 %lo)
  ret void
}

define void @ops(i128 %a, i128 %b, i32 %n) {
  %a.hi.w = lshr i128 %a, 64
  %a.hi = trunc i128 %a.hi.w to i64
  %a.lo = trunc i128 %a to i64
  %b.hi.w = lshr i128 %b, 64
  %b.hi = trunc i128 %b.hi.w to i64
  %b.lo = trunc i128 %b to i64
  call i32 (ptr, ...) @printf(ptr @.heading, i64 %a.hi, i64 %a.lo, i64 %b.hi, i64 %b.lo, i32 %n)
  %add = add i128 %a, %b
  call void @show(ptr @.add, i128 %add)
  %sub = sub i128 %a, %b
  call void @show(ptr @.sub, i128 %sub)
  %mul = mul i128 %a, %b
  call void @show(ptr @.mul, i128 %mul)
  %udiv = udiv i128 %a, %b
  call void @show(ptr @.udiv, i128 %udiv)
  %urem = urem i128 %a, %b
  call void @show(ptr @.urem, i128 %urem)
  %sdiv = sdiv i128 %a, %b
  call void @show(ptr @.sdiv, i128 %sdiv)
  %srem = srem i128 %a, %b
  call void @show(ptr @.srem, i128 %srem)
  %n.w = zext i32 %n to i128
  %shl = shl i128 %a, %n.w
  call void @show(ptr @.shl, i128 %shl)
  %lshr = lshr i128 %a, %n.w
  call void @show(ptr @.lshr, i128 %lshr)
  %ashr = ashr i128 %a, %n.w
  call void @show(ptr @.ashr, i128 %ashr)
  %and = and i128 %a, %b
  call void @show(ptr @.and, i128 %and)
  %or = or i128 %a, %b
  call void @show(ptr @.or, i128 %or)
  %xor = xor i128 %a, %b
  call void @show(ptr @.xor, i128 %xor)
  %ult = icmp ult i128 %a, %b
  %slt = icmp slt i128 %a, %b
  %eq = icmp eq i128 %a, %b
  %ne = icmp ne i128 %a, %b
  %uge = icmp uge i128 %a, %b
  %sgt = icmp sgt i128 %a, %b
  %ult.i = zext i1 %ult to i32
  %slt.i = zext i1 %slt to i32
  %eq.i = zext i1 %eq to i32
  %ne.i = zext i1 %ne to i32
  %uge.i = zext i1 %uge to i32
  %sgt.i = zext i1 %sgt to i32
  call i32 (ptr, ...) @printf(ptr @.cmp, i32 %ult.i, i32 %slt.i, i32 %eq.i, i32 %ne.i, i32 %uge.i, i32 %sgt.i)
  ret void
}

; The full 128-bit product of two 64-bit values.
define i128 @widening_mul(i64 %x, i64 %y) {
  %x.w = zext i64 %x to i128
  %y.w = zext i64 %y to i128
  %p = mul i128 %x.w, %y.w
  ret i128 %p
}

define i32 @main() {
  call void @ops(i128 1512366075204170929049582354406559215, i128 18446744073709551617, i32 4)
  call void @ops(i128 -1512366075204170929049582354406559215, i128 1000000007, i32 64)
  call void @ops(i128 -2, i128 -170141183460469231731687303715884105728, i32 100)
  %s = sext i64 -5 to i128
  call void @show(ptr @.sext, i128 %s)
  %z = zext i64 -5 to i128
  call void @show(ptr @.zext, i128 %z)
  %w = call i128 @widening_mul(i64 -1, i64 -1)
  call void @show(ptr @.square, i128 %w)
  ret i32 0
}
