; WP-117 (spec/codegen.md 7.3): which runtime routine llc calls for each
; operation, by type; crates/hod_cg/tests/runtime_names.rs compares every
; function with hod_cg::sym's tables under llc 19 and 22. Generated.

define float @op_sin(float %a) {
  %r = call float @llvm.sin.f32(float %a)
  ret float %r
}

define float @op_cos(float %a) {
  %r = call float @llvm.cos.f32(float %a)
  ret float %r
}

define float @op_tan(float %a) {
  %r = call float @llvm.tan.f32(float %a)
  ret float %r
}

define float @op_asin(float %a) {
  %r = call float @llvm.asin.f32(float %a)
  ret float %r
}

define float @op_acos(float %a) {
  %r = call float @llvm.acos.f32(float %a)
  ret float %r
}

define float @op_atan(float %a) {
  %r = call float @llvm.atan.f32(float %a)
  ret float %r
}

define float @op_sinh(float %a) {
  %r = call float @llvm.sinh.f32(float %a)
  ret float %r
}

define float @op_cosh(float %a) {
  %r = call float @llvm.cosh.f32(float %a)
  ret float %r
}

define float @op_tanh(float %a) {
  %r = call float @llvm.tanh.f32(float %a)
  ret float %r
}

define float @op_exp(float %a) {
  %r = call float @llvm.exp.f32(float %a)
  ret float %r
}

define float @op_exp2(float %a) {
  %r = call float @llvm.exp2.f32(float %a)
  ret float %r
}

define float @op_exp10(float %a) {
  %r = call float @llvm.exp10.f32(float %a)
  ret float %r
}

define float @op_log(float %a) {
  %r = call float @llvm.log.f32(float %a)
  ret float %r
}

define float @op_log2(float %a) {
  %r = call float @llvm.log2.f32(float %a)
  ret float %r
}

define float @op_log10(float %a) {
  %r = call float @llvm.log10.f32(float %a)
  ret float %r
}

define float @op_sqrt(float %a) {
  %r = call float @llvm.sqrt.f32(float %a)
  ret float %r
}

define float @op_floor(float %a) {
  %r = call float @llvm.floor.f32(float %a)
  ret float %r
}

define float @op_ceil(float %a) {
  %r = call float @llvm.ceil.f32(float %a)
  ret float %r
}

define float @op_trunc(float %a) {
  %r = call float @llvm.trunc.f32(float %a)
  ret float %r
}

define float @op_round(float %a) {
  %r = call float @llvm.round.f32(float %a)
  ret float %r
}

define float @op_roundeven(float %a) {
  %r = call float @llvm.roundeven.f32(float %a)
  ret float %r
}

define float @op_nearbyint(float %a) {
  %r = call float @llvm.nearbyint.f32(float %a)
  ret float %r
}

define float @op_rint(float %a) {
  %r = call float @llvm.rint.f32(float %a)
  ret float %r
}

define float @op_pow(float %a, float %b) {
  %r = call float @llvm.pow.f32(float %a, float %b)
  ret float %r
}

define float @op_atan2(float %a, float %b) {
  %r = call float @llvm.atan2.f32(float %a, float %b)
  ret float %r
}

define float @op_minnum(float %a, float %b) {
  %r = call float @llvm.minnum.f32(float %a, float %b)
  ret float %r
}

define float @op_maxnum(float %a, float %b) {
  %r = call float @llvm.maxnum.f32(float %a, float %b)
  ret float %r
}

define float @op_minimum(float %a, float %b) {
  %r = call float @llvm.minimum.f32(float %a, float %b)
  ret float %r
}

define float @op_maximum(float %a, float %b) {
  %r = call float @llvm.maximum.f32(float %a, float %b)
  ret float %r
}

define float @op_minimumnum(float %a, float %b) {
  %r = call float @llvm.minimumnum.f32(float %a, float %b)
  ret float %r
}

define float @op_maximumnum(float %a, float %b) {
  %r = call float @llvm.maximumnum.f32(float %a, float %b)
  ret float %r
}

define float @op_add(float %a, float %b) {
  %r = fadd float %a, %b
  ret float %r
}

define float @op_sub(float %a, float %b) {
  %r = fsub float %a, %b
  ret float %r
}

define float @op_mul(float %a, float %b) {
  %r = fmul float %a, %b
  ret float %r
}

define float @op_div(float %a, float %b) {
  %r = fdiv float %a, %b
  ret float %r
}

define float @op_rem(float %a, float %b) {
  %r = frem float %a, %b
  ret float %r
}

define float @op_fma(float %a, float %b, float %c) {
  %r = call float @llvm.fma.f32(float %a, float %b, float %c)
  ret float %r
}

define i64 @op_lround(float %a) {
  %r = call i64 @llvm.lround.i64.f32(float %a)
  ret i64 %r
}

define i64 @op_llround(float %a) {
  %r = call i64 @llvm.llround.i64.f32(float %a)
  ret i64 %r
}

define i64 @op_lrint(float %a) {
  %r = call i64 @llvm.lrint.i64.f32(float %a)
  ret i64 %r
}

define i64 @op_llrint(float %a) {
  %r = call i64 @llvm.llrint.i64.f32(float %a)
  ret i64 %r
}

define float @op_powi(float %a, i32 %n) {
  %r = call float @llvm.powi.f32.i32(float %a, i32 %n)
  ret float %r
}

define float @op_ldexp(float %a, i32 %n) {
  %r = call float @llvm.ldexp.f32.i32(float %a, i32 %n)
  ret float %r
}

define { float, i32 } @op_frexp(float %a) {
  %r = call { float, i32 } @llvm.frexp.f32.i32(float %a)
  ret { float, i32 } %r
}

declare float @llvm.sin.f32(float)
declare float @llvm.cos.f32(float)
declare float @llvm.tan.f32(float)
declare float @llvm.asin.f32(float)
declare float @llvm.acos.f32(float)
declare float @llvm.atan.f32(float)
declare float @llvm.sinh.f32(float)
declare float @llvm.cosh.f32(float)
declare float @llvm.tanh.f32(float)
declare float @llvm.exp.f32(float)
declare float @llvm.exp2.f32(float)
declare float @llvm.exp10.f32(float)
declare float @llvm.log.f32(float)
declare float @llvm.log2.f32(float)
declare float @llvm.log10.f32(float)
declare float @llvm.sqrt.f32(float)
declare float @llvm.floor.f32(float)
declare float @llvm.ceil.f32(float)
declare float @llvm.trunc.f32(float)
declare float @llvm.round.f32(float)
declare float @llvm.roundeven.f32(float)
declare float @llvm.nearbyint.f32(float)
declare float @llvm.rint.f32(float)
declare float @llvm.pow.f32(float, float)
declare float @llvm.atan2.f32(float, float)
declare float @llvm.minnum.f32(float, float)
declare float @llvm.maxnum.f32(float, float)
declare float @llvm.minimum.f32(float, float)
declare float @llvm.maximum.f32(float, float)
declare float @llvm.minimumnum.f32(float, float)
declare float @llvm.maximumnum.f32(float, float)
declare float @llvm.fma.f32(float, float, float)
declare i64 @llvm.lround.i64.f32(float)
declare i64 @llvm.llround.i64.f32(float)
declare i64 @llvm.lrint.i64.f32(float)
declare i64 @llvm.llrint.i64.f32(float)
declare float @llvm.powi.f32.i32(float, i32)
declare float @llvm.ldexp.f32.i32(float, i32)
declare { float, i32 } @llvm.frexp.f32.i32(float)
