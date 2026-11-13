; Vectors without AVX: 128-bit in xmm; narrower ones widened; wider split.
declare void @sv(<4 x float>, <2 x i32>, <8 x float>, <16 x i8>, <4 x i1>, <3 x float>)
define void @call_vec(<4 x float> %a, <2 x i32> %b, <8 x float> %c, <16 x i8> %d, <4 x i1> %e, <3 x float> %f) {
  call void @sv(<4 x float> %a, <2 x i32> %b, <8 x float> %c, <16 x i8> %d, <4 x i1> %e, <3 x float> %f)
  ret void }
define <8 x float> @ret_v8f(<8 x float> %a) { ret <8 x float> %a }
define <2 x i32> @ret_v2i(<2 x i32> %a) { ret <2 x i32> %a }
define <16 x float> @ret_v16f(<16 x float> %a) { ret <16 x float> %a }
