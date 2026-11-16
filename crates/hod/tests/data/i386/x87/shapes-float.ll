; WP-300: excess-precision shapes in float (spec/codegen-x86.md §9). Each @<shape>(ptr %in, ptr %out)
; reads a, b, c, -c from %in and stores results to %out; tests/i386_x87.rs lists the shapes.
target triple = "i686-unknown-linux-gnu"
declare void @ext() nounwind
declare float @llvm.sqrt.f32(float)
declare float @llvm.fmuladd.f32(float, float, float)

define void @single(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gnc = getelementptr float, ptr %in, i32 3
  %nc = load float, ptr %gnc
  %p = fmul float %a, %b
  %x = fadd float %p, %c
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  ret void
}

define void @twice(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gnc = getelementptr float, ptr %in, i32 3
  %nc = load float, ptr %gnc
  %p = fmul float %a, %b
  %x = fadd float %p, %c
  %y = fsub float %p, %nc
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  %oy1 = getelementptr float, ptr %out, i32 1
  store float %y, ptr %oy1
  ret void
}

define void @twice_sep(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gnc = getelementptr float, ptr %in, i32 3
  %nc = load float, ptr %gnc
  %p = fmul float %a, %b
  %x = fadd float %p, %c
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  %y = fsub float %p, %nc
  %oy1 = getelementptr float, ptr %out, i32 1
  store float %y, ptr %oy1
  ret void
}

define void @call(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gnc = getelementptr float, ptr %in, i32 3
  %nc = load float, ptr %gnc
  %p = fmul float %a, %b
  call void @ext()
  %x = fadd float %p, %c
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  ret void
}

define void @call_between(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gnc = getelementptr float, ptr %in, i32 3
  %nc = load float, ptr %gnc
  %p = fmul float %a, %b
  %x = fadd float %p, %c
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  call void @ext()
  %y = fsub float %p, %nc
  %oy1 = getelementptr float, ptr %out, i32 1
  store float %y, ptr %oy1
  ret void
}

define void @call_before(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gnc = getelementptr float, ptr %in, i32 3
  %nc = load float, ptr %gnc
  call void @ext()
  %p = fmul float %a, %b
  %x = fadd float %p, %c
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  ret void
}

define void @block(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gnc = getelementptr float, ptr %in, i32 3
  %nc = load float, ptr %gnc
  %p = fmul float %a, %b
  br label %next
next:
  %x = fadd float %p, %c
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  ret void
}

define void @block_and_local(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gnc = getelementptr float, ptr %in, i32 3
  %nc = load float, ptr %gnc
  %p = fmul float %a, %b
  %y = fsub float %p, %nc
  %oy1 = getelementptr float, ptr %out, i32 1
  store float %y, ptr %oy1
  br label %next
next:
  %x = fadd float %p, %c
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  ret void
}

define void @stored(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gnc = getelementptr float, ptr %in, i32 3
  %nc = load float, ptr %gnc
  %p = fmul float %a, %b
  %op1 = getelementptr float, ptr %out, i32 1
  store float %p, ptr %op1
  %x = fadd float %p, %c
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  ret void
}

define void @stored_after(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gnc = getelementptr float, ptr %in, i32 3
  %nc = load float, ptr %gnc
  %p = fmul float %a, %b
  %x = fadd float %p, %c
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  %op1 = getelementptr float, ptr %out, i32 1
  store float %p, ptr %op1
  ret void
}

define void @nonlifo(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gnc = getelementptr float, ptr %in, i32 3
  %nc = load float, ptr %gnc
  %p = fmul float %a, %b
  %q = fmul float %nc, %b
  %x = fadd float %p, %c
  %y = fsub float %q, %a
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  %oy1 = getelementptr float, ptr %out, i32 1
  store float %y, ptr %oy1
  ret void
}

define void @far_int(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gnc = getelementptr float, ptr %in, i32 3
  %nc = load float, ptr %gnc
  %p = fmul float %a, %b
  %i0 = add i32 0, 1
  %i1 = add i32 %i0, 1
  %i2 = add i32 %i1, 1
  %i3 = add i32 %i2, 1
  %i4 = add i32 %i3, 1
  %i5 = add i32 %i4, 1
  %i6 = add i32 %i5, 1
  %i7 = add i32 %i6, 1
  %i8 = add i32 %i7, 1
  %i9 = add i32 %i8, 1
  %i10 = add i32 %i9, 1
  %i11 = add i32 %i10, 1
  %i12 = add i32 %i11, 1
  %i13 = add i32 %i12, 1
  %i14 = add i32 %i13, 1
  %i15 = add i32 %i14, 1
  %i16 = add i32 %i15, 1
  %i17 = add i32 %i16, 1
  %i18 = add i32 %i17, 1
  %i19 = add i32 %i18, 1
  %i20 = add i32 %i19, 1
  %i21 = add i32 %i20, 1
  %i22 = add i32 %i21, 1
  %i23 = add i32 %i22, 1
  %i24 = add i32 %i23, 1
  %i25 = add i32 %i24, 1
  %i26 = add i32 %i25, 1
  %i27 = add i32 %i26, 1
  %i28 = add i32 %i27, 1
  %i29 = add i32 %i28, 1
  %x = fadd float %p, %c
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  %oi = getelementptr i8, ptr %out, i32 200
  store i32 %i29, ptr %oi
  ret void
}

