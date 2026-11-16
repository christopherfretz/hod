; WP-300: excess-precision shapes in double (spec/codegen-x86.md §9). Each @<shape>(ptr %in, ptr %out)
; reads a, b, c, -c from %in and stores results to %out; tests/i386_x87.rs lists the shapes.
target triple = "i686-unknown-linux-gnu"
declare void @ext() nounwind
declare double @llvm.sqrt.f64(double)
declare double @llvm.fmuladd.f64(double, double, double)

define void @single(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr double, ptr %in, i32 0
  %a = load double, ptr %ga
  %gb = getelementptr double, ptr %in, i32 1
  %b = load double, ptr %gb
  %gc = getelementptr double, ptr %in, i32 2
  %c = load double, ptr %gc
  %gnc = getelementptr double, ptr %in, i32 3
  %nc = load double, ptr %gnc
  %p = fmul double %a, %b
  %x = fadd double %p, %c
  %ox0 = getelementptr double, ptr %out, i32 0
  store double %x, ptr %ox0
  ret void
}

define void @twice(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr double, ptr %in, i32 0
  %a = load double, ptr %ga
  %gb = getelementptr double, ptr %in, i32 1
  %b = load double, ptr %gb
  %gc = getelementptr double, ptr %in, i32 2
  %c = load double, ptr %gc
  %gnc = getelementptr double, ptr %in, i32 3
  %nc = load double, ptr %gnc
  %p = fmul double %a, %b
  %x = fadd double %p, %c
  %y = fsub double %p, %nc
  %ox0 = getelementptr double, ptr %out, i32 0
  store double %x, ptr %ox0
  %oy1 = getelementptr double, ptr %out, i32 1
  store double %y, ptr %oy1
  ret void
}

define void @twice_sep(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr double, ptr %in, i32 0
  %a = load double, ptr %ga
  %gb = getelementptr double, ptr %in, i32 1
  %b = load double, ptr %gb
  %gc = getelementptr double, ptr %in, i32 2
  %c = load double, ptr %gc
  %gnc = getelementptr double, ptr %in, i32 3
  %nc = load double, ptr %gnc
  %p = fmul double %a, %b
  %x = fadd double %p, %c
  %ox0 = getelementptr double, ptr %out, i32 0
  store double %x, ptr %ox0
  %y = fsub double %p, %nc
  %oy1 = getelementptr double, ptr %out, i32 1
  store double %y, ptr %oy1
  ret void
}

define void @call(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr double, ptr %in, i32 0
  %a = load double, ptr %ga
  %gb = getelementptr double, ptr %in, i32 1
  %b = load double, ptr %gb
  %gc = getelementptr double, ptr %in, i32 2
  %c = load double, ptr %gc
  %gnc = getelementptr double, ptr %in, i32 3
  %nc = load double, ptr %gnc
  %p = fmul double %a, %b
  call void @ext()
  %x = fadd double %p, %c
  %ox0 = getelementptr double, ptr %out, i32 0
  store double %x, ptr %ox0
  ret void
}

define void @call_between(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr double, ptr %in, i32 0
  %a = load double, ptr %ga
  %gb = getelementptr double, ptr %in, i32 1
  %b = load double, ptr %gb
  %gc = getelementptr double, ptr %in, i32 2
  %c = load double, ptr %gc
  %gnc = getelementptr double, ptr %in, i32 3
  %nc = load double, ptr %gnc
  %p = fmul double %a, %b
  %x = fadd double %p, %c
  %ox0 = getelementptr double, ptr %out, i32 0
  store double %x, ptr %ox0
  call void @ext()
  %y = fsub double %p, %nc
  %oy1 = getelementptr double, ptr %out, i32 1
  store double %y, ptr %oy1
  ret void
}

define void @call_before(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr double, ptr %in, i32 0
  %a = load double, ptr %ga
  %gb = getelementptr double, ptr %in, i32 1
  %b = load double, ptr %gb
  %gc = getelementptr double, ptr %in, i32 2
  %c = load double, ptr %gc
  %gnc = getelementptr double, ptr %in, i32 3
  %nc = load double, ptr %gnc
  call void @ext()
  %p = fmul double %a, %b
  %x = fadd double %p, %c
  %ox0 = getelementptr double, ptr %out, i32 0
  store double %x, ptr %ox0
  ret void
}

define void @block(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr double, ptr %in, i32 0
  %a = load double, ptr %ga
  %gb = getelementptr double, ptr %in, i32 1
  %b = load double, ptr %gb
  %gc = getelementptr double, ptr %in, i32 2
  %c = load double, ptr %gc
  %gnc = getelementptr double, ptr %in, i32 3
  %nc = load double, ptr %gnc
  %p = fmul double %a, %b
  br label %next
next:
  %x = fadd double %p, %c
  %ox0 = getelementptr double, ptr %out, i32 0
  store double %x, ptr %ox0
  ret void
}

define void @block_and_local(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr double, ptr %in, i32 0
  %a = load double, ptr %ga
  %gb = getelementptr double, ptr %in, i32 1
  %b = load double, ptr %gb
  %gc = getelementptr double, ptr %in, i32 2
  %c = load double, ptr %gc
  %gnc = getelementptr double, ptr %in, i32 3
  %nc = load double, ptr %gnc
  %p = fmul double %a, %b
  %y = fsub double %p, %nc
  %oy1 = getelementptr double, ptr %out, i32 1
  store double %y, ptr %oy1
  br label %next
next:
  %x = fadd double %p, %c
  %ox0 = getelementptr double, ptr %out, i32 0
  store double %x, ptr %ox0
  ret void
}

define void @stored(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr double, ptr %in, i32 0
  %a = load double, ptr %ga
  %gb = getelementptr double, ptr %in, i32 1
  %b = load double, ptr %gb
  %gc = getelementptr double, ptr %in, i32 2
  %c = load double, ptr %gc
  %gnc = getelementptr double, ptr %in, i32 3
  %nc = load double, ptr %gnc
  %p = fmul double %a, %b
  %op1 = getelementptr double, ptr %out, i32 1
  store double %p, ptr %op1
  %x = fadd double %p, %c
  %ox0 = getelementptr double, ptr %out, i32 0
  store double %x, ptr %ox0
  ret void
}

