; WP-117 (spec/codegen.md 7.3): which runtime routine llc calls for each
; operation, by type; crates/hod_cg/tests/runtime_names.rs compares every
; function with hod_cg::sym's tables under llc 19 and 22. Generated.

define half @op_sin(half %a) {
  %r = call half @llvm.sin.f16(half %a)
  ret half %r
}

define half @op_cos(half %a) {
  %r = call half @llvm.cos.f16(half %a)
  ret half %r
}

define half @op_tan(half %a) {
  %r = call half @llvm.tan.f16(half %a)
  ret half %r
}

define half @op_asin(half %a) {
  %r = call half @llvm.asin.f16(half %a)
  ret half %r
}

define half @op_acos(half %a) {
  %r = call half @llvm.acos.f16(half %a)
  ret half %r
}

define half @op_atan(half %a) {
  %r = call half @llvm.atan.f16(half %a)
  ret half %r
}

define half @op_sinh(half %a) {
  %r = call half @llvm.sinh.f16(half %a)
  ret half %r
}

define half @op_cosh(half %a) {
  %r = call half @llvm.cosh.f16(half %a)
  ret half %r
}

define half @op_tanh(half %a) {
  %r = call half @llvm.tanh.f16(half %a)
  ret half %r
}

define half @op_exp(half %a) {
  %r = call half @llvm.exp.f16(half %a)
  ret half %r
}

define half @op_exp2(half %a) {
  %r = call half @llvm.exp2.f16(half %a)
  ret half %r
}

define half @op_exp10(half %a) {
  %r = call half @llvm.exp10.f16(half %a)
  ret half %r
}

define half @op_log(half %a) {
  %r = call half @llvm.log.f16(half %a)
  ret half %r
}

define half @op_log2(half %a) {
  %r = call half @llvm.log2.f16(half %a)
  ret half %r
}

define half @op_log10(half %a) {
  %r = call half @llvm.log10.f16(half %a)
  ret half %r
}

define half @op_sqrt(half %a) {
  %r = call half @llvm.sqrt.f16(half %a)
  ret half %r
}

define half @op_floor(half %a) {
  %r = call half @llvm.floor.f16(half %a)
  ret half %r
}

define half @op_ceil(half %a) {
  %r = call half @llvm.ceil.f16(half %a)
  ret half %r
}

define half @op_trunc(half %a) {
  %r = call half @llvm.trunc.f16(half %a)
  ret half %r
}

define half @op_round(half %a) {
  %r = call half @llvm.round.f16(half %a)
  ret half %r
}

define half @op_roundeven(half %a) {
  %r = call half @llvm.roundeven.f16(half %a)
  ret half %r
}

define half @op_nearbyint(half %a) {
  %r = call half @llvm.nearbyint.f16(half %a)
  ret half %r
}

define half @op_rint(half %a) {
  %r = call half @llvm.rint.f16(half %a)
  ret half %r
}

define half @op_pow(half %a, half %b) {
  %r = call half @llvm.pow.f16(half %a, half %b)
  ret half %r
}

define half @op_atan2(half %a, half %b) {
  %r = call half @llvm.atan2.f16(half %a, half %b)
  ret half %r
}

define half @op_minnum(half %a, half %b) {
  %r = call half @llvm.minnum.f16(half %a, half %b)
  ret half %r
}

define half @op_maxnum(half %a, half %b) {
  %r = call half @llvm.maxnum.f16(half %a, half %b)
  ret half %r
}

define half @op_minimum(half %a, half %b) {
  %r = call half @llvm.minimum.f16(half %a, half %b)
  ret half %r
}

define half @op_maximum(half %a, half %b) {
  %r = call half @llvm.maximum.f16(half %a, half %b)
  ret half %r
}

define half @op_minimumnum(half %a, half %b) {
  %r = call half @llvm.minimumnum.f16(half %a, half %b)
  ret half %r
}

define half @op_maximumnum(half %a, half %b) {
  %r = call half @llvm.maximumnum.f16(half %a, half %b)
  ret half %r
}

define half @op_add(half %a, half %b) {
  %r = fadd half %a, %b
  ret half %r
}

define half @op_sub(half %a, half %b) {
  %r = fsub half %a, %b
  ret half %r
}

define half @op_mul(half %a, half %b) {
  %r = fmul half %a, %b
  ret half %r
}

define half @op_div(half %a, half %b) {
  %r = fdiv half %a, %b
  ret half %r
}

define half @op_rem(half %a, half %b) {
  %r = frem half %a, %b
  ret half %r
}

define half @op_fma(half %a, half %b, half %c) {
  %r = call half @llvm.fma.f16(half %a, half %b, half %c)
  ret half %r
}

define half @op_powi(half %a, i32 %n) {
  %r = call half @llvm.powi.f16.i32(half %a, i32 %n)
  ret half %r
}

define half @op_ldexp(half %a, i32 %n) {
  %r = call half @llvm.ldexp.f16.i32(half %a, i32 %n)
  ret half %r
}

define { half, i32 } @op_frexp(half %a) {
  %r = call { half, i32 } @llvm.frexp.f16.i32(half %a)
  ret { half, i32 } %r
}

declare half @llvm.sin.f16(half)
declare half @llvm.cos.f16(half)
declare half @llvm.tan.f16(half)
declare half @llvm.asin.f16(half)
declare half @llvm.acos.f16(half)
declare half @llvm.atan.f16(half)
declare half @llvm.sinh.f16(half)
declare half @llvm.cosh.f16(half)
declare half @llvm.tanh.f16(half)
declare half @llvm.exp.f16(half)
declare half @llvm.exp2.f16(half)
declare half @llvm.exp10.f16(half)
declare half @llvm.log.f16(half)
declare half @llvm.log2.f16(half)
declare half @llvm.log10.f16(half)
declare half @llvm.sqrt.f16(half)
declare half @llvm.floor.f16(half)
declare half @llvm.ceil.f16(half)
declare half @llvm.trunc.f16(half)
declare half @llvm.round.f16(half)
declare half @llvm.roundeven.f16(half)
declare half @llvm.nearbyint.f16(half)
declare half @llvm.rint.f16(half)
declare half @llvm.pow.f16(half, half)
declare half @llvm.atan2.f16(half, half)
declare half @llvm.minnum.f16(half, half)
declare half @llvm.maxnum.f16(half, half)
declare half @llvm.minimum.f16(half, half)
declare half @llvm.maximum.f16(half, half)
declare half @llvm.minimumnum.f16(half, half)
declare half @llvm.maximumnum.f16(half, half)
declare half @llvm.fma.f16(half, half, half)
declare half @llvm.powi.f16.i32(half, i32)
declare half @llvm.ldexp.f16.i32(half, i32)
declare { half, i32 } @llvm.frexp.f16.i32(half)