define void @far_fp(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gnc = getelementptr float, ptr %in, i32 3
  %nc = load float, ptr %gnc
  %p = fmul float %a, %b
  %gl0 = getelementptr float, ptr %in, i32 4
  %l0 = load float, ptr %gl0
  %z0 = fadd float %l0, %c
  %oz02 = getelementptr float, ptr %out, i32 2
  store float %z0, ptr %oz02
  %gl1 = getelementptr float, ptr %in, i32 5
  %l1 = load float, ptr %gl1
  %z1 = fadd float %l1, %c
  %oz12 = getelementptr float, ptr %out, i32 2
  store float %z1, ptr %oz12
  %gl2 = getelementptr float, ptr %in, i32 6
  %l2 = load float, ptr %gl2
  %z2 = fadd float %l2, %c
  %oz22 = getelementptr float, ptr %out, i32 2
  store float %z2, ptr %oz22
  %gl3 = getelementptr float, ptr %in, i32 7
  %l3 = load float, ptr %gl3
  %z3 = fadd float %l3, %c
  %oz32 = getelementptr float, ptr %out, i32 2
  store float %z3, ptr %oz32
  %gl4 = getelementptr float, ptr %in, i32 8
  %l4 = load float, ptr %gl4
  %z4 = fadd float %l4, %c
  %oz42 = getelementptr float, ptr %out, i32 2
  store float %z4, ptr %oz42
  %gl5 = getelementptr float, ptr %in, i32 9
  %l5 = load float, ptr %gl5
  %z5 = fadd float %l5, %c
  %oz52 = getelementptr float, ptr %out, i32 2
  store float %z5, ptr %oz52
  %gl6 = getelementptr float, ptr %in, i32 10
  %l6 = load float, ptr %gl6
  %z6 = fadd float %l6, %c
  %oz62 = getelementptr float, ptr %out, i32 2
  store float %z6, ptr %oz62
  %gl7 = getelementptr float, ptr %in, i32 11
  %l7 = load float, ptr %gl7
  %z7 = fadd float %l7, %c
  %oz72 = getelementptr float, ptr %out, i32 2
  store float %z7, ptr %oz72
  %gl8 = getelementptr float, ptr %in, i32 12
  %l8 = load float, ptr %gl8
  %z8 = fadd float %l8, %c
  %oz82 = getelementptr float, ptr %out, i32 2
  store float %z8, ptr %oz82
  %gl9 = getelementptr float, ptr %in, i32 13
  %l9 = load float, ptr %gl9
  %z9 = fadd float %l9, %c
  %oz92 = getelementptr float, ptr %out, i32 2
  store float %z9, ptr %oz92
  %gl10 = getelementptr float, ptr %in, i32 14
  %l10 = load float, ptr %gl10
  %z10 = fadd float %l10, %c
  %oz102 = getelementptr float, ptr %out, i32 2
  store float %z10, ptr %oz102
  %gl11 = getelementptr float, ptr %in, i32 15
  %l11 = load float, ptr %gl11
  %z11 = fadd float %l11, %c
  %oz112 = getelementptr float, ptr %out, i32 2
  store float %z11, ptr %oz112
  %gl12 = getelementptr float, ptr %in, i32 16
  %l12 = load float, ptr %gl12
  %z12 = fadd float %l12, %c
  %oz122 = getelementptr float, ptr %out, i32 2
  store float %z12, ptr %oz122
  %gl13 = getelementptr float, ptr %in, i32 17
  %l13 = load float, ptr %gl13
  %z13 = fadd float %l13, %c
  %oz132 = getelementptr float, ptr %out, i32 2
  store float %z13, ptr %oz132
  %x = fadd float %p, %c
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  ret void
}

define void @many6_fifo(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gnc = getelementptr float, ptr %in, i32 3
  %nc = load float, ptr %gnc
  %ga0 = getelementptr float, ptr %in, i32 4
  %a0 = load float, ptr %ga0
  %p0 = fmul float %a0, %b
  %ga1 = getelementptr float, ptr %in, i32 5
  %a1 = load float, ptr %ga1
  %p1 = fmul float %a1, %b
  %ga2 = getelementptr float, ptr %in, i32 6
  %a2 = load float, ptr %ga2
  %p2 = fmul float %a2, %b
  %ga3 = getelementptr float, ptr %in, i32 7
  %a3 = load float, ptr %ga3
  %p3 = fmul float %a3, %b
  %ga4 = getelementptr float, ptr %in, i32 8
  %a4 = load float, ptr %ga4
  %p4 = fmul float %a4, %b
  %ga5 = getelementptr float, ptr %in, i32 9
  %a5 = load float, ptr %ga5
  %p5 = fmul float %a5, %b
  %x0 = fadd float %p0, %c
  %ox00 = getelementptr float, ptr %out, i32 0
  store float %x0, ptr %ox00
  %x1 = fadd float %p1, %c
  %ox11 = getelementptr float, ptr %out, i32 1
  store float %x1, ptr %ox11
  %x2 = fadd float %p2, %c
  %ox22 = getelementptr float, ptr %out, i32 2
  store float %x2, ptr %ox22
  %x3 = fadd float %p3, %c
  %ox33 = getelementptr float, ptr %out, i32 3
  store float %x3, ptr %ox33
  %x4 = fadd float %p4, %c
  %ox44 = getelementptr float, ptr %out, i32 4
  store float %x4, ptr %ox44
  %x5 = fadd float %p5, %c
  %ox55 = getelementptr float, ptr %out, i32 5
  store float %x5, ptr %ox55
  ret void
}

