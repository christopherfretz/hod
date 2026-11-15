target triple = "i686-unknown-linux-gnu"
@gd = external thread_local global i32
@ld = internal thread_local global i32 0
@ie = external thread_local(initialexec) global i32
@le = thread_local(localexec) global i32 0
define i32 @t() { %a = load i32, ptr @gd
  %b = load i32, ptr @ld
  %c = load i32, ptr @ie
  %d = load i32, ptr @le
  %s = add i32 %a, %b
  %u = add i32 %c, %d
  %v = add i32 %s, %u
  ret i32 %v }