define void @stored_after(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr double, ptr %in, i32 0
  %a = load double, ptr %ga
  %gb = getelementptr double, ptr %in, i32 1
  %b = load double, ptr %gb
  %gc = getelementptr double, ptr %in, i32 2
  %c = load double, ptr %gc
  %gnc = getelementptr double, ptr %in, i32 3
  %nc = load double, ptr %gnc
  %p = fmul double %a, %b
  %x = fadd double %p, %c
  %ox0 = getelementptr double, ptr %out, i32 0
  store double %x, ptr %ox0
  %op1 = getelementptr double, ptr %out, i32 1
  store double %p, ptr %op1
  ret void
}

define void @nonlifo(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr double, ptr %in, i32 0
  %a = load double, ptr %ga
  %gb = getelementptr double, ptr %in, i32 1
  %b = load double, ptr %gb
  %gc = getelementptr double, ptr %in, i32 2
  %c = load double, ptr %gc
  %gnc = getelementptr double, ptr %in, i32 3
  %nc = load double, ptr %gnc
  %p = fmul double %a, %b
  %q = fmul double %nc, %b
  %x = fadd double %p, %c
  %y = fsub double %q, %a
  %ox0 = getelementptr double, ptr %out, i32 0
  store double %x, ptr %ox0
  %oy1 = getelementptr double, ptr %out, i32 1
  store double %y, ptr %oy1
  ret void
}

define void @far_int(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr double, ptr %in, i32 0
  %a = load double, ptr %ga
  %gb = getelementptr double, ptr %in, i32 1
  %b = load double, ptr %gb
  %gc = getelementptr double, ptr %in, i32 2
  %c = load double, ptr %gc
  %gnc = getelementptr double, ptr %in, i32 3
  %nc = load double, ptr %gnc
  %p = fmul double %a, %b
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
  %x = fadd double %p, %c
  %ox0 = getelementptr double, ptr %out, i32 0
  store double %x, ptr %ox0
  %oi = getelementptr i8, ptr %out, i32 200
  store i32 %i29, ptr %oi
  ret void
}

define void @far_fp(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr double, ptr %in, i32 0
  %a = load double, ptr %ga
  %gb = getelementptr double, ptr %in, i32 1
  %b = load double, ptr %gb
  %gc = getelementptr double, ptr %in, i32 2
  %c = load double, ptr %gc
  %gnc = getelementptr double, ptr %in, i32 3
  %nc = load double, ptr %gnc
  %p = fmul double %a, %b
  %gl0 = getelementptr double, ptr %in, i32 4
  %l0 = load double, ptr %gl0
  %z0 = fadd double %l0, %c
  %oz02 = getelementptr double, ptr %out, i32 2
  store double %z0, ptr %oz02
  %gl1 = getelementptr double, ptr %in, i32 5
  %l1 = load double, ptr %gl1
  %z1 = fadd double %l1, %c
  %oz12 = getelementptr double, ptr %out, i32 2
  store double %z1, ptr %oz12
  %gl2 = getelementptr double, ptr %in, i32 6
  %l2 = load double, ptr %gl2
  %z2 = fadd double %l2, %c
  %oz22 = getelementptr double, ptr %out, i32 2
  store double %z2, ptr %oz22
  %gl3 = getelementptr double, ptr %in, i32 7
  %l3 = load double, ptr %gl3
  %z3 = fadd double %l3, %c
  %oz32 = getelementptr double, ptr %out, i32 2
  store double %z3, ptr %oz32
  %gl4 = getelementptr double, ptr %in, i32 8
  %l4 = load double, ptr %gl4
  %z4 = fadd double %l4, %c
  %oz42 = getelementptr double, ptr %out, i32 2
  store double %z4, ptr %oz42
  %gl5 = getelementptr double, ptr %in, i32 9
  %l5 = load double, ptr %gl5
  %z5 = fadd double %l5, %c
  %oz52 = getelementptr double, ptr %out, i32 2
  store double %z5, ptr %oz52
  %gl6 = getelementptr double, ptr %in, i32 10
  %l6 = load double, ptr %gl6
  %z6 = fadd double %l6, %c
  %oz62 = getelementptr double, ptr %out, i32 2
  store double %z6, ptr %oz62
  %gl7 = getelementptr double, ptr %in, i32 11
  %l7 = load double, ptr %gl7
  %z7 = fadd double %l7, %c
  %oz72 = getelementptr double, ptr %out, i32 2
  store double %z7, ptr %oz72
  %gl8 = getelementptr double, ptr %in, i32 12
  %l8 = load double, ptr %gl8
  %z8 = fadd double %l8, %c
  %oz82 = getelementptr double, ptr %out, i32 2
  store double %z8, ptr %oz82
  %gl9 = getelementptr double, ptr %in, i32 13
  %l9 = load double, ptr %gl9
  %z9 = fadd double %l9, %c
  %oz92 = getelementptr double, ptr %out, i32 2
  store double %z9, ptr %oz92
  %gl10 = getelementptr double, ptr %in, i32 14
  %l10 = load double, ptr %gl10
  %z10 = fadd double %l10, %c
  %oz102 = getelementptr double, ptr %out, i32 2
  store double %z10, ptr %oz102
  %gl11 = getelementptr double, ptr %in, i32 15
  %l11 = load double, ptr %gl11
  %z11 = fadd double %l11, %c
  %oz112 = getelementptr double, ptr %out, i32 2
  store double %z11, ptr %oz112
  %gl12 = getelementptr double, ptr %in, i32 16
  %l12 = load double, ptr %gl12
  %z12 = fadd double %l12, %c
  %oz122 = getelementptr double, ptr %out, i32 2
  store double %z12, ptr %oz122
  %gl13 = getelementptr double, ptr %in, i32 17
  %l13 = load double, ptr %gl13
  %z13 = fadd double %l13, %c
  %oz132 = getelementptr double, ptr %out, i32 2
  store double %z13, ptr %oz132
  %x = fadd double %p, %c
  %ox0 = getelementptr double, ptr %out, i32 0
  store double %x, ptr %ox0
  ret void
}

