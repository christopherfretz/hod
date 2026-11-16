; Values live in registers across an `invoke` (WP-314): `thrower` keeps
; values of its own in hod's pool registers (`ebx`, `esi`, `edi`) across
; the calls that throw, so the unwinder must restore the caller's from
; the thrower's CFI rows. `catcher` uses its values in the landing pad
; and on the normal path; `wide` has an `i64` and a `double` result of an
; `invoke`; `typeid` compares the selector with `llvm.eh.typeid.for`;
; `filtered` has a filter and a cleanup that resumes.
target datalayout = "e-m:e-p:32:32-p270:32:32-p271:32:32-p272:64:64-i128:128-f64:32:64-f80:32-n8:16:32-S128"
target triple = "i686-unknown-linux-gnu"

@_ZTIi = external constant ptr
@_ZTIl = external constant ptr
@.fmt = private constant [36 x i8] c"%d %d %d %d %lld %d %d %d %d %d %d\0A\00"

declare i32 @__gxx_personality_v0(...)
declare ptr @__cxa_allocate_exception(i32)
declare void @__cxa_throw(ptr, ptr, ptr)
declare ptr @__cxa_begin_catch(ptr)
declare void @__cxa_end_catch()
declare i32 @printf(ptr, ...)
declare i32 @llvm.eh.typeid.for.p0(ptr)

define void @thrower(i32 %v) noinline {
entry:
  %a = add i32 %v, 1000
  %b = mul i32 %v, 77
  %c = sub i32 0, %v
  %e = call ptr @__cxa_allocate_exception(i32 4)
  %s = add i32 %a, %b
  %s2 = add i32 %s, %c
  %neg = icmp slt i32 %v, 0
  br i1 %neg, label %ret, label %throw
throw:
  store i32 %s2, ptr %e
  call void @__cxa_throw(ptr %e, ptr @_ZTIi, ptr null)
  unreachable
ret:
  ret void
}

define i32 @catcher(i32 %a, i32 %b, i32 %c) noinline personality ptr @__gxx_personality_v0 {
entry:
  %x = mul i32 %a, 3
  %y = add i32 %b, 7
  %z = xor i32 %c, 85
  invoke void @thrower(i32 %a) to label %ok unwind label %pad
ok:
  %s1 = add i32 %x, %y
  %s2 = add i32 %s1, %z
  ret i32 %s2
pad:
  %lp = landingpad { ptr, i32 } catch ptr @_ZTIi
  %exn = extractvalue { ptr, i32 } %lp, 0
  %obj = call ptr @__cxa_begin_catch(ptr %exn)
  %thrown = load i32, ptr %obj
  call void @__cxa_end_catch()
  %t1 = sub i32 %x, %y
  %t2 = sub i32 %t1, %z
  %t3 = add i32 %t2, %thrown
  ret i32 %t3
}

define i64 @wide_value(i32 %v) noinline {
  %w = sext i32 %v to i64
  %m = mul i64 %w, 4294967297
  call void @thrower(i32 %v)
  ret i64 %m
}

define double @fp_value(i32 %v) noinline {
  %d = sitofp i32 %v to double
  %h = fmul double %d, 2.5
  call void @thrower(i32 %v)
  ret double %h
}

define i64 @wide(i32 %v) noinline personality ptr @__gxx_personality_v0 {
entry:
  %w = invoke i64 @wide_value(i32 %v) to label %fp unwind label %pad
fp:
  %d = invoke double @fp_value(i32 %v) to label %ok unwind label %pad
ok:
  %di = fptosi double %d to i64
  %r = add i64 %w, %di
  ret i64 %r
pad:
  %lp = landingpad { ptr, i32 } catch ptr null
  %exn = extractvalue { ptr, i32 } %lp, 0
  %obj = call ptr @__cxa_begin_catch(ptr %exn)
  call void @__cxa_end_catch()
  ret i64 -7
}

define i32 @typeid(i32 %v) noinline personality ptr @__gxx_personality_v0 {
entry:
  invoke void @thrower(i32 %v) to label %ok unwind label %pad
ok:
  ret i32 0
pad:
  %lp = landingpad { ptr, i32 } catch ptr @_ZTIl catch ptr @_ZTIi
  %sel = extractvalue { ptr, i32 } %lp, 1
  %exn = extractvalue { ptr, i32 } %lp, 0
  %id.i = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTIi)
  %is.i = icmp eq i32 %sel, %id.i
  %obj = call ptr @__cxa_begin_catch(ptr %exn)
  call void @__cxa_end_catch()
  %r = select i1 %is.i, i32 11, i32 22
  ret i32 %r
}

define i32 @cleanup(i32 %v) noinline personality ptr @__gxx_personality_v0 {
entry:
  invoke void @thrower(i32 %v) to label %ok unwind label %pad
ok:
  ret i32 1
pad:
  %lp = landingpad { ptr, i32 } cleanup
  %p = extractvalue { ptr, i32 } %lp, 0
  %s = extractvalue { ptr, i32 } %lp, 1
  %v1 = insertvalue { ptr, i32 } poison, ptr %p, 0
  %v2 = insertvalue { ptr, i32 } %v1, i32 %s, 1
  resume { ptr, i32 } %v2
}

define i32 @filtered(i32 %v) noinline personality ptr @__gxx_personality_v0 {
entry:
  invoke i32 @cleanup(i32 %v) to label %ok unwind label %pad
ok:
  ret i32 2
pad:
  %lp = landingpad { ptr, i32 } filter [1 x ptr] [ptr @_ZTIi] catch ptr @_ZTIi
  %exn = extractvalue { ptr, i32 } %lp, 0
  %obj = call ptr @__cxa_begin_catch(ptr %exn)
  %thrown = load i32, ptr %obj
  call void @__cxa_end_catch()
  ret i32 %thrown
}

define i32 @main() {
  %a = call i32 @catcher(i32 5, i32 6, i32 7)
  %b = call i32 @catcher(i32 -5, i32 6, i32 7)
  %c = call i32 @typeid(i32 3)
  %d = call i32 @typeid(i32 -3)
  %e = call i64 @wide(i32 -9)
  %f = call i64 @wide(i32 9)
  %ef = add i64 %e, %f
  %g = call i32 @filtered(i32 2)
  %h = call i32 @filtered(i32 -2)
  %i = call i32 @cleanup(i32 -1)
  %j = call i32 @catcher(i32 0, i32 0, i32 0)
  %k = call i32 @catcher(i32 1000, i32 -1, i32 -1)
  call i32 (ptr, ...) @printf(ptr @.fmt, i32 %a, i32 %b, i32 %c, i32 %d, i64 %ef, i32 %g, i32 %h, i32 %i, i32 %j, i32 %k, i32 0)
  ret i32 0
}
