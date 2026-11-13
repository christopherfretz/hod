; Drift seed (design 0006 §8 risk 7, WP-130): the width invariant at
; calls and the clearing of odd-width leaves. Narrow and odd-width
; integers whose register holds other bits above the width (each is a
; trunc of an i64) are passed with zeroext, with signext and plainly, in
; registers and on the stack past every argument register of both
; targets, returned the same three ways, and inside an { i7, i33 }
; passed and returned by value. Each callee widens what it got by zero
; and by sign, so a value whose bits above the width leak through
; prints wrong. Both targets must print the same. Expected values
; computed by a script, not a compiler.
;@ covers: spec/ir.md §5.7 attributes (zeroext, signext), §5.4 integer (odd widths across calls); spec/codegen.md §4 (the width invariant at calls), §7.2 (odd-leaf clearing)
;@ stdout: ze8 128 se8 -128 se16 -32640 ze1 1 se1 -1
;@ stdout: i7 zext 65 sext -63 i33 zext 4294967231 sext 4294967231
;@ stdout: ret i8 signext -128 i1 zeroext 1 i17 zext 79855 sext -51217
;@ stdout: pair zext 111 6604666863 sext -17 -1985267729
;@ stdout: take -1985267746
;@ stdout: stack 36 -128 65 -63 4294967361

; g[0] = 0xfedcba9876548080, g[1] = 0x0123456789ab37ef,
; g[2] = 0xffffffff00000041, g[3] = 0x80000000ff00ffef.
@g = global [4 x i64] [i64 -81985529216466816, i64 81985529216448495, i64 -4294967231, i64 -9223372032576520209]

@.f1 = private unnamed_addr constant [47 x i8] c"ze8 %lld se8 %lld se16 %lld ze1 %lld se1 %lld\0A\00"
@.f2 = private unnamed_addr constant [48 x i8] c"i7 zext %lld sext %lld i33 zext %lld sext %lld\0A\00"
@.f3 = private unnamed_addr constant [61 x i8] c"ret i8 signext %lld i1 zeroext %lld i17 zext %lld sext %lld\0A\00"
@.f4 = private unnamed_addr constant [36 x i8] c"pair zext %lld %lld sext %lld %lld\0A\00"
@.f5 = private unnamed_addr constant [11 x i8] c"take %lld\0A\00"
@.f6 = private unnamed_addr constant [32 x i8] c"stack %lld %lld %lld %lld %lld\0A\00"

declare i32 @printf(ptr, ...)

define internal i64 @g_at(i32 %i) {
  %p = getelementptr [4 x i64], ptr @g, i32 0, i32 %i
  %v = load i64, ptr %p
  ret i64 %v
}

define internal i64 @ze8(i8 zeroext %x) noinline {
  %w = zext i8 %x to i64
  ret i64 %w
}

define internal i64 @se8(i8 signext %x) noinline {
  %w = sext i8 %x to i64
  ret i64 %w
}

define internal i64 @se16(i16 signext %x) noinline {
  %w = sext i16 %x to i64
  ret i64 %w
}

define internal i64 @ze1(i1 zeroext %x) noinline {
  %w = zext i1 %x to i64
  ret i64 %w
}

define internal i64 @se1(i1 signext %x) noinline {
  %w = sext i1 %x to i64
  ret i64 %w
}

define internal i64 @z7(i7 %x) noinline {
  %w = zext i7 %x to i64
  ret i64 %w
}

define internal i64 @s7(i7 %x) noinline {
  %w = sext i7 %x to i64
  ret i64 %w
}

define internal i64 @z33(i33 %x) noinline {
  %w = zext i33 %x to i64
  ret i64 %w
}

define internal i64 @s33(i33 %x) noinline {
  %w = sext i33 %x to i64
  ret i64 %w
}

define internal signext i8 @ret8(i64 %x) noinline {
  %t = trunc i64 %x to i8
  ret i8 %t
}

define internal zeroext i1 @ret1(i64 %x) noinline {
  %t = trunc i64 %x to i1
  ret i1 %t
}

define internal i17 @ret17(i64 %x) noinline {
  %t = trunc i64 %x to i17
  ret i17 %t
}

define internal { i7, i33 } @pair(i64 %a, i64 %b) noinline {
  %ta = trunc i64 %a to i7
  %tb = trunc i64 %b to i33
  %p0 = insertvalue { i7, i33 } undef, i7 %ta, 0
  %p1 = insertvalue { i7, i33 } %p0, i33 %tb, 1
  ret { i7, i33 } %p1
}

