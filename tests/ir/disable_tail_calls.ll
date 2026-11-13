; "disable-tail-calls" on functions, as googletest's GTEST_NO_TAIL_CALL_
; (__attribute__((disable_tail_calls))) puts it there, with "true" and
; "false", with tail, plain, notail and musttail calls, on a caller and
; on a callee. llc's rule: in a function with "true" no call is a tail
; call except a musttail one; "false" is the default. hod makes no tail
; call but a musttail one in any function, so it agrees with llc wherever
; the program below looks.
;
; @probe tells whether it was entered by a tail call: its caller stores
; its own return address in @ra, and a tail call leaves that address as
; @probe's. Where llc makes a tail call that hod does not (a tail call in
; a function without the attribute, or with "false"), the program only
; uses the result and does not look.
;@ covers: spec/ir.md §5.7 attributes (disable-tail-calls), spec/codegen.md §7.1 tail kinds
;@ stdout: true tail 0 14
;@ stdout: true plain 0 16
;@ stdout: true notail 0 18
;@ stdout: true musttail 1 20
;@ stdout: false plain 0 22
;@ stdout: false notail 0 24
;@ stdout: false musttail 1 26
;@ stdout: false tail 28
;@ stdout: callee plain 0 30
;@ stdout: callee musttail 1 32
;@ stdout: callee tail 34
;@ stdout: none tail 36

@.fmt = private unnamed_addr constant [10 x i8] c"%s %s %d \00"
@.val = private unnamed_addr constant [4 x i8] c"%d\0A\00"
@.unseen = private unnamed_addr constant [10 x i8] c"%s %s %d\0A\00"
@s.true = private unnamed_addr constant [5 x i8] c"true\00"
@s.false = private unnamed_addr constant [6 x i8] c"false\00"
@s.callee = private unnamed_addr constant [7 x i8] c"callee\00"
@s.none = private unnamed_addr constant [5 x i8] c"none\00"
@s.tail = private unnamed_addr constant [5 x i8] c"tail\00"
@s.plain = private unnamed_addr constant [6 x i8] c"plain\00"
@s.notail = private unnamed_addr constant [7 x i8] c"notail\00"
@s.musttail = private unnamed_addr constant [9 x i8] c"musttail\00"
@ra = global ptr null
@seen = global i32 0

declare i32 @printf(ptr, ...)
declare ptr @llvm.returnaddress(i32 immarg)

; Records in @seen whether it was entered by a tail call; returns 2x.
define i32 @probe(i32 %x) noinline {
  %own = call ptr @llvm.returnaddress(i32 0)
  %caller = load ptr, ptr @ra
  %tail = icmp eq ptr %own, %caller
  %flag = zext i1 %tail to i32
  store i32 %flag, ptr @seen
  %r = shl i32 %x, 1
  ret i32 %r
}

; The same, with the attribute on the callee: it constrains the calls
; @probe_dis makes (none), not the calls to it.
define i32 @probe_dis(i32 %x) noinline #0 {
  %own = call ptr @llvm.returnaddress(i32 0)
  %caller = load ptr, ptr @ra
  %tail = icmp eq ptr %own, %caller
  %flag = zext i1 %tail to i32
  store i32 %flag, ptr @seen
  %r = shl i32 %x, 1
  ret i32 %r
}

define i32 @true_tail(i32 %x) noinline #0 {
  %own = call ptr @llvm.returnaddress(i32 0)
  store ptr %own, ptr @ra
  %r = tail call i32 @probe(i32 %x)
  ret i32 %r
}

define i32 @true_plain(i32 %x) noinline #0 {
  %own = call ptr @llvm.returnaddress(i32 0)
  store ptr %own, ptr @ra
  %r = call i32 @probe(i32 %x)
  ret i32 %r
}

define i32 @true_notail(i32 %x) noinline #0 {
  %own = call ptr @llvm.returnaddress(i32 0)
  store ptr %own, ptr @ra
  %r = notail call i32 @probe(i32 %x)
  ret i32 %r
}

