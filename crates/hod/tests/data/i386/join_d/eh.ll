; Exceptions with the other packages' constructs (i386 join D): `invoke`
; of functions that return an aggregate in memory (first-class and
; `sret`) and an `{ i64, i1 }` pair, the pad using values from before;
; landing pads in functions with dynamic `alloca`s and
; `stacksave`/`stackrestore` (the frame is `ebp`-based; the unwinder
; lands with `esp` as it was at the call); `memcpy` with an `i64` length
; inside a pad; an atomic loop inside a `try`; overflow intrinsics in a
; pad.
target datalayout = "e-m:e-p:32:32-p270:32:32-p271:32:32-p272:64:64-i128:128-f64:32:64-f80:32-n8:16:32-S128"
target triple = "i686-unknown-linux-gnu"

%big = type { i32, i64, i16, [3 x i32] }

@_ZTIi = external constant ptr
@fmt.agg = private constant [27 x i8] c"agg %d %lld %d %d | %d %d\0A\00"
@fmt.pair = private constant [22 x i8] c"pair %lld %d | %d %d\0A\00"
@fmt.dyn = private constant [28 x i8] c"dyn %d %d %d %d | %d %d %d\0A\00"
@fmt.cpy = private constant [24 x i8] c"cpy %d %d %d %lld | %d\0A\00"
@fmt.loop = private constant [22 x i8] c"loop %d %d %lld | %d\0A\00"
@hits = global i32 0
@total = global i64 0
@src = global [64 x i8] c"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!?"

declare i32 @__gxx_personality_v0(...)
declare ptr @__cxa_allocate_exception(i32)
declare void @__cxa_throw(ptr, ptr, ptr)
declare ptr @__cxa_begin_catch(ptr)
declare void @__cxa_end_catch()
declare i32 @printf(ptr, ...)
declare i32 @llvm.eh.typeid.for.p0(ptr)
declare ptr @llvm.stacksave.p0()
declare void @llvm.stackrestore.p0(ptr)
declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)
declare { i64, i1 } @llvm.sadd.with.overflow.i64(i64, i64)
declare { i32, i1 } @llvm.smul.with.overflow.i32(i32, i32)

define void @throw_if(i32 %v) noinline {
entry:
  %t = icmp sgt i32 %v, 0
  br i1 %t, label %throw, label %ret
throw:
  %e = call ptr @__cxa_allocate_exception(i32 4)
  store i32 %v, ptr %e
  call void @__cxa_throw(ptr %e, ptr @_ZTIi, ptr null)
  unreachable
ret:
  ret void
}

; A first-class aggregate too large for registers (a result in memory).
define %big @make_big(i32 %v) noinline {
  %w = sext i32 %v to i64
  %m = mul i64 %w, 1000000007
  %a = insertvalue %big undef, i32 %v, 0
  %b = insertvalue %big %a, i64 %m, 1
  %c = insertvalue %big %b, i16 -3, 2
  %d = insertvalue %big %c, i32 11, 3, 0
  %e = insertvalue %big %d, i32 22, 3, 1
  %f = insertvalue %big %e, i32 33, 3, 2
  call void @throw_if(i32 %v)
  ret %big %f
}

; An explicit `sret`.
define void @fill_big(ptr sret(%big) %out, i32 %v) noinline {
  store i32 %v, ptr %out
  %p = getelementptr %big, ptr %out, i32 0, i32 1
  store i64 -1, ptr %p
  %q = getelementptr %big, ptr %out, i32 0, i32 2
  store i16 7, ptr %q
  call void @throw_if(i32 %v)
  ret void
}

; `{ i64, i1 }` from an overflow intrinsic, returned by value.
define { i64, i1 } @add_pair(i64 %a, i64 %b, i32 %v) noinline {
  %r = call { i64, i1 } @llvm.sadd.with.overflow.i64(i64 %a, i64 %b)
  call void @throw_if(i32 %v)
  ret { i64, i1 } %r
}

define void @agg(i32 %v) noinline personality ptr @__gxx_personality_v0 {
entry:
  %x = mul i32 %v, 13
  %y = xor i32 %v, 255
  %b = invoke %big @make_big(i32 %v) to label %ok unwind label %pad
ok:
  %b0 = extractvalue %big %b, 0
  %b1 = extractvalue %big %b, 1
  %b2 = extractvalue %big %b, 2
  %b2i = sext i16 %b2 to i32
  %b3 = extractvalue %big %b, 3, 2
  %slot = alloca %big
  invoke void @fill_big(ptr sret(%big) %slot, i32 0) to label %ok2 unwind label %pad
ok2:
  %s0 = load i32, ptr %slot
  %sum = add i32 %b3, %s0
  call i32 (ptr, ...) @printf(ptr @fmt.agg, i32 %b0, i64 %b1, i32 %b2i, i32 %sum, i32 %x, i32 %y)
  ret void
pad:
  %lp = landingpad { ptr, i32 } catch ptr @_ZTIi
  %sel = extractvalue { ptr, i32 } %lp, 1
  %tid = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTIi)
  %match = icmp eq i32 %sel, %tid
  %mi = zext i1 %match to i32
  %exn = extractvalue { ptr, i32 } %lp, 0
  %obj = call ptr @__cxa_begin_catch(ptr %exn)
  %thrown = load i32, ptr %obj
  call void @__cxa_end_catch()
  %m = call { i32, i1 } @llvm.smul.with.overflow.i32(i32 %x, i32 %thrown)
  %mv = extractvalue { i32, i1 } %m, 0
  %mo = extractvalue { i32, i1 } %m, 1
  %moi = zext i1 %mo to i32
  call i32 (ptr, ...) @printf(ptr @fmt.agg, i32 %thrown, i64 0, i32 %mi, i32 %mv, i32 %moi, i32 %y)
  ret void
}

