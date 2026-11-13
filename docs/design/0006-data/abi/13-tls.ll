; Thread-local access sequences: every model, as llc writes them for a
; PIE (the corpus's modules carry "PIC Level" 2 and "PIE Level" 2) and,
; with the flags removed (13-tls-dso.ll), for a shared object.
@ext = external thread_local global i32
@loc = dso_local thread_local global i32 0
@ie = external thread_local(initialexec) global i32
@ld = internal thread_local(localdynamic) global i32 0
define i32 @get() {
  %a = call ptr @llvm.threadlocal.address.p0(ptr @ext)
  %b = call ptr @llvm.threadlocal.address.p0(ptr @loc)
  %c = call ptr @llvm.threadlocal.address.p0(ptr @ie)
  %d = call ptr @llvm.threadlocal.address.p0(ptr @ld)
  %x = load i32, ptr %a
  %y = load i32, ptr %b
  %z = load i32, ptr %c
  %w = load i32, ptr %d
  %s1 = add i32 %x, %y
  %s2 = add i32 %z, %w
  %s = add i32 %s1, %s2
  ret i32 %s }
declare ptr @llvm.threadlocal.address.p0(ptr)
!llvm.module.flags = !{!0, !1}
!0 = !{i32 8, !"PIC Level", i32 2}
!1 = !{i32 7, !"PIE Level", i32 2}
