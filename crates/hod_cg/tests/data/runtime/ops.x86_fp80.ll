; WP-117 (spec/codegen.md 7.3): which runtime routine llc calls for each
; operation, by type; crates/hod_cg/tests/runtime_names.rs compares every
; function with hod_cg::sym's tables under llc 19 and 22. Generated.

define x86_fp80 @op_sin(x86_fp80 %a) {
  %r = call x86_fp80 @llvm.sin.f80(x86_fp80 %a)
  ret x86_fp80 %r
}

define x86_fp80 @op_cos(x86_fp80 %a) {
  %r = call x86_fp80 @llvm.cos.f80(x86_fp80 %a)
  ret x86_fp80 %r
}

define x86_fp80 @op_tan(x86_fp80 %a) {
  %r = call x86_fp80 @llvm.tan.f80(x86_fp80 %a)
  ret x86_fp80 %r
}

define x86_fp80 @op_asin(x86_fp80 %a) {
  %r = call x86_fp80 @llvm.asin.f80(x86_fp80 %a)
  ret x86_fp80 %r
}

define x86_fp80 @op_acos(x86_fp80 %a) {
  %r = call x86_fp80 @llvm.acos.f80(x86_fp80 %a)
  ret x86_fp80 %r
}

define x86_fp80 @op_atan(x86_fp80 %a) {
  %r = call x86_fp80 @llvm.atan.f80(x86_fp80 %a)
  ret x86_fp80 %r
}

define x86_fp80 @op_sinh(x86_fp80 %a) {
  %r = call x86_fp80 @llvm.sinh.f80(x86_fp80 %a)
  ret x86_fp80 %r
}

define x86_fp80 @op_cosh(x86_fp80 %a) {
  %r = call x86_fp80 @llvm.cosh.f80(x86_fp80 %a)
  ret x86_fp80 %r
}

define x86_fp80 @op_tanh(x86_fp80 %a) {
  %r = call x86_fp80 @llvm.tanh.f80(x86_fp80 %a)
  ret x86_fp80 %r
}

define x86_fp80 @op_exp(x86_fp80 %a) {
  %r = call x86_fp80 @llvm.exp.f80(x86_fp80 %a)
  ret x86_fp80 %r
}

define x86_fp80 @op_exp2(x86_fp80 %a) {
  %r = call x86_fp80 @llvm.exp2.f80(x86_fp80 %a)
  ret x86_fp80 %r
}

define x86_fp80 @op_exp10(x86_fp80 %a) {
  %r = call x86_fp80 @llvm.exp10.f80(x86_fp80 %a)
  ret x86_fp80 %r
}

define x86_fp80 @op_log(x86_fp80 %a) {
  %r = call x86_fp80 @llvm.log.f80(x86_fp80 %a)
  ret x86_fp80 %r
}

define x86_fp80 @op_log2(x86_fp80 %a) {
  %r = call x86_fp80 @llvm.log2.f80(x86_fp80 %a)
  ret x86_fp80 %r
}

define x86_fp80 @op_log10(x86_fp80 %a) {
  %r = call x86_fp80 @llvm.log10.f80(x86_fp80 %a)
  ret x86_fp80 %r
}

define x86_fp80 @op_sqrt(x86_fp80 %a) {
  %r = call x86_fp80 @llvm.sqrt.f80(x86_fp80 %a)
  ret x86_fp80 %r
}

define x86_fp80 @op_floor(x86_fp80 %a) {
  %r = call x86_fp80 @llvm.floor.f80(x86_fp80 %a)
  ret x86_fp80 %r
}

define x86_fp80 @op_ceil(x86_fp80 %a) {
  %r = call x86_fp80 @llvm.ceil.f80(x86_fp80 %a)
  ret x86_fp80 %r
}

define x86_fp80 @op_trunc(x86_fp80 %a) {
  %r = call x86_fp80 @llvm.trunc.f80(x86_fp80 %a)
  ret x86_fp80 %r
}

define x86_fp80 @op_round(x86_fp80 %a) {
  %r = call x86_fp80 @llvm.round.f80(x86_fp80 %a)
  ret x86_fp80 %r
}

define x86_fp80 @op_roundeven(x86_fp80 %a) {
  %r = call x86_fp80 @llvm.roundeven.f80(x86_fp80 %a)
  ret x86_fp80 %r
}

define x86_fp80 @op_nearbyint(x86_fp80 %a) {
  %r = call x86_fp80 @llvm.nearbyint.f80(x86_fp80 %a)
  ret x86_fp80 %r
}

define x86_fp80 @op_rint(x86_fp80 %a) {
  %r = call x86_fp80 @llvm.rint.f80(x86_fp80 %a)
  ret x86_fp80 %r
}

define x86_fp80 @op_pow(x86_fp80 %a, x86_fp80 %b) {
  %r = call x86_fp80 @llvm.pow.f80(x86_fp80 %a, x86_fp80 %b)
  ret x86_fp80 %r
}

define x86_fp80 @op_atan2(x86_fp80 %a, x86_fp80 %b) {
  %r = call x86_fp80 @llvm.atan2.f80(x86_fp80 %a, x86_fp80 %b)
  ret x86_fp80 %r
}

define x86_fp80 @op_minnum(x86_fp80 %a, x86_fp80 %b) {
  %r = call x86_fp80 @llvm.minnum.f80(x86_fp80 %a, x86_fp80 %b)
  ret x86_fp80 %r
}

