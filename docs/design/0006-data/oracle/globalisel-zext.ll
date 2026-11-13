; spec/interp.md §10.1, GlobalISel's AArch64 miscompile, run on x86-64.
; Correct answer: 60128 & 0xff = 224 as exit status (60128 = 0xEAE0).
@g = global i16 -5408
define i64 @f() noinline {
  %a = load i16, ptr @g
  %b = zext i16 %a to i32
  %c = zext i32 %b to i33
  %d = sext i32 %b to i64
  %e = zext i33 %c to i64
  %r = or i64 %d, %e
  ret i64 %r
}
define i32 @main() {
  %r = call i64 @f()
  %t = trunc i64 %r to i32
  ret i32 %t
}
