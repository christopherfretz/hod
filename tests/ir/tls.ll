; Thread-local storage: initialized and zero-initialized variables, each
; TLS model, access directly and through llvm.threadlocal.address, a
; thread-local array, and two threads that each see fresh copies at
; addresses different from the main thread's.
;@ covers: spec/ir.md §2.2 symbols (thread-local model), §5.5 intrinsics (threadlocal.address)
;@ stdout: main: counter=7 zero=0 ie=30 le=40 ld=50 array=10
;@ stdout: thread 1: sum=237, own copy 1
;@ stdout: thread 2: sum=337, own copy 1
;@ stdout: after: counter=18 zero=5 ie=31 le=41 ld=51 array=16

@.main = private unnamed_addr constant [55 x i8] c"main: counter=%d zero=%lld ie=%d le=%d ld=%d array=%d\0A\00"
@.thread = private unnamed_addr constant [34 x i8] c"thread %d: sum=%lld, own copy %d\0A\00"
@.after = private unnamed_addr constant [56 x i8] c"after: counter=%d zero=%lld ie=%d le=%d ld=%d array=%d\0A\00"

@counter = thread_local global i32 7
@zero = thread_local global i64 0
@ie = thread_local(initialexec) global i32 30
@le = thread_local(localexec) global i32 40
@ld = internal thread_local(localdynamic) global i32 50
@array = thread_local global [4 x i16] [i16 1, i16 2, i16 3, i16 4], align 16
@main_counter_address = global ptr null

declare i32 @printf(ptr, ...)
declare i32 @pthread_create(ptr, ptr, ptr, ptr)
declare i32 @pthread_join(i64, ptr)
declare ptr @llvm.threadlocal.address.p0(ptr)

define i32 @bump(i32 %by) {
  %p = call ptr @llvm.threadlocal.address.p0(ptr @counter)
  %old = load i32, ptr %p
  %new = add i32 %old, %by
  store i32 %new, ptr %p
  ret i32 %new
}

define i32 @array_sum() {
entry:
  br label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %i.next, %loop ]
  %acc = phi i32 [ 0, %entry ], [ %acc.next, %loop ]
  %at = getelementptr inbounds [4 x i16], ptr @array, i64 0, i64 %i
  %v = load i16, ptr %at
  %v.w = sext i16 %v to i32
  %acc.next = add i32 %acc, %v.w
  %i.next = add i64 %i, 1
  %done = icmp eq i64 %i.next, 4
  br i1 %done, label %exit, label %loop
exit:
  ret i32 %acc.next
}

; Adds its argument to this thread's counter, then returns the sum of all
; of this thread's variables, with bit 32 set if its counter is at a
; different address from the main thread's.
define ptr @worker(ptr %arg) {
  %by.w = ptrtoint ptr %arg to i64
  %by = trunc i64 %by.w to i32
  %c = call i32 @bump(i32 %by)
  %z = load i64, ptr @zero
  %i = load i32, ptr @ie
  %l = load i32, ptr @le
  %d = load i32, ptr @ld
  %a = call i32 @array_sum()
  store i16 -100, ptr @array
  store i64 -1, ptr @zero
  %s0 = add i32 %c, %i
  %s1 = add i32 %s0, %l
  %s2 = add i32 %s1, %d
  %s3 = add i32 %s2, %a
  %s3.w = sext i32 %s3 to i64
  %sum = add i64 %s3.w, %z
  %mine = call ptr @llvm.threadlocal.address.p0(ptr @counter)
  %mains = load ptr, ptr @main_counter_address
  %own = icmp ne ptr %mine, %mains
  %own.w = zext i1 %own to i64
  %flag = shl i64 %own.w, 32
  %result = or i64 %sum, %flag
  %r = inttoptr i64 %result to ptr
  ret ptr %r
}

define void @run_thread(i32 %number, i64 %arg) {
  %id = alloca i64
  %out = alloca ptr
  %arg.p = inttoptr i64 %arg to ptr
  call i32 @pthread_create(ptr %id, ptr null, ptr @worker, ptr %arg.p)
  %tid = load i64, ptr %id
  call i32 @pthread_join(i64 %tid, ptr %out)
  %r = load ptr, ptr %out
  %bits = ptrtoint ptr %r to i64
  %sum = and i64 %bits, 4294967295
  %own = lshr i64 %bits, 32
  %own.i = trunc i64 %own to i32
  call i32 (ptr, ...) @printf(ptr @.thread, i32 %number, i64 %sum, i32 %own.i)
  ret void
}

define void @show(ptr %format) {
  %c = load i32, ptr @counter
  %z = load i64, ptr @zero
  %i = load i32, ptr @ie
  %l = load i32, ptr @le
  %d = load i32, ptr @ld
  %a = call i32 @array_sum()
  call i32 (ptr, ...) @printf(ptr %format, i32 %c, i64 %z, i32 %i, i32 %l, i32 %d, i32 %a)
  ret void
}

define i32 @main() {
  %mine = call ptr @llvm.threadlocal.address.p0(ptr @counter)
  store ptr %mine, ptr @main_counter_address
  call void @show(ptr @.main)
  call i32 @bump(i32 1)
  call i32 @bump(i32 10)
  store i64 5, ptr @zero
  store i32 31, ptr @ie
  store i32 41, ptr @le
  store i32 51, ptr @ld
  store i16 10, ptr getelementptr inbounds ([4 x i16], ptr @array, i64 0, i64 3)
  call void @run_thread(i32 1, i64 100)
  call void @run_thread(i32 2, i64 200)
  call void @show(ptr @.after)
  ret i32 0
}
