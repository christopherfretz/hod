; -mbranch-protection as clang 22 writes it (the module flags in the
; current form, value 2, so each function carries its own policy): every
; function a BTI landing pad, return addresses signed with the A key
; ("non-leaf"), the B key ("all"), or not at all, mixed in one object.
; On AArch64 the object's .note.gnu.property has the BTI, PAC and GCS
; bits, so a program linked from it (and the C runtime, which has them)
; runs with BTI enforced where the processor has it, and every indirect
; branch must land on a landing pad: a call through a pointer, a
; computed goto, longjmp's return to setjmp. Return addresses are
; authenticated on every return, a musttail exit included.
; llvm.returnaddress gives the address without its authentication code.
; On x86-64 llc and hod ignore every one of these names. Landing pads
; are in branch_protection_unwind.ll: llc 19 puts no `bti j` on them, so
; its program would fault here.
;@ covers: spec/ir.md §5.7 attributes (branch-target-enforcement, sign-return-address, sign-return-address-key, guarded-control-stack), spec/codegen.md §5.6.26 branch protection
;@ tier: quick
;@ link: c++
;@ stdout: through a pointer 43
;@ stdout: goto 0 10
;@ stdout: goto 1 20
;@ stdout: goto 2 30
;@ stdout: switch 0 100
;@ stdout: switch 1 1
;@ stdout: switch 2 4
;@ stdout: switch 7 -1
;@ stdout: musttail 14
;@ stdout: dynamic 7 600
;@ stdout: longjmp 5
;@ stdout: return address stripped 1

@fp = global ptr @leaf_all_b

@.ptr = private unnamed_addr constant [22 x i8] c"through a pointer %d\0A\00"
@.goto = private unnamed_addr constant [12 x i8] c"goto %d %d\0A\00"
@.sw = private unnamed_addr constant [14 x i8] c"switch %d %d\0A\00"
@.mt = private unnamed_addr constant [13 x i8] c"musttail %d\0A\00"
@.dyn = private unnamed_addr constant [15 x i8] c"dynamic %d %d\0A\00"
@.lj = private unnamed_addr constant [12 x i8] c"longjmp %d\0A\00"
@.ra = private unnamed_addr constant [28 x i8] c"return address stripped %d\0A\00"

declare i32 @printf(ptr, ...)
declare i32 @setjmp(ptr) returns_twice
declare void @longjmp(ptr, i32) noreturn
declare ptr @llvm.returnaddress(i32)

; Signed with the B key although it calls nothing.
define i32 @leaf_all_b(i32 %x) noinline #1 {
  %r = add i32 %x, 1
  ret i32 %r
}

define internal i32 @twice(i32 %x) noinline #0 {
  %r = shl i32 %x, 1
  ret i32 %r
}

; Not signed: a landing pad at the entry and at both targets.
define i32 @computed_goto(i32 %i) noinline #2 {
entry:
  %tab = alloca [2 x ptr]
  store ptr blockaddress(@computed_goto, %one), ptr %tab
  %p1 = getelementptr [2 x ptr], ptr %tab, i64 0, i64 1
  store ptr blockaddress(@computed_goto, %two), ptr %p1
  %c = icmp eq i32 %i, 0
  br i1 %c, label %zero, label %pick
pick:
  %j = sub i32 %i, 1
  %jx = zext i32 %j to i64
  %slot = getelementptr [2 x ptr], ptr %tab, i64 0, i64 %jx
  %target = load ptr, ptr %slot
  indirectbr ptr %target, [label %one, label %two]
zero:
  ret i32 10
one:
  ret i32 20
two:
  ret i32 30
}

define i32 @sw(i32 %x) noinline #0 {
entry:
  switch i32 %x, label %other [
    i32 0, label %a
    i32 1, label %b
    i32 2, label %c
  ]
a:
  %r0 = call i32 @twice(i32 50)
  ret i32 %r0
b:
  ret i32 1
c:
  %r2 = call i32 @twice(i32 2)
  ret i32 %r2
other:
  ret i32 -1
}

define i32 @tail(i32 %x) noinline #0 {
  %y = add i32 %x, 2
  %r = musttail call i32 @twice(i32 %y)
  ret i32 %r
}

