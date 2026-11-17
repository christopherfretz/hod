; A protected coroutine whose ramp uses a stack-passed argument after it
; wrote a buffer of its own (`spec/codegen.md` §5.6.24 rule 10, WP-343):
; `@victim`'s ninth argument `%dst` arrives on the stack on x86-64 and on
; AArch64. The ramp fills `%buf` (a machine-stack slot: its lifetime
; holds no suspend) with `%n` bytes, then stores through `%dst`, then
; suspends; the resumed activation stores again and finishes. With one
; argument `main` writes 16 bytes (in bounds); with two it writes 200,
; past the guard, the saved registers and the return address into the
; incoming argument area: the ramp must end in `__stack_chk_fail` (the
; abort of "stack smashing detected"), never in a store through an
; overwritten `%dst`. The test prepends the target lines, writes the
; protector attribute in place of `ATTR`, rewrites `llvm.coro.end` and
; the lifetime markers into LLVM 19's dialect for LLVM 19 and appends
; the PIC and PIE levels for a PIE.

declare token @llvm.coro.id(i32, ptr, ptr, ptr)
declare i64 @llvm.coro.size.i64()
declare ptr @llvm.coro.begin(token, ptr)
declare token @llvm.coro.save(ptr)
declare i8 @llvm.coro.suspend(token, i1)
declare ptr @llvm.coro.free(token, ptr)
declare void @llvm.coro.end(ptr, i1, token)
declare void @llvm.coro.resume(ptr)
declare void @llvm.coro.destroy(ptr)
declare void @llvm.lifetime.start.p0(ptr)
declare void @llvm.lifetime.end.p0(ptr)
declare void @llvm.memset.p0.i64(ptr, i8, i64, i1)
declare ptr @malloc(i64)
declare void @free(ptr)
declare i32 @printf(ptr, ...)
declare i32 @fflush(ptr)

@wrote = private unnamed_addr constant [20 x i8] c"ramp wrote %d (%c)\0A\00"
@stored = private unnamed_addr constant [13 x i8] c"stored %lld\0A\00"
@resumed = private unnamed_addr constant [18 x i8] c"resumed %lld %ld\0A\00"

define ptr @victim(i32 %n, i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6, i64 %a7, ptr %dst) presplitcoroutine ATTR {
entry:
  %buf = alloca [16 x i8], align 16
  %id = call token @llvm.coro.id(i32 16, ptr null, ptr null, ptr null)
  %size = call i64 @llvm.coro.size.i64()
  %mem = call ptr @malloc(i64 %size)
  %h = call ptr @llvm.coro.begin(token %id, ptr %mem)
  call void @llvm.lifetime.start.p0(ptr %buf)
  %len = zext i32 %n to i64
  call void @llvm.memset.p0.i64(ptr %buf, i8 65, i64 %len, i1 false)
  %at3 = getelementptr inbounds i8, ptr %buf, i64 3
  %c = load i8, ptr %at3
  %ci = zext i8 %c to i32
  %p = call i32 (ptr, ...) @printf(ptr @wrote, i32 %n, i32 %ci)
  %fl = call i32 @fflush(ptr null)
  %s1 = add i64 %a1, %a2
  %s2 = add i64 %s1, %a3
  %s3 = add i64 %s2, %a4
  %s4 = add i64 %s3, %a5
  %s5 = add i64 %s4, %a6
  %s6 = add i64 %s5, %a7
  store i64 %s6, ptr %dst
  call void @llvm.lifetime.end.p0(ptr %buf)
  %save = call token @llvm.coro.save(ptr null)
  %s = call i8 @llvm.coro.suspend(token %save, i1 false)
  switch i8 %s, label %out [
    i8 0, label %again
    i8 1, label %cleanup
  ]
again:
  %q = call i32 (ptr, ...) @printf(ptr @resumed, i64 7, i64 7)
  %save2 = call token @llvm.coro.save(ptr null)
  %s2b = call i8 @llvm.coro.suspend(token %save2, i1 true)
  switch i8 %s2b, label %out [
    i8 0, label %cleanup
    i8 1, label %cleanup
  ]
cleanup:
  %f = call ptr @llvm.coro.free(token %id, ptr %h)
  call void @free(ptr %f)
  br label %out
out:
  call void @llvm.coro.end(ptr null, i1 false, token none)
  ret ptr %h
}

define i32 @main(i32 %argc, ptr %argv) {
entry:
  %out = alloca i64, align 8
  store i64 0, ptr %out
  %big = icmp sgt i32 %argc, 1
  %n = select i1 %big, i32 200, i32 16
  %h = call ptr @victim(i32 %n, i64 1, i64 2, i64 3, i64 4, i64 5, i64 6, i64 7, ptr %out)
  %v = load i64, ptr %out
  %p = call i32 (ptr, ...) @printf(ptr @stored, i64 %v)
  call void @llvm.coro.resume(ptr %h)
  call void @llvm.coro.destroy(ptr %h)
  ret i32 0
}