define i32 @true_musttail(i32 %x) noinline #0 {
  %own = call ptr @llvm.returnaddress(i32 0)
  store ptr %own, ptr @ra
  %r = musttail call i32 @probe(i32 %x)
  ret i32 %r
}

define i32 @false_plain(i32 %x) noinline #1 {
  %own = call ptr @llvm.returnaddress(i32 0)
  store ptr %own, ptr @ra
  %r = call i32 @probe(i32 %x)
  ret i32 %r
}

define i32 @false_notail(i32 %x) noinline #1 {
  %own = call ptr @llvm.returnaddress(i32 0)
  store ptr %own, ptr @ra
  %r = notail call i32 @probe(i32 %x)
  ret i32 %r
}

define i32 @false_musttail(i32 %x) noinline #1 {
  %own = call ptr @llvm.returnaddress(i32 0)
  store ptr %own, ptr @ra
  %r = musttail call i32 @probe(i32 %x)
  ret i32 %r
}

define i32 @false_tail(i32 %x) noinline #1 {
  %r = tail call i32 @probe(i32 %x)
  ret i32 %r
}

define i32 @callee_plain(i32 %x) noinline {
  %own = call ptr @llvm.returnaddress(i32 0)
  store ptr %own, ptr @ra
  %r = call i32 @probe_dis(i32 %x)
  ret i32 %r
}

define i32 @callee_musttail(i32 %x) noinline {
  %own = call ptr @llvm.returnaddress(i32 0)
  store ptr %own, ptr @ra
  %r = musttail call i32 @probe_dis(i32 %x)
  ret i32 %r
}

define i32 @callee_tail(i32 %x) noinline {
  %r = tail call i32 @probe_dis(i32 %x)
  ret i32 %r
}

define i32 @none_tail(i32 %x) noinline {
  %r = tail call i32 @probe(i32 %x)
  ret i32 %r
}

; Prints "<a> <b> <seen> <value>" for a call that is looked at.
define void @report(ptr %a, ptr %b, i32 %v) noinline {
  %flag = load i32, ptr @seen
  call i32 (ptr, ...) @printf(ptr @.fmt, ptr %a, ptr %b, i32 %flag)
  call i32 (ptr, ...) @printf(ptr @.val, i32 %v)
  ret void
}

define i32 @main() {
  %v1 = call i32 @true_tail(i32 7)
  call void @report(ptr @s.true, ptr @s.tail, i32 %v1)
  %v2 = call i32 @true_plain(i32 8)
  call void @report(ptr @s.true, ptr @s.plain, i32 %v2)
  %v3 = call i32 @true_notail(i32 9)
  call void @report(ptr @s.true, ptr @s.notail, i32 %v3)
  %v4 = call i32 @true_musttail(i32 10)
  call void @report(ptr @s.true, ptr @s.musttail, i32 %v4)
  %v5 = call i32 @false_plain(i32 11)
  call void @report(ptr @s.false, ptr @s.plain, i32 %v5)
  %v6 = call i32 @false_notail(i32 12)
  call void @report(ptr @s.false, ptr @s.notail, i32 %v6)
  %v7 = call i32 @false_musttail(i32 13)
  call void @report(ptr @s.false, ptr @s.musttail, i32 %v7)
  %v8 = call i32 @false_tail(i32 14)
  call i32 (ptr, ...) @printf(ptr @.unseen, ptr @s.false, ptr @s.tail, i32 %v8)
  %v9 = call i32 @callee_plain(i32 15)
  call void @report(ptr @s.callee, ptr @s.plain, i32 %v9)
  %v10 = call i32 @callee_musttail(i32 16)
  call void @report(ptr @s.callee, ptr @s.musttail, i32 %v10)
  %v11 = call i32 @callee_tail(i32 17)
  call i32 (ptr, ...) @printf(ptr @.unseen, ptr @s.callee, ptr @s.tail, i32 %v11)
  %v12 = call i32 @none_tail(i32 18)
  call i32 (ptr, ...) @printf(ptr @.unseen, ptr @s.none, ptr @s.tail, i32 %v12)
  ret i32 0
}

attributes #0 = { "disable-tail-calls"="true" }
attributes #1 = { "disable-tail-calls"="false" }
