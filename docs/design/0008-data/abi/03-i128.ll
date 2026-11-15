target triple = "i686-unknown-linux-gnu"
define i128 @r128(i128 %a, i128 %b) { %c = add i128 %a, %b
  ret i128 %c }
declare i128 @e128(i32, i128)
define i128 @c128() { %r = call i128 @e128(i32 7, i128 9)
  ret i128 %r }
define i128 @mul128(i128 %a, i128 %b) { %c = mul i128 %a, %b
  ret i128 %c }
define i128 @div128(i128 %a, i128 %b) { %c = sdiv i128 %a, %b
  ret i128 %c }