define void @many6_fifo(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr double, ptr %in, i32 0
  %a = load double, ptr %ga
  %gb = getelementptr double, ptr %in, i32 1
  %b = load double, ptr %gb
  %gc = getelementptr double, ptr %in, i32 2
  %c = load double, ptr %gc
  %gnc = getelementptr double, ptr %in, i32 3
  %nc = load double, ptr %gnc
  %ga0 = getelementptr double, ptr %in, i32 4
  %a0 = load double, ptr %ga0
  %p0 = fmul double %a0, %b
  %ga1 = getelementptr double, ptr %in, i32 5
  %a1 = load double, ptr %ga1
  %p1 = fmul double %a1, %b
  %ga2 = getelementptr double, ptr %in, i32 6
  %a2 = load double, ptr %ga2
  %p2 = fmul double %a2, %b
  %ga3 = getelementptr double, ptr %in, i32 7
  %a3 = load double, ptr %ga3
  %p3 = fmul double %a3, %b
  %ga4 = getelementptr double, ptr %in, i32 8
  %a4 = load double, ptr %ga4
  %p4 = fmul double %a4, %b
  %ga5 = getelementptr double, ptr %in, i32 9
  %a5 = load double, ptr %ga5
  %p5 = fmul double %a5, %b
  %x0 = fadd double %p0, %c
  %ox00 = getelementptr double, ptr %out, i32 0
  store double %x0, ptr %ox00
  %x1 = fadd double %p1, %c
  %ox11 = getelementptr double, ptr %out, i32 1
  store double %x1, ptr %ox11
  %x2 = fadd double %p2, %c
  %ox22 = getelementptr double, ptr %out, i32 2
  store double %x2, ptr %ox22
  %x3 = fadd double %p3, %c
  %ox33 = getelementptr double, ptr %out, i32 3
  store double %x3, ptr %ox33
  %x4 = fadd double %p4, %c
  %ox44 = getelementptr double, ptr %out, i32 4
  store double %x4, ptr %ox44
  %x5 = fadd double %p5, %c
  %ox55 = getelementptr double, ptr %out, i32 5
  store double %x5, ptr %ox55
  ret void
}

define void @many6_lifo(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr double, ptr %in, i32 0
  %a = load double, ptr %ga
  %gb = getelementptr double, ptr %in, i32 1
  %b = load double, ptr %gb
  %gc = getelementptr double, ptr %in, i32 2
  %c = load double, ptr %gc
  %gnc = getelementptr double, ptr %in, i32 3
  %nc = load double, ptr %gnc
  %ga0 = getelementptr double, ptr %in, i32 4
  %a0 = load double, ptr %ga0
  %p0 = fmul double %a0, %b
  %ga1 = getelementptr double, ptr %in, i32 5
  %a1 = load double, ptr %ga1
  %p1 = fmul double %a1, %b
  %ga2 = getelementptr double, ptr %in, i32 6
  %a2 = load double, ptr %ga2
  %p2 = fmul double %a2, %b
  %ga3 = getelementptr double, ptr %in, i32 7
  %a3 = load double, ptr %ga3
  %p3 = fmul double %a3, %b
  %ga4 = getelementptr double, ptr %in, i32 8
  %a4 = load double, ptr %ga4
  %p4 = fmul double %a4, %b
  %ga5 = getelementptr double, ptr %in, i32 9
  %a5 = load double, ptr %ga5
  %p5 = fmul double %a5, %b
  %x5 = fadd double %p5, %c
  %ox55 = getelementptr double, ptr %out, i32 5
  store double %x5, ptr %ox55
  %x4 = fadd double %p4, %c
  %ox44 = getelementptr double, ptr %out, i32 4
  store double %x4, ptr %ox44
  %x3 = fadd double %p3, %c
  %ox33 = getelementptr double, ptr %out, i32 3
  store double %x3, ptr %ox33
  %x2 = fadd double %p2, %c
  %ox22 = getelementptr double, ptr %out, i32 2
  store double %x2, ptr %ox22
  %x1 = fadd double %p1, %c
  %ox11 = getelementptr double, ptr %out, i32 1
  store double %x1, ptr %ox11
  %x0 = fadd double %p0, %c
  %ox00 = getelementptr double, ptr %out, i32 0
  store double %x0, ptr %ox00
  ret void
}

define void @many7_fifo(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr double, ptr %in, i32 0
  %a = load double, ptr %ga
  %gb = getelementptr double, ptr %in, i32 1
  %b = load double, ptr %gb
  %gc = getelementptr double, ptr %in, i32 2
  %c = load double, ptr %gc
  %gnc = getelementptr double, ptr %in, i32 3
  %nc = load double, ptr %gnc
  %ga0 = getelementptr double, ptr %in, i32 4
  %a0 = load double, ptr %ga0
  %p0 = fmul double %a0, %b
  %ga1 = getelementptr double, ptr %in, i32 5
  %a1 = load double, ptr %ga1
  %p1 = fmul double %a1, %b
  %ga2 = getelementptr double, ptr %in, i32 6
  %a2 = load double, ptr %ga2
  %p2 = fmul double %a2, %b
  %ga3 = getelementptr double, ptr %in, i32 7
  %a3 = load double, ptr %ga3
  %p3 = fmul double %a3, %b
  %ga4 = getelementptr double, ptr %in, i32 8
  %a4 = load double, ptr %ga4
  %p4 = fmul double %a4, %b
  %ga5 = getelementptr double, ptr %in, i32 9
  %a5 = load double, ptr %ga5
  %p5 = fmul double %a5, %b
  %ga6 = getelementptr double, ptr %in, i32 10
  %a6 = load double, ptr %ga6
  %p6 = fmul double %a6, %b
  %x0 = fadd double %p0, %c
  %ox00 = getelementptr double, ptr %out, i32 0
  store double %x0, ptr %ox00
  %x1 = fadd double %p1, %c
  %ox11 = getelementptr double, ptr %out, i32 1
  store double %x1, ptr %ox11
  %x2 = fadd double %p2, %c
  %ox22 = getelementptr double, ptr %out, i32 2
  store double %x2, ptr %ox22
  %x3 = fadd double %p3, %c
  %ox33 = getelementptr double, ptr %out, i32 3
  store double %x3, ptr %ox33
  %x4 = fadd double %p4, %c
  %ox44 = getelementptr double, ptr %out, i32 4
  store double %x4, ptr %ox44
  %x5 = fadd double %p5, %c
  %ox55 = getelementptr double, ptr %out, i32 5
  store double %x5, ptr %ox55
  %x6 = fadd double %p6, %c
  %ox66 = getelementptr double, ptr %out, i32 6
  store double %x6, ptr %ox66
  ret void
}

