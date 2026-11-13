; WP-125: a second object holding the COMDAT function of eh.ll
; (tests/call.rs), and a caller of it.

@_ZTIi = external constant ptr
declare i32 @__gxx_personality_v0(...)
declare void @cxx_throw(i32)
declare i32 @cxx_handle({ ptr, i32 })

$h_comdat = comdat any
define linkonce_odr i32 @h_comdat(i32 %x) comdat personality ptr @__gxx_personality_v0 {
  invoke void @cxx_throw(i32 %x) to label %ok unwind label %pad
ok:
  ret i32 0
pad:
  %lp = landingpad { ptr, i32 } catch ptr @_ZTIi
  %v = call i32 @cxx_handle({ ptr, i32 } %lp)
  ret i32 %v
}

define i32 @h_comdat2(i32 %x) {
  %r = call i32 @h_comdat(i32 %x)
  ret i32 %r
}
