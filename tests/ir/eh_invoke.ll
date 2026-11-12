; C++ exceptions through the Itanium ABI as a C++ frontend emits them:
; invoke and landingpad with catch clauses for several types and a
; catch-all, a selector dispatch with llvm.eh.typeid.for, cleanup landing
; pads that resume, a rethrow from inside a handler, an invoke through a
; function pointer, values that live across an invoke into its handler,
; and unwinding through many frames. Needs the C++ runtime.
;@ covers: spec/ir.md §5.4 call (Invoke), exception (LandingPad, Resume), §5.5 intrinsics (eh.typeid.for)
;@ tier: quick
;@ link: c++
;@ unsupported: *
;@ stdout: catch_int(0, 5) = 5
;@ stdout: catch_int(1, 7) = 1007
;@ stdout: returned 1
;@ stdout: cleanup runs
;@ stdout: caught int 42 (tripled 126)
;@ stdout: cleanup runs
;@ stdout: caught text boom (tripled 15)
;@ stdout: cleanup runs
;@ stdout: caught something else (tripled -12)
;@ stdout: rethrowing 9
;@ stdout: catch_from: 9
;@ stdout: catch_from: -1
;@ stdout: unwound 50 frames, caught 77

@_ZTIi = external constant ptr
@_ZTId = external constant ptr
@_ZTIPKc = external constant ptr

@.result = private unnamed_addr constant [24 x i8] c"catch_int(%d, %d) = %d\0A\00"
@.returned = private unnamed_addr constant [13 x i8] c"returned %d\0A\00"
@.cleanup = private unnamed_addr constant [14 x i8] c"cleanup runs\0A\00"
@.int = private unnamed_addr constant [28 x i8] c"caught int %d (tripled %d)\0A\00"
@.text = private unnamed_addr constant [29 x i8] c"caught text %s (tripled %d)\0A\00"
@.other = private unnamed_addr constant [36 x i8] c"caught something else (tripled %d)\0A\00"
@.rethrowing = private unnamed_addr constant [15 x i8] c"rethrowing %d\0A\00"
@.from = private unnamed_addr constant [16 x i8] c"catch_from: %d\0A\00"
@.deep = private unnamed_addr constant [30 x i8] c"unwound %d frames, caught %d\0A\00"
@.boom = private unnamed_addr constant [5 x i8] c"boom\00"
@unwound = global i32 0

declare i32 @printf(ptr, ...)
declare ptr @__cxa_allocate_exception(i64)
declare void @__cxa_throw(ptr, ptr, ptr) noreturn
declare ptr @__cxa_begin_catch(ptr)
declare void @__cxa_end_catch()
declare void @__cxa_rethrow() noreturn
declare i32 @__gxx_personality_v0(...)
declare i32 @llvm.eh.typeid.for.p0(ptr)

define void @throw_int(i32 %v) {
  %e = call ptr @__cxa_allocate_exception(i64 4)
  store i32 %v, ptr %e
  call void @__cxa_throw(ptr %e, ptr @_ZTIi, ptr null)
  unreachable
}

; kind 0 returns v; 1 throws v as an int; 2 throws a string; 3 throws a
; double.
define i32 @may_throw(i32 %kind, i32 %v) {
entry:
  switch i32 %kind, label %return [
    i32 1, label %int
    i32 2, label %text
    i32 3, label %double
  ]
return:
  ret i32 %v
int:
  call void @throw_int(i32 %v)
  unreachable
text:
  %t = call ptr @__cxa_allocate_exception(i64 8)
  store ptr @.boom, ptr %t
  call void @__cxa_throw(ptr %t, ptr @_ZTIPKc, ptr null)
  unreachable
double:
  %d = call ptr @__cxa_allocate_exception(i64 8)
  store double 2.5, ptr %d
  call void @__cxa_throw(ptr %d, ptr @_ZTId, ptr null)
  unreachable
}

; try { return may_throw(kind, v); } catch (int e) { return e + 1000; }
define i32 @catch_int(i32 %kind, i32 %v) personality ptr @__gxx_personality_v0 {
entry:
  %r = invoke i32 @may_throw(i32 %kind, i32 %v)
          to label %normal unwind label %lpad
normal:
  ret i32 %r
lpad:
  %lp = landingpad { ptr, i32 }
          catch ptr @_ZTIi
  %exn = extractvalue { ptr, i32 } %lp, 0
  %sel = extractvalue { ptr, i32 } %lp, 1
  %id = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTIi)
  %match = icmp eq i32 %sel, %id
  br i1 %match, label %catch, label %other
catch:
  %obj = call ptr @__cxa_begin_catch(ptr %exn)
  %val = load i32, ptr %obj
  call void @__cxa_end_catch()
  %res = add i32 %val, 1000
  ret i32 %res
other:
  resume { ptr, i32 } %lp
}

; A frame with a cleanup and no handler.
define i32 @with_cleanup(i32 %kind, i32 %v) personality ptr @__gxx_personality_v0 {
entry:
  %r = invoke i32 @may_throw(i32 %kind, i32 %v)
          to label %normal unwind label %lpad
normal:
  ret i32 %r
lpad:
  %lp = landingpad { ptr, i32 }
          cleanup
  call i32 (ptr, ...) @printf(ptr @.cleanup)
  resume { ptr, i32 } %lp
}