define void @many7_lifo(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr double, ptr %in, i32 0
  %a = load double, ptr %ga
  %gb = getelementptr double, ptr %in, i32 1
  %b = load double, ptr %gb
  %gc = getelementptr double, ptr %in, i32 2
  %c = load double, ptr %gc
  %gnc = getelementptr double, ptr %in, i32 3
  %nc = load double, ptr %gnc
  %ga0 = getelementptr double, ptr %in, i32 4
  %a0 = load double, ptr %ga0
  %p0 = fmul double %a0, %b
  %ga1 = getelementptr double, ptr %in, i32 5
  %a1 = load double, ptr %ga1
  %p1 = fmul double %a1, %b
  %ga2 = getelementptr double, ptr %in, i32 6
  %a2 = load double, ptr %ga2
  %p2 = fmul double %a2, %b
  %ga3 = getelementptr double, ptr %in, i32 7
  %a3 = load double, ptr %ga3
  %p3 = fmul double %a3, %b
  %ga4 = getelementptr double, ptr %in, i32 8
  %a4 = load double, ptr %ga4
  %p4 = fmul double %a4, %b
  %ga5 = getelementptr double, ptr %in, i32 9
  %a5 = load double, ptr %ga5
  %p5 = fmul double %a5, %b
  %ga6 = getelementptr double, ptr %in, i32 10
  %a6 = load double, ptr %ga6
  %p6 = fmul double %a6, %b
  %x6 = fadd double %p6, %c
  %ox66 = getelementptr double, ptr %out, i32 6
  store double %x6, ptr %ox66
  %x5 = fadd double %p5, %c
  %ox55 = getelementptr double, ptr %out, i32 5
  store double %x5, ptr %ox55
  %x4 = fadd double %p4, %c
  %ox44 = getelementptr double, ptr %out, i32 4
  store double %x4, ptr %ox44
  %x3 = fadd double %p3, %c
  %ox33 = getelementptr double, ptr %out, i32 3
  store double %x3, ptr %ox33
  %x2 = fadd double %p2, %c
  %ox22 = getelementptr double, ptr %out, i32 2
  store double %x2, ptr %ox22
  %x1 = fadd double %p1, %c
  %ox11 = getelementptr double, ptr %out, i32 1
  store double %x1, ptr %ox11
  %x0 = fadd double %p0, %c
  %ox00 = getelementptr double, ptr %out, i32 0
  store double %x0, ptr %ox00
  ret void
}

define void @many8_fifo(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr double, ptr %in, i32 0
  %a = load double, ptr %ga
  %gb = getelementptr double, ptr %in, i32 1
  %b = load double, ptr %gb
  %gc = getelementptr double, ptr %in, i32 2
  %c = load double, ptr %gc
  %gnc = getelementptr double, ptr %in, i32 3
  %nc = load double, ptr %gnc
  %ga0 = getelementptr double, ptr %in, i32 4
  %a0 = load double, ptr %ga0
  %p0 = fmul double %a0, %b
  %ga1 = getelementptr double, ptr %in, i32 5
  %a1 = load double, ptr %ga1
  %p1 = fmul double %a1, %b
  %ga2 = getelementptr double, ptr %in, i32 6
  %a2 = load double, ptr %ga2
  %p2 = fmul double %a2, %b
  %ga3 = getelementptr double, ptr %in, i32 7
  %a3 = load double, ptr %ga3
  %p3 = fmul double %a3, %b
  %ga4 = getelementptr double, ptr %in, i32 8
  %a4 = load double, ptr %ga4
  %p4 = fmul double %a4, %b
  %ga5 = getelementptr double, ptr %in, i32 9
  %a5 = load double, ptr %ga5
  %p5 = fmul double %a5, %b
  %ga6 = getelementptr double, ptr %in, i32 10
  %a6 = load double, ptr %ga6
  %p6 = fmul double %a6, %b
  %ga7 = getelementptr double, ptr %in, i32 11
  %a7 = load double, ptr %ga7
  %p7 = fmul double %a7, %b
  %x0 = fadd double %p0, %c
  %ox00 = getelementptr double, ptr %out, i32 0
  store double %x0, ptr %ox00
  %x1 = fadd double %p1, %c
  %ox11 = getelementptr double, ptr %out, i32 1
  store double %x1, ptr %ox11
  %x2 = fadd double %p2, %c
  %ox22 = getelementptr double, ptr %out, i32 2
  store double %x2, ptr %ox22
  %x3 = fadd double %p3, %c
  %ox33 = getelementptr double, ptr %out, i32 3
  store double %x3, ptr %ox33
  %x4 = fadd double %p4, %c
  %ox44 = getelementptr double, ptr %out, i32 4
  store double %x4, ptr %ox44
  %x5 = fadd double %p5, %c
  %ox55 = getelementptr double, ptr %out, i32 5
  store double %x5, ptr %ox55
  %x6 = fadd double %p6, %c
  %ox66 = getelementptr double, ptr %out, i32 6
  store double %x6, ptr %ox66
  %x7 = fadd double %p7, %c
  %ox77 = getelementptr double, ptr %out, i32 7
  store double %x7, ptr %ox77
  ret void
}

