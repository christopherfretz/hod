; Edge cases of the assignment, each made visible by distinct values.
; {float x4}: the third and fourth floats are returned in st(0), st(1).
define { float, float, float, float } @ret_4f(float %a, float %b, float %c, float %d) {
  %1 = insertvalue { float, float, float, float } poison, float %a, 0
  %2 = insertvalue { float, float, float, float } %1, float %b, 1
  %3 = insertvalue { float, float, float, float } %2, float %c, 2
  %4 = insertvalue { float, float, float, float } %3, float %d, 3
  ret { float, float, float, float } %4 }
; {double x3}
define { double, double, double } @ret_3d(double %a, double %b, double %c) {
  %1 = insertvalue { double, double, double } poison, double %a, 0
  %2 = insertvalue { double, double, double } %1, double %b, 1
  %3 = insertvalue { double, double, double } %2, double %c, 2
  ret { double, double, double } %3 }
; {i64, i64} after five i64: split between r9 and the stack?
define i64 @split_pair(i64 %a, i64 %b, i64 %c, i64 %d, i64 %e, { i64, i64 } %p) {
  %x = extractvalue { i64, i64 } %p, 0
  %y = extractvalue { i64, i64 } %p, 1
  %s = sub i64 %x, %y
  ret i64 %s }
; [2 x i128] / i128 in a struct
define i128 @in_struct(i64 %a, { i128 } %p) { %x = extractvalue { i128 } %p, 0
  ret i128 %x }
; vectors: which register or stack slot each one takes
define <2 x i32> @v_second(<4 x float> %a, <2 x i32> %b) { ret <2 x i32> %b }
define <4 x float> @v8_hi(<8 x float> %a) { %h = shufflevector <8 x float> %a, <8 x float> poison, <4 x i32> <i32 4, i32 5, i32 6, i32 7>
  ret <4 x float> %h }
define <4 x i1> @v_i1(<4 x i1> %a) { %n = xor <4 x i1> %a, <i1 1, i1 1, i1 1, i1 1>
  ret <4 x i1> %n }
define <3 x float> @v3(<3 x float> %a) { ret <3 x float> %a }
define <4 x float> @v_ninth(<4 x float> %a, <4 x float> %b, <4 x float> %c, <4 x float> %d, <4 x float> %e, <4 x float> %f, <4 x float> %g, <4 x float> %h, <4 x float> %i) { ret <4 x float> %i }
define <16 x float> @ret_v16f(<16 x float> %a) { ret <16 x float> %a }
define <32 x float> @ret_v32f(<32 x float> %a) { ret <32 x float> %a }
define <2 x i64> @v2i64(<2 x i64> %a) { ret <2 x i64> %a }
define <1 x i64> @v1i64(<1 x i64> %a) { ret <1 x i64> %a }
define <2 x float> @v2f(<2 x float> %a) { ret <2 x float> %a }
; i1 and i8 arguments without attributes; a plain i1 call argument
declare void @take_i1(i1)
define void @pass_i1(i32 %x) { %b = icmp eq i32 %x, 3
  call void @take_i1(i1 %b)
  ret void }
; float on the stack after eight xmm, and an fp128 on the stack (align 16)
define fp128 @q_stack(double %a, double %b, double %c, double %d, double %e, double %f, double %g, double %h, float %i, fp128 %q) { ret fp128 %q }
