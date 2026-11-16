; Overflow intrinsics, atomics and varargs together (i386 join D):
; `*.with.overflow` + `extractvalue` + `br` with atomic loops in one
; function, a `double` live across them (x87 barriers of both packages);
; variadic functions that take and return aggregates and integers of 33
; to 63 bits; a `va_list` kept in a struct, copied as an aggregate.
target datalayout = "e-m:e-p:32:32-p270:32:32-p271:32:32-p272:64:64-i128:128-f64:32:64-f80:32-n8:16:32-S128"
target triple = "i686-unknown-linux-gnu"

@fmt.ovf = private constant [36 x i8] c"ovf %d %d | %lld %d | %d %lld %lld\0A\00"
@fmt.va = private constant [31 x i8] c"va %d %lld %d %d | %lld %d %d\0A\00"
@fmt.vs = private constant [20 x i8] c"vs %d %lld %d %lld\0A\00"
@counter = global i32 0
@wide = global i64 0
@flags = global i32 0

declare i32 @printf(ptr, ...)
declare { i32, i1 } @llvm.sadd.with.overflow.i32(i32, i32)
declare { i64, i1 } @llvm.umul.with.overflow.i64(i64, i64)
declare { i48, i1 } @llvm.ssub.with.overflow.i48(i48, i48)
declare void @llvm.va_start.p0(ptr)
declare void @llvm.va_copy.p0(ptr, ptr)
declare void @llvm.va_end.p0(ptr)

; The overflow flags pick the branches; the atomics loop (`or`, `max`,
; 8-byte `add`) and their results are used; `%d` lives across all of it.
define void @ovf_atomic(i32 %a, i32 %b, i64 %c, i48 %e) noinline {
entry:
  %d0 = sitofp i32 %a to double
  %d = fmul double %d0, 1.5
  %s = call { i32, i1 } @llvm.sadd.with.overflow.i32(i32 %a, i32 %b)
  %sv = extractvalue { i32, i1 } %s, 0
  %so = extractvalue { i32, i1 } %s, 1
  br i1 %so, label %over, label %fine
over:
  %old.or = atomicrmw or ptr @flags, i32 1 seq_cst
  br label %mul
fine:
  %old.max = atomicrmw max ptr @counter, i32 %sv seq_cst
  br label %mul
mul:
  %prev = phi i32 [ %old.or, %over ], [ %old.max, %fine ]
  %m = call { i64, i1 } @llvm.umul.with.overflow.i64(i64 %c, i64 %c)
  %mo = extractvalue { i64, i1 } %m, 1
  %mv = extractvalue { i64, i1 } %m, 0
  %old.w = atomicrmw add ptr @wide, i64 %mv seq_cst
  %x = call { i48, i1 } @llvm.ssub.with.overflow.i48(i48 %e, i48 140737488355327)
  %xo = extractvalue { i48, i1 } %x, 1
  br i1 %xo, label %sat, label %done
sat:
  %old.x = atomicrmw xor ptr @flags, i32 4 seq_cst
  br label %done
done:
  %xv = extractvalue { i48, i1 } %x, 0
  %x64 = sext i48 %xv to i64
  %di = fptosi double %d to i32
  %moi = zext i1 %mo to i32
  %fl = load atomic i32, ptr @flags seq_cst, align 4
  %w = load atomic i64, ptr @wide seq_cst, align 8
  call i32 (ptr, ...) @printf(ptr @fmt.ovf, i32 %prev, i32 %di, i64 %old.w, i32 %moi, i32 %fl, i64 %w, i64 %x64)
  ret void
}

; A variadic function with an aggregate and an `i40` before the dots
; and an aggregate result: it reads an `i32`, an `i64` (an `i48` at the
; call), a `double`, as clang expands `va_arg` (pointer arithmetic).
define { i32, i64, i16 } @vsum({ i16, i8 } %pair, i40 %wide, i32 %n, ...) noinline {
entry:
  %ap = alloca ptr
  call void @llvm.va_start.p0(ptr %ap)
  %p0 = load ptr, ptr %ap
  %a = load i32, ptr %p0
  %p1 = getelementptr i8, ptr %p0, i32 4
  %b = load i64, ptr %p1, align 4
  %p2 = getelementptr i8, ptr %p1, i32 8
  %c = load double, ptr %p2, align 4
  %p3 = getelementptr i8, ptr %p2, i32 8
  store ptr %p3, ptr %ap
  call void @llvm.va_end.p0(ptr %ap)
  %h = extractvalue { i16, i8 } %pair, 0
  %l = extractvalue { i16, i8 } %pair, 1
  %h32 = sext i16 %h to i32
  %l32 = zext i8 %l to i32
  %t0 = add i32 %a, %h32
  %t1 = add i32 %t0, %l32
  %t2 = add i32 %t1, %n
  %b48 = trunc i64 %b to i48
  %bx = sext i48 %b48 to i64
  %w64 = sext i40 %wide to i64
  %bw = add i64 %bx, %w64
  %ci = fptosi double %c to i16
  %r0 = insertvalue { i32, i64, i16 } undef, i32 %t2, 0
  %r1 = insertvalue { i32, i64, i16 } %r0, i64 %bw, 1
  %r2 = insertvalue { i32, i64, i16 } %r1, i16 %ci, 2
  ret { i32, i64, i16 } %r2
}

