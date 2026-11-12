; WP-038: the handwritten call forms, driven by hand.c (tests/call.rs).

@g_src = global [5 x i64] [i64 1, i64 2, i64 3, i64 4, i64 5], align 8
@jb = external global [1 x i8]

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

; byval: the copy goes to the stack-argument area, after eight registers.
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

; A variadic thunk: the unnamed arguments are forwarded.
define void @h_thunk(i32 %n, ...) {
  musttail call void (i32, ...) @c_vsum(i32 %n, ...)
  ret void
}

; A variadic thunk that calls first: the argument registers hold other
; values at the `musttail` call, so the unnamed ones must be reloaded.
declare void @c_clobber(i64, i64, i64, i64, i64, i64, i64, i64, double, double, double, double, double, double, double, double)
define void @h_thunk_call(i32 %n, ...) {
  call void @c_clobber(i64 -1, i64 -1, i64 -1, i64 -1, i64 -1, i64 -1, i64 -1, i64 -1, double -9.0, double -9.0, double -9.0, double -9.0, double -9.0, double -9.0, double -9.0, double -9.0)
  musttail call void (i32, ...) @c_vsum(i32 %n, ...)
  ret void
}

; An indirect musttail call.
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
declare void @c_see_ret8_i8(i8 signext)

define void @h_done() {
  call void @c_done() noreturn
  unreachable
}
