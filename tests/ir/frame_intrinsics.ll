; The frame intrinsics both targets build (spec/codegen.md §5.6.38): the
; frame chain followed to depth 2, the return address at depth 1 against
; the caller's own, llvm.addressofreturnaddress against the frame record,
; and llvm.eh.unwind.init in a function that returns a value and whose
; caller's values survive it.
;@ covers: spec/ir.md §5.4 call (Intrinsic)
;@ stdout: chain: 1 1
;@ stdout: return address: 1 1
;@ stdout: address of the return address: 1 1
;@ stdout: unwind.init: 49 kept 1

@.chain = private unnamed_addr constant [14 x i8] c"chain: %d %d\0A\00"
@.ra = private unnamed_addr constant [23 x i8] c"return address: %d %d\0A\00"
@.aora = private unnamed_addr constant [38 x i8] c"address of the return address: %d %d\0A\00"
@.init = private unnamed_addr constant [27 x i8] c"unwind.init: %lld kept %d\0A\00"
@frames = global [3 x ptr] zeroinitializer
@returns = global [2 x ptr] zeroinitializer

declare i32 @printf(ptr, ...)
declare ptr @llvm.frameaddress.p0(i32)
declare ptr @llvm.returnaddress(i32)
declare ptr @llvm.addressofreturnaddress.p0()
declare void @llvm.eh.unwind.init()

define void @inner() #0 {
  %f0 = call ptr @llvm.frameaddress.p0(i32 0)
  %f1 = call ptr @llvm.frameaddress.p0(i32 1)
  %f2 = call ptr @llvm.frameaddress.p0(i32 2)
  %r1 = call ptr @llvm.returnaddress(i32 1)
  %m1 = load ptr, ptr getelementptr (ptr, ptr @frames, i64 1)
  %m2 = load ptr, ptr getelementptr (ptr, ptr @frames, i64 2)
  %c1 = icmp eq ptr %f1, %m1
  %c2 = icmp eq ptr %f2, %m2
  %z1 = zext i1 %c1 to i32
  %z2 = zext i1 %c2 to i32
  call i32 (ptr, ...) @printf(ptr @.chain, i32 %z1, i32 %z2)
  %mr = load ptr, ptr getelementptr (ptr, ptr @returns, i64 1)
  %c3 = icmp eq ptr %r1, %mr
  %z3 = zext i1 %c3 to i32
  %nn = icmp ne ptr %r1, null
  %z4 = zext i1 %nn to i32
  call i32 (ptr, ...) @printf(ptr @.ra, i32 %z3, i32 %z4)
  %a = call ptr @llvm.addressofreturnaddress.p0()
  %v = load ptr, ptr %a
  %r0 = call ptr @llvm.returnaddress(i32 0)
  %c5 = icmp eq ptr %v, %r0
  ; One pointer above the frame record's saved frame pointer, on every target.
  %g = getelementptr ptr, ptr %f0, i64 1
  %c6 = icmp eq ptr %a, %g
  %z5 = zext i1 %c5 to i32
  %z6 = zext i1 %c6 to i32
  call i32 (ptr, ...) @printf(ptr @.aora, i32 %z5, i32 %z6)
  ret void
}

define void @middle() #0 {
  %f = call ptr @llvm.frameaddress.p0(i32 0)
  store ptr %f, ptr getelementptr (ptr, ptr @frames, i64 1)
  %r = call ptr @llvm.returnaddress(i32 0)
  store ptr %r, ptr getelementptr (ptr, ptr @returns, i64 1)
  call void @inner()
  ret void
}

define void @outer() #0 {
  %f = call ptr @llvm.frameaddress.p0(i32 0)
  store ptr %f, ptr getelementptr (ptr, ptr @frames, i64 2)
  call void @middle()
  ret void
}

define i64 @saving(i64 %x) #0 {
  call void @llvm.eh.unwind.init()
  %y = mul i64 %x, %x
  ret i64 %y
}

define i32 @main() #0 {
  call void @outer()
  %k = add i64 0, 1234
  %s = call i64 @saving(i64 7)
  %kept = icmp eq i64 %k, 1234
  %zk = zext i1 %kept to i32
  call i32 (ptr, ...) @printf(ptr @.init, i64 %s, i32 %zk)
  ret i32 0
}

attributes #0 = { noinline "frame-pointer"="all" }
