; i386 join tests (design 0008 I7 with I8), written by hand: 64-bit
; values loaded from and stored to symbols named directly as operands
; (globals of every kind, thread-locals of every model, no
; `llvm.threadlocal.address`), a `ptrtoint` constant of a symbol stored as
; an `i64`, 64-bit divisions (libgcc calls through the PLT under PIC)
; while 32-bit values are held in registers across all of it, and an
; `i64` `select`. Built as a program (no PIE, PIE) and, with `@main`
; renamed `@prog_main`, as a shared library behind a driver; `ext_v` and
; `ext_t` are another shared library's (`ext.c`).

target datalayout = "e-m:e-p:32:32-p270:32:32-p271:32:32-p272:64:64-i128:128-f64:32:64-f80:32-n8:16:32-S128"
target triple = "i686-unknown-linux-gnu"

@g = global i64 81985529216486895
@h = hidden global i64 -81985529216486896
@l = internal global i64 1000000000000
@w = weak global i64 5
@ext_v = external global i64
@ext_t = external thread_local global i64
@t_gd = thread_local global i64 4294967297
@t_ld = internal thread_local global i64 -3
@t_ie = thread_local(initialexec) global i64 77
@addr = global i64 0
@x = global i32 7
@fmt = private constant [13 x i8] c"%d %08x%08x\0A\00"
@fmt2 = private constant [15 x i8] c"%d %d %d\0Adone\0A\00"

declare i32 @printf(ptr, ...)
declare ptr @llvm.threadlocal.address.p0(ptr)

define internal void @show(i32 %k, ptr %p) #0 {
  %lo = load i32, ptr %p
  %hp = getelementptr i8, ptr %p, i32 4
  %hi = load i32, ptr %hp
  %n = call i32 (ptr, ...) @printf(ptr @fmt, i32 %k, i32 %hi, i32 %lo)
  ret void
}

define i32 @main() #0 {
  %s = alloca i64, align 8
  %x0 = load i32, ptr @x
  %x1 = mul i32 %x0, 3
  %x2 = add i32 %x0, 100
  %x3 = xor i32 %x0, 85
  ; Globals of every kind as operands.
  %a = load i64, ptr @g
  %b = load i64, ptr @h
  %c = add i64 %a, %b
  store i64 %c, ptr @l
  %d = load i64, ptr @ext_v
  %c2 = load i64, ptr @l
  %e = mul i64 %d, %c2
  store i64 %e, ptr @w
  %e2 = load i64, ptr @w
  store i64 %e2, ptr %s
  call void @show(i32 1, ptr %s)
  store i64 -2401053088876216593, ptr @ext_v
  %d2 = load i64, ptr @ext_v
  store i64 %d2, ptr %s
  call void @show(i32 2, ptr %s)
  ; Thread-locals of every model as operands.
  %t1 = load i64, ptr @t_gd
  %t2 = load i64, ptr @t_ld
  %t3 = sub i64 %t1, %t2
  store i64 %t3, ptr @t_ie
  %t4 = load i64, ptr @ext_t
  %t5 = xor i64 %t4, %t3
  store i64 %t5, ptr @ext_t
  %t6 = load i64, ptr @t_ie
  %t7 = shl i64 %t6, 35
  store i64 %t7, ptr @t_ld
  store i64 %t5, ptr %s
  call void @show(i32 3, ptr %s)
  %t8 = load i64, ptr @t_ld
  store i64 %t8, ptr %s
  call void @show(i32 4, ptr %s)
  %t9 = load i64, ptr @ext_t
  store i64 %t9, ptr %s
  call void @show(i32 5, ptr %s)
  ; Divisions, with %x1..%x3 held across them.
  %den = or i64 %t5, 1
  %q1 = udiv i64 %e, %den
  %q2 = sdiv i64 %d2, -12345
  %q3 = urem i64 %a, %den
  %q4 = srem i64 %b, 1000003
  %q5 = sdiv i64 %t1, %t2
  store i64 %q1, ptr @t_gd
  store i64 %q2, ptr @h
  %q6 = add i64 %q3, %q4
  %q7 = add i64 %q6, %q5
  %y1 = load i64, ptr @t_gd
  store i64 %y1, ptr %s
  call void @show(i32 6, ptr %s)
  %y2 = load i64, ptr @h
  store i64 %y2, ptr %s
  call void @show(i32 7, ptr %s)
  store i64 %q7, ptr %s
  call void @show(i32 8, ptr %s)
  ; A symbol's address as a 64-bit constant.
  store i64 ptrtoint (ptr @g to i64), ptr @addr
  %pa = load i64, ptr @addr
  %pg = ptrtoint ptr @g to i32
  %pz = zext i32 %pg to i64
  %eq1 = icmp eq i64 %pa, %pz
  %sel = select i1 %eq1, i64 %q1, i64 %q2
  store i64 %sel, ptr %s
  call void @show(i32 9, ptr %s)
  ; Thread-locals' addresses as `ptrtoint` instructions to `i64`, against
  ; `llvm.threadlocal.address` (as a constant expression it is
  ; `tls:address-in-constant`, a refusal of every target).
  %pt = ptrtoint ptr @t_ld to i64
  %ta = call ptr @llvm.threadlocal.address.p0(ptr @t_ld)
  %ti = ptrtoint ptr %ta to i32
  %tz = zext i32 %ti to i64
  %eq2 = icmp eq i64 %pt, %tz
  %pd = ptrtoint ptr @t_gd to i64
  %tb = call ptr @llvm.threadlocal.address.p0(ptr @t_gd)
  %tj = ptrtoint ptr %tb to i32
  %tk = zext i32 %tj to i64
  %eq3 = icmp eq i64 %pd, %tk
  store i64 1234567890123, ptr @t_gd
  %tc = load i64, ptr %tb
  store i64 %tc, ptr %s
  call void @show(i32 10, ptr %s)
  %ne0 = icmp ne i64 %pa, 0
  %ne1 = and i1 %ne0, %eq2
  %ne = and i1 %ne1, %eq3
  %z = zext i1 %ne to i32
  %sum = add i32 %x1, %x2
  %n = call i32 (ptr, ...) @printf(ptr @fmt2, i32 %sum, i32 %x3, i32 %z)
  ret i32 0
}

attributes #0 = { nounwind }
