; A variadic function defined in IR: va_start, va_copy and va_end, with the
; argument list consumed by the C library (vsnprintf, vprintf). The named
; parameters use up some of the argument registers first, and the calls
; pass enough arguments to reach the stack.
;
; The va_list is 32 bytes on AArch64 and 24 on x86-64; the alloca is large
; enough for both. The C library receives a pointer to it on both targets
; (an array that decays on x86-64, a struct passed by reference on AArch64).
;@ covers: spec/ir.md §5.4 call (variadic callee), §5.5 intrinsics (va_start, va_copy, va_end)
;@ tier: quick
;@ unsupported(aarch64): global:function-definition
;@ unsupported(x86_64): global:variable-definition
;@ stdout: tag 0 scale 0.5: no arguments
;@ stdout: tag 1 scale 1.5: 1 2 3 4 5 6 7 8 9 10
;@ stdout: tag 2 scale 2.5: alpha 3.25 -7 0.5 123456789012 2.125 omega
;@ stdout: tag 3 scale 3.5: 1.5 2.5 3.5 4.5 5.5 6.5 7.5 8.5 9.5
;@ stdout: lengths: 13 21 43 36

@.none = private unnamed_addr constant [14 x i8] c"no arguments\0A\00"
@.ints = private unnamed_addr constant [31 x i8] c"%d %d %d %d %d %d %d %d %d %d\0A\00"
@.mixed = private unnamed_addr constant [30 x i8] c"%s %.2f %d %.1f %lld %.3f %s\0A\00"
@.fps = private unnamed_addr constant [46 x i8] c"%.1f %.1f %.1f %.1f %.1f %.1f %.1f %.1f %.1f\0A\00"
@.tagged = private unnamed_addr constant [20 x i8] c"tag %d scale %.1f: \00"
@.len = private unnamed_addr constant [22 x i8] c"lengths: %d %d %d %d\0A\00"
@.a = private unnamed_addr constant [6 x i8] c"alpha\00"
@.b = private unnamed_addr constant [6 x i8] c"omega\00"

declare i32 @printf(ptr, ...)
declare i32 @vprintf(ptr, ptr)
declare i32 @vsnprintf(ptr, i64, ptr, ptr)
declare void @llvm.va_start.p0(ptr)
declare void @llvm.va_copy.p0.p0(ptr, ptr)
declare void @llvm.va_end.p0(ptr)

; Prints the tag and the scale, then the format with the variable
; arguments. Returns the length vsnprintf computes for the same list,
; which must equal what vprintf wrote.
define i32 @report(i32 %tag, double %scale, ptr %fmt, ...) {
  %ap = alloca [32 x i8], align 16
  %copy = alloca [32 x i8], align 16
  %buf = alloca [128 x i8]
  call void @llvm.va_start.p0(ptr %ap)
  call void @llvm.va_copy.p0.p0(ptr %copy, ptr %ap)
  %measured = call i32 @vsnprintf(ptr %buf, i64 128, ptr %fmt, ptr %copy)
  call void @llvm.va_end.p0(ptr %copy)
  call i32 (ptr, ...) @printf(ptr @.tagged, i32 %tag, double %scale)
  %written = call i32 @vprintf(ptr %fmt, ptr %ap)
  call void @llvm.va_end.p0(ptr %ap)
  %same = icmp eq i32 %measured, %written
  %r = select i1 %same, i32 %written, i32 -1
  ret i32 %r
}

define i32 @main() {
  %n0 = call i32 (i32, double, ptr, ...) @report(i32 0, double 0.5, ptr @.none)
  %n1 = call i32 (i32, double, ptr, ...) @report(i32 1, double 1.5, ptr @.ints, i32 1, i32 2, i32 3, i32 4, i32 5, i32 6, i32 7, i32 8, i32 9, i32 10)
  %n2 = call i32 (i32, double, ptr, ...) @report(i32 2, double 2.5, ptr @.mixed, ptr @.a, double 3.25, i32 -7, double 0.5, i64 123456789012, double 2.125, ptr @.b)
  %n3 = call i32 (i32, double, ptr, ...) @report(i32 3, double 3.5, ptr @.fps, double 1.5, double 2.5, double 3.5, double 4.5, double 5.5, double 6.5, double 7.5, double 8.5, double 9.5)
  call i32 (ptr, ...) @printf(ptr @.len, i32 %n0, i32 %n1, i32 %n2, i32 %n3)
  ret i32 0
}
