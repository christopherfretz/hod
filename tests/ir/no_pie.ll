; A position-dependent program: compiled with the static relocation model
; and linked as a non-PIE executable, so addresses of globals and
; functions are absolute, in code and in initialized data.
;@ covers: spec/ir.md §2.1 target (PIC level), §4.2 addresses
;@ link: no-pie
;@ unsupported(aarch64): global:function-definition
;@ unsupported(x86_64): global:variable-definition
;@ stdout: one: 20
;@ stdout: two: -20
;@ stdout: three: 900
;@ stdout: sum=2109 second=20 distance=4

@.line = private unnamed_addr constant [8 x i8] c"%s: %d\0A\00"
@.one = private unnamed_addr constant [4 x i8] c"one\00"
@.two = private unnamed_addr constant [4 x i8] c"two\00"
@.three = private unnamed_addr constant [6 x i8] c"three\00"
@.sum = private unnamed_addr constant [32 x i8] c"sum=%d second=%d distance=%lld\0A\00"
@.names = global [3 x ptr] [ptr @.one, ptr @.two, ptr @.three]
@values = global [3 x i32] [i32 10, i32 20, i32 30]
@second = global ptr getelementptr inbounds ([3 x i32], ptr @values, i64 0, i64 1)
@handlers = constant [3 x ptr] [ptr @double, ptr @negate, ptr @square]

declare i32 @printf(ptr, ...)

define i32 @double(i32 %x) {
  %r = shl i32 %x, 1
  ret i32 %r
}

define i32 @negate(i32 %x) {
  %r = sub i32 0, %x
  ret i32 %r
}

define i32 @square(i32 %x) {
  %r = mul i32 %x, %x
  ret i32 %r
}

define i32 @pick(i32 %which) {
entry:
  switch i32 %which, label %other [
    i32 0, label %a
    i32 1, label %b
    i32 2, label %c
    i32 3, label %d
    i32 4, label %e
  ]
a:
  ret i32 100
b:
  ret i32 201
c:
  ret i32 302
d:
  ret i32 403
e:
  ret i32 504
other:
  ret i32 -1
}

define i32 @main() {
entry:
  br label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %i.next, %loop ]
  %sum = phi i32 [ 0, %entry ], [ %sum.next, %loop ]
  %name.slot = getelementptr inbounds [3 x ptr], ptr @.names, i64 0, i64 %i
  %name = load ptr, ptr %name.slot
  %value.slot = getelementptr inbounds [3 x i32], ptr @values, i64 0, i64 %i
  %value = load i32, ptr %value.slot
  %handler.slot = getelementptr inbounds [3 x ptr], ptr @handlers, i64 0, i64 %i
  %handler = load ptr, ptr %handler.slot
  %r = call i32 %handler(i32 %value)
  call i32 (ptr, ...) @printf(ptr @.line, ptr %name, i32 %r)
  %i.32 = trunc i64 %i to i32
  %which = add i32 %i.32, 2
  %p = call i32 @pick(i32 %which)
  %s0 = add i32 %sum, %r
  %sum.next = add i32 %s0, %p
  %i.next = add i64 %i, 1
  %done = icmp eq i64 %i.next, 3
  br i1 %done, label %exit, label %loop
exit:
  %sp = load ptr, ptr @second
  %sv = load i32, ptr %sp
  %from = ptrtoint ptr @values to i64
  %to = ptrtoint ptr %sp to i64
  %distance = sub i64 %to, %from
  call i32 (ptr, ...) @printf(ptr @.sum, i32 %sum.next, i32 %sv, i64 %distance)
  ret i32 0
}
