; Which runtime functions llc calls for fp128, x86_fp80, i128 and friends on x86-64.
define fp128 @q_sin(fp128 %a) { %r = call fp128 @llvm.sin.f128(fp128 %a)
  ret fp128 %r }
define fp128 @q_rem(fp128 %a, fp128 %b) { %r = frem fp128 %a, %b
  ret fp128 %r }
define fp128 @q_add(fp128 %a, fp128 %b) { %r = fadd fp128 %a, %b
  ret fp128 %r }
define x86_fp80 @l_sin(x86_fp80 %a) { %r = call x86_fp80 @llvm.sin.f80(x86_fp80 %a)
  ret x86_fp80 %r }
define x86_fp80 @l_rem(x86_fp80 %a, x86_fp80 %b) { %r = frem x86_fp80 %a, %b
  ret x86_fp80 %r }
define x86_fp80 @l_sqrt(x86_fp80 %a) { %r = call x86_fp80 @llvm.sqrt.f80(x86_fp80 %a)
  ret x86_fp80 %r }
define x86_fp80 @l_from_q(fp128 %a) { %r = fptrunc fp128 %a to x86_fp80
  ret x86_fp80 %r }
define x86_fp80 @l_from_u64(i64 %a) { %r = uitofp i64 %a to x86_fp80
  ret x86_fp80 %r }
define x86_fp80 @l_from_i128(i128 %a) { %r = sitofp i128 %a to x86_fp80
  ret x86_fp80 %r }
define double @d_from_u64(i64 %a) { %r = uitofp i64 %a to double
  ret double %r }
define i64 @d_to_u64(double %a) { %r = fptoui double %a to i64
  ret i64 %r }
define double @d_rem(double %a, double %b) { %r = frem double %a, %b
  ret double %r }
define half @h_add(half %a, half %b) { %r = fadd half %a, %b
  ret half %r }
define i32 @ctlz32(i32 %a) { %r = call i32 @llvm.ctlz.i32(i32 %a, i1 false)
  ret i32 %r }
define i32 @pop32(i32 %a) { %r = call i32 @llvm.ctpop.i32(i32 %a)
  ret i32 %r }
define double @d_min(double %a, double %b) { %r = call double @llvm.minnum.f64(double %a, double %b)
  ret double %r }
define double @d_floor(double %a) { %r = call double @llvm.floor.f64(double %a)
  ret double %r }
define i128 @a128(ptr %p, i128 %v) { %r = atomicrmw add ptr %p, i128 %v seq_cst
  ret i128 %r }
define i128 @c128(ptr %p, i128 %o, i128 %n) { %r = cmpxchg ptr %p, i128 %o, i128 %n seq_cst seq_cst
  %v = extractvalue { i128, i1 } %r, 0
  ret i128 %v }
define i128 @l128(ptr %p) { %r = load atomic i128, ptr %p seq_cst, align 16
  ret i128 %r }
define i64 @a64(ptr %p, i64 %v) { %r = atomicrmw nand ptr %p, i64 %v seq_cst
  ret i64 %r }
define void @s64(ptr %p, i64 %v) { store atomic i64 %v, ptr %p seq_cst, align 8
  ret void }
declare fp128 @llvm.sin.f128(fp128)
declare x86_fp80 @llvm.sin.f80(x86_fp80)
declare x86_fp80 @llvm.sqrt.f80(x86_fp80)
declare i32 @llvm.ctlz.i32(i32, i1)
declare i32 @llvm.ctpop.i32(i32)
declare double @llvm.minnum.f64(double, double)
declare double @llvm.floor.f64(double)
