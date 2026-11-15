; WP-247 (`spec/abi-a64.md` §13.4, §13.5): what a `win64cc` and an
; `aarch64_vector_pcs` callee keep. `@w` and `@vp` call `clobber` (C,
; which writes every register a `ccc` callee may, `x18` and the upper
; halves of `v8`..`v15` included) twice around work of their own; `@cw`
; and `@cvp` call them as `ccc` functions do. The C side (`keep.c`)
; checks the registers around a `bl` to each.

declare void @clobber()

define win64cc i64 @w(i64 %a, double %b) {
  call void @clobber()
  %bi = fptosi double %b to i64
  %s = add i64 %a, %bi
  %m = mul i64 %s, 3
  call void @clobber()
  ret i64 %m
}

define aarch64_vector_pcs i64 @vp(i64 %a, <2 x double> %v) {
  call void @clobber()
  %w = fadd <2 x double> %v, %v
  %x = bitcast <2 x double> %w to <2 x i64>
  %e = extractelement <2 x i64> %x, i32 1
  %r = xor i64 %a, %e
  call void @clobber()
  ret i64 %r
}

define i64 @cw(i64 %a, i64 %b) {
  %d = sitofp i64 %b to double
  %r = call win64cc i64 @w(i64 %a, double %d)
  %q = add i64 %r, 1
  ret i64 %q
}

define i64 @cvp(i64 %a, i64 %b) {
  %f = sitofp i64 %b to double
  %v = insertelement <2 x double> <double 1.0, double 1.0>, double %f, i32 1
  %r = call aarch64_vector_pcs i64 @vp(i64 %a, <2 x double> %v)
  %q = add i64 %r, 1
  ret i64 %q
}
