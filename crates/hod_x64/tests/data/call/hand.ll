; WP-125: the handwritten call forms on x86-64, driven by hand.c
; (tests/call.rs). Straight-line functions only: arguments in, calls out
; to C and back (the integer, memory and control groups are not needed).

@g_src = global [5 x i64] [i64 1, i64 2, i64 3, i64 4, i64 5], align 8
@g_big = global [4 x i64] zeroinitializer, align 8
@g_ap = global [3 x i64] zeroinitializer, align 16
@g_ap2 = global [3 x i64] zeroinitializer, align 16
@jb = external global [1 x i8]

declare void @c_clobber()
declare { i7, i64 } @c_odd()
declare void @c_see_odd({ i7, i64 })
declare i65 @c_wide()
declare void @c_see_wide(i65)
declare void @c_byval(i64, i64, i64, i64, i64, i64, i64, i64, ptr byval([5 x i64]) align 8)
declare void @c_see_copy(ptr, ptr)
declare i32 @_setjmp(ptr) returns_twice
declare void @c_after(i32, i64)
declare i64 @c_mt(i64, i64, i64, i64, i64, i64, i64, i64, i64, i64)
declare void @c_vsum(i32, ...)
declare void @c_done() noreturn
declare i64 @c_ext(i8 signext, i16 zeroext, i1 zeroext, i32 signext, i8, i16)
declare signext i8 @c_ret8()
declare void @c_see_ret8_i8(i8 signext)

; A Mem-class result whose leaf is narrower than its byte: the bits above
; the i7 are cleared in the result's home after the call.
define void @h_odd() {
  %r = call { i7, i64 } @c_odd()
  call void @c_see_odd({ i7, i64 } %r)
  ret void
}

define void @h_wide() {
  %r = call i65 @c_wide()
  call void @c_see_wide(i65 %r)
  ret void
}

; byval: the copy goes to the stack-argument area, after six registers
; and two stack slots.
define void @h_byval() {
  tail call void @c_byval(i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, ptr byval([5 x i64]) align 8 @g_src)
  ret void
}

define void @h_byval_callee(ptr byval([5 x i64]) align 8 %p) {
  call void @c_see_copy(ptr %p, ptr @g_src)
  ret void
}

define void @h_byval_caller() {
  call void @h_byval_callee(ptr byval([5 x i64]) align 8 @g_src)
  ret void
}

; returns_twice: %x lives across the second return.
define void @h_sj(i64 %x) {
  %r = call i32 @_setjmp(ptr @jb) returns_twice
  call void @c_after(i32 %r, i64 %x)
  ret void
}

; musttail with arguments moved between registers and stack slots.
define i64 @h_mt(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6, i64 %a7, i64 %a8, i64 %a9) {
  %r = musttail call i64 @c_mt(i64 %a9, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6, i64 %a7, i64 %a0, i64 %a8)
  ret i64 %r
}

; A variadic thunk: the unnamed arguments are forwarded, `al` too.
define void @h_thunk(i32 %n, ...) {
  musttail call void (i32, ...) @c_vsum(i32 %n, ...)
  ret void
}

; A variadic thunk that calls first: the argument registers and `al`
; hold other values at the `musttail` call, so they must be reloaded.
define void @h_thunk_call(i32 %n, ...) {
  call void @c_clobber()
  musttail call void (i32, ...) @c_vsum(i32 %n, ...)
  ret void
}

; An indirect musttail call: the callee travels in r11.
define i64 @h_mti(ptr %f, i64 %x) {
  %r = musttail call i64 %f(ptr %f, i64 %x)
  ret i64 %r
}

; The extensions a caller owes, the call site's attributes and the
; declaration's.
define i64 @h_ext(i8 %a, i16 %b, i1 %c, i32 %d, i8 %e, i16 %f) {
  %r = call i64 @c_ext(i8 signext %a, i16 zeroext %b, i1 zeroext %c, i32 signext %d, i8 %e, i16 %f)
  ret i64 %r
}

define i64 @h_ext_decl(i8 %a, i16 %b, i1 %c, i32 %d, i8 %e, i16 %f) {
  %r = call i64 @c_ext(i8 %a, i16 %b, i1 %c, i32 %d, i8 %e, i16 %f)
  ret i64 %r
}

