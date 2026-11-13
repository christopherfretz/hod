; Integers: six in rdi rsi rdx rcx r8 r9, then the stack in 8-byte slots;
; extension attributes; narrow returns.
declare void @sink8(i8 signext, i8 zeroext, i16 signext, i16 zeroext, i1 zeroext, i32, i8, i64)
define void @call_ints() {
  call void @sink8(i8 -1, i8 -1, i16 -1, i16 -1, i1 true, i32 -1, i8 -1, i64 -1)
  ret void
}
define i64 @recv(i64 %a, i64 %b, i64 %c, i64 %d, i64 %e, i64 %f, i8 %g, i32 %h) {
  %g64 = sext i8 %g to i64
  %h64 = sext i32 %h to i64
  %s = add i64 %g64, %h64
  ret i64 %s
}
define signext i8 @ret_s8(i32 %x) { %t = trunc i32 %x to i8
  ret i8 %t }
define zeroext i1 @ret_bool(i32 %x) { %t = trunc i32 %x to i1
  ret i1 %t }
define i1 @ret_i1_plain(i32 %x) { %t = trunc i32 %x to i1
  ret i1 %t }
define i16 @ret_i16_plain(i32 %x) { %t = trunc i32 %x to i16
  ret i16 %t }
declare i8 @get8()
define i32 @use_ret8() { %v = call i8 @get8()
  %w = zext i8 %v to i32
  ret i32 %w }
