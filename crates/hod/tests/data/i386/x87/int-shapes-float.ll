; Written by int_shapes.py (i386 join C): integer intrinsics between an x87 result
; and its use, in float (spec/codegen-x86.md §11). tests/i386_x87.rs lists the shapes.
target triple = "i686-unknown-linux-gnu"
declare i1 @llvm.sadd.sat.i1(i1, i1)
declare i16 @llvm.cttz.i16(i16, i1)
declare i32 @llvm.ctlz.i32(i32, i1)
declare i32 @llvm.ctpop.i32(i32)
declare i32 @llvm.cttz.i32(i32, i1)
declare i32 @llvm.fshl.i32(i32, i32, i32)
declare i32 @llvm.smax.i32(i32, i32)
declare i32 @llvm.sshl.sat.i32(i32, i32)
declare i32 @llvm.ssub.sat.i32(i32, i32)
declare i32 @llvm.uadd.sat.i32(i32, i32)
declare i32 @llvm.umax.i32(i32, i32)
declare i32 @llvm.ushl.sat.i32(i32, i32)
declare i64 @llvm.abs.i64(i64, i1)
declare i64 @llvm.bswap.i64(i64)
declare i64 @llvm.ctlz.i64(i64, i1)
declare i64 @llvm.fshl.i64(i64, i64, i64)
declare i64 @llvm.fshr.i64(i64, i64, i64)
declare i64 @llvm.umin.i64(i64, i64)
declare i64 @llvm.usub.sat.i64(i64, i64)
declare i8 @llvm.scmp.i8.i32(i32, i32)
declare i8 @llvm.smin.i8(i8, i8)
declare i8 @llvm.ucmp.i8.i64(i64, i64)

define void @ctlz32_between(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gi = getelementptr i8, ptr %in, i32 180
  %i = load i32, ptr %gi
  %gj = getelementptr i8, ptr %in, i32 184
  %j = load i32, ptr %gj
  %p = fmul float %a, %b
  %gn = getelementptr float, ptr %in, i32 3
  %n = load float, ptr %gn
  %x = fsub float %p, %n
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  %s = call i32 @llvm.ctlz.i32(i32 %i, i1 false)
  %y = fadd float %p, %c
  %oy1 = getelementptr float, ptr %out, i32 1
  store float %y, ptr %oy1
  %os = getelementptr i8, ptr %out, i32 200
  store i32 %s, ptr %os
  ret void
}

define void @ctlz32_poison(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gi = getelementptr i8, ptr %in, i32 180
  %i = load i32, ptr %gi
  %gj = getelementptr i8, ptr %in, i32 184
  %j = load i32, ptr %gj
  %p = fmul float %a, %b
  %gn = getelementptr float, ptr %in, i32 3
  %n = load float, ptr %gn
  %x = fsub float %p, %n
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  %s = call i32 @llvm.ctlz.i32(i32 %i, i1 true)
  %y = fadd float %p, %c
  %oy1 = getelementptr float, ptr %out, i32 1
  store float %y, ptr %oy1
  %os = getelementptr i8, ptr %out, i32 200
  store i32 %s, ptr %os
  ret void
}

define void @cttz32_between(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gi = getelementptr i8, ptr %in, i32 180
  %i = load i32, ptr %gi
  %gj = getelementptr i8, ptr %in, i32 184
  %j = load i32, ptr %gj
  %p = fmul float %a, %b
  %gn = getelementptr float, ptr %in, i32 3
  %n = load float, ptr %gn
  %x = fsub float %p, %n
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  %s = call i32 @llvm.cttz.i32(i32 %i, i1 false)
  %y = fadd float %p, %c
  %oy1 = getelementptr float, ptr %out, i32 1
  store float %y, ptr %oy1
  %os = getelementptr i8, ptr %out, i32 200
  store i32 %s, ptr %os
  ret void
}

