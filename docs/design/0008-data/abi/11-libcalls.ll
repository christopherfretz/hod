target triple = "i686-unknown-linux-gnu"
define i64 @sdiv(i64 %a, i64 %b) { %r = sdiv i64 %a, %b
  ret i64 %r }
define i64 @udiv(i64 %a, i64 %b) { %r = udiv i64 %a, %b
  ret i64 %r }
define i64 @srem(i64 %a, i64 %b) { %r = srem i64 %a, %b
  ret i64 %r }
define i64 @urem(i64 %a, i64 %b) { %r = urem i64 %a, %b
  ret i64 %r }
define i64 @mul(i64 %a, i64 %b) { %r = mul i64 %a, %b
  ret i64 %r }
define i64 @shl(i64 %a, i64 %b) { %r = shl i64 %a, %b
  ret i64 %r }
define i64 @ashr(i64 %a, i64 %b) { %r = ashr i64 %a, %b
  ret i64 %r }
define i64 @f2s(double %a) { %r = fptosi double %a to i64
  ret i64 %r }
define i64 @f2u(double %a) { %r = fptoui double %a to i64
  ret i64 %r }
define double @s2f(i64 %a) { %r = sitofp i64 %a to double
  ret double %r }
define double @u2f(i64 %a) { %r = uitofp i64 %a to double
  ret double %r }
define i32 @f2s32(double %a) { %r = fptosi double %a to i32
  ret i32 %r }
define double @frem(double %a, double %b) { %r = frem double %a, %b
  ret double %r }
define i64 @smulo(i64 %a, i64 %b) { %r = call {i64, i1} @llvm.smul.with.overflow.i64(i64 %a, i64 %b)
  %v = extractvalue {i64, i1} %r, 0
  ret i64 %v }
declare {i64, i1} @llvm.smul.with.overflow.i64(i64, i64)
define i64 @ctpop(i64 %a) { %r = call i64 @llvm.ctpop.i64(i64 %a)
  ret i64 %r }
declare i64 @llvm.ctpop.i64(i64)
define void @mc(ptr %a, ptr %b, i32 %n) { call void @llvm.memcpy.p0.p0.i32(ptr %a, ptr %b, i32 %n, i1 false)
  ret void }
declare void @llvm.memcpy.p0.p0.i32(ptr, ptr, i32, i1)
