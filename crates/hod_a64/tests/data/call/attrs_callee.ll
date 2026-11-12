; WP-069: the callees attrs.ll calls through declarations that differ
; from these definitions (tests/call.rs). Each reports to attrs.c's chk.

declare void @chk(i32, i1)
declare void @llvm.va_start.p0(ptr)
declare void @llvm.va_end.p0(ptr)

define void @h_byval(ptr %p, i64 %x) {
  %a = load i64, ptr %p
  %e = getelementptr i64, ptr %p, i64 4
  %b = load i64, ptr %e
  %ok1 = icmp eq i64 %a, 1
  %ok2 = icmp eq i64 %b, 5
  %ok3 = icmp eq i64 %x, 7
  %ok12 = and i1 %ok1, %ok2
  %ok = and i1 %ok12, %ok3
  call void @chk(i32 101, i1 %ok)
  ret void
}

define void @h_sret(ptr %p, i64 %x) {
  %a = load i64, ptr %p
  %ok1 = icmp eq i64 %a, 1
  %ok2 = icmp eq i64 %x, 8
  %ok = and i1 %ok1, %ok2
  call void @chk(i32 102, i1 %ok)
  ret void
}

define void @h_nest(ptr %p, i64 %x, i64 %y) {
  %a = load i64, ptr %p
  %ok1 = icmp eq i64 %a, 1
  %ok2 = icmp eq i64 %x, 9
  %ok3 = icmp eq i64 %y, 10
  %ok12 = and i1 %ok1, %ok2
  %ok = and i1 %ok12, %ok3
  call void @chk(i32 103, i1 %ok)
  ret void
}

define void @h_var(ptr %p) {
  %a = load i64, ptr %p
  %ok = icmp eq i64 %a, 1
  call void @chk(i32 104, i1 %ok)
  ret void
}

; Variadic: the i64 after the pointer is the first variable argument,
; in x1 (the AAPCS64 va_list: __gr_top - 56 for the second register).
define void @h_fix(ptr %p, ...) {
  %ap = alloca [32 x i8], align 8
  call void @llvm.va_start.p0(ptr %ap)
  %grtop.at = getelementptr i8, ptr %ap, i64 8
  %grtop = load ptr, ptr %grtop.at
  %groffs.at = getelementptr i8, ptr %ap, i64 24
  %groffs = load i32, ptr %groffs.at
  %off = sext i32 %groffs to i64
  %slot = getelementptr i8, ptr %grtop, i64 %off
  %v = load i64, ptr %slot
  call void @llvm.va_end.p0(ptr %ap)
  %a = load i64, ptr %p
  %ok1 = icmp eq i64 %a, 1
  %ok2 = icmp eq i64 %v, 11
  %ok = and i1 %ok1, %ok2
  call void @chk(i32 105, i1 %ok)
  ret void
}

define void @h_union(ptr byval([5 x i64]) align 8 %p) {
  %a = load i64, ptr %p
  %e = getelementptr i64, ptr %p, i64 4
  %b = load i64, ptr %e
  store i64 99, ptr %p
  %ok1 = icmp eq i64 %a, 1
  %ok2 = icmp eq i64 %b, 5
  %ok = and i1 %ok1, %ok2
  call void @chk(i32 106, i1 %ok)
  ret void
}

; The register as a whole: -3 only if the caller extended the i8.
define void @h_sext(i32 %x) {
  %ok = icmp eq i32 %x, -3
  call void @chk(i32 107, i1 %ok)
  ret void
}
