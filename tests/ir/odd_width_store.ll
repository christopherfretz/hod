; An odd-width store writes its store size and zeroes the bits above the
; width (spec/ir.md §5.4 store; spec/codegen.md §2.2): `store i23 -1`
; over 0xAAAAAAAA leaves the bytes FF FF 7F AA. Byte 2 (0x7F) is then
; stored through an i8 index, sign-extended, taken from byte 3: 0xAA is
; -86, so it lands at %lut + 128 - 86 = %lut + 42 only if byte 3 was left
; alone; the i32 at %lut + 42 is the exit status, 127. A 4-byte store
; exits 0, a store that keeps the bit above the width exits 255, a
; zero-extended index exits 0. No global, no call, no arithmetic.
;@ covers: spec/ir.md §5.4 memory (odd-width store, sign-extended index; call-free)
;@ tier: quick
;@ exit: 127

define i32 @main() {
  %g = alloca i32, align 4
  %lut = alloca [256 x i8], align 1
  store volatile i32 -1431655766, ptr %g
  store i23 -1, ptr %g
  %b2p = getelementptr i8, ptr %g, i64 2
  %b2 = load volatile i8, ptr %b2p
  %b3p = getelementptr i8, ptr %g, i64 3
  %b3 = load volatile i8, ptr %b3p
  %rp = getelementptr i8, ptr %lut, i64 42
  store volatile i32 0, ptr %rp
  %mid = getelementptr i8, ptr %lut, i64 128
  %slot = getelementptr i8, ptr %mid, i8 %b3
  store volatile i8 %b2, ptr %slot
  %r = load volatile i32, ptr %rp
  ret i32 %r
}