; An `i36` result, an `i56` argument after the dots' fixed part.
define i36 @vnarrow(i56 %x, ...) noinline {
  %ap = alloca ptr
  call void @llvm.va_start.p0(ptr %ap)
  %p = load ptr, ptr %ap
  %k = load i32, ptr %p
  call void @llvm.va_end.p0(ptr %ap)
  %x36 = trunc i56 %x to i36
  %k36 = sext i32 %k to i36
  %r = mul i36 %x36, %k36
  ret i36 %r
}

; The `va_list` kept in a struct `{ ptr, i32 }`: started into the field,
; the struct copied by an aggregate load and store, `va_copy` from the
; copy's field, each list read separately.
%holder = type { ptr, i32 }

define i64 @vstruct(i32 %n, ...) noinline {
entry:
  %h = alloca %holder
  %ap = getelementptr %holder, ptr %h, i32 0, i32 0
  call void @llvm.va_start.p0(ptr %ap)
  %cnt = getelementptr %holder, ptr %h, i32 0, i32 1
  store i32 %n, ptr %cnt
  %val = load %holder, ptr %h
  %h2 = alloca %holder
  store %holder %val, ptr %h2
  %ap3 = alloca ptr
  %ap2 = getelementptr %holder, ptr %h2, i32 0, i32 0
  call void @llvm.va_copy.p0(ptr %ap3, ptr %ap2)
  ; first list: the first argument
  %q = load ptr, ptr %ap
  %first = load i32, ptr %q
  ; copied struct's list: the first two, the second is 8 bytes
  %q2 = load ptr, ptr %ap2
  %f2 = load i32, ptr %q2
  %q2b = getelementptr i8, ptr %q2, i32 4
  %s2 = load i64, ptr %q2b, align 4
  ; va_copy's list
  %q3 = load ptr, ptr %ap3
  %q3b = getelementptr i8, ptr %q3, i32 4
  %s3 = load i64, ptr %q3b, align 4
  call void @llvm.va_end.p0(ptr %ap3)
  call void @llvm.va_end.p0(ptr %ap2)
  call void @llvm.va_end.p0(ptr %ap)
  %c2 = extractvalue %holder %val, 1
  call i32 (ptr, ...) @printf(ptr @fmt.vs, i32 %first, i64 %s2, i32 %c2, i64 %s3)
  %f64 = sext i32 %f2 to i64
  %r = add i64 %f64, %s3
  ret i64 %r
}

define i32 @main() {
  call void @ovf_atomic(i32 2147483000, i32 1000, i64 4294967297, i48 5)
  call void @ovf_atomic(i32 7, i32 9, i64 3, i48 -140737488355328)
  call void @ovf_atomic(i32 -9, i32 2, i64 1, i48 -1)
  %r = call { i32, i64, i16 } ({ i16, i8 }, i40, i32, ...) @vsum({ i16, i8 } { i16 -300, i8 200 }, i40 -549755813888, i32 3, i32 40, i48 -70368744177664, double 1234.75)
  %r0 = extractvalue { i32, i64, i16 } %r, 0
  %r1 = extractvalue { i32, i64, i16 } %r, 1
  %r2 = extractvalue { i32, i64, i16 } %r, 2
  %r2i = sext i16 %r2 to i32
  %n = call i36 (i56, ...) @vnarrow(i56 -36028797018963968, i32 -3)
  %n64 = sext i36 %n to i64
  %n2 = call i36 (i56, ...) @vnarrow(i56 12345678901, i32 5)
  %n2t = trunc i36 %n2 to i32
  %n2h = lshr i36 %n2, 32
  %n2hi = trunc i36 %n2h to i32
  call i32 (ptr, ...) @printf(ptr @fmt.va, i32 %r0, i64 %r1, i32 %r2i, i32 %n2t, i64 %n64, i32 %n2hi, i32 0)
  %v = call i64 (i32, ...) @vstruct(i32 2, i32 -17, i64 -81985529216486895)
  %vt = trunc i64 %v to i32
  ret i32 0
}
