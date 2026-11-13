; WP-117 (spec/codegen.md 7.3): which runtime routine llc calls for each
; operation, by type; crates/hod_cg/tests/runtime_names.rs compares every
; function with hod_cg::sym's tables under llc 19 and 22. Generated.

define fp128 @op_sin(fp128 %a) {
  %r = call fp128 @llvm.sin.f128(fp128 %a)
  ret fp128 %r
}

define fp128 @op_cos(fp128 %a) {
  %r = call fp128 @llvm.cos.f128(fp128 %a)
  ret fp128 %r
}

define fp128 @op_tan(fp128 %a) {
  %r = call fp128 @llvm.tan.f128(fp128 %a)
  ret fp128 %r
}

define fp128 @op_asin(fp128 %a) {
  %r = call fp128 @llvm.asin.f128(fp128 %a)
  ret fp128 %r
}

define fp128 @op_acos(fp128 %a) {
  %r = call fp128 @llvm.acos.f128(fp128 %a)
  ret fp128 %r
}

define fp128 @op_atan(fp128 %a) {
  %r = call fp128 @llvm.atan.f128(fp128 %a)
  ret fp128 %r
}

define fp128 @op_sinh(fp128 %a) {
  %r = call fp128 @llvm.sinh.f128(fp128 %a)
  ret fp128 %r
}

define fp128 @op_cosh(fp128 %a) {
  %r = call fp128 @llvm.cosh.f128(fp128 %a)
  ret fp128 %r
}

define fp128 @op_tanh(fp128 %a) {
  %r = call fp128 @llvm.tanh.f128(fp128 %a)
  ret fp128 %r
}

define fp128 @op_exp(fp128 %a) {
  %r = call fp128 @llvm.exp.f128(fp128 %a)
  ret fp128 %r
}

define fp128 @op_exp2(fp128 %a) {
  %r = call fp128 @llvm.exp2.f128(fp128 %a)
  ret fp128 %r
}

define fp128 @op_exp10(fp128 %a) {
  %r = call fp128 @llvm.exp10.f128(fp128 %a)
  ret fp128 %r
}

define fp128 @op_log(fp128 %a) {
  %r = call fp128 @llvm.log.f128(fp128 %a)
  ret fp128 %r
}

define fp128 @op_log2(fp128 %a) {
  %r = call fp128 @llvm.log2.f128(fp128 %a)
  ret fp128 %r
}

define fp128 @op_log10(fp128 %a) {
  %r = call fp128 @llvm.log10.f128(fp128 %a)
  ret fp128 %r
}

define fp128 @op_sqrt(fp128 %a) {
  %r = call fp128 @llvm.sqrt.f128(fp128 %a)
  ret fp128 %r
}

define fp128 @op_floor(fp128 %a) {
  %r = call fp128 @llvm.floor.f128(fp128 %a)
  ret fp128 %r
}

define fp128 @op_ceil(fp128 %a) {
  %r = call fp128 @llvm.ceil.f128(fp128 %a)
  ret fp128 %r
}

define fp128 @op_trunc(fp128 %a) {
  %r = call fp128 @llvm.trunc.f128(fp128 %a)
  ret fp128 %r
}

define fp128 @op_round(fp128 %a) {
  %r = call fp128 @llvm.round.f128(fp128 %a)
  ret fp128 %r
}

define fp128 @op_roundeven(fp128 %a) {
  %r = call fp128 @llvm.roundeven.f128(fp128 %a)
  ret fp128 %r
}

define fp128 @op_nearbyint(fp128 %a) {
  %r = call fp128 @llvm.nearbyint.f128(fp128 %a)
  ret fp128 %r
}

define fp128 @op_rint(fp128 %a) {
  %r = call fp128 @llvm.rint.f128(fp128 %a)
  ret fp128 %r
}

define fp128 @op_pow(fp128 %a, fp128 %b) {
  %r = call fp128 @llvm.pow.f128(fp128 %a, fp128 %b)
  ret fp128 %r
}

define fp128 @op_atan2(fp128 %a, fp128 %b) {
  %r = call fp128 @llvm.atan2.f128(fp128 %a, fp128 %b)
  ret fp128 %r
}

define fp128 @op_minnum(fp128 %a, fp128 %b) {
  %r = call fp128 @llvm.minnum.f128(fp128 %a, fp128 %b)
  ret fp128 %r
}

