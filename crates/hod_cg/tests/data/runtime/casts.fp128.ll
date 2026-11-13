; WP-117 (spec/codegen.md 7.3): which runtime routine llc calls for each
; operation, by type; crates/hod_cg/tests/runtime_names.rs compares every
; function with hod_cg::sym's tables under llc 19 and 22. Generated.

define half @trunc_half(fp128 %a) {
  %r = fptrunc fp128 %a to half
  ret half %r
}

define float @trunc_float(fp128 %a) {
  %r = fptrunc fp128 %a to float
  ret float %r
}

define double @trunc_double(fp128 %a) {
  %r = fptrunc fp128 %a to double
  ret double %r
}

define i32 @fptosi_i32(fp128 %a) {
  %r = fptosi fp128 %a to i32
  ret i32 %r
}

define i32 @fptoui_i32(fp128 %a) {
  %r = fptoui fp128 %a to i32
  ret i32 %r
}

define fp128 @sitofp_i32(i32 %a) {
  %r = sitofp i32 %a to fp128
  ret fp128 %r
}

define fp128 @uitofp_i32(i32 %a) {
  %r = uitofp i32 %a to fp128
  ret fp128 %r
}

define i64 @fptosi_i64(fp128 %a) {
  %r = fptosi fp128 %a to i64
  ret i64 %r
}

define i64 @fptoui_i64(fp128 %a) {
  %r = fptoui fp128 %a to i64
  ret i64 %r
}

define fp128 @sitofp_i64(i64 %a) {
  %r = sitofp i64 %a to fp128
  ret fp128 %r
}

define fp128 @uitofp_i64(i64 %a) {
  %r = uitofp i64 %a to fp128
  ret fp128 %r
}

define i128 @fptosi_i128(fp128 %a) {
  %r = fptosi fp128 %a to i128
  ret i128 %r
}

define i128 @fptoui_i128(fp128 %a) {
  %r = fptoui fp128 %a to i128
  ret i128 %r
}

define fp128 @sitofp_i128(i128 %a) {
  %r = sitofp i128 %a to fp128
  ret fp128 %r
}

define fp128 @uitofp_i128(i128 %a) {
  %r = uitofp i128 %a to fp128
  ret fp128 %r
}