define void @many8_lifo(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr double, ptr %in, i32 0
  %a = load double, ptr %ga
  %gb = getelementptr double, ptr %in, i32 1
  %b = load double, ptr %gb
  %gc = getelementptr double, ptr %in, i32 2
  %c = load double, ptr %gc
  %gnc = getelementptr double, ptr %in, i32 3
  %nc = load double, ptr %gnc
  %ga0 = getelementptr double, ptr %in, i32 4
  %a0 = load double, ptr %ga0
  %p0 = fmul double %a0, %b
  %ga1 = getelementptr double, ptr %in, i32 5
  %a1 = load double, ptr %ga1
  %p1 = fmul double %a1, %b
  %ga2 = getelementptr double, ptr %in, i32 6
  %a2 = load double, ptr %ga2
  %p2 = fmul double %a2, %b
  %ga3 = getelementptr double, ptr %in, i32 7
  %a3 = load double, ptr %ga3
  %p3 = fmul double %a3, %b
  %ga4 = getelementptr double, ptr %in, i32 8
  %a4 = load double, ptr %ga4
  %p4 = fmul double %a4, %b
  %ga5 = getelementptr double, ptr %in, i32 9
  %a5 = load double, ptr %ga5
  %p5 = fmul double %a5, %b
  %ga6 = getelementptr double, ptr %in, i32 10
  %a6 = load double, ptr %ga6
  %p6 = fmul double %a6, %b
  %ga7 = getelementptr double, ptr %in, i32 11
  %a7 = load double, ptr %ga7
  %p7 = fmul double %a7, %b
  %x7 = fadd double %p7, %c
  %ox77 = getelementptr double, ptr %out, i32 7
  store double %x7, ptr %ox77
  %x6 = fadd double %p6, %c
  %ox66 = getelementptr double, ptr %out, i32 6
  store double %x6, ptr %ox66
  %x5 = fadd double %p5, %c
  %ox55 = getelementptr double, ptr %out, i32 5
  store double %x5, ptr %ox55
  %x4 = fadd double %p4, %c
  %ox44 = getelementptr double, ptr %out, i32 4
  store double %x4, ptr %ox44
  %x3 = fadd double %p3, %c
  %ox33 = getelementptr double, ptr %out, i32 3
  store double %x3, ptr %ox33
  %x2 = fadd double %p2, %c
  %ox22 = getelementptr double, ptr %out, i32 2
  store double %x2, ptr %ox22
  %x1 = fadd double %p1, %c
  %ox11 = getelementptr double, ptr %out, i32 1
  store double %x1, ptr %ox11
  %x0 = fadd double %p0, %c
  %ox00 = getelementptr double, ptr %out, i32 0
  store double %x0, ptr %ox00
  ret void
}

define void @many9_fifo(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr double, ptr %in, i32 0
  %a = load double, ptr %ga
  %gb = getelementptr double, ptr %in, i32 1
  %b = load double, ptr %gb
  %gc = getelementptr double, ptr %in, i32 2
  %c = load double, ptr %gc
  %gnc = getelementptr double, ptr %in, i32 3
  %nc = load double, ptr %gnc
  %ga0 = getelementptr double, ptr %in, i32 4
  %a0 = load double, ptr %ga0
  %p0 = fmul double %a0, %b
  %ga1 = getelementptr double, ptr %in, i32 5
  %a1 = load double, ptr %ga1
  %p1 = fmul double %a1, %b
  %ga2 = getelementptr double, ptr %in, i32 6
  %a2 = load double, ptr %ga2
  %p2 = fmul double %a2, %b
  %ga3 = getelementptr double, ptr %in, i32 7
  %a3 = load double, ptr %ga3
  %p3 = fmul double %a3, %b
  %ga4 = getelementptr double, ptr %in, i32 8
  %a4 = load double, ptr %ga4
  %p4 = fmul double %a4, %b
  %ga5 = getelementptr double, ptr %in, i32 9
  %a5 = load double, ptr %ga5
  %p5 = fmul double %a5, %b
  %ga6 = getelementptr double, ptr %in, i32 10
  %a6 = load double, ptr %ga6
  %p6 = fmul double %a6, %b
  %ga7 = getelementptr double, ptr %in, i32 11
  %a7 = load double, ptr %ga7
  %p7 = fmul double %a7, %b
  %ga8 = getelementptr double, ptr %in, i32 12
  %a8 = load double, ptr %ga8
  %p8 = fmul double %a8, %b
  %x0 = fadd double %p0, %c
  %ox00 = getelementptr double, ptr %out, i32 0
  store double %x0, ptr %ox00
  %x1 = fadd double %p1, %c
  %ox11 = getelementptr double, ptr %out, i32 1
  store double %x1, ptr %ox11
  %x2 = fadd double %p2, %c
  %ox22 = getelementptr double, ptr %out, i32 2
  store double %x2, ptr %ox22
  %x3 = fadd double %p3, %c
  %ox33 = getelementptr double, ptr %out, i32 3
  store double %x3, ptr %ox33
  %x4 = fadd double %p4, %c
  %ox44 = getelementptr double, ptr %out, i32 4
  store double %x4, ptr %ox44
  %x5 = fadd double %p5, %c
  %ox55 = getelementptr double, ptr %out, i32 5
  store double %x5, ptr %ox55
  %x6 = fadd double %p6, %c
  %ox66 = getelementptr double, ptr %out, i32 6
  store double %x6, ptr %ox66
  %x7 = fadd double %p7, %c
  %ox77 = getelementptr double, ptr %out, i32 7
  store double %x7, ptr %ox77
  %x8 = fadd double %p8, %c
  %ox88 = getelementptr double, ptr %out, i32 8
  store double %x8, ptr %ox88
  ret void
}

define void @many9_lifo(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr double, ptr %in, i32 0
  %a = load double, ptr %ga
  %gb = getelementptr double, ptr %in, i32 1
  %b = load double, ptr %gb
  %gc = getelementptr double, ptr %in, i32 2
  %c = load double, ptr %gc
  %gnc = getelementptr double, ptr %in, i32 3
  %nc = load double, ptr %gnc
  %ga0 = getelementptr double, ptr %in, i32 4
  %a0 = load double, ptr %ga0
  %p0 = fmul double %a0, %b
  %ga1 = getelementptr double, ptr %in, i32 5
  %a1 = load double, ptr %ga1
  %p1 = fmul double %a1, %b
  %ga2 = getelementptr double, ptr %in, i32 6
  %a2 = load double, ptr %ga2
  %p2 = fmul double %a2, %b
  %ga3 = getelementptr double, ptr %in, i32 7
  %a3 = load double, ptr %ga3
  %p3 = fmul double %a3, %b
  %ga4 = getelementptr double, ptr %in, i32 8
  %a4 = load double, ptr %ga4
  %p4 = fmul double %a4, %b
  %ga5 = getelementptr double, ptr %in, i32 9
  %a5 = load double, ptr %ga5
  %p5 = fmul double %a5, %b
  %ga6 = getelementptr double, ptr %in, i32 10
  %a6 = load double, ptr %ga6
  %p6 = fmul double %a6, %b
  %ga7 = getelementptr double, ptr %in, i32 11
  %a7 = load double, ptr %ga7
  %p7 = fmul double %a7, %b
  %ga8 = getelementptr double, ptr %in, i32 12
  %a8 = load double, ptr %ga8
  %p8 = fmul double %a8, %b
  %x8 = fadd double %p8, %c
  %ox88 = getelementptr double, ptr %out, i32 8
  store double %x8, ptr %ox88
  %x7 = fadd double %p7, %c
  %ox77 = getelementptr double, ptr %out, i32 7
  store double %x7, ptr %ox77
  %x6 = fadd double %p6, %c
  %ox66 = getelementptr double, ptr %out, i32 6
  store double %x6, ptr %ox66
  %x5 = fadd double %p5, %c
  %ox55 = getelementptr double, ptr %out, i32 5
  store double %x5, ptr %ox55
  %x4 = fadd double %p4, %c
  %ox44 = getelementptr double, ptr %out, i32 4
  store double %x4, ptr %ox44
  %x3 = fadd double %p3, %c
  %ox33 = getelementptr double, ptr %out, i32 3
  store double %x3, ptr %ox33
  %x2 = fadd double %p2, %c
  %ox22 = getelementptr double, ptr %out, i32 2
  store double %x2, ptr %ox22
  %x1 = fadd double %p1, %c
  %ox11 = getelementptr double, ptr %out, i32 1
  store double %x1, ptr %ox11
  %x0 = fadd double %p0, %c
  %ox00 = getelementptr double, ptr %out, i32 0
  store double %x0, ptr %ox00
  ret void
}

