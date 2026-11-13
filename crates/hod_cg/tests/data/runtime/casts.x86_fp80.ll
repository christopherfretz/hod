; WP-117 (spec/codegen.md 7.3): which runtime routine llc calls for each
; operation, by type; crates/hod_cg/tests/runtime_names.rs compares every
; function with hod_cg::sym's tables under llc 19 and 22. Generated.
; x86-64 only.

define half @trunc_half(x86_fp80 %a) {
  %r = fptrunc x86_fp80 %a to half
  ret half %r
}

define float @trunc_float(x86_fp80 %a) {
  %r = fptrunc x86_fp80 %a to float
  ret float %r
}

define double @trunc_double(x86_fp80 %a) {
  %r = fptrunc x86_fp80 %a to double
  ret double %r
}

define fp128 @ext_fp128(x86_fp80 %a) {
  %r = fpext x86_fp80 %a to fp128
  ret fp128 %r
}

define i32 @fptosi_i32(x86_fp80 %a) {
  %r = fptosi x86_fp80 %a to i32
  ret i32 %r
}

define i32 @fptoui_i32(x86_fp80 %a) {
  %r = fptoui x86_fp80 %a to i32
  ret i32 %r
}

define x86_fp80 @sitofp_i32(i32 %a) {
  %r = sitofp i32 %a to x86_fp80
  ret x86_fp80 %r
}

define x86_fp80 @uitofp_i32(i32 %a) {
  %r = uitofp i32 %a to x86_fp80
  ret x86_fp80 %r
}

define i64 @fptosi_i64(x86_fp80 %a) {
  %r = fptosi x86_fp80 %a to i64
  ret i64 %r
}

define i64 @fptoui_i64(x86_fp80 %a) {
  %r = fptoui x86_fp80 %a to i64
  ret i64 %r
}

define x86_fp80 @sitofp_i64(i64 %a) {
  %r = sitofp i64 %a to x86_fp80
  ret x86_fp80 %r
}

define x86_fp80 @uitofp_i64(i64 %a) {
  %r = uitofp i64 %a to x86_fp80
  ret x86_fp80 %r
}

define i128 @fptosi_i128(x86_fp80 %a) {
  %r = fptosi x86_fp80 %a to i128
  ret i128 %r
}

define i128 @fptoui_i128(x86_fp80 %a) {
  %r = fptoui x86_fp80 %a to i128
  ret i128 %r
}

define x86_fp80 @sitofp_i128(i128 %a) {
  %r = sitofp i128 %a to x86_fp80
  ret x86_fp80 %r
}

define x86_fp80 @uitofp_i128(i128 %a) {
  %r = uitofp i128 %a to x86_fp80
  ret x86_fp80 %r
}

define x86_fp80 @from_half(half %a) {
  %r = fpext half %a to x86_fp80
  ret x86_fp80 %r
}

define x86_fp80 @from_float(float %a) {
  %r = fpext float %a to x86_fp80
  ret x86_fp80 %r
}

define x86_fp80 @from_double(double %a) {
  %r = fpext double %a to x86_fp80
  ret x86_fp80 %r
}

define x86_fp80 @from_fp128(fp128 %a) {
  %r = fptrunc fp128 %a to x86_fp80
  ret x86_fp80 %r
}
