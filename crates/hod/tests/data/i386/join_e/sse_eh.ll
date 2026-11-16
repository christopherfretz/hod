; i386 join E (1): `invoke` of functions returning `float`, `double` and
; a 128-bit vector in SSE2 functions (`"target-cpu"="pentium4"`), the
; landing pad using values computed before the `invoke`, a `float`
; computed before the `invoke` used in both successors.
target triple = "i686-unknown-linux-gnu"

@_ZTIi = external constant ptr
@fmt_f = private constant [24 x i8] c"%s f=%.9g d=%.17g k=%d\0A\00"
@fmt_v = private constant [26 x i8] c"%s v=%.9g %.9g %.9g %.9g\0A\00"
@s_ok = private constant [3 x i8] c"ok\00"
@s_pad = private constant [4 x i8] c"pad\00"

declare i32 @printf(ptr, ...)
declare ptr @__cxa_allocate_exception(i32)
declare void @__cxa_throw(ptr, ptr, ptr)
declare ptr @__cxa_begin_catch(ptr)
declare void @__cxa_end_catch()
declare i32 @__gxx_personality_v0(...)

define internal void @throw_int(i32 %v) #0 {
  %e = call ptr @__cxa_allocate_exception(i32 4)
  store i32 %v, ptr %e
  call void @__cxa_throw(ptr %e, ptr @_ZTIi, ptr null)
  unreachable
}

define internal float @get_f(i32 %k, float %x) #0 {
  %odd = and i32 %k, 1
  %t = icmp ne i32 %odd, 0
  br i1 %t, label %thr, label %ok
thr:
  call void @throw_int(i32 %k)
  unreachable
ok:
  %kf = sitofp i32 %k to float
  %r = fmul float %x, %kf
  ret float %r
}

define internal double @get_d(i32 %k, double %x) #0 {
  %m = urem i32 %k, 3
  %t = icmp eq i32 %m, 2
  br i1 %t, label %thr, label %ok
thr:
  call void @throw_int(i32 %k)
  unreachable
ok:
  %kd = sitofp i32 %k to double
  %r = fdiv double %x, %kd
  ret double %r
}

define internal <4 x float> @get_v(i32 %k, <4 x float> %x) #0 {
  %m = urem i32 %k, 4
  %t = icmp eq i32 %m, 3
  br i1 %t, label %thr, label %ok
thr:
  call void @throw_int(i32 %k)
  unreachable
ok:
  %kf = sitofp i32 %k to float
  %s = insertelement <4 x float> poison, float %kf, i32 0
  %b = shufflevector <4 x float> %s, <4 x float> poison, <4 x i32> zeroinitializer
  %r = fadd <4 x float> %x, %b
  ret <4 x float> %r
}

define internal void @report(ptr %what, float %f, double %d, i32 %k) #0 {
  %fd = fpext float %f to double
  call i32 (ptr, ...) @printf(ptr @fmt_f, ptr %what, double %fd, double %d, i32 %k)
  ret void
}

define internal void @report_v(ptr %what, <4 x float> %v) #0 {
  %a = extractelement <4 x float> %v, i32 0
  %b = extractelement <4 x float> %v, i32 1
  %c = extractelement <4 x float> %v, i32 2
  %d = extractelement <4 x float> %v, i32 3
  %ad = fpext float %a to double
  %bd = fpext float %b to double
  %cd = fpext float %c to double
  %dd = fpext float %d to double
  call i32 (ptr, ...) @printf(ptr @fmt_v, ptr %what, double %ad, double %bd, double %cd, double %dd)
  ret void
}

; A `float` computed before the `invoke` is used on both edges; the
; results feed arithmetic in the normal successor; the pad uses the
; `double` and the vector computed before.
define internal void @one(i32 %k) #0 personality ptr @__gxx_personality_v0 {
entry:
  %kf = sitofp i32 %k to float
  %pre = fmul float %kf, 0x3FB99999A0000000
  %pred = fpext float %pre to double
  %pre2 = fadd double %pred, 1.000000e-01
  %pv0 = insertelement <4 x float> <float 1.0, float -0.0, float 0x36A0000000000000, float 3.5>, float %pre, i32 1
  %f = invoke float @get_f(i32 %k, float %pre)
          to label %got_f unwind label %pad
got_f:
  %d = invoke double @get_d(i32 %k, double %pre2)
          to label %got_d unwind label %pad
got_d:
  %v = invoke <4 x float> @get_v(i32 %k, <4 x float> %pv0)
          to label %got_v unwind label %pad
got_v:
  %sum = fadd float %f, %pre
  %dd = fmul double %d, %pre2
  call void @report(ptr @s_ok, float %sum, double %dd, i32 %k)
  %v2 = fmul <4 x float> %v, %pv0
  call void @report_v(ptr @s_ok, <4 x float> %v2)
  ret void
pad:
  %lp = landingpad { ptr, i32 } catch ptr @_ZTIi
  %obj = extractvalue { ptr, i32 } %lp, 0
  %c = call ptr @__cxa_begin_catch(ptr %obj)
  %thrown = load i32, ptr %c
  %pfx = fsub float %pre, 2.500000e-01
  call void @report(ptr @s_pad, float %pfx, double %pre2, i32 %thrown)
  %pv1 = fsub <4 x float> %pv0, <float 0.5, float 0.5, float 0.5, float 0.5>
  call void @report_v(ptr @s_pad, <4 x float> %pv1)
  call void @__cxa_end_catch()
  ret void
}

define i32 @main() #0 {
entry:
  br label %loop
loop:
  %k = phi i32 [ 0, %entry ], [ %k1, %loop ]
  call void @one(i32 %k)
  %k1 = add i32 %k, 1
  %done = icmp eq i32 %k1, 9
  br i1 %done, label %out, label %loop
out:
  ret i32 0
}

attributes #0 = { "target-cpu"="pentium4" }