define void @many6_lifo(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gnc = getelementptr float, ptr %in, i32 3
  %nc = load float, ptr %gnc
  %ga0 = getelementptr float, ptr %in, i32 4
  %a0 = load float, ptr %ga0
  %p0 = fmul float %a0, %b
  %ga1 = getelementptr float, ptr %in, i32 5
  %a1 = load float, ptr %ga1
  %p1 = fmul float %a1, %b
  %ga2 = getelementptr float, ptr %in, i32 6
  %a2 = load float, ptr %ga2
  %p2 = fmul float %a2, %b
  %ga3 = getelementptr float, ptr %in, i32 7
  %a3 = load float, ptr %ga3
  %p3 = fmul float %a3, %b
  %ga4 = getelementptr float, ptr %in, i32 8
  %a4 = load float, ptr %ga4
  %p4 = fmul float %a4, %b
  %ga5 = getelementptr float, ptr %in, i32 9
  %a5 = load float, ptr %ga5
  %p5 = fmul float %a5, %b
  %x5 = fadd float %p5, %c
  %ox55 = getelementptr float, ptr %out, i32 5
  store float %x5, ptr %ox55
  %x4 = fadd float %p4, %c
  %ox44 = getelementptr float, ptr %out, i32 4
  store float %x4, ptr %ox44
  %x3 = fadd float %p3, %c
  %ox33 = getelementptr float, ptr %out, i32 3
  store float %x3, ptr %ox33
  %x2 = fadd float %p2, %c
  %ox22 = getelementptr float, ptr %out, i32 2
  store float %x2, ptr %ox22
  %x1 = fadd float %p1, %c
  %ox11 = getelementptr float, ptr %out, i32 1
  store float %x1, ptr %ox11
  %x0 = fadd float %p0, %c
  %ox00 = getelementptr float, ptr %out, i32 0
  store float %x0, ptr %ox00
  ret void
}

define void @many7_fifo(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gnc = getelementptr float, ptr %in, i32 3
  %nc = load float, ptr %gnc
  %ga0 = getelementptr float, ptr %in, i32 4
  %a0 = load float, ptr %ga0
  %p0 = fmul float %a0, %b
  %ga1 = getelementptr float, ptr %in, i32 5
  %a1 = load float, ptr %ga1
  %p1 = fmul float %a1, %b
  %ga2 = getelementptr float, ptr %in, i32 6
  %a2 = load float, ptr %ga2
  %p2 = fmul float %a2, %b
  %ga3 = getelementptr float, ptr %in, i32 7
  %a3 = load float, ptr %ga3
  %p3 = fmul float %a3, %b
  %ga4 = getelementptr float, ptr %in, i32 8
  %a4 = load float, ptr %ga4
  %p4 = fmul float %a4, %b
  %ga5 = getelementptr float, ptr %in, i32 9
  %a5 = load float, ptr %ga5
  %p5 = fmul float %a5, %b
  %ga6 = getelementptr float, ptr %in, i32 10
  %a6 = load float, ptr %ga6
  %p6 = fmul float %a6, %b
  %x0 = fadd float %p0, %c
  %ox00 = getelementptr float, ptr %out, i32 0
  store float %x0, ptr %ox00
  %x1 = fadd float %p1, %c
  %ox11 = getelementptr float, ptr %out, i32 1
  store float %x1, ptr %ox11
  %x2 = fadd float %p2, %c
  %ox22 = getelementptr float, ptr %out, i32 2
  store float %x2, ptr %ox22
  %x3 = fadd float %p3, %c
  %ox33 = getelementptr float, ptr %out, i32 3
  store float %x3, ptr %ox33
  %x4 = fadd float %p4, %c
  %ox44 = getelementptr float, ptr %out, i32 4
  store float %x4, ptr %ox44
  %x5 = fadd float %p5, %c
  %ox55 = getelementptr float, ptr %out, i32 5
  store float %x5, ptr %ox55
  %x6 = fadd float %p6, %c
  %ox66 = getelementptr float, ptr %out, i32 6
  store float %x6, ptr %ox66
  ret void
}

