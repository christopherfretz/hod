; Integer operations at odd widths (i3, i5, i7, i17, i33, i48, i63) on
; values whose register holds other bits above the width: each operand is
; a `trunc` of a 64-bit product, so the bits above it are those of the
; product, and every operation that depends on them (right shifts,
; division, compares, `zext`/`sext`) must extend first (spec/codegen.md
; §2.2, the width invariant; the same rule on both targets, design 0006
; §8 risk 7). The program makes no call and reads no memory: everything
; comes from argc (1). The expected values were computed by LLVM's
; semantics in Python, independently of any compiler; the exit status is
; 100 + the number of the first check that failed, or 7.
;@ covers: spec/ir.md §5.4 integer, compare, cast, select (odd widths; call-free)
;@ tier: quick
;@ exit: 7

define i32 @main(i32 %argc, ptr %argv) {
entry:
  %a = zext i32 %argc to i64
  ; 0x9e3779b97f4a7c15 and 0xd67411c46c86742d.
  %big = mul i64 %a, -7046029254386353131
  %big2 = mul i64 %big, -4658895280553007687
  %t17 = trunc i64 %big to i17
  %t33 = trunc i64 %big to i33
  %t7 = trunc i64 %big to i7
  %t5 = trunc i64 %big2 to i5
  %t48 = trunc i64 %big2 to i48
  %t63 = trunc i64 %big2 to i63
  %t3 = trunc i64 %big2 to i3
  %u3 = trunc i64 %big to i3

  ; 1: lshr i17 by 3, zext: 0xf82.
  %l17 = lshr i17 %t17, 3
  %v1 = zext i17 %l17 to i64
  %ok1 = icmp eq i64 %v1, 3970
  ; 2: ashr i17 by 5, sext: 0x3e0.
  %r17 = ashr i17 %t17, 5
  %v2 = sext i17 %r17 to i64
  %ok2 = icmp eq i64 %v2, 992
  ; 3: udiv i17 by 7: 4537.
  %d17 = udiv i17 %t17, 7
  %v3 = zext i17 %d17 to i64
  %ok3 = icmp eq i64 %v3, 4537
  ; 4: srem i33 by -1000: -435.
  %m33 = srem i33 %t33, -1000
  %v4 = sext i33 %m33 to i64
  %ok4 = icmp eq i64 %v4, -435
  ; 5: sdiv i33 by -1000: 2159379.
  %q33 = sdiv i33 %t33, -1000
  %v5 = sext i33 %q33 to i64
  %ok5 = icmp eq i64 %v5, 2159379
  ; 6: icmp sgt i7 (21 > 20).
  %ok6 = icmp sgt i7 %t7, 20
  ; 7: icmp ult i7 (21 < 64).
  %ok7 = icmp ult i7 %t7, 64
  ; 8: sext i33: -2159379435.
  %v8 = sext i33 %t33 to i64
  %ok8 = icmp eq i64 %v8, -2159379435
  ; 9: shl i5 by a register amount (bits 22..23 of big2: 2): 0x14.
  %sh = lshr i64 %big2, 22
  %sh5 = trunc i64 %sh to i5
  %amt = and i5 %sh5, 3
  %s5 = shl i5 %t5, %amt
  %v9 = zext i5 %s5 to i64
  %ok9 = icmp eq i64 %v9, 20
  ; 10: mul i48: 0x978bcbd4cfe9.
  %p48 = mul i48 %t48, %t48
  %v10 = zext i48 %p48 to i64
  %ok10 = icmp eq i64 %v10, 166626675970025
  ; 11: add i63 of -1 (all 63 bits): 0x567411c46c86742c.
  %s63 = add i63 %t63, -1
  %v11 = zext i63 %s63 to i64
  %ok11 = icmp eq i64 %v11, 6229623719892251692
  ; 12: icmp sge i3 of two registers (-3 >= -3).
  %ok12 = icmp sge i3 %t3, %u3
  ; 13: select on icmp slt i17 0 (false): lshr by 1, 0x3e0a.
  %neg = icmp slt i17 %t17, 0
  %half = lshr i17 %t17, 1
  %pick = select i1 %neg, i17 %t17, i17 %half
  %v13 = zext i17 %pick to i64
  %ok13 = icmp eq i64 %v13, 15882

  ; The first failure, from the last check to the first.
  %c13 = select i1 %ok13, i32 7, i32 113
  %c12 = select i1 %ok12, i32 %c13, i32 112
  %c11 = select i1 %ok11, i32 %c12, i32 111
  %c10 = select i1 %ok10, i32 %c11, i32 110
  %c9 = select i1 %ok9, i32 %c10, i32 109
  %c8 = select i1 %ok8, i32 %c9, i32 108
  %c7 = select i1 %ok7, i32 %c8, i32 107
  %c6 = select i1 %ok6, i32 %c7, i32 106
  %c5 = select i1 %ok5, i32 %c6, i32 105
  %c4 = select i1 %ok4, i32 %c5, i32 104
  %c3 = select i1 %ok3, i32 %c4, i32 103
  %c2 = select i1 %ok2, i32 %c3, i32 102
  %c1 = select i1 %ok1, i32 %c2, i32 101
  ret i32 %c1
}
