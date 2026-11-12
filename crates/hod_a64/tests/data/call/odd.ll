; WP-069: Mem-class arguments with integer leaves of odd widths, in
; registers and on the stack. The producer owes nothing above a leaf
; (abi-a64.md §7), but in hod's memory, and so in a Mem-class value's
; home, the bits above a width are zero (spec/codegen.md §2.2). Driven by
; odd.c (tests/call.rs), which passes junk above every leaf. The l_* functions pass
; their argument on to odd_callee.ll's h_*.
declare void @h_a({ i17, i64 })
define void @l_a({ i17, i64 } %x) {
  call void @h_a({ i17, i64 } %x)
  ret void
}
declare void @h_b(i65)
define void @l_b(i65 %x) {
  call void @h_b(i65 %x)
  ret void
}
declare void @h_c({ i7, i17, i33 })
define void @l_c({ i7, i17, i33 } %x) {
  call void @h_c({ i7, i17, i33 } %x)
  ret void
}
declare void @h_d([2 x i9])
define void @l_d([2 x i9] %x) {
  call void @h_d([2 x i9] %x)
  ret void
}
declare void @h_e(i64, i64, i64, i64, i64, i64, i64, { i17, i64 })
define void @l_e(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6, { i17, i64 } %x) {
  call void @h_e(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6, { i17, i64 } %x)
  ret void
}
declare void @h_f(i64, i64, i64, i64, i64, i64, i64, i65)
define void @l_f(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6, i65 %x) {
  call void @h_f(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6, i65 %x)
  ret void
}
declare void @h_g(i64, i64, i64, i64, i64, i64, { i7, [2 x i9], i33 })
define void @l_g(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, { i7, [2 x i9], i33 } %x) {
  call void @h_g(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, { i7, [2 x i9], i33 } %x)
  ret void
}
