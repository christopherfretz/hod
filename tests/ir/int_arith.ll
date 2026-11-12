; Integer add, sub, mul, and, or, xor at i8, i16, i32 and i64, with
; operands that wrap at each width. The operands arrive as arguments, so
; nothing is folded.
;@ covers: spec/ir.md §5.4 integer
;@ tier: quick
;@ unsupported: *
;@ stdout: i8 add=127 sub=73 mul=-116 and=0 or=127 xor=127
;@ stdout: i8 add=-127 sub=127 mul=-128 and=0 or=-127 xor=-127
;@ stdout: i16 add=31000 sub=29000 mul=-15488 and=288 or=30712 xor=30424
;@ stdout: i16 add=1 sub=-1 mul=0 and=0 or=1 xor=1
;@ stdout: i16 add=-1 sub=1 mul=-32768 and=0 or=-1 xor=-1
;@ stdout: i32 add=-2147483648 sub=2147483646 mul=2147483647 and=1 or=2147483647 xor=2147483646
;@ stdout: i32 add=-294967296 sub=0 mul=-1651507200 and=2000000000 or=2000000000 xor=0
;@ stdout: i64 add=-9223372036854775808 sub=9223372036854775806 mul=9223372036854775807 and=1 or=9223372036854775807 xor=9223372036854775806
;@ stdout: i64 add=1111111110 sub=-864197532 mul=121932631112635269 and=39471121 or=1071639989 xor=1032168868

@.fmt = private unnamed_addr constant [57 x i8] c"%s add=%lld sub=%lld mul=%lld and=%lld or=%lld xor=%lld\0A\00"
@.i8 = private unnamed_addr constant [3 x i8] c"i8\00"
@.i16 = private unnamed_addr constant [4 x i8] c"i16\00"
@.i32 = private unnamed_addr constant [4 x i8] c"i32\00"
@.i64 = private unnamed_addr constant [4 x i8] c"i64\00"

declare i32 @printf(ptr, ...)

define void @arith8(i8 %a, i8 %b) {
  %add = add i8 %a, %b
  %sub = sub i8 %a, %b
  %mul = mul i8 %a, %b
  %and = and i8 %a, %b
  %or = or i8 %a, %b
  %xor = xor i8 %a, %b
  %x1 = sext i8 %add to i64
  %x2 = sext i8 %sub to i64
  %x3 = sext i8 %mul to i64
  %x4 = sext i8 %and to i64
  %x5 = sext i8 %or to i64
  %x6 = sext i8 %xor to i64
  call i32 (ptr, ...) @printf(ptr @.fmt, ptr @.i8, i64 %x1, i64 %x2, i64 %x3, i64 %x4, i64 %x5, i64 %x6)
  ret void
}

define void @arith16(i16 %a, i16 %b) {
  %add = add i16 %a, %b
  %sub = sub i16 %a, %b
  %mul = mul i16 %a, %b
  %and = and i16 %a, %b
  %or = or i16 %a, %b
  %xor = xor i16 %a, %b
  %x1 = sext i16 %add to i64
  %x2 = sext i16 %sub to i64
  %x3 = sext i16 %mul to i64
  %x4 = sext i16 %and to i64
  %x5 = sext i16 %or to i64
  %x6 = sext i16 %xor to i64
  call i32 (ptr, ...) @printf(ptr @.fmt, ptr @.i16, i64 %x1, i64 %x2, i64 %x3, i64 %x4, i64 %x5, i64 %x6)
  ret void
}

define void @arith32(i32 %a, i32 %b) {
  %add = add i32 %a, %b
  %sub = sub i32 %a, %b
  %mul = mul i32 %a, %b
  %and = and i32 %a, %b
  %or = or i32 %a, %b
  %xor = xor i32 %a, %b
  %x1 = sext i32 %add to i64
  %x2 = sext i32 %sub to i64
  %x3 = sext i32 %mul to i64
  %x4 = sext i32 %and to i64
  %x5 = sext i32 %or to i64
  %x6 = sext i32 %xor to i64
  call i32 (ptr, ...) @printf(ptr @.fmt, ptr @.i32, i64 %x1, i64 %x2, i64 %x3, i64 %x4, i64 %x5, i64 %x6)
  ret void
}

define void @arith64(i64 %a, i64 %b) {
  %add = add i64 %a, %b
  %sub = sub i64 %a, %b
  %mul = mul i64 %a, %b
  %and = and i64 %a, %b
  %or = or i64 %a, %b
  %xor = xor i64 %a, %b
  call i32 (ptr, ...) @printf(ptr @.fmt, ptr @.i64, i64 %add, i64 %sub, i64 %mul, i64 %and, i64 %or, i64 %xor)
  ret void
}

define i32 @main() {
  call void @arith8(i8 100, i8 27)
  call void @arith8(i8 -128, i8 1)
  call void @arith16(i16 30000, i16 1000)
  call void @arith16(i16 0, i16 1)
  call void @arith16(i16 -32768, i16 32767)
  call void @arith32(i32 2147483647, i32 1)
  call void @arith32(i32 2000000000, i32 2000000000)
  call void @arith64(i64 9223372036854775807, i64 1)
  call void @arith64(i64 123456789, i64 987654321)
  ret i32 0
}