; Handlers for int, for const char* and for everything else; %tripled is
; computed before the invoke and used in each handler.
define void @dispatch(i32 %kind, i32 %v) personality ptr @__gxx_personality_v0 {
entry:
  %tripled = mul i32 %v, 3
  %r = invoke i32 @with_cleanup(i32 %kind, i32 %v)
          to label %normal unwind label %lpad
normal:
  call i32 (ptr, ...) @printf(ptr @.returned, i32 %r)
  ret void
lpad:
  %lp = landingpad { ptr, i32 }
          catch ptr @_ZTIi
          catch ptr @_ZTIPKc
          catch ptr null
  %exn = extractvalue { ptr, i32 } %lp, 0
  %sel = extractvalue { ptr, i32 } %lp, 1
  %int.id = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTIi)
  %is.int = icmp eq i32 %sel, %int.id
  br i1 %is.int, label %catch.int, label %not.int
not.int:
  %text.id = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTIPKc)
  %is.text = icmp eq i32 %sel, %text.id
  br i1 %is.text, label %catch.text, label %catch.any
catch.int:
  %int.obj = call ptr @__cxa_begin_catch(ptr %exn)
  %int.val = load i32, ptr %int.obj
  call i32 (ptr, ...) @printf(ptr @.int, i32 %int.val, i32 %tripled)
  call void @__cxa_end_catch()
  ret void
catch.text:
  ; For a thrown pointer, __cxa_begin_catch returns the pointer itself.
  %text.val = call ptr @__cxa_begin_catch(ptr %exn)
  call i32 (ptr, ...) @printf(ptr @.text, ptr %text.val, i32 %tripled)
  call void @__cxa_end_catch()
  ret void
catch.any:
  call ptr @__cxa_begin_catch(ptr %exn)
  call i32 (ptr, ...) @printf(ptr @.other, i32 %tripled)
  call void @__cxa_end_catch()
  ret void
}

; try { throw_int(v); } catch (...) { print; throw; }
define void @rethrower(i32 %v) personality ptr @__gxx_personality_v0 {
entry:
  invoke void @throw_int(i32 %v)
          to label %impossible unwind label %lpad
lpad:
  %lp = landingpad { ptr, i32 }
          catch ptr null
  %exn = extractvalue { ptr, i32 } %lp, 0
  %obj = call ptr @__cxa_begin_catch(ptr %exn)
  %val = load i32, ptr %obj
  call i32 (ptr, ...) @printf(ptr @.rethrowing, i32 %val)
  invoke void @__cxa_rethrow()
          to label %impossible unwind label %lpad.end
lpad.end:
  %lp2 = landingpad { ptr, i32 }
          cleanup
  call void @__cxa_end_catch()
  resume { ptr, i32 } %lp2
impossible:
  unreachable
}

; Recurses n frames down and throws 77 from the bottom; every frame on
; the way back up counts itself in a cleanup.
define void @descend(i32 %n) personality ptr @__gxx_personality_v0 {
entry:
  %bottom = icmp eq i32 %n, 0
  br i1 %bottom, label %throw, label %deeper
throw:
  call void @throw_int(i32 77)
  unreachable
deeper:
  %below = sub i32 %n, 1
  invoke void @descend(i32 %below)
          to label %ok unwind label %lpad
ok:
  ret void
lpad:
  %lp = landingpad { ptr, i32 }
          cleanup
  %old = load i32, ptr @unwound
  %new = add i32 %old, 1
  store i32 %new, ptr @unwound
  resume { ptr, i32 } %lp
}

; Calls fn(arg) through the pointer; returns the int it throws, or -1 if
; it returns.
define i32 @catch_from(ptr %fn, i32 %arg) personality ptr @__gxx_personality_v0 {
entry:
  invoke void %fn(i32 %arg)
          to label %normal unwind label %lpad
normal:
  ret i32 -1
lpad:
  %lp = landingpad { ptr, i32 }
          catch ptr @_ZTIi
  %exn = extractvalue { ptr, i32 } %lp, 0
  %sel = extractvalue { ptr, i32 } %lp, 1
  %id = call i32 @llvm.eh.typeid.for.p0(ptr @_ZTIi)
  %match = icmp eq i32 %sel, %id
  br i1 %match, label %catch, label %other
catch:
  %obj = call ptr @__cxa_begin_catch(ptr %exn)
  %val = load i32, ptr %obj
  call void @__cxa_end_catch()
  ret i32 %val
other:
  resume { ptr, i32 } %lp
}

define void @quiet(i32 %v) {
  ret void
}

define i32 @main() {
  %a = call i32 @catch_int(i32 0, i32 5)
  call i32 (ptr, ...) @printf(ptr @.result, i32 0, i32 5, i32 %a)
  %b = call i32 @catch_int(i32 1, i32 7)
  call i32 (ptr, ...) @printf(ptr @.result, i32 1, i32 7, i32 %b)
  call void @dispatch(i32 0, i32 1)
  call void @dispatch(i32 1, i32 42)
  call void @dispatch(i32 2, i32 5)
  call void @dispatch(i32 3, i32 -4)
  %c = call i32 @catch_from(ptr @rethrower, i32 9)
  call i32 (ptr, ...) @printf(ptr @.from, i32 %c)
  %d = call i32 @catch_from(ptr @quiet, i32 9)
  call i32 (ptr, ...) @printf(ptr @.from, i32 %d)
  %e = call i32 @catch_from(ptr @descend, i32 50)
  %frames = load i32, ptr @unwound
  call i32 (ptr, ...) @printf(ptr @.deep, i32 %frames, i32 %e)
  ret i32 0
}
