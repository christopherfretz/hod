; The blocks llc's LowerConstantIntrinsics deletes (spec/codegen.md
; §5.6.8, WP-261). __builtin_constant_p of a non-constant is false, so
; the arm it guards is dead; llc 19 and 22 fold the branch (through the
; simple users InstSimplify folds), delete the blocks no longer reached
; and, in such a function, end a block after a noreturn call and fold a
; switch on a constant. Every deleted block calls @ensure_compile_error,
; which is declared and never defined (a C++ library's way of making a
; misuse fail to link): the program links only if no such block is
; emitted.
;@ covers: spec/codegen.md §5.6.8 (Folded constants and the blocks they remove)
;@ tier: quick
;@ stdout: direct 3
;@ stdout: manifest 1
;@ stdout: xor 4
;@ stdout: zext 5
;@ stdout: and 6
;@ stdout: select 7
;@ stdout: freeze 8
;@ stdout: phi 9
;@ stdout: bound 10
;@ stdout: objectsize 11
;@ stdout: switch 12
;@ stdout: loop 3
;@ stdout: noreturn 13
;@ exit: 7

@.fmt = private unnamed_addr constant [7 x i8] c"%s %d\0A\00"
@.direct = private unnamed_addr constant [7 x i8] c"direct\00"
@.manifest = private unnamed_addr constant [9 x i8] c"manifest\00"
@.xor = private unnamed_addr constant [4 x i8] c"xor\00"
@.zext = private unnamed_addr constant [5 x i8] c"zext\00"
@.and = private unnamed_addr constant [4 x i8] c"and\00"
@.select = private unnamed_addr constant [7 x i8] c"select\00"
@.freeze = private unnamed_addr constant [7 x i8] c"freeze\00"
@.phi = private unnamed_addr constant [4 x i8] c"phi\00"
@.bound = private unnamed_addr constant [6 x i8] c"bound\00"
@.objectsize = private unnamed_addr constant [11 x i8] c"objectsize\00"
@.switch = private unnamed_addr constant [7 x i8] c"switch\00"
@.loop = private unnamed_addr constant [5 x i8] c"loop\00"
@.noreturn = private unnamed_addr constant [9 x i8] c"noreturn\00"

declare i32 @printf(ptr, ...)
declare void @exit(i32) noreturn
declare i1 @llvm.is.constant.i32(i32)
declare i64 @llvm.objectsize.i64.p0(ptr, i1, i1, i1)
; Never defined.
declare void @ensure_compile_error(i32)

define void @say(ptr %what, i32 %n) noinline {
  %r = call i32 (ptr, ...) @printf(ptr @.fmt, ptr %what, i32 %n)
  ret void
}

; The reproducer's shape: the call, then a branch on it.
define i32 @direct(i32 %x) noinline {
entry:
  %c = call i1 @llvm.is.constant.i32(i32 %x)
  br i1 %c, label %dead, label %live
dead:
  call void @ensure_compile_error(i32 %x)
  br label %live
live:
  call void @say(ptr @.direct, i32 %x)
  ret i32 %x
}

; A constant operand: true, the other arm is the dead one.
define void @manifest() noinline {
entry:
  %c = call i1 @llvm.is.constant.i32(i32 5)
  br i1 %c, label %live, label %dead
dead:
  call void @ensure_compile_error(i32 0)
  ret void
live:
  %z = zext i1 %c to i32
  call void @say(ptr @.manifest, i32 %z)
  ret void
}

define void @xor(i32 %x) noinline {
entry:
  %c = call i1 @llvm.is.constant.i32(i32 %x)
  %n = xor i1 %c, true
  br i1 %n, label %live, label %dead
dead:
  call void @ensure_compile_error(i32 %x)
  br label %live
live:
  call void @say(ptr @.xor, i32 %x)
  ret void
}

define void @zext(i32 %x) noinline {
entry:
  %c = call i1 @llvm.is.constant.i32(i32 %x)
  %z = zext i1 %c to i32
  %t = icmp ne i32 %z, 0
  br i1 %t, label %dead, label %live
dead:
  call void @ensure_compile_error(i32 %x)
  br label %live
live:
  call void @say(ptr @.zext, i32 %x)
  ret void
}

; `and false, %p` is false whatever %p is.
define void @and(i32 %x, i1 %p) noinline {
entry:
  %c = call i1 @llvm.is.constant.i32(i32 %x)
  %a = and i1 %c, %p
  br i1 %a, label %dead, label %live
dead:
  call void @ensure_compile_error(i32 %x)
  br label %live
live:
  call void @say(ptr @.and, i32 %x)
  ret void
}

