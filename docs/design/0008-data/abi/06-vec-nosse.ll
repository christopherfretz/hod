target triple = "i686-unknown-linux-gnu"
define <4 x float> @v4f(<4 x float> %a, <4 x float> %b, <4 x float> %c, <4 x float> %d) #0 { ret <4 x float> %d }
define <2 x i64> @v2i(<2 x i64> %a) #0 { ret <2 x i64> %a }
define <2 x i32> @v2s(<2 x i32> %a) #0 { ret <2 x i32> %a }
define <8 x float> @v8f(<8 x float> %a) #0 { ret <8 x float> %a }
declare <4 x float> @ev(i32, <4 x float>, <4 x float>, <4 x float>, <4 x float>)
define <4 x float> @cv(<4 x float> %a) #0 { %r = call <4 x float> @ev(i32 1, <4 x float> %a, <4 x float> %a, <4 x float> %a, <4 x float> %a)
  ret <4 x float> %r }
define double @fd(double %a, double %b) #0 { %c = fmul double %a, %b
  ret double %c }
define float @ff(float %a) #0 { ret float %a }
define half @fh(half %a) #0 { ret half %a }
attributes #0 = { "target-cpu"="i686" }