; A dynamic allocation and a frame beyond one 12-bit immediate.
define i32 @dynamic(i32 %n) noinline #1 {
  %big = alloca [70000 x i8], align 16
  %nx = zext i32 %n to i64
  %buf = alloca i32, i64 %nx, align 4
  %last = getelementptr i32, ptr %buf, i64 6
  store volatile i32 100, ptr %last
  %far = getelementptr [70000 x i8], ptr %big, i64 0, i64 69999
  store volatile i8 5, ptr %far
  %v = load volatile i32, ptr %last
  %b = load volatile i8, ptr %far
  %bx = zext i8 %b to i32
  %t = call i32 @twice(i32 %v)
  %s = mul i32 %t, 3
  %r = add i32 %s, 0
  %u = mul i32 %bx, 0
  %w = add i32 %r, %u
  ret i32 %w
}

define void @jumper(ptr %buf) noinline #0 {
  call void @longjmp(ptr %buf, i32 5)
  unreachable
}

define i32 @jump() noinline #0 {
  %buf = alloca [64 x i64], align 16
  %r = call i32 @setjmp(ptr %buf) #3
  %first = icmp eq i32 %r, 0
  br i1 %first, label %go, label %back
go:
  call void @jumper(ptr %buf)
  unreachable
back:
  ret i32 %r
}

define i64 @where() noinline #1 {
  %ra = call ptr @llvm.returnaddress(i32 0)
  %i = ptrtoint ptr %ra to i64
  ret i64 %i
}

define i32 @main() #0 {
  %f = load volatile ptr, ptr @fp
  %p = call i32 %f(i32 42)
  call i32 (ptr, ...) @printf(ptr @.ptr, i32 %p)
  %g0 = call i32 @computed_goto(i32 0)
  call i32 (ptr, ...) @printf(ptr @.goto, i32 0, i32 %g0)
  %g1 = call i32 @computed_goto(i32 1)
  call i32 (ptr, ...) @printf(ptr @.goto, i32 1, i32 %g1)
  %g2 = call i32 @computed_goto(i32 2)
  call i32 (ptr, ...) @printf(ptr @.goto, i32 2, i32 %g2)
  %s0 = call i32 @sw(i32 0)
  call i32 (ptr, ...) @printf(ptr @.sw, i32 0, i32 %s0)
  %s1 = call i32 @sw(i32 1)
  call i32 (ptr, ...) @printf(ptr @.sw, i32 1, i32 %s1)
  %s2 = call i32 @sw(i32 2)
  call i32 (ptr, ...) @printf(ptr @.sw, i32 2, i32 %s2)
  %s7 = call i32 @sw(i32 7)
  call i32 (ptr, ...) @printf(ptr @.sw, i32 7, i32 %s7)
  %t = call i32 @tail(i32 5)
  call i32 (ptr, ...) @printf(ptr @.mt, i32 %t)
  %d = call i32 @dynamic(i32 7)
  call i32 (ptr, ...) @printf(ptr @.dyn, i32 7, i32 %d)
  %j = call i32 @jump()
  call i32 (ptr, ...) @printf(ptr @.lj, i32 %j)
  %w = call i64 @where()
  %top = lshr i64 %w, 48
  %clean = icmp eq i64 %top, 0
  %ci = zext i1 %clean to i32
  call i32 (ptr, ...) @printf(ptr @.ra, i32 %ci)
  ret i32 0
}

attributes #0 = { "branch-target-enforcement" "guarded-control-stack" "sign-return-address"="non-leaf" "sign-return-address-key"="a_key" }
attributes #1 = { "branch-target-enforcement" "guarded-control-stack" "sign-return-address"="all" "sign-return-address-key"="b_key" }
attributes #2 = { "branch-target-enforcement" "guarded-control-stack" }
attributes #3 = { returns_twice }

!llvm.module.flags = !{!0, !1, !2}
!0 = !{i32 8, !"branch-target-enforcement", i32 2}
!1 = !{i32 8, !"guarded-control-stack", i32 2}
!2 = !{i32 8, !"sign-return-address", i32 2}
