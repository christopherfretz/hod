; WP-069: integers with `zeroext`/`signext` of widths other than 8, 16,
; 32 and multiples of 64, in registers and on the stack, arguments and
; results (spec/abi-a64.md §7). Driven by ext.c via ext.ll (tests/call.rs): the
; t_* functions take an iN without an attribute (the driver passes an
; i128 of junk, so the bits above N are junk), pass the value
; to gcc's c_* checkers and to ext_callee.ll's e_* (which pass it on
; to c_* and return it), and return e_*'s result, all extended.
declare void @c_z7s(i64, i64, i64, i64, i64, i64, i64, i64, i7 zeroext)
define zeroext i7 @e_z7s(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6, i64 %a7, i7 zeroext %x) {
  call void @c_z7s(i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i7 zeroext %x)
  ret i7 %x
}
declare void @c_s7s(i64, i64, i64, i64, i64, i64, i64, i64, i7 signext)
define signext i7 @e_s7s(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6, i64 %a7, i7 signext %x) {
  call void @c_s7s(i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i7 signext %x)
  ret i7 %x
}
declare void @c_s17r(i17 signext)
define signext i17 @e_s17r(i17 signext %x) {
  call void @c_s17r(i17 signext %x)
  ret i17 %x
}
declare void @c_z17s(i64, i64, i64, i64, i64, i64, i64, i64, i17 zeroext)
define zeroext i17 @e_z17s(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6, i64 %a7, i17 zeroext %x) {
  call void @c_z17s(i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i17 zeroext %x)
  ret i17 %x
}
declare void @c_z33s(i64, i64, i64, i64, i64, i64, i64, i64, i33 zeroext)
define zeroext i33 @e_z33s(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6, i64 %a7, i33 zeroext %x) {
  call void @c_z33s(i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i33 zeroext %x)
  ret i33 %x
}
declare void @c_s48s(i64, i64, i64, i64, i64, i64, i64, i64, i48 signext)
define signext i48 @e_s48s(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6, i64 %a7, i48 signext %x) {
  call void @c_s48s(i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i48 signext %x)
  ret i48 %x
}
declare void @c_s63r(i63 signext)
define signext i63 @e_s63r(i63 signext %x) {
  call void @c_s63r(i63 signext %x)
  ret i63 %x
}
declare void @c_s65r(i65 signext)
define signext i65 @e_s65r(i65 signext %x) {
  call void @c_s65r(i65 signext %x)
  ret i65 %x
}
declare void @c_z65s(i64, i64, i64, i64, i64, i64, i64, i64, i65 zeroext)
define zeroext i65 @e_z65s(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6, i64 %a7, i65 zeroext %x) {
  call void @c_z65s(i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i65 zeroext %x)
  ret i65 %x
}
declare void @c_s100r(i100 signext)
define signext i100 @e_s100r(i100 signext %x) {
  call void @c_s100r(i100 signext %x)
  ret i100 %x
}
declare void @c_z100s(i64, i64, i64, i64, i64, i64, i64, i100 zeroext)
define zeroext i100 @e_z100s(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6, i100 zeroext %x) {
  call void @c_z100s(i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i100 zeroext %x)
  ret i100 %x
}
declare void @c_s100s(i64, i64, i64, i64, i64, i64, i64, i64, i100 signext)
define signext i100 @e_s100s(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6, i64 %a7, i100 signext %x) {
  call void @c_s100s(i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i100 signext %x)
  ret i100 %x
}
declare void @c_s127r(i64, i64, i64, i64, i64, i64, i127 signext)
define signext i127 @e_s127r(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i127 signext %x) {
  call void @c_s127r(i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i127 signext %x)
  ret i127 %x
}
declare void @c_z127s(i64, i64, i64, i64, i64, i64, i64, i64, i127 zeroext)
define zeroext i127 @e_z127s(i64 %a0, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6, i64 %a7, i127 zeroext %x) {
  call void @c_z127s(i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i127 zeroext %x)
  ret i127 %x
}
