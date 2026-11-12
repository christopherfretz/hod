; The library module of tests/tls.rs (WP-073): thread-local variables of
; every model a shared object may hold, reached as direct operands, as
; constant offsets, through llvm.threadlocal.address, in a phi, a select
; and call arguments, and with a value held in x0 across the access.
; Built as a shared object (PIC: TLS descriptors), or into the executable
; (static or PIE), by hod and by llc.

@lib_gd = thread_local global i32 1, align 4
@lib_local = dso_local thread_local global i32 2, align 4
@lib_ld = internal thread_local(localdynamic) global i32 3, align 4
@lib_ie = thread_local(initialexec) global i32 4, align 4
@lib_arr = thread_local global [64 x i64] zeroinitializer, align 8
@lib_big = internal thread_local global [2000000 x i8] zeroinitializer, align 1
@exe_tls = external thread_local global i32, align 4
@dso_tls = external thread_local global i32, align 4

declare i32 @check_args(i32, ptr, ptr, i64, ptr, ptr)
declare i64 @ident(i64)
declare ptr @llvm.threadlocal.address.p0(ptr)

; Every variable, read directly, through the intrinsic and at constant
; offsets (the last element of each array).
define i64 @lib_sum() {
  %a = load i32, ptr @lib_gd, align 4
  %b = load i32, ptr @lib_local, align 4
  %p = call ptr @llvm.threadlocal.address.p0(ptr @lib_ld)
  %c = load i32, ptr %p, align 4
  %d = load i32, ptr @lib_ie, align 4
  %e = load i64, ptr getelementptr inbounds ([64 x i64], ptr @lib_arr, i64 0, i64 63), align 8
  %f = load i8, ptr getelementptr inbounds ([2000000 x i8], ptr @lib_big, i64 0, i64 1999999), align 1
  %g = load i32, ptr @exe_tls, align 4
  %h = load i32, ptr @dso_tls, align 4
  %a1 = sext i32 %a to i64
  %b1 = sext i32 %b to i64
  %c1 = sext i32 %c to i64
  %d1 = sext i32 %d to i64
  %f1 = zext i8 %f to i64
  %g1 = sext i32 %g to i64
  %h1 = sext i32 %h to i64
  %s1 = add i64 %a1, %b1
  %s2 = add i64 %s1, %c1
  %s3 = add i64 %s2, %d1
  %s4 = add i64 %s3, %e
  %s5 = add i64 %s4, %f1
  %s6 = add i64 %s5, %g1
  %s7 = add i64 %s6, %h1
  ret i64 %s7
}

; Adds %k to every variable of this thread.
define void @lib_bump(i32 %k) {
  %a = load i32, ptr @lib_gd, align 4
  %a2 = add i32 %a, %k
  store i32 %a2, ptr @lib_gd, align 4
  %b = load i32, ptr @lib_local, align 4
  %b2 = add i32 %b, %k
  store i32 %b2, ptr @lib_local, align 4
  %c = load i32, ptr @lib_ld, align 4
  %c2 = add i32 %c, %k
  store i32 %c2, ptr @lib_ld, align 4
  %p = call ptr @llvm.threadlocal.address.p0(ptr @lib_ie)
  %d = load i32, ptr %p, align 4
  %d2 = add i32 %d, %k
  store i32 %d2, ptr %p, align 4
  %k64 = sext i32 %k to i64
  %e = load i64, ptr getelementptr inbounds ([64 x i64], ptr @lib_arr, i64 0, i64 63), align 8
  %e2 = add i64 %e, %k64
  store i64 %e2, ptr getelementptr inbounds ([64 x i64], ptr @lib_arr, i64 0, i64 63), align 8
  %k8 = trunc i32 %k to i8
  %f = load i8, ptr getelementptr inbounds ([2000000 x i8], ptr @lib_big, i64 0, i64 1999999), align 1
  %f2 = add i8 %f, %k8
  store i8 %f2, ptr getelementptr inbounds ([2000000 x i8], ptr @lib_big, i64 0, i64 1999999), align 1
  %g = load i32, ptr @exe_tls, align 4
  %g2 = add i32 %g, %k
  store i32 %g2, ptr @exe_tls, align 4
  %h = load i32, ptr @dso_tls, align 4
  %h2 = add i32 %h, %k
  store i32 %h2, ptr @dso_tls, align 4
  ret void
}

; The address of variable %which, through a phi of thread-local
; addresses; null for an unknown one.
define ptr @lib_addr(i32 %which) {
entry:
  switch i32 %which, label %none [
    i32 0, label %join
    i32 1, label %one
    i32 2, label %two
    i32 3, label %three
    i32 4, label %four
    i32 5, label %five
    i32 6, label %six
    i32 7, label %seven
  ]
one:
  br label %join
two:
  br label %join
three:
  br label %join
four:
  br label %join
five:
  br label %join
six:
  br label %join
seven:
  br label %join
none:
  br label %join
join:
  %p = phi ptr [ @lib_gd, %entry ], [ @lib_local, %one ], [ @lib_ld, %two ], [ @lib_ie, %three ], [ getelementptr inbounds ([64 x i64], ptr @lib_arr, i64 0, i64 1), %four ], [ @exe_tls, %five ], [ @dso_tls, %six ], [ getelementptr inbounds ([2000000 x i8], ptr @lib_big, i64 0, i64 1999999), %seven ], [ null, %none ]
  ret ptr %p
}

; A select between two thread-local addresses under a compare fused into
; it: the flags live across both address sequences.
define ptr @lib_select(i32 %a, i32 %b) {
  %c = icmp slt i32 %a, %b
  %p = select i1 %c, ptr @lib_gd, ptr @dso_tls
  ret ptr %p
}

; Thread-local addresses as call arguments, after and between others:
; x0 holds the first argument while the next ones are formed.
define i32 @lib_args() {
  %r = call i32 @check_args(i32 7, ptr @lib_gd, ptr @exe_tls, i64 9, ptr getelementptr inbounds ([64 x i64], ptr @lib_arr, i64 0, i64 1), ptr @dso_tls)
  ret i32 %r
}

; A call's result held in x0 while a thread-local variable is read.
define i64 @lib_keep(i64 %x) {
  %r = call i64 @ident(i64 %x)
  %v = load i32, ptr @dso_tls, align 4
  %v1 = sext i32 %v to i64
  %s = add i64 %r, %v1
  ret i64 %s
}

; Offsets far beyond the variable (never dereferenced): the difference
; from its address is the offset.
define i64 @lib_far() {
  %base = call ptr @llvm.threadlocal.address.p0(ptr @lib_arr)
  %b = ptrtoint ptr %base to i64
  %up = ptrtoint ptr getelementptr (i8, ptr @lib_arr, i64 1099511627776) to i64
  %down = ptrtoint ptr getelementptr (i8, ptr @lib_arr, i64 -1099511627776) to i64
  %d1 = sub i64 %up, %b
  %d2 = sub i64 %b, %down
  %d = add i64 %d1, %d2
  ret i64 %d
}

; The thread pointer.
define ptr @lib_tp() {
  %tp = call ptr @llvm.thread.pointer()
  ret ptr %tp
}
declare ptr @llvm.thread.pointer()