define x86_fp80 @op_maxnum(x86_fp80 %a, x86_fp80 %b) {
  %r = call x86_fp80 @llvm.maxnum.f80(x86_fp80 %a, x86_fp80 %b)
  ret x86_fp80 %r
}

define x86_fp80 @op_add(x86_fp80 %a, x86_fp80 %b) {
  %r = fadd x86_fp80 %a, %b
  ret x86_fp80 %r
}

define x86_fp80 @op_sub(x86_fp80 %a, x86_fp80 %b) {
  %r = fsub x86_fp80 %a, %b
  ret x86_fp80 %r
}

define x86_fp80 @op_mul(x86_fp80 %a, x86_fp80 %b) {
  %r = fmul x86_fp80 %a, %b
  ret x86_fp80 %r
}

define x86_fp80 @op_div(x86_fp80 %a, x86_fp80 %b) {
  %r = fdiv x86_fp80 %a, %b
  ret x86_fp80 %r
}

define x86_fp80 @op_rem(x86_fp80 %a, x86_fp80 %b) {
  %r = frem x86_fp80 %a, %b
  ret x86_fp80 %r
}

define x86_fp80 @op_fma(x86_fp80 %a, x86_fp80 %b, x86_fp80 %c) {
  %r = call x86_fp80 @llvm.fma.f80(x86_fp80 %a, x86_fp80 %b, x86_fp80 %c)
  ret x86_fp80 %r
}

define i64 @op_lround(x86_fp80 %a) {
  %r = call i64 @llvm.lround.i64.f80(x86_fp80 %a)
  ret i64 %r
}

define i64 @op_llround(x86_fp80 %a) {
  %r = call i64 @llvm.llround.i64.f80(x86_fp80 %a)
  ret i64 %r
}

define i64 @op_lrint(x86_fp80 %a) {
  %r = call i64 @llvm.lrint.i64.f80(x86_fp80 %a)
  ret i64 %r
}

define i64 @op_llrint(x86_fp80 %a) {
  %r = call i64 @llvm.llrint.i64.f80(x86_fp80 %a)
  ret i64 %r
}

define x86_fp80 @op_powi(x86_fp80 %a, i32 %n) {
  %r = call x86_fp80 @llvm.powi.f80.i32(x86_fp80 %a, i32 %n)
  ret x86_fp80 %r
}

define x86_fp80 @op_ldexp(x86_fp80 %a, i32 %n) {
  %r = call x86_fp80 @llvm.ldexp.f80.i32(x86_fp80 %a, i32 %n)
  ret x86_fp80 %r
}

define { x86_fp80, i32 } @op_frexp(x86_fp80 %a) {
  %r = call { x86_fp80, i32 } @llvm.frexp.f80.i32(x86_fp80 %a)
  ret { x86_fp80, i32 } %r
}

declare x86_fp80 @llvm.sin.f80(x86_fp80)
declare x86_fp80 @llvm.cos.f80(x86_fp80)
declare x86_fp80 @llvm.tan.f80(x86_fp80)
declare x86_fp80 @llvm.asin.f80(x86_fp80)
declare x86_fp80 @llvm.acos.f80(x86_fp80)
declare x86_fp80 @llvm.atan.f80(x86_fp80)
declare x86_fp80 @llvm.sinh.f80(x86_fp80)
declare x86_fp80 @llvm.cosh.f80(x86_fp80)
declare x86_fp80 @llvm.tanh.f80(x86_fp80)
declare x86_fp80 @llvm.exp.f80(x86_fp80)
declare x86_fp80 @llvm.exp2.f80(x86_fp80)
declare x86_fp80 @llvm.exp10.f80(x86_fp80)
declare x86_fp80 @llvm.log.f80(x86_fp80)
declare x86_fp80 @llvm.log2.f80(x86_fp80)
declare x86_fp80 @llvm.log10.f80(x86_fp80)
declare x86_fp80 @llvm.sqrt.f80(x86_fp80)
declare x86_fp80 @llvm.floor.f80(x86_fp80)
declare x86_fp80 @llvm.ceil.f80(x86_fp80)
declare x86_fp80 @llvm.trunc.f80(x86_fp80)
declare x86_fp80 @llvm.round.f80(x86_fp80)
declare x86_fp80 @llvm.roundeven.f80(x86_fp80)
declare x86_fp80 @llvm.nearbyint.f80(x86_fp80)
declare x86_fp80 @llvm.rint.f80(x86_fp80)
declare x86_fp80 @llvm.pow.f80(x86_fp80, x86_fp80)
declare x86_fp80 @llvm.atan2.f80(x86_fp80, x86_fp80)
declare x86_fp80 @llvm.minnum.f80(x86_fp80, x86_fp80)
declare x86_fp80 @llvm.maxnum.f80(x86_fp80, x86_fp80)
declare x86_fp80 @llvm.fma.f80(x86_fp80, x86_fp80, x86_fp80)
declare i64 @llvm.lround.i64.f80(x86_fp80)
declare i64 @llvm.llround.i64.f80(x86_fp80)
declare i64 @llvm.lrint.i64.f80(x86_fp80)
declare i64 @llvm.llrint.i64.f80(x86_fp80)
declare x86_fp80 @llvm.powi.f80.i32(x86_fp80, i32)
declare x86_fp80 @llvm.ldexp.f80.i32(x86_fp80, i32)
declare { x86_fp80, i32 } @llvm.frexp.f80.i32(x86_fp80)