; A result passed on with an extension the callee owes again.
define void @h_ret8() {
  %r = call signext i8 @c_ret8()
  call void @c_see_ret8_i8(i8 signext %r)
  ret void
}

; Odd widths with extensions, both ways: the producer's duty of
; spec/abi-x64.md §7 for every width class.
declare void @c_see_odd_ext(i7 zeroext, i7 signext, i17 zeroext, i17 signext, i33 zeroext, i33 signext)
declare void @c_see_odd_ext_stack(i64, i64, i64, i64, i64, i64, i7 zeroext, i7 signext, i33 zeroext, i33 signext)
define void @h_odd_ext() {
  call void @c_clobber()
  call void @c_see_odd_ext(i7 zeroext -1, i7 signext -1, i17 zeroext -1, i17 signext -1, i33 zeroext -1, i33 signext -1)
  ret void
}
define void @h_odd_ext_args(i7 %a, i7 %b, i17 %c, i17 %d, i33 %e, i33 %f) {
  call void @c_clobber()
  call void @c_see_odd_ext(i7 zeroext %a, i7 signext %b, i17 zeroext %c, i17 signext %d, i33 zeroext %e, i33 signext %f)
  call void @c_see_odd_ext_stack(i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i7 zeroext %a, i7 signext %b, i33 zeroext %e, i33 signext %f)
  ret void
}
define zeroext i7 @h_ret_i7z(i7 %a) {
  ret i7 %a
}
define signext i7 @h_ret_i7s(i7 %a) {
  ret i7 %a
}
define signext i33 @h_ret_i33s(i33 %a) {
  ret i33 %a
}
define zeroext i33 @h_ret_i33z(i33 %a) {
  ret i33 %a
}
define zeroext i1 @h_ret_i1z(i1 %a) {
  ret i1 %a
}
define signext i17 @h_ret_i17s(i17 %a) {
  ret i17 %a
}

; sret: the definition hands its pointer back in rax; a call with an
; sret argument; a call of a function that returns in memory.
declare void @c_fill(ptr)
define void @h_sret(ptr sret([4 x i64]) %p) {
  call void @c_clobber()
  call void @c_fill(ptr %p)
  ret void
}
define void @h_sret_second(ptr %q, ptr sret([4 x i64]) %p) {
  call void @c_fill(ptr %p)
  ret void
}
declare void @c_make(ptr sret([4 x i64]))
declare void @c_see_made(ptr)
define void @h_sret_call() {
  call void @c_make(ptr sret([4 x i64]) @g_big)
  call void @c_see_made(ptr @g_big)
  ret void
}
define void @h_sret_musttail(ptr sret([4 x i64]) %p) {
  musttail call void @c_make(ptr sret([4 x i64]) %p)
  ret void
}
declare [5 x i64] @c_big5()
declare void @c_see_big5([5 x i64])
define void @h_ret_memory_call() {
  %r = call [5 x i64] @c_big5()
  call void @c_clobber()
  call void @c_see_big5([5 x i64] %r)
  ret void
}

; Results in rax:rdx, xmm0:xmm1, rax:xmm0, passed on.
declare { i64, i64 } @c_pair()
declare void @c_see_pair({ i64, i64 })
declare { double, double } @c_dpair()
declare void @c_see_dpair({ double, double })
declare { i64, double } @c_mixed()
declare void @c_see_mixed({ i64, double })
define void @h_pairs() {
  %p = call { i64, i64 } @c_pair()
  %d = call { double, double } @c_dpair()
  %m = call { i64, double } @c_mixed()
  call void @c_clobber()
  call void @c_see_pair({ i64, i64 } %p)
  call void @c_see_dpair({ double, double } %d)
  call void @c_see_mixed({ i64, double } %m)
  ret void
}
define { double, double } @h_ret_dpair({ double, double } %v) {
  call void @c_clobber()
  ret { double, double } %v
}
define { i64, double } @h_ret_mixed({ i64, double } %v) {
  call void @c_clobber()
  ret { i64, double } %v
}

