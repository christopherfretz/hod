; WP-117 (spec/codegen.md 7.3): which runtime routine llc calls for each
; operation, by type; crates/hod_cg/tests/runtime_names.rs compares every
; function with hod_cg::sym's tables under llc 19 and 22. Generated.

define half @trunc_half(double %a) {
  %r = fptrunc double %a to half
  ret half %r
}

define float @trunc_float(double %a) {
  %r = fptrunc double %a to float
  ret float %r
}

define fp128 @ext_fp128(double %a) {
  %r = fpext double %a to fp128
  ret fp128 %r
}

define i32 @fptosi_i32(double %a) {
  %r = fptosi double %a to i32
  ret i32 %r
}

define i32 @fptoui_i32(double %a) {
  %r = fptoui double %a to i32
  ret i32 %r
}

define double @sitofp_i32(i32 %a) {
  %r = sitofp i32 %a to double
  ret double %r
}

define double @uitofp_i32(i32 %a) {
  %r = uitofp i32 %a to double
  ret double %r
}

define i64 @fptosi_i64(double %a) {
  %r = fptosi double %a to i64
  ret i64 %r
}

define i64 @fptoui_i64(double %a) {
  %r = fptoui double %a to i64
  ret i64 %r
}

define double @sitofp_i64(i64 %a) {
  %r = sitofp i64 %a to double
  ret double %r
}

define double @uitofp_i64(i64 %a) {
  %r = uitofp i64 %a to double
  ret double %r
}

define i128 @fptosi_i128(double %a) {
  %r = fptosi double %a to i128
  ret i128 %r
}

define i128 @fptoui_i128(double %a) {
  %r = fptoui double %a to i128
  ret i128 %r
}

define double @sitofp_i128(i128 %a) {
  %r = sitofp i128 %a to double
  ret double %r
}

define double @uitofp_i128(i128 %a) {
  %r = uitofp i128 %a to double
  ret double %r
}
