; WP-117 (spec/codegen.md 7.3): which runtime routine llc calls for each
; operation, by type; crates/hod_cg/tests/runtime_names.rs compares every
; function with hod_cg::sym's tables under llc 19 and 22. Generated.

define float @ext_float(half %a) {
  %r = fpext half %a to float
  ret float %r
}

define double @ext_double(half %a) {
  %r = fpext half %a to double
  ret double %r
}

define fp128 @ext_fp128(half %a) {
  %r = fpext half %a to fp128
  ret fp128 %r
}

define i32 @fptosi_i32(half %a) {
  %r = fptosi half %a to i32
  ret i32 %r
}

define i32 @fptoui_i32(half %a) {
  %r = fptoui half %a to i32
  ret i32 %r
}

define half @sitofp_i32(i32 %a) {
  %r = sitofp i32 %a to half
  ret half %r
}

define half @uitofp_i32(i32 %a) {
  %r = uitofp i32 %a to half
  ret half %r
}

define i64 @fptosi_i64(half %a) {
  %r = fptosi half %a to i64
  ret i64 %r
}

define i64 @fptoui_i64(half %a) {
  %r = fptoui half %a to i64
  ret i64 %r
}

define half @sitofp_i64(i64 %a) {
  %r = sitofp i64 %a to half
  ret half %r
}

define half @uitofp_i64(i64 %a) {
  %r = uitofp i64 %a to half
  ret half %r
}

define i128 @fptosi_i128(half %a) {
  %r = fptosi half %a to i128
  ret i128 %r
}

define i128 @fptoui_i128(half %a) {
  %r = fptoui half %a to i128
  ret i128 %r
}

define half @sitofp_i128(i128 %a) {
  %r = sitofp i128 %a to half
  ret half %r
}

define half @uitofp_i128(i128 %a) {
  %r = uitofp i128 %a to half
  ret half %r
}