define void @many7_lifo(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gnc = getelementptr float, ptr %in, i32 3
  %nc = load float, ptr %gnc
  %ga0 = getelementptr float, ptr %in, i32 4
  %a0 = load float, ptr %ga0
  %p0 = fmul float %a0, %b
  %ga1 = getelementptr float, ptr %in, i32 5
  %a1 = load float, ptr %ga1
  %p1 = fmul float %a1, %b
  %ga2 = getelementptr float, ptr %in, i32 6
  %a2 = load float, ptr %ga2
  %p2 = fmul float %a2, %b
  %ga3 = getelementptr float, ptr %in, i32 7
  %a3 = load float, ptr %ga3
  %p3 = fmul float %a3, %b
  %ga4 = getelementptr float, ptr %in, i32 8
  %a4 = load float, ptr %ga4
  %p4 = fmul float %a4, %b
  %ga5 = getelementptr float, ptr %in, i32 9
  %a5 = load float, ptr %ga5
  %p5 = fmul float %a5, %b
  %ga6 = getelementptr float, ptr %in, i32 10
  %a6 = load float, ptr %ga6
  %p6 = fmul float %a6, %b
  %x6 = fadd float %p6, %c
  %ox66 = getelementptr float, ptr %out, i32 6
  store float %x6, ptr %ox66
  %x5 = fadd float %p5, %c
  %ox55 = getelementptr float, ptr %out, i32 5
  store float %x5, ptr %ox55
  %x4 = fadd float %p4, %c
  %ox44 = getelementptr float, ptr %out, i32 4
  store float %x4, ptr %ox44
  %x3 = fadd float %p3, %c
  %ox33 = getelementptr float, ptr %out, i32 3
  store float %x3, ptr %ox33
  %x2 = fadd float %p2, %c
  %ox22 = getelementptr float, ptr %out, i32 2
  store float %x2, ptr %ox22
  %x1 = fadd float %p1, %c
  %ox11 = getelementptr float, ptr %out, i32 1
  store float %x1, ptr %ox11
  %x0 = fadd float %p0, %c
  %ox00 = getelementptr float, ptr %out, i32 0
  store float %x0, ptr %ox00
  ret void
}

define void @many8_fifo(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gnc = getelementptr float, ptr %in, i32 3
  %nc = load float, ptr %gnc
  %ga0 = getelementptr float, ptr %in, i32 4
  %a0 = load float, ptr %ga0
  %p0 = fmul float %a0, %b
  %ga1 = getelementptr float, ptr %in, i32 5
  %a1 = load float, ptr %ga1
  %p1 = fmul float %a1, %b
  %ga2 = getelementptr float, ptr %in, i32 6
  %a2 = load float, ptr %ga2
  %p2 = fmul float %a2, %b
  %ga3 = getelementptr float, ptr %in, i32 7
  %a3 = load float, ptr %ga3
  %p3 = fmul float %a3, %b
  %ga4 = getelementptr float, ptr %in, i32 8
  %a4 = load float, ptr %ga4
  %p4 = fmul float %a4, %b
  %ga5 = getelementptr float, ptr %in, i32 9
  %a5 = load float, ptr %ga5
  %p5 = fmul float %a5, %b
  %ga6 = getelementptr float, ptr %in, i32 10
  %a6 = load float, ptr %ga6
  %p6 = fmul float %a6, %b
  %ga7 = getelementptr float, ptr %in, i32 11
  %a7 = load float, ptr %ga7
  %p7 = fmul float %a7, %b
  %x0 = fadd float %p0, %c
  %ox00 = getelementptr float, ptr %out, i32 0
  store float %x0, ptr %ox00
  %x1 = fadd float %p1, %c
  %ox11 = getelementptr float, ptr %out, i32 1
  store float %x1, ptr %ox11
  %x2 = fadd float %p2, %c
  %ox22 = getelementptr float, ptr %out, i32 2
  store float %x2, ptr %ox22
  %x3 = fadd float %p3, %c
  %ox33 = getelementptr float, ptr %out, i32 3
  store float %x3, ptr %ox33
  %x4 = fadd float %p4, %c
  %ox44 = getelementptr float, ptr %out, i32 4
  store float %x4, ptr %ox44
  %x5 = fadd float %p5, %c
  %ox55 = getelementptr float, ptr %out, i32 5
  store float %x5, ptr %ox55
  %x6 = fadd float %p6, %c
  %ox66 = getelementptr float, ptr %out, i32 6
  store float %x6, ptr %ox66
  %x7 = fadd float %p7, %c
  %ox77 = getelementptr float, ptr %out, i32 7
  store float %x7, ptr %ox77
  ret void
}

