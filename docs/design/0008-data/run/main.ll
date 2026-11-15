target triple = "i686-unknown-linux-gnu"
@g = global i64 40
define i32 @main() { %v = load i64, ptr @g
  %w = add i64 %v, 2
  %r = trunc i64 %w to i32
  ret i32 %r }
