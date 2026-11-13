; The executable's module of tests/tls.rs (WP-073): local-exec and
; executable-only thread-local variables, built static or PIE.

@main_le = dso_local thread_local(localexec) global i32 10, align 4
@main_gd = thread_local global i32 11, align 4
@main_local = dso_local thread_local global [4 x i32] [i32 12, i32 13, i32 14, i32 15], align 16
@lib_gd = external thread_local global i32, align 4

define i64 @main_sum() {
  %a = load i32, ptr @main_le, align 4
  %b = load i32, ptr @main_gd, align 4
  %c = load i32, ptr getelementptr inbounds ([4 x i32], ptr @main_local, i64 0, i64 3), align 4
  %d = load i32, ptr @lib_gd, align 4
  %s1 = add i32 %a, %b
  %s2 = add i32 %s1, %c
  %s3 = add i32 %s2, %d
  %s = sext i32 %s3 to i64
  ret i64 %s
}

define void @main_bump(i32 %k) {
  %a = load i32, ptr @main_le, align 4
  %a2 = add i32 %a, %k
  store i32 %a2, ptr @main_le, align 4
  %b = load i32, ptr @main_gd, align 4
  %b2 = add i32 %b, %k
  store i32 %b2, ptr @main_gd, align 4
  %p = getelementptr inbounds [4 x i32], ptr @main_local, i64 0, i64 3
  %c = load i32, ptr %p, align 4
  %c2 = add i32 %c, %k
  store i32 %c2, ptr %p, align 4
  ret void
}

define ptr @main_addr(i32 %which) {
  %is0 = icmp eq i32 %which, 0
  %is1 = icmp eq i32 %which, 1
  %x = select i1 %is1, ptr @main_gd, ptr getelementptr inbounds ([4 x i32], ptr @main_local, i64 0, i64 3)
  %p = select i1 %is0, ptr @main_le, ptr %x
  ret ptr %p
}
