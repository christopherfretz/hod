; WP-247 (`spec/abi-a64.md` §13.4, §13.5): exceptions through `win64cc`
; and `aarch64_vector_pcs` frames (`mid_*`) and into landing pads of such
; frames (`catch_in_*`). `do_throw` and `mid_c` are C++ (`eh.cpp`): they
; throw the `int` value of a non-zero argument.

declare void @do_throw(i64)
declare i64 @mid_c(i64)
declare i32 @__gxx_personality_v0(...)
declare ptr @__cxa_begin_catch(ptr)
declare void @__cxa_end_catch()
@_ZTIi = external constant ptr

define win64cc i64 @mid_w(i64 %x) {
  %y = add i64 %x, 0
  call void @do_throw(i64 %y)
  %r = mul i64 %x, 2
  ret i64 %r
}

define aarch64_vector_pcs i64 @mid_v(i64 %x, <2 x double> %v) {
  call void @do_throw(i64 %x)
  %w = fadd <2 x double> %v, %v
  %e = extractelement <2 x double> %w, i32 1
  %i = fptosi double %e to i64
  %r = add i64 %x, %i
  ret i64 %r
}

define win64cc i64 @catch_in_w(i64 %x, double %d) personality ptr @__gxx_personality_v0 {
  %r = invoke i64 @mid_c(i64 %x) to label %ok unwind label %lp
ok:
  %di = fptosi double %d to i64
  %s = add i64 %r, %di
  ret i64 %s
lp:
  %l = landingpad { ptr, i32 } catch ptr @_ZTIi
  %e = extractvalue { ptr, i32 } %l, 0
  %o = call ptr @__cxa_begin_catch(ptr %e)
  %v = load i32, ptr %o
  call void @__cxa_end_catch()
  %vw = sext i32 %v to i64
  %r2 = add i64 %vw, 1000
  ret i64 %r2
}

define aarch64_vector_pcs i64 @catch_in_v(i64 %x, <2 x double> %v) personality ptr @__gxx_personality_v0 {
  %r = invoke i64 @mid_c(i64 %x) to label %ok unwind label %lp
ok:
  %e1 = extractelement <2 x double> %v, i32 1
  %ei = fptosi double %e1 to i64
  %s = add i64 %r, %ei
  ret i64 %s
lp:
  %l = landingpad { ptr, i32 } catch ptr @_ZTIi
  %e = extractvalue { ptr, i32 } %l, 0
  %o = call ptr @__cxa_begin_catch(ptr %e)
  %val = load i32, ptr %o
  call void @__cxa_end_catch()
  %vw = sext i32 %val to i64
  %r2 = add i64 %vw, 2000
  ret i64 %r2
}
