; Floating-point arithmetic, conversions and the fcmp predicates, NaN included.
; The values come from integers (6.5 = 13 * 0.5, -0.375 = -3 / 8,
; 1.75 = 7 / 4, NaN = 0 / 0): no floating-point load or store.
; The program makes no call: what it computes is checked against values
; derived independently of any compiler (Python, by LLVM's semantics),
; and the exit status says which check failed (100 + its number), or
; that all passed (7). The operands are volatile loads of globals, so
; nothing is folded.
;@ covers: spec/ir.md §5.4 floating point (call-free; WP-043)
;@ tier: quick
;@ exit: 7

@i13 = global i32 13
@im3 = global i32 -3
@i7 = global i32 7
@i0 = global i32 0

define i32 @main() {
entry:
  %i13 = load volatile i32, ptr @i13
  %im3 = load volatile i32, ptr @im3
  %i7 = load volatile i32, ptr @i7
  %i0 = load volatile i32, ptr @i0
  %d13 = sitofp i32 %i13 to double
  %a = fmul double %d13, 0x3FE0000000000000
  %dm3 = sitofp i32 %im3 to double
  %b = fdiv double %dm3, 0x4020000000000000
  %f7 = sitofp i32 %i7 to float
  %fa = fdiv float %f7, 0x4010000000000000
  %fb = sitofp i32 %im3 to float
  %d0 = sitofp i32 %i0 to double
  %nan = fdiv double %d0, %d0
  %r0 = fadd double %a, %b
  %c0 = fcmp une double %r0, 0x4018800000000000
  br i1 %c0, label %fail0, label %ok0
fail0:
  ret i32 100
ok0:
  %r1 = fsub double %a, %b
  %c1 = fcmp une double %r1, 0x401B800000000000
  br i1 %c1, label %fail1, label %ok1
fail1:
  ret i32 101
ok1:
  %r2 = fmul double %a, %b
  %c2 = fcmp une double %r2, 0xC003800000000000
  br i1 %c2, label %fail2, label %ok2
fail2:
  ret i32 102
ok2:
  %r3 = fdiv double %a, %b
  %c3 = fcmp une double %r3, 0xC031555555555555
  br i1 %c3, label %fail3, label %ok3
fail3:
  ret i32 103
ok3:
  %r4 = fneg double %b
  %c4 = fcmp une double %r4, 0x3FD8000000000000
  br i1 %c4, label %fail4, label %ok4
fail4:
  ret i32 104
ok4:
  %r5 = fadd float %fa, %fb
  %c5 = fcmp une float %r5, 0xBFF4000000000000
  br i1 %c5, label %fail5, label %ok5
fail5:
  ret i32 105
ok5:
  %r6 = fsub float %fa, %fb
  %c6 = fcmp une float %r6, 0x4013000000000000
  br i1 %c6, label %fail6, label %ok6
fail6:
  ret i32 106
ok6:
  %r7 = fmul float %fa, %fb
  %c7 = fcmp une float %r7, 0xC015000000000000
  br i1 %c7, label %fail7, label %ok7
fail7:
  ret i32 107
ok7:
  %r8 = fdiv float %fa, %fb
  %c8 = fcmp une float %r8, 0xBFE2AAAAA0000000
  br i1 %c8, label %fail8, label %ok8
fail8:
  ret i32 108
ok8:
  %r9 = fpext float %fa to double
  %c9 = fcmp une double %r9, 0x3FFC000000000000
  br i1 %c9, label %fail9, label %ok9
fail9:
  ret i32 109
ok9:
  %r10 = fptrunc double %b to float
  %c10 = fcmp une float %r10, 0xBFD8000000000000
  br i1 %c10, label %fail10, label %ok10
fail10:
  ret i32 110
ok10:
  %r11 = fptosi double %b to i32
  %c11 = icmp ne i32 %r11, 0
  br i1 %c11, label %fail11, label %ok11
fail11:
  ret i32 111
ok11:
  %r12 = fptosi double %a to i64
  %c12 = icmp ne i64 %r12, 6
  br i1 %c12, label %fail12, label %ok12
fail12:
  ret i32 112
ok12:
  %r13 = fptosi float %fb to i32
  %c13 = icmp ne i32 %r13, -3
  br i1 %c13, label %fail13, label %ok13
fail13:
  ret i32 113
ok13:
  %r14 = fptoui double %a to i32
  %c14 = icmp ne i32 %r14, 6
  br i1 %c14, label %fail14, label %ok14
fail14:
  ret i32 114
ok14:
  %r15 = sitofp i32 -7 to double
  %c15 = fcmp une double %r15, 0xC01C000000000000
  br i1 %c15, label %fail15, label %ok15
fail15:
  ret i32 115
ok15:
  %r16 = uitofp i32 4000000000 to double
  %c16 = fcmp une double %r16, 0x41EDCD6500000000
  br i1 %c16, label %fail16, label %ok16
fail16:
  ret i32 116
ok16:
  %r17 = fcmp oeq double %a, %b
  %c17 = icmp ne i1 %r17, false
  br i1 %c17, label %fail17, label %ok17
fail17:
  ret i32 117
ok17:
  %r18 = fcmp oeq double %a, %nan
  %c18 = icmp ne i1 %r18, false
  br i1 %c18, label %fail18, label %ok18
fail18:
  ret i32 118
ok18:
  %r19 = fcmp one double %a, %b
  %c19 = icmp ne i1 %r19, true
  br i1 %c19, label %fail19, label %ok19
fail19:
  ret i32 119
ok19:
  %r20 = fcmp one double %a, %nan
  %c20 = icmp ne i1 %r20, false
  br i1 %c20, label %fail20, label %ok20
fail20:
  ret i32 120
ok20:
  %r21 = fcmp olt double %a, %b
  %c21 = icmp ne i1 %r21, false
  br i1 %c21, label %fail21, label %ok21
fail21:
  ret i32 121
ok21:
  %r22 = fcmp olt double %a, %nan
  %c22 = icmp ne i1 %r22, false
  br i1 %c22, label %fail22, label %ok22
fail22:
  ret i32 122
ok22:
  %r23 = fcmp ole double %a, %b
  %c23 = icmp ne i1 %r23, false
  br i1 %c23, label %fail23, label %ok23
fail23:
  ret i32 123
ok23:
  %r24 = fcmp ole double %a, %nan
  %c24 = icmp ne i1 %r24, false
  br i1 %c24, label %fail24, label %ok24
fail24:
  ret i32 124
ok24:
  %r25 = fcmp ogt double %a, %b
  %c25 = icmp ne i1 %r25, true
  br i1 %c25, label %fail25, label %ok25
fail25:
  ret i32 125
ok25:
  %r26 = fcmp ogt double %a, %nan
  %c26 = icmp ne i1 %r26, false
  br i1 %c26, label %fail26, label %ok26
fail26:
  ret i32 126
ok26:
  %r27 = fcmp oge double %a, %b
  %c27 = icmp ne i1 %r27, true
  br i1 %c27, label %fail27, label %ok27
fail27:
  ret i32 127
ok27:
  %r28 = fcmp oge double %a, %nan
  %c28 = icmp ne i1 %r28, false
  br i1 %c28, label %fail28, label %ok28
fail28:
  ret i32 128
ok28:
  %r29 = fcmp ord double %a, %b
  %c29 = icmp ne i1 %r29, true
  br i1 %c29, label %fail29, label %ok29
fail29:
  ret i32 129
ok29:
  %r30 = fcmp ord double %a, %nan
  %c30 = icmp ne i1 %r30, false
  br i1 %c30, label %fail30, label %ok30
fail30:
  ret i32 130
ok30:
  %r31 = fcmp uno double %a, %b
  %c31 = icmp ne i1 %r31, false
  br i1 %c31, label %fail31, label %ok31
fail31:
  ret i32 131
ok31:
  %r32 = fcmp uno double %a, %nan
  %c32 = icmp ne i1 %r32, true
  br i1 %c32, label %fail32, label %ok32
fail32:
  ret i32 132
ok32:
  %r33 = fcmp ueq double %a, %b
  %c33 = icmp ne i1 %r33, false
  br i1 %c33, label %fail33, label %ok33
fail33:
  ret i32 133
ok33:
  %r34 = fcmp ueq double %a, %nan
  %c34 = icmp ne i1 %r34, true
  br i1 %c34, label %fail34, label %ok34
fail34:
  ret i32 134
ok34:
  %r35 = fcmp une double %a, %b
  %c35 = icmp ne i1 %r35, true
  br i1 %c35, label %fail35, label %ok35
fail35:
  ret i32 135
ok35:
  %r36 = fcmp une double %a, %nan
  %c36 = icmp ne i1 %r36, true
  br i1 %c36, label %fail36, label %ok36
fail36:
  ret i32 136
ok36:
  %r37 = fcmp ult double %a, %b
  %c37 = icmp ne i1 %r37, false
  br i1 %c37, label %fail37, label %ok37
fail37:
  ret i32 137
ok37:
  %r38 = fcmp ult double %a, %nan
  %c38 = icmp ne i1 %r38, true
  br i1 %c38, label %fail38, label %ok38
fail38:
  ret i32 138
ok38:
  %r39 = fcmp ule double %a, %b
  %c39 = icmp ne i1 %r39, false
  br i1 %c39, label %fail39, label %ok39
fail39:
  ret i32 139
ok39:
  %r40 = fcmp ule double %a, %nan
  %c40 = icmp ne i1 %r40, true
  br i1 %c40, label %fail40, label %ok40
fail40:
  ret i32 140
ok40:
  %r41 = fcmp ugt double %a, %b
  %c41 = icmp ne i1 %r41, true
  br i1 %c41, label %fail41, label %ok41
fail41:
  ret i32 141
ok41:
  %r42 = fcmp ugt double %a, %nan
  %c42 = icmp ne i1 %r42, true
  br i1 %c42, label %fail42, label %ok42
fail42:
  ret i32 142
ok42:
  %r43 = fcmp uge double %a, %b
  %c43 = icmp ne i1 %r43, true
  br i1 %c43, label %fail43, label %ok43
fail43:
  ret i32 143
ok43:
  %r44 = fcmp uge double %a, %nan
  %c44 = icmp ne i1 %r44, true
  br i1 %c44, label %fail44, label %ok44
fail44:
  ret i32 144
ok44:
  ret i32 7
}
