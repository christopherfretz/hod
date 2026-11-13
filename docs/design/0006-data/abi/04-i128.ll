; i128: a register pair, never split between the last register and the
; stack; on the stack aligned 16.
declare void @s5(i64, i64, i64, i64, i64, i128)
declare void @s4(i64, i64, i64, i64, i128, i64)
define void @call_i128(i128 %a) {
  call void @s5(i64 1, i64 2, i64 3, i64 4, i64 5, i128 %a)
  call void @s4(i64 1, i64 2, i64 3, i64 4, i128 %a, i64 6)
  ret void
}
define i128 @ret_i128(i128 %a, i128 %b) { %r = mul i128 %a, %b
  ret i128 %r }
define i128 @div_i128(i128 %a, i128 %b) { %r = sdiv i128 %a, %b
  ret i128 %r }