define void @many12_fifo(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr double, ptr %in, i32 0
  %a = load double, ptr %ga
  %gb = getelementptr double, ptr %in, i32 1
  %b = load double, ptr %gb
  %gc = getelementptr double, ptr %in, i32 2
  %c = load double, ptr %gc
  %gnc = getelementptr double, ptr %in, i32 3
  %nc = load double, ptr %gnc
  %ga0 = getelementptr double, ptr %in, i32 4
  %a0 = load double, ptr %ga0
  %p0 = fmul double %a0, %b
  %ga1 = getelementptr double, ptr %in, i32 5
  %a1 = load double, ptr %ga1
  %p1 = fmul double %a1, %b
  %ga2 = getelementptr double, ptr %in, i32 6
  %a2 = load double, ptr %ga2
  %p2 = fmul double %a2, %b
  %ga3 = getelementptr double, ptr %in, i32 7
  %a3 = load double, ptr %ga3
  %p3 = fmul double %a3, %b
  %ga4 = getelementptr double, ptr %in, i32 8
  %a4 = load double, ptr %ga4
  %p4 = fmul double %a4, %b
  %ga5 = getelementptr double, ptr %in, i32 9
  %a5 = load double, ptr %ga5
  %p5 = fmul double %a5, %b
  %ga6 = getelementptr double, ptr %in, i32 10
  %a6 = load double, ptr %ga6
  %p6 = fmul double %a6, %b
  %ga7 = getelementptr double, ptr %in, i32 11
  %a7 = load double, ptr %ga7
  %p7 = fmul double %a7, %b
  %ga8 = getelementptr double, ptr %in, i32 12
  %a8 = load double, ptr %ga8
  %p8 = fmul double %a8, %b
  %ga9 = getelementptr double, ptr %in, i32 13
  %a9 = load double, ptr %ga9
  %p9 = fmul double %a9, %b
  %ga10 = getelementptr double, ptr %in, i32 14
  %a10 = load double, ptr %ga10
  %p10 = fmul double %a10, %b
  %ga11 = getelementptr double, ptr %in, i32 15
  %a11 = load double, ptr %ga11
  %p11 = fmul double %a11, %b
  %x0 = fadd double %p0, %c
  %ox00 = getelementptr double, ptr %out, i32 0
  store double %x0, ptr %ox00
  %x1 = fadd double %p1, %c
  %ox11 = getelementptr double, ptr %out, i32 1
  store double %x1, ptr %ox11
  %x2 = fadd double %p2, %c
  %ox22 = getelementptr double, ptr %out, i32 2
  store double %x2, ptr %ox22
  %x3 = fadd double %p3, %c
  %ox33 = getelementptr double, ptr %out, i32 3
  store double %x3, ptr %ox33
  %x4 = fadd double %p4, %c
  %ox44 = getelementptr double, ptr %out, i32 4
  store double %x4, ptr %ox44
  %x5 = fadd double %p5, %c
  %ox55 = getelementptr double, ptr %out, i32 5
  store double %x5, ptr %ox55
  %x6 = fadd double %p6, %c
  %ox66 = getelementptr double, ptr %out, i32 6
  store double %x6, ptr %ox66
  %x7 = fadd double %p7, %c
  %ox77 = getelementptr double, ptr %out, i32 7
  store double %x7, ptr %ox77
  %x8 = fadd double %p8, %c
  %ox88 = getelementptr double, ptr %out, i32 8
  store double %x8, ptr %ox88
  %x9 = fadd double %p9, %c
  %ox99 = getelementptr double, ptr %out, i32 9
  store double %x9, ptr %ox99
  %x10 = fadd double %p10, %c
  %ox1010 = getelementptr double, ptr %out, i32 10
  store double %x10, ptr %ox1010
  %x11 = fadd double %p11, %c
  %ox1111 = getelementptr double, ptr %out, i32 11
  store double %x11, ptr %ox1111
  ret void
}

