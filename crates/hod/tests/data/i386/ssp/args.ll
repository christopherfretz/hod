; WP-324: in a protected function, the incoming stack arguments are read
; from copies the prologue makes below the guarded block. `victim` writes
; `@over` bytes past its buffer (0: in bounds), then stores through its
; struct-result pointer and through a pointer argument, and returns a
; scalar argument: with an overflow every one of them lies in what was
; overwritten, and the program must abort on the guard, not fault on a
; store through an overwritten pointer.
target triple = "i686-unknown-linux-gnu"

%struct.big = type { [5 x i32] }

@over = global i32 0
@out = global i32 0
@.fmt = private constant [13 x i8] c"%d %d %d %d\0A\00"

declare void @llvm.memset.p0.i32(ptr, i8, i32, i1)
declare i32 @printf(ptr, ...)

define void @victim(ptr noalias sret(%struct.big) align 4 %r, i32 %n, ptr %dst, i32 %k) sspstrong noinline {
entry:
  %buf = alloca [16 x i8], align 1
  %more = load volatile i32, ptr @over, align 4
  %len = add i32 16, %more
  call void @llvm.memset.p0.i32(ptr %buf, i8 65, i32 %len, i1 false)
  %c = load i8, ptr %buf, align 1
  %w = sext i8 %c to i32
  %s = add i32 %w, %n
  store i32 %s, ptr %r, align 4
  %last = getelementptr inbounds i32, ptr %r, i32 4
  store i32 %k, ptr %last, align 4
  store i32 %k, ptr %dst, align 4
  br label %tail

tail:
  %p = getelementptr inbounds i32, ptr %r, i32 2
  store i32 %n, ptr %p, align 4
  ret void
}

define i32 @main() {
entry:
  %b = alloca %struct.big, align 4
  call void @llvm.memset.p0.i32(ptr %b, i8 0, i32 20, i1 false)
  call void @victim(ptr sret(%struct.big) align 4 %b, i32 3, ptr @out, i32 9)
  %v0 = load i32, ptr %b, align 4
  %p2 = getelementptr inbounds i32, ptr %b, i32 2
  %v2 = load i32, ptr %p2, align 4
  %p4 = getelementptr inbounds i32, ptr %b, i32 4
  %v4 = load i32, ptr %p4, align 4
  %o = load i32, ptr @out, align 4
  %x = call i32 (ptr, ...) @printf(ptr @.fmt, i32 %v0, i32 %v2, i32 %v4, i32 %o)
  ret i32 0
}
