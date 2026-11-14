; -fcf-protection=full, as clang and kiln write it for x86-64: the module
; flags cf-protection-branch and cf-protection-return. Under them llc
; puts endbr64 at every function entry an indirect branch may reach, at
; address-taken blocks, at landing pads and after direct calls of
; returns_twice functions, and writes the IBT and SHSTK property note.
; The program reaches each such place: calls through function pointers
; held in data (internal and external callees, linkonce_odr, weak,
; hidden, one kept only by llvm.used), nocf_check functions called
; directly, an indirectbr whose target block has a phi, a block address
; held in data, a switch, a call of a returns_twice function, landing
; pads (one with a phi, one behind an invoke with stack arguments, a
; cleanup that resumes). AArch64 hod refuses the x86 flags (llc ignores
; them there). Needs the C++ runtime.
;@ covers: spec/ir.md §2.1 module flags (cf-protection-branch, cf-protection-return), §5.7 attributes (nocf_check)
;@ link: c++
;@ unsupported(aarch64): *
;@ stdout: calls 295
;@ stdout: calls2 15
;@ stdout: sw 11 0
;@ stdout: ib 1 7 1201
;@ stdout: ba 3 4
;@ stdout: caught 5
;@ stdout: phi 111 203
;@ stdout: stub 16
;@ stdout: cleanup 1004

@.calls = private unnamed_addr constant [10 x i8] c"calls %d\0A\00"
@.calls2 = private unnamed_addr constant [11 x i8] c"calls2 %d\0A\00"
@.sw = private unnamed_addr constant [10 x i8] c"sw %d %d\0A\00"
@.ib = private unnamed_addr constant [13 x i8] c"ib %d %d %d\0A\00"
@.ba = private unnamed_addr constant [10 x i8] c"ba %d %d\0A\00"
@.caught = private unnamed_addr constant [11 x i8] c"caught %d\0A\00"
@.phi = private unnamed_addr constant [11 x i8] c"phi %d %d\0A\00"
@.stub = private unnamed_addr constant [9 x i8] c"stub %d\0A\00"
@.cleanup = private unnamed_addr constant [12 x i8] c"cleanup %d\0A\00"

declare i32 @printf(ptr, ...)

; ---- calls ----

@fp_tab = internal constant [2 x ptr] [ptr @l_tab, ptr @g_ext]
@ptr_slot = global ptr @l_stored
@llvm.used = appending global [1 x ptr] [ptr @l_used], section "llvm.metadata"

define internal i32 @l_tab(i32 %x) noinline {
  %r = add i32 %x, 1
  ret i32 %r
}

define internal i32 @l_stored(i32 %x) noinline {
  %r = mul i32 %x, 3
  ret i32 %r
}

define internal i32 @l_used(i32 %x) noinline {
  ret i32 %x
}

define internal i32 @l_direct(i32 %x) noinline {
  %r = sub i32 %x, 1
  ret i32 %r
}

define linkonce_odr i32 @lo(i32 %x) noinline {
  %r = add i32 %x, 10
  ret i32 %r
}

define weak i32 @wk(i32 %x) noinline {
  %r = add i32 %x, 20
  ret i32 %r
}

define hidden i32 @hid(i32 %x) noinline {
  %r = add i32 %x, 30
  ret i32 %r
}

define i32 @g_ext(i32 %x) noinline {
  %r = add i32 %x, 40
  ret i32 %r
}

define i32 @nocf_g(i32 %x) noinline nocf_check {
  %r = add i32 %x, 50
  ret i32 %r
}

define internal i32 @nocf_l(i32 %x) noinline nocf_check {
  %r = add i32 %x, 60
  ret i32 %r
}

; Returns once: what matters is the declaration's returns_twice.
define i32 @rt(i32 %x) noinline returns_twice {
  ret i32 %x
}

define i32 @nrt(i32 %x) noinline {
  ret i32 %x
}

