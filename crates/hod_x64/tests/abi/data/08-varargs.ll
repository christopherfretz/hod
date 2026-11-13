; Varargs: al = vector registers used (upper bound); the callee's register
; save area and va_start; the va_arg instruction.
%va = type { i32, i32, ptr, ptr }
declare i32 @printf(ptr, ...)
@fmt = private constant [4 x i8] c"%f\0A\00"
define void @call_va(double %d, x86_fp80 %l, i128 %q) {
  call i32 (ptr, ...) @printf(ptr @fmt, double %d, i32 1, x86_fp80 %l, i128 %q)
  ret void }
define i32 @def_va(i32 %n, ...) {
  %ap = alloca %va, align 16
  call void @llvm.va_start(ptr %ap)
  %a = va_arg ptr %ap, i32
  %b = va_arg ptr %ap, double
  call void @llvm.va_end(ptr %ap)
  %bi = fptosi double %b to i32
  %s = add i32 %a, %bi
  ret i32 %s }
declare void @llvm.va_start(ptr)
declare void @llvm.va_end(ptr)