define void @many8_lifo(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gnc = getelementptr float, ptr %in, i32 3
  %nc = load float, ptr %gnc
  %ga0 = getelementptr float, ptr %in, i32 4
  %a0 = load float, ptr %ga0
  %p0 = fmul float %a0, %b
  %ga1 = getelementptr float, ptr %in, i32 5
  %a1 = load float, ptr %ga1
  %p1 = fmul float %a1, %b
  %ga2 = getelementptr float, ptr %in, i32 6
  %a2 = load float, ptr %ga2
  %p2 = fmul float %a2, %b
  %ga3 = getelementptr float, ptr %in, i32 7
  %a3 = load float, ptr %ga3
  %p3 = fmul float %a3, %b
  %ga4 = getelementptr float, ptr %in, i32 8
  %a4 = load float, ptr %ga4
  %p4 = fmul float %a4, %b
  %ga5 = getelementptr float, ptr %in, i32 9
  %a5 = load float, ptr %ga5
  %p5 = fmul float %a5, %b
  %ga6 = getelementptr float, ptr %in, i32 10
  %a6 = load float, ptr %ga6
  %p6 = fmul float %a6, %b
  %ga7 = getelementptr float, ptr %in, i32 11
  %a7 = load float, ptr %ga7
  %p7 = fmul float %a7, %b
  %x7 = fadd float %p7, %c
  %ox77 = getelementptr float, ptr %out, i32 7
  store float %x7, ptr %ox77
  %x6 = fadd float %p6, %c
  %ox66 = getelementptr float, ptr %out, i32 6
  store float %x6, ptr %ox66
  %x5 = fadd float %p5, %c
  %ox55 = getelementptr float, ptr %out, i32 5
  store float %x5, ptr %ox55
  %x4 = fadd float %p4, %c
  %ox44 = getelementptr float, ptr %out, i32 4
  store float %x4, ptr %ox44
  %x3 = fadd float %p3, %c
  %ox33 = getelementptr float, ptr %out, i32 3
  store float %x3, ptr %ox33
  %x2 = fadd float %p2, %c
  %ox22 = getelementptr float, ptr %out, i32 2
  store float %x2, ptr %ox22
  %x1 = fadd float %p1, %c
  %ox11 = getelementptr float, ptr %out, i32 1
  store float %x1, ptr %ox11
  %x0 = fadd float %p0, %c
  %ox00 = getelementptr float, ptr %out, i32 0
  store float %x0, ptr %ox00
  ret void
}

define void @many9_fifo(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gnc = getelementptr float, ptr %in, i32 3
  %nc = load float, ptr %gnc
  %ga0 = getelementptr float, ptr %in, i32 4
  %a0 = load float, ptr %ga0
  %p0 = fmul float %a0, %b
  %ga1 = getelementptr float, ptr %in, i32 5
  %a1 = load float, ptr %ga1
  %p1 = fmul float %a1, %b
  %ga2 = getelementptr float, ptr %in, i32 6
  %a2 = load float, ptr %ga2
  %p2 = fmul float %a2, %b
  %ga3 = getelementptr float, ptr %in, i32 7
  %a3 = load float, ptr %ga3
  %p3 = fmul float %a3, %b
  %ga4 = getelementptr float, ptr %in, i32 8
  %a4 = load float, ptr %ga4
  %p4 = fmul float %a4, %b
  %ga5 = getelementptr float, ptr %in, i32 9
  %a5 = load float, ptr %ga5
  %p5 = fmul float %a5, %b
  %ga6 = getelementptr float, ptr %in, i32 10
  %a6 = load float, ptr %ga6
  %p6 = fmul float %a6, %b
  %ga7 = getelementptr float, ptr %in, i32 11
  %a7 = load float, ptr %ga7
  %p7 = fmul float %a7, %b
  %ga8 = getelementptr float, ptr %in, i32 12
  %a8 = load float, ptr %ga8
  %p8 = fmul float %a8, %b
  %x0 = fadd float %p0, %c
  %ox00 = getelementptr float, ptr %out, i32 0
  store float %x0, ptr %ox00
  %x1 = fadd float %p1, %c
  %ox11 = getelementptr float, ptr %out, i32 1
  store float %x1, ptr %ox11
  %x2 = fadd float %p2, %c
  %ox22 = getelementptr float, ptr %out, i32 2
  store float %x2, ptr %ox22
  %x3 = fadd float %p3, %c
  %ox33 = getelementptr float, ptr %out, i32 3
  store float %x3, ptr %ox33
  %x4 = fadd float %p4, %c
  %ox44 = getelementptr float, ptr %out, i32 4
  store float %x4, ptr %ox44
  %x5 = fadd float %p5, %c
  %ox55 = getelementptr float, ptr %out, i32 5
  store float %x5, ptr %ox55
  %x6 = fadd float %p6, %c
  %ox66 = getelementptr float, ptr %out, i32 6
  store float %x6, ptr %ox66
  %x7 = fadd float %p7, %c
  %ox77 = getelementptr float, ptr %out, i32 7
  store float %x7, ptr %ox77
  %x8 = fadd float %p8, %c
  %ox88 = getelementptr float, ptr %out, i32 8
  store float %x8, ptr %ox88
  ret void
}