define i32 @calls(i32 %x) noinline {
  %f0 = load ptr, ptr @fp_tab
  %a = call i32 %f0(i32 %x)
  %f1p = getelementptr [2 x ptr], ptr @fp_tab, i64 0, i64 1
  %f1 = load ptr, ptr %f1p
  %b = call i32 %f1(i32 %a)
  %f2 = load ptr, ptr @ptr_slot
  %c = call i32 %f2(i32 %b)
  %d = call i32 @l_direct(i32 %c)
  %e = call i32 @lo(i32 %d)
  %f = call i32 @wk(i32 %e)
  %g = call i32 @hid(i32 %f)
  %h = call i32 @nocf_g(i32 %g) nocf_check
  %i = call i32 @nocf_l(i32 %h)
  %j = call i32 @rt(i32 %i)
  %k = call i32 @nrt(i32 %j) returns_twice
  %u = call i32 @l_used(i32 %k)
  ret i32 %u
}

; returns_twice on an indirect call site only: llc marks nothing after it.
define i32 @calls2(i32 %x) noinline {
  %f = load ptr, ptr @ptr_slot
  %r = call i32 %f(i32 %x) returns_twice
  ret i32 %r
}

; ---- blocks ----

define i32 @sw(i32 %x) noinline {
entry:
  switch i32 %x, label %d [
    i32 0, label %a
    i32 1, label %b
    i32 2, label %c
    i32 3, label %e
    i32 4, label %f
    i32 5, label %g
  ]
a:
  ret i32 5
b:
  ret i32 7
c:
  ret i32 9
e:
  ret i32 11
f:
  ret i32 2
g:
  ret i32 99
d:
  ret i32 0
}

@ib_tab = internal constant [3 x ptr] [ptr blockaddress(@ib, %a), ptr blockaddress(@ib, %b), ptr blockaddress(@ib, %c)]

define i32 @ib(i32 %x) noinline {
entry:
  %i = zext i32 %x to i64
  %p = getelementptr [3 x ptr], ptr @ib_tab, i64 0, i64 %i
  %t = load ptr, ptr %p
  %m = mul i32 %x, 100
  indirectbr ptr %t, [label %a, label %b, label %c]
a:
  %v = phi i32 [ %m, %entry ], [ %w, %c ]
  %r = add i32 %v, 1
  ret i32 %r
b:
  ret i32 7
c:
  %w = add i32 %m, 1000
  br label %a
}

; A block whose address is held in data, in a function without an
; indirectbr.
@ba_data = global ptr blockaddress(@ba_only, %there)

define i32 @ba_only(i32 %x) noinline {
entry:
  %c = icmp eq i32 %x, 0
  br i1 %c, label %there, label %other
there:
  ret i32 3
other:
  ret i32 4
}

; ---- landing pads ----

@_ZTIi = external constant ptr
@cleaned = global i32 0

declare ptr @__cxa_allocate_exception(i64)
declare void @__cxa_throw(ptr, ptr, ptr)
declare ptr @__cxa_begin_catch(ptr)
declare void @__cxa_end_catch()
declare i32 @__gxx_personality_v0(...)

define void @thrower(i32 %v) noinline {
  %e = call ptr @__cxa_allocate_exception(i64 4)
  store i32 %v, ptr %e
  call void @__cxa_throw(ptr %e, ptr @_ZTIi, ptr null)
  unreachable
}

define void @many(i64 %a, i64 %b, i64 %c, i64 %d, i64 %e, i64 %f, i64 %g, i64 %h) noinline {
  %s = add i64 %g, %h
  %t = trunc i64 %s to i32
  call void @thrower(i32 %t)
  unreachable
}

define i32 @catch_one(i32 %v) noinline personality ptr @__gxx_personality_v0 {
entry:
  invoke void @thrower(i32 %v) to label %ok unwind label %lp
ok:
  ret i32 0
lp:
  %x = landingpad { ptr, i32 } catch ptr @_ZTIi
  %p = extractvalue { ptr, i32 } %x, 0
  %b = call ptr @__cxa_begin_catch(ptr %p)
  %n = load i32, ptr %b
  call void @__cxa_end_catch()
  ret i32 %n
}

