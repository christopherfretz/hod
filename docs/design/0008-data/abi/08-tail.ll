target triple = "i686-unknown-linux-gnu"
declare i32 @t1(i32)
define i32 @tail(i32 %a) { %r = tail call i32 @t1(i32 %a)
  ret i32 %r }
define i32 @must(i32 %a) { %r = musttail call i32 @t1(i32 %a)
  ret i32 %r }
declare i32 @t2(i32, i32)
define i32 @mustfwd(i32 %a, i32 %b) { %r = musttail call i32 @t2(i32 %b, i32 %a)
  ret i32 %r }
declare void @nested(ptr nest)
define void @cn(ptr %p) { call void @nested(ptr nest %p)
  ret void }
define i32 @isnest(ptr nest %p, i32 %a) { ret i32 %a }
