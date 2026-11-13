; A smashed stack guard: a sspstrong function overflows its 16-byte char
; array by 64 bytes, through a pointer, then returns. Whatever the frame
; layout (llc's and hod's differ), the overflow passes the guard, so the
; check before the return calls __stack_chk_fail, which aborts: SIGABRT
; and "stack smashing detected" on stderr, before the overwritten return
; address is used. An intact call before it returns normally.
;@ covers: spec/ir.md §5.7 attributes (sspstrong); spec/codegen.md §5.6.24 stack protectors
;@ exit: signal 6
;@ stdout: intact 65
;@ stdout: smashing

@.intact = private unnamed_addr constant [11 x i8] c"intact %d\0A\00"
@.smashing = private unnamed_addr constant [10 x i8] c"smashing\0A\00"

declare i32 @printf(ptr, ...)
declare i32 @fflush(ptr)
declare void @llvm.memset.p0.i64(ptr, i8, i64, i1)

define void @fill(ptr %p, i32 %n) noinline {
  %w = sext i32 %n to i64
  call void @llvm.memset.p0.i64(ptr %p, i8 65, i64 %w, i1 false)
  ret void
}

define i32 @victim(i32 %extra) noinline sspstrong "stack-protector-buffer-size"="8" {
  %b = alloca [16 x i8], align 16
  %n = add i32 16, %extra
  call void @fill(ptr %b, i32 %n)
  %v = load i8, ptr %b
  %x = zext i8 %v to i32
  ret i32 %x
}

define i32 @main() sspstrong {
  %a = call i32 @victim(i32 0)
  call i32 (ptr, ...) @printf(ptr @.intact, i32 %a)
  call i32 (ptr, ...) @printf(ptr @.smashing)
  call i32 @fflush(ptr null)
  %b = call i32 @victim(i32 64)
  call i32 (ptr, ...) @printf(ptr @.intact, i32 %b)
  ret i32 0
}

; As clang writes them for a PIE: without "PIC Level", llc reads
; __stack_chk_guard directly on AArch64, which no PIE link accepts.
!llvm.module.flags = !{!0, !1}
!0 = !{i32 8, !"PIC Level", i32 2}
!1 = !{i32 7, !"PIE Level", i32 2}