define fp128 @op_maxnum(fp128 %a, fp128 %b) {
  %r = call fp128 @llvm.maxnum.f128(fp128 %a, fp128 %b)
  ret fp128 %r
}

define fp128 @op_add(fp128 %a, fp128 %b) {
  %r = fadd fp128 %a, %b
  ret fp128 %r
}

define fp128 @op_sub(fp128 %a, fp128 %b) {
  %r = fsub fp128 %a, %b
  ret fp128 %r
}

define fp128 @op_mul(fp128 %a, fp128 %b) {
  %r = fmul fp128 %a, %b
  ret fp128 %r
}

define fp128 @op_div(fp128 %a, fp128 %b) {
  %r = fdiv fp128 %a, %b
  ret fp128 %r
}

define fp128 @op_rem(fp128 %a, fp128 %b) {
  %r = frem fp128 %a, %b
  ret fp128 %r
}

define fp128 @op_fma(fp128 %a, fp128 %b, fp128 %c) {
  %r = call fp128 @llvm.fma.f128(fp128 %a, fp128 %b, fp128 %c)
  ret fp128 %r
}

define i64 @op_lround(fp128 %a) {
  %r = call i64 @llvm.lround.i64.f128(fp128 %a)
  ret i64 %r
}

define i64 @op_llround(fp128 %a) {
  %r = call i64 @llvm.llround.i64.f128(fp128 %a)
  ret i64 %r
}

define i64 @op_lrint(fp128 %a) {
  %r = call i64 @llvm.lrint.i64.f128(fp128 %a)
  ret i64 %r
}

define i64 @op_llrint(fp128 %a) {
  %r = call i64 @llvm.llrint.i64.f128(fp128 %a)
  ret i64 %r
}

define fp128 @op_powi(fp128 %a, i32 %n) {
  %r = call fp128 @llvm.powi.f128.i32(fp128 %a, i32 %n)
  ret fp128 %r
}

define fp128 @op_ldexp(fp128 %a, i32 %n) {
  %r = call fp128 @llvm.ldexp.f128.i32(fp128 %a, i32 %n)
  ret fp128 %r
}

define { fp128, i32 } @op_frexp(fp128 %a) {
  %r = call { fp128, i32 } @llvm.frexp.f128.i32(fp128 %a)
  ret { fp128, i32 } %r
}

declare fp128 @llvm.sin.f128(fp128)
declare fp128 @llvm.cos.f128(fp128)
declare fp128 @llvm.tan.f128(fp128)
declare fp128 @llvm.asin.f128(fp128)
declare fp128 @llvm.acos.f128(fp128)
declare fp128 @llvm.atan.f128(fp128)
declare fp128 @llvm.sinh.f128(fp128)
declare fp128 @llvm.cosh.f128(fp128)
declare fp128 @llvm.tanh.f128(fp128)
declare fp128 @llvm.exp.f128(fp128)
declare fp128 @llvm.exp2.f128(fp128)
declare fp128 @llvm.exp10.f128(fp128)
declare fp128 @llvm.log.f128(fp128)
declare fp128 @llvm.log2.f128(fp128)
declare fp128 @llvm.log10.f128(fp128)
declare fp128 @llvm.sqrt.f128(fp128)
declare fp128 @llvm.floor.f128(fp128)
declare fp128 @llvm.ceil.f128(fp128)
declare fp128 @llvm.trunc.f128(fp128)
declare fp128 @llvm.round.f128(fp128)
declare fp128 @llvm.roundeven.f128(fp128)
declare fp128 @llvm.nearbyint.f128(fp128)
declare fp128 @llvm.rint.f128(fp128)
declare fp128 @llvm.pow.f128(fp128, fp128)
declare fp128 @llvm.atan2.f128(fp128, fp128)
declare fp128 @llvm.minnum.f128(fp128, fp128)
declare fp128 @llvm.maxnum.f128(fp128, fp128)
declare fp128 @llvm.fma.f128(fp128, fp128, fp128)
declare i64 @llvm.lround.i64.f128(fp128)
declare i64 @llvm.llround.i64.f128(fp128)
declare i64 @llvm.lrint.i64.f128(fp128)
declare i64 @llvm.llrint.i64.f128(fp128)
declare fp128 @llvm.powi.f128.i32(fp128, i32)
declare fp128 @llvm.ldexp.f128.i32(fp128, i32)
declare { fp128, i32 } @llvm.frexp.f128.i32(fp128)
