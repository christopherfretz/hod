; The x87 operands of spec/asm.md §14: `{st}` and `{st(1)}` as inputs,
; outputs and clobbers, with float, double and x86_fp80 values (WP-204).

define x86_fp80 @x87_sqrt(x86_fp80 %x) {
  %r = call x86_fp80 asm "fsqrt", "={st},0,~{dirflag},~{fpsr},~{flags}"(x86_fp80 %x)
  ret x86_fp80 %r
}

define double @x87_sqrt_double(double %x) {
  %r = call double asm "fsqrt", "={st},0,~{dirflag},~{fpsr},~{flags}"(double %x)
  ret double %r
}

; gcc-torture 990413-2's `__atan2l`: both inputs popped, one output.
define x86_fp80 @x87_atan2(x86_fp80 %y, x86_fp80 %x) {
  %r = call x86_fp80 asm "fpatan", "={st},0,{st(1)},~{st(1)},~{dirflag},~{fpsr},~{flags}"(x86_fp80 %x, x86_fp80 %y)
  ret x86_fp80 %r
}

; `st(1)` is an input the template leaves: popped after it. (C2, the
; partial remainder, tested without `sahf`, which the baseline CPU lacks.)
define x86_fp80 @x87_fmod(x86_fp80 %x, x86_fp80 %y) {
  %r = call x86_fp80 asm "1: fprem\0A\09fnstsw %ax\0A\09testw $$0x400, %ax\0A\09jne 1b", "={st},0,{st(1)},~{ax},~{cc},~{dirflag},~{fpsr},~{flags}"(x86_fp80 %x, x86_fp80 %y)
  ret x86_fp80 %r
}

; An input and no x87 output: popped after the template.
define i16 @x87_fxam(x86_fp80 %x) {
  %r = call i16 asm "fxam\0A\09fnstsw $0", "={ax},{st},~{dirflag},~{fpsr},~{flags}"(x86_fp80 %x)
  ret i16 %r
}

; Two outputs: `st(0)` popped first, then `st(1)`.
define void @x87_sincos(x86_fp80 %x, ptr %s, ptr %c) {
  %r = call { x86_fp80, x86_fp80 } asm "fsincos", "={st},={st(1)},0,~{dirflag},~{fpsr},~{flags}"(x86_fp80 %x)
  %cos = extractvalue { x86_fp80, x86_fp80 } %r, 0
  %sin = extractvalue { x86_fp80, x86_fp80 } %r, 1
  store x86_fp80 %cos, ptr %c
  store x86_fp80 %sin, ptr %s
  ret void
}

; An output and no input.
define float @x87_pi() {
  %r = call float asm "fldpi", "={st},~{dirflag},~{fpsr},~{flags}"()
  ret float %r
}

; `st(1)` read and left: popped after the template.
define x86_fp80 @x87_scale(x86_fp80 %x, x86_fp80 %k) {
  %r = call x86_fp80 asm "fscale", "={st},0,{st(1)},~{dirflag},~{fpsr},~{flags}"(x86_fp80 %x, x86_fp80 %k)
  ret x86_fp80 %r
}
