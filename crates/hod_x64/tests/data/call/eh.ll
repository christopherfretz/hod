; WP-125: exceptions through hod's x86-64 frames, driven by eh.cpp
; (tests/call.rs). The port of hod_a64's eh.ll without the forms that need
; the integer, memory and control groups (h_nested, h_alloca); h_throw
; stores through a C++ helper.

@_ZTIi = external constant ptr
@_ZTId = external constant ptr

declare i32 @__gxx_personality_v0(...)
declare void @cxx_throw(i32)
declare void @cxx_throw_which(i32)
declare void @cxx_note(i32)
declare i32 @cxx_handle({ ptr, i32 })
declare i32 @cxx_kind({ ptr, i32 })
declare void @cxx_begin({ ptr, i32 })
declare void @__cxa_rethrow()
declare void @__cxa_end_catch()
declare i64 @cxx_many(i64, i64, i64, i64, i64, i64, i64, i64, i64, i64)
declare i64 @cxx_sum10(i64, i64, i64, i64, i64, i64, i64, i64, i64, i32)

; A cleanup, then resume.
define void @h_cleanup() personality ptr @__gxx_personality_v0 {
  invoke void @cxx_throw(i32 1) to label %ok unwind label %pad
ok:
  ret void
pad:
  %lp = landingpad { ptr, i32 } cleanup
  call void @cxx_note(i32 10)
  resume { ptr, i32 } %lp
}

; A catch by type, handled here.
define i32 @h_catch() personality ptr @__gxx_personality_v0 {
  invoke void @cxx_throw(i32 2) to label %ok unwind label %pad
ok:
  ret i32 0
pad:
  %lp = landingpad { ptr, i32 } catch ptr @_ZTIi
  %v = call i32 @cxx_handle({ ptr, i32 } %lp)
  ret i32 %v
}

; Catch all, rethrow from the handler; the handler ends in a cleanup.
define void @h_rethrow() personality ptr @__gxx_personality_v0 {
  invoke void @cxx_throw(i32 3) to label %ok unwind label %pad
ok:
  ret void
pad:
  %lp = landingpad { ptr, i32 } catch ptr null
  call void @cxx_begin({ ptr, i32 } %lp)
  invoke void @__cxa_rethrow() to label %never unwind label %pad2
never:
  unreachable
pad2:
  %lp2 = landingpad { ptr, i32 } cleanup
  call void @cxx_note(i32 30)
  call void @__cxa_end_catch()
  resume { ptr, i32 } %lp2
}

; Nested hod frames, each with a pad; the outer one's catch does not match.
define void @h_inner() personality ptr @__gxx_personality_v0 {
  invoke void @cxx_throw(i32 4) to label %ok unwind label %pad
ok:
  ret void
pad:
  %lp = landingpad { ptr, i32 } cleanup
  call void @cxx_note(i32 40)
  resume { ptr, i32 } %lp
}

define void @h_outer() personality ptr @__gxx_personality_v0 {
  invoke void @h_inner() to label %ok unwind label %pad
ok:
  ret void
pad:
  %lp = landingpad { ptr, i32 } cleanup catch ptr @_ZTId
  call void @cxx_note(i32 41)
  resume { ptr, i32 } %lp
}

; Two catch clauses: which one matched.
define i32 @h_two(i32 %which) personality ptr @__gxx_personality_v0 {
  invoke void @cxx_throw_which(i32 %which) to label %ok unwind label %pad
ok:
  ret i32 0
pad:
  %lp = landingpad { ptr, i32 } catch ptr @_ZTIi catch ptr @_ZTId
  %k = call i32 @cxx_kind({ ptr, i32 } %lp)
  ret i32 %k
}

; A satisfied exception specification, with a cleanup.
define void @h_filter() personality ptr @__gxx_personality_v0 {
  invoke void @cxx_throw(i32 5) to label %ok unwind label %pad
ok:
  ret void
pad:
  %lp = landingpad { ptr, i32 } cleanup filter [1 x ptr] [ptr @_ZTIi]
  call void @cxx_note(i32 50)
  resume { ptr, i32 } %lp
}

; A noreturn thrower.
define void @h_noreturn() personality ptr @__gxx_personality_v0 {
  invoke void @cxx_throw(i32 6) noreturn to label %never unwind label %pad
never:
  unreachable
pad:
  %lp = landingpad { ptr, i32 } cleanup
  call void @cxx_note(i32 60)
  resume { ptr, i32 } %lp
}

; An invoke with stack arguments (its pad stub restores the stack
; pointer), then a call with stack arguments from the pad.
define i64 @h_stack() personality ptr @__gxx_personality_v0 {
  %r = invoke i64 @cxx_many(i64 0, i64 1, i64 2, i64 3, i64 4, i64 5, i64 6, i64 7, i64 8, i64 9) to label %ok unwind label %pad
ok:
  ret i64 %r
pad:
  %lp = landingpad { ptr, i32 } catch ptr @_ZTIi
  %v = call i32 @cxx_handle({ ptr, i32 } %lp)
  %s = call i64 @cxx_sum10(i64 10, i64 11, i64 12, i64 13, i64 14, i64 15, i64 16, i64 17, i64 18, i32 %v)
  ret i64 %s
}

; A frame without a personality, unwound through.
define void @h_plain() {
  call void @cxx_throw(i32 8)
  ret void
}

; hod throws: `__cxa_allocate_exception` and `__cxa_throw`.
declare ptr @__cxa_allocate_exception(i64)
declare void @__cxa_throw(ptr, ptr, ptr)
declare void @cxx_store(ptr, i32)
define void @h_throw(i32 %v) {
  %e = call ptr @__cxa_allocate_exception(i64 4)
  call void @cxx_store(ptr %e, i32 %v)
  call void @__cxa_throw(ptr %e, ptr @_ZTIi, ptr null) noreturn
  unreachable
}

; A function in a COMDAT group with a pad (eh_comdat.ll holds another
; copy; the linker keeps one, and its FDE and LSDA with it).
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
