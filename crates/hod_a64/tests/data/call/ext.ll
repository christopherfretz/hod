; WP-069: integers with `zeroext`/`signext` of widths other than 8, 16,
; 32 and multiples of 64, in registers and on the stack, arguments and
; results (spec/abi-a64.md §7). Driven by ext.c (tests/call.rs): the
; t_* functions take an iN without an attribute (the driver passes an
; i128 of junk, so the bits above N are junk), pass the value
; to gcc's c_* checkers and to ext_callee.ll's e_* (which pass it on
; to c_* and return it), and return e_*'s result, all extended.
declare void @c_z7s(i64, i64, i64, i64, i64, i64, i64, i64, i7 zeroext)
declare zeroext i7 @e_z7s(i64, i64, i64, i64, i64, i64, i64, i64, i7 zeroext)
define zeroext i7 @t_z7s(i7 %x) {
  call void @c_z7s(i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i7 zeroext %x)
  %r = call zeroext i7 @e_z7s(i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i7 zeroext %x)
  ret i7 %r
}
declare void @c_s7s(i64, i64, i64, i64, i64, i64, i64, i64, i7 signext)
declare signext i7 @e_s7s(i64, i64, i64, i64, i64, i64, i64, i64, i7 signext)
define signext i7 @t_s7s(i7 %x) {
  call void @c_s7s(i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i7 signext %x)
  %r = call signext i7 @e_s7s(i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i7 signext %x)
  ret i7 %r
}
declare void @c_s17r(i17 signext)
declare signext i17 @e_s17r(i17 signext)
define signext i17 @t_s17r(i17 %x) {
  call void @c_s17r(i17 signext %x)
  %r = call signext i17 @e_s17r(i17 signext %x)
  ret i17 %r
}
declare void @c_z17s(i64, i64, i64, i64, i64, i64, i64, i64, i17 zeroext)
declare zeroext i17 @e_z17s(i64, i64, i64, i64, i64, i64, i64, i64, i17 zeroext)
define zeroext i17 @t_z17s(i17 %x) {
  call void @c_z17s(i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i17 zeroext %x)
  %r = call zeroext i17 @e_z17s(i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i17 zeroext %x)
  ret i17 %r
}
declare void @c_z33s(i64, i64, i64, i64, i64, i64, i64, i64, i33 zeroext)
declare zeroext i33 @e_z33s(i64, i64, i64, i64, i64, i64, i64, i64, i33 zeroext)
define zeroext i33 @t_z33s(i33 %x) {
  call void @c_z33s(i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i33 zeroext %x)
  %r = call zeroext i33 @e_z33s(i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i33 zeroext %x)
  ret i33 %r
}
declare void @c_s48s(i64, i64, i64, i64, i64, i64, i64, i64, i48 signext)
declare signext i48 @e_s48s(i64, i64, i64, i64, i64, i64, i64, i64, i48 signext)
define signext i48 @t_s48s(i48 %x) {
  call void @c_s48s(i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i48 signext %x)
  %r = call signext i48 @e_s48s(i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i48 signext %x)
  ret i48 %r
}
declare void @c_s63r(i63 signext)
declare signext i63 @e_s63r(i63 signext)
define signext i63 @t_s63r(i63 %x) {
  call void @c_s63r(i63 signext %x)
  %r = call signext i63 @e_s63r(i63 signext %x)
  ret i63 %r
}
declare void @c_s65r(i65 signext)
declare signext i65 @e_s65r(i65 signext)
define signext i65 @t_s65r(i65 %x) {
  call void @c_s65r(i65 signext %x)
  %r = call signext i65 @e_s65r(i65 signext %x)
  ret i65 %r
}
declare void @c_z65s(i64, i64, i64, i64, i64, i64, i64, i64, i65 zeroext)
declare zeroext i65 @e_z65s(i64, i64, i64, i64, i64, i64, i64, i64, i65 zeroext)
define zeroext i65 @t_z65s(i65 %x) {
  call void @c_z65s(i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i65 zeroext %x)
  %r = call zeroext i65 @e_z65s(i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i65 zeroext %x)
  ret i65 %r
}
declare void @c_s100r(i100 signext)
declare signext i100 @e_s100r(i100 signext)
define signext i100 @t_s100r(i100 %x) {
  call void @c_s100r(i100 signext %x)
  %r = call signext i100 @e_s100r(i100 signext %x)
  ret i100 %r
}
declare void @c_z100s(i64, i64, i64, i64, i64, i64, i64, i100 zeroext)
declare zeroext i100 @e_z100s(i64, i64, i64, i64, i64, i64, i64, i100 zeroext)
define zeroext i100 @t_z100s(i100 %x) {
  call void @c_z100s(i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i100 zeroext %x)
  %r = call zeroext i100 @e_z100s(i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i100 zeroext %x)
  ret i100 %r
}
declare void @c_s100s(i64, i64, i64, i64, i64, i64, i64, i64, i100 signext)
declare signext i100 @e_s100s(i64, i64, i64, i64, i64, i64, i64, i64, i100 signext)
define signext i100 @t_s100s(i100 %x) {
  call void @c_s100s(i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i100 signext %x)
  %r = call signext i100 @e_s100s(i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i100 signext %x)
  ret i100 %r
}
declare void @c_s127r(i64, i64, i64, i64, i64, i64, i127 signext)
declare signext i127 @e_s127r(i64, i64, i64, i64, i64, i64, i127 signext)
define signext i127 @t_s127r(i127 %x) {
  call void @c_s127r(i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i127 signext %x)
  %r = call signext i127 @e_s127r(i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i127 signext %x)
  ret i127 %r
}
declare void @c_z127s(i64, i64, i64, i64, i64, i64, i64, i64, i127 zeroext)
declare zeroext i127 @e_z127s(i64, i64, i64, i64, i64, i64, i64, i64, i127 zeroext)
define zeroext i127 @t_z127s(i127 %x) {
  call void @c_z127s(i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i127 zeroext %x)
  %r = call zeroext i127 @e_z127s(i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i64 0, i127 zeroext %x)
  ret i127 %r
}
