; Drift seed (design 0006 §8 risk 7, WP-130): lane indices and lane
; division. A variable lane index past the end of the vector gives a
; poison result, and nothing else: extractelement does not read and
; insertelement does not write outside the vector (an index of 2^40 and
; one of 7 on a vector in a stack slot between two canaries). A vector
; division whose lane count is not the register's (<3 x i32>, <3 x i16>)
; divides only its own lanes: the lanes a register has beyond them are
; not divided (x86-64's div raises #DE on a zero divisor; AArch64's
; never traps). Both targets must print the same. Operands come from
; memory, so nothing is folded. Expected values derived by hand, not by
; a compiler.
;@ covers: spec/ir.md §5.4 vector (extractelement and insertelement with an out-of-range index, udiv, srem of 3 lanes)
;@ unsupported(x86_64): *
;@ stdout: canaries 1111111111111111 2222222222222222 extract 0 lanes 10 20 30
;@ stdout: udiv 33 4 2 srem -1 2 0

@idx = global [2 x i64] [i64 1099511627776, i64 7]
@num = global <3 x i32> <i32 100, i32 17, i32 9>
@den = global <3 x i32> <i32 3, i32 4, i32 4>
@hnum = global <3 x i16> <i16 -7, i16 12, i16 -8>
@hden = global <3 x i16> <i16 3, i16 5, i16 4>

@.f1 = private unnamed_addr constant [46 x i8] c"canaries %llx %llx extract %d lanes %d %d %d\0A\00"
@.f2 = private unnamed_addr constant [29 x i8] c"udiv %d %d %d srem %d %d %d\0A\00"

declare i32 @printf(ptr, ...)

define internal void @indices() noinline {
  %c1 = alloca i64, align 8
  %slot = alloca <4 x i32>, align 16
  %c2 = alloca i64, align 8
  store i64 1229782938247303441, ptr %c1
  store i64 2459565876494606882, ptr %c2
  store <4 x i32> <i32 10, i32 20, i32 30, i32 40>, ptr %slot
  %far = load i64, ptr @idx
  %p7 = getelementptr [2 x i64], ptr @idx, i32 0, i32 1
  %seven = load i64, ptr %p7
  %v = load <4 x i32>, ptr %slot
  %e = extractelement <4 x i32> %v, i64 %far
  %fe = freeze i32 %e
  %ze = and i32 %fe, 0
  %w = insertelement <4 x i32> %v, i32 -1, i64 %seven
  %w2 = insertelement <4 x i32> %v, i32 -1, i64 %far
  %wa = extractelement <4 x i32> %w, i32 1
  %wb = extractelement <4 x i32> %w2, i32 2
  %fwa = freeze i32 %wa
  %fwb = freeze i32 %wb
  %za = and i32 %fwa, 0
  %zb = and i32 %fwb, 0
  %zab = add i32 %za, %zb
  %zall = add i32 %zab, %ze
  %l0 = extractelement <4 x i32> %v, i32 0
  %l1 = extractelement <4 x i32> %v, i32 1
  %l2 = extractelement <4 x i32> %v, i32 2
  %k1 = load i64, ptr %c1
  %k2 = load i64, ptr %c2
  call i32 (ptr, ...) @printf(ptr @.f1, i64 %k1, i64 %k2, i32 %zall, i32 %l0, i32 %l1, i32 %l2)
  ret void
}

define internal void @division() noinline {
  %n = load <3 x i32>, ptr @num
  %d = load <3 x i32>, ptr @den
  %q = udiv <3 x i32> %n, %d
  %q0 = extractelement <3 x i32> %q, i32 0
  %q1 = extractelement <3 x i32> %q, i32 1
  %q2 = extractelement <3 x i32> %q, i32 2
  %hn = load <3 x i16>, ptr @hnum
  %hd = load <3 x i16>, ptr @hden
  %r = srem <3 x i16> %hn, %hd
  %r0 = extractelement <3 x i16> %r, i32 0
  %r1 = extractelement <3 x i16> %r, i32 1
  %r2 = extractelement <3 x i16> %r, i32 2
  %s0 = sext i16 %r0 to i32
  %s1 = sext i16 %r1 to i32
  %s2 = sext i16 %r2 to i32
  call i32 (ptr, ...) @printf(ptr @.f2, i32 %q0, i32 %q1, i32 %q2, i32 %s0, i32 %s1, i32 %s2)
  ret void
}

define i32 @main() {
  call void @indices()
  call void @division()
  ret i32 0
}