define void @cttz16_between(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gi = getelementptr i8, ptr %in, i32 180
  %i = load i16, ptr %gi
  %gj = getelementptr i8, ptr %in, i32 184
  %j = load i16, ptr %gj
  %p = fmul float %a, %b
  %gn = getelementptr float, ptr %in, i32 3
  %n = load float, ptr %gn
  %x = fsub float %p, %n
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  %s = call i16 @llvm.cttz.i16(i16 %i, i1 false)
  %y = fadd float %p, %c
  %oy1 = getelementptr float, ptr %out, i32 1
  store float %y, ptr %oy1
  %os = getelementptr i8, ptr %out, i32 200
  %e = zext i16 %s to i32
  store i32 %e, ptr %os
  ret void
}

define void @ctlz64_poison(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gi = getelementptr i8, ptr %in, i32 180
  %i = load i64, ptr %gi
  %gj = getelementptr i8, ptr %in, i32 188
  %j = load i64, ptr %gj
  %p = fmul float %a, %b
  %gn = getelementptr float, ptr %in, i32 3
  %n = load float, ptr %gn
  %x = fsub float %p, %n
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  %s = call i64 @llvm.ctlz.i64(i64 %i, i1 true)
  %y = fadd float %p, %c
  %oy1 = getelementptr float, ptr %out, i32 1
  store float %y, ptr %oy1
  %os = getelementptr i8, ptr %out, i32 200
  store i64 %s, ptr %os
  ret void
}

define void @umax32_between(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gi = getelementptr i8, ptr %in, i32 180
  %i = load i32, ptr %gi
  %gj = getelementptr i8, ptr %in, i32 184
  %j = load i32, ptr %gj
  %p = fmul float %a, %b
  %gn = getelementptr float, ptr %in, i32 3
  %n = load float, ptr %gn
  %x = fsub float %p, %n
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  %s = call i32 @llvm.umax.i32(i32 %i, i32 %j)
  %y = fadd float %p, %c
  %oy1 = getelementptr float, ptr %out, i32 1
  store float %y, ptr %oy1
  %os = getelementptr i8, ptr %out, i32 200
  store i32 %s, ptr %os
  ret void
}

define void @umax32_zero(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gi = getelementptr i8, ptr %in, i32 180
  %i = load i32, ptr %gi
  %gj = getelementptr i8, ptr %in, i32 184
  %j = load i32, ptr %gj
  %p = fmul float %a, %b
  %gn = getelementptr float, ptr %in, i32 3
  %n = load float, ptr %gn
  %x = fsub float %p, %n
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  %s = call i32 @llvm.umax.i32(i32 %i, i32 0)
  %y = fadd float %p, %c
  %oy1 = getelementptr float, ptr %out, i32 1
  store float %y, ptr %oy1
  %os = getelementptr i8, ptr %out, i32 200
  store i32 %s, ptr %os
  ret void
}

define void @smin8_between(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gi = getelementptr i8, ptr %in, i32 180
  %i = load i8, ptr %gi
  %gj = getelementptr i8, ptr %in, i32 184
  %j = load i8, ptr %gj
  %p = fmul float %a, %b
  %gn = getelementptr float, ptr %in, i32 3
  %n = load float, ptr %gn
  %x = fsub float %p, %n
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  %s = call i8 @llvm.smin.i8(i8 %i, i8 %j)
  %y = fadd float %p, %c
  %oy1 = getelementptr float, ptr %out, i32 1
  store float %y, ptr %oy1
  %os = getelementptr i8, ptr %out, i32 200
  %e = zext i8 %s to i32
  store i32 %e, ptr %os
  ret void
}

define void @smax32_self(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gi = getelementptr i8, ptr %in, i32 180
  %i = load i32, ptr %gi
  %gj = getelementptr i8, ptr %in, i32 184
  %j = load i32, ptr %gj
  %p = fmul float %a, %b
  %gn = getelementptr float, ptr %in, i32 3
  %n = load float, ptr %gn
  %x = fsub float %p, %n
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  %s = call i32 @llvm.smax.i32(i32 %i, i32 %i)
  %y = fadd float %p, %c
  %oy1 = getelementptr float, ptr %out, i32 1
  store float %y, ptr %oy1
  %os = getelementptr i8, ptr %out, i32 200
  store i32 %s, ptr %os
  ret void
}