define void @pair(i64 %a, i64 %b, i32 %v) noinline personality ptr @__gxx_personality_v0 {
entry:
  %k = trunc i64 %a to i32
  %r = invoke { i64, i1 } @add_pair(i64 %a, i64 %b, i32 %v) to label %ok unwind label %pad
ok:
  %rv = extractvalue { i64, i1 } %r, 0
  %ro = extractvalue { i64, i1 } %r, 1
  %roi = zext i1 %ro to i32
  call i32 (ptr, ...) @printf(ptr @fmt.pair, i64 %rv, i32 %roi, i32 %k, i32 0)
  ret void
pad:
  %lp = landingpad { ptr, i32 } cleanup catch ptr @_ZTIi
  %exn = extractvalue { ptr, i32 } %lp, 0
  %obj = call ptr @__cxa_begin_catch(ptr %exn)
  %thrown = load i32, ptr %obj
  call void @__cxa_end_catch()
  %again = call { i64, i1 } @llvm.sadd.with.overflow.i64(i64 %a, i64 %b)
  %av = extractvalue { i64, i1 } %again, 0
  %ao = extractvalue { i64, i1 } %again, 1
  %aoi = zext i1 %ao to i32
  call i32 (ptr, ...) @printf(ptr @fmt.pair, i64 %av, i32 %aoi, i32 %k, i32 %thrown)
  ret void
}

; Dynamic `alloca`s before the `invoke` and in the pad, a `stacksave`
; before and a `stackrestore` in the pad, repeated in a loop: the
; values in the variable part are read back after each landing.
define void @dyn(i32 %n, i32 %rounds) noinline personality ptr @__gxx_personality_v0 {
entry:
  %fixed = alloca i32
  store i32 99, ptr %fixed
  %arr = alloca i32, i32 %n, align 32
  br label %head
head:
  %i = phi i32 [ 0, %entry ], [ %i.next, %next ]
  %acc = phi i32 [ 0, %entry ], [ %acc.next, %next ]
  %sp = call ptr @llvm.stacksave.p0()
  %tmp = alloca i32, i32 %n
  %last = sub i32 %n, 1
  %slot = getelementptr i32, ptr %tmp, i32 %last
  store i32 %i, ptr %slot
  %aslot = getelementptr i32, ptr %arr, i32 %last
  store i32 %acc, ptr %aslot
  %which = and i32 %i, 1
  invoke void @throw_if(i32 %which) to label %cont unwind label %pad
cont:
  call void @llvm.stackrestore.p0(ptr %sp)
  br label %next
pad:
  %lp = landingpad { ptr, i32 } catch ptr null
  %exn = extractvalue { ptr, i32 } %lp, 0
  %obj = call ptr @__cxa_begin_catch(ptr %exn)
  call void @__cxa_end_catch()
  %more = alloca i32, i32 %n, align 64
  %mslot = getelementptr i32, ptr %more, i32 %last
  %seen = load i32, ptr %slot
  store i32 %seen, ptr %mslot
  %aligned = ptrtoint ptr %more to i32
  %low = and i32 %aligned, 63
  %got = load i32, ptr %mslot
  %prev = load i32, ptr %aslot
  %f = load i32, ptr %fixed
  %t1 = add i32 %got, %prev
  %t2 = add i32 %t1, %f
  %t3 = add i32 %t2, %low
  call void @llvm.stackrestore.p0(ptr %sp)
  br label %next
next:
  %add = phi i32 [ 1, %cont ], [ %t3, %pad ]
  %acc.next = add i32 %acc, %add
  %i.next = add i32 %i, 1
  %done = icmp eq i32 %i.next, %rounds
  br i1 %done, label %exit, label %head
exit:
  %after = alloca i32, i32 %n
  %al = getelementptr i32, ptr %after, i32 %last
  store i32 5, ptr %al
  %r = load i32, ptr %al
  %fx = load i32, ptr %fixed
  %a0 = load i32, ptr %aslot
  call i32 (ptr, ...) @printf(ptr @fmt.dyn, i32 %n, i32 %rounds, i32 %acc.next, i32 %r, i32 %fx, i32 %a0, i32 0)
  ret void
}

