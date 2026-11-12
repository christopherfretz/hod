; WP-069: Mem-class arguments with integer leaves of odd widths, in
; registers and on the stack. The producer owes nothing above a leaf
; (abi-a64.md §7), but in hod's memory, and so in a Mem-class value's
; home, the bits above a width are zero (spec/codegen.md §2.2). Driven by
; odd.c (tests/call.rs), which passes junk above every leaf. The h_* functions pass
; their argument on to odd.c's c_*, which, when h_* is hod's, check
; that what hod passes on from the home is zero above every width.
declare void @c_a({ i17, i64 })
define void @h_a({ i17, i64 } %x) {
  call void @c_a({ i17, i64 } %x)
  ret void
}
declare void @c_b(i65)
define void @h_b(i65 %x) {
  call void @c_b(i65 %x)
  ret void
}
declare void @c_c({ i7, i17, i33 })
define void @h_c({ i7, i17, i33 } %x) {
  call void @c_c({ i7, i17, i33 } %x)
  ret void
}
declare void @c_d([2 x i9])
define void @h_d([2 x i9] %x) {
  call void @c_d([2 x i9] %x)
  ret void
}
declare void @c_e(i64, i64, i64, i64, i64, i64, i64, { i17, i64 })
define void @h_e(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6, { i17, i64 } %x) {
  call void @c_e(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6, { i17, i64 } %x)
  ret void
}
declare void @c_f(i64, i64, i64, i64, i64, i64, i64, i65)
define void @h_f(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6, i65 %x) {
  call void @c_f(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6, i65 %x)
  ret void
}
declare void @c_g(i64, i64, i64, i64, i64, i64, { i7, [2 x i9], i33 })
define void @h_g(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, { i7, [2 x i9], i33 } %x) {
  call void @c_g(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, { i7, [2 x i9], i33 } %x)
  ret void
}
