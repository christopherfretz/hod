; SSA values live across suspends: the loop of LLVM's coroutine
; documentation, with phis %i, %k and %prod defined before a suspend and
; used after it, so their home is the heap frame. The older form
; without await_suspend wrappers: the suspended region starts at
; coro.save. In LLVM 19's (coro.end returns i1) dialect.
;@ covers: spec/ir.md §5.5 intrinsics (coroutine bodies: heap values; spec/codegen.md §7.6)
;@ input: pre-opt
;@ dialect: 19
;@ unsupported(x86_64): *
;@ stdout: 4 160
;@ stdout: 5 200
;@ stdout: 6 240
;@ stdout: done

@.fmt = private unnamed_addr constant [7 x i8] c"%d %d\0A\00"
@.bye = private unnamed_addr constant [6 x i8] c"done\0A\00"

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

define i32 @main() {
  %h = call ptr @f(i32 4)
  call void @llvm.coro.resume(ptr %h)
  call void @llvm.coro.resume(ptr %h)
  call void @llvm.coro.resume(ptr %h)
  call void @llvm.coro.destroy(ptr %h)
  ret i32 0
}
