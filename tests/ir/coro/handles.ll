; Coroutine handle operations outside any coroutine, on frames built by
; hand: a frame is a block whose first two words are the addresses of a
; resume and a destroy function (plain fastcc functions here), followed
; by the promise at alignTo(16, align). llvm.coro.resume and destroy (as
; calls and as invokes, one of which unwinds), llvm.coro.done (the resume
; pointer is null) and llvm.coro.promise in both directions, with a
; promise aligned to 8 and one aligned to 32. What opt's coroutine passes
; make of these is what hod must do without them.
;@ covers: spec/ir.md §5.5 intrinsics (coro.resume, coro.destroy, coro.done, coro.promise)
;@ input: pre-opt
;@ link: c++
;@ unsupported(x86_64): *
;@ stdout: small: promise +16 back 1, done 0
;@ stdout: resumed 1 2 3, done 1
;@ stdout: destroyed 1
;@ stdout: wide: promise +32 back 1, done 0
;@ stdout: resumed 10 20 30, done 1
;@ stdout: destroyed 1
;@ stdout: invoke: normal 1, caught 42, done 1
;@ stdout: destroy through invoke: 1

@_ZTIi = external constant ptr
@.small = private unnamed_addr constant [38 x i8] c"small: promise +%ld back %d, done %d\0A\00"
@.wide = private unnamed_addr constant [37 x i8] c"wide: promise +%ld back %d, done %d\0A\00"
@.resumed = private unnamed_addr constant [30 x i8] c"resumed %ld %ld %ld, done %d\0A\00"
@.destroyed = private unnamed_addr constant [14 x i8] c"destroyed %d\0A\00"
@.invoke = private unnamed_addr constant [39 x i8] c"invoke: normal %d, caught %d, done %d\0A\00"
@.dinvoke = private unnamed_addr constant [28 x i8] c"destroy through invoke: %d\0A\00"
@destroyed = global i32 0

declare i32 @printf(ptr, ...)
declare ptr @__cxa_allocate_exception(i64)
declare void @__cxa_throw(ptr, ptr, ptr) noreturn
declare ptr @__cxa_begin_catch(ptr)
declare void @__cxa_end_catch()
declare i32 @__gxx_personality_v0(...)
declare void @llvm.coro.resume(ptr)
declare void @llvm.coro.destroy(ptr)
declare i1 @llvm.coro.done(ptr)
declare ptr @llvm.coro.promise(ptr, i32, i1)

; Adds `step` to the i64 promise at `offset`; the third resume is the
; last: it clears the resume pointer, as a coroutine at its final
; suspend point has.
define internal fastcc void @resume_at(ptr %frame, i64 %offset) {
entry:
  %p = getelementptr i8, ptr %frame, i64 %offset
  %v = load i64, ptr %p
  %next = add i64 %v, 1
  store i64 %next, ptr %p
  %last = icmp eq i64 %next, 3
  br i1 %last, label %final, label %done
final:
  store ptr null, ptr %frame
  br label %done
done:
  ret void
}

define internal fastcc void @resume_small(ptr %frame) {
  call fastcc void @resume_at(ptr %frame, i64 16)
  ret void
}

define internal fastcc void @resume_wide(ptr %frame) {
  call fastcc void @resume_at(ptr %frame, i64 32)
  ret void
}

define internal fastcc void @resume_throws(ptr %frame) {
  store ptr null, ptr %frame
  %e = call ptr @__cxa_allocate_exception(i64 4)
  store i32 42, ptr %e
  call void @__cxa_throw(ptr %e, ptr @_ZTIi, ptr null)
  unreachable
}

define internal fastcc void @resume_quiet(ptr %frame) {
  %slot = getelementptr i8, ptr %frame, i64 16
  store i64 1, ptr %slot
  store ptr @resume_throws, ptr %frame
  ret void
}

define internal fastcc void @destroy(ptr %frame) {
  %d = load i32, ptr @destroyed
  %n = add i32 %d, 1
  store i32 %n, ptr @destroyed
  ret void
}

