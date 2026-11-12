; Dynamic stack allocation: an alloca whose size is an argument, one with
; an alignment above the stack alignment, one in a loop bracketed by
; stacksave and stackrestore, and one in every frame of a recursion, with
; calls made while the allocations are live.
;@ covers: spec/ir.md §5.2 stack objects, §5.4 memory (Alloca), §5.5 intrinsics (stacksave, stackrestore)
;@ unsupported: *
;@ stdout: sum of 1 squares = 0
;@ stdout: sum of 10 squares = 285
;@ stdout: sum of 1000 squares = 332833500
;@ stdout: aligned(1): low bits 0, checksum 0
;@ stdout: aligned(100): low bits 0, checksum 4950
;@ stdout: aligned(4097): low bits 0, checksum 522240
;@ stdout: loop: total 573225, stable 1
;@ stdout: depth 1: 1
;@ stdout: depth 40: 22140

@.squares = private unnamed_addr constant [26 x i8] c"sum of %d squares = %lld\0A\00"
@.aligned = private unnamed_addr constant [43 x i8] c"aligned(%lld): low bits %lld, checksum %d\0A\00"
@.loop = private unnamed_addr constant [29 x i8] c"loop: total %lld, stable %d\0A\00"
@.depth = private unnamed_addr constant [16 x i8] c"depth %d: %lld\0A\00"

declare i32 @printf(ptr, ...)
declare ptr @llvm.stacksave.p0()
declare void @llvm.stackrestore.p0(ptr)

; Fills a run-time-sized array with squares, then adds them up.
define i64 @sum_squares(i32 %n) {
entry:
  %buf = alloca i32, i32 %n
  br label %fill
fill:
  %i = phi i32 [ 0, %entry ], [ %i.next, %fill ]
  %sq = mul i32 %i, %i
  %slot = getelementptr inbounds i32, ptr %buf, i32 %i
  store i32 %sq, ptr %slot
  %i.next = add i32 %i, 1
  %filled = icmp eq i32 %i.next, %n
  br i1 %filled, label %add, label %fill
add:
  %j = phi i32 [ 0, %fill ], [ %j.next, %add ]
  %acc = phi i64 [ 0, %fill ], [ %acc.next, %add ]
  %at = getelementptr inbounds i32, ptr %buf, i32 %j
  %v = load i32, ptr %at
  %v.w = zext i32 %v to i64
  %acc.next = add i64 %acc, %v.w
  %j.next = add i32 %j, 1
  %done = icmp eq i32 %j.next, %n
  br i1 %done, label %exit, label %add
exit:
  ret i64 %acc.next
}

; An over-aligned dynamic allocation; a fixed slot shares the frame.
define void @aligned(i64 %n) {
entry:
  %fixed = alloca i32
  %p = alloca i8, i64 %n, align 64
  store i32 0, ptr %fixed
  %addr = ptrtoint ptr %p to i64
  %low = and i64 %addr, 63
  br label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %i.next, %loop ]
  %b = trunc i64 %i to i8
  %slot = getelementptr inbounds i8, ptr %p, i64 %i
  store i8 %b, ptr %slot
  %old = load i32, ptr %fixed
  %b.w = zext i8 %b to i32
  %new = add i32 %old, %b.w
  store i32 %new, ptr %fixed
  %i.next = add i64 %i, 1
  %done = icmp eq i64 %i.next, %n
  br i1 %done, label %exit, label %loop
exit:
  %sum = load i32, ptr %fixed
  call i32 (ptr, ...) @printf(ptr @.aligned, i64 %n, i64 %low, i32 %sum)
  ret void
}

; Allocates in every iteration and releases with stackrestore, so the
; allocation lands at the same address each time.
define void @in_loop(i32 %rounds, i32 %n) {
entry:
  br label %loop
loop:
  %i = phi i32 [ 0, %entry ], [ %i.next, %loop ]
  %total = phi i64 [ 0, %entry ], [ %total.next, %loop ]
  %first = phi i64 [ 0, %entry ], [ %first.next, %loop ]
  %moved = phi i32 [ 0, %entry ], [ %moved.next, %loop ]
  %sp = call ptr @llvm.stacksave.p0()
  %buf = alloca i64, i32 %n
  %addr = ptrtoint ptr %buf to i64
  %is.first = icmp eq i32 %i, 0
  %first.next = select i1 %is.first, i64 %addr, i64 %first
  %differs = icmp ne i64 %addr, %first.next
  %differs.i = zext i1 %differs to i32
  %moved.next = or i32 %moved, %differs.i
  %last = sub i32 %n, 1
  %slot = getelementptr inbounds i64, ptr %buf, i32 %last
  %i.w = zext i32 %i to i64
  store i64 %i.w, ptr %slot
  %part = call i64 @sum_squares(i32 %n)
  %back = load i64, ptr %slot
  %t0 = add i64 %total, %part
  %total.next = add i64 %t0, %back
  call void @llvm.stackrestore.p0(ptr %sp)
  %i.next = add i32 %i, 1
  %done = icmp eq i32 %i.next, %rounds
  br i1 %done, label %exit, label %loop
exit:
  %stable = xor i32 %moved.next, 1
  call i32 (ptr, ...) @printf(ptr @.loop, i64 %total.next, i32 %stable)
  ret void
}

; Each frame owns n words, writes its depth into all of them, recurses,
; and then reads them back.
define i64 @depth(i32 %n) {
entry:
  %zero = icmp eq i32 %n, 0
  br i1 %zero, label %base, label %work
base:
  ret i64 0
work:
  %buf = alloca i64, i32 %n
  br label %fill
fill:
  %i = phi i32 [ 0, %work ], [ %i.next, %fill ]
  %slot = getelementptr inbounds i64, ptr %buf, i32 %i
  %n.w = zext i32 %n to i64
  store i64 %n.w, ptr %slot
  %i.next = add i32 %i, 1
  %filled = icmp eq i32 %i.next, %n
  br i1 %filled, label %recurse, label %fill
recurse:
  %below = sub i32 %n, 1
  %rest = call i64 @depth(i32 %below)
  br label %add
add:
  %j = phi i32 [ 0, %recurse ], [ %j.next, %add ]
  %acc = phi i64 [ %rest, %recurse ], [ %acc.next, %add ]
  %at = getelementptr inbounds i64, ptr %buf, i32 %j
  %v = load i64, ptr %at
  %acc.next = add i64 %acc, %v
  %j.next = add i32 %j, 1
  %done = icmp eq i32 %j.next, %n
  br i1 %done, label %exit, label %add
exit:
  ret i64 %acc.next
}

define i32 @main() {
  %s1 = call i64 @sum_squares(i32 1)
  call i32 (ptr, ...) @printf(ptr @.squares, i32 1, i64 %s1)
  %s2 = call i64 @sum_squares(i32 10)
  call i32 (ptr, ...) @printf(ptr @.squares, i32 10, i64 %s2)
  %s3 = call i64 @sum_squares(i32 1000)
  call i32 (ptr, ...) @printf(ptr @.squares, i32 1000, i64 %s3)
  call void @aligned(i64 1)
  call void @aligned(i64 100)
  call void @aligned(i64 4097)
  call void @in_loop(i32 50, i32 33)
  %d1 = call i64 @depth(i32 1)
  call i32 (ptr, ...) @printf(ptr @.depth, i32 1, i64 %d1)
  %d2 = call i64 @depth(i32 40)
  call i32 (ptr, ...) @printf(ptr @.depth, i32 40, i64 %d2)
  ret i32 0
}
