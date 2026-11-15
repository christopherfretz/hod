target triple = "i686-unknown-linux-gnu"
%S = type { i32, i32, i32 }
define void @rs(ptr sret(%S) align 4 %r, i32 %x) { store i32 %x, ptr %r
  ret void }
declare void @ers(ptr sret(%S) align 4, i32)
define i32 @crs() { %t = alloca %S
  call void @ers(ptr sret(%S) align 4 %t, i32 5)
  %v = load i32, ptr %t
  ret i32 %v }
define i32 @bv(ptr byval(%S) align 4 %p, i8 %c) { %v = load i32, ptr %p
  ret i32 %v }
declare i32 @ebv(i8, ptr byval(%S) align 4, i8)
define i32 @cbv(ptr %p) { %r = call i32 @ebv(i8 1, ptr byval(%S) align 4 %p, i8 2)
  ret i32 %r }
%B = type { [5 x i32] }
declare i32 @ebv16(ptr byval(%B) align 16)
define i32 @cbv16(ptr %p) { %r = call i32 @ebv16(ptr byval(%B) align 16 %p)
  ret i32 %r }
define { i32, i32 } @rpair() { ret { i32, i32 } { i32 1, i32 2 } }
define { i32, i32, i32 } @rtriple() { ret { i32, i32, i32 } { i32 1, i32 2, i32 3 } }
define { i32, i32, i32, i32, i32 } @rquint() { ret { i32, i32, i32, i32, i32 } { i32 1, i32 2, i32 3, i32 4, i32 5 } }
define { double, double } @rdd() { ret { double, double } { double 1.0, double 2.0 } }
define { float, i32 } @rfi() { ret { float, i32 } { float 1.0, i32 2 } }
define [2 x i32] @rarr() { ret [2 x i32] [i32 1, i32 2] }
