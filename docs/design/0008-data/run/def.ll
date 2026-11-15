target triple = "i686-unknown-linux-gnu"
@g = global i32 40
define i32 @h(i32 %a) { %r = add i32 %a, 2
  ret i32 %r }