define internal i64 @take({ i7, i33 } %p) noinline {
  %a = extractvalue { i7, i33 } %p, 0
  %b = extractvalue { i7, i33 } %p, 1
  %wa = sext i7 %a to i64
  %wb = sext i33 %b to i64
  %s = add i64 %wa, %wb
  ret i64 %s
}

; Eight i64 arguments fill AArch64's argument registers (and more than
; x86-64's six), so the narrow ones after them are on the stack.
define internal void @stack(i64 %a1, i64 %a2, i64 %a3, i64 %a4, i64 %a5, i64 %a6, i64 %a7, i64 %a8, i8 signext %s8, i16 zeroext %z16, i7 %p7, i33 %p33) noinline {
  %s0 = add i64 %a1, %a2
  %s1 = add i64 %s0, %a3
  %s2 = add i64 %s1, %a4
  %s3 = add i64 %s2, %a5
  %s4 = add i64 %s3, %a6
  %s5 = add i64 %s4, %a7
  %s6 = add i64 %s5, %a8
  %w8 = sext i8 %s8 to i64
  %w16 = zext i16 %z16 to i64
  %w7 = sext i7 %p7 to i64
  %w33 = zext i33 %p33 to i64
  call i32 (ptr, ...) @printf(ptr @.f6, i64 %s6, i64 %w8, i64 %w16, i64 %w7, i64 %w33)
  ret void
}

define i32 @main() {
  %g0 = call i64 @g_at(i32 0)
  %g1 = call i64 @g_at(i32 1)
  %g2 = call i64 @g_at(i32 2)
  %g3 = call i64 @g_at(i32 3)
  %b8 = trunc i64 %g0 to i8
  %h16 = trunc i64 %g0 to i16
  %b1 = trunc i64 %g2 to i1
  %b16x = trunc i64 %g2 to i16
  %r1 = call i64 @ze8(i8 zeroext %b8)
  %r2 = call i64 @se8(i8 signext %b8)
  %r3 = call i64 @se16(i16 signext %h16)
  %r4 = call i64 @ze1(i1 zeroext %b1)
  %r5 = call i64 @se1(i1 signext %b1)
  call i32 (ptr, ...) @printf(ptr @.f1, i64 %r1, i64 %r2, i64 %r3, i64 %r4, i64 %r5)
  %b7 = trunc i64 %g2 to i7
  %b33 = trunc i64 %g2 to i33
  %n33 = sub i33 0, %b33
  %r6 = call i64 @z7(i7 %b7)
  %r7 = call i64 @s7(i7 %b7)
  %r8 = call i64 @z33(i33 %n33)
  %r9 = call i64 @s33(i33 %n33)
  call i32 (ptr, ...) @printf(ptr @.f2, i64 %r6, i64 %r7, i64 %r8, i64 %r9)
  %q8 = call signext i8 @ret8(i64 %g0)
  %w8 = sext i8 %q8 to i64
  %q1 = call zeroext i1 @ret1(i64 %g2)
  %w1 = zext i1 %q1 to i64
  %q17 = call i17 @ret17(i64 %g1)
  %wz17 = zext i17 %q17 to i64
  %ws17 = sext i17 %q17 to i64
  call i32 (ptr, ...) @printf(ptr @.f3, i64 %w8, i64 %w1, i64 %wz17, i64 %ws17)
  %p = call { i7, i33 } @pair(i64 %g3, i64 %g1)
  %pa = extractvalue { i7, i33 } %p, 0
  %pb = extractvalue { i7, i33 } %p, 1
  %za = zext i7 %pa to i64
  %zb = zext i33 %pb to i64
  %sa = sext i7 %pa to i64
  %sb = sext i33 %pb to i64
  call i32 (ptr, ...) @printf(ptr @.f4, i64 %za, i64 %zb, i64 %sa, i64 %sb)
  %t = call i64 @take({ i7, i33 } %p)
  call i32 (ptr, ...) @printf(ptr @.f5, i64 %t)
  call void @stack(i64 1, i64 2, i64 3, i64 4, i64 5, i64 6, i64 7, i64 8, i8 signext %b8, i16 zeroext %b16x, i7 %b7, i33 %b33)
  ret i32 0
}
