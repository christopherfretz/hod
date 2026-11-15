target triple = "i686-unknown-linux-gnu"
define float @rf(float %a, double %b) { %c = fptrunc double %b to float
  %d = fadd float %a, %c
  ret float %d }
define double @rd(double %a) { %b = fadd double %a, %a
  ret double %b }
define x86_fp80 @rx(x86_fp80 %a) { ret x86_fp80 %a }
declare double @extd(float, double, x86_fp80)
define double @calld() { %r = call double @extd(float 1.0, double 2.0, x86_fp80 0xK3FFF8000000000000000)
  ret double %r }
define void @calldrop() { %r = call double @extd(float 1.0, double 2.0, x86_fp80 0xK3FFF8000000000000000)
  ret void }
define fp128 @rq(fp128 %a) { ret fp128 %a }
define half @rh(half %a) { ret half %a }
