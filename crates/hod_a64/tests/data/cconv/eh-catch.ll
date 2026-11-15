; WP-247: `ccc` functions that catch what `eh.ll`'s `mid_*` let through,
; and call its `catch_in_*` (which C++ cannot name the convention of).

declare win64cc i64 @mid_w(i64)
declare aarch64_vector_pcs i64 @mid_v(i64, <2 x double>)
declare win64cc i64 @catch_in_w(i64, double)
declare aarch64_vector_pcs i64 @catch_in_v(i64, <2 x double>)
declare i32 @__gxx_personality_v0(...)
declare ptr @__cxa_begin_catch(ptr)
declare void @__cxa_end_catch()
@_ZTIi = external constant ptr

define i64 @catch_w(i64 %x) personality ptr @__gxx_personality_v0 {
  %keep = alloca i64, align 8
  store i64 %x, ptr %keep
  %r = invoke win64cc i64 @mid_w(i64 %x) to label %ok unwind label %lp
ok:
  ret i64 %r
lp:
  %l = landingpad { ptr, i32 } catch ptr @_ZTIi
  %e = extractvalue { ptr, i32 } %l, 0
  %o = call ptr @__cxa_begin_catch(ptr %e)
  %v = load i32, ptr %o
  call void @__cxa_end_catch()
  %vw = sext i32 %v to i64
  %k = load i64, ptr %keep
  %s = mul i64 %k, 100
  %r2 = add i64 %vw, %s
  ret i64 %r2
}

define i64 @catch_v(i64 %x) personality ptr @__gxx_personality_v0 {
  %r = invoke aarch64_vector_pcs i64 @mid_v(i64 %x, <2 x double> <double 1.0, double 3.5>) to label %ok unwind label %lp
ok:
  ret i64 %r
lp:
  %l = landingpad { ptr, i32 } catch ptr @_ZTIi
  %e = extractvalue { ptr, i32 } %l, 0
  %o = call ptr @__cxa_begin_catch(ptr %e)
  %v = load i32, ptr %o
  call void @__cxa_end_catch()
  %vw = sext i32 %v to i64
  %r2 = sub i64 0, %vw
  ret i64 %r2
}

define i64 @call_in_w(i64 %x) {
  %r = call win64cc i64 @catch_in_w(i64 %x, double 7.5)
  ret i64 %r
}

define i64 @call_in_v(i64 %x) {
  %r = call aarch64_vector_pcs i64 @catch_in_v(i64 %x, <2 x double> <double 0.0, double 9.0>)
  ret i64 %r
}
