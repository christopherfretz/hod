; A program that ends by a signal: it prints, flushes, and calls abort
; through a noreturn function, so the expected status is SIGABRT. The
; blocks after the noreturn call end in unreachable.
;@ covers: spec/ir.md §5.4 terminator (Unreachable), §5.7 attributes (noreturn)
;@ exit: signal 6
;@ unsupported(x86_64): *
;@ stdout: checking 1
;@ stdout: checking 100
;@ stdout: checking 101
;@ stdout: fatal: 101 is out of range

@.before = private unnamed_addr constant [13 x i8] c"checking %d\0A\00"
@.fatal = private unnamed_addr constant [27 x i8] c"fatal: %d is out of range\0A\00"
@.after = private unnamed_addr constant [13 x i8] c"not reached\0A\00"

declare i32 @printf(ptr, ...)
declare i32 @fflush(ptr)
declare void @abort() noreturn

define void @fail(i32 %v) noreturn {
  call i32 (ptr, ...) @printf(ptr @.fatal, i32 %v)
  call i32 @fflush(ptr null)
  call void @abort()
  unreachable
}

define void @check(i32 %v) {
entry:
  call i32 (ptr, ...) @printf(ptr @.before, i32 %v)
  %bad = icmp sgt i32 %v, 100
  br i1 %bad, label %failed, label %ok
failed:
  call void @fail(i32 %v)
  unreachable
ok:
  ret void
}

define i32 @main() {
  call void @check(i32 1)
  call void @check(i32 100)
  call void @check(i32 101)
  call i32 (ptr, ...) @printf(ptr @.after)
  ret i32 0
}
