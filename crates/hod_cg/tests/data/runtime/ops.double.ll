; WP-117 (spec/codegen.md 7.3): which runtime routine llc calls for each
; operation, by type; crates/hod_cg/tests/runtime_names.rs compares every
; function with hod_cg::sym's tables under llc 19 and 22. Generated.

define double @op_sin(double %a) {
  %r = call double @llvm.sin.f64(double %a)
  ret double %r
}

define double @op_cos(double %a) {
  %r = call double @llvm.cos.f64(double %a)
  ret double %r
}

define double @op_tan(double %a) {
  %r = call double @llvm.tan.f64(double %a)
  ret double %r
}

define double @op_asin(double %a) {
  %r = call double @llvm.asin.f64(double %a)
  ret double %r
}

define double @op_acos(double %a) {
  %r = call double @llvm.acos.f64(double %a)
  ret double %r
}

define double @op_atan(double %a) {
  %r = call double @llvm.atan.f64(double %a)
  ret double %r
}

define double @op_sinh(double %a) {
  %r = call double @llvm.sinh.f64(double %a)
  ret double %r
}

define double @op_cosh(double %a) {
  %r = call double @llvm.cosh.f64(double %a)
  ret double %r
}

define double @op_tanh(double %a) {
  %r = call double @llvm.tanh.f64(double %a)
  ret double %r
}

define double @op_exp(double %a) {
  %r = call double @llvm.exp.f64(double %a)
  ret double %r
}

define double @op_exp2(double %a) {
  %r = call double @llvm.exp2.f64(double %a)
  ret double %r
}

define double @op_exp10(double %a) {
  %r = call double @llvm.exp10.f64(double %a)
  ret double %r
}

define double @op_log(double %a) {
  %r = call double @llvm.log.f64(double %a)
  ret double %r
}

define double @op_log2(double %a) {
  %r = call double @llvm.log2.f64(double %a)
  ret double %r
}

define double @op_log10(double %a) {
  %r = call double @llvm.log10.f64(double %a)
  ret double %r
}

define double @op_sqrt(double %a) {
  %r = call double @llvm.sqrt.f64(double %a)
  ret double %r
}

define double @op_floor(double %a) {
  %r = call double @llvm.floor.f64(double %a)
  ret double %r
}

define double @op_ceil(double %a) {
  %r = call double @llvm.ceil.f64(double %a)
  ret double %r
}

define double @op_trunc(double %a) {
  %r = call double @llvm.trunc.f64(double %a)
  ret double %r
}

define double @op_round(double %a) {
  %r = call double @llvm.round.f64(double %a)
  ret double %r
}

define double @op_roundeven(double %a) {
  %r = call double @llvm.roundeven.f64(double %a)
  ret double %r
}

define double @op_nearbyint(double %a) {
  %r = call double @llvm.nearbyint.f64(double %a)
  ret double %r
}

define double @op_rint(double %a) {
  %r = call double @llvm.rint.f64(double %a)
  ret double %r
}

define double @op_pow(double %a, double %b) {
  %r = call double @llvm.pow.f64(double %a, double %b)
  ret double %r
}

define double @op_atan2(double %a, double %b) {
  %r = call double @llvm.atan2.f64(double %a, double %b)
  ret double %r
}

define double @op_minnum(double %a, double %b) {
  %r = call double @llvm.minnum.f64(double %a, double %b)
  ret double %r
}

define double @op_maxnum(double %a, double %b) {
  %r = call double @llvm.maxnum.f64(double %a, double %b)
  ret double %r
}

define double @op_minimum(double %a, double %b) {
  %r = call double @llvm.minimum.f64(double %a, double %b)
  ret double %r
}

define double @op_maximum(double %a, double %b) {
  %r = call double @llvm.maximum.f64(double %a, double %b)
  ret double %r
}

define double @op_minimumnum(double %a, double %b) {
  %r = call double @llvm.minimumnum.f64(double %a, double %b)
  ret double %r
}

define double @op_maximumnum(double %a, double %b) {
  %r = call double @llvm.maximumnum.f64(double %a, double %b)
  ret double %r
}

define double @op_add(double %a, double %b) {
  %r = fadd double %a, %b
  ret double %r
}

define double @op_sub(double %a, double %b) {
  %r = fsub double %a, %b
  ret double %r
}

define double @op_mul(double %a, double %b) {
  %r = fmul double %a, %b
  ret double %r
}

define double @op_div(double %a, double %b) {
  %r = fdiv double %a, %b
  ret double %r
}

define double @op_rem(double %a, double %b) {
  %r = frem double %a, %b
  ret double %r
}

define double @op_fma(double %a, double %b, double %c) {
  %r = call double @llvm.fma.f64(double %a, double %b, double %c)
  ret double %r
}

define i64 @op_lround(double %a) {
  %r = call i64 @llvm.lround.i64.f64(double %a)
  ret i64 %r
}

define i64 @op_llround(double %a) {
  %r = call i64 @llvm.llround.i64.f64(double %a)
  ret i64 %r
}

define i64 @op_lrint(double %a) {
  %r = call i64 @llvm.lrint.i64.f64(double %a)
  ret i64 %r
}

define i64 @op_llrint(double %a) {
  %r = call i64 @llvm.llrint.i64.f64(double %a)
  ret i64 %r
}

define double @op_powi(double %a, i32 %n) {
  %r = call double @llvm.powi.f64.i32(double %a, i32 %n)
  ret double %r
}

define double @op_ldexp(double %a, i32 %n) {
  %r = call double @llvm.ldexp.f64.i32(double %a, i32 %n)
  ret double %r
}

define { double, i32 } @op_frexp(double %a) {
  %r = call { double, i32 } @llvm.frexp.f64.i32(double %a)
  ret { double, i32 } %r
}

declare double @llvm.sin.f64(double)
declare double @llvm.cos.f64(double)
declare double @llvm.tan.f64(double)
declare double @llvm.asin.f64(double)
declare double @llvm.acos.f64(double)
declare double @llvm.atan.f64(double)
declare double @llvm.sinh.f64(double)
declare double @llvm.cosh.f64(double)
declare double @llvm.tanh.f64(double)
declare double @llvm.exp.f64(double)
declare double @llvm.exp2.f64(double)
declare double @llvm.exp10.f64(double)
declare double @llvm.log.f64(double)
declare double @llvm.log2.f64(double)
declare double @llvm.log10.f64(double)
declare double @llvm.sqrt.f64(double)
declare double @llvm.floor.f64(double)
declare double @llvm.ceil.f64(double)
declare double @llvm.trunc.f64(double)
declare double @llvm.round.f64(double)
declare double @llvm.roundeven.f64(double)
declare double @llvm.nearbyint.f64(double)
declare double @llvm.rint.f64(double)
declare double @llvm.pow.f64(double, double)
declare double @llvm.atan2.f64(double, double)
declare double @llvm.minnum.f64(double, double)
declare double @llvm.maxnum.f64(double, double)
declare double @llvm.minimum.f64(double, double)
declare double @llvm.maximum.f64(double, double)
declare double @llvm.minimumnum.f64(double, double)
declare double @llvm.maximumnum.f64(double, double)
declare double @llvm.fma.f64(double, double, double)
declare i64 @llvm.lround.i64.f64(double)
declare i64 @llvm.llround.i64.f64(double)
declare i64 @llvm.lrint.i64.f64(double)
declare i64 @llvm.llrint.i64.f64(double)
declare double @llvm.powi.f64.i32(double, i32)
declare double @llvm.ldexp.f64.i32(double, i32)
declare { double, i32 } @llvm.frexp.f64.i32(double)
