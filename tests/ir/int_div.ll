; Integer division and remainder, signed and unsigned, at i8, i16, i32 and
; i64, with negative operands (truncation toward zero; the remainder takes
; the dividend's sign).
;@ covers: spec/ir.md §5.4 integer
;@ unsupported(x86_64): *
;@ stdout: i8 udiv=124 sdiv=-3 urem=1 srem=-1
;@ stdout: i8 udiv=0 sdiv=-11 urem=100 srem=1
;@ stdout: i16 udiv=32764 sdiv=-3 urem=1 srem=-1
;@ stdout: i16 udiv=0 sdiv=-11 urem=100 srem=1
;@ stdout: i32 udiv=2147483644 sdiv=-3 urem=1 srem=-1
;@ stdout: i32 udiv=0 sdiv=-11 urem=100 srem=1
;@ stdout: i32 udiv=715827882 sdiv=-715827882 urem=2 srem=-2
;@ stdout: i64 udiv=9223372036854775804 sdiv=-3 urem=1 srem=-1
;@ stdout: i64 udiv=0 sdiv=-11 urem=100 srem=1
;@ stdout: i64 udiv=142857142857142857 sdiv=142857142857142857 urem=1 srem=1

@.fmt = private unnamed_addr constant [44 x i8] c"%s udiv=%llu sdiv=%lld urem=%llu srem=%lld\0A\00"
@.i8 = private unnamed_addr constant [3 x i8] c"i8\00"
@.i16 = private unnamed_addr constant [4 x i8] c"i16\00"
@.i32 = private unnamed_addr constant [4 x i8] c"i32\00"
@.i64 = private unnamed_addr constant [4 x i8] c"i64\00"

declare i32 @printf(ptr, ...)

define void @div8(i8 %a, i8 %b) {
  %ud = udiv i8 %a, %b
  %sd = sdiv i8 %a, %b
  %ur = urem i8 %a, %b
  %sr = srem i8 %a, %b
  %x1 = zext i8 %ud to i64
  %x2 = sext i8 %sd to i64
  %x3 = zext i8 %ur to i64
  %x4 = sext i8 %sr to i64
  call i32 (ptr, ...) @printf(ptr @.fmt, ptr @.i8, i64 %x1, i64 %x2, i64 %x3, i64 %x4)
  ret void
}

define void @div16(i16 %a, i16 %b) {
  %ud = udiv i16 %a, %b
  %sd = sdiv i16 %a, %b
  %ur = urem i16 %a, %b
  %sr = srem i16 %a, %b
  %x1 = zext i16 %ud to i64
  %x2 = sext i16 %sd to i64
  %x3 = zext i16 %ur to i64
  %x4 = sext i16 %sr to i64
  call i32 (ptr, ...) @printf(ptr @.fmt, ptr @.i16, i64 %x1, i64 %x2, i64 %x3, i64 %x4)
  ret void
}

define void @div32(i32 %a, i32 %b) {
  %ud = udiv i32 %a, %b
  %sd = sdiv i32 %a, %b
  %ur = urem i32 %a, %b
  %sr = srem i32 %a, %b
  %x1 = zext i32 %ud to i64
  %x2 = sext i32 %sd to i64
  %x3 = zext i32 %ur to i64
  %x4 = sext i32 %sr to i64
  call i32 (ptr, ...) @printf(ptr @.fmt, ptr @.i32, i64 %x1, i64 %x2, i64 %x3, i64 %x4)
  ret void
}

define void @div64(i64 %a, i64 %b) {
  %ud = udiv i64 %a, %b
  %sd = sdiv i64 %a, %b
  %ur = urem i64 %a, %b
  %sr = srem i64 %a, %b
  call i32 (ptr, ...) @printf(ptr @.fmt, ptr @.i64, i64 %ud, i64 %sd, i64 %ur, i64 %sr)
  ret void
}

define i32 @main() {
  call void @div8(i8 -7, i8 2)
  call void @div8(i8 100, i8 -9)
  call void @div16(i16 -7, i16 2)
  call void @div16(i16 100, i16 -9)
  call void @div32(i32 -7, i32 2)
  call void @div32(i32 100, i32 -9)
  call void @div32(i32 -2147483648, i32 3)
  call void @div64(i64 -7, i64 2)
  call void @div64(i64 100, i64 -9)
  call void @div64(i64 1000000000000000000, i64 7)
  ret i32 0
}