define void @umin64_between(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gi = getelementptr i8, ptr %in, i32 180
  %i = load i64, ptr %gi
  %gj = getelementptr i8, ptr %in, i32 188
  %j = load i64, ptr %gj
  %p = fmul float %a, %b
  %gn = getelementptr float, ptr %in, i32 3
  %n = load float, ptr %gn
  %x = fsub float %p, %n
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  %s = call i64 @llvm.umin.i64(i64 %i, i64 %j)
  %y = fadd float %p, %c
  %oy1 = getelementptr float, ptr %out, i32 1
  store float %y, ptr %oy1
  %os = getelementptr i8, ptr %out, i32 200
  store i64 %s, ptr %os
  ret void
}

define void @uaddsat32_between(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gi = getelementptr i8, ptr %in, i32 180
  %i = load i32, ptr %gi
  %gj = getelementptr i8, ptr %in, i32 184
  %j = load i32, ptr %gj
  %p = fmul float %a, %b
  %gn = getelementptr float, ptr %in, i32 3
  %n = load float, ptr %gn
  %x = fsub float %p, %n
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  %s = call i32 @llvm.uadd.sat.i32(i32 %i, i32 %j)
  %y = fadd float %p, %c
  %oy1 = getelementptr float, ptr %out, i32 1
  store float %y, ptr %oy1
  %os = getelementptr i8, ptr %out, i32 200
  store i32 %s, ptr %os
  ret void
}

define void @saddsat1_between(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gi = getelementptr i8, ptr %in, i32 180
  %i = load i1, ptr %gi
  %gj = getelementptr i8, ptr %in, i32 184
  %j = load i1, ptr %gj
  %p = fmul float %a, %b
  %gn = getelementptr float, ptr %in, i32 3
  %n = load float, ptr %gn
  %x = fsub float %p, %n
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  %s = call i1 @llvm.sadd.sat.i1(i1 %i, i1 %j)
  %y = fadd float %p, %c
  %oy1 = getelementptr float, ptr %out, i32 1
  store float %y, ptr %oy1
  %os = getelementptr i8, ptr %out, i32 200
  %e = zext i1 %s to i32
  store i32 %e, ptr %os
  ret void
}

define void @ssubsat32_self(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gi = getelementptr i8, ptr %in, i32 180
  %i = load i32, ptr %gi
  %gj = getelementptr i8, ptr %in, i32 184
  %j = load i32, ptr %gj
  %p = fmul float %a, %b
  %gn = getelementptr float, ptr %in, i32 3
  %n = load float, ptr %gn
  %x = fsub float %p, %n
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  %s = call i32 @llvm.ssub.sat.i32(i32 %i, i32 %i)
  %y = fadd float %p, %c
  %oy1 = getelementptr float, ptr %out, i32 1
  store float %y, ptr %oy1
  %os = getelementptr i8, ptr %out, i32 200
  store i32 %s, ptr %os
  ret void
}

define void @usubsat64_between(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gi = getelementptr i8, ptr %in, i32 180
  %i = load i64, ptr %gi
  %gj = getelementptr i8, ptr %in, i32 188
  %j = load i64, ptr %gj
  %p = fmul float %a, %b
  %gn = getelementptr float, ptr %in, i32 3
  %n = load float, ptr %gn
  %x = fsub float %p, %n
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  %s = call i64 @llvm.usub.sat.i64(i64 %i, i64 %j)
  %y = fadd float %p, %c
  %oy1 = getelementptr float, ptr %out, i32 1
  store float %y, ptr %oy1
  %os = getelementptr i8, ptr %out, i32 200
  store i64 %s, ptr %os
  ret void
}