define void @many9_lifo(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gnc = getelementptr float, ptr %in, i32 3
  %nc = load float, ptr %gnc
  %ga0 = getelementptr float, ptr %in, i32 4
  %a0 = load float, ptr %ga0
  %p0 = fmul float %a0, %b
  %ga1 = getelementptr float, ptr %in, i32 5
  %a1 = load float, ptr %ga1
  %p1 = fmul float %a1, %b
  %ga2 = getelementptr float, ptr %in, i32 6
  %a2 = load float, ptr %ga2
  %p2 = fmul float %a2, %b
  %ga3 = getelementptr float, ptr %in, i32 7
  %a3 = load float, ptr %ga3
  %p3 = fmul float %a3, %b
  %ga4 = getelementptr float, ptr %in, i32 8
  %a4 = load float, ptr %ga4
  %p4 = fmul float %a4, %b
  %ga5 = getelementptr float, ptr %in, i32 9
  %a5 = load float, ptr %ga5
  %p5 = fmul float %a5, %b
  %ga6 = getelementptr float, ptr %in, i32 10
  %a6 = load float, ptr %ga6
  %p6 = fmul float %a6, %b
  %ga7 = getelementptr float, ptr %in, i32 11
  %a7 = load float, ptr %ga7
  %p7 = fmul float %a7, %b
  %ga8 = getelementptr float, ptr %in, i32 12
  %a8 = load float, ptr %ga8
  %p8 = fmul float %a8, %b
  %x8 = fadd float %p8, %c
  %ox88 = getelementptr float, ptr %out, i32 8
  store float %x8, ptr %ox88
  %x7 = fadd float %p7, %c
  %ox77 = getelementptr float, ptr %out, i32 7
  store float %x7, ptr %ox77
  %x6 = fadd float %p6, %c
  %ox66 = getelementptr float, ptr %out, i32 6
  store float %x6, ptr %ox66
  %x5 = fadd float %p5, %c
  %ox55 = getelementptr float, ptr %out, i32 5
  store float %x5, ptr %ox55
  %x4 = fadd float %p4, %c
  %ox44 = getelementptr float, ptr %out, i32 4
  store float %x4, ptr %ox44
  %x3 = fadd float %p3, %c
  %ox33 = getelementptr float, ptr %out, i32 3
  store float %x3, ptr %ox33
  %x2 = fadd float %p2, %c
  %ox22 = getelementptr float, ptr %out, i32 2
  store float %x2, ptr %ox22
  %x1 = fadd float %p1, %c
  %ox11 = getelementptr float, ptr %out, i32 1
  store float %x1, ptr %ox11
  %x0 = fadd float %p0, %c
  %ox00 = getelementptr float, ptr %out, i32 0
  store float %x0, ptr %ox00
  ret void
}

define void @many12_fifo(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gnc = getelementptr float, ptr %in, i32 3
  %nc = load float, ptr %gnc
  %ga0 = getelementptr float, ptr %in, i32 4
  %a0 = load float, ptr %ga0
  %p0 = fmul float %a0, %b
  %ga1 = getelementptr float, ptr %in, i32 5
  %a1 = load float, ptr %ga1
  %p1 = fmul float %a1, %b
  %ga2 = getelementptr float, ptr %in, i32 6
  %a2 = load float, ptr %ga2
  %p2 = fmul float %a2, %b
  %ga3 = getelementptr float, ptr %in, i32 7
  %a3 = load float, ptr %ga3
  %p3 = fmul float %a3, %b
  %ga4 = getelementptr float, ptr %in, i32 8
  %a4 = load float, ptr %ga4
  %p4 = fmul float %a4, %b
  %ga5 = getelementptr float, ptr %in, i32 9
  %a5 = load float, ptr %ga5
  %p5 = fmul float %a5, %b
  %ga6 = getelementptr float, ptr %in, i32 10
  %a6 = load float, ptr %ga6
  %p6 = fmul float %a6, %b
  %ga7 = getelementptr float, ptr %in, i32 11
  %a7 = load float, ptr %ga7
  %p7 = fmul float %a7, %b
  %ga8 = getelementptr float, ptr %in, i32 12
  %a8 = load float, ptr %ga8
  %p8 = fmul float %a8, %b
  %ga9 = getelementptr float, ptr %in, i32 13
  %a9 = load float, ptr %ga9
  %p9 = fmul float %a9, %b
  %ga10 = getelementptr float, ptr %in, i32 14
  %a10 = load float, ptr %ga10
  %p10 = fmul float %a10, %b
  %ga11 = getelementptr float, ptr %in, i32 15
  %a11 = load float, ptr %ga11
  %p11 = fmul float %a11, %b
  %x0 = fadd float %p0, %c
  %ox00 = getelementptr float, ptr %out, i32 0
  store float %x0, ptr %ox00
  %x1 = fadd float %p1, %c
  %ox11 = getelementptr float, ptr %out, i32 1
  store float %x1, ptr %ox11
  %x2 = fadd float %p2, %c
  %ox22 = getelementptr float, ptr %out, i32 2
  store float %x2, ptr %ox22
  %x3 = fadd float %p3, %c
  %ox33 = getelementptr float, ptr %out, i32 3
  store float %x3, ptr %ox33
  %x4 = fadd float %p4, %c
  %ox44 = getelementptr float, ptr %out, i32 4
  store float %x4, ptr %ox44
  %x5 = fadd float %p5, %c
  %ox55 = getelementptr float, ptr %out, i32 5
  store float %x5, ptr %ox55
  %x6 = fadd float %p6, %c
  %ox66 = getelementptr float, ptr %out, i32 6
  store float %x6, ptr %ox66
  %x7 = fadd float %p7, %c
  %ox77 = getelementptr float, ptr %out, i32 7
  store float %x7, ptr %ox77
  %x8 = fadd float %p8, %c
  %ox88 = getelementptr float, ptr %out, i32 8
  store float %x8, ptr %ox88
  %x9 = fadd float %p9, %c
  %ox99 = getelementptr float, ptr %out, i32 9
  store float %x9, ptr %ox99
  %x10 = fadd float %p10, %c
  %ox1010 = getelementptr float, ptr %out, i32 10
  store float %x10, ptr %ox1010
  %x11 = fadd float %p11, %c
  %ox1111 = getelementptr float, ptr %out, i32 11
  store float %x11, ptr %ox1111
  ret void
}

