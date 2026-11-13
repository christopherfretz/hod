; llvm.coro.noop outside any coroutine: the address of the module's noop
; coroutine frame, the same from every function of the module, whose
; resume and destroy pointers are one function that returns. Resuming and
; destroying it does nothing, also through an invoke; it is never done.
;@ covers: spec/ir.md §5.5 intrinsics (coro.noop, coro.resume, coro.destroy, coro.done)
;@ input: pre-opt
;@ link: c++
;@ unsupported(x86_64): *
;@ stdout: same frame 1, done 0
;@ stdout: resume and destroy: one function 1, not null 1
;@ stdout: resumed, destroyed and invoked: still not done 0

@.same = private unnamed_addr constant [24 x i8] c"same frame %d, done %d\0A\00"
@.slots = private unnamed_addr constant [50 x i8] c"resume and destroy: one function %d, not null %d\0A\00"
@.after = private unnamed_addr constant [51 x i8] c"resumed, destroyed and invoked: still not done %d\0A\00"

declare i32 @printf(ptr, ...)
declare i32 @__gxx_personality_v0(...)
declare ptr @llvm.coro.noop()
declare void @llvm.coro.resume(ptr)
declare void @llvm.coro.destroy(ptr)
declare i1 @llvm.coro.done(ptr)

define internal ptr @noop_elsewhere() {
  %n = call ptr @llvm.coro.noop()
  ret ptr %n
}

define internal i32 @invoke_noop(ptr %h) personality ptr @__gxx_personality_v0 {
entry:
  invoke void @llvm.coro.resume(ptr %h) to label %next unwind label %pad
next:
  invoke void @llvm.coro.destroy(ptr %h) to label %ok unwind label %pad
ok:
  ret i32 0
pad:
  %lp = landingpad { ptr, i32 } cleanup
  ret i32 1
}

define i32 @main() {
  %a = call ptr @llvm.coro.noop()
  %b = call ptr @noop_elsewhere()
  %same = icmp eq ptr %a, %b
  %same.i = zext i1 %same to i32
  %d0 = call i1 @llvm.coro.done(ptr %a)
  %d0.i = zext i1 %d0 to i32
  call i32 (ptr, ...) @printf(ptr @.same, i32 %same.i, i32 %d0.i)
  %resume = load ptr, ptr %a
  %d.slot = getelementptr i8, ptr %a, i64 8
  %destroy = load ptr, ptr %d.slot
  %one = icmp eq ptr %resume, %destroy
  %one.i = zext i1 %one to i32
  %set = icmp ne ptr %resume, null
  %set.i = zext i1 %set to i32
  call i32 (ptr, ...) @printf(ptr @.slots, i32 %one.i, i32 %set.i)
  call void @llvm.coro.resume(ptr %a)
  call void @llvm.coro.destroy(ptr %b)
  %unwound = call i32 @invoke_noop(ptr %a)
  %d1 = call i1 @llvm.coro.done(ptr %b)
  %d1.i = zext i1 %d1 to i32
  %r = or i32 %d1.i, %unwound
  call i32 (ptr, ...) @printf(ptr @.after, i32 %r)
  ret i32 0
}
