; Stack protectors as clang writes them for -fstack-protector (ssp),
; -fstack-protector-strong (sspstrong) and -fstack-protector-all (sspreq),
; on intact frames: every function returns normally and computes what it
; does without the attribute. A char array filled through a pointer, an
; int array indexed by a variable, a struct holding an array, a scalar
; whose address escapes, a dynamic alloca, a function with only plain
; locals, one with no local at all (sspreq), a musttail call from a
; protected function, a noreturn path not taken, and a loop that calls a
; protected function many times.
;@ covers: spec/ir.md §5.7 attributes (ssp, sspstrong, sspreq); spec/codegen.md §5.6.24 stack protectors
;@ stdout: chars 104 111
;@ stdout: ints 30
;@ stdout: struct 7 65
;@ stdout: escape 42
;@ stdout: dynamic 1300
;@ stdout: plain 9
;@ stdout: empty 5
;@ stdout: tail 17
;@ stdout: noreturn 3
;@ stdout: loop 4950

@.chars = private unnamed_addr constant [13 x i8] c"chars %d %d\0A\00"
@.ints = private unnamed_addr constant [9 x i8] c"ints %d\0A\00"
@.struct = private unnamed_addr constant [14 x i8] c"struct %d %d\0A\00"
@.escape = private unnamed_addr constant [11 x i8] c"escape %d\0A\00"
@.dynamic = private unnamed_addr constant [12 x i8] c"dynamic %d\0A\00"
@.plain = private unnamed_addr constant [10 x i8] c"plain %d\0A\00"
@.empty = private unnamed_addr constant [10 x i8] c"empty %d\0A\00"
@.tail = private unnamed_addr constant [9 x i8] c"tail %d\0A\00"
@.noreturn = private unnamed_addr constant [13 x i8] c"noreturn %d\0A\00"
@.loop = private unnamed_addr constant [9 x i8] c"loop %d\0A\00"
@.hello = private unnamed_addr constant [6 x i8] c"hello\00"

%struct.S = type { i32, [12 x i8] }

declare i32 @printf(ptr, ...)
declare ptr @strcpy(ptr, ptr)
declare void @abort() noreturn
declare void @llvm.memset.p0.i64(ptr, i8, i64, i1)

define void @fill(ptr %p, i32 %n) noinline {
  %w = sext i32 %n to i64
  call void @llvm.memset.p0.i64(ptr %p, i8 65, i64 %w, i1 false)
  ret void
}

define void @take(ptr %p) noinline {
  store i32 42, ptr %p
  ret void
}

; ssp: a char array of 16 bytes.
define i32 @chars() noinline ssp "stack-protector-buffer-size"="8" {
  %b = alloca [16 x i8], align 16
  call ptr @strcpy(ptr %b, ptr @.hello)
  %e = getelementptr [16 x i8], ptr %b, i64 0, i64 4
  %v0 = load i8, ptr %b
  %v4 = load i8, ptr %e
  %x0 = zext i8 %v0 to i32
  %x4 = zext i8 %v4 to i32
  call i32 (ptr, ...) @printf(ptr @.chars, i32 %x0, i32 %x4)
  ret i32 0
}

; sspstrong: an int array, indexed by a variable.
define i32 @ints(i32 %n) noinline sspstrong {
entry:
  %a = alloca [5 x i32], align 16
  %i = alloca i32, align 4
  store i32 0, ptr %i
  br label %loop
loop:
  %k = load i32, ptr %i
  %w = sext i32 %k to i64
  %p = getelementptr [5 x i32], ptr %a, i64 0, i64 %w
  %v = mul i32 %k, %n
  store i32 %v, ptr %p
  %k1 = add i32 %k, 1
  store i32 %k1, ptr %i
  %more = icmp slt i32 %k1, 5
  br i1 %more, label %loop, label %sum
sum:
  %p1 = getelementptr [5 x i32], ptr %a, i64 0, i64 1
  %p4 = getelementptr [5 x i32], ptr %a, i64 0, i64 4
  %v1 = load i32, ptr %p1
  %v4 = load i32, ptr %p4
  %s = add i32 %v1, %v4
  ret i32 %s
}

; ssp: a struct with a char array.
define i32 @structs() noinline ssp {
  %s = alloca %struct.S, align 4
  store i32 7, ptr %s
  %c = getelementptr %struct.S, ptr %s, i32 0, i32 1
  call void @fill(ptr %c, i32 12)
  %last = getelementptr %struct.S, ptr %s, i32 0, i32 1, i64 11
  %k = load i32, ptr %s
  %l = load i8, ptr %last
  %lx = zext i8 %l to i32
  call i32 (ptr, ...) @printf(ptr @.struct, i32 %k, i32 %lx)
  ret i32 0
}

