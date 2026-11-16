; `invoke` of callees with a convention of their own (WP-314): `make`
; returns through an `sret` pointer that it pops (`ret $4`), `big` returns
; an `i64` in `edx:eax`, `fast` is `fastcc`
; (arguments in `ecx`, `edx`). Each one throws when `%v` is its number
; and returns otherwise; `calls` keeps values live across all three.
;
; The x87 rule (`spec/codegen-x86.md` §11, §14): `x87_live` computes a
; `double` and a `float` before an `invoke` and uses them in the normal
; successor and in the landing pad. `invoke` ends its block, so `llc`
; (its fast allocator spills at the block end) and hod (an `invoke` is a
; barrier) both round them to their type: the products overflow to an
; infinity. `x87_pad` computes and uses them inside the pad, with no
; barrier between: both keep 80 bits and the quotients are finite.
;
; The test replaces `attributes #0` to build the module at a CPU with
; `cmov` as well.
target datalayout = "e-m:e-p:32:32-p270:32:32-p271:32:32-p272:64:64-i128:128-f64:32:64-f80:32-n8:16:32-S128"
target triple = "i686-unknown-linux-gnu"

%struct.S = type { i32, i32, i32 }

@_ZTIi = external constant ptr
@.fmt = private constant [14 x i8] c"%d: %d %d %d\0A\00"

declare i32 @__gxx_personality_v0(...)
declare ptr @__cxa_allocate_exception(i32)
declare void @__cxa_throw(ptr, ptr, ptr)
declare ptr @__cxa_begin_catch(ptr)
declare void @__cxa_end_catch()
declare i32 @printf(ptr, ...)

define void @throw_if(i32 %v, i32 %id) #0 {
entry:
  %is = icmp eq i32 %v, %id
  br i1 %is, label %throw, label %ret
throw:
  %e = call ptr @__cxa_allocate_exception(i32 4)
  %t = mul i32 %id, 100
  store i32 %t, ptr %e
  call void @__cxa_throw(ptr %e, ptr @_ZTIi, ptr null)
  unreachable
ret:
  ret void
}

define void @make(ptr sret(%struct.S) align 4 %out, i32 %v) #0 {
  call void @throw_if(i32 %v, i32 1)
  %y = mul i32 %v, 2
  %z = mul i32 %v, 3
  %p1 = getelementptr %struct.S, ptr %out, i32 0, i32 1
  %p2 = getelementptr %struct.S, ptr %out, i32 0, i32 2
  store i32 %v, ptr %out
  store i32 %y, ptr %p1
  store i32 %z, ptr %p2
  ret void
}

define i64 @big(i32 %v) #0 {
  call void @throw_if(i32 %v, i32 2)
  %w = zext i32 %v to i64
  %h = shl i64 %w, 38
  %r = or i64 %h, 9
  ret i64 %r
}

define fastcc i32 @fast(i32 %a, i32 %b, i32 %c) #0 {
  call void @throw_if(i32 %a, i32 3)
  %m = mul i32 %a, %b
  %r = add i32 %m, %c
  ret i32 %r
}

define i32 @calls(i32 %v) #0 personality ptr @__gxx_personality_v0 {
entry:
  %s = alloca %struct.S, align 4
  %k = mul i32 %v, 13
  %j = add i32 %v, 1000
  invoke void @make(ptr sret(%struct.S) align 4 %s, i32 %v) to label %a unwind label %pad
a:
  %w = invoke i64 @big(i32 %v) to label %b unwind label %pad
b:
  %f = invoke fastcc i32 @fast(i32 %v, i32 %k, i32 %j) to label %c unwind label %pad
c:
  %p1 = getelementptr %struct.S, ptr %s, i32 0, i32 1
  %p2 = getelementptr %struct.S, ptr %s, i32 0, i32 2
  %s0 = load i32, ptr %s
  %s1 = load i32, ptr %p1
  %s2 = load i32, ptr %p2
  %wh = lshr i64 %w, 32
  %wt = trunc i64 %wh to i32
  %wl = trunc i64 %w to i32
  %t1 = add i32 %s0, %s1
  %t2 = add i32 %t1, %s2
  %t3 = add i32 %t2, %wt
  %t4 = add i32 %t3, %wl
  %t5 = add i32 %t4, %f
  %t6 = add i32 %t5, %k
  ret i32 %t6
