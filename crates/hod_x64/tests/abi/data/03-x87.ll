; x86_fp80 (long double): passed in memory (16 bytes, align 16), returned in st(0).
declare x86_fp80 @ld(x86_fp80, i64, x86_fp80)
define x86_fp80 @call_ld(x86_fp80 %a) {
  %r = call x86_fp80 @ld(x86_fp80 %a, i64 1, x86_fp80 %a)
  %s = fadd x86_fp80 %r, %a
  ret x86_fp80 %s
}
define double @ld_to_d(x86_fp80 %a) { %d = fptrunc x86_fp80 %a to double
  ret double %d }
define i64 @ld_to_i(x86_fp80 %a) { %d = fptosi x86_fp80 %a to i64
  ret i64 %d }
define i64 @ld_to_u(x86_fp80 %a) { %d = fptoui x86_fp80 %a to i64
  ret i64 %d }
define i1 @ld_cmp(x86_fp80 %a, x86_fp80 %b) { %c = fcmp olt x86_fp80 %a, %b
  ret i1 %c }
define { x86_fp80, x86_fp80 } @ld_complex(x86_fp80 %a) {
  %1 = insertvalue { x86_fp80, x86_fp80 } poison, x86_fp80 %a, 0
  %2 = insertvalue { x86_fp80, x86_fp80 } %1, x86_fp80 %a, 1
  ret { x86_fp80, x86_fp80 } %2 }