; `memcpy` with an `i64` length (a variable one: libc's `memcpy`) and a
; constant one of `2^32 + 5` (5 bytes) inside a pad.
define void @copy(i64 %len, i32 %v) noinline personality ptr @__gxx_personality_v0 {
entry:
  %buf = alloca [64 x i8]
  %small = alloca [8 x i8]
  call void @llvm.memset.p0.i32(ptr %buf, i8 46, i32 64, i1 false)
  call void @llvm.memset.p0.i32(ptr %small, i8 0, i32 8, i1 false)
  invoke void @throw_if(i32 %v) to label %ok unwind label %pad
ok:
  call i32 (ptr, ...) @printf(ptr @fmt.cpy, i32 0, i32 0, i32 0, i64 %len, i32 %v)
  ret void
pad:
  %lp = landingpad { ptr, i32 } catch ptr @_ZTIi
  %exn = extractvalue { ptr, i32 } %lp, 0
  %obj = call ptr @__cxa_begin_catch(ptr %exn)
  call void @__cxa_end_catch()
  call void @llvm.memcpy.p0.p0.i64(ptr %buf, ptr @src, i64 %len, i1 false)
  call void @llvm.memcpy.p0.p0.i64(ptr %small, ptr @src, i64 4294967301, i1 false)
  %c0 = load i8, ptr %buf
  %pl = trunc i64 %len to i32
  %pm = sub i32 %pl, 1
  %pe = getelementptr i8, ptr %buf, i32 %pm
  %c1 = load i8, ptr %pe
  %pz = getelementptr i8, ptr %buf, i32 %pl
  %c2 = load i8, ptr %pz
  %s4 = getelementptr i8, ptr %small, i32 4
  %s5 = getelementptr i8, ptr %small, i32 5
  %d4 = load i8, ptr %s4
  %d5 = load i8, ptr %s5
  %c0i = zext i8 %c0 to i32
  %c1i = zext i8 %c1 to i32
  %c2i = zext i8 %c2 to i32
  %d4i = zext i8 %d4 to i32
  %d5i = zext i8 %d5 to i32
  %dd = add i32 %d4i, %d5i
  call i32 (ptr, ...) @printf(ptr @fmt.cpy, i32 %c0i, i32 %c1i, i32 %c2i, i64 %len, i32 %dd)
  ret void
}

declare void @llvm.memset.p0.i32(ptr, i8, i32, i1)

; An atomic loop (`atomicrmw` `or` with its result used, 8-byte `add`, a
; `cmpxchg`) inside the `try`, and one in the pad.
define void @atomic_try(i32 %rounds) noinline personality ptr @__gxx_personality_v0 {
entry:
  br label %body
body:
  %i = phi i32 [ 0, %entry ], [ %i.next, %latch ]
  %old = atomicrmw or ptr @hits, i32 2 seq_cst
  %w = zext i32 %old to i64
  %ow = atomicrmw add ptr @total, i64 %w seq_cst
  %third = urem i32 %i, 3
  %t = icmp eq i32 %third, 0
  %arg = select i1 %t, i32 %i, i32 0
  invoke void @throw_if(i32 %arg) to label %latch unwind label %pad
pad:
  %lp = landingpad { ptr, i32 } catch ptr @_ZTIi
  %exn = extractvalue { ptr, i32 } %lp, 0
  %obj = call ptr @__cxa_begin_catch(ptr %exn)
  %thrown = load i32, ptr %obj
  call void @__cxa_end_catch()
  %cx = cmpxchg ptr @hits, i32 %old, i32 %thrown seq_cst seq_cst
  %cxf = extractvalue { i32, i1 } %cx, 1
  %cxv = extractvalue { i32, i1 } %cx, 0
  %ow2 = atomicrmw sub ptr @total, i64 1 seq_cst
  br label %latch
latch:
  %i.next = add i32 %i, 1
  %done = icmp eq i32 %i.next, %rounds
  br i1 %done, label %exit, label %body
exit:
  %h = load atomic i32, ptr @hits seq_cst, align 4
  %tt = load atomic i64, ptr @total seq_cst, align 8
  call i32 (ptr, ...) @printf(ptr @fmt.loop, i32 %rounds, i32 %h, i64 %tt, i32 %i)
  ret void
}

define i32 @main() {
  call void @agg(i32 0)
  call void @agg(i32 7)
  call void @agg(i32 -200000)
  call void @pair(i64 9223372036854775807, i64 1, i32 0)
  call void @pair(i64 -5, i64 8589934592, i32 3)
  call void @dyn(i32 5, i32 9)
  call void @dyn(i32 37, i32 200)
  call void @copy(i64 17, i32 0)
  call void @copy(i64 17, i32 4)
  call void @copy(i64 63, i32 1)
  call void @atomic_try(i32 10)
  call void @atomic_try(i32 31)
  ret i32 0
}