define void @select(i32 %x) noinline {
entry:
  %c = call i1 @llvm.is.constant.i32(i32 %x)
  %s = select i1 %c, i32 1, i32 0
  %t = icmp eq i32 %s, 1
  br i1 %t, label %dead, label %live
dead:
  call void @ensure_compile_error(i32 %x)
  br label %live
live:
  call void @say(ptr @.select, i32 %x)
  ret void
}

define void @freeze(i32 %x) noinline {
entry:
  %c = call i1 @llvm.is.constant.i32(i32 %x)
  %f = freeze i1 %c
  br i1 %f, label %dead, label %live
dead:
  call void @ensure_compile_error(i32 %x)
  br label %live
live:
  call void @say(ptr @.freeze, i32 %x)
  ret void
}

; A phi of the folded value and the same constant.
define void @phi(i32 %x, i1 %p) noinline {
entry:
  %c = call i1 @llvm.is.constant.i32(i32 %x)
  br i1 %p, label %a, label %b
a:
  br label %j
b:
  br label %j
j:
  %v = phi i1 [ %c, %a ], [ false, %b ]
  br i1 %v, label %dead, label %live
dead:
  call void @ensure_compile_error(i32 %x)
  br label %live
live:
  call void @say(ptr @.phi, i32 %x)
  ret void
}

; `0 >u %y` is false whatever %y is.
define void @bound(i32 %x, i32 %y) noinline {
entry:
  %c = call i1 @llvm.is.constant.i32(i32 %x)
  %z = zext i1 %c to i32
  %t = icmp ugt i32 %z, %y
  br i1 %t, label %dead, label %live
dead:
  call void @ensure_compile_error(i32 %x)
  br label %live
live:
  call void @say(ptr @.bound, i32 %x)
  ret void
}

; The size of what an argument points to is unknown: -1.
define void @objectsize(ptr %q, i32 %x) noinline {
entry:
  %o = call i64 @llvm.objectsize.i64.p0(ptr %q, i1 false, i1 true, i1 false)
  %t = icmp ne i64 %o, -1
  br i1 %t, label %dead, label %live
dead:
  call void @ensure_compile_error(i32 %x)
  br label %live
live:
  call void @say(ptr @.objectsize, i32 %x)
  ret void
}

; Once a block is deleted, llc also folds a switch on the constant.
define void @switch(i32 %x) noinline {
entry:
  %c = call i1 @llvm.is.constant.i32(i32 %x)
  br i1 %c, label %dead, label %sw
dead:
  call void @ensure_compile_error(i32 %x)
  br label %sw
sw:
  switch i1 %c, label %live [
    i1 true, label %dead2
  ]
dead2:
  call void @ensure_compile_error(i32 1)
  br label %live
live:
  call void @say(ptr @.switch, i32 %x)
  ret void
}

; A folded branch inside a loop; the dead arm is the loop's only other
; way back to its header.
define void @loop(i32 %x, i32 %n) noinline {
entry:
  br label %head
head:
  %i = phi i32 [ 0, %entry ], [ %next, %latch ], [ %unused, %dead ]
  %c = call i1 @llvm.is.constant.i32(i32 %x)
  br i1 %c, label %dead, label %latch
dead:
  call void @ensure_compile_error(i32 %i)
  %unused = add i32 %i, 1
  br label %head
latch:
  %next = add i32 %i, 1
  %more = icmp slt i32 %next, %n
  br i1 %more, label %head, label %out
out:
  call void @say(ptr @.loop, i32 %next)
  ret void
}

; Once a block is deleted, llc ends a block at a noreturn call: what
; follows it (a branch to a block of its own) is deleted too.
define void @noreturn(i32 %x) noinline {
entry:
  %c = call i1 @llvm.is.constant.i32(i32 %x)
  br i1 %c, label %dead, label %live
dead:
  call void @ensure_compile_error(i32 %x)
  br label %live
live:
  call void @say(ptr @.noreturn, i32 %x)
  call void @exit(i32 7)
  br label %after
after:
  call void @ensure_compile_error(i32 2)
  ret void
}

define i32 @main() {
  %r = call i32 @direct(i32 3)
  call void @manifest()
  call void @xor(i32 4)
  call void @zext(i32 5)
  call void @and(i32 6, i1 true)
  call void @select(i32 7)
  call void @freeze(i32 8)
  call void @phi(i32 9, i1 true)
  call void @bound(i32 10, i32 0)
  %buf = alloca [4 x i8]
  call void @objectsize(ptr %buf, i32 11)
  call void @switch(i32 12)
  call void @loop(i32 0, i32 3)
  call void @noreturn(i32 13)
  ret i32 0
}
