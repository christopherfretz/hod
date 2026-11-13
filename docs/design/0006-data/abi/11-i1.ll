; A plain i1 argument and result: what the caller writes and the callee assumes.
declare void @take_i1(i1)
declare void @take_i8(i8)
declare i1 @give_i1()
define void @pass_trunc(i32 %x) { %b = trunc i32 %x to i1
  call void @take_i1(i1 %b)
  ret void }
define void @pass_trunc8(i32 %x) { %b = trunc i32 %x to i8
  call void @take_i8(i8 %b)
  ret void }
define i32 @recv_i1(i1 %b) { %z = zext i1 %b to i32
  ret i32 %z }
define i32 @use_give() { %b = call i1 @give_i1()
  %z = zext i1 %b to i32
  ret i32 %z }
define i1 @ret_trunc(i32 %x) { %b = trunc i32 %x to i1
  ret i1 %b }
