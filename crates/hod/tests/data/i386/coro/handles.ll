; WP-327 stage A: the coroutine handle operations on i386 in a function
; that is no coroutine, on frames built by hand with LLVM's layout
; (resume at 0, destroy at 4, the promise at alignTo(8, align)).
; `opt -passes='default<O0>'` lowers the intrinsics for llc; hod lowers
; them itself. A `double` live across `coro.resume` and `coro.destroy`
; is rounded to 64 bits at each (the x87 barrier): kept in 80 bits,
; `y * 3.0 - x` prints another value.
target datalayout = "e-m:e-p:32:32-p270:32:32-p271:32:32-p272:64:64-i128:128-f64:32:64-f80:32-n8:16:32-S128"
target triple = "i686-unknown-linux-gnu"

; resume, destroy, an i32 promise at 8.
@frame = global { ptr, ptr, i32 } { ptr @resume, ptr @destroy, i32 0 }, align 8
; resume, destroy, padding, a promise aligned to 16 at 16.
@frame16 = global { ptr, ptr, [8 x i8], i64 } { ptr @resume16, ptr @destroy, [8 x i8] zeroinitializer, i64 40 }, align 16

@fmt.count = private constant [17 x i8] c"count %d done %d\00"
@fmt.line = private constant [2 x i8] c"\0A\00"
@fmt.double = private constant [12 x i8] c" double %a\0A\00"
@fmt.diff = private constant [15 x i8] c"promise at %d\0A\00"
@fmt.noop = private constant [24 x i8] c"noop done %d same %d\0A\00\00\00"
@fmt.wide = private constant [15 x i8] c"wide %lld %d\0A\00\00"

; Adds 1 to the promise; the fourth call ends the coroutine (a null
; resume pointer is `done`).
define internal fastcc void @resume(ptr %f) {
  %p = getelementptr i8, ptr %f, i32 8
  %v = load i32, ptr %p
  %n = add i32 %v, 1
  store i32 %n, ptr %p
  %last = icmp eq i32 %n, 4
  br i1 %last, label %end, label %out
end:
  store ptr null, ptr %f
  br label %out
out:
  ret void
}

define internal fastcc void @resume16(ptr %f) {
  %p = getelementptr i8, ptr %f, i32 16
  %v = load i64, ptr %p
  %n = mul i64 %v, 1000000007
  store i64 %n, ptr %p
  ret void
}

define internal fastcc void @destroy(ptr %f) {
  store ptr null, ptr %f
  %p = getelementptr i8, ptr %f, i32 8
  store i32 -1, ptr %p
  ret void
}

define i32 @main(i32 %argc, ptr %argv) {
  %x = sitofp i32 %argc to double
  %y = fdiv double %x, 3.0
  br label %loop
loop:
  %done = call i1 @llvm.coro.done(ptr @frame)
  br i1 %done, label %after, label %step
step:
  call void @llvm.coro.resume(ptr @frame)
  %pr = call ptr @llvm.coro.promise(ptr @frame, i32 4, i1 false)
  %count = load i32, ptr %pr
  %d = call i1 @llvm.coro.done(ptr @frame)
  %dz = zext i1 %d to i32
  call i32 (ptr, ...) @printf(ptr @fmt.count, i32 %count, i32 %dz)
  call i32 (ptr, ...) @printf(ptr @fmt.line)
  br label %loop
after:
  %y3 = fmul double %y, 3.0
  %r = fsub double %y3, %x
  call i32 (ptr, ...) @printf(ptr @fmt.double, double %r)
  ; The promise back to its frame.
  %pb = call ptr @llvm.coro.promise(ptr @frame, i32 4, i1 false)
  %back = call ptr @llvm.coro.promise(ptr %pb, i32 4, i1 true)
  %pi = ptrtoint ptr %pb to i32
  %fi = ptrtoint ptr %back to i32
  %diff = sub i32 %pi, %fi
  call i32 (ptr, ...) @printf(ptr @fmt.diff, i32 %diff)
  ; An over-aligned promise.
  call void @llvm.coro.resume(ptr @frame16)
  %p16 = call ptr @llvm.coro.promise(ptr @frame16, i32 16, i1 false)
  %w = load i64, ptr %p16
  %p16i = ptrtoint ptr %p16 to i32
  %d16 = sub i32 %p16i, ptrtoint (ptr @frame16 to i32)
  call i32 (ptr, ...) @printf(ptr @fmt.wide, i64 %w, i32 %d16)
  ; The noop coroutine: never done, resumes and destroys as nothing.
  %n = call ptr @llvm.coro.noop()
  call void @llvm.coro.resume(ptr %n)
  call void @llvm.coro.destroy(ptr %n)
  %nd = call i1 @llvm.coro.done(ptr %n)
  %ndz = zext i1 %nd to i32
  %n2 = call ptr @llvm.coro.noop()
  %same = icmp eq ptr %n, %n2
  %samez = zext i1 %same to i32
  call i32 (ptr, ...) @printf(ptr @fmt.noop, i32 %ndz, i32 %samez)
  ; destroy, with a double live across it.
  %z = fdiv double %x, 7.0
  call void @llvm.coro.destroy(ptr @frame)
  %z7 = fmul double %z, 7.0
  %rz = fsub double %z7, %x
  call i32 (ptr, ...) @printf(ptr @fmt.double, double %rz)
  %pe = call ptr @llvm.coro.promise(ptr @frame, i32 4, i1 false)
  %e = load i32, ptr %pe
  %de = call i1 @llvm.coro.done(ptr @frame)
  %dez = zext i1 %de to i32
  call i32 (ptr, ...) @printf(ptr @fmt.count, i32 %e, i32 %dez)
  call i32 (ptr, ...) @printf(ptr @fmt.line)
  ret i32 0
}

declare i32 @printf(ptr, ...)
declare void @llvm.coro.resume(ptr)
declare void @llvm.coro.destroy(ptr)
declare i1 @llvm.coro.done(ptr)
declare ptr @llvm.coro.promise(ptr, i32, i1)
declare ptr @llvm.coro.noop()