; Runs one frame: `scale` is what the promise is printed multiplied by.
define internal void @run(ptr %frame, ptr %resume, i32 %align, ptr %format, i64 %scale) {
entry:
  store ptr %resume, ptr %frame
  %d.slot = getelementptr i8, ptr %frame, i64 8
  store ptr @destroy, ptr %d.slot
  store i32 0, ptr @destroyed
  br label %pick
pick:
  %is.wide = icmp eq i32 %align, 32
  br i1 %is.wide, label %wide, label %small
small:
  %ps = call ptr @llvm.coro.promise(ptr %frame, i32 8, i1 false)
  %bs = call ptr @llvm.coro.promise(ptr %ps, i32 8, i1 true)
  br label %have
wide:
  %pw = call ptr @llvm.coro.promise(ptr %frame, i32 32, i1 false)
  %bw = call ptr @llvm.coro.promise(ptr %pw, i32 32, i1 true)
  br label %have
have:
  %promise = phi ptr [ %ps, %small ], [ %pw, %wide ]
  %back = phi ptr [ %bs, %small ], [ %bw, %wide ]
  store i64 0, ptr %promise
  %a = ptrtoint ptr %promise to i64
  %f = ptrtoint ptr %frame to i64
  %offset = sub i64 %a, %f
  %same = icmp eq ptr %back, %frame
  %same.i = zext i1 %same to i32
  %d0 = call i1 @llvm.coro.done(ptr %frame)
  %d0.i = zext i1 %d0 to i32
  call i32 (ptr, ...) @printf(ptr %format, i64 %offset, i32 %same.i, i32 %d0.i)
  call void @llvm.coro.resume(ptr %frame)
  %v1 = load i64, ptr %promise
  call void @llvm.coro.resume(ptr %frame)
  %v2 = load i64, ptr %promise
  call void @llvm.coro.resume(ptr %frame)
  %v3 = load i64, ptr %promise
  %d1 = call i1 @llvm.coro.done(ptr %frame)
  %d1.i = zext i1 %d1 to i32
  %s1 = mul i64 %v1, %scale
  %s2 = mul i64 %v2, %scale
  %s3 = mul i64 %v3, %scale
  call i32 (ptr, ...) @printf(ptr @.resumed, i64 %s1, i64 %s2, i64 %s3, i32 %d1.i)
  call void @llvm.coro.destroy(ptr %frame)
  %n = load i32, ptr @destroyed
  call i32 (ptr, ...) @printf(ptr @.destroyed, i32 %n)
  ret void
}

; resume through invoke: once normally, once unwinding into the handler.
define internal void @invokes(ptr %frame) personality ptr @__gxx_personality_v0 {
entry:
  store ptr @resume_quiet, ptr %frame
  %d.slot = getelementptr i8, ptr %frame, i64 8
  store ptr @destroy, ptr %d.slot
  %slot = getelementptr i8, ptr %frame, i64 16
  store i64 0, ptr %slot
  invoke void @llvm.coro.resume(ptr %frame) to label %again unwind label %unexpected
again:
  %normal = load i64, ptr %slot
  %normal.i = trunc i64 %normal to i32
  invoke void @llvm.coro.resume(ptr %frame) to label %unexpected.return unwind label %pad
pad:
  %lp = landingpad { ptr, i32 } catch ptr @_ZTIi
  %e = extractvalue { ptr, i32 } %lp, 0
  %x = call ptr @__cxa_begin_catch(ptr %e)
  %caught = load i32, ptr %x
  call void @__cxa_end_catch()
  %d = call i1 @llvm.coro.done(ptr %frame)
  %d.i = zext i1 %d to i32
  call i32 (ptr, ...) @printf(ptr @.invoke, i32 %normal.i, i32 %caught, i32 %d.i)
  store i32 0, ptr @destroyed
  invoke void @llvm.coro.destroy(ptr %frame) to label %destroyed unwind label %unexpected
destroyed:
  %n = load i32, ptr @destroyed
  call i32 (ptr, ...) @printf(ptr @.dinvoke, i32 %n)
  ret void
unexpected.return:
  ret void
unexpected:
  %lp2 = landingpad { ptr, i32 } cleanup
  resume { ptr, i32 } %lp2
}

define i32 @main() {
  %small = alloca [4 x i64], align 16
  %wide = alloca [6 x i64], align 32
  call void @run(ptr %small, ptr @resume_small, i32 8, ptr @.small, i64 1)
  call void @run(ptr %wide, ptr @resume_wide, i32 32, ptr @.wide, i64 10)
  call void @invokes(ptr %small)
  ret i32 0
}
