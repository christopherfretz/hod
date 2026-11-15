target triple = "i686-unknown-linux-gnu"
define i8 @r8(i8 %a, i8 signext %b, i8 zeroext %c) { %s = add i8 %a, %b
  %t = add i8 %s, %c
  ret i8 %t }
define signext i16 @r16s(i16 %a) { ret i16 %a }
define i64 @r64(i64 %a, i32 %b) { %c = zext i32 %b to i64
  %d = add i64 %a, %c
  ret i64 %d }
define i1 @r1(i1 %a) { ret i1 %a }
declare i64 @ext64(i8, i64, i16)
define i64 @call64() { %r = call i64 @ext64(i8 1, i64 2, i16 3)
  ret i64 %r }
define ptr @rp(ptr %p) { ret ptr %p }
