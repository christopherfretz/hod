; -mbranch-protection=pac-ret+b-key mixed with pac-ret in one object, as
; clang 22 writes it (module flags of value 2; each function carries its
; policy), and a C++ exception unwound through signed frames: the A-key
; ones under a "zPLR" CIE, the B-key ones under "zPLRB", each with
; DW_CFA_AARCH64_negate_ra_state after the signing instruction, so the
; unwinder authenticates every return address it reads with the right
; key. A cleanup in a B-key frame resumes the exception. Without
; branch-target-enforcement (llc 19 puts no `bti j` on a landing pad, so
; under BTI its own program would fault; spec/codegen.md §5.6.26). On
; AArch64 the object's .note.gnu.property has the PAC bit only. On x86-64
; llc and hod ignore every one of these names.
;@ covers: spec/codegen.md §5.6.26 branch protection, spec/eh.md §4.5 negate_ra_state and the B key
;@ tier: quick
;@ link: c++
;@ stdout: caught 1234 through 4 signed frames, 1 cleanup
;@ stdout: not thrown 0, 0 cleanups

@_ZTIi = external constant ptr
@cleanups = global i32 0

@.caught = private unnamed_addr constant [47 x i8] c"caught %d through 4 signed frames, %d cleanup\0A\00"
@.none = private unnamed_addr constant [28 x i8] c"not thrown %d, %d cleanups\0A\00"

declare i32 @printf(ptr, ...)
declare ptr @__cxa_allocate_exception(i64)
declare void @__cxa_throw(ptr, ptr, ptr) noreturn
declare ptr @__cxa_begin_catch(ptr)
declare void @__cxa_end_catch()
declare i32 @__gxx_personality_v0(...)
declare i32 @llvm.eh.typeid.for.p0(ptr)

define internal i32 @twice(i32 %x) noinline #0 {
  %r = shl i32 %x, 1
  ret i32 %r
}

define void @thrower(i32 %v) noinline #1 {
  %c = icmp eq i32 %v, 0
  br i1 %c, label %quiet, label %throw
quiet:
  ret void
throw:
  %e = call ptr @__cxa_allocate_exception(i64 4)
  store i32 %v, ptr %e
  call void @__cxa_throw(ptr %e, ptr @_ZTIi, ptr null)
  unreachable
}

define void @middle(i32 %v) noinline #0 {
  call void @thrower(i32 %v)
  %w = call i32 @twice(i32 %v)
  ret void
}

; B key, a cleanup that counts and resumes.
define void @cleaner(i32 %v) noinline #1 personality ptr @__gxx_personality_v0 {
entry:
  %big = alloca [5000 x i8], align 16
  store volatile i8 1, ptr %big
  invoke void @middle(i32 %v) to label %done unwind label %pad
done:
  ret void
pad:
  %lp = landingpad { ptr, i32 } cleanup
  %n = load i32, ptr @cleanups
  %n1 = add i32 %n, 1
  store i32 %n1, ptr @cleanups
  resume { ptr, i32 } %lp
}

define i32 @catcher(i32 %v) noinline #0 personality ptr @__gxx_personality_v0 {
entry:
  invoke void @cleaner(i32 %v) to label %done unwind label %pad
done:
  ret i32 0
pad:
  %lp = landingpad { ptr, i32 } catch ptr @_ZTIi
  %exn = extractvalue { ptr, i32 } %lp, 0
  %sel = extractvalue { ptr, i32 } %lp, 1
  %id = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTIi)
  %is = icmp eq i32 %sel, %id
  br i1 %is, label %int, label %other
int:
  %obj = call ptr @__cxa_begin_catch(ptr %exn)
  %val = load i32, ptr %obj
  call void @__cxa_end_catch()
  ret i32 %val
other:
  ret i32 -1
}

define i32 @main() #0 {
  %c = call i32 @catcher(i32 1234)
  %k = load i32, ptr @cleanups
  call i32 (ptr, ...) @printf(ptr @.caught, i32 %c, i32 %k)
  store i32 0, ptr @cleanups
  %n = call i32 @catcher(i32 0)
  %k0 = load i32, ptr @cleanups
  call i32 (ptr, ...) @printf(ptr @.none, i32 %n, i32 %k0)
  ret i32 0
}

attributes #0 = { "sign-return-address"="non-leaf" "sign-return-address-key"="a_key" }
attributes #1 = { "sign-return-address"="all" "sign-return-address-key"="b_key" }

!llvm.module.flags = !{!0, !1, !2}
!0 = !{i32 8, !"branch-target-enforcement", i32 0}
!1 = !{i32 8, !"sign-return-address", i32 2}
!2 = !{i32 8, !"sign-return-address-all", i32 0}