pad:
  %lp = landingpad { ptr, i32 } catch ptr @_ZTIi
  %exn = extractvalue { ptr, i32 } %lp, 0
  %obj = call ptr @__cxa_begin_catch(ptr %exn)
  %thrown = load i32, ptr %obj
  call void @__cxa_end_catch()
  %u1 = add i32 %thrown, %k
  %u2 = add i32 %u1, %j
  %u3 = sub i32 0, %u2
  ret i32 %u3
}

; Bit 0: the `double` quotient is finite; bit 1: the `float` one is;
; bit 4: the landing pad ran.
define i32 @x87_live(double %a, double %b, float %fa, float %fb, i32 %v) #0 personality ptr @__gxx_personality_v0 {
entry:
  %m = fmul double %a, %b
  %fm = fmul float %fa, %fb
  invoke void @throw_if(i32 %v, i32 4) to label %ok unwind label %pad
ok:
  %r = fdiv double %m, %b
  %fr = fdiv float %fm, %fb
  %rf = fcmp one double %r, 0x7FF0000000000000
  %frf = fcmp one float %fr, 0x7FF0000000000000
  %rb = zext i1 %rf to i32
  %frb = zext i1 %frf to i32
  %frs = shl i32 %frb, 1
  %ro = or i32 %rb, %frs
  ret i32 %ro
pad:
  %lp = landingpad { ptr, i32 } catch ptr null
  %exn = extractvalue { ptr, i32 } %lp, 0
  %obj = call ptr @__cxa_begin_catch(ptr %exn)
  call void @__cxa_end_catch()
  %q = fdiv double %m, %b
  %fq = fdiv float %fm, %fb
  %qf = fcmp one double %q, 0x7FF0000000000000
  %fqf = fcmp one float %fq, 0x7FF0000000000000
  %qb = zext i1 %qf to i32
  %fqb = zext i1 %fqf to i32
  %fqs = shl i32 %fqb, 1
  %qo = or i32 %qb, %fqs
  %qp = or i32 %qo, 16
  ret i32 %qp
}

; The same quantities computed and used inside the pad: one run, no
; barrier, so the quotients keep 80 bits and are finite (bits 0 and 1).
define i32 @x87_pad(double %a, double %b, float %fa, float %fb, i32 %v) #0 personality ptr @__gxx_personality_v0 {
entry:
  invoke void @throw_if(i32 %v, i32 5) to label %ok unwind label %pad
ok:
  ret i32 0
pad:
  %lp = landingpad { ptr, i32 } catch ptr null
  %exn = extractvalue { ptr, i32 } %lp, 0
  %obj = call ptr @__cxa_begin_catch(ptr %exn)
  call void @__cxa_end_catch()
  %m = fmul double %a, %b
  %fm = fmul float %fa, %fb
  %q = fdiv double %m, %b
  %fq = fdiv float %fm, %fb
  %qf = fcmp one double %q, 0x7FF0000000000000
  %fqf = fcmp one float %fq, 0x7FF0000000000000
  %qb = zext i1 %qf to i32
  %fqb = zext i1 %fqf to i32
  %fqs = shl i32 %fqb, 1
  %qo = or i32 %qb, %fqs
  %qp = or i32 %qo, 16
  ret i32 %qp
}

define i32 @main() {
  br label %loop
loop:
  %i = phi i32 [ 0, %0 ], [ %n, %loop ]
  %c = call i32 @calls(i32 %i)
  %x = call i32 @x87_live(double 1.0e300, double 1.0e300, float 0x46293E5940000000, float 0x46293E5940000000, i32 %i)
  %y = call i32 @x87_pad(double 1.0e300, double 1.0e300, float 0x46293E5940000000, float 0x46293E5940000000, i32 %i)
  call i32 (ptr, ...) @printf(ptr @.fmt, i32 %i, i32 %c, i32 %x, i32 %y)
  %n = add i32 %i, 1
  %done = icmp eq i32 %n, 6
  br i1 %done, label %out, label %loop
out:
  ret i32 0
}

attributes #0 = { noinline }