define void @many12_lifo(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr double, ptr %in, i32 0
  %a = load double, ptr %ga
  %gb = getelementptr double, ptr %in, i32 1
  %b = load double, ptr %gb
  %gc = getelementptr double, ptr %in, i32 2
  %c = load double, ptr %gc
  %gnc = getelementptr double, ptr %in, i32 3
  %nc = load double, ptr %gnc
  %ga0 = getelementptr double, ptr %in, i32 4
  %a0 = load double, ptr %ga0
  %p0 = fmul double %a0, %b
  %ga1 = getelementptr double, ptr %in, i32 5
  %a1 = load double, ptr %ga1
  %p1 = fmul double %a1, %b
  %ga2 = getelementptr double, ptr %in, i32 6
  %a2 = load double, ptr %ga2
  %p2 = fmul double %a2, %b
  %ga3 = getelementptr double, ptr %in, i32 7
  %a3 = load double, ptr %ga3
  %p3 = fmul double %a3, %b
  %ga4 = getelementptr double, ptr %in, i32 8
  %a4 = load double, ptr %ga4
  %p4 = fmul double %a4, %b
  %ga5 = getelementptr double, ptr %in, i32 9
  %a5 = load double, ptr %ga5
  %p5 = fmul double %a5, %b
  %ga6 = getelementptr double, ptr %in, i32 10
  %a6 = load double, ptr %ga6
  %p6 = fmul double %a6, %b
  %ga7 = getelementptr double, ptr %in, i32 11
  %a7 = load double, ptr %ga7
  %p7 = fmul double %a7, %b
  %ga8 = getelementptr double, ptr %in, i32 12
  %a8 = load double, ptr %ga8
  %p8 = fmul double %a8, %b
  %ga9 = getelementptr double, ptr %in, i32 13
  %a9 = load double, ptr %ga9
  %p9 = fmul double %a9, %b
  %ga10 = getelementptr double, ptr %in, i32 14
  %a10 = load double, ptr %ga10
  %p10 = fmul double %a10, %b
  %ga11 = getelementptr double, ptr %in, i32 15
  %a11 = load double, ptr %ga11
  %p11 = fmul double %a11, %b
  %x11 = fadd double %p11, %c
  %ox1111 = getelementptr double, ptr %out, i32 11
  store double %x11, ptr %ox1111
  %x10 = fadd double %p10, %c
  %ox1010 = getelementptr double, ptr %out, i32 10
  store double %x10, ptr %ox1010
  %x9 = fadd double %p9, %c
  %ox99 = getelementptr double, ptr %out, i32 9
  store double %x9, ptr %ox99
  %x8 = fadd double %p8, %c
  %ox88 = getelementptr double, ptr %out, i32 8
  store double %x8, ptr %ox88
  %x7 = fadd double %p7, %c
  %ox77 = getelementptr double, ptr %out, i32 7
  store double %x7, ptr %ox77
  %x6 = fadd double %p6, %c
  %ox66 = getelementptr double, ptr %out, i32 6
  store double %x6, ptr %ox66
  %x5 = fadd double %p5, %c
  %ox55 = getelementptr double, ptr %out, i32 5
  store double %x5, ptr %ox55
  %x4 = fadd double %p4, %c
  %ox44 = getelementptr double, ptr %out, i32 4
  store double %x4, ptr %ox44
  %x3 = fadd double %p3, %c
  %ox33 = getelementptr double, ptr %out, i32 3
  store double %x3, ptr %ox33
  %x2 = fadd double %p2, %c
  %ox22 = getelementptr double, ptr %out, i32 2
  store double %x2, ptr %ox22
  %x1 = fadd double %p1, %c
  %ox11 = getelementptr double, ptr %out, i32 1
  store double %x1, ptr %ox11
  %x0 = fadd double %p0, %c
  %ox00 = getelementptr double, ptr %out, i32 0
  store double %x0, ptr %ox00
  ret void
}

define void @sqrt_between(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr double, ptr %in, i32 0
  %a = load double, ptr %ga
  %gb = getelementptr double, ptr %in, i32 1
  %b = load double, ptr %gb
  %gc = getelementptr double, ptr %in, i32 2
  %c = load double, ptr %gc
  %gnc = getelementptr double, ptr %in, i32 3
  %nc = load double, ptr %gnc
  %p = fmul double %a, %b
  %s = call double @llvm.sqrt.f64(double %c)
  %x = fadd double %p, %c
  %ox0 = getelementptr double, ptr %out, i32 0
  store double %x, ptr %ox0
  %os2 = getelementptr double, ptr %out, i32 2
  store double %s, ptr %os2
  ret void
}

define void @frem_between(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr double, ptr %in, i32 0
  %a = load double, ptr %ga
  %gb = getelementptr double, ptr %in, i32 1
  %b = load double, ptr %gb
  %gc = getelementptr double, ptr %in, i32 2
  %c = load double, ptr %gc
  %gnc = getelementptr double, ptr %in, i32 3
  %nc = load double, ptr %gnc
  %p = fmul double %a, %b
  %s = frem double %c, %b
  %x = fadd double %p, %c
  %ox0 = getelementptr double, ptr %out, i32 0
  store double %x, ptr %ox0
  %os2 = getelementptr double, ptr %out, i32 2
  store double %s, ptr %os2
  ret void
}

define void @chain_use_twice(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr double, ptr %in, i32 0
  %a = load double, ptr %ga
  %gb = getelementptr double, ptr %in, i32 1
  %b = load double, ptr %gb
  %gc = getelementptr double, ptr %in, i32 2
  %c = load double, ptr %gc
  %gnc = getelementptr double, ptr %in, i32 3
  %nc = load double, ptr %gnc
  %p = fmul double %a, %b
  %x = fadd double %p, %c
  %y = fadd double %x, %x
  %oy0 = getelementptr double, ptr %out, i32 0
  store double %y, ptr %oy0
  %ox1 = getelementptr double, ptr %out, i32 1
  store double %x, ptr %ox1
  ret void
}

define void @phi_use(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr double, ptr %in, i32 0
  %a = load double, ptr %ga
  %gb = getelementptr double, ptr %in, i32 1
  %b = load double, ptr %gb
  %gc = getelementptr double, ptr %in, i32 2
  %c = load double, ptr %gc
  %gnc = getelementptr double, ptr %in, i32 3
  %nc = load double, ptr %gnc
  %p = fmul double %a, %b
  br label %next
next:
  %ph = phi double [ %p, %entry ]
  %x = fadd double %ph, %c
  %ox0 = getelementptr double, ptr %out, i32 0
  store double %x, ptr %ox0
  ret void
}

define void @select_use(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr double, ptr %in, i32 0
  %a = load double, ptr %ga
  %gb = getelementptr double, ptr %in, i32 1
  %b = load double, ptr %gb
  %gc = getelementptr double, ptr %in, i32 2
  %c = load double, ptr %gc
  %gnc = getelementptr double, ptr %in, i32 3
  %nc = load double, ptr %gnc
  %p = fmul double %a, %b
  %s = select i1 true, double %p, double %c
  %x = fadd double %s, %c
  %ox0 = getelementptr double, ptr %out, i32 0
  store double %x, ptr %ox0
  ret void
}

