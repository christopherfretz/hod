; WP-315: the small intrinsics, each folded into a printed value:
; frameaddress / returnaddress / addressofreturnaddress agree with each
; other and with the caller's frame, objectsize of an unknown object,
; is.constant, ptrmask, readcyclecounter, and pause / prefetch /
; clear_cache, which have no result; ubsantrap / debugtrap compiled, not
; run.
target triple = "i686-unknown-linux-gnu"
@fmt = private constant [22 x i8] c"%d %d %d %d %d %d %d\0A\00"
@done = private constant [5 x i8] c"done\00"
@g = global [16 x i8] zeroinitializer, align 16
declare i32 @printf(ptr, ...)
declare i32 @puts(ptr)
declare ptr @llvm.frameaddress.p0(i32)
declare ptr @llvm.returnaddress(i32)
declare ptr @llvm.addressofreturnaddress.p0()
declare i32 @llvm.objectsize.i32.p0(ptr, i1, i1, i1)
declare i1 @llvm.is.constant.i32(i32)
declare ptr @llvm.ptrmask.p0.i32(ptr, i32)
declare i64 @llvm.readcyclecounter()
declare void @llvm.x86.sse2.pause()
declare void @llvm.prefetch.p0(ptr, i32, i32, i32)
declare void @llvm.clear_cache(ptr, ptr)
define internal i32 @inner(ptr %outer_fa) noinline nounwind {
  %fa0 = call ptr @llvm.frameaddress.p0(i32 0)
  %fa1 = call ptr @llvm.frameaddress.p0(i32 1)
  %ra = call ptr @llvm.returnaddress(i32 0)
  %ara = call ptr @llvm.addressofreturnaddress.p0()
  %ra2 = load ptr, ptr %ara
  %same = icmp eq ptr %fa1, %outer_fa
  %same2 = icmp eq ptr %ra, %ra2
  %d = ptrtoint ptr %ara to i32
  %e = ptrtoint ptr %fa0 to i32
  %diff = sub i32 %d, %e
  %a = zext i1 %same to i32
  %b = zext i1 %same2 to i32
  %r = shl i32 %a, 1
  %r2 = or i32 %r, %b
  %r3 = shl i32 %diff, 4
  %r4 = or i32 %r3, %r2
  ret i32 %r4
}
define i32 @main(i32 %argc) nounwind {
  %fa = call ptr @llvm.frameaddress.p0(i32 0)
  %x = call i32 @inner(ptr %fa)
  %os = call i32 @llvm.objectsize.i32.p0(ptr %fa, i1 false, i1 false, i1 false)
  %os2 = call i32 @llvm.objectsize.i32.p0(ptr %fa, i1 true, i1 false, i1 false)
  %c1 = call i1 @llvm.is.constant.i32(i32 5)
  %c2 = call i1 @llvm.is.constant.i32(i32 %argc)
  %c1z = zext i1 %c1 to i32
  %c2z = zext i1 %c2 to i32
  %p = getelementptr i8, ptr @g, i32 13
  %m = call ptr @llvm.ptrmask.p0.i32(ptr %p, i32 -8)
  %mi = ptrtoint ptr %m to i32
  %gi = ptrtoint ptr @g to i32
  %md = sub i32 %mi, %gi
  %t1 = call i64 @llvm.readcyclecounter()
  call void @llvm.x86.sse2.pause()
  call void @llvm.prefetch.p0(ptr @g, i32 0, i32 3, i32 1)
  call void @llvm.clear_cache(ptr @g, ptr %p)
  %t2 = call i64 @llvm.readcyclecounter()
  %ok = icmp uge i64 %t2, %t1
  %okz = zext i1 %ok to i32
  call i32 (ptr, ...) @printf(ptr @fmt, i32 %x, i32 %os, i32 %os2, i32 %c1z, i32 %c2z, i32 %md, i32 %okz)
  %dn = call i32 @puts(ptr @done)
  ret i32 0
}
declare void @llvm.ubsantrap(i8)
declare void @llvm.debugtrap()
define void @traps(i1 %c) nounwind {
  br i1 %c, label %a, label %b
a:
  call void @llvm.ubsantrap(i8 7)
  unreachable
b:
  call void @llvm.debugtrap()
  ret void
}
