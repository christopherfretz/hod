; Values live across suspends, homed in the heap frame. @f: the loop of
; LLVM's coroutine documentation, with %i, %k and %prod defined before a
; suspend and used after it. @g: the resume edge of the suspend switch is
; the loop's back edge, with an incoming value for the heap phi %j. @e:
; a slot whose address escapes before the suspend and that is read only
; through that address after it (design 0005 rule 5). main overwrites
; the dead stack before each resume, so a value left on the machine stack
; is not found again by chance. The older form without await_suspend
; wrappers: the suspended region starts at coro.save. In LLVM 19's (coro.end returns i1) dialect.
;@ covers: spec/ir.md §5.5 intrinsics (coroutine bodies: heap values; spec/codegen.md §7.6)
;@ input: pre-opt
;@ dialect: 19
;@ unsupported(x86_64): *
;@ stdout: 4 160
;@ stdout: 5 200
;@ stdout: 6 240
;@ stdout: done
;@ stdout: 7 70
;@ stdout: 8 70
;@ stdout: 8 70
;@ stdout: 8 70
;@ stdout: 3 -1

@.fmt = private unnamed_addr constant [7 x i8] c"%d %d\0A\00"
@.bye = private unnamed_addr constant [6 x i8] c"done\0A\00"
@reg = global ptr null

declare i32 @printf(ptr, ...)
declare ptr @malloc(i64)
declare void @free(ptr)
declare token @llvm.coro.id(i32, ptr, ptr, ptr)
declare i64 @llvm.coro.size.i64()
declare ptr @llvm.coro.begin(token, ptr)
declare token @llvm.coro.save(ptr)
declare i8 @llvm.coro.suspend(token, i1)
declare ptr @llvm.coro.free(token, ptr)
declare i1 @llvm.coro.end(ptr, i1, token)
declare void @llvm.coro.resume(ptr)
declare void @llvm.coro.destroy(ptr)
declare void @llvm.memset.p0.i64(ptr, i8, i64, i1)

define ptr @f(i32 %n) presplitcoroutine {
entry:
  %id = call token @llvm.coro.id(i32 0, ptr null, ptr null, ptr null)
  %size = call i64 @llvm.coro.size.i64()
  %alloc = call ptr @malloc(i64 %size)
  %hdl = call ptr @llvm.coro.begin(token %id, ptr %alloc)
  %k = mul i32 %n, 10
  br label %loop

loop:
  %i = phi i32 [ %n, %entry ], [ %inc, %resume ]
  %prod = mul i32 %i, %k
  %save = call token @llvm.coro.save(ptr %hdl)
  %s = call i8 @llvm.coro.suspend(token %save, i1 false)
  switch i8 %s, label %suspend [
    i8 0, label %resume
    i8 1, label %cleanup
  ]

resume:
  %p = call i32 (ptr, ...) @printf(ptr @.fmt, i32 %i, i32 %prod)
  %inc = add i32 %i, 1
  br label %loop

cleanup:
  %q = call i32 (ptr, ...) @printf(ptr @.bye)
  %mem = call ptr @llvm.coro.free(token %id, ptr %hdl)
  call void @free(ptr %mem)
  br label %suspend

suspend:
  %unused = call i1 @llvm.coro.end(ptr %hdl, i1 false, token none)
  ret ptr %hdl
}

; The documentation's own shape: the resume edge of the suspend switch
; is a back edge, so the phi %j (a heap value: cleanup reads it) has its
; incoming value on that edge.
define ptr @g(i32 %n) presplitcoroutine {
entry:
  %id = call token @llvm.coro.id(i32 0, ptr null, ptr null, ptr null)
  %size = call i64 @llvm.coro.size.i64()
  %alloc = call ptr @malloc(i64 %size)
  %hdl = call ptr @llvm.coro.begin(token %id, ptr %alloc)
  %k = mul i32 %n, 10
  br label %loop

loop:
  %j = phi i32 [ %n, %entry ], [ 8, %loop ]
  %p = call i32 (ptr, ...) @printf(ptr @.fmt, i32 %j, i32 %k)
  %save = call token @llvm.coro.save(ptr %hdl)
  %s = call i8 @llvm.coro.suspend(token %save, i1 false)
  switch i8 %s, label %suspend [
    i8 0, label %loop
    i8 1, label %cleanup
  ]

cleanup:
  %q = call i32 (ptr, ...) @printf(ptr @.fmt, i32 %j, i32 %k)
  %mem = call ptr @llvm.coro.free(token %id, ptr %hdl)
  call void @free(ptr %mem)
  br label %suspend

suspend:
  %unused = call i1 @llvm.coro.end(ptr %hdl, i1 false, token none)
  ret ptr %hdl
}

; Design 0005 rule 5: %x escapes (its address is stored to @reg) before
; the suspend and has no use of its own after it; it is read through
; @reg there, so its home is the heap frame too.
define ptr @e(i32 %n) presplitcoroutine {
entry:
  %x = alloca i32, align 4
  %id = call token @llvm.coro.id(i32 0, ptr null, ptr null, ptr null)
  %size = call i64 @llvm.coro.size.i64()
  %alloc = call ptr @malloc(i64 %size)
  %hdl = call ptr @llvm.coro.begin(token %id, ptr %alloc)
  store i32 %n, ptr %x, align 4
  store ptr %x, ptr @reg, align 8
  %save = call token @llvm.coro.save(ptr %hdl)
  %s = call i8 @llvm.coro.suspend(token %save, i1 false)
  switch i8 %s, label %suspend [
    i8 0, label %resume
    i8 1, label %cleanup
  ]

resume:
  %r = load ptr, ptr @reg, align 8
  %v = load i32, ptr %r, align 4
  %p = call i32 (ptr, ...) @printf(ptr @.fmt, i32 %v, i32 -1)
  br label %cleanup

cleanup:
  %mem = call ptr @llvm.coro.free(token %id, ptr %hdl)
  call void @free(ptr %mem)
  br label %suspend

suspend:
  %unused = call i1 @llvm.coro.end(ptr %hdl, i1 false, token none)
  ret ptr %hdl
}

; Overwrites the stack where the ramp's machine frame was.
define void @scrub() {
  %junk = alloca [512 x i8], align 16
  call void @llvm.memset.p0.i64(ptr %junk, i8 90, i64 512, i1 true)
  ret void
}

define i32 @main() {
  %h = call ptr @f(i32 4)
  call void @scrub()
  call void @llvm.coro.resume(ptr %h)
  call void @scrub()
  call void @llvm.coro.resume(ptr %h)
  call void @scrub()
  call void @llvm.coro.resume(ptr %h)
  call void @llvm.coro.destroy(ptr %h)
  %h2 = call ptr @g(i32 7)
  call void @scrub()
  call void @llvm.coro.resume(ptr %h2)
  call void @scrub()
  call void @llvm.coro.resume(ptr %h2)
  call void @llvm.coro.destroy(ptr %h2)
  %h3 = call ptr @e(i32 3)
  call void @scrub()
  call void @llvm.coro.resume(ptr %h3)
  ret i32 0
}
