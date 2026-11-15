target triple = "i686-unknown-linux-gnu"
@g = external global i32
declare i32 @h(i32)
define i32 @main() { %v = load i32, ptr @g
  %r = call i32 @h(i32 %v)
  ret i32 %r }
