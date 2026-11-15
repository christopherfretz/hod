; WP-246 (`spec/abi-x64.md` §13.6): exceptions through `win64cc` frames
; (`tests/win64cc.rs`, the driver `eh.cpp`, the catcher `eh-catch.ll`).
; `w_pass` lets the exception `thrower` throws pass through its frame
; (whose rows give back the catcher's `rbx`, which `w_pass` writes);
; `w_catch` catches it in a landing pad of its own.

declare void @thrower(i64)
declare i32 @__gxx_personality_v0(...)
declare ptr @__cxa_begin_catch(ptr)
declare void @__cxa_end_catch()

define win64cc i64 @w_pass(i64 %a, double %d) uwtable {
  %b = fptosi double %d to i64
  %s = add i64 %a, %b
  call void @thrower(i64 %s)
  ret i64 0
}

define win64cc i64 @w_catch(i64 %a, double %d) uwtable personality ptr @__gxx_personality_v0 {
entry:
  %k = fmul double %d, 3.0
  invoke void @thrower(i64 %a) to label %ok unwind label %lp
ok:
  ret i64 0
lp:
  %l = landingpad { ptr, i32 } catch ptr null
  %e = extractvalue { ptr, i32 } %l, 0
  %p = call ptr @__cxa_begin_catch(ptr %e)
  %v = load i64, ptr %p
  call void @__cxa_end_catch()
  %ki = fptosi double %k to i64
  %r = add i64 %v, %ki
  ret i64 %r
}
