; The spellings of the variable-argument intrinsics that LLVM 19 and 22
; read and rename to `.p0`: `llvm.va_start` and `llvm.va_end` without a
; suffix, and `llvm.va_copy.p0.p0` (WP-147). The copy is consumed after
; the original has been, so both walk the same arguments.
;@ covers: spec/ir.md §5.5 intrinsics (the upgraded va_* spellings, WP-147)
;@ tier: quick
;@ stdout: 7 2.50 -3 x
;@ stdout: 7 2.50 -3 x
;@ stdout: 1 2 3 4 5 6 7 8 9
;@ stdout: 1 2 3 4 5 6 7 8 9

@.mixed = private unnamed_addr constant [17 x i8] c"%d %.2f %lld %s\0A\00"
@.many = private unnamed_addr constant [28 x i8] c"%d %d %d %d %d %d %d %d %d\0A\00"
@.x = private unnamed_addr constant [2 x i8] c"x\00"

declare i32 @vprintf(ptr, ptr)
declare void @llvm.va_start(ptr)
declare void @llvm.va_copy.p0.p0(ptr, ptr)
declare void @llvm.va_end(ptr)

; Prints the arguments twice: through the list, then through its copy.
define void @twice(ptr %fmt, ...) {
  %ap = alloca [32 x i8], align 16
  %copy = alloca [32 x i8], align 16
  call void @llvm.va_start(ptr %ap)
  call void @llvm.va_copy.p0.p0(ptr %copy, ptr %ap)
  call i32 @vprintf(ptr %fmt, ptr %ap)
  call void @llvm.va_end(ptr %ap)
  call i32 @vprintf(ptr %fmt, ptr %copy)
  call void @llvm.va_end(ptr %copy)
  ret void
}

define i32 @main() {
  call void (ptr, ...) @twice(ptr @.mixed, i32 7, double 2.5, i64 -3, ptr @.x)
  call void (ptr, ...) @twice(ptr @.many, i32 1, i32 2, i32 3, i32 4, i32 5, i32 6, i32 7, i32 8, i32 9)
  ret i32 0
}
