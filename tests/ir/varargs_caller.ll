; Calls to variadic C functions: more integer arguments than there are
; argument registers on either target, floating-point arguments mixed in,
; more floating-point arguments than vector registers, small integers
; (already promoted to i32, as C requires), pointers, and a call that
; writes into a buffer.
;@ covers: spec/ir.md §5.4 call (Call with a variadic function type)
;@ unsupported: codegen-not-implemented
;@ stdout: ints: 1 2 3 4 5 6 7 8 9 10 11 12
;@ stdout: mixed: -1 2.5 text -4000000000 0.25 z 4294967295 1000.000
;@ stdout: fps: 1.0 2.0 3.0 4.0 5.0 6.0 7.0 8.0 9.0 10.0
;@ stdout: wide: -9223372036854775808 18446744073709551615 deadbeef (nil)
;@ stdout: small: -7 -300 1
;@ stdout: snprintf wrote 21: [00042|text|+1.2e+04]

@.ints = private unnamed_addr constant [43 x i8] c"ints: %d %d %d %d %d %d %d %d %d %d %d %d\0A\00"
@.mixed = private unnamed_addr constant [40 x i8] c"mixed: %d %.1f %s %lld %.2f %c %u %.3f\0A\00"
@.fps = private unnamed_addr constant [56 x i8] c"fps: %.1f %.1f %.1f %.1f %.1f %.1f %.1f %.1f %.1f %.1f\0A\00"
@.wide = private unnamed_addr constant [24 x i8] c"wide: %lld %llu %lx %p\0A\00"
@.small = private unnamed_addr constant [17 x i8] c"small: %d %d %d\0A\00"
@.str = private unnamed_addr constant [5 x i8] c"text\00"
@.into = private unnamed_addr constant [18 x i8] c"[%05d|%-4s|%+.1e]\00"
@.out = private unnamed_addr constant [23 x i8] c"snprintf wrote %d: %s\0A\00"

declare i32 @printf(ptr, ...)
declare i32 @snprintf(ptr, i64, ptr, ...)

define void @small(i8 %a, i16 %b, i1 %c) {
  %a32 = sext i8 %a to i32
  %b32 = sext i16 %b to i32
  %c32 = zext i1 %c to i32
  call i32 (ptr, ...) @printf(ptr @.small, i32 %a32, i32 %b32, i32 %c32)
  ret void
}

define i32 @main() {
  %buf = alloca [64 x i8]
  call i32 (ptr, ...) @printf(ptr @.ints, i32 1, i32 2, i32 3, i32 4, i32 5, i32 6, i32 7, i32 8, i32 9, i32 10, i32 11, i32 12)
  call i32 (ptr, ...) @printf(ptr @.mixed, i32 -1, double 2.5, ptr @.str, i64 -4000000000, double 0.25, i32 122, i32 -1, double 1.0e3)
  call i32 (ptr, ...) @printf(ptr @.fps, double 1.0, double 2.0, double 3.0, double 4.0, double 5.0, double 6.0, double 7.0, double 8.0, double 9.0, double 10.0)
  call i32 (ptr, ...) @printf(ptr @.wide, i64 -9223372036854775808, i64 -1, i64 3735928559, ptr null)
  call void @small(i8 -7, i16 -300, i1 true)
  %n = call i32 (ptr, i64, ptr, ...) @snprintf(ptr %buf, i64 64, ptr @.into, i32 42, ptr @.str, double 12345.678)
  call i32 (ptr, ...) @printf(ptr @.out, i32 %n, ptr %buf)
  ret i32 0
}