define void @ushlsat32_between(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gi = getelementptr i8, ptr %in, i32 180
  %i = load i32, ptr %gi
  %gj = getelementptr i8, ptr %in, i32 184
  %j = load i32, ptr %gj
  %p = fmul float %a, %b
  %gn = getelementptr float, ptr %in, i32 3
  %n = load float, ptr %gn
  %x = fsub float %p, %n
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  %s = call i32 @llvm.ushl.sat.i32(i32 %i, i32 %j)
  %y = fadd float %p, %c
  %oy1 = getelementptr float, ptr %out, i32 1
  store float %y, ptr %oy1
  %os = getelementptr i8, ptr %out, i32 200
  store i32 %s, ptr %os
  ret void
}

define void @sshlsat32_zero(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gi = getelementptr i8, ptr %in, i32 180
  %i = load i32, ptr %gi
  %gj = getelementptr i8, ptr %in, i32 184
  %j = load i32, ptr %gj
  %p = fmul float %a, %b
  %gn = getelementptr float, ptr %in, i32 3
  %n = load float, ptr %gn
  %x = fsub float %p, %n
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  %s = call i32 @llvm.sshl.sat.i32(i32 %i, i32 0)
  %y = fadd float %p, %c
  %oy1 = getelementptr float, ptr %out, i32 1
  store float %y, ptr %oy1
  %os = getelementptr i8, ptr %out, i32 200
  store i32 %s, ptr %os
  ret void
}

define void @fshl32_var(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gi = getelementptr i8, ptr %in, i32 180
  %i = load i32, ptr %gi
  %gj = getelementptr i8, ptr %in, i32 184
  %j = load i32, ptr %gj
  %p = fmul float %a, %b
  %gn = getelementptr float, ptr %in, i32 3
  %n = load float, ptr %gn
  %x = fsub float %p, %n
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  %s = call i32 @llvm.fshl.i32(i32 %i, i32 %j, i32 %j)
  %y = fadd float %p, %c
  %oy1 = getelementptr float, ptr %out, i32 1
  store float %y, ptr %oy1
  %os = getelementptr i8, ptr %out, i32 200
  store i32 %s, ptr %os
  ret void
}

define void @fshl64_var(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gi = getelementptr i8, ptr %in, i32 180
  %i = load i64, ptr %gi
  %gj = getelementptr i8, ptr %in, i32 188
  %j = load i64, ptr %gj
  %p = fmul float %a, %b
  %gn = getelementptr float, ptr %in, i32 3
  %n = load float, ptr %gn
  %x = fsub float %p, %n
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  %s = call i64 @llvm.fshl.i64(i64 %i, i64 %j, i64 %j)
  %y = fadd float %p, %c
  %oy1 = getelementptr float, ptr %out, i32 1
  store float %y, ptr %oy1
  %os = getelementptr i8, ptr %out, i32 200
  store i64 %s, ptr %os
  ret void
}

define void @fshr64_const(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gi = getelementptr i8, ptr %in, i32 180
  %i = load i64, ptr %gi
  %gj = getelementptr i8, ptr %in, i32 188
  %j = load i64, ptr %gj
  %p = fmul float %a, %b
  %gn = getelementptr float, ptr %in, i32 3
  %n = load float, ptr %gn
  %x = fsub float %p, %n
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  %s = call i64 @llvm.fshr.i64(i64 %i, i64 %j, i64 5)
  %y = fadd float %p, %c
  %oy1 = getelementptr float, ptr %out, i32 1
  store float %y, ptr %oy1
  %os = getelementptr i8, ptr %out, i32 200
  store i64 %s, ptr %os
  ret void
}

define void @bswap64_between(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gi = getelementptr i8, ptr %in, i32 180
  %i = load i64, ptr %gi
  %gj = getelementptr i8, ptr %in, i32 188
  %j = load i64, ptr %gj
  %p = fmul float %a, %b
  %gn = getelementptr float, ptr %in, i32 3
  %n = load float, ptr %gn
  %x = fsub float %p, %n
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  %s = call i64 @llvm.bswap.i64(i64 %i)
  %y = fadd float %p, %c
  %oy1 = getelementptr float, ptr %out, i32 1
  store float %y, ptr %oy1
  %os = getelementptr i8, ptr %out, i32 200
  store i64 %s, ptr %os
  ret void
}

