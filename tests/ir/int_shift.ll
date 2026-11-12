; shl, lshr and ashr at i8, i16, i32 and i64, with shift amounts that are
; only known at run time, including 0 and width-1.
;@ covers: spec/ir.md §5.4 integer
;@ unsupported(x86_64): *
;@ stdout: i8 by 3: shl=-80 lshr=18 ashr=-14
;@ stdout: i8 by 0: shl=-106 lshr=150 ashr=-106
;@ stdout: i8 by 7: shl=0 lshr=1 ashr=-1
;@ stdout: i16 by 5: shl=-1824 lshr=1662 ashr=-386
;@ stdout: i16 by 15: shl=-32768 lshr=0 ashr=0
;@ stdout: i32 by 9: shl=1534975488 lshr=7296735 ashr=-1091873
;@ stdout: i32 by 31: shl=-2147483648 lshr=1 ashr=-1
;@ stdout: i64 by 12: shl=-3771334343958396928 lshr=4483583629026627 ashr=-20015998343869
;@ stdout: i64 by 63: shl=-9223372036854775808 lshr=0 ashr=0
;@ stdout: i64 by 63: shl=0 lshr=1 ashr=-1

@.fmt = private unnamed_addr constant [40 x i8] c"%s by %d: shl=%lld lshr=%llu ashr=%lld\0A\00"
@.i8 = private unnamed_addr constant [3 x i8] c"i8\00"
@.i16 = private unnamed_addr constant [4 x i8] c"i16\00"
@.i32 = private unnamed_addr constant [4 x i8] c"i32\00"
@.i64 = private unnamed_addr constant [4 x i8] c"i64\00"

declare i32 @printf(ptr, ...)

define void @shift8(i8 %a, i8 %n) {
  %shl = shl i8 %a, %n
  %lshr = lshr i8 %a, %n
  %ashr = ashr i8 %a, %n
  %x0 = zext i8 %n to i32
  %x1 = sext i8 %shl to i64
  %x2 = zext i8 %lshr to i64
  %x3 = sext i8 %ashr to i64
  call i32 (ptr, ...) @printf(ptr @.fmt, ptr @.i8, i32 %x0, i64 %x1, i64 %x2, i64 %x3)
  ret void
}

define void @shift16(i16 %a, i16 %n) {
  %shl = shl i16 %a, %n
  %lshr = lshr i16 %a, %n
  %ashr = ashr i16 %a, %n
  %x0 = zext i16 %n to i32
  %x1 = sext i16 %shl to i64
  %x2 = zext i16 %lshr to i64
  %x3 = sext i16 %ashr to i64
  call i32 (ptr, ...) @printf(ptr @.fmt, ptr @.i16, i32 %x0, i64 %x1, i64 %x2, i64 %x3)
  ret void
}

define void @shift32(i32 %a, i32 %n) {
  %shl = shl i32 %a, %n
  %lshr = lshr i32 %a, %n
  %ashr = ashr i32 %a, %n
  %x1 = sext i32 %shl to i64
  %x2 = zext i32 %lshr to i64
  %x3 = sext i32 %ashr to i64
  call i32 (ptr, ...) @printf(ptr @.fmt, ptr @.i32, i32 %n, i64 %x1, i64 %x2, i64 %x3)
  ret void
}

define void @shift64(i64 %a, i64 %n) {
  %shl = shl i64 %a, %n
  %lshr = lshr i64 %a, %n
  %ashr = ashr i64 %a, %n
  %x0 = trunc i64 %n to i32
  call i32 (ptr, ...) @printf(ptr @.fmt, ptr @.i64, i32 %x0, i64 %shl, i64 %lshr, i64 %ashr)
  ret void
}

define i32 @main() {
  call void @shift8(i8 -106, i8 3)
  call void @shift8(i8 -106, i8 0)
  call void @shift8(i8 -106, i8 7)
  call void @shift16(i16 -12345, i16 5)
  call void @shift16(i16 1, i16 15)
  call void @shift32(i32 -559038737, i32 9)
  call void @shift32(i32 -559038737, i32 31)
  call void @shift64(i64 -81985529216486896, i64 12)
  call void @shift64(i64 1, i64 63)
  call void @shift64(i64 -81985529216486896, i64 63)
  ret i32 0
}
