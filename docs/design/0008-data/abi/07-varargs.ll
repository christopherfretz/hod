target triple = "i686-unknown-linux-gnu"
define i32 @va(i32 %n, ...) { %ap = alloca ptr
  call void @llvm.va_start.p0(ptr %ap)
  %x = va_arg ptr %ap, i32
  %d = va_arg ptr %ap, double
  call void @llvm.va_end.p0(ptr %ap)
  ret i32 %x }
declare void @llvm.va_start.p0(ptr)
declare void @llvm.va_end.p0(ptr)
declare i32 @printf(ptr, ...)
@fmt = private constant [4 x i8] c"%d\0A\00"
define i32 @cva() { %r = call i32 (ptr, ...) @printf(ptr @fmt, i32 1, double 2.0, i64 3, x86_fp80 0xK3FFF8000000000000000)
  ret i32 %r }
