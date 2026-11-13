; Aggregates passed with byval and returned with sret: a 40-byte struct
; filled through sret, passed byval to a callee that writes its copy (the
; caller's object must not change), byval behind eight integer arguments
; (past the argument registers) at alignment 16 and through a function
; pointer, sret and byval of the same type in one call, and a 3-byte
; struct byval. The expected values are worked out in the comments.
;@ covers: spec/ir.md §5.3 parameter attributes (byval, sret)
;@ stdout: made: 3000000021 4 0 3 6 9 12 1.5
;@ stdout: sum 3000000058, after: 3000000021 12
;@ stdout: twice: 6000000042 8 0 6 12 18 24 3
;@ stdout: sum9 6000000152, after: 6000000042 24
;@ stdout: small 123 (in 7 -2 9)

%Big = type { i64, i32, [5 x i16], double }
%Small = type { i8, i8, i8 }

@.made = private unnamed_addr constant [7 x i8] c"made: \00"
@.twice = private unnamed_addr constant [8 x i8] c"twice: \00"
@.fields = private unnamed_addr constant [27 x i8] c"%lld %d %d %d %d %d %d %g\0A\00"
@.sum = private unnamed_addr constant [25 x i8] c"%s %lld, after: %lld %d\0A\00"
@.small = private unnamed_addr constant [24 x i8] c"small %d (in %d %d %d)\0A\00"
@.s = private unnamed_addr constant [4 x i8] c"sum\00"
@.s9 = private unnamed_addr constant [5 x i8] c"sum9\00"

; The function pointer @main calls @sum9 through; it is loaded, so the
; call is indirect.
@sum9.ptr = global ptr @sum9

declare i32 @printf(ptr, ...)

; a = seed * 1000000007, b = seed + 1, c[i] = seed * i, d = seed * 0.5.
define internal void @make(ptr sret(%Big) align 8 %out, i64 %seed) {
entry:
  %a = mul i64 %seed, 1000000007
  store i64 %a, ptr %out
  %s32 = trunc i64 %seed to i32
  %b = add i32 %s32, 1
  %pb = getelementptr %Big, ptr %out, i32 0, i32 1
  store i32 %b, ptr %pb
  %s16 = trunc i64 %seed to i16
  br label %loop
loop:
  %i = phi i32 [ 0, %entry ], [ %next, %loop ]
  %i16 = trunc i32 %i to i16
  %c = mul i16 %s16, %i16
  %pc = getelementptr %Big, ptr %out, i32 0, i32 2, i32 %i
  store i16 %c, ptr %pc
  %next = add i32 %i, 1
  %more = icmp ult i32 %next, 5
  br i1 %more, label %loop, label %done
done:
  %sf = sitofp i64 %seed to double
  %d = fmul double %sf, 0.5
  %pd = getelementptr %Big, ptr %out, i32 0, i32 3
  store double %d, ptr %pd
  ret void
}

; a + b + c[0..4] + (d * 2), then overwrites a and c[4] of its own copy.
; For seed 3: 3000000021 + 4 + 30 + 3 = 3000000058.
define internal i64 @sum(ptr byval(%Big) align 8 %b) {
entry:
  %a = load i64, ptr %b
  %pb = getelementptr %Big, ptr %b, i32 0, i32 1
  %bv = load i32, ptr %pb
  %bw = sext i32 %bv to i64
  %r0 = add i64 %a, %bw
  br label %loop
loop:
  %i = phi i32 [ 0, %entry ], [ %next, %loop ]
  %acc = phi i64 [ %r0, %entry ], [ %acc1, %loop ]
  %pc = getelementptr %Big, ptr %b, i32 0, i32 2, i32 %i
  %c = load i16, ptr %pc
  %cw = sext i16 %c to i64
  %acc1 = add i64 %acc, %cw
  %next = add i32 %i, 1
  %more = icmp ult i32 %next, 5
  br i1 %more, label %loop, label %done
done:
  %pd = getelementptr %Big, ptr %b, i32 0, i32 3
  %d = load double, ptr %pd
  %d2 = fmul double %d, 2.0
  %di = fptosi double %d2 to i64
  %r = add i64 %acc1, %di
  store i64 -1, ptr %b
  %pc4 = getelementptr %Big, ptr %b, i32 0, i32 2, i32 4
  store i16 -1, ptr %pc4
  ret i64 %r
}

; Each field of %in doubled into %out, through sret and byval at once.
define internal void @twice(ptr sret(%Big) align 8 %out, ptr byval(%Big) align 8 %in) {
entry:
  %a = load i64, ptr %in
  %a2 = shl i64 %a, 1
  store i64 %a2, ptr %out
  %pb = getelementptr %Big, ptr %in, i32 0, i32 1
  %b = load i32, ptr %pb
  %b2 = shl i32 %b, 1
  %qb = getelementptr %Big, ptr %out, i32 0, i32 1
  store i32 %b2, ptr %qb
  br label %loop
loop:
  %i = phi i32 [ 0, %entry ], [ %next, %loop ]
  %pc = getelementptr %Big, ptr %in, i32 0, i32 2, i32 %i
  %c = load i16, ptr %pc
  %c2 = shl i16 %c, 1
  %qc = getelementptr %Big, ptr %out, i32 0, i32 2, i32 %i
  store i16 %c2, ptr %qc
  %next = add i32 %i, 1
  %more = icmp ult i32 %next, 5
  br i1 %more, label %loop, label %done
done:
  %pd = getelementptr %Big, ptr %in, i32 0, i32 3
  %d = load double, ptr %pd
  %d2 = fmul double %d, 2.0
  %qd = getelementptr %Big, ptr %out, i32 0, i32 3
  store double %d2, ptr %qd
  ret void
}

; x1 + ... + x8 + @sum(%b); %b comes after the argument registers, at
; alignment 16 (the most AAPCS64 gives a byval). For twice(seed 3): 36 + 6000000042 + 8 + 60 + 6.
define i64 @sum9(i64 %x1, i64 %x2, i64 %x3, i64 %x4, i64 %x5, i64 %x6, i64 %x7, i64 %x8, ptr byval(%Big) align 16 %b) {
  %low = ptrtoint ptr %b to i64
  %mis = and i64 %low, 15
  %bad = icmp ne i64 %mis, 0
  br i1 %bad, label %misaligned, label %ok
misaligned:
  ret i64 -1
ok:
  %s1 = add i64 %x1, %x2
  %s2 = add i64 %s1, %x3
  %s3 = add i64 %s2, %x4
  %s4 = add i64 %s3, %x5
  %s5 = add i64 %s4, %x6
  %s6 = add i64 %s5, %x7
  %s7 = add i64 %s6, %x8
  %s = call i64 @sum(ptr byval(%Big) align 8 %b)
  %r = add i64 %s7, %s
  ret i64 %r
}

; 100 + x + y + 2z with the bytes signed, then writes its copy's z:
; 100 + 7 - 2 + 18 = 123.
define internal i32 @small(ptr byval(%Small) align 1 %s) {
  %x = load i8, ptr %s
  %py = getelementptr %Small, ptr %s, i32 0, i32 1
  %y = load i8, ptr %py
  %pz = getelementptr %Small, ptr %s, i32 0, i32 2
  %z = load i8, ptr %pz
  store i8 0, ptr %pz
  %xw = sext i8 %x to i32
  %yw = sext i8 %y to i32
  %zw = sext i8 %z to i32
  %t1 = add i32 100, %xw
  %t2 = add i32 %t1, %yw
  %t3 = add i32 %t2, %zw
  %t4 = add i32 %t3, %zw
  ret i32 %t4
}

define internal void @print(ptr %label, ptr %b) {
entry:
  call i32 (ptr, ...) @printf(ptr %label)
  %a = load i64, ptr %b
  %pb = getelementptr %Big, ptr %b, i32 0, i32 1
  %bv = load i32, ptr %pb
  %p0 = getelementptr %Big, ptr %b, i32 0, i32 2, i32 0
  %c0 = load i16, ptr %p0
  %p1 = getelementptr %Big, ptr %b, i32 0, i32 2, i32 1
  %c1 = load i16, ptr %p1
  %p2 = getelementptr %Big, ptr %b, i32 0, i32 2, i32 2
  %c2 = load i16, ptr %p2
  %p3 = getelementptr %Big, ptr %b, i32 0, i32 2, i32 3
  %c3 = load i16, ptr %p3
  %p4 = getelementptr %Big, ptr %b, i32 0, i32 2, i32 4
  %c4 = load i16, ptr %p4
  %w0 = sext i16 %c0 to i32
  %w1 = sext i16 %c1 to i32
  %w2 = sext i16 %c2 to i32
  %w3 = sext i16 %c3 to i32
  %w4 = sext i16 %c4 to i32
  %pd = getelementptr %Big, ptr %b, i32 0, i32 3
  %d = load double, ptr %pd
  call i32 (ptr, ...) @printf(ptr @.fields, i64 %a, i32 %bv, i32 %w0, i32 %w1, i32 %w2, i32 %w3, i32 %w4, double %d)
  ret void
}

define internal void @after(ptr %label, i64 %r, ptr %b) {
  %a = load i64, ptr %b
  %p4 = getelementptr %Big, ptr %b, i32 0, i32 2, i32 4
  %c4 = load i16, ptr %p4
  %w4 = sext i16 %c4 to i32
  call i32 (ptr, ...) @printf(ptr @.sum, ptr %label, i64 %r, i64 %a, i32 %w4)
  ret void
}

define i32 @main() {
  %b = alloca %Big, align 8
  %c = alloca %Big, align 16  ; a byval align is also the source's
  %s = alloca %Small, align 1
  call void @make(ptr sret(%Big) align 8 %b, i64 3)
  call void @print(ptr @.made, ptr %b)
  %r = call i64 @sum(ptr byval(%Big) align 8 %b)
  call void @after(ptr @.s, i64 %r, ptr %b)
  call void @twice(ptr sret(%Big) align 8 %c, ptr byval(%Big) align 8 %b)
  call void @print(ptr @.twice, ptr %c)
  %f = load ptr, ptr @sum9.ptr
  %r9 = call i64 %f(i64 1, i64 2, i64 3, i64 4, i64 5, i64 6, i64 7, i64 8, ptr byval(%Big) align 16 %c)
  call void @after(ptr @.s9, i64 %r9, ptr %c)
  store i8 7, ptr %s
  %sy = getelementptr %Small, ptr %s, i32 0, i32 1
  store i8 -2, ptr %sy
  %sz = getelementptr %Small, ptr %s, i32 0, i32 2
  store i8 9, ptr %sz
  %rs = call i32 @small(ptr byval(%Small) align 1 %s)
  %x = load i8, ptr %s
  %y = load i8, ptr %sy
  %z = load i8, ptr %sz
  %xw = sext i8 %x to i32
  %yw = sext i8 %y to i32
  %zw = sext i8 %z to i32
  call i32 (ptr, ...) @printf(ptr @.small, i32 %rs, i32 %xw, i32 %yw, i32 %zw)
  ret i32 0
}
