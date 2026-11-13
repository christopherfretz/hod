; Floating point: xmm0..xmm7 then the stack; half, bfloat, fp128; returns.
declare void @sinkf(float, double, float, double, float, double, float, double, float, double)
define void @call_fp() {
  call void @sinkf(float 1.0, double 2.0, float 3.0, double 4.0, float 5.0, double 6.0, float 7.0, double 8.0, float 9.0, double 10.0)
  ret void
}
declare half @h(half, bfloat)
define half @call_h(half %a, bfloat %b) { %r = call half @h(half %a, bfloat %b)
  ret half %r }
declare fp128 @q(fp128, fp128)
define fp128 @call_q(fp128 %a) { %r = call fp128 @q(fp128 %a, fp128 %a)
  ret fp128 %r }
define { double, double } @ret_dd(double %a) { %1 = insertvalue { double, double } poison, double %a, 0
  %2 = insertvalue { double, double } %1, double %a, 1
  ret { double, double } %2 }
define { i64, double } @ret_id(i64 %a, double %b) { %1 = insertvalue { i64, double } poison, i64 %a, 0
  %2 = insertvalue { i64, double } %1, double %b, 1
  ret { i64, double } %2 }
define { float, float, float, float } @ret_4f(float %a) {
  %1 = insertvalue { float, float, float, float } poison, float %a, 0
  %2 = insertvalue { float, float, float, float } %1, float %a, 3
  ret { float, float, float, float } %2 }
define { float, float, float, float, float } @ret_5f(float %a) {
  %1 = insertvalue { float, float, float, float, float } poison, float %a, 4
  ret { float, float, float, float, float } %1 }
