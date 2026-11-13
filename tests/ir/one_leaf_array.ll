; Arrays of one leaf passed and returned by value, as clang writes a
; one-member homogeneous aggregate (`struct { long double x; }` packed is
; `[1 x fp128] alignstack(8)`): in registers, and on the stack behind
; eight integer and eight floating-point arguments, at an odd offset,
; with an `alignstack` that differs from the leaf's own slot (8 for an
; fp128, 16 for a double, a float or an i64), nested, and next to a
; two-leaf array. An fp128 is printed as its two 64-bit halves.
;@ covers: spec/ir.md §5.3 parameter attributes (alignstack)
;@ stdout: regs: 4003000000000000 0 2.5 -7 3.25
;@ stdout: stack: 9 4003000000000000 0 -5 2.5 -0.75 -7 3.25 4000000000000000 1 bfff000000000000 2 77
;@ stdout: ret: 4000800000000000 5 1.125

@.regs = private unnamed_addr constant [28 x i8] c"regs: %llx %llx %g %lld %g\0A\00"
@.stack = private unnamed_addr constant [61 x i8] c"stack: %d %llx %llx %d %g %g %lld %g %llx %llx %llx %llx %d\0A\00"
@.ret = private unnamed_addr constant [19 x i8] c"ret: %llx %llx %g\0A\00"

declare i32 @printf(ptr, ...)

define void @halves(fp128 %q, ptr %hi, ptr %lo) noinline {
  %bits = bitcast fp128 %q to i128
  %l = trunc i128 %bits to i64
  %s = lshr i128 %bits, 64
  %h = trunc i128 %s to i64
  store i64 %h, ptr %hi
  store i64 %l, ptr %lo
  ret void
}

define void @regs([1 x fp128] alignstack(8) %q, [1 x float] alignstack(16) %f,
                  [1 x i64] alignstack(16) %l, [1 x [1 x double]] %n) noinline {
  %hi = alloca i64
  %lo = alloca i64
  %qv = extractvalue [1 x fp128] %q, 0
  call void @halves(fp128 %qv, ptr %hi, ptr %lo)
  %h = load i64, ptr %hi
  %o = load i64, ptr %lo
  %fv = extractvalue [1 x float] %f, 0
  %fd = fpext float %fv to double
  %lv = extractvalue [1 x i64] %l, 0
  %nv = extractvalue [1 x [1 x double]] %n, 0, 0
  call i32 (ptr, ...) @printf(ptr @.regs, i64 %h, i64 %o, double %fd, i64 %lv, double %nv)
  ret void
}

define void @stack(i64 %x0, i64 %x1, i64 %x2, i64 %x3, i64 %x4, i64 %x5, i64 %x6, i64 %x7,
                   double %d0, double %d1, double %d2, double %d3,
                   double %d4, double %d5, double %d6, double %d7,
                   i8 %t, [1 x fp128] alignstack(8) %q, i8 %u, [1 x float] alignstack(16) %f,
                   [1 x double] alignstack(16) %d, [1 x i64] alignstack(16) %l,
                   [1 x [1 x double]] %n, [2 x fp128] alignstack(8) %p, i32 %w) noinline {
  %hi = alloca i64
  %lo = alloca i64
  %qv = extractvalue [1 x fp128] %q, 0
  call void @halves(fp128 %qv, ptr %hi, ptr %lo)
  %qh = load i64, ptr %hi
  %ql = load i64, ptr %lo
  %p0 = extractvalue [2 x fp128] %p, 0
  call void @halves(fp128 %p0, ptr %hi, ptr %lo)
  %p0h = load i64, ptr %hi
  %p0l = load i64, ptr %lo
  %p1 = extractvalue [2 x fp128] %p, 1
  call void @halves(fp128 %p1, ptr %hi, ptr %lo)
  %p1h = load i64, ptr %hi
  %p1l = load i64, ptr %lo
  %ti = sext i8 %t to i32
  %ui = sext i8 %u to i32
  %fv = extractvalue [1 x float] %f, 0
  %fd = fpext float %fv to double
  %dv = extractvalue [1 x double] %d, 0
  %lv = extractvalue [1 x i64] %l, 0
  %nv = extractvalue [1 x [1 x double]] %n, 0, 0
  call i32 (ptr, ...) @printf(ptr @.stack, i32 %ti, i64 %qh, i64 %ql, i32 %ui, double %fd,
                              double %dv, i64 %lv, double %nv, i64 %p0h, i64 %p0l,
                              i64 %p1h, i64 %p1l, i32 %w)
  ret void
}

; Returns [1 x fp128] (high half 0x4000800000000000, %k in the low half)
; and [1 x double].
define [1 x fp128] @ret_q(i64 %k) noinline {
  %high = zext i64 4611826755915743232 to i128
  %bits = shl i128 %high, 64
  %low = zext i64 %k to i128
  %all = or i128 %bits, %low
  %q = bitcast i128 %all to fp128
  %r = insertvalue [1 x fp128] poison, fp128 %q, 0
  ret [1 x fp128] %r
}

define [1 x double] @ret_d(double %x) noinline {
  %r = insertvalue [1 x double] poison, double %x, 0
  ret [1 x double] %r
}

define i32 @main() {
  ; 0xL: the low half first, then the high half.
  %q = insertvalue [1 x fp128] poison, fp128 0xL00000000000000004003000000000000, 0
  %f = insertvalue [1 x float] poison, float 2.5, 0
  %l = insertvalue [1 x i64] poison, i64 -7, 0
  %n = insertvalue [1 x [1 x double]] poison, double 3.25, 0, 0
  call void @regs([1 x fp128] alignstack(8) %q, [1 x float] alignstack(16) %f,
                  [1 x i64] alignstack(16) %l, [1 x [1 x double]] %n)
  %d = insertvalue [1 x double] poison, double -0.75, 0
  %p0 = insertvalue [2 x fp128] poison, fp128 0xL00000000000000014000000000000000, 0
  %p = insertvalue [2 x fp128] %p0, fp128 0xL0000000000000002BFFF000000000000, 1
  call void @stack(i64 1, i64 2, i64 3, i64 4, i64 5, i64 6, i64 7, i64 8,
                   double 1.0, double 2.0, double 3.0, double 4.0,
                   double 5.0, double 6.0, double 7.0, double 8.0,
                   i8 9, [1 x fp128] alignstack(8) %q, i8 -5, [1 x float] alignstack(16) %f,
                   [1 x double] alignstack(16) %d, [1 x i64] alignstack(16) %l,
                   [1 x [1 x double]] %n, [2 x fp128] alignstack(8) %p, i32 77)
  %rq = call [1 x fp128] @ret_q(i64 5)
  %rqv = extractvalue [1 x fp128] %rq, 0
  %hi = alloca i64
  %lo = alloca i64
  call void @halves(fp128 %rqv, ptr %hi, ptr %lo)
  %h = load i64, ptr %hi
  %o = load i64, ptr %lo
  %rd = call [1 x double] @ret_d(double 1.125)
  %rdv = extractvalue [1 x double] %rd, 0
  call i32 (ptr, ...) @printf(ptr @.ret, i64 %h, i64 %o, double %rdv)
  ret i32 0
}
