@foo = global i32 1
@wk = weak global i32 2
@hid = hidden global i32 3
@loc = internal global i32 4
declare void @bar()
declare void @ext2()
@keep = global [1 x ptr] [ptr @loc]