define void @abs64_between(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gi = getelementptr i8, ptr %in, i32 180
  %i = load i64, ptr %gi
  %gj = getelementptr i8, ptr %in, i32 188
  %j = load i64, ptr %gj
  %p = fmul float %a, %b
  %gn = getelementptr float, ptr %in, i32 3
  %n = load float, ptr %gn
  %x = fsub float %p, %n
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  %s = call i64 @llvm.abs.i64(i64 %i, i1 false)
  %y = fadd float %p, %c
  %oy1 = getelementptr float, ptr %out, i32 1
  store float %y, ptr %oy1
  %os = getelementptr i8, ptr %out, i32 200
  store i64 %s, ptr %os
  ret void
}

define void @ctpop32_between(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gi = getelementptr i8, ptr %in, i32 180
  %i = load i32, ptr %gi
  %gj = getelementptr i8, ptr %in, i32 184
  %j = load i32, ptr %gj
  %p = fmul float %a, %b
  %gn = getelementptr float, ptr %in, i32 3
  %n = load float, ptr %gn
  %x = fsub float %p, %n
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  %s = call i32 @llvm.ctpop.i32(i32 %i)
  %y = fadd float %p, %c
  %oy1 = getelementptr float, ptr %out, i32 1
  store float %y, ptr %oy1
  %os = getelementptr i8, ptr %out, i32 200
  store i32 %s, ptr %os
  ret void
}

define void @scmp32_between(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gi = getelementptr i8, ptr %in, i32 180
  %i = load i32, ptr %gi
  %gj = getelementptr i8, ptr %in, i32 184
  %j = load i32, ptr %gj
  %p = fmul float %a, %b
  %gn = getelementptr float, ptr %in, i32 3
  %n = load float, ptr %gn
  %x = fsub float %p, %n
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  %s = call i8 @llvm.scmp.i8.i32(i32 %i, i32 %j)
  %y = fadd float %p, %c
  %oy1 = getelementptr float, ptr %out, i32 1
  store float %y, ptr %oy1
  %os = getelementptr i8, ptr %out, i32 200
  %e = zext i8 %s to i32
  store i32 %e, ptr %os
  ret void
}

define void @ucmp64_between(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gi = getelementptr i8, ptr %in, i32 180
  %i = load i64, ptr %gi
  %gj = getelementptr i8, ptr %in, i32 188
  %j = load i64, ptr %gj
  %p = fmul float %a, %b
  %gn = getelementptr float, ptr %in, i32 3
  %n = load float, ptr %gn
  %x = fsub float %p, %n
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  %s = call i8 @llvm.ucmp.i8.i64(i64 %i, i64 %j)
  %y = fadd float %p, %c
  %oy1 = getelementptr float, ptr %out, i32 1
  store float %y, ptr %oy1
  %os = getelementptr i8, ptr %out, i32 200
  %e = zext i8 %s to i32
  store i32 %e, ptr %os
  ret void
}

@names = constant [24 x ptr] [ptr @ctlz32_between, ptr @ctlz32_poison, ptr @cttz32_between, ptr @cttz16_between, ptr @ctlz64_poison, ptr @umax32_between, ptr @umax32_zero, ptr @smin8_between, ptr @smax32_self, ptr @umin64_between, ptr @uaddsat32_between, ptr @saddsat1_between, ptr @ssubsat32_self, ptr @usubsat64_between, ptr @ushlsat32_between, ptr @sshlsat32_zero, ptr @fshl32_var, ptr @fshl64_var, ptr @fshr64_const, ptr @bswap64_between, ptr @abs64_between, ptr @ctpop32_between, ptr @scmp32_between, ptr @ucmp64_between]
@count = constant i32 24
