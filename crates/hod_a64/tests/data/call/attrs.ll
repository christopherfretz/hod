; WP-069: the attributes a direct call passes with (spec/codegen.md §7.1):
; its own, plus the declaration's when the call's function type is the
; callee's. Driven by attrs.c (tests/call.rs); the g_* callees are gcc's,
; the h_* callees are attrs_callee.ll's, compiled by hod or llc. Each
; callee is defined with the prototype the call passes by, not the one
; declared here.

@src = global [5 x i64] [i64 1, i64 2, i64 3, i64 4, i64 5], align 8

declare void @g_byval(ptr byval([5 x i64]) align 8)
declare void @g_sret(ptr sret([4 x i64]))
declare void @g_nest(ptr nest, i64)
declare void @g_var(ptr byval([2 x i64]), ...)
declare void @g_fix(ptr byval([2 x i64]))
declare void @g_sext(i8 signext)
declare zeroext i8 @g_ret(i32)

declare void @h_byval(ptr byval([5 x i64]) align 8)
declare void @h_sret(ptr sret([4 x i64]))
declare void @h_nest(ptr nest, i64)
declare void @h_var(ptr byval([2 x i64]), ...)
declare void @h_fix(ptr byval([2 x i64]))
declare void @h_union(ptr byval([5 x i64]) align 8)
declare void @h_sext(i8 signext)

; A mismatched call takes nothing from the declaration: a pointer and an
; i64 in x0 and x1, no copy, no x8, no x15.
define void @m_byval() {
  call void (ptr, i64) @g_byval(ptr @src, i64 7)
  call void (ptr, i64) @h_byval(ptr @src, i64 7)
  ret void
}

define void @m_sret() {
  call void (ptr, i64) @g_sret(ptr @src, i64 8)
  call void (ptr, i64) @h_sret(ptr @src, i64 8)
  ret void
}

define void @m_nest() {
  call void (ptr, i64, i64) @g_nest(ptr @src, i64 9, i64 10)
  call void (ptr, i64, i64) @h_nest(ptr @src, i64 9, i64 10)
  ret void
}

; A variadic declaration called through a fixed type, and the reverse.
define void @m_var_fixed() {
  call void (ptr) @g_var(ptr @src)
  call void (ptr) @h_var(ptr @src)
  ret void
}

define void @m_fixed_var() {
  call void (ptr, ...) @g_fix(ptr @src, i64 11)
  call void (ptr, ...) @h_fix(ptr @src, i64 11)
  ret void
}

; Through the callee's own type: the declaration's `byval` is taken
; although the call site has an attribute (`align`) of its own. (No gcc
; callee: C passes a 40-byte structure by reference.)
define void @m_union() {
  call void @h_union(ptr align 8 @src)
  ret void
}

; Through the callee's own type: the declaration's `signext` is taken.
; The i8 is the low byte of 0x1fd, so the register holds more than the
; byte unless the caller extends it.
define void @m_sext(i32 %w) {
  %x = trunc i32 %w to i8
  call void @g_sext(i8 %x)
  call void @h_sext(i8 %x)
  ret void
}

; A result through a mismatched type: the caller assumes nothing, and
; extends what it uses itself.
define i32 @m_ret() {
  %r = call i8 (i32, i32) @g_ret(i32 5, i32 6)
  %e = zext i8 %r to i32
  ret i32 %e
}