define void @many12_lifo(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gnc = getelementptr float, ptr %in, i32 3
  %nc = load float, ptr %gnc
  %ga0 = getelementptr float, ptr %in, i32 4
  %a0 = load float, ptr %ga0
  %p0 = fmul float %a0, %b
  %ga1 = getelementptr float, ptr %in, i32 5
  %a1 = load float, ptr %ga1
  %p1 = fmul float %a1, %b
  %ga2 = getelementptr float, ptr %in, i32 6
  %a2 = load float, ptr %ga2
  %p2 = fmul float %a2, %b
  %ga3 = getelementptr float, ptr %in, i32 7
  %a3 = load float, ptr %ga3
  %p3 = fmul float %a3, %b
  %ga4 = getelementptr float, ptr %in, i32 8
  %a4 = load float, ptr %ga4
  %p4 = fmul float %a4, %b
  %ga5 = getelementptr float, ptr %in, i32 9
  %a5 = load float, ptr %ga5
  %p5 = fmul float %a5, %b
  %ga6 = getelementptr float, ptr %in, i32 10
  %a6 = load float, ptr %ga6
  %p6 = fmul float %a6, %b
  %ga7 = getelementptr float, ptr %in, i32 11
  %a7 = load float, ptr %ga7
  %p7 = fmul float %a7, %b
  %ga8 = getelementptr float, ptr %in, i32 12
  %a8 = load float, ptr %ga8
  %p8 = fmul float %a8, %b
  %ga9 = getelementptr float, ptr %in, i32 13
  %a9 = load float, ptr %ga9
  %p9 = fmul float %a9, %b
  %ga10 = getelementptr float, ptr %in, i32 14
  %a10 = load float, ptr %ga10
  %p10 = fmul float %a10, %b
  %ga11 = getelementptr float, ptr %in, i32 15
  %a11 = load float, ptr %ga11
  %p11 = fmul float %a11, %b
  %x11 = fadd float %p11, %c
  %ox1111 = getelementptr float, ptr %out, i32 11
  store float %x11, ptr %ox1111
  %x10 = fadd float %p10, %c
  %ox1010 = getelementptr float, ptr %out, i32 10
  store float %x10, ptr %ox1010
  %x9 = fadd float %p9, %c
  %ox99 = getelementptr float, ptr %out, i32 9
  store float %x9, ptr %ox99
  %x8 = fadd float %p8, %c
  %ox88 = getelementptr float, ptr %out, i32 8
  store float %x8, ptr %ox88
  %x7 = fadd float %p7, %c
  %ox77 = getelementptr float, ptr %out, i32 7
  store float %x7, ptr %ox77
  %x6 = fadd float %p6, %c
  %ox66 = getelementptr float, ptr %out, i32 6
  store float %x6, ptr %ox66
  %x5 = fadd float %p5, %c
  %ox55 = getelementptr float, ptr %out, i32 5
  store float %x5, ptr %ox55
  %x4 = fadd float %p4, %c
  %ox44 = getelementptr float, ptr %out, i32 4
  store float %x4, ptr %ox44
  %x3 = fadd float %p3, %c
  %ox33 = getelementptr float, ptr %out, i32 3
  store float %x3, ptr %ox33
  %x2 = fadd float %p2, %c
  %ox22 = getelementptr float, ptr %out, i32 2
  store float %x2, ptr %ox22
  %x1 = fadd float %p1, %c
  %ox11 = getelementptr float, ptr %out, i32 1
  store float %x1, ptr %ox11
  %x0 = fadd float %p0, %c
  %ox00 = getelementptr float, ptr %out, i32 0
  store float %x0, ptr %ox00
  ret void
}

define void @sqrt_between(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gnc = getelementptr float, ptr %in, i32 3
  %nc = load float, ptr %gnc
  %p = fmul float %a, %b
  %s = call float @llvm.sqrt.f32(float %c)
  %x = fadd float %p, %c
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  %os2 = getelementptr float, ptr %out, i32 2
  store float %s, ptr %os2
  ret void
}

define void @frem_between(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gnc = getelementptr float, ptr %in, i32 3
  %nc = load float, ptr %gnc
  %p = fmul float %a, %b
  %s = frem float %c, %b
  %x = fadd float %p, %c
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  %os2 = getelementptr float, ptr %out, i32 2
  store float %s, ptr %os2
  ret void
}

define void @chain_use_twice(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gnc = getelementptr float, ptr %in, i32 3
  %nc = load float, ptr %gnc
  %p = fmul float %a, %b
  %x = fadd float %p, %c
  %y = fadd float %x, %x
  %oy0 = getelementptr float, ptr %out, i32 0
  store float %y, ptr %oy0
  %ox1 = getelementptr float, ptr %out, i32 1
  store float %x, ptr %ox1
  ret void
}

