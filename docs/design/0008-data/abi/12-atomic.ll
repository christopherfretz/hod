target triple = "i686-unknown-linux-gnu"
define i64 @ld64(ptr %p) { %v = load atomic i64, ptr %p seq_cst, align 8
  ret i64 %v }
define void @st64(ptr %p, i64 %v) { store atomic i64 %v, ptr %p seq_cst, align 8
  ret void }
define i64 @add64(ptr %p, i64 %v) { %o = atomicrmw add ptr %p, i64 %v seq_cst
  ret i64 %o }
define i64 @cx64(ptr %p, i64 %e, i64 %n) { %r = cmpxchg ptr %p, i64 %e, i64 %n seq_cst seq_cst
  %o = extractvalue {i64, i1} %r, 0
  ret i64 %o }
define i32 @add32(ptr %p, i32 %v) { %o = atomicrmw add ptr %p, i32 %v seq_cst
  ret i32 %o }
define i128 @ld128(ptr %p) { %v = load atomic i128, ptr %p seq_cst, align 16
  ret i128 %v }
define double @ldd(ptr %p) { %v = load atomic double, ptr %p seq_cst, align 8
  ret double %v }
define void @fence() { fence seq_cst
  ret void }
