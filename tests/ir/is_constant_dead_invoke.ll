; An invoke in the arm __builtin_constant_p of a non-constant guards
; (spec/codegen.md §5.6.8, WP-261), as a C++ front end writes
; `try { if (__builtin_constant_p(x)) undefined_fn(); } catch (...) {}`.
; llc 19 and 22 delete the dead arm with its invoke, and the landing pad
; only that invoke unwinds to; a landing pad a live invoke shares stays.
; @ensure_compile_error is declared and never defined: the program links
; only if neither the invoke nor the deleted landing pad is emitted.
; Needs the C++ runtime.
;@ covers: spec/codegen.md §5.6.8 (Folded constants and the blocks they remove)
;@ tier: quick
;@ link: c++
;@ stdout: own pad 1
;@ stdout: caught 7
;@ stdout: shared pad 2

@_ZTIi = external constant ptr
@.fmt = private unnamed_addr constant [7 x i8] c"%s %d\0A\00"
@.own = private unnamed_addr constant [8 x i8] c"own pad\00"
@.caught = private unnamed_addr constant [7 x i8] c"caught\00"
@.shared = private unnamed_addr constant [11 x i8] c"shared pad\00"

declare i32 @printf(ptr, ...)
declare ptr @__cxa_allocate_exception(i64)
declare void @__cxa_throw(ptr, ptr, ptr) noreturn
declare ptr @__cxa_begin_catch(ptr)
declare void @__cxa_end_catch()
declare i32 @__gxx_personality_v0(...)
declare i1 @llvm.is.constant.i32(i32)
; Never defined.
declare void @ensure_compile_error(i32)

define void @say(ptr %what, i32 %n) noinline {
  %r = call i32 (ptr, ...) @printf(ptr @.fmt, ptr %what, i32 %n)
  ret void
}

define void @thrower(i32 %v) noinline {
  %e = call ptr @__cxa_allocate_exception(i64 4)
  store i32 %v, ptr %e
  call void @__cxa_throw(ptr %e, ptr @_ZTIi, ptr null)
  unreachable
}

; The landing pad is reached only from the dead invoke.
define void @own_pad(i32 %x) noinline personality ptr @__gxx_personality_v0 {
entry:
  %c = call i1 @llvm.is.constant.i32(i32 %x)
  br i1 %c, label %dead, label %out
dead:
  invoke void @ensure_compile_error(i32 %x)
          to label %out unwind label %pad
pad:
  %l = landingpad { ptr, i32 }
          catch ptr null
  %e = extractvalue { ptr, i32 } %l, 0
  %b = call ptr @__cxa_begin_catch(ptr %e)
  call void @ensure_compile_error(i32 0)
  call void @__cxa_end_catch()
  br label %out
out:
  call void @say(ptr @.own, i32 %x)
  ret void
}

; The landing pad is shared with a live invoke that throws.
define void @shared_pad(i32 %x) noinline personality ptr @__gxx_personality_v0 {
entry:
  %c = call i1 @llvm.is.constant.i32(i32 %x)
  br i1 %c, label %dead, label %live
dead:
  invoke void @ensure_compile_error(i32 %x)
          to label %out unwind label %pad
live:
  invoke void @thrower(i32 7)
          to label %out unwind label %pad
pad:
  %l = landingpad { ptr, i32 }
          catch ptr @_ZTIi
  %e = extractvalue { ptr, i32 } %l, 0
  %b = call ptr @__cxa_begin_catch(ptr %e)
  %v = load i32, ptr %b
  call void @say(ptr @.caught, i32 %v)
  call void @__cxa_end_catch()
  br label %out
out:
  call void @say(ptr @.shared, i32 %x)
  ret void
}

define i32 @main() {
  call void @own_pad(i32 1)
  call void @shared_pad(i32 2)
  ret i32 0
}
