; WP-246 (`spec/abi-x64.md` §13.6): the catching frame of `eh.ll`'s
; `w_pass`, built by `llc` only (`tests/win64cc.rs`, the driver
; `eh.cpp`): `catcher` (System V) invokes `w_pass`, whose callee throws,
; and returns what it catches. It keeps a value in `rbx` across the
; invoke, which the unwinder restores from the rows of the frames it
; passes. It does not look at `rsi`, `rdi` or `xmm6`..`xmm15`: libgcc's
; unwinder installs in a landing pad only the registers its own frame
; saves, the System V ones (measured with `llc`'s `w_pass` as with
; hod's; §13.6).

declare win64cc i64 @w_pass(i64, double)
declare i32 @__gxx_personality_v0(...)
declare ptr @__cxa_begin_catch(ptr)
declare void @__cxa_end_catch()

define i64 @catcher() personality ptr @__gxx_personality_v0 {
entry:
  %b = call i64 asm sideeffect "movq $$1000, %rbx", "={rbx}"()
  %r = invoke win64cc i64 @w_pass(i64 40, double 2.0) to label %ok unwind label %lp
ok:
  ret i64 -1
lp:
  %l = landingpad { ptr, i32 } catch ptr null
  %e = extractvalue { ptr, i32 } %l, 0
  %p = call ptr @__cxa_begin_catch(ptr %e)
  %v = load i64, ptr %p
  call void @__cxa_end_catch()
  %s = add i64 %v, %b
  ret i64 %s
}
