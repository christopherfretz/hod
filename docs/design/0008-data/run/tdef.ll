target triple = "i686-unknown-linux-gnu"
@gd = thread_local global i32 1
@ie = thread_local global i32 2
define i32 @main() { ret i32 0 }
