; WP-069: Mem-class arguments with integer leaves of odd widths, in
; registers and on the stack. The producer owes nothing above a leaf
; (abi-a64.md §7), but in hod's memory, and so in a Mem-class value's
; home, the bits above a width are zero (spec/codegen.md §2.2). Driven by
; odd.c (tests/call.rs), which passes junk above every leaf. The h_* functions pass
; their argument on to odd.c's c_*, which, when h_* is hod's, check
; that what hod passes on from the home is zero above every width. Each
; h_* also stores its argument to odd.c's s_* buffer, which odd.c checks
; byte for byte: zeros above every width up to its store size, whoever
; compiled h_* (WP-066; before WP-067 the store was a64:mem-store and
; the home could only be seen through what h_* passed on).
@s_a = external global [16 x i8]
@s_b = external global [16 x i8]
@s_c = external global [16 x i8]
@s_d = external global [16 x i8]
@s_e = external global [16 x i8]
@s_f = external global [16 x i8]
@s_g = external global [16 x i8]
declare void @c_a({ i17, i64 })
define void @h_a({ i17, i64 } %x) {
  store { i17, i64 } %x, ptr @s_a
  call void @c_a({ i17, i64 } %x)
  ret void
}
declare void @c_b(i65)
define void @h_b(i65 %x) {
  store i65 %x, ptr @s_b
  call void @c_b(i65 %x)
  ret void
}
declare void @c_c({ i7, i17, i33 })
define void @h_c({ i7, i17, i33 } %x) {
  store { i7, i17, i33 } %x, ptr @s_c
  call void @c_c({ i7, i17, i33 } %x)
  ret void
}
declare void @c_d([2 x i9])
define void @h_d([2 x i9] %x) {
  store [2 x i9] %x, ptr @s_d
  call void @c_d([2 x i9] %x)
  ret void
}
declare void @c_e(i64, i64, i64, i64, i64, i64, i64, { i17, i64 })
define void @h_e(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6, { i17, i64 } %x) {
  store { i17, i64 } %x, ptr @s_e
  call void @c_e(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6, { i17, i64 } %x)
  ret void
}
declare void @c_f(i64, i64, i64, i64, i64, i64, i64, i65)
define void @h_f(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6, i65 %x) {
  store i65 %x, ptr @s_f
  call void @c_f(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6, i65 %x)
  ret void
}
declare void @c_g(i64, i64, i64, i64, i64, i64, { i7, [2 x i9], i33 })
define void @h_g(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, { i7, [2 x i9], i33 } %x) {
  store { i7, [2 x i9], i33 } %x, ptr @s_g
  call void @c_g(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, { i7, [2 x i9], i33 } %x)
  ret void
}
