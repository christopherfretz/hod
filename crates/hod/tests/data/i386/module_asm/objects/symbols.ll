declare void @f()
declare void @g()
@v = external global i32
declare void @U()
@V = global i32 7
@W = global [2 x i32] [i32 1, i32 2]
@H = hidden global i16 3
@P = global i32 4
@T = thread_local(localexec) global i32 9
@keep = global [3 x ptr] [ptr @f, ptr @g, ptr @v]
