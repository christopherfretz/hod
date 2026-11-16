; llvm.eh.dwarf.cfa (spec/codegen.md §5.6.38, WP-294) on both targets:
; a constant and a variable offset move the value by exactly that much,
; the value is not below the function's own frame pointer and not above
; its caller's, and stays the same after a VLA and in a variadic
; function. The absolute value is not printed: on AArch64 hod gives the
; CFA, llc 19 and 22 16 less (their frame pointer; at -O2 that is below
; the function's own locals, so they are not compared).
;@ covers: spec/ir.md §5.4 call (Intrinsic)
;@ stdout: offsets: 1 1 1
;@ stdout: above own frame: 1 below caller: 1
;@ stdout: vla: 1 variadic: 1

@.off = private unnamed_addr constant [19 x i8] c"offsets: %d %d %d\0A\00"
@.pos = private unnamed_addr constant [38 x i8] c"above own frame: %d below caller: %d\0A\00"
@.shape = private unnamed_addr constant [22 x i8] c"vla: %d variadic: %d\0A\00"

declare i32 @printf(ptr, ...)
declare ptr @llvm.eh.dwarf.cfa(i32)
declare ptr @llvm.frameaddress.p0(i32)
declare void @llvm.va_start.p0(ptr)
declare void @llvm.va_end.p0(ptr)

define void @sink(ptr %p) #0 {
  store volatile i8 1, ptr %p
  ret void
}

define i64 @delta(ptr %a, ptr %b) #0 {
  %x = ptrtoint ptr %a to i64
  %y = ptrtoint ptr %b to i64
  %d = sub i64 %x, %y
  ret i64 %d
}

; 1 where cfa(k) - cfa(0) is k: a variable 24, a variable -16, a constant 40.
define i32 @offsets(i32 %k, ptr %out) #0 {
  %c0 = call ptr @llvm.eh.dwarf.cfa(i32 0)
  %ck = call ptr @llvm.eh.dwarf.cfa(i32 %k)
  %m = sub i32 0, 16
  %cm = call ptr @llvm.eh.dwarf.cfa(i32 %m)
  %c40 = call ptr @llvm.eh.dwarf.cfa(i32 40)
  %d1 = call i64 @delta(ptr %ck, ptr %c0)
  %d2 = call i64 @delta(ptr %cm, ptr %c0)
  %d3 = call i64 @delta(ptr %c40, ptr %c0)
  %e1 = icmp eq i64 %d1, 24
  %e2 = icmp eq i64 %d2, -16
  %e3 = icmp eq i64 %d3, 40
  %z1 = zext i1 %e1 to i32
  %z2 = zext i1 %e2 to i32
  %z3 = zext i1 %e3 to i32
  store i32 %z2, ptr %out
  %o1 = getelementptr i32, ptr %out, i64 1
  store i32 %z3, ptr %o1
  ret i32 %z1
}

; The value against the function's own frame pointer and its caller's.
define ptr @position(ptr %caller, ptr %below) #0 {
  %local = alloca i64, align 8
  call void @sink(ptr %local)
  %c = call ptr @llvm.eh.dwarf.cfa(i32 0)
  %own = call ptr @llvm.frameaddress.p0(i32 0)
  %a = icmp uge ptr %c, %own
  %b = icmp ule ptr %c, %caller
  %zb = zext i1 %b to i32
  store i32 %zb, ptr %below
  %r = select i1 %a, ptr %c, ptr null
  ret ptr %r
}

; The value before and after a VLA.
define i32 @vla(i64 %n) #0 {
  %before = call ptr @llvm.eh.dwarf.cfa(i32 0)
  %v = alloca i8, i64 %n, align 16
  call void @sink(ptr %v)
  %after = call ptr @llvm.eh.dwarf.cfa(i32 0)
  %e = icmp eq ptr %before, %after
  %z = zext i1 %e to i32
  ret i32 %z
}

; In a variadic function: the same as the value its plain twin gives
; when called from the same place with the same stack arguments (none).
define ptr @variadic(i32 %n, ...) #0 {
  %ap = alloca [32 x i8], align 8
  call void @llvm.va_start.p0(ptr %ap)
  %c = call ptr @llvm.eh.dwarf.cfa(i32 0)
  call void @llvm.va_end.p0(ptr %ap)
  ret ptr %c
}

define ptr @plain(i32 %n, i64 %a, double %b) #0 {
  %c = call ptr @llvm.eh.dwarf.cfa(i32 0)
  ret ptr %c
}

define i32 @main() #0 {
  %out = alloca [2 x i32], align 4
  %e1 = call i32 @offsets(i32 24, ptr %out)
  %e2 = load i32, ptr %out
  %o1 = getelementptr i32, ptr %out, i64 1
  %e3 = load i32, ptr %o1
  call i32 (ptr, ...) @printf(ptr @.off, i32 %e1, i32 %e2, i32 %e3)
  %below = alloca i32, align 4
  %f = call ptr @llvm.frameaddress.p0(i32 0)
  %p = call ptr @position(ptr %f, ptr %below)
  %above = icmp ne ptr %p, null
  %za = zext i1 %above to i32
  %zb = load i32, ptr %below
  call i32 (ptr, ...) @printf(ptr @.pos, i32 %za, i32 %zb)
  %v = call i32 @vla(i64 100)
  %cv = call ptr (i32, ...) @variadic(i32 2, i64 7, double 1.5)
  %cp = call ptr @plain(i32 2, i64 7, double 1.5)
  %same = icmp eq ptr %cv, %cp
  %zs = zext i1 %same to i32
  call i32 (ptr, ...) @printf(ptr @.shape, i32 %v, i32 %zs)
  ret i32 0
}

attributes #0 = { noinline "frame-pointer"="all" }
