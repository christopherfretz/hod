; WP-316: the IR `va_arg` instruction for every type the i386 ABI
; passes (spec/codegen-x86.md §12); called with what C passes for them.
target triple = "i686-unknown-linux-gnu"

@fmt.i = private constant [10 x i8] c"%d %d %d\0A\00"
@fmt.l = private constant [11 x i8] c"%lld %lld\0A\00"
@fmt.d = private constant [12 x i8] c"%.3f %.3Lf\0A\00"
@fmt.p = private constant [7 x i8] c"%s %d\0A\00"
@done = private constant [6 x i8] c"done\0A\00"
@str = private constant [4 x i8] c"abc\00"

declare i32 @printf(ptr, ...)

; (i32, i8 passed as int, i16 passed as int, i64, i48 in an i64 slot)
define void @ints(i32 %n, ...) #0 {
  %ap = alloca ptr, align 4
  call void @llvm.va_start.p0(ptr %ap)
  %a = va_arg ptr %ap, i32
  %b = va_arg ptr %ap, i32
  %c = va_arg ptr %ap, i32
  %b8 = trunc i32 %b to i8
  %c16 = trunc i32 %c to i16
  %bs = sext i8 %b8 to i32
  %cs = sext i16 %c16 to i32
  call i32 (ptr, ...) @printf(ptr @fmt.i, i32 %a, i32 %bs, i32 %cs)
  %l = va_arg ptr %ap, i64
  %w = va_arg ptr %ap, i48
  %wx = sext i48 %w to i64
  call i32 (ptr, ...) @printf(ptr @fmt.l, i64 %l, i64 %wx)
  %ap2 = alloca ptr, align 4
  call void @llvm.va_copy.p0(ptr %ap2, ptr %ap)
  %d = va_arg ptr %ap2, double
  %x = va_arg ptr %ap2, x86_fp80
  call i32 (ptr, ...) @printf(ptr @fmt.d, double %d, x86_fp80 %x)
  %p = va_arg ptr %ap2, ptr
  %q = va_arg ptr %ap2, i32
  call i32 (ptr, ...) @printf(ptr @fmt.p, ptr %p, i32 %q)
  call void @llvm.va_end.p0(ptr %ap2)
  call void @llvm.va_end.p0(ptr %ap)
  ret void
}

; A byte and a short read as themselves: the pointer steps by 1 and 2
; and the next `i32` rounds it up again (as llc's expansion).
define i32 @narrow(ptr %area) #0 {
  %ap = alloca ptr, align 4
  store ptr %area, ptr %ap
  %a = va_arg ptr %ap, i8
  %b = va_arg ptr %ap, i16
  %c = va_arg ptr %ap, i32
  %f = va_arg ptr %ap, float
  %a32 = zext i8 %a to i32
  %b32 = zext i16 %b to i32
  %s1 = add i32 %a32, %b32
  %s2 = add i32 %s1, %c
  %fi = fptosi float %f to i32
  %s3 = add i32 %s2, %fi
  ret i32 %s3
}

define i32 @main() #0 {
  call void (i32, ...) @ints(i32 0, i32 -7, i32 200, i32 40000, i64 -81985529216486895,
                              i64 140737488355327, double 2.5, x86_fp80 0xK4000C000000000000000,
                              ptr @str, i32 99)
  %area = alloca [16 x i8], align 4
  store i8 7, ptr %area
  %p1 = getelementptr i8, ptr %area, i32 2
  store i16 300, ptr %p1
  %p2 = getelementptr i8, ptr %area, i32 4
  store i32 5000, ptr %p2
  %p3 = getelementptr i8, ptr %area, i32 8
  store float 12.5, ptr %p3
  %n = call i32 @narrow(ptr %area)
  call i32 (ptr, ...) @printf(ptr @fmt.i, i32 %n, i32 0, i32 0)
  call i32 (ptr, ...) @printf(ptr @done)
  ret i32 0
}

declare void @llvm.va_start.p0(ptr)
declare void @llvm.va_copy.p0(ptr, ptr)
declare void @llvm.va_end.p0(ptr)

attributes #0 = { nounwind }
