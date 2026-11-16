; WP-299: thread-local symbols as plain operands (no llvm.threadlocal.address):
; a select's arms after a compare, stored values, call arguments, a load and
; a store straight through the symbol. In a shared object their model is
; general dynamic (@op_gd) or local dynamic (@op_ld, internal): hod's operand
; form saves what the ___tls_get_addr call clobbers. @op_intr forms the same
; addresses through the intrinsic, for the driver to compare.
target datalayout = "e-m:e-p:32:32-p270:32:32-p271:32:32-p272:64:64-i128:128-f64:32:64-f80:32-n8:16:32-S128"
target triple = "i686-unknown-linux-gnu"

@op_gd = thread_local global i32 21
@op_ld = internal thread_local global [4 x i32] [i32 1, i32 2, i32 3, i32 4]

declare ptr @llvm.threadlocal.address.p0(ptr)

define ptr @op_intr(i32 %w) {
  %c = icmp eq i32 %w, 0
  br i1 %c, label %a, label %b
a:
  %p = call ptr @llvm.threadlocal.address.p0(ptr @op_gd)
  ret ptr %p
b:
  %q = call ptr @llvm.threadlocal.address.p0(ptr @op_ld)
  %r = getelementptr i8, ptr %q, i32 8
  ret ptr %r
}

define ptr @op_select(i32 %w) {
  %c = icmp eq i32 %w, 0
  %p = select i1 %c, ptr @op_gd, ptr getelementptr (i8, ptr @op_ld, i32 8)
  ret ptr %p
}

define void @op_store(ptr %out) {
  store ptr @op_gd, ptr %out
  %o2 = getelementptr i8, ptr %out, i32 4
  store ptr getelementptr (i8, ptr @op_ld, i32 8), ptr %o2
  ret void
}

define internal i32 @op_take(ptr %a, ptr %b, i32 %k) {
  %x = load i32, ptr %a
  %y = load i32, ptr %b
  %s = add i32 %x, %y
  %t = add i32 %s, %k
  ret i32 %t
}

define i32 @op_pass(i32 %k) nounwind {
  %r = call i32 @op_take(ptr @op_gd, ptr getelementptr (i8, ptr @op_ld, i32 8), i32 %k)
  ret i32 %r
}

define i32 @op_load_bump(i32 %by) {
  %v = load i32, ptr @op_gd
  %n = add i32 %v, %by
  store i32 %n, ptr @op_gd
  %w = load i32, ptr getelementptr (i8, ptr @op_ld, i32 8)
  %m = add i32 %w, %by
  store i32 %m, ptr getelementptr (i8, ptr @op_ld, i32 8)
  %s = add i32 %n, %m
  ret i32 %s
}
