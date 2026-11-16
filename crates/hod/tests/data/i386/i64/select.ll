; WP-298: `select`, `phi`s, `freeze` and constants of `i64` and `i40` on
; i386, from globals; the output is the checksum's halves.
target datalayout = "e-m:e-p:32:32-p270:32:32-p271:32:32-p272:64:64-i128:128-f64:32:64-f80:32-n8:16:32-S128"
target triple = "i686-unknown-linux-gnu"

@vals = internal global [6 x i64] [i64 0, i64 -1, i64 4294967295, i64 4294967296, i64 -9223372036854775808, i64 81985529216486895]
@sum = internal global i64 0
@fmt = private constant [11 x i8] c"%08x %08x\0A\00"
@done = private constant [5 x i8] c"done\00"

declare i32 @printf(ptr, ...)
declare i32 @puts(ptr)

define internal void @step(ptr %pa, ptr %pb) #0 {
entry:
  %a = load i64, ptr %pa
  %b = load i64, ptr %pb
  %lt = icmp slt i64 %a, %b
  %min = select i1 %lt, i64 %a, i64 %b
  %ugt = icmp ugt i64 %a, %b
  %max = select i1 %ugt, i64 %a, i64 %b
  %eq = icmp eq i64 %a, %b
  %k = select i1 %eq, i64 81985529216486895, i64 -4294967296
  %fa = freeze i64 %a
  %t40 = trunc i64 %fa to i40
  %u40 = trunc i64 %b to i40
  %c40 = icmp ult i40 %t40, %u40
  %s40 = select i1 %c40, i40 %t40, i40 549755813887
  %z40 = zext i40 %s40 to i64
  %lo = trunc i64 %b to i32
  %odd = and i32 %lo, 1
  %isodd = icmp ne i32 %odd, 0
  br i1 %isodd, label %one, label %two
one:
  %x1 = add i64 %min, 1
  br label %join
two:
  br label %join
join:
  %p = phi i64 [ %x1, %one ], [ 12345678901234, %two ]
  %q = phi i64 [ %max, %one ], [ %k, %two ]
  %s0 = load i64, ptr @sum
  %s1 = mul i64 %s0, 1000003
  %s2 = add i64 %s1, %p
  %s3 = xor i64 %s2, %q
  %s4 = add i64 %s3, %z40
  %s5 = add i64 %s4, %min
  %s6 = sub i64 %s5, %k
  store i64 %s6, ptr @sum
  ret void
}

define i32 @main() #0 {
entry:
  br label %outer
outer:
  %i = phi i32 [ 0, %entry ], [ %i1, %outer.next ]
  br label %inner
inner:
  %j = phi i32 [ 0, %outer ], [ %j1, %inner ]
  %pa = getelementptr [6 x i64], ptr @vals, i32 0, i32 %i
  %pb = getelementptr [6 x i64], ptr @vals, i32 0, i32 %j
  call void @step(ptr %pa, ptr %pb)
  %j1 = add i32 %j, 1
  %jd = icmp eq i32 %j1, 6
  br i1 %jd, label %outer.next, label %inner
outer.next:
  %i1 = add i32 %i, 1
  %id = icmp eq i32 %i1, 6
  br i1 %id, label %exit, label %outer
exit:
  %s = load i64, ptr @sum
  %hi64 = lshr i64 %s, 32
  %hi = trunc i64 %hi64 to i32
  %lo = trunc i64 %s to i32
  %r = call i32 (ptr, ...) @printf(ptr @fmt, i32 %hi, i32 %lo)
  %d = call i32 @puts(ptr @done)
  ret i32 0
}

attributes #0 = { nounwind }
