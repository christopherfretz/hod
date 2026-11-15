target triple = "i686-unknown-linux-gnu"
@g = external global i32
@l = internal global i32 0
@h = hidden global i32 0
@d = dso_local global i32 0
@fp = global ptr @ext
declare void @ext()
declare hidden void @exth()
define internal void @loc() { ret void }
define i32 @ld() { %a = load i32, ptr @g
  %b = load i32, ptr @l
  %c = load i32, ptr @h
  %s = add i32 %a, %b
  %t = add i32 %s, %c
  ret i32 %t }
define void @calls() { call void @ext()
  call void @exth()
  call void @loc()
  ret void }
define ptr @addr() { ret ptr @ext }
define ptr @addrl() { ret ptr @loc }
define double @cst(double %x) { %y = fadd double %x, 3.5
  ret double %y }
define void @sw(i32 %x) { switch i32 %x, label %d [ i32 0, label %a
   i32 1, label %b
   i32 2, label %c
   i32 3, label %e
   i32 4, label %a ]
a: call void @ext()
  br label %d
b: call void @exth()
  br label %d
c: call void @loc()
  br label %d
e: call void @ext()
  br label %d
d: ret void }