; sspstrong: a scalar whose address escapes.
define i32 @escape() noinline sspstrong {
  %x = alloca i32, align 4
  store i32 0, ptr %x
  call void @take(ptr %x)
  %v = load i32, ptr %x
  ret i32 %v
}

; ssp: a dynamic alloca.
define i32 @dynamic(i32 %n) noinline ssp {
entry:
  %w = sext i32 %n to i64
  %p = alloca i8, i64 %w, align 16
  call void @fill(ptr %p, i32 %n)
  br label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %i1, %loop ]
  %acc = phi i32 [ 0, %entry ], [ %acc1, %loop ]
  %q = getelementptr i8, ptr %p, i64 %i
  %b = load i8, ptr %q
  %bx = zext i8 %b to i32
  %acc1 = add i32 %acc, %bx
  %i1 = add i64 %i, 1
  %more = icmp slt i64 %i1, %w
  br i1 %more, label %loop, label %done
done:
  ret i32 %acc1
}

; sspstrong with plain locals only: LLVM leaves it alone, hod protects it.
define i32 @plain(i32 %n) noinline sspstrong {
  %x = alloca i32, align 4
  store i32 %n, ptr %x
  %v = load i32, ptr %x
  %r = add i32 %v, 4
  ret i32 %r
}

; sspreq without a local.
define i32 @empty(i32 %n) noinline sspreq {
  %r = add i32 %n, 2
  ret i32 %r
}

define i32 @callee(i32 %a, i32 %b) noinline {
  %r = add i32 %a, %b
  ret i32 %r
}

; sspreq with a musttail call: the check comes before the jump.
define i32 @tail(i32 %a, i32 %b) noinline sspreq {
  %buf = alloca [8 x i8], align 1
  call void @fill(ptr %buf, i32 8)
  %v = load i8, ptr %buf
  %vx = zext i8 %v to i32
  %c = sub i32 %vx, 65
  %a1 = add i32 %a, %c
  %r = musttail call i32 @callee(i32 %a1, i32 %b)
  ret i32 %r
}

; sspstrong with a noreturn path that is not taken.
define i32 @noreturn(i32 %n) noinline sspstrong {
entry:
  %b = alloca [4 x i32], align 16
  store i32 %n, ptr %b
  %bad = icmp slt i32 %n, 0
  br i1 %bad, label %die, label %ok
die:
  call void @abort()
  unreachable
ok:
  %v = load i32, ptr %b
  ret i32 %v
}

define i32 @main() {
entry:
  call i32 @chars()
  %i = call i32 @ints(i32 6)
  call i32 (ptr, ...) @printf(ptr @.ints, i32 %i)
  call i32 @structs()
  %e = call i32 @escape()
  call i32 (ptr, ...) @printf(ptr @.escape, i32 %e)
  %d = call i32 @dynamic(i32 15)
  %d1 = call i32 @dynamic(i32 5)
  %dd = add i32 %d, %d1
  call i32 (ptr, ...) @printf(ptr @.dynamic, i32 %dd)
  %p = call i32 @plain(i32 5)
  call i32 (ptr, ...) @printf(ptr @.plain, i32 %p)
  %m = call i32 @empty(i32 3)
  call i32 (ptr, ...) @printf(ptr @.empty, i32 %m)
  %t = call i32 @tail(i32 7, i32 10)
  call i32 (ptr, ...) @printf(ptr @.tail, i32 %t)
  %n = call i32 @noreturn(i32 3)
  call i32 (ptr, ...) @printf(ptr @.noreturn, i32 %n)
  br label %loop
loop:
  %k = phi i32 [ 0, %entry ], [ %k1, %loop ]
  %acc = phi i32 [ 0, %entry ], [ %acc1, %loop ]
  %v = call i32 @plain(i32 %k)
  %v4 = sub i32 %v, 4
  %acc1 = add i32 %acc, %v4
  %k1 = add i32 %k, 1
  %more = icmp slt i32 %k1, 100
  br i1 %more, label %loop, label %out
out:
  call i32 (ptr, ...) @printf(ptr @.loop, i32 %acc1)
  ret i32 0
}

; As clang writes them for a PIE: without "PIC Level", llc reads
; __stack_chk_guard directly on AArch64, which no PIE link accepts.
!llvm.module.flags = !{!0, !1}
!0 = !{i32 8, !"PIC Level", i32 2}
!1 = !{i32 7, !"PIE Level", i32 2}
