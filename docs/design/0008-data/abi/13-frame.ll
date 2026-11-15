target triple = "i686-unknown-linux-gnu"
declare void @use(ptr)
define void @fp() #0 { %a = alloca i32
  call void @use(ptr %a)
  ret void }
define void @dyn(i32 %n) #0 { %a = alloca i8, i32 %n, align 16
  call void @use(ptr %a)
  ret void }
define void @over() #0 { %a = alloca i32, align 32
  call void @use(ptr %a)
  ret void }
define void @big() #0 { %a = alloca [8192 x i8]
  call void @use(ptr %a)
  ret void }
define void @realign() #1 { %a = alloca i32
  call void @use(ptr %a)
  ret void }
attributes #0 = { "frame-pointer"="all" }
attributes #1 = { "frame-pointer"="all" "stackrealign" }
