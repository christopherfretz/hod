; Two modules that make one program (spec/harness.md "Test directives",
; "Several modules"): each is compiled on its own and the objects are
; linked in this order. `main` reads and writes a variable the other module
; defines, calls a function it defines, hands it a callback that is local
; to `main`'s module, and both modules have a private function of the same
; name. A linkonce_odr function is defined in both and is one function in
; the program: its two addresses compare equal.
;@ covers: spec/ir.md §2.2 symbols (definitions and declarations across modules, internal linkage, linkonce_odr)
;@ tier: quick
;@ unsupported(aarch64): global:function-definition
;@ unsupported(x86_64): global:variable-definition
;@ stdout: counter=7
;@ stdout: bump=10 counter=10
;@ stdout: apply=45 helper=2
;@ stdout: local=101 other=202
;@ stdout: shared=1 value=5
;@ module: counter

@counter = global i32 7

define i32 @bump(i32 %by) {
  %old = load i32, ptr @counter
  %new = add i32 %old, %by
  store i32 %new, ptr @counter
  ret i32 %new
}

define i32 @apply(ptr %f, i32 %x) {
  %r = call i32 %f(i32 %x)
  ret i32 %r
}

define internal i32 @local() {
  ret i32 202
}

define i32 @other_local() {
  %r = call i32 @local()
  ret i32 %r
}

define linkonce_odr i32 @shared() {
  ret i32 5
}

define ptr @shared_there() {
  ret ptr @shared
}
;@ module: main

@counter = external global i32
@helper_calls = internal global i32 0
@.s1 = private unnamed_addr constant [12 x i8] c"counter=%d\0A\00"
@.s2 = private unnamed_addr constant [20 x i8] c"bump=%d counter=%d\0A\00"
@.s3 = private unnamed_addr constant [20 x i8] c"apply=%d helper=%d\0A\00"
@.s4 = private unnamed_addr constant [19 x i8] c"local=%d other=%d\0A\00"
@.s5 = private unnamed_addr constant [20 x i8] c"shared=%d value=%d\0A\00"

declare i32 @printf(ptr, ...)
declare i32 @bump(i32)
declare i32 @apply(ptr, i32)
declare i32 @other_local()
declare ptr @shared_there()

define internal i32 @helper(i32 %x) {
  %n = load i32, ptr @helper_calls
  %n1 = add i32 %n, 1
  store i32 %n1, ptr @helper_calls
  %r = mul i32 %x, 3
  ret i32 %r
}

define internal i32 @local() {
  ret i32 101
}

define linkonce_odr i32 @shared() {
  ret i32 5
}

define i32 @main() {
  %c0 = load i32, ptr @counter
  call i32 (ptr, ...) @printf(ptr @.s1, i32 %c0)
  %b = call i32 @bump(i32 3)
  %c1 = load i32, ptr @counter
  call i32 (ptr, ...) @printf(ptr @.s2, i32 %b, i32 %c1)
  %a1 = call i32 @apply(ptr @helper, i32 5)
  %a2 = call i32 @apply(ptr @helper, i32 %c1)
  %a = add i32 %a1, %a2
  %h = load i32, ptr @helper_calls
  call i32 (ptr, ...) @printf(ptr @.s3, i32 %a, i32 %h)
  %l = call i32 @local()
  %o = call i32 @other_local()
  call i32 (ptr, ...) @printf(ptr @.s4, i32 %l, i32 %o)
  %there = call ptr @shared_there()
  %same = icmp eq ptr %there, @shared
  %same.i = zext i1 %same to i32
  %v = call i32 %there()
  call i32 (ptr, ...) @printf(ptr @.s5, i32 %same.i, i32 %v)
  ret i32 0
}