; Floating-point and mixed arguments, in registers and on the stack,
; received and passed on (no arithmetic: the values pass through).
declare void @c_see_many(double, float, i64, double, double, double, double, double, double, double, float, i32, i64, i64, i64, i64, i64, ptr)
define void @h_many(double %a, float %b, i64 %c, double %d, double %e, double %f, double %g, double %h, double %i, double %j, float %k, i32 %l, i64 %m, i64 %n, i64 %o, i64 %p, i64 %q, ptr %r) {
  call void @c_clobber()
  call void @c_see_many(double %a, float %b, i64 %c, double %d, double %e, double %f, double %g, double %h, double %i, double %j, float %k, i32 %l, i64 %m, i64 %n, i64 %o, i64 %p, i64 %q, ptr %r)
  ret void
}
define double @h_ret_double(i64 %x, double %d) {
  call void @c_clobber()
  ret double %d
}
define float @h_ret_float(float %f) {
  call void @c_clobber()
  ret float %f
}

; Calls of variadic functions: al holds the number of vector registers.
declare i32 @c_vcheck(i32, ...)
define i32 @h_vcall(double %d, i64 %x, float %f, ptr %p) {
  call void @c_clobber()
  %r = call i32 (i32, ...) @c_vcheck(i32 1, double %d, i64 %x, ptr %p, double %d)
  ret i32 %r
}
define i32 @h_vcall_none(i64 %x) {
  call void @c_clobber()
  %r = call i32 (i32, ...) @c_vcheck(i32 2, i64 %x, i64 %x)
  ret i32 %r
}
define i32 @h_vcall_stack(double %d, i64 %x) {
  %r = call i32 (i32, ...) @c_vcheck(i32 3, double %d, double %d, double %d, double %d, double %d, double %d, double %d, double %d, double %d, double %d, i64 %x, i64 %x, i64 %x, i64 %x, i64 %x, i64 %x, ptr byval([5 x i64]) align 8 @g_src)
  ret i32 %r
}
define i32 @h_vcall_ind(ptr %f, double %d) {
  %r = call i32 (i32, ...) %f(i32 1, double %d, i64 7, ptr null, double %d)
  ret i32 %r
}

; Variadic definitions: the register save area, va_start, va_copy,
; va_end. The va_list is a global or an argument (no alloca here).
declare void @llvm.va_start.p0(ptr)
declare void @llvm.va_end.p0(ptr)
declare void @llvm.va_copy.p0(ptr, ptr)
declare void @c_walk(i32, ptr)
define void @h_vdef(i32 %n, ...) {
  call void @llvm.va_start.p0(ptr @g_ap)
  call void @c_clobber()
  call void @llvm.va_copy.p0(ptr @g_ap2, ptr @g_ap)
  call void @c_walk(i32 %n, ptr @g_ap)
  call void @llvm.va_end.p0(ptr @g_ap)
  call void @c_walk(i32 %n, ptr @g_ap2)
  call void @llvm.va_end.p0(ptr @g_ap2)
  ret void
}
; Every argument register named: gp_offset 48, fp_offset 176, the
; variable arguments start after two named stack arguments.
define void @h_vdef_full(ptr %ap, i64 %a, i64 %b, i64 %c, i64 %d, i64 %e, double %f0, double %f1, double %f2, double %f3, double %f4, double %f5, double %f6, double %f7, i64 %s0, double %s1, ...) {
  call void @llvm.va_start.p0(ptr %ap)
  call void @c_walk(i32 4, ptr %ap)
  call void @llvm.va_end.p0(ptr %ap)
  ret void
}
; A variadic function that returns through sret.
define void @h_vdef_sret(ptr sret([4 x i64]) %p, i32 %n, ...) {
  call void @llvm.va_start.p0(ptr @g_ap)
  call void @c_walk(i32 %n, ptr @g_ap)
  call void @c_fill(ptr %p)
  ret void
}

; Indirect calls through r11, with the markers tail and notail.
define i64 @h_ind(ptr %f, i64 %x) {
  %r = notail call i64 %f(ptr %f, i64 %x)
  call void @c_clobber()
  %s = tail call i64 %f(ptr %f, i64 %r)
  ret i64 %s
}

define void @h_done() {
  call void @c_done() noreturn
  unreachable
}