define void @cmp_twice(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr double, ptr %in, i32 0
  %a = load double, ptr %ga
  %gb = getelementptr double, ptr %in, i32 1
  %b = load double, ptr %gb
  %gc = getelementptr double, ptr %in, i32 2
  %c = load double, ptr %gc
  %gnc = getelementptr double, ptr %in, i32 3
  %nc = load double, ptr %gnc
  %p = fmul double %a, %b
  %e = fcmp oeq double %p, %nc
  %e2 = fcmp une double %p, %nc
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
  %ga = getelementptr double, ptr %in, i32 0
  %a = load double, ptr %ga
  %gb = getelementptr double, ptr %in, i32 1
  %b = load double, ptr %gb
  %gc = getelementptr double, ptr %in, i32 2
  %c = load double, ptr %gc
  %gnc = getelementptr double, ptr %in, i32 3
  %nc = load double, ptr %gnc
  %p = fmul double %a, %b
  %k = fcmp oeq double %a, %b
  %s = select i1 %k, double %p, double %c
  %x = fadd double %s, %c
  %ox0 = getelementptr double, ptr %out, i32 0
  store double %x, ptr %ox0
  ret void
}

define void @freeze_use(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr double, ptr %in, i32 0
  %a = load double, ptr %ga
  %gb = getelementptr double, ptr %in, i32 1
  %b = load double, ptr %gb
  %gc = getelementptr double, ptr %in, i32 2
  %c = load double, ptr %gc
  %gnc = getelementptr double, ptr %in, i32 3
  %nc = load double, ptr %gnc
  %p = fmul double %a, %b
  %fr = freeze double %p
  %x = fadd double %fr, %c
  %ox0 = getelementptr double, ptr %out, i32 0
  store double %x, ptr %ox0
  ret void
}

define void @fpext_use(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr double, ptr %in, i32 0
  %a = load double, ptr %ga
  %gb = getelementptr double, ptr %in, i32 1
  %b = load double, ptr %gb
  %gc = getelementptr double, ptr %in, i32 2
  %c = load double, ptr %gc
  %gnc = getelementptr double, ptr %in, i32 3
  %nc = load double, ptr %gnc
  %p = fmul double %a, %b
  %d = fpext double %p to x86_fp80
  %e = fpext double %c to x86_fp80
  %x = fadd x86_fp80 %d, %e
  %xr = fptrunc x86_fp80 %x to double
  %y = fmul double %xr, 0x4190000000000000
  %oy0 = getelementptr double, ptr %out, i32 0
  store double %y, ptr %oy0
  ret void
}

define void @fptrunc_use(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr double, ptr %in, i32 0
  %a = load double, ptr %ga
  %gb = getelementptr double, ptr %in, i32 1
  %b = load double, ptr %gb
  %gc = getelementptr double, ptr %in, i32 2
  %c = load double, ptr %gc
  %gnc = getelementptr double, ptr %in, i32 3
  %nc = load double, ptr %gnc
  %p = fmul double %a, %b
  %d = fpext double %p to x86_fp80
  %t = fptrunc x86_fp80 %d to double
  %x = fadd double %t, %c
  %ox0 = getelementptr double, ptr %out, i32 0
  store double %x, ptr %ox0
  ret void
}

define void @sitofp_use(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr double, ptr %in, i32 0
  %a = load double, ptr %ga
  %gb = getelementptr double, ptr %in, i32 1
  %b = load double, ptr %gb
  %gc = getelementptr double, ptr %in, i32 2
  %c = load double, ptr %gc
  %gnc = getelementptr double, ptr %in, i32 3
  %nc = load double, ptr %gnc
  %f = sitofp i32 16777217 to double
  %i = sitofp i32 -16777216 to double
  %f2 = fadd double %f, 0.0
  %x = fadd double %f2, %i
  %ox0 = getelementptr double, ptr %out, i32 0
  store double %x, ptr %ox0
  ret void
}

define void @sitofp_var(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr double, ptr %in, i32 0
  %a = load double, ptr %ga
  %gb = getelementptr double, ptr %in, i32 1
  %b = load double, ptr %gb
  %gc = getelementptr double, ptr %in, i32 2
  %c = load double, ptr %gc
  %gnc = getelementptr double, ptr %in, i32 3
  %nc = load double, ptr %gnc
  %gi = getelementptr i8, ptr %in, i32 180
  %iv = load i32, ptr %gi
  %f = sitofp i32 %iv to double
  %x = fadd double %f, -16777216.0
  %ox0 = getelementptr double, ptr %out, i32 0
  store double %x, ptr %ox0
  ret void
}

define void @fneg_use(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr double, ptr %in, i32 0
  %a = load double, ptr %ga
  %gb = getelementptr double, ptr %in, i32 1
  %b = load double, ptr %gb
  %gc = getelementptr double, ptr %in, i32 2
  %c = load double, ptr %gc
  %gnc = getelementptr double, ptr %in, i32 3
  %nc = load double, ptr %gnc
  %p = fmul double %a, %b
  %n = fneg double %p
  %x = fsub double %c, %n
  %ox0 = getelementptr double, ptr %out, i32 0
  store double %x, ptr %ox0
  ret void
}

define void @fmuladd(ptr %in, ptr %out) nounwind {
entry:
  %ga = getelementptr double, ptr %in, i32 0
  %a = load double, ptr %ga
  %gb = getelementptr double, ptr %in, i32 1
  %b = load double, ptr %gb
  %gc = getelementptr double, ptr %in, i32 2
  %c = load double, ptr %gc
  %gnc = getelementptr double, ptr %in, i32 3
  %nc = load double, ptr %gnc
  %m = call double @llvm.fmuladd.f64(double %a, double %b, double %c)
  %om0 = getelementptr double, ptr %out, i32 0
  store double %m, ptr %om0
  ret void
}

@names = constant [37 x ptr] [ptr @single, ptr @twice, ptr @twice_sep, ptr @call, ptr @call_between, ptr @call_before, ptr @block, ptr @block_and_local, ptr @stored, ptr @stored_after, ptr @nonlifo, ptr @far_int, ptr @far_fp, ptr @many6_fifo, ptr @many6_lifo, ptr @many7_fifo, ptr @many7_lifo, ptr @many8_fifo, ptr @many8_lifo, ptr @many9_fifo, ptr @many9_lifo, ptr @many12_fifo, ptr @many12_lifo, ptr @sqrt_between, ptr @frem_between, ptr @chain_use_twice, ptr @phi_use, ptr @select_use, ptr @cmp_twice, ptr @select_var, ptr @freeze_use, ptr @fpext_use, ptr @fptrunc_use, ptr @sitofp_use, ptr @sitofp_var, ptr @fneg_use, ptr @fmuladd]
@count = constant i32 37
