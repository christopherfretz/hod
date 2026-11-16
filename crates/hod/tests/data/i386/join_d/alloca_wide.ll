; Dynamic `alloca`s counted by integers of 33 to 64 bits (i386 join D,
; `spec/codegen-x86.md` §12.3): `llc` truncates the count to 32 bits, so
; counts above 2^32 allocate their low word's worth.
target datalayout = "e-m:e-p:32:32-p270:32:32-p271:32:32-p272:64:64-i128:128-f64:32:64-f80:32-n8:16:32-S128"
target triple = "i686-unknown-linux-gnu"
@fmt = private constant [16 x i8] c"%d %d %d %d %d\0A\00"
declare i32 @printf(ptr, ...)
define i32 @f64(i64 %n, i40 %m, i33 %k) noinline {
  %p = alloca i32, i64 %n, align 64
  %q = alloca i16, i40 %m, align 4
  %r = alloca i8, i33 %k, align 16
  store i32 7, ptr %p
  %pe = getelementptr i32, ptr %p, i32 3
  store i32 11, ptr %pe
  store i16 5, ptr %q
  store i8 3, ptr %r
  %ai = ptrtoint ptr %p to i32
  %al = and i32 %ai, 63
  %qi = ptrtoint ptr %q to i32
  %ri = ptrtoint ptr %r to i32
  %d1 = sub i32 %qi, %ai
  %d2 = sub i32 %ri, %qi
  %a = load i32, ptr %pe
  %b = load i16, ptr %q
  %bw = zext i16 %b to i32
  %s = add i32 %a, %bw
  call i32 (ptr, ...) @printf(ptr @fmt, i32 %al, i32 %s, i32 %d1, i32 %d2, i32 0)
  ret i32 %s
}
define i32 @main() {
  %a = call i32 @f64(i64 4, i40 3, i33 5)
  %b = call i32 @f64(i64 4294967300, i40 1099511627777, i33 4294967297)
  %c = call i32 @f64(i64 17, i40 1, i33 33)
  ret i32 0
}