define i32 @catch_phi(i32 %v) noinline personality ptr @__gxx_personality_v0 {
entry:
  %c = icmp eq i32 %v, 0
  br i1 %c, label %a, label %b
a:
  invoke void @thrower(i32 11) to label %ok unwind label %lp
b:
  invoke void @thrower(i32 %v) to label %ok unwind label %lp
ok:
  ret i32 0
lp:
  %w = phi i32 [ 100, %a ], [ 200, %b ]
  %x = landingpad { ptr, i32 } catch ptr @_ZTIi
  %p = extractvalue { ptr, i32 } %x, 0
  %bc = call ptr @__cxa_begin_catch(ptr %p)
  %n = load i32, ptr %bc
  call void @__cxa_end_catch()
  %r = add i32 %n, %w
  ret i32 %r
}

; Stack arguments: hod lands in a stub that restores the stack pointer.
define i32 @catch_stub(i64 %h) noinline personality ptr @__gxx_personality_v0 {
entry:
  invoke void @many(i64 1, i64 2, i64 3, i64 4, i64 5, i64 6, i64 7, i64 %h) to label %ok unwind label %lp
ok:
  ret i32 0
lp:
  %x = landingpad { ptr, i32 } catch ptr @_ZTIi
  %p = extractvalue { ptr, i32 } %x, 0
  %b = call ptr @__cxa_begin_catch(ptr %p)
  %n = load i32, ptr %b
  call void @__cxa_end_catch()
  ret i32 %n
}

define i32 @cleanup(i32 %v) noinline personality ptr @__gxx_personality_v0 {
entry:
  invoke void @thrower(i32 %v) to label %ok unwind label %lp
ok:
  ret i32 0
lp:
  %x = landingpad { ptr, i32 } cleanup
  store i32 1, ptr @cleaned
  resume { ptr, i32 } %x
}

define i32 @outer(i32 %v) noinline personality ptr @__gxx_personality_v0 {
entry:
  %r = invoke i32 @cleanup(i32 %v) to label %ok unwind label %lp
ok:
  ret i32 %r
lp:
  %x = landingpad { ptr, i32 } catch ptr @_ZTIi
  %p = extractvalue { ptr, i32 } %x, 0
  %b = call ptr @__cxa_begin_catch(ptr %p)
  %n = load i32, ptr %b
  call void @__cxa_end_catch()
  %k = load i32, ptr @cleaned
  %k1 = mul i32 %k, 1000
  %s = add i32 %n, %k1
  ret i32 %s
}

define i32 @main() {
  %c = call i32 @calls(i32 1)
  call i32 (ptr, ...) @printf(ptr @.calls, i32 %c)
  %c2 = call i32 @calls2(i32 5)
  call i32 (ptr, ...) @printf(ptr @.calls2, i32 %c2)
  %s0 = call i32 @sw(i32 3)
  %s1 = call i32 @sw(i32 9)
  call i32 (ptr, ...) @printf(ptr @.sw, i32 %s0, i32 %s1)
  %i0 = call i32 @ib(i32 0)
  %i1 = call i32 @ib(i32 1)
  %i2 = call i32 @ib(i32 2)
  call i32 (ptr, ...) @printf(ptr @.ib, i32 %i0, i32 %i1, i32 %i2)
  %b0 = call i32 @ba_only(i32 0)
  %b1 = call i32 @ba_only(i32 1)
  call i32 (ptr, ...) @printf(ptr @.ba, i32 %b0, i32 %b1)
  %e0 = call i32 @catch_one(i32 5)
  call i32 (ptr, ...) @printf(ptr @.caught, i32 %e0)
  %e1 = call i32 @catch_phi(i32 0)
  %e2 = call i32 @catch_phi(i32 3)
  call i32 (ptr, ...) @printf(ptr @.phi, i32 %e1, i32 %e2)
  %e3 = call i32 @catch_stub(i64 9)
  call i32 (ptr, ...) @printf(ptr @.stub, i32 %e3)
  %e4 = call i32 @outer(i32 4)
  call i32 (ptr, ...) @printf(ptr @.cleanup, i32 %e4)
  ret i32 0
}

!llvm.module.flags = !{!0, !1}
!0 = !{i32 8, !"cf-protection-return", i32 1}
!1 = !{i32 8, !"cf-protection-branch", i32 1}
