; WP-069: `alignstack` on a block argument that goes to the stack
; (abi-a64.md §3.3, §6.4), and on a function (no code: AArch64 keeps `sp`
; 16-aligned, and an over-aligned slot is aligned by itself, codegen.md
; §5.1). Driven by alignstack.c (tests/call.rs). The l_* functions pass their
; arguments on to alignstack_callee.ll's h_*.
declare void @h_a(double, double, double, double, double, double, double, double, float, [2 x double] alignstack(16))
define void @l_a(double %d0, double %d1, double %d2, double %d3, double %d4, double %d5, double %d6, double %d7, float %f, [2 x double] alignstack(16) %x) {
  call void @h_a(double %d0, double %d1, double %d2, double %d3, double %d4, double %d5, double %d6, double %d7, float %f, [2 x double] alignstack(16) %x)
  ret void
}
declare void @h_b(fp128, fp128, fp128, fp128, fp128, fp128, fp128, fp128, i8, [2 x fp128] alignstack(16))
define void @l_b(fp128 %q0, fp128 %q1, fp128 %q2, fp128 %q3, fp128 %q4, fp128 %q5, fp128 %q6, fp128 %q7, i8 %c, [2 x fp128] alignstack(16) %x) {
  call void @h_b(fp128 %q0, fp128 %q1, fp128 %q2, fp128 %q3, fp128 %q4, fp128 %q5, fp128 %q6, fp128 %q7, i8 %c, [2 x fp128] alignstack(16) %x)
  ret void
}
declare void @h_c(double, double, double, double, double, double, double, double, i8, [2 x float] alignstack(8))
define void @l_c(double %d0, double %d1, double %d2, double %d3, double %d4, double %d5, double %d6, double %d7, i8 %c, [2 x float] alignstack(8) %x) {
  call void @h_c(double %d0, double %d1, double %d2, double %d3, double %d4, double %d5, double %d6, double %d7, i8 %c, [2 x float] alignstack(8) %x)
  ret void
}
declare void @h_d(double, double, double, double, double, double, double, double, i64, i64, i64, i64, i64, i64, i64, i64, i32, [3 x float] alignstack(16))
define void @l_d(double %d0, double %d1, double %d2, double %d3, double %d4, double %d5, double %d6, double %d7, i64 %i0, i64 %i1, i64 %i2, i64 %i3, i64 %i4, i64 %i5, i64 %i6, i64 %i7, i32 %w, [3 x float] alignstack(16) %x) {
  call void @h_d(double %d0, double %d1, double %d2, double %d3, double %d4, double %d5, double %d6, double %d7, i64 %i0, i64 %i1, i64 %i2, i64 %i3, i64 %i4, i64 %i5, i64 %i6, i64 %i7, i32 %w, [3 x float] alignstack(16) %x)
  ret void
}
declare void @c_slot32(ptr)
declare void @c_slot16(ptr)
define void @l_fn() alignstack(32) {
  %s = alloca i64, align 32
  call void @c_slot32(ptr %s)
  ret void
}
define void @l_fn16(i64 %x) alignstack(16) {
  %s = alloca i64, align 16
  call void @c_slot16(ptr %s)
  ret void
}