define void @phi_use(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gnc = getelementptr float, ptr %in, i32 3
  %nc = load float, ptr %gnc
  %p = fmul float %a, %b
  br label %next
next:
  %ph = phi float [ %p, %entry ]
  %x = fadd float %ph, %c
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  ret void
}

define void @select_use(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gnc = getelementptr float, ptr %in, i32 3
  %nc = load float, ptr %gnc
  %p = fmul float %a, %b
  %s = select i1 true, float %p, float %c
  %x = fadd float %s, %c
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  ret void
}

define void @cmp_twice(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gnc = getelementptr float, ptr %in, i32 3
  %nc = load float, ptr %gnc
  %p = fmul float %a, %b
  %e = fcmp oeq float %p, %nc
  %e2 = fcmp une float %p, %nc
  %ei = zext i1 %e to i32
  %ei2 = zext i1 %e2 to i32
  %oi = getelementptr i8, ptr %out, i32 200
  store i32 %ei, ptr %oi
  %oi2 = getelementptr i8, ptr %out, i32 204
  store i32 %ei2, ptr %oi2
  ret void
}

define void @select_var(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gnc = getelementptr float, ptr %in, i32 3
  %nc = load float, ptr %gnc
  %p = fmul float %a, %b
  %k = fcmp oeq float %a, %b
  %s = select i1 %k, float %p, float %c
  %x = fadd float %s, %c
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  ret void
}

define void @freeze_use(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gnc = getelementptr float, ptr %in, i32 3
  %nc = load float, ptr %gnc
  %p = fmul float %a, %b
  %fr = freeze float %p
  %x = fadd float %fr, %c
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  ret void
}

define void @fpext_use(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gnc = getelementptr float, ptr %in, i32 3
  %nc = load float, ptr %gnc
  %p = fmul float %a, %b
  %d = fpext float %p to x86_fp80
  %e = fpext float %c to x86_fp80
  %x = fadd x86_fp80 %d, %e
  %xr = fptrunc x86_fp80 %x to float
  %y = fmul float %xr, 0x4190000000000000
  %oy0 = getelementptr float, ptr %out, i32 0
  store float %y, ptr %oy0
  ret void
}

define void @fptrunc_use(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gnc = getelementptr float, ptr %in, i32 3
  %nc = load float, ptr %gnc
  %p = fmul float %a, %b
  %d = fpext float %p to x86_fp80
  %t = fptrunc x86_fp80 %d to float
  %x = fadd float %t, %c
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  ret void
}

define void @sitofp_use(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gnc = getelementptr float, ptr %in, i32 3
  %nc = load float, ptr %gnc
  %f = sitofp i32 16777217 to float
  %i = sitofp i32 -16777216 to float
  %f2 = fadd float %f, 0.0
  %x = fadd float %f2, %i
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  ret void
}

define void @sitofp_var(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gnc = getelementptr float, ptr %in, i32 3
  %nc = load float, ptr %gnc
  %gi = getelementptr i8, ptr %in, i32 180
  %iv = load i32, ptr %gi
  %f = sitofp i32 %iv to float
  %x = fadd float %f, -16777216.0
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  ret void
}

define void @fneg_use(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gnc = getelementptr float, ptr %in, i32 3
  %nc = load float, ptr %gnc
  %p = fmul float %a, %b
  %n = fneg float %p
  %x = fsub float %c, %n
  %ox0 = getelementptr float, ptr %out, i32 0
  store float %x, ptr %ox0
  ret void
}

define void @fmuladd(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr float, ptr %in, i32 0
  %a = load float, ptr %ga
  %gb = getelementptr float, ptr %in, i32 1
  %b = load float, ptr %gb
  %gc = getelementptr float, ptr %in, i32 2
  %c = load float, ptr %gc
  %gnc = getelementptr float, ptr %in, i32 3
  %nc = load float, ptr %gnc
  %m = call float @llvm.fmuladd.f32(float %a, float %b, float %c)
  %om0 = getelementptr float, ptr %out, i32 0
  store float %m, ptr %om0
  ret void
}

@names = constant [37 x ptr] [ptr @single, ptr @twice, ptr @twice_sep, ptr @call, ptr @call_between, ptr @call_before, ptr @block, ptr @block_and_local, ptr @stored, ptr @stored_after, ptr @nonlifo, ptr @far_int, ptr @far_fp, ptr @many6_fifo, ptr @many6_lifo, ptr @many7_fifo, ptr @many7_lifo, ptr @many8_fifo, ptr @many8_lifo, ptr @many9_fifo, ptr @many9_lifo, ptr @many12_fifo, ptr @many12_lifo, ptr @sqrt_between, ptr @frem_between, ptr @chain_use_twice, ptr @phi_use, ptr @select_use, ptr @cmp_twice, ptr @select_var, ptr @freeze_use, ptr @fpext_use, ptr @fptrunc_use, ptr @sitofp_use, ptr @sitofp_var, ptr @fneg_use, ptr @fmuladd]
@count = constant i32 37
